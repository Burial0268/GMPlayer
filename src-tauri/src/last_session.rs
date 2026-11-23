//! What the backend last played, kept on disk so a cold start resumes it.
//!
//! On Android the page is frozen while the planner advances, so the queue cursor
//! it persists names whatever played when the app was backgrounded; a process
//! killed then would restart that track. The backend is the only party that saw
//! the advances, so it records them here.
//!
//! SIGKILL runs no handler, so nothing waits for an exit: the record is written
//! as it changes, by tmp + rename so a kill mid-write leaves the previous one.
//! A process kill keeps the page cache, so there is no fsync — only a power cut
//! can lose the last write, and then it costs a few seconds of position.
//!
//! The random permutation and the play mode are kept for the same reason: a
//! notification press changes both while the page is frozen. They live in a
//! file of their own, written only when they change, so a queue of thousands
//! is not rewritten with every position.
//!
//! Deliberately outside the event bus and its snapshot: a plain subscriber plus
//! one command, asked only by a page that boots with nothing loaded.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gmplayer_audio_backend::commands::PlayerState;
use gmplayer_audio_backend::{
    AudioThreadEvent, NativePlaybackMode, PlayerEventSubscriber, TrackIdentity,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

/// How stale the position may get while playing. Heartbeats keep arriving, so
/// a leading-edge throttle bounds the loss; while paused every anchor is written.
const POSITION_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSession {
    pub identity: TrackIdentity,
    pub position: f64,
    pub duration: f64,
    /// Unix ms, for logs.
    pub saved_at: u64,
}

/// What the queue looked like to the backend, beyond what the page persisted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastQueue {
    /// The last mode anyone set. Every change passes through the backend, so
    /// this is never older than the page's.
    pub play_mode: Option<NativePlaybackMode>,
    /// A permutation the backend drew itself (`NativeQueueOrderChanged`) and the
    /// page has not republished over since. Once a manifest at a higher
    /// revision lands, the page holds what the backend holds and this is gone.
    pub order: Option<PendingOrder>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingOrder {
    pub manifest_revision: u64,
    pub order: Vec<TrackIdentity>,
    pub shuffle_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    pub track: Option<LastSession>,
    pub queue: LastQueue,
}

pub struct LastSessionState(Arc<Persister>);

/// Read the previous process's record and persist from here on. Call from
/// `setup`, after `PlayerState` is managed.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(err) => {
            log::warn!(target: "last_session", "no app data directory, not persisted: {err}");
            return;
        }
    };
    let Some(persister) = Persister::start(dir) else {
        return;
    };
    app.state::<PlayerState>().subscribe_events(persister.clone());
    app.manage(LastSessionState(persister));
}

/// The newest records: this process's once it has them, else the previous
/// process's. `track` is `None` when nothing is known, including the last track
/// having no identity — then the page's own cursor is the better answer.
#[tauri::command]
pub fn last_session_get(state: tauri::State<'_, LastSessionState>) -> Resume {
    let tracked = state.0.state.lock();
    Resume {
        track: tracked.record.clone(),
        queue: tracked.queue.clone(),
    }
}

const TRACK_FILE: &str = "last-session.json";
const QUEUE_FILE: &str = "last-queue.json";

/// `None` for a missing or unreadable file; `null` in the file is a valid
/// `T` when `T` is an `Option` (the last track had no identity).
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<T>(&bytes)
            .inspect_err(|err| log::warn!(target: "last_session", "ignoring {path:?}: {err}"))
            .ok(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            log::warn!(target: "last_session", "cannot read {path:?}: {err}");
            None
        }
    }
}

enum Write {
    Track(Option<LastSession>),
    Queue(LastQueue),
}

struct Persister {
    tx: mpsc::Sender<Write>,
    state: parking_lot::Mutex<Tracked>,
}

