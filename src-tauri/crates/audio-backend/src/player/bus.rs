//! The one ordering point for everything the player tells the outside world,
//! and the sinks that carry it there.
//!
//! [`EventBus::emit`] stamps `seq`, the current timeline epoch and the last
//! frontend command taken up, applies the event to the session snapshot, and
//! queues it — under one lock, so queue order, `seq` order and snapshot order
//! are the same order. A snapshot taken here is therefore exactly "every event
//! up to its `seq`", which is what lets a subscriber start from one with
//! neither a gap nor a stale overwrite. Analysis frames skip all of that
//! ([`EventBus::emit_ephemeral`]): each is superseded by the next and describes
//! no state.
//!
//! The bus and the sinks outlive any one [`super::Player`]: a player that
//! stopped is replaced under the same bus, which announces the replacement as a
//! fresh `Hello` in the stream rather than leaving every subscriber holding
//! state from a process that no longer exists.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use tokio::sync::mpsc;

use crate::types::{AudioThreadEvent, EventEnvelope, NativeSessionSnapshot};

pub struct EventBus {
    state: parking_lot::Mutex<BusState>,
    tx: mpsc::UnboundedSender<EventEnvelope>,
    /// Mirror of `BusState::epoch` for analysis frames, which take no lock.
    epoch: AtomicU64,
}

#[derive(Default)]
struct BusState {
    seq: u64,
    epoch: u64,
    ack: u64,
    boot_id: u64,
    session: NativeSessionSnapshot,
}

impl EventBus {
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<EventEnvelope>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let bus = Self {
            state: parking_lot::Mutex::new(BusState::default()),
            tx,
            epoch: AtomicU64::new(0),
        };
        (Arc::new(bus), rx)
    }

    pub fn emit(&self, event: AudioThreadEvent) {
        self.emit_with(Some(event), |_| {});
    }

    /// Emit `event` (if any) with `update` applied to the snapshot in the same
    /// step, for snapshot fields the event itself does not carry.
    pub fn emit_with(
        &self,
        event: Option<AudioThreadEvent>,
        update: impl FnOnce(&mut NativeSessionSnapshot),
    ) {
        let mut state = self.state.lock();
        update(&mut state.session);
        if let Some(event) = event {
            reduce(&mut state.session, &event);
            state.push(&self.tx, event);
        }
    }

    pub fn update(&self, update: impl FnOnce(&mut NativeSessionSnapshot)) {
        self.emit_with(None, update);
    }

    /// Analysis output: unordered (`seq` 0), never part of the snapshot.
    /// `false` once nothing drains the bus.
    pub fn emit_ephemeral(&self, event: AudioThreadEvent) -> bool {
        self.tx.send(EventEnvelope {
            seq: 0,
            epoch: self.epoch.load(Ordering::Acquire),
            ack: 0,
            data: event,
        })
        .is_ok()
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// The player moved to a new timeline; events from here on carry it.
    pub fn begin_epoch(&self, epoch: u64) {
        self.state.lock().epoch = epoch;
        self.epoch.store(epoch, Ordering::Release);
    }

    /// The player is taking up frontend command `cmd`; events from here on
    /// acknowledge it. Commands arrive in one FIFO, so this only grows within a
    /// page; a reloaded page restarts above the `ack` its `Hello` carried.
    pub fn take_up_command(&self, cmd: u64) {
        if cmd != 0 {
            self.state.lock().ack = cmd;
        }
    }

    pub fn session(&self) -> NativeSessionSnapshot {
        self.state.lock().session.clone()
    }

    /// A `Hello` for a new subscriber, numbered in the stream but not queued:
    /// the caller hands it straight to that subscriber, ahead of anything the
    /// forwarder still holds — all of which has a lower `seq`.
    pub fn hello(&self) -> EventEnvelope {
        self.state.lock().next_hello()
    }

    /// A stopped player was replaced. The new one holds nothing, so the
    /// snapshot starts over and every subscriber hears it in order.
    pub fn restart(&self) {
        let mut state = self.state.lock();
        state.boot_id += 1;
        state.session = NativeSessionSnapshot::default();
        let hello = state.next_hello();
        let _ = self.tx.send(hello);
    }
}

impl BusState {
    fn push(&mut self, tx: &mpsc::UnboundedSender<EventEnvelope>, event: AudioThreadEvent) {
        self.seq += 1;
        let _ = tx.send(EventEnvelope {
            seq: self.seq,
            epoch: self.epoch,
            ack: self.ack,
            data: event,
        });
    }

    fn next_hello(&mut self) -> EventEnvelope {
        self.seq += 1;
        EventEnvelope {
            seq: self.seq,
            epoch: self.epoch,
            ack: self.ack,
            data: AudioThreadEvent::Hello {
                boot_id: self.boot_id,
                snapshot: self.session.clone(),
            },
        }
    }
}

