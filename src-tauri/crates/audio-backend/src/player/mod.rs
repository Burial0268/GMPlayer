/// AMLL-style message-driven audio player.
///
/// Architecture (mirrors `amll-player-core`):
/// - `Player` — public API: sends `AudioThreadMessage` to the internal player thread.
/// - `PlayerHandle` — cloneable handle for sending messages from anywhere.
/// - `AudioPlayer` — internal event loop that processes messages + emits events.
///
/// Message flow:  frontend → WebSocket → Player::send_msg() → AudioPlayer → decoder/output
/// Event flow:   AudioPlayer → callback → WebSocket broadcast → frontend
///
/// The `AudioPlayer` impl is split by responsibility across sibling modules —
/// this file owns the struct, construction and the `run()` event loop:
/// - `messages`        — `AudioThreadMessage` dispatch (`process_message`)
/// - `playback`        — track loading, prebuffer gating, decoder-finished advance
/// - `seek`            — seek validation/apply/defer
/// - `output_runtime`  — device polling/health, hot-swap refresh, chain rebuilds
/// - `automix_runtime` — native AutoMix deck preload/crossfade scheduling
/// - `status`          — `EventEmitter` + position/seek/SyncStatus publishing
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::sync::RwLock as TokioRwLock;
use tracing::{info, warn};

use crate::analysis::{self, AnalysisSender};
use crate::decoder;
use crate::error::{AudioError, AudioResult};
use crate::output::{self, LowLatencyOutput};
use crate::types::*;

mod api;
mod automix;
mod automix_runtime;
mod clock;
mod messages;
mod mixer;
mod output_runtime;
mod platform;
mod playback;
pub mod queue;
mod seek;
mod status;

#[allow(unused_imports)]
pub use api::{EventBuffer, Player, PlayerHandle, PlayerShared};

use api::{join_thread_async, SeekRequest};
use automix::AutoMixManager;
use clock::PlayerClock;
use mixer::{DeckId, DeckMixer};
use output_runtime::OutputRefreshEvent;
use queue::PlaybackQueue;
use status::EventEmitter;

// ═══════════════════════════════════════════════════════════════════
//  Internal AudioPlayer
// ═══════════════════════════════════════════════════════════════════

struct AudioPlayer {
    // Channels
    msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
    seek_rx: mpsc::UnboundedReceiver<SeekRequest>,
    evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    // Self-message channel: lets `run()` re-enter `process_message` for
    // auto-advance, matching the AMLL reference's NextSongGapless pattern.
    self_msg_tx: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadMessage>>,
    self_msg_rx: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,

    // CPAL playback. Decoding runs on a worker thread and pushes PCM blocks
    // into the deck mixer; the mixer pushes mixed PCM blocks to this output queue.
    output: LowLatencyOutput,
    output_selector: output::OutputDeviceSelector,
    deck_mixer: DeckMixer,
    active_deck: DeckId,
    /// Persistent per-track loudness-normalization gain currently applied to the
    /// active deck (the AutoMix `volume_norm` adjustment, range ~[0.1, 2.0]).
    /// Master volume rides separately on the output writer (`set_volume`), so
    /// this must survive crossfades, cancels and output rebuilds unchanged —
    /// otherwise the deck gain snaps back to 1.0 and the track jumps in level.
    /// Mirrors the mixer's active-deck gain during steady-state playback.
    active_norm_gain: f32,
    dsp_config: DspConfig,
    automix: AutoMixManager,
    volume: f64,
    /// What the user wants the sink to be doing. Used so auto-advance and
    /// seek transitions preserve the playing/paused state even though the
    /// underlying sink momentarily becomes empty/stopped during the swap.
    playback_intent: PlaybackIntent,
    clock: Arc<parking_lot::Mutex<PlayerClock>>,

