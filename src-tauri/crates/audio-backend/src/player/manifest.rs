//! Native playback manifest store.
//!
//! The manifest is the full but lightweight track list the backend plans
//! against: stable provider identities, traversal order and the minimum
//! metadata the resolver needs. It never contains temporary CDN URLs — those
//! live only in the short-lived resolver cache (`super::source_cache`).
//!
//! Revisions are monotonic. A manifest whose revision is not newer than the
//! stored one is rejected, which is what makes stale in-flight frontend state
//! (a wake-up replay, a racing list edit) unable to overwrite newer state.
//!
//! Traversal order is explicit (`order`), and in random mode it is the
//! backend's: the permutation lives here, survives republishes by identity, and
//! is reported to the frontend, which mirrors it into its queue.
//!
//! The frontend used to shuffle its own playlist and ship that as the order,
//! while `set_mode` shuffled a second copy for a notification press. Two
//! writers meant the queue the user saw and the order the planner walked could
//! differ for as long as a page was away — which is how the screen came to name
//! a different song from the one playing. In random mode `entries` is now a
//! set (plus a hint for where new tracks go); `set_owning_order` is the only
//! place the permutation is decided. Every other mode keeps the wire order.

use std::collections::{HashMap, VecDeque};

use tracing::warn;

use crate::types::{
    NativeManifestEntry, NativePlaybackManifest, NativePlaybackMode, OrderEdit, TrackIdentity,
};

#[derive(Debug, Default)]
pub struct ManifestStore {
    manifest: Option<NativePlaybackManifest>,
    /// Traversal order as positions into `entries`, always fully populated and
    /// validated (the wire `order` may be empty, partial or contain garbage).
    order: Vec<usize>,
    /// Which pass of a random traversal we are on. Bumped on every backend-side
    /// reshuffle so two presses of the notification's mode button do not produce
    /// the same permutation.
    shuffle_pass: u64,
    /// Identity order before shuffling, independent of the UI's mirrored rows.
    natural_order: Vec<String>,
    /// Last completed transaction; retries must reuse its permutation.
    shuffle_id: Option<String>,
}

impl ManifestStore {
    pub fn new() -> Self {
        Self {
            manifest: None,
            order: Vec::new(),
            shuffle_pass: 0,
            natural_order: Vec::new(),
            shuffle_id: None,
        }
    }

    /// Revision of the stored manifest, or `0` when nothing is loaded.
    /// Frontend revisions start at 1 so "loaded" is always distinguishable.
    pub fn revision(&self) -> u64 {
        self.manifest.as_ref().map(|m| m.revision).unwrap_or(0)
    }

    pub fn is_loaded(&self) -> bool {
        self.manifest.is_some()
    }

    /// Replace the manifest wholesale. Returns `false` when the incoming
    /// revision is not strictly newer, leaving the store untouched.
    pub fn set(&mut self, mut manifest: NativePlaybackManifest) -> bool {
        if manifest.revision <= self.revision() || !self.fill_entries(&mut manifest) {
            return false;
        }
        let natural_keys = if !manifest.natural_order.is_empty() {
            manifest
                .natural_order
                .iter()
                .map(TrackIdentity::key)
                .collect()
        } else if self.mode() == NativePlaybackMode::Random
            && manifest.mode == NativePlaybackMode::Random
        {
            self.natural_order.clone()
        } else {
            Vec::new()
        };
        self.natural_order = rebase_order(&natural_keys, &manifest.entries)
            .iter()
            .map(|&position| manifest.entries[position].identity.key())
            .collect();
        self.order = sanitize_order(&manifest.order, manifest.entries.len());
        self.manifest = Some(manifest);
        true
    }

    /// Install a manifest and, in random mode, decide the permutation.
    ///
    /// `None` means the revision was stale and nothing changed. Otherwise the
    /// bool says whether the frontend must be told the order: it differs from
    /// what was published, or a fresh permutation was drawn at all. The second
    /// case matters even when the draw happens to equal the published order —
    /// the frontend keeps asking for a reshuffle until an order answers it.
    ///
    /// - fresh permutation when the frontend replaced the list (`reshuffle`) or
    ///   the mode just became random, with `pin` kept at the head so the change
    ///   alone never moves what is audible;
    /// - the published list as given when its `order_base` is our current
    ///   generation: the page had mirrored our order, so a difference is an edit;
    /// - otherwise the previous permutation carried over by identity, so a
    ///   republish — which follows every track change — cannot restart the pass;
    /// - the very first manifest is trusted as it is: it is the queue the user
    ///   saw last session, which is the permutation to resume.
    pub fn set_owning_order(
        &mut self,
        mut manifest: NativePlaybackManifest,
        pin: Option<&str>,
    ) -> Option<bool> {
        if manifest.revision <= self.revision() || !self.fill_entries(&mut manifest) {
            return None;
        }
        let edits = std::mem::take(&mut manifest.order_edits);
        let previous_mode = self.manifest.as_ref().map(|m| m.mode);
        // Built on the permutation we hold, so any difference is the user's edit
        // ("play next" on a queued track); otherwise it may be a page that was away.
        let edited_ours = manifest.order_base == Some(self.order_generation());
        let previous_keys = if edited_ours {
            Vec::new()
        } else {
            self.order_keys()
        };
        let random = manifest.mode == NativePlaybackMode::Random;
        let legacy_reshuffle = manifest.reshuffle;
        let shuffle = manifest.shuffle.clone();
        self.set(manifest);
        if !random {
            return Some(false);
        }

        let switched = matches!(previous_mode, Some(mode) if mode != NativePlaybackMode::Random);
        let pending = shuffle.as_ref().filter(|state| !state.applied);
        let fresh = match shuffle.as_ref() {
            Some(state) => !state.applied && self.shuffle_id.as_ref() != Some(&state.id),
            None => legacy_reshuffle || switched,
        };
        if fresh {
            self.reshuffle_random();
            if let Some(state) = pending {
                self.shuffle_id = Some(state.id.clone());
            }
            self.pin_to_head(pin);
        } else if previous_mode.is_some() {
            if !edited_ours {
                self.order = rebase_order(&previous_keys, self.entries());
            }
        } else {
            // An applied token proves that the first entries already contain
            // the saved permutation. A new process must not draw it again.
            self.shuffle_id = shuffle.as_ref().map(|state| state.id.clone());
        }
        self.apply_edits(&edits);
        // A stale base is reported even when nothing moved, or the page never
        // learns the generation and every later edit of it is rebased away.
        Some(pending.is_some() || fresh || !edited_ours || !self.is_natural_order())
    }

