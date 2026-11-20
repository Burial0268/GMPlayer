use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use tauri::{Emitter, Runtime};
use tokio::sync::mpsc;
use tracing::warn;

use super::clock::normalize_seek_position;
use super::AudioPlayer;
use crate::error::{AudioError, AudioResult};
use crate::types::*;

// ── EventBuffer for session-scoped polling (kept for compat) ──

pub struct EventBuffer {
    session_id: u64,
    events: VecDeque<AudioThreadEvent>,
    max_events: usize,
}

impl EventBuffer {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            events: VecDeque::new(),
            max_events: 256,
        }
    }

    pub fn push(&mut self, event: AudioThreadEvent) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn reset(&mut self, session_id: u64) {
        self.session_id = session_id;
        self.events.clear();
    }

    pub fn drain(&mut self, session_id: u64) -> Vec<AudioThreadEvent> {
        if session_id != self.session_id {
            self.events.clear();
            return Vec::new();
        }
        self.events.drain(..).collect()
    }
}

// ── Public Player API ───────────────────────────────────────────

/// An in-process consumer of the player's event stream.
///
/// Registered through [`Player::subscribe`]. This is how platform glue that
/// must keep working while the WebView is dead — the OS media session,
/// listen-together keepalive — observes playback without going through JS.
///
/// Subscribers see every event the frontend sees **except** the high-rate
/// analysis frames (`FFTData`, `LowFreqBins`). Those are ~20 Hz of
/// multi-kilobyte payloads that go straight to the webview channel without a
/// clone; no in-process consumer has a use for them, and putting them through a
/// fan-out would add a per-subscriber copy to the hot path for nothing.
///
/// Implementations must not block: `on_event` runs on the single event
/// forwarding task, ahead of every later event. They must also not call
/// [`Player::subscribe`] / [`Player::unsubscribe`] from inside `on_event` —
/// the registry lock is held across the call.
pub trait PlayerEventSubscriber: Send + Sync {
    fn on_event(&self, event: &AudioThreadEvent);
}

/// Handle returned by [`Player::subscribe`], used to detach again.
pub type SubscriberId = u64;

pub struct Player {
    msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_tx: mpsc::UnboundedSender<SeekRequest>,
    shared: Arc<PlayerShared>,
    thread: Option<thread::JoinHandle<()>>,
}

pub struct PlayerShared {
    pub state: AtomicU8,
    pub position_ms: AtomicU64,
    pub duration_ms: AtomicU64,
    /// True once a poll consumer has shown up (`audio_poll_events` /
    /// `audio_set_session`). Until then the forwarder skips the per-event
    /// clone into `event_buf` — the Channel/emit path is the live consumer and
    /// buffering for a poller that never arrives is pure waste.
    pub event_poll_active: AtomicBool,
    pub event_buf: parking_lot::Mutex<EventBuffer>,
    /// Event sink registered by the frontend via `audio_subscribe_events`.
    /// The forwarder streams every `AudioThreadEventMessage` here; when no
    /// channel is registered yet (startup / secondary windows) or a send
    /// fails (webview reload), it falls back to a Tauri global `emit`.
    pub event_channel:
        parking_lot::Mutex<Option<Channel<AudioThreadEventMessage<AudioThreadEvent>>>>,
    /// Authoritative session snapshot, written by the `AudioPlayer` thread and
    /// read synchronously by `audio_get_session`. Shares the same allocation as
    /// `AudioPlayer::session`.
    pub session: Arc<parking_lot::Mutex<NativeSessionSnapshot>>,
    /// In-process event consumers. See [`PlayerEventSubscriber`].
    pub subscribers: parking_lot::RwLock<Vec<(SubscriberId, Arc<dyn PlayerEventSubscriber>)>>,
    next_subscriber_id: AtomicU64,
}

