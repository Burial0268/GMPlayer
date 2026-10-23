//! In-process OS media-session bridge.
//!
//! Subscribes to the audio backend's event stream and drives the platform media
//! session directly from Rust — and, on Android, takes the transport commands
//! back the other way.
//!
//! Why not from JS: on Android the WebView is destroyed and the page reloaded
//! while the Rust process and playback keep running. Anything pushed from JS
//! stops dead in that window — the notification freezes on whatever track was
//! showing when the WebView died, and stays wrong after the backend advances.
//! Driving it from here means the session is correct even with no JS runtime.
//!
//! The same argument applies to the control direction, which is why
//! [`install_controls`] exists: a notification button that only reaches a
//! WebView is a button that stops working exactly when the notification is the
//! only UI the user has. Desktop has no WebView-death problem, but it went the
//! same way for the weaker-but-sufficient reason: the JS hop can fail silently
//! in a way the push direction cannot reveal, and routing SMTC/MPRIS through
//! the store made play/pause a second writer on the transport.
//!
//! Ordering: `on_event` runs on the backend's single forwarding task and must
//! not block, but the pushes themselves are I/O (artwork download on desktop, a
//! JNI round-trip on Android). So events are converted to commands and drained
//! by one task — spawning per event would let a play/pause reorder become
//! visible in the notification.

use std::sync::Arc;
use std::time::Duration;

use gmplayer_audio_backend::{
    AudioThreadEvent, NowPlayingInfo, PlayerEventSubscriber, SessionControls,
};
use log::warn;
use tauri::{AppHandle, Runtime};
use tokio::sync::mpsc;

/// How far the position may drift before we re-anchor the system timeline.
///
/// The OS extrapolates from the last anchor at the reported rate, so steady
/// playback needs no updates at all; this only catches seeks and the drift that
/// accumulates across pauses. Deliberately not a periodic push.
const TIMELINE_RESYNC_TOLERANCE_SECS: f64 = 2.0;

/// How long to wait for the platform to acknowledge one push.
///
/// The call itself is milliseconds of work, but it has to be serviced on the
/// Android main thread, and a stopped Activity on a background-restricted ROM
/// can leave that arbitrarily late. Without a bound the drain task waits
/// forever and every later push queues behind it — which is how the session
/// froze on one track while playback carried on and the OS kept extrapolating
/// the progress bar from the last anchor it got.
const APPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// How many consecutive failed pushes to keep re-asserting through.
///
/// A session that is simply *unavailable* — no SMTC/MPRIS, a denied
/// notification permission, a wedged main thread — fails every push, and
/// re-asserting on each incoming event would turn that into a storm: events
/// arrive continuously during playback, and on desktop every metadata push also
/// re-fetches the cover. One successful push resets the streak, so a session
/// that comes back is picked up again on the next real change.
const MAX_REASSERT_ATTEMPTS: u32 = 3;

enum MediaCommand {
    Metadata(Box<NowPlayingInfo>),
    PlayState { is_playing: bool, position: f64 },
    /// Preparing a track (resolve, download, decoder open) rather than paused.
    ///
    /// Carries the play state alongside the flag. The native side merges field
    /// by field, so a command that set only `is_loading` could land on a play
    /// state captured at a different instant — which is exactly how a track
    /// change came to show "paused at 0:00" for one round trip.
    ///
    /// Android-only in practice: the desktop system-controls plugin exposes no
    /// buffering state, so these fields go unread there.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    Loading {
        is_loading: bool,
        is_playing: bool,
        position: f64,
    },
    Timeline { position: f64, duration: f64 },
    /// Play mode / favourite. Separate from `Metadata` because they change on
    /// their own schedule — a user cycling shuffle does not change the track —
    /// and a metadata rebuild costs an artwork re-download on Android.
    Controls(SessionControls),
    Clear,
}

pub struct MediaSessionBridge {
    tx: mpsc::UnboundedSender<MediaCommand>,
    state: parking_lot::Mutex<BridgeState>,
}

/// Everything the bridge knows about what the OS is currently showing.
///
/// The decisions live on this type rather than on [`MediaSessionBridge`] so they
/// can be exercised without an `AppHandle`: each `apply_*` takes an event and
/// returns the command it should produce, if any.
#[derive(Default)]
struct BridgeState {
    last: NowPlayingInfo,
    /// Position we last handed to the OS, used to decide when re-anchoring is
    /// worth an IPC round-trip.
    anchored_position: f64,
    anchored_playing: bool,
    /// Buffering state the OS currently shows. Deduped for the same reason as
    /// the play state: every push rebuilds the notification.
    anchored_loading: bool,
    /// A buffering-clear was withheld, so the next projection has to be pushed
    /// in full even if `same_metadata` says nothing changed. See
    /// [`BridgeState::apply_loading`].
    force_metadata: bool,
    /// A push did not land, so what the OS shows no longer matches `last`.
    ///
    /// Without this the dedup makes a lost push *permanent*: metadata is only
    /// sent when the track changes, so one failed update leaves the session
    /// showing the wrong song until the next track — or forever, if that push
    /// fails too. Set on failure, cleared when a re-assert is queued.
    stale: bool,
    /// Consecutive failed pushes, so a permanently unavailable session stops
    /// costing anything. See [`MAX_REASSERT_ATTEMPTS`].
    failures: u32,
    /// Controls the OS currently shows. Deduped like the play state: every push
    /// rebuilds the notification, and these arrive with every projection.
    anchored_controls: SessionControls,
}

