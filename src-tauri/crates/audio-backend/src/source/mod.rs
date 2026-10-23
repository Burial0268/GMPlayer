//! One place that turns a *source locator* into something symphonia can read.
//!
//! Three call sites used to hold their own `File::open` — the decoder, the
//! metadata probe and the AutoMix analyzer — which is fine as long as every
//! locator is a filesystem path. Android's Storage Access Framework hands out
//! `content://` URIs instead: there is no path, only a file descriptor obtained
//! through `ContentResolver`. Converging the three on this module is what makes
//! a local track on Android decode, report its tags *and* get a crossfade;
//! missing any one of them fails silently in a different subsystem.
//!
//! The `content://` half cannot live here. This crate also builds for
//! `wasm32-unknown-unknown`, so it must not depend on Tauri's Android plugin
//! machinery; the host app installs a provider instead, exactly like
//! `install_ncm_call_hook` does for the NCM protocol layer, and for the same
//! reason.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use symphonia::core::io::MediaSource;

use crate::error::{AudioError, AudioResult};

// ── Locator ──────────────────────────────────────────────────────

/// Where a track's bytes come from. Borrowed on purpose: classification happens
/// on a hot-ish path (every load, every prepare) and must not allocate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLocator<'a> {
    /// A filesystem path. Desktop local files, and every downloaded temp file.
    Path(&'a Path),
    /// An `http(s)` URL, downloaded to a temp file before decoding.
    Http(&'a str),
    /// An Android SAF `content://` URI, opened through the installed provider.
    Content(&'a str),
}

/// Whether `s` is an Android SAF content URI.
pub fn is_content_uri(s: &str) -> bool {
    s.starts_with("content://")
}

impl<'a> SourceLocator<'a> {
    /// Classify a locator string. `content://` and `http(s)://` are recognised
    /// by scheme; everything else is a path.
    pub fn classify(s: &'a str) -> Self {
        if is_content_uri(s) {
            SourceLocator::Content(s)
        } else if crate::decoder::is_http_url(s) {
            SourceLocator::Http(s)
        } else {
            SourceLocator::Path(Path::new(s))
        }
    }

    /// Classify a locator that is already carried as a `PathBuf`.
    ///
    /// The player threads locators through `PathBuf` end to end, and a
    /// `content://` URI survives that round trip unchanged on Unix (an
    /// `OsString` is bytes, `PathBuf::from` normalises nothing). Non-UTF-8
    /// paths cannot be URIs, so they classify as paths.
    pub fn from_path(path: &'a Path) -> Self {
        match path.to_str() {
            Some(s) => Self::classify(s),
            None => SourceLocator::Path(path),
        }
    }

    /// The locator in string form, when it has one.
    pub fn as_str(&self) -> Option<&'a str> {
        match self {
            SourceLocator::Path(p) => p.to_str(),
            SourceLocator::Http(s) | SourceLocator::Content(s) => Some(s),
        }
    }

    /// Filename extension, for symphonia's `Hint`.
    ///
    /// symphonia 0.5's `Probe::format` ignores the hint entirely (format
    /// detection is a magic-byte scan), so this is forward-compatibility rather
    /// than a correctness dependency — but a `content://` URI has no
    /// `Path::extension`, and deriving one from the last percent-encoded
    /// segment costs nothing.
    pub fn extension(&self) -> Option<String> {
        match self {
            SourceLocator::Path(p) => p.extension().and_then(|e| e.to_str()).map(str::to_string),
            SourceLocator::Http(s) | SourceLocator::Content(s) => extension_from_uri(s),
        }
    }
}

fn extension_from_uri(uri: &str) -> Option<String> {
    let tail = uri.split(['?', '#']).next().unwrap_or(uri);
    let last = tail.rsplit(['/', '%']).next().unwrap_or(tail);
    let ext = last.rsplit_once('.')?.1;
    if ext.is_empty() || ext.len() > 8 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

// ── Content provider (installed by the host app) ─────────────────

/// A file descriptor handed over by `ContentResolver.openFileDescriptor`.
///
/// Ownership transfers with the value: Kotlin has already called `detachFd()`
/// and will not close it. `open` wraps it in a `File` on the first line of the
/// branch that receives it, so the descriptor is closed by `Drop` on every exit
/// path — a leak here costs one fd per failed track, and a bulk scan of a few
/// hundred files reaches the per-process limit.
#[derive(Debug, Clone, Copy)]
pub struct ContentFd {
    pub fd: i32,
    /// Whether the descriptor refers to a regular file, i.e. whether `lseek`
    /// works. Cloud-backed document providers hand out pipes, on which
    /// symphonia's probe fails at the first backward seek.
    pub is_regular_file: bool,
}

/// The host app's bridge to `ContentResolver`. Installed once at setup.
pub trait ContentSourceProvider: Send + Sync {
    /// Open `uri` for reading. The returned descriptor is owned by the caller.
    fn open_fd(&self, uri: &str) -> Result<ContentFd, String>;

    /// Whether `uri` is still readable (the document exists and our persisted
    /// permission survives). The default implementation opens and immediately
    /// closes, which is correct everywhere but costs a full open; providers
    /// that can answer from a cheap `query` should override it.
    fn exists(&self, uri: &str) -> bool {
        match self.open_fd(uri) {
            Ok(handle) => {
                drop(adopt_fd(handle.fd));
                true
            }
            Err(_) => false,
        }
    }
}

static CONTENT_PROVIDER: OnceLock<Arc<dyn ContentSourceProvider>> = OnceLock::new();

/// Install the `content://` bridge. First install wins, mirroring
/// `install_ncm_call_hook`; returns whether this call was the one that landed.
pub fn install_content_source_provider(provider: Arc<dyn ContentSourceProvider>) -> bool {
    CONTENT_PROVIDER.set(provider).is_ok()
}

/// Whether a `content://` bridge has been installed.
pub fn has_content_source_provider() -> bool {
    CONTENT_PROVIDER.get().is_some()
}

fn content_provider() -> Option<&'static Arc<dyn ContentSourceProvider>> {
    CONTENT_PROVIDER.get()
}

/// Wrap a raw descriptor in a `File`, taking ownership of it.
#[cfg(unix)]
fn adopt_fd(fd: i32) -> Option<std::fs::File> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        return None;
    }
    // SAFETY: the provider contract is that `detachFd()` has already been
    // called, so this descriptor has exactly one owner and it is now us.
    Some(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn adopt_fd(_fd: i32) -> Option<std::fs::File> {
    None
}

// ── Stream cache (non-seekable providers) ────────────────────────

static STREAM_CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where to spool a non-seekable `content://` stream so it can be decoded.
///
/// Optional: without it the copy lands in the OS temp directory and is deleted
/// when playback releases it, which is correct but re-copies for every open
/// (analysis and playback each open the track). Pointing this at the app cache
/// directory makes the copy survive between them.
pub fn set_stream_cache_dir(dir: PathBuf) -> bool {
    STREAM_CACHE_DIR.set(dir).is_ok()
}

/// 128-bit FNV-1a, as two rounds with different bases. Only used to name cache
/// files, so speed matters more than the distribution guarantees of a real
/// digest — and the cost of a collision is a wrong cached body, which the
/// `.part`-then-rename write already bounds to whole files.
fn hash_uri(uri: &str) -> String {
    fn fnv(bytes: &[u8], mut hash: u64) -> u64 {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
    let lo = fnv(uri.as_bytes(), 0xcbf2_9ce4_8422_2325);
    let hi = fnv(uri.as_bytes(), 0x9e37_79b9_7f4a_7c15);
    format!("{lo:016x}{hi:016x}")
}

/// Spool `reader` to a file and return it opened for reading.
///
/// Written to a sibling `.part` and renamed, so an interrupted copy can never
/// be picked up as a complete one by a later run.
fn spool_to_file<R: Read>(uri: &str, mut reader: R) -> AudioResult<(std::fs::File, Option<tempfile::TempPath>)> {
    if let Some(dir) = STREAM_CACHE_DIR.get() {
        let target = dir.join(format!("{}.bin", hash_uri(uri)));
        if let Ok(file) = std::fs::File::open(&target) {
            if file.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                return Ok((file, None));
            }
        }
        std::fs::create_dir_all(dir)?;
        let part = dir.join(format!("{}.part", hash_uri(uri)));
        {
            let mut out = std::fs::File::create(&part)?;
            std::io::copy(&mut reader, &mut out)?;
            out.sync_all()?;
        }
        std::fs::rename(&part, &target)?;
        return Ok((std::fs::File::open(&target)?, None));
    }

    let mut temp = tempfile::Builder::new()
        .prefix("gmplayer-content-")
        .suffix(".tmp")
        .tempfile()?;
    std::io::copy(&mut reader, &mut temp)?;
    let path = temp.into_temp_path();
    let file = std::fs::File::open(&path)?;
    Ok((file, Some(path)))
}

// ── Opened source ────────────────────────────────────────────────

/// A readable, seekable handle on a track's bytes, plus whatever temporary
/// state has to outlive the read.
///
/// Implements `MediaSource` so it can be handed straight to a
/// `MediaSourceStream`, and `Read`/`Seek` so the AutoMix analyzer (which goes
/// through rodio's `Decoder`, not symphonia's probe) can use the same value.
pub struct OpenedSource {
    media: Box<dyn MediaSource>,
    ext_hint: Option<String>,
    /// Whether the bytes had to be copied out of a non-seekable source before
    /// they could be decoded. The handle that comes back is seekable either way,
    /// so `is_seekable()` cannot answer this — and the library UI wants to say
    /// "this file has to be cached first" rather than let it look free.
    spooled: bool,
    /// Kept alive for exactly as long as the read: dropping it deletes the
    /// downloaded or spooled file.
    _temp: Option<tempfile::TempPath>,
}

impl OpenedSource {
    /// Filename extension for symphonia's `Hint`, if the locator had one.
    pub fn extension(&self) -> Option<&str> {
        self.ext_hint.as_deref()
    }

    /// Whether opening this source required spooling it to a file first.
    pub fn was_spooled(&self) -> bool {
        self.spooled
    }

    /// Total size in bytes, when the underlying source knows it.
    ///
    /// This replaces `std::fs::metadata(path).len()` for the bitrate estimate:
    /// a `content://` URI has no path to stat.
    pub fn size(&self) -> Option<u64> {
        self.media.byte_len()
    }

    /// Detach the temp-file guard so a caller that keeps the bytes around (the
    /// player, which holds a downloaded file for the length of the track) can
    /// own it directly.
    pub fn into_parts(self) -> (Box<dyn MediaSource>, Option<tempfile::TempPath>) {
        (self.media, self._temp)
    }
}

impl Read for OpenedSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.media.read(buf)
    }
}

