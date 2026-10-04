//! Bounded radio reservoir and refill policy. No I/O; time is supplied by the caller.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::types::{NativeFmSnapshot, NativeFmTrack, NativeManifestEntry, TrackIdentity};

pub(super) const TARGET: usize = 24;
const LOW_WATER: usize = 8;
const CAPACITY: usize = 32;
const RECENT_LIMIT: usize = 200;
const MAX_BATCHES: usize = 10;

pub(super) struct PersonalFm {
    pub id: u64,
    pub generation: u64,
    pub revision: u64,
    pub tracks: Vec<NativeFmTrack>,
    pub current: Option<TrackIdentity>,
    pub desired_playing: bool,
    pub waiting: bool,
    pub in_flight: bool,
    pub trash_in_flight: Option<String>,
    pub error: Option<String>,
    pub auth_failed: bool,
    pub deadline: Option<Instant>,
    pub source_deadline: Option<Instant>,
    source_backoff: usize,
    recent: VecDeque<String>,
    disliked: HashSet<String>,
    batches: usize,
    empty_batches: usize,
    backoff: usize,
}

impl PersonalFm {
    pub fn new(id: u64, seed: Option<serde_json::Value>, now: Instant) -> Self {
        let mut session = Self {
            id,
            generation: 0,
            revision: 1,
            tracks: Vec::new(),
            current: None,
            desired_playing: true,
            waiting: true,
            in_flight: false,
            trash_in_flight: None,
            error: None,
            auth_failed: false,
            deadline: Some(now),
            source_deadline: None,
            source_backoff: 0,
            recent: VecDeque::new(),
            disliked: HashSet::new(),
            batches: 0,
            empty_batches: 0,
            backoff: 0,
        };
        if let Some(seed) = seed {
            session.ingest(vec![seed]);
        }
        session
    }

