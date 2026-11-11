//! AutoMix analysis API. PCM algorithms live independently of the playback engine.

#[cfg(target_arch = "wasm32")]
pub(crate) use gmplayer_automix_analysis::sections;
pub use gmplayer_automix_analysis::{
    analyze_mono_samples, BPMResult, BeatRegion, EnergyAnalysis, IntroAnalysis, MixPointAnalysis,
    MixPointCandidate, MultibandEnergy, OutroAnalysis, Phrase, PhraseAnalysis, SectionAnalysis,
    SongSection, SongSectionKind, SpectralFingerprint, TrackAnalysis, VocalActivityAnalysis,
    VolumeAnalysis,
};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod emotion;
#[cfg(not(target_arch = "wasm32"))]
mod file;
#[cfg(not(target_arch = "wasm32"))]
pub use file::{
    analyze_audio_bytes, analyze_audio_file, analyze_audio_file_until, analyze_audio_source,
    AutomixAnalyzeRequest, AutomixAnalyzeSourceRequest,
};