impl Player {
    pub fn new<R: Runtime>(app_handle: tauri::AppHandle<R>) -> AudioResult<Self> {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let (seek_tx, seek_rx) = mpsc::unbounded_channel();
        let (evt_tx, mut evt_rx) =
            mpsc::unbounded_channel::<AudioThreadEventMessage<AudioThreadEvent>>();

        let session = Arc::new(parking_lot::Mutex::new(NativeSessionSnapshot::default()));
        let shared = Arc::new(PlayerShared {
            state: AtomicU8::new(PlaybackState::Stopped as u8),
            position_ms: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            event_poll_active: AtomicBool::new(false),
            event_buf: parking_lot::Mutex::new(EventBuffer::new(0)),
            event_channel: parking_lot::Mutex::new(None),
            session: Arc::clone(&session),
            subscribers: parking_lot::RwLock::new(Vec::new()),
            next_subscriber_id: AtomicU64::new(1),
        });

        // Forward events from the internal evt channel → the frontend's
        // `Channel` (registered via `audio_subscribe_events`), falling back to
        // a Tauri global `emit` when no channel is registered yet or a send
        // fails. We peek at certain events to update `shared` atomics so
        // `audio_get_state` can return up-to-date values without going through
        // the message loop.
        //
        // High-rate analysis events are coalesced before forwarding. Playback
        // controls/status events must not sit behind stale 2048-bin FFT JSON
        // frames in the channel queue; keeping only the latest FFT/lowFreq
        // sample preserves visual freshness while state/control events stay
        // realtime.
        let shared_clone = Arc::clone(&shared);
        let app = app_handle.clone();
        let seq_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        tauri::async_runtime::spawn(async move {
            let forward_msg = |mut evt_msg: AudioThreadEventMessage<AudioThreadEvent>| {
                evt_msg.seq = seq_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let mut droppable = false;
                if let Some(event) = &evt_msg.data {
                    update_shared_from_event(&shared_clone, event);
                    if is_analysis_frame(event) {
                        // High-rate FFT/lowFreq frames: the next frame supersedes
                        // this one anyway, so skip the fallback global emit and
                        // the pre-emptive clone it requires (an ~8KB copy at
                        // 30 Hz for FFT during steady playback). Deliberately
                        // also skipped for the poll buffer and the subscriber
                        // fan-out — see `PlayerEventSubscriber`.
                        droppable = true;
                    } else {
                        if shared_clone.event_poll_active.load(Ordering::Relaxed) {
                            shared_clone.event_buf.lock().push(event.clone());
                        }
                        // Fan out to in-process consumers (media session,
                        // listen-together keepalive). These keep working while
                        // the WebView is gone, which is the whole point.
                        let subscribers = shared_clone.subscribers.read();
                        for (_, subscriber) in subscribers.iter() {
                            subscriber.on_event(event);
                        }
                    }
                }
                let channel = shared_clone.event_channel.lock().clone();
                if let Some(channel) = channel {
                    if droppable {
                        if channel.send(evt_msg).is_err() {
                            *shared_clone.event_channel.lock() = None;
                        }
                    } else if channel.send(evt_msg.clone()).is_err() {
                        // The webview/channel went away (e.g. reload). Drop the
                        // stale sink so it self-heals on the next subscribe, and
                        // fall back to a global emit for this event.
                        *shared_clone.event_channel.lock() = None;
                        let _ = app.emit("audio-player://event", &evt_msg);
                    }
                } else {
                    let _ = app.emit("audio-player://event", &evt_msg);
                }
            };

            // Outlives one burst: a frame held back by the pace goes with a later one.
            let mut analysis = AnalysisPacer::default();
            while let Some(first_msg) = evt_rx.recv().await {
                let mut realtime = Vec::new();
                analysis.collect(first_msg, &mut realtime);
                while let Ok(next_msg) = evt_rx.try_recv() {
                    analysis.collect(next_msg, &mut realtime);
                }

                for evt_msg in realtime {
                    forward_msg(evt_msg);
                }
                if let Some(evt_msg) = analysis.take_due(Instant::now()) {
                    forward_msg(evt_msg);
                }
            }
        });

        // Spawn the audio player on a dedicated thread with its own tokio runtime.
        // `AudioPlayer` owns the live `OutputStream` so it can reopen the stream
        // with a source-aware channel count when tracks change.
        let thread = spawn_initialised(
            "audio-player",
            PLAYER_INIT_TIMEOUT,
            move || AudioPlayer::new(msg_rx, seek_rx, evt_tx, session),
            AudioPlayer::run,
        )?;

        Ok(Player {
            msg_tx,
            seek_tx,
            shared,
            thread: Some(thread),
        })
    }

    /// Whether the player loop is still running. It ends on its own only when it cannot go on
    /// (its runtime is gone); a dead player answers every message with "channel closed".
    pub fn is_alive(&self) -> bool {
        !self.msg_tx.is_closed()
    }

    pub fn handle(&self) -> PlayerHandle {
        PlayerHandle {
            msg_tx: self.msg_tx.clone(),
            seek_tx: self.seek_tx.clone(),
        }
    }

