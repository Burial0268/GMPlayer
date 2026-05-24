pub mod symphonia;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use rodio::{Decoder, Source};

use crate::analysis::AnalysisCommand;
use crate::error::{AudioError, AudioResult};
use crate::types::AudioInfo;

// ── URL / remote source helpers ──────────────────────────────────

/// Whether this path looks like an HTTP(S) URL that needs to be
/// downloaded before rodio can decode it.
pub fn is_http_url(s: &str) -> bool {
  s.starts_with("http://") || s.starts_with("https://")
}

/// Download `url` to a temporary file. The returned `TempPath` deletes the
/// file when dropped — keep it alive for the lifetime of playback.
pub fn download_to_temp_path(url: &str) -> AudioResult<tempfile::TempPath> {
  let agent = ureq::AgentBuilder::new()
    .timeout_connect(Duration::from_secs(15))
    .timeout_read(Duration::from_secs(60))
    .build();
  let response = agent
    .get(url)
    .call()
    .map_err(|e| AudioError::Download(format!("Request failed: {e}")))?;

  let mut temp = tempfile::Builder::new()
    .prefix("gmplayer-audio-")
    .suffix(".tmp")
    .tempfile()
    .map_err(|e| AudioError::Download(format!("Create temp file: {e}")))?;

  let mut reader = response.into_reader();
  std::io::copy(&mut reader, &mut temp)
    .map_err(|e| AudioError::Download(format!("Write temp file: {e}")))?;

  Ok(temp.into_temp_path())
}

// ── DecoderHandle (placeholder for future FFmpeg-based seeking) ──

/// Handle for controlling an active decoder (seek, etc.).
/// Currently a no-op placeholder — seeking is done by recreating
/// the decoder from scratch. When FFmpeg is added, this will
/// hold a channel to send seek commands to the decode thread.
#[derive(Debug, Clone)]
pub struct DecoderHandle;

/// Open an audio file and return a rodio Source + metadata.
pub fn open_source(
  path: &Path,
) -> AudioResult<(Box<dyn Source<Item = f32> + Send>, AudioInfo)> {
  let info = symphonia::extract_metadata_only(path)?;

  let file = std::io::BufReader::new(std::fs::File::open(path)?);
  let decoder = Decoder::new(file).map_err(|e| AudioError::Decode(e.to_string()))?;
  let source = decoder.convert_samples::<f32>();

  Ok((Box::new(source), info))
}

/// Open a file and wire it through the FFT-feeding source. Returns the
/// source, metadata, control handle, and the sample counter that the
/// source increments as it yields PCM samples (used for sample-accurate
/// position tracking).
pub fn open_source_with_fft(
  path: &Path,
  analysis_tx: mpsc::Sender<AnalysisCommand>,
) -> AudioResult<(
  Box<dyn Source<Item = f32> + Send>,
  AudioInfo,
  DecoderHandle,
  Arc<AtomicU64>,
)> {
  let info = symphonia::extract_metadata_only(path)?;

  let file = std::io::BufReader::new(std::fs::File::open(path)?);
  let decoder = Decoder::new(file).map_err(|e| AudioError::Decode(e.to_string()))?;
  let source = decoder.convert_samples::<f32>();
  let channels = source.channels();
  let sample_rate = source.sample_rate();

  let samples_count = Arc::new(AtomicU64::new(0));
  let source = FFTFeedSource::new(source, analysis_tx, channels, sample_rate, samples_count.clone());

  Ok((Box::new(source), info, DecoderHandle, samples_count))
}

