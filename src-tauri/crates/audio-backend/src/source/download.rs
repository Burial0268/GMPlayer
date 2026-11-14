//! HTTP bodies land through the shared growing-file source protocol.

use crate::error::{AudioError, AudioResult};

#[cfg(test)]
mod tests;
use std::sync::Arc;
use std::time::Duration;

/// Bodies under this are downloaded whole before returning: the handshake for a
/// background writer costs more than it saves on a file that lands in a moment.
const STREAM_MIN_BYTES: u64 = 2 * 1024 * 1024;

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
    use crate::source::growing::{self, Progress};
    use std::io::Read;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| AudioError::Download(format!("Request failed: {e}")))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= STREAM_MIN_BYTES);

    let mut temp = tempfile::Builder::new()
        .prefix("gmplayer-audio-")
        .suffix(".tmp")
        .tempfile()
        .map_err(|e| AudioError::Download(format!("Create temp file: {e}")))?;

    let mut reader = response.into_reader();
    let Some(total) = total else {
        std::io::copy(&mut reader, &mut temp)
            .map_err(|e| AudioError::Download(format!("Write temp file: {e}")))?;
        return Ok(temp.into_temp_path().into());
    };

    let (file, path) = temp.into_parts();
    let progress = Progress::new(total);
    growing::register(&path, progress.clone());

    // A forward seek far past the head writer starts a second one at the target.
    let (jump_url, jump_path) = (url.to_string(), path.to_path_buf());
    progress.set_jumper(Box::new(move |progress, id, start| {
        let (url, path) = (jump_url.clone(), jump_path.clone());
        let worker = Arc::clone(&progress);
        let spawned = std::thread::Builder::new()
            .name("audio-download-range".into())
            .spawn(move || {
                let outcome = open_range(&url, start, worker.total()).and_then(|body| {
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .map_err(|e| format!("Open temp file: {e}"))?;
                    growing::pump(&worker, id, file, body)
                });
                worker.end_writer(id, outcome);
            });
        if let Err(e) = spawned {
            progress.end_writer(id, Err(format!("Start range thread: {e}")));
        }
    }));

    let guard = crate::source::TempGuard::streaming(path, progress.clone());
    let head = progress.begin_writer(0);
    let writer = progress.clone();
    let spawned = std::thread::Builder::new()
        .name("audio-download".into())
        .spawn(move || {
            let outcome = growing::pump(&writer, head, file, reader.take(total));
            writer.end_writer(head, outcome);
        });
    spawned.map_err(|e| AudioError::Download(format!("Start download thread: {e}")))?;

    Ok(guard)
}

/// Open `bytes=start-` of `url`. A server that answers anything but a matching
/// `206` is refused outright: the bytes would be appended at the offset we
/// asked for regardless of where they came from, which is a corrupt track
/// rather than a failed request. Per call, like the main download, for the same
/// reason.
fn open_range(url: &str, start: u64, total: u64) -> Result<Box<dyn std::io::Read + Send>, String> {
    use std::io::Read;

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(url)
        .set("Range", &format!("bytes={start}-"))
        .call()
        .map_err(|e| format!("Range request failed: {e}"))?;
    if response.status() != 206 {
        return Err(format!(
            "server answered {} to a Range request",
            response.status()
        ));
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
        return Err(format!(
            "Content-Range {range:?} does not start at {start} of {total}"
        ));
    }
    Ok(Box::new(response.into_reader().take(total - start)))
}
