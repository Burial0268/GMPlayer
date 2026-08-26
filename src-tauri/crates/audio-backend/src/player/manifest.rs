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
//! Traversal order is explicit (`order`), not derived. For random mode the
//! frontend ships a full shuffled permutation so both sides walk the list
//! identically without duplicating a PRNG. `random_seed` lets the backend
//! generate the *next* pass deterministically when it wraps while JS is
//! frozen — that is what removes the old depth-1 random limitation.

use crate::types::{NativeManifestEntry, NativePlaybackManifest, NativePlaybackMode};

#[derive(Debug, Default)]
pub struct ManifestStore {
    manifest: Option<NativePlaybackManifest>,
    /// Traversal order as positions into `entries`, always fully populated and
    /// validated (the wire `order` may be empty, partial or contain garbage).
    order: Vec<usize>,
    /// Which pass of a random traversal we are on. Bumped on every wrap so a
    /// backend-side reshuffle produces a different permutation each pass.
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
    /// The frontend normally owns the order and ships the whole permutation, but
    /// a mode change can arrive from a notification button while the WebView is
    /// dead — and the next hop has to honour it immediately, not whenever the
    /// page comes back. So the backend rebuilds its own order: a shuffle pinned
    /// so the playing track stays put (`keep_position`), and natural order on
    /// the way out of random. Returns `false` when nothing changed.
    ///
    /// The frontend republishes at a higher revision when it wakes, which
    /// replaces this order with its own — that is the convergence point, and it
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
                self.reshuffle_random(None);
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

    /// Reshuffle the traversal order for the next random pass. Deterministic in
    /// `(random_seed, revision, shuffle_pass)` so a resumed snapshot reproduces
    /// the same permutation instead of diverging from what the UI last saw.
    ///
    /// `keep_first` pins one position away from slot 0 — used to avoid
    /// replaying the track that just finished as the first track of the pass.
    pub fn reshuffle_random(&mut self, keep_first: Option<usize>) {
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

        // Avoid an immediate repeat across the pass boundary: if the finished
        // track landed at slot 0, swap it with a later slot.
        if let Some(keep) = keep_first {
            if order.first() == Some(&keep) && len > 1 {
                let target = 1 + (splitmix64(seed ^ 0xD6E8_FEB8_6659_FD93) as usize % (len - 1));
                order.swap(0, target);
            }
        }
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
            assert_eq!(sorted, vec![0, 1, 2, 3, 4], "seed {seed} is not a permutation");
        }
    }

    /// Leaving random restores the list the user is looking at, rather than
    /// stranding them in whatever permutation the shuffle produced.
    ///
    /// Both non-random modes get natural order, which is not an arbitrary choice:
    /// it is what the frontend's own `buildOrder` publishes for `normal` and
    /// `single` alike. The interim order this sets has to match the manifest that
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
            assert_eq!(store.order(), &[0, 1, 2, 3], "leaving random for {target:?}");
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

        store.reshuffle_random(None);
        let first = store.order().to_vec();
        store.reshuffle_random(None);
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
            store.reshuffle_random(None);
            store.order().to_vec()
        };
        assert_eq!(build(), build(), "resume must reproduce the same pass");
    }

    #[test]
    fn reshuffle_avoids_replaying_the_finished_track_first() {
        let ids: Vec<String> = (0..8).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();

        for seed in 0..64u64 {
            let mut store = ManifestStore::new();
            let mut m = manifest(1, &refs);
            m.random_seed = Some(seed);
            store.set(m);

            store.reshuffle_random(None);
            let natural_first = store.order()[0];

            let mut pinned = ManifestStore::new();
            let mut m2 = manifest(1, &refs);
            m2.random_seed = Some(seed);
            pinned.set(m2);
            pinned.reshuffle_random(Some(natural_first));

            assert_ne!(
                pinned.order()[0],
                natural_first,
                "seed {seed}: finished track must not lead the next pass"
            );
            let mut sorted = pinned.order().to_vec();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..8).collect::<Vec<_>>());
        }
    }

    #[test]
    fn single_entry_reshuffle_is_a_noop() {
        let mut store = ManifestStore::new();
        store.set(manifest(1, &["only"]));
        store.reshuffle_random(Some(0));
        assert_eq!(store.order(), &[0]);
    }
}
