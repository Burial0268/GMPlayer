//! CoreAudio property coverage follows SPlayer-Next; see ../../../UPSTREAM.md.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle, Thread};

use coreaudio::sys::{
    kAudioDevicePropertyDataSource, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyJackIsConnected, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyScopeOutput, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreams, kAudioHardwareNoError, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectAddPropertyListener,
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
};
use tracing::{debug, warn};

use super::{PendingChanges, DEFAULT_CHANGED, DEVICES_CHANGED};

struct WatchState {
    pending: Arc<PendingChanges>,
    refresh_devices: AtomicBool,
    stopped: AtomicBool,
    worker: OnceLock<Thread>,
}

pub(super) struct Backend {
    state: Arc<WatchState>,
    thread: Option<JoinHandle<()>>,
}

impl Backend {
    pub(super) fn start(pending: Arc<PendingChanges>) -> Option<Self> {
        let state = Arc::new(WatchState {
            pending,
            refresh_devices: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            worker: OnceLock::new(),
        });
        let worker_state = Arc::clone(&state);
        match thread::Builder::new()
            .name("audio-device-watch".into())
            .spawn(move || watch(worker_state))
        {
            Ok(thread) => Some(Self {
                state,
                thread: Some(thread),
            }),
            Err(error) => {
                warn!("CoreAudio 设备监听线程创建失败，使用轮询：{error}");
                None
            }
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

struct CallbackContext(usize);

fn contexts() -> &'static Mutex<HashMap<usize, Arc<WatchState>>> {
    static CONTEXTS: OnceLock<Mutex<HashMap<usize, Arc<WatchState>>>> = OnceLock::new();
    CONTEXTS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl CallbackContext {
    fn new(state: Arc<WatchState>) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        contexts().lock().unwrap().insert(id, state);
        Self(id)
    }
}

impl Drop for CallbackContext {
    fn drop(&mut self) {
        contexts().lock().unwrap().remove(&self.0);
    }
}

fn watch(state: Arc<WatchState>) {
    let _ = state.worker.set(thread::current());
    let context = CallbackContext::new(Arc::clone(&state));
    let mut system = Vec::new();
    for selector in [
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioHardwarePropertyDevices,
    ] {
        match Registration::new(
            kAudioObjectSystemObject,
            selector,
            kAudioObjectPropertyScopeGlobal,
            context.0,
        ) {
            Ok(registration) => system.push(registration),
            Err(status) => {
                warn!("CoreAudio 系统属性监听注册失败，使用轮询：{status}");
                return;
            }
        }
    }
    state.pending.record(DEFAULT_CHANGED | DEVICES_CHANGED);
    let mut devices = HashMap::new();
    while !state.stopped.load(Ordering::Acquire) {
        if state.refresh_devices.swap(false, Ordering::AcqRel) {
            refresh_devices(&mut devices, context.0);
        }
        thread::park();
    }
    // Retire the id before unregistering so late callbacks become no-ops.
    drop(context);
    drop(devices);
    drop(system);
}

struct Registration {
    object: AudioObjectID,
    address: AudioObjectPropertyAddress,
    context: usize,
}

impl Registration {
    fn new(object: AudioObjectID, selector: u32, scope: u32, context: usize) -> Result<Self, i32> {
        let address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let status = unsafe {
            AudioObjectAddPropertyListener(
                object,
                &address,
                Some(property_changed),
                context as *mut c_void,
            )
        };
        if status != kAudioHardwareNoError as i32 {
            return Err(status);
        }
        Ok(Self {
            object,
            address,
            context,
        })
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let status = unsafe {
            AudioObjectRemovePropertyListener(
                self.object,
                &self.address,
                Some(property_changed),
                self.context as *mut c_void,
            )
        };
        if status != kAudioHardwareNoError as i32 {
            warn!("CoreAudio 属性监听移除失败：{status}");
        }
    }
}

unsafe extern "C" fn property_changed(
    _object: AudioObjectID,
    count: u32,
    addresses: *const AudioObjectPropertyAddress,
    context: *mut c_void,
) -> i32 {
    // The context is an id, never a pointer to freed listener data. A callback
    // already in flight keeps its own Arc; one arriving after retirement is inert.
    let state = contexts()
        .lock()
        .ok()
        .and_then(|contexts| contexts.get(&(context as usize)).cloned());
    let Some(state) = state else {
        return kAudioHardwareNoError as i32;
    };
    for index in 0..count as usize {
        let address = unsafe { &*addresses.add(index) };
        match address.mSelector {
            kAudioHardwarePropertyDefaultOutputDevice => state.pending.record(DEFAULT_CHANGED),
            kAudioHardwarePropertyDevices => {
                state.pending.record(DEVICES_CHANGED);
                state.refresh_devices.store(true, Ordering::Release);
                if let Some(worker) = state.worker.get() {
                    worker.unpark();
                }
            }
            _ => state.pending.record(DEVICES_CHANGED),
        }
    }
    kAudioHardwareNoError as i32
}

fn refresh_devices(registrations: &mut HashMap<AudioObjectID, Vec<Registration>>, context: usize) {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut size = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &address, 0, null(), &mut size)
    };
    if status != kAudioHardwareNoError as i32 {
        return;
    }
    let mut devices = vec![0 as AudioObjectID; size as usize / mem::size_of::<AudioObjectID>()];
    if size > 0 {
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                null(),
                &mut size,
                devices.as_mut_ptr().cast(),
            )
        };
        if status != kAudioHardwareNoError as i32 {
            return;
        }
    }
    devices.truncate(size as usize / mem::size_of::<AudioObjectID>());
    let devices: HashSet<_> = devices.into_iter().collect();
    registrations.retain(|device, _| devices.contains(device));
    for device in devices {
        registrations.entry(device).or_insert_with(|| {
            let streams = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyStreams,
                mScope: kAudioDevicePropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMaster,
            };
            let mut size = 0;
            let status =
                unsafe { AudioObjectGetPropertyDataSize(device, &streams, 0, null(), &mut size) };
            if status != kAudioHardwareNoError as i32 || size == 0 {
                return Vec::new();
            }
            [
                (
                    kAudioDevicePropertyDeviceIsAlive,
                    kAudioObjectPropertyScopeGlobal,
                ),
                (
                    kAudioDevicePropertyNominalSampleRate,
                    kAudioObjectPropertyScopeGlobal,
                ),
                (kAudioDevicePropertyStreams, kAudioDevicePropertyScopeOutput),
                (
                    kAudioDevicePropertyStreamConfiguration,
                    kAudioDevicePropertyScopeOutput,
                ),
                (
                    kAudioDevicePropertyDataSource,
                    kAudioDevicePropertyScopeOutput,
                ),
                (
                    kAudioDevicePropertyJackIsConnected,
                    kAudioDevicePropertyScopeOutput,
                ),
            ]
            .into_iter()
            .filter_map(|(selector, scope)| {
                Registration::new(device, selector, scope, context)
                    .map_err(|status| {
                        debug!("CoreAudio 可选设备属性不可监听（{device}/{selector}）：{status}")
                    })
                    .ok()
            })
            .collect()
        });
    }
}

#[cfg(test)]
mod tests;