#[derive(Default)]
struct Tracked {
    record: Option<LastSession>,
    queue: LastQueue,
    /// Timeline `record` belongs to; `None` until this process starts a track.
    epoch: Option<u64>,
    /// Newest anchor seen. A track's first anchor precedes its `TrackChanged`.
    anchor: (u64, f64),
    playing: bool,
    written: Option<Instant>,
}

impl Persister {
    fn start(dir: PathBuf) -> Option<Arc<Self>> {
        let previous = load::<Option<LastSession>>(&dir.join(TRACK_FILE)).flatten();
        let queue = load::<LastQueue>(&dir.join(QUEUE_FILE)).unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("last-session".into())
            .spawn(move || writer(dir, rx))
            .inspect_err(|err| log::warn!(target: "last_session", "writer not started: {err}"))
            .ok()?;
        Some(Arc::new(Self {
            tx,
            state: parking_lot::Mutex::new(Tracked {
                record: previous,
                queue,
                ..Default::default()
            }),
        }))
    }

    /// The queue half: rare, written whole and at once on every change.
    fn on_queue_event(&self, tracked: &mut Tracked, event: &AudioThreadEvent) {
        let queue = &mut tracked.queue;
        let changed = match event {
            AudioThreadEvent::SessionControlsChanged { controls }
                if controls.play_mode_known && queue.play_mode != Some(controls.play_mode) =>
            {
                queue.play_mode = Some(controls.play_mode);
                // Leaving random restores natural order on both sides.
                if controls.play_mode != NativePlaybackMode::Random {
                    queue.order = None;
                }
                true
            }
            AudioThreadEvent::NativeQueueOrderChanged {
                manifest_revision,
                order,
                shuffle_id,
            } => {
                queue.order = Some(PendingOrder {
                    manifest_revision: *manifest_revision,
                    order: order.clone(),
                    shuffle_id: shuffle_id.clone(),
                });
                true
            }
            // In stream order a divergent publish is followed by its own
            // `NativeQueueOrderChanged`, which puts the order back.
            AudioThreadEvent::NativePlannerStatusChanged { status } => queue
                .order
                .take_if(|pending| pending.manifest_revision < status.manifest_revision)
                .is_some(),
            _ => false,
        };
        if changed {
            let _ = self.tx.send(Write::Queue(queue.clone()));
        }
    }
}

impl PlayerEventSubscriber for Persister {
    fn on_event(&self, event: &AudioThreadEvent) {
        let mut tracked = self.state.lock();
        self.on_queue_event(&mut tracked, event);
        let urgent = match event {
            AudioThreadEvent::TrackChanged {
                timeline_epoch,
                identity,
                music_info,
                is_playing,
                ..
            } => {
                let position = if tracked.anchor.0 == *timeline_epoch {
                    tracked.anchor.1
                } else {
                    music_info.position
                };
                tracked.epoch = Some(*timeline_epoch);
                tracked.playing = *is_playing;
                // Unknown track: forget the previous one rather than resume it
                // over whatever the page chose.
                tracked.record = identity.clone().map(|identity| LastSession {
                    identity,
                    position,
                    duration: music_info.duration,
                    saved_at: 0,
                });
                true
            }
            AudioThreadEvent::PlayPosition {
                position,
                timeline_epoch,
            } => {
                tracked.anchor = (*timeline_epoch, *position);
                if tracked.epoch != Some(*timeline_epoch) {
                    return;
                }
                let Some(record) = &mut tracked.record else {
                    return;
                };
                record.position = *position;
                // No heartbeat follows an anchor while paused (pause, seek).
                !tracked.playing
            }
            AudioThreadEvent::SyncStatus {
                duration,
                timeline_epoch,
                ..
            } if tracked.epoch == Some(*timeline_epoch) && *duration > 0.0 => {
                match &mut tracked.record {
                    Some(record) if record.duration != *duration => record.duration = *duration,
                    _ => return,
                }
                false
            }
            AudioThreadEvent::PlayStatus { is_playing } if tracked.epoch.is_some() => {
                tracked.playing = *is_playing;
                tracked.record.is_some()
            }
            _ => return,
        };
        if !urgent && tracked.written.is_some_and(|at| at.elapsed() < POSITION_INTERVAL) {
            return;
        }
        if let Some(record) = &mut tracked.record {
            record.saved_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
        }
        tracked.written = Some(Instant::now());
        let _ = self.tx.send(Write::Track(tracked.record.clone()));
    }
}

