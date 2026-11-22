//! Scratch PCM preserves exact decoder samples while bounding analysis heap use.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::decoder::PcmSource;
use gmplayer_automix_analysis::{analyze_pcm, MonoPcm, TrackAnalysis};

const BLOCK: usize = 16_384;
const MEMORY_LIMIT: usize = 4 * 1024 * 1024;

pub(super) struct PcmSpool<'a> {
    file: tempfile::SpooledTempFile,
    samples: usize,
    rate: u32,
    duration: f32,
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a> PcmSpool<'a> {
    pub(super) fn decode<D: PcmSource>(
        decoder: &mut D,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Result<Self, String> {
        let rate = decoder.sample_rate().get();
        let channels = decoder.channels().get() as usize;
        let file = crate::source::temporary_file(MEMORY_LIMIT).map_err(scratch_error)?;
        let mut spool = Self {
            file,
            samples: 0,
            rate,
            duration: 0.0,
            cancelled,
        };
        let mut mono = Vec::with_capacity(BLOCK);
        let (mut sum, mut channel) = (0.0, 0usize);
        // Whole decoded buffers rather than one `next()` per sample; ends where the iterator
        // would, including at a refill that yields nothing.
        loop {
            let (samples, offset) = decoder.samples_and_offset();
            let rest = &samples[(*offset).min(samples.len())..];
            *offset += rest.len();
            for &sample in rest {
                sum += sample;
                channel += 1;
                if channel == channels {
                    mono.push(sum / channels as f32);
                    sum = 0.0;
                    channel = 0;
                    if mono.len() == BLOCK {
                        spool.append(&mono)?;
                        mono.clear();
                    }
                }
            }
            if !decoder.refill_buffer() || decoder.samples_and_offset().0.is_empty() {
                break;
            }
        }
        if channel > 0 {
            mono.push(sum / channel as f32);
        }
        spool.append(&mono)?;
        if spool.samples == 0 {
            return Err("decode audio: no samples".into());
        }
        // What was decoded, not what the container claimed: a headerless
        // stream's estimate schedules the handoff against the wrong ending.
        spool.duration = spool.samples as f32 / rate as f32;
        Ok(spool)
    }

    fn append(&mut self, mono: &[f32]) -> Result<(), String> {
        self.check_cancelled()?;
        self.file
            .write_all(bytemuck::cast_slice(mono))
            .map_err(scratch_error)?;
        self.samples += mono.len();
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if (self.cancelled)() {
            Err("analysis cancelled".into())
        } else {
            Ok(())
        }
    }

    pub(super) fn analyze(mut self, bpm: bool) -> Result<TrackAnalysis, String> {
        let (rate, duration) = (self.rate, self.duration);
        analyze_pcm(&mut self, rate, duration, bpm)
    }
}

impl MonoPcm for PcmSpool<'_> {
    type Error = String;
    fn sample_count(&self) -> usize {
        self.samples
    }
    fn read_at(&mut self, first: usize, output: &mut [f32]) -> Result<(), String> {
        self.check_cancelled()?;
        self.file
            .seek(SeekFrom::Start(first as u64 * 4))
            .map_err(scratch_error)?;
        self.file
            .read_exact(bytemuck::cast_slice_mut(output))
            .map_err(scratch_error)?;
        Ok(())
    }
}

fn scratch_error(error: std::io::Error) -> String {
    format!("analysis scratch PCM: {error}")
}