/// Snapshot fields an event carries in full. Everything else is written by
/// the emitting site through [`EventBus::emit_with`].
fn reduce(session: &mut NativeSessionSnapshot, event: &AudioThreadEvent) {
    match event {
        AudioThreadEvent::PlayPosition { position, .. } => session.position = *position,
        AudioThreadEvent::PlaybackPhaseChanged { phase, .. } => session.phase = *phase,
        AudioThreadEvent::LoadProgress { position } => session.buffered_position = *position,
        AudioThreadEvent::PersonalFmChanged { session: fm } => session.personal_fm = fm.clone(),
        AudioThreadEvent::VolumeChanged { volume } => session.volume = *volume,
        _ => {}
    }
}

// ── Sinks ────────────────────────────────────────────────────────

/// An in-process consumer of the event stream.
///
/// This is how platform glue that must keep working while the WebView is dead
/// — the OS media session, listen-together keepalive — observes playback
/// without going through JS. Subscribers see every ordered event; analysis
/// frames are not delivered (nothing in-process uses them, and a per-subscriber
/// copy of a multi-kilobyte frame at ~30 Hz is not free).
///
/// Implementations must not block: `on_event` runs on the forwarding task,
/// ahead of every later event. They must not subscribe from inside
/// `on_event` — the registry lock is held across the call.
pub trait PlayerEventSubscriber: Send + Sync {
    fn on_event(&self, event: &AudioThreadEvent);
}

type FrontendChannel = Channel<EventEnvelope>;

/// Where the forwarder delivers: one frontend channel, any number of
/// in-process subscribers.
#[derive(Default)]
pub struct EventSinks {
    channel: parking_lot::Mutex<Option<FrontendChannel>>,
    subscribers: parking_lot::RwLock<Vec<Arc<dyn PlayerEventSubscriber>>>,
}

impl EventSinks {
    /// Make `channel` the frontend sink, starting it with a `Hello`.
    ///
    /// The slot is held across numbering the `Hello`, sending it and installing
    /// the channel: an event numbered after the `Hello` can then only be
    /// forwarded once the new channel is in place, so the page that subscribed
    /// sees every event the `Hello` does not already include. Events numbered
    /// before it may still reach the new channel afterwards; their lower `seq`
    /// says they are covered.
    pub fn attach_channel(&self, bus: &EventBus, channel: FrontendChannel) -> Result<(), String> {
        let mut slot = self.channel.lock();
        channel.send(bus.hello()).map_err(|e| e.to_string())?;
        *slot = Some(channel);
        Ok(())
    }

    pub fn subscribe(&self, subscriber: Arc<dyn PlayerEventSubscriber>) {
        self.subscribers.write().push(subscriber);
    }

    fn deliver(&self, envelope: EventEnvelope) {
        if envelope.seq != 0 {
            for subscriber in self.subscribers.read().iter() {
                subscriber.on_event(&envelope.data);
            }
        }
        let Some(channel) = self.channel.lock().clone() else {
            return;
        };
        if channel.send(envelope).is_err() {
            // The webview is gone. Only clear the slot if nobody replaced it.
            let mut slot = self.channel.lock();
            if slot.as_ref().is_some_and(|current| current.id() == channel.id()) {
                *slot = None;
            }
        }
    }
}

/// Drain the bus into the sinks, pacing analysis frames.
pub fn spawn_forwarder(mut rx: mpsc::UnboundedReceiver<EventEnvelope>, sinks: Arc<EventSinks>) {
    tauri::async_runtime::spawn(async move {
        // Outlives one burst: a frame held back by the pace goes with a later one.
        let mut analysis = AnalysisPacer::default();
        let mut ordered = Vec::new();
        while let Some(first) = rx.recv().await {
            analysis.collect(first, &mut ordered);
            while let Ok(next) = rx.try_recv() {
                analysis.collect(next, &mut ordered);
            }
            for envelope in ordered.drain(..) {
                sinks.deliver(envelope);
            }
            if let Some(frame) = analysis.take_due(Instant::now()) {
                sinks.deliver(frame);
            }
        }
    });
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
    epoch: u64,
    last_sent: Option<Instant>,
}

impl AnalysisPacer {
    /// Hold analysis frames back; everything else goes to `ordered` in order.
    fn collect(&mut self, envelope: EventEnvelope, ordered: &mut Vec<EventEnvelope>) {
        match envelope.data {
            AudioThreadEvent::FFTData { data } => {
                self.fft = Some(data);
                self.epoch = envelope.epoch;
            }
            AudioThreadEvent::LowFreqBins { bins } => {
                self.bins = Some(bins);
                self.epoch = envelope.epoch;
            }
            _ => ordered.push(envelope),
        }
    }

