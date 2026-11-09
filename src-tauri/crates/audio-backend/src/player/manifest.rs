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

use crate::types::{
    NativeManifestEntry, NativePlaybackManifest, NativePlaybackMode, TrackIdentity,
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
}

impl ManifestStore {
    pub fn new() -> Self {
        Self {
            manifest: None,
            order: Vec::new(),
            shuffle_pass: 0,
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
    pub fn set(&mut self, manifest: NativePlaybackManifest) -> bool {
        if manifest.revision <= self.revision() {
            return false;
        }
        self.order = sanitize_order(&manifest.order, manifest.entries.len());
        self.shuffle_pass = 0;
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
    /// - otherwise the previous permutation carried over by identity, so a
    ///   republish — which follows every track change — cannot restart the pass;
    /// - the very first manifest is trusted as it is: it is the queue the user
    ///   saw last session, which is the permutation to resume.
    pub fn set_owning_order(
        &mut self,
        manifest: NativePlaybackManifest,
        pin: Option<&str>,
    ) -> Option<bool> {
        if manifest.revision <= self.revision() {
            return None;
        }
        let previous_mode = self.manifest.as_ref().map(|m| m.mode);
        let previous_keys = self.order_keys();
        let random = manifest.mode == NativePlaybackMode::Random;
        let reshuffle = manifest.reshuffle;
        self.set(manifest);
        if !random {
            return Some(false);
        }

        let switched = matches!(previous_mode, Some(mode) if mode != NativePlaybackMode::Random);
        let fresh = reshuffle || switched;
        if fresh {
            self.reshuffle_random();
            self.pin_to_head(pin);
        } else if previous_mode.is_some() {
            self.order = rebase_order(&previous_keys, self.entries());
        }
        Some(fresh || !self.is_natural_order())
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
            // Out of random: back to the list the user sees.
            (NativePlaybackMode::Random, _) => {
                self.order = sanitize_order(&[], self.len());
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
            shuffle_pin: None,
        }
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
            ..manifest(revision, ids)
        }
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
        let outcome = store.set_owning_order(random_manifest(3, &mirrored), None);
        assert_eq!(outcome, Some(false));
        assert_eq!(keys(&store), permutation);
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
            store.set_owning_order(random_manifest(3, &["a", "b"]), None),
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
