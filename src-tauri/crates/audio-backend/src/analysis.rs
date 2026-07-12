//! Audio analysis pipeline (FFT) running on a dedicated OS thread.
//!
//! Previously `FFTFeedSource` (audio callback thread) and the FFT broadcast
//! task (audio player tokio runtime) shared an
//! `Arc<ParkingLotRwLock<AudioProcessor>>`. That gave us `try_write`-based
//! non-blocking pushes from the audio thread, but the FFT task still tied up
//! the audio player's `current_thread` runtime for ~1 ms per tick during
//! `process_frame`, and the shared lock was a potential priority-inversion
//! risk if a low-priority thread held the write lock while the audio thread
//! tried to push.
//!
//! Here the `AudioProcessor` lives entirely on a dedicated `audio-analysis`
//! OS thread. Reliable controls and best-effort PCM use separate channels.
//! The PCM channel is deliberately tiny and bounded: analysis may drop audio
//! under load, but it can never backlog memory or stall playback/decoding.
//!
//! `LowFrequencyVolume` is derived from the same FFT frame here, matching
//! AMLL's frontend low-pass path while keeping extra DSP out of the CPAL
//! callback.
//!
//! Events leave through the same `tokio::sync::mpsc::UnboundedSender` that
//! every other player event uses, so the WS forwarder picks them up unchanged.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use audio_analysis::{AudioProcessor, LowFreqConfig};
use tokio::sync::mpsc as tokio_mpsc;

use crate::types::{AudioThreadEvent, AudioThreadEventMessage};

const FFT_SIZE: usize = 2048;
const ANALYSIS_INTERVAL: Duration = Duration::from_millis(16);
const FFT_EMIT_INTERVAL: Duration = Duration::from_millis(33);
const PCM_QUEUE_CAPACITY: usize = 3;
const PCM_BLOCKS_PER_TICK: usize = 2;

/// Reliable controls from the player and decoder threads.
pub enum AnalysisCommand {
    Clear,
    SetEnabled { enabled: bool },
    SetFftEnabled { enabled: bool },
    SetFreqRange { from: f32, to: f32 },
}

pub struct AnalysisPcm {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    pub recycle: Option<mpsc::Sender<Vec<f32>>>,
}

#[derive(Clone)]
pub struct AnalysisSender {
    control_tx: mpsc::Sender<AnalysisCommand>,
    pcm_tx: mpsc::SyncSender<AnalysisPcm>,
}

impl AnalysisSender {
    pub fn send(&self, command: AnalysisCommand) -> Result<(), mpsc::SendError<AnalysisCommand>> {
        self.control_tx.send(command)
    }

    /// Submit PCM without ever waiting for the analysis thread. On overload
    /// the caller gets ownership back and can immediately recycle the buffer.
    pub fn try_send_pcm(&self, pcm: AnalysisPcm) -> Result<(), mpsc::TrySendError<AnalysisPcm>> {
        self.pcm_tx.try_send(pcm)
    }
}

/// Spawn the dedicated analysis OS thread and return a `Sender` for
/// commands plus its `JoinHandle` (so the owner can join on shutdown if
/// it wants to).
///
/// The loop terminates when every `Sender` has been dropped (the channel
/// returns `Disconnected`) or the event sink is closed.
pub fn spawn_analysis_thread(
    evt_sender: tokio_mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
) -> std::io::Result<(AnalysisSender, std::thread::JoinHandle<()>)> {
    let (control_tx, control_rx) = mpsc::channel::<AnalysisCommand>();
    let (pcm_tx, pcm_rx) = mpsc::sync_channel::<AnalysisPcm>(PCM_QUEUE_CAPACITY);
    let handle = std::thread::Builder::new()
        .name("audio-analysis".into())
        .spawn(move || analysis_loop(control_rx, pcm_rx, evt_sender))?;
    Ok((AnalysisSender { control_tx, pcm_tx }, handle))
}

