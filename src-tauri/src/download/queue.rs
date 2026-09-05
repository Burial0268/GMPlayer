//! The download queue: task table, scheduler, and the per-task pipeline.
//!
//! # Why the URL is resolved here and not in the frontend
//!
//! `/song/download/url` answers with a **signed, expiring** URL. A batch of 300
//! tracks resolved up front in the page would have its tail expire before the
//! transfers reached it, so every task resolves at the moment it starts — which
//! is also why this side needs the account cookie.
//!
//! # Why there is no semaphore
//!
//! Concurrency is a setting the user can change while the queue is running.
//! [`pump`] instead counts what is in flight against the *current* limit every
//! time anything changes, so lowering it simply stops new starts and raising it
//! takes effect on the next completion — where a fixed-permit semaphore would
//! have to be rebuilt, and tokio's cannot shrink.
//!
//! The connection budget a split transfer draws on is counted the same way and for
//! the same reason: [`Inner::spare_streams`] answers "how many extra streams may
//! open right now" from the current config, and a task reserves what it can get
//! rather than waiting for a permit — see [`run_parts`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};
use tokio::io::AsyncWriteExt;

use super::config::{self, DownloadConfig, DownloadTarget};
use super::{fetch, sink, tags};

/// What a task is doing. String-valued over the wire (`serde(rename_all)`), so
/// the frontend discriminates on a string rather than on a boolean or an ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    /// Asking Netease for a fresh signed URL.
    Resolving,
    Downloading,
    /// Stopped by the user. The partial file is kept, so resuming costs only the
    /// bytes that are missing.
    Paused,
    Done,
    Failed,
    /// The file was already there at the right size. Still indexed, so the row
    /// exists even if this app has never seen the file before.
    Skipped,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Skipped)
    }

    fn is_active(self) -> bool {
        matches!(self, TaskStatus::Resolving | TaskStatus::Downloading)
    }
}

/// One song the frontend asked for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueItem {
    pub song_id: i64,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    /// Overrides the configured default quality for this one item.
    #[serde(default)]
    pub br: Option<u32>,
    /// Carried from the page so a cover sidecar needs no extra API call.
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTask {
    pub id: String,
    pub song_id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub br: u32,
    pub status: TaskStatus,
    pub received: u64,
    /// `None` while the server has not said how big the file is (a chunked
    /// response), which the UI shows as an indeterminate transfer.
    pub total: Option<u64>,
    pub error: Option<String>,
    /// The library key, once published.
    pub locator: Option<String>,
    pub file_name: Option<String>,
    pub created_at: i64,
    /// Position in its batch, for the `{index}` placeholder.
    pub index: usize,
    #[serde(skip)]
    cover_url: Option<String>,
    /// How many `Range` streams this file is split across. `0` until the first
    /// attempt decides it.
    ///
    /// Kept on the task rather than recomputed, and not sent to the page. The split
    /// decides the part filenames, so a resume that derived it again from a setting
    /// the user changed in between would address different spans and orphan every
    /// byte already on disk.
    #[serde(skip)]
    segments: usize,
}

/// Pushed to the frontend. Per-task rather than whole-list: a 300-track batch
/// would otherwise reserialize the entire queue on every chunk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DownloadEvent {
    /// The whole table. Sent on subscribe and after a bulk edit.
    Snapshot { tasks: Vec<DownloadTask> },
    Updated { task: DownloadTask },
    ConfigChanged { config: DownloadConfig },
}

// ── State ────────────────────────────────────────────────────────

pub struct DownloadState {
    inner: Mutex<Inner>,
}

struct Inner {
    config: DownloadConfig,
    tasks: Vec<DownloadTask>,
    /// Per-task interrupt flags. Only live tasks have one.
    cancels: HashMap<String, Arc<AtomicBool>>,
    /// The page's event sink. Replaced on re-subscribe (a reload), dropped when a
    /// send fails — the queue keeps running either way, which is the whole point
    /// of it living here.
    channel: Option<Channel<DownloadEvent>>,
    /// Account cookie for `song_download_url`.
    ///
    /// Memory only, deliberately: it is a credential, and `download.json` is a
    /// settings file the user may well copy between machines.
    cookie: Option<String>,
    /// Monotonic id source. Not a UUID — nothing outside this process refers to a
    /// task, and the queue does not survive a restart.
    next_id: u64,
    /// The library source id for the current directory, resolved once.
    source_id: Option<String>,
    /// The locator `source_id` was derived from, so changing the download
    /// directory re-registers rather than reusing the old source.
    source_locator: Option<String>,
    /// Streams currently open *beyond* the one every running task gets.
    ///
    /// Counted rather than permitted, for the same reason [`pump`] counts: the
    /// ceiling moves whenever the user edits `concurrency`.
    extra_streams: usize,
}

impl DownloadState {
    pub fn new<R: Runtime>(app: &AppHandle<R>) -> Self {
        DownloadState {
            inner: Mutex::new(Inner {
                config: config::load(app),
                tasks: Vec::new(),
                cancels: HashMap::new(),
                channel: None,
                cookie: None,
                next_id: 1,
                source_id: None,
                source_locator: None,
                extra_streams: 0,
            }),
        }
    }

    pub fn config(&self) -> DownloadConfig {
        self.inner.lock().config.clone()
    }

    pub fn tasks(&self) -> Vec<DownloadTask> {
        self.inner.lock().tasks.clone()
    }

    pub fn set_cookie(&self, cookie: Option<String>) {
        let cookie = cookie.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        self.inner.lock().cookie = cookie;
    }

    pub fn subscribe(&self, channel: Channel<DownloadEvent>) {
        let mut inner = self.inner.lock();
        inner.channel = Some(channel);
        let event = DownloadEvent::Snapshot {
            tasks: inner.tasks.clone(),
        };
        inner.emit(event);
    }
}

