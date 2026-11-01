mod convert;
mod resample;
pub mod symphonia;

use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ::symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
use ::symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use ::symphonia::core::errors::Error as SymphoniaError;
use ::symphonia::core::formats::{FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo};
use ::symphonia::core::io::MediaSourceStream;
use ::symphonia::core::meta::MetadataOptions;
use ::symphonia::core::probe::Hint;
use ::symphonia::core::units::Time;
use rodio::source::SeekError;
use rodio::Source;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::warn;

use crate::analysis::{AnalysisCommand, AnalysisPcm, AnalysisSender};
use crate::error::{AudioError, AudioResult};
use crate::output::{OutputWriter, PushCancel};

use convert::FrameConverter;

pub(crate) trait PlaybackSink: Clone + Send + 'static {
    fn push_block(&self, block: Vec<f32>, cancel: PushCancel<'_>) -> bool;
    fn flush_for_seek(&self);
    fn set_paused(&self, paused: bool);
    fn queued_samples(&self) -> usize;
    /// Device delay past `queued_samples`; `0` until measured.
    fn output_latency_us(&self) -> u32;
    fn generation(&self) -> u64;

    fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        Vec::with_capacity(capacity)
    }
}

impl PlaybackSink for OutputWriter {
    fn push_block(&self, block: Vec<f32>, cancel: PushCancel<'_>) -> bool {
        OutputWriter::push_block(self, block, cancel)
    }

    fn flush_for_seek(&self) {
        OutputWriter::flush(self);
    }

    fn set_paused(&self, paused: bool) {
        OutputWriter::set_paused(self, paused);
    }

    fn queued_samples(&self) -> usize {
        OutputWriter::queued_samples(self)
    }

    fn output_latency_us(&self) -> u32 {
        OutputWriter::output_latency_us(self)
    }

    fn generation(&self) -> u64 {
        OutputWriter::generation(self)
    }

    fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        OutputWriter::take_recycled_buffer(self, capacity)
    }
}

// ── URL / remote source helpers ──────────────────────────────────

/// Whether this path looks like an HTTP(S) URL that needs to be
/// downloaded before rodio can decode it.
pub fn is_http_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Bodies under this are downloaded whole before returning: the handshake for a
/// background writer costs more than it saves on a file that lands in a moment.
const STREAM_MIN_BYTES: u64 = 2 * 1024 * 1024;

/// Download `url` to a temporary file. The returned guard deletes the file when
/// dropped — keep it alive for the lifetime of playback.
///
/// When the server declares a `Content-Length` of at least `STREAM_MIN_BYTES`
/// this returns as soon as the response headers are in, with the body still
/// landing on a writer thread. The path is registered with `source`, so every
/// `source::open` of it waits for bytes rather than seeing EOF; dropping the
/// guard stops the writer. Anything else is downloaded whole, as it always was.
///
/// The agent is built per call, and must stay that way. Sharing one — i.e.
/// pooling the connection between track downloads — saves a TLS handshake per
/// track and was tried; the next track then decoded as the *previous* track's
/// length plus its own, which is what a reused connection handing over an
/// undrained body looks like. A whole audio body is the one response here, so
/// any framing slip corrupts the file rather than erroring, and the file is the
/// thing playback is built on. One handshake per track, on a path the planner
/// already runs a track ahead, is not worth that. Streaming keeps that
/// guarantee by reading exactly `Content-Length` bytes and failing on a short
/// body, never by trusting the connection to be at a boundary.
pub fn download_to_temp_path(url: &str) -> AudioResult<crate::source::TempGuard> {
    use crate::source::growing::{self, Progress};
    use std::io::Read;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| AudioError::Download(format!("Request failed: {e}")))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= STREAM_MIN_BYTES);

    let mut temp = tempfile::Builder::new()
        .prefix("gmplayer-audio-")
        .suffix(".tmp")
        .tempfile()
        .map_err(|e| AudioError::Download(format!("Create temp file: {e}")))?;

    let mut reader = response.into_reader();
    let Some(total) = total else {
        std::io::copy(&mut reader, &mut temp)
            .map_err(|e| AudioError::Download(format!("Write temp file: {e}")))?;
        return Ok(temp.into_temp_path().into());
    };

    let (file, path) = temp.into_parts();
    let progress = Progress::new(total);
    growing::register(&path, progress.clone());

    // A forward seek far past the head writer starts a second one at the target.
    let (jump_url, jump_path) = (url.to_string(), path.to_path_buf());
    progress.set_jumper(Box::new(move |progress, id, start| {
        let (url, path) = (jump_url.clone(), jump_path.clone());
        let worker = Arc::clone(&progress);
        let spawned = std::thread::Builder::new().name("audio-download-range".into()).spawn(move || {
            let outcome = open_range(&url, start, worker.total()).and_then(|body| {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .map_err(|e| format!("Open temp file: {e}"))?;
                growing::pump(&worker, id, file, body)
            });
            worker.end_writer(id, outcome);
        });
        if let Err(e) = spawned {
            progress.end_writer(id, Err(format!("Start range thread: {e}")));
        }
    }));

    let guard = crate::source::TempGuard::streaming(path, progress.clone());
    let head = progress.begin_writer(0);
    let writer = progress.clone();
    let spawned = std::thread::Builder::new().name("audio-download".into()).spawn(move || {
        let outcome = growing::pump(&writer, head, file, reader.take(total));
        writer.end_writer(head, outcome);
    });
    spawned.map_err(|e| AudioError::Download(format!("Start download thread: {e}")))?;

    Ok(guard)
}

