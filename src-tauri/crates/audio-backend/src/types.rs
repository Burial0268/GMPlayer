use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU8;

// ── Playback state & configuration (kept from original) ──────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PlaybackState {
    Stopped = 0,
    Playing = 1,
    Paused = 2,
    Ended = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopMode {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "all")]
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossfadeCurve {
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "equal_power")]
    EqualPower,
    #[serde(rename = "s_curve")]
    SCurve,
}

impl PlaybackState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Stopped,
            1 => Self::Playing,
            2 => Self::Paused,
            3 => Self::Ended,
            _ => Self::Stopped,
        }
    }

    pub fn load(atomic: &AtomicU8) -> Self {
        Self::from_u8(atomic.load(std::sync::atomic::Ordering::SeqCst))
    }

    pub fn store(self, atomic: &AtomicU8) {
        atomic.store(self as u8, std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: f64,
    pub bitrate_bps: Option<u64>,
    pub total_frames: Option<u64>,
    pub container_format: String,
    pub metadata_tags: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectrumConfig {
    pub fft_size: usize,
    pub smoothing: f32,
    pub max_freq: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrackSource {
    File(String),
    Url(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: u64,
    pub source: TrackSource,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_secs: Option<f64>,
}

// ═══════════════════════════════════════════════════════════════════
// AMLL-style message/event system — IPC contract with frontend
// ═══════════════════════════════════════════════════════════════════

/// Messages sent from frontend → player (via a single Tauri command).
///
/// IMPORTANT: serde `rename_all = "camelCase"` at the enum level only
/// renames variant **tag** names.  Each variant with named fields MUST
/// also carry its own `#[serde(rename_all = "camelCase")]` for the fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AudioThreadMessage {
    #[serde(rename_all = "camelCase")]
    ResumeAudio,
    #[serde(rename_all = "camelCase")]
    PauseAudio,
    #[serde(rename_all = "camelCase")]
    ResumeOrPauseAudio,
    #[serde(rename_all = "camelCase")]
    SeekAudio { position: f64 },
    #[serde(rename_all = "camelCase")]
    JumpToSong { song_index: usize },
    /// Same as `JumpToSong` but pre-seeks to `position` seconds before the
    /// source starts emitting samples (uses `decoder::open_source_with_fft_at`
    /// internally). Used on app startup with `memoryLastPlaybackPosition`
    /// so the resumed track plays from the saved position without a
    /// follow-up `SeekAudio` command — which avoided a race where
    /// `SyncStatus` emitted from the seek's `finish_message` carried a
    /// stale `position=0` and overwrote the frontend's optimistic value.
    #[serde(rename_all = "camelCase")]
    JumpToSongAt { song_index: usize, position: f64 },
    #[serde(rename_all = "camelCase")]
    PrevSong,
    #[serde(rename_all = "camelCase")]
    NextSong,
    #[serde(rename_all = "camelCase")]
    NextSongGapless,
    #[serde(rename_all = "camelCase")]
    SetPlaylist { songs: Vec<SongData> },
    #[serde(rename_all = "camelCase")]
    SetVolume { volume: f64 },
    #[serde(rename_all = "camelCase")]
    SetVolumeRelative { volume: f64 },
    #[serde(rename_all = "camelCase")]
    SetAudioOutput { name: String },
    #[serde(rename_all = "camelCase")]
    SetFFT { enabled: bool },
    #[serde(rename_all = "camelCase")]
    SetFFTRange { from_freq: f32, to_freq: f32 },
    #[serde(rename_all = "camelCase")]
    SyncStatus,
    #[serde(rename_all = "camelCase")]
    Close,
    #[serde(rename_all = "camelCase")]
    SetMediaControlsEnabled { enabled: bool },
}

/// Events emitted from player → frontend (via Tauri event emit).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type", content = "data")]
pub enum AudioThreadEvent {
    #[serde(rename_all = "camelCase")]
    PlayPosition { position: f64 },
    #[serde(rename_all = "camelCase")]
    LoadProgress { position: f64 },
    #[serde(rename_all = "camelCase")]
    LoadAudio {
        music_id: String,
        music_info: DisplayAudioInfo,
        quality: AudioQuality,
        current_play_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    LoadingAudio {
        music_id: String,
        current_play_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    AudioPlayFinished { music_id: String },
    #[serde(rename_all = "camelCase")]
    SyncStatus {
        music_id: String,
        music_info: DisplayAudioInfo,
        is_playing: bool,
        duration: f64,
        position: f64,
        volume: f64,
        load_position: f64,
        playlist: Vec<SongData>,
        current_play_index: usize,
        playlist_inited: bool,
        quality: AudioQuality,
    },
    #[serde(rename_all = "camelCase")]
    PlayListChanged {
        playlist: Vec<SongData>,
        current_play_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    PlayStatus { is_playing: bool },
    #[serde(rename_all = "camelCase")]
    LoadError { error: String },
    #[serde(rename_all = "camelCase")]
    PlayError { error: String },
    #[serde(rename_all = "camelCase")]
    VolumeChanged { volume: f64 },
    // FFTData → "fftData" needs explicit rename: serde's `rename_all = "camelCase"`
    // only lowercases the first character, which would produce "fFTData" and miss
    // the frontend listener.
    #[serde(rename = "fftData", rename_all = "camelCase")]
    FFTData { data: Vec<f32> },
    /// Smoothed low-frequency volume in `[0.0, ~1.0]`, computed in Rust from
    /// the actual CPAL output stream using a realtime `70Hz..2kHz` band. This
    /// keeps background motion tied to playback instead of the FFT analysis
    /// thread's cadence.
    #[serde(rename_all = "camelCase")]
    LowFrequencyVolume { volume: f64 },
}

/// Wrapper message that carries a `callback_id` for request/response
/// correlation (same shape as AMLL's `AudioThreadEventMessage<T>`).
///
/// `seq` is a monotonic counter the event forwarder stamps on every
/// outbound event. Both transports (local WebSocket + Tauri channel)
/// deliver the same event with the same `seq`, so the frontend can drop
/// the duplicate that arrives second. Without this, a fast Pause →
/// Seek → Resume burst causes the second transport to re-play
/// `PlayStatus(false)` after the state has already flipped to playing,
/// flipping it back to paused and triggering a spurious `play` toast on
/// the recovery to `true`. `seq = 0` means "unsequenced" — used for
/// inbound messages from the frontend, where there's no risk of dup
/// delivery (single transport per send).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioThreadEventMessage<T> {
    pub callback_id: String,
    pub data: Option<T>,
    #[serde(default)]
    pub seq: u64,
}

impl<T> AudioThreadEventMessage<T> {
    pub fn new(callback_id: String, data: Option<T>) -> Self {
        Self {
            callback_id,
            data,
            seq: 0,
        }
    }

    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    pub fn callback_id(&self) -> &str {
        &self.callback_id
    }

    pub fn to<D>(self, new_data: D) -> AudioThreadEventMessage<D> {
        AudioThreadEventMessage {
            callback_id: self.callback_id,
            data: Some(new_data),
            seq: self.seq,
        }
    }

    pub fn to_none<D>(self) -> AudioThreadEventMessage<D> {
        AudioThreadEventMessage {
            callback_id: self.callback_id,
            data: None,
            seq: self.seq,
        }
    }
}

/// Song data matching AMLL's `SongData` — used in SetPlaylist and SyncStatus.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SongData {
    #[serde(rename_all = "camelCase")]
    Local {
        file_path: String,
        orig_order: usize,
    },
    #[serde(rename_all = "camelCase")]
    Custom {
        id: String,
        song_json_data: String,
        orig_order: usize,
    },
}

impl SongData {
    pub fn file_path(&self) -> Option<&str> {
        match self {
            SongData::Local { file_path, .. } => Some(file_path),
            _ => None,
        }
    }

    pub fn get_id(&self) -> String {
        match self {
            SongData::Local { file_path, .. } => format!("local:{}", file_path),
            SongData::Custom { id, .. } => format!("custom:{}", id),
        }
    }

    pub fn orig_order(&self) -> usize {
        match self {
            SongData::Local { orig_order, .. } => *orig_order,
            SongData::Custom { orig_order, .. } => *orig_order,
        }
    }
}

/// AMLL-style audio display info — what gets sent in events to frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayAudioInfo {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub lyric: String,
    pub cover_media_type: String,
    pub cover: Option<Vec<u8>>,
    pub comment: String,
    pub duration: f64,
    pub position: f64,
}

impl Default for DisplayAudioInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            artist: String::new(),
            album: String::new(),
            lyric: String::new(),
            cover_media_type: String::new(),
            cover: None,
            comment: String::new(),
            duration: 0.0,
            position: 0.0,
        }
    }
}

/// AMLL-style audio quality info sent in LoadAudio / SyncStatus events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioQuality {
    pub bitrate: u32,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for AudioQuality {
    fn default() -> Self {
        Self {
            bitrate: 0,
            sample_rate: 44100,
            channels: 2,
        }
    }
}