impl Inner {
    /// Send an event, dropping the channel if the page it belonged to is gone.
    ///
    /// A dead channel must not be retried forever: on Android the WebView is
    /// killed routinely, and every chunk of every transfer would otherwise take
    /// the failing IPC path.
    fn emit(&mut self, event: DownloadEvent) {
        let Some(channel) = self.channel.as_ref() else {
            return;
        };
        if channel.send(event).is_err() {
            self.channel = None;
        }
    }

    fn task(&mut self, id: &str) -> Option<&mut DownloadTask> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    fn snapshot_of(&self, id: &str) -> Option<DownloadTask> {
        self.tasks.iter().find(|task| task.id == id).cloned()
    }

    fn active_count(&self) -> usize {
        self.tasks.iter().filter(|task| task.status.is_active()).count()
    }

    /// How many streams may be opened beyond the one each running task gets.
    ///
    /// Every active task is charged one whether it has opened it yet or not, which
    /// is what keeps a queue that fills up from also splitting: `concurrency` is the
    /// worst case for that half, so the split half gets what is left over.
    fn spare_streams(&self) -> usize {
        config::MAX_TOTAL_STREAMS
            .saturating_sub(self.config.concurrency)
            .saturating_sub(self.extra_streams)
    }
}

/// Mutate one task and push the result to the frontend.
fn update<R: Runtime>(app: &AppHandle<R>, id: &str, mutate: impl FnOnce(&mut DownloadTask)) {
    let state = app.state::<DownloadState>();
    let mut inner = state.inner.lock();
    let Some(task) = inner.task(id) else { return };
    mutate(task);
    let snapshot = task.clone();
    inner.emit(DownloadEvent::Updated { task: snapshot });
}

/// Start whatever the current concurrency limit allows.
///
/// Called after every state change — enqueue, completion, pause, resume, a
/// concurrency edit — and safe to call spuriously: with nothing startable it
/// takes the lock, finds nothing, and returns.
pub fn pump<R: Runtime>(app: &AppHandle<R>) {
    loop {
        let started = {
            let state = app.state::<DownloadState>();
            let mut inner = state.inner.lock();
            let limit = inner.config.concurrency;
            if inner.active_count() >= limit {
                None
            } else {
                let next = inner
                    .tasks
                    .iter()
                    .position(|task| task.status == TaskStatus::Queued);
                match next {
                    None => None,
                    Some(position) => {
                        let task = &mut inner.tasks[position];
                        task.status = TaskStatus::Resolving;
                        task.error = None;
                        let id = task.id.clone();
                        let snapshot = task.clone();
                        let cancel = Arc::new(AtomicBool::new(false));
                        inner.cancels.insert(id.clone(), cancel.clone());
                        inner.emit(DownloadEvent::Updated { task: snapshot });
                        Some((id, cancel))
                    }
                }
            }
        };
        let Some((id, cancel)) = started else { return };
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            run_task(app, id, cancel).await;
        });
    }
}

// ── Queue operations ─────────────────────────────────────────────

/// Add songs to the queue, skipping any that are already in it unfinished.
///
/// De-duplication is on `(song_id, br)` and only against non-terminal rows:
/// pressing "download all" twice on an album must not double the work, but
/// re-downloading something that failed, or at a different quality, has to be
/// possible.
pub fn enqueue<R: Runtime>(app: &AppHandle<R>, items: Vec<EnqueueItem>) -> Vec<DownloadTask> {
    let added = {
        let state = app.state::<DownloadState>();
        let mut inner = state.inner.lock();
        let default_br = inner.config.default_br;
        let created_at = now_secs();
        let mut added = Vec::new();
        for item in items {
            let br = item.br.unwrap_or(default_br);
            let duplicate = inner.tasks.iter().any(|task| {
                task.song_id == item.song_id && task.br == br && !task.status.is_terminal()
            });
            if duplicate {
                continue;
            }
            let index = inner.tasks.len() + 1;
            let id = format!("dl-{}", inner.next_id);
            inner.next_id += 1;
            let task = DownloadTask {
                id,
                song_id: item.song_id,
                title: item.title,
                artist: item.artist,
                album: item.album,
                br,
                status: TaskStatus::Queued,
                received: 0,
                total: None,
                error: None,
                locator: None,
                file_name: None,
                created_at,
                index,
                cover_url: item.cover_url,
                segments: 0,
            };
            inner.tasks.push(task.clone());
            added.push(task);
        }
        if !added.is_empty() {
            let event = DownloadEvent::Snapshot {
                tasks: inner.tasks.clone(),
            };
            inner.emit(event);
        }
        added
    };
    pump(app);
    added
}

/// Stop a task, keeping its partial file.
///
/// The flag is what the transfer loop polls; the status flips to `Paused` here
/// rather than in the worker so a task still queued (never started) also pauses.
pub fn pause<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let state = app.state::<DownloadState>();
    {
        let mut inner = state.inner.lock();
        if let Some(cancel) = inner.cancels.get(id) {
            cancel.store(true, Ordering::Relaxed);
        }
        let Some(task) = inner.task(id) else { return };
        if task.status.is_terminal() {
            return;
        }
        task.status = TaskStatus::Paused;
        let snapshot = task.clone();
        inner.emit(DownloadEvent::Updated { task: snapshot });
    }
    pump(app);
}

pub fn resume<R: Runtime>(app: &AppHandle<R>, id: &str) {
    {
        let state = app.state::<DownloadState>();
        let mut inner = state.inner.lock();
        let Some(task) = inner.task(id) else { return };
        if task.status != TaskStatus::Paused {
            return;
        }
        task.status = TaskStatus::Queued;
        let snapshot = task.clone();
        inner.emit(DownloadEvent::Updated { task: snapshot });
    }
    pump(app);
}

/// Put a failed task back in line. The partial file survives, so this resumes
/// rather than restarts.
pub fn retry<R: Runtime>(app: &AppHandle<R>, id: &str) {
    {
        let state = app.state::<DownloadState>();
        let mut inner = state.inner.lock();
        let Some(task) = inner.task(id) else { return };
        if task.status == TaskStatus::Done {
            return;
        }
        task.status = TaskStatus::Queued;
        task.error = None;
        let snapshot = task.clone();
        inner.emit(DownloadEvent::Updated { task: snapshot });
    }
    pump(app);
}