/// Open `bytes=start-` of `url`. A server that answers anything but a matching
/// `206` is refused outright: the bytes would be appended at the offset we
/// asked for regardless of where they came from, which is a corrupt track
/// rather than a failed request. Per call, like the main download, for the same
/// reason.
fn open_range(url: &str, start: u64, total: u64) -> Result<Box<dyn std::io::Read + Send>, String> {
    use std::io::Read;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .set("Range", &format!("bytes={start}-"))
        .call()
        .map_err(|e| format!("Range request failed: {e}"))?;
    if response.status() != 206 {
        return Err(format!("server answered {} to a Range request", response.status()));
    }
    // "bytes <start>-<end>/<total>"
    let range = response.header("Content-Range").unwrap_or_default();
    let matches = range
        .strip_prefix("bytes ")
        .and_then(|r| r.split_once('/'))
        .and_then(|(span, t)| Some((span.split_once('-')?.0.parse::<u64>().ok()?, t.parse::<u64>().ok()?)))
        .is_some_and(|(s, t)| s == start && t == total);
    if !matches {
        return Err(format!("Content-Range {range:?} does not start at {start} of {total}"));
    }
    Ok(Box::new(response.into_reader().take(total - start)))
}

// ── DecoderHandle ────────────────────────────────────────────────

enum DecoderControl {
    Seek {
        pos: Duration,
        epoch: u64,
        ack: oneshot::Sender<Result<(), String>>,
    },
    SetPaused(bool),
    Stop,
}

/// Handle for controlling the active source. This mirrors AMLL's decoder
/// handle: the player sends seek intent directly to the decoder/source layer
/// instead of waiting for `rodio::Sink::try_seek()` to round-trip through
/// rodio's periodic access callback.
pub struct DecoderHandle {
    control_tx: mpsc::Sender<DecoderControl>,
    stop_flag: Arc<AtomicBool>,
    seek_epoch: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DecoderHandle {
    /// Interrupt a decode thread parked on a full sink queue. Producers park
    /// for about the time the consumer needs to free a slot, which is far
    /// longer than a control message should take to land, so every control
    /// path pokes the thread instead of waiting for that park to expire.
    fn wake(&self) {
        if let Some(thread) = self.thread.as_ref() {
            thread.thread().unpark();
        }
    }

    pub(crate) fn seek(&self, pos: Duration) -> AudioResult<DecoderSeekAck> {
        let (ack_tx, ack_rx) = oneshot::channel();
        let epoch = self
            .seek_epoch
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        self.control_tx
            .send(DecoderControl::Seek {
                pos,
                epoch,
                ack: ack_tx,
            })
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))?;
        self.wake();

        Ok(DecoderSeekAck { ack_rx })
    }

    pub fn set_paused(&self, paused: bool) -> AudioResult<()> {
        self.control_tx
            .send(DecoderControl::SetPaused(paused))
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))?;
        self.wake();
        Ok(())
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        self.seek_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.control_tx.send(DecoderControl::Stop);
        self.wake();
    }
}

impl Drop for DecoderHandle {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            join_decoder_thread_async(thread);
        }
    }
}

pub(crate) struct DecoderSeekAck {
    ack_rx: oneshot::Receiver<Result<(), String>>,
}

impl DecoderSeekAck {
    pub(crate) async fn wait(self) -> AudioResult<()> {
        match tokio::time::timeout(Duration::from_secs(2), self.ack_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(err))) => Err(AudioError::Decode(err)),
            Ok(Err(_)) => Err(AudioError::ThreadError(
                "decoder seek acknowledgement channel closed".into(),
            )),
            Err(_) => Err(AudioError::ThreadError("decoder seek timed out".into())),
        }
    }
}

fn join_decoder_thread_async(handle: std::thread::JoinHandle<()>) {
    let _ = std::thread::Builder::new()
        .name("audio-decode-join".into())
        .spawn(move || {
            let _ = handle.join();
        });
}

#[derive(Debug, Clone, Copy)]
pub enum DecoderEvent {
    Finished { playback_id: u64 },
    /// The decoder ran ahead of a download still landing and is holding for it
    /// (`starved`), or has enough again. Only streamed sources emit it.
    Starved { playback_id: u64, starved: bool },
}

const DECODE_BLOCK_FRAMES: usize = 512;
const PRE_ROLL_SILENCE_FRAMES: usize = 1024;
const START_RAMP_FRAMES: usize = 2048;
const SEEK_RAMP_FRAMES: usize = 512;
const SEEK_CONTROL_CHECK_FRAMES: usize = 64;
const MAX_DECODE_RETRIES: usize = 3;
// A paused decoder resumes, seeks and stops through the control channel, which
// wakes this wait immediately. The timeout is only a backstop for `stop_flag`
// being set without a message, so it costs nothing to keep it low-frequency.
const PAUSED_POLL_INTERVAL: Duration = Duration::from_millis(200);

struct SeekableSymphoniaSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn ::symphonia::core::codecs::Decoder>,
    track_id: u32,
    spec: SignalSpec,
    total_duration: Option<Time>,
    buffer: SampleBuffer<f32>,
    buffer_offset: usize,
}

