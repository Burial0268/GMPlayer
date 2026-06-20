pub mod symphonia;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use rodio::{Decoder, Source};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::warn;

use crate::analysis::AnalysisCommand;
use crate::error::{AudioError, AudioResult};
use crate::output::OutputWriter;

pub trait PlaybackSink: Clone + Send + 'static {
    fn push_block(&self, block: Vec<f32>, cancel: &AtomicBool) -> bool;
    fn clear(&self);
    fn set_paused(&self, paused: bool);
    fn queued_samples(&self) -> usize;
    fn generation(&self) -> u64;
}

impl PlaybackSink for OutputWriter {
    fn push_block(&self, block: Vec<f32>, cancel: &AtomicBool) -> bool {
        OutputWriter::push_block(self, block, cancel)
    }

    fn clear(&self) {
        OutputWriter::clear(self);
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
    Seek(Duration),
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
}

impl DecoderHandle {
    pub fn seek(&self, pos: Duration) -> AudioResult<()> {
        self.control_tx
            .send(DecoderControl::Seek(pos))
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))
    }

    pub fn set_paused(&self, paused: bool) -> AudioResult<()> {
        self.control_tx
            .send(DecoderControl::SetPaused(paused))
            .map_err(|_| AudioError::ThreadError("decoder control channel closed".into()))
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        let _ = self.control_tx.send(DecoderControl::Stop);
    }
}

impl Drop for DecoderHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DecoderEvent {
    Finished { playback_id: u64 },
}

const DECODE_BLOCK_FRAMES: usize = 512;
const PRE_ROLL_SILENCE_FRAMES: usize = 1024;
const START_RAMP_FRAMES: usize = 2048;

#[allow(clippy::too_many_arguments)]
pub fn spawn_playback_decoder<S>(
    path: &Path,
    initial_position: Option<f64>,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: mpsc::Sender<AnalysisCommand>,
    analysis_enabled: Arc<AtomicBool>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    start_paused: bool,
) -> AudioResult<DecoderHandle>
where
    S: PlaybackSink,
{
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    let decoder = Decoder::new(file).map_err(|e| AudioError::Decode(e.to_string()))?;
    let mut source = decoder.convert_samples::<f32>();
    let input_channels = source.channels().max(1);
    let input_sample_rate = source.sample_rate().max(1);

    if let Some(position) = initial_position.filter(|position| *position > 0.0) {
        source
            .try_seek(Duration::from_secs_f64(position))
            .map_err(|e| AudioError::Decode(format!("initial seek failed: {e}")))?;
    }

    let (control_tx, control_rx) = mpsc::channel();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop_flag);
    let output_channels = output_channels.max(1);
    let output_sample_rate = output_sample_rate.max(1);

    std::thread::Builder::new()
        .name("audio-decode".into())
        .spawn(move || {
            let mut worker = DecodeWorker::new(
                Box::new(source),
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
                start_paused,
            );
            worker.run();
        })
        .map_err(|e| AudioError::ThreadError(format!("spawn audio-decode thread: {e}")))?;

    Ok(DecoderHandle {
        control_tx,
        stop_flag,
    })
}

struct DecodeWorker<S: PlaybackSink> {
    frames: FrameConverter,
    output: S,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: mpsc::Sender<AnalysisCommand>,
    analysis_enabled: Arc<AtomicBool>,
    analysis_recycle_tx: mpsc::Sender<Vec<f32>>,
    analysis_recycle_rx: mpsc::Receiver<Vec<f32>>,
    control_rx: mpsc::Receiver<DecoderControl>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    stop_flag: Arc<AtomicBool>,
    paused: bool,
    pre_roll_frames_remaining: usize,
    start_ramp_frames_remaining: usize,
}

