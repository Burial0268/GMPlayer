use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, FromSample, Sample, SampleRate, SizedSample, Stream, StreamConfig,
    SupportedBufferSize,
};
use parking_lot::Mutex;
use tracing::{info, warn};

const PREFERRED_RATES: [u32; 2] = [48_000, 44_100];
const PREFERRED_FORMATS: [cpal::SampleFormat; 3] = [
    cpal::SampleFormat::F32,
    cpal::SampleFormat::I16,
    cpal::SampleFormat::U16,
];
#[cfg(target_os = "android")]
const DEFAULT_QUEUE_BLOCKS: usize = 16;
#[cfg(not(target_os = "android"))]
const DEFAULT_QUEUE_BLOCKS: usize = 8;
// Depth of the buffer-recycling channel that returns spent mix blocks from the
// CPAL callback back to the mixer for reuse. Sized a little above the data
// queue so every in-flight buffer has a slot; overflow simply drops the buffer
// (a plain free on the producer side), so the callback never blocks.
const RECYCLE_QUEUE_BLOCKS: usize = DEFAULT_QUEUE_BLOCKS + 4;
const OUTPUT_INIT_TIMEOUT: Duration = Duration::from_secs(4);
#[cfg(target_os = "android")]
const STABLE_OUTPUT_BUFFER_MS: u32 = 40;
#[cfg(not(target_os = "android"))]
const STABLE_OUTPUT_BUFFER_MS: u32 = 20;
#[cfg(target_os = "android")]
const MIN_STABLE_OUTPUT_BUFFER_FRAMES: u32 = 512;
#[cfg(not(target_os = "android"))]
const MIN_STABLE_OUTPUT_BUFFER_FRAMES: u32 = 512;
const UNDERRUN_FADE_FRAMES: usize = 64;
const SEEK_FLUSH_FADE_FRAMES: usize = 256;
const PRODUCER_YIELD_RETRIES: u32 = 8;
const PRODUCER_MIN_PARK_US: u64 = 100;
const PRODUCER_MAX_PARK_US: u64 = 1_000;

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

struct OutputBlock {
    samples: Vec<f32>,
    generation: u64,
    flush_epoch: u64,
}

enum OutputControl {
    Clear { generation: u64, flush_epoch: u64 },
}

#[derive(Clone)]
pub struct OutputWriter {
    data_tx: mpsc::SyncSender<OutputBlock>,
    control_tx: mpsc::Sender<OutputControl>,
    paused: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    rendered_samples: Arc<AtomicU64>,
    /// Spent mix blocks returned by the CPAL callback for reuse. Single
    /// consumer (the mixer) behind a `Mutex` so `OutputWriter` stays `Sync`
    /// while remaining cloneable; the lock is only ever taken off the audio
    /// callback (on the mixer thread), so it never blocks real-time work.
    recycle_rx: Arc<Mutex<mpsc::Receiver<Vec<f32>>>>,
}

#[derive(Clone, Debug)]
pub struct OutputRenderClock {
    rendered_samples: Arc<AtomicU64>,
}