    /// Build the entries of a manifest told relative to the stored one
    /// (`entries_from`, see its doc). `false` when the store holds another
    /// revision (a replaced player, a cleared or refused manifest) or the rows do
    /// not fit: the planner status that answers it acknowledges the publish with a
    /// lower revision, and the frontend resends the whole list.
    fn fill_entries(&self, manifest: &mut NativePlaybackManifest) -> bool {
        let Some(base) = manifest.entries_from.take() else {
            return true;
        };
        let Some(held) = self.manifest.as_ref().filter(|held| held.revision == base) else {
            warn!(
                "manifest {} is told relative to revision {base}, store holds {}",
                manifest.revision,
                self.revision()
            );
            return false;
        };
        let rows = std::mem::take(&mut manifest.entry_rows);
        if rows.is_empty() {
            manifest.entries = held.entries.clone();
        } else {
            let added = std::mem::take(&mut manifest.entries);
            let entries: Option<Vec<_>> = rows
                .chunks(2)
                .map(|pair| {
                    let (&source, &index) = (pair.first()?, pair.get(1)?);
                    let mut entry = match usize::try_from(source) {
                        Ok(position) => held.entries.get(position)?,
                        Err(_) => added.get(usize::try_from(-1 - source).ok()?)?,
                    }
                    .clone();
                    entry.playlist_index = usize::try_from(index).ok()?;
                    Some(entry)
                })
                .collect();
            let Some(entries) = entries else {
                warn!("manifest {} has rows that do not fit", manifest.revision);
                return false;
            };
            manifest.entries = entries;
        }
        if manifest.natural_order.is_empty() && manifest.mode == NativePlaybackMode::Random {
            manifest.natural_order = held.natural_order.clone();
        }
        true
    }

    /// Replay the user's moves on the order just decided.
    ///
    /// shortcut: a duplicated identity moves its first occurrence; upgrade if a
    /// queue that lists one track twice needs a specific copy moved.
    fn apply_edits(&mut self, edits: &[OrderEdit]) {
        for edit in edits {
            let Some(position) = self.position_of_key(&edit.identity.key()) else {
                continue;
            };
            let Some(from) = self.slot_of_position(position) else {
                continue;
            };
            self.order.remove(from);
            let at = match &edit.after {
                None => Some(0),
                Some(after) => self
                    .position_of_key(&after.key())
                    .and_then(|anchor| self.slot_of_position(anchor))
                    .map(|slot| slot + 1),
            };
            // An anchor that is gone (or the track itself) leaves it where it was.
            self.order.insert(at.unwrap_or(from), position);
        }
    }

    /// Changes exactly when the backend draws a permutation of its own. A
    /// manifest whose `order_base` names it was built on the current one.
    pub fn order_generation(&self) -> u64 {
        self.shuffle_pass
    }

    /// Keys of the traversal, in order. The identity-keyed form of `order`, which
    /// is what survives a manifest replacement.
    #[cfg(test)]
    pub fn order_keys_for_test(&self) -> Vec<String> {
        self.order_keys()
    }

    fn order_keys(&self) -> Vec<String> {
        self.order
            .iter()
            .filter_map(|position| self.entry_at(*position))
            .map(|entry| entry.identity.key())
            .collect()
    }

    /// The traversal by identity, for reporting to a frontend.
    pub fn order_identities(&self) -> Vec<TrackIdentity> {
        self.order
            .iter()
            .filter_map(|position| self.entry_at(*position))
            .map(|entry| entry.identity.clone())
            .collect()
    }

    pub fn shuffle_id(&self) -> Option<&str> {
        self.shuffle_id.as_deref()
    }

    fn is_natural_order(&self) -> bool {
        self.order
            .iter()
            .enumerate()
            .all(|(slot, position)| slot == *position)
    }

    /// Swap the track named by `key` into slot 0. Unknown keys change nothing.
    fn pin_to_head(&mut self, key: Option<&str>) {
        let Some(position) = key.and_then(|key| self.position_of_key(key)) else {
            return;
        };
        if let Some(slot) = self.slot_of_position(position) {
            self.order.swap(0, slot);
        }
    }

    /// Drop the manifest. Returns `false` for a stale clear so a late clear
    /// from an old frontend generation cannot wipe a newer list.
    pub fn clear(&mut self, revision: u64) -> bool {
        if revision < self.revision() {
            return false;
        }
        self.manifest = None;
        self.order.clear();
        self.natural_order.clear();
        self.shuffle_id = None;
        self.shuffle_pass = 0;
        true
    }