    // Current track
    current_file_path: Option<String>,
    /// Resolved local path: same as `current_file_path` for local files, or
    /// the temp-file path created when streaming a remote URL.
    current_local_path: Option<PathBuf>,
    /// RAII guard for the downloaded temp file — dropped when a new track
    /// loads, which deletes the temp file from disk.
    current_temp_file: Option<tempfile::TempPath>,
    current_decoder_handle: Option<decoder::DecoderHandle>,
    pending_seek: Option<SeekRequest>,
    secondary_decoder_handle: Option<decoder::DecoderHandle>,
    secondary_temp_file: Option<tempfile::TempPath>,
    secondary_local_path: Option<PathBuf>,
    secondary_song: Option<SongData>,
    secondary_duration: f64,
    /// Normalization gain of the preloaded/incoming (secondary) deck's track.
    /// Captured at crossfade start and promoted to `active_norm_gain` when the
    /// crossfade completes, so the incoming level is continuous into steady
    /// state and into the next crossfade's outgoing side.
    secondary_norm_gain: f32,
    secondary_display_info: Option<DisplayAudioInfo>,
    secondary_quality: Option<AudioQuality>,
    secondary_playback_id: Option<u64>,
    decoder_event_tx: mpsc::UnboundedSender<decoder::DecoderEvent>,
    decoder_event_rx: mpsc::UnboundedReceiver<decoder::DecoderEvent>,
    automix_prepare_tx: mpsc::UnboundedSender<automix::AutoMixPrepareResult>,
    automix_prepare_rx: mpsc::UnboundedReceiver<automix::AutoMixPrepareResult>,
    output_refresh_tx: mpsc::UnboundedSender<OutputRefreshEvent>,
    output_refresh_rx: mpsc::UnboundedReceiver<OutputRefreshEvent>,
    output_refresh_pending: bool,
    output_refresh_generation: u64,
    output_epoch: u64,
    output_refresh_dirty: bool,
    output_refresh_dirty_force: bool,
    output_refresh_dirty_rebuild_chain: bool,
    output_refresh_failures: u8,
    output_refresh_backoff_until: Option<std::time::Instant>,
    output_poll_stride: u32,
    output_poll_ticks: u32,
    output_health_last_samples: u64,
    output_health_stalled_ticks: u8,
    last_output_error: Option<String>,
    decoder_playback_id: u64,
    native_crossfade_generation: u64,
    native_crossfade_active: bool,
    native_crossfade_transition_id: Option<u64>,
    automix_prepare_generation: u64,

    // Playlist
    playback_queue: PlaybackQueue,
    playlist: Vec<SongData>,
    playlist_inited: bool,
    current_play_index: usize,
    current_song: Option<SongData>,

    // Shared state snapshots
    current_audio_info: Arc<TokioRwLock<DisplayAudioInfo>>,
    current_position: Arc<TokioRwLock<f64>>,
    current_audio_quality: Arc<TokioRwLock<AudioQuality>>,

    // Background tasks
    tasks: Vec<tokio::task::JoinHandle<()>>,

