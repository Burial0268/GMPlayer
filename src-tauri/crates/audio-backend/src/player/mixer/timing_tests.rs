use super::*;
use crate::output::{test_support::TestOutput, PcmPosition};

fn deck_with_timing(
    blocks: Vec<(usize, f64, f64)>,
    channels: usize,
    id: u64,
    gain: f32,
) -> DeckRuntime {
    let (tx, rx) = mpsc::sync_channel(blocks.len().max(1));
    let (recycle, _) = mpsc::sync_channel(64);
    let mut queued = 0;
    for (frames, seconds, rate) in blocks {
        queued += frames * channels;
        tx.send(DeckBlock {
            samples: vec![if id == 1 { 0.4 } else { 0.2 }; frames * channels],
            generation: 0,
            flush_epoch: 0,
            position: Some(PcmPosition {
                playback_id: id,
                seconds,
                rate,
            }),
        })
        .unwrap();
    }
    DeckRuntime::new(
        rx,
        Arc::new(AtomicUsize::new(queued)),
        Arc::new(AtomicBool::new(false)),
        recycle,
        gain,
    )
}

fn worker(
    primary: DeckRuntime,
    secondary: DeckRuntime,
    channels: usize,
) -> (MixerWorker, TestOutput) {
    let output = TestOutput::new(channels as u16, 48_000, 8);
    let (_, rx) = mpsc::channel();
    let worker = MixerWorker {
        output: output.writer.clone(),
        output_channels: channels,
        primary,
        secondary,
        crossfade: None,
        scheduled: None,
        transition: 0,
        phase: 0,
        expected_timeline: None,
        last_phase: (0, 0),
        block_anchor: None,
        block_target: MIX_BLOCK_FRAMES * channels,
        sample_rate: 48_000,
        dsp: DspChain::new(48_000, channels as u16),
        control_rx: rx,
        control_epoch: Arc::new(AtomicU64::new(0)),
        stop_flag: Arc::new(AtomicBool::new(false)),
        spare_block: None,
        underruns_in_window: 0,
        underrun_window_start: None,
    };
    (worker, output)
}

#[test]
fn scheduled_fade_and_handoff_reach_the_device_at_exact_frame_boundaries() {
    for channels in [1, 2, 6] {
        let start = 333.0 / 48_000.0;
        let primary = deck_with_timing(
            vec![(64, 0.0, 0.0), (333, 0.0, 1.0), (4000, start, 1.03)],
            channels,
            1,
            1.0,
        );
        let secondary = deck_with_timing(vec![(96, 0.0, 0.0), (4000, 0.0, 1.0)], channels, 2, 0.0);
        let (mut worker, mut output) = worker(primary, secondary, channels);
        worker.apply_control(MixerControl::StartCrossfade {
            outgoing: DeckId::Primary,
            incoming: DeckId::Secondary,
            duration_samples: 777 * channels,
            start_position: Some(start),
            transition: 12,
            params: CrossfadeParams::default(),
        });
        let mut frames = 0;
        let mut began = None;
        let mut completed = None;
        let stop = AtomicBool::new(false);
        let epoch = AtomicU64::new(0);
        for _ in 0..20 {
            let mut block = Vec::with_capacity(512 * channels);
            assert!(worker.mix_block(&mut block, channels, false, false));
            let count = block.len();
            let marker = worker.block_anchor;
            if let Some(marker) = marker {
                if marker.phase == 1 {
                    began.get_or_insert(frames);
                }
                if marker.phase == 2 {
                    completed.get_or_insert(frames);
                    assert_eq!(marker.source.playback_id, 2);
                    assert!((marker.source.seconds - 777.0 / 48_000.0).abs() < 1e-9);
                }
            }
            assert!(output.writer.push_block_with_anchor(
                block,
                marker,
                PushCancel::with_interrupt_epoch(&stop, &epoch, 0)
            ));
            output.render(count);
            frames += count / channels;
            if completed.is_some() {
                break;
            }
        }
        assert_eq!(began, Some(64 + 333));
        assert_eq!(completed, Some(64 + 333 + 777));
        let rendered = output.writer.render_clock().anchor().unwrap();
        assert_eq!(rendered.anchor.phase, 2);
        assert_eq!(rendered.rendered_at, ((64 + 333 + 777) * channels) as u64);
    }
}

