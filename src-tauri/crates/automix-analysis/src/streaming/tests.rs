use super::*;
use crate::analyze_mono_samples;

struct Samples<'a> {
    data: &'a [f32],
    reads: usize,
    largest_read: usize,
    fail_after: Option<usize>,
}
impl MonoPcm for Samples<'_> {
    type Error = &'static str;
    fn sample_count(&self) -> usize {
        self.data.len()
    }
    fn read_at(&mut self, first: usize, output: &mut [f32]) -> Result<(), Self::Error> {
        if self.fail_after == Some(self.reads) {
            return Err("reader cancelled or failed");
        }
        self.reads += 1;
        self.largest_read = self.largest_read.max(output.len());
        output.copy_from_slice(&self.data[first..first + output.len()]);
        Ok(())
    }
}

fn song(rate: u32, seconds: f32, silence: f32) -> Vec<f32> {
    (0..(rate as f32 * seconds) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            if t > seconds - silence {
                return 0.0;
            }
            let section = (t / 8.0) as usize % 4;
            let amplitude = [0.08, 0.22, 0.12, 0.22][section];
            let pitch = [220.0, 330.0, 275.0, 330.0][section];
            let pulse =
                (-((t % 0.5) / 0.012).powi(2)).exp() * (std::f32::consts::TAU * 110.0 * t).sin();
            amplitude * (std::f32::consts::TAU * pitch * t).sin() + 0.25 * pulse
        })
        .collect()
}

fn compare(data: &[f32], rate: u32, duration: f32, bpm: bool) {
    let expected = analyze_mono_samples(data, rate, duration, bpm);
    let mut source = Samples {
        data,
        reads: 0,
        largest_read: 0,
        fail_after: None,
    };
    let actual = analyze_pcm(&mut source, rate, duration, bpm).unwrap();
    assert_eq!(
        serde_json::to_value(&actual).unwrap(),
        serde_json::to_value(&expected).unwrap(),
        "all analysis fields must match at {rate} Hz / {duration} s / bpm={bpm}"
    );
    assert!(source.largest_read <= BLOCK.max((rate as f32 * 0.1) as usize));
}

#[test]
fn windows_preserve_complete_analysis_across_rates_and_partial_seconds() {
    for (rate, seconds) in [
        (8000, 5.17),
        (11025, 4.1),
        (22050, 21.17),
        (44100, 85.37),
        (48000, 65.333),
        (96000, 11.51),
        (192000, 5.17),
    ] {
        let data = song(rate, seconds, 3.17);
        for bpm in [false, true] {
            compare(&data, rate, seconds, bpm);
        }
    }
}

#[test]
fn silence_short_audio_and_duration_hints_match_the_reference() {
    for seconds in [0.031, 3.99, 25.17] {
        let data = vec![0.0; (11025.0 * seconds) as usize];
        compare(&data, 11025, seconds, true);
    }
    let data = song(22050, 31.37, 0.0);
    for duration in [31.243, 31.37, 31.583] {
        compare(&data, 22050, duration, true);
    }
}

#[test]
fn a_long_silent_tail_does_not_hide_the_audible_outro() {
    let data = song(11025, 100.17, 83.13);
    compare(&data, 11025, 100.17, true);
}

#[test]
fn a_silent_intro_does_not_turn_an_audible_ending_into_silence() {
    let rate = 11025;
    for tail in [0.0, 1.5, 6.0] {
        let mut data = song(rate, 6.0, tail);
        data[..rate as usize].fill(0.0);
        let mut source = Samples {
            data: &data,
            reads: 0,
            largest_read: 0,
            fail_after: None,
        };
        let streamed = analyze_pcm(&mut source, rate, 6.0, false).unwrap();
        let buffered = analyze_mono_samples(&data, rate, 6.0, false);
        for analysis in [streamed, buffered] {
            assert!(
                (analysis.energy.trailing_silence - tail).abs() < 0.11,
                "expected {tail}s of trailing silence, got {}",
                analysis.energy.trailing_silence
            );
        }
    }
}

#[test]
fn reader_failures_abort_early_and_late_analysis_work() {
    let data = song(11025, 6.0, 0.0);
    let mut successful = Samples {
        data: &data,
        reads: 0,
        largest_read: 0,
        fail_after: None,
    };
    analyze_pcm(&mut successful, 11025, 6.0, true).unwrap();
    for fail_after in [
        0,
        successful.reads / 3,
        successful.reads * 2 / 3,
        successful.reads - 1,
    ] {
        let mut source = Samples {
            data: &data,
            reads: 0,
            largest_read: 0,
            fail_after: Some(fail_after),
        };
        assert_eq!(
            analyze_pcm(&mut source, 11025, 6.0, true).unwrap_err(),
            "reader cancelled or failed"
        );
    }
}

#[test]
fn silence_scan_includes_the_partial_window_at_the_start() {
    for (len, audible, expected) in [(2050, 50, 2.0), (50, 50, 0.0), (50, 0, 0.1)] {
        let mut data = vec![0.0; len];
        data[..audible].fill(0.5);
        let mut source = Samples {
            data: &data,
            reads: 0,
            largest_read: 0,
            fail_after: None,
        };
        let duration = len as f32 / 1000.0;
        let streamed = trailing_silence(&mut source, 1000, duration, &mut Vec::new()).unwrap();
        let buffered = analysis::analyze_energy(&data, 1000, duration).trailing_silence;
        assert_eq!(streamed, expected);
        assert_eq!(buffered, expected);
    }
}
