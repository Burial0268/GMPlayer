//! Scratch PCM preserves exact decoder samples while bounding analysis heap use.

use std::io::{Read, Seek, SeekFrom, Write};

use gmplayer_automix_analysis::{analyze_pcm, MonoPcm, TrackAnalysis};
use rodio::Source;

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
    pub(super) fn decode<D: Source<Item = f32>>(
        decoder: &mut D,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Result<Self, String> {
        let rate = decoder.sample_rate().get();
        let channels = decoder.channels().get() as usize;
        let duration = decoder.total_duration().map(|d| d.as_secs_f32());
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
        for sample in decoder {
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
        if channel > 0 {
            mono.push(sum / channel as f32);
        }
        spool.append(&mono)?;
        if spool.samples == 0 {
            return Err("decode audio: no samples".into());
        }
        spool.duration = duration
            .filter(|d| *d > 0.0)
            .unwrap_or(spool.samples as f32 / rate as f32);
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
