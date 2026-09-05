//! The library index: sources, tracks, playlists, favourites.
//!
//! Held whole in memory behind one lock and written to `$APPDATA` in **two
//! files**, split by whether the data can be regenerated:
//!
//! - `local-library.bin` — sources and tracks, `bincode`. Compact and fast to
//!   load, which is what makes a cold start with tens of thousands of tracks
//!   cheap. A format that tolerates no schema evolution is acceptable here
//!   precisely because a lost index is recovered by re-scanning.
//! - `local-user-data.json` — playlists and favourites, JSON with
//!   `#[serde(default)]` on every field. These are *not* regenerable: nothing
//!   can reconstruct which songs a person collected. So they get the format
//!   that survives a field being added, and one a human can repair by hand.
//!
//! The split also fixes write amplification, which is the practical reason it
//! exists: pressing the heart on the lock screen rewrites a few kilobytes of
//! JSON rather than the whole track table.
//!
//! No SQLite. `rusqlite` would mean cross-compiling C for Android, and the CI
//! there has already paid that tax once for `rquickjs-sys`'s bindgen.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{
    CoverImport, LocalPlaylist, LocalSource, LocalTrack, LyricCompanions, LyricImport,
    TrackOverride,
};

/// Bumped when the `bincode` layout of [`LibraryFile`] changes. A mismatch
/// discards the file and re-scans; it must never discard `local-user-data.json`.
pub const LIBRARY_VERSION: u32 = 1;

/// Bumped only for a change JSON's `#[serde(default)]` cannot absorb.
pub const USER_DATA_VERSION: u32 = 1;

const LIBRARY_FILE: &str = "local-library.bin";
const USER_DATA_FILE: &str = "local-user-data.json";

#[derive(Debug, Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    sources: Vec<LocalSource>,
    tracks: Vec<LocalTrack>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserDataFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    playlists: Vec<LocalPlaylist>,
    /// Track keys. Keyed on the locator rather than on `song_id` because the id
    /// is *derived* from the key — re-deriving it (a different hash, a
    /// collision resolved differently) must not be able to drop a favourite.
    #[serde(default)]
    favourites: Vec<String>,
    /// Tag corrections, by track key. Not regenerable — a re-scan overwrites the
    /// scanned row, and this is what survives it.
    #[serde(default)]
    overrides: HashMap<String, TrackOverride>,
    /// Imported lyric files, by track key. The record only names the file; the
    /// text is next to it under `local-lyrics/`.
    #[serde(default)]
    lyrics: HashMap<String, LyricImport>,
    /// Translation and romanisation, by track key. Additive to whatever the main
    /// lyric turns out to be, which is why they are not part of `lyrics` — see
    /// [`LyricCompanions`].
    #[serde(default)]
    companions: HashMap<String, LyricCompanions>,
    /// Imported cover art, by track key. Names a file under `local-covers/`,
    /// alongside the scanned covers — see `CoverImport`.
    #[serde(default)]
    covers: HashMap<String, CoverImport>,
}

/// 64-bit FNV-1a. Small, dependency-free, and stable across releases — which is
/// the only property that matters, since the ids it produces are persisted in
/// `persistData.playlists` and have to still resolve after a restart.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The frontend-facing id for a local track.
///
/// **Negative, always.** `SongData.id` is a `number` all over the frontend and
/// widening it to `string | number` would be a high-risk sweep, so local tracks
/// borrow the negative half of the space. That is not just an encoding trick:
/// Netease ids are always positive, so any code path that forgets to branch on
/// `song.local` degrades to "not plannable" (`identityForSongId` returns `null`
/// for `id <= 0`) instead of asking Netease about track `-123`.
///
/// Masked to 47 bits so the result stays inside `Number.MAX_SAFE_INTEGER` — a
/// full 64-bit hash would round in JavaScript and two tracks would silently
/// collide after the trip through JSON.
///
/// Computed **only here**. The frontend never recomputes it; every track row it
/// sees already carries `songId`. Two implementations of one hash is the exact
/// trap `identity.ts` ↔ `TrackIdentity::key()` already documents, and there is
/// no reason to repeat it.
pub fn local_song_id(key: &str) -> i64 {
    let masked = fnv1a64(key.as_bytes()) & 0x7fff_ffff_ffff;
    // 0 would be falsy in JS and would compare equal to "no id".
    let magnitude = if masked == 0 { 1 } else { masked };
    -(magnitude as i64)
}

/// Stable id for a source, derived from its locator.
pub fn source_id_for(locator: &str) -> String {
    format!("src-{:016x}", fnv1a64(locator.as_bytes()))
}