impl SeekableSymphoniaSource {
    fn open(path: &Path) -> AudioResult<Self> {
        let opened = crate::source::open_playback(crate::source::SourceLocator::from_path(path))?;
        let mut hint = Hint::new();
        // symphonia 0.5's `Probe::format` takes the hint as `_hint` and never
        // reads it — format detection is a pure magic-byte scan, so a container
        // is identified correctly no matter what the file is named (or that
        // downloaded sources land on a `.tmp` path). Kept because it costs
        // nothing and later symphonia releases do consult it.
        if let Some(ext) = opened.extension() {
            hint.with_extension(ext);
        }
        let mss = MediaSourceStream::new(Box::new(opened), Default::default());

        let format_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let probed = ::symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &MetadataOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;
        let mut format = probed.format;

        let track = format
            .default_track()
            .or_else(|| {
                format
                    .tracks()
                    .iter()
                    .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            })
            .ok_or_else(|| AudioError::Decode("no supported audio track".into()))?
            .clone();
        let track_id = track.id;
        let total_duration = track
            .codec_params
            .time_base
            .zip(track.codec_params.n_frames)
            .map(|(base, frames)| base.calc_time(frames));
        let mut decoder = ::symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let (spec, buffer) = decode_next_audio_buffer(
            format.as_mut(),
            decoder.as_mut(),
            track_id,
            sample_buffer_from_decoded,
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?
        .ok_or_else(|| AudioError::Decode("no decodable audio samples".into()))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            spec,
            total_duration,
            buffer,
            buffer_offset: 0,
        })
    }

    fn refill(&mut self) -> Option<()> {
        let buffer = &mut self.buffer;
        let spec = match decode_next_audio_buffer(
            self.format.as_mut(),
            self.decoder.as_mut(),
            self.track_id,
            |decoded| copy_decoded_into_sample_buffer(decoded, buffer),
        ) {
            Ok(Some(spec)) => spec,
            Ok(None) => return None,
            Err(err) => {
                warn!("decode failed while refilling playback buffer: {err}");
                return None;
            }
        };

        // The channel count and sample rate were sampled once, when the decoder
        // was opened, and are baked into `FrameConverter`'s mix matrix, its
        // resampler and the bulk-passthrough stride. A mid-stream change would
        // therefore be reinterpreted at the wrong stride and come out as noise,
        // so end the track instead. Symphonia's MP3 and FLAC decoders reject
        // such a change themselves, but container-level readers do not all.
        if spec.channels.count() != self.spec.channels.count() || spec.rate != self.spec.rate {
            warn!(
                "音轨中途改变音频规格 ({} ch @ {} Hz -> {} ch @ {} Hz)，停止解码",
                self.spec.channels.count(),
                self.spec.rate,
                spec.channels.count(),
                spec.rate
            );
            return None;
        }

        self.spec = spec;
        self.buffer_offset = 0;
        Some(())
    }

    /// Decode forward from the seek landing point to the exact requested
    /// position, discarding the output produced along the way.
    ///
    /// `format.seek()` deliberately lands *before* the requested timestamp.
    /// MPEG Layer III granules reference up to 511 bytes of "main data" carried
    /// by earlier frames — the bit reservoir — so symphonia's demuxer walks back
    /// far enough that those references resolve and reports the rewound position
    /// as `actual_ts`. Those run-up packets have to be fed through the decoder:
    /// skipping them undoes the rewind and leaves the first audible frame
    /// pointing at a reservoir that was never filled, which symphonia reports as
    /// `invalid main_data_begin, underflow by N bytes` and renders as a dropped,
    /// silent granule. Only the run-up's *samples* are thrown away.
    fn refine_position(&mut self, seeked: SeekedTo) -> Result<(), SymphoniaError> {
        let mut frames_to_skip = seeked.required_ts.saturating_sub(seeked.actual_ts);
        loop {
            let packet = self.next_track_packet()?;
            let duration = packet.dur();

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // A run-up packet that will not decode leaves the reservoir cold
                // for one more frame, but it must not abort the seek. Note the
                // variant: Layer III reports a granule whose `main_data_begin`
                // points past what the reservoir actually holds as an `IoError`
                // from the bit reader running off the end of the packet buffer,
                // *not* as a `DecodeError` — and that is exactly the packet a
                // run-up is most likely to land on, since the run-up exists
                // because the reservoir starts cold. Both are therefore
                // tolerated here; `ResetRequired` and the fatal variants still
                // propagate. `decode()` reads only from the already-demuxed
                // packet, so this cannot swallow a failure of the file itself —
                // that surfaces from `next_track_packet` above.
                Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_))
                    if duration <= frames_to_skip =>
                {
                    frames_to_skip -= duration;
                    continue;
                }
                Err(err) => return Err(err),
            };

            if duration <= frames_to_skip {
                frames_to_skip -= duration;
                continue;
            }

            self.spec = copy_decoded_into_sample_buffer(decoded, &mut self.buffer);
            self.buffer_offset = (frames_to_skip as usize)
                .saturating_mul(self.channels().get() as usize)
                .min(self.buffer.len());
            return Ok(());
        }
    }

    fn next_track_packet(&mut self) -> Result<Packet, SymphoniaError> {
        loop {
            let packet = self.format.next_packet()?;
            if packet.track_id() == self.track_id {
                return Ok(packet);
            }
        }
    }

    /// Append up to `max_frames` complete interleaved frames from the decoded
    /// sample buffer. This is the steady-state passthrough hot path: copying a
    /// contiguous slice avoids one Iterator call and bounds check per sample.
    ///
    /// `channels` is fixed for the life of the source — `refill` ends the track
    /// rather than let a mid-stream spec change reach this stride.
    fn append_interleaved_frames(
        &mut self,
        dst: &mut Vec<f32>,
        channels: usize,
        max_frames: usize,
    ) -> usize {
        debug_assert_eq!(channels, self.channels().get() as usize);
        let mut copied_frames = 0;

        while copied_frames < max_frames {
            if self.buffer_offset >= self.buffer.len() && self.refill().is_none() {
                break;
            }

            let copied = append_available_interleaved_frames(
                self.buffer.samples(),
                &mut self.buffer_offset,
                dst,
                channels,
                max_frames - copied_frames,
            );
            if copied == 0 {
                // Symphonia buffers should be frame-aligned. If a malformed
                // packet leaves a partial frame, discard it before refilling.
                self.buffer_offset = self.buffer.len();
                continue;
            }
            copied_frames += copied;
        }

        copied_frames
    }
}

fn append_available_interleaved_frames(
    samples: &[f32],
    offset: &mut usize,
    dst: &mut Vec<f32>,
    channels: usize,
    max_frames: usize,
) -> usize {
    debug_assert!(channels > 0);
    let available_samples = samples.len().saturating_sub(*offset);
    let frames = (available_samples / channels).min(max_frames);
    let sample_count = frames * channels;
    let end = *offset + sample_count;
    dst.extend_from_slice(&samples[*offset..end]);
    *offset = end;
    frames
}