/// Remove a task. A live transfer is interrupted first; the partial file is
/// deleted, because nothing will come back for it.
pub fn cancel<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let state = app.state::<DownloadState>();
    {
        let mut inner = state.inner.lock();
        if let Some(cancel) = inner.cancels.remove(id) {
            cancel.store(true, Ordering::Relaxed);
        }
        inner.tasks.retain(|task| task.id != id);
        let event = DownloadEvent::Snapshot {
            tasks: inner.tasks.clone(),
        };
        inner.emit(event);
    }
    // The `.part` is left to the worker, which owns the open handle and will see
    // the flag. Deleting it from here would race a write still in flight.
    pump(app);
}

/// Drop every finished row. Nothing on disk is touched.
pub fn clear_finished<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<DownloadState>();
    let mut inner = state.inner.lock();
    inner.tasks.retain(|task| !task.status.is_terminal());
    let event = DownloadEvent::Snapshot {
        tasks: inner.tasks.clone(),
    };
    inner.emit(event);
}

pub fn set_config<R: Runtime>(
    app: &AppHandle<R>,
    mut config: DownloadConfig,
) -> Result<DownloadConfig, String> {
    config.normalize();
    {
        let state = app.state::<DownloadState>();
        let mut inner = state.inner.lock();
        // A different directory is a different library source. Forgetting the
        // resolved id here is what stops the next download from being filed under
        // the folder the user just moved away from.
        if inner.config.dir != config.dir {
            inner.source_id = None;
            inner.source_locator = None;
        }
        inner.config = config.clone();
        let event = DownloadEvent::ConfigChanged {
            config: config.clone(),
        };
        inner.emit(event);
    }
    config::save(app, &config)?;
    // Concurrency may have gone up.
    pump(app);
    Ok(config)
}

/// Record a directory without going through [`set_config`]'s `pump`.
///
/// Used from inside a running task, where re-entering the scheduler would be
/// pointless work on the way to the same place.
///
/// Desktop-only, because filling in a default is: Android has nothing to fill in
/// with — the destination is a grant the user gives — so every directory change
/// there arrives through `download_dir_pick` and `set_config` instead.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn store_dir<R: Runtime>(app: &AppHandle<R>, dir: String, label: Option<String>) {
    let state = app.state::<DownloadState>();
    let config = {
        let mut inner = state.inner.lock();
        inner.config.dir = Some(dir);
        if label.is_some() {
            inner.config.dir_label = label;
        }
        inner.source_id = None;
        inner.source_locator = None;
        inner.config.clone()
    };
    let event = DownloadEvent::ConfigChanged {
        config: config.clone(),
    };
    state.inner.lock().emit(event);
    if let Err(err) = config::save(app, &config) {
        log::warn!(target: "download", "could not persist the download directory: {err}");
    }
}

/// The destination, filling in and persisting a desktop default on first use.
///
/// Android returns [`config::NEEDS_DIRECTORY`] instead of guessing: the whole
/// point of the SAF route is that the folder is the user's choice, and the only
/// directory we could invent without asking is the app-private one they would
/// not be able to browse.
pub fn resolve_target<R: Runtime>(app: &AppHandle<R>) -> Result<DownloadTarget, String> {
    let existing = app.state::<DownloadState>().inner.lock().config.dir.clone();
    if let Some(dir) = existing {
        if gmplayer_audio_backend::source::is_content_uri(&dir) {
            return Ok(DownloadTarget::Tree(dir));
        }
        let path = std::path::PathBuf::from(&dir);
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("could not create {}: {e}", path.display()))?;
        return Ok(DownloadTarget::Path(path));
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        Err(config::NEEDS_DIRECTORY.to_string())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let path = config::default_path(app);
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("could not create {}: {e}", path.display()))?;
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(config::DIR_NAME)
            .to_string();
        store_dir(app, path.to_string_lossy().to_string(), Some(label));
        Ok(DownloadTarget::Path(path))
    }
}

/// The library source the download directory is, creating it if needed.
///
/// Memoised against the locator it was derived from, so the common case costs a
/// lock rather than an index write.
fn ensure_source<R: Runtime>(app: &AppHandle<R>, target: &DownloadTarget) -> String {
    let locator = target.locator();
    let state = app.state::<DownloadState>();
    let label = {
        let inner = state.inner.lock();
        if inner.source_locator.as_deref() == Some(locator.as_str()) {
            if let Some(id) = &inner.source_id {
                return id.clone();
            }
        }
        inner.config.dir_label.clone()
    };
    let display_name = label.unwrap_or_else(|| match target {
        DownloadTarget::Path(path) => path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(config::DIR_NAME)
            .to_string(),
        DownloadTarget::Tree(_) => config::DIR_NAME.to_string(),
    });
    let id = crate::local::ensure_download_source(app, &locator, &display_name);
    let mut inner = state.inner.lock();
    inner.source_id = Some(id.clone());
    inner.source_locator = Some(locator);
    id
}

// ── The per-task pipeline ────────────────────────────────────────

enum Outcome {
    Done {
        locator: String,
        file_name: String,
        size: u64,
    },
    Skipped {
        locator: String,
        file_name: String,
    },
    /// Paused or cancelled. Whoever asked for it already set the status, or
    /// removed the row outright.
    Interrupted,
    Failed(String),
}