/// FFT-feeding source for seek: skips `seek_pos_secs` worth of samples
/// before producing output, so the FFT receives the post-seek audio only.
pub fn open_source_with_fft_at(
  path: &Path,
  seek_pos_secs: f64,
  analysis_tx: mpsc::Sender<AnalysisCommand>,
) -> AudioResult<(
  Box<dyn Source<Item = f32> + Send>,
  AudioInfo,
  DecoderHandle,
  Arc<AtomicU64>,
)> {
  let info = symphonia::extract_metadata_only(path)?;

  let file = std::io::BufReader::new(std::fs::File::open(path)?);
  let decoder = Decoder::new(file).map_err(|e| AudioError::Decode(e.to_string()))?;
  let source = decoder.convert_samples::<f32>();
  let channels = source.channels();
  let sample_rate = source.sample_rate();

  let skipped = source.skip_duration(Duration::from_secs_f64(seek_pos_secs.max(0.0)));
  let samples_count = Arc::new(AtomicU64::new(0));
  let source = FFTFeedSource::new(skipped, analysis_tx, channels, sample_rate, samples_count.clone());

  Ok((Box::new(source), info, DecoderHandle, samples_count))
}

// ── FFTFeedSource: batched PCM forwarding to the analysis thread ──
//
// `next()` is called from rodio's audio thread, once per (interleaved)
// sample. The previous version held a `parking_lot::RwLock` over an
// `AudioProcessor` and called `try_write` to push batches; we now send
// interleaved PCM via `std::sync::mpsc` to a dedicated analysis OS thread
// (see `crate::analysis::spawn_analysis_thread`). The audio callback thread
// only handles batching and channel sends — no FFT work, no lock contention.
//
// `BATCH_SIZE` is the count of interleaved samples per send. At 44.1 kHz
// stereo that's ~12 ms per batch, ~88 sends/sec.
//
// BUFFER RECYCLING: we keep a pool of `Vec<f32>` allocations alive and
// hand them back and forth with the analysis thread, instead of asking the
// allocator for a fresh 4 KB block on every flush. This is the only
// per-batch allocation in the audio callback path; eliminating it removes
// a glitch source under memory pressure or when Windows trims the app's
// working set (background EcoQoS). The pool is pre-seeded with 4 buffers
// at construction so the first few batches don't need to allocate either.
//
// PRE-ROLL SILENCE: many codecs (notably MP3 with its bit-reservoir) emit
// garbage / underflow samples for the first ~25 ms of any new decode
// (open or seek). We yield clean `0.0` for the first `PRE_ROLL_SILENCE`
// samples after each open / seek so those artifacts never reach the
// speakers. The position counter still ticks, so the position display
// stays in sync with elapsed time.

const BATCH_SIZE: usize = 1024;
/// 2048 samples ≈ 23 ms at 44.1 kHz stereo per-channel — enough for the
/// MP3 bit reservoir to fill, virtually imperceptible to the listener.
const PRE_ROLL_SILENCE: u32 = 2048;
/// Pre-seed count for the recycle channel. ~2 buffers in flight + headroom.
const RECYCLE_PRESEED: usize = 4;

struct FFTFeedSource<S: Source<Item = f32>> {
  inner: S,
  analysis_tx: mpsc::Sender<AnalysisCommand>,
  /// Cloned into every `Pcm` command so the analysis thread can return
  /// the processed buffer here for reuse.
  recycle_tx: mpsc::Sender<Vec<f32>>,
  /// Receiver for recycled buffers. `flush()` pulls from this first.
  recycle_rx: mpsc::Receiver<Vec<f32>>,
  channels: u16,
  sample_rate: u32,
  /// Incremented once per yielded sample. Used by the player's position
  /// task to compute `position = base_time + samples / (rate * channels)`
  /// without wall-clock guesswork — paused sinks don't pull samples, so
  /// the counter naturally freezes.
  samples_count: Arc<AtomicU64>,
  /// Interleaved batch buffer (handed to the analysis thread when full).
  pcm_batch: Vec<f32>,
  /// Samples remaining in the pre-roll silence window after open / seek.
  pre_roll_silence_remaining: u32,
}

