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
//! every other player event uses, so the existing Tauri-emit + WS broadcast
//! forwarder picks them up unchanged.

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

  let mut fft_buffer = vec![0.0f32; 2048];
  let emit_interval = Duration::from_millis(50);
  let mut last_emit = Instant::now();

  // AMLL-style low-frequency volume state. The audio-analysis crate's
  // `LowFreqAnalyzer` follows an older AMLL revision (sliding-window
  // gradient + 0.003*delta smoothing) that reacts too slowly; AMLL's
  // current `FFTToLowPassContext`
  // (deps/apoint-amll/packages/player/src/components/LocalMusicContext/index.tsx
  // lines 68-147) is a much simpler threshold-gated boost on a handful of
  // low bins, smoothed at 0.2/frame. Implement that here directly.
  let mut amll_lowfreq_smoothed: f32 = 0.0;

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
      // `process_frame` populates `fft_buffer` (peak-normalized 0-255
      // f32 spectrum). We ignore the analyzer's returned lowFreq value
      // and recompute it with the AMLL-current algorithm below.
      let _ = proc.process_frame(50.0, &mut fft_buffer);

      let amll_lf = amll_low_freq(&fft_buffer, &mut amll_lowfreq_smoothed);

      if evt
        .send(AudioThreadEventMessage::new(
          String::new(),
          Some(AudioThreadEvent::FFTData {
            data: fft_buffer.clone(),
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
            volume: amll_lf as f64,
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

/// Port of AMLL's `FFTToLowPassContext.updateMeter`
/// (deps/apoint-amll/packages/player/src/components/LocalMusicContext/index.tsx
/// L94-L135). Operates on the byte-equivalent peak-normalized spectrum
/// we emit to the frontend, so the visualizer and lowFreq stay in sync.
///
/// - Sum bins `[2..10]` (AMLL's choice — low bass bins; in our config
///   the output spans 80-2000 Hz so these are ~82-90 Hz, the floor of
///   the kick-drum range).
/// - Normalize to `[0, 2.0]` via `/255 * 2.0`.
/// - Threshold gate: > 0.1 → snap to at least 0.4 (loud beat); else 0.
/// - Per-call smoothing at 0.2.
fn amll_low_freq(spectrum: &[f32], smoothed: &mut f32) -> f32 {
  const START: usize = 2;
  const END: usize = 10;

  let end = END.min(spectrum.len());
  if end <= START {
    return *smoothed;
  }
  let count = (end - START) as f32;
  let sum: f32 = spectrum[START..end].iter().sum();
  let average = sum / count;
  let mut target = (average / 255.0) * 2.0;
  if target > 0.1 {
    target = target.max(0.4);
  } else {
    target = 0.0;
  }
  *smoothed += (target - *smoothed) * 0.2;
  *smoothed
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
