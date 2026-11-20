//! Played downloads kept for the next time the same file is asked for.
//!
//! A download used to be a temp file that died with its `TempGuard`, so a replay, a "previous",
//! or an AutoMix prepare whose fade never happened all fetched the whole track again. The URL
//! cannot be the key — Netease signs it per resolve — so the key is the content hash the resolver
//! is told alongside it (`song_url_v1`'s `md5`). Nothing is trusted on that say-so: a finished
//! download is hashed, and kept only when the hash matches. A source that declares no hash (UNM,
//! an unlocked track) is simply never kept.
//!
//! Every download lands in this directory while one is configured, kept or not, so the sweep at
//! start-up also removes the downloads a killed process left behind (`SIGKILL` runs no `Drop`).
//! The index is the only record of what is kept: a file it does not name is debris.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

const INDEX_FILE: &str = "index.json";
/// What a download is called until the cache adopts it (or the guard deletes it).
pub(crate) const DOWNLOAD_PREFIX: &str = "dl-";
/// Used until the frontend pushes the user's choice.
const DEFAULT_BUDGET: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Entry {
    md5: String,
    file: String,
    size: u64,
    last_used: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct IndexFile {
    entries: Vec<Entry>,
}

struct Index {
    entries: HashMap<String, Entry>,
    /// Guards holding each entry's file open; a pinned file is never evicted.
    pins: HashMap<String, u32>,
}

struct AudioCache {
    dir: PathBuf,
    budget: AtomicU64,
    index: Mutex<Index>,
}

fn cache() -> Option<&'static AudioCache> {
    CACHE.get()
}

static CACHE: OnceLock<AudioCache> = OnceLock::new();

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A declared hash we can key on: 32 hex digits, compared lowercase.
pub(crate) fn normalize_md5(md5: Option<&str>) -> Option<String> {
    let md5 = md5?.trim();
    (md5.len() == 32 && md5.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| md5.to_ascii_lowercase())
}

/// Point the cache at `dir` (the app cache directory's `audio`), sweeping what a previous run
/// left there. First call wins; without one, downloads stay plain temp files and nothing is kept.
pub fn set_audio_cache_dir(dir: PathBuf) -> bool {
    if CACHE.get().is_some() {
        return false;
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("audio cache disabled, cannot create {}: {e}", dir.display());
        return false;
    }
    let entries = load_and_sweep(&dir);
    CACHE
        .set(AudioCache {
            dir,
            budget: AtomicU64::new(DEFAULT_BUDGET),
            index: Mutex::new(Index {
                entries,
                pins: HashMap::new(),
            }),
        })
        .is_ok()
}

/// Read the index, keep the entries whose file is still there at its size, delete everything
/// else in the directory.
fn load_and_sweep(dir: &Path) -> HashMap<String, Entry> {
    let stored: IndexFile = std::fs::read(dir.join(INDEX_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let entries: HashMap<String, Entry> = stored
        .entries
        .into_iter()
        .filter(|entry| {
            std::fs::metadata(dir.join(&entry.file)).is_ok_and(|m| m.len() == entry.size)
        })
        .map(|entry| (entry.md5.clone(), entry))
        .collect();
    let (mut swept, mut freed) = (0usize, 0u64);
    if let Ok(listing) = std::fs::read_dir(dir) {
        for item in listing.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if name == INDEX_FILE || entries.values().any(|entry| entry.file == name) {
                continue;
            }
            let size = item.metadata().map_or(0, |m| m.len());
            if std::fs::remove_file(item.path()).is_ok() {
                swept += 1;
                freed += size;
            }
        }
    }
    if swept > 0 {
        info!("audio cache: removed {swept} leftover files ({freed} bytes)");
    }
    entries
}

fn lock(cache: &AudioCache) -> MutexGuard<'_, Index> {
    cache.index.lock().unwrap_or_else(|e| e.into_inner())
}

impl AudioCache {
    /// Persist the index. Written beside and renamed, so a crash mid-write leaves the old one.
    fn save(&self, index: &Index) {
        let file = IndexFile {
            entries: index.entries.values().cloned().collect(),
        };
        let Ok(bytes) = serde_json::to_vec(&file) else {
            return;
        };
        let tmp = self.dir.join("index.json.tmp");
        let written = std::fs::write(&tmp, bytes)
            .and_then(|()| std::fs::rename(&tmp, self.dir.join(INDEX_FILE)));
        if let Err(e) = written {
            warn!("audio cache index not saved: {e}");
        }
    }

