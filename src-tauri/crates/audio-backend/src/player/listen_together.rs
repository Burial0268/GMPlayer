//! Listen-together keepalive.
//!
//! The room's server-side `CONNECTED` state is maintained by a 30 s heartbeat
//! carrying the current track and position. That loop used to be a
//! `setInterval` in the frontend store — which means it stopped dead whenever
//! the Android WebView was destroyed, and the server dropped the user from the
//! room while their audio was still playing.
//!
//! Only the keepalive lives here. Room lifecycle (create / join / leave / end)
//! and the status poll that observes *remote* commands are still owned by the
//! frontend: acting on a remote command changes what plays, and having two
//! writers race over that is worse than a few seconds of missed observation.
//! See Phase 3b in `docs/backend-authority-subscriber-ipc-plan.md`.

use tracing::{debug, warn};

use super::source_resolver::{self, agent};
use super::{AudioPlayer, PlaybackIntent};

/// Matches the frontend's `startHeartbeat` cadence, which is what the server
/// expects. Not a poll: it is the liveness signal itself.
pub(super) const HEARTBEAT_PERIOD_SECS: u64 = 30;

impl AudioPlayer {
    pub(super) fn set_listen_together_room(&mut self, room_id: Option<String>) {
        let room_id = room_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());
        if self.listen_together_room == room_id {
            return;
        }
        match &room_id {
            Some(id) => debug!("listen-together keepalive armed for room {id}"),
            None => debug!("listen-together keepalive disarmed"),
        }
        self.listen_together_room = room_id;
        // Send one immediately so the server flips to CONNECTED without waiting
        // out a full period — the frontend's old implementation did the same.
        self.send_listen_together_heartbeat();
    }

    /// Fire one keepalive. Cheap no-op when not in a room, so the caller can
    /// tick unconditionally.
    pub(super) fn send_listen_together_heartbeat(&self) {
        let Some(room_id) = self.listen_together_room.clone() else {
            return;
        };
        let Some(base) = self
            .resolver_config
            .ncm_base_url
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
            .map(str::to_string)
        else {
            return;
        };
        // Only a Netease track can be reported. Mirrors the frontend, which
        // bails when there is no current song id.
        let Some(song_id) = self
            .current_identity
            .as_ref()
            .and_then(|identity| identity.netease_id())
            .map(str::to_string)
        else {
            return;
        };

        let is_playing = self.playback_intent == PlaybackIntent::Playing;
        let progress_ms = (self.clock_position() * 1000.0).max(0.0) as u64;
        let cookie = self.resolver_config.cookie.clone();

        // Fire and forget on the blocking pool: a stalled network must never
        // hold up the player loop, and a missed beat is recovered by the next.
        drop(tokio::task::spawn_blocking(move || {
            send_heartbeat_blocking(
                &base,
                cookie.as_deref(),
                &room_id,
                &song_id,
                is_playing,
                progress_ms,
            );
        }));
    }
}

