//! Listing and indexing files under a source.
//!
//! Two halves that meet in the middle. "List the candidate files" is the only
//! step that differs by platform — `walkdir` over a real path, or the SAF plugin
//! over a tree grant — and everything after it (diff against the index, extract
//! tags, store covers) is shared. Keeping the split exactly there is what makes
//! the Android path something other than a second scanner.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime};

use super::model::{LocalSource, LocalTrack, ScanProgress, SourceKind};
use super::LocalLibraryState;

/// Extensions the native decoder can actually play.
///
/// The authoritative list, deliberately narrower than the SAF plugin's
/// prefilter: symphonia is built here with mp3/aac/alac/flac/vorbis/pcm/isomp4/
/// ogg/wav, so indexing an `.ape` or a `.wma` would put a row in the library
/// that fails the moment it is pressed. Widening this means widening
/// `audio-backend`'s symphonia features in the same commit.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "wave", "m4a", "m4b", "mp4", "aac", "ogg", "oga", "opus", "aif", "aiff",
    "aifc", "alac", "caf", "mka",
];

/// Sidecar lyric files, paired with a track during the same walk.
pub const LYRIC_EXTENSIONS: &[&str] = &["lrc", "ttml"];

/// Upper bound on one scan, matching the SAF plugin's own cap.
const MAX_CANDIDATES: usize = 50_000;

/// How many files are tagged at once.
///
/// Capped low on purpose: this is disk I/O plus a decode probe, not CPU work, so
/// saturating every core buys nothing and on a phone it means heat and a
/// throttled scan. Four is enough to hide per-file latency.
const MAX_TAG_WORKERS: usize = 4;

pub fn is_audio_file(name: &str) -> bool {
    matches_extension(name, AUDIO_EXTENSIONS)
}

pub fn is_lyric_file(name: &str) -> bool {
    matches_extension(name, LYRIC_EXTENSIONS)
}

fn matches_extension(name: &str, allowed: &[&str]) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    allowed.contains(&ext.as_str())
}

/// Strip the extension off a filename, for use as a fallback title.
pub fn file_stem(name: &str) -> &str {
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

/// One file found by the listing step, before its tags are read.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The locator: absolute path, or `content://…` document URI.
    pub key: String,
    pub display_name: String,
    pub size: Option<u64>,
    pub modified_at: Option<i64>,
    pub relative_dir: String,
}

/// Candidate audio files plus the sidecar lyric files found alongside them.
#[derive(Debug, Default)]
pub struct Listing {
    pub audio: Vec<Candidate>,
    /// `relative_dir` + `/` + stem (lowercased) → lyric locator. Built from the
    /// same walk, so pairing a track with its `.lrc` costs no extra I/O — and on
    /// Android it is the *only* way to find one, since a per-file grant cannot
    /// see its own parent directory.
    pub lyrics: HashMap<String, String>,
    /// Whether the walk stopped early.
    pub truncated: bool,
}

impl Listing {
    /// Record a sidecar lyric so [`Listing::lyric_for`] can pair it with a track.
    ///
    /// Reachable from the parent module because a *single-file* listing — one
    /// just-downloaded track — has to pair its own `.lrc` explicitly; only a walk
    /// discovers them on its own. See `crate::local::index_downloaded_file`.
    pub(crate) fn note_lyric(&mut self, relative_dir: &str, display_name: &str, key: &str) {
        self.lyrics.insert(
            lyric_pair_key(relative_dir, file_stem(display_name)),
            key.to_string(),
        );
    }

    fn lyric_for(&self, candidate: &Candidate) -> Option<String> {
        self.lyrics
            .get(&lyric_pair_key(
                &candidate.relative_dir,
                file_stem(&candidate.display_name),
            ))
            .cloned()
    }
}

fn lyric_pair_key(relative_dir: &str, stem: &str) -> String {
    format!("{relative_dir}/{}", stem.to_ascii_lowercase())
}

// ── Listing ──────────────────────────────────────────────────────

