//! Song sections for AutoMix: where a song's parts begin and end, and which
//! part is the intro, a verse, the chorus, and so on.
//!
//! Boundaries are peaks of Foote novelty over a self-similarity matrix of
//! per-block harmony (chroma) and timbre. Names come from repetition: the
//! loudest part that recurs is the chorus, a unique or quiet opening is the
//! intro, a unique passage between late choruses is the bridge. It is a
//! heuristic for the big parts of a song, not a bar-level transcriber.
//!
//! Feature extraction streams, so the WASM build can be fed chunk by chunk
//! instead of copying a whole track into linear memory, which never shrinks.

use std::ops::Range;
use std::sync::Arc;

use rustfft::{num_complex::Complex, Fft, FftPlanner};

use super::structure::{section_mix_suitability, section_vocal_risk};
use super::{
    SectionAnalysis, SongSection, SongSectionKind, VocalActivityAnalysis, MAX_SONG_SECTIONS,
};

/// The FFT size is the power of two nearest this frame length, which keeps
/// bins ~5.5 Hz wide — about a semitone at 100 Hz — at any sample rate.
const FRAME_SECONDS: f32 = 0.18;
const MIN_FFT_SIZE: usize = 1024;
const MAX_FFT_SIZE: usize = 16_384;
/// Frames are averaged into blocks of about this length before comparing.
const BLOCK_SECONDS: f32 = 0.5;
const CHROMA_MIN_HZ: f32 = 100.0;
const CHROMA_MAX_HZ: f32 = 4_000.0;
const TIMBRE_EDGES_HZ: [f32; 9] = [
    40.0, 120.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
const TIMBRE_BANDS: usize = TIMBRE_EDGES_HZ.len() - 1;
/// Band shape, then loudness, then spectral flux.
const TIMBRE_DIMS: usize = TIMBRE_BANDS + 2;
const LOUDNESS_DIM: usize = TIMBRE_BANDS;
const FLUX_DIM: usize = TIMBRE_BANDS + 1;
const NO_BIN: u8 = u8::MAX;

/// Shorter content has no structure worth planning a transition around.
const MIN_STRUCTURE_SECONDS: f32 = 20.0;
const DEFAULT_KERNEL_SECONDS: f32 = 8.0;
const DEFAULT_MIN_SECTION_SECONDS: f32 = 6.0;
/// Two passages are the same part when their aligned blocks correlate this well.
const REPEAT_SIMILARITY: f32 = 0.45;
/// Novelty a boundary needs regardless of how the rest of the song scores.
/// On the synthetic song in the tests real boundaries score 0.23–0.60 and the
/// chord changes inside unchanging material at most 0.11.
const MIN_BOUNDARY_NOVELTY: f32 = 0.14;
const MAX_INTRO_SECONDS: f32 = 45.0;
const MAX_OUTRO_SECONDS: f32 = 60.0;

// ─── Feature extraction ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct FrameFeature {
    chroma: [f32; 12],
    /// Power per timbre band.
    bands: [f32; TIMBRE_BANDS],
    rms: f32,
    /// Spectral rise relative to the frame's total magnitude.
    flux: f32,
}

pub(crate) struct StructureFeatures {
    frame_seconds: f32,
    frames: Vec<FrameFeature>,
}

pub(crate) struct StructureFeatureExtractor {
    fft: Arc<dyn Fft<f32>>,
    frame_seconds: f32,
    window: Vec<f32>,
    pending: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    previous_magnitude: Vec<f32>,
    bin_pitch_class: Vec<u8>,
    bin_band: Vec<u8>,
    frames: Vec<FrameFeature>,
}

impl StructureFeatureExtractor {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        let fft_size = fft_size_for(sample_rate);
        let fft = FftPlanner::new().plan_fft_forward(fft_size);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let half = fft_size / 2;
        let bin_hz = sample_rate as f32 / fft_size as f32;

        let mut bin_pitch_class = vec![NO_BIN; half];
        let mut bin_band = vec![NO_BIN; half];
        for bin in 1..half {
            let hz = bin as f32 * bin_hz;
            if (CHROMA_MIN_HZ..=CHROMA_MAX_HZ).contains(&hz) {
                let midi = 69.0 + 12.0 * (hz / 440.0).log2();
                bin_pitch_class[bin] = (midi.round() as i32).rem_euclid(12) as u8;
            }
            if let Some(band) = TIMBRE_EDGES_HZ
                .windows(2)
                .position(|edge| hz >= edge[0] && hz < edge[1])
            {
                bin_band[bin] = band as u8;
            }
        }