    pub fn snapshot(&self) -> NativeFmSnapshot {
        NativeFmSnapshot {
            session_id: self.id,
            revision: self.revision,
            tracks: self.tracks.clone(),
            current_identity: self.current.clone(),
            desired_playing: self.desired_playing,
            refilling: self.in_flight,
            waiting: self.waiting,
            error: self.error.clone(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.tracks
            .len()
            .saturating_sub(usize::from(self.current.is_some()))
    }

    pub fn request_refill(&mut self, now: Instant) {
        if self.desired_playing
            && !self.auth_failed
            && self.remaining() <= LOW_WATER
            && self.deadline.is_none()
            && !self.in_flight
        {
            self.batches = 0;
            self.deadline = Some(now);
        }
    }

    pub fn begin_fetch(&mut self, now: Instant) -> Option<u64> {
        if !self.desired_playing
            || self.auth_failed
            || self.in_flight
            || self.remaining() >= TARGET
            || self.deadline.is_none_or(|at| at > now)
        {
            return None;
        }
        self.deadline = None;
        self.in_flight = true;
        self.batches += 1;
        self.revision += 1;
        Some(self.generation)
    }

    pub fn accepts(&self, session: u64, generation: u64) -> bool {
        self.id == session && self.generation == generation
    }

    pub fn ingest(&mut self, rows: Vec<serde_json::Value>) -> usize {
        let mut seen: HashSet<String> = self.tracks.iter().map(|t| t.identity.key()).collect();
        let mut added = 0;
        for row in rows {
            if self.remaining() >= CAPACITY {
                break;
            }
            let Some(track) = normalize(row) else {
                continue;
            };
            let key = track.identity.key();
            if self.disliked.contains(&key) || self.recent.contains(&key) || !seen.insert(key) {
                continue;
            }
            self.tracks.push(track);
            added += 1;
        }
        if added > 0 {
            self.revision += 1;
        }
        added
    }

    pub fn fetched(&mut self, added: usize, now: Instant) {
        self.in_flight = false;
        self.error = None;
        self.revision += 1;
        if added == 0 {
            self.empty_batches += 1;
        } else {
            self.empty_batches = 0;
            self.backoff = 0;
        }
        if self.remaining() >= TARGET {
            self.deadline = None;
            self.batches = 0;
        } else if self.empty_batches >= 2 || self.batches >= MAX_BATCHES {
            self.defer("FM returned no further recommendations".into(), false, now);
        } else {
            self.deadline = Some(now + Duration::from_secs(1));
        }
    }

    pub fn defer(&mut self, message: String, auth: bool, now: Instant) {
        const DELAYS: [u64; 7] = [5, 15, 30, 60, 120, 240, 300];
        self.in_flight = false;
        self.error = Some(message);
        self.auth_failed = auth;
        self.deadline = (!auth).then(|| now + Duration::from_secs(DELAYS[self.backoff]));
        self.backoff = (self.backoff + 1).min(DELAYS.len() - 1);
        self.batches = 0;
        self.empty_batches = 0;
        self.revision += 1;
    }

    pub fn set_playing(&mut self, playing: bool, now: Instant) {
        self.desired_playing = playing;
        self.revision += 1;
        if playing {
            self.request_refill(now);
        }
    }

    /// Called only after a successful load, before the next source is prefetched.
    pub fn started(&mut self, identity: &TrackIdentity, now: Instant) -> bool {
        let Some(index) = self.tracks.iter().position(|t| &t.identity == identity) else {
            return false;
        };
        for track in self.tracks.drain(..index) {
            self.recent.push_back(track.identity.key());
        }
        while self.recent.len() > RECENT_LIMIT {
            self.recent.pop_front();
        }
        self.current = Some(identity.clone());
        self.waiting = false;
        self.source_deadline = None;
        self.source_backoff = 0;
        self.error = None;
        self.revision += 1;
        self.request_refill(now);
        index > 0
    }

    pub fn dislike(&mut self, id: &str) {
        let key = format!("netease:{id}");
        self.disliked.insert(key.clone());
        // The server also filters trash; bound the local session guard.
        if self.disliked.len() > RECENT_LIMIT {
            if let Some(old) = self
                .disliked
                .iter()
                .find(|candidate| **candidate != key)
                .cloned()
            {
                self.disliked.remove(&old);
            }
        }
        // Keep the loaded row until the next load retires it; it anchors the planner.
        let current = self.current.as_ref().map(TrackIdentity::key);
        self.tracks
            .retain(|t| t.identity.key() != key || current.as_ref() == Some(&key));
        self.revision += 1;
    }

    pub fn reject(&mut self, identity: &TrackIdentity, now: Instant) {
        if self.current.as_ref() == Some(identity) {
            return;
        }
        self.tracks.retain(|track| &track.identity != identity);
        self.recent.push_back(identity.key());
        while self.recent.len() > RECENT_LIMIT {
            self.recent.pop_front();
        }
        self.revision += 1;
        self.request_refill(now);
    }

    pub fn defer_source(&mut self, message: String, auth: bool, now: Instant) {
        const DELAYS: [u64; 7] = [5, 15, 30, 60, 120, 240, 300];
        self.error = Some(message);
        self.auth_failed |= auth;
        self.source_deadline = Some(now + Duration::from_secs(DELAYS[self.source_backoff]));
        self.source_backoff = (self.source_backoff + 1).min(DELAYS.len() - 1);
        self.revision += 1;
    }

    pub fn account_changed(&mut self, now: Instant) {
        self.tracks
            .retain(|track| self.current.as_ref() == Some(&track.identity));
        self.recent.clear();
        self.disliked.clear();
        self.credentials_changed(now);
    }

    pub fn credentials_changed(&mut self, now: Instant) {
        self.generation += 1;
        self.in_flight = false;
        self.trash_in_flight = None;
        self.auth_failed = false;
        self.error = None;
        self.batches = 0;
        self.backoff = 0;
        self.source_backoff = 0;
        self.source_deadline = None;
        self.deadline = Some(now);
        self.revision += 1;
    }
}

fn text(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

pub(super) fn normalize(row: serde_json::Value) -> Option<NativeFmTrack> {
    let id = row.get("id")?;
    let id = id
        .as_u64()
        .filter(|n| *n > 0)
        .map(|n| n.to_string())
        .or_else(|| {
            id.as_str()
                .filter(|s| !s.is_empty() && *s != "0" && s.bytes().all(|b| b.is_ascii_digit()))
                .map(str::to_owned)
        })?;
    let mut song = serde_json::Map::new();
    // Only the fields the FM card, song actions and resolver consume cross the bridge.
    song.insert("id".into(), id.clone().into());
    for field in [
        "name", "artists", "album", "alias", "duration", "fee", "pc", "mvid",
    ] {
        if let Some(value) = row.get(field) {
            song.insert(field.into(), value.clone());
        }
    }
    Some(NativeFmTrack {
        identity: TrackIdentity::Netease { id },
        song: song.into(),
    })
}

pub(super) fn manifest_entry(track: &NativeFmTrack, index: usize) -> NativeManifestEntry {
    let song = &track.song;
    let artist = song["artists"].as_array().map(|artists| {
        artists
            .iter()
            .filter_map(|a| a["name"].as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    });
    NativeManifestEntry {
        identity: track.identity.clone(),
        playlist_index: index,
        title: text(&song["name"]),
        artist,
        album: text(&song["album"]["name"]),
        artwork_url: text(&song["album"]["picUrl"]).map(|s| s.replacen("http://", "https://", 1)),
        duration_ms: song["duration"].as_u64(),
        fee: song["fee"].as_i64(),
        has_pc: !song["pc"].is_null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rows(start: u64, end: u64) -> Vec<serde_json::Value> {
        (start..end)
            .map(|id| json!({"id": id, "name": "track"}))
            .collect()
    }

    #[test]
    fn reservoir_is_bounded_deduped_and_preserves_integer_identity() {
        let mut fm = PersonalFm::new(1, None, Instant::now());
        assert_eq!(fm.ingest(rows(1, 101)), CAPACITY);
        assert_eq!(fm.ingest(rows(1, 10)), 0);
        let track = normalize(json!({"id": 9007199254740993u64, "cookie": "secret"})).unwrap();
        assert_eq!(track.identity.key(), "netease:9007199254740993");
        assert!(track.song.get("cookie").is_none());
    }

    #[test]
    fn repeated_empty_batches_back_off_without_recursive_requests() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        assert_eq!(fm.begin_fetch(now), Some(0));
        assert_eq!(fm.begin_fetch(now), None);
        fm.fetched(0, now);
        assert_eq!(fm.begin_fetch(now), None);
        assert_eq!(fm.begin_fetch(now + Duration::from_secs(1)), Some(0));
        fm.fetched(0, now + Duration::from_secs(1));
        assert!(fm.error.is_some());
        assert_eq!(fm.begin_fetch(now + Duration::from_secs(2)), None);
    }

    #[test]
    fn paused_and_stale_sessions_cannot_fetch() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(9, None, now);
        fm.set_playing(false, now);
        assert_eq!(fm.begin_fetch(now), None);
        fm.credentials_changed(now);
        assert!(!fm.accepts(9, 0));
        assert!(!fm.accepts(8, 1));
        fm.set_playing(true, now);
        assert_eq!(fm.begin_fetch(now), Some(1));
    }

    #[test]
    fn high_water_stops_fetching_and_consumption_rearms_it() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        fm.ingest(rows(1, 27));
        fm.fetched(26, now);
        assert_eq!(fm.begin_fetch(now), None);
        fm.started(&TrackIdentity::Netease { id: "19".into() }, now);
        assert_eq!(fm.remaining(), 7);
        assert_eq!(fm.begin_fetch(now), Some(0));
        assert_eq!(fm.ingest(rows(1, 19)), 0);
    }

    #[test]
    fn pause_during_refill_keeps_rows_without_restarting_playback() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        fm.begin_fetch(now);
        fm.set_playing(false, now);
        let added = fm.ingest(rows(1, 4));
        fm.fetched(added, now);
        assert!(!fm.desired_playing);
        assert!(fm.waiting);
        assert_eq!(fm.begin_fetch(now + Duration::from_secs(60)), None);
        assert_eq!(fm.remaining(), 3);
    }

    #[test]
    fn auth_failure_requires_new_credentials_and_rejects_old_results() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(3, None, now);
        fm.begin_fetch(now);
        fm.defer("unauthorized".into(), true, now);
        assert_eq!(fm.begin_fetch(now + Duration::from_secs(3600)), None);
        fm.credentials_changed(now);
        assert!(!fm.accepts(3, 0));
        assert_eq!(fm.begin_fetch(now), Some(1));
    }

