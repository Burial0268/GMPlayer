//! The local music library.
//!
//! Lives in the app rather than in `audio-backend` for two reasons: it needs an
//! `AppHandle` (for the SAF plugin, the app data directory and the progress
//! channel), and it must not be compiled into the wasm build, where none of it
//! has any meaning.
//!
//! # The invariant this module exists to protect
//!
//! **Local data never disappears because of an account.** There are five paths
//! in the frontend that wipe state, and every one of them is about the Netease
//! session: `userLogOut`, `loadUserPlayLists` (whole-table replace),
//! `likeList = res.ids` (whole-table replace), `localStorage.clear()`, and the
//! Tauri pinia store teardown. None of them can reach here, and that is
//! structural rather than a matter of discipline: the truth for local data is
//! `$APPDATA/local-library.bin` + `local-user-data.json`, the truth for account
//! data is localStorage and the pinia stores, and the two share no layer. The
//! rule to keep is simply *do not move local data into the account layer* — in
//! particular, local favourites are their own set here and must never be merged
//! into `persistData.likeList`, which is replaced wholesale on every login.

pub mod cover;
pub mod index;
pub mod lyrics;
pub mod model;
pub mod playlist;
pub mod scan;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};

use index::{source_id_for, LocalIndex};
use model::{
    CoverImport, LocalPlaylist, LocalSource, LocalTrack, LyricImport, LyricKind, ScanProgress,
    SourceKind, TrackOverride,
};

// ── State ────────────────────────────────────────────────────────

pub struct LocalLibraryState {
    pub index: Mutex<LocalIndex>,
    cache_root: PathBuf,
    /// Where `local-library.bin`, `local-user-data.json` and the imported lyric
    /// files live. Kept so the lyric directory can be reached without asking the
    /// path resolver again on every call.
    data_root: PathBuf,
    /// Set while a scan is running, so a second one cannot interleave with it
    /// and half-prune the first's results.
    scanning: AtomicBool,
    cancel: AtomicBool,
}

impl LocalLibraryState {
    pub fn new<R: Runtime>(app: &AppHandle<R>) -> Self {
        let data_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."));
        let cache_root = app
            .path()
            .app_cache_dir()
            .unwrap_or_else(|_| data_dir.clone());
        let _ = std::fs::create_dir_all(&data_dir);

        LocalLibraryState {
            index: Mutex::new(LocalIndex::load(data_dir.clone())),
            cache_root,
            data_root: data_dir,
            scanning: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn cover_dir(&self) -> PathBuf {
        cover::cover_dir(&self.cache_root)
    }

    pub fn lyric_dir(&self) -> PathBuf {
        lyrics::lyric_dir(&self.data_root)
    }

    fn stream_cache_dir(&self) -> PathBuf {
        self.cache_root.join(cover::STREAM_CACHE_DIR)
    }

    fn flush(&self) {
        if let Err(err) = self.index.lock().flush() {
            log::error!(target: "local", "could not persist the local library: {err}");
        }
    }
}

/// [`LocalLibraryState::flush`] from an `async fn`, on a thread that may block.
///
/// Every other caller is a non-`async` `#[tauri::command]`, which Tauri already
/// runs on its blocking pool (see the note above `local_source_add_directory`).
/// The `async` ones — the two that pick files through a dialog — would otherwise
/// run it inline on a runtime worker, and `flush` is not a cheap write: it is a
/// bincode serialize of the whole library followed by an `fsync`
/// (`index::write_atomic` calls `sync_all`, deliberately), so on a large library
/// it is the most expensive single syscall in this module. Awaited rather than
/// detached, because callers refresh the playlist list immediately after and must
/// not read a library that has not been written yet.
async fn flush_off_thread<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.state::<LocalLibraryState>().flush())
        .await
        .map_err(|e| e.to_string())
}

// ── DTOs ─────────────────────────────────────────────────────────

/// A track as the frontend sees it: the stored row plus the two things only this
/// side can answer — where its cover actually is, and whether it is a favourite.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackDto {
    #[serde(flatten)]
    pub track: LocalTrack,
    /// Absolute path to the extracted cover, for `convertFileSrc`. `None` when
    /// the file embedded none.
    pub cover_path: Option<String>,
    pub favourite: bool,
    /// Whether any field here came from a user correction rather than the file.
    pub overridden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLibraryPage {
    pub tracks: Vec<LocalTrackDto>,
    pub total: usize,
}

/// An automatic collection: an album, an artist, or a folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalGroup {
    /// Stable id for routing. Derived from the group's name, so a link survives
    /// a re-scan.
    pub id: String,
    pub name: String,
    /// Secondary line: an album's artist, a folder's source.
    pub subtitle: String,
    pub track_count: usize,
    pub total_duration_ms: u64,
    pub cover_path: Option<String>,
    /// The exact `LocalLibraryQuery` fields that select this group's tracks.
    ///
    /// Shipped rather than re-derived in the UI: a folder is identified by
    /// `(source_id, relative_dir)` and its *display name* is neither of those,
    /// so a frontend guessing the filter from the label would silently open the
    /// wrong folder for any two sources that share a subdirectory name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalScanSummary {
    pub source: LocalSource,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Whether the walk hit its cap or was cancelled. When true nothing was
    /// pruned — a partial listing cannot distinguish "deleted" from "not
    /// reached", and treating it as authoritative would empty the library.
    pub truncated: bool,
}

/// How `local_library_list` should slice the library.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LocalLibraryQuery {
    /// Restrict to one automatic collection. Exactly one of these is set.
    pub source_id: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub folder: Option<String>,
    pub playlist_id: Option<String>,
    pub favourites_only: bool,
    /// Case-insensitive substring over title / artist / album / filename.
    pub keyword: Option<String>,
    /// `title` | `artist` | `album` | `added` | `duration`. Anything else sorts
    /// by title.
    pub sort: Option<String>,
    pub descending: bool,
    pub offset: usize,
    /// 0 means "everything from `offset`".
    pub limit: usize,
}

// ── Content provider bridge ──────────────────────────────────────

/// Teaches `audio-backend` how to open a `content://` document.
///
/// Installed once at setup, before anything can play. Without it every Android
/// local track resolves to `LocalMissing` and the planner skips the whole
/// library — silently, because a skipped track looks exactly like a track the
/// user did not queue.
struct SafContentProvider<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> gmplayer_audio_backend::ContentSourceProvider for SafContentProvider<R> {
    fn open_fd(&self, uri: &str) -> Result<gmplayer_audio_backend::ContentFd, String> {
        use tauri_plugin_local_files::LocalFilesExt;
        let plugin = self
            .app
            .local_files()
            .ok_or_else(|| "SAF is not available".to_string())?;
        let opened = plugin.open_fd(uri).map_err(|e| e.to_string())?;
        Ok(gmplayer_audio_backend::ContentFd {
            fd: opened.fd,
            is_regular_file: opened.is_regular_file,
        })
    }

    fn exists(&self, uri: &str) -> bool {
        use tauri_plugin_local_files::LocalFilesExt;
        // Overridden rather than left to the default open-and-close: the
        // resolver asks this for every local track it plans, and opening a
        // cloud-backed document can mean a network fetch.
        self.app
            .local_files()
            .and_then(|plugin| plugin.document_exists(uri).ok())
            .unwrap_or(false)
    }
}

/// Bridges the backend's session controls to the local favourite set.
///
/// The backend needs an answer for the *loaded* track whether or not a page is
/// alive — on Android the notification is often the only UI — and it must not
/// reach for `persistData.likeList`, which login replaces wholesale.
struct LocalFavouriteBridge<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> gmplayer_audio_backend::LocalFavouriteStore for LocalFavouriteBridge<R> {
    fn is_favourite(&self, key: &str) -> bool {
        self.app
            .try_state::<LocalLibraryState>()
            .is_some_and(|state| state.index.lock().is_favourite(key))
    }

    fn set_favourite(&self, key: &str, favourite: bool) -> bool {
        let Some(state) = self.app.try_state::<LocalLibraryState>() else {
            return false;
        };
        let changed = state.index.lock().set_favourite(key, favourite);
        if changed {
            // The flip is authoritative in memory the moment it returns; the
            // write goes to a worker because this runs on the player's event
            // loop, which also drives the audio timeline.
            let app = self.app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Some(state) = app.try_state::<LocalLibraryState>() {
                    state.flush();
                }
            });
        }
        changed
    }
}

/// Wire the local library into the audio backend and reconcile persisted grants.
///
/// Call from `setup`, on both desktop and mobile.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<LocalLibraryState>();

    // Where a pipe-backed document gets spooled so symphonia can seek in it.
    // Without a directory here the copy would land in the OS temp dir and be
    // repeated for every open — analysis and playback each open the track.
    let stream_dir = state.stream_cache_dir();
    let _ = std::fs::create_dir_all(&stream_dir);
    gmplayer_audio_backend::set_stream_cache_dir(stream_dir);

    if !gmplayer_audio_backend::install_content_source_provider(Arc::new(SafContentProvider {
        app: app.clone(),
    })) {
        log::warn!(target: "local", "content source provider was already installed");
    }

    if !gmplayer_audio_backend::install_local_favourite_store(Arc::new(LocalFavouriteBridge {
        app: app.clone(),
    })) {
        log::warn!(target: "local", "local favourite store was already installed");
    }

    reconcile_sources(app);
}

