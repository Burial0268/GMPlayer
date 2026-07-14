use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample, Stream, StreamConfig};
use parking_lot::Mutex;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use tracing::{info, warn};

mod config;
mod platform;
mod render;

use config::{select_any_output_config, select_output_config, stable_stream_config};
use platform::DEFAULT_QUEUE_BLOCKS;
use render::{fill_output, CallbackState, OutputBlock};

// Depth of the buffer-recycling channel that returns spent mix blocks from the
// CPAL callback back to the mixer for reuse. Sized a little above the data
// queue so every in-flight buffer normally has a slot. If it is full, the
// callback retains the spent block and pauses dequeuing until a slot opens;
// this keeps allocation and deallocation off the real-time thread.
const RECYCLE_QUEUE_BLOCKS: usize = DEFAULT_QUEUE_BLOCKS + 4;
// Callback-owned overflow slots for transient recycle backpressure. Reserved
// before stream start; matching the PCM queue depth lets the callback drain a
// full ready queue without allocating even if the mixer is briefly descheduled.
const PENDING_RECYCLE_BLOCKS: usize = DEFAULT_QUEUE_BLOCKS;
const OUTPUT_INIT_TIMEOUT: Duration = Duration::from_secs(4);
const PRODUCER_YIELD_RETRIES: u32 = 8;
const PRODUCER_MIN_PARK_US: u64 = 100;
// Steady-state playback keeps the output ring full, so the mixer producer
// lives in this park-poll loop: the cap IS the wakeup cadence. 4ms ≈ 2-3
// polls per freed ~10ms block instead of ~10, cutting wakeups ~4× under CPU
// stress. Interrupts (seek/stop/generation bump) are re-checked after every
// park, so those paths gain at most one cap of latency.
const PRODUCER_MAX_PARK_US: u64 = 4_000;

#[derive(Clone, Copy)]
pub(crate) struct PushCancel<'a> {
    stop: &'a AtomicBool,
    interrupt_epoch: Option<(&'a AtomicU64, u64)>,
}

impl<'a> PushCancel<'a> {
    pub(crate) fn with_interrupt_epoch(
        stop: &'a AtomicBool,
        interrupt_epoch: &'a AtomicU64,
        observed_epoch: u64,
    ) -> Self {
        Self {
            stop,
            interrupt_epoch: Some((interrupt_epoch, observed_epoch)),
        }
    }

