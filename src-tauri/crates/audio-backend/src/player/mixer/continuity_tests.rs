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
    // Loud enough that two balanced decks still overshoot the limiter's ceiling, and B at its
    // matched gain keeps overshooting it after the fade: the limiter must still only release.
    let a = noise(fade + tail, channels, 0.95, 1);
    let b = noise(fade + tail, channels, 0.95, 2);
    let params = CrossfadeParams {
        style: FadeStyle::BeatMix(0.9),
        overlap_headroom_db: 0.0,
        incoming_gain: 1.5,
        ..CrossfadeParams::default()
    };
    let (out, completed) = render(&a, &b, channels, fade, tail, params);

    // After completion only B plays, so output / (B * its gain) is the limiter's, frame by frame.
    let gains: Vec<f32> = (completed..(out.len() / channels))
        .filter_map(|frame| {
            let i = frame * channels;
            (b[i].abs() > 0.05).then(|| out[i] / (b[i] * params.incoming_gain))
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
        gains.windows(2).all(|w| w[1] >= w[0] - 1e-5),
        "and only a release: B's own peaks never pull it down again"
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

/// Offline render of a real pair, for tuning by measurement and by ear:
///
/// ```text
/// AUTOMIX_CURRENT_FILE=a.flac AUTOMIX_NEXT_FILE=b.flac AUTOMIX_RENDER_OUT=/tmp/out \
///   cargo test -p gmplayer-audio-backend --lib render_real_pair -- --ignored --nocapture
/// ```
///
/// Analyses both files, builds the plan a native prepare would, and renders 10 s of A, the fade
/// and 10 s of B through `mix_block`. Writes `transition.wav` (the mix), `metrics.csv` (per 100 ms:
/// level of the mix and of each deck, overall and per band) and prints a summary. Not rendered:
/// the outgoing tempo stretch of a beat-synced plan (A plays at its own speed).
#[test]
#[ignore = "set AUTOMIX_CURRENT_FILE, AUTOMIX_NEXT_FILE and AUTOMIX_RENDER_OUT"]
fn render_real_pair() {
    use crate::player::automix::plan_for_pair;
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let (current_path, next_path) = (var("AUTOMIX_CURRENT_FILE"), var("AUTOMIX_NEXT_FILE"));
    let out_dir = std::path::PathBuf::from(var("AUTOMIX_RENDER_OUT"));
    std::fs::create_dir_all(&out_dir).unwrap();

    let current = crate::automix::analyze_audio_file(&current_path, true).unwrap();
    let next = crate::automix::analyze_audio_file(&next_path, true).unwrap();
    let config = crate::types::AutoMixConfig {
        enabled: true,
        ..Default::default()
    };
    let plan = plan_for_pair(&config, &current, &next);
    println!("plan = {plan:#?}");

    let (a, rate, channels) =
        crate::automix::decode_interleaved(std::path::Path::new(&current_path)).unwrap();
    let (b, b_rate, b_channels) =
        crate::automix::decode_interleaved(std::path::Path::new(&next_path)).unwrap();
    assert_eq!(
        (rate, channels),
        (b_rate, b_channels),
        "render needs matching formats"
    );

    // The params `automix_runtime` would arm, including its downgrade of an unsynced beat-mix.
    let mut style = plan.style;
    let mut bass_swap = plan.bass_swap;
    if !plan.beat_synced && matches!(style, FadeStyle::BeatMix(_)) {
        style = FadeStyle::Blend;
        bass_swap = bass_swap.map(|_| BassSwap::MIDDLE);
    }
    let params = CrossfadeParams {
        curve: plan.curve,
        incoming_gain: plan.incoming_gain_adjustment as f32,
        outgoing_gain: 1.0,
        overlap_headroom_db: plan.overlap_headroom_db as f32,
        in_shape: plan.in_shape as f32,
        out_shape: plan.out_shape as f32,
        bass_swap,
        morph: plan.morph,
        style,
    };

    let frame = |seconds: f64| (seconds.max(0.0) * f64::from(rate)) as usize * channels;
    let lead = 10.0;
    let a_from = frame(plan.start_time - lead).min(a.len());
    let b_from = frame(plan.incoming_start).min(b.len());
    let fade_frames = (plan.duration * f64::from(rate)) as usize;
    // A starts `lead` before the fade, or at its top when the fade starts sooner.
    let lead_frames = (plan.start_time.clamp(0.0, lead) * f64::from(rate)) as usize;
    let tail_frames = 10 * rate as usize;
    let total_frames = lead_frames + fade_frames + tail_frames;

    // Render the mix, then each deck alone (the mixer is linear apart from the safety limiter).
    let render_with = |a_on: bool, b_on: bool| {
        let a_part: Vec<f32> = a[a_from..]
            .iter()
            .map(|s| if a_on { *s } else { 0.0 })
            .collect();
        let b_part: Vec<f32> = b[b_from..]
            .iter()
            .map(|s| if b_on { *s } else { 0.0 })
            .collect();
        let primary = deck(&a_part, channels, params.outgoing_gain);
        let secondary = deck(&b_part, channels, 0.0);
        let (mut worker, _output) =
            super::timing_tests::worker_at(primary, secondary, channels, rate);
        let mut out = Vec::with_capacity(total_frames * channels);
        let mut started = false;
        while out.len() < total_frames * channels {
            if !started && out.len() >= lead_frames * channels {
                worker.apply_control(MixerControl::StartCrossfade {
                    outgoing: DeckId::Primary,
                    incoming: DeckId::Secondary,
                    duration_samples: fade_frames * channels,
                    start_position: None,
                    transition: 1,
                    params,
                });
                started = true;
            }
            let mut block = Vec::with_capacity(MIX_BLOCK_FRAMES * channels);
            if !worker.mix_block(&mut block, channels, false, false) {
                break;
            }
            out.extend_from_slice(&block);
        }
        out
    };
    let mix = render_with(true, true);
    let a_only = render_with(true, false);
    let b_only = render_with(false, true);

    write_wav(&out_dir.join("transition.wav"), &mix, rate, channels);

    // Per-100 ms levels, overall and per band (low < 300 Hz < mid < 4 kHz < high).
    let window = rate as usize / 10;
    let levels = |signal: &[f32]| band_levels(signal, rate, channels, window);
    let (mix_l, a_l, b_l) = (levels(&mix), levels(&a_only), levels(&b_only));
    let mut csv = String::from("t,mix,a,b,a_low,a_mid,a_high,b_low,b_mid,b_high\n");
    for i in 0..mix_l.len().min(a_l.len()).min(b_l.len()) {
        let t = i as f64 * 0.1 - lead_frames as f64 / f64::from(rate);
        csv += &format!(
            "{t:.1},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}\n",
            mix_l[i][0],
            a_l[i][0],
            b_l[i][0],
            a_l[i][1],
            a_l[i][2],
            a_l[i][3],
            b_l[i][1],
            b_l[i][2],
            b_l[i][3]
        );
    }
    std::fs::write(out_dir.join("metrics.csv"), csv).unwrap();

    // Summary: how the level moves across the fade, when B is heard, where the decks clash.
    let start_w = lead_frames / window;
    let end_w = (lead_frames + fade_frames) / window;
    let mean =
        |v: &[[f32; 4]], band: usize| v.iter().map(|l| l[band]).sum::<f32>() / v.len() as f32;
    let before = mean(&mix_l[start_w.saturating_sub(20)..start_w], 0);
    let after = mean(&mix_l[end_w + 10..(end_w + 30).min(mix_l.len())], 0);
    let fade = &mix_l[start_w..end_w];
    let deviation = fade
        .iter()
        .enumerate()
        .map(|(i, l)| l[0] - (before + (after - before) * i as f32 / fade.len() as f32))
        .fold((0.0f32, 0.0f32), |(lo, hi), d| (lo.min(d), hi.max(d)));
    let share = |pred: &dyn Fn(usize) -> bool| {
        (start_w..end_w)
            .find(|w| pred(*w))
            .map_or(1.0, |w| (w - start_w) as f32 / (end_w - start_w) as f32)
    };
    let heard = share(&|w| b_l[w][0] > a_l[w][0] - 10.0);
    let crossed = share(&|w| b_l[w][0] > a_l[w][0]);
    let clash = |band: usize| {
        (start_w..end_w)
            .filter(|w| {
                let (x, y) = (a_l[*w][band], b_l[*w][band]);
                x > -50.0 && y > -50.0 && (x - y).abs() < 6.0
            })
            .count() as f32
            * 0.1
    };
    // B's own level at points through the fade, and the jump in the second after completion.
    let b_at = |share: f32| b_l[start_w + ((end_w - start_w) as f32 * share) as usize][0];
    let step_after = mean(&mix_l[end_w..(end_w + 10).min(mix_l.len())], 0)
        - mean(&mix_l[end_w.saturating_sub(10)..end_w], 0);
    println!(
        "B level at 10/25/50% of the fade: {:.1} / {:.1} / {:.1} dB, \
         step in the 1 s after completion {:+.1} dB",
        b_at(0.10),
        b_at(0.25),
        b_at(0.50),
        step_after,
    );
    println!(
        "fade {:.1} s at {:.1} s ({:?}, beat_synced={})\n\
         level before {before:.1} dB, after {after:.1} dB, \
         deviation from a straight dB path {:.1}..{:+.1} dB\n\
         B heard (within 10 dB of A) at {:.0}% of the fade, louder than A at {:.0}%\n\
         both decks within 6 dB: low {:.1} s, mid {:.1} s, high {:.1} s",
        plan.duration,
        plan.start_time,
        params.style,
        plan.beat_synced,
        deviation.0,
        deviation.1,
        heard * 100.0,
        crossed * 100.0,
        clash(1),
        clash(2),
        clash(3),
    );
}

/// `[overall, low, mid, high]` RMS in dBFS per `window` frames of an interleaved signal.
fn band_levels(signal: &[f32], rate: u32, channels: usize, window: usize) -> Vec<[f32; 4]> {
    let rate = f64::from(rate);
    let mut low = Biquad::low_pass(rate, 300.0);
    let (mut mid_hp, mut mid_lp) = (
        Biquad::high_pass(rate, 300.0),
        Biquad::low_pass(rate, 4_000.0),
    );
    let mut high = Biquad::high_pass(rate, 4_000.0);
    let db = |sum: f64, n: usize| (10.0 * (sum / n.max(1) as f64 + 1e-12).log10()) as f32;
    signal
        .chunks_exact(channels)
        .collect::<Vec<_>>()
        .chunks(window)
        .filter(|w| w.len() == window)
        .map(|frames| {
            let mut sums = [0f64; 4];
            for frame in frames {
                let x = frame.iter().map(|s| f64::from(*s)).sum::<f64>() / channels as f64;
                let bands = [x, low.run(x), mid_lp.run(mid_hp.run(x)), high.run(x)];
                for (sum, value) in sums.iter_mut().zip(bands) {
                    *sum += value * value;
                }
            }
            sums.map(|sum| db(sum, frames.len()))
        })
        .collect()
}

/// RBJ biquad for measurement only.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn design(rate: f64, hz: f64, high: bool) -> Self {
        let w = std::f64::consts::TAU * hz / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let b = if high {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        } else {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        };
        Self {
            b: b.map(|x| x / a0),
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }
    fn low_pass(rate: f64, hz: f64) -> Self {
        Self::design(rate, hz, false)
    }
    fn high_pass(rate: f64, hz: f64) -> Self {
        Self::design(rate, hz, true)
    }
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32, channels: usize) {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}