    /// Drop unpinned entries, least recently used first, until the total fits `budget`. A file
    /// that will not delete (still open elsewhere on Windows) keeps its entry and is retried later.
    fn evict_to(&self, index: &mut Index, budget: u64) -> u64 {
        let mut total: u64 = index.entries.values().map(|e| e.size).sum();
        let mut victims: Vec<Entry> = index
            .entries
            .values()
            .filter(|e| !index.pins.contains_key(&e.md5))
            .cloned()
            .collect();
        victims.sort_by_key(|e| e.last_used);
        let mut freed = 0;
        for victim in victims {
            if total <= budget {
                break;
            }
            if std::fs::remove_file(self.dir.join(&victim.file)).is_ok() {
                index.entries.remove(&victim.md5);
                total -= victim.size;
                freed += victim.size;
            }
        }
        freed
    }
}

pub fn set_audio_cache_budget(bytes: u64) {
    let Some(cache) = cache() else { return };
    cache.budget.store(bytes, Ordering::Relaxed);
    let mut index = lock(cache);
    if cache.evict_to(&mut index, bytes) > 0 {
        cache.save(&index);
    }
}

/// Bytes the kept files take.
pub fn audio_cache_usage() -> u64 {
    cache().map_or(0, |cache| {
        lock(cache).entries.values().map(|e| e.size).sum()
    })
}

/// Delete every kept file not in use, returning the bytes freed. One in use (the playing track)
/// stays and is evicted normally later.
pub fn clear_audio_cache() -> u64 {
    let Some(cache) = cache() else { return 0 };
    let mut index = lock(cache);
    let freed = cache.evict_to(&mut index, 0);
    cache.save(&index);
    freed
}

/// Where a download should be written: inside the cache directory when there is one, so the next
/// start's sweep covers it.
pub(crate) fn download_dir() -> Option<&'static Path> {
    cache().map(|cache| cache.dir.as_path())
}

/// A kept file for `md5`, pinned until the returned hold is released.
pub(crate) fn lookup(md5: &str) -> Option<(PathBuf, Hold)> {
    let cache = cache()?;
    if cache.budget.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let mut index = lock(cache);
    let entry = index.entries.get_mut(md5)?;
    let path = cache.dir.join(&entry.file);
    if !std::fs::metadata(&path).is_ok_and(|m| m.len() == entry.size) {
        index.entries.remove(md5);
        cache.save(&index);
        return None;
    }
    entry.last_used = now_secs();
    *index.pins.entry(md5.to_string()).or_default() += 1;
    cache.save(&index);
    Some((path, Hold::committed(md5.to_string())))
}

