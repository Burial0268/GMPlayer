pub mod symphonia;

use std::fs::File;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

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

pub(crate) trait PlaybackSink: Clone + Send + 'static {
    fn push_block(&self, block: Vec<f32>, cancel: PushCancel<'_>) -> bool;
    fn flush_for_seek(&self);
    fn set_paused(&self, paused: bool);
    fn queued_samples(&self) -> usize;
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

        Ok(DecoderSeekAck { ack_rx })
    }

    pub fn set_paused(&self, paused: bool) -> AudioResult<()> {
        self.control_tx
            .send(DecoderControl::SetPaused(paused))
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        self.seek_epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.control_tx.send(DecoderControl::Stop);
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
}

const DECODE_BLOCK_FRAMES: usize = 512;
const PRE_ROLL_SILENCE_FRAMES: usize = 1024;
const START_RAMP_FRAMES: usize = 2048;
const SEEK_RAMP_FRAMES: usize = 512;
const SEEK_CONTROL_CHECK_FRAMES: usize = 64;
const MAX_DECODE_RETRIES: usize = 3;

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
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(ext);
        }

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
        self.spec = spec;
        self.buffer_offset = 0;
        Some(())
    }

    fn refine_position(&mut self, seeked: SeekedTo) -> Result<(), SymphoniaError> {
        let mut frames_to_skip = seeked.required_ts.saturating_sub(seeked.actual_ts);
        loop {
            let packet = self.next_track_packet()?;
            if packet.dur() <= frames_to_skip {
                frames_to_skip -= packet.dur();
                continue;
            }

            let decoded = self.decoder.decode(&packet)?;
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
            Err(SymphoniaError::IoError(_)) => return Ok(None),
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

                // The common file/device layout needs neither resampling nor
                // channel mixing. Once startup/seek shaping is complete, copy
                // decoded interleaved PCM in contiguous chunks instead of
                // pulling it through next_frame() sample by sample. Stop each
                // chunk at the next control-check boundary so seek latency is
                // unchanged.
                if self.pre_roll_frames_remaining == 0
                    && self.start_ramp_frames_remaining == 0
                    && self.frames.can_bulk_passthrough()
                {
                    let until_control_check =
                        SEEK_CONTROL_CHECK_FRAMES - (frame_index % SEEK_CONTROL_CHECK_FRAMES);
                    let requested = until_control_check.min(DECODE_BLOCK_FRAMES - frame_index);
                    let block_start = block.len();
                    let copied = self.frames.append_passthrough_frames(&mut block, requested);
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

                if self.pre_roll_frames_remaining > 0 {
                    self.pre_roll_frames_remaining -= 1;
                    block.extend(std::iter::repeat(0.0).take(self.output_channels as usize));
                } else {
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

    fn wait_while_paused(&mut self) -> bool {
        loop {
            if self.stop_flag.load(Ordering::Acquire) {
                return false;
            }

            match self.control_rx.recv_timeout(Duration::from_millis(10)) {
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
                true
            }
            DecoderControl::Stop => false,
        }
    }
}

struct FrameConverter {
    // Playback always uses our Symphonia source. Keeping the concrete type
    // here lets the compiler inline the per-sample iterator hot path instead
    // of paying a trait-object dispatch for every decoded sample.
    source: SeekableSymphoniaSource,
    input_channels: usize,
    output_channels: usize,
    matrix: Vec<Vec<(usize, f32)>>,
    /// Fixed-size scratch buffers. They are swapped, never reallocated or
    /// resized, so the per-frame hot path only overwrites existing samples.
    current_frame: Vec<f32>,
    next_frame: Vec<f32>,
    has_current: bool,
    has_next: bool,
    reached_end: bool,
    frac: f64,
    step: f64,
    /// `true` when input and output sample rates match, letting us skip the
    /// linear-interpolation resampler (and its lookahead frame) entirely.
    resample: bool,
    /// `true` when channels map 1:1 (identity matrix), letting the no-resample
    /// path read straight into the output frame with no matrix mixing.
    passthrough: bool,
}

impl FrameConverter {
    fn new(
        source: SeekableSymphoniaSource,
        input_channels: u16,
        input_sample_rate: u32,
        output_channels: u16,
        output_sample_rate: u32,
    ) -> Self {
        let input_channels = input_channels.max(1) as usize;
        let output_channels = output_channels.max(1) as usize;
        let input_sample_rate = input_sample_rate.max(1);
        let output_sample_rate = output_sample_rate.max(1);
        let step = input_sample_rate as f64 / output_sample_rate as f64;
        let matrix = build_mix_matrix(input_channels, output_channels);
        let passthrough = input_channels == output_channels
            && matrix
                .iter()
                .enumerate()
                .all(|(out, row)| row.len() == 1 && row[0].0 == out && row[0].1 == 1.0);
        Self {
            source,
            input_channels,
            output_channels,
            matrix,
            current_frame: vec![0.0; input_channels],
            next_frame: vec![0.0; input_channels],
            has_current: false,
            has_next: false,
            reached_end: false,
            frac: 0.0,
            step,
            resample: input_sample_rate != output_sample_rate,
            passthrough,
        }
    }

    fn seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.source.try_seek(pos)?;
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.has_current = false;
        self.has_next = false;
        self.reached_end = false;
        self.frac = 0.0;
    }

    #[inline]
    fn can_bulk_passthrough(&self) -> bool {
        !self.resample && self.passthrough
    }

    fn append_passthrough_frames(&mut self, output: &mut Vec<f32>, frames: usize) -> usize {
        debug_assert!(self.can_bulk_passthrough());
        self.source
            .append_interleaved_frames(output, self.output_channels, frames)
    }

    fn next_frame(&mut self, output: &mut [f32]) -> Option<()> {
        debug_assert_eq!(output.len(), self.output_channels);

        if !self.resample {
            // No sample-rate conversion: one input frame maps to one output frame.
            if self.passthrough {
                // Hottest path (e.g. stereo file → stereo device): pull straight
                // into the output frame — no buffering, no matrix, no alloc.
                for slot in output.iter_mut() {
                    *slot = self.source.next()?;
                }
                return Some(());
            }
            if !read_input_frame(
                &mut self.source,
                self.input_channels,
                &mut self.current_frame,
            ) {
                return None;
            }
            apply_mix_matrix(&self.matrix, &self.current_frame, output);
            return Some(());
        }

        // Linear-interpolation resampling path.
        if !self.has_current {
            if !read_input_frame(
                &mut self.source,
                self.input_channels,
                &mut self.current_frame,
            ) {
                return None;
            }
            self.has_current = true;
        }
        if !self.has_next && !self.reached_end {
            if read_input_frame(&mut self.source, self.input_channels, &mut self.next_frame) {
                self.has_next = true;
            } else {
                self.reached_end = true;
            }
        }

        self.mix_current(output);

        if self.reached_end && !self.has_next {
            self.has_current = false;
            return Some(());
        }

        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            if self.has_next {
                std::mem::swap(&mut self.current_frame, &mut self.next_frame);
                self.has_next = false;
                if read_input_frame(&mut self.source, self.input_channels, &mut self.next_frame) {
                    self.has_next = true;
                } else {
                    self.reached_end = true;
                }
            } else {
                self.has_current = false;
                break;
            }
        }

        Some(())
    }

    fn mix_current(&self, output: &mut [f32]) {
        let current = &self.current_frame;
        let frac = self.frac as f32;

        if self.has_next {
            let next = &self.next_frame;
            for (out, row) in output.iter_mut().zip(&self.matrix) {
                let mut mixed = 0.0;
                for &(input, gain) in row {
                    let sample = current[input] + (next[input] - current[input]) * frac;
                    mixed += sample * gain;
                }
                *out = mixed;
            }
        } else {
            apply_mix_matrix(&self.matrix, current, output);
        }
    }
}

/// Pull one interleaved input frame from `source` into a fixed-size scratch
/// buffer. Returns `false` if the source ends, leaving any partial frame to
/// be discarded by the caller — matching rodio's frame-aligned EOF behaviour.
#[inline]
fn read_input_frame(
    source: &mut SeekableSymphoniaSource,
    channels: usize,
    buf: &mut [f32],
) -> bool {
    debug_assert_eq!(buf.len(), channels);
    for slot in &mut buf[..channels] {
        match source.next() {
            Some(sample) => *slot = sample,
            None => return false,
        }
    }
    true
}

/// Apply the channel mix matrix for a single frame (no interpolation).
#[inline]
fn apply_mix_matrix(matrix: &[Vec<(usize, f32)>], frame: &[f32], output: &mut [f32]) {
    for (out, row) in output.iter_mut().zip(matrix) {
        let mut mixed = 0.0;
        for &(input, gain) in row {
            mixed += frame[input] * gain;
        }
        *out = mixed;
    }
}

fn build_mix_matrix(input_channels: usize, output_channels: usize) -> Vec<Vec<(usize, f32)>> {
    debug_assert!(input_channels > 0);
    debug_assert!(output_channels > 0);

    if output_channels == 1 {
        let gain = 1.0 / input_channels as f32;
        return vec![(0..input_channels).map(|ch| (ch, gain)).collect()];
    }

    if input_channels == 1 {
        return (0..output_channels).map(|_| vec![(0, 1.0)]).collect();
    }

    if output_channels == 2 {
        let mut rows = vec![vec![(0, 1.0)], vec![(1, 1.0)]];
        if input_channels > 2 {
            let gain = 0.5 / (input_channels - 2) as f32;
            for ch in 2..input_channels {
                rows[0].push((ch, gain));
                rows[1].push((ch, gain));
            }
        }
        return rows;
    }

    let mut rows = Vec::with_capacity(output_channels);
    for out in 0..output_channels {
        if out < input_channels {
            rows.push(vec![(out, 1.0)]);
        } else {
            rows.push(Vec::new());
        }
    }

    if input_channels > output_channels {
        let extra_count = input_channels - output_channels;
        let gain = 0.5 / extra_count as f32;
        for ch in output_channels..input_channels {
            rows[ch % output_channels].push((ch, gain));
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

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
