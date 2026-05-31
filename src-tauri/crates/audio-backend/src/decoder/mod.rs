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

#[allow(clippy::too_many_arguments)]
pub fn spawn_playback_decoder(
    path: &Path,
    initial_position: Option<f64>,
    output: OutputWriter,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: mpsc::Sender<AnalysisCommand>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    start_paused: bool,
) -> AudioResult<DecoderHandle> {
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

struct DecodeWorker {
    frames: FrameConverter,
    output: OutputWriter,
    output_channels: u16,
    output_sample_rate: u32,
    analysis_tx: mpsc::Sender<AnalysisCommand>,
    control_rx: mpsc::Receiver<DecoderControl>,
    event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
    playback_id: u64,
    stop_flag: Arc<AtomicBool>,
    paused: bool,
    pre_roll_frames_remaining: usize,
}

#[allow(clippy::too_many_arguments)]
impl DecodeWorker {
    fn new(
        source: Box<dyn Source<Item = f32> + Send>,
        input_channels: u16,
        input_sample_rate: u32,
        output: OutputWriter,
        output_channels: u16,
        output_sample_rate: u32,
        analysis_tx: mpsc::Sender<AnalysisCommand>,
        control_rx: mpsc::Receiver<DecoderControl>,
        event_tx: tokio_mpsc::UnboundedSender<DecoderEvent>,
        playback_id: u64,
        stop_flag: Arc<AtomicBool>,
        paused: bool,
    ) -> Self {
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
            control_rx,
            event_tx,
            playback_id,
            stop_flag,
            paused,
            pre_roll_frames_remaining: PRE_ROLL_SILENCE_FRAMES,
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
                Vec::with_capacity(DECODE_BLOCK_FRAMES * self.output_channels as usize);
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
                    block.extend_from_slice(&frame);
                    analysis_block.extend_from_slice(&frame);
                }
            }

            let pushed = block.is_empty() || self.output.push_block(block, self.stop_flag.as_ref());
            let generation_current = self.output.generation() == generation;

            if pushed && generation_current && !analysis_block.is_empty() {
                let _ = self.analysis_tx.send(AnalysisCommand::Pcm {
                    samples: analysis_block,
                    channels: self.output_channels,
                    sample_rate: self.output_sample_rate,
                    recycle: None,
                });
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
    current_frame: Option<Vec<f32>>,
    next_frame: Option<Vec<f32>>,
    reached_end: bool,
    frac: f64,
    step: f64,
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
        let step = input_sample_rate.max(1) as f64 / output_sample_rate.max(1) as f64;
        Self {
            source,
            input_channels,
            output_channels,
            matrix: build_mix_matrix(input_channels, output_channels),
            current_frame: None,
            next_frame: None,
            reached_end: false,
            frac: 0.0,
            step,
        }
    }

    fn seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.source.try_seek(pos)?;
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.current_frame = None;
        self.next_frame = None;
        self.reached_end = false;
        self.frac = 0.0;
    }

    fn next_frame(&mut self, output: &mut [f32]) -> Option<()> {
        debug_assert_eq!(output.len(), self.output_channels);
        if self.current_frame.is_none() {
            self.current_frame = Some(self.read_input_frame()?);
        }
        if self.next_frame.is_none() && !self.reached_end {
            match self.read_input_frame() {
                Some(frame) => self.next_frame = Some(frame),
                None => self.reached_end = true,
            }
        }

        self.mix_current(output);

        if self.reached_end && self.next_frame.is_none() {
            self.current_frame = None;
            return Some(());
        }

        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            if let Some(next) = self.next_frame.take() {
                self.current_frame = Some(next);
            } else {
                self.current_frame = None;
                break;
            }

            match self.read_input_frame() {
                Some(frame) => self.next_frame = Some(frame),
                None => {
                    self.reached_end = true;
                    break;
                }
            }
        }

        Some(())
    }

    fn read_input_frame(&mut self) -> Option<Vec<f32>> {
        let mut frame = Vec::with_capacity(self.input_channels);
        for _ in 0..self.input_channels {
            frame.push(self.source.next()?);
        }
        Some(frame)
    }

    fn mix_current(&self, output: &mut [f32]) {
        let current = self.current_frame.as_ref().expect("current frame exists");
        let next = self.next_frame.as_ref();
        let frac = self.frac as f32;

        for (out, row) in output.iter_mut().zip(&self.matrix) {
            let mut mixed = 0.0;
            for &(input, gain) in row {
                let sample = match next {
                    Some(next) => current[input] + (next[input] - current[input]) * frac,
                    None => current[input],
                };
                mixed += sample * gain;
            }
            *out = mixed;
        }
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