impl BridgeState {
    /// Whether what the OS is showing is a track *announcement* rather than a
    /// loaded track.
    ///
    /// The backend publishes an announcement to cover the resolve+download
    /// window, and it carries no duration and is not playing. A transport built
    /// from it alone therefore reads as "paused at 0:00", which is only correct
    /// for as long as the buffering flag is also set.
    fn showing_announcement(&self) -> bool {
        !self.anchored_playing && self.last.duration <= 0.0
    }

    fn apply_now_playing(&mut self, info: &NowPlayingInfo) -> Option<MediaCommand> {
        if !info.has_track {
            if !self.last.has_track {
                return None;
            }
            self.last = NowPlayingInfo::default();
            self.anchored_loading = false;
            self.force_metadata = false;
            return Some(MediaCommand::Clear);
        }
        // Rebuild metadata only when the *track* changed: on Android a metadata
        // push re-downloads the artwork, so doing it on every `sync_ui` (which
        // also fires on seeks and output rebuilds) would hammer the network.
        if self.force_metadata || !self.last.same_metadata(info) {
            self.force_metadata = false;
            self.last = info.clone();
            self.anchored_position = info.position;
            self.anchored_playing = info.is_playing;
            // The metadata push carries the playback state with it, so the
            // buffering flag has to travel on the same command or a track swap
            // would silently clear a load that is still running.
            self.anchored_loading = info.is_loading;
            // Same reasoning for the controls: the push carries them, so the OS
            // is up to date and a `Controls` command behind this one would be a
            // second round trip for a value it already has.
            self.anchored_controls = info.controls;
            return Some(MediaCommand::Metadata(Box::new(info.clone())));
        }
        // Same track, so no metadata rebuild is due — but the projection is
        // still authoritative about whether a load is running. Reconciling here
        // is what recovers the buffering state from a load that ended without a
        // `LoadAudio` (a failed resolve, a decoder that would not open).
        if let Some(command) = self.apply_loading(info.is_loading) {
            return Some(command);
        }
        if let Some(command) = self.apply_controls(info.controls) {
            return Some(command);
        }
        if !self.force_metadata {
            return None;
        }
        // The clear was withheld — a bare one reads as "paused at 0:00" — and
        // this projection is exactly what settles it, so push it in full rather
        // than defer to whatever event comes next.
        self.force_metadata = false;
        self.last = info.clone();
        self.anchored_position = info.position;
        self.anchored_playing = info.is_playing;
        self.anchored_controls = info.controls;
        Some(MediaCommand::Metadata(Box::new(info.clone())))
    }

    /// Record a controls change, deduped.
    ///
    /// Guarded on `has_track` for the same reason as the play state: these are
    /// rendered *inside* a session, and a bare controls push before any metadata
    /// would have the platform layer create an empty one.
    fn apply_controls(&mut self, controls: SessionControls) -> Option<MediaCommand> {
        if !self.last.has_track || self.anchored_controls == controls {
            return None;
        }
        self.anchored_controls = controls;
        // Keep `last` in step: it is what `reassert_if_stale` re-pushes, and a
        // re-assert carrying superseded controls would undo this.
        self.last.controls = controls;
        Some(MediaCommand::Controls(controls))
    }

    fn apply_play_status(&mut self, is_playing: bool) -> Option<MediaCommand> {
        if !self.last.has_track || self.anchored_playing == is_playing {
            return None;
        }
        self.anchored_playing = is_playing;
        // Playing or paused is an answer; buffering is the absence of one.
        // Whichever arrives last wins.
        self.anchored_loading = false;
        Some(MediaCommand::PlayState {
            is_playing,
            position: self.anchored_position,
        })
    }

    /// Record a buffering transition, deduped.
    ///
    /// Guarded on `has_track` like the play state is: the first load of a
    /// session arrives before any metadata does, and a bare "buffering" push
    /// would make the platform layer create a session with nothing in it.
    fn apply_loading(&mut self, is_loading: bool) -> Option<MediaCommand> {
        if !self.last.has_track || self.anchored_loading == is_loading {
            return None;
        }
        self.anchored_loading = is_loading;
        if !is_loading && self.showing_announcement() {
            // Withhold it. Clearing the buffering flag is the *only* thing this
            // command would change, and on an announcement projection that
            // demotes the transport from "buffering" to "paused at 0:00" —
            // which, for one round trip, is what the user sees. The play state
            // and the real timeline follow immediately (`PlayStatus`, then
            // `sync_ui`), and `force_metadata` guarantees the projection is
            // re-pushed in full even if `same_metadata` would have deduped it,
            // so nothing is left showing a spinner.
            self.force_metadata = true;
            return None;
        }
        Some(MediaCommand::Loading {
            is_loading,
            is_playing: self.anchored_playing,
            position: self.anchored_position,
        })
    }