/// Off the forwarding task: `on_event` must not block on the disk.
fn writer(dir: PathBuf, rx: mpsc::Receiver<Write>) {
    let mut failing = false;
    while let Ok(first) = rx.recv() {
        // Only the newest of each matters.
        let (mut track, mut queue) = (None, None);
        for write in std::iter::once(first).chain(rx.try_iter()) {
            match write {
                Write::Track(record) => track = Some(record),
                Write::Queue(record) => queue = Some(record),
            }
        }
        let result = std::fs::create_dir_all(&dir)
            .and_then(|()| track.map_or(Ok(()), |r| replace(&dir.join(TRACK_FILE), &r)))
            .and_then(|()| queue.map_or(Ok(()), |r| replace(&dir.join(QUEUE_FILE), &r)));
        // Once per outage: a full disk would otherwise log every ten seconds.
        match result {
            Err(err) if !failing => {
                failing = true;
                log::warn!(target: "last_session", "cannot write under {dir:?}: {err}");
            }
            Ok(()) if failing => {
                failing = false;
                log::info!(target: "last_session", "writing under {dir:?} again");
            }
            _ => {}
        }
    }
}

/// Tmp + rename, so a kill mid-write leaves the previous file whole.
fn replace(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    replace_bytes(path, &serde_json::to_vec(value).map_err(std::io::Error::other)?)
}

