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
    match automix_analysis::analyze_audio_file_until(path, analyze_bpm, &|| cancel.is_cancelled()) {
        Ok(analysis) => Some(analysis),
        Err(_) if cancel.is_cancelled() => None,
        Err(err) => {
            warn!("AutoMix {label} analysis failed: {err}");
            None
        }
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
    // rest of the file and competes with the playback decoder for it. Plan by
    // time until the source is whole; a later prepare fills the cache.
    if crate::source::is_complete(path) == Some(false) {
        return None;
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
