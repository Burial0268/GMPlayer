use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::warn;

use super::bus::EventBus;
use super::clock::normalize_seek_position;
use super::AudioPlayer;
use crate::error::{AudioError, AudioResult};
use crate::types::*;

/// The running player: one thread, one command queue. Events leave through
/// the [`EventBus`] it was built with, which outlives it.
pub struct Player {
    msg_tx: mpsc::UnboundedSender<CommandEnvelope>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Player {
    pub fn new(bus: Arc<EventBus>) -> AudioResult<Self> {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        // Spawn the audio player on a dedicated thread with its own tokio runtime.
        // `AudioPlayer` owns the live `OutputStream` so it can reopen the stream
        // with a source-aware channel count when tracks change.
        let thread = spawn_initialised(
            "audio-player",
            PLAYER_INIT_TIMEOUT,
            move || AudioPlayer::new(msg_rx, bus),
            AudioPlayer::run,
        )?;
        Ok(Player {
            msg_tx,
            thread: Some(thread),
        })
    }

    /// Whether the player loop is still running. It ends on its own only when it cannot go on
    /// (its runtime is gone); a dead player answers every message with "channel closed".
    pub fn is_alive(&self) -> bool {
        !self.msg_tx.is_closed()
    }

    /// Queue a command. Seeks share the queue with everything else, so the
    /// player takes commands up in the order they were sent — the order the
    /// `ack` on every event is measured against. A run of seeks is coalesced
    /// to its last one when the player gets to it (see `AudioPlayer::run`).
    pub fn send_msg(&self, msg: CommandEnvelope) -> AudioResult<()> {
        self.msg_tx
            .send(msg)
            .map_err(|_| AudioError::ThreadError("player channel closed".into()))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self
            .msg_tx
            .send(CommandEnvelope::internal(AudioThreadMessage::Close));
        if let Some(thread) = self.thread.take() {
            join_thread_async("audio-player-join", thread);
        }
    }
}

/// Opening the output can wait on the platform (an Android AAudio stream, a WASAPI endpoint
/// waking up); past this the player is reported as not started rather than left half-made.
const PLAYER_INIT_TIMEOUT: Duration = Duration::from_secs(20);

/// Start `init` on its own thread and runtime, and return only once it has come up.
///
/// The player used to be spawned and handed back at once, so an output that would not open
/// (nothing plugged in, a device mid-switch) ended the thread after `Player::new` had already
/// answered `Ok`: the caller kept a player whose every message failed with "channel closed", until
/// the app restarted. Waiting here turns that into an error the caller can retry. On a timeout the
/// thread is left to finish on its own; whatever it built then finds its channels closed and exits.
fn spawn_initialised<T, F, G>(
    name: &str,
    timeout: Duration,
    init: impl FnOnce() -> F + Send + 'static,
    run: impl FnOnce(T) -> G + Send + 'static,
) -> AudioResult<thread::JoinHandle<()>>
where
    F: std::future::Future<Output = AudioResult<T>>,
    G: std::future::Future<Output = ()>,
{
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<AudioResult<()>>(1);
    let thread_name = name.to_string();
    let thread = thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(AudioError::ThreadError(format!(
                        "build {thread_name} runtime: {e}"
                    ))));
                    return;
                }
            };
            rt.block_on(async move {
                match init().await {
                    Ok(value) => {
                        let _ = ready_tx.send(Ok(()));
                        run(value).await;
                    }
                    Err(e) => {
                        warn!("{thread_name} failed to start: {e}");
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });
        })
        .map_err(|e| AudioError::ThreadError(format!("spawn {name} thread: {e}")))?;
    match ready_rx.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(e)) => Err(e),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(AudioError::ThreadError(format!(
            "{name} did not start within {timeout:?}"
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::ThreadError(
            format!("{name} thread ended while starting"),
        )),
    }
}

pub(super) fn join_thread_async(name: &'static str, handle: thread::JoinHandle<()>) {
    let _ = thread::Builder::new().name(name.into()).spawn(move || {
        let _ = handle.join();
    });
}

#[derive(Clone, Debug)]
pub(super) struct SeekRequest {
    pub(super) position: f64,
    pub(super) request_id: Option<u64>,
    pub(super) expected_music_id: Option<String>,
}

impl SeekRequest {
    pub(super) fn new(
        position: f64,
        request_id: Option<u64>,
        expected_music_id: Option<String>,
    ) -> Self {
        Self {
            position,
            request_id,
            expected_music_id,
        }
    }

    pub(super) fn normalized(self) -> Self {
        Self {
            position: normalize_seek_position(self.position),
            request_id: self.request_id,
            expected_music_id: self.expected_music_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread whose setup fails must say so to whoever started it, not hand back a handle to
    /// something that has already exited.
    #[test]
    fn a_thread_that_cannot_start_reports_it_instead_of_returning_a_dead_handle() {
        let failed = spawn_initialised(
            "init-fails",
            Duration::from_secs(5),
            || async { Err::<(), _>(AudioError::Output("no device".into())) },
            |_| async {},
        );
        assert!(matches!(failed, Err(AudioError::Output(_))));

        let (ran_tx, ran_rx) = std::sync::mpsc::channel();
        let started = spawn_initialised(
            "init-succeeds",
            Duration::from_secs(5),
            || async { Ok(7) },
            move |value| async move {
                let _ = ran_tx.send(value);
            },
        )
        .expect("starts");
        assert_eq!(ran_rx.recv_timeout(Duration::from_secs(5)), Ok(7));
        started.join().unwrap();

        let slow = spawn_initialised(
            "init-hangs",
            Duration::from_millis(20),
            || async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(())
            },
            |_| async {},
        );
        assert!(matches!(slow, Err(AudioError::ThreadError(_))));
    }
}