async fn run_task<R: Runtime>(app: AppHandle<R>, id: String, cancel: Arc<AtomicBool>) {
    let outcome = execute(&app, &id, &cancel).await;
    {
        let state = app.state::<DownloadState>();
        let mut inner = state.inner.lock();
        inner.cancels.remove(&id);
        if let Some(task) = inner.task(&id) {
            match outcome {
                Outcome::Done {
                    locator,
                    file_name,
                    size,
                } => {
                    task.status = TaskStatus::Done;
                    task.locator = Some(locator);
                    task.file_name = Some(file_name);
                    task.received = size;
                    task.total = Some(size);
                    task.error = None;
                }
                Outcome::Skipped { locator, file_name } => {
                    task.status = TaskStatus::Skipped;
                    task.locator = Some(locator);
                    task.file_name = Some(file_name);
                    task.error = None;
                }
                Outcome::Failed(error) => {
                    task.status = TaskStatus::Failed;
                    task.error = Some(error);
                }
                // The status is already whatever the interrupter set. Leaving it
                // alone is deliberate: a pause that raced the last chunk must not
                // be overwritten back into `downloading`.
                Outcome::Interrupted => {}
            }
            let snapshot = task.clone();
            inner.emit(DownloadEvent::Updated { task: snapshot });
        }
    }
    pump(&app);
}

async fn execute<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    cancel: &Arc<AtomicBool>,
) -> Outcome {
    let Some(task) = app.state::<DownloadState>().inner.lock().snapshot_of(id) else {
        return Outcome::Interrupted;
    };
    let target = match resolve_target(app) {
        Ok(target) => target,
        Err(err) => return Outcome::Failed(err),
    };
    let source_id = ensure_source(app, &target);

    let resolved = match resolve_url(app, task.song_id, task.br).await {
        Ok(resolved) => resolved,
        Err(err) => return Outcome::Failed(err),
    };
    if cancel.load(Ordering::Relaxed) {
        return Outcome::Interrupted;
    }
    let mut resolved = resolved;
    let mut retried = false;

    let config = app.state::<DownloadState>().config();
    let file_name = config::render_filename(
        &config.filename_template,
        &task.artist,
        &task.title,
        &task.album,
        task.index,
        &resolved.extension,
    );

    let existing = match sink::probe(app, &target, &file_name) {
        Ok(existing) => existing,
        Err(err) => return Outcome::Failed(err),
    };
    // Size is the only cheap evidence that the file on disk is the same download.
    // Without a declared size there is nothing to compare, so the transfer runs
    // and overwrites — the wrong call in the other direction would be to skip a
    // file that is actually a different quality.
    if let Some(found) = &existing {
        if resolved.size > 0 && found.size == Some(resolved.size) {
            let locator = found.locator.clone();
            // Sidecars only. The file is the one an earlier run already published,
            // so its tags are that run's business — rewriting a multi-megabyte
            // container to restate what is in it is not what "already downloaded"
            // should cost.
            let extras = fetch_extras(app, &task, config.write_lyric, config.write_cover).await;
            let lyric = write_sidecars(app, &target, &file_name, &extras, &config).await;
            index_file(app, &source_id, &locator, &file_name, found.size, lyric).await;
            store_companions(app, &locator, &extras).await;
            return Outcome::Skipped { locator, file_name };
        }
    }

    // Decided before the row flips to `downloading`, so the split lands in the same
    // update the page already gets rather than costing an event of its own.
    let segments = match decide_segments(app, &task, &target, &file_name, &resolved, &config) {
        Ok(segments) => segments,
        Err(err) => return Outcome::Failed(err),
    };
    update(app, id, |task| {
        task.status = TaskStatus::Downloading;
        task.file_name = Some(file_name.clone());
        task.total = (resolved.size > 0).then_some(resolved.size);
        task.segments = segments;
    });

    let outcome = loop {
        match transfer(app, id, &target, &file_name, &resolved, segments, cancel).await {
            Ok(outcome) => break outcome,
            // A signed URL that expired between resolving and transferring is the
            // one failure worth handling silently — it is what a long queue does
            // to its own tail. Retrying the same URL cannot work, so this asks for
            // a new one and keeps the partial file; the *name* stays as it was,
            // since the bytes already on disk are under it.
            Err(err) if fetch::is_url_stale(&err) && !retried => {
                retried = true;
                log::info!(
                    target: "download",
                    "re-resolving {} after {err}",
                    task.song_id
                );
                update(app, id, |task| task.status = TaskStatus::Resolving);
                let fresh = match resolve_url(app, task.song_id, task.br).await {
                    Ok(fresh) => fresh,
                    Err(err) => return Outcome::Failed(err),
                };
                resolved.url = fresh.url;
                resolved.size = fresh.size;
                update(app, id, |task| task.status = TaskStatus::Downloading);
            }
            Err(err) => return Outcome::Failed(err),
        }
    };
    let parts = match outcome {
        TransferOutcome::Finished { parts, .. } => parts,
        TransferOutcome::Interrupted { locators } => {
            // A cancel took the row out of the table, and nothing will ever come
            // back for the partial files, so they go. A *pause* keeps them — that is
            // the entire difference between the two.
            if app.state::<DownloadState>().inner.lock().snapshot_of(id).is_none() {
                for locator in &locators {
                    sink::discard(app, &target, locator);
                }
            }
            return Outcome::Interrupted;
        }
    };

    // Fetched before the commit because the tags are written *inside* it, while the
    // bytes are still under `.part` — see `tags`. The sidecars below then reuse
    // exactly these bytes instead of asking again.
    let extras = fetch_extras(
        app,
        &task,
        config.write_lyric || config.embed_tags,
        config.write_cover || config.embed_tags,
    )
    .await;
    let tags = config.embed_tags.then(|| tags::Tags {
        title: task.title.clone(),
        artist: task.artist.clone(),
        album: task.album.clone(),
        lyric: extras.lyric.clone(),
        cover: extras.cover.clone(),
    });

    // Off-thread because this is the blocking half: an `fsync`, the rename, the tag
    // rewrite, and — for a split download — concatenating the parts into the first
    // of them.
    let published = {
        let app_for_commit = app.clone();
        let target = target.clone();
        let replacing = existing.as_ref().map(|found| found.locator.clone());
        let joined = tauri::async_runtime::spawn_blocking(move || {
            sink::commit_parts(
                &app_for_commit,
                &target,
                parts,
                replacing.as_deref(),
                tags.as_ref(),
            )
        })
        .await;
        match joined {
            Ok(Ok(published)) => published,
            Ok(Err(err)) => return Outcome::Failed(err),
            Err(err) => return Outcome::Failed(err.to_string()),
        }
    };

    // Sidecars first, so the lyric locator can go into the row the next line
    // writes. `Listing::note_lyric` only pairs a `.lrc` during a *full* walk, so a
    // single-file registration has to carry the pairing itself — otherwise the
    // downloaded lyric would not attach until some later re-scan.
    let lyric = write_sidecars(app, &target, &published.display_name, &extras, &config).await;
    index_file(
        app,
        &source_id,
        &published.locator,
        &published.display_name,
        Some(published.size),
        lyric,
    )
    .await;
    store_companions(app, &published.locator, &extras).await;

    Outcome::Done {
        locator: published.locator,
        file_name: published.display_name,
        size: published.size,
    }
}

