//! End-to-end level continuity through `MixerWorker::mix_block`: what a listener hears across a
//! fade and after it, not just what `fade_gains` returns.

use super::timing_tests::worker;
use super::*;

/// Deterministic white noise in `[-amplitude, amplitude]`.
fn noise(frames: usize, channels: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    (0..frames * channels)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
        })
        .collect()
}

fn deck(samples: &[f32], channels: usize, gain: f32) -> DeckRuntime {
    let block = MIX_BLOCK_FRAMES * channels;
    let blocks: Vec<_> = samples.chunks(block).map(<[f32]>::to_vec).collect();
    let (tx, rx) = mpsc::sync_channel(blocks.len().max(1));
    let (recycle, _) = mpsc::sync_channel(blocks.len().max(1));
    for samples in blocks {
        tx.send(DeckBlock {
            samples,
            generation: 0,
            flush_epoch: 0,
            position: None,
        })
        .unwrap();
    }
    DeckRuntime::new(
        rx,
        Arc::new(AtomicUsize::new(samples.len())),
        Arc::new(AtomicBool::new(false)),
        recycle,
        gain,
    )
}

/// Render a fade that starts at once and runs `fade_frames`, then `tail_frames` more. Returns the
/// output and the frame at which the fade completed.
fn render(
    outgoing: &[f32],
    incoming: &[f32],
    channels: usize,
    fade_frames: usize,
    tail_frames: usize,
    params: CrossfadeParams,
) -> (Vec<f32>, usize) {
    let primary = deck(outgoing, channels, params.outgoing_gain);
    let secondary = deck(incoming, channels, 0.0);
    let (mut worker, _output) = worker(primary, secondary, channels);
    worker.apply_control(MixerControl::StartCrossfade {
        outgoing: DeckId::Primary,
        incoming: DeckId::Secondary,
        duration_samples: fade_frames * channels,
        start_position: None,
        transition: 1,
        params,
    });
    let mut out = Vec::new();
    let mut completed = None;
    while out.len() < (fade_frames + tail_frames) * channels {
        let mut block = Vec::with_capacity(MIX_BLOCK_FRAMES * channels);
        if !worker.mix_block(&mut block, channels, false, false) {
            break;
        }
        out.extend_from_slice(&block);
        if completed.is_none() && worker.crossfade.is_none() {
            completed = Some(out.len() / channels);
        }
    }
    (out, completed.expect("the fade completes"))
}

/// The overlap limiter's gain reduction must release gradually once A has gone, not snap back to
/// unity on the first block after the fade: that snap was the "volume jump when the transition
/// finishes" — the single-deck fast path skipped the limiter entirely.
#[test]
fn the_overlap_limiter_releases_smoothly_after_the_fade_completes() {
    let channels = 2;
    let rate = 48_000;
    // A short fade whose outgoing side holds until 90%: the limiter is still pulling the sum down
    // when A lets go, so completion lands mid-release.
    let fade = rate / 10;
    let tail = rate / 2;
    // Loud enough that two balanced decks still overshoot the limiter's ceiling.
    let a = noise(fade + tail, channels, 0.95, 1);
    let b = noise(fade + tail, channels, 0.95, 2);
    let params = CrossfadeParams {
        style: FadeStyle::BeatMix(0.9),
        overlap_headroom_db: 0.0,
        ..CrossfadeParams::default()
    };
    let (out, completed) = render(&a, &b, channels, fade, tail, params);

    // After completion only B plays, so output / B is the limiter's gain, frame by frame.
    let gains: Vec<f32> = (completed..(out.len() / channels))
        .filter_map(|frame| {
            let i = frame * channels;
            (b[i].abs() > 0.05).then(|| out[i] / b[i])
        })
        .collect();
    assert!(gains.len() > 1000, "enough audible frames to judge");
    assert!(
        gains[0] < 0.97,
        "the overlap was limited, so the release has something to undo (gain {})",
        gains[0]
    );
    let worst_step = gains
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        worst_step < 0.01,
        "the release is gradual, worst step between frames {worst_step}"
    );
    assert!(
        (gains[gains.len() - 1] - 1.0).abs() < 0.01,
        "and it finishes at unity"
    );
}

/// What a listener hears through a balanced fade: the level moves from A's to B's without a bump
/// or a dip, and stays at B's once the fade is over. Equal-RMS noise on both decks, so the level
/// is the gains' doing; quiet enough that the limiter never acts.
#[test]
fn a_balanced_fade_moves_the_level_from_a_to_b_without_bumps() {
    let channels = 2;
    let rate = 48_000;
    let fade = rate * 2;
    let tail = rate / 2;
    let a = noise(fade + tail, channels, 0.3, 3);
    let b = noise(fade + tail, channels, 0.3, 4);
    let source_rms = 0.3 / 3f32.sqrt();
    let window = rate / 20 * channels; // 50 ms
    for style in [
        FadeStyle::Blend,
        FadeStyle::Handoff(0.6),
        FadeStyle::BeatMix(0.5),
    ] {
        let params = CrossfadeParams {
            style,
            outgoing_gain: 0.8,
            incoming_gain: 1.1,
            overlap_headroom_db: -1.2,
            ..CrossfadeParams::default()
        };
        let (out, completed) = render(&a, &b, channels, fade, tail, params);
        let levels: Vec<f32> = out
            .chunks_exact(window)
            .map(|w| {
                20.0 * ((w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt()).log10()
            })
            .collect();
        let worst = levels
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // Two independent noises sum to noise of the summed power, so the level follows the gains;
        // 50 ms windows of noise wander by a few tenths of a dB on their own.
        assert!(
            worst < 1.0,
            "{style:?}: a step of {worst} dB between 50 ms windows"
        );
        let lowest = levels.iter().copied().fold(f32::INFINITY, f32::min);
        let from = 20.0 * (0.8 * source_rms).log10();
        let to = 20.0 * (1.1 * source_rms).log10();
        assert!(
            lowest > from - 1.2 - 0.6,
            "{style:?}: dips to {lowest} dB, below A's level less the headroom"
        );
        // The old BeatMix stacked both decks to about +0.6 dB above B here, a bump that then fell
        // away as A left; 50 ms of noise is steady to about 0.1 dB.
        let highest = levels.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            highest < from.max(to) + 0.3,
            "{style:?}: swells to {highest} dB, above both A ({from}) and B ({to})"
        );
        let after = &levels[completed * channels / window + 1..];
        for level in after {
            assert!(
                (level - to).abs() < 0.5,
                "{style:?}: settles at {level}, not {to}"
            );
        }
    }
}