#[allow(clippy::too_many_arguments)]
impl<S: PlaybackSink> DecodeWorker<S> {
    fn new(
        source: Box<dyn Source<Item = f32> + Send>,
        input_channels: u16,
        input_sample_rate: u32,
        output: S,
        output_channels: u16,
        output_sample_rate: u32,
        analysis_tx: mpsc::Sender<AnalysisCommand>,
        analysis_enabled: Arc<AtomicBool>,
        control_rx: mpsc::Receiver<DecoderControl>,
        event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
        playback_id: u64,
        stop_flag: Arc<AtomicBool>,
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
            paused,
            pre_roll_frames_remaining: PRE_ROLL_SILENCE_FRAMES,
            start_ramp_frames_remaining: START_RAMP_FRAMES,
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

            let generation = self.output.generation();
            let mut block = Vec::with_capacity(DECODE_BLOCK_FRAMES * self.output_channels as usize);
            let mut analysis_block =
                if self.analysis_enabled.load(Ordering::Acquire) {
                    Some(self.analysis_recycle_rx.try_recv().unwrap_or_else(|_| {
                        Vec::with_capacity(DECODE_BLOCK_FRAMES * self.output_channels as usize)
                    }))
                } else {
                    None
                };
            if let Some(block) = analysis_block.as_mut() {
                block.clear();
            }
            let mut ended = false;

            for _ in 0..DECODE_BLOCK_FRAMES {
                if self.frames.next_frame(&mut frame).is_none() {
                    ended = true;
                    break;
                }

                if self.pre_roll_frames_remaining > 0 {
                    self.pre_roll_frames_remaining -= 1;
                    block.extend(std::iter::repeat(0.0).take(self.output_channels as usize));
                } else {
                    if self.start_ramp_frames_remaining > 0 {
                        let elapsed = START_RAMP_FRAMES - self.start_ramp_frames_remaining;
                        let t = (elapsed as f32 / START_RAMP_FRAMES as f32).clamp(0.0, 1.0);
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
            }

            let pushed = block.is_empty() || self.output.push_block(block, self.stop_flag.as_ref());
            let generation_current = self.output.generation() == generation;

            if pushed && generation_current && self.analysis_enabled.load(Ordering::Acquire) {
                if let Some(analysis_block) = analysis_block.take() {
                    if !analysis_block.is_empty() {
                        let _ = self.analysis_tx.send(AnalysisCommand::Pcm {
                            samples: analysis_block,
                            channels: self.output_channels,
                            sample_rate: self.output_sample_rate,
                            recycle: Some(self.analysis_recycle_tx.clone()),
                        });
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
            DecoderControl::Seek(pos) => {
                self.output.clear();
                match self.frames.seek(pos) {
                    Ok(()) => {
                        self.pre_roll_frames_remaining = PRE_ROLL_SILENCE_FRAMES;
                        self.start_ramp_frames_remaining = START_RAMP_FRAMES;
                        let _ = self.analysis_tx.send(AnalysisCommand::Clear);
                    }
                    Err(e) => warn!("decoder seek failed: {e}"),
                }
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
    source: Box<dyn Source<Item = f32> + Send>,
    input_channels: usize,
    output_channels: usize,
    matrix: Vec<Vec<(usize, f32)>>,
    /// Reused scratch buffers (capacity == input_channels). They are swapped,
    /// never reallocated, so the per-frame hot path performs zero heap
    /// allocations regardless of how long playback runs.
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
        source: Box<dyn Source<Item = f32> + Send>,
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
            current_frame: Vec::with_capacity(input_channels),
            next_frame: Vec::with_capacity(input_channels),
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
        self.current_frame.clear();
        self.next_frame.clear();
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
            if !read_input_frame(self.source.as_mut(), self.input_channels, &mut self.current_frame)
            {
                return None;
            }
            apply_mix_matrix(&self.matrix, &self.current_frame, output);
            return Some(());
        }

        // Linear-interpolation resampling path.
        if !self.has_current {
            if !read_input_frame(self.source.as_mut(), self.input_channels, &mut self.current_frame)
            {
                return None;
            }
            self.has_current = true;
        }
        if !self.has_next && !self.reached_end {
            if read_input_frame(self.source.as_mut(), self.input_channels, &mut self.next_frame) {
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
                if read_input_frame(self.source.as_mut(), self.input_channels, &mut self.next_frame)
                {
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

/// Pull one interleaved input frame from `source` into `buf` (reusing its
/// capacity). Returns `false` if the source ends, leaving any partial frame to
/// be discarded by the caller — matching rodio's frame-aligned EOF behaviour.
#[inline]
fn read_input_frame(
    source: &mut (dyn Source<Item = f32> + Send),
    channels: usize,
    buf: &mut Vec<f32>,
) -> bool {
    buf.clear();
    for _ in 0..channels {
        match source.next() {
            Some(sample) => buf.push(sample),
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
