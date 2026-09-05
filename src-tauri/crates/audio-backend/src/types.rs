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
pub enum CrossfadeCurve {
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "equalPower", alias = "equal_power")]
    EqualPower,
    #[serde(rename = "sCurve", alias = "s_curve")]
    SCurve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMixConfig {
    pub enabled: bool,
    pub crossfade_duration: f64,
    pub bpm_match: bool,
    pub beat_align: bool,
    pub volume_norm: bool,
    pub smart_curve: bool,
    pub transition_style: CrossfadeCurve,
    pub transition_effects: bool,
    pub vocal_guard: bool,
}

impl Default for AutoMixConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            crossfade_duration: 8.0,
            bpm_match: true,
            beat_align: true,
            volume_norm: true,
            smart_curve: true,
            transition_style: CrossfadeCurve::EqualPower,
            transition_effects: true,
            vocal_guard: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DspConfig {
    pub enabled: bool,
    #[serde(default)]
    pub input_gain_db: f32,
    #[serde(default)]
    pub equalizer: EqualizerConfig,
    #[serde(default)]
    pub output_gain_db: f32,
    #[serde(default)]
    pub limiter: LimiterConfig,
}

impl Default for DspConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            input_gain_db: 0.0,
            equalizer: EqualizerConfig::default(),
            output_gain_db: 0.0,
            limiter: LimiterConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqualizerConfig {
    pub enabled: bool,
    #[serde(default)]
    pub preamp_db: f32,
    #[serde(default)]
    pub bands: Vec<EqualizerBand>,
}

