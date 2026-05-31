use crate::types::{LoopMode, QueueItem, TrackSource};

#[derive(Debug, Clone)]
pub struct PlaybackQueue {
    items: Vec<QueueItem>,
    current_index: usize,
    next_id: u64,
    loop_mode: LoopMode,
}

impl PlaybackQueue {
    pub fn new() -> Self {
        PlaybackQueue {
            items: Vec::new(),
            current_index: 0,
            next_id: 1,
            loop_mode: LoopMode::Off,
        }
    }

    pub fn add(
        &mut self,
        source: TrackSource,
        title: Option<String>,
        artist: Option<String>,
        duration_secs: Option<f64>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(QueueItem {
            id,
            source,
            title,
            artist,
            duration_secs,
        });
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(pos) = self.items.iter().position(|item| item.id == id) {
            self.items.remove(pos);
            if pos < self.current_index && self.current_index > 0 {
                self.current_index -= 1;
            } else if pos == self.current_index {
                self.current_index = self.current_index.min(self.items.len().saturating_sub(1));
            }
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = 0;
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.items.get(self.current_index)
    }

    pub fn current_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.current_index)
        }
    }

    pub fn peek_next(&self) -> Option<&QueueItem> {
        let next_idx = self.compute_next_index()?;
        self.items.get(next_idx)
    }

    fn compute_next_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }

        let next = self.current_index + 1;
        if next < self.items.len() {
            Some(next)
        } else {
            match self.loop_mode {
                LoopMode::Off => None,
                LoopMode::Single => Some(self.current_index),
                LoopMode::All => {
                    if self.items.is_empty() {
                        None
                    } else {
                        Some(0)
                    }
                }
            }
        }
    }

    pub fn advance(&mut self) -> Option<&QueueItem> {
        let next_idx = self.compute_next_index()?;
        self.current_index = next_idx;
        self.items.get(next_idx)
    }

    pub fn set_index(&mut self, index: usize) -> Option<&QueueItem> {
        if index < self.items.len() {
            self.current_index = index;
            self.items.get(index)
        } else {
            None
        }
    }

    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }
}
