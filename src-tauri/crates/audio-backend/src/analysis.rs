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
//! OS thread. Everything arrives via `AnalysisCommand` over `std::sync::mpsc`:
//!   - `Pcm` carries interleaved samples (audio thread's native format) so
//!     downmix-to-mono work happens on the analysis thread, not the audio
//!     callback thread.
//!   - `Clear` / `SetFreqRange` are control commands triggered by the
//!     player thread on seek / track change / frequency-range updates.
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

/// Commands from the audio callback / player thread to the analysis thread.
/// `Pcm` carries interleaved samples — the downmix runs on this thread so
/// the audio callback thread doesn't pay the cost.
///
/// `Pcm.recycle` is an optional return channel: after processing, the
/// analysis thread clears the Vec and sends it back through this channel
/// so the audio callback thread can reuse the backing allocation instead
/// of asking the allocator for a fresh 4 KB block ~88 times/sec. When the
/// FFTFeedSource is dropped the receiver disappears and recycled sends
/// silently fail — which is exactly what we want (the Vec just gets
/// dropped).
pub enum AnalysisCommand {
    Pcm {
        samples: Vec<f32>,
        channels: u16,
        sample_rate: u32,
        recycle: Option<mpsc::Sender<Vec<f32>>>,
    },
    Clear,
    SetFreqRange {
        from: f32,
        to: f32,
    },
}

/// Spawn the dedicated analysis OS thread and return a `Sender` for
/// commands plus its `JoinHandle` (so the owner can join on shutdown if
/// it wants to).
///
/// The loop terminates when every `Sender` has been dropped (the channel
/// returns `Disconnected`) or the event sink is closed.
pub fn spawn_analysis_thread(
    evt_sender: tokio_mpsc::UnboundedSender<AudioThreadEventMessage<AudioThreadEvent>>,
) -> std::io::Result<(mpsc::Sender<AnalysisCommand>, std::thread::JoinHandle<()>)> {
    let (tx, rx) = mpsc::channel::<AnalysisCommand>();
    let handle = std::thread::Builder::new()
        .name("audio-analysis".into())
        .spawn(move || analysis_loop(rx, evt_sender))?;
    Ok((tx, handle))
}

fn analysis_loop(
    rx: mpsc::Receiver<AnalysisCommand>,
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

    // LowFreqAnalyzer::new starts at 1.0 for WASM compatibility. Native IPC
    // should start from silence so the first lowFreq event does not jump.
    proc.clear();

    loop {
        let wait = ANALYSIS_INTERVAL.saturating_sub(last_analysis.elapsed());
        match rx.recv_timeout(wait) {
            Ok(cmd) => {
                let result = handle_cmd(&mut proc, cmd);
                let reset = matches!(result, HandleCmdResult::Reset);
                apply_cmd_result(result, &mut pending_mono_samples, &mut current_sample_rate);
                if reset && emit_low_freq(&evt, 0.0).is_err() {
                    break;
                }
                while let Ok(more) = rx.try_recv() {
                    let result = handle_cmd(&mut proc, more);
                    let reset = matches!(result, HandleCmdResult::Reset);
                    apply_cmd_result(result, &mut pending_mono_samples, &mut current_sample_rate);
                    if reset && emit_low_freq(&evt, 0.0).is_err() {
                        return;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if last_analysis.elapsed() >= ANALYSIS_INTERVAL && pending_mono_samples >= FFT_SIZE {
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
                last_fft_emit = now;
            }
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

fn handle_cmd(proc: &mut AudioProcessor, cmd: AnalysisCommand) -> HandleCmdResult {
    match cmd {
        AnalysisCommand::Pcm {
            mut samples,
            channels,
            sample_rate,
            recycle,
        } => {
            let len = proc.push_interleaved_pcm(&samples, channels, sample_rate);
            if let Some(tx) = recycle {
                samples.clear();
                let _ = tx.send(samples);
            }
            HandleCmdResult::Pushed {
                samples: len,
                sample_rate,
            }
        }
        AnalysisCommand::Clear => {
            proc.clear();
            HandleCmdResult::Reset
        }
        AnalysisCommand::SetFreqRange { from, to } => {
            proc.fft.set_freq_range(from, to);
            HandleCmdResult::None
        }
    }
}