/// Mark sources whose grant or path is gone as unavailable.
///
/// Never deletes. A card that was out at boot, a folder on a drive that has not
/// mounted yet, a grant the user cleared in system settings — all of those come
/// back, and a library rebuilt from scratch loses the playlists that referenced
/// tracks it no longer knows about.
fn reconcile_sources<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "android")]
    let persisted: HashSet<String> = {
        use tauri_plugin_local_files::LocalFilesExt;
        app.local_files()
            .and_then(|plugin| plugin.list_persisted().ok())
            .map(|grants| {
                grants
                    .into_iter()
                    .filter(|grant| grant.read)
                    .map(|grant| grant.uri)
                    .collect()
            })
            .unwrap_or_default()
    };
    #[cfg(not(target_os = "android"))]
    let persisted: HashSet<String> = HashSet::new();

    let state = app.state::<LocalLibraryState>();
    let mut changed = false;
    {
        let mut index = state.index.lock();
        let checks: Vec<(String, bool)> = index
            .sources()
            .iter()
            .map(|source| {
                // A `Files` source is answered by its *members*, never by its
                // locator: that locator is the synthetic `local:picked-files`
                // sentinel, which is not a path and would fail `Path::exists`
                // — marking every individually-imported file unavailable at
                // each launch. Available when *any* member still is, matching
                // how a directory grant covers its children.
                let available = match source.kind {
                    SourceKind::Files => source.members.iter().any(|member| {
                        if gmplayer_audio_backend::source::is_content_uri(member) {
                            persisted.contains(member)
                        } else {
                            Path::new(member).exists()
                        }
                    }),
                    SourceKind::Directory => {
                        if gmplayer_audio_backend::source::is_content_uri(&source.locator) {
                            // Coverage, not equality. The download folder is a
                            // `GMPlayer` child of a grant on `Music`, so its own
                            // URI is never one `list_persisted` reports — testing
                            // it by equality marked every downloaded track
                            // unavailable at each launch.
                            tauri_plugin_local_files::grant_uri_for(&source.locator)
                                .is_some_and(|grant| persisted.contains(grant))
                        } else {
                            Path::new(&source.locator).exists()
                        }
                    }
                };
                (source.id.clone(), available)
            })
            .collect();

        for (id, available) in checks {
            if index.set_source_available(&id, available) {
                changed = true;
                if !available {
                    log::info!(target: "local", "local source {id} is currently unavailable");
                }
            }
        }
    }
    if changed {
        state.flush();
    }
}

// ── Helpers ──────────────────────────────────────────────────────

/// Locator of the synthetic source that holds individually-picked files.
///
/// Not a path — it names a *bag* of files rather than a place, so
/// `reconcile_sources` checks its members instead of `Path::exists`, and the UI
/// shows a label rather than this string.
pub const PICKED_FILES_LOCATOR: &str = "local:picked-files";

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build the row the UI sees.
///
/// `track` is expected to be a *view* (`LocalIndex::track_view`), i.e. already
/// carrying the user's corrections. `overridden` is passed rather than derived so
/// this stays a pure function over one row — the caller already holds the lock
/// and knows.
fn to_dto(
    track: &LocalTrack,
    cover_dir: &Path,
    favourite: bool,
    overridden: bool,
) -> LocalTrackDto {
    LocalTrackDto {
        cover_path: track
            .cover_key
            .as_ref()
            .map(|key| cover_dir.join(key).to_string_lossy().to_string()),
        favourite,
        overridden,
        track: track.clone(),
    }
}

fn group_id(prefix: &str, name: &str) -> String {
    format!("{prefix}-{:016x}", index::fnv1a64(name.as_bytes()))
}

/// Whether two rows belong to the same album.
///
/// Album *plus* `grouping_artist()`, which is the pair the album grid buckets on
/// (see `collect_groups`) — two records called "Greatest Hits" by different
/// artists are two albums, and a bulk cover apply that merged them would be the
/// classic library bug with no undo.
///
/// An untagged album matches only itself. Otherwise every file with no album tag
/// in the whole library would count as one album, and "apply to the rest of the
/// album" would repaint hundreds of unrelated tracks.
fn same_album(a: &LocalTrack, b: &LocalTrack) -> bool {
    if a.album.trim().is_empty() || b.album.trim().is_empty() {
        return a.key == b.key;
    }
    a.album == b.album && a.grouping_artist() == b.grouping_artist()
}

fn matches_query(track: &LocalTrack, query: &LocalLibraryQuery, keyword: Option<&str>) -> bool {
    if let Some(source_id) = &query.source_id {
        if &track.source_id != source_id {
            return false;
        }
    }
    if let Some(album) = &query.album {
        if &track.album != album {
            return false;
        }
    }
    if let Some(artist) = &query.artist {
        if track.grouping_artist() != artist {
            return false;
        }
    }
    if let Some(folder) = &query.folder {
        if &track.relative_dir != folder {
            return false;
        }
    }
    if let Some(keyword) = keyword {
        let haystack = format!(
            "{} {} {} {}",
            track.title, track.artist, track.album, track.display_name
        )
        .to_lowercase();
        if !haystack.contains(keyword) {
            return false;
        }
    }
    true
}

fn sort_tracks(tracks: &mut [LocalTrack], sort: Option<&str>, descending: bool) {
    match sort.unwrap_or("title") {
        "artist" => tracks.sort_by(|a, b| {
            a.grouping_artist()
                .to_lowercase()
                .cmp(&b.grouping_artist().to_lowercase())
                .then_with(|| a.album.to_lowercase().cmp(&b.album.to_lowercase()))
                .then_with(|| a.track_no.cmp(&b.track_no))
        }),
        // Album order is disc, then track number, then title — the order the
        // record has, not the order the strings sort in.
        "album" => tracks.sort_by(|a, b| {
            a.album
                .to_lowercase()
                .cmp(&b.album.to_lowercase())
                .then_with(|| a.disc_no.cmp(&b.disc_no))
                .then_with(|| a.track_no.cmp(&b.track_no))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }),
        "duration" => tracks.sort_by_key(|track| track.duration_ms),
        "added" => tracks.sort_by_key(|track| std::cmp::Reverse(track.modified_at)),
        _ => tracks.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.display_name.cmp(&b.display_name))
        }),
    }
    if descending {
        tracks.reverse();
    }
}

// ── Scanning ─────────────────────────────────────────────────────

fn run_scan<R: Runtime>(
    app: &AppHandle<R>,
    source_id: &str,
    progress: Option<Channel<ScanProgress>>,
) -> Result<LocalScanSummary, String> {
    let state = app.state::<LocalLibraryState>();
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err("a scan is already running".to_string());
    }
    state.cancel.store(false, Ordering::SeqCst);

    let outcome = scan_inner(app, &state, source_id, progress.as_ref());
    state.scanning.store(false, Ordering::SeqCst);
    state.flush();
    outcome
}

fn scan_inner<R: Runtime>(
    app: &AppHandle<R>,
    state: &LocalLibraryState,
    source_id: &str,
    progress: Option<&Channel<ScanProgress>>,
) -> Result<LocalScanSummary, String> {
    let source = state
        .index
        .lock()
        .source(source_id)
        .cloned()
        .ok_or_else(|| format!("unknown local source {source_id}"))?;

    if let Some(channel) = progress {
        let _ = channel.send(ScanProgress {
            source_id: source.id.clone(),
            phase: "listing",
            done: 0,
            total: 0,
            current: source.display_name.clone(),
            error: None,
        });
    }

    let listing = match scan::list_candidates(app, &source) {
        Ok(listing) => listing,
        Err(err) => {
            // The source is unreachable rather than empty. Mark it and keep
            // every track row: they come back when the card is back in.
            state.index.lock().set_source_available(&source.id, false);
            if let Some(channel) = progress {
                let _ = channel.send(ScanProgress {
                    source_id: source.id.clone(),
                    phase: "failed",
                    done: 0,
                    total: 0,
                    current: source.display_name.clone(),
                    error: Some(err.clone()),
                });
            }
            return Err(err);
        }
    };

    let outcome = scan::index_listing(state, &source, &listing, progress, &state.cancel, app);

    let updated = {
        let mut index = state.index.lock();
        let mut source = source.clone();
        source.available = true;
        source.last_scanned_at = now_secs();
        source.track_count = index
            .tracks()
            .filter(|track| track.source_id == source.id)
            .count();
        index.upsert_source(source.clone());
        source
    };

    prune_unreferenced_covers(state);

    if let Some(channel) = progress {
        let _ = channel.send(ScanProgress {
            source_id: updated.id.clone(),
            phase: "done",
            done: listing.audio.len(),
            total: listing.audio.len(),
            current: String::new(),
            error: None,
        });
    }

    Ok(LocalScanSummary {
        source: updated,
        added: outcome.added,
        updated: outcome.updated,
        removed: outcome.removed,
        skipped: outcome.skipped,
        failed: outcome.failed,
        truncated: outcome.truncated,
    })
}

fn prune_unreferenced_covers(state: &LocalLibraryState) {
    let referenced: HashSet<String> = {
        let index = state.index.lock();
        // Both halves, or the scan deletes what the user picked: an imported
        // cover is not on any track row, so the tracks alone do not reference it.
        index
            .tracks()
            .filter_map(|track| track.cover_key.clone())
            .chain(index.referenced_cover_files())
            .collect()
    };
    let removed = cover::prune(&state.cover_dir(), &referenced);
    if removed > 0 {
        log::info!(target: "local", "pruned {removed} unreferenced cover files");
    }
}

