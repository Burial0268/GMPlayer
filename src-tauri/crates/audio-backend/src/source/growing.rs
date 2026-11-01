//! A download that is still arriving, readable while it lands.
//!
//! The player threads every track through a `PathBuf`, and every reader of that
//! path goes through `source::open`. So instead of changing those types, a
//! download in flight is *registered* under its temp path and `open` hands back
//! a reader that waits for the writers — the decoder, the metadata probe and the
//! AutoMix analyzer all get it without knowing. A plain `File` would read EOF at
//! the writer's current end and the decoder would take that for the end of the
//! song.
//!
//! The file is filled by one writer from byte 0 and, when the reader seeks far
//! past that writer, by a second one that starts at the target with a `Range`
//! request. What has landed is a set of byte intervals; a read waits only for
//! the interval it is in. A writer stops the moment it reaches bytes another one
//! already holds, so the file ends up covered exactly once. Nothing is declared
//! done until the intervals are the whole file, which is what keeps a sparse
//! temp file from ever being mistaken for a finished one.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use symphonia::core::io::MediaSource;

/// How often a blocked read re-checks for cancellation. The writers notify on
/// every chunk, so this only bounds how long a cancel can go unnoticed.
const CANCEL_POLL: Duration = Duration::from_millis(200);

/// A seek less than this far past a writer's position just waits for it: a new
/// connection costs more than the bytes between.
const JUMP_MIN_GAP: u64 = 1024 * 1024;

/// A jump starts this far before the seek target. A format reader often seeks a
/// little behind the position it was asked for (an MP3 backs up a frame), and a
/// start exactly at the target would miss that and ask for a third connection.
const JUMP_LEAD_IN: u64 = 64 * 1024;

type Interval = (u64, u64);

struct Writer {
    id: u64,
    start: u64,
    pos: u64,
}

#[derive(Default)]
struct State {
    /// Sorted, disjoint, half-open, adjacent ones merged.
    covered: Vec<Interval>,
    writers: Vec<Writer>,
    next_writer: u64,
    done: bool,
    error: Option<String>,
    /// The server did not honour a `Range`; stop asking.
    jumps_off: bool,
}

impl State {
    fn covered_end(&self, pos: u64) -> Option<u64> {
        self.covered
            .iter()
            .find(|(s, e)| *s <= pos && pos < *e)
            .map(|(_, e)| *e)
    }

    fn next_start_after(&self, pos: u64, total: u64) -> u64 {
        self.covered
            .iter()
            .map(|(s, _)| *s)
            .find(|s| *s > pos)
            .unwrap_or(total)
    }

