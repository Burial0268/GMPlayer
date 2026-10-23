//! Downloading songs to a place the platform considers correct, and putting them
//! in the local library the moment they land.
//!
//! # Why this is not a `fetch` in the page
//!
//! It used to be: `fetch` → `blob` → `URL.createObjectURL` → a synthetic `<a
//! download>` click. That has three problems and one of them is fatal.
//!
//! On **Android it does nothing at all**. A WebView only downloads through a
//! `DownloadListener`, the generated app installs none, and a `blob:` URL is not
//! something the system download manager can be handed anyway — so the button was
//! silently inert on the platform where "where did my file go" matters most.
//!
//! On **desktop** the destination belonged to WebView2, not to the app: whatever
//! the embedded browser had configured, possibly behind a "save as" prompt, with
//! no way for the app to know the path afterwards — and therefore no way to add
//! it to the library.
//!
//! And either way the whole file was materialised as a blob in the WebView first,
//! which for a Hi-Res FLAC is tens of megabytes pinned in the renderer.
//!
//! # Layout
//!
//! - [`config`] — the directory and the queue's knobs, persisted in `$APPDATA`.
//! - [`sink`] — the write target: a real directory, or a SAF tree.
//! - [`fetch`] — one resumable HTTP transfer, split across streams or not.
//! - [`tags`] — writing the metadata a streaming source does not carry.
//! - [`queue`] — the task table, the scheduler, and the per-task pipeline.

pub mod config;
pub mod fetch;
pub mod queue;
pub mod sink;
pub mod tags;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};

pub use config::DownloadConfig;
pub use queue::{DownloadEvent, DownloadState, DownloadTask, EnqueueItem};

/// The settings page's whole view of this module.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSettings {
    #[serde(flatten)]
    pub config: DownloadConfig,
    /// Where downloads *would* go if nothing has been chosen. Shown as a hint and
    /// deliberately not created — a folder should appear because the user
    /// downloaded something, not because they opened the settings page.
    pub default_dir: String,
    /// Whether this platform needs the user to pick a directory before anything
    /// can be downloaded. True on Android, where the destination is a SAF grant.
    pub requires_pick: bool,
}

fn settings_for<R: Runtime>(app: &AppHandle<R>) -> DownloadSettings {
    DownloadSettings {
        config: app.state::<DownloadState>().config(),
        default_dir: config::default_path(app).to_string_lossy().to_string(),
        requires_pick: cfg!(any(target_os = "android", target_os = "ios")),
    }
}

// ── Commands ─────────────────────────────────────────────────────
//
// `#[tauri::command(async)]` on the synchronous ones for the same reason as in
// `crate::local`: a non-`async` command body runs *inline in the IPC handler*,
// which on desktop is the thread that paints. The `async fn` ones open a picker
// or await the protocol layer and must not block a runtime worker either.

#[tauri::command(async)]
pub fn download_settings_get<R: Runtime>(app: AppHandle<R>) -> DownloadSettings {
    settings_for(&app)
}

#[tauri::command(async)]
pub fn download_config_set<R: Runtime>(
    app: AppHandle<R>,
    config: DownloadConfig,
) -> Result<DownloadSettings, String> {
    queue::set_config(&app, config)?;
    Ok(settings_for(&app))
}