impl Seek for OpenedSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.media.seek(pos)
    }
}

impl MediaSource for OpenedSource {
    fn is_seekable(&self) -> bool {
        self.media.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.media.byte_len()
    }
}

// ── open ─────────────────────────────────────────────────────────

/// Open a locator string. Convenience wrapper over [`open`].
pub fn open_str(locator: &str) -> AudioResult<OpenedSource> {
    open(SourceLocator::classify(locator))
}

/// Open whatever `loc` points at.
///
/// Blocking. `Http` downloads the whole body first (see
/// `decoder::download_to_temp_path` for why the agent is per-call), and a
/// non-seekable `Content` source is spooled to a file before it is returned —
/// symphonia's probe seeks backwards, so handing it a pipe fails at the first
/// format check rather than at a boundary anyone can act on.
pub fn open(loc: SourceLocator<'_>) -> AudioResult<OpenedSource> {
    let ext_hint = loc.extension();

    match loc {
        SourceLocator::Path(path) => {
            let file = std::fs::File::open(path)?;
            Ok(OpenedSource {
                media: Box::new(file),
                ext_hint,
                spooled: false,
                _temp: None,
            })
        }
        SourceLocator::Http(url) => {
            let temp = crate::decoder::download_to_temp_path(url)?;
            let file = std::fs::File::open(&temp)?;
            Ok(OpenedSource {
                media: Box::new(file),
                ext_hint,
                spooled: false,
                _temp: Some(temp),
            })
        }
        SourceLocator::Content(uri) => open_content(uri, ext_hint),
    }
}

