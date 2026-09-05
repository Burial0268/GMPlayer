//! On-disk half of the response cache.
//!
//! The memory cache dies with the process, so every launch re-fetches the same
//! home page, the same playlists, the same lyrics — at ~80 ms a request over a
//! connection that has to be established first (~190 ms). That is the whole of
//! why a cold start feels slow, and it is entirely avoidable: a song's title and
//! its lyric are the same today as they were when the app last closed.
//!
//! ## Cookies do not reach the disk
//!
//! The cache key contains the caller's cookie, because two accounts must never
//! share an entry. Writing that key out would persist a session token, which the
//! rest of this crate goes to some length not to do.
//!
//! So the key is never stored — only `SHA-256(key)`, truncated to 128 bits and
//! hex-encoded, as the *filename*. A different cookie still lands on a different
//! file, so the isolation property survives, and nothing on disk can be read
//! back into a credential. Which also means a hit cannot be confirmed by
//! comparing keys, so a collision would serve the wrong answer; at 128 bits
//! against a cache bounded to [`MAX_ENTRIES`] files that is not a risk worth
//! spending bytes on.
//!
//! ## The endpoint is in the filename, and that is what makes invalidation work
//!
//! `{endpoint}~{digest}`. The endpoint name is not a secret — the query is what
//! carries the cookie, and that stays inside the digest — and having it in the
//! name is the only way [`crate::cache::ResponseCache::invalidate_for`] can drop
//! this tier at all: a digest cannot be scanned by prefix, so a write used to
//! remove only the files whose keys happened to still be *in memory*.
//!
//! That gap was not theoretical, and `playlist_detail` fell straight into it. It
//! is in the list policy class, so it has the soonest stale bound of anything a
//! playlist page creates (24 h against `song_detail`'s 7 d) and the memory tier
//! sheds it first; it is also the largest single response the app makes, while
//! hydrating one big playlist writes eleven `song_detail` chunks of ~2 MB — so the
//! byte budget is reached, and the entry evicted to make room is exactly the one a
//! write has to invalidate. A like or an add after that dropped nothing on disk,
//! and the frontend's post-write reconcile was answered with the pre-write list
//! for the rest of the entry's stale window. See
//! `src/views/PlayList/PlayListView.vue`'s `reconcileQuietly`.
//!
//! Matching is on the whole segment before the separator, so it is exact rather
//! than a prefix — `user_playlist` is not caught by anything invalidating
//! `user_detail`.
//!
//! ## What is written
//!
//! Only what [`crate::cache`] already decided is cacheable, minus the
//! short-lived classes where a file would expire before it could be read
//! (see `Policy::persist`). Response bodies are Netease's own public metadata,
//! stored as they arrived.
//!
//! Format is a one-line ASCII header and then the envelope verbatim:
//!
//! ```text
//! NCMC1 <expires_unix_secs> <stale_until_unix_secs>\n
//! {"ok":true,"status":200,...}
//! ```
//!
//! Verbatim matters: the envelope's *field order* is what `cache::is_storable`
//! reads, and re-encoding it through a serializer would reorder it and silently
//! make everything loaded from disk unstorable.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// Files kept. Bounds the directory walk on startup as much as the disk usage.
const MAX_ENTRIES: usize = 2_000;

/// Total bytes on disk.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Largest single entry. `playlist_track_all` is both the biggest response the
/// app makes and one of the most worth keeping, so this is generous rather than
/// tight.
const MAX_ENTRY_BYTES: usize = 4 * 1024 * 1024;

const MAGIC: &str = "NCMC1";

/// Separates the endpoint name from the digest in a filename.
///
/// Not a character any endpoint name contains (they are `[a-z0-9_]`), and legal
/// on every filesystem we target.
const NAME_SEP: char = '~';

pub(crate) struct DiskCache {
    dir: PathBuf,
}

/// What a file held.
pub(crate) struct Stored {
    pub body: Arc<str>,
    pub expires: SystemTime,
    pub stale_until: SystemTime,
}

impl DiskCache {
    /// Open (and create) the cache directory.
    ///
    /// Returns `None` when there is nowhere to put it — an isolate built without
    /// a state directory, which is how the offline tests run. A cache that
    /// cannot be created is not an error: the memory cache above it still works
    /// and every miss simply reaches the network as before.
    pub(crate) fn open(state_dir: &Path) -> Option<Self> {
        let dir = state_dir.join("cache");
        if let Err(e) = fs::create_dir_all(&dir) {
            log::warn!(target: "ncm-core", "no disk cache: {e}");
            return None;
        }
        Some(Self { dir })
    }

