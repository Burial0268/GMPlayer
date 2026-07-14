use crate::types::SongData;

#[derive(Debug, Clone)]
pub struct PlaybackQueue {
    songs: Vec<SongData>,
    /// Logical playlist index from the frontend. For native AutoMix the backend
    /// may only know the current and prepared next source, so this can be
    /// larger than `songs.len()` and is matched through `SongData::orig_order`.
    current_index: usize,
    /// `true` when `songs` is a bounded prefill window instead of a full
    /// playlist: `next()` stops at the last entry rather than wrapping, so a
    /// frozen frontend can never make the backend loop stale entries.
    windowed: bool,
}

impl PlaybackQueue {
    pub fn new() -> Self {
        Self {
            songs: Vec::new(),
            current_index: 0,
            windowed: false,
        }
    }

    pub fn set_playlist(&mut self, songs: Vec<SongData>, windowed: bool) {
        self.songs = songs;
        self.windowed = windowed;
        self.current_index = self.current_index.min(self.songs.len().saturating_sub(1));
    }

    pub fn playlist_cloned(&self) -> Vec<SongData> {
        self.songs.clone()
    }

    pub fn current_index(&self) -> usize {
        self.current_index
    }

    pub fn current_song(&self) -> Option<SongData> {
        self.song_for_index(self.current_index)
    }

    pub fn set_index(&mut self, index: usize) -> Option<SongData> {
        self.song_for_index(index)?;
        self.current_index = index;
        self.current_song()
    }

    /// Re-anchor `current_index` to the entry whose `SongData::get_id()`
    /// matches. Identity wins over positional index when the playlist is
    /// replaced mid-track (same principle as the frontend's `playingSongId`),
    /// which keeps window prefills from flipping `current_song` when the
    /// window contains an entry whose `orig_order` collides with the stale
    /// clamped index (e.g. end-of-list wrap `[cur@5, next@0]`).
    pub fn set_index_by_song_id(&mut self, song_id: &str) -> bool {
        let Some(song) = self.songs.iter().find(|song| song.get_id() == song_id) else {
            return false;
        };
        self.current_index = song.orig_order();
        true
    }

    pub fn next(&mut self) -> Option<SongData> {
        if self.songs.is_empty() {
            return None;
        }
        let pos = self.position_for_index(self.current_index).unwrap_or(0);
        if self.windowed && pos + 1 >= self.songs.len() {
            return None;
        }
        let next_pos = (pos + 1) % self.songs.len();
        self.current_index = self.songs[next_pos].orig_order();
        self.current_song()
    }

    pub fn prev(&mut self) -> Option<SongData> {
        if self.songs.is_empty() {
            return None;
        }
        let pos = self.position_for_index(self.current_index).unwrap_or(0);
        if self.windowed && pos == 0 {
            return None;
        }
        let prev_pos = pos.checked_sub(1).unwrap_or(self.songs.len() - 1);
        self.current_index = self.songs[prev_pos].orig_order();
        self.current_song()
    }

    pub fn replace_or_set_current(&mut self, index: usize, song: SongData) {
        if let Some(pos) = self.position_for_index(index) {
            self.songs[pos] = song;
            self.current_index = index;
            return;
        }

        if index == self.songs.len() {
            self.songs.push(song);
            self.current_index = index;
            return;
        }

        self.songs.push(song);
        self.current_index = index;
    }

    fn song_for_index(&self, index: usize) -> Option<SongData> {
        self.songs
            .iter()
            .find(|song| song.orig_order() == index)
            .or_else(|| self.songs.get(index))
            .cloned()
    }

    fn position_for_index(&self, index: usize) -> Option<usize> {
        self.songs
            .iter()
            .position(|song| song.orig_order() == index)
            .or_else(|| (index < self.songs.len()).then_some(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(path: &str, orig_order: usize) -> SongData {
        SongData::Local {
            file_path: path.to_string(),
            orig_order,
        }
    }

    fn queue_with(songs: Vec<SongData>, windowed: bool) -> PlaybackQueue {
        let mut queue = PlaybackQueue::new();
        queue.set_playlist(songs, windowed);
        queue
    }

    #[test]
    fn single_entry_wraps_when_not_windowed() {
        let mut queue = queue_with(vec![local("a", 0)], false);
        let song = queue.next().expect("wrap replays the only entry");
        assert_eq!(song.get_id(), "local:a");
        assert_eq!(queue.current_index(), 0);
    }

    #[test]
    fn windowed_single_entry_stops_instead_of_replaying() {
        let mut queue = queue_with(vec![local("a", 0)], true);
        assert!(queue.next().is_none());
        assert_eq!(queue.current_index(), 0);
    }

    #[test]
    fn windowed_advances_through_real_indices_then_stops() {
        let mut queue = queue_with(vec![local("cur", 7), local("n1", 8), local("n2", 9)], true);
        queue.set_index(7);

        let n1 = queue.next().expect("advance to first prefilled track");
        assert_eq!(n1.get_id(), "local:n1");
        assert_eq!(queue.current_index(), 8);

        let n2 = queue.next().expect("advance to second prefilled track");
        assert_eq!(n2.get_id(), "local:n2");
        assert_eq!(queue.current_index(), 9);

        assert!(queue.next().is_none(), "window exhausted must stop");
        assert_eq!(queue.current_index(), 9, "stopping must not move the index");
    }

    #[test]
    fn windowed_prev_stops_at_window_start() {
        let mut queue = queue_with(vec![local("cur", 3), local("n1", 4)], true);
        queue.set_index(3);
        assert!(queue.prev().is_none());
        assert_eq!(queue.current_index(), 3);
    }

    #[test]
    fn full_playlist_keeps_wrap_semantics() {
        let mut queue = queue_with(vec![local("a", 0), local("b", 1)], false);
        queue.set_index(1);
        let song = queue.next().expect("wrap to playlist head");
        assert_eq!(song.get_id(), "local:a");
        assert_eq!(queue.current_index(), 0);
    }

    #[test]
    fn set_index_by_song_id_reanchors_to_orig_order() {
        let mut queue = queue_with(vec![local("cur", 5), local("next", 0)], true);
        assert!(queue.set_index_by_song_id("local:cur"));
        assert_eq!(queue.current_index(), 5);
        assert_eq!(
            queue.current_song().expect("current resolves").get_id(),
            "local:cur"
        );

        let next = queue.next().expect("wrap-indexed next entry is reachable");
        assert_eq!(next.get_id(), "local:next");
        assert_eq!(queue.current_index(), 0);
    }

    #[test]
    fn set_index_by_song_id_rejects_unknown_ids() {
        let mut queue = queue_with(vec![local("cur", 5)], true);
        queue.set_index(5);
        assert!(!queue.set_index_by_song_id("local:other"));
        assert_eq!(queue.current_index(), 5);
    }

    #[test]
    fn set_playlist_clamps_when_identity_is_gone() {
        let mut queue = queue_with(vec![local("a", 0), local("b", 1), local("c", 2)], false);
        queue.set_index(2);
        queue.set_playlist(vec![local("z", 0)], false);
        assert_eq!(queue.current_index(), 0);
        assert_eq!(
            queue.current_song().expect("clamped current").get_id(),
            "local:z"
        );
    }
}
