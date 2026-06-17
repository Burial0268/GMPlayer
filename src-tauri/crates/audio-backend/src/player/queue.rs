use crate::types::SongData;

#[derive(Debug, Clone)]
pub struct PlaybackQueue {
    songs: Vec<SongData>,
    /// Logical playlist index from the frontend. For native AutoMix the backend
    /// may only know the current and prepared next source, so this can be
    /// larger than `songs.len()` and is matched through `SongData::orig_order`.
    current_index: usize,
}

impl PlaybackQueue {
    pub fn new() -> Self {
        Self {
            songs: Vec::new(),
            current_index: 0,
        }
    }

    pub fn set_playlist(&mut self, songs: Vec<SongData>) {
        self.songs = songs;
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

    pub fn next(&mut self) -> Option<SongData> {
        if self.songs.is_empty() {
            return None;
        }
        let pos = self.position_for_index(self.current_index).unwrap_or(0);
        let next_pos = (pos + 1) % self.songs.len();
        self.current_index = self.songs[next_pos].orig_order();
        self.current_song()
    }

    pub fn prev(&mut self) -> Option<SongData> {
        if self.songs.is_empty() {
            return None;
        }
        let pos = self.position_for_index(self.current_index).unwrap_or(0);
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
