//! Bounded PCM access. Readers own storage and cancellation; analysis owns windows.

use crate::analysis::{bpm_from_window, finish_energy, volume_from_totals};
use crate::sections::{confident_tempo, segment_song, SectionHints, StructureFeatureExtractor};
use crate::{
    analysis, beats, complete_analysis, structure, vocal, TrackAnalysis, BPM_ANALYSIS_DURATION,
    BPM_ANALYSIS_RATE, SILENCE_THRESHOLD,
};

const BLOCK: usize = 16_384;

#[cfg(test)]
mod tests;

/// Exact mono samples, addressable without retaining the whole recording in RAM.
/// `read_at` fills the requested in-bounds range or returns the reader's error.
pub trait MonoPcm {
    type Error;
    fn sample_count(&self) -> usize;
    fn read_at(&mut self, first: usize, output: &mut [f32]) -> Result<(), Self::Error>;
}

/// Analyze nonempty PCM at a positive sample rate and duration. Working PCM is
/// limited to fixed-size blocks and at most 60 seconds plus filter warm-up.
/// Result arrays and structure features still scale with recording duration.
pub fn analyze_pcm<P: MonoPcm>(
    pcm: &mut P,
    sample_rate: u32,
    duration: f32,
    analyze_bpm: bool,
) -> Result<TrackAnalysis, P::Error> {
    let len = pcm.sample_count();
    let mut block = vec![0.0; BLOCK];
    let (mut peak, mut squares, mut second_squares, mut second_frames) = (0.0, 0.0, 0.0, 0usize);
    let mut raw_energy = Vec::new();
    for first in (0..len).step_by(BLOCK) {
        let count = BLOCK.min(len - first);
        pcm.read_at(first, &mut block[..count])?;
        for &sample in &block[..count] {
            let magnitude = if sample < 0.0 { -sample } else { sample };
            if magnitude > peak {
                peak = magnitude;
            }
            let square = sample as f64 * sample as f64;
            squares += square;
            second_squares += square;
            second_frames += 1;
            if second_frames == sample_rate as usize {
                raw_energy.push((second_squares / second_frames as f64).sqrt() as f32);
                second_squares = 0.0;
                second_frames = 0;
            }
        }
    }
    if second_frames > 0 {
        raw_energy.push((second_squares / second_frames as f64).sqrt() as f32);
    }
    raw_energy.resize(duration.ceil() as usize, 0.0);
    let trailing = trailing_silence(pcm, sample_rate, duration, &mut block)?;
    let energy = finish_energy(raw_energy, duration, trailing);
    let volume = volume_from_totals(peak, squares, len);
    let content_end = (duration - trailing).clamp(0.0, duration);
    let content_samples = ((content_end * sample_rate as f32) as usize).min(len);
    let mut extractor = (content_end >= crate::sections::MIN_STRUCTURE_SECONDS)
        .then(|| StructureFeatureExtractor::new(sample_rate));
    let mut vocal = (duration >= 4.0)
        .then(|| vocal::VocalAccumulator::new(sample_rate))
        .flatten();
    for first in (0..len).step_by(BLOCK) {
        let count = BLOCK.min(len - first);
        pcm.read_at(first, &mut block[..count])?;
        if let Some(vocal) = vocal.as_mut() {
            vocal.push(&block[..count]);
        }
        if let Some(extractor) = extractor.as_mut() {
            let take = content_samples.saturating_sub(first).min(count);
            extractor.push(&block[..take]);
        }
    }
    let vocal_activity = vocal.and_then(|vocal| vocal.finish(duration));
    let mut bpm = if analyze_bpm && duration >= 5.0 {
        let (samples, offset) = bpm_window(pcm, sample_rate)?;
        bpm_from_window(&samples, offset)
    } else {
        None
    };
    if let Some(bpm) = bpm.as_mut() {
        let frames = (beats::SECONDS * sample_rate as usize).min(len);
        bpm.head = beats::window_region(&window(pcm, 0, frames)?, sample_rate, 0, false);
        bpm.tail = beats::window_region(
            &window(pcm, len - frames, frames)?,
            sample_rate,
            len - frames,
            true,
        );
    }
    let sections = extractor.and_then(|extractor| {
        segment_song(
            &extractor.finish(),
            &SectionHints {
                duration,
                content_end,
                tempo: bpm
                    .as_ref()
                    .and_then(|bpm| confident_tempo(bpm.bpm, bpm.confidence)),
                vocal_activity: vocal_activity.as_ref(),
            },
        )
    });
    let intro = structure::analyze_intro(
        &energy.energy_per_second,
        energy.average_energy,
        &window(pcm, 0, (20 * sample_rate as usize).min(len))?,
        sample_rate,
        duration,
    );
    let outro = match structure::outro_sample_range(sample_rate, duration, trailing, len) {
        Some(range) => structure::analyze_outro_window(
            &window(pcm, range.start, range.len())?,
            sample_rate,
            duration,
            trailing,
            range.start,
        ),
        None => None,
    };
    let count = (len / 2048).min(8);
    let mut fingerprint_samples = vec![0.0; count * 2048];
    for i in 0..count {
        pcm.read_at(
            i * (len / count),
            &mut fingerprint_samples[i * 2048..(i + 1) * 2048],
        )?;
    }
    let fingerprint = analysis::compute_fingerprint(&fingerprint_samples, sample_rate);
    Ok(complete_analysis(TrackAnalysis {
        volume,
        energy,
        bpm,
        fingerprint,
        outro,
        intro,
        phrases: None,
        sections,
        vocal_activity,
        mix_candidates: None,
        duration,
    }))
}

