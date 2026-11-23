//! Bounded analysis cache and serialized, cancellable decoding.

use super::prepare::PrepareCancel;
use crate::automix as automix_analysis;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tracing::warn;

/// One decode/analysis at a time. A superseded prepare releases its scratch
/// before the next one starts spending CPU and cache I/O.
fn analysis_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

fn analyze_optional(
    label: &str,
    path: &Path,
    analyze_bpm: bool,
    cancel: &PrepareCancel,
) -> Option<automix_analysis::TrackAnalysis> {
    let _gate = analysis_gate().lock().unwrap_or_else(|e| e.into_inner());
    if cancel.is_cancelled() {
        return None;
    }
    #[cfg(target_os = "windows")]
    let _efficiency = efficiency::EcoQos::enter();
    match automix_analysis::analyze_audio_file_until(path, analyze_bpm, &|| cancel.is_cancelled()) {
        Ok(analysis) => Some(analysis),
        Err(_) if cancel.is_cancelled() => None,
        Err(err) => {
            warn!("AutoMix {label} analysis failed: {err}");
            None
        }
    }
}

/// Analysis is background work with minutes of slack, so Windows may run it on efficiency cores at
/// a lower clock (EcoQoS) instead of boosting a performance core for it.
#[cfg(target_os = "windows")]
mod efficiency {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadInformation, ThreadPowerThrottling,
        THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        THREAD_POWER_THROTTLING_STATE,
    };

    /// Held for one analysis. The thread is borrowed from Tokio's blocking pool and serves other
    /// tasks afterwards, so the drop hands it back to the system's default rather than leaving it
    /// throttled.
    pub(super) struct EcoQos;

    impl EcoQos {
        pub(super) fn enter() -> Option<Self> {
            let speed = THREAD_POWER_THROTTLING_EXECUTION_SPEED;
            set(speed, speed).then_some(Self)
        }
    }

    impl Drop for EcoQos {
        fn drop(&mut self) {
            // An empty control mask is "let the system decide", the state a thread starts in.
            set(0, 0);
        }
    }

    fn set(control: u32, state: u32) -> bool {
        let throttling = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: control,
            StateMask: state,
        };
        // SAFETY: the pointer and size describe `throttling`, which outlives the call.
        let result = unsafe {
            SetThreadInformation(
                GetCurrentThread(),
                ThreadPowerThrottling,
                std::ptr::from_ref(&throttling).cast(),
                std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
            )
        };
        // Windows before 10 1709 has no such class; analysis then runs as it always did.
        if let Err(error) = &result {
            tracing::debug!("AutoMix analysis thread power throttling unavailable: {error}");
        }
        result.is_ok()
    }
}

// ─── Analysis cache ────────────────────────────────────────────────
//
// A track is analyzed when it is the "next" candidate and again once it becomes
// "current" on the following prepare — a guaranteed double-decode in steady-state
// playback. Repeated prepares (playlist edits, "add to play next") compound it.
// Cache the compact analysis after PCM scratch is discarded so
// each distinct file is decoded at most once. Keyed by path + size + mtime, so a
// replaced file invalidates automatically.

const ANALYSIS_CACHE_CAP: usize = 8;

struct CachedTrackAnalysis {
    key: String,
    analysis: automix_analysis::TrackAnalysis,
}

fn analysis_cache() -> &'static Mutex<Vec<CachedTrackAnalysis>> {
    static CACHE: OnceLock<Mutex<Vec<CachedTrackAnalysis>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn analysis_cache_key(path: &Path) -> String {
    // A `content://` URI cannot be stat'd, so both fields fall to 0 and the key
    // degenerates to the URI. That is still correct as an identity — SAF
    // document ids are stable — it just means a document replaced in place
    // keeps its cached analysis until the process restarts.
    // A download of ours grows under this key and its mtime moves with it, so
    // its declared size stands in for both.
    let streamed = crate::source::expected_len(path);
    let meta = std::fs::metadata(path).ok();
    let len = streamed
        .or_else(|| meta.as_ref().map(|m| m.len()))
        .unwrap_or(0);
    let mtime = meta
        .filter(|_| streamed.is_none())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}|{len}|{mtime}", path.display())
}

/// Cache-backed wrapper around `analyze_optional`. A cached entry only satisfies
/// a request that does not need BPM, or one whose cached analysis already carries
/// BPM — otherwise it is recomputed and replaces the stale entry.
pub(super) fn analyze_cached(
    label: &str,
    path: &Path,
    analyze_bpm: bool,
    wait_until: Option<std::time::Instant>,
    cancel: &PrepareCancel,
) -> Option<automix_analysis::TrackAnalysis> {
    let key = analysis_cache_key(path);

    if let Ok(cache) = analysis_cache().lock() {
        if let Some(entry) = cache.iter().find(|entry| entry.key == key) {
            if !analyze_bpm || entry.analysis.bpm.is_some() {
                return Some(entry.analysis.clone());
            }
        }
    }

    // A second reader on a download still landing waits on the network for the
    // rest of the file and competes with the playback decoder for it, so wait for
    // it to land first — waiting opens nothing. Nothing prepares again later, so
    // giving up here plans this transition by time.
    if crate::source::is_complete(path) == Some(false) {
        let landed = wait_until.is_some_and(|deadline| {
            crate::source::wait_complete(path, deadline, || cancel.is_cancelled())
        });
        if !landed {
            return None;
        }
    }

    let analysis = analyze_optional(label, path, analyze_bpm, cancel)?;

    if let Ok(mut cache) = analysis_cache().lock() {
        cache.retain(|entry| entry.key != key);
        cache.push(CachedTrackAnalysis {
            key,
            analysis: analysis.clone(),
        });
        let overflow = cache.len().saturating_sub(ANALYSIS_CACHE_CAP);
        if overflow > 0 {
            cache.drain(0..overflow);
        }
    }

    Some(analysis)
}