enum TransferOutcome {
    /// Every byte is on disk. The size is deliberately not carried: the file grows
    /// again when `commit_parts` writes the tags, and `Published::size` is the only
    /// length the library should ever be told about.
    Finished { parts: sink::Parts },
    /// Paused or cancelled. Carries every partial file's locator so the caller can
    /// decide whether they survive.
    Interrupted { locators: Vec<String> },
}

enum StreamOutcome {
    Complete { parts: sink::Parts },
    Interrupted { locators: Vec<String> },
    RangeRefused { locators: Vec<String> },
}

/// What one lane of a split transfer ended up doing.
enum Lane {
    Complete,
    Interrupted,
    RangeRefused,
}

/// How many streams to split this task's file across, decided once and remembered.
///
/// Three things can turn a split back off, and none of them is an error:
/// the file is too small or its size was never declared ([`fetch::segments_for`]);
/// an earlier *unsplit* attempt left resumable bytes behind, which are worth more
/// than the split; or the shared stream budget is exhausted, in which case n
/// segments would run down one connection as n round trips where one would do.
fn decide_segments<R: Runtime>(
    app: &AppHandle<R>,
    task: &DownloadTask,
    target: &DownloadTarget,
    file_name: &str,
    resolved: &Resolved,
    config: &DownloadConfig,
) -> Result<usize, String> {
    if task.segments > 0 {
        return Ok(task.segments);
    }
    let mut segments = fetch::segments_for(resolved.size, config.segments);
    if segments > 1 && sink::unsplit_progress(app, target, file_name)? > 0 {
        segments = 1;
    }
    if segments > 1 && app.state::<DownloadState>().inner.lock().spare_streams() == 0 {
        segments = 1;
    }
    Ok(segments)
}

/// One expected length per segment, or a single `None` when the file is not split.
///
/// `None` is what keeps an unsplit transfer on the wire behaviour it always had:
/// no `Range` header on a fresh download, an open-ended one on a resume, and a
/// short body caught against `Content-Length` rather than against a length we
/// asked for.
fn segment_wants(size: u64, segments: usize) -> Vec<Option<u64>> {
    if segments <= 1 {
        return vec![None];
    }
    fetch::split(size, segments)
        .into_iter()
        .map(|span| Some(span.len))
        .collect()
}

/// Take up to `wanted` extra streams from the shared budget, returning how many.
fn reserve_extra_streams<R: Runtime>(app: &AppHandle<R>, wanted: usize) -> usize {
    let state = app.state::<DownloadState>();
    let mut inner = state.inner.lock();
    let taken = wanted.min(inner.spare_streams());
    inner.extra_streams += taken;
    taken
}

fn release_extra_streams<R: Runtime>(app: &AppHandle<R>, count: usize) {
    if count == 0 {
        return;
    }
    let state = app.state::<DownloadState>();
    let mut inner = state.inner.lock();
    inner.extra_streams = inner.extra_streams.saturating_sub(count);
}

/// Open the partial files and move their bytes, retrying once — unsplit — if the
/// server refuses to serve a range.
async fn transfer<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    target: &DownloadTarget,
    file_name: &str,
    resolved: &Resolved,
    segments: usize,
    cancel: &Arc<AtomicBool>,
) -> Result<TransferOutcome, String> {
    let mut wants = segment_wants(resolved.size, segments);
    for attempt in 1..=2 {
        let parts = sink::open_parts(app, target, file_name, &wants)?;
        // Every byte already on disk means no transfer is needed — the previous run
        // got the whole file and only the rename is left.
        if resolved.size > 0 && parts.received() >= resolved.size {
            return Ok(TransferOutcome::Finished { parts });
        }
        match run_parts(app, id, parts, &resolved.url, cancel).await? {
            StreamOutcome::Complete { parts } => {
                return Ok(TransferOutcome::Finished { parts })
            }
            StreamOutcome::Interrupted { locators } => {
                return Ok(TransferOutcome::Interrupted { locators })
            }
            StreamOutcome::RangeRefused { locators } => {
                // The server sent the whole file from byte zero. The handles are
                // append-only by design, so the partial files have to go and the
                // transfer starts over — once, then it is an error rather than a
                // loop. Unsplit the second time: a server that will not serve one
                // range will not serve several either.
                for locator in &locators {
                    sink::discard(app, target, locator);
                }
                update(app, id, |task| {
                    task.received = 0;
                    task.segments = 1;
                });
                if attempt == 2 {
                    return Err("the server would not resume this download".to_string());
                }
                wants = vec![None];
            }
        }
    }
    unreachable!("the loop either returns or exhausts its two attempts")
}