impl Iterator for SeekableSymphoniaSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer_offset >= self.buffer.len() {
            self.refill()?;
        }
        let sample = *self.buffer.samples().get(self.buffer_offset)?;
        self.buffer_offset += 1;
        Some(sample)
    }
}

impl Source for SeekableSymphoniaSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.buffer_offset))
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(self.spec.channels.count().max(1) as u16).unwrap()
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(self.spec.rate.max(1)).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration.map(|Time { seconds, frac }| {
            Duration::new(
                seconds,
                (frac.clamp(0.0, 0.999_999_999) * 1_000_000_000.0) as u32,
            )
        })
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let target = if self
            .total_duration()
            .is_some_and(|duration| duration.saturating_sub(pos).as_millis() < 1)
        {
            self.total_duration
                .map(skip_back_a_tiny_bit)
                .unwrap_or_else(|| pos.as_secs_f64().into())
        } else {
            pos.as_secs_f64().into()
        };

        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: target,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(to_seek_error)?;

        self.decoder.reset();
        self.refine_position(seeked).map_err(to_seek_error)?;
        Ok(())
    }
}

fn decode_next_audio_buffer<T>(
    format: &mut dyn FormatReader,
    decoder: &mut dyn ::symphonia::core::codecs::Decoder,
    track_id: u32,
    mut consume: impl FnMut(AudioBufferRef<'_>) -> T,
) -> Result<Option<T>, SymphoniaError> {
    let mut decode_errors = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Symphonia signals a clean end of stream as an `UnexpectedEof` IO
            // error. Anything else is a genuine read failure — a vanished file,
            // a truncated download — and must not be reported to the caller as
            // a track that simply finished.
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None)
            }
            Err(err) => return Err(err),
        };
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => return Ok(Some(consume(decoded))),
            Err(SymphoniaError::DecodeError(_)) => {
                decode_errors += 1;
                if decode_errors > MAX_DECODE_RETRIES {
                    return Err(SymphoniaError::DecodeError("too many decode errors"));
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn sample_buffer_from_decoded(decoded: AudioBufferRef<'_>) -> (SignalSpec, SampleBuffer<f32>) {
    let spec = *decoded.spec();
    let mut sample_buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
    sample_buffer.copy_interleaved_ref(decoded);
    (spec, sample_buffer)
}

fn copy_decoded_into_sample_buffer(
    decoded: AudioBufferRef<'_>,
    sample_buffer: &mut SampleBuffer<f32>,
) -> SignalSpec {
    let spec = *decoded.spec();
    let required_samples = decoded.frames().saturating_mul(spec.channels.count());
    if sample_buffer.capacity() < required_samples {
        *sample_buffer = SampleBuffer::new(decoded.capacity() as u64, spec);
    }
    sample_buffer.copy_interleaved_ref(decoded);
    spec
}

fn to_seek_error(err: SymphoniaError) -> SeekError {
    SeekError::Other(Arc::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        err.to_string(),
    )))
}