    fn insert(&mut self, start: u64, end: u64) {
        self.covered.push((start, end));
        self.covered.sort_unstable();
        let mut merged: Vec<Interval> = Vec::with_capacity(self.covered.len());
        for (s, e) in self.covered.drain(..) {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        self.covered = merged;
    }
}

/// Starts a ranged writer for `(progress, writer id, start byte)`. Installed by
/// whoever knows the URL; this module only knows bytes.
type Jumper = Box<dyn Fn(Arc<Progress>, u64, u64) + Send + Sync>;

/// What the writers have landed so far, shared with every reader.
pub(crate) struct Progress {
    total: u64,
    state: Mutex<State>,
    changed: Condvar,
    cancelled: AtomicBool,
    /// Where the reader last was; the gap to the end of its interval is the lead
    /// the decoder has before it would have to wait.
    read_pos: AtomicU64,
    jumper: OnceLock<Jumper>,
}

impl Progress {
    pub(crate) fn new(total: u64) -> Arc<Self> {
        Arc::new(Self {
            total,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            cancelled: AtomicBool::new(false),
            read_pos: AtomicU64::new(0),
            jumper: OnceLock::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn set_jumper(&self, jumper: Jumper) {
        let _ = self.jumper.set(jumper);
    }

    /// Bytes readable from the reader's position without waiting. Zero for a
    /// seek past what has landed.
    pub(crate) fn lead(&self) -> u64 {
        let pos = self.read_pos.load(Ordering::Relaxed);
        let s = self.lock();
        s.covered_end(pos).map_or(0, |end| end - pos)
    }

    /// Nothing more will arrive: complete, failed or cancelled. A reader waiting
    /// on this must stop waiting and let `read` report which.
    pub(crate) fn is_finished(&self) -> bool {
        let s = self.lock();
        s.done || s.error.is_some() || self.is_cancelled()
    }

    /// Block until a writer makes progress or `timeout` passes.
    pub(crate) fn wait_for_change(&self, timeout: Duration) {
        let s = self.lock();
        drop(
            self.changed
                .wait_timeout(s, timeout)
                .unwrap_or_else(|e| e.into_inner()),
        );
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        // Take the lock so a reader between its check and its wait cannot miss it.
        drop(self.lock());
        self.changed.notify_all();
    }

    // ── Writers ──────────────────────────────────────────────────

    /// Register a writer that will fill from `start`. Registering before the
    /// first byte lands is what stops a second seek from asking for the same
    /// range again.
    pub(crate) fn begin_writer(&self, start: u64) -> u64 {
        let mut s = self.lock();
        let id = s.next_writer;
        s.next_writer += 1;
        s.writers.push(Writer {
            id,
            start,
            pos: start,
        });
        id
    }

    pub(crate) fn writer_start(&self, id: u64) -> u64 {
        self.lock()
            .writers
            .iter()
            .find(|w| w.id == id)
            .map_or(0, |w| w.start)
    }

    /// How many bytes writer `id` may still write: up to the next interval
    /// another writer holds, or the end. Zero means stop — it has caught up with
    /// someone else's bytes (or finished).
    fn room(&self, id: u64) -> u64 {
        let s = self.lock();
        let Some(w) = s.writers.iter().find(|w| w.id == id) else {
            return 0;
        };
        if w.pos >= self.total || s.covered_end(w.pos).is_some() {
            return 0;
        }
        s.next_start_after(w.pos, self.total) - w.pos
    }

    pub(crate) fn advance(&self, id: u64, n: u64) {
        let mut s = self.lock();
        let Some(w) = s.writers.iter_mut().find(|w| w.id == id) else {
            return;
        };
        let (from, to) = (w.pos, w.pos + n);
        w.pos = to;
        s.insert(from, to);
        self.changed.notify_all();
    }

    /// A writer is finished, successfully or not. A failure that cost bytes
    /// fails the download; one that cost none — a server that ignored the
    /// `Range` — only turns jumping off, since the first writer is still
    /// walking the file.
    pub(crate) fn end_writer(&self, id: u64, outcome: Result<(), String>) {
        let mut s = self.lock();
        let Some(i) = s.writers.iter().position(|w| w.id == id) else {
            return;
        };
        let w = s.writers.remove(i);
        match outcome {
            Err(e) if w.pos == w.start && w.start > 0 => {
                tracing::warn!("ranged download refused, falling back to sequential: {e}");
                s.jumps_off = true;
            }
            Err(e) => s.error = Some(e),
            Ok(()) => {}
        }
        if s.covered.len() == 1 && s.covered[0] == (0, self.total) {
            s.done = true;
        } else if s.writers.is_empty() && s.error.is_none() && !self.is_cancelled() {
            s.error = Some("download ended with parts of the file missing".into());
        }
        self.changed.notify_all();
    }

    /// Ask for a writer at `target` when the reader has moved far past every
    /// writer. Called on each seek; cheap when it declines.
    fn request_jump(self: &Arc<Self>, target: u64) {
        let Some(jumper) = self.jumper.get() else {
            return;
        };
        let (id, start) = {
            let mut s = self.lock();
            if s.done
                || s.error.is_some()
                || s.jumps_off
                || self.is_cancelled()
                || target >= self.total
                || s.covered_end(target).is_some()
            {
                return;
            }
            // The nearest writer behind the target is the one that would get
            // there; if it is close, waiting for it beats a new connection.
            let behind = s
                .writers
                .iter()
                .filter(|w| w.pos <= target)
                .map(|w| w.pos)
                .max();
            if behind.is_some_and(|p| target - p <= JUMP_MIN_GAP) {
                return;
            }
            let start = target.saturating_sub(JUMP_LEAD_IN).max(behind.unwrap_or(0));
            let id = s.next_writer;
            s.next_writer += 1;
            s.writers.push(Writer {
                id,
                start,
                pos: start,
            });
            (id, start)
        };
        jumper(Arc::clone(self), id, start);
    }

    /// Bytes readable at `pos`, waiting for a writer if there are none yet.
    /// `0` means end of file.
    fn available(&self, pos: u64) -> io::Result<u64> {
        let mut s = self.lock();
        loop {
            if self.is_cancelled() {
                return Err(io::Error::other("download cancelled"));
            }
            if let Some(end) = s.covered_end(pos) {
                return Ok(end - pos);
            }
            if let Some(err) = &s.error {
                return Err(io::Error::other(err.clone()));
            }
            if s.done || pos >= self.total {
                return Ok(0);
            }
            s = self
                .changed
                .wait_timeout(s, CANCEL_POLL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

/// Copy `body` into `file` as writer `id`, from that writer's start offset,
/// until the body ends or the writer reaches bytes someone else already landed.
/// A body that ends short of where it should is an error, not an early end: an
/// MP3 with no frame count would otherwise just play as a shorter song.
pub(crate) fn pump(
    progress: &Arc<Progress>,
    id: u64,
    mut file: File,
    mut body: impl Read,
) -> Result<(), String> {
    file.seek(SeekFrom::Start(progress.writer_start(id)))
        .map_err(|e| format!("Seek temp file: {e}"))?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if progress.is_cancelled() {
            return Ok(());
        }
        let room = progress.room(id);
        if room == 0 {
            return Ok(());
        }
        let want = buf.len().min(usize::try_from(room).unwrap_or(usize::MAX));
        match body.read(&mut buf[..want]) {
            Ok(0) => return Err("download body ended early".into()),
            Ok(n) => {
                file.write_all(&buf[..n])
                    .map_err(|e| format!("Write temp file: {e}"))?;
                progress.advance(id, n as u64);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("Download body: {e}")),
        }
    }
}

// ── Registry ─────────────────────────────────────────────────────

fn active() -> &'static Mutex<HashMap<PathBuf, Arc<Progress>>> {
    static ACTIVE: OnceLock<Mutex<HashMap<PathBuf, Arc<Progress>>>> = OnceLock::new();
    ACTIVE.get_or_init(Default::default)
}

pub(crate) fn register(path: &Path, progress: Arc<Progress>) {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf(), progress);
}

pub(crate) fn unregister(path: &Path) {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
}

pub(crate) fn lookup(path: &Path) -> Option<Arc<Progress>> {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
        .cloned()
}

/// Declared size of a download this process made, until its guard is dropped.
/// For cache keys that would otherwise embed a length and mtime that are still
/// changing, and would differ again once the download lands. Registered for the
/// guard's lifetime, not the writer's, for exactly that reason.
pub fn expected_len(path: &Path) -> Option<u64> {
    lookup(path).map(|p| p.total())
}

// ── Reader ───────────────────────────────────────────────────────

pub(crate) struct GrowingFile {
    file: File,
    pos: u64,
    progress: Arc<Progress>,
}

impl GrowingFile {
    pub(crate) fn open(path: &Path, progress: Arc<Progress>) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            pos: 0,
            progress,
        })
    }
}

impl Read for GrowingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let avail = self.progress.available(self.pos)?;
        if avail == 0 {
            return Ok(0);
        }
        let want = buf.len().min(usize::try_from(avail).unwrap_or(usize::MAX));
        let n = self.file.read(&mut buf[..want])?;
        self.pos += n as u64;
        self.progress.read_pos.store(self.pos, Ordering::Relaxed);
        Ok(n)
    }
}

