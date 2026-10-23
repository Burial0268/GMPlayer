use super::*;

// The old per-sample queue path is deliberately independent of the bulk path.
fn mix_segment_reference(
    primary: &mut DeckRuntime,
    secondary: &mut DeckRuntime,
    block: &mut Vec<f32>,
    channels: usize,
    frames: usize,
    paused: (bool, bool),
    ramp: Option<CrossfadeBlockRamp>,
) -> (usize, usize) {
    let mut consumed = (0, 0);
    for frame in 0..frames {
        let gains = match ramp {
            Some(ramp) => {
                let (outgoing, incoming) = ramp.gains(frame);
                match ramp.outgoing {
                    DeckId::Primary => (outgoing, incoming),
                    DeckId::Secondary => (incoming, outgoing),
                }
            }
            None => (primary.gain, secondary.gain),
        };
        for _ in 0..channels {
            let p = if paused.0 {
                None
            } else {
                primary.next_sample(&mut consumed.0)
            };
            let s = if paused.1 {
                None
            } else {
                secondary.next_sample(&mut consumed.1)
            };
            block.push((p.unwrap_or(0.0) * gains.0 + s.unwrap_or(0.0) * gains.1).clamp(-1.0, 1.0));
        }
    }
    consumed
}

fn signal(frames: usize, channels: usize, phase: f32) -> Vec<f32> {
    (0..frames * channels)
        .map(|i| (i as f32 * 0.173 + phase).sin() * 0.85)
        .collect()
}

fn deck(samples: &[f32], block_samples: usize) -> DeckRuntime {
    let (tx, rx) = mpsc::sync_channel(samples.len() / block_samples + 1);
    for chunk in samples.chunks(block_samples) {
        tx.send(DeckBlock {
            samples: chunk.to_vec(),
            generation: 0,
            flush_epoch: 0,
        })
        .unwrap();
    }
    let (recycle_tx, _) = mpsc::sync_channel(1);
    DeckRuntime::new(
        rx,
        Arc::new(AtomicUsize::new(samples.len())),
        Arc::new(AtomicBool::new(false)),
        recycle_tx,
        1.0,
    )
}

fn ramp(outgoing: DeckId, active_frames: usize) -> CrossfadeBlockRamp {
    CrossfadeBlockRamp {
        outgoing,
        active_frames,
        outgoing_start: 0.9,
        incoming_start: 0.1,
        outgoing_step: -0.013,
        incoming_step: 0.017,
        outgoing_final: 0.0,
        incoming_final: 1.2,
    }
}

#[test]
fn reference_mixer_keeps_paused_decks_and_counts_only_pcm() {
    let mut primary = deck(&[0.5, -0.25], 2);
    let mut secondary = deck(&[0.25, 0.75], 2);
    let mut block = Vec::new();
    let consumed = mix_segment_reference(
        &mut primary,
        &mut secondary,
        &mut block,
        2,
        2,
        (false, true),
        None,
    );
    assert_eq!(block, [0.5, -0.25, 0.0, 0.0]);
    assert_eq!(consumed, (2, 0));
    assert_eq!(secondary.queued_samples.load(Ordering::Relaxed), 2);
}