    pub fn send_msg(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
        // Route seeks to the dedicated priority channel (coalesced via
        // `drain_latest_seek`) instead of the generic message queue, matching
        // the retired WebSocket control path. The invoke `callback_id` is not
        // used for seek correlation — the frontend correlates on `request_id`
        // via SeekCommitted/SeekFailed — so bypassing the queue is safe.
        if let Some(AudioThreadMessage::SeekAudio {
            position,
            request_id,
            expected_music_id,
        }) = msg.data.as_ref()
        {
            return self
                .seek_tx
                .send(SeekRequest::new(
                    *position,
                    *request_id,
                    expected_music_id.clone(),
                ))
                .map_err(|_| AudioError::ThreadError("player seek channel closed".into()));
        }
        self.msg_tx
            .send(msg)
            .map_err(|_| AudioError::ThreadError("player channel closed".into()))
    }

    // ── Quick state accessors (for commands that need sync reads) ──

    pub fn state(&self) -> PlaybackState {
        PlaybackState::from_u8(self.shared.state.load(Ordering::Relaxed))
    }

    pub fn position(&self) -> f64 {
        self.shared.position_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn duration(&self) -> f64 {
        self.shared.duration_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn is_playing(&self) -> bool {
        self.state() == PlaybackState::Playing
    }

    /// Snapshot of the live playback session. Synchronous — no round-trip
    /// through the player message loop, so a booting frontend can ask "what are
    /// you playing?" before it commits to loading anything.
    pub fn session(&self) -> NativeSessionSnapshot {
        self.shared.session.lock().clone()
    }

    pub fn poll_events(&self, session_id: u64) -> Vec<AudioThreadEvent> {
        self.shared.event_poll_active.store(true, Ordering::Relaxed);
        self.shared.event_buf.lock().drain(session_id)
    }

    pub fn set_session(&self, session_id: u64) {
        self.shared.event_poll_active.store(true, Ordering::Relaxed);
        self.shared.event_buf.lock().reset(session_id);
    }

    /// Register an in-process event consumer. See [`PlayerEventSubscriber`].
    ///
    /// Returns a [`SubscriberId`] for [`Player::unsubscribe`]. Registering the
    /// same logical consumer twice delivers every event twice — hold the id.
    pub fn subscribe(&self, subscriber: Arc<dyn PlayerEventSubscriber>) -> SubscriberId {
        let id = self.shared.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
        self.shared.subscribers.write().push((id, subscriber));
        id
    }

    /// Detach a subscriber. Unknown ids are ignored.
    pub fn unsubscribe(&self, id: SubscriberId) {
        self.shared
            .subscribers
            .write()
            .retain(|(existing, _)| *existing != id);
    }

    /// Register the frontend event `Channel`. The event forwarder streams all
    /// `AudioThreadEventMessage`s here until it is replaced or the webview
    /// reloads (a failed send clears the slot and falls back to global emit).
    pub fn set_event_channel(&self, channel: Channel<AudioThreadEventMessage<AudioThreadEvent>>) {
        *self.shared.event_channel.lock() = Some(channel);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.msg_tx.send(AudioThreadEventMessage::new(
            String::new(),
            Some(AudioThreadMessage::Close),
        ));
        if let Some(thread) = self.thread.take() {
            join_thread_async("audio-player-join", thread);
        }
    }
}

/// Opening the output can wait on the platform (an Android AAudio stream, a WASAPI endpoint
/// waking up); past this the player is reported as not started rather than left half-made.
const PLAYER_INIT_TIMEOUT: Duration = Duration::from_secs(20);

/// Start `init` on its own thread and runtime, and return only once it has come up.
///
/// The player used to be spawned and handed back at once, so an output that would not open
/// (nothing plugged in, a device mid-switch) ended the thread after `Player::new` had already
/// answered `Ok`: the caller kept a player whose every message failed with "channel closed", until
/// the app restarted. Waiting here turns that into an error the caller can retry. On a timeout the
/// thread is left to finish on its own; whatever it built then finds its channels closed and exits.
fn spawn_initialised<T, F, G>(
    name: &str,
    timeout: Duration,
    init: impl FnOnce() -> F + Send + 'static,
    run: impl FnOnce(T) -> G + Send + 'static,
) -> AudioResult<thread::JoinHandle<()>>
where
    F: std::future::Future<Output = AudioResult<T>>,
    G: std::future::Future<Output = ()>,
{
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<AudioResult<()>>(1);
    let thread_name = name.to_string();
    let thread = thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(AudioError::ThreadError(format!(
                        "build {thread_name} runtime: {e}"
                    ))));
                    return;
                }
            };
            rt.block_on(async move {
                match init().await {
                    Ok(value) => {
                        let _ = ready_tx.send(Ok(()));
                        run(value).await;
                    }
                    Err(e) => {
                        warn!("{thread_name} failed to start: {e}");
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });
        })
        .map_err(|e| AudioError::ThreadError(format!("spawn {name} thread: {e}")))?;
    match ready_rx.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(e)) => Err(e),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(AudioError::ThreadError(format!(
            "{name} did not start within {timeout:?}"
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::ThreadError(
            format!("{name} thread ended while starting"),
        )),
    }
}

