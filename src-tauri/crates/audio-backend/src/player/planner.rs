//! Playback planner: owns the cursor, traversal order, failure policy and the
//! decision of "what plays next" — independent of how a track is resolved to a
//! playable source.
//!
//! The planner walks `ManifestStore::order()`, so random mode is a full
//! permutation traversal rather than a per-hop dice roll: every track plays
//! once per pass, and wrapping reshuffles deterministically. This is what lets
//! random mode advance indefinitely with the JS runtime frozen, with no
//! prefill depth limit.
//!
//! All state here is plain data on the player's control path — no allocation
//! or blocking work happens in an audio callback.

use crate::types::{NativePlaybackMode, TrackIdentity};

use super::manifest::ManifestStore;

/// How many consecutive resolve/playback failures to tolerate within one
/// manifest revision before declaring the planner exhausted. Bounds the
/// "every track is broken / device is offline" case so it cannot become a
/// request storm.
const MAX_CONSECUTIVE_FAILURES: usize = 8;

/// How many played tracks to remember for "previous".
///
/// Bounded because this is per-session state on the player's control path and
/// nobody walks back further than a handful; the cap is generous enough that
/// it never truncates a real session's worth of back-presses.
const MAX_HISTORY: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTrack {
    pub identity: TrackIdentity,
    pub playlist_index: usize,
    /// Position within `ManifestStore::entries()`.
    pub position: usize,
}

#[derive(Debug, Default)]
pub struct Planner {
    /// Current position within `entries`, or `None` before anchoring.
    cursor_position: Option<usize>,
    /// Identity of the cursor, mirrored so history can be recorded without a
    /// manifest to look the position up in — and so a manifest replacement
    /// that moves entries around cannot silently repoint it.
    cursor_key: Option<String>,
    /// Identity keys of tracks that actually played, oldest first.
    ///
    /// "Previous" means the track you just heard. That is only the same thing
    /// as "the entry before this one in the list" in normal mode: random mode
    /// traverses a permutation, and the manifest pins the current track to
    /// slot 0 on every publish, so walking the order backwards there lands on
    /// an unrelated track. Keyed by identity rather than position because
    /// positions are exactly what list edits, reshuffles and manifest
    /// replacement invalidate.
    history: Vec<String>,
    /// Memoized successor of `cursor_position`.
    ///
    /// Deciding the next track is NOT idempotent in random mode: crossing the
    /// pass boundary reshuffles the traversal order. Prefetch and the actual
    /// advance both need "what plays next", so recomputing per call would
    /// reshuffle twice — the prefetched source and the played track would
    /// diverge, skipping some tracks and repeating others. Compute once, reuse
    /// until the cursor actually moves.
    pending_next: Option<PlannedTrack>,
    /// Positions that failed within the current revision; skipped when
    /// choosing a successor so one dead track cannot trap the traversal.
    failed: Vec<usize>,
    consecutive_failures: usize,
    exhausted: bool,
    enabled: bool,
}

