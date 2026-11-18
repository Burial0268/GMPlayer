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
    let a = noise(fade + tail, channels, 0.7, 1);
    let b = noise(fade + tail, channels, 0.7, 2);
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
