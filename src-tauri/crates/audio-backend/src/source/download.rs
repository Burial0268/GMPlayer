//! HTTP bodies land through the shared growing-file source protocol.

use crate::error::{AudioError, AudioResult};

#[cfg(test)]
mod tests;
use crate::source::{cache, growing};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Bodies under this are downloaded whole before returning: the handshake for a
/// background writer costs more than it saves on a file that lands in a moment.
const STREAM_MIN_BYTES: u64 = 2 * 1024 * 1024;

/// A body silent this long is a dead connection, not a slow one; with resuming, giving up on it
/// sooner costs a reconnect rather than the track.
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(20);

/// Failed attempts in a row, none of which landed a byte, before a download is given up. Any
/// attempt that lands bytes starts the count again, so a slow but live connection never runs out.
const RESUME_ATTEMPTS: u32 = 6;

/// The wait before the first resume, doubling with each attempt that lands nothing.
#[cfg(not(test))]
const RESUME_BACKOFF: Duration = Duration::from_millis(500);
#[cfg(test)]
const RESUME_BACKOFF: Duration = Duration::from_millis(10);

/// Download `url` to a temporary file. The returned guard deletes the file when
/// dropped — keep it alive for the lifetime of playback.
///
/// When the server declares a `Content-Length` of at least `STREAM_MIN_BYTES`
/// this returns as soon as the response headers are in, with the body still
/// landing on a writer thread. The path is registered with `source`, so every
/// `source::open` of it waits for bytes rather than seeing EOF; dropping the
/// guard stops the writer. Anything else is downloaded whole, as it always was.
///
/// The agent is built per call, and must stay that way. Sharing one — i.e.
/// pooling the connection between track downloads — saves a TLS handshake per
/// track and was tried; the next track then decoded as the *previous* track's
/// length plus its own, which is what a reused connection handing over an
/// undrained body looks like. A whole audio body is the one response here, so
/// any framing slip corrupts the file rather than erroring, and the file is the
/// thing playback is built on. One handshake per track, on a path the planner
/// already runs a track ahead, is not worth that. Streaming keeps that
/// guarantee by reading exactly `Content-Length` bytes and failing on a short
/// body, never by trusting the connection to be at a boundary.
pub fn download_to_temp_path(url: &str) -> AudioResult<crate::source::TempGuard> {
    fetch(url, None)
}

/// [`download_to_temp_path`], through the audio cache. `md5` is the hash the resolver declared
/// for this file: a kept copy of it is used instead of the network, and a new download is kept
/// once it has landed and hashes to it. Without one this is exactly a download.
pub fn fetch(url: &str, md5: Option<&str>) -> AudioResult<crate::source::TempGuard> {
    use crate::source::growing::Progress;
    use std::io::Read;

    let md5 = cache::normalize_md5(md5);
    if let Some((path, hold)) = md5.as_deref().and_then(cache::lookup) {
        tracing::info!("audio cache hit: {}", md5.as_deref().unwrap_or_default());
        return Ok(crate::source::TempGuard::cached(path, hold));
    }

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(BODY_READ_TIMEOUT)
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| AudioError::Download(format!("Request failed: {e}")))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= STREAM_MIN_BYTES);

    let mut temp = match cache::download_dir() {
        Some(dir) => tempfile::Builder::new()
            .prefix(cache::DOWNLOAD_PREFIX)
            .suffix(".tmp")
            .tempfile_in(dir),
        None => tempfile::Builder::new()
            .prefix("gmplayer-audio-")
            .suffix(".tmp")
            .tempfile(),
    }
    .map_err(|e| AudioError::Download(format!("Create temp file: {e}")))?;
    let hold = md5.map(cache::Hold::pending);
    let committer = hold
        .as_ref()
        .map(|hold| hold.committer(temp.path().to_path_buf()));

    let mut reader = response.into_reader();
    let Some(total) = total else {
        std::io::copy(&mut reader, &mut temp)
            .map_err(|e| AudioError::Download(format!("Write temp file: {e}")))?;
        let guard = crate::source::TempGuard::from(temp.into_temp_path());
        return Ok(match (hold, committer) {
            (Some(hold), Some(committer)) => {
                committer.commit();
                guard.with_cache(hold)
            }
            _ => guard,
        });
    };

    let (file, path) = temp.into_parts();
    let progress = Progress::new(total);
    growing::register(&path, progress.clone());

    // A forward seek far past the head writer starts a second one at the target.
    let (jump_url, jump_path) = (url.to_string(), path.to_path_buf());
    let jump_committer = committer.clone();
    progress.set_jumper(Box::new(move |progress, id, start| {
        let (url, path) = (jump_url.clone(), jump_path.clone());
        let committer = jump_committer.clone();
        let worker = Arc::clone(&progress);
        let spawned = std::thread::Builder::new()
            .name("audio-download-range".into())
            .spawn(move || {
                let outcome = fill_range(&worker, &url, &path, id, start);
                keep_writing(&worker, &url, &path, id, outcome);
                commit_if_complete(&worker, committer.as_ref());
            });
        if let Err(e) = spawned {
            progress.end_writer(id, Err(format!("Start range thread: {e}")));
        }
    }));

    let (head_url, head_path) = (url.to_string(), path.to_path_buf());
    let mut guard = crate::source::TempGuard::streaming(path, progress.clone());
    if let Some(hold) = hold {
        guard = guard.with_cache(hold);
    }
    let head = progress.begin_writer(0);
    let writer = progress.clone();
    let spawned = std::thread::Builder::new()
        .name("audio-download".into())
        .spawn(move || {
            let outcome =
                growing::pump(&writer, head, file, reader.take(total)).map_err(RangeError::Failed);
            keep_writing(&writer, &head_url, &head_path, head, outcome);
            commit_if_complete(&writer, committer.as_ref());
        });
    spawned.map_err(|e| AudioError::Download(format!("Start download thread: {e}")))?;

    Ok(guard)
}