    /// Where `key` lives. The query — and with it the cookie — is not
    /// recoverable from this; the endpoint name deliberately is.
    ///
    /// `{endpoint}~{digest}`. See the module docs: the digest alone cannot be
    /// scanned, so without the endpoint in the name a write could not invalidate
    /// this tier except by luck.
    fn path_for(&self, key: &str) -> PathBuf {
        let digest = Sha256::digest(key.as_bytes());
        let endpoint = file_safe(endpoint_of(key));
        let mut name = String::with_capacity(endpoint.len() + 33);
        name.push_str(&endpoint);
        name.push(NAME_SEP);
        for byte in &digest[..16] {
            name.push_str(&format!("{byte:02x}"));
        }
        self.dir.join(name)
    }

    /// Read an entry back, or `None` when there is nothing usable.
    ///
    /// A file that will not parse is removed rather than left to be re-read on
    /// every miss: the usual cause is a write interrupted by the process going
    /// away, and it will never become valid.
    pub(crate) fn load(&self, key: &str) -> Option<Stored> {
        let path = self.path_for(key);
        let raw = fs::read(&path).ok()?;

        match parse(&raw) {
            Some(stored) => Some(stored),
            None => {
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    /// Write an entry, replacing any earlier one.
    ///
    /// Written to a temporary name and renamed, so a reader never sees a partial
    /// file and an interrupted write leaves the previous entry intact rather
    /// than a truncated one.
    pub(crate) fn store(
        &self,
        key: &str,
        body: &str,
        expires: SystemTime,
        stale_until: SystemTime,
    ) {
        if body.len() > MAX_ENTRY_BYTES {
            return;
        }
        let path = self.path_for(key);
        let temp = path.with_extension("tmp");

        let mut out = Vec::with_capacity(body.len() + 32);
        out.extend_from_slice(
            format!("{MAGIC} {} {}\n", unix(expires), unix(stale_until)).as_bytes(),
        );
        out.extend_from_slice(body.as_bytes());

        if fs::write(&temp, &out).is_ok() && fs::rename(&temp, &path).is_err() {
            // Windows will not rename onto an existing file that another handle
            // has open. Removing first is not atomic, but the loser of that race
            // is a cache miss.
            let _ = fs::remove_file(&path);
            if fs::rename(&temp, &path).is_err() {
                let _ = fs::remove_file(&temp);
            }
        }
    }

    /// Drop every entry belonging to any of `endpoints`, for
    /// [`crate::cache::ResponseCache::invalidate_for`].
    ///
    /// By endpoint rather than by key, and that is the whole point. Removing by
    /// key is all a digest-named file allows, so `invalidate_for` could only ever
    /// reach the files whose keys were still in the memory tier — and the entries
    /// most worth invalidating are exactly the ones memory evicts first, because
    /// they are the largest. `playlist_detail` at a couple of megabytes is the case
    /// that showed: a like or an add dropped nothing on disk, and the reconcile
    /// that followed was answered with the pre-write list.
    ///
    /// Costs one directory listing and a string compare per file — no reads, no
    /// header parses — against a directory bounded to [`MAX_ENTRIES`]. It runs on
    /// a write, so a few hundred iterations are not worth avoiding.
    ///
    /// Returns how many files went, for the caller's log.
    pub(crate) fn remove_endpoints(&self, endpoints: &[&str]) -> usize {
        if endpoints.is_empty() {
            return 0;
        }
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let wanted: Vec<String> = endpoints.iter().map(|e| file_safe(e)).collect();
        let mut dropped = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            // Whole segment, not a prefix: `user_playlist` must not be caught by
            // an invalidation naming `user_detail`, and the memory tier's own
            // matching is equally exact.
            let Some((endpoint, _)) = name.split_once(NAME_SEP) else {
                continue;
            };
            if !wanted.iter().any(|w| w == endpoint) {
                continue;
            }
            if fs::remove_file(entry.path()).is_ok() {
                dropped += 1;
            }
        }
        dropped
    }

    /// Drop what is no longer worth keeping: entries past even their stale
    /// window, then oldest-first until the directory is inside its bounds.
    ///
    /// Run once at startup, off the startup path. Nothing depends on it having
    /// finished — an over-budget directory is only over budget, not wrong.
    pub(crate) fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };

        // (stale_until, size, path) for everything that is still valid.
        let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        let mut total: u64 = 0;

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            // Leftover from an interrupted write.
            if path.extension().is_some_and(|e| e == "tmp") {
                let _ = fs::remove_file(&path);
                continue;
            }
            // Not in `{endpoint}~{digest}` form: either a file from the earlier
            // digest-only scheme, which nothing can look up any more, or
            // something that is not ours at all. Both would otherwise sit here
            // counting against the budget until their stale bound passed — up to
            // a week for the metadata classes.
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.contains(NAME_SEP))
            {
                let _ = fs::remove_file(&path);
                continue;
            }