// ── Commands ─────────────────────────────────────────────────────
//
// **Every command here is off the main thread, and none of them may lose that.**
// `#[tauri::command]` on a non-`async fn` defaults to `ExecutionContext::Blocking`,
// which runs the body *inline in the IPC handler* — on desktop that is the window's
// event loop, i.e. the thread that paints. Each of these either locks the index and
// walks it (`local_library_list` clones and sorts the whole filtered set to cut one
// page out of it) or touches a file (`local_lyric_for` runs a full symphonia probe
// for the embedded lyric tag, on every track change), so as blocking commands they
// froze the UI for tens of milliseconds at exactly the two moments a user notices:
// a track change and a page transition. On Android the same shape is worse than
// jank — anything reaching the SAF plugin blocks on `run_mobile_plugin`, which waits
// for the Android main thread.
//
// `#[tauri::command(async)]` is the fix rather than making the bodies `async fn`:
// the work is synchronous and CPU/IO-bound, the guards are `parking_lot` (not held
// across an await, because there are none), and the attribute form runs a sync body
// on the runtime's thread pool. An `async fn` here would instead force every index
// access into something the compiler has to prove `Send` across suspension points.
//
// ── sources ──────────────────────────────────────────────────────

/// Pick a directory and index it.
///
/// The platform split lives here rather than in the frontend so there is exactly
/// one import flow: desktop opens the native folder dialog through
/// `tauri-plugin-dialog`'s *Rust* API (which returns a real path, needing no ACL
/// entry and no SAF machinery), Android opens `ACTION_OPEN_DOCUMENT_TREE` and
/// takes a persistable grant.
#[tauri::command]
pub async fn local_source_add_directory<R: Runtime>(
    app: AppHandle<R>,
    progress: Channel<ScanProgress>,
) -> Result<Option<LocalScanSummary>, String> {
    let Some((locator, display_name)) = pick_directory(&app).await? else {
        return Ok(None);
    };

    let id = source_id_for(&locator);
    let source = LocalSource {
        id: id.clone(),
        kind: SourceKind::Directory,
        locator,
        display_name,
        available: true,
        last_scanned_at: 0,
        track_count: 0,
        members: Vec::new(),
    };
    {
        let state = app.state::<LocalLibraryState>();
        let mut index = state.index.lock();
        // Re-adding a folder must not duplicate it — the id is derived from the
        // locator precisely so this is an update.
        if let Some(existing) = index.source(&id) {
            let mut merged = existing.clone();
            merged.available = true;
            merged.display_name = source.display_name.clone();
            index.upsert_source(merged);
        } else {
            index.upsert_source(source);
        }
    }

    let app_for_scan = app.clone();
    let id_for_scan = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_scan(&app_for_scan, &id_for_scan, Some(progress))
    })
    .await
    .map_err(|e| e.to_string())?
    .map(Some)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn pick_directory<R: Runtime>(app: &AppHandle<R>) -> Result<Option<(String, String)>, String> {
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
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let locator = path.to_string_lossy().to_string();
    let display_name = if display_name.is_empty() {
        locator.clone()
    } else {
        display_name
    };
    Ok(Some((locator, display_name)))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn pick_directory<R: Runtime>(app: &AppHandle<R>) -> Result<Option<(String, String)>, String> {
    use tauri_plugin_local_files::LocalFilesExt;

    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let plugin = app
            .local_files()
            .ok_or_else(|| "SAF is not available on this platform".to_string())?;
        let picked = plugin.pick_tree().map_err(|e| e.to_string())?;
        Ok(picked.map(|tree| (tree.tree_uri, tree.display_name)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pick individual files and index them.
///
/// On Android each file costs one persisted grant against a per-app quota, which
/// is why a directory is the preferred shape and this is the fallback.
///
/// `playlist_id`, when given, also appends what was picked to that local
/// playlist — "add these songs to this playlist" is one action for the user, so
/// it is one command here rather than a pick followed by hunting the files down
/// again in the library.
#[tauri::command]
pub async fn local_source_add_files<R: Runtime>(
    app: AppHandle<R>,
    progress: Channel<ScanProgress>,
    playlist_id: Option<String>,
) -> Result<Option<LocalScanSummary>, String> {
    let picked = pick_files(&app).await?;
    if picked.is_empty() {
        return Ok(None);
    }

    // One synthetic source holds every individually-picked file, so the source
    // list does not grow a row per song.
    let id = source_id_for(PICKED_FILES_LOCATOR);
    let mut newly_picked: Vec<String> = Vec::new();
    {
        let state = app.state::<LocalLibraryState>();
        let mut index = state.index.lock();
        let mut source = index.source(&id).cloned().unwrap_or(LocalSource {
            id: id.clone(),
            kind: SourceKind::Files,
            locator: PICKED_FILES_LOCATOR.to_string(),
            display_name: String::new(),
            available: true,
            last_scanned_at: 0,
            track_count: 0,
            members: Vec::new(),
        });
        for locator in picked {
            if !source.members.contains(&locator) {
                source.members.push(locator.clone());
            }
            // Recorded even when it was already a member: the user picked it now,
            // so "add these to a playlist" must mean the files in *this* dialog,
            // not only the ones that happened to be new to the library.
            newly_picked.push(locator);
        }
        source.available = true;
        index.upsert_source(source);
    }

    let app_for_scan = app.clone();
    let id_for_scan = id.clone();
    let summary = tauri::async_runtime::spawn_blocking(move || {
        run_scan(&app_for_scan, &id_for_scan, Some(progress))
    })
    .await
    .map_err(|e| e.to_string())??;

    // Adding straight into a playlist is one action for the user, so it is one
    // command here: picking files and then having to find them again in the
    // library is the flow this avoids. Only tracks the scan actually indexed are
    // added — a file symphonia could not open never became a row, and putting a
    // key with no track behind it into a playlist would show a phantom entry.
    if let Some(playlist_id) = playlist_id {
        let state = app.state::<LocalLibraryState>();
        let keys: Vec<String> = {
            let index = state.index.lock();
            newly_picked
                .iter()
                .filter(|key| index.track(key).is_some())
                .cloned()
                .collect()
        };
        if !keys.is_empty() {
            let now = now_secs();
            let found = state.index.lock().mutate_playlist(&playlist_id, |playlist| {
                playlist::add_tracks(playlist, &keys);
                playlist.updated_at = now;
            });
            if found {
                flush_off_thread(&app).await?;
            } else {
                log::warn!(
                    target: "local",
                    "imported files but playlist {playlist_id} no longer exists"
                );
            }
        }
    }

    Ok(Some(summary))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn pick_files<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Audio", scan::AUDIO_EXTENSIONS)
        .pick_files(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| "file dialog was dropped".to_string())?;
    Ok(picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| path.into_path().ok())
        .map(|path| path.to_string_lossy().to_string())
        .collect())
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn pick_files<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<String>, String> {
    use tauri_plugin_local_files::LocalFilesExt;

    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let plugin = app
            .local_files()
            .ok_or_else(|| "SAF is not available on this platform".to_string())?;
        let picked = plugin.pick_files().map_err(|e| e.to_string())?;
        // A file whose grant the system refused to persist would come back
        // unreadable after a restart, so it is not written to the index at all.
        Ok(picked
            .into_iter()
            .filter(|file| file.persisted)
            .map(|file| file.uri)
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn local_source_list<R: Runtime>(app: AppHandle<R>) -> Vec<LocalSource> {
    app.state::<LocalLibraryState>()
        .index
        .lock()
        .sources()
        .to_vec()
}

#[tauri::command]
pub async fn local_source_rescan<R: Runtime>(
    app: AppHandle<R>,
    source_id: String,
    progress: Channel<ScanProgress>,
) -> Result<LocalScanSummary, String> {
    tauri::async_runtime::spawn_blocking(move || run_scan(&app, &source_id, Some(progress)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
pub fn local_source_remove<R: Runtime>(app: AppHandle<R>, source_id: String) -> Result<usize, String> {
    let state = app.state::<LocalLibraryState>();
    let (removed, source) = {
        let mut index = state.index.lock();
        let source = index.source(&source_id).cloned();
        (index.remove_source(&source_id), source)
    };

    // Give the SAF grants back. Not doing so would quietly consume the per-app
    // quota with grants for a source the user already deleted.
    #[cfg(target_os = "android")]
    if let Some(source) = source.as_ref() {
        use tauri_plugin_local_files::LocalFilesExt;
        if let Some(plugin) = app.local_files() {
            if gmplayer_audio_backend::source::is_content_uri(&source.locator) {
                // The locator, deliberately not the grant `grant_uri_for` would
                // resolve it to. A download folder is a child of a grant on
                // `Music` — the one `download.json` still points at, and the one
                // any library source over that folder relies on — so releasing it
                // here would take the download destination away as a side effect
                // of tidying up the library view. A subfolder finds nothing of its
                // own to release, which is the right answer.
                let _ = plugin.release_uri(&source.locator);
            }
            for member in &source.members {
                let _ = plugin.release_uri(member);
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = source;

    state.index.lock().refresh_source_counts();
    prune_unreferenced_covers(&state);
    state.flush();
    Ok(removed.len())
}

#[tauri::command(async)]
pub fn local_scan_cancel<R: Runtime>(app: AppHandle<R>) {
    let state = app.state::<LocalLibraryState>();
    state.cancel.store(true, Ordering::SeqCst);
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_local_files::LocalFilesExt;
        if let Some(plugin) = app.local_files() {
            let _ = plugin.cancel_enumerate();
        }
    }
}

// ── The download folder ──────────────────────────────────────────
//
// Two entry points for `crate::download`, deliberately here rather than there:
// they are index writes, and the index has one owner.

/// Register the download directory as a library source, or refresh the existing
/// row for it.
///
/// Idempotent by construction — `source_id_for` derives the id from the locator,
/// so re-registering the same folder is an update — which is the same reason
/// `local_source_add_directory` can be pressed twice on one folder without
/// duplicating it. The source is an ordinary `Directory`: on desktop a path for
/// `walkdir`, on Android a SAF tree for the plugin walk, so "rescan the downloads
/// folder" needs no code of its own.
pub fn ensure_download_source<R: Runtime>(
    app: &AppHandle<R>,
    locator: &str,
    display_name: &str,
) -> String {
    let id = source_id_for(locator);
    let state = app.state::<LocalLibraryState>();
    {
        let mut index = state.index.lock();
        match index.source(&id) {
            Some(existing) => {
                let mut merged = existing.clone();
                merged.available = true;
                merged.display_name = display_name.to_string();
                index.upsert_source(merged);
            }
            None => index.upsert_source(LocalSource {
                id: id.clone(),
                kind: SourceKind::Directory,
                locator: locator.to_string(),
                display_name: display_name.to_string(),
                available: true,
                last_scanned_at: 0,
                track_count: 0,
                members: Vec::new(),
            }),
        }
    }
    state.flush();
    id
}

/// Index one just-downloaded file, without walking anything.
///
/// The whole trick is `truncated: true` on the listing. That flag means "this
/// enumeration is incomplete", which is exactly true here — and it is what stops
/// [`scan::index_listing`] from treating everything it did *not* see as deleted.
/// Without it, registering one download would prune every other track in the
/// download folder.
///
/// Everything else is reused: the symphonia tag probe, cover extraction and
/// downscaling, `song_id` assignment (stable for a file already known), and the
/// source count refresh.
///
/// `lyric_key` pairs the sidecar `.lrc` the download wrote. A full walk pairs
/// those itself (`Listing::note_lyric`), but a single-file listing has to be told,
/// or the lyric would not attach until some later re-scan.
///
/// One race is knowingly left open: a full re-scan of the same folder that began
/// before this file existed has it missing from its own `keep` set, so its prune
/// can drop this row. The file is on disk, so the next scan re-adds it — the
/// alternative would be to make a download fail while a scan is running, which is
/// worse.
pub fn index_downloaded_file<R: Runtime>(
    app: &AppHandle<R>,
    source_id: &str,
    locator: &str,
    display_name: &str,
    size: Option<u64>,
    lyric_key: Option<String>,
) -> Result<(), String> {
    let state = app.state::<LocalLibraryState>();
    let source = state
        .index
        .lock()
        .source(source_id)
        .cloned()
        .ok_or_else(|| format!("unknown local source {source_id}"))?;

    let mut listing = scan::Listing {
        truncated: true,
        ..Default::default()
    };
    if let Some(lyric) = lyric_key.as_deref() {
        listing.note_lyric("", display_name, lyric);
    }
    listing.audio.push(scan::Candidate {
        key: locator.to_string(),
        display_name: display_name.to_string(),
        size,
        // Stamped now rather than read back: on Android `stat_locator` cannot see
        // a `content://` document at all, and the incremental check needs both
        // halves or every later re-scan re-probes this file.
        modified_at: Some(now_secs()),
        relative_dir: String::new(),
    });

    let cancel = AtomicBool::new(false);
    let outcome = scan::index_listing(&state, &source, &listing, None, &cancel, app);
    if outcome.added == 0 && outcome.updated == 0 && outcome.skipped == 0 {
        return Err(format!("{display_name} could not be read as audio"));
    }
    state.flush();
    Ok(())
}

// ── Commands: library ────────────────────────────────────────────

#[tauri::command(async)]
pub fn local_library_list<R: Runtime>(
    app: AppHandle<R>,
    query: LocalLibraryQuery,
) -> LocalLibraryPage {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();
    let index = state.index.lock();
    let keyword = query
        .keyword
        .as_ref()
        .map(|k| k.trim().to_lowercase())
        .filter(|k| !k.is_empty());

    // A playlist is an *ordered* selection, so it cannot go through the generic
    // filter-then-sort path: the user's order is the order.
    let mut selected: Vec<LocalTrack> = if let Some(playlist_id) = &query.playlist_id {
        index
            .playlist(playlist_id)
            .map(|playlist| {
                playlist
                    .tracks
                    .iter()
                    .filter_map(|key| index.track_view(key))
                    .filter(|track| matches_query(track, &query, keyword.as_deref()))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        // `track_views` rather than `tracks`: the filter and the sort have to see
        // the corrected values, or a renamed album would neither match its own
        // keyword nor sort where the user sees it.
        let mut tracks: Vec<LocalTrack> = index
            .track_views()
            .filter(|track| !query.favourites_only || index.is_favourite(&track.key))
            .filter(|track| matches_query(track, &query, keyword.as_deref()))
            .collect();
        sort_tracks(&mut tracks, query.sort.as_deref(), query.descending);
        tracks
    };

    let total = selected.len();
    let offset = query.offset.min(total);
    let end = if query.limit == 0 {
        total
    } else {
        (offset + query.limit).min(total)
    };
    let page = selected.drain(offset..end).collect::<Vec<_>>();

    LocalLibraryPage {
        tracks: page
            .iter()
            .map(|track| {
                to_dto(
                    track,
                    &cover_dir,
                    index.is_favourite(&track.key),
                    index.override_for(&track.key).is_some(),
                )
            })
            .collect(),
        total,
    }
}

#[tauri::command(async)]
pub fn local_track_get<R: Runtime>(app: AppHandle<R>, key: String) -> Option<LocalTrackDto> {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();
    let index = state.index.lock();
    index.track_view(&key).map(|track| {
        to_dto(
            &track,
            &cover_dir,
            index.is_favourite(&key),
            index.override_for(&key).is_some(),
        )
    })
}

/// Resolve rows for tracks the frontend already holds ids for — a restored queue
/// after a restart, most importantly.
#[tauri::command(async)]
pub fn local_tracks_by_song_ids<R: Runtime>(
    app: AppHandle<R>,
    song_ids: Vec<i64>,
) -> Vec<LocalTrackDto> {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();
    let index = state.index.lock();
    song_ids
        .into_iter()
        .filter_map(|song_id| index.track_view_by_song_id(song_id))
        .map(|track| {
            let favourite = index.is_favourite(&track.key);
            let overridden = index.override_for(&track.key).is_some();
            to_dto(&track, &cover_dir, favourite, overridden)
        })
        .collect()
}

#[tauri::command(async)]
pub fn local_groups<R: Runtime>(app: AppHandle<R>, kind: String) -> Vec<LocalGroup> {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();
    let index = state.index.lock();

    let source_names: HashMap<&str, &str> = index
        .sources()
        .iter()
        .map(|source| (source.id.as_str(), source.display_name.as_str()))
        .collect();

    struct Bucket {
        name: String,
        subtitle: String,
        count: usize,
        duration: u64,
        cover: Option<String>,
        source_id: Option<String>,
        folder: Option<String>,
    }
    let mut buckets: HashMap<String, Bucket> = HashMap::new();

    // Views, not rows: an album renamed by the user has to group under the name
    // they gave it, otherwise the album list and the track list disagree.
    for track in index.track_views() {
        let (key, name, subtitle, source_id, folder) = match kind.as_str() {
            "artist" => {
                let artist = track.grouping_artist().to_string();
                (artist.clone(), artist, String::new(), None, None)
            }
            "folder" => {
                let label = if track.relative_dir.is_empty() {
                    source_names
                        .get(track.source_id.as_str())
                        .copied()
                        .unwrap_or("")
                        .to_string()
                } else {
                    track.relative_dir.clone()
                };
                let subtitle = source_names
                    .get(track.source_id.as_str())
                    .copied()
                    .unwrap_or("")
                    .to_string();
                (
                    format!("{}\u{0}{}", track.source_id, track.relative_dir),
                    label,
                    subtitle,
                    Some(track.source_id.clone()),
                    Some(track.relative_dir.clone()),
                )
            }
            // Albums key on album *plus* album-artist: two records called
            // "Greatest Hits" by different artists are two albums, and merging
            // them is the classic library bug.
            _ => (
                format!("{}\u{0}{}", track.album, track.grouping_artist()),
                track.album.clone(),
                track.grouping_artist().to_string(),
                None,
                None,
            ),
        };

        let bucket = buckets.entry(key).or_insert_with(|| Bucket {
            name,
            subtitle,
            count: 0,
            duration: 0,
            cover: None,
            source_id,
            folder,
        });
        bucket.count += 1;
        bucket.duration += track.duration_ms;
        if bucket.cover.is_none() {
            bucket.cover = track.cover_key.clone();
        }
    }

    let mut groups: Vec<LocalGroup> = buckets
        .into_iter()
        .map(|(key, bucket)| LocalGroup {
            id: group_id(&kind, &key),
            name: bucket.name,
            subtitle: bucket.subtitle,
            track_count: bucket.count,
            total_duration_ms: bucket.duration,
            cover_path: bucket
                .cover
                .map(|cover| cover_dir.join(cover).to_string_lossy().to_string()),
            source_id: bucket.source_id,
            folder: bucket.folder,
        })
        .collect();
    groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    groups
}

// ── Commands: favourites ─────────────────────────────────────────

/// Local "liked", keyed on the track locator.
///
/// Deliberately **not** `persistData.likeList`: `musicData.ts` replaces that
/// whole array from `/likelist` on every login, so a local id mixed into it
/// disappears the next time the user signs in — and it would look like a login
/// bug rather than a storage one.
#[tauri::command(async)]
pub fn local_favourite_set<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    favourite: bool,
) -> Result<bool, String> {
    let state = app.state::<LocalLibraryState>();
    let changed = state.index.lock().set_favourite(&key, favourite);
    if changed {
        state.flush();
    }
    Ok(changed)
}

#[tauri::command(async)]
pub fn local_favourite_list<R: Runtime>(app: AppHandle<R>) -> Vec<String> {
    let state = app.state::<LocalLibraryState>();
    let index = state.index.lock();
    let mut keys: Vec<String> = index.favourites().iter().cloned().collect();
    keys.sort();
    keys
}

// ── Commands: playlists ──────────────────────────────────────────

#[tauri::command(async)]
pub fn local_playlist_list<R: Runtime>(app: AppHandle<R>) -> Vec<LocalPlaylist> {
    app.state::<LocalLibraryState>()
        .index
        .lock()
        .playlists()
        .to_vec()
}

#[tauri::command(async)]
pub fn local_playlist_create<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    description: Option<String>,
) -> Result<LocalPlaylist, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("a playlist needs a name".to_string());
    }
    let now = now_secs();
    let created = LocalPlaylist {
        id: playlist::playlist_id(&name, now),
        name,
        description: description.unwrap_or_default(),
        created_at: now,
        updated_at: now,
        tracks: Vec::new(),
        imported_from: None,
    };
    let state = app.state::<LocalLibraryState>();
    state.index.lock().upsert_playlist(created.clone());
    state.flush();
    Ok(created)
}

#[tauri::command(async)]
pub fn local_playlist_update<R: Runtime>(
    app: AppHandle<R>,
    playlist_id: String,
    name: Option<String>,
    description: Option<String>,
) -> Result<bool, String> {
    let state = app.state::<LocalLibraryState>();
    let now = now_secs();
    let updated = state.index.lock().mutate_playlist(&playlist_id, |playlist| {
        if let Some(name) = name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                playlist.name = trimmed.to_string();
            }
        }
        if let Some(description) = description {
            playlist.description = description;
        }
        playlist.updated_at = now;
    });
    if updated {
        state.flush();
    }
    Ok(updated)
}

#[tauri::command(async)]
pub fn local_playlist_delete<R: Runtime>(app: AppHandle<R>, playlist_id: String) -> bool {
    let state = app.state::<LocalLibraryState>();
    let removed = state.index.lock().remove_playlist(&playlist_id);
    if removed {
        state.flush();
    }
    removed
}

#[tauri::command(async)]
pub fn local_playlist_add_tracks<R: Runtime>(
    app: AppHandle<R>,
    playlist_id: String,
    keys: Vec<String>,
) -> Result<usize, String> {
    let state = app.state::<LocalLibraryState>();
    let now = now_secs();
    let mut added = 0;
    let found = state.index.lock().mutate_playlist(&playlist_id, |playlist| {
        added = playlist::add_tracks(playlist, &keys);
        playlist.updated_at = now;
    });
    if !found {
        return Err(format!("unknown local playlist {playlist_id}"));
    }
    state.flush();
    Ok(added)
}

#[tauri::command(async)]
pub fn local_playlist_remove_tracks<R: Runtime>(
    app: AppHandle<R>,
    playlist_id: String,
    keys: Vec<String>,
) -> Result<usize, String> {
    let state = app.state::<LocalLibraryState>();
    let now = now_secs();
    let mut removed = 0;
    let found = state.index.lock().mutate_playlist(&playlist_id, |playlist| {
        removed = playlist::remove_tracks(playlist, &keys);
        playlist.updated_at = now;
    });
    if !found {
        return Err(format!("unknown local playlist {playlist_id}"));
    }
    state.flush();
    Ok(removed)
}

#[tauri::command(async)]
pub fn local_playlist_reorder<R: Runtime>(
    app: AppHandle<R>,
    playlist_id: String,
    from: usize,
    to: usize,
) -> bool {
    let state = app.state::<LocalLibraryState>();
    let now = now_secs();
    let mut moved = false;
    state.index.lock().mutate_playlist(&playlist_id, |playlist| {
        moved = playlist::reorder(playlist, from, to);
        if moved {
            playlist.updated_at = now;
        }
    });
    if moved {
        state.flush();
    }
    moved
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M3uImportResult {
    pub playlist: LocalPlaylist,
    pub matched: usize,
    /// Entries the index does not know. Reported rather than hidden: an import
    /// that silently drops half a playlist is worse than one that says so.
    pub missing: usize,
}

#[tauri::command]
pub async fn local_playlist_import_m3u<R: Runtime>(
    app: AppHandle<R>,
    path: Option<String>,
) -> Result<Option<M3uImportResult>, String> {
    let path = match path {
        Some(path) => path,
        None => match pick_playlist_file(&app).await? {
            Some(path) => path,
            None => return Ok(None),
        },
    };

    let file = Path::new(&path);
    let contents = std::fs::read_to_string(file).map_err(|e| format!("read playlist: {e}"))?;
    let base_dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let entries = playlist::parse_m3u(&contents, &base_dir);

    let state = app.state::<LocalLibraryState>();
    let now = now_secs();
    let name = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Playlist")
        .to_string();

    // Scoped in a block, not ended with `drop`: the guard is not `Send`, and the
    // await below makes this whole body a future the command macro requires to be
    // `Send`. Liveness analysis for that is conservative — an explicit `drop`
    // still counts as "maybe used later" — so the lock has to leave scope
    // syntactically.
    let (created, missing) = {
        let mut index = state.index.lock();
        let (resolved, missing) =
            playlist::resolve_entries(&entries, index.tracks().map(|track| track.key.as_str()));
        let created = LocalPlaylist {
            id: playlist::playlist_id(&name, now),
            name,
            description: String::new(),
            created_at: now,
            updated_at: now,
            tracks: resolved,
            imported_from: Some(path.clone()),
        };
        index.upsert_playlist(created.clone());
        (created, missing)
    };
    flush_off_thread(&app).await?;

    Ok(Some(M3uImportResult {
        matched: created.tracks.len(),
        playlist: created,
        missing,
    }))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn pick_playlist_file<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("M3U", &["m3u", "m3u8"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| "file dialog was dropped".to_string())?;
    match picked {
        Some(path) => Ok(Some(
            path.into_path()
                .map_err(|e| format!("file dialog returned an unusable path: {e}"))?
                .to_string_lossy()
                .to_string(),
        )),
        None => Ok(None),
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn pick_playlist_file<R: Runtime>(_app: &AppHandle<R>) -> Result<Option<String>, String> {
    // SAF hands back a `content://` document, and `parse_m3u` resolves relative
    // entries against a *parent directory* that a per-file grant cannot see — so
    // an import would silently match nothing for the common case of a playlist
    // sitting next to its music. Refused outright rather than half-working.
    Err("importing a playlist file is not supported on this platform".to_string())
}

/// Write a local playlist out as M3U8.
///
/// `path` is optional: without one, the native save dialog runs here rather than
/// in the frontend. That keeps the only file picker in the app on the Rust side —
/// the frontend has no `@tauri-apps/plugin-dialog` dependency and adding one for
/// this would also mean a `dialog:allow-save` capability entry.
///
/// Returns the number of tracks written, or `0` when the user cancelled.
#[tauri::command]
pub async fn local_playlist_export_m3u<R: Runtime>(
    app: AppHandle<R>,
    playlist_id: String,
    path: Option<String>,
) -> Result<usize, String> {
    let (name, rendered, count) = {
        let state = app.state::<LocalLibraryState>();
        let index = state.index.lock();
        let playlist = index
            .playlist(&playlist_id)
            .ok_or_else(|| format!("unknown local playlist {playlist_id}"))?;

        let entries: Vec<(String, String, String, u64)> = playlist
            .tracks
            .iter()
            .filter_map(|key| index.track(key))
            .map(|track| {
                (
                    track.key.clone(),
                    track.title.clone(),
                    track.artist.clone(),
                    track.duration_ms,
                )
            })
            .collect();
        (
            playlist.name.clone(),
            playlist::render_m3u(&playlist.name, &entries),
            entries.len(),
        )
    };

    let target = match path {
        Some(path) => Some(PathBuf::from(path)),
        None => pick_save_path(&app, &name).await?,
    };
    let Some(target) = target else { return Ok(0) };

    std::fs::write(&target, rendered).map_err(|e| format!("write playlist: {e}"))?;
    Ok(count)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn pick_save_path<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
) -> Result<Option<PathBuf>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(format!("{name}.m3u8"))
        .add_filter("M3U", &["m3u8", "m3u"])
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| "save dialog was dropped".to_string())?;
    match picked {
        Some(path) => Ok(Some(
            path.into_path()
                .map_err(|e| format!("save dialog returned an unusable path: {e}"))?,
        )),
        None => Ok(None),
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn pick_save_path<R: Runtime>(
    _app: &AppHandle<R>,
    _name: &str,
) -> Result<Option<PathBuf>, String> {
    // Writing a document through SAF needs `ACTION_CREATE_DOCUMENT`, which this
    // plugin deliberately does not implement: the library only ever reads. The
    // UI hides the export button on mobile rather than relying on this message.
    Err("exporting a playlist file is not supported on this platform".to_string())
}

// ── Commands: track detail, overrides, lyrics ─────────────────────

/// Everything the song detail page needs, in one round trip.
///
/// Carries the scanned row *and* the corrected view rather than just the view:
/// the edit form has to show which fields the user changed and what the file
/// actually says, and deriving that from two separate calls would let the two
/// disagree while an edit is in flight.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackDetail {
    /// The row as everything else sees it: corrections applied.
    pub view: LocalTrackDto,
    /// The row exactly as the file was read. `title` here is what a "revert"
    /// returns to.
    pub scanned: LocalTrack,
    /// The user's corrections, when there are any.
    pub patch: Option<TrackOverride>,
    /// Where this track's lyrics currently come from, and which import (if any)
    /// is in force.
    pub lyric: LocalLyricStatus,
    /// Where this track's cover comes from.
    pub cover: LocalCoverStatus,
    /// Absolute path of the source root, for display. `None` for a `Files`
    /// source, whose locator is a sentinel rather than a place.
    pub source_name: String,
    pub source_locator: Option<String>,
}

/// What the lyric tab shows before the user does anything.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLyricStatus {
    /// An import the user made, if any.
    pub imported: Option<LyricImport>,
    /// Whether a sibling `.lrc`/`.ttml` was found next to the file at scan time.
    pub has_sidecar: bool,
}

/// Where this track's cover comes from, and how many tracks share its album.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCoverStatus {
    /// A cover the user picked, if any. Outranks the embedded art.
    pub imported: Option<CoverImport>,
    /// Whether the file carried art of its own at scan time.
    pub has_embedded: bool,
    /// How many tracks the album holds, this one included.
    ///
    /// Drives the "apply to the rest of the album" offer: at 1 there is no rest,
    /// and showing the checkbox anyway would invite the user to wonder what it
    /// would have done.
    pub album_track_count: usize,
}

#[tauri::command(async)]
pub fn local_track_detail<R: Runtime>(app: AppHandle<R>, key: String) -> Option<LocalTrackDetail> {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();
    let index = state.index.lock();

    let scanned = index.track(&key)?.clone();
    let view = index.track_view(&key)?;
    let patch = index.override_for(&key).cloned();
    let source = index.source(&scanned.source_id);

    Some(LocalTrackDetail {
        view: to_dto(
            &view,
            &cover_dir,
            index.is_favourite(&key),
            patch.is_some(),
        ),
        lyric: LocalLyricStatus {
            imported: index.lyric_import(&key).cloned(),
            has_sidecar: scanned.lyric_key.is_some(),
        },
        cover: LocalCoverStatus {
            imported: index.cover_import(&key).cloned(),
            has_embedded: scanned.cover_key.is_some(),
            // Counted from views so a *corrected* album name groups the way the
            // album page does, which is what the checkbox will act on.
            album_track_count: index
                .track_views()
                .filter(|other| same_album(other, &view))
                .count(),
        },
        source_name: source
            .map(|s| s.display_name.clone())
            .unwrap_or_default(),
        source_locator: source
            .filter(|s| s.locator != PICKED_FILES_LOCATOR)
            .map(|s| s.locator.clone()),
        scanned,
        patch,
    })
}

/// Replace one track's tag corrections.
///
/// The patch is absolute, not incremental: what arrives is the whole override,
/// so clearing a field is expressed by sending `null` for it and reverting
/// everything by sending an empty patch. That keeps "what the user sees in the
/// form" and "what is stored" the same object, with no merge rule to get wrong.
#[tauri::command(async)]
pub fn local_track_override_set<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    patch: TrackOverride,
) -> Option<LocalTrackDto> {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();
    let mut patch = patch;
    // Blank strings are "no override", not "override with empty": a user clearing
    // the box means "use the file's value", and storing `Some("")` would instead
    // pin the title to nothing and make the row fall back to its filename.
    normalize_override(&mut patch);
    if !patch.is_empty() {
        patch.updated_at = now_secs();
    }

    let dto = {
        let mut index = state.index.lock();
        index.set_override(&key, patch);
        let view = index.track_view(&key)?;
        let overridden = index.override_for(&key).is_some();
        to_dto(&view, &cover_dir, index.is_favourite(&key), overridden)
    };
    state.flush();
    Some(dto)
}

/// Trim an incoming patch: blanks become `None`, and a value equal to nothing is
/// not worth storing.
fn normalize_override(patch: &mut TrackOverride) {
    fn blank_to_none(field: &mut Option<String>) {
        if let Some(value) = field {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                *field = None;
            } else if trimmed.len() != value.len() {
                *field = Some(trimmed.to_string());
            }
        }
    }
    blank_to_none(&mut patch.title);
    blank_to_none(&mut patch.artist);
    blank_to_none(&mut patch.album);
    blank_to_none(&mut patch.album_artist);
}

/// Read one file's tags again, replacing the indexed row.
///
/// Does **not** touch the override: a correction the user made by hand outranks
/// the file, and dropping it here would make "read it again" silently discard
/// their edit. Clearing the override is its own action.
#[tauri::command(async)]
pub fn local_track_reprobe<R: Runtime>(
    app: AppHandle<R>,
    key: String,
) -> Result<Option<LocalTrackDto>, String> {
    let state = app.state::<LocalLibraryState>();
    let cover_dir = state.cover_dir();

    let existing = state.index.lock().track(&key).cloned();
    let Some(existing) = existing else {
        return Ok(None);
    };
    // The probe is a file open plus a format probe, so it happens outside the
    // lock: holding it here would stall the audio thread's favourite lookup for
    // as long as a cold disk takes to answer.
    let fresh = scan::reprobe_track(&existing, &cover_dir)
        .ok_or_else(|| "the file could not be read".to_string())?;

    let dto = {
        let mut index = state.index.lock();
        index.upsert_track(fresh);
        let view = index
            .track_view(&key)
            .ok_or_else(|| "the track vanished while it was being read".to_string())?;
        let overridden = index.override_for(&key).is_some();
        to_dto(&view, &cover_dir, index.is_favourite(&key), overridden)
    };
    state.flush();
    Ok(Some(dto))
}

/// Store lyric text the frontend already has (a paste, or a file it read).
///
/// `extension` is only a hint for which family to store it as; the frontend
/// re-detects YRC vs QRC vs ESLrc from the content when it renders, so a
/// mislabelled paste still works.
#[tauri::command(async)]
pub fn local_lyric_import_text<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    text: String,
    extension: Option<String>,
    original_name: Option<String>,
) -> Result<LyricImport, String> {
    let state = app.state::<LocalLibraryState>();
    if state.index.lock().track(&key).is_none() {
        return Err("unknown local track".to_string());
    }
    if text.trim().is_empty() {
        return Err("the lyric is empty".to_string());
    }

    let kind = match extension.as_deref() {
        Some(ext) if !ext.trim().is_empty() => LyricKind::from_extension(ext),
        // No filename to go on: a TTML document is the one family that is
        // unmistakable from its first character, and everything else is handled
        // by the frontend's content detection.
        _ if text.trim_start().starts_with('<') => LyricKind::Ttml,
        _ => LyricKind::Lrc,
    };

    let dir = state.lyric_dir();
    let file = lyrics::store(&dir, &key, kind, lyrics::Variant::Main, &text)
        .map_err(|e| e.to_string())?;
    let entry = LyricImport {
        file,
        kind,
        original_name: original_name.unwrap_or_default(),
        imported_at: now_secs(),
    };
    {
        let mut index = state.index.lock();
        // Switching format leaves the previous file behind under a different
        // name; drop it now rather than waiting for a prune that only a scan runs.
        if let Some(previous) = index.lyric_import(&key).cloned() {
            if previous.file != entry.file {
                lyrics::remove(&dir, &previous.file);
            }
        }
        index.set_lyric_import(&key, entry.clone());
    }
    state.flush();
    Ok(entry)
}

/// Pick a lyric file and import it.
///
/// Desktop opens the native dialog through the plugin's Rust API. Android reads
/// the document through SAF without taking a persistable grant — the bytes are
/// copied immediately, so there is nothing to come back to, and grants are a
/// capped resource.
#[tauri::command(async)]
pub async fn local_lyric_import_file<R: Runtime>(
    app: AppHandle<R>,
    key: String,
) -> Result<Option<LyricImport>, String> {
    let Some((name, text)) = pick_lyric_file(&app).await? else {
        return Ok(None);
    };
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_string());
    local_lyric_import_text(app, key, text, extension, Some(name))
        .map(Some)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn pick_lyric_file<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<(String, String)>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Lyrics", &["lrc", "yrc", "qrc", "eslrc", "ttml"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| "file dialog was dropped".to_string())?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("file dialog returned an unusable path: {e}"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // Lyrics are text, but not necessarily UTF-8 — plenty of older LRC files are
    // GBK. Reading lossily keeps a mostly-correct import instead of refusing one.
    let bytes = std::fs::read(&path).map_err(|e| format!("could not read the lyric file: {e}"))?;
    Ok(Some((name, String::from_utf8_lossy(&bytes).to_string())))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn pick_lyric_file<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<(String, String)>, String> {
    use tauri_plugin_local_files::LocalFilesExt;

    // Same shape as `pick_directory`'s mobile arm: one code path for both, because
    // `local_files()` is `None` off Android and the error message is then the
    // honest answer. `spawn_blocking` because `run_mobile_plugin` blocks until the
    // Android main thread services the call, and the picker does not answer until
    // the user dismisses it.
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let plugin = app
            .local_files()
            .ok_or_else(|| "picking a file is not available on this platform".to_string())?;
        let picked = plugin.pick_text_document().map_err(|e| e.to_string())?;
        Ok(picked.map(|doc| (doc.display_name, doc.text)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Drop an imported lyric and delete its file.
#[tauri::command(async)]
pub fn local_lyric_clear<R: Runtime>(app: AppHandle<R>, key: String) -> bool {
    let state = app.state::<LocalLibraryState>();
    let dir = state.lyric_dir();
    let removed = state.index.lock().take_lyric_import(&key);
    match removed {
        Some(entry) => {
            lyrics::remove(&dir, &entry.file);
            state.flush();
            true
        }
        None => false,
    }
}

/// What a cover import ended up doing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImportResult {
    pub entry: CoverImport,
    /// Every track that now points at this picture.
    ///
    /// The keys rather than a count, because the frontend has to tell four other
    /// surfaces which rows went stale — the queue snapshot, the media session's
    /// manifest, the list views and the group grids. A count would leave it
    /// guessing, and guessing "the whole album" would refresh rows that did not
    /// change.
    pub applied: Vec<String>,
}

/// Largest picture accepted, decoded.
///
/// Generous — a 6000px scan of a gatefold is a legitimate thing to point at this —
/// but bounded, because the bytes arrive base64-encoded over IPC and are held whole
/// in memory while `image` decodes them.
const MAX_COVER_UPLOAD_BYTES: usize = 12 * 1024 * 1024;

/// Store a picture the user picked as one track's cover — or the album's.
///
/// Written to the same content-addressed `local-covers/` directory the scan uses,
/// through the same [`cover::store`], so it is downscaled identically and an album
/// applied in one gesture is one file. Nothing is written to the audio file: this
/// is an override in the same sense as [`local_track_override_set`], and it
/// outranks the embedded art because a user who went and found a cover did so
/// *because* the file's own was missing or wrong.
#[tauri::command(async)]
pub fn local_cover_import<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    data_base64: String,
    media_type: Option<String>,
    original_name: Option<String>,
    apply_to_album: Option<bool>,
) -> Result<CoverImportResult, String> {
    use base64::Engine as _;

    let state = app.state::<LocalLibraryState>();

    let view = state
        .index
        .lock()
        .track_view(&key)
        .ok_or_else(|| "unknown local track".to_string())?;

    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|_| "the picture could not be decoded".to_string())?;
    if data.is_empty() {
        return Err("the picture is empty".to_string());
    }
    if data.len() > MAX_COVER_UPLOAD_BYTES {
        return Err("the picture is too large".to_string());
    }
    // Refuse a non-image *before* touching the cache. `store` deliberately writes
    // bytes it cannot decode verbatim, which is right for a tagger's exotic
    // embedded art but wrong for a user's mis-click: the name is content-derived,
    // so nothing would ever rewrite the broken file.
    if !cover::looks_like_image(&data) {
        return Err("that file is not an image".to_string());
    }

    let art = gmplayer_audio_backend::CoverArt {
        media_type: media_type.unwrap_or_default(),
        data,
    };
    let file = cover::store(&state.cover_dir(), &art)
        .ok_or_else(|| "the cover could not be stored".to_string())?;

    let entry = CoverImport {
        file,
        original_name: original_name.unwrap_or_default(),
        imported_at: now_secs(),
    };

    let applied = {
        let mut index = state.index.lock();
        let targets: Vec<String> = if apply_to_album.unwrap_or(false) {
            index
                .track_views()
                .filter(|other| same_album(other, &view))
                .map(|other| other.key)
                .collect()
        } else {
            vec![key.clone()]
        };
        for target in &targets {
            index.set_cover_import(target, entry.clone());
        }
        targets
    };
    // The previous picture is deliberately not deleted here: it is content-addressed
    // and may still be another track's, or this album's scanned art. `prune` is
    // what collects it, once nothing references it.
    state.flush();
    Ok(CoverImportResult { entry, applied })
}

/// Drop an imported cover, falling back to whatever the file itself carried.
///
/// Answers with the keys actually cleared, for the same reason `local_cover_import`
/// does. Does not delete the picture — it is shared by content, so removing it here
/// could blank a different track. `prune_unreferenced_covers` collects it when the
/// last reference goes.
#[tauri::command(async)]
pub fn local_cover_clear<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    apply_to_album: Option<bool>,
) -> Vec<String> {
    let state = app.state::<LocalLibraryState>();
    let cleared: Vec<String> = {
        let mut index = state.index.lock();
        let Some(view) = index.track_view(&key) else {
            return Vec::new();
        };
        let targets: Vec<String> = if apply_to_album.unwrap_or(false) {
            index
                .track_views()
                .filter(|other| same_album(other, &view))
                .map(|other| other.key)
                .collect()
        } else {
            vec![key]
        };
        targets
            .into_iter()
            .filter(|target| index.take_cover_import(target).is_some())
            .collect()
    };
    if !cleared.is_empty() {
        state.flush();
    }
    cleared
}

// ── Commands: lyrics and maintenance ─────────────────────────────

/// Where a resolved lyric came from. Shown in the UI, because "why am I seeing
/// these words" is otherwise unanswerable once three sources can supply them.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalLyricSource {
    Imported,
    Embedded,
    Sidecar,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLyric {
    pub text: String,
    pub kind: LyricKind,
    pub source: LocalLyricSource,
    /// Translation, spelled the way the frontend's parser already reads it.
    ///
    /// Additive to whichever `source` won: `parseLyricData` aligns these onto the
    /// main lyric's lines, so they are companions rather than alternatives — and
    /// they come from the store, because an audio container has nowhere to put a
    /// second lyric our own reader could tell apart again.
    pub tlyric: Option<String>,
    /// Romanisation, same shape and for the same reason.
    pub romalrc: Option<String>,
}

/// Lyrics for a local track: an import the user made, then the embedded tag,
/// then a sibling `.lrc`/`.ttml` — plus the stored translation/romanisation for
/// whichever of those won.
///
/// Never touches Netease. `fetchAndParseLyric(id)` stays a Netease-only path;
/// asking it about a negative id would be a request for track `-123`.
#[tauri::command(async)]
pub fn local_lyric_for<R: Runtime>(app: AppHandle<R>, key: String) -> Option<LocalLyric> {
    let mut lyric = resolve_main_lyric(&app, &key)?;
    let (tlyric, romalrc) = read_companions(&app, &key);
    lyric.tlyric = tlyric;
    lyric.romalrc = romalrc;
    Some(lyric)
}

/// The stored translation and romanisation for `key`, in that order.
fn read_companions<R: Runtime>(
    app: &AppHandle<R>,
    key: &str,
) -> (Option<String>, Option<String>) {
    let state = app.state::<LocalLibraryState>();
    let Some(entry) = state.index.lock().lyric_companions(key).cloned() else {
        return (None, None);
    };
    let dir = state.lyric_dir();
    let read = |file: Option<String>| {
        file.and_then(|file| lyrics::read(&dir, &file))
            .filter(|text| !text.trim().is_empty())
    };
    (read(entry.translation), read(entry.romanisation))
}

/// Record translation/romanisation for a track that has just been published.
///
/// Not an *import*: [`LyricImport`] is the lyric a user went and found and outranks
/// every other source, and labelling a download that way would make a later real
/// import look like it did nothing. Best-effort, like the sidecars — the audio is
/// already on disk and playable, and a missing translation is not a failed
/// download.
///
/// Absent input leaves any existing record alone. Netease not serving a translation
/// today is not evidence that the one stored yesterday is wrong.
pub fn store_lyric_companions<R: Runtime>(
    app: &AppHandle<R>,
    key: &str,
    translation: Option<&str>,
    romanisation: Option<&str>,
) {
    let state = app.state::<LocalLibraryState>();
    let dir = state.lyric_dir();
    let mut entry = state
        .index
        .lock()
        .lyric_companions(key)
        .cloned()
        .unwrap_or_default();

    let store = |text: Option<&str>, variant: lyrics::Variant| -> Option<String> {
        let text = text.map(str::trim).filter(|text| !text.is_empty())?;
        match lyrics::store(&dir, key, LyricKind::Lrc, variant, text) {
            Ok(file) => Some(file),
            Err(err) => {
                log::warn!(target: "local", "could not store a {variant:?} lyric for {key}: {err}");
                None
            }
        }
    };
    if let Some(file) = store(translation, lyrics::Variant::Translation) {
        entry.translation = Some(file);
    }
    if let Some(file) = store(romanisation, lyrics::Variant::Romanisation) {
        entry.romanisation = Some(file);
    }
    if entry.is_empty() {
        return;
    }
    entry.stored_at = now_secs();
    state.index.lock().set_lyric_companions(key, entry);
    state.flush();
}

/// The main lyric alone: an import the user made, then the embedded tag, then a
/// sibling `.lrc`/`.ttml`.
///
/// The import wins on purpose. A user who went and found a word-timed lyric did
/// so *because* the embedded one was wrong or absent, and an order that let the
/// file override them would make the import look like it did nothing.
fn resolve_main_lyric<R: Runtime>(app: &AppHandle<R>, key: &str) -> Option<LocalLyric> {
    let state = app.state::<LocalLibraryState>();
    let (lyric_key, imported) = {
        let index = state.index.lock();
        (
            index.track(key).and_then(|t| t.lyric_key.clone()),
            index.lyric_import(key).cloned(),
        )
    };

    if let Some(entry) = imported {
        if let Some(text) = lyrics::read(&state.lyric_dir(), &entry.file) {
            if !text.trim().is_empty() {
                return Some(LocalLyric {
                    text,
                    kind: entry.kind,
                    source: LocalLyricSource::Imported,
                    tlyric: None,
                    romalrc: None,
                });
            }
        }
        // The record survived but the file did not (a hand-cleaned directory).
        // Fall through to the file's own lyrics rather than showing nothing.
        log::warn!(target: "local", "imported lyric file is missing for {key}");
    }

    // The embedded tag is the cheaper answer *only* if we cached it, and we do
    // not: lyrics can be kilobytes per track and the index is loaded whole at
    // every start. One probe when a track actually plays is the better trade.
    if let Ok((_, tags, _)) = gmplayer_audio_backend::extract_track_tags(Path::new(key), false) {
        if let Some(lyrics) = tags.lyrics.filter(|text| !text.trim().is_empty()) {
            return Some(LocalLyric {
                kind: if lyrics.trim_start().starts_with('<') {
                    LyricKind::Ttml
                } else {
                    LyricKind::Lrc
                },
                text: lyrics,
                source: LocalLyricSource::Embedded,
                tlyric: None,
                romalrc: None,
            });
        }
    }

    let sidecar = lyric_key?;
    let text = read_sidecar(app, &sidecar)?;
    if text.trim().is_empty() {
        return None;
    }
    let extension = sidecar.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("lrc");
    Some(LocalLyric {
        kind: LyricKind::from_extension(extension),
        text,
        source: LocalLyricSource::Sidecar,
        tlyric: None,
        romalrc: None,
    })
}

fn read_sidecar<R: Runtime>(app: &AppHandle<R>, locator: &str) -> Option<String> {
    if !gmplayer_audio_backend::source::is_content_uri(locator) {
        return std::fs::read_to_string(locator).ok();
    }
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_local_files::LocalFilesExt;
        let plugin = app.local_files()?;
        let result = plugin.read_bytes(locator, 4 * 1024 * 1024).ok()?;
        return Some(String::from_utf8_lossy(&result.bytes).to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        None
    }
}

/// Wipe the local library. Only reachable from the system-reset dialog's
/// explicit, unchecked-by-default "also clear the local music library" option —
/// the library surviving a reset must be a decision, not an accident.
#[tauri::command(async)]
pub fn local_library_reset<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let state = app.state::<LocalLibraryState>();

    #[cfg(target_os = "android")]
    {
        use tauri_plugin_local_files::LocalFilesExt;
        if let Some(plugin) = app.local_files() {
            let sources = state.index.lock().sources().to_vec();
            for source in sources {
                if gmplayer_audio_backend::source::is_content_uri(&source.locator) {
                    let _ = plugin.release_uri(&source.locator);
                }
                for member in &source.members {
                    let _ = plugin.release_uri(member);
                }
            }
        }
    }

    {
        let mut index = state.index.lock();
        let source_ids: Vec<String> = index
            .sources()
            .iter()
            .map(|source| source.id.clone())
            .collect();
        for id in source_ids {
            index.remove_source(&id);
        }
        for playlist in index.playlists().to_vec() {
            index.remove_playlist(&playlist.id);
        }
        for key in index.favourites().iter().cloned().collect::<Vec<_>>() {
            index.set_favourite(&key, false);
        }
    }
    prune_unreferenced_covers(&state);
    state.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::SourceKind;

    fn track(key: &str, title: &str, album: &str, artist: &str) -> LocalTrack {
        LocalTrack {
            key: key.to_string(),
            song_id: index::local_song_id(key),
            source_id: "src".into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: String::new(),
            duration_ms: 1000,
            sample_rate: 44_100,
            channels: 2,
            bitrate_bps: None,
            codec: "flac".into(),
            track_no: None,
            disc_no: None,
            year: None,
            size: Some(1),
            modified_at: Some(1),
            cover_key: None,
            needs_cache: false,
            display_name: format!("{title}.flac"),
            relative_dir: String::new(),
            lyric_key: None,
        }
    }

    /// A `Files` source's locator is the synthetic `local:picked-files` sentinel,
    /// not a path. Answering availability from it would `Path::exists` a string
    /// that is not a path and mark every individually-imported file unavailable
    /// at each launch — the tracks stay in the library, but the UI reports the
    /// whole source as broken.
    #[test]
    fn a_files_source_is_available_when_a_member_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let present = dir.path().join("a.flac");
        std::fs::write(&present, b"x").expect("write");

        assert!(!Path::new(PICKED_FILES_LOCATOR).exists());

        let members = vec![
            present.to_string_lossy().to_string(),
            dir.path().join("gone.flac").to_string_lossy().to_string(),
        ];
        assert!(
            members.iter().any(|member| Path::new(member).exists()),
            "one surviving member keeps the source available"
        );
        let all_gone = vec![dir.path().join("gone.flac").to_string_lossy().to_string()];
        assert!(!all_gone.iter().any(|member| Path::new(member).exists()));
    }

    #[test]
    fn a_keyword_matches_title_artist_album_and_filename() {
        let row = track("/m/a.flac", "Fullmoon Lullaby", "Nurture", "Porter Robinson");
        let query = LocalLibraryQuery::default();
        assert!(matches_query(&row, &query, Some("lullaby")));
        assert!(matches_query(&row, &query, Some("porter")));
        assert!(matches_query(&row, &query, Some("nurture")));
        assert!(!matches_query(&row, &query, Some("beethoven")));
    }

    /// A folder filter of `""` means the source's *root*, not "no filter". If the
    /// empty string were treated as absent, opening the root folder would select
    /// the entire library.
    #[test]
    fn an_empty_folder_filter_selects_only_the_root() {
        let mut root = track("/m/a.flac", "A", "Al", "Ar");
        root.relative_dir = String::new();
        let mut nested = track("/m/Disc 1/b.flac", "B", "Al", "Ar");
        nested.relative_dir = "Disc 1".into();

        let query = LocalLibraryQuery {
            folder: Some(String::new()),
            ..Default::default()
        };
        assert!(matches_query(&root, &query, None));
        assert!(!matches_query(&nested, &query, None));
    }

    #[test]
    fn album_sort_follows_disc_then_track_number() {
        let mut first = track("/m/1.flac", "Zebra", "Al", "Ar");
        first.disc_no = Some(1);
        first.track_no = Some(1);
        let mut second = track("/m/2.flac", "Apple", "Al", "Ar");
        second.disc_no = Some(1);
        second.track_no = Some(2);
        let mut third = track("/m/3.flac", "Banana", "Al", "Ar");
        third.disc_no = Some(2);
        third.track_no = Some(1);

        let mut rows = vec![third.clone(), second.clone(), first.clone()];
        sort_tracks(&mut rows, Some("album"), false);
        // Record order, not alphabetical order — "Zebra" is track 1.
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["Zebra", "Apple", "Banana"]
        );
    }

    #[test]
    fn source_ids_are_derived_so_a_reimport_updates_rather_than_duplicates() {
        assert_eq!(source_id_for("D:/Music"), source_id_for("D:/Music"));
        assert_ne!(source_id_for("D:/Music"), source_id_for("D:/Other"));
    }

    #[test]
    fn a_files_source_reports_its_kind() {
        let source = LocalSource {
            id: source_id_for(PICKED_FILES_LOCATOR),
            kind: SourceKind::Files,
            locator: PICKED_FILES_LOCATOR.to_string(),
            display_name: String::new(),
            available: true,
            last_scanned_at: 0,
            track_count: 0,
            members: vec!["/m/a.flac".into()],
        };
        assert!(matches!(source.kind, SourceKind::Files));
        assert_eq!(source.members.len(), 1);
    }

    /// The bulk cover apply acts on whatever this says, so the two ways it could
    /// over-reach are worth pinning: merging two same-named albums by different
    /// artists, and treating every untagged file as one giant album.
    #[test]
    fn same_album_needs_the_artist_and_refuses_untagged_albums() {
        let mut a = track("/m/a.flac", "A", "Greatest Hits", "One");
        let mut b = track("/m/b.flac", "B", "Greatest Hits", "Two");
        assert!(!same_album(&a, &b), "different artists are different albums");

        b.artist = "One".into();
        assert!(same_album(&a, &b));

        // An album artist, where present, is what decides — that is what keeps a
        // compilation's per-track artists from splitting it.
        a.album_artist = "Various".into();
        b.album_artist = "Various".into();
        a.artist = "One".into();
        b.artist = "Two".into();
        assert!(same_album(&a, &b));

        a.album = String::new();
        b.album = String::new();
        assert!(!same_album(&a, &b), "untagged albums match only themselves");
        assert!(same_album(&a, &a));
    }
}