#[test]
fn a_missing_incoming_deck_refuses_a_late_beat_and_cancel_keeps_the_active_generation() {
    let primary = deck_with_timing(vec![(4096, 0.0, 1.0)], 2, 1, 1.0);
    let secondary = deck_with_timing(Vec::new(), 2, 2, 0.0);
    let (mut worker, _) = worker(primary, secondary, 2);
    worker.apply_control(MixerControl::StartCrossfade {
        outgoing: DeckId::Primary,
        incoming: DeckId::Secondary,
        duration_samples: 2048,
        start_position: Some(100.0 / 48_000.0),
        transition: 9,
        params: CrossfadeParams::default(),
    });
    let mut block = Vec::new();
    worker.mix_block(&mut block, 2, false, false);
    block.clear();
    worker.mix_block(&mut block, 2, false, false);
    assert_eq!(worker.block_anchor.unwrap().phase, 3);
    assert!(worker.crossfade.is_none());
    let generation = worker.primary.accepted_generation;
    let (ack, _) = mpsc::channel();
    worker.apply_control(MixerControl::ClearDeck {
        deck: DeckId::Secondary,
        generation: 1,
        flush_epoch: 1,
        clear_output: false,
        ack,
    });
    assert_eq!(worker.primary.accepted_generation, generation);
    assert_eq!(worker.primary.gain, 1.0);
    assert!(worker.scheduled.is_none());
}

#[test]
fn pausing_before_a_queued_cue_freezes_start_and_handoff_until_resume() {
    for channels in [1, 2, 6] {
        let primary = deck_with_timing(vec![(4000, 0.0, 1.0)], channels, 1, 1.0);
        let secondary = deck_with_timing(vec![(4000, 0.0, 1.0)], channels, 2, 0.0);
        let (mut worker, mut output) = worker(primary, secondary, channels);
        worker.apply_control(MixerControl::StartCrossfade {
            outgoing: DeckId::Primary,
            incoming: DeckId::Secondary,
            duration_samples: 777 * channels,
            start_position: Some(333.0 / 48_000.0),
            transition: 12,
            params: CrossfadeParams::default(),
        });
        let stop = AtomicBool::new(false);
        let epoch = AtomicU64::new(0);
        // Queue the whole overlap before the device reaches its start.
        while worker.phase != 2 {
            let mut block = Vec::new();
            assert!(worker.mix_block(&mut block, channels, false, false));
            assert!(output.writer.push_block_with_anchor(
                block,
                worker.block_anchor,
                PushCancel::with_interrupt_epoch(&stop, &epoch, 0),
            ));
        }
        output.render(332 * channels);
        let clock = output.writer.render_clock();
        let before = clock.anchor().unwrap();
        let queued = output.writer.queued_samples();
        assert_eq!(before.anchor.phase, 0);
        output.writer.set_paused(true);
        // The callback decays from the last frame over its anti-click window.
        output.render(64 * channels);
        // Device callbacks continue for a minute while playback is paused.
        for _ in 0..60 {
            assert!(output.render(48_000 * channels).iter().all(|s| *s == 0.0));
            assert_eq!(clock.rendered_samples(), (332 * channels) as u64);
            assert_eq!(clock.anchor().unwrap().version, before.version);
            assert_eq!(output.writer.queued_samples(), queued);
        }
        output.writer.set_paused(false);
        output.render(channels);
        assert_eq!(clock.anchor().unwrap().anchor.phase, 0);
        output.render(channels);
        assert_eq!(clock.anchor().unwrap().anchor.phase, 1);
        assert_eq!(clock.anchor().unwrap().rendered_at, (333 * channels) as u64);
        assert_eq!(clock.anchor().unwrap().anchor.phase, 1);
    }
}