    /// Sender to the dedicated `audio-analysis` OS thread (see
    /// `crate::analysis`). Cloned into each `FFTFeedSource` so the audio
    /// callback thread can push interleaved PCM via the channel — replaces
    /// the previous `Arc<ParkingLotRwLock<AudioProcessor>>` lock pattern.
    analysis_tx: AnalysisSender,
    /// JoinHandle for the analysis thread. Held so `Drop for AudioPlayer`
    /// can join after dropping the `Sender` (which closes the channel and
    /// signals the thread to exit). Wrapped in `Option` so `take()` works
    /// in `Drop`.
    analysis_thread: Option<std::thread::JoinHandle<()>>,
    analysis_enabled: Arc<AtomicBool>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PlaybackIntent {
    Playing,
    Paused,
}

impl AudioPlayer {
    async fn new(
        msg_receiver: mpsc::UnboundedReceiver<AudioThreadEventMessage<AudioThreadMessage>>,
        seek_rx: mpsc::UnboundedReceiver<SeekRequest>,
        evt_sender: mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    ) -> AudioResult<Self> {
        let output_selector = output::OutputDeviceSelector::Default;
        let output =
            output::open_output(output_selector.clone(), None).map_err(AudioError::Output)?;
        output.writer().set_paused(true);
        let dsp_config = DspConfig::default();
        let deck_mixer = DeckMixer::new(
            output.writer(),
            output.config().channels,
            output.config().sample_rate,
            &dsp_config,
        );
        let automix = AutoMixManager::new();
        let clock = Arc::new(parking_lot::Mutex::new(PlayerClock::new()));
        {
            let writer = output.writer();
            let config = output.config();
            clock.lock().set_render_clock(
                writer.render_clock(),
                config.sample_rate,
                config.channels,
            );
        }

        info!("音频输出设备 准备就绪");

        let current_audio_info = Arc::new(TokioRwLock::new(DisplayAudioInfo::default()));
        let current_position = Arc::new(TokioRwLock::new(0.0));
        let current_audio_quality = Arc::new(TokioRwLock::new(AudioQuality::default()));

        // Dedicated `audio-analysis` OS thread owns the `AudioProcessor` so
        // FFT work doesn't compete with the player's `current_thread` tokio
        // runtime. PCM flows in via the returned `Sender`; FFT/LowFreq events
        // are emitted through the existing `evt_sender` (multi-`Send`).
        let (analysis_tx, analysis_thread) = analysis::spawn_analysis_thread(evt_sender.clone())
            .map_err(|e| AudioError::ThreadError(format!("spawn audio-analysis thread: {e}")))?;
        let analysis_thread = Some(analysis_thread);
        let analysis_enabled = Arc::new(AtomicBool::new(true));

        let mut tasks = Vec::new();

        // ── Position tracking task ───────────────────────────────────
        //
        // The frontend extrapolates intermediate position locally from the
        // same clock anchor. Rust publishes immediate anchors on play/pause/
        // seek and a low-rate heartbeat for reconciliation.
        let position_writer = current_position.clone();
        let clock_reader = Arc::clone(&clock);
        let emitter_pos = EventEmitter::new(evt_sender.clone());

        tasks.push(tokio::task::spawn(async move {
            let mut time_it = tokio::time::interval(Duration::from_secs(1));
            time_it.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                time_it.tick().await;
                let (is_playing, current_pos) = {
                    let clock = clock_reader.lock();
                    (clock.is_playing(), clock.position())
                };
                if is_playing {
                    *position_writer.write().await = current_pos;
                    let _ = emitter_pos
                        .emit(AudioThreadEvent::PlayPosition {
                            position: current_pos,
                        })
                        .await;
                }
            }
        }));

        // ── FFT + lowFreq broadcast ──────────────────────────────────
        //
        // Moved out of this runtime entirely — see `crate::analysis`. The
        // dedicated OS thread owns the `AudioProcessor`, receives PCM via
        // `analysis_tx`, and emits `FFTData` + `LowFrequencyVolume` events
        // through the same `evt_sender` we use here.

        let (self_msg_tx, self_msg_rx) = mpsc::unbounded_channel();
        let (decoder_event_tx, decoder_event_rx) = mpsc::unbounded_channel();
        let (automix_prepare_tx, automix_prepare_rx) = mpsc::unbounded_channel();
        let (output_refresh_tx, output_refresh_rx) = mpsc::unbounded_channel();