/// Open the platform picker for a download directory.
///
/// The split lives here rather than in the frontend so there is one flow:
/// desktop opens the native folder dialog through `tauri-plugin-dialog`'s *Rust*
/// API, Android opens `ACTION_OPEN_DOCUMENT_TREE` on `Music`, takes a persistable
/// **read+write** grant, and narrows it to a `GMPlayer` child — see
/// [`own_subdirectory`]. `Ok(None)` means the user cancelled.
#[tauri::command]
pub async fn download_dir_pick<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<DownloadSettings>, String> {
    let Some((dir, label)) = pick_dir(&app).await? else {
        return Ok(None);
    };
    let mut config = app.state::<DownloadState>().config();
    config.dir = Some(dir);
    config.dir_label = Some(label);
    queue::set_config(&app, config)?;
    Ok(Some(settings_for(&app)))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn pick_dir<R: Runtime>(app: &AppHandle<R>) -> Result<Option<(String, String)>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx.await.map_err(|_| "folder dialog was dropped".to_string())?;
    let Some(path) = picked else { return Ok(None) };
    let path = path
        .into_path()
        .map_err(|e| format!("folder dialog returned an unusable path: {e}"))?;
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let dir = path.to_string_lossy().to_string();
    let label = if label.is_empty() { dir.clone() } else { label };
    Ok(Some((dir, label)))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn pick_dir<R: Runtime>(app: &AppHandle<R>) -> Result<Option<(String, String)>, String> {
    use tauri_plugin_local_files::LocalFilesExt;

    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let plugin = app
            .local_files()
            .ok_or_else(|| "SAF is not available on this platform".to_string())?;
        let picked = plugin
            .pick_writable_tree(Some(config::PICKER_INITIAL_DIR))
            .map_err(|e| e.to_string())?;
        Ok(picked.map(|tree| own_subdirectory(&plugin, tree)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The MIME that makes `createDocument` create a folder rather than a file.
#[cfg(any(target_os = "android", target_os = "ios"))]
const DIRECTORY_MIME: &str = "vnd.android.document/directory";

/// Narrow a fresh grant down to our own folder inside it.
///
/// The picker opens on `Music` (see [`config::PICKER_INITIAL_DIR`]) and the grant
/// is therefore usually over the user's whole music collection. Writing loose
/// into it would make that collection the library source this module registers —
/// so "rescan the downloads folder" would rescan everything — and it would put a
/// children query over every file they own in front of *each* download, because a
/// SAF tree has no `stat` by name for [`sink::probe`] to use instead.
///
/// The child is addressed with the same grant: the URI comes back in
/// `…/tree/<grant>/document/<child>` form, which the Kotlin side resolves through
/// `addressedDocumentId`, so this costs no second permission and no second row in
/// the app's persisted-grant budget.
///
/// Best-effort by construction. If the child cannot be created, the folder the
/// user just granted is still a perfectly good destination and is used as-is —
/// failing the pick outright would discard a real grant over a preference.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn own_subdirectory<R: Runtime>(
    plugin: &tauri_plugin_local_files::LocalFiles<R>,
    tree: tauri_plugin_local_files::PickedTree,
) -> (String, String) {
    // Picking our own folder again must not nest a second one inside it.
    if tree.display_name == config::DIR_NAME {
        return (tree.tree_uri, tree.display_name);
    }
    // Reuse rather than create: `createDocument` does not merge with an existing
    // name, it appends `(1)`, which would strand every earlier download in a
    // folder the library no longer treats as the download source.
    if let Ok(Some(found)) = plugin.find_document(&tree.tree_uri, config::DIR_NAME) {
        if found.mime_type.as_deref() == Some(DIRECTORY_MIME) {
            return (found.uri, found.display_name);
        }
    }
    match plugin.create_document(&tree.tree_uri, config::DIR_NAME, Some(DIRECTORY_MIME)) {
        Ok(created) => (created.uri, created.display_name),
        Err(err) => {
            log::warn!(
                target: "download",
                "could not create {} inside the granted folder ({err}); using the grant itself",
                config::DIR_NAME
            );
            (tree.tree_uri, tree.display_name)
        }
    }
}

/// Hand the queue an event sink.
///
/// One channel at a time, replaced on re-subscribe — a page reload gets a fresh
/// one and the previous is dropped. Nothing about the queue depends on a
/// subscriber existing: on Android the WebView it belonged to may be killed
/// mid-batch and the transfers carry on regardless.
#[tauri::command(async)]
pub fn download_subscribe<R: Runtime>(app: AppHandle<R>, events: Channel<DownloadEvent>) {
    app.state::<DownloadState>().subscribe(events);
}

#[tauri::command(async)]
pub fn download_list<R: Runtime>(app: AppHandle<R>) -> Vec<DownloadTask> {
    app.state::<DownloadState>().tasks()
}

/// Record the account cookie the queue resolves URLs with.
///
/// Pushed by the frontend whenever the login state changes, and held in memory
/// only — `download.json` is a settings file, and a credential does not belong in
/// one.
#[tauri::command(async)]
pub fn download_set_credentials<R: Runtime>(app: AppHandle<R>, cookie: Option<String>) {
    app.state::<DownloadState>().set_cookie(cookie);
}

/// Queue songs for download.
///
/// Fails with [`config::NEEDS_DIRECTORY`] when Android has no directory yet, so
/// the frontend can open the picker instead of showing an error. Checked before
/// anything is enqueued: a batch of 300 rows all failing for the same reason is
/// not a useful thing to show someone.
#[tauri::command(async)]
pub fn download_enqueue<R: Runtime>(
    app: AppHandle<R>,
    items: Vec<EnqueueItem>,
) -> Result<Vec<DownloadTask>, String> {
    queue::resolve_target(&app)?;
    Ok(queue::enqueue(&app, items))
}

#[tauri::command(async)]
pub fn download_pause<R: Runtime>(app: AppHandle<R>, id: String) {
    queue::pause(&app, &id);
}

#[tauri::command(async)]
pub fn download_resume<R: Runtime>(app: AppHandle<R>, id: String) {
    queue::resume(&app, &id);
}

#[tauri::command(async)]
pub fn download_retry<R: Runtime>(app: AppHandle<R>, id: String) {
    queue::retry(&app, &id);
}

#[tauri::command(async)]
pub fn download_cancel<R: Runtime>(app: AppHandle<R>, id: String) {
    queue::cancel(&app, &id);
}

#[tauri::command(async)]
pub fn download_clear_finished<R: Runtime>(app: AppHandle<R>) {
    queue::clear_finished(&app);
}