impl<S: Source<Item = f32>> FFTFeedSource<S> {
  fn new(
    inner: S,
    analysis_tx: mpsc::Sender<AnalysisCommand>,
    channels: u16,
    sample_rate: u32,
    samples_count: Arc<AtomicU64>,
  ) -> Self {
    let channels = channels.max(1);
    let (recycle_tx, recycle_rx) = mpsc::channel::<Vec<f32>>();
    // Pre-seed the recycle channel so the first few flushes don't allocate.
    // After steady state, the analysis thread feeds the same buffers back.
    for _ in 0..RECYCLE_PRESEED {
      let _ = recycle_tx.send(Vec::with_capacity(BATCH_SIZE));
    }
    Self {
      inner,
      analysis_tx,
      recycle_tx,
      recycle_rx,
      channels,
      sample_rate: sample_rate.max(1),
      samples_count,
      pcm_batch: Vec::with_capacity(BATCH_SIZE),
      pre_roll_silence_remaining: PRE_ROLL_SILENCE,
    }
  }

  /// Swap the current batch for a recycled (or fresh) buffer and ship the
  /// full one off to the analysis thread. The recycle channel is drained
  /// first so any backlog of returned buffers is absorbed before falling
  /// back to a fresh `Vec::with_capacity`.
  fn flush(&mut self) {
    if self.pcm_batch.is_empty() {
      return;
    }
    let mut next = match self.recycle_rx.try_recv() {
      Ok(v) => v,
      Err(_) => Vec::with_capacity(BATCH_SIZE),
    };
    next.clear();
    if next.capacity() < BATCH_SIZE {
      next.reserve(BATCH_SIZE - next.capacity());
    }
    let batch = std::mem::replace(&mut self.pcm_batch, next);
    let _ = self.analysis_tx.send(AnalysisCommand::Pcm {
      samples: batch,
      channels: self.channels,
      sample_rate: self.sample_rate,
      recycle: Some(self.recycle_tx.clone()),
    });
  }
}

impl<S: Source<Item = f32>> Iterator for FFTFeedSource<S> {
  type Item = f32;

  fn next(&mut self) -> Option<Self::Item> {
    if self.pre_roll_silence_remaining > 0 {
      self.pre_roll_silence_remaining -= 1;
      // Advance the decoder so we don't desync the stream, but yield
      // clean silence instead of the codec's underflow garbage. Don't
      // feed the analyzer either — the silence ramps in naturally.
      let _ = self.inner.next()?;
      self.samples_count.fetch_add(1, Ordering::Relaxed);
      return Some(0.0);
    }

    let sample = self.inner.next()?;
    self.samples_count.fetch_add(1, Ordering::Relaxed);
    self.pcm_batch.push(sample);
    if self.pcm_batch.len() >= BATCH_SIZE {
      self.flush();
    }
    Some(sample)
  }
}

impl<S: Source<Item = f32>> Source for FFTFeedSource<S> {
  fn current_frame_len(&self) -> Option<usize> {
    self.inner.current_frame_len()
  }

  fn channels(&self) -> u16 {
    self.inner.channels()
  }

  fn sample_rate(&self) -> u32 {
    self.inner.sample_rate()
  }

  fn total_duration(&self) -> Option<Duration> {
    self.inner.total_duration()
  }

  /// Forward in-place seek to the underlying Symphonia-backed decoder,
  /// drop any samples we'd batched pre-seek (so they don't leak into the
  /// FFT visualization), clear the analyzer queue, reset the sample
  /// counter, and re-arm the pre-roll-silence window so codec underflow
  /// after the seek is masked.
  fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
    self.pcm_batch.clear();
    let result = self.inner.try_seek(pos);
    if result.is_ok() {
      let _ = self.analysis_tx.send(AnalysisCommand::Clear);
      self.samples_count.store(0, Ordering::SeqCst);
      self.pre_roll_silence_remaining = PRE_ROLL_SILENCE;
    }
    result
  }
}