    /// The merged frame, if one is waiting and the interval has passed.
    fn take_due(&mut self, now: Instant) -> Option<EventEnvelope> {
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
        Some(EventEnvelope {
            seq: 0,
            epoch: self.epoch,
            ack: 0,
            data: AudioThreadEvent::AnalysisFrame {
                fft: self.fft.take(),
                bins: self.bins.take(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut mpsc::UnboundedReceiver<EventEnvelope>) -> Vec<EventEnvelope> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn every_ordered_event_carries_the_epoch_and_ack_current_when_it_was_emitted() {
        let (bus, mut rx) = EventBus::new();
        bus.emit(AudioThreadEvent::PlayStatus { is_playing: true });
        bus.take_up_command(7);
        bus.begin_epoch(3);
        bus.emit(AudioThreadEvent::PlayStatus { is_playing: false });

        let events = drain(&mut rx);
        assert_eq!(
            events.iter().map(|e| (e.seq, e.epoch, e.ack)).collect::<Vec<_>>(),
            vec![(1, 0, 0), (2, 3, 7)]
        );
    }

    #[test]
    fn a_hello_covers_exactly_the_events_numbered_before_it() {
        let (bus, mut rx) = EventBus::new();
        bus.emit(AudioThreadEvent::PlayPosition {
            position: 12.0,
            timeline_epoch: 0,
        });
        let hello = bus.hello();
        bus.emit(AudioThreadEvent::PlayPosition {
            position: 13.0,
            timeline_epoch: 0,
        });

        let AudioThreadEvent::Hello { snapshot, .. } = &hello.data else {
            panic!("not a hello: {:?}", hello.data);
        };
        assert_eq!(snapshot.position, 12.0);
        let events = drain(&mut rx);
        assert!(events[0].seq < hello.seq);
        assert!(events[1].seq > hello.seq);
    }

    #[test]
    fn a_snapshot_update_lands_with_its_event_in_one_step() {
        let (bus, mut rx) = EventBus::new();
        bus.emit_with(
            Some(AudioThreadEvent::PlayPosition {
                position: 4.0,
                timeline_epoch: 0,
            }),
            |session| session.is_playing = true,
        );
        let session = bus.session();
        assert!(session.is_playing);
        assert_eq!(session.position, 4.0);
        assert_eq!(drain(&mut rx).len(), 1);
    }

    #[test]
    fn analysis_frames_are_unordered_and_never_touch_the_snapshot() {
        let (bus, mut rx) = EventBus::new();
        bus.begin_epoch(5);
        bus.emit_ephemeral(AudioThreadEvent::FFTData { data: vec![1.0] });
        let events = drain(&mut rx);
        assert_eq!((events[0].seq, events[0].epoch), (0, 5));
        assert_eq!(bus.hello().seq, 1);
    }

    #[test]
    fn a_restart_is_announced_in_order_with_an_empty_snapshot() {
        let (bus, mut rx) = EventBus::new();
        bus.update(|session| session.has_track = true);
        bus.restart();
        let events = drain(&mut rx);
        let AudioThreadEvent::Hello { boot_id, snapshot } = &events[0].data else {
            panic!("not a hello: {:?}", events[0].data);
        };
        assert_eq!(*boot_id, 1);
        assert!(!snapshot.has_track);
    }

    #[test]
    fn the_pacer_merges_analysis_frames_and_passes_everything_else_in_order() {
        let mut pacer = AnalysisPacer::default();
        let mut ordered = Vec::new();
        let frame = |data, epoch| EventEnvelope {
            seq: 0,
            epoch,
            ack: 0,
            data,
        };
        pacer.collect(frame(AudioThreadEvent::FFTData { data: vec![1.0] }, 1), &mut ordered);
        pacer.collect(
            frame(AudioThreadEvent::PlayStatus { is_playing: true }, 1),
            &mut ordered,
        );
        pacer.collect(frame(AudioThreadEvent::LowFreqBins { bins: vec![2.0] }, 2), &mut ordered);
        assert_eq!(ordered.len(), 1);

        let t0 = Instant::now();
        let merged = pacer.take_due(t0).expect("a frame is waiting");
        assert_eq!(merged.epoch, 2);
        assert!(matches!(
            merged.data,
            AudioThreadEvent::AnalysisFrame {
                fft: Some(_),
                bins: Some(_)
            }
        ));
        pacer.collect(frame(AudioThreadEvent::FFTData { data: vec![3.0] }, 2), &mut ordered);
        assert!(pacer.take_due(t0 + ANALYSIS_FORWARD_INTERVAL / 2).is_none());
        assert!(pacer.take_due(t0 + ANALYSIS_FORWARD_INTERVAL).is_some());
    }
}