pub(super) fn join_thread_async(name: &'static str, handle: thread::JoinHandle<()>) {
    let _ = thread::Builder::new().name(name.into()).spawn(move || {
        let _ = handle.join();
    });
}

/// Update `PlayerShared` atomics from events we'd otherwise miss because
/// state transitions happen inside `AudioPlayer` without writing through
/// here. Keeps `audio_get_state` honest.
fn update_shared_from_event(shared: &Arc<PlayerShared>, event: &AudioThreadEvent) {
    match event {
        AudioThreadEvent::PlayStatus { is_playing } => {
            let s = if *is_playing {
                PlaybackState::Playing
            } else {
                PlaybackState::Paused
            };
            s.store(&shared.state);
        }
        AudioThreadEvent::PlayPosition { position, .. } => {
            shared
                .position_ms
                .store((position * 1000.0).max(0.0) as u64, Ordering::Relaxed);
        }
        AudioThreadEvent::SyncStatus {
            is_playing,
            position,
            duration,
            ..
        } => {
            let s = if *is_playing {
                PlaybackState::Playing
            } else {
                PlaybackState::Paused
            };
            s.store(&shared.state);
            shared
                .position_ms
                .store((position * 1000.0).max(0.0) as u64, Ordering::Relaxed);
            shared
                .duration_ms
                .store((duration * 1000.0).max(0.0) as u64, Ordering::Relaxed);
        }
        AudioThreadEvent::AudioPlayFinished { .. } => {
            PlaybackState::Ended.store(&shared.state);
        }
        _ => {}
    }
}

/// High-rate analysis output. These are excluded from the poll buffer, the
/// global-emit fallback and the subscriber fan-out: each frame is superseded by
/// the next, so a dropped one costs nothing, while copying it per consumer at
/// ~30 Hz costs real bandwidth on the forwarding task.
fn is_analysis_frame(event: &AudioThreadEvent) -> bool {
    matches!(
        event,
        AudioThreadEvent::FFTData { .. }
            | AudioThreadEvent::LowFreqBins { .. }
            | AudioThreadEvent::AnalysisFrame { .. }
    )
}

/// How often analysis frames may reach the WebView: ~30 Hz.
///
/// Every forwarded message is one IPC crossing, and on WebView2 each crossing
/// leaves memory behind that a garbage collection does not return (measured:
/// ~100 MB over 8 minutes with the spectrum and low-frequency streams at
/// ~60 Hz each). The frontend eases the spectrum between frames.
const ANALYSIS_FORWARD_INTERVAL: Duration = Duration::from_millis(30);

/// Keeps only the newest spectrum and low-frequency frame and releases them
/// together, at most once per [`ANALYSIS_FORWARD_INTERVAL`]. A frame is
/// superseded by the next one anyway, so nothing a consumer needs is lost.
#[derive(Default)]
struct AnalysisPacer {
    fft: Option<Vec<f32>>,
    bins: Option<Vec<f32>>,
    callback_id: String,
    last_sent: Option<Instant>,
}

impl AnalysisPacer {
    /// Hold analysis frames back; everything else goes to `realtime` in order.
    fn collect(
        &mut self,
        msg: AudioThreadEventMessage<AudioThreadEvent>,
        realtime: &mut Vec<AudioThreadEventMessage<AudioThreadEvent>>,
    ) {
        match msg.data {
            Some(AudioThreadEvent::FFTData { data }) => {
                self.fft = Some(data);
                self.callback_id = msg.callback_id;
            }
            Some(AudioThreadEvent::LowFreqBins { bins }) => {
                self.bins = Some(bins);
                self.callback_id = msg.callback_id;
            }
            _ => realtime.push(msg),
        }
    }