#[derive(Debug, Default)]
pub struct LocalIndex {
    dir: PathBuf,
    sources: Vec<LocalSource>,
    tracks: HashMap<String, LocalTrack>,
    /// `song_id` → key. Keeps id assignment collision-free without scanning the
    /// whole table on every insert.
    by_song_id: HashMap<i64, String>,
    playlists: Vec<LocalPlaylist>,
    favourites: HashSet<String>,
    overrides: HashMap<String, TrackOverride>,
    lyrics: HashMap<String, LyricImport>,
    companions: HashMap<String, LyricCompanions>,
    covers: HashMap<String, CoverImport>,
    library_dirty: bool,
    user_data_dirty: bool,
}

impl LocalIndex {
    /// Load both files from `dir`, tolerating either being absent or corrupt.
    pub fn load(dir: PathBuf) -> Self {
        let mut index = LocalIndex {
            dir,
            ..Default::default()
        };
        index.load_library();
        index.load_user_data();
        index.rebuild_song_id_map();
        index
    }

    fn library_path(&self) -> PathBuf {
        self.dir.join(LIBRARY_FILE)
    }

    fn user_data_path(&self) -> PathBuf {
        self.dir.join(USER_DATA_FILE)
    }

    fn load_library(&mut self) {
        let path = self.library_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        match bincode::deserialize::<LibraryFile>(&bytes) {
            Ok(file) if file.version == LIBRARY_VERSION => {
                self.sources = file.sources;
                self.tracks = file
                    .tracks
                    .into_iter()
                    .map(|track| (track.key.clone(), track))
                    .collect();
            }
            Ok(file) => {
                log::warn!(
                    target: "local",
                    "local library index is version {} (expected {LIBRARY_VERSION}); re-scan required",
                    file.version
                );
            }
            Err(err) => {
                log::warn!(target: "local", "local library index is unreadable, discarding: {err}");
            }
        }
    }

    fn load_user_data(&mut self) {
        let path = self.user_data_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        match serde_json::from_str::<UserDataFile>(&text) {
            Ok(file) => {
                self.playlists = file.playlists;
                self.favourites = file.favourites.into_iter().collect();
                // An entry that overrides nothing is the same as no entry, and
                // dropping it on load keeps a hand-edited file from growing a
                // tail of empty objects.
                self.overrides = file
                    .overrides
                    .into_iter()
                    .filter(|(_, patch)| !patch.is_empty())
                    .collect();
                self.lyrics = file.lyrics;
                // Same reason as `overrides`: a record naming neither document is
                // the same as no record.
                self.companions = file
                    .companions
                    .into_iter()
                    .filter(|(_, entry)| !entry.is_empty())
                    .collect();
                self.covers = file.covers;
            }
            Err(err) => {
                // Deliberately loud: this file holds the only copy of something
                // the user made by hand, and a silent reset would look like the
                // app forgetting on purpose.
                log::error!(
                    target: "local",
                    "local playlists/favourites could not be read ({err}); \
                     leaving {} in place and starting empty",
                    path.display()
                );
            }
        }
    }

    fn rebuild_song_id_map(&mut self) {
        self.by_song_id = self
            .tracks
            .values()
            .map(|track| (track.song_id, track.key.clone()))
            .collect();
    }

    // ── Persistence ─────────────────────────────────────────────

    /// Write whatever changed. Both writes go through a sibling temp file and a
    /// rename, so a kill mid-write cannot leave a half-written index behind.
    pub fn flush(&mut self) -> std::io::Result<()> {
        if self.library_dirty {
            let file = LibraryFile {
                version: LIBRARY_VERSION,
                sources: self.sources.clone(),
                tracks: self.tracks.values().cloned().collect(),
            };
            let bytes = bincode::serialize(&file)
                .map_err(|e| std::io::Error::other(format!("serialize local library: {e}")))?;
            write_atomic(&self.library_path(), &bytes)?;
            self.library_dirty = false;
        }
        if self.user_data_dirty {
            let mut favourites: Vec<String> = self.favourites.iter().cloned().collect();
            // Sorted so the file is stable between writes: an unordered set
            // would produce a different byte sequence every time and defeat any
            // sync or backup that compares contents.
            favourites.sort();
            let file = UserDataFile {
                version: USER_DATA_VERSION,
                playlists: self.playlists.clone(),
                favourites,
                overrides: self.overrides.clone(),
                lyrics: self.lyrics.clone(),
                companions: self.companions.clone(),
                covers: self.covers.clone(),
            };
            let text = serde_json::to_string_pretty(&file)
                .map_err(|e| std::io::Error::other(format!("serialize local user data: {e}")))?;
            write_atomic(&self.user_data_path(), text.as_bytes())?;
            self.user_data_dirty = false;
        }
        Ok(())
    }

