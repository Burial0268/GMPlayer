//! Seekable Symphonia PCM source, independent of playback worker controls.

use crate::decoder::pcm::PcmSource;
use crate::error::{AudioError, AudioResult};
use ::symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
use ::symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use ::symphonia::core::errors::Error as SymphoniaError;
use ::symphonia::core::formats::{FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo};
use ::symphonia::core::io::MediaSourceStream;
use ::symphonia::core::meta::MetadataOptions;
use ::symphonia::core::probe::Hint;
use ::symphonia::core::units::Time;
use rodio::source::SeekError;
use rodio::Source;
use std::num::{NonZeroU16, NonZeroU32};
#[cfg(test)]
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

const MAX_DECODE_RETRIES: usize = 3;

pub(crate) struct SeekableSymphoniaSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn ::symphonia::core::codecs::Decoder>,
    track_id: u32,
    spec: SignalSpec,
    total_duration: Option<Time>,
    buffer: SampleBuffer<f32>,
    buffer_offset: usize,
}

impl SeekableSymphoniaSource {
    #[cfg(test)]
    pub(crate) fn open(path: &Path) -> AudioResult<Self> {
        let opened = crate::source::open_playback(crate::source::SourceLocator::from_path(path))?;
        let extension = opened.extension().map(str::to_string);
        Self::from_input(opened, extension.as_deref())
    }

    pub(crate) fn from_input(
        input: impl ::symphonia::core::io::MediaSource + 'static,
        extension: Option<&str>,
    ) -> AudioResult<Self> {
        let mut hint = Hint::new();
        // symphonia 0.5's `Probe::format` takes the hint as `_hint` and never
        // reads it — format detection is a pure magic-byte scan, so a container
        // is identified correctly no matter what the file is named (or that
        // downloaded sources land on a `.tmp` path). Kept because it costs
        // nothing and later symphonia releases do consult it.
        if let Some(ext) = extension {
            hint.with_extension(ext);
        }
        let mss = MediaSourceStream::new(Box::new(input), Default::default());

        let format_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let probed = ::symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &MetadataOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;
        let mut format = probed.format;

        let track = format
            .default_track()
            .or_else(|| {
                format
                    .tracks()
                    .iter()
                    .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            })
            .ok_or_else(|| AudioError::Decode("no supported audio track".into()))?
            .clone();
        let track_id = track.id;
        let total_duration = track
            .codec_params
            .time_base
            .zip(track.codec_params.n_frames)
            .map(|(base, frames)| base.calc_time(frames));
        let mut decoder = ::symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::Decode(e.to_string()))?;

        let (spec, buffer) = decode_next_audio_buffer(
            format.as_mut(),
            decoder.as_mut(),
            track_id,
            sample_buffer_from_decoded,
        )
        .map_err(|e| AudioError::Decode(e.to_string()))?
        .ok_or_else(|| AudioError::Decode("no decodable audio samples".into()))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            spec,
            total_duration,
            buffer,
            buffer_offset: 0,
        })
    }

    fn refill(&mut self) -> Option<()> {
        let buffer = &mut self.buffer;
        let spec = match decode_next_audio_buffer(
            self.format.as_mut(),
            self.decoder.as_mut(),
            self.track_id,
            |decoded| copy_decoded_into_sample_buffer(decoded, buffer),
        ) {
            Ok(Some(spec)) => spec,
            Ok(None) => return None,
            Err(err) => {
                warn!("decode failed while refilling playback buffer: {err}");
                return None;
            }
        };

        // The channel count and sample rate were sampled once, when the decoder
        // was opened, and are baked into `FrameConverter`'s mix matrix, its
        // resampler and the bulk-passthrough stride. A mid-stream change would
        // therefore be reinterpreted at the wrong stride and come out as noise,
        // so end the track instead. Symphonia's MP3 and FLAC decoders reject
        // such a change themselves, but container-level readers do not all.
        if spec.channels.count() != self.spec.channels.count() || spec.rate != self.spec.rate {
            warn!(
                "音轨中途改变音频规格 ({} ch @ {} Hz -> {} ch @ {} Hz)，停止解码",
                self.spec.channels.count(),
                self.spec.rate,
                spec.channels.count(),
                spec.rate
            );
            return None;
        }

        self.spec = spec;
        self.buffer_offset = 0;
        Some(())
    }

    /// Decode forward from the seek landing point to the exact requested
    /// position, discarding the output produced along the way.
    ///
    /// `format.seek()` deliberately lands *before* the requested timestamp.
    /// MPEG Layer III granules reference up to 511 bytes of "main data" carried
    /// by earlier frames — the bit reservoir — so symphonia's demuxer walks back
    /// far enough that those references resolve and reports the rewound position
    /// as `actual_ts`. Those run-up packets have to be fed through the decoder:
    /// skipping them undoes the rewind and leaves the first audible frame
    /// pointing at a reservoir that was never filled, which symphonia reports as
    /// `invalid main_data_begin, underflow by N bytes` and renders as a dropped,
    /// silent granule. Only the run-up's *samples* are thrown away.
    fn refine_position(&mut self, seeked: SeekedTo) -> Result<(), SymphoniaError> {
        let mut frames_to_skip = seeked.required_ts.saturating_sub(seeked.actual_ts);
        loop {
            let packet = self.next_track_packet()?;
            let duration = packet.dur();

            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // A run-up packet that will not decode leaves the reservoir cold
                // for one more frame, but it must not abort the seek. Note the
                // variant: Layer III reports a granule whose `main_data_begin`
                // points past what the reservoir actually holds as an `IoError`
                // from the bit reader running off the end of the packet buffer,
                // *not* as a `DecodeError` — and that is exactly the packet a
                // run-up is most likely to land on, since the run-up exists
                // because the reservoir starts cold. Both are therefore
                // tolerated here; `ResetRequired` and the fatal variants still
                // propagate. `decode()` reads only from the already-demuxed
                // packet, so this cannot swallow a failure of the file itself —
                // that surfaces from `next_track_packet` above.
                Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_))
                    if duration <= frames_to_skip =>
                {
                    frames_to_skip -= duration;
                    continue;
                }
                Err(err) => return Err(err),
            };

            if duration <= frames_to_skip {
                frames_to_skip -= duration;
                continue;
            }

            self.spec = copy_decoded_into_sample_buffer(decoded, &mut self.buffer);
            self.buffer_offset = (frames_to_skip as usize)
                .saturating_mul(self.channels().get() as usize)
                .min(self.buffer.len());
            return Ok(());
        }
    }

    fn next_track_packet(&mut self) -> Result<Packet, SymphoniaError> {
        loop {
            let packet = self.format.next_packet()?;
            if packet.track_id() == self.track_id {
                return Ok(packet);
            }
        }
    }
}