fn open_content(uri: &str, ext_hint: Option<String>) -> AudioResult<OpenedSource> {
    let provider = content_provider().ok_or_else(|| {
        AudioError::UnsupportedFormat("no content:// provider installed".to_string())
    })?;

    let handle = provider
        .open_fd(uri)
        .map_err(|e| AudioError::Io(std::io::Error::other(format!("open content uri: {e}"))))?;

    // Adopt the descriptor immediately: from here every exit path drops a
    // `File` and therefore closes it.
    let file = adopt_fd(handle.fd).ok_or_else(|| {
        AudioError::UnsupportedFormat("content:// descriptors need a unix host".to_string())
    })?;

    if handle.is_regular_file {
        return Ok(OpenedSource {
            media: Box::new(file),
            ext_hint,
            spooled: false,
            _temp: None,
        });
    }

    // Not a regular file: `lseek` would fail, so spool it and decode the copy.
    let (spooled, temp) = spool_to_file(uri, file)?;
    Ok(OpenedSource {
        media: Box::new(spooled),
        ext_hint,
        spooled: true,
        _temp: temp,
    })
}

/// Whether a *local* locator (path or `content://` URI) is still readable.
///
/// Used by the resolver's `LocalPath` plan. A missing SAF permission and a
/// deleted file are the same answer here — both mean the planner should skip
/// the track — but they arrive by different routes, so the scheme has to be
/// checked rather than assumed.
pub fn local_exists(locator: &str) -> bool {
    match SourceLocator::classify(locator) {
        SourceLocator::Path(path) => path.exists(),
        SourceLocator::Content(uri) => match content_provider() {
            Some(provider) => provider.exists(uri),
            None => false,
        },
        // An http URL is not a local source; the caller planned wrong.
        SourceLocator::Http(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn classify_dispatches_on_scheme() {
        assert!(matches!(
            SourceLocator::classify("content://com.android.externalstorage.documents/tree/x"),
            SourceLocator::Content(_)
        ));
        assert!(matches!(
            SourceLocator::classify("https://music.163.com/a.mp3"),
            SourceLocator::Http(_)
        ));
        assert!(matches!(
            SourceLocator::classify("http://example.com/a.mp3"),
            SourceLocator::Http(_)
        ));
        assert!(matches!(
            SourceLocator::classify("/home/u/a.flac"),
            SourceLocator::Path(_)
        ));
        assert!(matches!(
            SourceLocator::classify("D:\\Music\\a.flac"),
            SourceLocator::Path(_)
        ));
    }

    /// A content URI is carried through the player as a `PathBuf`; if that
    /// round trip normalised anything, every Android local track would classify
    /// as a path and fail to open.
    #[test]
    fn content_uri_survives_a_pathbuf_round_trip() {
        let uri = "content://com.android.externalstorage.documents/tree/primary%3AMusic";
        let buf = PathBuf::from(uri);
        assert!(matches!(
            SourceLocator::from_path(&buf),
            SourceLocator::Content(_)
        ));
        assert_eq!(SourceLocator::from_path(&buf).as_str(), Some(uri));
    }

    #[test]
    fn extension_is_derived_from_percent_encoded_uris() {
        assert_eq!(
            extension_from_uri(
                "content://com.android.externalstorage.documents/document/primary%3AMusic%2Fa.flac"
            ),
            Some("flac".to_string())
        );
        assert_eq!(
            extension_from_uri("https://m701.music.126.net/x/y.mp3?authSecret=deadbeef"),
            Some("mp3".to_string())
        );
        // A bare document id with no filename must not invent one.
        assert_eq!(
            extension_from_uri("content://com.example.provider/document/1234"),
            None
        );
    }

    #[test]
    fn open_reads_a_filesystem_path() {
        let mut tmp = tempfile::NamedTempFile::new().expect("temp");
        tmp.write_all(b"0123456789").expect("write");
        tmp.flush().expect("flush");

        let mut opened = open(SourceLocator::Path(tmp.path())).expect("open");
        assert_eq!(opened.size(), Some(10));
        let mut buf = String::new();
        opened.read_to_string(&mut buf).expect("read");
        assert_eq!(buf, "0123456789");
    }

    #[test]
    fn local_exists_splits_on_scheme() {
        let tmp = tempfile::NamedTempFile::new().expect("temp");
        assert!(local_exists(tmp.path().to_str().expect("utf8")));
        assert!(!local_exists("/definitely/not/here.flac"));
        // No provider installed in unit tests, so a content URI is unreadable
        // rather than optimistically assumed present.
        assert!(!local_exists("content://com.example/doc/1"));
        assert!(!local_exists("https://example.com/a.mp3"));
    }

    #[test]
    fn hash_is_stable_and_distinguishes_uris() {
        let a = hash_uri("content://a");
        assert_eq!(a, hash_uri("content://a"));
        assert_ne!(a, hash_uri("content://b"));
        assert_eq!(a.len(), 32);
    }
}