/// Run every segment of `parts`, as many at a time as the stream budget allows.
///
/// The segments are a work queue rather than one lane each: the split is
/// deterministic — it names the files — so the budget may only decide how many run
/// at once. With no budget at all this is a single lane walking the segments in
/// order, which is a correct if pointless sequential download.
async fn run_parts<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    parts: sink::Parts,
    url: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<StreamOutcome, String> {
    let sink::Parts { files, final_name } = parts;
    let count = files.len();
    let locators: Vec<String> = files.iter().map(|part| part.locator.clone()).collect();
    let declared: Option<u64> = files.iter().map(|part| part.want).sum();
    let total = declared.filter(|size| *size > 0);
    // Where each part's bytes begin in the remote resource: the running sum of the
    // spans ahead of it, and zero for the one part of an unsplit download.
    let mut starts = Vec::with_capacity(count);
    let mut at = 0u64;
    for part in &files {
        starts.push(at);
        at += part.want.unwrap_or(0);
    }

    let received = Arc::new(AtomicU64::new(files.iter().map(|part| part.offset).sum()));
    let throttle = Arc::new(Mutex::new(Throttle::new()));
    let abort = Arc::new(AtomicBool::new(false));
    let next = Arc::new(AtomicUsize::new(0));
    let slots: Arc<Vec<Mutex<Option<sink::PartFile>>>> =
        Arc::new(files.into_iter().map(|part| Mutex::new(Some(part))).collect());

    // One lane is already paid for — `spare_streams` charges every running task a
    // stream whether it opened it or not — so only the rest is reserved, and it is
    // given back when the lanes finish rather than when the task does.
    let extra = reserve_extra_streams(app, count.saturating_sub(1));
    let mut lanes = Vec::with_capacity(1 + extra);
    for _ in 0..(1 + extra).min(count) {
        lanes.push(tauri::async_runtime::spawn(run_lane(
            app.clone(),
            id.to_string(),
            url.to_string(),
            starts.clone(),
            slots.clone(),
            next.clone(),
            cancel.clone(),
            abort.clone(),
            received.clone(),
            throttle.clone(),
            total,
            count == 1,
        )));
    }

    let mut interrupted = false;
    let mut refused = false;
    let mut error: Option<String> = None;
    for lane in lanes {
        match lane.await {
            Ok(Ok(Lane::Complete)) => {}
            Ok(Ok(Lane::Interrupted)) => interrupted = true,
            Ok(Ok(Lane::RangeRefused)) => refused = true,
            Ok(Err(err)) => {
                error.get_or_insert(err);
            }
            // A panicked lane. Its part is still in its slot and the file on disk is
            // whatever it managed to append, so this is a failure the retry resumes
            // from like any other.
            Err(err) => {
                error.get_or_insert(err.to_string());
            }
        }
    }
    release_extra_streams(app, extra);

    // The parts come back before anything is answered: the caller needs them to
    // commit or to delete, and a slot left holding one would be a leaked
    // descriptor. Every lane has been awaited, so nothing else holds the `Arc`.
    let files: Vec<sink::PartFile> = Arc::try_unwrap(slots)
        .map_err(|_| "a download lane outlived its own transfer".to_string())?
        .into_iter()
        .filter_map(|slot| slot.into_inner())
        .collect();
    if files.len() != count {
        // A lane that panicked while holding a part. Unreachable without a
        // `JoinError` already in `error`, but a part quietly missing from the set
        // would be concatenated into a plausible-looking truncated file, so it is
        // worth saying out loud rather than inferring.
        error.get_or_insert_with(|| "a downloaded part went missing".to_string());
    }
    let parts = sink::Parts { files, final_name };

    if let Some(err) = error {
        return Err(err);
    }
    if refused {
        return Ok(StreamOutcome::RangeRefused { locators });
    }
    let received = parts.received();
    if interrupted {
        // What the parts actually hold, since a pause races the last chunk of every
        // lane that was still running.
        update(app, id, |task| task.received = received);
        return Ok(StreamOutcome::Interrupted { locators });
    }
    Ok(StreamOutcome::Complete { parts })
}

/// One lane of a transfer: claim the next segment, move it, repeat.
///
/// `single` says this is the only segment, in which case the length the response
/// declares *is* the file's total and worth adopting — a split transfer already
/// knows the total, and a segment's own length is not it.
#[allow(clippy::too_many_arguments)]
async fn run_lane<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    url: String,
    starts: Vec<u64>,
    slots: Arc<Vec<Mutex<Option<sink::PartFile>>>>,
    next: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    abort: Arc<AtomicBool>,
    received: Arc<AtomicU64>,
    throttle: Arc<Mutex<Throttle>>,
    total: Option<u64>,
    single: bool,
) -> Result<Lane, String> {
    let stop = || cancel.load(Ordering::Relaxed) || abort.load(Ordering::Relaxed);
    loop {
        if stop() {
            return Ok(Lane::Interrupted);
        }
        let index = next.fetch_add(1, Ordering::Relaxed);
        if index >= slots.len() {
            return Ok(Lane::Complete);
        }
        // `fetch_add` hands each index to exactly one lane, so the slot is always
        // occupied the first and only time it is claimed.
        let Some(mut part) = slots[index].lock().take() else {
            continue;
        };
        let segment = fetch::Segment {
            url: &url,
            start: starts[index],
            have: part.offset,
            want: part.want,
        };
        // A `tokio::fs::File` for the transfer, so the writes do not sit on a
        // runtime worker; handed back as a `std::fs::File` because
        // `sink::commit_parts` is the blocking half and owns the fsync, the
        // concatenation and the rename.
        let mut file = tokio::fs::File::from_std(part.file);
        let mut counted = part.offset;
        let result = fetch::stream(&segment, &mut file, &stop, |written, declared| {
            let delta = written.saturating_sub(counted);
            if delta == 0 {
                return;
            }
            counted = written;
            let now = received.fetch_add(delta, Ordering::Relaxed) + delta;
            let total = if single { declared } else { total };
            // Throttled for the same reason the scan's tagging progress is: one IPC
            // message per chunk is thousands of round trips the UI cannot render.
            // The throttle is shared across the lanes, so splitting a transfer does
            // not multiply the traffic by the number of streams.
            if throttle.lock().should_emit(now, total) {
                update(&app, &id, |task| {
                    task.received = now;
                    if total.is_some() {
                        task.total = total;
                    }
                });
            }
        })
        .await;

        // Flush and take the handle back *before* looking at the result. A
        // `tokio::fs::File` buffers writes and does not flush when dropped, so
        // bailing out on the error path first would lose the tail of a partial file
        // that the whole point of `.part` is to keep resumable.
        let _ = file.flush().await;
        part.file = file.into_std().await;

        match &result {
            Ok(fetch::Transfer::Complete { total }) => part.offset = *total,
            Ok(fetch::Transfer::Interrupted { written }) => part.offset = *written,
            _ => {}
        }
        *slots[index].lock() = Some(part);

        match result {
            Ok(fetch::Transfer::Complete { .. }) => continue,
            Ok(fetch::Transfer::Interrupted { .. }) => return Ok(Lane::Interrupted),
            // A lane that gives up tells the others, so a failure or a server that
            // will not serve ranges costs one round trip rather than one per lane.
            Ok(fetch::Transfer::RangeRefused) => {
                abort.store(true, Ordering::Relaxed);
                return Ok(Lane::RangeRefused);
            }
            Err(err) => {
                abort.store(true, Ordering::Relaxed);
                return Err(err);
            }
        }
    }
}