impl OutputRenderClock {
    pub fn rendered_samples(&self) -> u64 {
        self.rendered_samples.load(Ordering::Acquire)
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
            match self.data_tx.try_send(output_block) {
                Ok(()) => {
                    // The CPAL callback subtracts samples as it writes them.
                    // This is only for end-of-track draining, not timing.
                    if cancel.is_cancelled()
                        || self.generation.load(Ordering::Acquire) != generation
                        || self.flush_epoch.load(Ordering::Acquire) != flush_epoch
                        || self.stream_failed.load(Ordering::Acquire)
                    {
                        saturating_sub(&self.queued_samples, block_len);
                        return false;
                    }
                    return true;
                }
                Err(mpsc::TrySendError::Full(returned)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    block = returned.samples;
                    producer_retry_backoff(&mut retry_count);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    saturating_sub(&self.queued_samples, block_len);
                    return false;
                }
            }
        }
    }

    pub fn flush(&self) {
        let generation = self.generation.load(Ordering::Acquire);
        let flush_epoch = self.flush_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.queued_samples.store(0, Ordering::Release);
        let _ = self.control_tx.send(OutputControl::Clear {
            generation,
            flush_epoch,
        });
    }

    pub fn clear(&self) {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let flush_epoch = self.flush_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.queued_samples.store(0, Ordering::Release);
        let _ = self.control_tx.send(OutputControl::Clear {
            generation,
            flush_epoch,
        });
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
        self.queued_samples.load(Ordering::Acquire)
    }

    /// Reuse a spent mix block returned by the CPAL callback, or allocate a
    /// fresh one when the pool is empty. Called only from the mixer thread, so
    /// the `try_lock` is effectively uncontended and never touches the audio
    /// callback. The returned buffer is empty (`len == 0`) with at least
    /// `capacity` capacity, ready to be filled by pushing.
    pub fn take_recycled_buffer(&self, capacity: usize) -> Vec<f32> {
        if let Some(rx) = self.recycle_rx.try_lock() {
            if let Ok(mut buf) = rx.try_recv() {
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
    let (data_tx, data_rx) = mpsc::sync_channel::<OutputBlock>(DEFAULT_QUEUE_BLOCKS);
    let (control_tx, control_rx) = mpsc::channel::<OutputControl>();
    let (recycle_tx, recycle_rx) = mpsc::sync_channel::<Vec<f32>>(RECYCLE_QUEUE_BLOCKS);
    let (stream_stop_tx, stream_stop_rx) = mpsc::channel::<()>();
    let (init_tx, init_rx) = mpsc::channel::<Result<(OutputDeviceKey, OutputConfigKey), String>>();
    let paused = Arc::new(AtomicBool::new(true));
    let stream_failed = Arc::new(AtomicBool::new(false));
    let volume_bits = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let generation = Arc::new(AtomicU64::new(0));
    let flush_epoch = Arc::new(AtomicU64::new(0));
    let queued_samples = Arc::new(AtomicUsize::new(0));
    let rendered_samples = Arc::new(AtomicU64::new(0));
    let stream_selector = selector.clone();

    let writer = OutputWriter {
        data_tx,
        control_tx,
        paused: Arc::clone(&paused),
        stream_failed: Arc::clone(&stream_failed),
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
                control_rx,
                recycle_tx,
                paused,
                stream_failed,
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

fn find_output_device_by_name(host: &cpal::Host, name: &str) -> Result<cpal::Device, String> {
    let target = name.trim();
    let devices = host
        .output_devices()
        .map_err(|e| format!("enumerate output devices: {e:?}"))?;
    let mut case_insensitive_match = None;

    for device in devices {
        let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
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
    let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
    let default_config = match device.default_output_config() {
        Ok(config) => Some(config),
        Err(e) => {
            warn!("default_output_config 失败 (device={device_name}): {e:?}");
            None
        }
    };
    let platform_id = default_identity.then(platform_default_output_id).flatten();
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
        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
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

#[cfg(target_os = "windows")]
fn platform_default_output_id() -> Option<String> {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED,
    };

    struct ComGuard(bool);

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct IdGuard(windows::core::PWSTR);

    impl Drop for IdGuard {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(Some(self.0.as_ptr() as *const _));
            }
        }
    }

    let init_result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if !init_result.is_ok() && init_result != RPC_E_CHANGED_MODE {
        return None;
    }
    let _com_guard = ComGuard(init_result.is_ok());

    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()? };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()? };
    let id = IdGuard(unsafe { device.GetId().ok()? });
    let id = unsafe { id.0.to_string().ok()? };

    Some(format!("wasapi:{id}"))
}

#[cfg(target_os = "macos")]
fn platform_default_output_id() -> Option<String> {
    use coreaudio::sys::{
        kAudioHardwareNoError, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal,
        kAudioObjectSystemObject, AudioDeviceID, AudioObjectGetPropertyData,
        AudioObjectPropertyAddress,
    };
    use std::mem;
    use std::ptr::null;

    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut audio_device_id: AudioDeviceID = 0;
    let mut data_size = mem::size_of::<AudioDeviceID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &property_address as *const _,
            0,
            null(),
            &mut data_size as *mut _,
            &mut audio_device_id as *mut _ as *mut _,
        )
    };
    if status != kAudioHardwareNoError as i32 || audio_device_id == 0 {
        return None;
    }

    Some(format!("coreaudio:{audio_device_id}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_default_output_id() -> Option<String> {
    None
}

fn open_output_stream(
    selector: OutputDeviceSelector,
    target: Option<OutputTarget>,
    data_rx: mpsc::Receiver<OutputBlock>,
    control_rx: mpsc::Receiver<OutputControl>,
    recycle_tx: mpsc::SyncSender<Vec<f32>>,
    paused: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
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
                .map(|config| config.sample_rate().0)
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
        stream_config.sample_rate.0,
        config_key.sample_format,
        stream_config.buffer_size
    );

    let stream = match config_key.sample_format {
        cpal::SampleFormat::I8 => build_stream::<i8>(
            &device,
            &stream_config,
            data_rx,
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
            control_rx,
            recycle_tx,
            paused,
            stream_failed,
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
    data_rx: mpsc::Receiver<OutputBlock>,
    control_rx: mpsc::Receiver<OutputControl>,
    recycle_tx: mpsc::SyncSender<Vec<f32>>,
    paused: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
    volume_bits: Arc<AtomicU32>,
    generation: Arc<AtomicU64>,
    flush_epoch: Arc<AtomicU64>,
    queued_samples: Arc<AtomicUsize>,
    rendered_samples: Arc<AtomicU64>,
) -> Result<Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let mut state = CallbackState {
        data_rx,
        control_rx,
        recycle_tx,
        current_block: Vec::new(),
        current_index: 0,
        accepted_generation: generation.load(Ordering::Acquire),
        accepted_flush_epoch: flush_epoch.load(Ordering::Acquire),
        output_channels: config.channels.max(1) as usize,
        last_values: vec![0.0; config.channels.max(1) as usize],
        underrun_fade_remaining: vec![0; config.channels.max(1) as usize],
        flush_fade_frames_remaining: 0,
    };
    let err_fn = move |err| {
        if !stream_failed.swap(true, Ordering::AcqRel) {
            warn!("音频输出流错误: {err}");
        }
    };

    device
        .build_output_stream(
            config,
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

struct CallbackState {
    data_rx: mpsc::Receiver<OutputBlock>,
    control_rx: mpsc::Receiver<OutputControl>,
    /// Returns spent mix blocks to the mixer for reuse instead of freeing them
    /// inside the real-time callback. Best-effort: a full channel just drops
    /// the buffer, so this never blocks.
    recycle_tx: mpsc::SyncSender<Vec<f32>>,
    current_block: Vec<f32>,
    current_index: usize,
    accepted_generation: u64,
    accepted_flush_epoch: u64,
    output_channels: usize,
    last_values: Vec<f32>,
    underrun_fade_remaining: Vec<usize>,
    flush_fade_frames_remaining: usize,
}

fn fill_output<T>(
    data: &mut [T],
    state: &mut CallbackState,
    paused: &AtomicBool,
    volume_bits: &AtomicU32,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
    queued_samples: &AtomicUsize,
    rendered_samples: &AtomicU64,
) where
    T: Sample + FromSample<f32>,
{
    while let Ok(control) = state.control_rx.try_recv() {
        match control {
            OutputControl::Clear {
                generation,
                flush_epoch,
            } => apply_output_clear(state, generation, flush_epoch),
        }
    }
    sync_output_barrier(state, generation, flush_epoch);

    if paused.load(Ordering::Acquire) {
        data.fill(T::from_sample(0.0));
        state.last_values.fill(0.0);
        state.underrun_fade_remaining.fill(0);
        state.flush_fade_frames_remaining = 0;
        return;
    }

    let volume = f32::from_bits(volume_bits.load(Ordering::Acquire));
    let channels = state.output_channels;
    let total = data.len();
    let mut pos = fill_flush_fade(data, state, channels);
    let mut queued_written = 0usize;

    // Bulk-copy frame-aligned runs straight from the queued block, applying
    // volume + clamp over a contiguous slice. This lets the conversion loop
    // autovectorize (and collapses to a tight copy on the common f32 device
    // path) instead of branching, taking a modulo, and touching the fade
    // bookkeeping on every single sample. Both cpal buffers and producer
    // blocks are whole frames, so every run is a multiple of `channels` and
    // frame alignment is preserved across block boundaries.
    while pos < total {
        if sync_output_barrier(state, generation, flush_epoch) {
            pos += fill_flush_fade(&mut data[pos..], state, channels);
            if pos >= total {
                break;
            }
        }

        if state.current_index >= state.current_block.len() {
            match next_current_block(state, generation, flush_epoch) {
                Some(block) => {
                    // Return the spent block to the mixer's pool instead of
                    // dropping (freeing) it inside the real-time callback.
                    let spent = std::mem::replace(&mut state.current_block, block);
                    recycle_buffer(&state.recycle_tx, spent);
                    state.current_index = 0;
                }
                None => break,
            }
        }

        let start = state.current_index;
        let avail = state.current_block.len() - start;
        let run = avail.min(total - pos);
        let src = &state.current_block[start..start + run];
        let dst = &mut data[pos..pos + run];
        if volume == 1.0 {
            for (slot, &raw) in dst.iter_mut().zip(src) {
                *slot = T::from_sample(raw.clamp(-1.0, 1.0));
            }
        } else {
            for (slot, &raw) in dst.iter_mut().zip(src) {
                *slot = T::from_sample((raw * volume).clamp(-1.0, 1.0));
            }
        }
        state.current_index += run;
        pos += run;
        queued_written += run;

        // Remember the most recent frame so an underrun can fade from it.
        // `run` is frame-aligned, so the final `channels` samples are exactly
        // one frame in channel order.
        let frame_start = state.current_index - channels;
        for ch in 0..channels {
            let raw = state.current_block[frame_start + ch];
            state.last_values[ch] = (raw * volume).clamp(-1.0, 1.0);
        }
    }

    let written = queued_written;
    if written > 0 {
        // Every channel just received audio, so arm the full anti-click fade
        // window once per callback instead of once per sample.
        state.underrun_fade_remaining.fill(UNDERRUN_FADE_FRAMES);
    }

    // Any tail the queue could not fill decays from the last frame to avoid a click.
    if pos < total {
        for sample_index in pos..total {
            let channel = sample_index % channels;
            let value = underrun_fade_sample(state, channel);
            data[sample_index] = T::from_sample(value);
        }
    }

    if written > 0 {
        saturating_sub(queued_samples, written);
        rendered_samples.fetch_add(written as u64, Ordering::AcqRel);
    }
}

fn next_current_block(
    state: &mut CallbackState,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
) -> Option<Vec<f32>> {
    loop {
        match state.data_rx.try_recv() {
            Ok(block) => {
                let current_generation = generation.load(Ordering::Acquire);
                let current_flush_epoch = flush_epoch.load(Ordering::Acquire);
                if block.generation != current_generation
                    || block.flush_epoch != current_flush_epoch
                {
                    // Stale block from a superseded generation — recycle its
                    // buffer instead of freeing it in the callback.
                    recycle_buffer(&state.recycle_tx, block.samples);
                    continue;
                }
                if block.samples.is_empty() {
                    recycle_buffer(&state.recycle_tx, block.samples);
                    continue;
                }
                state.accepted_generation = block.generation;
                state.accepted_flush_epoch = block.flush_epoch;
                return Some(block.samples);
            }
            Err(_) => return None,
        }
    }
}

fn sync_output_barrier(
    state: &mut CallbackState,
    generation: &AtomicU64,
    flush_epoch: &AtomicU64,
) -> bool {
    let current_generation = generation.load(Ordering::Acquire);
    let current_flush_epoch = flush_epoch.load(Ordering::Acquire);
    if current_generation == state.accepted_generation
        && current_flush_epoch == state.accepted_flush_epoch
    {
        return false;
    }

    apply_output_clear(state, current_generation, current_flush_epoch);
    true
}

fn apply_output_clear(state: &mut CallbackState, generation: u64, flush_epoch: u64) {
    state.accepted_generation = generation;
    state.accepted_flush_epoch = flush_epoch;
    state.current_block.clear();
    state.current_index = 0;
    state.underrun_fade_remaining.fill(0);
    state.flush_fade_frames_remaining = SEEK_FLUSH_FADE_FRAMES;
}

#[inline]
fn fill_flush_fade<T>(data: &mut [T], state: &mut CallbackState, channels: usize) -> usize
where
    T: Sample + FromSample<f32>,
{
    if state.flush_fade_frames_remaining == 0 {
        return 0;
    }

    let channels = channels.max(1);
    let frames = (data.len() / channels).min(state.flush_fade_frames_remaining);
    let mut pos = 0usize;

    for _ in 0..frames {
        let scale = state.flush_fade_frames_remaining as f32 / SEEK_FLUSH_FADE_FRAMES as f32;
        for ch in 0..channels {
            data[pos + ch] = T::from_sample((state.last_values[ch] * scale).clamp(-1.0, 1.0));
        }
        pos += channels;
        state.flush_fade_frames_remaining -= 1;
    }

    if state.flush_fade_frames_remaining == 0 {
        state.last_values.fill(0.0);
    }

    pos
}

#[inline]
fn underrun_fade_sample(state: &mut CallbackState, channel: usize) -> f32 {
    let remaining = state.underrun_fade_remaining[channel];
    if remaining == 0 {
        return 0.0;
    }

    let scale = remaining as f32 / UNDERRUN_FADE_FRAMES as f32;
    let value = state.last_values[channel] * scale;
    let next_remaining = remaining - 1;
    state.underrun_fade_remaining[channel] = next_remaining;
    if next_remaining == 0 {
        state.last_values[channel] = 0.0;
    }
    value
}

#[inline]
fn recycle_buffer(recycle_tx: &mpsc::SyncSender<Vec<f32>>, buf: Vec<f32>) {
    // Best-effort hand-back to the mixer's pool; a full channel just drops the
    // buffer (a normal free), so the real-time callback never blocks.
    let _ = recycle_tx.try_send(buf);
}

fn producer_retry_backoff(retry_count: &mut u32) {
    if *retry_count < PRODUCER_YIELD_RETRIES {
        *retry_count += 1;
        thread::yield_now();
        return;
    }

    let shift = (*retry_count - PRODUCER_YIELD_RETRIES).min(4);
    let park_us = (PRODUCER_MIN_PARK_US << shift).min(PRODUCER_MAX_PARK_US);
    *retry_count = (*retry_count).saturating_add(1);
    thread::park_timeout(Duration::from_micros(park_us));
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

#[cfg_attr(target_os = "android", allow(dead_code))]
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

fn stable_stream_config(supported_config: &cpal::SupportedStreamConfig) -> StreamConfig {
    let mut config = supported_config.config();
    config.buffer_size = stable_buffer_size(config.sample_rate.0, supported_config.buffer_size());
    config
}

fn stable_buffer_size(sample_rate: u32, supported: &SupportedBufferSize) -> BufferSize {
    let target_frames =
        ((sample_rate.max(1) as u64 * STABLE_OUTPUT_BUFFER_MS as u64) / 1_000) as u32;
    let target_frames = target_frames.max(MIN_STABLE_OUTPUT_BUFFER_FRAMES);
    match supported {
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(target_frames.clamp(*min, *max))
        }
        #[cfg(target_os = "android")]
        SupportedBufferSize::Unknown => BufferSize::Fixed(target_frames),
        #[cfg(not(target_os = "android"))]
        SupportedBufferSize::Unknown => BufferSize::Default,
    }
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
    if let Some(default_rate) = default_rate {
        if rate_supported(range, default_rate) {
            let score = if default_rate == target_rate {
                0
            } else {
                10 + default_rate.abs_diff(target_rate) / 100
            };
            return (SampleRate(default_rate), score);
        }
    }

    if rate_supported(range, target_rate) {
        return (SampleRate(target_rate), 50);
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

    #[test]
    fn stable_buffer_size_uses_supported_range() {
        let buffer = stable_buffer_size(
            48_000,
            &SupportedBufferSize::Range {
                min: 128,
                max: 2_048,
            },
        );

        assert_eq!(buffer, BufferSize::Fixed(960));
    }

    #[test]
    fn stable_buffer_size_clamps_to_supported_range() {
        let buffer = stable_buffer_size(44_100, &SupportedBufferSize::Range { min: 128, max: 512 });

        assert_eq!(buffer, BufferSize::Fixed(512));
    }

    #[test]
    fn flush_does_not_invalidate_output_generation() {
        let (data_tx, _data_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::channel();
        let (_recycle_tx, recycle_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let generation = Arc::new(AtomicU64::new(7));
        let queued_samples = Arc::new(AtomicUsize::new(42));
        let writer = OutputWriter {
            data_tx,
            control_tx,
            paused: Arc::new(AtomicBool::new(false)),
            stream_failed: Arc::new(AtomicBool::new(false)),
            volume_bits: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            generation: Arc::clone(&generation),
            flush_epoch: Arc::new(AtomicU64::new(0)),
            queued_samples: Arc::clone(&queued_samples),
            rendered_samples: Arc::new(AtomicU64::new(0)),
            recycle_rx: Arc::new(Mutex::new(recycle_rx)),
        };

        writer.flush();

        assert_eq!(generation.load(Ordering::Acquire), 7);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
        assert!(matches!(
            control_rx.try_recv(),
            Ok(OutputControl::Clear {
                generation: 7,
                flush_epoch: 1,
            })
        ));

        queued_samples.store(13, Ordering::Release);
        writer.clear();

        assert_eq!(generation.load(Ordering::Acquire), 8);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
        assert!(matches!(
            control_rx.try_recv(),
            Ok(OutputControl::Clear {
                generation: 8,
                flush_epoch: 2,
            })
        ));
    }

    #[test]
    fn clear_output_fades_from_last_frame() {
        let (_data_tx, data_rx) = mpsc::sync_channel(1);
        let (control_tx, control_rx) = mpsc::channel();
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(1);
        let queued_samples = AtomicUsize::new(0);
        let rendered_samples = AtomicU64::new(0);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let mut state = CallbackState {
            data_rx,
            control_rx,
            recycle_tx,
            current_block: Vec::new(),
            current_index: 0,
            accepted_generation: 0,
            accepted_flush_epoch: 0,
            output_channels: 2,
            last_values: vec![0.75, -0.5],
            underrun_fade_remaining: vec![0; 2],
            flush_fade_frames_remaining: 0,
        };
        let mut data = vec![0.0f32; 8];

        control_tx
            .send(OutputControl::Clear {
                generation: 0,
                flush_epoch: 1,
            })
            .unwrap();
        fill_output(
            &mut data,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );

        assert!(data[0] > 0.7);
        assert!(data[1] < -0.45);
        assert!(data[2].abs() < data[0].abs());
        assert!(data[3].abs() < data[1].abs());
        assert_eq!(rendered_samples.load(Ordering::Acquire), 0);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
    }

    #[test]
    fn stale_blocks_after_flush_are_not_rendered() {
        let (data_tx, data_rx) = mpsc::sync_channel(4);
        let (_control_tx, control_rx) = mpsc::channel();
        let paused = AtomicBool::new(false);
        let volume_bits = AtomicU32::new(1.0f32.to_bits());
        let generation = AtomicU64::new(0);
        let flush_epoch = AtomicU64::new(1);
        let queued_samples = AtomicUsize::new(4);
        let rendered_samples = AtomicU64::new(0);
        let (recycle_tx, _recycle_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let mut state = CallbackState {
            data_rx,
            control_rx,
            recycle_tx,
            current_block: Vec::new(),
            current_index: 0,
            accepted_generation: 0,
            accepted_flush_epoch: 1,
            output_channels: 2,
            last_values: vec![0.0; 2],
            underrun_fade_remaining: vec![0; 2],
            flush_fade_frames_remaining: 0,
        };
        data_tx
            .send(OutputBlock {
                samples: vec![0.9, 0.9, 0.9, 0.9],
                generation: 0,
                flush_epoch: 0,
            })
            .unwrap();
        data_tx
            .send(OutputBlock {
                samples: vec![0.25, -0.25, 0.5, -0.5],
                generation: 0,
                flush_epoch: 1,
            })
            .unwrap();
        let mut data = vec![0.0f32; 4];

        fill_output(
            &mut data,
            &mut state,
            &paused,
            &volume_bits,
            &generation,
            &flush_epoch,
            &queued_samples,
            &rendered_samples,
        );

        assert_eq!(data, vec![0.25, -0.25, 0.5, -0.5]);
        assert_eq!(rendered_samples.load(Ordering::Acquire), 4);
        assert_eq!(queued_samples.load(Ordering::Acquire), 0);
    }

    #[test]
    fn select_sample_rate_prefers_default_mix_rate_over_source_rate() {
        let range = cpal::SupportedStreamConfigRange::new(
            2,
            SampleRate(44_100),
            SampleRate(48_000),
            SupportedBufferSize::Unknown,
            cpal::SampleFormat::F32,
        );

        let (sample_rate, score) = select_sample_rate(&range, 44_100, Some(48_000));

        assert_eq!(sample_rate, SampleRate(48_000));
        assert!(score > 0);
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
}