        let window = (0..fft_size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / fft_size as f32).cos())
            .collect();

        Self {
            fft,
            frame_seconds: fft_size as f32 / sample_rate as f32,
            window,
            pending: Vec::with_capacity(fft_size),
            spectrum: vec![Complex::default(); fft_size],
            scratch,
            previous_magnitude: vec![0.0; half],
            bin_pitch_class,
            bin_band,
            frames: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, mut samples: &[f32]) {
        let fft_size = self.window.len();
        while !samples.is_empty() {
            let take = (fft_size - self.pending.len()).min(samples.len());
            self.pending.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.pending.len() == fft_size {
                self.process_pending();
            }
        }
    }

    pub(crate) fn finish(mut self) -> StructureFeatures {
        // A tail under half a frame is below block resolution anyway.
        if self.pending.len() * 2 >= self.window.len() {
            self.process_pending();
        }
        StructureFeatures {
            frame_seconds: self.frame_seconds,
            frames: self.frames,
        }
    }

    fn process_pending(&mut self) {
        let valid = self.pending.len().max(1);
        let mut sum_sq = 0.0f64;
        for (i, slot) in self.spectrum.iter_mut().enumerate() {
            let sample = self.pending.get(i).copied().unwrap_or(0.0);
            sum_sq += f64::from(sample) * f64::from(sample);
            *slot = Complex::new(sample * self.window[i], 0.0);
        }
        self.pending.clear();
        self.fft
            .process_with_scratch(&mut self.spectrum, &mut self.scratch);

        let mut feature = FrameFeature {
            rms: (sum_sq / valid as f64).sqrt() as f32,
            ..FrameFeature::default()
        };
        let mut rise = 0.0f32;
        let mut total = 0.0f32;
        for bin in 1..self.previous_magnitude.len() {
            let magnitude = self.spectrum[bin].norm_sqr().sqrt();
            let class = self.bin_pitch_class[bin];
            if class != NO_BIN {
                feature.chroma[class as usize] += magnitude;
            }
            let band = self.bin_band[bin];
            if band != NO_BIN {
                feature.bands[band as usize] += magnitude * magnitude;
            }
            rise += (magnitude - self.previous_magnitude[bin]).max(0.0);
            total += magnitude;
            self.previous_magnitude[bin] = magnitude;
        }
        // The first frame rises from nothing; that is not a change in the music.
        feature.flux = if self.frames.is_empty() || total <= 1e-9 {
            0.0
        } else {
            rise / total
        };
        self.frames.push(feature);
    }
}

fn fft_size_for(sample_rate: u32) -> usize {
    let target = (sample_rate as f32 * FRAME_SECONDS).round().max(1.0) as usize;
    let above = target.next_power_of_two();
    let below = above / 2;
    let nearest = if below > 0 && target - below < above - target {
        below
    } else {
        above
    };
    nearest.clamp(MIN_FFT_SIZE, MAX_FFT_SIZE)
}

// ─── Segmentation ──────────────────────────────────────────────────

pub(crate) struct SectionHints<'a> {
    pub duration: f32,
    /// End of audible content: `duration` minus trailing silence.
    pub content_end: f32,
    /// Tempo in BPM, only when the detector was confident about it.
    pub tempo: Option<f32>,
    pub vocal_activity: Option<&'a VocalActivityAnalysis>,
}

/// A tempo worth sizing sections by. The detector's beat grid carries no
/// phase, so tempo only scales the search — it never places a boundary.
pub(crate) fn confident_tempo(bpm: f32, confidence: f32) -> Option<f32> {
    (confidence >= 0.3 && bpm.is_finite() && (60.0..=200.0).contains(&bpm)).then_some(bpm)
}

pub(super) fn analyze_song_sections(
    samples: &[f32],
    sample_rate: u32,
    hints: &SectionHints,
) -> Option<SectionAnalysis> {
    if hints.content_end < MIN_STRUCTURE_SECONDS {
        return None;
    }
    let content_samples = ((hints.content_end * sample_rate as f32) as usize).min(samples.len());
    let mut extractor = StructureFeatureExtractor::new(sample_rate);
    extractor.push(&samples[..content_samples]);
    segment_song(&extractor.finish(), hints)
}