    // ── Sources ─────────────────────────────────────────────────

    pub fn sources(&self) -> &[LocalSource] {
        &self.sources
    }

    pub fn source(&self, id: &str) -> Option<&LocalSource> {
        self.sources.iter().find(|source| source.id == id)
    }

    /// Insert or replace a source, keyed by id.
    pub fn upsert_source(&mut self, source: LocalSource) {
        match self.sources.iter_mut().find(|s| s.id == source.id) {
            Some(existing) => *existing = source,
            None => self.sources.push(source),
        }
        self.library_dirty = true;
    }

    pub fn set_source_available(&mut self, id: &str, available: bool) -> bool {
        let Some(source) = self.sources.iter_mut().find(|s| s.id == id) else {
            return false;
        };
        if source.available == available {
            return false;
        }
        source.available = available;
        self.library_dirty = true;
        true
    }

    /// Drop a source and every track under it.
    ///
    /// Playlist membership and favourites are *not* touched: they are keyed on
    /// locators, so re-importing the same folder restores them intact. A user
    /// who removes a source to re-add it from a different mount point should not
    /// have to rebuild their playlists.
    pub fn remove_source(&mut self, id: &str) -> Vec<String> {
        let removed: Vec<String> = self
            .tracks
            .values()
            .filter(|track| track.source_id == id)
            .map(|track| track.key.clone())
            .collect();
        for key in &removed {
            if let Some(track) = self.tracks.remove(key) {
                self.by_song_id.remove(&track.song_id);
            }
        }
        self.sources.retain(|source| source.id != id);
        self.library_dirty = true;
        removed
    }

    // ── Tracks ──────────────────────────────────────────────────

    pub fn track(&self, key: &str) -> Option<&LocalTrack> {
        self.tracks.get(key)
    }

    pub fn track_by_song_id(&self, song_id: i64) -> Option<&LocalTrack> {
        self.by_song_id
            .get(&song_id)
            .and_then(|key| self.tracks.get(key))
    }

    pub fn tracks(&self) -> impl Iterator<Item = &LocalTrack> {
        self.tracks.values()
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    // ── Overrides ───────────────────────────────────────────────
    //
    // A scanned row is what the file says; a *view* is what the user should see.
    // Every read path that the UI can observe — the page list, the album/artist/
    // folder grouping, the row a restored queue resolves — goes through
    // `track_view`, so a corrected album name is the album name everywhere,
    // including in the keyword filter and the sort. Reading `track()` directly is
    // for the scan and for identity work, where the file's own tags are the point.

    pub fn override_for(&self, key: &str) -> Option<&TrackOverride> {
        self.overrides.get(key)
    }

    /// Lay the user's own data over a scanned row.
    ///
    /// Both layers live outside the track table on purpose, so that a re-scan —
    /// which replaces rows wholesale through `upsert_track` — cannot clobber
    /// them. Everything user-facing reads a *view*, so this is the single place
    /// that decides what "the row" means: `to_dto`'s `coverPath`, the album
    /// grid's bucket cover, `localTrackToSongData`, and the OS media session all
    /// come out of here.
    fn layer_user_data(&self, track: &mut LocalTrack) {
        if let Some(patch) = self.overrides.get(&track.key) {
            patch.apply(track);
        }
        if let Some(cover) = self.covers.get(&track.key) {
            track.cover_key = Some(cover.file.clone());
        }
    }

    /// The row with the user's corrections layered on.
    pub fn track_view(&self, key: &str) -> Option<LocalTrack> {
        let mut track = self.tracks.get(key)?.clone();
        self.layer_user_data(&mut track);
        Some(track)
    }

    pub fn track_view_by_song_id(&self, song_id: i64) -> Option<LocalTrack> {
        let key = self.by_song_id.get(&song_id)?;
        self.track_view(key)
    }

    /// Every row, corrected. Clones — which is what the query paths did anyway.
    pub fn track_views(&self) -> impl Iterator<Item = LocalTrack> + '_ {
        self.tracks.values().map(|track| {
            let mut view = track.clone();
            self.layer_user_data(&mut view);
            view
        })
    }