    #[test]
    fn account_switch_discards_other_accounts_recommendations() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        fm.ingest(rows(1, 6));
        fm.started(&TrackIdentity::Netease { id: "2".into() }, now);
        fm.dislike("3");
        fm.account_changed(now);
        assert_eq!(fm.tracks.len(), 1);
        assert_eq!(fm.tracks[0].identity.key(), "netease:2");
        assert_eq!(fm.ingest(rows(1, 6)), 4);
        assert!(!fm.accepts(1, 0));
    }

    #[test]
    fn unavailable_tracks_rearm_refill_and_cannot_immediately_reenter() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        fm.ingest(rows(1, 26));
        fm.fetched(25, now);
        for id in 1..20 {
            fm.reject(&TrackIdentity::Netease { id: id.to_string() }, now);
        }
        assert_eq!(fm.remaining(), 6);
        assert_eq!(fm.ingest(rows(1, 20)), 0);
        assert_eq!(fm.begin_fetch(now), Some(0));
    }

    #[test]
    fn source_retries_and_recommendation_retries_have_separate_deadlines() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        fm.defer_source("network".into(), false, now);
        assert_eq!(fm.source_deadline, Some(now + Duration::from_secs(5)));
        assert_eq!(fm.begin_fetch(now), Some(0));
        fm.defer("network".into(), false, now);
        assert_eq!(fm.begin_fetch(now + Duration::from_secs(4)), None);
        assert_eq!(fm.begin_fetch(now + Duration::from_secs(5)), Some(0));
    }

    #[test]
    fn recent_and_trash_filters_are_bounded() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        for id in 1..1000 {
            fm.dislike(&id.to_string());
            fm.reject(&TrackIdentity::Netease { id: id.to_string() }, now);
        }
        assert!(fm.recent.len() <= RECENT_LIMIT);
        assert!(fm.disliked.len() <= RECENT_LIMIT);
    }

    #[test]
    fn trash_cannot_reenter_after_compaction() {
        let now = Instant::now();
        let mut fm = PersonalFm::new(1, None, now);
        fm.ingest(rows(1, 4));
        fm.started(&TrackIdentity::Netease { id: "1".into() }, now);
        fm.dislike("1");
        fm.started(&TrackIdentity::Netease { id: "2".into() }, now);
        assert_eq!(fm.ingest(rows(1, 2)), 0);
        assert_eq!(fm.tracks[0].identity.key(), "netease:2");
    }
}