/// Shared, rate-limited progress reporting for one task.
struct Throttle {
    last_percent: i64,
    last_emitted: u64,
}

impl Throttle {
    fn new() -> Throttle {
        Throttle {
            last_percent: -1,
            last_emitted: 0,
        }
    }

    /// One event per whole percent, or per 512 KiB when the server declared no
    /// size.
    fn should_emit(&mut self, received: u64, total: Option<u64>) -> bool {
        match total {
            Some(total) if total > 0 => {
                let percent = (received * 100 / total) as i64;
                let changed = percent != self.last_percent;
                self.last_percent = percent;
                changed
            }
            _ => {
                let changed = received.saturating_sub(self.last_emitted) >= 512 * 1024;
                if changed {
                    self.last_emitted = received;
                }
                changed
            }
        }
    }
}

// ── Netease ──────────────────────────────────────────────────────

struct Resolved {
    url: String,
    /// Container extension as Netease reports it, lowercased. Decides the
    /// filename, and `local::scan` decides what is audio by extension alone.
    extension: String,
    /// 0 when the answer carried no size.
    size: u64,
}

/// Ask for a fresh signed URL.
///
/// Never cached: `song_download_url` is on `ncm-core`'s uncacheable list
/// precisely because the URL it hands back expires, so this is one round trip
/// every time and belongs at the start of a task rather than at enqueue.
async fn resolve_url<R: Runtime>(
    app: &AppHandle<R>,
    song_id: i64,
    br: u32,
) -> Result<Resolved, String> {
    let cookie = app.state::<DownloadState>().inner.lock().cookie.clone();
    let mut query = serde_json::json!({ "id": song_id, "br": br });
    if let Some(cookie) = cookie {
        query["cookie"] = serde_json::Value::String(cookie);
    }

    let body = call_ncm(app, "song_download_url", &query.to_string()).await?;
    let data = body
        .get("data")
        .ok_or_else(|| "netease answered without a data object".to_string())?;
    let url = data
        .get("url")
        .and_then(|url| url.as_str())
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| "this track is not downloadable for this account".to_string())?;
    // Same normalisation the page did: the CDN answers on both, and a plain-HTTP
    // URL would be blocked by the app's own network policy on Android.
    let url = match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    };
    let extension = data
        .get("type")
        .and_then(|kind| kind.as_str())
        .map(|kind| kind.trim().to_ascii_lowercase())
        .filter(|kind| !kind.is_empty())
        .unwrap_or_else(|| "mp3".to_string());
    let size = data.get("size").and_then(|size| size.as_u64()).unwrap_or(0);

    Ok(Resolved {
        url,
        extension,
        size,
    })
}

/// One call through the in-process protocol layer, unwrapped to its body.
///
/// Bounded, like every other caller of `NcmCore::call`: a hang here would leave
/// a task sitting in `resolving` forever with nothing in the log to say why.
async fn call_ncm<R: Runtime>(
    app: &AppHandle<R>,
    endpoint: &str,
    query: &str,
) -> Result<serde_json::Value, String> {
    let state = app.state::<crate::ncm::NcmState>();
    let core = state.core().await?;
    let envelope = tokio::time::timeout(CALL_TIMEOUT, core.call(endpoint, query))
        .await
        .map_err(|_| format!("{endpoint} did not answer within {CALL_TIMEOUT:?}"))?
        .map_err(|e| e.to_string())?;

    let parsed: serde_json::Value =
        serde_json::from_str(&envelope).map_err(|_| format!("{endpoint} answered malformed JSON"))?;
    if parsed.get("ok").and_then(|ok| ok.as_bool()) != Some(true) {
        let status = parsed.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
        return Err(format!("http {status}"));
    }
    parsed
        .get("body")
        .cloned()
        .ok_or_else(|| format!("{endpoint} answered without a body"))
}

/// The lyric documents Netease serves for one track.
struct FetchedLyric {
    /// Plain LRC. The only one that becomes a `.lrc` sidecar or an embedded tag.
    lrc: Option<String>,
    /// Translation, and below it the romanisation.
    ///
    /// Separate documents upstream and separate here, because they only ever render
    /// *aligned onto* the lines above — so they go to the library's own lyric store
    /// rather than into the audio file. A container has one lyric field our reader
    /// can find again (symphonia folds every `USLT` into one), and a `.lrc` sidecar
    /// is a plain lyric by definition.
    translation: Option<String>,
    romanisation: Option<String>,
}