fn window<P: MonoPcm>(pcm: &mut P, first: usize, count: usize) -> Result<Vec<f32>, P::Error> {
    let mut samples = vec![0.0; count];
    // A long window must remain cancellable at block boundaries too.
    for (i, chunk) in samples.chunks_mut(BLOCK).enumerate() {
        pcm.read_at(first + i * BLOCK, chunk)?;
    }
    Ok(samples)
}

fn trailing_silence<P: MonoPcm>(
    pcm: &mut P,
    rate: u32,
    duration: f32,
    block: &mut Vec<f32>,
) -> Result<f32, P::Error> {
    let len = pcm.sample_count();
    let width = ((rate as f32 * 0.1) as usize).min(len);
    if width == 0 {
        return Ok(0.0);
    }
    let mut end = len;
    let mut trailing = duration;
    'scan: while end > 0 {
        let windows = (end / width).min((BLOCK / width).max(1)).max(1);
        let first = end.saturating_sub(windows * width);
        block.resize(end - first, 0.0);
        pcm.read_at(first, block)?;
        for (i, window) in block.rchunks(width).enumerate() {
            let sum = window
                .iter()
                .fold(0.0f64, |sum, &sample| sum + sample as f64 * sample as f64);
            if (sum / window.len() as f64).sqrt() as f32 > SILENCE_THRESHOLD {
                trailing = (len - end + i * width) as f32 / rate as f32;
                break 'scan;
            }
        }
        end = first;
    }
    block.resize(BLOCK, 0.0);
    Ok((trailing * 10.0).round() / 10.0)
}

fn bpm_window<P: MonoPcm>(pcm: &mut P, rate: u32) -> Result<(Vec<f32>, f32), P::Error> {
    let ratio = rate as f32 / BPM_ANALYSIS_RATE as f32;
    let total = if rate == BPM_ANALYSIS_RATE {
        pcm.sample_count()
    } else {
        (pcm.sample_count() as f32 / ratio) as usize
    };
    let count = (BPM_ANALYSIS_DURATION * BPM_ANALYSIS_RATE as usize).min(total);
    let start = (total - count) / 2;
    let offset = start as f32 / BPM_ANALYSIS_RATE as f32;
    if count == 0 {
        return Ok((Vec::new(), offset));
    }
    if rate == BPM_ANALYSIS_RATE {
        return Ok((window(pcm, start, count)?, offset));
    }
    // Match the legacy f32 mapping at global indices, including its rounding.
    let first = (start as f32 * ratio) as usize;
    let last = ((start + count - 1) as f32 * ratio) as usize;
    let input = window(pcm, first, last + 1 - first)?;
    let samples = (start..start + count)
        .map(|i| input[(i as f32 * ratio) as usize - first])
        .collect();
    Ok((samples, offset))
}