    /// Replace the override for `key`. An empty patch removes it, which is how
    /// "revert to the file" is expressed. Returns whether anything changed.
    pub fn set_override(&mut self, key: &str, patch: TrackOverride) -> bool {
        if !self.tracks.contains_key(key) {
            return false;
        }
        if patch.is_empty() {
            let removed = self.overrides.remove(key).is_some();
            self.user_data_dirty |= removed;
            return removed;
        }
        self.overrides.insert(key.to_string(), patch);
        self.user_data_dirty = true;
        true
    }

    // ── Imported lyrics ─────────────────────────────────────────

    pub fn lyric_import(&self, key: &str) -> Option<&LyricImport> {
        self.lyrics.get(key)
    }

    pub fn set_lyric_import(&mut self, key: &str, entry: LyricImport) {
        self.lyrics.insert(key.to_string(), entry);
        self.user_data_dirty = true;
    }

    /// Forget an imported lyric. Returns the record, so the caller can delete the
    /// file it named.
    pub fn take_lyric_import(&mut self, key: &str) -> Option<LyricImport> {
        let removed = self.lyrics.remove(key);
        if removed.is_some() {
            self.user_data_dirty = true;
        }
        removed
    }

    /// Filenames every live import references, for pruning the lyric directory.
    ///
    /// The companions count as live references too: they sit in the same directory
    /// and a prune that only knew about imports would delete every downloaded
    /// translation on the next scan.
    pub fn referenced_lyric_files(&self) -> HashSet<String> {
        self.lyrics
            .values()
            .map(|entry| entry.file.clone())
            .chain(
                self.companions
                    .values()
                    .flat_map(|entry| [entry.translation.clone(), entry.romanisation.clone()])
                    .flatten(),
            )
            .collect()
    }

    // ── Lyric companions ────────────────────────────────────────

    pub fn lyric_companions(&self, key: &str) -> Option<&LyricCompanions> {
        self.companions.get(key)
    }

    /// Record translation/romanisation filenames. An entry naming neither is
    /// dropped rather than stored, so the file cannot grow empty objects.
    pub fn set_lyric_companions(&mut self, key: &str, entry: LyricCompanions) {
        if entry.is_empty() {
            self.companions.remove(key);
        } else {
            self.companions.insert(key.to_string(), entry);
        }
        self.user_data_dirty = true;
    }

    pub fn take_lyric_companions(&mut self, key: &str) -> Option<LyricCompanions> {
        let removed = self.companions.remove(key);
        if removed.is_some() {
            self.user_data_dirty = true;
        }
        removed
    }

    // ── Imported cover art ──────────────────────────────────────

    pub fn cover_import(&self, key: &str) -> Option<&CoverImport> {
        self.covers.get(key)
    }

    pub fn set_cover_import(&mut self, key: &str, entry: CoverImport) {
        self.covers.insert(key.to_string(), entry);
        self.user_data_dirty = true;
    }

    /// Forget an imported cover. Returns the record so the caller can decide
    /// whether the file it named is still wanted — it usually is, since a cover
    /// is content-addressed and may be shared with a whole album.
    pub fn take_cover_import(&mut self, key: &str) -> Option<CoverImport> {
        let removed = self.covers.remove(key);
        if removed.is_some() {
            self.user_data_dirty = true;
        }
        removed
    }

    /// Cover filenames every live import references.
    ///
    /// **`prune_unreferenced_covers` must union this with the tracks' own
    /// `cover_key`s.** An imported cover is not on any track row, so a referenced
    /// set built from the tracks alone marks it unreferenced and the next scan
    /// deletes it — which shows up as covers quietly disappearing later, far from
    /// the code that caused it.
    pub fn referenced_cover_files(&self) -> HashSet<String> {
        self.covers.values().map(|entry| entry.file.clone()).collect()
    }

    /// Assign `key` an id that no other key already holds.
    ///
    /// A collision at 47 bits needs on the order of a million tracks to become
    /// likely, so this branch will essentially never be taken — but "essentially
    /// never" is not the same as "cannot", and the failure it would otherwise
    /// produce is two files that are the same song to the entire frontend.
    pub fn assign_song_id(&self, key: &str) -> i64 {
        let mut candidate = key.to_string();
        for _ in 0..8 {
            let id = local_song_id(&candidate);
            match self.by_song_id.get(&id) {
                Some(existing) if existing != key => candidate.push('\0'),
                _ => return id,
            }
        }
        // Eight collisions in a row is not a hash problem, it is a bug. Take the
        // id anyway rather than refusing to index the file.
        local_song_id(&candidate)
    }