/// Whichever writer finishes the file offers it to the cache; the commit itself runs once.
fn commit_if_complete(progress: &growing::Progress, committer: Option<&cache::Committer>) {
    if let Some(committer) = committer.filter(|_| progress.is_complete()) {
        committer.commit();
    }
}

/// Why a writer stopped short: a server that will not serve a `Range` cannot be resumed, a
/// connection that failed can.
enum RangeError {
    Refused(String),
    Failed(String),
}

impl RangeError {
    fn into_message(self) -> String {
        match self {
            Self::Refused(e) | Self::Failed(e) => e,
        }
    }
}

/// Writer `id`, registered at `start`, fills from a `Range` request.
fn fill_range(
    progress: &Arc<growing::Progress>,
    url: &str,
    path: &Path,
    id: u64,
    start: u64,
) -> Result<(), RangeError> {
    let body = open_range(url, start, progress.total())?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| RangeError::Failed(format!("Open temp file: {e}")))?;
    growing::pump(progress, id, file, body).map_err(RangeError::Failed)
}

/// Finish writer `id`, resuming it while it fails. A body cut off part-way — a dropped or stalled
/// connection, routine on a slow mobile network — used to fail the whole download, and the
/// decoder took the error for the end of the song: the track stopped where the connection did,
/// and AutoMix or the queue moved on as if it had finished. Each resume is a new writer at the
/// failed one's position, registered before the old one is retired so the download never reads as
/// abandoned in between; readers just keep waiting.
fn keep_writing(
    progress: &Arc<growing::Progress>,
    url: &str,
    path: &Path,
    mut id: u64,
    mut outcome: Result<(), RangeError>,
) {
    let mut failures = 0u32;
    while let Err(RangeError::Failed(error)) = &outcome {
        if progress.is_cancelled() {
            break;
        }
        let pos = progress.writer_pos(id);
        failures = if pos > progress.writer_start(id) {
            1
        } else {
            failures + 1
        };
        if failures > RESUME_ATTEMPTS {
            break;
        }
        tracing::warn!(
            "download interrupted at {pos} of {} bytes, resuming (attempt {failures}): {error}",
            progress.total()
        );
        if !sleep_unless_cancelled(progress, RESUME_BACKOFF * (1 << (failures - 1))) {
            break;
        }
        let next = progress.begin_writer(pos);
        progress.end_writer(id, Ok(()));
        id = next;
        outcome = fill_range(progress, url, path, id, pos);
    }
    if let Err(error) = &outcome {
        if !progress.is_cancelled() {
            let (RangeError::Refused(e) | RangeError::Failed(e)) = error;
            tracing::warn!("download writer gave up: {e}");
        }
    }
    progress.end_writer(id, outcome.map_err(RangeError::into_message));
}

fn sleep_unless_cancelled(progress: &growing::Progress, wait: Duration) -> bool {
    let until = std::time::Instant::now() + wait;
    loop {
        if progress.is_cancelled() {
            return false;
        }
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// Open `bytes=start-` of `url`. A server that answers anything but a matching
/// `206` is refused outright: the bytes would be appended at the offset we
/// asked for regardless of where they came from, which is a corrupt track
/// rather than a failed request. Per call, like the main download, for the same
/// reason.
fn open_range(
    url: &str,
    start: u64,
    total: u64,
) -> Result<Box<dyn std::io::Read + Send>, RangeError> {
    use std::io::Read;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(BODY_READ_TIMEOUT)
        .build();
    let response = agent
        .get(url)
        .set("Range", &format!("bytes={start}-"))
        .call()
        .map_err(|e| RangeError::Failed(format!("Range request failed: {e}")))?;
    if response.status() != 206 {
        return Err(RangeError::Refused(format!(
            "server answered {} to a Range request",
            response.status()
        )));
    }
    // "bytes <start>-<end>/<total>"
    let range = response.header("Content-Range").unwrap_or_default();
    let matches = range
        .strip_prefix("bytes ")
        .and_then(|r| r.split_once('/'))
        .and_then(|(span, t)| {
            Some((
                span.split_once('-')?.0.parse::<u64>().ok()?,
                t.parse::<u64>().ok()?,
            ))
        })
        .is_some_and(|(s, t)| s == start && t == total);
    if !matches {
        return Err(RangeError::Refused(format!(
            "Content-Range {range:?} does not start at {start} of {total}"
        )));
    }
    Ok(Box::new(response.into_reader().take(total - start)))
}