    #[inline]
    pub(crate) fn is_cancelled(self) -> bool {
        self.stop.load(Ordering::Acquire)
            || self
                .interrupt_epoch
                .is_some_and(|(epoch, observed)| epoch.load(Ordering::Acquire) != observed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputTarget {
    pub channels: u16,
    pub sample_rate: u32,
}

impl OutputTarget {
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub fn for_source(source_channels: u16, sample_rate: u32) -> Self {
        Self {
            channels: config::desired_output_channels(source_channels),
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
            sample_rate: config.sample_rate(),
            sample_format: config.sample_format(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDeviceKey {
    platform_id: Option<String>,
    name: String,
    default_config: Option<OutputConfigKey>,
    device_signature: Option<u64>,
}

impl OutputDeviceKey {
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputDeviceSelector {
    Default,
    Named(String),
}

impl OutputDeviceSelector {
    pub fn from_name(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty()
            || name.eq_ignore_ascii_case("default")
            || name.eq_ignore_ascii_case("system")
            || name == "__default__"
        {
            Self::Default
        } else {
            Self::Named(name.to_string())
        }
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }

    fn label(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::Named(name) => name,
        }
    }
}

#[derive(Clone)]
pub struct OutputWriter {
    /// All clones share one producer endpoint. Production playback has one
    /// mixer thread; the mutex also preserves SPSC safety for the cloneable
    /// public handle if another caller ever attempts to push concurrently.
    /// The CPAL callback never takes this lock.
    data_tx: Arc<Mutex<Producer<OutputBlock>>>,
    paused: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
    callback_alive: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    rendered_samples: Arc<AtomicU64>,
    /// Spent mix blocks returned by the CPAL callback for reuse. Single
    /// consumer (the mixer) behind a `Mutex` so `OutputWriter` stays `Sync`
    /// while remaining cloneable; the lock is only ever taken off the audio
    /// callback (on the mixer thread), so it never blocks real-time work.
    recycle_rx: Arc<Mutex<Consumer<Vec<f32>>>>,
}

#[derive(Clone, Debug)]
pub struct OutputRenderClock {
    rendered_samples: Arc<AtomicU64>,
}

impl OutputRenderClock {
    pub fn rendered_samples(&self) -> u64 {
        // Monotonic telemetry only. PCM visibility is synchronized by the
        // channel plus generation/flush barriers, not by this counter.
        self.rendered_samples.load(Ordering::Relaxed)
    }
}

impl OutputWriter {
    pub fn push_block(&self, mut block: Vec<f32>, cancel: PushCancel<'_>) -> bool {
        if block.is_empty() {
            return true;
        }

        let generation = self.generation.load(Ordering::Acquire);
        let flush_epoch = self.flush_epoch.load(Ordering::Acquire);
        let mut retry_count = 0;
        loop {
            if cancel.is_cancelled()
                || self.generation.load(Ordering::Acquire) != generation
                || self.flush_epoch.load(Ordering::Acquire) != flush_epoch
                || self.stream_failed.load(Ordering::Acquire)
                || !self.callback_alive.load(Ordering::Acquire)
            {
                return false;
            }

            let block_len = block.len();
            let output_block = OutputBlock {
                samples: block,
                generation,
                flush_epoch,
            };
            self.queued_samples.fetch_add(block_len, Ordering::Release);
            let push_result = self.data_tx.lock().push(output_block);
            match push_result {
                Ok(()) => {
                    // The CPAL callback subtracts samples as it writes them.
                    // This is only for end-of-track draining, not timing.
                    // Once published into the ring, ownership cannot be
                    // revoked. A concurrent flush/generation change makes the
                    // block stale at the consumer barrier instead.
                    return true;
                }
                Err(PushError::Full(returned)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    block = returned.samples;
                    producer_retry_backoff(&mut retry_count);
                }
            }
        }
    }

    pub fn flush(&self) {
        self.flush_epoch.fetch_add(1, Ordering::AcqRel);
        self.queued_samples.store(0, Ordering::Release);
    }

    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.flush_epoch.fetch_add(1, Ordering::AcqRel);
        self.queued_samples.store(0, Ordering::Release);
    }

    pub fn retire(&self) {
        self.stream_failed.store(true, Ordering::Release);
        self.paused.store(true, Ordering::Release);
        self.clear();
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }

    pub fn queued_samples(&self) -> usize {
        // Queue depth participates in prebuffer and drain decisions. Pair the
        // read with producer/flush Release operations so control threads do
        // not rely on a purely relaxed observation on weak-memory targets.
        self.queued_samples.load(Ordering::Acquire)
    }

    /// Reuse a spent mix block returned by the CPAL callback, or allocate a
    /// fresh one when the pool is empty. Called only from the mixer thread, so
    /// the `try_lock` is effectively uncontended and never touches the audio
    /// callback. The returned buffer is empty (`len == 0`) with at least
    /// `capacity` capacity, ready to be filled by pushing.
    pub fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        if let Some(mut rx) = self.recycle_rx.try_lock() {
            if let Ok(mut buf) = rx.pop() {
                buf.clear();
                if buf.capacity() < capacity {
                    buf.reserve(capacity - buf.capacity());
                }
                return buf;
            }
        }
        Vec::with_capacity(capacity)
    }

    pub fn render_clock(&self) -> OutputRenderClock {
        OutputRenderClock {
            rendered_samples: Arc::clone(&self.rendered_samples),
        }
    }

    pub fn has_failed(&self) -> bool {
        self.stream_failed.load(Ordering::Acquire)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub struct LowLatencyOutput {
    stream_stop_tx: mpsc::Sender<()>,
    stream_thread: Option<thread::JoinHandle<()>>,
    writer: OutputWriter,
    selector: OutputDeviceSelector,
    device: OutputDeviceKey,
    config: OutputConfigKey,
    #[cfg_attr(target_os = "android", allow(dead_code))]
    target: Option<OutputTarget>,
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

    pub fn selector(&self) -> &OutputDeviceSelector {
        &self.selector
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub fn target(&self) -> Option<OutputTarget> {
        self.target
    }

    pub fn has_failed(&self) -> bool {
        self.writer.has_failed()
    }
}

pub fn open_output(
    selector: OutputDeviceSelector,
    target: Option<OutputTarget>,
) -> Result<LowLatencyOutput, String> {
    let (data_tx, data_rx) = RingBuffer::<OutputBlock>::new(DEFAULT_QUEUE_BLOCKS);
    let (recycle_tx, recycle_rx) = RingBuffer::<Vec<f32>>::new(RECYCLE_QUEUE_BLOCKS);
    let (stream_stop_tx, stream_stop_rx) = mpsc::channel::<()>();
    let (init_tx, init_rx) = mpsc::channel::<Result<(OutputDeviceKey, OutputConfigKey), String>>();
    let paused = Arc::new(AtomicBool::new(true));
    let stream_failed = Arc::new(AtomicBool::new(false));
    let callback_alive = Arc::new(AtomicBool::new(true));
    let volume_bits = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let generation = Arc::new(AtomicU64::new(0));
    let flush_epoch = Arc::new(AtomicU64::new(0));
    let queued_samples = Arc::new(AtomicUsize::new(0));
    let rendered_samples = Arc::new(AtomicU64::new(0));
    let stream_selector = selector.clone();

    let writer = OutputWriter {
        data_tx: Arc::new(Mutex::new(data_tx)),
        paused: Arc::clone(&paused),
        stream_failed: Arc::clone(&stream_failed),
        callback_alive: Arc::clone(&callback_alive),
        volume_bits: Arc::clone(&volume_bits),
        generation: Arc::clone(&generation),
        flush_epoch: Arc::clone(&flush_epoch),
        queued_samples: Arc::clone(&queued_samples),
        rendered_samples: Arc::clone(&rendered_samples),
        recycle_rx: Arc::new(Mutex::new(recycle_rx)),
    };

    let stream_thread = thread::Builder::new()
        .name("audio-output".into())
        .spawn(move || {
            let result = open_output_stream(
                stream_selector,
                target,
                data_rx,
                recycle_tx,
                paused,
                stream_failed,
                callback_alive,
                volume_bits,
                generation,
                flush_epoch,
                queued_samples,
                rendered_samples,
            );
            match result {
                Ok((stream, device_key, config_key)) => {
                    let _ = init_tx.send(Ok((device_key, config_key)));
                    let _ = stream_stop_rx.recv();
                    drop(stream);
                }
                Err(err) => {
                    let _ = init_tx.send(Err(err));
                }
            }
        })
        .map_err(|e| format!("spawn output thread: {e}"))?;

    let (device, config) = match init_rx.recv_timeout(OUTPUT_INIT_TIMEOUT) {
        Ok(result) => result?,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = stream_stop_tx.send(());
            join_output_thread_async(stream_thread);
            return Err("output stream init timed out".to_string());
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err("output thread exited during init".to_string());
        }
    };

    Ok(LowLatencyOutput {
        stream_stop_tx,
        stream_thread: Some(stream_thread),
        writer,
        selector,
        device,
        config,
        target,
    })
}

impl Drop for LowLatencyOutput {
    fn drop(&mut self) {
        let _ = self.stream_stop_tx.send(());
        if let Some(thread) = self.stream_thread.take() {
            join_output_thread_async(thread);
        }
    }
}

fn join_output_thread_async(handle: thread::JoinHandle<()>) {
    let _ = thread::Builder::new()
        .name("audio-output-join".into())
        .spawn(move || {
            let _ = handle.join();
        });
}

pub fn selected_output_device_key(
    selector: &OutputDeviceSelector,
) -> Result<OutputDeviceKey, String> {
    let host = cpal::default_host();
    let device = resolve_output_device(&host, selector)?;
    let (device_key, _) = output_device_key_and_config(&host, &device, selector.is_default());
    Ok(device_key)
}

fn resolve_output_device(
    host: &cpal::Host,
    selector: &OutputDeviceSelector,
) -> Result<cpal::Device, String> {
    match selector {
        OutputDeviceSelector::Default => host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string()),
        OutputDeviceSelector::Named(name) => find_output_device_by_name(host, name),
    }
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| device.to_string())
}

fn find_output_device_by_name(host: &cpal::Host, name: &str) -> Result<cpal::Device, String> {
    let target = name.trim();
    let devices = host
        .output_devices()
        .map_err(|e| format!("enumerate output devices: {e:?}"))?;
    let mut case_insensitive_match = None;

    for device in devices {
        let device_name = device_name(&device);
        if device_name == target {
            return Ok(device);
        }
        if case_insensitive_match.is_none() && device_name.eq_ignore_ascii_case(target) {
            case_insensitive_match = Some(device);
        }
    }

    case_insensitive_match.ok_or_else(|| format!("output device not found: {target}"))
}

fn output_device_key_and_config(
    host: &cpal::Host,
    device: &cpal::Device,
    default_identity: bool,
) -> (OutputDeviceKey, Option<cpal::SupportedStreamConfig>) {
    let device_name = device_name(device);
    let default_config = match device.default_output_config() {
        Ok(config) => Some(config),
        Err(e) => {
            warn!("default_output_config 失败 (device={device_name}): {e:?}");
            None
        }
    };
    let platform_id = default_identity.then(platform::default_output_id).flatten();
    let device_signature = if default_identity && platform_id.is_none() {
        fallback_output_device_signature(host)
    } else {
        None
    };
    let device_key = OutputDeviceKey {
        platform_id,
        name: device_name,
        default_config: default_config.as_ref().map(OutputConfigKey::from_config),
        device_signature,
    };
    (device_key, default_config)
}

fn fallback_output_device_signature(host: &cpal::Host) -> Option<u64> {
    let devices = host.output_devices().ok()?;
    let mut entries = Vec::new();

    for device in devices {
        let name = device_name(&device);
        let config = device
            .default_output_config()
            .ok()
            .map(|config| OutputConfigKey::from_config(&config));
        entries.push(format!("{name}\0{config:?}"));
    }

    entries.sort_unstable();

    let mut hasher = DefaultHasher::new();
    host.id().name().hash(&mut hasher);
    for entry in entries {
        entry.hash(&mut hasher);
    }
    Some(hasher.finish())
}

fn open_output_stream(
    selector: OutputDeviceSelector,
    target: Option<OutputTarget>,
    data_rx: Consumer<OutputBlock>,
    recycle_tx: Producer<Vec<f32>>,
    paused: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
    callback_alive: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    rendered_samples: Arc<AtomicU64>,
) -> Result<(Stream, OutputDeviceKey, OutputConfigKey), String> {
    let host = cpal::default_host();
    let device = resolve_output_device(&host, &selector)?;
    let (device_key, default_config) =
        output_device_key_and_config(&host, &device, selector.is_default());
    let device_name = device_key.name().to_string();

    #[cfg(target_os = "android")]
    let effective_target = target.or_else(|| {
        Some(OutputTarget {
            channels: 2,
            sample_rate: default_config
                .as_ref()
                .map(|config| config.sample_rate())
                .unwrap_or(48_000),
        })
    });
    #[cfg(not(target_os = "android"))]
    let effective_target = target;

    let supported_config = match effective_target {
        Some(target) => select_output_config(&device, target, default_config.as_ref())
            .or_else(|| default_config.clone())
            .ok_or_else(|| "no supported output config".to_string())?,
        None => default_config
            .clone()
            .or_else(|| select_any_output_config(&device))
            .ok_or_else(|| "no supported output config".to_string())?,
    };
    let config_key = OutputConfigKey::from_config(&supported_config);
    let stream_config = stable_stream_config(&supported_config);

    info!(
        "音频输出：selector={} device={device_name} channels={} rate={} format={:?} buffer={:?} target={effective_target:?}",
        selector.label(),
        stream_config.channels,
        stream_config.sample_rate,
        config_key.sample_format,
        stream_config.buffer_size
    );

    let stream = match config_key.sample_format {
        cpal::SampleFormat::I8 => build_stream::<i8>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::I32 => build_stream::<i32>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::U8 => build_stream::<u8>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::U32 => build_stream::<u32>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        cpal::SampleFormat::F64 => build_stream::<f64>(
            &device,
            &stream_config,
            data_rx,
            recycle_tx,
            paused,
            stream_failed,
            callback_alive,
            volume_bits,
            generation,
            flush_epoch,
            queued_samples,
            rendered_samples,
        ),
        other => Err(format!("unsupported output sample format: {other:?}")),
    }?;
    stream
        .play()
        .map_err(|e| format!("play output stream: {e:?}"))?;

    Ok((stream, device_key, config_key))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    data_rx: Consumer<OutputBlock>,
    recycle_tx: Producer<Vec<f32>>,
    paused: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
    callback_alive: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    rendered_samples: Arc<AtomicU64>,
) -> Result<Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let mut state = CallbackState::new(
        data_rx,
        recycle_tx,
        callback_alive,
        generation.load(Ordering::Acquire),
        flush_epoch.load(Ordering::Acquire),
        config.channels.max(1) as usize,
        PENDING_RECYCLE_BLOCKS,
    );
    let mut reported_recoverable_errors = 0u8;
    let err_fn = move |err| {
        if let Some(error_bit) = recoverable_stream_error_bit(&err) {
            // CPAL reports scheduling fallback, automatic route changes, and
            // individual xruns through the same callback as terminal stream
            // failures. These conditions leave the stream usable; stopping
            // the producer here would turn one recoverable glitch into
            // permanent silence on every backend that emits them.
            // Log each category once: repeated xrun logging from an audio
            // backend thread can itself add pressure during an overload.
            if reported_recoverable_errors & error_bit == 0 {
                reported_recoverable_errors |= error_bit;
                warn!("音频输出流可恢复事件 ({:?}): {err}", err.kind());
            }
        } else if !stream_failed.swap(true, Ordering::AcqRel) {
            warn!("音频输出流错误 ({:?}): {err}", err.kind());
        }
    };

    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _info| {
                fill_output(
                    data,
                    &mut state,
                    &paused,
                    &volume_bits,
                    &generation,
                    &flush_epoch,
                    &queued_samples,
                    &rendered_samples,
                )
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("build output stream: {e:?}"))
}

#[inline]
fn recoverable_stream_error_bit(err: &cpal::Error) -> Option<u8> {
    match err.kind() {
        cpal::ErrorKind::DeviceChanged => Some(1 << 0),
        cpal::ErrorKind::RealtimeDenied => Some(1 << 1),
        cpal::ErrorKind::Xrun => Some(1 << 2),
        _ => None,
    }
}

fn producer_retry_backoff(retry_count: &mut u32) {
    if *retry_count < PRODUCER_YIELD_RETRIES {
        *retry_count += 1;
        thread::yield_now();
        return;
    }

    let shift = (*retry_count - PRODUCER_YIELD_RETRIES).min(6);
    let park_us = (PRODUCER_MIN_PARK_US << shift).min(PRODUCER_MAX_PARK_US);
    *retry_count = (*retry_count).saturating_add(1);
    thread::park_timeout(Duration::from_micros(park_us));
}

fn saturating_sub(counter: &AtomicUsize, amount: usize) {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_writer(
        capacity: usize,
    ) -> (
        OutputWriter,
        Consumer<OutputBlock>,
        Producer<Vec<f32>>,
        Arc<AtomicU64>,
        Arc<AtomicU64>,
        Arc<AtomicUsize>,
        Arc<AtomicBool>,
    ) {
        let (data_tx, data_rx) = RingBuffer::new(capacity);
        let (recycle_tx, recycle_rx) = RingBuffer::new(capacity + 1);
        let generation = Arc::new(AtomicU64::new(7));
        let flush_epoch = Arc::new(AtomicU64::new(0));
        let queued_samples = Arc::new(AtomicUsize::new(42));
        let callback_alive = Arc::new(AtomicBool::new(true));
        let writer = OutputWriter {
            data_tx: Arc::new(Mutex::new(data_tx)),
            paused: Arc::new(AtomicBool::new(false)),
            stream_failed: Arc::new(AtomicBool::new(false)),
            callback_alive: Arc::clone(&callback_alive),
            volume_bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            generation: Arc::clone(&generation),
            flush_epoch: Arc::clone(&flush_epoch),
            queued_samples: Arc::clone(&queued_samples),
            rendered_samples: Arc::new(AtomicU64::new(0)),
            recycle_rx: Arc::new(Mutex::new(recycle_rx)),
        };
        (
            writer,
            data_rx,
            recycle_tx,
            generation,
            flush_epoch,
            queued_samples,
            callback_alive,
        )
    }

    #[test]
    fn flush_does_not_invalidate_output_generation() {
        let (writer, _data_rx, _recycle_tx, generation, flush_epoch, queued_samples, _alive) =
            test_writer(1);

        writer.flush();

        assert_eq!(generation.load(Ordering::Acquire), 7);
        assert_eq!(flush_epoch.load(Ordering::Acquire), 1);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);

        queued_samples.store(13, Ordering::Release);
        writer.clear();

        assert_eq!(generation.load(Ordering::Acquire), 8);
        assert_eq!(flush_epoch.load(Ordering::Acquire), 2);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
    }

    #[test]
    fn push_stops_when_callback_has_gone_away() {
        let (writer, _data_rx, _recycle_tx, _generation, _flush, queued, callback_alive) =
            test_writer(1);
        callback_alive.store(false, Ordering::Release);
        static STOP: AtomicBool = AtomicBool::new(false);

        assert!(!writer.push_block(
            vec![0.25, -0.25],
            PushCancel {
                stop: &STOP,
                interrupt_epoch: None,
            },
        ));
        assert_eq!(queued.load(Ordering::Acquire), 42);
    }

    #[test]
    fn cloned_writers_serialize_the_single_ring_producer() {
        const BLOCKS_PER_THREAD: usize = 16;
        static STOP: AtomicBool = AtomicBool::new(false);
        let (writer, mut data_rx, _recycle_tx, _generation, _flush, queued, _alive) =
            test_writer(BLOCKS_PER_THREAD * 2);
        queued.store(0, Ordering::Release);

        let first = {
            let writer = writer.clone();
            std::thread::spawn(move || {
                for value in 0..BLOCKS_PER_THREAD {
                    assert!(writer.push_block(
                        vec![value as f32],
                        PushCancel {
                            stop: &STOP,
                            interrupt_epoch: None,
                        },
                    ));
                }
            })
        };
        let second = std::thread::spawn(move || {
            for value in BLOCKS_PER_THREAD..BLOCKS_PER_THREAD * 2 {
                assert!(writer.push_block(
                    vec![value as f32],
                    PushCancel {
                        stop: &STOP,
                        interrupt_epoch: None,
                    },
                ));
            }
        });

        first.join().unwrap();
        second.join().unwrap();
        let mut values = Vec::with_capacity(BLOCKS_PER_THREAD * 2);
        while let Ok(block) = data_rx.pop() {
            values.push(block.samples[0] as usize);
        }
        values.sort_unstable();
        assert_eq!(values, (0..BLOCKS_PER_THREAD * 2).collect::<Vec<_>>());
        assert_eq!(queued.load(Ordering::Acquire), BLOCKS_PER_THREAD * 2);
    }

    #[test]
    fn selector_from_name_treats_default_aliases_as_system_default() {
        assert_eq!(
            OutputDeviceSelector::from_name(""),
            OutputDeviceSelector::Default
        );
        assert_eq!(
            OutputDeviceSelector::from_name("system"),
            OutputDeviceSelector::Default
        );
        assert_eq!(
            OutputDeviceSelector::from_name("Headphones"),
            OutputDeviceSelector::Named("Headphones".into())
        );
    }

    #[test]
    fn recoverable_stream_events_do_not_require_rebuild() {
        for kind in [
            cpal::ErrorKind::DeviceChanged,
            cpal::ErrorKind::RealtimeDenied,
            cpal::ErrorKind::Xrun,
        ] {
            assert!(recoverable_stream_error_bit(&cpal::Error::new(kind)).is_some());
        }

        assert!(recoverable_stream_error_bit(&cpal::Error::new(
            cpal::ErrorKind::StreamInvalidated,
        ))
        .is_none());
        assert!(recoverable_stream_error_bit(&cpal::Error::new(
            cpal::ErrorKind::DeviceNotAvailable,
        ))
        .is_none());
    }
}