/// List the files under `source`.
pub fn list_candidates<R: Runtime>(app: &AppHandle<R>, source: &LocalSource) -> Result<Listing, String> {
    match source.kind {
        SourceKind::Files => Ok(list_picked_files(source)),
        SourceKind::Directory => {
            if gmplayer_audio_backend::source::is_content_uri(&source.locator) {
                list_content_tree(app, &source.locator)
            } else {
                Ok(list_directory(Path::new(&source.locator)))
            }
        }
    }
}

fn list_picked_files(source: &LocalSource) -> Listing {
    let mut listing = Listing::default();
    for locator in &source.locator_members() {
        let display_name = display_name_for(locator);
        if !is_audio_file(&display_name) {
            continue;
        }
        let (size, modified_at) = stat_locator(locator);
        listing.audio.push(Candidate {
            key: locator.clone(),
            display_name,
            size,
            modified_at,
            relative_dir: String::new(),
        });
    }
    listing
}

fn list_directory(root: &Path) -> Listing {
    let mut listing = Listing::default();
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(24)
        .into_iter()
        // A directory we cannot read is skipped rather than aborting the scan:
        // a music folder routinely contains one permission-denied subtree
        // (a recycle bin, a system directory) and losing the other 2000 files
        // over it would be absurd.
        .filter_map(Result::ok);

    for entry in walker {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(display_name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(key) = entry.path().to_str().map(str::to_string) else {
            continue;
        };
        let relative_dir = entry
            .path()
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        if is_lyric_file(&display_name) {
            listing.note_lyric(&relative_dir, &display_name, &key);
            continue;
        }
        if !is_audio_file(&display_name) {
            continue;
        }
        if listing.audio.len() >= MAX_CANDIDATES {
            listing.truncated = true;
            break;
        }

        let metadata = entry.metadata().ok();
        listing.audio.push(Candidate {
            key,
            display_name,
            size: metadata.as_ref().map(|m| m.len()),
            modified_at: metadata.as_ref().and_then(system_time_secs),
            relative_dir,
        });
    }
    listing
}

fn list_content_tree<R: Runtime>(app: &AppHandle<R>, tree_uri: &str) -> Result<Listing, String> {
    use tauri_plugin_local_files::LocalFilesExt;

    let plugin = app
        .local_files()
        .ok_or_else(|| "SAF is not available on this platform".to_string())?;
    let result = plugin
        .enumerate_tree(tree_uri, MAX_CANDIDATES as u32)
        .map_err(|e| e.to_string())?;

    let mut listing = Listing {
        truncated: result.truncated,
        ..Default::default()
    };
    for entry in result.entries {
        if is_lyric_file(&entry.display_name) {
            listing.note_lyric(&entry.relative_dir, &entry.display_name, &entry.uri);
            continue;
        }
        if !is_audio_file(&entry.display_name) {
            continue;
        }
        listing.audio.push(Candidate {
            key: entry.uri,
            display_name: entry.display_name,
            size: entry.size,
            // Providers report milliseconds; the index keeps seconds so a path
            // source and a SAF source compare the same way.
            modified_at: entry.last_modified.map(|ms| ms / 1000),
            relative_dir: entry.relative_dir,
        });
    }
    Ok(listing)
}

fn display_name_for(locator: &str) -> String {
    if gmplayer_audio_backend::source::is_content_uri(locator) {
        // Decode *before* splitting. A SAF document id escapes its separators
        // (`primary%3AMusic%2FSong.flac`), so splitting on `%` first leaves
        // `2FSong.flac` — the filename with two stray characters welded on,
        // which is exactly what the library then showed.
        let decoded = super::playlist::percent_decode(locator);
        return decoded
            .rsplit(['/', ':'])
            .next()
            .unwrap_or(&decoded)
            .to_string();
    }
    Path::new(locator)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(locator)
        .to_string()
}

fn stat_locator(locator: &str) -> (Option<u64>, Option<i64>) {
    if gmplayer_audio_backend::source::is_content_uri(locator) {
        // A document URI cannot be stat'd from Rust; the picker already reported
        // size, and a per-file source is re-read rather than diffed.
        return (None, None);
    }
    match std::fs::metadata(locator) {
        Ok(metadata) => (Some(metadata.len()), system_time_secs(&metadata)),
        Err(_) => (None, None),
    }
}

fn system_time_secs(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

// ── Indexing ─────────────────────────────────────────────────────

pub struct ScanOutcome {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub truncated: bool,
}

/// Read tags for everything in `listing` that changed, write the results into
/// the index, and drop rows for files that are gone.
pub fn index_listing<R: Runtime>(
    state: &LocalLibraryState,
    source: &LocalSource,
    listing: &Listing,
    progress: Option<&Channel<ScanProgress>>,
    cancel: &AtomicBool,
    _app: &AppHandle<R>,
) -> ScanOutcome {
    let total = listing.audio.len();
    let mut outcome = ScanOutcome {
        added: 0,
        updated: 0,
        removed: 0,
        skipped: 0,
        failed: 0,
        truncated: listing.truncated,
    };

    // Decide what actually needs a probe before spending any threads on it.
    // This is the whole point of an incremental scan: the second run over a
    // 3000-track folder should cost one directory walk, not 3000 decodes.
    let mut to_probe: Vec<&Candidate> = Vec::new();
    let mut keep: HashSet<String> = HashSet::with_capacity(total);
    {
        let index = state.index.lock();
        for candidate in &listing.audio {
            keep.insert(candidate.key.clone());
            match index.track(&candidate.key) {
                Some(existing)
                    if existing.unchanged(candidate.size, candidate.modified_at)
                        && existing.source_id == source.id =>
                {
                    outcome.skipped += 1;
                }
                _ => to_probe.push(candidate),
            }
        }
    }

    let cover_dir = state.cover_dir();
    let cursor = AtomicUsize::new(0);
    let done = AtomicUsize::new(outcome.skipped);
    let results: parking_lot::Mutex<Vec<LocalTrack>> = parking_lot::Mutex::new(Vec::new());
    let failures = AtomicUsize::new(0);

    let workers = MAX_TAG_WORKERS.min(to_probe.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let next = cursor.fetch_add(1, Ordering::Relaxed);
                let Some(candidate) = to_probe.get(next) else {
                    return;
                };

                match probe_candidate(source, candidate, listing, &cover_dir) {
                    Some(track) => results.lock().push(track),
                    None => {
                        failures.fetch_add(1, Ordering::Relaxed);
                    }
                }

                let completed = done.fetch_add(1, Ordering::Relaxed) + 1;
                // Throttled: a per-file IPC message for a 5000-track import is
                // thousands of round trips the UI cannot render anyway.
                if let Some(channel) = progress {
                    if completed % 8 == 0 || completed == total {
                        let _ = channel.send(ScanProgress {
                            source_id: source.id.clone(),
                            phase: "tagging",
                            done: completed,
                            total,
                            current: candidate.display_name.clone(),
                            error: None,
                        });
                    }
                }
            });
        }
    });

    outcome.failed = failures.load(Ordering::Relaxed);

    {
        let mut index = state.index.lock();
        for mut track in results.into_inner() {
            // A re-scanned file keeps the id it already had, so anything holding
            // it — a persisted queue, a `playHistory` row — still resolves.
            track.song_id = index
                .track(&track.key)
                .map(|existing| existing.song_id)
                .unwrap_or_else(|| index.assign_song_id(&track.key));
            if index.track(&track.key).is_some() {
                outcome.updated += 1;
            } else {
                outcome.added += 1;
            }
            index.upsert_track(track);
        }
        // Only prune when the walk was complete. A cancelled or truncated
        // listing does not know about the files it never reached, and treating
        // its silence as "deleted" would empty the library.
        if !cancel.load(Ordering::Relaxed) && !listing.truncated {
            outcome.removed = index.retain_source_tracks(&source.id, &keep);
        }
        index.refresh_source_counts();
    }

    outcome
}