    /// The merged frame, if one is waiting and the interval has passed.
    fn take_due(&mut self, now: Instant) -> Option<AudioThreadEventMessage<AudioThreadEvent>> {
        if self.fft.is_none() && self.bins.is_none() {
            return None;
        }
        if self
            .last_sent
            .is_some_and(|last| now.duration_since(last) < ANALYSIS_FORWARD_INTERVAL)
        {
            return None;
        }
        self.last_sent = Some(now);
        Some(AudioThreadEventMessage::new(
            self.callback_id.clone(),
            Some(AudioThreadEvent::AnalysisFrame {
                fft: self.fft.take(),
                bins: self.bins.take(),
            }),
        ))
    }
}

// ── Cloneable handle for sending messages ────────────────────────

#[derive(Clone, Debug)]
pub struct PlayerHandle {
    msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_tx: mpsc::UnboundedSender<SeekRequest>,
}

#[derive(Clone, Debug)]
pub(super) struct SeekRequest {
    pub(super) position: f64,
    pub(super) request_id: Option<u64>,
    pub(super) expected_music_id: Option<String>,
}

impl SeekRequest {
    pub(super) fn new(
        position: f64,
        request_id: Option<u64>,
        expected_music_id: Option<String>,
    ) -> Self {
        Self {
            position,
            request_id,
            expected_music_id,
        }
    }

    pub(super) fn normalized(self) -> Self {
        Self {
            position: normalize_seek_position(self.position),
            request_id: self.request_id,
            expected_music_id: self.expected_music_id,
        }
    }
}

impl PlayerHandle {
    pub fn send(&self, msg: AudioThreadEventMessage<AudioThreadMessage>) -> AudioResult<()> {
        if msg.callback_id.is_empty() {
            if let Some(AudioThreadMessage::SeekAudio {
                position,
                request_id,
                expected_music_id,
            }) = msg.data.as_ref()
            {
                return self.send_seek(*position, *request_id, expected_music_id.clone());
            }
        }

        self.msg_tx
            .send(msg)
            .map_err(|_| AudioError::ThreadError("player channel closed".into()))
    }

    pub async fn send_anonymous(&self, msg: AudioThreadMessage) -> AudioResult<()> {
        self.send(AudioThreadEventMessage::new("".into(), Some(msg)))
    }

