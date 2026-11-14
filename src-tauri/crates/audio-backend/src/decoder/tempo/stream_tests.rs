use super::*;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn the_decode_worker_stretches_a_future_span_and_drains_every_sample_at_eof() {
    run(false);
}

#[test]
fn pause_and_cancel_preserve_source_time_then_return_to_unity() {
    run(true);
}

fn run(cancel_and_pause: bool) {
    use crate::output::test_support::TestOutput;
    let bytes = include_bytes!("../../../tests/fixtures/codecs/tone.wv");
    let mut file = tempfile::Builder::new().suffix(".wv").tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    let mut output = TestOutput::new(2, 48_000, 16);
    let (events, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (analysis, analysis_thread) = crate::analysis::spawn_analysis_thread(events).unwrap();
    let (decoder_events, mut decoder_rx) = tokio::sync::mpsc::unbounded_channel();
    let rate = 1.03;
    let processor = AutoMixStretch::new(2, 48_000, rate);
    let handle = crate::decoder::spawn_playback_decoder(
        file.path(),
        None,
        output.writer.clone(),
        2,
        48_000,
        analysis.clone(),
        Arc::new(AtomicBool::new(false)),
        decoder_events,
        42,
        false,
    )
    .unwrap();
    let ack = handle.arm_tempo(0.5, processor).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let wait = ack.wait();
        tokio::pin!(wait);
        loop {
            tokio::select! {
                result = &mut wait => { result.unwrap(); break; }
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    let queued = output.writer.queued_samples().min(256);
                    if queued > 0 { output.render(queued); }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut cancelled = false;
    let mut previous = 0.0;
    loop {
        if matches!(
            decoder_rx.try_recv(),
            Ok(crate::decoder::DecoderEvent::Finished { playback_id: 42 })
        ) {
            break;
        }
        let queued = output.writer.queued_samples().min(256);
        if queued > 0 {
            output.render(queued);
        } else {
            std::thread::yield_now();
        }
        let clock = output.writer.render_clock();
        if let Some(anchor) = clock.anchor() {
            let position = anchor.anchor.source.seconds
                + clock.rendered_samples().saturating_sub(anchor.rendered_at) as f64 / 96_000.0
                    * anchor.anchor.source.rate;
            assert!(
                position + 2.0 / 48_000.0 >= previous,
                "source clock went backwards: {previous} -> {position}"
            );
            previous = position;
            if cancel_and_pause && !cancelled && position >= 1.0 {
                output.writer.set_paused(true);
                let samples = clock.rendered_samples();
                output.render(2048);
                assert_eq!(clock.rendered_samples(), samples);
                handle.cancel_tempo();
                output.writer.set_paused(false);
                cancelled = true;
            }
        }
        assert!(Instant::now() < deadline, "decoder did not drain");
    }
    let clock = output.writer.render_clock();
    let samples = clock.rendered_samples();
    let expected_frames = 1024.0 + 24_000.0 + 72_000.0 / rate;
    if !cancel_and_pause {
        assert!(
            (samples as f64 / 2.0 - expected_frames).abs() <= 2.0,
            "samples={samples}, expected={expected_frames}"
        );
    } else {
        assert!(cancelled);
    }
    let anchor = clock.anchor().unwrap();
    if cancel_and_pause {
        assert_eq!(anchor.anchor.source.rate, 1.0);
    }
    let position = anchor.anchor.source.seconds
        + (samples - anchor.rendered_at) as f64 / 96_000.0 * anchor.anchor.source.rate;
    assert!(
        (position - 2.0).abs() < 2.0 / 48_000.0,
        "source position={position}"
    );
    drop(handle);
    drop(analysis);
    let _ = analysis_thread.join();
}