fn skip_back_a_tiny_bit(
    Time {
        mut seconds,
        mut frac,
    }: Time,
) -> Time {
    frac -= 0.0001;
    if frac < 0.0 {
        seconds = seconds.saturating_sub(1);
        frac = 1.0 + frac;
    }
    Time { seconds, frac }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_playback_decoder<S>(
    path: &Path,
    initial_position: Option<f64>,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: AnalysisSender,
    analysis_enabled: Arc<AtomicBool>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    start_paused: bool,
) -> AudioResult<DecoderHandle>
where
    S: PlaybackSink,
{
    let mut source = SeekableSymphoniaSource::open(path)?;
    let input_channels = source.channels().get();
    let input_sample_rate = source.sample_rate().get();

    if let Some(position) = initial_position.filter(|position| *position > 0.0) {
        source
            .try_seek(Duration::from_secs_f64(position))
            .map_err(|e| AudioError::Decode(format!("initial seek failed: {e}")))?;
    }

    let gate = crate::source::growing::lookup(path)
        .map(|progress| StreamGate::new(progress, rodio::Source::total_duration(&source)));

    let (control_tx, control_rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let seek_epoch = Arc::new(AtomicU64::new(0));
    let worker_stop = Arc::clone(&stop_flag);
    let worker_seek_epoch = Arc::clone(&seek_epoch);
    let output_channels = output_channels.max(1);
    let output_sample_rate = output_sample_rate.max(1);

    let thread = std::thread::Builder::new()
        .name("audio-decode".into())
        .spawn(move || {
            // Held for the thread's lifetime: this thread is the only producer
            // for the deck queue, so its scheduling latency is playback's. It
            // renders one DECODE_BLOCK_FRAMES block per wake, which is the
            // period the deadline-based backends want to know about.
            let _priority = crate::rt_priority::promote_current_thread(
                crate::rt_priority::AudioThreadKind::Decode,
                DECODE_BLOCK_FRAMES,
                output_sample_rate,
            );
            let mut worker = DecodeWorker::new(
                source,
                input_channels,
                input_sample_rate,
                output,
                output_channels,
                output_sample_rate,
                analysis_tx,
                analysis_enabled,
                control_rx,
                event_tx,
                playback_id,
                worker_stop,
                worker_seek_epoch,
                start_paused,
                gate,
            );
            worker.run();
        })
        .map_err(|e| AudioError::ThreadError(format!("spawn audio-decode thread: {e}")))?;

    Ok(DecoderHandle {
        control_tx,
        stop_flag,
        seek_epoch,
        thread: Some(thread),
    })
}

struct DecodeWorker<S: PlaybackSink> {
    frames: FrameConverter,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: AnalysisSender,
    analysis_enabled: Arc<AtomicBool>,
    analysis_recycle_tx: mpsc::Sender<Vec<f32>>,
    analysis_recycle_rx: mpsc::Receiver<Vec<f32>>,
    control_rx: mpsc::Receiver<DecoderControl>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    stop_flag: Arc<AtomicBool>,
    seek_epoch: Arc<AtomicU64>,
    paused: bool,
    pre_roll_frames_remaining: usize,
    start_ramp_total_frames: usize,
    start_ramp_frames_remaining: usize,
    applied_seek_epoch: u64,
    pending_seek_flush_epoch: Option<u64>,
    gate: Option<StreamGate>,
}

/// Watermarks for a source that is still downloading. The decoder stops
/// producing when the bytes ahead of it fall below `low` and resumes at `high`;
/// the gap is hysteresis, so a link running just under the bitrate stalls in
/// long rebuffers instead of flapping every block.
struct StreamGate {
    progress: Arc<crate::source::growing::Progress>,
    low: u64,
    high: u64,
    starved: bool,
}

const GATE_LOW_SECONDS: f64 = 1.0;
const GATE_HIGH_SECONDS: f64 = 3.0;
/// Bytes per second assumed when the container declares no duration.
const GATE_FALLBACK_BYTES_PER_SEC: f64 = 128.0 * 1024.0;
const GATE_POLL: Duration = Duration::from_millis(100);

impl StreamGate {
    fn new(progress: Arc<crate::source::growing::Progress>, duration: Option<Duration>) -> Self {
        let rate = duration
            .map(|d| d.as_secs_f64())
            .filter(|d| *d > 0.0)
            .map_or(GATE_FALLBACK_BYTES_PER_SEC, |d| progress.total() as f64 / d);
        let low = ((rate * GATE_LOW_SECONDS) as u64).max(16 * 1024);
        let high = ((rate * GATE_HIGH_SECONDS) as u64).max(low * 2);
        Self { progress, low, high, starved: false }
    }

    /// Whether the decoder must hold now. A finished download never holds, which
    /// is also what releases the tail: less than `high` may be left to arrive.
    fn should_hold(&self) -> bool {
        if self.progress.is_finished() || self.progress.reaches_end() {
            return false;
        }
        self.progress.lead() < if self.starved { self.high } else { self.low }
    }
}

/// Frames of pre-roll silence to emit in one run of the decode loop.
///
/// Must never return zero while `remaining > 0`: the loop `continue`s on this
/// value without consuming a source frame, so a zero-length run would spin the
/// decode thread forever. It also must not run past the next control-check
/// boundary (seek latency) or the end of the block (reserved capacity).
#[inline]
fn pre_roll_run_frames(remaining: usize, frame_index: usize) -> usize {
    debug_assert!(remaining > 0);
    debug_assert!(frame_index < DECODE_BLOCK_FRAMES);
    let until_control_check = SEEK_CONTROL_CHECK_FRAMES - (frame_index % SEEK_CONTROL_CHECK_FRAMES);
    remaining
        .min(until_control_check)
        .min(DECODE_BLOCK_FRAMES - frame_index)
}

#[allow(clippy::too_many_arguments)]
impl<S: PlaybackSink> DecodeWorker<S> {
    fn new(
        source: SeekableSymphoniaSource,
        input_channels: u16,
        input_sample_rate: u32,
        output: S,
        output_channels: u16,
        output_sample_rate: u32,
        analysis_tx: AnalysisSender,
        analysis_enabled: Arc<AtomicBool>,
        control_rx: mpsc::Receiver<DecoderControl>,
        event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
        playback_id: u64,
        stop_flag: Arc<AtomicBool>,
        seek_epoch: Arc<AtomicU64>,
        paused: bool,
        gate: Option<StreamGate>,
    ) -> Self {
        let (analysis_recycle_tx, analysis_recycle_rx) = mpsc::channel();
        Self {
            frames: FrameConverter::new(
                source,
                input_channels,
                input_sample_rate,
                output_channels,
                output_sample_rate,
            ),
            output,
            output_channels,
            output_sample_rate,
            analysis_tx,
            analysis_enabled,
            analysis_recycle_tx,
            analysis_recycle_rx,
            control_rx,
            event_tx,
            playback_id,
            stop_flag,
            seek_epoch,
            paused,
            pre_roll_frames_remaining: PRE_ROLL_SILENCE_FRAMES,
            start_ramp_total_frames: START_RAMP_FRAMES,
            start_ramp_frames_remaining: START_RAMP_FRAMES,
            applied_seek_epoch: 0,
            pending_seek_flush_epoch: None,
            gate,
        }
    }

    fn run(&mut self) {
        let mut frame = vec![0.0f32; self.output_channels as usize];

        loop {
            if self.stop_flag.load(Ordering::Acquire) || !self.drain_controls() {
                break;
            }

            if self.paused {
                if !self.wait_while_paused() {
                    break;
                }
                continue;
            }

            if self.seek_epoch.load(Ordering::Acquire) != self.applied_seek_epoch {
                std::thread::yield_now();
                continue;
            }

            if !self.hold_for_stream() {
                break;
            }
            if self.paused || self.seek_epoch.load(Ordering::Acquire) != self.applied_seek_epoch {
                continue;
            }

            let block_seek_epoch = self.applied_seek_epoch;
            let generation = self.output.generation();
            let block_capacity = DECODE_BLOCK_FRAMES * self.output_channels as usize;
            let mut block = self.output.take_recycled_buffer(block_capacity);
            block.clear();
            if block.capacity() < block_capacity {
                block.reserve(block_capacity - block.capacity());
            }
            let mut analysis_block = if self.analysis_enabled.load(Ordering::Acquire) {
                Some(
                    self.analysis_recycle_rx
                        .try_recv()
                        .unwrap_or_else(|_| Vec::with_capacity(block_capacity)),
                )
            } else {
                None
            };
            if let Some(block) = analysis_block.as_mut() {
                block.clear();
            }
            let mut ended = false;
            let mut superseded = false;

            let mut frame_index = 0;
            while frame_index < DECODE_BLOCK_FRAMES {
                if frame_index % SEEK_CONTROL_CHECK_FRAMES == 0
                    && self.seek_epoch.load(Ordering::Acquire) != block_seek_epoch
                {
                    superseded = true;
                    break;
                }

                // Prime the sink with silence before any audio is emitted. This
                // must not pull from the source: doing so discarded the frame it
                // decoded, which truncated the first PRE_ROLL_SILENCE_FRAMES
                // (~21 ms at 48 kHz) of every track. Emitting the run in one
                // resize keeps the reserved capacity intact and costs no source
                // frames, so the pre-roll now only delays audio, never drops it.
                // Clamped to the next control-check boundary so seek latency is
                // unchanged.
                if self.pre_roll_frames_remaining > 0 {
                    let frames =
                        pre_roll_run_frames(self.pre_roll_frames_remaining, frame_index);
                    block.resize(block.len() + frames * self.output_channels as usize, 0.0);
                    self.pre_roll_frames_remaining -= frames;
                    frame_index += frames;
                    continue;
                }

                // The common file/device layout needs no channel mixing. Once
                // startup/seek shaping is complete, append whole runs of output
                // frames — copied straight through when the rates match,
                // produced by the resampler when they do not — instead of
                // pulling them one at a time through next_frame(). Stop each
                // chunk at the next control-check boundary so seek latency is
                // unchanged.
                if self.start_ramp_frames_remaining == 0 && self.frames.can_bulk_append() {
                    let until_control_check =
                        SEEK_CONTROL_CHECK_FRAMES - (frame_index % SEEK_CONTROL_CHECK_FRAMES);
                    let requested = until_control_check.min(DECODE_BLOCK_FRAMES - frame_index);
                    let block_start = block.len();
                    let copied = self.frames.append_frames(&mut block, requested);
                    if copied == 0 {
                        ended = true;
                        break;
                    }
                    if let Some(analysis_block) = analysis_block.as_mut() {
                        analysis_block.extend_from_slice(&block[block_start..]);
                    }
                    frame_index += copied;
                    if copied < requested {
                        ended = true;
                        break;
                    }
                    continue;
                }

                if self.frames.next_frame(&mut frame).is_none() {
                    ended = true;
                    break;
                }

                if self.start_ramp_frames_remaining > 0 {
                    let ramp_total = self.start_ramp_total_frames.max(1);
                    let elapsed = ramp_total.saturating_sub(self.start_ramp_frames_remaining);
                    let t = (elapsed as f32 / ramp_total as f32).clamp(0.0, 1.0);
                    let gain = t * t * (3.0 - 2.0 * t);
                    for sample in &mut frame {
                        *sample *= gain;
                    }
                    self.start_ramp_frames_remaining -= 1;
                }
                block.extend_from_slice(&frame);
                if let Some(block) = analysis_block.as_mut() {
                    block.extend_from_slice(&frame);
                }
                frame_index += 1;
            }

            if superseded || self.seek_epoch.load(Ordering::Acquire) != block_seek_epoch {
                continue;
            }

            if self.pending_seek_flush_epoch == Some(block_seek_epoch)
                && (!block.is_empty() || ended)
            {
                self.output.flush_for_seek();
                self.pending_seek_flush_epoch = None;
            }

            let pushed = block.is_empty()
                || self.output.push_block(
                    block,
                    PushCancel::with_interrupt_epoch(
                        self.stop_flag.as_ref(),
                        self.seek_epoch.as_ref(),
                        block_seek_epoch,
                    ),
                );
            let generation_current = self.output.generation() == generation;

            if pushed && generation_current && self.analysis_enabled.load(Ordering::Acquire) {
                if let Some(analysis_block) = analysis_block.take() {
                    if !analysis_block.is_empty() {
                        let pcm = AnalysisPcm {
                            samples: analysis_block,
                            channels: self.output_channels,
                            sample_rate: self.output_sample_rate,
                            // Everything queued ahead of the block just pushed
                            // plays first, then the device adds its own delay
                            // (the one lyrics already compensate for); analysis
                            // holds the block back that long.
                            audible_at: Instant::now()
                                + Duration::from_secs_f64(
                                    self.output.queued_samples() as f64
                                        / (self.output_sample_rate.max(1) as f64
                                            * self.output_channels.max(1) as f64),
                                )
                                + Duration::from_micros(
                                    // Past 2 s it is a broken timestamp, as in the
                                    // frontend's `MAX_PLAUSIBLE_OUTPUT_LATENCY_SECONDS`.
                                    (self.output.output_latency_us() as u64).min(2_000_000),
                                ),
                            recycle: Some(self.analysis_recycle_tx.clone()),
                        };
                        if let Err(err) = self.analysis_tx.try_send_pcm(pcm) {
                            // Analysis is best-effort. A full queue must never
                            // slow decode; recover this allocation immediately.
                            let mut samples = match err {
                                mpsc::TrySendError::Full(pcm)
                                | mpsc::TrySendError::Disconnected(pcm) => pcm.samples,
                            };
                            samples.clear();
                            let _ = self.analysis_recycle_tx.send(samples);
                        }
                    }
                }
            }

            if ended {
                if self.stop_flag.load(Ordering::Acquire) {
                    break;
                }
                if pushed && generation_current {
                    self.finish_after_drain(generation);
                    break;
                }
            }
        }
    }

    /// Hold while the download is too far behind to decode without waiting,
    /// reporting the edges so the player can freeze its clock and show a
    /// rebuffer instead of playing silence. Controls keep being served, so a
    /// pause, seek or stop does not wait for the network. `false` means stop.
    fn hold_for_stream(&mut self) -> bool {
        let Some(gate) = self.gate.as_mut() else {
            return true;
        };
        if !gate.should_hold() {
            if gate.starved {
                gate.starved = false;
                let _ = self.event_tx.send(DecoderEvent::Starved {
                    playback_id: self.playback_id,
                    starved: false,
                });
            }
            return true;
        }
        if !gate.starved {
            gate.starved = true;
            let _ = self.event_tx.send(DecoderEvent::Starved {
                playback_id: self.playback_id,
                starved: true,
            });
        }
        let progress = Arc::clone(&gate.progress);
        loop {
            if self.stop_flag.load(Ordering::Acquire) || !self.drain_controls() {
                return false;
            }
            if self.paused
                || self.seek_epoch.load(Ordering::Acquire) != self.applied_seek_epoch
            {
                // Let the main loop serve it; the gate re-evaluates on return.
                return true;
            }
            let Some(gate) = self.gate.as_mut() else {
                return true;
            };
            if !gate.should_hold() {
                return true;
            }
            progress.wait_for_change(GATE_POLL);
        }
    }

    fn wait_while_paused(&mut self) -> bool {
        loop {
            if self.stop_flag.load(Ordering::Acquire) {
                return false;
            }

            match self.control_rx.recv_timeout(PAUSED_POLL_INTERVAL) {
                Ok(control) => {
                    if !self.apply_control(control) {
                        return false;
                    }
                    if !self.drain_controls() {
                        return false;
                    }
                    if !self.paused {
                        return true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn finish_after_drain(&mut self, generation: u64) {
        loop {
            if self.stop_flag.load(Ordering::Acquire) || self.output.generation() != generation {
                return;
            }
            if !self.drain_controls() || self.output.generation() != generation {
                return;
            }
            if self.paused {
                let _ = self.wait_while_paused();
                continue;
            }
            if self.output.queued_samples() == 0 {
                let _ = self.event_tx.send(DecoderEvent::Finished {
                    playback_id: self.playback_id,
                });
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn drain_controls(&mut self) -> bool {
        while let Ok(control) = self.control_rx.try_recv() {
            if !self.apply_control(control) {
                return false;
            }
        }
        true
    }

    fn apply_control(&mut self, control: DecoderControl) -> bool {
        match control {
            DecoderControl::Seek { pos, epoch, ack } => {
                let result = match self.frames.seek(pos) {
                    Ok(()) => {
                        self.applied_seek_epoch = epoch;
                        if cfg!(target_os = "android") {
                            self.output.flush_for_seek();
                            self.pending_seek_flush_epoch = None;
                        } else {
                            self.pending_seek_flush_epoch = Some(epoch);
                        }
                        self.pre_roll_frames_remaining = 0;
                        self.start_ramp_total_frames = SEEK_RAMP_FRAMES;
                        self.start_ramp_frames_remaining = SEEK_RAMP_FRAMES;
                        let _ = self.analysis_tx.send(AnalysisCommand::Clear);
                        Ok(())
                    }
                    Err(e) => {
                        self.applied_seek_epoch = epoch;
                        self.pending_seek_flush_epoch = None;
                        let err = format!("decoder seek failed: {e}");
                        warn!("{err}");
                        Err(err)
                    }
                };
                let _ = ack.send(result);
                true
            }
            DecoderControl::SetPaused(paused) => {
                self.paused = paused;
                self.output.set_paused(paused);
                let _ = self.analysis_tx.send(AnalysisCommand::SetPaused { paused });
                true
            }
            DecoderControl::Stop => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_roll_silence_never_stalls_and_lands_on_control_boundaries() {
        // Zero would spin the decode loop forever: the pre-roll branch
        // `continue`s without consuming a source frame.
        let mut frame_index = 0;
        let mut remaining = PRE_ROLL_SILENCE_FRAMES;
        let mut runs = 0;
        while remaining > 0 {
            if frame_index == DECODE_BLOCK_FRAMES {
                frame_index = 0; // next block
            }
            let frames = pre_roll_run_frames(remaining, frame_index);
            assert!(frames > 0, "pre-roll run of 0 frames would spin forever");
            assert!(frame_index + frames <= DECODE_BLOCK_FRAMES, "ran past the block");
            frame_index += frames;
            assert_eq!(
                frame_index % SEEK_CONTROL_CHECK_FRAMES,
                0,
                "a run must end on a control-check boundary"
            );
            remaining -= frames;
            runs += 1;
            assert!(runs <= PRE_ROLL_SILENCE_FRAMES, "failed to converge");
        }

        // The whole pre-roll is emitted, and it is a whole number of blocks
        // here, so no partial block precedes the first audio.
        assert_eq!(remaining, 0);
        assert_eq!(
            runs,
            PRE_ROLL_SILENCE_FRAMES / SEEK_CONTROL_CHECK_FRAMES,
            "pre-roll should advance one control-check span per run"
        );
    }

    #[test]
    fn pre_roll_emits_silence_without_consuming_source_frames() {
        // The regression this guards: the pre-roll branch used to call
        // `next_frame()` and then throw the decoded frame away, silently
        // truncating the first PRE_ROLL_SILENCE_FRAMES of every track. The run
        // length is derived from the pre-roll counter alone — no source is
        // touched — so a reintroduced `next_frame()` call would have to be
        // added back explicitly rather than hidden in the arithmetic.
        const CHANNELS: usize = 2;
        let mut block: Vec<f32> = Vec::with_capacity(DECODE_BLOCK_FRAMES * CHANNELS);
        let frames = pre_roll_run_frames(PRE_ROLL_SILENCE_FRAMES, 0);

        block.resize(block.len() + frames * CHANNELS, 0.0);

        assert_eq!(block.len(), frames * CHANNELS);
        assert!(block.iter().all(|&s| s == 0.0), "pre-roll must be silence");
        // Filling in place must not outgrow the block the sink recycled for us.
        assert!(block.len() <= DECODE_BLOCK_FRAMES * CHANNELS);
    }

    #[test]
    fn bulk_passthrough_copies_only_complete_frames() {
        let samples = [0.1, -0.1, 0.2, -0.2, 9.0];
        let mut offset = 0;
        let mut output = Vec::new();

        let frames = append_available_interleaved_frames(&samples, &mut offset, &mut output, 2, 8);

        assert_eq!(frames, 2);
        assert_eq!(offset, 4);
        assert_eq!(output, vec![0.1, -0.1, 0.2, -0.2]);
    }

    #[test]
    fn bulk_passthrough_respects_chunk_limit_and_offset() {
        let samples = [0.1, -0.1, 0.2, -0.2, 0.3, -0.3];
        let mut offset = 2;
        let mut output = vec![7.0];

        let frames = append_available_interleaved_frames(&samples, &mut offset, &mut output, 2, 1);

        assert_eq!(frames, 1);
        assert_eq!(offset, 4);
        assert_eq!(output, vec![7.0, 0.2, -0.2]);
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serve `declared` bytes of header but only `sent` of body, in slow chunks.
    fn serve(declared: usize, sent: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/t.mp3", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0u8; 1024];
            let _ = s.read(&mut req);
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\n\r\n");
            let chunk = vec![7u8; 256 * 1024];
            let mut left = sent;
            while left > 0 {
                let n = left.min(chunk.len());
                if s.write_all(&chunk[..n]).is_err() {
                    return;
                }
                left -= n;
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        url
    }

    #[test]
    fn a_large_body_is_readable_while_it_lands() {
        let total = 3 * 1024 * 1024;
        let t0 = Instant::now();
        let guard = download_to_temp_path(&serve(total, total)).unwrap();
        // Returned before the ~360 ms the body takes to send.
        assert!(t0.elapsed() < Duration::from_millis(250));
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        assert_eq!(src.size(), Some(total as u64));
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), total);
    }

    #[test]
    fn a_body_cut_short_fails_the_read() {
        let guard = download_to_temp_path(&serve(3 * 1024 * 1024, 1024 * 1024)).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        assert!(src.read_to_end(&mut Vec::new()).is_err());
    }

    #[test]
    fn dropping_the_guard_unblocks_a_waiting_reader() {
        let guard = download_to_temp_path(&serve(3 * 1024 * 1024, 256 * 1024)).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        let t = std::thread::spawn(move || src.read_to_end(&mut Vec::new()));
        std::thread::sleep(Duration::from_millis(300));
        drop(guard);
        assert!(t.join().unwrap().is_err());
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use crate::source::growing::Progress;

    #[test]
    fn the_gate_holds_below_low_and_releases_only_at_high() {
        // 100 KB/s declared: low = 100 KB, high = 300 KB.
        let progress = Progress::new(10_000_000);
        let mut gate = StreamGate::new(progress.clone(), Some(Duration::from_secs(100)));
        assert_eq!((gate.low, gate.high), (100_000, 300_000));

        let id = progress.begin_writer(0);
        assert!(gate.should_hold(), "nothing landed yet");
        progress.advance(id, 150_000);
        assert!(!gate.should_hold(), "above low and not yet starved");

        gate.starved = true;
        assert!(gate.should_hold(), "starved: 150 KB is below high");
        progress.advance(id, 200_000);
        assert!(!gate.should_hold(), "350 KB clears high");
    }

    #[test]
    fn a_finished_download_never_holds() {
        let progress = Progress::new(1000);
        let gate = StreamGate::new(progress.clone(), None);
        assert!(gate.should_hold());
        let id = progress.begin_writer(0);
        progress.advance(id, 1000);
        progress.end_writer(id, Ok(()));
        assert!(!gate.should_hold(), "the tail is released by completion");
    }
}

#[cfg(test)]
mod range_tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    const TOTAL: usize = 12 * 1024 * 1024;

    fn byte(i: usize) -> u8 {
        (i % 251) as u8
    }

    /// Serves `TOTAL` bytes at roughly 3 MB/s per connection. With
    /// `honor_range` off it answers every request `200` from byte 0.
    fn serve(honor_range: bool) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/t.flac", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let counted = connections.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { return };
                counted.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut req = [0u8; 2048];
                    let n = s.read(&mut req).unwrap_or(0);
                    let req = String::from_utf8_lossy(&req[..n]).to_string();
                    let start = req
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string))
                        .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
                        .filter(|_| honor_range)
                        .unwrap_or(0);
                    let head = if honor_range && start > 0 {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{TOTAL}\r\n\r\n",
                            TOTAL - start,
                            TOTAL - 1
                        )
                    } else {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {TOTAL}\r\n\r\n")
                    };
                    if s.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    let mut at = start;
                    while at < TOTAL {
                        let end = (at + 128 * 1024).min(TOTAL);
                        let chunk: Vec<u8> = (at..end).map(byte).collect();
                        if s.write_all(&chunk).is_err() {
                            return;
                        }
                        at = end;
                        std::thread::sleep(Duration::from_millis(40));
                    }
                });
            }
        });
        (url, connections)
    }

    #[test]
    fn a_far_seek_is_served_by_a_second_range_request() {
        let (url, connections) = serve(true);
        let guard = download_to_temp_path(&url).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();

        // The whole body takes ~3.8 s at the server's pace; the tail must not.
        let at = TOTAL - 50_000;
        let t0 = Instant::now();
        src.seek(SeekFrom::Start(at as u64)).unwrap();
        let mut got = vec![0u8; 1000];
        src.read_exact(&mut got).unwrap();
        assert!(t0.elapsed() < Duration::from_millis(1500), "took {:?}", t0.elapsed());
        assert!(got.iter().enumerate().all(|(i, b)| *b == byte(at + i)));
        assert_eq!(connections.load(Ordering::SeqCst), 2);

        // And the file still ends up whole and exact, with the writers met.
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), TOTAL);
        assert!(all.iter().enumerate().all(|(i, b)| *b == byte(i)));
    }

    #[test]
    fn a_server_that_ignores_range_falls_back_to_waiting() {
        let (url, _) = serve(false);
        let guard = download_to_temp_path(&url).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        let at = TOTAL - 50_000;
        src.seek(SeekFrom::Start(at as u64)).unwrap();
        let mut got = vec![0u8; 1000];
        src.read_exact(&mut got).unwrap();
        assert!(got.iter().enumerate().all(|(i, b)| *b == byte(at + i)));
    }
}