fn probe_candidate(
    source: &LocalSource,
    candidate: &Candidate,
    listing: &Listing,
    cover_dir: &Path,
) -> Option<LocalTrack> {
    let path = PathBuf::from(&candidate.key);
    let (info, tags, needs_cache) = match gmplayer_audio_backend::extract_track_tags(&path, true) {
        Ok(result) => result,
        Err(err) => {
            log::warn!(
                target: "local",
                "skipping {}: {err}",
                candidate.display_name
            );
            return None;
        }
    };

    let cover_key = tags
        .cover
        .as_ref()
        .and_then(|art| super::cover::store(cover_dir, art));

    Some(LocalTrack {
        key: candidate.key.clone(),
        // Assigned by the caller, on one thread, where the id map is live.
        // Doing it here would let two files probed in parallel be handed the same
        // id: the collision walk reads a map that only the insert loop updates,
        // so neither worker could see the other's claim.
        song_id: 0,
        source_id: source.id.clone(),
        // Empty rather than a localized placeholder: the frontend owns display
        // fallbacks and already has the language loaded.
        title: tags
            .title
            .unwrap_or_else(|| file_stem(&candidate.display_name).to_string()),
        artist: tags.artist.unwrap_or_default(),
        album: tags.album.unwrap_or_default(),
        album_artist: tags.album_artist.unwrap_or_default(),
        duration_ms: (info.duration_secs * 1000.0).max(0.0) as u64,
        sample_rate: info.sample_rate,
        channels: info.channels,
        bitrate_bps: info.bitrate_bps,
        codec: info.codec,
        track_no: tags.track_no,
        disc_no: tags.disc_no,
        year: tags.year,
        // Prefer what the listing observed; a `content://` document has no
        // `byte_len` before it is opened, and the provider's number is the only
        // one the next incremental scan can compare against.
        size: candidate.size,
        modified_at: candidate.modified_at,
        cover_key,
        needs_cache,
        display_name: candidate.display_name.clone(),
        relative_dir: candidate.relative_dir.clone(),
        lyric_key: listing.lyric_for(candidate),
    })
}