impl Seek for GrowingFile {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => self.progress.total().checked_add_signed(d),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        // Position only; the wait happens on the read that needs the bytes.
        self.file.seek(SeekFrom::Start(target))?;
        self.pos = target;
        self.progress.read_pos.store(target, Ordering::Relaxed);
        self.progress.request_jump(target);
        Ok(target)
    }
}

impl MediaSource for GrowingFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.progress.total())
    }
}

// ── Guard ────────────────────────────────────────────────────────

/// A temp file plus, while its download is running, the means to stop it.
/// Dropping it cancels the writers and then deletes the file, so a skipped
/// track stops costing bandwidth at once.
pub struct TempGuard {
    // Declared first so the writers are told to stop before the file goes.
    progress: Option<Arc<Progress>>,
    path: tempfile::TempPath,
}

impl TempGuard {
    pub(crate) fn streaming(path: tempfile::TempPath, progress: Arc<Progress>) -> Self {
        Self {
            progress: Some(progress),
            path,
        }
    }
}

impl From<tempfile::TempPath> for TempGuard {
    fn from(path: tempfile::TempPath) -> Self {
        Self {
            progress: None,
            path,
        }
    }
}

impl Deref for TempGuard {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempGuard {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(p) = self.progress.take() {
            p.cancel();
            unregister(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A reader over a `total`-byte file whose bytes are `i % 251`, with a
    /// helper to land a span the way a writer would.
    struct Rig {
        _path: tempfile::TempPath,
        progress: Arc<Progress>,
        reader: GrowingFile,
        path: PathBuf,
    }

    fn rig(total: u64) -> Rig {
        let (file, path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        file.set_len(total).unwrap();
        let progress = Progress::new(total);
        let reader = GrowingFile::open(&path, progress.clone()).unwrap();
        Rig {
            path: path.to_path_buf(),
            _path: path,
            progress,
            reader,
        }
    }

    fn body(from: u64, to: u64) -> Vec<u8> {
        (from..to).map(|i| (i % 251) as u8).collect()
    }

    impl Rig {
        /// Write `[from, to)` through a real writer, as `pump` would.
        fn land(&self, from: u64, to: u64) -> Result<(), String> {
            let id = self.progress.begin_writer(from);
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&self.path)
                .unwrap();
            let r = pump(&self.progress, id, file, io::Cursor::new(body(from, to)));
            self.progress.end_writer(id, r.clone());
            r
        }
    }

    #[test]
    fn a_read_waits_for_the_writer_and_ends_at_done() {
        let mut rig = rig(8);
        let id = rig.progress.begin_writer(0);
        let p = rig.progress.clone();
        let path = rig.path.clone();
        let writer = std::thread::spawn(move || {
            let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            // Two slow chunks.
            let slow = SlowRead {
                left: body(0, 8),
                chunk: 4,
            };
            let r = pump(&p, id, file, slow);
            p.end_writer(id, r);
        });
        let mut out = Vec::new();
        rig.reader.read_to_end(&mut out).unwrap();
        writer.join().unwrap();
        assert_eq!(out, body(0, 8));
    }

    struct SlowRead {
        left: Vec<u8>,
        chunk: usize,
    }
    impl Read for SlowRead {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_millis(40));
            let n = self.chunk.min(self.left.len()).min(buf.len());
            buf[..n].copy_from_slice(&self.left[..n]);
            self.left.drain(..n);
            Ok(n)
        }
    }

