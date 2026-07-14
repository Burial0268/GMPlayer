//! Web/WASM IPC state machine for the audio backend contract.
//!
//! The browser owns actual playback through a media output host.
//! This module keeps the AMLL-style message/event contract identical to the
//! native backend and returns explicit effects that the JS runtime applies.

use std::io::Cursor;

use audio_analysis::{AudioProcessor, LowFreqConfig};
use serde::Serialize;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use wasm_bindgen::prelude::*;

use crate::types::{
    AudioQuality, AudioThreadEvent, AudioThreadEventMessage, AudioThreadMessage, DisplayAudioInfo,
    PlaybackState, SongData,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebBackendReply {
    events: Vec<AudioThreadEventMessage<AudioThreadEvent>>,
    effects: Vec<WebBackendEffect>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisFrameReply<'a> {
    events: [AnalysisEventMessage<'a>; 2],
    effects: [WebBackendEffect; 0],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalysisEventMessage<'a> {
    callback_id: &'static str,
    data: AnalysisFrameEvent<'a>,
    seq: u64,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data")]
enum AnalysisFrameEvent<'a> {
    #[serde(rename = "fftData")]
    FftData { data: &'a [f32] },
    #[serde(rename = "lowFrequencyVolume")]
    LowFrequencyVolume { volume: f64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WebBackendEffect {
    #[serde(rename_all = "camelCase")]
    LoadTrack {
        src: String,
        initial_position: f64,
        music_id: String,
        current_play_index: usize,
    },
    Play,
    Pause,
    #[serde(rename_all = "camelCase")]
    Seek {
        position: f64,
    },
    #[serde(rename_all = "camelCase")]
    SetVolume {
        volume: f64,
    },
    #[serde(rename_all = "camelCase")]
    SetOutputDevice {
        name: String,
    },
    Close,
}

struct DecodedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
    duration: f64,
}

#[wasm_bindgen]
pub struct DecodedAudioJs {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
    duration: f64,
}

#[wasm_bindgen]
impl DecodedAudioJs {
    #[wasm_bindgen(js_name = "samples")]
    pub fn samples(&self) -> Vec<f32> {
        self.samples.clone()
    }

    /// Move PCM into JavaScript without first cloning the complete track in
    /// WASM memory. `samples()` remains available for compatibility.
    #[wasm_bindgen(js_name = "takeSamples")]
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    #[wasm_bindgen(js_name = "sampleRate")]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[wasm_bindgen(js_name = "channels")]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    #[wasm_bindgen(js_name = "duration")]
    pub fn duration(&self) -> f64 {
        self.duration
    }
}

struct AnalysisState {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
    duration: f64,
    processor: AudioProcessor,
    spectrum: Vec<f32>,
    next_sample_index: usize,
}

impl AnalysisState {
    fn new(decoded: DecodedAudio, freq_min: f32, freq_max: f32) -> Self {
        let cfg = LowFreqConfig::default();
        let mut processor = AudioProcessor::new(
            2048,
            freq_min,
            freq_max,
            cfg.bin_count,
            cfg.window_size,
            cfg.gradient_threshold,
            cfg.smoothing_factor,
        );
        processor.clear();
        let sample_rate = decoded.sample_rate.max(1);
        let channels = decoded.channels.max(1);

        Self {
            samples: decoded.samples,
            sample_rate,
            channels,
            duration: decoded.duration,
            processor,
            spectrum: vec![0.0; 2048],
            next_sample_index: 0,
        }
    }

    fn set_freq_range(&mut self, from: f32, to: f32) {
        self.processor.fft.set_freq_range(from, to);
    }

    fn clear(&mut self) {
        self.processor.clear();
        self.spectrum.fill(0.0);
        self.next_sample_index = 0;
    }

    fn process(&mut self, position: f64, delta_ms: f64) -> f64 {
        let target = sample_index_for_position(position, self.sample_rate, self.samples.len());
        if target >= self.samples.len() {
            self.processor.clear();
            self.spectrum.fill(0.0);
            self.next_sample_index = self.samples.len();
            return 0.0;
        }

        let seek_back_edge = self.next_sample_index.saturating_sub(2048);
        let seek_forward_edge = self
            .next_sample_index
            .saturating_add((self.sample_rate as usize / 2).max(2048));
        if target < seek_back_edge || target > seek_forward_edge {
            self.processor.clear();
            self.next_sample_index = target;
        }

        let feed_until = target.saturating_add(2048).min(self.samples.len());
        if feed_until > self.next_sample_index {
            self.processor.push_pcm(
                &self.samples[self.next_sample_index..feed_until],
                self.sample_rate,
            );
            self.next_sample_index = feed_until;
        }

        let delta = if delta_ms.is_finite() {
            delta_ms.clamp(1.0, 100.0) as f32
        } else {
            33.0
        };
        let low_freq = self.processor.process_frame(delta, &mut self.spectrum);
        low_freq as f64
    }
}

/// WASM-side state holder for the browser IPC runtime.
///
/// It deliberately does not touch sockets, threads, files, CPAL, or Tauri.
/// Browser code calls `sendMessageJson`, applies returned effects to the Web
/// output host, and feeds media callbacks back via the `apply*` methods so
/// events stay in the same shape as the native backend.
#[wasm_bindgen]
pub struct WasmAudioBackend {
    state: PlaybackState,
    position: f64,
    duration: f64,
    volume: f64,
    load_position: f64,
    playlist: Vec<SongData>,
    playlist_inited: bool,
    current_play_index: usize,
    music_info: DisplayAudioInfo,
    quality: AudioQuality,
    analysis: Option<AnalysisState>,
    analysis_freq_min: f32,
    analysis_freq_max: f32,
    seq: u64,
}

#[wasm_bindgen]
impl WasmAudioBackend {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            load_position: 0.0,
            playlist: Vec::new(),
            playlist_inited: false,
            current_play_index: 0,
            music_info: DisplayAudioInfo::default(),
            quality: AudioQuality::default(),
            analysis: None,
            analysis_freq_min: 80.0,
            analysis_freq_max: 2000.0,
            seq: 0,
        }
    }

    /// Process an `AudioThreadEventMessage<AudioThreadMessage>` JSON envelope.
    ///
    /// Returns `{ events, effects }` as JSON. Parse failures are returned as a
    /// `loadError` event instead of panicking across the WASM boundary.
    #[wasm_bindgen(js_name = "sendMessageJson")]
    pub fn send_message_json(&mut self, envelope_json: &str) -> String {
        let envelope = match serde_json::from_str::<AudioThreadEventMessage<AudioThreadMessage>>(
            envelope_json,
        ) {
            Ok(envelope) => envelope,
            Err(err) => {
                return self.reply(
                    vec![AudioThreadEvent::LoadError {
                        error: format!("invalid audio message JSON: {err}"),
                    }],
                    Vec::new(),
                );
            }
        };

        let Some(message) = envelope.data else {
            return self.reply(Vec::new(), Vec::new());
        };

        self.handle_message(message)
    }

    /// Feed browser metadata once the HTML media element has loaded.
    #[wasm_bindgen(js_name = "applyLoadedTrack")]
    pub fn apply_loaded_track(
        &mut self,
        duration: f64,
        sample_rate: u32,
        channels: u16,
        bitrate: u32,
    ) -> String {
        let analysis_quality = self
            .analysis
            .as_ref()
            .map(|analysis| (analysis.duration, analysis.sample_rate, analysis.channels));
        let (duration, sample_rate, channels) = analysis_quality.unwrap_or_else(|| {
            (
                finite_nonnegative(duration),
                sample_rate.max(1),
                channels.max(1),
            )
        });

        self.duration = duration;
        self.music_info.duration = self.duration;
        self.music_info.position = self.position;
        self.quality = AudioQuality {
            bitrate,
            sample_rate,
            channels,
        };

        let mut events = vec![AudioThreadEvent::LoadAudio {
            music_id: self.current_music_id(),
            music_info: self.music_info.clone(),
            quality: self.quality.clone(),
            current_play_index: self.current_play_index,
        }];

        events.push(self.sync_status_event());
        if self.state == PlaybackState::Playing {
            events.push(AudioThreadEvent::PlayStatus { is_playing: true });
        }

        self.reply(events, Vec::new())
    }

    /// Decode browser-fetched audio bytes for the WASM analysis path.
    ///
    /// Playback remains owned by the browser media host; this sidechain feeds
    /// the same Rust `audio-analysis` processor used by native playback.
    #[wasm_bindgen(js_name = "loadAnalysisBytes")]
    pub fn load_analysis_bytes(
        &mut self,
        bytes: Vec<u8>,
        extension: String,
        music_id: String,
    ) -> String {
        if music_id != self.current_music_id() {
            return self.reply(Vec::new(), Vec::new());
        }

        let decoded = match decode_audio_bytes(bytes, &extension) {
            Ok(decoded) => decoded,
            Err(err) => {
                return format!(
                    r#"{{"events":[],"effects":[],"error":"{}"}}"#,
                    escape_json_string(&format!("WASM audio analysis decode failed: {err}"))
                );
            }
        };

        self.duration = decoded.duration;
        self.music_info.duration = decoded.duration;
        self.music_info.position = self.position;
        self.quality = AudioQuality {
            bitrate: 0,
            sample_rate: decoded.sample_rate.max(1),
            channels: decoded.channels.max(1),
        };
        self.analysis = Some(AnalysisState::new(
            decoded,
            self.analysis_freq_min,
            self.analysis_freq_max,
        ));

        self.reply(
            vec![
                AudioThreadEvent::LoadAudio {
                    music_id: self.current_music_id(),
                    music_info: self.music_info.clone(),
                    quality: self.quality.clone(),
                    current_play_index: self.current_play_index,
                },
                self.sync_status_event(),
            ],
            Vec::new(),
        )
    }

    /// Decode browser-fetched audio bytes into mono PCM for offline AutoMix analysis.
    ///
    /// This is intentionally state-independent: AutoMix analysis must not mutate
    /// the playback backend's playlist/current-track state.
    #[wasm_bindgen(js_name = "decodeAudioBytes")]
    pub fn decode_audio_bytes(
        &self,
        bytes: Vec<u8>,
        extension: String,
    ) -> Result<DecodedAudioJs, JsValue> {
        decode_audio_bytes(bytes, &extension)
            .map(|decoded| DecodedAudioJs {
                samples: decoded.samples,
                sample_rate: decoded.sample_rate,
                channels: decoded.channels,
                duration: decoded.duration,
            })
            .map_err(|err| JsValue::from_str(&err))
    }

    #[wasm_bindgen(js_name = "processAnalysisFrame")]
    pub fn process_analysis_frame(&mut self, position: f64, delta_ms: f64) -> String {
        self.position = finite_nonnegative(position);
        self.music_info.position = self.position;

        let Some(analysis) = self.analysis.as_mut() else {
            return self.reply(Vec::new(), Vec::new());
        };

        let low_freq = analysis.process(self.position, delta_ms);
        let fft_seq = self.next_seq();
        let low_freq_seq = self.next_seq();
        let spectrum = &self.analysis.as_ref().expect("analysis exists").spectrum;
        let reply = AnalysisFrameReply {
            events: [
                AnalysisEventMessage {
                    callback_id: "",
                    data: AnalysisFrameEvent::FftData { data: spectrum },
                    seq: fft_seq,
                },
                AnalysisEventMessage {
                    callback_id: "",
                    data: AnalysisFrameEvent::LowFrequencyVolume { volume: low_freq },
                    seq: low_freq_seq,
                },
            ],
            effects: [],
        };

        serde_json::to_string(&reply)
            .unwrap_or_else(|err| format!(r#"{{"events":[],"effects":[],"error":"{err}"}}"#))
    }

    #[wasm_bindgen(js_name = "applyPlaybackState")]
    pub fn apply_playback_state(&mut self, is_playing: bool) -> String {
        self.state = if is_playing {
            PlaybackState::Playing
        } else {
            PlaybackState::Paused
        };
        self.reply(
            vec![AudioThreadEvent::PlayStatus { is_playing }],
            Vec::new(),
        )
    }

    #[wasm_bindgen(js_name = "applyPlayPosition")]
    pub fn apply_play_position(&mut self, position: f64) -> String {
        self.position = finite_nonnegative(position);
        self.music_info.position = self.position;
        self.reply(
            vec![AudioThreadEvent::PlayPosition {
                position: self.position,
            }],
            Vec::new(),
        )
    }

    #[wasm_bindgen(js_name = "applyPlaybackFinished")]
    pub fn apply_playback_finished(&mut self) -> String {
        self.state = PlaybackState::Ended;
        self.position = self.duration;
        self.music_info.position = self.position;
        self.reply(
            vec![AudioThreadEvent::AudioPlayFinished {
                music_id: self.current_music_id(),
            }],
            Vec::new(),
        )
    }

    #[wasm_bindgen(js_name = "applyVolume")]
    pub fn apply_volume(&mut self, volume: f64) -> String {
        self.volume = volume.clamp(0.0, 1.0);
        self.reply(
            vec![AudioThreadEvent::VolumeChanged {
                volume: self.volume,
            }],
            Vec::new(),
        )
    }

    #[wasm_bindgen(js_name = "applyLoadError")]
    pub fn apply_load_error(&mut self, error: String) -> String {
        self.reply(vec![AudioThreadEvent::LoadError { error }], Vec::new())
    }

    #[wasm_bindgen(js_name = "applyPlayError")]
    pub fn apply_play_error(&mut self, error: String) -> String {
        self.state = PlaybackState::Paused;
        self.reply(vec![AudioThreadEvent::PlayError { error }], Vec::new())
    }

    #[wasm_bindgen(js_name = "syncStatusJson")]
    pub fn sync_status_json(&mut self) -> String {
        self.reply(vec![self.sync_status_event()], Vec::new())
    }

    #[wasm_bindgen(js_name = "stateJson")]
    pub fn state_json(&self) -> String {
        let state = match self.state {
            PlaybackState::Stopped => "stopped",
            PlaybackState::Playing => "playing",
            PlaybackState::Paused => "paused",
            PlaybackState::Ended => "ended",
        };

        serde_json::json!({
            "state": state,
            "isPlaying": self.state == PlaybackState::Playing,
            "position": self.position,
            "duration": self.duration,
            "volume": self.volume,
            "playlistInited": self.playlist_inited,
            "currentPlayIndex": self.current_play_index,
            "musicId": self.current_music_id(),
        })
        .to_string()
    }
}

impl WasmAudioBackend {
    fn handle_message(&mut self, message: AudioThreadMessage) -> String {
        match message {
            AudioThreadMessage::ResumeAudio => self.reply(Vec::new(), vec![WebBackendEffect::Play]),
            AudioThreadMessage::PauseAudio => self.reply(Vec::new(), vec![WebBackendEffect::Pause]),
            AudioThreadMessage::ResumeOrPauseAudio => {
                if self.state == PlaybackState::Playing {
                    self.handle_message(AudioThreadMessage::PauseAudio)
                } else {
                    self.handle_message(AudioThreadMessage::ResumeAudio)
                }
            }
            AudioThreadMessage::SeekAudio { position, .. } => {
                self.position = finite_nonnegative(position);
                self.music_info.position = self.position;
                self.reply(
                    vec![AudioThreadEvent::PlayPosition {
                        position: self.position,
                    }],
                    vec![WebBackendEffect::Seek {
                        position: self.position,
                    }],
                )
            }
            AudioThreadMessage::JumpToSong { song_index } => self.start_track(song_index, 0.0),
            AudioThreadMessage::JumpToSongAt {
                song_index,
                position,
            } => self.start_track(song_index, position),
            AudioThreadMessage::PrevSong => {
                if self.playlist.is_empty() {
                    self.reply(Vec::new(), Vec::new())
                } else {
                    let next = self
                        .current_play_index
                        .checked_sub(1)
                        .unwrap_or(self.playlist.len() - 1);
                    self.start_track(next, 0.0)
                }
            }
            AudioThreadMessage::NextSong | AudioThreadMessage::NextSongGapless => {
                if self.playlist.is_empty() {
                    self.reply(Vec::new(), Vec::new())
                } else {
                    self.start_track((self.current_play_index + 1) % self.playlist.len(), 0.0)
                }
            }
            // `windowed` is a native-queue hint (Tauri prefill windows); the web
            // backend never auto-advances, so positional semantics are enough.
            AudioThreadMessage::SetPlaylist { songs, .. } => {
                self.playlist = songs;
                self.playlist_inited = true;
                if self.current_play_index >= self.playlist.len() {
                    self.current_play_index = 0;
                }
                self.reply(
                    vec![AudioThreadEvent::PlayListChanged {
                        playlist: self.playlist.clone(),
                        current_play_index: self.current_play_index,
                    }],
                    Vec::new(),
                )
            }
            AudioThreadMessage::SetVolume { volume } => {
                self.volume = volume.clamp(0.0, 1.0);
                self.reply(
                    vec![AudioThreadEvent::VolumeChanged {
                        volume: self.volume,
                    }],
                    vec![WebBackendEffect::SetVolume {
                        volume: self.volume,
                    }],
                )
            }
            AudioThreadMessage::SetVolumeRelative { volume } => {
                self.volume = (self.volume + volume).clamp(0.0, 1.0);
                self.reply(
                    vec![AudioThreadEvent::VolumeChanged {
                        volume: self.volume,
                    }],
                    vec![WebBackendEffect::SetVolume {
                        volume: self.volume,
                    }],
                )
            }
            AudioThreadMessage::SetFFT { enabled } => {
                if !enabled {
                    if let Some(analysis) = self.analysis.as_mut() {
                        analysis.clear();
                    }
                }
                self.reply(Vec::new(), Vec::new())
            }
            AudioThreadMessage::SetFFTRange { from_freq, to_freq } => {
                self.analysis_freq_min = from_freq;
                self.analysis_freq_max = to_freq;
                if let Some(analysis) = self.analysis.as_mut() {
                    analysis.set_freq_range(from_freq, to_freq);
                }
                self.reply(Vec::new(), Vec::new())
            }
            AudioThreadMessage::SetAudioOutput { name } => {
                self.reply(Vec::new(), vec![WebBackendEffect::SetOutputDevice { name }])
            }
            AudioThreadMessage::SetAnalysis { .. }
            | AudioThreadMessage::SetEqualizer { .. }
            | AudioThreadMessage::SetDsp { .. }
            | AudioThreadMessage::SetMediaControlsEnabled { .. }
            | AudioThreadMessage::AutomixSetEnabled { .. }
            | AudioThreadMessage::AutomixConfigure { .. }
            | AudioThreadMessage::AutomixPrepareNext { .. }
            | AudioThreadMessage::AutomixCancel
            | AudioThreadMessage::AutomixForceStart { .. }
            | AudioThreadMessage::AutomixCompleteNative { .. } => {
                self.reply(Vec::new(), Vec::new())
            }
            AudioThreadMessage::SyncStatus => {
                self.reply(vec![self.sync_status_event()], Vec::new())
            }
            AudioThreadMessage::Close => {
                self.state = PlaybackState::Stopped;
                self.position = 0.0;
                self.duration = 0.0;
                self.load_position = 0.0;
                self.playlist.clear();
                self.playlist_inited = false;
                self.current_play_index = 0;
                self.music_info = DisplayAudioInfo::default();
                self.quality = AudioQuality::default();
                self.analysis = None;
                self.reply(
                    vec![AudioThreadEvent::PlayStatus { is_playing: false }],
                    vec![WebBackendEffect::Close],
                )
            }
        }
    }

    fn start_track(&mut self, song_index: usize, initial_position: f64) -> String {
        let Some(song) = self.playlist.get(song_index).cloned() else {
            return self.reply(
                vec![AudioThreadEvent::LoadError {
                    error: format!("invalid playlist index: {song_index}"),
                }],
                Vec::new(),
            );
        };

        let Some(src) = song.file_path().map(ToOwned::to_owned) else {
            return self.reply(
                vec![AudioThreadEvent::LoadError {
                    error: "web audio backend only supports local/filePath song entries".into(),
                }],
                Vec::new(),
            );
        };

        self.current_play_index = song_index;
        self.position = finite_nonnegative(initial_position);
        self.load_position = self.position;
        self.duration = 0.0;
        self.analysis = None;
        self.music_info = DisplayAudioInfo {
            name: infer_name_from_src(&src),
            duration: 0.0,
            position: self.position,
            ..Default::default()
        };
        self.quality = AudioQuality::default();

        let music_id = song.get_id();
        self.reply(
            vec![AudioThreadEvent::LoadingAudio {
                music_id: music_id.clone(),
                current_play_index: self.current_play_index,
            }],
            vec![WebBackendEffect::LoadTrack {
                src,
                initial_position: self.position,
                music_id,
                current_play_index: self.current_play_index,
            }],
        )
    }

    fn sync_status_event(&self) -> AudioThreadEvent {
        AudioThreadEvent::SyncStatus {
            music_id: self.current_music_id(),
            music_info: self.music_info.clone(),
            is_playing: self.state == PlaybackState::Playing,
            duration: self.duration,
            position: self.position,
            volume: self.volume,
            load_position: self.load_position,
            playlist: self.playlist.clone(),
            current_play_index: self.current_play_index,
            playlist_inited: self.playlist_inited,
            quality: self.quality.clone(),
        }
    }

    fn current_music_id(&self) -> String {
        self.playlist
            .get(self.current_play_index)
            .map(SongData::get_id)
            .unwrap_or_default()
    }

    fn reply(&mut self, events: Vec<AudioThreadEvent>, effects: Vec<WebBackendEffect>) -> String {
        let events = events
            .into_iter()
            .map(|event| self.wrap_event(event))
            .collect::<Vec<_>>();

        serde_json::to_string(&WebBackendReply { events, effects })
            .unwrap_or_else(|err| format!(r#"{{"events":[],"effects":[],"error":"{err}"}}"#))
    }

    fn wrap_event(&mut self, event: AudioThreadEvent) -> AudioThreadEventMessage<AudioThreadEvent> {
        AudioThreadEventMessage {
            callback_id: String::new(),
            data: Some(event),
            seq: self.next_seq(),
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.wrapping_add(1).max(1);
        self.seq
    }
}

impl Default for WasmAudioBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn infer_name_from_src(src: &str) -> String {
    src.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

fn sample_index_for_position(position: f64, sample_rate: u32, sample_count: usize) -> usize {
    let raw = finite_nonnegative(position) * sample_rate.max(1) as f64;
    if !raw.is_finite() || raw <= 0.0 {
        return 0;
    }
    raw.floor().min(sample_count as f64) as usize
}

fn decode_audio_bytes(bytes: Vec<u8>, extension: &str) -> Result<DecodedAudio, String> {
    if bytes.is_empty() {
        return Err("empty audio data".into());
    }

    let cursor = Cursor::new(bytes);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let mut hint = Hint::new();
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    if !extension.is_empty() {
        hint.with_extension(&extension);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|err| err.to_string())?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "no supported audio track".to_string())?
        .clone();
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|err| err.to_string())?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100).max(1);
    let mut channels = track
        .codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .unwrap_or(1)
        .max(1);
    let sample_capacity = track
        .codec_params
        .n_frames
        .and_then(|frames| usize::try_from(frames).ok())
        .unwrap_or(0);
    let mut samples = Vec::<f32>::with_capacity(sample_capacity);
    let mut sample_buf = None::<SampleBuffer<f32>>;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::ResetRequired) => break,
            Err(err) => return Err(err.to_string()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                sample_rate = spec.rate.max(1);
                channels = (spec.channels.count() as u16).max(1);

                let channel_count = channels as usize;
                let required_samples = decoded.capacity().saturating_mul(channel_count);
                let needs_buffer = sample_buf
                    .as_ref()
                    .map(|buf| buf.capacity() < required_samples)
                    .unwrap_or(true);
                if needs_buffer {
                    sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
                }
                let sample_buf = sample_buf.as_mut().expect("sample buffer initialized");
                sample_buf.copy_interleaved_ref(decoded);
                let decoded_samples = sample_buf.samples();

                if channel_count == 1 {
                    samples.extend_from_slice(decoded_samples);
                } else if channel_count == 2 {
                    samples.extend(
                        decoded_samples
                            .chunks_exact(2)
                            .map(|frame| (frame[0] + frame[1]) * 0.5),
                    );
                } else {
                    let inv_channels = 1.0 / channel_count as f32;
                    for frame in decoded_samples.chunks_exact(channel_count) {
                        samples.push(frame.iter().copied().sum::<f32>() * inv_channels);
                    }
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(err.to_string()),
        }
    }

    if samples.is_empty() {
        return Err("no decodable audio samples".into());
    }

    let duration = samples.len() as f64 / sample_rate.max(1) as f64;
    Ok(DecodedAudio {
        samples,
        sample_rate,
        channels,
        duration,
    })
}

fn escape_json_string(value: &str) -> String {
    serde_json::to_string(value)
        .map(|quoted| quoted.trim_matches('"').to_string())
        .unwrap_or_else(|_| "unknown error".to_string())
}