impl Planner {
    pub fn new() -> Self {
        Self {
            cursor_position: None,
            cursor_key: None,
            history: Vec::new(),
            pending_next: None,
            failed: Vec::new(),
            consecutive_failures: 0,
            exhausted: false,
            enabled: true,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    pub fn failure_count(&self) -> usize {
        self.failed.len()
    }

    pub fn cursor_position(&self) -> Option<usize> {
        self.cursor_position
    }

    pub fn cursor<'a>(&self, manifest: &'a ManifestStore) -> Option<PlannedTrack> {
        let position = self.cursor_position?;
        let entry = manifest.entry_at(position)?;
        Some(PlannedTrack {
            identity: entry.identity.clone(),
            playlist_index: entry.playlist_index,
            position,
        })
    }

    /// Reset per-revision state. Called whenever a new manifest is accepted:
    /// failures from a previous revision must not suppress tracks in the new
    /// one, and an exhausted planner must come back to life.
    ///
    /// History deliberately survives: it is keyed by identity precisely so a
    /// republish — which happens after *every* track change — cannot erase
    /// what the user just listened to.
    ///
    /// `playing_key` is the identity of the track that is actually loaded, and
    /// it outranks the manifest's declared cursor. The declared cursor is
    /// computed from a frontend store snapshot, and on Android that snapshot is
    /// routinely *older* than the backend's own advances: the WebView is frozen
    /// while the planner keeps moving, then thaws and publishes a cursor naming
    /// a track that finished several hops ago. Planning from it replays
    /// everything in between. The declared cursor is authoritative only when
    /// nothing is loaded to contradict it — which is exactly what
    /// `invalidate_planner_anchor` arranges after a frontend-driven load.
    pub fn reset_for_new_manifest(&mut self, manifest: &ManifestStore, playing_key: Option<&str>) {
        self.failed.clear();
        self.consecutive_failures = 0;
        self.exhausted = false;
        self.pending_next = None;

        let resolve = |key: String| manifest.position_of_key(&key).map(|position| (key, position));
        let anchor = playing_key
            .map(str::to_string)
            .and_then(resolve)
            .or_else(|| manifest.declared_cursor_key().and_then(resolve));

        // Landing on a different track means playback moved on without the
        // planner (manual selection). That track did play, so it belongs in
        // history.
        if anchor.as_ref().map(|(key, _)| key.as_str()) != self.cursor_key.as_deref() {
            if let Some(previous) = self.cursor_key.take() {
                self.push_history(previous);
            }
        }
        match anchor {
            Some((key, position)) => {
                self.cursor_position = Some(position);
                self.cursor_key = Some(key);
            }
            None => {
                self.cursor_position = None;
                self.cursor_key = None;
            }
        }
    }

    /// Re-anchor the cursor onto a known identity (adoption after wake, or the
    /// frontend explicitly starting a track). Returns `false` when the identity
    /// is not in the current manifest.
    pub fn anchor_to_key(&mut self, manifest: &ManifestStore, key: &str) -> bool {
        let Some(position) = manifest.position_of_key(key) else {
            return false;
        };
        if self.cursor_key.as_deref() != Some(key) {
            if let Some(previous) = self.cursor_key.take() {
                self.push_history(previous);
            }
        }
        self.cursor_position = Some(position);
        self.cursor_key = Some(key.to_string());
        self.pending_next = None;
        // A track that actually started playing clears the failure streak:
        // the traversal is demonstrably alive again.
        self.consecutive_failures = 0;
        self.exhausted = false;
        self.failed.retain(|failed| *failed != position);
        true
    }

    /// Decide the successor of the current cursor without moving it.
    ///
    /// Memoized: repeated calls return the same track until the cursor moves or
    /// the plan is invalidated. This matters because the random-mode wrap
    /// reshuffles the traversal order, so a recomputation would hand back a
    /// different track than the one already prefetched.
    ///
    /// `single` mode repeats the current position. Otherwise this walks the
    /// traversal order, skipping known-failed positions, and — for random mode
    /// on wrap — asks the manifest to reshuffle for the next pass.
    pub fn peek_next(&mut self, manifest: &mut ManifestStore) -> Option<PlannedTrack> {
        if !self.enabled || self.exhausted || manifest.is_empty() {
            return None;
        }
        if let Some(pending) = self.pending_next.clone() {
            // Guard against a manifest replacement that dropped the memoized
            // track: only reuse it while it still resolves to the same identity.
            if manifest
                .entry_at(pending.position)
                .is_some_and(|entry| entry.identity.key() == pending.identity.key())
            {
                return Some(pending);
            }
            self.pending_next = None;
        }

        let planned = self.compute_next(manifest);
        self.pending_next = planned.clone();
        planned
    }

    fn compute_next(&self, manifest: &mut ManifestStore) -> Option<PlannedTrack> {
        if manifest.mode() == NativePlaybackMode::Single {
            let position = self.cursor_position?;
            return self.planned_at(manifest, position);
        }

        let len = manifest.len();
        let current_slot = self
            .cursor_position
            .and_then(|position| manifest.slot_of_position(position));

        // Walk forward through the traversal order. `len` steps is a full pass;
        // every candidate is either playable or explicitly known-failed.
        let start = current_slot.map(|slot| slot + 1).unwrap_or(0);
        for step in 0..len {
            let raw_slot = start + step;
            let wrapped = raw_slot >= len;
            if wrapped && !manifest.repeat_list() {
                return None;
            }

            // Crossing the pass boundary in random mode: reshuffle so the next
            // pass is a different permutation, pinning the just-finished track
            // away from the front to avoid an audible immediate repeat. The
            // memoization above guarantees this runs once per boundary.
            if wrapped && raw_slot == len && manifest.mode() == NativePlaybackMode::Random {
                manifest.reshuffle_random(self.cursor_position);
                // The order was rebuilt: restart the walk from the new slot 0
                // rather than indexing the old permutation.
                for slot in 0..len {
                    let Some(position) = manifest.position_at_slot(slot) else {
                        continue;
                    };
                    if self.failed.contains(&position) {
                        continue;
                    }
                    return self.planned_at(manifest, position);
                }
                return None;
            }

            let slot = raw_slot % len;
            let Some(position) = manifest.position_at_slot(slot) else {
                continue;
            };
            if self.failed.contains(&position) {
                continue;
            }
            return self.planned_at(manifest, position);
        }

        None
    }

    /// Decide what "previous" means right now, without moving the cursor.
    ///
    /// History first, list order only as a cold-start fallback. Walking the
    /// traversal order backwards looks equivalent and is not: the manifest
    /// pins the playing track to slot 0 of the random permutation on every
    /// publish, and a publish follows every track change, so a backwards walk
    /// in random mode wraps to the tail of the permutation — an unrelated
    /// track. Worse, it silently *loses* the track in between (a list of
    /// A,B,C sitting on B would go back to A and then never see B again),
    /// which is the bug this replaced.
    ///
    /// Deliberately does not touch `pending_next`: nothing prefetches
    /// backwards, so a "previous" press must not throw away the successor the
    /// prefetcher already warmed.
    pub fn peek_prev(&self, manifest: &ManifestStore) -> Option<PlannedTrack> {
        if !self.enabled || self.exhausted || manifest.is_empty() {
            return None;
        }

        // What actually played, newest first. Entries that the list no longer
        // contains simply drop out — identity keys cannot silently repoint at
        // a different song the way a stored position would.
        for key in self.history.iter().rev() {
            if Some(key) == self.cursor_key.as_ref() {
                continue;
            }
            let Some(position) = manifest.position_of_key(key) else {
                continue;
            };
            if self.failed.contains(&position) {
                continue;
            }
            return self.planned_at(manifest, position);
        }

        // Nothing has played yet this session (cold start, or playback adopted
        // from a previous WebView). Falling back to the list order is only
        // defensible when the list order is what the user is hearing.
        if manifest.mode() == NativePlaybackMode::Random {
            return None;
        }

        let len = manifest.len();
        let start = self
            .cursor_position
            .and_then(|position| manifest.slot_of_position(position))?;

        for step in 1..len {
            // Stepping past slot 0 is the backward wrap, gated by the same
            // repeat-list rule as the forward one.
            if step > start && !manifest.repeat_list() {
                return None;
            }
            let slot = (start + len - step) % len;
            let Some(position) = manifest.position_at_slot(slot) else {
                continue;
            };
            if self.failed.contains(&position) {
                continue;
            }
            return self.planned_at(manifest, position);
        }

        None
    }

    /// Move the cursor onto `track`. Call once the backend has committed to
    /// playing it.
    pub fn commit(&mut self, track: &PlannedTrack) {
        // The track being left is now "the previous one".
        if let Some(previous) = self.cursor_key.take() {
            if previous != track.identity.key() {
                self.push_history(previous);
            }
        }
        self.set_cursor(track);
    }

    /// Commit a *backwards* hop.
    ///
    /// Consumes history instead of extending it. Recording the outgoing track
    /// here would make two presses of "previous" ping-pong between the same
    /// two songs instead of walking back.
    pub fn commit_back(&mut self, track: &PlannedTrack) {
        let key = track.identity.key();
        if let Some(at) = self.history.iter().rposition(|entry| *entry == key) {
            self.history.truncate(at);
        }
        self.set_cursor(track);
    }

    fn set_cursor(&mut self, track: &PlannedTrack) {
        self.cursor_position = Some(track.position);
        self.cursor_key = Some(track.identity.key());
        self.pending_next = None;
    }

    fn push_history(&mut self, key: String) {
        if self.history.last() == Some(&key) {
            return;
        }
        self.history.push(key);
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// Drop the memoized plan without moving the cursor. Use when the inputs to
    /// the decision changed (manifest replaced, failure recorded, gating).
    pub fn invalidate_plan(&mut self) {
        self.pending_next = None;
    }

    /// Record that `position` could not be resolved or played. Returns `true`
    /// when the planner has just become exhausted.
    ///
    /// `permanent` distinguishes "this track is gone" (taken down, region
    /// locked, missing file) from a transient network failure. Transient
    /// failures count toward the streak but do not blacklist the position, so a
    /// Wi-Fi→cellular handover cannot permanently remove a track — or, on a
    /// one-track playlist, stop playback forever.
    pub fn mark_failed(
        &mut self,
        position: usize,
        manifest: &ManifestStore,
        permanent: bool,
    ) -> bool {
        if permanent && !self.failed.contains(&position) {
            self.failed.push(position);
        }
        self.consecutive_failures += 1;
        self.pending_next = None;

        // The cap is about stopping request storms, so it is a fixed number of
        // consecutive failures — NOT scaled to list length. Scaling it made a
        // 1-track playlist exhaust after a single failed resolve.
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
            || (!manifest.is_empty() && self.failed.len() >= manifest.len())
        {
            self.exhausted = true;
            return true;
        }
        false
    }

    /// Clear the failure streak after a track successfully starts playing.
    pub fn mark_started(&mut self, position: usize) {
        self.consecutive_failures = 0;
        self.failed.retain(|failed| *failed != position);
    }

    fn planned_at(&self, manifest: &ManifestStore, position: usize) -> Option<PlannedTrack> {
        let entry = manifest.entry_at(position)?;
        Some(PlannedTrack {
            identity: entry.identity.clone(),
            playlist_index: entry.playlist_index,
            position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NativeManifestEntry, NativePlaybackManifest};
    use std::collections::HashSet;

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

    fn store(ids: &[&str], mode: NativePlaybackMode, repeat_list: bool) -> ManifestStore {
        let mut store = ManifestStore::new();
        store.set(NativePlaybackManifest {
            schema_version: 1,
            revision: 1,
            entries: ids.iter().enumerate().map(|(i, id)| entry(id, i)).collect(),
            order: Vec::new(),
            cursor_identity: None,
            cursor_index: 0,
            mode,
            repeat_list,
            random_seed: Some(1234),
        });
        store
    }

    fn planner_at(store: &ManifestStore, key: &str) -> Planner {
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(store, None);
        assert!(planner.anchor_to_key(store, key));
        planner
    }

    fn cursor_key(planner: &Planner, store: &ManifestStore) -> Option<String> {
        planner.cursor(store).map(|track| track.identity.key())
    }

    /// A manifest that names its cursor, the way the frontend publishes one.
    fn store_at(
        ids: &[&str],
        mode: NativePlaybackMode,
        repeat_list: bool,
        cursor: &str,
        revision: u64,
    ) -> ManifestStore {
        let mut store = ManifestStore::new();
        store.set(NativePlaybackManifest {
            schema_version: 1,
            revision,
            entries: ids.iter().enumerate().map(|(i, id)| entry(id, i)).collect(),
            order: Vec::new(),
            cursor_identity: Some(TrackIdentity::Netease {
                id: cursor.to_string(),
            }),
            cursor_index: 0,
            mode,
            repeat_list,
            random_seed: Some(1234),
        });
        store
    }

    #[test]
    fn normal_mode_advances_in_order() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        let next = planner.peek_next(&mut store).expect("advance");
        assert_eq!(next.identity.key(), "netease:b");
        assert_eq!(next.playlist_index, 1);
    }

    #[test]
    fn normal_mode_wraps_only_when_repeat_list() {
        let mut wrapping = store(&["a", "b"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&wrapping, "netease:b");
        assert_eq!(
            planner.peek_next(&mut wrapping).map(|t| t.identity.key()),
            Some("netease:a".to_string())
        );

        let mut stopping = store(&["a", "b"], NativePlaybackMode::Normal, false);
        let mut planner = planner_at(&stopping, "netease:b");
        assert!(
            planner.peek_next(&mut stopping).is_none(),
            "end of list without repeat must stop"
        );
    }

    #[test]
    fn single_mode_repeats_current() {
        let mut store = store(&["a", "b"], NativePlaybackMode::Single, true);
        let mut planner = planner_at(&store, "netease:b");
        assert_eq!(
            planner.peek_next(&mut store).map(|t| t.identity.key()),
            Some("netease:b".to_string())
        );
    }

    /// A planner that has played nothing yet: anchored only by the manifest's
    /// declared cursor, the way a cold start or a WebView reload leaves it.
    fn cold_planner(store: &ManifestStore) -> Planner {
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(store, None);
        planner
    }

    #[test]
    fn prev_falls_back_to_list_order_with_no_history() {
        let store = store_at(&["a", "b", "c"], NativePlaybackMode::Normal, true, "c", 1);
        let planner = cold_planner(&store);

        let prev = planner.peek_prev(&store).expect("go back");
        assert_eq!(prev.identity.key(), "netease:b");
        assert_eq!(prev.playlist_index, 1);
    }

    /// The reported bug, end to end: A,B,C sitting on B. Previous must land on
    /// A and must not consume B — pressing next has to bring B back.
    #[test]
    fn prev_then_next_returns_to_the_track_it_left() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        // Play through to B the way the backend does.
        let b = planner.peek_next(&mut store).expect("advance to b");
        assert_eq!(b.identity.key(), "netease:b");
        planner.commit(&b);

        let back = planner.peek_prev(&store).expect("go back");
        assert_eq!(back.identity.key(), "netease:a", "previous must be A");
        planner.commit_back(&back);

        let forward = planner.peek_next(&mut store).expect("advance again");
        assert_eq!(
            forward.identity.key(),
            "netease:b",
            "B must still be reachable — going back must not drop it"
        );
    }

    /// Two presses walk two tracks back, rather than ping-ponging between the
    /// last two. This is what `commit_back` consuming history buys.
    #[test]
    fn prev_twice_walks_two_tracks_back() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        for expected in ["netease:b", "netease:c"] {
            let hop = planner.peek_next(&mut store).expect("advance");
            assert_eq!(hop.identity.key(), expected);
            planner.commit(&hop);
        }

        let first = planner.peek_prev(&store).expect("first back");
        assert_eq!(first.identity.key(), "netease:b");
        planner.commit_back(&first);

        let second = planner.peek_prev(&store).expect("second back");
        assert_eq!(
            second.identity.key(),
            "netease:a",
            "the second press must keep walking back, not return to C"
        );
    }

    /// Random mode is where a positional walk fails hardest: the manifest pins
    /// the playing track to slot 0 on every publish, so "the slot before this
    /// one" is the tail of the permutation — an unrelated song.
    #[test]
    fn prev_uses_history_not_the_permutation_in_random_mode() {
        let ids: Vec<String> = (0..8).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut store = store(&refs, NativePlaybackMode::Random, true);
        let mut planner = planner_at(&store, "netease:0");

        let hop = planner.peek_next(&mut store).expect("advance");
        planner.commit(&hop);

        let back = planner.peek_prev(&store).expect("go back");
        assert_eq!(
            back.identity.key(),
            "netease:0",
            "previous must be the track that actually played"
        );
    }

    /// With nothing played yet there is no honest answer in random mode, and
    /// inventing one by walking the permutation is what produced "prev plays a
    /// song I have never heard".
    #[test]
    fn prev_declines_on_a_cold_random_start() {
        let ids: Vec<String> = (0..8).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let store = store_at(&refs, NativePlaybackMode::Random, true, "3", 1);
        let planner = cold_planner(&store);

        assert!(planner.peek_prev(&store).is_none());
    }

    /// History is identity-keyed so the thing that invalidates positions —
    /// a republished, reordered manifest — cannot repoint it at another song.
    /// A manifest publish follows every track change, so this is the common
    /// path, not an edge case.
    #[test]
    fn history_survives_a_reordered_manifest() {
        let mut first = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&first, "netease:a");
        let hop = planner.peek_next(&mut first).expect("advance to b");
        planner.commit(&hop);

        // The user drags the list around; the frontend republishes with the
        // same cursor. A stored *position* would now name a different song.
        let reordered = store_at(&["c", "b", "a"], NativePlaybackMode::Normal, true, "b", 2);
        planner.reset_for_new_manifest(&reordered, None);

        let back = planner.peek_prev(&reordered).expect("go back");
        assert_eq!(back.identity.key(), "netease:a");
        assert_eq!(back.position, 2, "position follows the new list, not the old");
    }

    /// A track the user removed while it sat in history must be skipped, not
    /// resurrected or turned into whatever now occupies its old slot.
    #[test]
    fn prev_skips_history_entries_that_left_the_list() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");
        for _ in 0..2 {
            let hop = planner.peek_next(&mut store).expect("advance");
            planner.commit(&hop);
        }

        // "b" is deleted from the playlist; "a" and "c" remain.
        let shrunk = store_at(&["a", "c"], NativePlaybackMode::Normal, true, "c", 2);
        planner.reset_for_new_manifest(&shrunk, None);

        let back = planner.peek_prev(&shrunk).expect("go back");
        assert_eq!(back.identity.key(), "netease:a");
    }