    fn apply_position(&mut self, position: f64) -> Option<MediaCommand> {
        if !self.last.has_track {
            return None;
        }
        let drifted =
            (position - self.anchored_position).abs() >= TIMELINE_RESYNC_TOLERANCE_SECS;
        self.anchored_position = position;
        // Steady playback: the OS extrapolates, so silence is correct. Only a
        // discontinuity (seek, track restart) needs an anchor.
        if !drifted {
            return None;
        }
        Some(MediaCommand::Timeline {
            position,
            duration: self.last.duration,
        })
    }
}

impl MediaSessionBridge {
    pub fn new<R: Runtime>(app: AppHandle<R>) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<MediaCommand>();
        let bridge = Arc::new(Self {
            tx,
            state: parking_lot::Mutex::new(BridgeState::default()),
        });

        // Weak so the drain task cannot keep the bridge alive by itself.
        let weak = Arc::downgrade(&bridge);
        tauri::async_runtime::spawn(async move {
            let mut batch: Vec<MediaCommand> = Vec::new();
            while let Some(first) = rx.recv().await {
                batch.push(first);
                // Whatever else is already queued belongs to the same instant.
                // Applying a command is a round trip that has to be serviced on
                // the Android main thread, so draining first and collapsing
                // costs nothing and removes most of them.
                while let Ok(next) = rx.try_recv() {
                    batch.push(next);
                }
                for command in coalesce(std::mem::take(&mut batch)) {
                    // Spawned rather than awaited in place so the timeout does
                    // not *drop* the pending call: tauri's response handler
                    // unwraps its send, so a late answer to a dropped future
                    // panics on the JNI thread. Dispatch order is preserved
                    // regardless — the native side is fed through an ordered
                    // pipe at call time, not at completion.
                    let app = app.clone();
                    let call =
                        tauri::async_runtime::spawn(
                            async move { platform::apply(&app, command).await },
                        );
                    let outcome = match tokio::time::timeout(APPLY_TIMEOUT, call).await {
                        Ok(Ok(result)) => result,
                        Ok(Err(err)) => Err(format!("media session task failed: {err}")),
                        Err(_) => Err(format!(
                            "the platform did not acknowledge within {APPLY_TIMEOUT:?}"
                        )),
                    };
                    if let Err(err) = outcome {
                        warn!("media session update failed: {err}");
                        // Whatever the OS is showing is now unknown to us, and
                        // the dedup would suppress the correction.
                        if let Some(bridge) = weak.upgrade() {
                            let mut state = bridge.state.lock();
                            state.stale = true;
                            state.failures = state.failures.saturating_add(1);
                        }
                    } else if let Some(bridge) = weak.upgrade() {
                        bridge.state.lock().failures = 0;
                    }
                }
            }
        });

        bridge
    }

    /// Re-send the full projection after a failed push.
    ///
    /// Driven off *any* incoming event rather than a timer: events arrive
    /// continuously during playback, and re-asserting on the next one recovers
    /// within a second or so without adding a polling loop. `stale` is cleared
    /// as the command is queued, so a run of failures re-asserts once per
    /// failure instead of once per event.
    fn reassert_if_stale(&self) {
        let mut state = self.state.lock();
        if !state.stale || state.failures >= MAX_REASSERT_ATTEMPTS {
            return;
        }
        state.stale = false;
        if !state.last.has_track {
            let _ = self.tx.send(MediaCommand::Clear);
            return;
        }
        let _ = self
            .tx
            .send(MediaCommand::Metadata(Box::new(state.last.clone())));
    }
}

/// Collapse a burst of commands into the smallest sequence with the same
/// visible result.
///
/// Each command is one blocking IPC round trip — on Android a JNI call that has
/// to be serviced on the UI thread — and a single track change emits several in
/// the same instant: the announcement's metadata, the load-start/load-end
/// buffering flips, then the real metadata once the decoder is up. They all
/// merge into one native state, so only the newest of each kind can still
/// change what the user sees.
///
/// Two supersession rules do the work:
/// - a `Clear` makes everything queued before it irrelevant;
/// - a `Metadata` carries the play state, the buffering flag and the timeline
///   with it, so it subsumes anything queued before it — including a `Clear`,
///   whose release-and-recreate the metadata push would only undo.
fn coalesce(batch: Vec<MediaCommand>) -> Vec<MediaCommand> {
    if batch.len() < 2 {
        return batch;
    }

    let mut keep = vec![true; batch.len()];
    let newest_clear = batch
        .iter()
        .rposition(|command| matches!(command, MediaCommand::Clear));
    let newest_metadata = batch
        .iter()
        .rposition(|command| matches!(command, MediaCommand::Metadata(_)));
    let floor = match (newest_clear, newest_metadata) {
        (Some(clear), Some(metadata)) => clear.max(metadata),
        (Some(clear), None) => clear,
        (None, Some(metadata)) => metadata,
        (None, None) => 0,
    };
    for slot in keep.iter_mut().take(floor) {
        *slot = false;
    }

    // Past that point every kind is last-write-wins into the same fields, so
    // keep only the newest of each — in its original position, because the
    // relative order of the survivors is what produces the final state.
    let (mut playstate, mut loading, mut timeline, mut controls) = (false, false, false, false);
    for index in (floor..batch.len()).rev() {
        let superseded = match &batch[index] {
            MediaCommand::PlayState { .. } => std::mem::replace(&mut playstate, true),
            MediaCommand::Loading { .. } => std::mem::replace(&mut loading, true),
            MediaCommand::Timeline { .. } => std::mem::replace(&mut timeline, true),
            MediaCommand::Controls(_) => std::mem::replace(&mut controls, true),
            _ => false,
        };
        if superseded {
            keep[index] = false;
        }
    }

    batch
        .into_iter()
        .zip(keep)
        .filter_map(|(command, keep)| keep.then_some(command))
        .collect()
}