pub(crate) fn segment_song(
    features: &StructureFeatures,
    hints: &SectionHints,
) -> Option<SectionAnalysis> {
    let duration = hints.duration.max(0.0);
    let content_end = hints.content_end.clamp(0.0, duration);
    if content_end < MIN_STRUCTURE_SECONDS || features.frame_seconds <= 0.0 {
        return None;
    }

    let (blocks, block_seconds) = aggregate_blocks(features, content_end);
    let (kernel, min_section) = section_scales(hints.tempo, block_seconds);
    if blocks.len() < min_section * 2 {
        return None;
    }

    let descriptors = describe_blocks(&blocks);
    let novelty = novelty_curve(&descriptors, kernel);
    let boundaries = pick_boundaries(
        &novelty,
        (min_section / 2).max(2),
        min_section,
        MAX_SONG_SECTIONS.saturating_sub(2),
    );
    if boundaries.is_empty() {
        return None;
    }

    let segments = build_segments(&descriptors, &boundaries, block_seconds, content_end);
    let similarity = segments
        .iter()
        .map(|a| {
            segments
                .iter()
                .map(|b| passage_similarity(&descriptors, &a.blocks, &b.blocks))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let group_of = group_repeats(&similarity);
    let typical_energy = median(descriptors.iter().map(|d| d.energy).collect());
    let labels = label_segments(&segments, &group_of, typical_energy, content_end);

    let mut sections = Vec::with_capacity(segments.len() + 1);
    for (segment, (kind, confidence)) in segments.iter().zip(labels) {
        let boundary_strength = (segment.start_strength + segment.end_strength) / 2.0;
        let confidence = (confidence * (0.7 + 0.3 * boundary_strength)).clamp(0.0, 1.0);
        let vocal_risk =
            section_vocal_risk(hints.vocal_activity, segment.start, segment.end).unwrap_or(0.5);
        sections.push(SongSection {
            section_type: kind,
            start: segment.start,
            end: segment.end,
            index: 0,
            confidence,
            energy: segment.energy,
            vocal_risk,
            mix_suitability: section_mix_suitability(kind, segment.energy, confidence, vocal_risk),
        });
    }
    if duration - content_end >= 1.0 {
        let confidence = 0.9;
        sections.push(SongSection {
            section_type: SongSectionKind::Silence,
            start: content_end,
            end: duration,
            index: 0,
            confidence,
            energy: 0.0,
            vocal_risk: 0.0,
            mix_suitability: section_mix_suitability(
                SongSectionKind::Silence,
                0.0,
                confidence,
                0.0,
            ),
        });
    }

    let mut sections = merge_adjacent(sections);
    // Everything grouped into one part: there is no structure to plan with.
    let parts = sections
        .iter()
        .filter(|s| s.section_type != SongSectionKind::Silence)
        .count();
    if parts < 2 {
        return None;
    }
    sections.truncate(MAX_SONG_SECTIONS);
    for (index, section) in sections.iter_mut().enumerate() {
        section.index = index as u32;
    }

    let total = sections
        .iter()
        .map(|s| s.end - s.start)
        .sum::<f32>()
        .max(1e-3);
    let weighted = sections
        .iter()
        .map(|s| s.confidence * (s.end - s.start))
        .sum::<f32>()
        / total;
    let repeats = group_of
        .iter()
        .any(|&g| group_of.iter().filter(|&&other| other == g).count() >= 2);
    let confidence = (weighted * if repeats { 1.0 } else { 0.8 }).clamp(0.0, 1.0);

    Some(SectionAnalysis {
        sections,
        confidence,
        method: "noveltyRepetition".to_string(),
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct Block {
    chroma: [f32; 12],
    bands: [f32; TIMBRE_BANDS],
    rms: f32,
    flux: f32,
}

fn aggregate_blocks(features: &StructureFeatures, content_end: f32) -> (Vec<Block>, f32) {
    let frames_per_block = (BLOCK_SECONDS / features.frame_seconds).round().max(1.0) as usize;
    let block_seconds = frames_per_block as f32 * features.frame_seconds;
    let content_frames =
        ((content_end / features.frame_seconds).ceil() as usize).min(features.frames.len());

    let blocks = features.frames[..content_frames]
        .chunks(frames_per_block)
        .map(|chunk| {
            let mut block = Block::default();
            for frame in chunk {
                for (sum, value) in block.chroma.iter_mut().zip(frame.chroma) {
                    *sum += value;
                }
                for (sum, value) in block.bands.iter_mut().zip(frame.bands) {
                    *sum += value;
                }
                block.rms += frame.rms;
                block.flux += frame.flux;
            }
            let count = chunk.len() as f32;
            block.bands.iter_mut().for_each(|value| *value /= count);
            block.rms /= count;
            block.flux /= count;
            block
        })
        .collect();

    (blocks, block_seconds)
}

/// Kernel half-width and shortest section, in blocks. With a tempo, the
/// kernel spans a four-bar phrase each side and anything under two bars is
/// treated as a fill rather than a section.
fn section_scales(tempo: Option<f32>, block_seconds: f32) -> (usize, usize) {
    let (kernel_seconds, min_section_seconds) = match tempo {
        Some(bpm) => {
            let beat = 60.0 / bpm;
            ((16.0 * beat).clamp(6.0, 12.0), (8.0 * beat).clamp(4.5, 9.0))
        }
        None => (DEFAULT_KERNEL_SECONDS, DEFAULT_MIN_SECTION_SECONDS),
    };
    let to_blocks = |seconds: f32| (seconds / block_seconds).round().max(2.0) as usize;
    (to_blocks(kernel_seconds), to_blocks(min_section_seconds))
}

struct Descriptor {
    chroma: [f32; 12],
    timbre: [f32; TIMBRE_DIMS],
    /// RMS against the song's own 95th percentile, as `energy_per_second` is.
    energy: f32,
}

fn describe_blocks(blocks: &[Block]) -> Vec<Descriptor> {
    let count = blocks.len().max(1) as f32;

    // Centre chroma on the song's own average, or the key every part shares
    // makes all pairs look alike.
    let mut chroma = blocks.iter().map(|b| unit(b.chroma)).collect::<Vec<_>>();
    let mut mean_chroma = [0.0f32; 12];
    for vector in &chroma {
        for (sum, value) in mean_chroma.iter_mut().zip(vector) {
            *sum += value / count;
        }
    }
    for vector in &mut chroma {
        for (value, mean) in vector.iter_mut().zip(mean_chroma) {
            *value -= mean;
        }
        *vector = unit(*vector);
    }

    let mut timbre = blocks
        .iter()
        .map(|block| {
            let mut vector = [0.0f32; TIMBRE_DIMS];
            let db = block.bands.map(|power| 10.0 * (power + 1e-10).log10());
            let level = db.iter().sum::<f32>() / TIMBRE_BANDS as f32;
            for (slot, value) in vector.iter_mut().zip(db) {
                *slot = value - level;
            }
            vector[LOUDNESS_DIM] = 20.0 * (block.rms + 1e-6).log10();
            vector[FLUX_DIM] = block.flux;
            vector
        })
        .collect::<Vec<_>>();
    for dim in 0..TIMBRE_DIMS {
        // Floors keep a dimension that barely moves from being amplified
        // into noise; loudness counts for more than one band's shape.
        let (floor, weight) = match dim {
            LOUDNESS_DIM => (1.5, 1.5),
            FLUX_DIM => (0.01, 0.75),
            _ => (1.0, 1.0),
        };
        let mean = timbre.iter().map(|v| v[dim]).sum::<f32>() / count;
        let std = (timbre.iter().map(|v| (v[dim] - mean).powi(2)).sum::<f32>() / count)
            .sqrt()
            .max(floor);
        for vector in &mut timbre {
            vector[dim] = (vector[dim] - mean) / std * weight;
        }
    }

    let mut rms = blocks.iter().map(|b| b.rms).collect::<Vec<_>>();
    rms.sort_by(f32::total_cmp);
    let p95 = percentile_sorted(&rms, 0.95).max(1e-6);

    blocks
        .iter()
        .zip(chroma)
        .zip(timbre)
        .map(|((block, chroma), timbre)| Descriptor {
            chroma,
            timbre: unit(timbre),
            energy: (block.rms / p95).min(1.0),
        })
        .collect()
}

fn similarity(a: &Descriptor, b: &Descriptor) -> f32 {
    0.5 * dot(&a.chroma, &b.chroma) + 0.5 * dot(&a.timbre, &b.timbre)
}

/// Foote novelty: a Gaussian-tapered checkerboard kernel slid along the
/// diagonal of the self-similarity matrix. Only the band the kernel reaches is
/// ever computed. Near the edges the kernel is truncated and renormalised.
fn novelty_curve(descriptors: &[Descriptor], half: usize) -> Vec<f32> {
    let n = descriptors.len();
    let width = 2 * half;
    let mut band = vec![0.0f32; n * width];
    for i in 0..n {
        for d in 0..width.min(n - i) {
            band[i * width + d] = similarity(&descriptors[i], &descriptors[i + d]);
        }
    }
    let lookup = |a: usize, b: usize| {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        band[lo * width + (hi - lo)]
    };
    let taper = (0..width)
        .map(|k| {
            // σ = half/2: wide enough to average over a chord cycle, so chord
            // changes inside a section do not read as boundaries.
            let u = (k as f32 + 0.5 - half as f32) / half as f32;
            (-2.0 * u * u).exp()
        })
        .collect::<Vec<_>>();

    let min_side = (half / 2).max(2);
    let mut novelty = vec![0.0f32; n];
    for (center, slot) in novelty.iter_mut().enumerate() {
        let past = center.min(half);
        let future = (n - center).min(half);
        if past < min_side || future < min_side {
            continue;
        }
        let span = center - past..center + future;
        let signed = |k: usize| {
            let weight = taper[k + half - center];
            if k < center {
                -weight
            } else {
                weight
            }
        };
        let mut sum = 0.0f32;
        let mut norm = 0.0f32;
        for a in span.clone() {
            let wa = signed(a);
            for b in span.clone() {
                let w = wa * signed(b);
                sum += w * lookup(a, b);
                norm += w.abs();
            }
        }
        *slot = (sum / norm).max(0.0);
    }
    novelty
}

/// Boundary block indices with their strength relative to the strongest peak.
fn pick_boundaries(
    novelty: &[f32],
    edge_gap: usize,
    min_gap: usize,
    max_boundaries: usize,
) -> Vec<(usize, f32)> {
    let n = novelty.len();
    if n < 3 {
        return Vec::new();
    }
    let smooth = (0..n)
        .map(|i| {
            let lo = i.saturating_sub(1);
            let hi = (i + 2).min(n);
            novelty[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
        })
        .collect::<Vec<_>>();
    let peak = smooth.iter().copied().fold(0.0f32, f32::max);
    if peak <= 1e-4 {
        return Vec::new();
    }
    let mean = smooth.iter().sum::<f32>() / n as f32;
    let std = (smooth.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n as f32).sqrt();
    let threshold = (mean + 0.5 * std).max(peak * 0.2).max(MIN_BOUNDARY_NOVELTY);

    let mut candidates = (1..n - 1)
        .filter(|&i| {
            smooth[i] >= threshold && smooth[i] >= smooth[i - 1] && smooth[i] > smooth[i + 1]
        })
        .map(|i| (i, smooth[i]))
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut chosen: Vec<(usize, f32)> = Vec::new();
    for (index, value) in candidates {
        if index < edge_gap || n - index < edge_gap {
            continue;
        }
        if chosen
            .iter()
            .any(|&(other, _)| index.abs_diff(other) < min_gap)
        {
            continue;
        }
        chosen.push((index, value / peak));
        if chosen.len() == max_boundaries {
            break;
        }
    }
    chosen.sort_by_key(|&(index, _)| index);
    chosen
}

struct Segment {
    blocks: Range<usize>,
    start: f32,
    end: f32,
    energy: f32,
    start_strength: f32,
    end_strength: f32,
}

fn build_segments(
    descriptors: &[Descriptor],
    boundaries: &[(usize, f32)],
    block_seconds: f32,
    content_end: f32,
) -> Vec<Segment> {
    // The ends of the content are boundaries by definition.
    let mut edges = Vec::with_capacity(boundaries.len() + 2);
    edges.push((0usize, 1.0f32));
    edges.extend_from_slice(boundaries);
    edges.push((descriptors.len(), 1.0));

    edges
        .windows(2)
        .map(|pair| {
            let (from, start_strength) = pair[0];
            let (to, end_strength) = pair[1];
            let energy =
                descriptors[from..to].iter().map(|d| d.energy).sum::<f32>() / (to - from) as f32;
            Segment {
                blocks: from..to,
                start: from as f32 * block_seconds,
                end: if to == descriptors.len() {
                    content_end
                } else {
                    to as f32 * block_seconds
                },
                energy,
                start_strength,
                end_strength,
            }
        })
        .collect()
}

/// Mean block similarity of two passages laid over each other. The shorter
/// one is tried against both ends of the longer, since a doubled last chorus
/// repeats the first either way, with a couple of blocks of slack.
fn passage_similarity(descriptors: &[Descriptor], a: &Range<usize>, b: &Range<usize>) -> f32 {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let len = short.len();
    if len < 4 {
        return 0.0;
    }
    let mut best = -1.0f32;
    for anchor in [long.start, long.end - len] {
        for lag in -2i64..=2 {
            let mut sum = 0.0f32;
            let mut count = 0usize;
            for k in 0..len {
                let j = anchor as i64 + k as i64 + lag;
                if j < long.start as i64 || j >= long.end as i64 {
                    continue;
                }
                sum += similarity(&descriptors[short.start + k], &descriptors[j as usize]);
                count += 1;
            }
            if count * 2 >= len {
                best = best.max(sum / count as f32);
            }
        }
    }
    best
}

/// Group id per passage. Average linkage in time order, so a chain of
/// near-misses cannot pull unrelated parts into one group.
fn group_repeats(similarity: &[Vec<f32>]) -> Vec<usize> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of = vec![0usize; similarity.len()];
    for passage in 0..similarity.len() {
        let best = groups
            .iter()
            .enumerate()
            .map(|(group, members)| {
                let average = members
                    .iter()
                    .map(|&member| similarity[member][passage])
                    .sum::<f32>()
                    / members.len() as f32;
                (group, average)
            })
            .filter(|&(_, average)| average >= REPEAT_SIMILARITY)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((group, _)) => {
                groups[group].push(passage);
                group_of[passage] = group;
            }
            None => {
                group_of[passage] = groups.len();
                groups.push(vec![passage]);
            }
        }
    }
    group_of
}

fn label_segments(
    segments: &[Segment],
    group_of: &[usize],
    typical_energy: f32,
    content_end: f32,
) -> Vec<(SongSectionKind, f32)> {
    let n = segments.len();
    let group_count = group_of.iter().copied().max().map_or(0, |g| g + 1);
    let mut members = vec![0usize; group_count];
    for &group in group_of {
        members[group] += 1;
    }
    let unique = |s: usize| members[group_of[s]] == 1;
    let quiet = |s: usize| segments[s].energy < typical_energy * 0.85;
    let mut labels: Vec<Option<(SongSectionKind, f32)>> = vec![None; n];

    // The chorus is the loudest part that comes back.
    let group_energy = |group: usize| {
        let (sum, weight) = segments
            .iter()
            .zip(group_of)
            .filter(|&(_, &g)| g == group)
            .fold((0.0f32, 0.0f32), |(sum, weight), (segment, _)| {
                let length = segment.end - segment.start;
                (sum + segment.energy * length, weight + length)
            });
        sum / weight.max(1e-3)
    };
    let chorus = (0..group_count)
        .filter(|&group| members[group] >= 2)
        .map(|group| (group, group_energy(group)))
        .filter(|&(_, energy)| energy >= typical_energy * 0.95)
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match chorus {
        Some((group, energy)) => {
            let repeats = members[group].min(4) - 2;
            let confidence =
                (0.55 + 0.08 * repeats as f32 + (energy - typical_energy).clamp(0.0, 0.3))
                    .min(0.95);
            for s in (0..n).filter(|&s| group_of[s] == group) {
                labels[s] = Some((SongSectionKind::Chorus, confidence));
            }
        }
        None => {
            // Nothing loud recurs: settle for a clearly loud passage away from the ends.
            let inner = if n >= 3 { 1..n - 1 } else { 0..n };
            if let Some(s) =
                inner.max_by(|&a, &b| segments[a].energy.total_cmp(&segments[b].energy))
            {
                if segments[s].energy >= typical_energy + 0.08 {
                    labels[s] = Some((SongSectionKind::Chorus, 0.42));
                }
            }
        }
    }

    let end_confidence = |s: usize| {
        0.55 + if unique(s) { 0.1 } else { 0.0 }
            + if segments[s].energy < typical_energy * 0.7 {
                0.1
            } else {
                0.0
            }
    };
    let first = &segments[0];
    if labels[0].is_none()
        && first.end - first.start <= MAX_INTRO_SECONDS
        && (unique(0) || quiet(0))
    {
        labels[0] = Some((SongSectionKind::Intro, end_confidence(0)));
    }
    let last = n - 1;
    let tail = &segments[last];
    if last > 0
        && labels[last].is_none()
        && tail.end - tail.start <= MAX_OUTRO_SECONDS
        && (unique(last) || quiet(last))
    {
        labels[last] = Some((SongSectionKind::Outro, end_confidence(last)));
    }

    let chorus_starts = (0..n)
        .filter(|&s| matches!(labels[s], Some((SongSectionKind::Chorus, _))))
        .map(|s| segments[s].start)
        .collect::<Vec<_>>();
    let first_chorus = chorus_starts.first().copied();
    let last_chorus = chorus_starts.last().copied();

    (0..n)
        .map(|s| {
            labels[s].unwrap_or_else(|| {
                let segment = &segments[s];
                if unique(s) && segment.energy <= typical_energy * 0.6 {
                    (SongSectionKind::Breakdown, 0.5)
                } else if unique(s)
                    && segment.start >= content_end * 0.45
                    && first_chorus.is_some_and(|start| segment.start > start)
                    && last_chorus.is_some_and(|start| segment.start < start)
                {
                    (SongSectionKind::Bridge, 0.45)
                } else if unique(s) {
                    (SongSectionKind::Verse, 0.4)
                } else {
                    (SongSectionKind::Verse, 0.55)
                }
            })
        })
        .collect()
}

fn merge_adjacent(sections: Vec<SongSection>) -> Vec<SongSection> {
    let mut merged: Vec<SongSection> = Vec::with_capacity(sections.len());
    for section in sections {
        if let Some(last) = merged.last_mut() {
            if last.section_type == section.section_type {
                let a = (last.end - last.start).max(1e-3);
                let b = (section.end - section.start).max(1e-3);
                let blend = |x: f32, y: f32| (x * a + y * b) / (a + b);
                last.confidence = blend(last.confidence, section.confidence);
                last.energy = blend(last.energy, section.energy);
                last.vocal_risk = blend(last.vocal_risk, section.vocal_risk);
                last.mix_suitability = blend(last.mix_suitability, section.mix_suitability);
                last.end = section.end;
                continue;
            }
        }
        merged.push(section);
    }
    merged
}

fn unit<const N: usize>(mut vector: [f32; N]) -> [f32; N] {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= 1e-6 {
        return [0.0; N];
    }
    vector.iter_mut().for_each(|v| *v /= norm);
    vector
}

fn dot<const N: usize>(a: &[f32; N], b: &[f32; N]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn percentile_sorted(values: &[f32], percentile: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len() - 1) as f32 * percentile.clamp(0.0, 1.0)).round() as usize;
    values[index.min(values.len() - 1)]
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    percentile_sorted(&values, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const RATE: u32 = 11_025;

    struct Part {
        seconds: f32,
        /// (root pitch class, minor) per chord, two seconds each.
        chords: &'static [(u8, bool)],
        level: f32,
        brightness: f32,
        drums: f32,
    }

    const INTRO: Part = Part {
        seconds: 10.0,
        chords: &[(9, true), (5, false)],
        level: 0.35,
        brightness: 0.1,
        drums: 0.0,
    };
    const VERSE: Part = Part {
        seconds: 16.0,
        chords: &[(0, false), (7, false), (9, true), (5, false)],
        level: 0.6,
        brightness: 0.2,
        drums: 0.3,
    };
    const CHORUS: Part = Part {
        seconds: 16.0,
        chords: &[(2, false), (9, false), (11, true), (7, false)],
        level: 1.0,
        brightness: 0.6,
        drums: 1.0,
    };
    const BRIDGE: Part = Part {
        seconds: 12.0,
        chords: &[(3, false), (10, false)],
        level: 0.7,
        brightness: 0.3,
        drums: 0.5,
    };
    const OUTRO: Part = Part {
        seconds: 10.0,
        chords: &[(9, true), (5, false)],
        level: 0.3,
        brightness: 0.1,
        drums: 0.0,
    };

    /// Triads over a root, a bass note and a decaying noise hit every half
    /// second — enough harmony and timbre to tell the parts apart.
    fn render(parts: &[&Part]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut noise = 0x1234_5678u32;
        for part in parts {
            let samples = (part.seconds * RATE as f32) as usize;
            for i in 0..samples {
                let t = i as f32 / RATE as f32;
                let (root, minor) = part.chords[(t / 2.0) as usize % part.chords.len()];
                let mut value = 0.0;
                for interval in [0u8, if minor { 3 } else { 4 }, 7] {
                    let hz = 261.63 * 2f32.powf(f32::from((root + interval) % 12) / 12.0);
                    value += (TAU * hz * t).sin()
                        + part.brightness * (TAU * 2.0 * hz * t).sin()
                        + part.brightness * 0.5 * (TAU * 4.0 * hz * t).sin();
                }
                value += 0.8 * (TAU * 130.81 * 2f32.powf(f32::from(root) / 12.0) * t).sin();
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hit = (noise >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                value += part.drums * hit * (-(t % 0.5) * 30.0).exp();
                out.push(value * part.level * 0.1);
            }
        }
        out
    }

    fn hints(duration: f32) -> SectionHints<'static> {
        SectionHints {
            duration,
            content_end: duration,
            tempo: None,
            vocal_activity: None,
        }
    }

    #[test]
    fn a_pop_song_is_split_and_named_by_its_parts() {
        let song = [
            &INTRO, &VERSE, &CHORUS, &VERSE, &CHORUS, &BRIDGE, &CHORUS, &OUTRO,
        ];
        let samples = render(&song);
        let duration = samples.len() as f32 / RATE as f32;
        let analysis = analyze_song_sections(&samples, RATE, &hints(duration))
            .expect("a structured song must produce sections");

        let expected = [
            (SongSectionKind::Intro, 0.0),
            (SongSectionKind::Verse, 10.0),
            (SongSectionKind::Chorus, 26.0),
            (SongSectionKind::Verse, 42.0),
            (SongSectionKind::Chorus, 58.0),
            (SongSectionKind::Bridge, 74.0),
            (SongSectionKind::Chorus, 86.0),
            (SongSectionKind::Outro, 102.0),
        ];
        let found = analysis
            .sections
            .iter()
            .map(|s| (s.section_type, s.start))
            .collect::<Vec<_>>();
        assert_eq!(found.len(), expected.len(), "sections: {found:?}");
        for ((kind, start), (expected_kind, expected_start)) in found.iter().zip(expected) {
            assert_eq!(*kind, expected_kind, "sections: {found:?}");
            assert!(
                (start - expected_start).abs() <= 1.2,
                "{kind:?} starts at {start}, expected ~{expected_start}; sections: {found:?}"
            );
        }
        assert_eq!(analysis.method, "noveltyRepetition");
        assert!(
            analysis.confidence >= 0.45,
            "confidence {}",
            analysis.confidence
        );

        let chorus = analysis
            .sections
            .iter()
            .find(|s| s.section_type == SongSectionKind::Chorus)
            .unwrap();
        let intro = &analysis.sections[0];
        assert!(chorus.energy > intro.energy);
    }

    #[test]
    fn trailing_silence_becomes_its_own_section() {
        let mut samples = render(&[&INTRO, &VERSE, &CHORUS, &VERSE, &CHORUS, &OUTRO]);
        let content_end = samples.len() as f32 / RATE as f32;
        samples.extend(std::iter::repeat_n(0.0, 3 * RATE as usize));
        let duration = samples.len() as f32 / RATE as f32;
        let analysis = analyze_song_sections(
            &samples,
            RATE,
            &SectionHints {
                content_end,
                ..hints(duration)
            },
        )
        .expect("sections");

        let last = analysis.sections.last().unwrap();
        assert_eq!(last.section_type, SongSectionKind::Silence);
        assert!((last.start - content_end).abs() < 1e-3);
        assert!((last.end - duration).abs() < 1e-3);
    }

    #[test]
    fn short_content_has_no_sections() {
        let samples = render(&[&VERSE]);
        let duration = samples.len() as f32 / RATE as f32;
        assert!(analyze_song_sections(&samples, RATE, &hints(duration)).is_none());
    }

    #[test]
    fn unchanging_audio_has_no_structure() {
        let samples = render(&[&VERSE, &VERSE, &VERSE, &VERSE]);
        let duration = samples.len() as f32 / RATE as f32;
        assert!(analyze_song_sections(&samples, RATE, &hints(duration)).is_none());
    }

    #[test]
    fn streaming_in_chunks_matches_one_push() {
        let samples = render(&[&INTRO, &CHORUS]);
        let mut whole = StructureFeatureExtractor::new(RATE);
        whole.push(&samples);
        let whole = whole.finish();

        let mut chunked = StructureFeatureExtractor::new(RATE);
        for chunk in samples.chunks(1_000) {
            chunked.push(chunk);
        }
        let chunked = chunked.finish();

        assert_eq!(whole.frames, chunked.frames);
    }

    #[test]
    fn chroma_follows_pitch_at_common_rates() {
        for rate in [22_050u32, 44_100, 48_000] {
            let tone = (0..rate as usize)
                .map(|i| (TAU * 440.0 * i as f32 / rate as f32).sin())
                .collect::<Vec<_>>();
            let mut extractor = StructureFeatureExtractor::new(rate);
            extractor.push(&tone);
            let features = extractor.finish();
            let frame = features.frames[1];
            let strongest = (0..12)
                .max_by(|&a, &b| frame.chroma[a].total_cmp(&frame.chroma[b]))
                .unwrap();
            assert_eq!(strongest, 9, "440 Hz is an A at {rate} Hz");
            assert!((0.15..=0.2).contains(&features.frame_seconds));
        }
    }
}
