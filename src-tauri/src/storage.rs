//! What the app keeps on disk on the user's behalf: how big each cache is, clearing it, and the
//! audio cache's budget. Before this there was no way to see or reclaim any of it.

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use gmplayer_audio_backend::source;

/// Point the audio cache at the app cache directory and sweep what a killed process left in the
/// OS temp directory. Call from `setup`, on both desktop and mobile, before the player exists.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    match app.path().app_cache_dir() {
        Ok(dir) => {
            source::set_audio_cache_dir(dir.join("audio"));
        }
        Err(err) => log::warn!(target: "storage", "no app cache directory, audio cache off: {err}"),
    }
    let _ = std::thread::Builder::new()
        .name("temp-sweep".into())
        .spawn(|| {
            let freed = source::sweep_temp_leftovers();
            if freed > 0 {
                log::info!(target: "storage", "removed {freed} bytes of leftover temp files");
            }
        });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// Played tracks kept for replay.
    audio: u64,
    /// Cached Netease API responses.
    api: u64,
    /// Copies of local documents that cannot be read in place (Android).
    local_stream: u64,
    /// Downloads and copies of ours left in the OS temp directory, mostly by versions before the
    /// audio cache.
    temp: u64,
}

#[tauri::command]
pub async fn storage_usage(app: AppHandle) -> Result<StorageUsage, String> {
    tauri::async_runtime::spawn_blocking(move || StorageUsage {
        audio: source::audio_cache_usage(),
        api: app
            .try_state::<crate::ncm::NcmState>()
            .map_or(0, |ncm| ncm.cache_usage()),
        local_stream: source::stream_cache_usage(),
        temp: source::temp_leftover_usage(),
    })
    .await
    .map_err(|e| e.to_string())
}

/// Clear one cache (`audio`, `api`, `localStream` or `temp`), returning the bytes freed. A file
/// in use (the playing track) survives and is reclaimed later.
#[tauri::command]
pub async fn storage_clear(app: AppHandle, kind: String) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || match kind.as_str() {
        "audio" => Ok(source::clear_audio_cache()),
        "api" => Ok(app
            .try_state::<crate::ncm::NcmState>()
            .map_or(0, |ncm| ncm.clear_cache())),
        "localStream" => Ok(source::clear_stream_cache()),
        "temp" => Ok(source::clear_temp_leftovers()),
        other => Err(format!("unknown cache: {other}")),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The most the audio cache may keep; `0` turns it off. Lowering it evicts at once.
#[tauri::command]
pub fn storage_set_audio_budget(bytes: u64) {
    source::set_audio_cache_budget(bytes);
}