fn send_heartbeat_blocking(
    base: &str,
    cookie: Option<&str>,
    room_id: &str,
    song_id: &str,
    is_playing: bool,
    progress_ms: u64,
) {
    // NOTE: `heatbeat` is not a typo on our side — it is the upstream
    // NeteaseCloudMusicApi route name.
    let url = source_resolver::join_url(base, "listentogether/heatbeat");
    let mut request = agent()
        .post(&url)
        .set("X-Requested-With", "XMLHttpRequest");
    if let Some(cookie) = cookie.map(str::trim).filter(|c| !c.is_empty()) {
        request = request.set("Cookie", cookie);
    }

    let payload = serde_json::json!({
        "roomId": room_id,
        "songId": song_id,
        "playStatus": if is_playing { "PLAY" } else { "PAUSE" },
        "progress": progress_ms,
    });

    match request.send_json(payload) {
        Ok(_) => {}
        // Never log the URL or body: they carry the room id and ride a cookie.
        Err(ureq::Error::Status(status, _)) => {
            warn!("listen-together heartbeat rejected with status {status}");
        }
        Err(ureq::Error::Transport(_)) => {
            warn!("listen-together heartbeat failed to reach the API");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Room → playback decision (Phase 3b-1)
// ═══════════════════════════════════════════════════════════════════
//
// The risky half of moving the room poll into Rust is not the networking — it
// is deciding *what to do* with a polled room state, because getting that wrong
// changes what the user hears. The decision is a pure function, so it is
// written and tested here first; 3b-2 only has to feed it.
//
// Mirrors the frontend `handleRemoteSync` exactly, including the 3 s seek
// tolerance, so the two can be swapped without a behaviour change.
//
// Echo immunity comes from comparing against **local playback state** rather
// than against "what we last sent": the server echoes our own command back, and
// if we are already in the state it describes there is nothing to do. Guarding
// on timing instead is what produced the feedback loop in the frontend playlist
// sync (a `flush: "pre"` watcher outlived the guard).

/// Seek tolerance. The room reports the other client's progress, which drifts
/// from ours continuously; only a gap this large means a real seek happened.
pub(super) const REMOTE_SEEK_TOLERANCE_SECS: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RoomPlayStatus {
    Play,
    Pause,
    Stop,
}

/// What the room says, as polled from `/listentogether/status`.
#[derive(Debug, Clone, Default)]
pub(super) struct RoomSnapshot {
    pub in_room: bool,
    /// Netease id. `None` when the room has not reported a track yet.
    pub current_song_id: Option<String>,
    pub play_status: Option<RoomPlayStatus>,
    pub current_progress_ms: Option<u64>,
}

/// What this client is doing.
#[derive(Debug, Clone, Default)]
pub(super) struct LocalPlaybackSnapshot {
    /// Netease id of `current_identity`, when it is a Netease track.
    pub song_id: Option<String>,
    pub is_playing: bool,
    pub position_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum RoomAction {
    /// The room is gone — tear down the session.
    Leave,
    /// Switch to this track. Position and play state follow from the load.
    Goto { song_id: String },
    Resume,
    Pause,
    Seek { position_secs: f64 },
}

/// Decide what to apply from a polled room state.
///
/// Returns actions in application order. An empty result is the steady state —
/// including for our own echoed commands.
pub(super) fn decide_room_actions(
    room: &RoomSnapshot,
    local: &LocalPlaybackSnapshot,
) -> Vec<RoomAction> {
    if !room.in_room {
        return vec![RoomAction::Leave];
    }

    // A track change supersedes everything else: the load establishes position
    // and play state on its own, so emitting a seek/pause alongside it would
    // fight the loader.
    if let Some(song_id) = room.current_song_id.as_deref() {
        if local.song_id.as_deref() != Some(song_id) {
            return vec![RoomAction::Goto {
                song_id: song_id.to_string(),
            }];
        }
    }

    let mut actions = Vec::new();

    // Same track: reconcile the timeline, then the transport — matching the
    // frontend order, so a remote "jump then resume" does not resume at the old
    // position first.
    if let Some(progress_ms) = room.current_progress_ms {
        let remote_secs = progress_ms as f64 / 1000.0;
        if (local.position_secs - remote_secs).abs() > REMOTE_SEEK_TOLERANCE_SECS {
            actions.push(RoomAction::Seek {
                position_secs: remote_secs,
            });
        }
    }

    match room.play_status {
        Some(RoomPlayStatus::Play) if !local.is_playing => actions.push(RoomAction::Resume),
        // `Stop` is reported for a room that has not started; treat it as pause
        // rather than inventing a third local state.
        Some(RoomPlayStatus::Pause | RoomPlayStatus::Stop) if local.is_playing => {
            actions.push(RoomAction::Pause)
        }
        _ => {}
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(song: Option<&str>, is_playing: bool, position_secs: f64) -> LocalPlaybackSnapshot {
        LocalPlaybackSnapshot {
            song_id: song.map(str::to_string),
            is_playing,
            position_secs,
        }
    }

    fn room(song: Option<&str>, status: RoomPlayStatus, progress_ms: u64) -> RoomSnapshot {
        RoomSnapshot {
            in_room: true,
            current_song_id: song.map(str::to_string),
            play_status: Some(status),
            current_progress_ms: Some(progress_ms),
        }
    }

    #[test]
    fn a_closed_room_is_the_only_thing_that_tears_down() {
        let closed = RoomSnapshot {
            in_room: false,
            ..Default::default()
        };
        assert_eq!(
            decide_room_actions(&closed, &local(Some("1"), true, 10.0)),
            vec![RoomAction::Leave]
        );
    }

    /// The headline property: the server echoes our own commands back. If we
    /// are already in the state it describes, acting would be a feedback loop
    /// — the exact bug that bit the frontend playlist sync.
    #[test]
    fn our_own_echoed_state_produces_no_actions() {
        let snapshot = room(Some("777"), RoomPlayStatus::Play, 42_000);
        assert!(decide_room_actions(&snapshot, &local(Some("777"), true, 42.0)).is_empty());
    }

    #[test]
    fn a_different_remote_track_wins_and_suppresses_everything_else() {
        // Deliberately also mismatched on progress and play state: the load
        // establishes both, so a concurrent seek/pause would fight it.
        let snapshot = room(Some("999"), RoomPlayStatus::Pause, 0);
        assert_eq!(
            decide_room_actions(&snapshot, &local(Some("777"), true, 120.0)),
            vec![RoomAction::Goto {
                song_id: "999".into()
            }]
        );
    }

    #[test]
    fn adopting_a_track_we_have_no_local_identity_for_is_a_goto() {
        let snapshot = room(Some("999"), RoomPlayStatus::Play, 0);
        assert_eq!(
            decide_room_actions(&snapshot, &local(None, false, 0.0)),
            vec![RoomAction::Goto {
                song_id: "999".into()
            }]
        );
    }

    #[test]
    fn a_room_that_reports_no_track_never_forces_a_goto() {
        let snapshot = RoomSnapshot {
            in_room: true,
            current_song_id: None,
            play_status: Some(RoomPlayStatus::Play),
            current_progress_ms: Some(5_000),
        };
        // Absent data must not be read as "switch to nothing".
        assert!(decide_room_actions(&snapshot, &local(Some("777"), true, 5.0)).is_empty());
    }

    #[test]
    fn ordinary_drift_never_seeks() {
        let snapshot = room(Some("777"), RoomPlayStatus::Play, 44_000);
        // 2 s apart: two clients drift this much on their own. Seeking here
        // would stutter playback on every poll.
        assert!(decide_room_actions(&snapshot, &local(Some("777"), true, 42.0)).is_empty());
    }

    #[test]
    fn a_real_remote_seek_is_applied_in_both_directions() {
        let forward = room(Some("777"), RoomPlayStatus::Play, 120_000);
        assert_eq!(
            decide_room_actions(&forward, &local(Some("777"), true, 42.0)),
            vec![RoomAction::Seek {
                position_secs: 120.0
            }]
        );

        let backward = room(Some("777"), RoomPlayStatus::Play, 5_000);
        assert_eq!(
            decide_room_actions(&backward, &local(Some("777"), true, 42.0)),
            vec![RoomAction::Seek {
                position_secs: 5.0
            }]
        );
    }

    #[test]
    fn transport_mismatches_resolve_to_a_single_action() {
        let playing = room(Some("777"), RoomPlayStatus::Play, 42_000);
        assert_eq!(
            decide_room_actions(&playing, &local(Some("777"), false, 42.0)),
            vec![RoomAction::Resume]
        );

        let paused = room(Some("777"), RoomPlayStatus::Pause, 42_000);
        assert_eq!(
            decide_room_actions(&paused, &local(Some("777"), true, 42.0)),
            vec![RoomAction::Pause]
        );
    }

    #[test]
    fn stop_is_treated_as_pause() {
        let stopped = room(Some("777"), RoomPlayStatus::Stop, 42_000);
        assert_eq!(
            decide_room_actions(&stopped, &local(Some("777"), true, 42.0)),
            vec![RoomAction::Pause]
        );
    }

    /// Seek before transport: a remote "jump then resume" must not resume at
    /// the old position first.
    #[test]
    fn a_seek_and_a_resume_are_ordered_timeline_first() {
        let snapshot = room(Some("777"), RoomPlayStatus::Play, 120_000);
        assert_eq!(
            decide_room_actions(&snapshot, &local(Some("777"), false, 42.0)),
            vec![
                RoomAction::Seek {
                    position_secs: 120.0
                },
                RoomAction::Resume
            ]
        );
    }

    #[test]
    fn a_room_with_no_reported_progress_only_reconciles_transport() {
        let snapshot = RoomSnapshot {
            in_room: true,
            current_song_id: Some("777".into()),
            play_status: Some(RoomPlayStatus::Pause),
            current_progress_ms: None,
        };
        assert_eq!(
            decide_room_actions(&snapshot, &local(Some("777"), true, 42.0)),
            vec![RoomAction::Pause]
        );
    }
}