impl Default for EqualizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterConfig {
    pub enabled: bool,
    #[serde(default = "default_limiter_threshold_db")]
    pub threshold_db: f32,
    #[serde(default = "default_limiter_ceiling_db")]
    pub ceiling_db: f32,
    #[serde(default = "default_limiter_release_ms")]
    pub release_ms: f32,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -1.0,
            ceiling_db: -1.0,
            release_ms: 80.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqualizerBand {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub filter_type: EqualizerFilterType,
    pub frequency: f32,
    pub gain_db: f32,
    pub q: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EqualizerFilterType {
    Peaking,
    LowShelf,
    HighShelf,
}

fn default_true() -> bool {
    true
}

fn default_limiter_threshold_db() -> f32 {
    -1.0
}

fn default_limiter_ceiling_db() -> f32 {
    -1.0
}

fn default_limiter_release_ms() -> f32 {
    80.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoMixNativeState {
    Idle,
    Preparing,
    Waiting,
    Crossfading,
    Finishing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMixStatus {
    pub state: AutoMixNativeState,
    pub enabled: bool,
    pub transition_id: Option<u64>,
    pub current_index: usize,
    pub next_index: Option<usize>,
    pub current_id: Option<String>,
    pub next_id: Option<String>,
    pub crossfade_start: Option<f64>,
    pub crossfade_duration: Option<f64>,
    pub error: Option<String>,
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
    SeekAudio {
        position: f64,
        #[serde(default)]
        request_id: Option<u64>,
        #[serde(default)]
        expected_music_id: Option<String>,
    },
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
    SetPlaylist {
        songs: Vec<SongData>,
        /// `true` when `songs` is a bounded prefill window (current track +
        /// pre-resolved next tracks) rather than a full playlist: advancing
        /// past the last entry must stop instead of wrapping around, so a
        /// frozen frontend (Android background) never re-plays stale entries.
        #[serde(default)]
        windowed: bool,
        /// When present, replace the queue and start this logical index as one
        /// player-loop operation. This avoids racing a separate JumpToSong
        /// command against the playlist replacement.
        #[serde(default)]
        play_index: Option<usize>,
        /// Optional position used by the atomic `play_index` load.
        #[serde(default)]
        initial_position: Option<f64>,
        /// Frontend load generation used to correlate lifecycle events with
        /// the controller that initiated this atomic load.
        #[serde(default)]
        load_request_id: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    SetVolume { volume: f64 },
    #[serde(rename_all = "camelCase")]
    SetVolumeRelative { volume: f64 },
    #[serde(rename_all = "camelCase")]
    SetAudioOutput { name: String },
    #[serde(rename_all = "camelCase")]
    SetAnalysis { enabled: bool },
    #[serde(rename_all = "camelCase")]
    SetFFT { enabled: bool },
    #[serde(rename_all = "camelCase")]
    SetFFTRange { from_freq: f32, to_freq: f32 },
    #[serde(rename_all = "camelCase")]
    SetEqualizer { config: EqualizerConfig },
    #[serde(rename_all = "camelCase")]
    SetDsp { config: DspConfig },
    #[serde(rename_all = "camelCase")]
    SyncStatus,
    #[serde(rename_all = "camelCase")]
    Close,
    #[serde(rename_all = "camelCase")]
    SetMediaControlsEnabled { enabled: bool },
    #[serde(rename_all = "camelCase")]
    AutomixSetEnabled { enabled: bool },
    #[serde(rename_all = "camelCase")]
    AutomixConfigure { config: AutoMixConfig },
    #[serde(rename_all = "camelCase")]
    AutomixPrepareNext {
        current_index: usize,
        next_index: usize,
        next_song: SongData,
        #[serde(default)]
        transition_id: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    AutomixCancel,
    #[serde(rename_all = "camelCase")]
    AutomixForceStart {
        #[serde(default)]
        generation: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    AutomixCompleteNative {
        generation: u64,
        current_index: usize,
        position: f64,
    },
    /// Replace the native playback manifest wholesale. `revision` must be
    /// strictly newer than the stored one or the message is ignored.
    #[serde(rename_all = "camelCase")]
    SetNativeManifest { manifest: NativePlaybackManifest },
    /// Drop the manifest and stop planner-driven advancement. Strong
    /// semantics: cancels prefetch and clears the bounded queue.
    #[serde(rename_all = "camelCase")]
    ClearNativeManifest { revision: u64 },
    /// Push resolver endpoints/credentials. Sent on login, logout, setting
    /// changes, and at startup.
    #[serde(rename_all = "camelCase")]
    SetNativeResolverConfig { config: NativeResolverConfig },
    /// Announce the track that is *about to* load, before its URL is resolved.
    ///
    /// Resolving a playback URL is a network round trip, and the download that
    /// follows is another. Without this the OS media session keeps showing the
    /// previous track for that whole window — the user presses next and nothing
    /// visibly happens. Announcing decouples "what is playing" from "is it
    /// ready", so the session swaps instantly and simply shows it as not yet
    /// playing.
    ///
    /// Superseded by the real load: once a track with this identity is actually
    /// loaded, the announcement is dropped and the normal projection takes
    /// over. An announcement for a track that never loads is cleared by the
    /// next announcement or by a load of anything else.
    #[serde(rename_all = "camelCase")]
    AnnounceTrack {
        identity: TrackIdentity,
        display: TrackDisplay,
    },
    /// Enable/disable planner-driven advancement without dropping the
    /// manifest. Used to gate personal FM / listen-together.
    #[serde(rename_all = "camelCase")]
    SetNativePlannerEnabled { enabled: bool },
    /// Request an authoritative `NativePlannerStatus` emission.
    #[serde(rename_all = "camelCase")]
    SyncNativePlannerStatus,
    /// Arm or disarm the listen-together keepalive. `room_id: None` disarms.
    ///
    /// The heartbeat must outlive the WebView — on Android the page is
    /// destroyed while playback continues, and a missed heartbeat drops the
    /// user out of the room server-side.
    #[serde(rename_all = "camelCase")]
    SetListenTogetherRoom {
        #[serde(default)]
        room_id: Option<String>,
    },
    /// Push what the frontend knows about the session controls. A patch, so it
    /// can report the play mode without claiming to know the like state and
    /// vice versa.
    #[serde(rename_all = "camelCase")]
    SetSessionControls { controls: SessionControlsPatch },
    /// Advance the play mode one step around the ring. An *intent*, not a value:
    /// it comes from a notification button that cannot know what the current
    /// mode is, so the backend — which does — resolves it.
    SetNextPlayMode,
    /// Toggle the loaded track's like state, performing the account call.
    ///
    /// The backend owns the side effect because the button has to work with no
    /// WebView alive, which is the only time the notification is the user's
    /// only UI.
    ToggleFavourite,
}

/// Events emitted from player → frontend (via Tauri event emit).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type", content = "data")]
pub enum AudioThreadEvent {
    #[serde(rename_all = "camelCase")]
    PlayPosition {
        position: f64,
        /// Which timeline this position belongs to — see `PlayerClock::epoch`.
        ///
        /// A subscriber that holds a different epoch has not adopted this track
        /// yet and must drop the packet rather than reconcile it against the
        /// clock it still holds: the `LoadAudio`/`SyncStatus` carrying the same
        /// epoch is what anchors it, and that one arrives right behind this.
        #[serde(default)]
        timeline_epoch: u64,
    },
    #[serde(rename_all = "camelCase")]
    LoadProgress { position: f64 },
    #[serde(rename_all = "camelCase")]
    LoadAudio {
        music_id: String,
        music_info: DisplayAudioInfo,
        quality: AudioQuality,
        current_play_index: usize,
        load_request_id: Option<u64>,
        /// Stable identity of the loaded track, when the backend knows it
        /// (planner-driven load, or a frontend load already re-anchored by a
        /// manifest). `music_id` is `local:<cdn-url>` and therefore changes on
        /// every re-resolve, so it can never be used to reconcile across a
        /// WebView reload — this can.
        #[serde(default)]
        identity: Option<TrackIdentity>,
        /// Timeline this load started. Authoritative signal that the clock the
        /// subscriber holds is retired, on every path — including the ones where
        /// neither `identity` nor `music_id` changed observably.
        #[serde(default)]
        timeline_epoch: u64,
    },
    #[serde(rename_all = "camelCase")]
    LoadingAudio {
        music_id: String,
        current_play_index: usize,
        load_request_id: Option<u64>,
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
        /// Stable identity of the playing track — see `LoadAudio::identity`.
        #[serde(default)]
        identity: Option<TrackIdentity>,
        /// Timeline this snapshot describes — see `LoadAudio::timeline_epoch`.
        #[serde(default)]
        timeline_epoch: u64,
    },
    #[serde(rename_all = "camelCase")]
    PlayListChanged {
        playlist: Vec<SongData>,
        current_play_index: usize,
    },
    #[serde(rename_all = "camelCase")]
    PlayStatus { is_playing: bool },
    #[serde(rename_all = "camelCase")]
    SeekCommitted {
        request_id: Option<u64>,
        position: f64,
    },
    #[serde(rename_all = "camelCase")]
    SeekFailed {
        request_id: Option<u64>,
        position: f64,
        error: String,
    },
    #[serde(rename_all = "camelCase")]
    LoadError {
        music_id: String,
        load_request_id: Option<u64>,
        error: String,
    },
    #[serde(rename_all = "camelCase")]
    PlayError { error: String },
    #[serde(rename_all = "camelCase")]
    VolumeChanged { volume: f64 },
    #[serde(rename_all = "camelCase")]
    AudioOutputChanged {
        device_name: String,
        is_default: bool,
        channels: u16,
        sample_rate: u32,
        sample_format: String,
    },
    #[serde(rename_all = "camelCase")]
    AudioOutputError { error: String, recoverable: bool },
    // FFTData → "fftData" needs explicit rename: serde's `rename_all = "camelCase"`
    // only lowercases the first character, which would produce "fFTData" and miss
    // the frontend listener.
    #[serde(rename = "fftData", rename_all = "camelCase")]
    FFTData { data: Vec<f32> },
    /// Smoothed low-frequency volume in `[0.0, ~1.0]`, computed from the same
    /// raw FFT magnitudes emitted as `fftData`.
    #[serde(rename_all = "camelCase")]
    LowFrequencyVolume { volume: f64 },
    #[serde(rename_all = "camelCase")]
    AutomixStatus { status: AutoMixStatus },
    #[serde(rename_all = "camelCase")]
    AutomixAnalysisReady {
        current_id: String,
        next_id: String,
        transition_id: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    AutomixCrossfadeStarted {
        from_id: String,
        to_id: String,
        duration: f64,
        transition_id: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    AutomixCrossfadeComplete {
        current_index: usize,
        music_id: String,
        position: f64,
        duration: f64,
        transition_id: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    AutomixError { error: String, recoverable: bool },
    /// Authoritative planner state. Carries `manifest_revision` so the
    /// frontend can drop anything from a superseded generation.
    #[serde(rename_all = "camelCase")]
    NativePlannerStatusChanged { status: NativePlannerStatus },
    /// The planner advanced to a new track on its own (JS was frozen or not
    /// involved). Carries stable identity plus the UI index to adopt.
    #[serde(rename_all = "camelCase")]
    NativePlannerAdvanced {
        manifest_revision: u64,
        identity: TrackIdentity,
        playlist_index: usize,
        music_id: String,
    },
    /// Every candidate in this revision failed to resolve or play. Terminal
    /// until a new manifest arrives; `reason` is already redacted.
    #[serde(rename_all = "camelCase")]
    NativePlannerExhausted {
        manifest_revision: u64,
        attempted: usize,
        reason: String,
    },
    /// Resolved display metadata for the current track. Consumed in-process by
    /// the OS media-session bridge (which keeps working while the WebView is
    /// gone) and by the frontend for reconciliation.
    #[serde(rename_all = "camelCase")]
    NowPlayingChanged { info: NowPlayingInfo },
    /// The authoritative session controls changed, whoever asked for it.
    ///
    /// Emitted only on a real change (see [`SessionControls::apply`]) so the
    /// frontend can adopt it unconditionally without the adopt→republish→adopt
    /// loop an echo would create.
    #[serde(rename_all = "camelCase")]
    SessionControlsChanged { controls: SessionControls },
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

// ═══════════════════════════════════════════════════════════════════
// Native manifest / planner protocol
// ═══════════════════════════════════════════════════════════════════

/// Stable track identity. Deliberately independent of any resolved CDN URL:
/// URLs expire and get re-resolved, identity must not change when they do.
///
/// `key()` is the canonical string form used for all reconciliation between
/// frontend and backend (and inside the manifest/planner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "camelCase")]
pub enum TrackIdentity {
    #[serde(rename_all = "camelCase")]
    Netease { id: String },
    #[serde(rename_all = "camelCase")]
    Local { path: String },
}

impl TrackIdentity {
    pub fn key(&self) -> String {
        match self {
            TrackIdentity::Netease { id } => format!("netease:{id}"),
            TrackIdentity::Local { path } => format!("local-file:{path}"),
        }
    }

    pub fn netease_id(&self) -> Option<&str> {
        match self {
            TrackIdentity::Netease { id } => Some(id),
            _ => None,
        }
    }
}

/// One manifest row: stable identity plus the minimum the resolver and the
/// media-session UI need. No CDN URL, no credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeManifestEntry {
    pub identity: TrackIdentity,
    /// UI-facing playlist index. May be sparse or non-monotonic; never used
    /// for advancement ordering (that is `NativePlaybackManifest::order`).
    pub playlist_index: usize,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    /// Album name. Display-only, but the backend needs it because it drives the
    /// OS media session for tracks it advanced to on its own — the frontend may
    /// not be alive to describe them.
    #[serde(default)]
    pub album: Option<String>,
    /// Cover art URL (https). Same rationale as `album`; the platform media
    /// session fetches it natively, so no bytes cross this boundary.
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Netease `fee` field, mirrored so the Rust resolver can apply the same
    /// VIP pre-check the frontend does (`fee == 1 || fee == 4` → try UNM first).
    #[serde(default)]
    pub fee: Option<i64>,
    /// Whether the track carries a `pc` field (cloud-uploaded). Cloud tracks
    /// bypass the VIP pre-check, matching `resolveSongUrl`.
    #[serde(default)]
    pub has_pc: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativePlaybackMode {
    Normal,
    Single,
    Random,
}

impl Default for NativePlaybackMode {
    fn default() -> Self {
        Self::Normal
    }
}

/// Full lightweight playback manifest. `revision` is monotonic per frontend
/// session; the backend rejects anything not strictly newer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePlaybackManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub revision: u64,
    pub entries: Vec<NativeManifestEntry>,
    /// Explicit traversal order as indices into `entries`. Empty means natural
    /// order, which is what the frontend ships for every mode: random mode
    /// shuffles the playlist itself, so the permutation *is* the entry order.
    #[serde(default)]
    pub order: Vec<usize>,
    #[serde(default)]
    pub cursor_identity: Option<TrackIdentity>,
    #[serde(default)]
    pub cursor_index: usize,
    #[serde(default)]
    pub mode: NativePlaybackMode,
    /// Whether running off the end of the traversal wraps.
    #[serde(default = "default_true")]
    pub repeat_list: bool,
    /// Seed for the one order the backend builds itself: the shuffle
    /// `ManifestStore::set_mode` installs for a play-mode press that arrives
    /// with no WebView alive to republish one.
    #[serde(default)]
    pub random_seed: Option<u64>,
}

fn default_schema_version() -> u32 {
    1
}

/// Credentials/endpoints the Rust resolver needs to call the same deployed
/// NeteaseCloudMusicApi the frontend uses. Pushed by the frontend; never
/// persisted to disk and never logged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeResolverConfig {
    /// Base URL of the deployed NCM API (e.g. `https://ncm-api.example.com`).
    #[serde(default)]
    pub ncm_base_url: Option<String>,
    /// UNM match endpoint base, when the user has one configured.
    #[serde(default)]
    pub unm_base_url: Option<String>,
    #[serde(default)]
    pub unm_enabled: bool,
    /// NCM cookie (`MUSIC_U=...`). Sensitive: redacted in all logs.
    #[serde(default)]
    pub cookie: Option<String>,
    /// Netease user id of the signed-in account.
    ///
    /// Needed because `/likelist` is keyed by `uid`, and the backend has to be
    /// able to answer "is this track liked" for itself: the notification's heart
    /// is drawn while the WebView is dead, which is exactly when nothing can
    /// tell it. `None` means signed out — no like list, no heart.
    #[serde(default)]
    pub user_id: Option<String>,
    /// Quality level string passed straight through to `/song/url/v1`.
    #[serde(default)]
    pub level: Option<String>,
    /// Whether the frontend selected the in-process protocol layer. Playback
    /// resolution follows the same choice as the UI: split transports would
    /// mean two sessions and two source IPs, with only one of them benefiting.
    #[serde(default)]
    pub use_local_ncm: bool,
}

impl NativeResolverConfig {
    /// Whether the config can drive an NCM lookup at all.
    pub fn is_usable(&self) -> bool {
        // The in-process layer needs no base URL, but it does need to actually
        // be installed. On the web build there is no `player` module at all,
        // hence the cfg — a QuickJS isolate has no place in wasm32.
        #[cfg(not(target_arch = "wasm32"))]
        if self.use_local_ncm && crate::player::source_resolver::has_ncm_call_hook() {
            return true;
        }
        self.ncm_base_url
            .as_deref()
            .is_some_and(|base| !base.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativePlannerPlaybackState {
    Stopped,
    Loading,
    Playing,
    Paused,
    Ended,
}

/// Authoritative planner state, used by the frontend to reconcile after a
/// freeze/wake or a WebView restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePlannerStatus {
    pub manifest_revision: u64,
    pub cursor_identity: Option<TrackIdentity>,
    pub cursor_index: Option<usize>,
    pub playback_state: NativePlannerPlaybackState,
    pub prepared_identity: Option<TrackIdentity>,
    pub failure_count: usize,
    pub exhausted: bool,
    /// Whether the planner is allowed to choose the next track at all.
    ///
    /// Distinct from "a manifest is loaded": server-driven modes (personal FM,
    /// listen-together) still publish a manifest so the backend can resolve a
    /// track it is *told* to play, but must not advance on their own. The
    /// frontend needs this to decide whether to wait for a backend-initiated
    /// advance at track end or run its own transition immediately.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Authoritative snapshot of the live playback session, readable synchronously
/// (no round-trip through the player message loop) via the `audio_get_session`
/// command.
///
/// This exists for one reason: on Android the WebView is destroyed and the page
/// reloaded while the Rust process — and playback — survives. The reloaded
/// frontend rehydrates from persisted storage, which describes the track that
/// was playing when the app went to the background, not what is playing now.
/// Without an authoritative read the frontend would re-`SetPlaylist` that stale
/// track and roll playback back. The boot path asks here first and adopts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSessionSnapshot {
    /// `false` when nothing is loaded — the frontend then owns startup as before.
    pub has_track: bool,
    /// Transport id (`local:<url>`). Only useful for re-seeding a controller's
    /// expected id; never for reconciliation (see `identity`).
    pub music_id: String,
    /// Stable identity of the playing track, when known.
    pub identity: Option<TrackIdentity>,
    pub playlist_index: usize,
    pub position: f64,
    pub duration: f64,
    pub is_playing: bool,
    pub volume: f64,
    /// Manifest revision the backend currently holds, so the frontend can keep
    /// its own monotonic counter ahead of it without waiting for an event.
    pub manifest_revision: u64,
    /// Whether the planner is in a position to drive advancement right now.
    pub planner_active: bool,
}

/// Resolved "what is playing" for OS media sessions.
///
/// The backend must be able to describe a track it advanced to on its own, with
/// no JS runtime alive — that is the whole point of driving the media session
/// from Rust. Streamed tracks carry no file tags, so the display fields come
/// from the manifest entry; local files fall back to the decoder's tag read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlayingInfo {
    /// `false` when nothing is loaded — the session should be cleared.
    pub has_track: bool,
    pub identity: Option<TrackIdentity>,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Cover art URL. Fetched natively by the platform layer, so no image bytes
    /// cross this boundary.
    pub artwork_url: Option<String>,
    pub duration: f64,
    pub position: f64,
    pub is_playing: bool,
    /// A source resolve / download / decoder open is in flight for this track.
    ///
    /// Distinct from `!is_playing`: "paused" is a state the user chose, this
    /// one is "working on it". The OS session renders it as buffering, which
    /// is the only thing that makes a slow start look different from a dead
    /// play button.
    pub is_loading: bool,
    pub playlist_index: usize,
    /// User-facing controls projected alongside the track — see
    /// [`SessionControls`]. Carried here rather than on a separate event because
    /// the OS session renders them next to the metadata and a subscriber that
    /// received one without the other would render a half-updated notification.
    pub controls: SessionControls,
}

/// Session controls that are *not* audio state.
///
/// The transport fields on [`NowPlayingInfo`] are derived from the decoder;
/// these are the user's own choices (play mode) and their account state
/// (favourite). They are projected onto every surface and settable from every
/// surface, so the backend holds the one copy all three agree on: the frontend
/// pushes what it knows through `SetSessionControls`, the OS pushes intents
/// through `CyclePlayMode` / `ToggleFavourite`, and `SessionControlsChanged`
/// fans the result back out. Nothing else may write them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionControls {
    pub play_mode: NativePlaybackMode,
    /// Whether the loaded track is in the user's 我喜欢的音乐.
    pub favourite: bool,
    /// Whether toggling is possible at all. A logged-out user, or a track with
    /// no Netease identity, has nothing to like — and a control the OS renders
    /// but cannot honour is worse than one it does not render.
    pub can_favourite: bool,
}

/// Partial update to [`SessionControls`]. Absent fields are left alone, which is
/// what lets the frontend push what it knows (play mode, likelist membership)
/// without claiming authority over the rest.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionControlsPatch {
    #[serde(default)]
    pub play_mode: Option<NativePlaybackMode>,
    #[serde(default)]
    pub favourite: Option<bool>,
    #[serde(default)]
    pub can_favourite: Option<bool>,
}

impl SessionControls {
    /// Apply a patch, reporting whether anything actually changed.
    ///
    /// The bool is load-bearing: every writer round-trips through
    /// `SessionControlsChanged`, so an unconditional emit would have the
    /// frontend adopt its own push, republish, and emit again.
    pub fn apply(&mut self, patch: &SessionControlsPatch) -> bool {
        let next = SessionControls {
            play_mode: patch.play_mode.unwrap_or(self.play_mode),
            favourite: patch.favourite.unwrap_or(self.favourite),
            can_favourite: patch.can_favourite.unwrap_or(self.can_favourite),
        };
        let changed = next != *self;
        *self = next;
        changed
    }
}

impl NativePlaybackMode {
    /// Next mode for a single OS-side press.
    ///
    /// Mirrors the frontend's own cycle in `musicData.setPlaySongMode`, because
    /// a user cycling from the notification and from the app must walk the same
    /// ring — they are the same setting.
    pub fn cycled(self) -> Self {
        match self {
            Self::Normal => Self::Random,
            Self::Random => Self::Single,
            Self::Single => Self::Normal,
        }
    }
}

impl Default for NowPlayingInfo {
    fn default() -> Self {
        Self {
            has_track: false,
            identity: None,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            artwork_url: None,
            duration: 0.0,
            position: 0.0,
            is_playing: false,
            is_loading: false,
            playlist_index: 0,
            controls: SessionControls::default(),
        }
    }
}

impl NowPlayingInfo {
    /// Whether two snapshots describe the same track with the same display
    /// text. Position/playing/loading state deliberately excluded: those ride
    /// on `PlayPosition`/`PlayStatus`/`LoadingAudio` and must not force a
    /// metadata rebuild (which on Android re-downloads the artwork).
    pub fn same_metadata(&self, other: &Self) -> bool {
        self.has_track == other.has_track
            && self.identity == other.identity
            && self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.artwork_url == other.artwork_url
            && self.duration == other.duration
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
        /// Display metadata for the OS media session, sent with the track.
        ///
        /// Every streamed track arrives as `Local` with an https `file_path`,
        /// and the backend downloads it to a temp file before decoding. Without
        /// this the only name available at load time is that temp file's stem —
        /// a random string — so the notification showed garbage until a decoded
        /// tag or a `/song/detail` round trip corrected it.
        ///
        /// Optional so older frontends and hand-built rows still deserialize;
        /// absent simply means "fall back to tags".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<TrackDisplay>,
    },
    #[serde(rename_all = "camelCase")]
    Custom {
        id: String,
        song_json_data: String,
        orig_order: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<TrackDisplay>,
    },
}

/// Caller-supplied display metadata, carried alongside a queued track.
///
/// Deliberately a flat typed struct rather than a JSON blob: this is on the hot
/// path of every track change, and the backend should not be parsing an
/// arbitrary song document to find a title.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackDisplay {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artwork_url: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend reconciles a WebView reload by comparing these exact
    /// strings (`src/utils/tauri/audio/identity.ts::trackIdentityKey`). A change
    /// here silently breaks adoption — playback would rewind to the persisted
    /// track on every app resume — so pin the format.
    #[test]
    fn track_identity_key_format_is_pinned() {
        assert_eq!(
            TrackIdentity::Netease { id: "123".into() }.key(),
            "netease:123"
        );
        assert_eq!(
            TrackIdentity::Local {
                path: "/music/a.flac".into()
            }
            .key(),
            "local-file:/music/a.flac"
        );
    }

    /// `audio_get_session` is read by the boot path before it commits to
    /// loading anything; the field names must match
    /// `NativeSessionSnapshot` in `src/utils/tauri/audio/protocol/manifest.ts`.
    #[test]
    fn session_snapshot_uses_camel_case_wire_names() {
        let snapshot = NativeSessionSnapshot {
            has_track: true,
            music_id: "local:https://cdn/a.mp3".into(),
            identity: Some(TrackIdentity::Netease { id: "7".into() }),
            playlist_index: 4,
            position: 12.5,
            duration: 200.0,
            is_playing: true,
            volume: 0.8,
            manifest_revision: 9,
            planner_active: true,
        };
        let value: serde_json::Value = serde_json::to_value(&snapshot).expect("serialize");
        let object = value.as_object().expect("object");

        for key in [
            "hasTrack",
            "musicId",
            "identity",
            "playlistIndex",
            "position",
            "duration",
            "isPlaying",
            "volume",
            "manifestRevision",
            "plannerActive",
        ] {
            assert!(object.contains_key(key), "missing wire field `{key}`");
        }
        assert_eq!(object["identity"]["provider"], "netease");
        assert_eq!(object["identity"]["id"], "7");
    }

    /// A `SyncStatus` without an identity must still deserialize: the backend
    /// omits it for frontend-driven loads that no manifest has anchored yet.
    #[test]
    fn sync_status_identity_is_optional_on_the_wire() {
        let json = serde_json::json!({
            "type": "syncStatus",
            "data": {
                "musicId": "local:x",
                "musicInfo": serde_json::to_value(DisplayAudioInfo::default()).unwrap(),
                "isPlaying": false,
                "duration": 0.0,
                "position": 0.0,
                "volume": 1.0,
                "loadPosition": 0.0,
                "playlist": [],
                "currentPlayIndex": 0,
                "playlistInited": false,
                "quality": serde_json::to_value(AudioQuality::default()).unwrap(),
            }
        });
        let event: AudioThreadEvent = serde_json::from_value(json).expect("deserialize");
        match event {
            AudioThreadEvent::SyncStatus { identity, .. } => assert!(identity.is_none()),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    /// Server-driven modes (personal FM, listen-together) publish a manifest but
    /// gate the planner off. The frontend reads `enabled` to decide whether to
    /// wait for a backend advance at track end; if it went missing, every track
    /// change in those modes would stall behind the adoption fallback timer.
    #[test]
    fn planner_status_carries_the_gate_and_defaults_to_enabled() {
        let status = NativePlannerStatus {
            manifest_revision: 3,
            cursor_identity: None,
            cursor_index: None,
            playback_state: NativePlannerPlaybackState::Playing,
            prepared_identity: None,
            failure_count: 0,
            exhausted: false,
            enabled: false,
        };
        let value = serde_json::to_value(&status).expect("serialize");
        assert_eq!(value["enabled"], false, "gate must reach the frontend");

        // An older payload without the field must read as enabled — that was
        // the behaviour before the gate existed.
        let legacy = serde_json::json!({
            "manifestRevision": 1,
            "cursorIdentity": null,
            "cursorIndex": null,
            "playbackState": "playing",
            "preparedIdentity": null,
            "failureCount": 0,
            "exhausted": false,
        });
        let back: NativePlannerStatus = serde_json::from_value(legacy).expect("deserialize");
        assert!(back.enabled);
    }

    /// The media bridge rebuilds notification metadata only when this says the
    /// track changed. On Android a rebuild re-downloads the artwork, and
    /// `NowPlayingChanged` is emitted from `sync_ui` — which also fires on
    /// seeks and output-device rebuilds. If position or playing state leaked
    /// into the comparison, every seek would re-fetch the cover.
    #[test]
    fn now_playing_metadata_equality_ignores_position_and_play_state() {
        let base = NowPlayingInfo {
            has_track: true,
            identity: Some(TrackIdentity::Netease { id: "1".into() }),
            title: "t".into(),
            artist: "a".into(),
            album: "al".into(),
            artwork_url: Some("https://img/x".into()),
            duration: 200.0,
            position: 10.0,
            is_playing: true,
            is_loading: false,
            playlist_index: 3,
            controls: SessionControls::default(),
        };

        let seeked = NowPlayingInfo {
            position: 150.0,
            is_playing: false,
            playlist_index: 9,
            ..base.clone()
        };
        assert!(base.same_metadata(&seeked), "a seek must not rebuild metadata");

        // Buffering rides on the load events, not on a metadata rebuild —
        // otherwise every track start would re-download the cover twice.
        let buffering = NowPlayingInfo {
            is_loading: true,
            ..base.clone()
        };
        assert!(
            base.same_metadata(&buffering),
            "a load transition must not rebuild metadata"
        );

        let next_track = NowPlayingInfo {
            identity: Some(TrackIdentity::Netease { id: "2".into() }),
            ..base.clone()
        };
        assert!(!base.same_metadata(&next_track), "a track change must rebuild");

        let retitled = NowPlayingInfo {
            title: "t2".into(),
            ..base.clone()
        };
        assert!(!base.same_metadata(&retitled));

        let recovered_duration = NowPlayingInfo {
            duration: 201.0,
            ..base.clone()
        };
        assert!(
            !base.same_metadata(&recovered_duration),
            "duration reaches the seek bar, so it must refresh"
        );

        // Session controls travel *with* the projection but must not force a
        // rebuild on their own — `apply_controls` handles them, and cycling
        // shuffle should not cost an artwork download.
        let shuffled = NowPlayingInfo {
            controls: SessionControls {
                play_mode: NativePlaybackMode::Random,
                favourite: true,
                can_favourite: true,
            },
            ..base.clone()
        };
        assert!(
            base.same_metadata(&shuffled),
            "a controls change must not rebuild metadata"
        );

        let stopped = NowPlayingInfo::default();
        assert!(!base.same_metadata(&stopped));
    }

    /// A patch reports whether it changed anything, and that bool gates the
    /// event. Without it every writer would re-emit its own push, the frontend
    /// would adopt it, republish, and the three surfaces would trade the same
    /// value forever.
    #[test]
    fn a_no_op_patch_reports_no_change() {
        let mut controls = SessionControls {
            play_mode: NativePlaybackMode::Random,
            favourite: true,
            can_favourite: true,
        };

        // Empty patch: nothing claimed, nothing changed.
        assert!(!controls.apply(&SessionControlsPatch::default()));
        // Same values restated — this is the echo case.
        assert!(!controls.apply(&SessionControlsPatch {
            play_mode: Some(NativePlaybackMode::Random),
            favourite: Some(true),
            can_favourite: Some(true),
        }));
        assert!(controls.apply(&SessionControlsPatch {
            favourite: Some(false),
            ..Default::default()
        }));
        assert!(!controls.favourite);
        assert_eq!(
            controls.play_mode,
            NativePlaybackMode::Random,
            "an absent field must be left alone, not defaulted"
        );
    }

    /// The OS button sends "next mode", so both rings have to agree — the app's
    /// own cycle is normal → random → single.
    #[test]
    fn play_mode_cycles_the_same_ring_as_the_app() {
        let mut mode = NativePlaybackMode::Normal;
        let mut seen = Vec::new();
        for _ in 0..4 {
            mode = mode.cycled();
            seen.push(mode);
        }
        assert_eq!(
            seen,
            vec![
                NativePlaybackMode::Random,
                NativePlaybackMode::Single,
                NativePlaybackMode::Normal,
                NativePlaybackMode::Random,
            ]
        );
    }

    /// Manifest entries carry the display metadata the OS media session needs
    /// for tracks the backend advanced to on its own.
    #[test]
    fn manifest_entry_round_trips_display_metadata() {
        let entry = NativeManifestEntry {
            identity: TrackIdentity::Netease { id: "1".into() },
            playlist_index: 0,
            title: Some("t".into()),
            artist: Some("a".into()),
            album: Some("al".into()),
            artwork_url: Some("https://img/x?param=512y512".into()),
            duration_ms: Some(225_000),
            fee: Some(1),
            has_pc: false,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"artworkUrl\""), "camelCase artworkUrl: {json}");
        assert!(json.contains("\"durationMs\""), "camelCase durationMs: {json}");

        let back: NativeManifestEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.album.as_deref(), Some("al"));
        assert_eq!(back.duration_ms, Some(225_000));
    }
}