impl PlayerEventSubscriber for MediaSessionBridge {
    fn on_event(&self, event: &AudioThreadEvent) {
        // Any event is an opportunity to repair a push that did not land.
        self.reassert_if_stale();

        let command = {
            let mut state = self.state.lock();
            match event {
                AudioThreadEvent::NowPlayingChanged { info } => state.apply_now_playing(info),

                AudioThreadEvent::PlayStatus { is_playing } => state.apply_play_status(*is_playing),

                // Load lifecycle → buffering. Taken from the events rather than
                // from `NowPlayingChanged` because `sync_ui` does not run at the
                // start of a load: the whole point is to show something during
                // the resolve/download window, which is over by the time it does.
                AudioThreadEvent::LoadingAudio { .. } => state.apply_loading(true),
                AudioThreadEvent::LoadAudio { .. } => state.apply_loading(false),

                AudioThreadEvent::PlayPosition { position, .. } => state.apply_position(*position),

                // The controls' own event. `NowPlayingChanged` carries them too,
                // but it only fires where playback identity or the timeline
                // changes — cycling shuffle does neither.
                AudioThreadEvent::SessionControlsChanged { controls } => {
                    state.apply_controls(*controls)
                }

                _ => None,
            }
        };

        if let Some(command) = command {
            let _ = self.tx.send(command);
        }
    }
}

