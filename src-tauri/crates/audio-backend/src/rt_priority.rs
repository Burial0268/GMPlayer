//! Scheduling priority for the audio producer threads.
//!
//! CPAL's `realtime` feature promotes the *device callback* thread, but the
//! callback only copies out of a ring that two ordinary threads have to keep
//! full: `audio-decode` and `audio-deck-mixer`. Left at default priority, a busy
//! machine — and this app animates a WebView2 surface continuously while playing
//! — can deschedule them for longer than the queue holds, and the callback then
//! fades to silence.
//!
//! The promotion itself is delegated to `audio_thread_priority`, which CPAL
//! already pulls in for its own callback thread. It is Mozilla's, drives
//! Firefox's audio stack, and does considerably better per platform than
//! anything worth hand-rolling here: MMCSS on Windows, a mach
//! `THREAD_TIME_CONSTRAINT_POLICY` (real deadline scheduling) on macOS, a
//! dedicated Android backend, and RealtimeKit over D-Bus on Linux.
//!
//! **Linux caveat:** that D-Bus backend is behind the crate's `with_dbus`
//! feature, which CPAL disables (`default-features = false`). Without it,
//! `audio_thread_priority` compiles to a no-op on Linux desktop — see the
//! `cfg_if` chain at the top of its `lib.rs`. Turning it on means enabling
//! `cpal/realtime-dbus` plus `audio_thread_priority/with_dbus` and accepting a
//! libdbus build dependency on Linux, which is the same packaging trade-off the
//! CPAL dependency comment already weighs for PipeWire. Until that call is made,
//! Linux threads stay at default priority.
//!
//! Every promotion is best-effort: a refusal (no privileges, no MMCSS, a
//! sandbox) must never fail playback. This module is native-only; the WASM
//! backend has no threads to promote.

use audio_thread_priority::{
    demote_current_thread_from_real_time, promote_current_thread_to_real_time, RtPriorityHandle,
};
use tracing::debug;

/// Kind of audio worker being promoted, for logging only.
#[derive(Clone, Copy, Debug)]
pub(crate) enum AudioThreadKind {
    Decode,
    Mixer,
}

impl AudioThreadKind {
    fn name(self) -> &'static str {
        match self {
            Self::Decode => "audio-decode",
            Self::Mixer => "audio-deck-mixer",
        }
    }
}

/// RAII handle for a thread's elevated scheduling class. Dropping it restores
/// the previous class.
pub(crate) struct AudioThreadPriority {
    kind: AudioThreadKind,
    handle: Option<RtPriorityHandle>,
}

/// Promote the calling thread. Hold the returned guard for the thread's
/// lifetime; dropping it reverts the promotion.
///
/// `block_frames` and `sample_rate` describe the thread's actual work period —
/// both workers render a fixed-size block per wake — and are what the macOS
/// backend turns into a scheduling deadline. Passing the real values matters
/// there: the period is `block_frames / sample_rate` and the computation budget
/// is derived from it, so a fabricated period would ask the kernel for the wrong
/// guarantee.
pub(crate) fn promote_current_thread(
    kind: AudioThreadKind,
    block_frames: usize,
    sample_rate: u32,
) -> AudioThreadPriority {
    let block_frames = u32::try_from(block_frames).unwrap_or(0);
    match promote_current_thread_to_real_time(block_frames, sample_rate.max(1)) {
        Ok(handle) => {
            debug!(
                "{}: 已提升调度优先级 (block={block_frames} frames @ {sample_rate} Hz)",
                kind.name()
            );
            AudioThreadPriority {
                kind,
                handle: Some(handle),
            }
        }
        Err(err) => {
            debug!(
                "{}: 提升调度优先级失败，按默认优先级运行: {err}",
                kind.name()
            );
            AudioThreadPriority { kind, handle: None }
        }
    }
}

impl Drop for AudioThreadPriority {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            if let Err(err) = demote_current_thread_from_real_time(handle) {
                debug!("{}: 恢复默认调度优先级失败: {err}", self.kind.name());
            }
        }
    }
}