    /// The frontend picking a song directly still counts as "played", so the
    /// next previous press goes back to it rather than to whatever the planner
    /// last advanced through on its own.
    #[test]
    fn a_manual_selection_lands_in_history() {
        let store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:c");

        assert!(planner.anchor_to_key(&store, "netease:a"));
        let back = planner.peek_prev(&store).expect("go back");
        assert_eq!(back.identity.key(), "netease:c");
    }

    /// The Android lock-screen case: the WebView is frozen while the planner
    /// hops, then thaws and republishes with a cursor several tracks stale.
    /// What is loaded wins, or the planner replays everything in between —
    /// and the media session resolves its metadata off the stale row.
    #[test]
    fn a_stale_declared_cursor_loses_to_the_loaded_track() {
        let ids = ["a", "b", "c", "d"];
        let mut store = store(&ids, NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");
        for _ in 0..2 {
            let hop = planner.peek_next(&mut store).expect("advance");
            planner.commit(&hop);
        }
        assert_eq!(cursor_key(&planner, &store).as_deref(), Some("netease:c"));

        // The thawed frontend still believes "a" is playing.
        let mut republished = store_at(&ids, NativePlaybackMode::Normal, true, "a", 2);
        planner.reset_for_new_manifest(&republished, Some("netease:c"));

        assert_eq!(
            cursor_key(&planner, &republished).as_deref(),
            Some("netease:c"),
            "the loaded track keeps the cursor"
        );
        assert_eq!(
            planner.peek_next(&mut republished).map(|t| t.identity.key()).as_deref(),
            Some("netease:d"),
            "and the next hop moves forward, not back to b"
        );
    }

    /// With nothing loaded to contradict it the declared cursor is the only
    /// answer there is — this is the handshake after a frontend-driven load,
    /// which clears the backend's anchor precisely so this can happen.
    #[test]
    fn the_declared_cursor_wins_when_nothing_is_loaded() {
        let store = store_at(&["a", "b", "c"], NativePlaybackMode::Normal, true, "b", 1);
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(&store, None);
        assert_eq!(cursor_key(&planner, &store).as_deref(), Some("netease:b"));
    }

    /// A loaded track that is no longer in the list cannot anchor anything, so
    /// the declared cursor takes over rather than leaving the planner blind.
    #[test]
    fn a_loaded_track_outside_the_manifest_falls_back_to_the_cursor() {
        let store = store_at(&["a", "b", "c"], NativePlaybackMode::Normal, true, "c", 1);
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(&store, Some("netease:zzz"));
        assert_eq!(cursor_key(&planner, &store).as_deref(), Some("netease:c"));
    }

    #[test]
    fn prev_wraps_only_when_repeat_list() {
        let wrapping = store(&["a", "b"], NativePlaybackMode::Normal, true);
        let planner = planner_at(&wrapping, "netease:a");
        assert_eq!(
            planner.peek_prev(&wrapping).map(|t| t.identity.key()),
            Some("netease:b".to_string()),
            "backward wrap follows the same repeat rule as the forward one"
        );

        let stopping = store(&["a", "b"], NativePlaybackMode::Normal, false);
        let planner = planner_at(&stopping, "netease:a");
        assert!(planner.peek_prev(&stopping).is_none());
    }

    /// Repeat-one is about what happens when a track *ends*. The previous
    /// button must still move, or it looks broken in single mode.
    #[test]
    fn prev_moves_even_in_single_mode() {
        let store = store(&["a", "b"], NativePlaybackMode::Single, true);
        let planner = planner_at(&store, "netease:b");
        assert_eq!(
            planner.peek_prev(&store).map(|t| t.identity.key()),
            Some("netease:a".to_string())
        );
    }

    #[test]
    fn prev_skips_failed_positions() {
        let store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:c");

        planner.mark_failed(1, &store, true);
        assert_eq!(
            planner.peek_prev(&store).map(|t| t.identity.key()),
            Some("netease:a".to_string())
        );
    }

    /// The prefetcher warms `pending_next` for the forward hop. Pressing
    /// previous must not invalidate it — recomputing in random mode would
    /// hand back a different track than the one already downloaded.
    #[test]
    fn prev_leaves_the_memoized_successor_alone() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Random, true);
        let mut planner = planner_at(&store, "netease:b");