#[test]
fn segmented_mixing_matches_per_sample_reference() {
    for channels in [1, 2, 6] {
        for paused in [(false, false), (true, false), (false, true), (true, true)] {
            for p_frames in [0, 17, 128] {
                for s_frames in [0, 23, 128] {
                    for ramp in [
                        None,
                        Some(ramp(DeckId::Primary, 64)),
                        Some(ramp(DeckId::Secondary, 64)),
                        Some(ramp(DeckId::Primary, 1)),
                        Some(ramp(DeckId::Secondary, 19)),
                    ] {
                        let p = signal(p_frames, channels, 0.0);
                        let s = signal(s_frames, channels, 1.3);
                        let mut primary = deck(&p, 7 * channels);
                        let mut secondary = deck(&s, 11 * channels);
                        let mut ref_primary = deck(&p, 7 * channels);
                        let mut ref_secondary = deck(&s, 11 * channels);
                        primary.gain = 1.2;
                        ref_primary.gain = 1.2;
                        secondary.gain = 0.85;
                        ref_secondary.gain = 0.85;
                        let mut actual = Vec::with_capacity(128 * channels);
                        let capacity = actual.capacity();
                        let mut expected = Vec::with_capacity(capacity);
                        for _ in 0..2 {
                            let consumed = mix_segment(
                                &mut primary,
                                &mut secondary,
                                &mut actual,
                                channels,
                                64,
                                paused,
                                ramp,
                            );
                            let ref_consumed = mix_segment_reference(
                                &mut ref_primary,
                                &mut ref_secondary,
                                &mut expected,
                                channels,
                                64,
                                paused,
                                ramp,
                            );
                            assert_eq!(consumed, ref_consumed);
                            assert_eq!(actual, expected);
                            assert_eq!(actual.capacity(), capacity);
                            primary.commit_consumed(consumed.0);
                            secondary.commit_consumed(consumed.1);
                            ref_primary.commit_consumed(ref_consumed.0);
                            ref_secondary.commit_consumed(ref_consumed.1);
                            assert_eq!(primary.current_index, ref_primary.current_index);
                            assert_eq!(secondary.current_index, ref_secondary.current_index);
                            assert_eq!(
                                primary.queued_samples.load(Ordering::Relaxed),
                                ref_primary.queued_samples.load(Ordering::Relaxed),
                            );
                            assert_eq!(
                                secondary.queued_samples.load(Ordering::Relaxed),
                                ref_secondary.queued_samples.load(Ordering::Relaxed),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn segmented_mixing_recycles_stale_and_empty_blocks() {
    let (tx, rx) = mpsc::sync_channel(8);
    let (recycle_tx, recycle_rx) = mpsc::sync_channel(8);
    for (samples, generation, flush_epoch) in [
        (vec![9.0; 4], 0, 2),
        (vec![9.0; 4], 1, 1),
        (Vec::with_capacity(4), 1, 2),
        (vec![0.5, -0.25], 1, 2),
        (vec![0.25, -0.5], 1, 2),
    ] {
        tx.send(DeckBlock {
            samples,
            generation,
            flush_epoch,
        })
        .unwrap();
    }
    let mut primary = DeckRuntime::new(
        rx,
        Arc::new(AtomicUsize::new(4)),
        Arc::new(AtomicBool::new(false)),
        recycle_tx,
        1.0,
    );
    primary.accepted_generation = 1;
    primary.accepted_flush_epoch = 2;
    let mut secondary = deck(&[], 1);
    let mut output = Vec::with_capacity(8);
    let consumed = mix_segment(
        &mut primary,
        &mut secondary,
        &mut output,
        2,
        4,
        (false, false),
        None,
    );
    assert_eq!(output, [0.5, -0.25, 0.25, -0.5, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(consumed, (4, 0));
    primary.commit_consumed(consumed.0);
    assert_eq!(primary.queued_samples.load(Ordering::Relaxed), 0);
    assert_eq!(recycle_rx.try_iter().count(), 4);

    // The same deck may refill after a starved segment, without a generation change.
    tx.send(DeckBlock {
        samples: vec![0.75, -0.75],
        generation: 1,
        flush_epoch: 2,
    })
    .unwrap();
    output.clear();
    assert_eq!(
        mix_segment(
            &mut primary,
            &mut secondary,
            &mut output,
            2,
            1,
            (false, false),
            None,
        ),
        (2, 0)
    );
    assert_eq!(output, [0.75, -0.75]);
}

#[test]
#[ignore = "manual release benchmark; run with --ignored --nocapture --test-threads=1"]
fn benchmark_mixing() {
    use std::hint::black_box;
    use std::time::Instant;

    for channels in [1, 2, 6] {
        let input = signal(MIX_BLOCK_FRAMES, channels, 0.0);
        let mut primary = deck(&[], 1);
        let mut secondary = deck(&[], 1);
        primary.current_block = input.clone();
        secondary.current_block = signal(MIX_BLOCK_FRAMES, channels, 0.5);
        let mut block = Vec::with_capacity(input.len());
        let ramp = Some(ramp(DeckId::Primary, CROSSFADE_RAMP_FRAMES));
        for reference in [true, false] {
            let start = Instant::now();
            for _ in 0..20_000 {
                primary.current_index = 0;
                secondary.current_index = 0;
                block.clear();
                for _ in 0..MIX_BLOCK_FRAMES / CROSSFADE_RAMP_FRAMES {
                    let consumed = if reference {
                        mix_segment_reference(
                            &mut primary,
                            &mut secondary,
                            &mut block,
                            black_box(channels),
                            CROSSFADE_RAMP_FRAMES,
                            (false, false),
                            black_box(ramp),
                        )
                    } else {
                        mix_segment(
                            &mut primary,
                            &mut secondary,
                            &mut block,
                            black_box(channels),
                            CROSSFADE_RAMP_FRAMES,
                            (false, false),
                            black_box(ramp),
                        )
                    };
                    black_box(consumed);
                }
                black_box(&block);
            }
            eprintln!(
                "mixer channels={channels} reference={reference}: {:?}",
                start.elapsed()
            );
        }
    }
}
