use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleRate, SizedSample, Stream, StreamConfig};
use tracing::{info, warn};

const PREFERRED_RATES: [u32; 2] = [48_000, 44_100];
const PREFERRED_FORMATS: [cpal::SampleFormat; 3] = [
    cpal::SampleFormat::F32,
    cpal::SampleFormat::I16,
    cpal::SampleFormat::U16,
];
const DEFAULT_QUEUE_BLOCKS: usize = 12;
const LOW_FREQ_MIN_HZ: f32 = 70.0;
const LOW_FREQ_MAX_HZ: f32 = 2_000.0;
const LOW_FREQ_GAIN: f32 = 3.2;
const LOW_FREQ_ATTACK_MS: f32 = 35.0;
const LOW_FREQ_RELEASE_MS: f32 = 160.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputTarget {
    pub channels: u16,
    pub sample_rate: u32,
}

impl OutputTarget {
    pub fn for_source(source_channels: u16, sample_rate: u32) -> Self {
        Self {
            channels: desired_output_channels(source_channels),
            sample_rate: sample_rate.max(1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputConfigKey {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: cpal::SampleFormat,
}

impl OutputConfigKey {
    fn from_config(config: &cpal::SupportedStreamConfig) -> Self {
        Self {
            channels: config.channels(),
            sample_rate: config.sample_rate().0,
            sample_format: config.sample_format(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDeviceKey {
    name: String,
    default_config: Option<OutputConfigKey>,
}

enum OutputControl {
    Clear,
}

#[derive(Clone)]
pub struct OutputWriter {
    data_tx: mpsc::SyncSender<Vec<f32>>,
    control_tx: mpsc::Sender<OutputControl>,
    paused: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
}

impl OutputWriter {
    pub fn push_block(&self, mut block: Vec<f32>, cancel: &AtomicBool) -> bool {
        if block.is_empty() {
            return true;
        }

        let generation = self.generation.load(Ordering::Acquire);
        loop {
            if cancel.load(Ordering::Acquire)
                || self.generation.load(Ordering::Acquire) != generation
            {
                return false;
            }

            if self.paused.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
                continue;
            }

            let block_len = block.len();
            self.queued_samples.fetch_add(block_len, Ordering::Release);
            match self.data_tx.try_send(block) {
                Ok(()) => {
                    // The CPAL callback subtracts samples as it writes them.
                    // This is only for end-of-track draining, not timing.
                    return true;
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    block = returned;
                    thread::sleep(Duration::from_millis(2));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    return false;
                }
            }
        }
    }

    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.queued_samples.store(0, Ordering::Release);
        let _ = self.control_tx.send(OutputControl::Clear);
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }

    pub fn queued_samples(&self) -> usize {
        self.queued_samples.load(Ordering::Acquire)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub struct LowLatencyOutput {
    _stream: Stream,
    writer: OutputWriter,
    device: OutputDeviceKey,
    config: OutputConfigKey,
    target: Option<OutputTarget>,
    low_freq_rx: Option<mpsc::Receiver<f32>>,
}

impl LowLatencyOutput {
    pub fn writer(&self) -> OutputWriter {
        self.writer.clone()
    }

    pub fn config(&self) -> OutputConfigKey {
        self.config
    }

    pub fn device(&self) -> &OutputDeviceKey {
        &self.device
    }

    pub fn target(&self) -> Option<OutputTarget> {
        self.target
    }

    pub fn take_low_freq_rx(&mut self) -> Option<mpsc::Receiver<f32>> {
        self.low_freq_rx.take()
    }
}

pub fn open_preferred_output(target: Option<OutputTarget>) -> Result<LowLatencyOutput, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
    let default_config = match device.default_output_config() {
        Ok(config) => Some(config),
        Err(e) => {
            warn!("default_output_config 失败 (device={device_name}): {e:?}");
            None
        }
    };
    let device_key = OutputDeviceKey {
        name: device_name.clone(),
        default_config: default_config.as_ref().map(OutputConfigKey::from_config),
    };

    let supported_config = match target {
        Some(target) => select_output_config(&device, target, default_config.as_ref())
            .or_else(|| default_config.clone())
            .ok_or_else(|| "no supported output config".to_string())?,
        None => default_config
            .clone()
            .or_else(|| select_any_output_config(&device))
            .ok_or_else(|| "no supported output config".to_string())?,
    };
    let config_key = OutputConfigKey::from_config(&supported_config);
    let stream_config = supported_config.config();

    info!(
        "音频输出：device={device_name} channels={} rate={} format={:?} target={target:?}",
        stream_config.channels, stream_config.sample_rate.0, config_key.sample_format
    );

    let (data_tx, data_rx) = mpsc::sync_channel::<Vec<f32>>(DEFAULT_QUEUE_BLOCKS);
    let (control_tx, control_rx) = mpsc::channel::<OutputControl>();
    let (low_freq_tx, low_freq_rx) = mpsc::sync_channel::<f32>(8);
    let paused = Arc::new(AtomicBool::new(true));
    let volume_bits = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let generation = Arc::new(AtomicU64::new(0));
    let queued_samples = Arc::new(AtomicUsize::new(0));

    let writer = OutputWriter {
        data_tx,
        control_tx,
        paused: Arc::clone(&paused),
        volume_bits: Arc::clone(&volume_bits),
        generation: Arc::clone(&generation),
        queued_samples: Arc::clone(&queued_samples),
    };

    let stream = match config_key.sample_format {
        cpal::SampleFormat::I8 => build_stream::<i8>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::I32 => build_stream::<i32>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::U8 => build_stream::<u8>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::U32 => build_stream::<u32>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        cpal::SampleFormat::F64 => build_stream::<f64>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            paused,
            volume_bits,
            queued_samples,
            low_freq_tx,
        ),
        other => Err(format!("unsupported output sample format: {other:?}")),
    }?;
    stream
        .play()
        .map_err(|e| format!("play output stream: {e:?}"))?;

    Ok(LowLatencyOutput {
        _stream: stream,
        writer,
        device: device_key,
        config: config_key,
        target,
        low_freq_rx: Some(low_freq_rx),
    })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    data_rx: mpsc::Receiver<Vec<f32>>,
    control_rx: mpsc::Receiver<OutputControl>,
    paused: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    queued_samples: Arc<AtomicUsize>,
    low_freq_tx: mpsc::SyncSender<f32>,
) -> Result<Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let mut state = CallbackState {
        data_rx,
        control_rx,
        current_block: Vec::new(),
        current_index: 0,
        output_channels: config.channels.max(1) as usize,
        low_freq: LowFreqState::new(config.sample_rate.0),
        low_freq_tx,
    };
    let err_fn = move |err| warn!("音频输出流错误: {err}");

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _info| {
                fill_output(data, &mut state, &paused, &volume_bits, &queued_samples)
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("build output stream: {e:?}"))
}

struct CallbackState {
    data_rx: mpsc::Receiver<Vec<f32>>,
    control_rx: mpsc::Receiver<OutputControl>,
    current_block: Vec<f32>,
    current_index: usize,
    output_channels: usize,
    low_freq: LowFreqState,
    low_freq_tx: mpsc::SyncSender<f32>,
}

struct LowFreqState {
    channel_index: usize,
    frame_sum: f32,
    prev_input: f32,
    highpass: f32,
    band: f32,
    envelope: f32,
    highpass_alpha: f32,
    lowpass_alpha: f32,
    attack_alpha: f32,
    release_alpha: f32,
}

impl LowFreqState {
    fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(1) as f32;
        let highpass_alpha = (-2.0 * std::f32::consts::PI * LOW_FREQ_MIN_HZ / rate).exp();
        let lowpass_alpha = 1.0 - (-2.0 * std::f32::consts::PI * LOW_FREQ_MAX_HZ / rate).exp();
        let attack_alpha = 1.0 - (-1000.0 / (LOW_FREQ_ATTACK_MS * rate)).exp();
        let release_alpha = 1.0 - (-1000.0 / (LOW_FREQ_RELEASE_MS * rate)).exp();
        Self {
            channel_index: 0,
            frame_sum: 0.0,
            prev_input: 0.0,
            highpass: 0.0,
            band: 0.0,
            envelope: 0.0,
            highpass_alpha,
            lowpass_alpha,
            attack_alpha,
            release_alpha,
        }
    }

    fn push_sample(&mut self, sample: f32, channels: usize) {
        self.frame_sum += sample;
        self.channel_index += 1;
        if self.channel_index < channels {
            return;
        }

        let mono = self.frame_sum / channels as f32;
        self.frame_sum = 0.0;
        self.channel_index = 0;

        self.highpass = self.highpass_alpha * (self.highpass + mono - self.prev_input);
        self.prev_input = mono;
        self.band += (self.highpass - self.band) * self.lowpass_alpha;

        let mut target = (self.band.abs() * LOW_FREQ_GAIN).clamp(0.0, 1.0);
        if target < 0.01 {
            target = 0.0;
        }
        let alpha = if target > self.envelope {
            self.attack_alpha
        } else {
            self.release_alpha
        };
        self.envelope += (target - self.envelope) * alpha;
        self.envelope = self.envelope.clamp(0.0, 1.0);
    }

    fn push_silence(&mut self, samples: usize, channels: usize) {
        for _ in 0..samples {
            self.push_sample(0.0, channels);
        }
    }

    fn reset(&mut self) {
        self.channel_index = 0;
        self.frame_sum = 0.0;
        self.prev_input = 0.0;
        self.highpass = 0.0;
        self.band = 0.0;
        self.envelope = 0.0;
    }
}

fn fill_output<T>(
    data: &mut [T],
    state: &mut CallbackState,
    paused: &AtomicBool,
    volume_bits: &AtomicU32,
    queued_samples: &AtomicUsize,
) where
    T: Sample + FromSample<f32>,
{
    while let Ok(control) = state.control_rx.try_recv() {
        match control {
            OutputControl::Clear => {
                while state.data_rx.try_recv().is_ok() {}
                state.current_block.clear();
                state.current_index = 0;
                state.low_freq.reset();
                send_low_freq(&state.low_freq_tx, 0.0);
            }
        }
    }

    if paused.load(Ordering::Acquire) {
        state.low_freq.push_silence(data.len(), state.output_channels);
        send_low_freq(&state.low_freq_tx, state.low_freq.envelope);
        data.fill(T::from_sample(0.0));
        return;
    }

    let volume = f32::from_bits(volume_bits.load(Ordering::Acquire));
    let mut written = 0usize;

    for sample in data.iter_mut() {
        if state.current_index >= state.current_block.len() {
            match state.data_rx.try_recv() {
                Ok(block) => {
                    state.current_block = block;
                    state.current_index = 0;
                }
                Err(_) => {
                    state.low_freq.push_sample(0.0, state.output_channels);
                    *sample = T::from_sample(0.0);
                    continue;
                }
            }
        }

        let raw_value = state.current_block[state.current_index];
        let value = raw_value * volume;
        state.current_index += 1;
        written += 1;
        state.low_freq.push_sample(raw_value, state.output_channels);
        *sample = T::from_sample(value.clamp(-1.0, 1.0));
    }

    if written > 0 {
        saturating_sub(queued_samples, written);
    }
    send_low_freq(&state.low_freq_tx, state.low_freq.envelope);
}

fn send_low_freq(tx: &mpsc::SyncSender<f32>, value: f32) {
    match tx.try_send(value) {
        Ok(()) => {}
        Err(mpsc::TrySendError::Full(_)) => {}
        Err(mpsc::TrySendError::Disconnected(_)) => {}
    }
}

fn saturating_sub(counter: &AtomicUsize, amount: usize) {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let next = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

fn desired_output_channels(source_channels: u16) -> u16 {
    let source_channels = source_channels.max(1);
    if source_channels <= 2 {
        2
    } else {
        source_channels
    }
}

fn select_any_output_config(device: &cpal::Device) -> Option<cpal::SupportedStreamConfig> {
    let configs = device.supported_output_configs().ok()?;
    configs
        .map(|range| {
            let (sample_rate, rate_score) = select_sample_rate(&range, 48_000, None);
            let score = (format_score(range.sample_format()), rate_score);
            (score, range.with_sample_rate(sample_rate))
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, config)| config)
}

fn select_output_config(
    device: &cpal::Device,
    target: OutputTarget,
    default_config: Option<&cpal::SupportedStreamConfig>,
) -> Option<cpal::SupportedStreamConfig> {
    let default_rate = default_config.map(|config| config.sample_rate().0);
    let configs = device.supported_output_configs().ok()?;

    configs
        .map(|range| {
            let channels = range.channels();
            let sample_format = range.sample_format();
            let (sample_rate, rate_score) =
                select_sample_rate(&range, target.sample_rate, default_rate);
            let score = (
                channel_score(channels, target.channels),
                rate_score,
                format_score(sample_format),
            );
            (score, range.with_sample_rate(sample_rate))
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, config)| config)
}

fn select_sample_rate(
    range: &cpal::SupportedStreamConfigRange,
    target_rate: u32,
    default_rate: Option<u32>,
) -> (SampleRate, u32) {
    if rate_supported(range, target_rate) {
        return (SampleRate(target_rate), 0);
    }

    if let Some(default_rate) = default_rate {
        if rate_supported(range, default_rate) {
            return (
                SampleRate(default_rate),
                10 + default_rate.abs_diff(target_rate) / 100,
            );
        }
    }

    for preferred_rate in PREFERRED_RATES {
        if rate_supported(range, preferred_rate) {
            return (
                SampleRate(preferred_rate),
                100 + preferred_rate.abs_diff(target_rate) / 100,
            );
        }
    }

    let min = range.min_sample_rate().0;
    let max = range.max_sample_rate().0;
    let clamped = target_rate.clamp(min, max);
    (
        SampleRate(clamped),
        1_000 + clamped.abs_diff(target_rate) / 100,
    )
}

fn rate_supported(range: &cpal::SupportedStreamConfigRange, rate: u32) -> bool {
    range.min_sample_rate().0 <= rate && rate <= range.max_sample_rate().0
}

fn channel_score(channels: u16, target_channels: u16) -> u32 {
    if channels == target_channels {
        return 0;
    }
    if target_channels > 2 && channels > target_channels {
        return 10 + u32::from(channels - target_channels);
    }
    if channels > target_channels {
        return 50 + u32::from(channels - target_channels);
    }
    100 + u32::from(target_channels - channels)
}

fn format_score(format: cpal::SampleFormat) -> u32 {
    PREFERRED_FORMATS
        .iter()
        .position(|preferred| *preferred == format)
        .map(|index| index as u32)
        .unwrap_or(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_channels_keep_stereo_for_music_sources() {
        assert_eq!(desired_output_channels(1), 2);
        assert_eq!(desired_output_channels(2), 2);
        assert_eq!(desired_output_channels(6), 6);
    }

    #[test]
    fn channel_score_prefers_exact_then_wider_for_multichannel() {
        assert!(channel_score(6, 6) < channel_score(8, 6));
        assert!(channel_score(8, 6) < channel_score(2, 6));
        assert!(channel_score(2, 2) < channel_score(6, 2));
    }
}