fn unpin(md5: &str) {
    let Some(cache) = cache() else { return };
    let mut index = lock(cache);
    if let Some(pins) = index.pins.get_mut(md5) {
        *pins -= 1;
        if *pins == 0 {
            index.pins.remove(md5);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tie {
    /// Downloading; the guard deletes the file unless this changes.
    Pending,
    /// Complete and being hashed.
    Verifying,
    /// The cache owns the file; the guard only unpins it.
    Committed,
    /// The guard is gone; whatever was verifying must not adopt the file.
    Abandoned,
}

/// A guard's claim on a cache entry, or on a download that may become one.
pub(crate) struct Hold {
    md5: String,
    tie: Arc<Mutex<Tie>>,
}

impl Hold {
    fn committed(md5: String) -> Self {
        Self {
            md5,
            tie: Arc::new(Mutex::new(Tie::Committed)),
        }
    }

    pub(crate) fn pending(md5: String) -> Self {
        Self {
            md5,
            tie: Arc::new(Mutex::new(Tie::Pending)),
        }
    }

    /// The commit side of this hold, for whoever finishes the download.
    pub(crate) fn committer(&self, path: PathBuf) -> Committer {
        Committer {
            md5: self.md5.clone(),
            tie: Arc::clone(&self.tie),
            path,
        }
    }

    /// The guard is going. `true`: the cache owns the file, so it must not be deleted.
    pub(crate) fn release(self) -> bool {
        let mut tie = self.tie.lock().unwrap_or_else(|e| e.into_inner());
        if *tie == Tie::Committed {
            unpin(&self.md5);
            true
        } else {
            *tie = Tie::Abandoned;
            false
        }
    }
}

/// Adopts a finished download into the cache once its hash checks out.
#[derive(Clone)]
pub(crate) struct Committer {
    md5: String,
    tie: Arc<Mutex<Tie>>,
    path: PathBuf,
}

impl Committer {
    /// Called once the whole file has landed. Hashing a track takes a moment, so it runs outside
    /// the tie's lock; a guard dropped meanwhile marks it abandoned and the file is not adopted.
    pub(crate) fn commit(&self) {
        let Some(cache) = cache() else { return };
        if cache.budget.load(Ordering::Relaxed) == 0 {
            return;
        }
        {
            let mut tie = self.tie.lock().unwrap_or_else(|e| e.into_inner());
            if *tie != Tie::Pending {
                return;
            }
            *tie = Tie::Verifying;
        }
        let digest = file_md5(&self.path);
        let mut tie = self.tie.lock().unwrap_or_else(|e| e.into_inner());
        if *tie != Tie::Verifying {
            return;
        }
        let Some((digest, size)) = digest.filter(|(digest, _)| *digest == self.md5) else {
            warn!(
                "audio cache: download does not match its declared md5 {}, not kept",
                self.md5
            );
            *tie = Tie::Pending;
            return;
        };
        let Some(file) = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            *tie = Tie::Pending;
            return;
        };
        let mut index = lock(cache);
        if let Some(existing) = index.entries.get(&digest) {
            // Another download of the same file got there first; this copy stays temporary.
            if existing.file != file {
                *tie = Tie::Pending;
                return;
            }
        }
        index.entries.insert(
            digest.clone(),
            Entry {
                md5: digest.clone(),
                file,
                size,
                last_used: now_secs(),
            },
        );
        *index.pins.entry(digest).or_default() += 1;
        *tie = Tie::Committed;
        let budget = cache.budget.load(Ordering::Relaxed);
        cache.evict_to(&mut index, budget);
        cache.save(&index);
    }
}

/// The file has landed in full by the time this runs, so a plain read sees all of it; the
/// growing-file reader exists for reads that race the writer.
fn file_md5(path: &Path) -> Option<(String, u64)> {
    use md5::Digest;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = md5::Md5::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    let digest = hasher.finalize();
    Some((digest.iter().map(|b| format!("{b:02x}")).collect(), size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_32_hex_digit_hash_is_a_key() {
        assert_eq!(
            normalize_md5(Some(" 0123456789ABCDEF0123456789abcdef ")).as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(normalize_md5(Some("")), None);
        assert_eq!(normalize_md5(Some("xyz")), None);
        assert_eq!(normalize_md5(None), None);
    }

    #[test]
    fn the_sweep_keeps_indexed_files_and_removes_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("kept.bin"), b"abcd").unwrap();
        std::fs::write(dir.path().join("dl-orphan.part"), b"half").unwrap();
        std::fs::write(dir.path().join("resized.bin"), b"12").unwrap();
        let index = IndexFile {
            entries: vec![
                Entry {
                    md5: "a".repeat(32),
                    file: "kept.bin".into(),
                    size: 4,
                    last_used: 1,
                },
                Entry {
                    md5: "b".repeat(32),
                    file: "resized.bin".into(),
                    size: 99,
                    last_used: 1,
                },
                Entry {
                    md5: "c".repeat(32),
                    file: "gone.bin".into(),
                    size: 1,
                    last_used: 1,
                },
            ],
        };
        std::fs::write(
            dir.path().join(INDEX_FILE),
            serde_json::to_vec(&index).unwrap(),
        )
        .unwrap();

        let entries = load_and_sweep(dir.path());
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key(&"a".repeat(32)));
        assert!(dir.path().join("kept.bin").exists());
        assert!(!dir.path().join("dl-orphan.part").exists());
        assert!(!dir.path().join("resized.bin").exists());
    }

    #[test]
    fn eviction_drops_the_least_recently_used_unpinned_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = AudioCache {
            dir: dir.path().to_path_buf(),
            budget: AtomicU64::new(0),
            index: Mutex::new(Index {
                entries: HashMap::new(),
                pins: HashMap::new(),
            }),
        };
        let mut index = lock(&cache);
        for (name, used) in [("old", 1), ("mid", 2), ("new", 3)] {
            std::fs::write(dir.path().join(name), [0u8; 10]).unwrap();
            index.entries.insert(
                name.into(),
                Entry {
                    md5: name.into(),
                    file: name.into(),
                    size: 10,
                    last_used: used,
                },
            );
        }
        index.pins.insert("old".into(), 1);

        assert_eq!(cache.evict_to(&mut index, 15), 20);
        assert!(index.entries.contains_key("old"), "pinned: in use");
        assert!(!index.entries.contains_key("mid"));
        assert!(!index.entries.contains_key("new"));
        assert!(dir.path().join("old").exists());
    }

    #[test]
    fn the_hash_of_a_file_is_its_md5() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"hello").unwrap();
        assert_eq!(
            file_md5(&path),
            Some(("5d41402abc4b2a76b9719d911017c592".into(), 5))
        );
    }
}