/// Fetch every lyric document for a track in one request.
///
/// `lyric_new` answers with all of them at once, so keeping the translation and the
/// romanisation costs nothing beyond the `lrc` this used to take.
async fn fetch_lyric<R: Runtime>(app: &AppHandle<R>, song_id: i64) -> FetchedLyric {
    let query = serde_json::json!({ "id": song_id }).to_string();
    let body = match call_ncm(app, "lyric_new", &query).await {
        Ok(body) => body,
        Err(err) => {
            log::debug!(target: "download", "no lyric for {song_id}: {err}");
            return FetchedLyric {
                lrc: None,
                translation: None,
                romanisation: None,
            };
        }
    };
    let document = |name: &str| {
        body.get(name)
            .and_then(|doc| doc.get("lyric"))
            .and_then(|lyric| lyric.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    FetchedLyric {
        lrc: document("lrc"),
        translation: document("tlyric"),
        romanisation: document("romalrc"),
    }
}

// ── Sidecars and the library ─────────────────────────────────────

/// The lyric and the cover, fetched once for both the tags and the sidecars.
///
/// Fetched once because they are two network requests and up to three consumers —
/// the `.lrc`, the `.jpg`, and the tags written into the audio itself — and asking
/// twice for the same picture on every track is the kind of thing that only shows
/// up as "downloads got slower".
#[derive(Debug, Default)]
struct Extras {
    lyric: Option<String>,
    /// Translation and romanisation. Kept whenever the lyric is fetched at all —
    /// they ride in the same response — and written only to the library's lyric
    /// store, never to the `.lrc` sidecar or the audio file.
    translation: Option<String>,
    romanisation: Option<String>,
    cover: Option<Vec<u8>>,
}

async fn fetch_extras<R: Runtime>(
    app: &AppHandle<R>,
    task: &DownloadTask,
    want_lyric: bool,
    want_cover: bool,
) -> Extras {
    let lyric = if want_lyric {
        fetch_lyric(app, task.song_id).await
    } else {
        FetchedLyric {
            lrc: None,
            translation: None,
            romanisation: None,
        }
    };
    let cover = match task.cover_url.as_deref().filter(|_| want_cover) {
        Some(url) => match fetch::bytes(url).await {
            Ok(bytes) if !bytes.is_empty() => Some(bytes),
            Ok(_) => None,
            Err(err) => {
                log::warn!(target: "download", "could not fetch cover art: {err}");
                None
            }
        },
        None => None,
    };
    Extras {
        lyric: lyric.lrc,
        translation: lyric.translation,
        romanisation: lyric.romanisation,
        cover,
    }
}

/// Write the `.lrc` and, if asked, the cover. Returns the lyric's locator.
///
/// Every failure here is logged and swallowed: the audio file is already on disk
/// and playable, and turning a missing lyric into a failed download would be a
/// worse answer than a track with no words.
async fn write_sidecars<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    audio_name: &str,
    extras: &Extras,
    config: &DownloadConfig,
) -> Option<String> {
    let stem = audio_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(audio_name)
        .to_string();

    let mut lyric_locator = None;
    if config.write_lyric {
        if let Some(text) = extras.lyric.as_deref() {
            let name = format!("{stem}.lrc");
            match write_bytes(app, target, &name, text.as_bytes().to_vec()).await {
                Ok(locator) => lyric_locator = Some(locator),
                Err(err) => {
                    log::warn!(target: "download", "could not write {name}: {err}");
                }
            }
        }
    }

    if config.write_cover {
        if let Some(bytes) = extras.cover.as_deref() {
            // Per track, not `cover.jpg` per directory: the download folder is flat,
            // so one shared name would be overwritten by every song that landed
            // after it.
            let name = format!("{stem}.jpg");
            if let Err(err) = write_bytes(app, target, &name, bytes.to_vec()).await {
                log::warn!(target: "download", "could not write {name}: {err}");
            }
        }
    }

    lyric_locator
}

/// Put the translation and the romanisation in the library's lyric store.
///
/// Off-thread because it writes two small files and then rewrites
/// `local-user-data.json`. Best-effort: the track is already indexed and playable.
async fn store_companions<R: Runtime>(app: &AppHandle<R>, locator: &str, extras: &Extras) {
    if extras.translation.is_none() && extras.romanisation.is_none() {
        return;
    }
    let app = app.clone();
    let locator = locator.to_string();
    let translation = extras.translation.clone();
    let romanisation = extras.romanisation.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        crate::local::store_lyric_companions(
            &app,
            &locator,
            translation.as_deref(),
            romanisation.as_deref(),
        );
    })
    .await;
    if let Err(err) = joined {
        log::warn!(target: "download", "storing the lyric companions failed: {err}");
    }
}

async fn write_bytes<R: Runtime>(
    app: &AppHandle<R>,
    target: &DownloadTarget,
    name: &str,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let app = app.clone();
    let target = target.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn_blocking(move || sink::write_sibling(&app, &target, &name, &bytes))
        .await
        .map_err(|e| e.to_string())?
}

/// Put the finished file in the local library straight away.
///
/// Off-thread because it runs a symphonia tag probe and a cover extraction, then
/// an `fsync` of the whole index — the most expensive thing in this pipeline
/// after the transfer itself.
async fn index_file<R: Runtime>(
    app: &AppHandle<R>,
    source_id: &str,
    locator: &str,
    display_name: &str,
    size: Option<u64>,
    lyric_key: Option<String>,
) {
    let app_for_index = app.clone();
    let source_id = source_id.to_string();
    let locator = locator.to_string();
    let display_name = display_name.to_string();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        crate::local::index_downloaded_file(
            &app_for_index,
            &source_id,
            &locator,
            &display_name,
            size,
            lyric_key,
        )
    })
    .await;
    match joined {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            // The file is on disk either way, and a full re-scan of the download
            // folder will pick it up — so this is a "did not appear immediately"
            // failure rather than a lost download.
            log::warn!(target: "download", "downloaded file could not be indexed: {err}");
        }
        Err(err) => log::warn!(target: "download", "indexing task failed: {err}"),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Bound on one protocol-layer call. Matches `ncm::CALL_TIMEOUT`, for the same
/// reason: a call that never settles must not park a task forever.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);
