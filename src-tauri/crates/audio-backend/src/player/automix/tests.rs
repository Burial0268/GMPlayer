use super::plan::build_analysis_backed_plan;
use super::*;
use crate::automix as automix_analysis;
use crate::automix::emotion::EmotionalArc;
use crate::automix::{
    EnergyAnalysis, SectionAnalysis, SongSection, SongSectionKind, SpectralFingerprint,
    TrackAnalysis, VolumeAnalysis,
};
use crate::types::CrossfadeCurve;
use std::path::Path;

fn section(kind: SongSectionKind, start: f32, end: f32, energy: f32) -> SongSection {
    SongSection {
        section_type: kind,
        start,
        end,
        index: 0,
        confidence: 0.7,
        energy,
        vocal_risk: 0.3,
        mix_suitability: 0.5,
    }
}

fn track(sections: Vec<SongSection>) -> TrackAnalysis {
    TrackAnalysis {
        volume: VolumeAnalysis {
            peak: 1.0,
            rms: 0.2,
            estimated_lufs: -14.0,
            gain_adjustment: 1.0,
        },
        energy: EnergyAnalysis {
            energy_per_second: vec![0.6; 200],
            outro_start_offset: 8.0,
            intro_end_offset: 2.0,
            average_energy: 0.6,
            trailing_silence: 0.0,
            is_fade_out: false,
        },
        bpm: None,
        fingerprint: SpectralFingerprint { bands: Vec::new() },
        outro: None,
        intro: None,
        phrases: None,
        sections: (!sections.is_empty()).then(|| SectionAnalysis {
            sections,
            confidence: 0.7,
            method: "noveltyRepetition".into(),
        }),
        vocal_activity: None,
        mix_candidates: None,
        duration: 200.0,
    }
}

fn outgoing() -> TrackAnalysis {
    track(vec![
        section(SongSectionKind::Intro, 0.0, 10.0, 0.3),
        section(SongSectionKind::Verse, 10.0, 100.0, 0.55),
        section(SongSectionKind::Chorus, 100.0, 180.0, 0.9),
        section(SongSectionKind::Outro, 180.0, 200.0, 0.35),
    ])
}

/// Opens straight on its chorus.
fn cold_open() -> TrackAnalysis {
    track(vec![
        section(SongSectionKind::Chorus, 0.0, 30.0, 0.9),
        section(SongSectionKind::Verse, 30.0, 200.0, 0.55),
    ])
}

fn config(emotional_transition: bool) -> AutoMixConfig {
    AutoMixConfig {
        enabled: true,
        emotional_transition,
        ..AutoMixConfig::default()
    }
}

#[test]
#[ignore = "set AUTOMIX_CURRENT_FILE and AUTOMIX_NEXT_FILE to inspect a real pair"]
fn inspect_real_pair() {
    let current =
        automix_analysis::analyze_audio_file(std::env::var("AUTOMIX_CURRENT_FILE").unwrap(), true)
            .unwrap();
    let next =
        automix_analysis::analyze_audio_file(std::env::var("AUTOMIX_NEXT_FILE").unwrap(), true)
            .unwrap();
    for (label, track) in [("current", &current), ("next", &next)] {
        println!(
            "{label}: duration={} trailing_silence={} outro={:?}",
            track.duration, track.energy.trailing_silence, track.outro
        );
        if let Ok(output) = std::env::var("AUTOMIX_PAIR_OUTPUT") {
            std::fs::write(
                Path::new(&output).join(format!("{label}.json")),
                serde_json::to_vec(track).unwrap(),
            )
            .unwrap();
        }
    }
    let duration = current.duration as f64;
    let plan = build_analysis_backed_plan(&config(true), duration, Some(&current), Some(&next));
    println!("plan={plan:?}");
}

#[test]
fn reconfiguring_for_a_new_attempt_does_not_replay_the_previous_failure() {
    let mut manager = AutoMixManager::new();
    manager.configure(config(false), 0);
    manager.mark_failed("previous track's preparation failed".into(), 0);
    assert_eq!(manager.status(0).state, AutoMixNativeState::Failed);
    manager.configure(config(false), 1);
    assert_eq!(manager.status(1).state, AutoMixNativeState::Idle);
    assert!(manager.status(1).error.is_none());
}

#[test]
fn cancellation_identifies_the_retired_attempt_only_in_its_event() {
    let mut manager = AutoMixManager::new();
    manager.configure(config(false), 0);
    for state in [AutoMixNativeState::Preparing, AutoMixNativeState::Waiting] {
        manager.state = state;
        manager.active_transition_id = Some(21);
        let events = manager.cancel(0);
        let [AudioThreadEvent::AutomixStatus { status }] = events.as_slice() else {
            panic!("expected one cancellation status");
        };
        assert_eq!(status.state, AutoMixNativeState::Idle);
        assert_eq!(status.transition_id, Some(21));
        assert_eq!(status.next_index, None);
        assert_eq!(manager.status(0).transition_id, None);
    }
}

#[test]
fn without_emotional_transitions_sections_change_nothing() {
    let next = cold_open();
    let with_sections =
        build_analysis_backed_plan(&config(false), 200.0, Some(&outgoing()), Some(&next));
    let without_sections =
        build_analysis_backed_plan(&config(false), 200.0, Some(&track(vec![])), Some(&next));

    assert_eq!(with_sections.start_time, without_sections.start_time);
    assert_eq!(with_sections.duration, without_sections.duration);
    assert_eq!(with_sections.curve, without_sections.curve);
    assert_eq!(
        with_sections.overlap_headroom_db,
        without_sections.overlap_headroom_db
    );
    assert_eq!(with_sections.section_cue, None);
    assert_eq!(with_sections.arc, EmotionalArc::Neutral);
    assert_eq!(
        (with_sections.in_shape, with_sections.out_shape),
        (1.0, 1.0)
    );
}

#[test]
fn the_overlap_finishes_before_trailing_silence() {
    for silence in [1.5, 11.4, 45.0] {
        let mut current = track(vec![]);
        current.energy.trailing_silence = silence;
        current.energy.outro_start_offset = silence + 8.0;
        let plan = build_analysis_backed_plan(&config(false), 200.0, Some(&current), None);
        let content_end = 200.0 - f64::from(silence);
        assert!(plan.start_time > 0.0);
        assert!(plan.duration >= 2.0);
        assert!(plan.start_time + plan.duration * plan.outgoing_rate <= content_end + 1e-5);
    }
}

#[test]
fn emotional_transitions_hand_over_after_the_last_chorus() {
    let plan =
        build_analysis_backed_plan(&config(true), 200.0, Some(&outgoing()), Some(&cold_open()));

    assert_eq!(plan.section_cue, Some(180.0));
    assert_eq!(plan.start_time, 180.0);
    // A quiet outro handing over to a song that opens on its chorus.
    assert_eq!(plan.arc, EmotionalArc::Lift);
    assert_eq!(plan.curve, CrossfadeCurve::EqualPower);
    assert!((plan.duration - 8.0 * 0.8).abs() < 1e-6);
    assert!((plan.in_shape - 1.2).abs() < 1e-6);
    assert!(plan.overlap_headroom_db <= -1.6);
}

#[test]
fn the_arc_never_stretches_past_the_configured_duration() {
    // Quiet outro into a quiet intro drifts, which wants a longer blend.
    let plan =
        build_analysis_backed_plan(&config(true), 200.0, Some(&outgoing()), Some(&outgoing()));

    assert_eq!(plan.arc, EmotionalArc::Drift);
    assert!(plan.duration <= AutoMixConfig::default().crossfade_duration);
}