            match header_of(&path) {
                Some(stale_until) if stale_until > SystemTime::now() => {
                    total += meta.len();
                    kept.push((stale_until, meta.len(), path));
                }
                // Expired, or not one of ours.
                _ => {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        if kept.len() <= MAX_ENTRIES && total <= MAX_BYTES {
            return;
        }

        // Shed whatever stops being useful soonest — the same order the memory
        // cache sheds in, and it needs no access tracking to compute.
        kept.sort_by_key(|(stale_until, _, _)| *stale_until);
        let mut count = kept.len();
        for (_, size, path) in &kept {
            if count <= MAX_ENTRIES && total <= MAX_BYTES {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(*size);
                count -= 1;
            }
        }
        log::debug!(
            target: "ncm-core",
            "disk cache pruned to {count} entries, {} KB",
            total / 1024
        );
    }
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The endpoint half of a cache key.
///
/// Keys are `{endpoint}\0{canonical_query}` (see `cache::ResponseCache::key`), and
/// only the query side carries the cookie — so this is the part that may be
/// written in the clear.
fn endpoint_of(key: &str) -> &str {
    key.split('\u{0}').next().unwrap_or(key)
}

/// An endpoint name reduced to something that is certainly a single filename
/// component.
///
/// Defence in depth rather than a live concern: `path_for` is only ever reached
/// through a `CacheKey`, and `cache::policy_for` is an allowlist of ~40 literal
/// names, all `[a-z0-9_]`. But this builds a path out of a string the frontend
/// chose, so it should not be *this* function that has to be read carefully to
/// rule out a `../`. Anything outside the expected alphabet becomes `_`, and the
/// result is bounded so a long name cannot push the digest past a filesystem's
/// component limit.
fn file_safe(endpoint: &str) -> String {
    const MAX: usize = 48;
    let mut out = String::with_capacity(endpoint.len().min(MAX));
    for ch in endpoint.chars().take(MAX) {
        out.push(match ch {
            'a'..='z' | '0'..='9' | '_' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '_',
        });
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

fn from_unix(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

/// Split a file into its header and its body.
fn parse(raw: &[u8]) -> Option<Stored> {
    let split = raw.iter().position(|b| *b == b'\n')?;
    let header = std::str::from_utf8(&raw[..split]).ok()?;

    let mut fields = header.split(' ');
    if fields.next()? != MAGIC {
        return None;
    }
    let expires = from_unix(fields.next()?.parse().ok()?);
    let stale_until = from_unix(fields.next()?.parse().ok()?);

    let now = SystemTime::now();
    if stale_until <= now {
        return None;
    }

    let body = std::str::from_utf8(&raw[split + 1..]).ok()?;
    // The same shape check the memory cache applies on the way in. A body that
    // fails it here is a truncated write, and serving it would hand the caller
    // half an envelope.
    if !crate::cache::is_storable(body) {
        return None;
    }

    Some(Stored {
        body: Arc::from(body),
        expires,
        stale_until,
    })
}

/// Just the stale bound, for pruning — reads the first line rather than the
/// whole file, which matters when the directory holds megabytes.
fn header_of(path: &Path) -> Option<SystemTime> {
    use std::io::Read;

    let mut head = [0u8; 64];
    let mut file = fs::File::open(path).ok()?;
    let read = file.read(&mut head).ok()?;
    let head = &head[..read];

    let split = head.iter().position(|b| *b == b'\n')?;
    let mut fields = std::str::from_utf8(&head[..split]).ok()?.split(' ');
    if fields.next()? != MAGIC {
        return None;
    }
    let _expires = fields.next()?;
    Some(from_unix(fields.next()?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(body: &str) -> String {
        format!(r#"{{"ok":true,"status":200,"body":{body},"cookie":[]}}"#)
    }

    /// A scratch directory that cleans up after itself. Avoids a dev-dependency
    /// for four tests.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ncm-core-disk-test-{tag}"));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ahead(secs: u64) -> SystemTime {
        SystemTime::now() + Duration::from_secs(secs)
    }

    #[test]
    fn an_entry_survives_a_round_trip() {
        let scratch = Scratch::new("roundtrip");
        let disk = DiskCache::open(&scratch.0).unwrap();
        let body = envelope(r#"{"code":200,"songs":[{"id":1}]}"#);

        assert!(disk.load("k").is_none());
        disk.store("k", &body, ahead(60), ahead(600));

        let stored = disk.load("k").expect("stored entry");
        assert_eq!(&*stored.body, body.as_str());
        assert!(stored.expires > SystemTime::now());
        assert!(stored.stale_until > stored.expires);
    }

    /// The property that lets this exist at all: nothing recoverable as a
    /// credential is written, but two cookies still land apart.
    ///
    /// The endpoint name *is* in the filename, deliberately — see the module docs
    /// — so what this pins is that the boundary falls in the right place: the
    /// query side of the key, which is the side carrying the cookie, stays inside
    /// the digest.
    #[test]
    fn the_query_is_not_recoverable_from_disk() {
        let scratch = Scratch::new("nokey");
        let disk = DiskCache::open(&scratch.0).unwrap();
        let key = "user_detail\u{0}cookie\u{1}\"MUSIC_U=deadbeefsecret\"\u{2}";
        disk.store(&key, &envelope("1"), ahead(60), ahead(600));

        let path = disk.path_for(&key);
        let name = path.file_name().unwrap().to_str().unwrap();
        // `{endpoint}~{digest}`, and nothing past the separator is anything but
        // the digest.
        let (endpoint, digest) = name.split_once(NAME_SEP).expect("named by endpoint");
        assert_eq!(endpoint, "user_detail");
        assert_eq!(digest.len(), 32);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!name.contains("MUSIC_U"));

        // ...and the contents carry the body only.
        let raw = String::from_utf8(fs::read(&path).unwrap()).unwrap();
        assert!(
            !raw.contains("deadbeefsecret") && !raw.contains("MUSIC_U"),
            "the cookie reached the disk: {raw}"
        );

        // Two accounts do not share a file.
        let other = "user_detail\u{0}cookie\u{1}\"MUSIC_U=someoneelse\"\u{2}";
        assert_ne!(disk.path_for(&key), disk.path_for(other));
        assert!(disk.load(other).is_none());
    }

    #[test]
    fn an_expired_file_is_a_miss_and_is_removed() {
        let scratch = Scratch::new("expired");
        let disk = DiskCache::open(&scratch.0).unwrap();
        // Already past its stale bound.
        disk.store(
            "k",
            &envelope("1"),
            SystemTime::now() - Duration::from_secs(120),
            SystemTime::now() - Duration::from_secs(60),
        );
        let path = disk.path_for("k");
        assert!(path.exists());

        assert!(disk.load("k").is_none());
        assert!(!path.exists(), "a dead entry should not be re-read forever");
    }

    /// A write interrupted by the process going away leaves a truncated file,
    /// and serving half an envelope would be worse than a miss.
    #[test]
    fn a_corrupt_file_is_discarded() {
        let scratch = Scratch::new("corrupt");
        let disk = DiskCache::open(&scratch.0).unwrap();

        for junk in [
            "".to_owned(),
            "not ours at all".to_owned(),
            format!("{MAGIC} 99999999999 99999999999\n{{\"ok\":true,\"status\":200,\"bo"),
            format!("{MAGIC} bad bad\n{}", envelope("1")),
        ] {
            fs::write(disk.path_for("k"), junk).unwrap();
            assert!(disk.load("k").is_none());
            assert!(!disk.path_for("k").exists());
        }
    }

    #[test]
    fn an_oversized_entry_is_refused() {
        let scratch = Scratch::new("oversized");
        let disk = DiskCache::open(&scratch.0).unwrap();
        let huge = envelope(&format!("\"{}\"", "x".repeat(MAX_ENTRY_BYTES)));
        disk.store("k", &huge, ahead(60), ahead(600));
        assert!(!disk.path_for("k").exists());
    }

    #[test]
    fn pruning_drops_dead_entries_and_leftover_temporaries() {
        let scratch = Scratch::new("prune");
        let disk = DiskCache::open(&scratch.0).unwrap();

        disk.store("live", &envelope("1"), ahead(60), ahead(600));
        disk.store(
            "dead",
            &envelope("2"),
            SystemTime::now() - Duration::from_secs(120),
            SystemTime::now() - Duration::from_secs(60),
        );
        fs::write(disk.dir.join("abcdef.tmp"), "half a write").unwrap();

        disk.prune();

        assert!(disk.path_for("live").exists(), "a live entry was dropped");
        assert!(!disk.path_for("dead").exists());
        assert!(!disk.dir.join("abcdef.tmp").exists());
    }

    /// Replacing an entry must not leave the old bytes behind.
    #[test]
    fn a_rewrite_replaces_rather_than_appends() {
        let scratch = Scratch::new("rewrite");
        let disk = DiskCache::open(&scratch.0).unwrap();
        disk.store("k", &envelope(r#"{"v":1}"#), ahead(60), ahead(600));
        disk.store("k", &envelope(r#"{"v":2}"#), ahead(60), ahead(600));

        let stored = disk.load("k").unwrap();
        assert_eq!(&*stored.body, envelope(r#"{"v":2}"#).as_str());
        assert!(!disk.dir.join("k.tmp").exists());
    }

    /// What the memory tier cannot do for us. A write invalidates by endpoint,
    /// and the entries that most need dropping are the ones memory evicted
    /// first — so this has to work without ever having seen the key.
    #[test]
    fn an_endpoints_files_go_without_their_keys() {
        let scratch = Scratch::new("byendpoint");
        let disk = DiskCache::open(&scratch.0).unwrap();

        // Two accounts' worth of the same endpoint, plus two neighbours that must
        // survive: one unrelated, one whose name merely starts the same way.
        let a = "playlist_detail\u{0}id\u{1}1\u{2}cookie\u{1}\"MUSIC_U=aaa\"\u{2}";
        let b = "playlist_detail\u{0}id\u{1}2\u{2}cookie\u{1}\"MUSIC_U=bbb\"\u{2}";
        let song = "song_detail\u{0}ids\u{1}\"1\"\u{2}";
        let user = "user_playlist\u{0}uid\u{1}1\u{2}";
        for key in [a, b, song, user] {
            disk.store(key, &envelope("1"), ahead(60), ahead(600));
        }

        assert_eq!(
            disk.remove_endpoints(&["playlist_detail", "user_playlist"]),
            3
        );
        assert!(disk.load(a).is_none(), "a like left the playlist on disk");
        assert!(disk.load(b).is_none(), "another account's copy survived");
        assert!(disk.load(user).is_none());
        assert!(
            disk.load(song).is_some(),
            "an unrelated endpoint was dropped"
        );
    }

    /// Prefix-matching here would drop things a write did not touch — the memory
    /// tier's own matching is exact for the same reason.
    #[test]
    fn invalidation_matches_the_whole_endpoint_name() {
        let scratch = Scratch::new("wholename");
        let disk = DiskCache::open(&scratch.0).unwrap();
        let detail = "user_detail\u{0}uid\u{1}1\u{2}";
        disk.store(detail, &envelope("1"), ahead(60), ahead(600));

        // `user_playlist` is what a playlist write invalidates; `user_detail`
        // shares its first five characters and must not be caught by it.
        assert_eq!(disk.remove_endpoints(&["user_playlist"]), 0);
        assert!(
            disk.load(detail).is_some(),
            "user_detail was dropped by mistake"
        );
    }

    /// Files from the digest-only naming are unreachable — nothing can look one
    /// up, and nothing can invalidate one — so pruning has to collect them rather
    /// than let them hold up to a week of the budget.
    #[test]
    fn pruning_drops_files_from_the_old_naming() {
        let scratch = Scratch::new("legacy");
        let disk = DiskCache::open(&scratch.0).unwrap();
        let legacy = disk.dir.join("0123456789abcdef0123456789abcdef");
        let mut out = format!("{MAGIC} {} {}\n", unix(ahead(60)), unix(ahead(600)));
        out.push_str(&envelope("1"));
        fs::write(&legacy, out).unwrap();
        disk.store("k", &envelope("2"), ahead(60), ahead(600));

        disk.prune();

        assert!(!legacy.exists(), "an unreachable file was kept");
        assert!(disk.path_for("k").exists(), "a live entry was dropped");
    }
}