        Ok(Self {
            msg_receiver,
            seek_rx,
            evt_sender,
            self_msg_tx,
            self_msg_rx,
            output,
            output_selector,
            deck_mixer,
            active_deck: DeckId::Primary,
            active_norm_gain: 1.0,
            dsp_config,
            automix,
            volume: 1.0,
            playback_intent: PlaybackIntent::Paused,
            clock,
            current_file_path: None,
            current_local_path: None,
            current_temp_file: None,
            current_decoder_handle: None,
            pending_seek: None,
            secondary_decoder_handle: None,
            secondary_temp_file: None,
            secondary_local_path: None,
            secondary_song: None,
            secondary_duration: 0.0,
            secondary_norm_gain: 1.0,
            secondary_display_info: None,
            secondary_quality: None,
            secondary_playback_id: None,
            decoder_event_tx,
            decoder_event_rx,
            automix_prepare_tx,
            automix_prepare_rx,
            output_refresh_tx,
            output_refresh_rx,
            output_refresh_pending: false,
            output_refresh_generation: 0,
            output_epoch: 0,
            output_refresh_dirty: false,
            output_refresh_dirty_force: false,
            output_refresh_dirty_rebuild_chain: false,
            output_refresh_failures: 0,
            output_refresh_backoff_until: None,
            output_poll_stride: 1,
            output_poll_ticks: 0,
            output_health_last_samples: 0,
            output_health_stalled_ticks: 0,
            last_output_error: None,
            decoder_playback_id: 0,
            native_crossfade_generation: 0,
            native_crossfade_active: false,
            native_crossfade_transition_id: None,
            automix_prepare_generation: 0,
            playback_queue: PlaybackQueue::new(),
            playlist: Vec::new(),
            playlist_inited: false,
            current_play_index: 0,
            current_song: None,
            current_audio_info,
            current_position,
            current_audio_quality,
            tasks,
            analysis_tx,
            analysis_thread,
            analysis_enabled,
        })
    }

    async fn run(mut self) {
        let mut output_device_check = tokio::time::interval(Duration::from_secs(1));
        output_device_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut output_health_check = tokio::time::interval(Duration::from_millis(100));
        output_health_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
              biased;

              seek = self.seek_rx.recv() => {
                if let Some(first_seek) = seek {
                  let seek = self.drain_latest_seek(first_seek);
                  if let Err(err) = self.process_seek_request(seek).await {
                    warn!("处理 seek 消息时出错：{err:?}");
                  }
                } else {
                  break;
                }
              }

              msg = self.msg_receiver.recv() => {
                if let Some(msg) = msg {
                  if let Some(AudioThreadMessage::Close) = &msg.data { break; }
                  if let Err(err) = self.process_message(msg).await {
                    warn!("处理音频线程消息时出错：{err:?}");
                  }
                } else { break; }
              }

              msg = self.self_msg_rx.recv() => {
                if let Some(msg) = msg {
                  if let Err(err) = self.process_message(msg).await {
                    warn!("处理内部音频线程消息时出错：{err:?}");
                  }
                }
              }

              event = self.decoder_event_rx.recv() => {
                if let Some(decoder::DecoderEvent::Finished { playback_id }) = event {
                  self.handle_decoder_finished(playback_id).await;
                }
              }

              result = self.automix_prepare_rx.recv() => {
                if let Some(result) = result {
                  self.handle_automix_prepare_result(result).await;
                }
              }

              _ = output_device_check.tick() => {
                self.poll_output_device_tick();
              }

              _ = output_health_check.tick() => {
                let output_failed = self.output.has_failed();
                let output_stalled = self.output_render_stalled();
                if output_failed || output_stalled {
                  self.request_output_refresh(true, output_stalled);
                }
              }

              event = self.output_refresh_rx.recv() => {
                if let Some(event) = event {
                  self.handle_output_refresh_event(event).await;
                }
              }
            }
        }

        // Cleanup
        for task in &self.tasks {
            task.abort();
        }
        // Dropping `self.analysis_tx` (when `AudioPlayer` drops below) signals
        // the analysis thread to exit via `mpsc::RecvTimeoutError::Disconnected`.
        // The `Drop for AudioPlayer` impl joins the thread.
    }

    fn drain_latest_seek(&mut self, first_seek: SeekRequest) -> SeekRequest {
        let mut seek = first_seek;
        while let Ok(next_seek) = self.seek_rx.try_recv() {
            seek = next_seek;
        }
        seek
    }

    fn sync_current_from_queue(&mut self) -> bool {
        let Some(song) = self.playback_queue.current_song() else {
            self.current_song = None;
            self.current_play_index = 0;
            return false;
        };
        self.current_play_index = self.playback_queue.current_index();
        self.current_song = Some(song);
        true
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(thread) = self.analysis_thread.take() {
            join_thread_async("audio-analysis-join", thread);
        }
    }
}