    pub fn upsert_track(&mut self, track: LocalTrack) {
        if let Some(previous) = self.tracks.get(&track.key) {
            self.by_song_id.remove(&previous.song_id);
        }
        self.by_song_id.insert(track.song_id, track.key.clone());
        self.tracks.insert(track.key.clone(), track);
        self.library_dirty = true;
    }

    /// Remove tracks belonging to `source_id` whose keys are not in `keep`.
    /// This is how a re-scan reflects deletions.
    pub fn retain_source_tracks(&mut self, source_id: &str, keep: &HashSet<String>) -> usize {
        let doomed: Vec<String> = self
            .tracks
            .values()
            .filter(|track| track.source_id == source_id && !keep.contains(&track.key))
            .map(|track| track.key.clone())
            .collect();
        for key in &doomed {
            if let Some(track) = self.tracks.remove(key) {
                self.by_song_id.remove(&track.song_id);
            }
        }
        if !doomed.is_empty() {
            self.library_dirty = true;
        }
        doomed.len()
    }

    /// Recompute and store the cached per-source counts.
    pub fn refresh_source_counts(&mut self) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for track in self.tracks.values() {
            *counts.entry(track.source_id.clone()).or_default() += 1;
        }
        for source in &mut self.sources {
            source.track_count = counts.get(&source.id).copied().unwrap_or(0);
        }
        self.library_dirty = true;
    }

    // ── Favourites ──────────────────────────────────────────────

    pub fn is_favourite(&self, key: &str) -> bool {
        self.favourites.contains(key)
    }

    pub fn favourites(&self) -> &HashSet<String> {
        &self.favourites
    }

    /// Set the favourite flag for `key`. Returns whether anything changed, which
    /// is what gates the `SessionControlsChanged` emit — an unconditional emit
    /// would have the frontend adopt its own push and republish forever.
    pub fn set_favourite(&mut self, key: &str, favourite: bool) -> bool {
        let changed = if favourite {
            self.favourites.insert(key.to_string())
        } else {
            self.favourites.remove(key)
        };
        if changed {
            self.user_data_dirty = true;
        }
        changed
    }

    // ── Playlists ───────────────────────────────────────────────

    pub fn playlists(&self) -> &[LocalPlaylist] {
        &self.playlists
    }

    pub fn playlist(&self, id: &str) -> Option<&LocalPlaylist> {
        self.playlists.iter().find(|list| list.id == id)
    }

    pub fn upsert_playlist(&mut self, playlist: LocalPlaylist) {
        match self.playlists.iter_mut().find(|p| p.id == playlist.id) {
            Some(existing) => *existing = playlist,
            None => self.playlists.push(playlist),
        }
        self.user_data_dirty = true;
    }

    pub fn remove_playlist(&mut self, id: &str) -> bool {
        let before = self.playlists.len();
        self.playlists.retain(|list| list.id != id);
        let removed = self.playlists.len() != before;
        if removed {
            self.user_data_dirty = true;
        }
        removed
    }

    pub fn mutate_playlist<F: FnOnce(&mut LocalPlaylist)>(&mut self, id: &str, f: F) -> bool {
        let Some(playlist) = self.playlists.iter_mut().find(|list| list.id == id) else {
            return false;
        };
        f(playlist);
        self.user_data_dirty = true;
        true
    }
}