        let warmed = planner.peek_next(&mut store).expect("warm the successor");
        let _ = planner.peek_prev(&store);
        assert_eq!(
            planner.peek_next(&mut store),
            Some(warmed),
            "peek_prev must not disturb the prefetched plan"
        );
    }

    #[test]
    fn failed_positions_are_skipped() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        planner.mark_failed(1, &store, true);
        assert_eq!(
            planner.peek_next(&mut store).map(|t| t.identity.key()),
            Some("netease:c".to_string()),
            "the broken track must be stepped over"
        );
    }

    #[test]
    fn planner_exhausts_when_every_track_fails() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        assert!(!planner.mark_failed(0, &store, true));
        assert!(!planner.mark_failed(1, &store, true));
        assert!(
            planner.mark_failed(2, &store, true),
            "all tracks failed must exhaust"
        );
        assert!(planner.is_exhausted());
        assert!(
            planner.peek_next(&mut store).is_none(),
            "an exhausted planner must stop, not loop"
        );
    }

    #[test]
    fn consecutive_failure_cap_stops_request_storms() {
        let ids: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let store = store(&refs, NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:0");

        let mut exhausted_at = None;
        for position in 0..50 {
            if planner.mark_failed(position, &store, true) {
                exhausted_at = Some(position);
                break;
            }
        }
        assert_eq!(
            exhausted_at,
            Some(MAX_CONSECUTIVE_FAILURES - 1),
            "must stop after the consecutive cap, not after all 50"
        );
    }

    #[test]
    fn a_successful_start_clears_the_failure_streak() {
        let store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        planner.mark_failed(0, &store, true);
        planner.mark_failed(1, &store, true);
        planner.mark_started(2);

        assert_eq!(planner.failure_count(), 2, "history is kept for skipping");
        // Streak reset means the next isolated failure cannot immediately trip
        // the cap.
        assert!(!planner.mark_failed(0, &store, true));
        assert!(!planner.is_exhausted());
    }

    #[test]
    fn new_manifest_revives_an_exhausted_planner() {
        let old = store(&["a"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&old, "netease:a");
        planner.mark_failed(0, &old, true);
        assert!(planner.is_exhausted());

        let fresh = store(&["a", "b"], NativePlaybackMode::Normal, true);
        planner.reset_for_new_manifest(&fresh, None);
        assert!(!planner.is_exhausted());
        assert_eq!(planner.failure_count(), 0);
    }

    #[test]
    fn disabled_planner_never_advances() {
        let mut store = store(&["a", "b"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");
        planner.set_enabled(false);
        assert!(
            planner.peek_next(&mut store).is_none(),
            "personal FM / listen-together gate must hold"
        );
    }

    /// The headline property: random mode must traverse the whole list without
    /// repeats and without any prefill depth limit, driven only by the backend.
    #[test]
    fn random_mode_covers_every_track_once_per_pass() {
        let ids: Vec<String> = (0..25).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut store = store(&refs, NativePlaybackMode::Random, true);
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(&store, None);
        planner.anchor_to_key(&store, "netease:0");

        let mut seen = HashSet::new();
        seen.insert(planner.cursor_position().unwrap());

        // One full pass minus the anchored track.
        for _ in 0..24 {
            let next = planner.peek_next(&mut store).expect("random must advance");
            assert!(
                seen.insert(next.position),
                "random pass repeated position {} before covering the list",
                next.position
            );
            planner.commit(&next);
        }
        assert_eq!(seen.len(), 25, "every track must play once per pass");
    }

    #[test]
    fn random_mode_keeps_advancing_across_pass_boundaries() {
        let ids: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut store = store(&refs, NativePlaybackMode::Random, true);
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(&store, None);
        planner.anchor_to_key(&store, "netease:0");

        // Five full passes with no JS involvement whatsoever.
        for hop in 0..50 {
            let next = planner
                .peek_next(&mut store)
                .unwrap_or_else(|| panic!("random stalled at hop {hop}"));
            planner.commit(&next);
        }
    }

    #[test]
    fn random_wrap_does_not_immediately_replay_the_same_track() {
        let ids: Vec<String> = (0..6).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();

        for seed in 0..32u64 {
            let mut store = ManifestStore::new();
            store.set(NativePlaybackManifest {
                schema_version: 1,
                revision: 1,
                entries: refs
                    .iter()
                    .enumerate()
                    .map(|(i, id)| entry(id, i))
                    .collect(),
                order: Vec::new(),
                cursor_identity: None,
                cursor_index: 0,
                mode: NativePlaybackMode::Random,
                repeat_list: true,
                random_seed: Some(seed),
            });
            let mut planner = Planner::new();
            planner.reset_for_new_manifest(&store, None);
            planner.anchor_to_key(&store, "netease:0");

            let mut previous = planner.cursor_position().unwrap();
            for _ in 0..30 {
                let next = planner.peek_next(&mut store).expect("advance");
                assert_ne!(
                    next.position, previous,
                    "seed {seed}: back-to-back repeat of the same track"
                );
                planner.commit(&next);
                previous = next.position;
            }
        }
    }

    #[test]
    fn random_mode_without_repeat_stops_after_one_pass() {
        let ids: Vec<String> = (0..5).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut store = store(&refs, NativePlaybackMode::Random, false);
        let mut planner = Planner::new();
        planner.reset_for_new_manifest(&store, None);
        planner.anchor_to_key(&store, "netease:0");

        for _ in 0..4 {
            let next = planner.peek_next(&mut store).expect("advance within pass");
            planner.commit(&next);
        }
        assert!(
            planner.peek_next(&mut store).is_none(),
            "no-repeat random must stop at the end of the pass"
        );
    }

    #[test]
    fn anchoring_to_unknown_identity_fails_without_moving_cursor() {
        let store = store(&["a", "b"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:b");
        assert!(!planner.anchor_to_key(&store, "netease:zzz"));
        assert_eq!(planner.cursor_position(), Some(1));
    }

    #[test]
    fn empty_manifest_yields_no_plan() {
        let mut store = store(&[], NativePlaybackMode::Normal, true);
        let mut planner = Planner::new();
        assert!(planner.peek_next(&mut store).is_none());
    }

    // ── regressions for the review fixes ─────────────────────────────────

    #[test]
    fn peek_is_stable_across_repeated_calls_in_random_mode() {
        // Prefetch and the actual advance both ask "what's next". Before
        // memoization, the random-mode wrap reshuffled on every call, so the
        // prefetched track and the played track diverged.
        let ids: Vec<String> = (0..6).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut store = store(&refs, NativePlaybackMode::Random, true);
        // Park the cursor on the final slot so the next hop crosses the pass
        // boundary — the only place a reshuffle can happen.
        let last_position = store.position_at_slot(store.len() - 1).unwrap();
        let last_key = store.entry_at(last_position).unwrap().identity.key();
        let mut planner = planner_at(&store, &last_key);

        let first = planner.peek_next(&mut store).expect("a wrap must plan");
        for _ in 0..5 {
            assert_eq!(
                planner.peek_next(&mut store),
                Some(first.clone()),
                "peek must be idempotent until the cursor moves"
            );
        }
    }

    #[test]
    fn committing_a_track_replans_the_successor() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        let first = planner.peek_next(&mut store).expect("plan");
        assert_eq!(first.identity.key(), "netease:b");
        // `commit` is what moves the cursor (and drops the memo); `mark_started`
        // only clears failure state. `start_planned_track` calls them in that
        // order.
        planner.commit(&first);
        planner.mark_started(first.position);

        assert_eq!(
            planner.peek_next(&mut store).map(|t| t.identity.key()),
            Some("netease:c".to_string()),
            "the memo must be dropped once the cursor moves"
        );
    }

    #[test]
    fn transient_failures_do_not_blacklist_the_track() {
        let mut store = store(&["a", "b", "c"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        // A network blip on `b` must not remove it from the traversal, or one
        // offline moment would permanently shrink the user's playlist.
        assert!(!planner.mark_failed(1, &store, false));
        assert_eq!(
            planner.failure_count(),
            0,
            "transient failures are not sticky"
        );
        assert_eq!(
            planner.peek_next(&mut store).map(|t| t.identity.key()),
            Some("netease:b".to_string()),
            "a transient failure must remain retryable"
        );
    }

    #[test]
    fn transient_failures_still_bound_the_retry_storm() {
        let store = store(&["a", "b"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:a");

        let mut exhausted = false;
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            exhausted = planner.mark_failed(1, &store, false);
        }
        assert!(
            exhausted,
            "unbounded transient retries would become a request storm"
        );
    }

    #[test]
    fn short_playlist_survives_a_single_failure() {
        // The old cap was `min(MAX, len)`, so a 1-track list exhausted on the
        // first blip and background playback died permanently.
        let store = store(&["only"], NativePlaybackMode::Normal, true);
        let mut planner = planner_at(&store, "netease:only");
        assert!(
            !planner.mark_failed(0, &store, false),
            "one transient failure must not end a single-track list"
        );
        assert!(!planner.is_exhausted());
    }
}