fn replace_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmplayer_audio_backend::{AudioQuality, DisplayAudioInfo};

    fn netease(id: &str) -> TrackIdentity {
        TrackIdentity::Netease { id: id.into() }
    }

    fn changed(epoch: u64, identity: Option<TrackIdentity>, playing: bool) -> AudioThreadEvent {
        AudioThreadEvent::TrackChanged {
            timeline_epoch: epoch,
            music_id: "local:x".into(),
            identity,
            current_play_index: 0,
            music_info: DisplayAudioInfo {
                duration: 200.0,
                ..Default::default()
            },
            quality: AudioQuality::default(),
            is_playing: playing,
            load_request_id: None,
        }
    }

    fn pos(epoch: u64, position: f64) -> AudioThreadEvent {
        AudioThreadEvent::PlayPosition {
            position,
            timeline_epoch: epoch,
        }
    }

    fn status(playing: bool) -> AudioThreadEvent {
        AudioThreadEvent::PlayStatus {
            is_playing: playing,
        }
    }

    /// A persister whose writer is a receiver the test reads.
    fn probe(previous: Option<LastSession>) -> (Persister, mpsc::Receiver<Write>) {
        let (tx, rx) = mpsc::channel();
        let state = parking_lot::Mutex::new(Tracked {
            record: previous,
            ..Default::default()
        });
        (Persister { tx, state }, rx)
    }

    /// Track writes only.
    fn drain(rx: &mpsc::Receiver<Write>) -> Vec<Option<LastSession>> {
        rx.try_iter()
            .filter_map(|w| match w {
                Write::Track(record) => Some(record),
                Write::Queue(_) => None,
            })
            .collect()
    }

    /// Queue writes only.
    fn drain_queue(rx: &mpsc::Receiver<Write>) -> Vec<LastQueue> {
        rx.try_iter()
            .filter_map(|w| match w {
                Write::Queue(record) => Some(record),
                Write::Track(_) => None,
            })
            .collect()
    }

    fn controls(mode: NativePlaybackMode, known: bool) -> AudioThreadEvent {
        AudioThreadEvent::SessionControlsChanged {
            controls: gmplayer_audio_backend::SessionControls {
                play_mode: mode,
                play_mode_known: known,
                ..Default::default()
            },
        }
    }

    fn order_changed(revision: u64, ids: &[&str], shuffle: &str) -> AudioThreadEvent {
        AudioThreadEvent::NativeQueueOrderChanged {
            manifest_revision: revision,
            order: ids.iter().map(|id| netease(id)).collect(),
            shuffle_id: Some(shuffle.into()),
        }
    }

    fn planner_status(revision: u64) -> AudioThreadEvent {
        serde_json::from_value(serde_json::json!({
            "type": "nativePlannerStatusChanged",
            "data": { "status": {
                "manifestRevision": revision, "cursorIdentity": null, "cursorIndex": null,
                "playbackState": "stopped", "preparedIdentity": null, "failureCount": 0,
                "exhausted": false, "enabled": true,
            }}
        }))
        .unwrap()
    }

    /// A fresh directory for one test; removed by the guard.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("last-session-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
        fn dir(&self) -> PathBuf {
            self.0.join("nested")
        }
        fn file(&self) -> PathBuf {
            self.dir().join(TRACK_FILE)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_resumed_track_takes_the_anchor_that_preceded_its_change() {
        let (p, rx) = probe(None);
        p.on_event(&pos(4, 31.0));
        p.on_event(&changed(4, Some(netease("1")), true));
        let written = drain(&rx);
        assert_eq!(written.len(), 1);
        let record = written[0].as_ref().unwrap();
        assert_eq!((record.identity.clone(), record.position), (netease("1"), 31.0));
    }

    #[test]
    fn a_packet_from_a_retired_timeline_does_not_move_the_new_track() {
        let (p, rx) = probe(None);
        p.on_event(&changed(4, Some(netease("1")), true));
        p.on_event(&changed(5, Some(netease("2")), true));
        p.on_event(&pos(4, 99.0));
        p.on_event(&status(false));
        let last = drain(&rx).pop().flatten().unwrap();
        assert_eq!((last.identity, last.position), (netease("2"), 0.0));
    }

    #[test]
    fn heartbeats_are_throttled_while_playing_but_every_paused_anchor_is_written() {
        let (p, rx) = probe(None);
        p.on_event(&changed(1, Some(netease("1")), true));
        for s in 1..=5 {
            p.on_event(&pos(1, s as f64));
        }
        assert_eq!(drain(&rx).len(), 1, "only the track change, not each heartbeat");
        // Kept in memory regardless, for the command.
        assert_eq!(p.state.lock().record.as_ref().unwrap().position, 5.0);

        p.on_event(&pos(1, 6.0));
        p.on_event(&status(false));
        p.on_event(&pos(1, 120.0)); // a seek while paused
        let written = drain(&rx);
        assert_eq!(written.len(), 2);
        assert_eq!(written[0].as_ref().unwrap().position, 6.0);
        assert_eq!(written[1].as_ref().unwrap().position, 120.0);
    }

    #[test]
    fn a_track_with_no_identity_forgets_the_previous_one() {
        let (p, rx) = probe(Some(LastSession {
            identity: netease("old"),
            position: 3.0,
            duration: 9.0,
            saved_at: 1,
        }));
        p.on_event(&changed(2, None, true));
        assert_eq!(drain(&rx), vec![None]);
        p.on_event(&pos(2, 50.0));
        p.on_event(&status(false));
        assert!(drain(&rx).is_empty(), "nothing to write for an unknown track");
        assert!(p.state.lock().record.is_none());
    }

    #[test]
    fn the_previous_process_record_is_served_until_a_track_starts() {
        let previous = LastSession {
            identity: netease("old"),
            position: 42.0,
            duration: 200.0,
            saved_at: 1,
        };
        let (p, rx) = probe(Some(previous.clone()));
        // An empty player's status before this process starts any track.
        p.on_event(&pos(0, 0.0));
        p.on_event(&status(false));
        assert!(drain(&rx).is_empty());
        assert_eq!(p.state.lock().record.as_ref(), Some(&previous));
    }

    #[test]
    fn duration_comes_from_status_when_the_change_did_not_know_it() {
        let (p, _rx) = probe(None);
        let mut event = changed(3, Some(netease("1")), true);
        if let AudioThreadEvent::TrackChanged { music_info, .. } = &mut event {
            music_info.duration = 0.0;
        }
        p.on_event(&event);
        let sync: AudioThreadEvent = serde_json::from_value(serde_json::json!({
            "type": "syncStatus",
            "data": {
                "musicId": "local:x", "musicInfo": DisplayAudioInfo::default(),
                "isPlaying": true, "duration": 187.5, "position": 1.0, "volume": 1.0,
                "loadPosition": 0.0, "playlist": [], "currentPlayIndex": 0,
                "playlistInited": true, "quality": AudioQuality::default(),
                "timelineEpoch": 3,
            }
        }))
        .unwrap();
        p.on_event(&sync);
        assert_eq!(p.state.lock().record.as_ref().unwrap().duration, 187.5);
    }

    #[test]
    fn unreadable_files_are_ignored_and_a_written_record_reads_back() {
        let scratch = Scratch::new("roundtrip");
        let path = scratch.file();
        let track = |path: &Path| load::<Option<LastSession>>(path).flatten();
        assert!(track(&path).is_none(), "missing file");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{\"identity\":").unwrap();
        assert!(track(&path).is_none(), "truncated file");
        std::fs::write(&path, b"null").unwrap();
        assert!(track(&path).is_none(), "explicitly unknown");
        std::fs::write(scratch.dir().join(QUEUE_FILE), b"[1,2").unwrap();

        let p = Persister::start(scratch.dir()).unwrap();
        assert_eq!(p.state.lock().queue, LastQueue::default(), "a bad queue file is ignored");
        p.on_event(&changed(1, Some(netease("7")), false));
        p.on_event(&pos(1, 12.5));
        let deadline = Instant::now() + Duration::from_secs(5);
        while track(&path).is_none_or(|r| r.position != 12.5) {
            assert!(Instant::now() < deadline, "record never landed");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn a_notification_shuffle_is_kept_until_the_page_republishes() {
        let (p, rx) = probe(None);
        // The page set random itself; its own publish needed no correction.
        p.on_event(&controls(NativePlaybackMode::Random, true));
        p.on_event(&planner_status(7));
        let q = drain_queue(&rx);
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].play_mode, Some(NativePlaybackMode::Random));
        assert!(q[0].order.is_none(), "the page already holds its own order");

        // A backend-drawn order (a notification press, no revision bump).
        p.on_event(&order_changed(7, &["3", "1", "2"], "native:7:1"));
        let pending = drain_queue(&rx).pop().unwrap().order.unwrap();
        assert_eq!((pending.manifest_revision, pending.order.len()), (7, 3));

        // Status at the same revision (an advance) does not retire it.
        p.on_event(&planner_status(7));
        assert!(drain_queue(&rx).is_empty());
        assert!(p.state.lock().queue.order.is_some());

        // The page woke, mirrored and republished: it holds the order now.
        p.on_event(&planner_status(8));
        assert_eq!(drain_queue(&rx).pop().unwrap().order, None);
    }

    #[test]
    fn a_divergent_publish_keeps_the_order_its_correction_carries() {
        let (p, rx) = probe(None);
        p.on_event(&order_changed(3, &["1", "2"], "a"));
        // Same apply: status first, then the backend's correction of it.
        p.on_event(&planner_status(4));
        p.on_event(&order_changed(4, &["2", "1"], "a"));
        let last = drain_queue(&rx).pop().unwrap().order.unwrap();
        assert_eq!((last.manifest_revision, last.order[0].clone()), (4, netease("2")));
    }

    #[test]
    fn leaving_random_drops_the_order_and_an_unknown_mode_is_not_recorded() {
        let (p, rx) = probe(None);
        p.on_event(&controls(NativePlaybackMode::Random, false));
        assert!(drain_queue(&rx).is_empty(), "a default nobody chose");
        p.on_event(&controls(NativePlaybackMode::Random, true));
        p.on_event(&order_changed(2, &["1", "2"], "a"));
        p.on_event(&controls(NativePlaybackMode::Random, true));
        assert_eq!(drain_queue(&rx).len(), 2, "an unchanged mode is not rewritten");
        p.on_event(&controls(NativePlaybackMode::Single, true));
        let last = drain_queue(&rx).pop().unwrap();
        assert_eq!(last.play_mode, Some(NativePlaybackMode::Single));
        assert!(last.order.is_none());
    }

    #[test]
    fn queue_writes_do_not_ride_on_position_writes() {
        let (p, rx) = probe(None);
        p.on_event(&order_changed(2, &["1"], "a"));
        p.on_event(&changed(1, Some(netease("1")), false));
        for s in 1..=20 {
            p.on_event(&pos(1, s as f64));
        }
        let writes: Vec<_> = rx.try_iter().collect();
        let queues = writes.iter().filter(|w| matches!(w, Write::Queue(_))).count();
        assert_eq!((queues, writes.len()), (1, 22));
    }

    /// Child half of the SIGKILL test: rewrites both files back to back, so a
    /// kill almost always lands inside a write. Serialised once up front — in a
    /// debug build encoding 3000 identities takes longer than writing them, and
    /// a kill would then land in the encoder rather than the write under test.
    /// Nothing queues up in memory, and it stops on its own if never killed.
    #[test]
    #[ignore = "spawned by a_sigkill_mid_write_leaves_a_whole_record"]
    fn sigkill_child() {
        let Ok(dir) = std::env::var("LAST_SESSION_CHILD") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let queue = serde_json::to_vec(&LastQueue {
            play_mode: Some(NativePlaybackMode::Random),
            order: Some(PendingOrder {
                manifest_revision: 0,
                order: (0..3000).map(|n| netease(&n.to_string())).collect(),
                shuffle_id: None,
            }),
        })
        .unwrap();
        for n in 1..=20_000u64 {
            let record = Some(LastSession {
                identity: netease("9"),
                position: n as f64,
                duration: 200.0,
                saved_at: n,
            });
            replace(&dir.join(TRACK_FILE), &record).unwrap();
            replace_bytes(&dir.join(QUEUE_FILE), &queue).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_sigkill_mid_write_leaves_a_whole_record() {
        let scratch = Scratch::new("sigkill");
        let path = scratch.file();
        let queue_path = scratch.dir().join(QUEUE_FILE);
        let track = |path: &Path| load::<Option<LastSession>>(path).flatten();
        std::fs::create_dir_all(scratch.dir()).unwrap();
        for round in 1..=40u64 {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(&queue_path);
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "last_session::tests::sigkill_child", "--ignored"])
                .env("LAST_SESSION_CHILD", scratch.dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            while track(&path).is_none_or(|r| r.position < 5.0) || !queue_path.exists() {
                assert!(Instant::now() < deadline, "child never wrote");
                std::thread::sleep(Duration::from_millis(1));
            }
            // Vary where in the write loop the kill lands.
            std::thread::sleep(Duration::from_micros(137 * (round % 13)));
            child.kill().unwrap(); // SIGKILL on unix
            child.wait().unwrap();
            let record = track(&path).expect("a whole record survives the kill");
            assert_eq!(record.identity, netease("9"));
            assert!(record.position >= 5.0);
            let queue = load::<LastQueue>(&queue_path).expect("a whole queue survives the kill");
            assert_eq!(queue.order.expect("order").order.len(), 3000);
        }
    }
}