    #[test]
    fn a_short_body_is_an_error_not_a_shorter_song() {
        let mut rig = rig(8);
        assert!(rig.land(0, 4).is_err());
        assert!(rig.reader.read_to_end(&mut Vec::new()).is_err());
    }

    #[test]
    fn seek_from_end_uses_the_declared_length() {
        let mut rig = rig(100);
        assert_eq!(rig.reader.seek(SeekFrom::End(-10)).unwrap(), 90);
    }

    #[test]
    fn two_writers_meet_in_the_middle_and_the_file_is_whole() {
        let mut rig = rig(300);
        // The head is registered first, as in a real download; the tail lands
        // while it is still going, then the head runs until it reaches it.
        let id = rig.progress.begin_writer(0);
        rig.land(200, 300).unwrap();
        assert!(!rig.progress.is_finished(), "a hole is not done");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&rig.path)
            .unwrap();
        // The head's body is longer than the gap; it must stop at 200.
        pump(&rig.progress, id, file, io::Cursor::new(body(0, 300))).unwrap();
        rig.progress.end_writer(id, Ok(()));
        let mut out = Vec::new();
        rig.reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, body(0, 300));
    }

    #[test]
    fn lead_is_what_has_landed_past_the_reader() {
        let mut rig = rig(100);
        rig.land(0, 40).ok();
        assert_eq!(rig.progress.lead(), 40);
        rig.reader.read_exact(&mut [0u8; 10]).unwrap();
        assert_eq!(rig.progress.lead(), 30);
        rig.reader.seek(SeekFrom::Start(90)).unwrap();
        assert_eq!(rig.progress.lead(), 0);
    }

    #[test]
    fn a_far_seek_asks_for_a_jump_once_and_a_near_one_does_not() {
        let mut rig = rig(10 * 1024 * 1024);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (c, s) = (calls.clone(), seen.clone());
        rig.progress.set_jumper(Box::new(move |_, _, start| {
            c.fetch_add(1, Ordering::SeqCst);
            s.lock().unwrap().push(start);
        }));
        let head = rig.progress.begin_writer(0);
        rig.progress.advance(head, 100 * 1024);

        // Within a megabyte of the head: wait for it.
        rig.reader.seek(SeekFrom::Start(600 * 1024)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // Far: one jump, with the lead-in.
        let target = 8 * 1024 * 1024;
        rig.reader.seek(SeekFrom::Start(target)).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![target - JUMP_LEAD_IN]);

        // The same area again — a format reader backing up — is already asked for.
        rig.reader.seek(SeekFrom::Start(target - 2000)).unwrap();
        rig.reader.seek(SeekFrom::Start(target + 5000)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_refused_range_turns_jumping_off_without_failing_the_download() {
        let rig = rig(10 * 1024 * 1024);
        let _head = rig.progress.begin_writer(0);
        let id = rig.progress.begin_writer(5 * 1024 * 1024);
        rig.progress.end_writer(id, Err("HTTP 200".into()));
        assert!(
            !rig.progress.is_finished(),
            "the head writer is still going"
        );
        assert!(rig.progress.lock().jumps_off);
    }

    #[test]
    fn cancel_interrupts_a_blocked_read() {
        let mut rig = rig(8);
        let p = rig.progress.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            p.cancel();
        });
        assert!(rig.reader.read(&mut [0u8; 4]).is_err());
        t.join().unwrap();
    }
}
