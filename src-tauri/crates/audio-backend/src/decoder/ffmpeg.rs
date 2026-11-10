//! Native fallback for codecs missing from Symphonia. See ../../UPSTREAM.md.

use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ffmpeg_audio::{AudioReader, ResampleOptions, Resampler, SeekMode};
use rodio::{source::SeekError, Source};
use tracing::warn;

use crate::error::{AudioError, AudioResult};
use crate::source::{self, OpenedSource, SourceLocator};

use super::pcm::PcmSource;

pub(crate) fn reader(opened: OpenedSource) -> AudioResult<AudioReader> {
    AudioReader::new(opened).map_err(decode_error)
}

pub(crate) fn pcm_format(reader: &AudioReader) -> AudioResult<(NonZeroU16, NonZeroU32)> {
    let info = reader.source_info();
    let channels = u16::try_from(info.channels)
        .ok()
        .and_then(NonZeroU16::new)
        .ok_or_else(|| AudioError::Decode("FFmpeg: invalid channel count".into()))?;
    let sample_rate = u32::try_from(info.sample_rate)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| AudioError::Decode("FFmpeg: invalid sample rate".into()))?;
    Ok((channels, sample_rate))
}

fn decode_error(error: ffmpeg_audio::AudioError) -> AudioError {
    AudioError::Decode(format!("FFmpeg: {error}"))
}

pub(crate) struct FfmpegSource {
    reader: AudioReader,
    converter: Resampler,
    channels: NonZeroU16,
    sample_rate: NonZeroU32,
    duration: Option<Duration>,
    offset: usize,
    exhausted: bool,
    error: Option<String>,
}

impl FfmpegSource {
    pub(crate) fn open(path: &Path) -> AudioResult<Self> {
        let opened = source::open_playback(SourceLocator::from_path(path))?;
        let reader = reader(opened)?;
        let (channels, sample_rate) = pcm_format(&reader)?;
        let duration = reader.duration();
        let converter = reader
            .build_resampler(
                ResampleOptions::new()
                    .sample_rate(sample_rate.get() as i32)
                    .channels(channels.get() as i32)
                    .format::<f32>(),
            )
            .map_err(decode_error)?;
        let mut source = Self {
            reader,
            converter,
            channels,
            sample_rate,
            duration,
            offset: 0,
            exhausted: false,
            error: None,
        };
        if !source.refill_buffer() {
            return Err(AudioError::Decode(
                source
                    .error
                    .take()
                    .unwrap_or_else(|| "FFmpeg: no decodable audio samples".into()),
            ));
        }
        Ok(source)
    }

    pub(crate) fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn refill(&mut self) -> ffmpeg_audio::Result<bool> {
        if self.exhausted {
            return Ok(false);
        }
        loop {
            let frame = self.reader.receive_frame()?;
            let eof = frame.is_none();
            let has_output = self.converter.process::<f32>(frame.as_ref())?;
            if has_output {
                self.offset = 0;
                return Ok(true);
            }
            if eof {
                self.exhausted = true;
                return Ok(false);
            }
        }
    }
}

impl PcmSource for FfmpegSource {
    #[inline]
    fn samples_and_offset(&mut self) -> (&[f32], &mut usize) {
        (self.converter.output_as::<f32>(), &mut self.offset)
    }

    fn refill_buffer(&mut self) -> bool {
        match self.refill() {
            Ok(has_output) => has_output,
            Err(error) => {
                warn!("FFmpeg 解码失败：{error}");
                self.error = Some(error.to_string());
                self.exhausted = true;
                false
            }
        }
    }
}

impl Iterator for FfmpegSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.offset >= self.converter.output_as::<f32>().len() && !self.refill_buffer() {
            return None;
        }
        let sample = *self.converter.output_as::<f32>().get(self.offset)?;
        self.offset += 1;
        Some(sample)
    }
}

impl Source for FfmpegSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(
            self.converter
                .output_as::<f32>()
                .len()
                .saturating_sub(self.offset),
        )
    }

    fn channels(&self) -> NonZeroU16 {
        self.channels
    }
    fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        let result = (|| {
            self.reader.seek(position, SeekMode::Accurate)?;
            self.converter.flush()?;
            self.exhausted = false;
            self.error = None;
            self.offset = self.converter.output_as::<f32>().len();
            self.refill().map(|_| ())
        })();
        result.map_err(|error| SeekError::Other(Arc::new(std::io::Error::other(error.to_string()))))
    }
}

#[cfg(test)]
mod tests;