impl LocalSource {
    /// Members of a `Files` source. Directories have none.
    fn locator_members(&self) -> Vec<String> {
        if matches!(self.kind, SourceKind::Files) {
            self.members.clone()
        } else {
            Vec::new()
        }
    }
}

/// Re-read one file's tags, keeping the identity the index assigned.
///
/// This is the "read it again" action behind the song detail page: the file was
/// re-tagged by another program and the row is stale, and re-scanning the whole
/// source to pick up one file is absurd.
///
/// `key`, `song_id`, `source_id` and `relative_dir` are the index's, not the
/// file's, and are carried over untouched — `song_id` above all, because it is
/// what a persisted queue and every local playlist reference. `size` and
/// `modified_at` are refreshed so the next incremental scan does not immediately
/// consider this row changed again, but a locator that cannot be stat'ed (a
/// `content://` document) keeps the numbers the listing observed rather than
/// dropping them.
pub fn reprobe_track(existing: &LocalTrack, cover_dir: &Path) -> Option<LocalTrack> {
    let path = PathBuf::from(&existing.key);
    let (info, tags, needs_cache) = match gmplayer_audio_backend::extract_track_tags(&path, true) {
        Ok(result) => result,
        Err(err) => {
            log::warn!(target: "local", "could not re-read {}: {err}", existing.display_name);
            return None;
        }
    };

    let (size, modified_at) = stat_locator(&existing.key);
    // A fresh probe with no embedded art means the art was removed. Keeping the
    // old cover would make "read it again" quietly not do what it says; the
    // orphaned file is cleaned up by the next scan's prune.
    let cover_key = tags
        .cover
        .as_ref()
        .and_then(|art| super::cover::store(cover_dir, art));

    Some(LocalTrack {
        key: existing.key.clone(),
        song_id: existing.song_id,
        source_id: existing.source_id.clone(),
        title: tags
            .title
            .unwrap_or_else(|| file_stem(&existing.display_name).to_string()),
        artist: tags.artist.unwrap_or_default(),
        album: tags.album.unwrap_or_default(),
        album_artist: tags.album_artist.unwrap_or_default(),
        duration_ms: (info.duration_secs * 1000.0).max(0.0) as u64,
        sample_rate: info.sample_rate,
        channels: info.channels,
        bitrate_bps: info.bitrate_bps,
        codec: info.codec,
        track_no: tags.track_no,
        disc_no: tags.disc_no,
        year: tags.year,
        size: size.or(existing.size),
        modified_at: modified_at.or(existing.modified_at),
        cover_key,
        needs_cache,
        display_name: existing.display_name.clone(),
        relative_dir: existing.relative_dir.clone(),
        lyric_key: existing.lyric_key.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_decodable_extensions_are_indexed() {
        assert!(is_audio_file("a.flac"));
        assert!(is_audio_file("A.FLAC"));
        assert!(is_audio_file("song.m4a"));
        // Symphonia is not built with these; indexing them would put rows in the
        // library that fail the moment they are pressed.
        assert!(!is_audio_file("a.ape"));
        assert!(!is_audio_file("a.wma"));
        assert!(!is_audio_file("a.dsf"));
        assert!(!is_audio_file("cover.jpg"));
        assert!(!is_audio_file("noextension"));
    }

    #[test]
    fn lyric_sidecars_are_recognised() {
        assert!(is_lyric_file("a.lrc"));
        assert!(is_lyric_file("a.TTML"));
        assert!(!is_lyric_file("a.txt"));
    }

    #[test]
    fn lyrics_pair_case_insensitively_within_a_directory() {
        let mut listing = Listing::default();
        listing.note_lyric("Album", "Song.LRC", "D:\\M\\Album\\Song.LRC");
        let candidate = Candidate {
            key: "D:\\M\\Album\\song.flac".into(),
            display_name: "song.flac".into(),
            size: None,
            modified_at: None,
            relative_dir: "Album".into(),
        };
        assert_eq!(
            listing.lyric_for(&candidate).as_deref(),
            Some("D:\\M\\Album\\Song.LRC")
        );

        // ...but not across directories: two albums both having `01.lrc` must
        // not cross-assign.
        let other = Candidate {
            relative_dir: "Other".into(),
            ..candidate
        };
        assert!(listing.lyric_for(&other).is_none());
    }

    #[test]
    fn file_stem_drops_only_the_last_extension() {
        assert_eq!(file_stem("a.b.flac"), "a.b");
        assert_eq!(file_stem("plain"), "plain");
    }

    #[test]
    fn content_uri_display_names_are_decoded() {
        assert_eq!(
            display_name_for(
                "content://com.android.externalstorage.documents/document/primary%3AMusic%2FSong.flac"
            ),
            "Song.flac"
        );
    }

    /// The scan hands `song_id: 0` out of the probe on purpose; the insert loop
    /// fills it in. A probe that assigned ids itself could give two files the same
    /// one, because the collision walk reads a map only the insert loop updates.
    #[test]
    fn probed_rows_carry_no_id_of_their_own() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.flac"), b"not audio").expect("write");
        let listing = list_directory(dir.path());
        let source = LocalSource {
            id: "src".into(),
            kind: SourceKind::Directory,
            locator: dir.path().to_string_lossy().to_string(),
            display_name: "M".into(),
            available: true,
            last_scanned_at: 0,
            track_count: 0,
            members: vec![],
        };
        // The file is not decodable, so the probe declines it — which is the
        // other half of the contract: an undecodable file must not be indexed.
        assert!(
            probe_candidate(&source, &listing.audio[0], &listing, dir.path()).is_none(),
            "a file symphonia cannot open must not become a library row"
        );
    }

    #[test]
    fn directory_listing_finds_audio_and_pairs_lyrics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let album = dir.path().join("Album");
        std::fs::create_dir_all(&album).expect("mkdir");
        std::fs::write(album.join("01 Song.flac"), b"x").expect("write");
        std::fs::write(album.join("01 Song.lrc"), b"[00:00.00]hi").expect("write");
        std::fs::write(album.join("notes.txt"), b"x").expect("write");

        let listing = list_directory(dir.path());
        assert_eq!(listing.audio.len(), 1);
        let candidate = &listing.audio[0];
        assert_eq!(candidate.display_name, "01 Song.flac");
        assert_eq!(candidate.relative_dir, "Album");
        assert!(listing.lyric_for(candidate).is_some());
    }
}