/// Write to a sibling temp file and rename over the target.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        // Without this the rename can land before the contents do, which on a
        // power loss leaves a correctly-named empty file — worse than no file,
        // because it looks valid.
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::model::SourceKind;

    fn track(key: &str, song_id: i64, source_id: &str) -> LocalTrack {
        LocalTrack {
            key: key.to_string(),
            song_id,
            source_id: source_id.to_string(),
            title: "t".into(),
            artist: "a".into(),
            album: "al".into(),
            album_artist: String::new(),
            duration_ms: 1000,
            sample_rate: 44_100,
            channels: 2,
            bitrate_bps: None,
            codec: "flac".into(),
            track_no: None,
            disc_no: None,
            year: None,
            size: Some(10),
            modified_at: Some(20),
            cover_key: None,
            needs_cache: false,
            display_name: "t.flac".into(),
            relative_dir: String::new(),
            lyric_key: None,
        }
    }

    #[test]
    fn song_ids_are_negative_stable_and_safe_integers() {
        let id = local_song_id("local-file:D:\\Music\\a.flac");
        assert!(id < 0, "a local id must never collide with a Netease id");
        assert_eq!(id, local_song_id("local-file:D:\\Music\\a.flac"));
        assert!(
            id.unsigned_abs() <= (1u64 << 53) - 1,
            "must survive the trip through a JS number"
        );
        assert_ne!(id, local_song_id("local-file:D:\\Music\\b.flac"));
    }

    #[test]
    fn assign_song_id_walks_away_from_a_collision() {
        let mut index = LocalIndex::default();
        let taken = local_song_id("victim");
        index.upsert_track(track("other-key", taken, "src"));

        // "victim" hashes onto an id a *different* key already holds.
        let assigned = index.assign_song_id("victim");
        assert_ne!(assigned, taken);
        // Deterministic, so a second scan of the same library agrees with the
        // first — otherwise every re-scan would renumber the colliding track and
        // strand it in `persistData.playlists`.
        assert_eq!(index.assign_song_id("victim"), assigned);
    }

    /// The invariant a re-scan depends on: a track that already holds its own
    /// derived id keeps it, so a restored queue still resolves.
    #[test]
    fn assign_song_id_is_stable_for_a_key_that_already_holds_its_id() {
        let mut index = LocalIndex::default();
        let own = local_song_id("mine");
        index.upsert_track(track("mine", own, "src"));
        assert_eq!(index.assign_song_id("mine"), own);
    }

    #[test]
    fn removing_a_source_keeps_playlists_and_favourites() {
        let mut index = LocalIndex::default();
        index.upsert_source(LocalSource {
            id: "src".into(),
            kind: SourceKind::Directory,
            locator: "D:\\Music".into(),
            display_name: "Music".into(),
            available: true,
            last_scanned_at: 0,
            track_count: 0,
            members: vec![],
        });
        index.upsert_track(track("D:\\Music\\a.flac", -1, "src"));
        index.set_favourite("D:\\Music\\a.flac", true);
        index.upsert_playlist(LocalPlaylist {
            id: "p1".into(),
            name: "mine".into(),
            description: String::new(),
            created_at: 0,
            updated_at: 0,
            tracks: vec!["D:\\Music\\a.flac".into()],
            imported_from: None,
        });

        let removed = index.remove_source("src");
        assert_eq!(removed, vec!["D:\\Music\\a.flac".to_string()]);
        assert_eq!(index.track_count(), 0);
        // The whole point: re-importing the folder brings these back.
        assert!(index.is_favourite("D:\\Music\\a.flac"));
        assert_eq!(index.playlist("p1").expect("playlist").tracks.len(), 1);
    }

    /// The scanned row is what the file said; the view is what the user should
    /// see. Grouping and filtering read the view, so this is the assertion that
    /// keeps the album list and the track list from disagreeing.
    #[test]
    fn an_override_is_layered_over_the_scanned_row() {
        let mut index = LocalIndex::default();
        index.upsert_track(track("a", -1, "s1"));
        assert!(index.set_override(
            "a",
            TrackOverride {
                album: Some("Corrected".into()),
                year: Some(1999),
                ..Default::default()
            }
        ));

        let scanned = index.track("a").expect("row");
        assert_eq!(scanned.album, "al", "the scanned row is untouched");
        assert_eq!(scanned.year, None);

        let view = index.track_view("a").expect("view");
        assert_eq!(view.album, "Corrected");
        assert_eq!(view.year, Some(1999));
        // Everything not overridden still comes from the file.
        assert_eq!(view.title, "t");
        assert_eq!(view.song_id, -1, "identity is never an override");
        assert_eq!(index.track_views().count(), 1);
    }

    /// "Revert" is not a separate operation: it is an empty patch. Storing one
    /// would leave the row marked as edited forever.
    #[test]
    fn an_empty_override_removes_the_entry() {
        let mut index = LocalIndex::default();
        index.upsert_track(track("a", -1, "s1"));
        index.set_override(
            "a",
            TrackOverride {
                title: Some("mine".into()),
                ..Default::default()
            },
        );
        assert!(index.override_for("a").is_some());

        assert!(index.set_override("a", TrackOverride::default()));
        assert!(index.override_for("a").is_none());
        assert_eq!(index.track_view("a").expect("view").title, "t");
        // Nothing to remove the second time.
        assert!(!index.set_override("a", TrackOverride::default()));
    }

    /// Same invariant as favourites and playlists: an unplugged drive must not
    /// cost the user work they did by hand.
    #[test]
    fn overrides_and_lyric_imports_survive_removing_a_source() {
        let mut index = LocalIndex::default();
        index.upsert_source(LocalSource {
            id: "src".into(),
            kind: SourceKind::Directory,
            locator: "D:\\Music".into(),
            display_name: "Music".into(),
            available: true,
            last_scanned_at: 0,
            track_count: 0,
            members: vec![],
        });
        index.upsert_track(track("D:\\Music\\a.flac", -1, "src"));
        index.set_override(
            "D:\\Music\\a.flac",
            TrackOverride {
                title: Some("mine".into()),
                ..Default::default()
            },
        );
        index.set_lyric_import(
            "D:\\Music\\a.flac",
            LyricImport {
                file: "dead.lrc".into(),
                kind: crate::local::model::LyricKind::Lrc,
                original_name: "a.lrc".into(),
                imported_at: 0,
            },
        );

        index.remove_source("src");
        assert!(index.override_for("D:\\Music\\a.flac").is_some());
        assert!(index.lyric_import("D:\\Music\\a.flac").is_some());
        // And the file the record names is still considered live, so a prune
        // triggered in between cannot delete it.
        assert!(index.referenced_lyric_files().contains("dead.lrc"));
    }

    /// The companions share the lyric directory with the imports, so a prune that
    /// only knew about imports would delete every downloaded translation on the
    /// next scan — silently, since nothing else names those files.
    #[test]
    fn lyric_companions_are_live_references_too() {
        let mut index = LocalIndex::default();
        index.set_lyric_companions(
            "D:\\Music\\a.flac",
            LyricCompanions {
                translation: Some("1234.t.lrc".into()),
                romanisation: Some("1234.r.lrc".into()),
                stored_at: 0,
            },
        );
        let live = index.referenced_lyric_files();
        assert!(live.contains("1234.t.lrc"));
        assert!(live.contains("1234.r.lrc"));

        // A record naming neither document is the same as no record, so setting one
        // removes rather than stores it — otherwise the file grows empty objects.
        index.set_lyric_companions("D:\\Music\\a.flac", LyricCompanions::default());
        assert!(index.lyric_companions("D:\\Music\\a.flac").is_none());
        assert!(index.referenced_lyric_files().is_empty());
    }

    /// The whole point of putting the import outside the track row: a view carries
    /// it, so every consumer of `cover_key` sees the picked cover, and a re-scan —
    /// which replaces the row — cannot undo it.
    #[test]
    fn an_imported_cover_wins_over_the_scanned_one_and_survives_a_rescan() {
        let mut index = LocalIndex::default();
        let mut scanned = track("D:\\Music\\a.flac", -1, "src");
        scanned.cover_key = Some("from-file.jpg".into());
        index.upsert_track(scanned.clone());

        index.set_cover_import(
            "D:\\Music\\a.flac",
            CoverImport {
                file: "picked.jpg".into(),
                original_name: "art.jpg".into(),
                imported_at: 0,
            },
        );

        let view = index.track_view("D:\\Music\\a.flac").expect("view");
        assert_eq!(view.cover_key.as_deref(), Some("picked.jpg"));
        // The scanned row is untouched, which is what "revert" reads.
        assert_eq!(
            index.track("D:\\Music\\a.flac").unwrap().cover_key.as_deref(),
            Some("from-file.jpg")
        );

        // A re-scan writes the row again; the import must still be in force.
        index.upsert_track(scanned);
        assert_eq!(
            index
                .track_view("D:\\Music\\a.flac")
                .expect("view")
                .cover_key
                .as_deref(),
            Some("picked.jpg")
        );

        // And `prune_unreferenced_covers` has to be able to see it. Without this
        // the next scan deletes the picture and the cover quietly disappears.
        assert!(index.referenced_cover_files().contains("picked.jpg"));

        assert!(index.take_cover_import("D:\\Music\\a.flac").is_some());
        assert_eq!(
            index
                .track_view("D:\\Music\\a.flac")
                .expect("view")
                .cover_key
                .as_deref(),
            Some("from-file.jpg")
        );
    }

    /// A patch for a track the index does not know is refused rather than stored:
    /// it would otherwise sit in the user-data file forever, since nothing ever
    /// walks overrides looking for orphans.
    #[test]
    fn an_override_for_an_unknown_track_is_refused() {
        let mut index = LocalIndex::default();
        assert!(!index.set_override(
            "nope",
            TrackOverride {
                title: Some("x".into()),
                ..Default::default()
            }
        ));
        assert!(index.override_for("nope").is_none());
    }

    #[test]
    fn retain_source_tracks_reflects_deletions_only_for_that_source() {        let mut index = LocalIndex::default();
        index.upsert_track(track("a", -1, "s1"));
        index.upsert_track(track("b", -2, "s1"));
        index.upsert_track(track("c", -3, "s2"));

        let keep: HashSet<String> = ["a".to_string()].into_iter().collect();
        assert_eq!(index.retain_source_tracks("s1", &keep), 1);
        assert!(index.track("a").is_some());
        assert!(index.track("b").is_none());
        assert!(index.track("c").is_some(), "other sources are untouched");
    }

    #[test]
    fn set_favourite_reports_whether_it_changed() {
        let mut index = LocalIndex::default();
        assert!(index.set_favourite("k", true));
        assert!(!index.set_favourite("k", true), "a no-op must stay silent");
        assert!(index.set_favourite("k", false));
        assert!(!index.set_favourite("k", false));
    }

    #[test]
    fn unchanged_needs_both_size_and_mtime() {
        let t = track("a", -1, "s");
        assert!(t.unchanged(Some(10), Some(20)));
        assert!(!t.unchanged(Some(11), Some(20)));
        assert!(!t.unchanged(Some(10), Some(21)));
        // A file whose size we never learned can never be skipped.
        let mut unknown = track("a", -1, "s");
        unknown.size = None;
        assert!(!unknown.unchanged(None, Some(20)));
    }

    /// An incremental scan exists to not re-probe anything, so a fix to how a
    /// field is *derived* would otherwise never reach a library that was already
    /// indexed. `codec` used to be Symphonia's newtype `Debug` — `codectype(4099)`
    /// for every MP3 — and a row still carrying one has to be re-probed even
    /// though the file itself has not moved.
    #[test]
    fn a_row_with_a_stale_derived_codec_is_never_skipped() {
        let mut stale = track("a", -1, "s");
        stale.codec = "codectype(4099)".into();
        assert!(stale.has_stale_derived_fields());
        assert!(!stale.unchanged(Some(10), Some(20)));

        let fixed = track("a", -1, "s");
        assert!(!fixed.has_stale_derived_fields(), "the helper writes a real name");
        assert!(fixed.unchanged(Some(10), Some(20)));
    }

    #[test]
    fn both_files_round_trip_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut index = LocalIndex::load(dir.path().to_path_buf());
            index.upsert_source(LocalSource {
                id: "src".into(),
                kind: SourceKind::Directory,
                locator: "D:\\Music".into(),
                display_name: "Music".into(),
                available: true,
                last_scanned_at: 7,
                track_count: 1,
                members: vec![],
            });
            index.upsert_track(track("D:\\Music\\a.flac", -42, "src"));
            index.set_favourite("D:\\Music\\a.flac", true);
            index.upsert_playlist(LocalPlaylist {
                id: "p1".into(),
                name: "mine".into(),
                description: String::new(),
                created_at: 1,
                updated_at: 2,
                tracks: vec!["D:\\Music\\a.flac".into()],
                imported_from: None,
            });
            index.flush().expect("flush");
        }

        let reloaded = LocalIndex::load(dir.path().to_path_buf());
        assert_eq!(reloaded.sources().len(), 1);
        assert_eq!(reloaded.track_count(), 1);
        assert!(reloaded.is_favourite("D:\\Music\\a.flac"));
        assert_eq!(reloaded.playlists().len(), 1);
        assert_eq!(
            reloaded.track_by_song_id(-42).map(|t| t.key.as_str()),
            Some("D:\\Music\\a.flac")
        );
    }

    /// The two files are independent on purpose: a track index the app cannot
    /// read is re-scanned, and that must not take the user's playlists with it.
    #[test]
    fn a_corrupt_track_index_does_not_lose_user_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut index = LocalIndex::load(dir.path().to_path_buf());
            index.upsert_track(track("a", -1, "src"));
            index.set_favourite("a", true);
            index.flush().expect("flush");
        }
        std::fs::write(dir.path().join(LIBRARY_FILE), b"not bincode").expect("corrupt");

        let reloaded = LocalIndex::load(dir.path().to_path_buf());
        assert_eq!(reloaded.track_count(), 0);
        assert!(reloaded.is_favourite("a"));
    }

    #[test]
    fn a_version_bump_discards_the_index_and_keeps_favourites() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stale = LibraryFile {
            version: LIBRARY_VERSION + 1,
            sources: vec![],
            tracks: vec![track("a", -1, "src")],
        };
        std::fs::write(
            dir.path().join(LIBRARY_FILE),
            bincode::serialize(&stale).expect("serialize"),
        )
        .expect("write");
        std::fs::write(
            dir.path().join(USER_DATA_FILE),
            r#"{"version":1,"playlists":[],"favourites":["a"]}"#,
        )
        .expect("write");

        let index = LocalIndex::load(dir.path().to_path_buf());
        assert_eq!(index.track_count(), 0);
        assert!(index.is_favourite("a"));
    }
}
