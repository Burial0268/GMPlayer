//! Audio analysis pipeline (FFT + lowFreq) running on a dedicated OS thread.
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
//! Events leave through the same `tokio::sync::mpsc::UnboundedSender` that
//! every other player event uses, so the WS forwarder picks them up unchanged.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use audio_analysis::{AudioProcessor, LowFreqConfig};
use tokio::sync::mpsc as tokio_mpsc;

use crate::types::{AudioThreadEvent, AudioThreadEventMessage};

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

    // Scratch buffer for `process_frame`'s 0-255 normalized output. We do NOT
    // emit it — it only exists because `process_frame` requires an output
    // slice. The spectrum we actually broadcast is the RAW freq-sampled
    // magnitudes from `proc.fft.raw_spectrum()` / `frame_buf` (length 2048):
    // one unwindowed FFT per tick, no cross-frame EMA.
    let mut scratch = vec![0.0f32; 2048];
    let emit_interval = Duration::from_millis(50);
    let mut last_emit = Instant::now();
    let mut low_freq_smoothed = 0.0f32;

    loop {
        match rx.recv_timeout(emit_interval) {
            Ok(cmd) => {
                handle_cmd(&mut proc, cmd);
                while let Ok(more) = rx.try_recv() {
                    handle_cmd(&mut proc, more);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if last_emit.elapsed() >= emit_interval {
            // Update FFT once. `scratch` receives the internal display path
            // and is intentionally not emitted.
            let _ = proc.process_frame(50.0, &mut scratch);
            let raw_spectrum = proc.fft.raw_spectrum();
            let low_freq = amll_low_freq_from_raw(raw_spectrum, &mut low_freq_smoothed);

            // IPC Service: emit RAW freq-sampled magnitudes (length 2048). The
            // frontend normalizes once for display (getFrequencyData), and the
            // lowFreq below is likewise derived from raw bins — no double-processing.
            if evt
                .send(AudioThreadEventMessage::new(
                    String::new(),
                    Some(AudioThreadEvent::FFTData {
                        data: raw_spectrum.to_vec(),
                    }),
                ))
                .is_err()
            {
                break;
            }
            if evt
                .send(AudioThreadEventMessage::new(
                    String::new(),
                    Some(AudioThreadEvent::LowFrequencyVolume {
                        volume: low_freq as f64,
                    }),
                ))
                .is_err()
            {
                break;
            }
            last_emit = Instant::now();
        }
    }
}

/// Low-frequency volume derived from the same raw FFT frame sent to the
/// frontend. This preserves the AMLL-current threshold + 0.2 smoothing behavior
/// but replaces the old 0-255 smoothed spectrum input with per-frame raw
/// magnitudes, normalized only against the current raw frame peak.
fn amll_low_freq_from_raw(raw: &[f32], smoothed: &mut f32) -> f32 {
    const LOW_BINS: usize = 128;
    const THRESHOLD: f32 = 0.08;
    const BOOST_FLOOR: f32 = 0.4;
    const SMOOTHING: f32 = 0.2;

    let frame_peak = raw.iter().copied().fold(0.0f32, f32::max);
    let target = if frame_peak > f32::EPSILON {
        let end = LOW_BINS.min(raw.len());
        let low_peak = raw[..end].iter().copied().fold(0.0f32, f32::max);
        let mut value = (low_peak / frame_peak) * 2.0;
        if value > THRESHOLD {
            value = value.max(BOOST_FLOOR);
        } else {
            value = 0.0;
        }
        value
    } else {
        0.0
    };

    *smoothed += (target - *smoothed) * SMOOTHING;
    smoothed.clamp(0.0, 1.0)
}

fn handle_cmd(proc: &mut AudioProcessor, cmd: AnalysisCommand) {
    match cmd {
        AnalysisCommand::Pcm {
            mut samples,
            channels,
            sample_rate,
            recycle,
        } => {
            let ch = (channels as usize).max(1);
            if ch == 1 {
                proc.push_pcm(&samples, sample_rate);
            } else {
                let inv = 1.0 / ch as f32;
                let mut mono = Vec::with_capacity(samples.len() / ch);
                for chunk in samples.chunks_exact(ch) {
                    let m: f32 = chunk.iter().sum::<f32>() * inv;
                    mono.push(m);
                }
                proc.push_pcm(&mono, sample_rate);
            }
            // Return the buffer to the audio thread for reuse. If the source
            // has been dropped (song change, app shutdown) the send fails
            // silently — the Vec just gets dropped, no harm done.
            if let Some(tx) = recycle {
                samples.clear();
                let _ = tx.send(samples);
            }
        }
        AnalysisCommand::Clear => proc.clear(),
        AnalysisCommand::SetFreqRange { from, to } => {
            proc.fft.set_freq_range(from, to);
        }
    }
}
