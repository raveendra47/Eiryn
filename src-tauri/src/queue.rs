use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "all" => RepeatMode::All,
            "one" => RepeatMode::One,
            _ => RepeatMode::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackRef {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    pub duration: f64,
    pub file_path: String,
    pub cue_parent_id: Option<String>,
    pub cue_offset: f64,
    pub bpm: f64,
    pub energy: f64,
    #[serde(default)]
    pub last_played: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueState {
    pub original_list: Vec<TrackRef>,
    pub shuffled_list: Vec<TrackRef>,
    pub current_index: usize,
    pub repeat_mode: RepeatMode,
    pub shuffle: bool,
}

impl QueueState {
    pub fn new() -> Self {
        Self {
            original_list: Vec::new(),
            shuffled_list: Vec::new(),
            current_index: 0,
            repeat_mode: RepeatMode::Off,
            shuffle: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.original_list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.original_list.len()
    }

    pub fn active_list(&self) -> &[TrackRef] {
        if self.shuffle {
            &self.shuffled_list
        } else {
            &self.original_list
        }
    }

    pub fn set_tracks(&mut self, tracks: Vec<TrackRef>, start_index: usize) {
        self.original_list = tracks;
        self.current_index = start_index.min(self.original_list.len().saturating_sub(1));
        self.shuffled_list = self.original_list.clone();
        self.shuffle = false;
        self.repeat_mode = RepeatMode::Off;
    }

    pub fn current_track(&self) -> Option<&TrackRef> {
        self.active_list().get(self.current_index)
    }

    pub fn peek_next(&self) -> Option<&TrackRef> {
        let len = self.original_list.len();
        if len == 0 {
            return None;
        }
        if self.repeat_mode == RepeatMode::One {
            return self.active_list().get(self.current_index);
        }
        let next_idx = self.current_index.wrapping_add(1);
        if next_idx >= len {
            if self.repeat_mode == RepeatMode::All {
                self.active_list().get(0)
            } else {
                None
            }
        } else {
            self.active_list().get(next_idx)
        }
    }

    pub fn next_track(&mut self) -> Option<&TrackRef> {
        let len = self.original_list.len();
        if len == 0 {
            return None;
        }

        if self.repeat_mode == RepeatMode::One {
            return self.active_list().get(self.current_index);
        }

        self.current_index = self.current_index.wrapping_add(1);

        if self.current_index >= len {
            if self.repeat_mode == RepeatMode::All {
                self.current_index = 0;
            } else {
                self.current_index = len;
                return None;
            }
        }

        self.active_list().get(self.current_index)
    }

    pub fn next_track_auto(&mut self) -> Option<&TrackRef> {
        self.next_track()
    }

    pub fn next_track_manual(&mut self) -> Option<&TrackRef> {
        let len = self.original_list.len();
        if len == 0 {
            return None;
        }
        self.current_index = self.current_index.wrapping_add(1);
        if self.current_index >= len {
            if self.repeat_mode == RepeatMode::All {
                self.current_index = 0;
            } else {
                self.current_index = len;
                return None;
            }
        }
        self.active_list().get(self.current_index)
    }

    pub fn prev_track(&mut self) -> Option<&TrackRef> {
        let len = self.original_list.len();
        if len == 0 {
            return None;
        }

        if self.current_index > 0 {
            self.current_index -= 1;
        } else if self.repeat_mode == RepeatMode::All {
            self.current_index = len - 1;
        }

        self.active_list().get(self.current_index)
    }

    pub fn set_shuffle(&mut self, enabled: bool) {
        if enabled == self.shuffle {
            return;
        }
        self.shuffle = enabled;
        if enabled {
            self.rebuild_shuffle();
        }
    }

    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.repeat_mode = mode;
    }

    fn rebuild_shuffle(&mut self) {
        if self.original_list.is_empty() {
            self.shuffled_list.clear();
            self.current_index = 0;
            return;
        }

        let current_track = self.original_list.get(self.current_index).cloned();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let entries: Vec<crate::shuffle::Entry> = self.original_list.iter().map(|t| crate::shuffle::Entry {
            artist: &t.artist,
            album: &t.album,
            last_played: t.last_played,
        }).collect();

        let lead = current_track.as_ref().map(|t| (t.artist.as_str(), t.album.as_str()));
        let seed = crate::shuffle::clock_seed();
        let order = crate::shuffle::balanced(&entries, lead, now, seed);

        let mut shuffled = Vec::with_capacity(self.original_list.len());
        for idx in order {
            if let Some(t) = self.original_list.get(idx) {
                shuffled.push(t.clone());
            }
        }

        if let Some(ref cur) = current_track {
            if let Some(pos) = shuffled.iter().position(|t| t.id == cur.id) {
                shuffled.swap(0, pos);
                self.current_index = 0;
            }
        }

        self.shuffled_list = shuffled;
    }

    pub fn remove_track(&mut self, index: usize) {
        if index >= self.original_list.len() {
            return;
        }
        let removed = self.original_list.remove(index);
        if let Some(pos) = self.shuffled_list.iter().position(|t| t.id == removed.id) {
            self.shuffled_list.remove(pos);
        }
        if self.current_index >= self.original_list.len() && !self.original_list.is_empty() {
            self.current_index = self.original_list.len() - 1;
        }
    }

    pub fn insert_after_current(&mut self, tracks: Vec<TrackRef>) {
        let len = self.original_list.len();
        // Clamp so an empty or exhausted queue never panics on insert
        let pos = (self.current_index + 1).min(len);
        for (i, t) in tracks.into_iter().enumerate() {
            self.original_list.insert(pos + i, t);
        }
        if self.shuffle {
            self.rebuild_shuffle();
        }
    }

    pub fn append_tracks(&mut self, tracks: Vec<TrackRef>) {
        for t in tracks {
            self.original_list.push(t);
        }
        if self.shuffle {
            self.rebuild_shuffle();
        }
    }

    pub fn reorder(&mut self, new_order: Vec<TrackRef>) {
        let current_id = self.original_list.get(self.current_index).map(|t| t.id.clone());
        self.original_list = new_order;
        if let Some(ref id) = current_id {
            if let Some(pos) = self.original_list.iter().position(|t| t.id == *id) {
                self.current_index = pos;
            }
        }
        self.shuffled_list = self.original_list.clone();
    }
}