    pub fn send_seek(
        &self,
        position: f64,
        request_id: Option<u64>,
        expected_music_id: Option<String>,
    ) -> AudioResult<()> {
        self.seek_tx
            .send(SeekRequest::new(position, request_id, expected_music_id))
            .map_err(|_| AudioError::ThreadError("player seek channel closed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AudioQuality, DisplayAudioInfo};

    /// The subscriber fan-out and the poll buffer are both gated on this. It is
    /// the only thing keeping ~30 Hz multi-kilobyte frames off the bus, so a new
    /// high-rate event that forgets to land here would silently start copying
    /// itself to every in-process consumer.
    fn msg(event: AudioThreadEvent) -> AudioThreadEventMessage<AudioThreadEvent> {
        AudioThreadEventMessage::new("cb".into(), Some(event))
    }

    #[test]
    fn analysis_frames_are_merged_paced_and_never_delay_realtime_events() {
        let mut pacer = AnalysisPacer::default();
        let mut realtime = Vec::new();
        pacer.collect(
            msg(AudioThreadEvent::FFTData { data: vec![1.0] }),
            &mut realtime,
        );
        pacer.collect(
            msg(AudioThreadEvent::PlayStatus { is_playing: true }),
            &mut realtime,
        );
        pacer.collect(
            msg(AudioThreadEvent::FFTData { data: vec![2.0] }),
            &mut realtime,
        );
        pacer.collect(
            msg(AudioThreadEvent::LowFreqBins {
                bins: vec![0.5, 0.25],
            }),
            &mut realtime,
        );

        // Realtime events pass straight through; analysis frames are held.
        assert_eq!(realtime.len(), 1);
        assert!(matches!(
            realtime[0].data,
            Some(AudioThreadEvent::PlayStatus { is_playing: true })
        ));

        // One merged message carrying only the newest of each.
        let t0 = Instant::now();
        let sent = pacer.take_due(t0).expect("first frame is due at once");
        match sent.data {
            Some(AudioThreadEvent::AnalysisFrame { fft, bins }) => {
                assert_eq!(fft, Some(vec![2.0]));
                assert_eq!(bins, Some(vec![0.5, 0.25]));
            }
            other => panic!("expected an analysis frame, got {other:?}"),
        }
        assert_eq!(sent.callback_id, "cb");
        assert!(pacer.take_due(t0).is_none(), "nothing left to send");

        // Inside the interval a new frame waits; past it, it goes.
        pacer.collect(
            msg(AudioThreadEvent::LowFreqBins { bins: vec![1.0] }),
            &mut realtime,
        );
        assert!(pacer.take_due(t0 + ANALYSIS_FORWARD_INTERVAL / 2).is_none());
        match pacer
            .take_due(t0 + ANALYSIS_FORWARD_INTERVAL)
            .and_then(|m| m.data)
        {
            Some(AudioThreadEvent::AnalysisFrame { fft: None, bins }) => {
                assert_eq!(bins, Some(vec![1.0]));
            }
            other => panic!("expected a bins-only frame, got {other:?}"),
        }
    }

    #[test]
    fn only_fft_and_low_freq_count_as_analysis_frames() {
        assert!(is_analysis_frame(&AudioThreadEvent::FFTData {
            data: vec![0.0; 2048]
        }));
        assert!(is_analysis_frame(&AudioThreadEvent::LowFreqBins {
            bins: vec![0.5; 2]
        }));
        assert!(is_analysis_frame(&AudioThreadEvent::AnalysisFrame {
            fft: None,
            bins: None
        }));

        // Everything a subscriber actually needs must pass through.
        assert!(!is_analysis_frame(&AudioThreadEvent::PlayStatus {
            is_playing: true
        }));
        assert!(!is_analysis_frame(&AudioThreadEvent::PlayPosition {
            position: 12.0,
            timeline_epoch: 1,
        }));
        assert!(!is_analysis_frame(&AudioThreadEvent::LoadAudio {
            music_id: "local:x".into(),
            music_info: DisplayAudioInfo::default(),
            quality: AudioQuality::default(),
            current_play_index: 0,
            load_request_id: None,
            identity: None,
            timeline_epoch: 1,
        }));
        assert!(!is_analysis_frame(&AudioThreadEvent::AudioPlayFinished {
            music_id: "local:x".into()
        }));
    }

    /// `PlayerShared` is what the forwarder walks; registration must be
    /// append-only and removal must be exact, or an unsubscribed media-session
    /// sink would keep receiving events after its window is gone.
    #[test]
    fn subscriber_registry_adds_and_removes_by_id() {
        struct Noop;
        impl PlayerEventSubscriber for Noop {
            fn on_event(&self, _event: &AudioThreadEvent) {}
        }

        let registry: parking_lot::RwLock<Vec<(SubscriberId, Arc<dyn PlayerEventSubscriber>)>> =
            parking_lot::RwLock::new(Vec::new());
        registry.write().push((1, Arc::new(Noop)));
        registry.write().push((2, Arc::new(Noop)));
        assert_eq!(registry.read().len(), 2);

        registry.write().retain(|(existing, _)| *existing != 1);
        assert_eq!(registry.read().len(), 1);
        assert_eq!(registry.read()[0].0, 2);

        // Unknown ids are a no-op, not a panic.
        registry.write().retain(|(existing, _)| *existing != 99);
        assert_eq!(registry.read().len(), 1);
    }

    /// A thread whose setup fails must say so to whoever started it, not hand back a handle to
    /// something that has already exited.
    #[test]
    fn a_thread_that_cannot_start_reports_it_instead_of_returning_a_dead_handle() {
        let failed = spawn_initialised(
            "init-fails",
            Duration::from_secs(5),
            || async { Err::<(), _>(AudioError::Output("no device".into())) },
            |_| async {},
        );
        assert!(matches!(failed, Err(AudioError::Output(_))));

        let (ran_tx, ran_rx) = std::sync::mpsc::channel();
        let started = spawn_initialised(
            "init-succeeds",
            Duration::from_secs(5),
            || async { Ok(7) },
            move |value| async move {
                let _ = ran_tx.send(value);
            },
        )
        .expect("starts");
        assert_eq!(ran_rx.recv_timeout(Duration::from_secs(5)), Ok(7));
        started.join().unwrap();

        let slow = spawn_initialised(
            "init-hangs",
            Duration::from_millis(20),
            || async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(())
            },
            |_| async {},
        );
        assert!(matches!(slow, Err(AudioError::ThreadError(_))));
    }
}
