//! Subscribe to the same PulseAudio protocol CPAL uses, including pipewire-pulse.

use std::io::BufReader;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use pulseaudio::protocol::{self, Command, SubscriptionEventFacility, SubscriptionMask};
use tracing::{info, warn};

use super::{PendingChanges, PlatformBackend, DEFAULT_CHANGED, DEVICES_CHANGED};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Control {
    stopped: AtomicBool,
    socket: Mutex<Option<UnixStream>>,
}

pub(super) struct Backend {
    control: Arc<Control>,
    events: Arc<PendingChanges>,
    thread: Option<JoinHandle<()>>,
}

impl PlatformBackend for Backend {
    fn start() -> Option<Self> {
        let pending = Arc::new(PendingChanges::default());
        // ALSA-only hosts keep the existing identity-only polling fallback.
        pulseaudio::socket_path_from_env()?;
        let control = Arc::new(Control::default());
        let worker_control = Arc::clone(&control);
        let events = Arc::clone(&pending);
        match thread::Builder::new()
            .name("audio-device-watch".into())
            .spawn(move || watch(worker_control, pending))
        {
            Ok(thread) => Some(Self {
                control,
                events,
                thread: Some(thread),
            }),
            Err(error) => {
                warn!("音频设备监听线程创建失败，使用轮询：{error}");
                None
            }
        }
    }

    fn events(&self) -> &PendingChanges {
        &self.events
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.control.stopped.store(true, Ordering::Release);
        if let Some(socket) = self.control.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn watch(control: Arc<Control>, pending: Arc<PendingChanges>) {
    let mut unavailable = false;
    while !control.stopped.load(Ordering::Acquire) {
        let result = (|| {
            let path =
                pulseaudio::socket_path_from_env().context("PulseAudio socket unavailable")?;
            let socket = UnixStream::connect(path)?;
            {
                let mut active = control.socket.lock().unwrap();
                if control.stopped.load(Ordering::Acquire) {
                    return Ok(());
                }
                *active = Some(socket.try_clone()?);
            }
            subscribe(socket, &pending, || {
                if unavailable {
                    info!("音频设备监听已恢复");
                    unavailable = false;
                }
                // Reconcile changes missed while the sound server was unavailable.
                pending.record(DEFAULT_CHANGED | DEVICES_CHANGED);
            })
        })();
        control.socket.lock().unwrap().take();
        if control.stopped.load(Ordering::Acquire) {
            break;
        }
        if let Err(error) = result {
            if !unavailable {
                warn!("音频设备监听中断，使用轮询并等待重连：{error:#}");
                unavailable = true;
            }
        }
        thread::park_timeout(RECONNECT_DELAY);
    }
}

fn subscribe(socket: UnixStream, pending: &PendingChanges, ready: impl FnOnce()) -> Result<()> {
    socket.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    socket.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let mut socket = BufReader::new(socket);
    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    protocol::write_command_message(
        socket.get_mut(),
        0,
        &Command::Auth(protocol::AuthParams {
            version: protocol::MAX_VERSION,
            supports_shm: false,
            supports_memfd: false,
            cookie,
        }),
        protocol::MAX_VERSION,
    )?;
    let (sequence, auth) =
        protocol::read_reply_message::<protocol::AuthReply>(&mut socket, protocol::MAX_VERSION)?;
    if sequence != 0 {
        bail!("unexpected authentication reply sequence: {sequence}");
    }
    let version = auth.version.min(protocol::MAX_VERSION);
    let mut props = protocol::Props::new();
    props.set(protocol::Prop::ApplicationName, c"GMPlayer device watcher");
    protocol::write_command_message(socket.get_mut(), 1, &Command::SetClientName(props), version)?;
    let (sequence, _) =
        protocol::read_reply_message::<protocol::SetClientNameReply>(&mut socket, version)?;
    if sequence != 1 {
        bail!("unexpected client-name reply sequence: {sequence}");
    }
    protocol::write_command_message(
        socket.get_mut(),
        2,
        &Command::Subscribe(SubscriptionMask::SERVER | SubscriptionMask::SINK),
        version,
    )?;
    if protocol::read_ack_message(&mut socket)? != 2 {
        bail!("unexpected subscription acknowledgement sequence");
    }
    // Idle listening blocks in the kernel; shutdown() wakes it on player teardown.
    socket.get_ref().set_read_timeout(None)?;
    ready();
    loop {
        let (_, command) = protocol::read_command_message(&mut socket, version)?;
        if let Command::SubscribeEvent(event) = command {
            record_event(pending, event.event_facility);
        }
    }
}

fn record_event(pending: &PendingChanges, facility: SubscriptionEventFacility) {
    match facility {
        SubscriptionEventFacility::Server => pending.record(DEFAULT_CHANGED),
        SubscriptionEventFacility::Sink => pending.record(DEVICES_CHANGED),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