    pub fn entries(&self) -> &[NativeManifestEntry] {
        self.manifest
            .as_ref()
            .map(|m| m.entries.as_slice())
            .unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    pub fn mode(&self) -> NativePlaybackMode {
        self.manifest
            .as_ref()
            .map(|m| m.mode)
            .unwrap_or(NativePlaybackMode::Normal)
    }

    /// Whether advancing past the end of the traversal wraps to the start.
    /// Distinct from `windowed` on the bounded queue: that is a transport
    /// detail, this is user-visible repeat semantics.
    pub fn repeat_list(&self) -> bool {
        self.manifest
            .as_ref()
            .map(|m| m.repeat_list)
            .unwrap_or(true)
    }

    /// Full traversal order (positions into `entries`).
    #[cfg(test)]
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// Slot of `position` within the traversal order.
    pub fn slot_of_position(&self, position: usize) -> Option<usize> {
        self.order.iter().position(|p| *p == position)
    }

    pub fn position_at_slot(&self, slot: usize) -> Option<usize> {
        self.order.get(slot).copied()
    }

    /// Switch the traversal mode in place, without a republish.
    ///
    /// A mode change can arrive from a notification button while the WebView is
    /// dead, and the next hop has to honour it immediately, not whenever the page
    /// comes back. Into random this builds a shuffle pinned so the playing track
    /// stays put (`keep_position`); out of random it restores natural order.
    /// Returns `false` when nothing changed.
    ///
    /// The frontend mirrors this order when it is told (`NativeQueueOrderChanged`),
    /// and a republish at a higher revision carries it forward by identity — which
    /// is why this deliberately does not touch `revision`.
    pub fn set_mode(&mut self, mode: NativePlaybackMode, keep_position: Option<usize>) -> bool {
        let Some(manifest) = self.manifest.as_mut() else {
            return false;
        };
        if manifest.mode == mode {
            return false;
        }
        let previous = manifest.mode;
        manifest.mode = mode;

        match (previous, mode) {
            // Into random: shuffle, then pin the playing track to slot 0 so the
            // mode change alone never moves what is currently audible.
            (_, NativePlaybackMode::Random) => {
                self.reshuffle_random();
                if let Some(position) = keep_position {
                    if let Some(slot) = self.slot_of_position(position) {
                        self.order.swap(0, slot);
                    }
                }
            }
            // Entries may themselves be the mirrored random permutation.
            (NativePlaybackMode::Random, _) => {
                self.order = rebase_order(&self.natural_order, self.entries());
            }
            // Normal ↔ single is a repeat decision, not a traversal one.
            _ => {}
        }
        true
    }

    /// Build a traversal order the backend owns. Deterministic in
    /// `(random_seed, revision, shuffle_pass)` so a resumed snapshot reproduces
    /// the same permutation instead of diverging from what the UI last saw.
    pub fn reshuffle_random(&mut self) {
        let len = self.len();
        if len <= 1 {
            return;
        }
        self.shuffle_pass = self.shuffle_pass.wrapping_add(1);
        let seed = self
            .manifest
            .as_ref()
            .and_then(|m| m.random_seed)
            .unwrap_or(DEFAULT_SHUFFLE_SEED)
            ^ self.revision().wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.shuffle_pass.wrapping_mul(0xBF58_476D_1CE4_E5B9);

        let mut order: Vec<usize> = (0..len).collect();
        fisher_yates(&mut order, seed);
        self.order = order;
        // Keep an explicit request's proof through notification mode changes:
        // a page still awaiting its answer must not replay that completed draw.
        if self.shuffle_id.is_none() {
            self.shuffle_id = Some(format!("native:{}:{}", self.revision(), self.shuffle_pass));
        }
    }

    /// Position within `entries` of the identity `key`. Identity — never the
    /// UI-facing `playlist_index`, which may be sparse — decides ordering.
    pub fn position_of_key(&self, key: &str) -> Option<usize> {
        self.entries()
            .iter()
            .position(|entry| entry.identity.key() == key)
    }

    pub fn entry_at(&self, position: usize) -> Option<&NativeManifestEntry> {
        self.entries().get(position)
    }

    /// Cursor the frontend declared when it published this manifest. Used to
    /// re-anchor the planner after a wholesale replace.
    pub fn declared_cursor_key(&self) -> Option<String> {
        let manifest = self.manifest.as_ref()?;
        if let Some(identity) = manifest.cursor_identity.as_ref() {
            return Some(identity.key());
        }
        manifest
            .entries
            .iter()
            .find(|entry| entry.playlist_index == manifest.cursor_index)
            .map(|entry| entry.identity.key())
    }
}

const DEFAULT_SHUFFLE_SEED: u64 = 0x2545_F491_4F6C_DD1D;

/// Validate a wire-supplied traversal order. Anything malformed (wrong length,
/// out-of-range, duplicated, empty) falls back to natural order, and a partial
/// order is completed with the positions it omitted — the planner must always
/// be able to reach every entry.
fn sanitize_order(order: &[usize], len: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let mut seen = vec![false; len];
    let mut out = Vec::with_capacity(len);
    for &position in order {
        if position < len && !seen[position] {
            seen[position] = true;
            out.push(position);
        }
    }
    for (position, visited) in seen.iter().enumerate() {
        if !visited {
            out.push(position);
        }
    }
    out
}

/// Carry a previous permutation over to a new entry list, by identity.
///
/// Tracks that were already ordered keep their relative order; removed ones
/// drop out; a new one is placed right after the nearest earlier entry of the
/// frontend's list, so "play next" (inserted after the cursor) and "add to
/// queue" (appended) land where the user put them. A frontend that mirrors the
/// permutation therefore gets it back unchanged.
///
/// shortcut: a new entry's slot is found by a linear scan, so a list that shares
/// nothing with the old one is handled by the natural-order early return below
/// rather than by n inserts; upgrade if bulk partial overlaps ever get large.
fn rebase_order(previous: &[String], entries: &[NativeManifestEntry]) -> Vec<usize> {
    let len = entries.len();
    // Duplicate identities are legal in a playlist; each occurrence is consumed once.
    let mut available: HashMap<String, VecDeque<usize>> = HashMap::new();
    for (position, entry) in entries.iter().enumerate() {
        available
            .entry(entry.identity.key())
            .or_default()
            .push_back(position);
    }

    let mut placed = vec![false; len];
    let mut order = Vec::with_capacity(len);
    for key in previous {
        if let Some(position) = available.get_mut(key).and_then(VecDeque::pop_front) {
            placed[position] = true;
            order.push(position);
        }
    }
    if order.is_empty() {
        return (0..len).collect();
    }

    for position in 0..len {
        if placed[position] {
            continue;
        }
        let at = (0..position)
            .rev()
            .find(|earlier| placed[*earlier])
            .and_then(|earlier| order.iter().position(|slot| *slot == earlier))
            .map_or(0, |slot| slot + 1);
        order.insert(at, position);
        placed[position] = true;
    }
    order
}

fn splitmix64(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn fisher_yates(order: &mut [usize], seed: u64) {
    let mut state = seed | 1;
    for i in (1..order.len()).rev() {
        state = splitmix64(state);
        let j = (state % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TrackIdentity;

    fn entry(id: &str, playlist_index: usize) -> NativeManifestEntry {
        NativeManifestEntry {
            identity: TrackIdentity::Netease { id: id.to_string() },
            playlist_index,
            title: None,
            artist: None,
            album: None,
            artwork_url: None,
            duration_ms: None,
            fee: None,
            has_pc: false,
        }
    }

    fn manifest(revision: u64, ids: &[&str]) -> NativePlaybackManifest {
        NativePlaybackManifest {
            schema_version: 1,
            revision,
            entries: ids.iter().enumerate().map(|(i, id)| entry(id, i)).collect(),
            order: Vec::new(),
            cursor_identity: None,
            cursor_index: 0,
            mode: NativePlaybackMode::Normal,
            repeat_list: true,
            random_seed: None,
            reshuffle: false,
            shuffle: None,
            natural_order: Vec::new(),
            shuffle_pin: None,
            order_base: None,
            order_edits: Vec::new(),
            entries_from: None,
            entry_rows: Vec::new(),
        }
    }

    /// What a page publishes when only its cursor or mode moved.
    fn reusing(base: u64, revision: u64) -> NativePlaybackManifest {
        NativePlaybackManifest {
            entries_from: Some(base),
            ..manifest(revision, &[])
        }
    }

    fn ids_of(store: &ManifestStore) -> Vec<(String, usize)> {
        store
            .entries()
            .iter()
            .map(|e| (e.identity.key(), e.playlist_index))
            .collect()
    }

    #[test]
    fn a_publish_naming_the_held_entries_reuses_them() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["a", "b"]));
        let held = ids_of(&store);
        assert!(store.set(NativePlaybackManifest {
            cursor_index: 1,
            ..reusing(1, 2)
        }));
        assert_eq!(store.revision(), 2);
        assert_eq!(ids_of(&store), held);
        // The next one names the revision that reused them.
        assert!(store.set(reusing(2, 3)));
        assert_eq!(ids_of(&store), held);
    }

    #[test]
    fn a_random_publish_without_a_natural_order_keeps_the_held_one() {
        let mut store = ManifestStore::new();
        let id = |id: &str| entry(id, 0).identity;
        store.set(NativePlaybackManifest {
            natural_order: vec![id("b"), id("a")],
            ..random_manifest(1, &["a", "b"])
        });
        let random_reuse = |base, revision| NativePlaybackManifest {
            mode: NativePlaybackMode::Random,
            ..reusing(base, revision)
        };
        assert!(store.set(random_reuse(1, 2)));
        assert!(store.set(random_reuse(2, 3)));
        assert_eq!(store.natural_order, vec![id("b").key(), id("a").key()]);
        // Outside random there is no natural order to keep.
        assert!(store.set(reusing(3, 4)));
        assert_eq!(store.natural_order, vec![id("a").key(), id("b").key()]);
    }

    #[test]
    fn rows_reorder_drop_and_add_entries() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["a", "b", "c"]));
        // c, a, then the new d; b is gone.
        assert!(store.set(NativePlaybackManifest {
            entries: vec![entry("d", 0)],
            entry_rows: vec![2, 0, 0, 1, -1, 2],
            ..reusing(1, 2)
        }));
        let key = |id: &str| entry(id, 0).identity.key();
        assert_eq!(
            ids_of(&store),
            vec![(key("c"), 0), (key("a"), 1), (key("d"), 2)]
        );
    }

    #[test]
    fn rows_that_do_not_fit_are_refused() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["a", "b"]));
        let rows = |entry_rows: Vec<i64>, entries| NativePlaybackManifest {
            entries,
            entry_rows,
            ..reusing(1, 2)
        };
        assert!(
            !store.set(rows(vec![2, 0], Vec::new())),
            "past the held entries"
        );
        assert!(
            !store.set(rows(vec![-2, 0], vec![entry("c", 0)])),
            "past the additions"
        );
        assert!(
            !store.set(rows(vec![0, -1], Vec::new())),
            "negative playlist index"
        );
        assert!(!store.set(rows(vec![0], Vec::new())), "half a pair");
        assert_eq!(store.revision(), 1);
    }

    #[test]
    fn entries_named_from_another_revision_are_refused() {
        let mut store = ManifestStore::new();
        assert!(!store.set(reusing(1, 2)), "nothing held");
        store.set(manifest(1, &["a", "b"]));
        assert!(!store.set(reusing(5, 6)));
        assert_eq!(store.set_owning_order(reusing(5, 7), None), None);
        assert_eq!(store.revision(), 1);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn a_reused_list_keeps_the_mirrored_permutation() {
        let mut store = ManifestStore::new();
        let ids = ["a", "b", "c", "d", "e", "f"];
        let order = shuffled(&mut store, &ids);
        let order_ids: Vec<&str> = order
            .iter()
            .map(|k| k.rsplit(':').next().unwrap())
            .collect();
        assert_eq!(
            store.set_owning_order(mirrored_manifest(&store, 3, &order_ids), None),
            Some(false)
        );
        let delta = NativePlaybackManifest {
            entries_from: Some(3),
            ..mirrored_manifest(&store, 4, &[])
        };
        assert_eq!(store.set_owning_order(delta, None), Some(false));
        assert_eq!(keys(&store), order);
    }

    #[test]
    fn empty_store_reports_revision_zero() {
        let store = ManifestStore::new();
        assert_eq!(store.revision(), 0);
        assert!(!store.is_loaded());
        assert!(store.is_empty());
    }

    /// A mode change must never move what is audible. The notification's button
    /// changes the traversal for the *next* hop; if the shuffle also reordered
    /// the playing track out of slot 0 the planner's cursor would be pointing at
    /// a different song than the one in the user's ears.
    #[test]
    fn switching_into_random_keeps_the_playing_track_first() {
        for seed in 0..24u64 {
            let mut store = ManifestStore::new();
            let mut m = manifest(1, &["a", "b", "c", "d", "e"]);
            m.random_seed = Some(seed);
            store.set(m);

            let playing = 2; // "c"
            assert!(store.set_mode(NativePlaybackMode::Random, Some(playing)));
            assert_eq!(
                store.position_at_slot(0),
                Some(playing),
                "seed {seed} moved the playing track out of slot 0"
            );
            let mut sorted = store.order().to_vec();
            sorted.sort_unstable();
            assert_eq!(
                sorted,
                vec![0, 1, 2, 3, 4],
                "seed {seed} is not a permutation"
            );
        }
    }

    /// Leaving random restores the list the user is looking at, rather than
    /// stranding them in whatever permutation the shuffle produced.
    ///
    /// Natural order is not an arbitrary choice: it is what the frontend
    /// publishes for every mode, random included (its shuffle is the playlist
    /// order). The interim order this sets has to match the manifest that
    /// replaces it, or the traversal would visibly change twice for one press.
    #[test]
    fn leaving_random_restores_natural_order() {
        for target in [NativePlaybackMode::Normal, NativePlaybackMode::Single] {
            let mut store = ManifestStore::new();
            let mut m = manifest(1, &["a", "b", "c", "d"]);
            m.random_seed = Some(7);
            store.set(m);

            store.set_mode(NativePlaybackMode::Random, Some(1));
            assert!(store.set_mode(target, Some(1)));
            assert_eq!(
                store.order(),
                &[0, 1, 2, 3],
                "leaving random for {target:?}"
            );
        }
    }

    /// Normal ↔ single is a repeat decision, so it must not reshuffle anything.
    #[test]
    fn normal_and_single_share_one_traversal() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["a", "b", "c", "d"]));

        assert!(store.set_mode(NativePlaybackMode::Single, Some(0)));
        assert_eq!(store.order(), &[0, 1, 2, 3]);
        assert!(store.set_mode(NativePlaybackMode::Normal, Some(0)));
        assert_eq!(store.order(), &[0, 1, 2, 3]);
    }

    #[test]
    fn setting_the_same_mode_is_a_no_op() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["a", "b"]));
        assert!(!store.set_mode(NativePlaybackMode::Normal, Some(0)));
        // Nothing loaded: there is no traversal to change.
        let mut empty = ManifestStore::new();
        assert!(!empty.set_mode(NativePlaybackMode::Random, None));
    }

    #[test]
    fn set_rejects_non_monotonic_revisions() {
        let mut store = ManifestStore::new();
        assert!(store.set(manifest(2, &["a", "b"])));
        assert_eq!(store.revision(), 2);

        assert!(!store.set(manifest(2, &["x"])), "same revision is stale");
        assert!(!store.set(manifest(1, &["y"])), "older revision is stale");
        assert_eq!(store.len(), 2, "rejected sets must not mutate entries");

        assert!(store.set(manifest(3, &["c"])));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn clear_rejects_stale_revision_but_accepts_current() {
        let mut store = ManifestStore::new();
        store.set(manifest(5, &["a"]));

        assert!(!store.clear(4), "a clear from an older generation is stale");
        assert!(store.is_loaded());

        assert!(store.clear(5));
        assert!(!store.is_loaded());
        assert_eq!(store.revision(), 0);
    }

    #[test]
    fn position_uses_identity_not_playlist_index() {
        let mut store = ManifestStore::new();
        let mut m = manifest(1, &["a", "b", "c"]);
        // Sparse/non-monotonic UI indices must not affect advancement order.
        m.entries[0].playlist_index = 40;
        m.entries[1].playlist_index = 7;
        m.entries[2].playlist_index = 19;
        store.set(m);

        assert_eq!(store.position_of_key("netease:b"), Some(1));
        assert_eq!(store.entry_at(1).unwrap().playlist_index, 7);
    }

    #[test]
    fn declared_cursor_falls_back_to_playlist_index_match() {
        let mut store = ManifestStore::new();
        let mut m = manifest(1, &["a", "b", "c"]);
        m.cursor_index = 2;
        store.set(m);
        assert_eq!(store.declared_cursor_key().as_deref(), Some("netease:c"));
    }

    #[test]
    fn declared_cursor_prefers_explicit_identity() {
        let mut store = ManifestStore::new();
        let mut m = manifest(1, &["a", "b", "c"]);
        m.cursor_index = 2;
        m.cursor_identity = Some(TrackIdentity::Netease { id: "a".into() });
        store.set(m);
        assert_eq!(store.declared_cursor_key().as_deref(), Some("netease:a"));
    }

    #[test]
    fn absent_order_is_natural_order() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["a", "b", "c"]));
        assert_eq!(store.order(), &[0, 1, 2]);
    }

    #[test]
    fn explicit_order_is_preserved() {
        let mut store = ManifestStore::new();
        let mut m = manifest(1, &["a", "b", "c"]);
        m.order = vec![2, 0, 1];
        store.set(m);
        assert_eq!(store.order(), &[2, 0, 1]);
        assert_eq!(store.slot_of_position(0), Some(1));
        assert_eq!(store.position_at_slot(0), Some(2));
    }

    #[test]
    fn malformed_order_is_repaired_not_trusted() {
        let mut store = ManifestStore::new();
        let mut m = manifest(1, &["a", "b", "c", "d"]);
        // Out of range, duplicated, and incomplete all at once.
        m.order = vec![9, 2, 2, 0];
        store.set(m);

        let order = store.order().to_vec();
        assert_eq!(order.len(), 4, "every entry must stay reachable");
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3], "order must be a permutation");
        assert_eq!(&order[..2], &[2, 0], "valid prefix is respected");
    }

    #[test]
    fn reshuffle_produces_permutation_and_varies_per_pass() {
        let mut store = ManifestStore::new();
        let ids: Vec<String> = (0..12).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut m = manifest(1, &refs);
        m.random_seed = Some(42);
        store.set(m);

        store.reshuffle_random();
        let first = store.order().to_vec();
        store.reshuffle_random();
        let second = store.order().to_vec();

        for pass in [&first, &second] {
            let mut sorted = (*pass).clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..12).collect::<Vec<_>>());
        }
        assert_ne!(first, second, "each pass must reshuffle");
    }

    #[test]
    fn reshuffle_is_deterministic_for_same_seed_and_pass() {
        let build = || {
            let mut store = ManifestStore::new();
            let ids: Vec<String> = (0..10).map(|i| i.to_string()).collect();
            let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
            let mut m = manifest(7, &refs);
            m.random_seed = Some(99);
            store.set(m);
            store.reshuffle_random();
            store.order().to_vec()
        };
        assert_eq!(build(), build(), "resume must reproduce the same pass");
    }

    #[test]
    fn single_entry_reshuffle_is_a_noop() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["only"]));
        store.reshuffle_random();
        assert_eq!(store.order(), &[0]);
    }

    // ── the backend owns the random permutation ──────────────────────────

    fn random_manifest(revision: u64, ids: &[&str]) -> NativePlaybackManifest {
        NativePlaybackManifest {
            mode: NativePlaybackMode::Random,
            random_seed: Some(11),
            order_base: Some(0),
            ..manifest(revision, ids)
        }
    }

    /// What a page publishes after mirroring the store's latest order.
    fn mirrored_manifest(
        store: &ManifestStore,
        revision: u64,
        ids: &[&str],
    ) -> NativePlaybackManifest {
        NativePlaybackManifest {
            order_base: Some(store.order_generation()),
            ..random_manifest(revision, ids)
        }
    }

    fn shuffled(store: &mut ManifestStore, ids: &[&str]) -> Vec<String> {
        store.set_owning_order(random_manifest(1, ids), None);
        store.set_owning_order(
            NativePlaybackManifest {
                reshuffle: true,
                ..random_manifest(2, ids)
            },
            None,
        );
        keys(store)
    }

    fn keys(store: &ManifestStore) -> Vec<String> {
        store.order_keys()
    }

    /// The first manifest after a process start is the queue the user saw last
    /// session. Reshuffling it would throw away the permutation they were in.
    #[test]
    fn the_first_random_manifest_is_trusted() {
        let mut store = ManifestStore::new();
        let outcome = store.set_owning_order(random_manifest(1, &["a", "b", "c", "d"]), None);
        assert_eq!(
            outcome,
            Some(false),
            "nothing differs from what was published"
        );
        assert_eq!(store.order(), &[0, 1, 2, 3]);
    }

    /// A frontend that mirrors the permutation must be able to republish it —
    /// after every track change — without the backend finding a difference.
    #[test]
    fn a_mirrored_republish_changes_nothing() {
        let mut store = ManifestStore::new();
        store.set_owning_order(random_manifest(1, &["a", "b", "c", "d"]), None);
        store.set_owning_order(
            NativePlaybackManifest {
                reshuffle: true,
                ..random_manifest(2, &["a", "b", "c", "d"])
            },
            Some("netease:b"),
        );
        let permutation = keys(&store);

        // The frontend applied that order and publishes its queue as it now is.
        let mirrored: Vec<&str> = permutation.iter().map(|k| &k["netease:".len()..]).collect();
        let outcome = store.set_owning_order(mirrored_manifest(&store, 3, &mirrored), None);
        assert_eq!(outcome, Some(false));
        assert_eq!(keys(&store), permutation);
    }

    /// "Play next" on a track already in the queue moves it in the page's mirror.
    /// Rebasing by identity used to put it straight back, so the action did nothing.
    #[test]
    fn an_edit_of_the_mirrored_order_is_taken_as_published() {
        let mut store = ManifestStore::new();
        let held = shuffled(&mut store, &["a", "b", "c", "d", "e"]);
        let mut edited: Vec<&str> = held.iter().map(|k| &k["netease:".len()..]).collect();
        let moved = edited.remove(4);
        edited.insert(1, moved);

        let outcome = store.set_owning_order(mirrored_manifest(&store, 3, &edited), None);
        assert_eq!(outcome, Some(false), "nothing for the page to mirror back");
        let expected: Vec<String> = edited.iter().map(|id| format!("netease:{id}")).collect();
        assert_eq!(keys(&store), expected);
    }

    /// The same list built on an order the backend has since replaced is a stale
    /// page, not an edit: rebased, and reported so the page learns the generation.
    #[test]
    fn an_edit_on_a_superseded_order_is_rebased_and_reported() {
        let mut store = ManifestStore::new();
        let held = shuffled(&mut store, &["a", "b", "c", "d", "e"]);
        let mut edited: Vec<&str> = held.iter().map(|k| &k["netease:".len()..]).collect();
        let moved = edited.remove(4);
        edited.insert(1, moved);

        let stale = NativePlaybackManifest {
            order_base: Some(store.order_generation() - 1),
            ..random_manifest(3, &edited)
        };
        assert_eq!(store.set_owning_order(stale, None), Some(true));
        assert_eq!(keys(&store), held);
    }

    /// "Play next" pressed while a reshuffle was in flight: the list is built on
    /// the old order, so it is rebased onto the new draw, and the move carried
    /// with it is replayed there instead of being lost.
    #[test]
    fn an_edit_on_a_superseded_order_is_replayed_on_the_new_one() {
        let mut store = ManifestStore::new();
        let held = shuffled(&mut store, &["a", "b", "c", "d", "e"]);
        let id = |key: &String| key["netease:".len()..].to_string();
        let (playing, moved) = (id(&held[0]), id(&held[4]));
        let netease = |id: &str| TrackIdentity::Netease { id: id.to_string() };

        let stale = NativePlaybackManifest {
            order_base: Some(store.order_generation() - 1),
            order_edits: vec![OrderEdit {
                identity: netease(&moved),
                after: Some(netease(&playing)),
            }],
            ..random_manifest(3, &["a", "b", "c", "d", "e"])
        };
        assert_eq!(store.set_owning_order(stale, None), Some(true));
        let mut expected = held.clone();
        let key = expected.remove(4);
        expected.insert(1, key);
        assert_eq!(keys(&store), expected);
    }

    #[test]
    fn an_edit_to_the_head_or_after_a_missing_track() {
        let mut store = ManifestStore::new();
        let held = shuffled(&mut store, &["a", "b", "c"]);
        let netease = |key: &String| TrackIdentity::Netease {
            id: key["netease:".len()..].to_string(),
        };
        let ids: Vec<String> = held
            .iter()
            .map(|k| k["netease:".len()..].to_string())
            .collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();

        let manifest = NativePlaybackManifest {
            order_edits: vec![
                OrderEdit {
                    identity: netease(&held[2]),
                    after: None,
                },
                OrderEdit {
                    identity: netease(&held[0]),
                    after: Some(TrackIdentity::Netease { id: "gone".into() }),
                },
            ],
            ..mirrored_manifest(&store, 3, &refs)
        };
        store.set_owning_order(manifest, None);
        assert_eq!(
            keys(&store),
            vec![held[2].clone(), held[0].clone(), held[1].clone()]
        );
    }

    /// The bug itself: a page that was away publishes its stale queue order. The
    /// backend keeps the permutation it has been walking and says it differs.
    #[test]
    fn a_stale_queue_does_not_overwrite_the_permutation() {
        let mut store = ManifestStore::new();
        store.set_owning_order(random_manifest(1, &["a", "b", "c", "d", "e"]), None);
        store.set_owning_order(
            NativePlaybackManifest {
                reshuffle: true,
                ..random_manifest(2, &["a", "b", "c", "d", "e"])
            },
            Some("netease:c"),
        );
        let held = keys(&store);
        assert_eq!(held[0], "netease:c", "the pinned track leads the pass");

        let outcome = store.set_owning_order(random_manifest(3, &["a", "b", "c", "d", "e"]), None);
        assert_eq!(outcome, Some(true), "the frontend's queue is out of date");
        assert_eq!(keys(&store), held);
    }

    /// Entering random on a republish (the notification press and the in-app
    /// button race) shuffles once, with the playing track still first.
    #[test]
    fn becoming_random_shuffles_with_the_pin_first() {
        for seed in 0..16u64 {
            let mut store = ManifestStore::new();
            store.set_owning_order(manifest(1, &["a", "b", "c", "d", "e"]), None);
            let outcome = store.set_owning_order(
                NativePlaybackManifest {
                    random_seed: Some(seed),
                    ..random_manifest(2, &["a", "b", "c", "d", "e"])
                },
                Some("netease:d"),
            );
            assert!(outcome.is_some());
            assert_eq!(keys(&store)[0], "netease:d", "seed {seed}");
            let mut sorted = store.order().to_vec();
            sorted.sort_unstable();
            assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
        }
    }

    /// The frontend keeps asking for a reshuffle until an order answers it, so a
    /// draw that lands on the published order must still be reported. Two tracks
    /// with the first pinned can only ever land there.
    #[test]
    fn a_fresh_draw_is_reported_even_when_it_matches() {
        let mut store = ManifestStore::new();
        store.set_owning_order(random_manifest(1, &["a", "b"]), None);
        let outcome = store.set_owning_order(
            NativePlaybackManifest {
                reshuffle: true,
                ..random_manifest(2, &["a", "b"])
            },
            Some("netease:a"),
        );
        assert_eq!(store.order(), &[0, 1], "the pin leaves only one order");
        assert_eq!(outcome, Some(true), "the request is answered anyway");

        // Carried over rather than drawn, the same order is not news.
        assert_eq!(
            store.set_owning_order(mirrored_manifest(&store, 3, &["a", "b"]), None),
            Some(false)
        );
    }

    /// Other modes never get a backend order.
    #[test]
    fn non_random_manifests_keep_the_wire_order() {
        let mut store = ManifestStore::new();
        let mut m = manifest(1, &["a", "b", "c"]);
        m.order = vec![2, 0, 1];
        assert_eq!(store.set_owning_order(m, None), Some(false));
        assert_eq!(store.order(), &[2, 0, 1]);
    }

    #[test]
    fn a_stale_revision_is_refused_without_touching_the_order() {
        let mut store = ManifestStore::new();
        store.set_owning_order(random_manifest(5, &["a", "b", "c"]), None);
        assert_eq!(
            store.set_owning_order(random_manifest(5, &["c"]), None),
            None
        );
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn retries_of_one_shuffle_request_keep_the_same_pass() {
        let mut store = ManifestStore::new();
        let request = |revision, id: &str| NativePlaybackManifest {
            shuffle: Some(crate::types::QueueShuffleState {
                id: id.into(),
                applied: false,
            }),
            reshuffle: true,
            ..random_manifest(revision, &["a", "b", "c", "d", "e", "f"])
        };
        assert_eq!(
            store.set_owning_order(request(1, "first"), Some("netease:a")),
            Some(true)
        );
        let held = keys(&store);
        let pass = store.shuffle_pass;
        for revision in 2..5 {
            assert_eq!(
                store.set_owning_order(request(revision, "first"), Some("netease:c")),
                Some(true)
            );
            assert_eq!(keys(&store), held, "a retry must not restart the pass");
            assert_eq!(store.shuffle_pass, pass);
            assert_eq!(store.shuffle_id(), Some("first"));
        }
        store.set_owning_order(request(5, "second"), Some("netease:c"));
        assert_eq!(store.shuffle_pass, pass + 1);
        assert_eq!(store.shuffle_id(), Some("second"));
        assert_eq!(keys(&store)[0], "netease:c");
    }

    #[test]
    fn applied_shuffle_token_survives_a_cold_start() {
        let saved = ["c", "a", "d", "b"];
        for revision in [1, 20, 99] {
            let mut store = ManifestStore::new();
            let manifest = NativePlaybackManifest {
                shuffle: Some(crate::types::QueueShuffleState {
                    id: "saved".into(),
                    applied: true,
                }),
                ..random_manifest(revision, &saved)
            };
            assert_eq!(
                store.set_owning_order(manifest, Some("netease:d")),
                Some(false)
            );
            assert_eq!(keys(&store), keyed(&saved));
            assert_eq!(store.shuffle_id(), Some("saved"));
            assert_eq!(store.shuffle_pass, 0);
        }
    }

    #[test]
    fn leaving_random_after_cold_restore_uses_the_original_order() {
        for mode in [NativePlaybackMode::Normal, NativePlaybackMode::Single] {
            let mut store = ManifestStore::new();
            let natural: Vec<_> = ["a", "b", "c", "d"]
                .iter()
                .map(|id| TrackIdentity::Netease { id: (*id).into() })
                .collect();
            store.set_owning_order(
                NativePlaybackManifest {
                    natural_order: natural.clone(),
                    shuffle: Some(crate::types::QueueShuffleState {
                        id: "saved".into(),
                        applied: true,
                    }),
                    ..random_manifest(1, &["c", "a", "d", "b"])
                },
                None,
            );
            // A mirrored republish replaces the entry positions, not their origin.
            store.set_owning_order(
                NativePlaybackManifest {
                    natural_order: natural,
                    ..random_manifest(2, &["c", "a", "d", "b"])
                },
                None,
            );
            assert!(store.set_mode(mode, Some(0)));
            assert_eq!(keys(&store), keyed(&["a", "b", "c", "d"]));
        }
    }

    #[test]
    fn natural_order_retains_local_duplicates_and_tracks_list_edits() {
        let local = |path: &str| TrackIdentity::Local { path: path.into() };
        let mut initial = random_manifest(1, &["c", "a", "b", "a"]);
        for entry in &mut initial.entries {
            entry.identity = local(entry.identity.netease_id().unwrap());
        }
        initial.natural_order = ["a", "b", "a", "c"]
            .iter()
            .map(|path| local(path))
            .collect();
        let mut store = ManifestStore::new();
        store.set_owning_order(initial, None);
        let mut edited = random_manifest(2, &["c", "a", "x", "a"]);
        for entry in &mut edited.entries {
            entry.identity = local(entry.identity.netease_id().unwrap());
        }
        edited.natural_order = ["a", "a", "c", "x"]
            .iter()
            .map(|path| local(path))
            .collect();
        store.set_owning_order(edited, None);
        store.set_mode(NativePlaybackMode::Normal, None);
        assert_eq!(
            store.order_identities(),
            ["a", "a", "c", "x"]
                .iter()
                .map(|path| local(path))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn notification_shuffle_ids_survive_republishes() {
        let mut store = ManifestStore::new();
        store.set_owning_order(manifest(1, &["a", "b", "c", "d"]), None);
        store.set_mode(NativePlaybackMode::Random, Some(0));
        let first = store.shuffle_id().unwrap().to_string();
        let order: Vec<_> = store
            .order_identities()
            .iter()
            .map(|id| id.netease_id().unwrap().to_string())
            .collect();
        let refs: Vec<_> = order.iter().map(String::as_str).collect();
        store.set_owning_order(random_manifest(2, &refs), None);
        assert_eq!(store.shuffle_id(), Some(first.as_str()));
        let pass = store.shuffle_pass;
        store.set_mode(NativePlaybackMode::Single, Some(0));
        store.set_mode(NativePlaybackMode::Random, Some(0));
        assert_eq!(store.shuffle_pass, pass + 1);
        assert_eq!(store.shuffle_id(), Some(first.as_str()));
    }

    #[test]
    fn a_completed_request_stays_completed_after_notification_mode_changes() {
        let mut store = ManifestStore::new();
        let request = |revision| NativePlaybackManifest {
            shuffle: Some(crate::types::QueueShuffleState {
                id: "request".into(),
                applied: false,
            }),
            ..random_manifest(revision, &["a", "b", "c", "d", "e", "f"])
        };
        store.set_owning_order(request(1), Some("netease:a"));
        store.set_mode(NativePlaybackMode::Normal, Some(0));
        store.set_mode(NativePlaybackMode::Random, Some(0));
        let pass = store.shuffle_pass;
        let order = keys(&store);
        // The page died before the answer and still has the pending request.
        assert_eq!(
            store.set_owning_order(request(2), Some("netease:a")),
            Some(true)
        );
        assert_eq!(store.shuffle_pass, pass);
        assert_eq!(keys(&store), order);
        assert_eq!(store.shuffle_id(), Some("request"));
    }

    fn entries(ids: &[&str]) -> Vec<NativeManifestEntry> {
        ids.iter().enumerate().map(|(i, id)| entry(id, i)).collect()
    }

    fn keyed(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| format!("netease:{id}")).collect()
    }

    #[test]
    fn rebase_keeps_relative_order_and_drops_removed_tracks() {
        // previous permutation: c a d b
        let order = rebase_order(&keyed(&["c", "a", "d", "b"]), &entries(&["a", "b", "c"]));
        let ids: Vec<&str> = order.iter().map(|p| ["a", "b", "c"][*p]).collect();
        assert_eq!(ids, ["c", "a", "b"]);
    }

    /// "Play next" inserts after the cursor in the frontend's list, "add to
    /// queue" appends. Both must land where the user put them.
    #[test]
    fn rebase_places_a_new_track_after_its_predecessor() {
        // previous permutation: c a d b; the frontend mirrors it and inserts x
        // right after `a`, and appends y.
        let incoming = entries(&["c", "a", "x", "d", "b", "y"]);
        let order = rebase_order(&keyed(&["c", "a", "d", "b"]), &incoming);
        let ids: Vec<String> = order.iter().map(|p| incoming[*p].identity.key()).collect();
        assert_eq!(ids, keyed(&["c", "a", "x", "d", "b", "y"]));
    }

    #[test]
    fn rebase_with_nothing_in_common_is_the_new_list_as_given() {
        let order = rebase_order(&keyed(&["x", "y"]), &entries(&["a", "b", "c"]));
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn rebase_survives_duplicate_identities() {
        let incoming = entries(&["a", "b", "a"]);
        let order = rebase_order(&keyed(&["a", "b", "a"]), &incoming);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![0, 1, 2],
            "every occurrence is visited exactly once"
        );
    }
}