// ── Platform back-ends ───────────────────────────────────────────

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod platform {
    use super::MediaCommand;
    use gmplayer_audio_backend::NativePlaybackMode;
    use gmplayer_now_playing_controls::RepeatMode;
    use tauri::{AppHandle, Manager, Runtime};

    pub(super) async fn apply<R: Runtime>(
        app: &AppHandle<R>,
        command: MediaCommand,
    ) -> Result<(), String> {
        let state = app.state::<gmplayer_now_playing_controls::NowPlayingState>();

        match command {
            MediaCommand::Metadata(info) => {
                // Artwork download is blocking I/O — off the drain task's thread.
                let cover = match info.artwork_url.clone() {
                    Some(url) => tauri::async_runtime::spawn_blocking(move || {
                        gmplayer_now_playing_controls::fetch_cover_blocking(&url)
                    })
                    .await
                    .ok()
                    .flatten(),
                    None => None,
                };
                let track_id = info
                    .identity
                    .as_ref()
                    .and_then(|identity| identity.netease_id())
                    .and_then(|id| id.parse::<i64>().ok());
                let duration = (info.duration > 0.0).then_some(info.duration);
                gmplayer_now_playing_controls::apply_metadata(
                    app,
                    &state,
                    info.title.clone(),
                    info.artist.clone(),
                    info.album.clone(),
                    info.artwork_url.clone(),
                    cover,
                    track_id,
                    duration,
                )
                .and_then(|()| {
                    gmplayer_now_playing_controls::apply_timeline(
                        app,
                        &state,
                        info.position,
                        duration,
                        None,
                    )
                })
                .and_then(|()| {
                    gmplayer_now_playing_controls::apply_play_state(app, &state, info.is_playing)
                })
            }
            MediaCommand::PlayState {
                is_playing,
                position,
            } => gmplayer_now_playing_controls::apply_timeline(app, &state, position, None, None)
                .and_then(|()| {
                    gmplayer_now_playing_controls::apply_play_state(app, &state, is_playing)
                }),
            // Desktop system controls have no buffering state in this plugin's
            // surface, and desktop always has a live UI showing the real one.
            MediaCommand::Loading { .. } => Ok(()),
            // Windows exposes shuffle/repeat as first-class session properties
            // (`ShuffleEnabled` / `AutoRepeatMode`), as does MPRIS (`Shuffle` /
            // `LoopStatus`), so the OS draws its own controls from these. Neither
            // has a favourite surface, so those fields go unread on desktop —
            // the app's own UI is what renders the heart there.
            MediaCommand::Controls(controls) => gmplayer_now_playing_controls::apply_play_mode(
                app,
                &state,
                matches!(controls.play_mode, NativePlaybackMode::Random),
                match controls.play_mode {
                    NativePlaybackMode::Single => RepeatMode::Track,
                    // `Random` still repeats the list — shuffle is a traversal
                    // choice, not a repeat one, and they are separate properties.
                    NativePlaybackMode::Normal | NativePlaybackMode::Random => RepeatMode::List,
                },
            ),
            MediaCommand::Timeline { position, duration } => {
                gmplayer_now_playing_controls::apply_timeline(
                    app,
                    &state,
                    position,
                    (duration > 0.0).then_some(duration),
                    Some(true),
                )
            }
            MediaCommand::Clear => {
                state.clear_session();
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::MediaCommand;
    use gmplayer_audio_backend::NativePlaybackMode;
    use tauri::{AppHandle, Runtime};
    use tauri_plugin_media_session::{MediaSessionExt, MediaState, TimelineUpdate};

    /// Wire name for a play mode. Matched by the Kotlin side to pick the glyph;
    /// kept as a string rather than an int so a mode added on one side degrades
    /// to "unknown, draw the default" instead of silently drawing the wrong one.
    fn play_mode_name(mode: NativePlaybackMode) -> &'static str {
        match mode {
            NativePlaybackMode::Normal => "normal",
            NativePlaybackMode::Random => "random",
            NativePlaybackMode::Single => "single",
        }
    }

    pub(super) async fn apply<R: Runtime>(
        app: &AppHandle<R>,
        command: MediaCommand,
    ) -> Result<(), String> {
        let session = app.media_session();
        match command {
            MediaCommand::Metadata(info) => {
                session
                    .update_state(MediaState {
                        title: Some(info.title.clone()),
                        artist: Some(info.artist.clone()),
                        album: Some(info.album.clone()),
                        // The plugin downloads this natively (no CORS, no WebView).
                        artwork_url: info.artwork_url.clone(),
                        duration: Some(info.duration),
                        position: Some(info.position),
                        is_playing: Some(info.is_playing),
                        is_loading: Some(info.is_loading),
                        can_prev: Some(true),
                        can_next: Some(true),
                        can_seek: Some(true),
                        play_mode: Some(play_mode_name(info.controls.play_mode).to_string()),
                        favourite: Some(info.controls.favourite),
                        ..Default::default()
                    })
                    .await
            }
            MediaCommand::PlayState {
                is_playing,
                position,
            } => {
                session
                    .update_state(MediaState {
                        is_playing: Some(is_playing),
                        is_loading: Some(false),
                        position: Some(position),
                        ..Default::default()
                    })
                    .await
            }
            MediaCommand::Loading {
                is_loading,
                is_playing,
                position,
            } => {
                session
                    .update_state(MediaState {
                        is_loading: Some(is_loading),
                        // Sent with the flag, never on its own: the native side
                        // merges field by field, so omitting it would let this
                        // command inherit a play state from another instant.
                        is_playing: Some(is_playing),
                        position: Some(position),
                        ..Default::default()
                    })
                    .await
            }
            // Timeline-only: skips the notification rebuild (and the artwork
            // re-fetch that comes with it).
            MediaCommand::Timeline { position, duration } => {
                session
                    .update_timeline(TimelineUpdate {
                        position: Some(position),
                        duration: (duration > 0.0).then_some(duration),
                        ..Default::default()
                    })
                    .await
            }
            // Rebuilds the notification, because the two glyphs live in it —
            // but carries no `artworkUrl`, so the merge on the native side
            // reuses the bitmap it already holds rather than re-downloading.
            MediaCommand::Controls(controls) => {
                session
                    .update_state(MediaState {
                        play_mode: Some(play_mode_name(controls.play_mode).to_string()),
                        favourite: Some(controls.favourite),
                        ..Default::default()
                    })
                    .await
            }
            MediaCommand::Clear => session.clear().await,
        }
    }
}

#[cfg(target_os = "ios")]
mod platform {
    use super::MediaCommand;
    use tauri::{AppHandle, Runtime};

    pub(super) async fn apply<R: Runtime>(
        _app: &AppHandle<R>,
        _command: MediaCommand,
    ) -> Result<(), String> {
        // iOS is not wired up in either direction: nothing pushes state here,
        // so the Swift plugin never creates a session and its remote-command
        // targets can never fire. Wiring the push half is what would make
        // `install_controls` worth extending to `cfg(mobile)`.
        Ok(())
    }
}

// ── Control direction ────────────────────────────────────────────

/// Route OS media-session actions straight into the audio backend.
///
/// On Android this is the half that the WebView cannot own. The notification,
/// the lock screen, a headset button and the audio-focus handler all originate
/// in Kotlin, and the Kotlin side reaches Rust over the plugin's `Channel` — a
/// JNI hop into this process, with no WebView anywhere in it. Handling them
/// here is what makes the buttons work while the page is destroyed, which is
/// the only time the notification is the user's only UI.
///
/// Transport only: play/pause/next/previous/seek. Anything that is a *view*
/// concern (volume UI) stays in the frontend, and the frontend follows this
/// through the backend's own event stream — `PlayStatus` and
/// `NativePlannerAdvanced` are already adopted there, so there is one writer.
#[cfg(target_os = "android")]
pub fn install_controls<R: Runtime>(app: &AppHandle<R>) {
    use gmplayer_audio_backend::{commands::PlayerState, AudioThreadMessage};
    use log::warn;
    use tauri::Manager;
    use tauri_plugin_media_session::{MediaAction, MediaSessionExt};

    let handle = app.clone();
    app.media_session().on_action(move |event| {
        log::info!("media action: {:?}", event.action);
        let msg = match event.action {
            MediaAction::Play => AudioThreadMessage::ResumeAudio,
            // `Stop` is the notification being dismissed or focus being lost
            // for good. Pausing (rather than tearing the session down) keeps
            // the position, which is what the user gets back on resume.
            MediaAction::Pause | MediaAction::Stop => AudioThreadMessage::PauseAudio,
            MediaAction::Next => AudioThreadMessage::NextSong,
            MediaAction::Previous => AudioThreadMessage::PrevSong,
            // Session controls: intents, not values. The button cannot know the
            // current mode or like state, so the backend — the one holder of
            // both — resolves them. See `player::session_controls`.
            MediaAction::CyclePlayMode => AudioThreadMessage::SetNextPlayMode,
            MediaAction::ToggleFavourite => AudioThreadMessage::ToggleFavourite,
            MediaAction::Seek => {
                let Some(position) = event.seek_position else {
                    return;
                };
                AudioThreadMessage::SeekAudio {
                    position: position.max(0.0),
                    request_id: None,
                    expected_music_id: None,
                }
            }
        };

        if let Err(err) = handle.state::<PlayerState>().try_send_msg(msg) {
            warn!("media action dropped: {err}");
        }
    });
}

/// The desktop twin: SMTC / MPRIS / `MPRemoteCommandCenter` actions, in Rust.
///
/// Desktop has no WebView-death problem, so this used to be the one direction
/// that still went out to JS — the plugin emitted a Tauri event and
/// `useNativeMediaControls` wrote the store. That hop is a liability rather
/// than a shortcut. It only works while a page happens to be mounted *and*
/// listening, `AppHandle::emit` reports success even when no webview has
/// registered a listener, and the push direction is entirely independent — so
/// the whole failure mode is "the flyout shows the right track, the log shows
/// the button press, and nothing happens", with no error anywhere. That is
/// exactly what it did.
///
/// Handling it here also removes the second writer on the transport: play/pause
/// no longer travels store → watcher → `fadePlayOrPause`, it goes to the one
/// component that owns playback, and the frontend adopts the result through
/// `PlayStatus` like it already does on Android.
///
/// Three deliberate asymmetries with Android:
/// - shuffle and repeat arrive as *separate* OS properties on both Windows and
///   MPRIS, but this app has one three-mode ring, so both requests are the same
///   intent — `SetNextPlayMode` — and the backend resolves it.
/// - there is no favourite surface on any desktop platform, so
///   `ToggleFavourite` has nothing to bind to here.
/// - volume is not taken at all; see the `SetVolume` arm.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn install_controls<R: Runtime>(app: &AppHandle<R>) {
    use gmplayer_audio_backend::{commands::PlayerState, AudioThreadMessage};
    use gmplayer_now_playing_controls::SystemMediaEventType;
    use log::{info, warn};
    use tauri::Manager;

    let handle = app.clone();
    gmplayer_now_playing_controls::on_action(move |event| {
        info!("system media action: {:?}", event.type_);
        let msg = match event.type_ {
            SystemMediaEventType::Play => AudioThreadMessage::ResumeAudio,
            SystemMediaEventType::Pause | SystemMediaEventType::Stop => {
                AudioThreadMessage::PauseAudio
            }
            SystemMediaEventType::NextSong => AudioThreadMessage::NextSong,
            SystemMediaEventType::PreviousSong => AudioThreadMessage::PrevSong,
            // One ring, two OS properties. See the doc comment above.
            SystemMediaEventType::ToggleShuffle | SystemMediaEventType::ToggleRepeat => {
                AudioThreadMessage::SetNextPlayMode
            }
            SystemMediaEventType::Seek => {
                let Some(position) = event.position else {
                    return;
                };
                AudioThreadMessage::SeekAudio {
                    position: position.as_secs_f64(),
                    request_id: None,
                    expected_music_id: None,
                }
            }
            SystemMediaEventType::SetVolume => {
                // Not ours to take. Volume's owner is the frontend store —
                // `persistData.playVolume` is what the sound follows, and every
                // track load re-asserts it (`PlayerFunctions` sets it on the new
                // sound). Sending it to the backend here would change the output
                // and then have it snap back at the next track, which is worse
                // than not moving. Wiring it properly means teaching the store
                // to adopt the volume `SyncStatus` already carries; until then
                // this is inert, which only MPRIS can even reach — Windows has
                // no volume surface and `update_volume` is unimplemented there.
                return;
            }
            // Variable-rate playback is not implemented anywhere in the chain;
            // the SMTC handler only exists because Windows raises the request.
            SystemMediaEventType::SetRate => return,
        };

        // `try_send_msg` deliberately does not create a player: a press with no
        // player behind it is a stale one, and opening an audio device in
        // response would be worse than dropping it.
        if let Err(err) = handle.state::<PlayerState>().try_send_msg(msg) {
            warn!("system media action dropped: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmplayer_audio_backend::NativePlaybackMode;

    fn kind(command: &MediaCommand) -> &'static str {
        match command {
            MediaCommand::Metadata(_) => "metadata",
            MediaCommand::PlayState { .. } => "playstate",
            MediaCommand::Loading { .. } => "loading",
            MediaCommand::Timeline { .. } => "timeline",
            MediaCommand::Controls(_) => "controls",
            MediaCommand::Clear => "clear",
        }
    }

    fn kinds(batch: Vec<MediaCommand>) -> Vec<&'static str> {
        coalesce(batch).iter().map(kind).collect()
    }

    fn metadata(title: &str) -> MediaCommand {
        MediaCommand::Metadata(Box::new(NowPlayingInfo {
            has_track: true,
            title: title.to_string(),
            ..Default::default()
        }))
    }

    fn loading(is_loading: bool) -> MediaCommand {
        MediaCommand::Loading {
            is_loading,
            is_playing: false,
            position: 0.0,
        }
    }

    /// A loaded, playing track: the real projection.
    fn playing(title: &str, duration: f64) -> NowPlayingInfo {
        NowPlayingInfo {
            has_track: true,
            title: title.to_string(),
            duration,
            is_playing: true,
            ..Default::default()
        }
    }

    /// What the backend publishes to cover the resolve+download window: no
    /// duration, not playing, load in flight.
    fn announced(title: &str) -> NowPlayingInfo {
        NowPlayingInfo {
            has_track: true,
            title: title.to_string(),
            duration: 0.0,
            is_playing: false,
            is_loading: true,
            ..Default::default()
        }
    }

    /// The track-change burst: buffering flips and a stale metadata push all
    /// land behind the real one, which carries every field they set.
    #[test]
    fn a_metadata_push_subsumes_everything_queued_before_it() {
        assert_eq!(
            kinds(vec![
                metadata("announced"),
                loading(true),
                loading(false),
                metadata("loaded"),
            ]),
            vec!["metadata"]
        );
    }

    /// What follows the newest metadata is still live state and must survive.
    #[test]
    fn state_after_the_newest_metadata_survives() {
        assert_eq!(
            kinds(vec![
                metadata("loaded"),
                MediaCommand::PlayState {
                    is_playing: true,
                    position: 0.0
                },
                MediaCommand::Timeline {
                    position: 30.0,
                    duration: 200.0
                },
            ]),
            vec!["metadata", "playstate", "timeline"]
        );
    }

    /// Only the newest of each kind matters, and the survivors keep their
    /// original order — the native side merges them field by field, so a
    /// reorder would change the result.
    #[test]
    fn repeated_kinds_collapse_to_the_newest_in_place() {
        assert_eq!(
            kinds(vec![
                MediaCommand::Timeline {
                    position: 1.0,
                    duration: 200.0
                },
                MediaCommand::PlayState {
                    is_playing: false,
                    position: 1.0
                },
                MediaCommand::Timeline {
                    position: 2.0,
                    duration: 200.0
                },
                loading(true),
            ]),
            vec!["playstate", "timeline", "loading"]
        );
    }

    #[test]
    fn a_clear_drops_what_came_before_it() {
        assert_eq!(
            kinds(vec![
                metadata("gone"),
                MediaCommand::PlayState {
                    is_playing: true,
                    position: 5.0
                },
                MediaCommand::Clear,
            ]),
            vec!["clear"]
        );
    }

    /// Track end followed immediately by the next track: the release and
    /// recreate cancel out, and the metadata push rewrites every field anyway.
    #[test]
    fn a_metadata_after_a_clear_absorbs_it() {
        assert_eq!(
            kinds(vec![MediaCommand::Clear, metadata("next")]),
            vec!["metadata"]
        );
    }

    #[test]
    fn a_single_command_is_passed_through() {
        assert_eq!(kinds(vec![MediaCommand::Clear]), vec!["clear"]);
    }

    // ── Projection state machine ─────────────────────────────────────

    /// The reported bug: a track change showing "paused at 0:00".
    ///
    /// `LoadAudio` clears the buffering flag before `PlayStatus` says the track
    /// is playing, and on an announcement projection that is the only thing
    /// separating "buffering" from "paused". The three commands normally
    /// coalesce, but each apply is a JNI round trip, so a busy main thread gets
    /// them one at a time — and the middle one is what the user sees.
    #[test]
    fn a_load_over_an_announcement_never_reports_paused_at_zero() {
        let mut state = BridgeState::default();
        assert!(matches!(
            state.apply_now_playing(&playing("A", 200.0)),
            Some(MediaCommand::Metadata(_))
        ));

        // Track A ends: the anchor rewinds, then B is announced.
        state.apply_position(0.0);
        assert!(matches!(
            state.apply_now_playing(&announced("B")),
            Some(MediaCommand::Metadata(_))
        ));

        // The load ends. Pushing the clear on its own here is the bug.
        assert!(
            state.apply_loading(false).is_none(),
            "a bare buffering-clear on an announcement demotes the transport to paused"
        );

        // The play state is what legitimately moves the transport, and it
        // carries the buffering flag with it.
        assert!(matches!(
            state.apply_play_status(true),
            Some(MediaCommand::PlayState {
                is_playing: true,
                ..
            })
        ));

        // Then the real projection lands, duration and all.
        assert!(matches!(
            state.apply_now_playing(&playing("B", 240.0)),
            Some(MediaCommand::Metadata(_))
        ));
    }

    /// The withheld clear must not be able to strand a spinner.
    ///
    /// `same_metadata` ignores the loading flag, so a projection that settles
    /// with the same title and no duration would otherwise be deduped and the
    /// clear would never reach the OS.
    #[test]
    fn a_withheld_buffering_clear_forces_the_next_projection() {
        let mut state = BridgeState::default();
        state.apply_now_playing(&announced("B"));
        assert!(state.apply_loading(false).is_none());

        let settled = NowPlayingInfo {
            is_loading: false,
            ..announced("B")
        };
        assert!(
            state.last.same_metadata(&settled),
            "the premise: this projection is a dedup candidate"
        );
        assert!(matches!(
            state.apply_now_playing(&settled),
            Some(MediaCommand::Metadata(_))
        ));
    }

    /// Withholding is only for the announcement case — a load that interrupts a
    /// real, playing projection still gets its buffering flip, and the flip
    /// carries the play state so the native merge cannot invert it.
    #[test]
    fn a_load_over_a_real_projection_still_flips_buffering() {
        let mut state = BridgeState::default();
        state.apply_now_playing(&playing("A", 200.0));
        assert!(matches!(
            state.apply_loading(true),
            Some(MediaCommand::Loading {
                is_loading: true,
                is_playing: true,
                ..
            })
        ));
        assert!(matches!(
            state.apply_loading(false),
            Some(MediaCommand::Loading {
                is_loading: false,
                is_playing: true,
                ..
            })
        ));
    }

    /// Steady playback needs no pushes at all: the OS extrapolates from the
    /// last anchor, and only a discontinuity is worth a round trip.
    #[test]
    fn only_a_discontinuity_re_anchors_the_timeline() {
        let mut state = BridgeState::default();
        state.apply_now_playing(&playing("A", 200.0));
        assert!(state.apply_position(1.0).is_none());
        assert!(state.apply_position(2.0).is_none());
        assert!(matches!(
            state.apply_position(120.0),
            Some(MediaCommand::Timeline {
                duration: 200.0,
                ..
            })
        ));
    }

    // ── Session controls ─────────────────────────────────────────────

    fn controls(play_mode: NativePlaybackMode, favourite: bool) -> SessionControls {
        SessionControls {
            play_mode,
            favourite,
            can_favourite: true,
        }
    }

    /// Cycling shuffle must not cost an artwork download, which is what a
    /// `Metadata` push means on Android.
    #[test]
    fn a_controls_change_does_not_rebuild_metadata() {
        let mut state = BridgeState::default();
        state.apply_now_playing(&playing("A", 200.0));

        assert!(matches!(
            state.apply_controls(controls(NativePlaybackMode::Random, false)),
            Some(MediaCommand::Controls(_))
        ));
        assert!(
            state
                .apply_controls(controls(NativePlaybackMode::Random, false))
                .is_none(),
            "the same controls must dedup"
        );
        assert!(matches!(
            state.apply_controls(controls(NativePlaybackMode::Random, true)),
            Some(MediaCommand::Controls(_))
        ));
    }

    /// A metadata push carries the controls, so one queued behind it is a second
    /// JNI round trip for a value the OS already has.
    #[test]
    fn a_metadata_push_carries_the_controls_with_it() {
        let mut state = BridgeState::default();
        let mut info = playing("A", 200.0);
        info.controls = controls(NativePlaybackMode::Single, true);

        assert!(matches!(
            state.apply_now_playing(&info),
            Some(MediaCommand::Metadata(_))
        ));
        assert!(
            state.apply_controls(info.controls).is_none(),
            "the metadata push already delivered these"
        );
    }

    /// A re-assert re-sends `last`, so a controls change has to be recorded
    /// there too — otherwise recovering from a failed push would silently roll
    /// the notification back to the previous mode.
    #[test]
    fn a_reassert_carries_the_newest_controls() {
        let mut state = BridgeState::default();
        state.apply_now_playing(&playing("A", 200.0));
        state.apply_controls(controls(NativePlaybackMode::Random, true));

        assert_eq!(state.last.controls.play_mode, NativePlaybackMode::Random);
        assert!(state.last.controls.favourite);
    }

    /// Controls before any track would have the platform layer create an empty
    /// session — the same rule the play state and the buffering flag follow.
    #[test]
    fn controls_before_a_track_are_withheld() {
        let mut state = BridgeState::default();
        assert!(state
            .apply_controls(controls(NativePlaybackMode::Random, true))
            .is_none());
    }

    #[test]
    fn only_the_newest_controls_command_survives_a_burst() {
        assert_eq!(
            kinds(vec![
                MediaCommand::Controls(controls(NativePlaybackMode::Normal, false)),
                MediaCommand::PlayState {
                    is_playing: true,
                    position: 1.0
                },
                MediaCommand::Controls(controls(NativePlaybackMode::Random, true)),
            ]),
            vec!["playstate", "controls"]
        );
    }
}