fn analysis_loop(
    control_rx: mpsc::Receiver<AnalysisCommand>,
    pcm_rx: mpsc::Receiver<AnalysisPcm>,
    evt: tokio_mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
) {
    let cfg = LowFreqConfig::default();
    let mut proc = AudioProcessor::new(
        2048,
        80.0,
        2000.0,
        cfg.bin_count,
        cfg.window_size,
        cfg.gradient_threshold,
        cfg.smoothing_factor,
    );

    // `spectrum` is only a scratch buffer for the processor's normalized WASM
    // compatibility path. Native IPC emits raw FFT magnitudes from
    // `proc.fft.raw_spectrum()`, matching AMLL's fftDataAtom data flow.
    let mut spectrum = vec![0.0f32; 2048];
    let mut last_analysis = Instant::now();
    let mut last_fft_emit = Instant::now() - FFT_EMIT_INTERVAL;
    let mut pending_mono_samples = 0usize;
    let mut current_sample_rate = 44_100u32;
    let mut analysis_enabled = true;
    let mut fft_enabled = true;

    // LowFreqAnalyzer::new starts at 1.0 for WASM compatibility. Native IPC
    // should start from silence so the first lowFreq event does not jump.
    proc.clear();

    loop {
        // Controls are reliable and always drained before best-effort PCM.
        while let Ok(cmd) = control_rx.try_recv() {
            let result = handle_cmd(&mut proc, cmd, &mut analysis_enabled, &mut fft_enabled);
            let reset = matches!(result, HandleCmdResult::Reset);
            apply_cmd_result(result, &mut pending_mono_samples, &mut current_sample_rate);
            if reset && emit_low_freq(&evt, 0.0).is_err() {
                return;
            }
        }

        // Bound work per iteration so a PCM burst cannot starve controls or
        // FFT cadence. Two blocks/tick still keeps up with 512-frame decode
        // blocks at common sample rates during normal operation.
        for _ in 0..PCM_BLOCKS_PER_TICK {
            let Ok(pcm) = pcm_rx.try_recv() else {
                break;
            };
            apply_cmd_result(
                handle_pcm(&mut proc, pcm, analysis_enabled),
                &mut pending_mono_samples,
                &mut current_sample_rate,
            );
        }

        if analysis_enabled
            && last_analysis.elapsed() >= ANALYSIS_INTERVAL
            && pending_mono_samples >= FFT_SIZE
        {
            let now = Instant::now();
            let delta_ms = now.duration_since(last_analysis).as_secs_f32() * 1000.0;
            let low_freq = proc.process_frame(delta_ms, &mut spectrum);

            let drained = ((delta_ms / 1000.0) * current_sample_rate.max(1) as f32) as usize;
            pending_mono_samples = pending_mono_samples.saturating_sub(drained.max(1));
            last_analysis = now;

            if last_fft_emit.elapsed() >= FFT_EMIT_INTERVAL {
                if emit_low_freq(&evt, low_freq).is_err() {
                    break;
                }
                if fft_enabled {
                    if evt
                        .send(AudioThreadEventMessage::new(
                            String::new(),
                            Some(AudioThreadEvent::FFTData {
                                data: proc.fft.raw_spectrum().to_vec(),
                            }),
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
                last_fft_emit = now;
            }
        }

        // Sleep on the reliable control channel rather than polling every
        // millisecond. A control wakes the thread immediately; PCM remains
        // best-effort and is sampled on the next analysis tick.
        let wait = if analysis_enabled && pending_mono_samples >= FFT_SIZE {
            ANALYSIS_INTERVAL.saturating_sub(last_analysis.elapsed())
        } else {
            ANALYSIS_INTERVAL
        };
        match control_rx.recv_timeout(wait) {
            Ok(cmd) => {
                let result = handle_cmd(&mut proc, cmd, &mut analysis_enabled, &mut fft_enabled);
                let reset = matches!(result, HandleCmdResult::Reset);
                apply_cmd_result(result, &mut pending_mono_samples, &mut current_sample_rate);
                if reset && emit_low_freq(&evt, 0.0).is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn emit_low_freq(
    evt: &tokio_mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
    value: f32,
) -> Result<(), tokio_mpsc::error::SendError<AudioThreadEventMessage<AudioThreadEvent>>> {
    evt.send(AudioThreadEventMessage::new(
        String::new(),
        Some(AudioThreadEvent::LowFrequencyVolume {
            volume: value as f64,
        }),
    ))
}

#[derive(Clone, Copy)]
enum HandleCmdResult {
    Pushed { samples: usize, sample_rate: u32 },
    Reset,
    None,
}

fn apply_cmd_result(
    result: HandleCmdResult,
    pending_mono_samples: &mut usize,
    current_sample_rate: &mut u32,
) {
    match result {
        HandleCmdResult::Pushed {
            samples,
            sample_rate,
        } => {
            *pending_mono_samples += samples;
            *current_sample_rate = sample_rate.max(1);
        }
        HandleCmdResult::Reset => {
            *pending_mono_samples = 0;
        }
        HandleCmdResult::None => {}
    }
}

fn handle_cmd(
    proc: &mut AudioProcessor,
    cmd: AnalysisCommand,
    analysis_enabled: &mut bool,
    fft_enabled: &mut bool,
) -> HandleCmdResult {
    match cmd {
        AnalysisCommand::Clear => {
            proc.clear();
            HandleCmdResult::Reset
        }
        AnalysisCommand::SetEnabled { enabled } => {
            if *analysis_enabled == enabled {
                return HandleCmdResult::None;
            }
            *analysis_enabled = enabled;
            proc.clear();
            HandleCmdResult::Reset
        }
        AnalysisCommand::SetFftEnabled { enabled } => {
            *fft_enabled = enabled;
            HandleCmdResult::None
        }
        AnalysisCommand::SetFreqRange { from, to } => {
            proc.fft.set_freq_range(from, to);
            HandleCmdResult::None
        }
    }
}

fn handle_pcm(proc: &mut AudioProcessor, pcm: AnalysisPcm, enabled: bool) -> HandleCmdResult {
    let AnalysisPcm {
        mut samples,
        channels,
        sample_rate,
        recycle,
    } = pcm;
    let len = if enabled {
        proc.push_interleaved_pcm(&samples, channels, sample_rate)
    } else {
        0
    };
    if let Some(tx) = recycle {
        samples.clear();
        let _ = tx.send(samples);
    }
    if enabled {
        HandleCmdResult::Pushed {
            samples: len,
            sample_rate,
        }
    } else {
        HandleCmdResult::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(value: f32) -> AnalysisPcm {
        AnalysisPcm {
            samples: vec![value; 16],
            channels: 2,
            sample_rate: 48_000,
            recycle: None,
        }
    }

    #[test]
    fn pcm_overload_is_bounded_and_returns_ownership() {
        let (control_tx, control_rx) = mpsc::channel();
        let (pcm_tx, _pcm_rx) = mpsc::sync_channel(2);
        let sender = AnalysisSender { control_tx, pcm_tx };

        assert!(sender.try_send_pcm(pcm(1.0)).is_ok());
        assert!(sender.try_send_pcm(pcm(2.0)).is_ok());
        let dropped = match sender.try_send_pcm(pcm(3.0)) {
            Err(mpsc::TrySendError::Full(pcm)) => pcm,
            _ => panic!("full PCM queue must reject immediately"),
        };
        assert_eq!(dropped.samples[0], 3.0);

        // A saturated PCM queue is independent of reliable controls.
        sender.send(AnalysisCommand::Clear).unwrap();
        assert!(matches!(control_rx.try_recv(), Ok(AnalysisCommand::Clear)));
    }
}