impl PcmSource for SeekableSymphoniaSource {
    #[inline]
    fn samples_and_offset(&mut self) -> (&[f32], &mut usize) {
        (self.buffer.samples(), &mut self.buffer_offset)
    }

    fn refill_buffer(&mut self) -> bool {
        self.refill().is_some()
    }
}

impl Iterator for SeekableSymphoniaSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer_offset >= self.buffer.len() {
            self.refill()?;
        }
        let sample = *self.buffer.samples().get(self.buffer_offset)?;
        self.buffer_offset += 1;
        Some(sample)
    }
}

impl Source for SeekableSymphoniaSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.buffer_offset))
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(self.spec.channels.count().max(1) as u16).unwrap()
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(self.spec.rate.max(1)).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration.map(|Time { seconds, frac }| {
            Duration::new(
                seconds,
                (frac.clamp(0.0, 0.999_999_999) * 1_000_000_000.0) as u32,
            )
        })
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let target = if self
            .total_duration()
            .is_some_and(|duration| duration.saturating_sub(pos).as_millis() < 1)
        {
            self.total_duration
                .map(skip_back_a_tiny_bit)
                .unwrap_or_else(|| pos.as_secs_f64().into())
        } else {
            pos.as_secs_f64().into()
        };

        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: target,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(to_seek_error)?;

        self.decoder.reset();
        self.refine_position(seeked).map_err(to_seek_error)?;
        Ok(())
    }
}

fn decode_next_audio_buffer<T>(
    format: &mut dyn FormatReader,
    decoder: &mut dyn ::symphonia::core::codecs::Decoder,
    track_id: u32,
    mut consume: impl FnMut(AudioBufferRef<'_>) -> T,
) -> Result<Option<T>, SymphoniaError> {
    let mut decode_errors = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Symphonia signals a clean end of stream as an `UnexpectedEof` IO
            // error. Anything else is a genuine read failure — a vanished file,
            // a truncated download — and must not be reported to the caller as
            // a track that simply finished.
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None)
            }
            Err(err) => return Err(err),
        };
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => return Ok(Some(consume(decoded))),
            Err(SymphoniaError::DecodeError(_)) => {
                decode_errors += 1;
                if decode_errors > MAX_DECODE_RETRIES {
                    return Err(SymphoniaError::DecodeError("too many decode errors"));
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn sample_buffer_from_decoded(decoded: AudioBufferRef<'_>) -> (SignalSpec, SampleBuffer<f32>) {
    let spec = *decoded.spec();
    let mut sample_buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
    sample_buffer.copy_interleaved_ref(decoded);
    (spec, sample_buffer)
}

fn copy_decoded_into_sample_buffer(
    decoded: AudioBufferRef<'_>,
    sample_buffer: &mut SampleBuffer<f32>,
) -> SignalSpec {
    let spec = *decoded.spec();
    let required_samples = decoded.frames().saturating_mul(spec.channels.count());
    if sample_buffer.capacity() < required_samples {
        *sample_buffer = SampleBuffer::new(decoded.capacity() as u64, spec);
    }
    sample_buffer.copy_interleaved_ref(decoded);
    spec
}

fn to_seek_error(err: SymphoniaError) -> SeekError {
    SeekError::Other(Arc::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        err.to_string(),
    )))
}

fn skip_back_a_tiny_bit(
    Time {
        mut seconds,
        mut frac,
    }: Time,
) -> Time {
    frac -= 0.0001;
    if frac < 0.0 {
        seconds = seconds.saturating_sub(1);
        frac = 1.0 + frac;
    }
    Time { seconds, frac }
}
