use std::{
    io::{Read, Seek},
    time::Duration,
};

use crate::{
    AudioError, AudioFrame, Decoder, Demuxer, Result,
    core::{
        frame::frame_timing,
        timeline::{FramePlacement, Timeline},
    },
    decode::{
        SeekMode,
        cursor::{ReadCursor, Resume},
        io::IoContext,
    },
};

/// Specifies the strategy used to scan an audio stream to determine its exact duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Rapidly scans the stream by reading demuxer packet timestamps without decoding them.
    ///
    /// This mode is extremely fast and relies entirely on the container's metadata.
    /// However, it may fail or return inaccurate results for raw formats or highly
    /// corrupted streams that lack valid timestamp information.
    Packet,

    /// Fully decodes the stream into raw physical audio frames to calculate the duration.
    ///
    /// This mode is the most accurate fallback method, as it calculates time based purely
    /// on the actual number of generated audio samples and the stream's sample rate.
    /// Because it requires full decompression, it consumes significantly more CPU
    /// and takes much longer to complete.
    Frame,
}

/// A decode engine that orchestrates the extraction and decoding of audio data.
///
/// This engine acts as a unified abstraction over FFmpeg's underlying parsing (`Demuxer`)
/// and decompression (`Decoder`) stages. It encapsulates the complex send/receive
/// state machines and buffering required to safely yield raw audio frames, and relies on the
/// [`Timeline`] for every timestamp decision.
pub struct DecodeEngine {
    /// The underlying component responsible for reading and parsing the media container.
    demuxer: Demuxer,

    /// The underlying component responsible for decompressing raw packets into audio frames.
    decoder: Decoder,

    /// Maps the stream's raw timestamps onto the public timeline.
    timeline: Timeline,

    /// Where reading stands.
    cursor: ReadCursor,
}

impl DecodeEngine {
    /// Constructs a new `DecodeEngine` reading from the given source.
    ///
    /// # Returns
    /// * `Ok(DecodeEngine)` if the source is opened and the time base is valid.
    /// * `Err(AudioError)` if the source cannot be opened or decoded, or the demuxer fails to
    ///   provide a valid time base for synchronization.
    pub fn new<T>(source: T) -> Result<Self>
    where
        T: Read + Seek + Send + 'static,
    {
        let io_ctx = IoContext::new(source)?;

        let demuxer = Demuxer::new(io_ctx)?;

        let codec_params = demuxer.stream_codec_params();
        let time_base = demuxer.time_base()?;
        let decoder = Decoder::new(codec_params, time_base)?;

        let timeline = Timeline::new(time_base, demuxer.start_time())?;

        Ok(Self {
            demuxer,
            decoder,
            timeline,
            cursor: ReadCursor::default(),
        })
    }

    /// Pulls and decodes the next available audio frame from the underlying stream.
    ///
    /// # Returns
    /// * `Ok(Some(AudioFrame))` containing the decompressed audio data ready for consumption.
    /// * `Ok(None)` if the stream has reached the End Of File (EOF).
    /// * `Err(AudioError)` if an I/O failure or a fatal FFmpeg decoding error occurs.
    pub fn receive_frame(&mut self) -> Result<Option<AudioFrame<'_>>> {
        if self.cursor.is_exhausted() {
            return Ok(None);
        }

        let next = match self.cursor.pending() {
            Some(placement) => Some(placement),
            None => self.decode_next(Duration::ZERO)?,
        };
        let Some(placement) = next else {
            return Ok(None);
        };

        self.cursor.record_delivery(placement);

        Ok(Some(AudioFrame::new(
            self.decoder.current_frame(),
            placement,
        )))
    }

    /// Decodes until the decoder holds a frame with samples at or after `not_before`, and
    /// returns where that frame lands on the public timeline.
    ///
    /// Frames lying entirely before `not_before` are discarded. Returns `Ok(None)` once the
    /// stream is exhausted.
    fn decode_next(&mut self, not_before: Duration) -> Result<Option<FramePlacement>> {
        loop {
            match self.decoder.receive_frame() {
                Ok(Some(frame)) => {
                    // The decoder has just filled this frame, so it points to a valid AVFrame.
                    let timing = unsafe { frame_timing(frame) };

                    if let Some(placement) = self.timeline.place(timing, not_before) {
                        return Ok(Some(placement));
                    }

                    #[cfg(feature = "tracing")]
                    if not_before.is_zero() {
                        tracing::debug!("Dropped preroll frame (raw pts: {})", timing.pts);
                    }
                }
                Err(AudioError::Eagain) => {
                    if let Some(packet) = self.demuxer.read_packet()? {
                        self.decoder.send_packet(packet)?;
                    } else {
                        if self.decoder.is_flushing() {
                            self.cursor.record_exhaustion();
                            return Ok(None);
                        }

                        self.decoder.send_eof_flush()?;
                    }
                }
                Ok(None) => {
                    self.cursor.record_exhaustion();
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Seeks the underlying audio stream to the specified position on the public timeline.
    ///
    /// # Arguments
    /// * `target` - The position on the public timeline to seek to.
    /// * `mode` - The strategy ([`SeekMode`]) to employ for resolving the exact position.
    ///
    /// # Errors
    /// Returns an `AudioError` if the underlying demuxer fails to seek, or if a decoding
    /// error occurs during the frame alignment process.
    pub fn seek(&mut self, target: Duration, mode: SeekMode) -> Result<()> {
        self.demuxer.seek_to(self.timeline.seek_pts(target))?;
        self.decoder.flush();
        self.cursor.record_coarse_seek(target);

        if mode == SeekMode::Accurate
            && let Some(placement) = self.decode_next(target)?
        {
            if placement.pts().is_none() {
                return Err(AudioError::InvalidData(
                    "Cannot perform accurate seek on a stream lacking valid timestamps."
                        .to_string(),
                ));
            }

            self.cursor.record_pending(
                Resume::Seek {
                    target,
                    mode: SeekMode::Accurate,
                },
                placement,
            );
        }

        Ok(())
    }

    /// Seeks so that reading continues with the frame following the one whose raw timestamp is
    /// `raw_pts`, which is expected to start around `next`.
    fn seek_after(&mut self, raw_pts: i64, next: Duration) -> Result<()> {
        self.demuxer.seek_to(raw_pts)?;
        self.decoder.flush();
        self.cursor.record_coarse_seek(next);

        while let Some(placement) = self.decode_next(Duration::ZERO)? {
            if placement.raw_pts().is_none_or(|pts| pts > raw_pts) {
                self.cursor
                    .record_pending(Resume::After { raw_pts, next }, placement);
                break;
            }
        }

        Ok(())
    }

    /// Scans the audio stream to determine its exact duration: the public time right after the
    /// last sample.
    ///
    /// Reading afterwards continues where it stood, as if the scan had not happened (see
    /// [`ReadCursor::resume`]).
    ///
    /// # Arguments
    /// * `mode` - The strategy ([`ScanMode`]) to employ during the scanning process.
    ///
    /// # Returns
    /// * `Ok(Some(Duration))` representing the exact duration of the audio stream.
    /// * `Ok(None)` if the file is completely empty or lacks valid timestamp data.
    /// * `Err(AudioError)` if an I/O or parsing failure halts the scanning process.
    pub fn scan_duration(&mut self, mode: ScanMode) -> Result<Option<Duration>> {
        let saved = self.cursor;

        self.seek(Duration::ZERO, SeekMode::Coarse)?;

        let mut exact_end: Option<Duration> = None;
        let mut total_duration_fallback = Duration::ZERO;
        let mut scan_error = None;

        match mode {
            ScanMode::Packet => loop {
                match self.demuxer.read_packet() {
                    Ok(Some(packet)) => {
                        // The demuxer has just filled this packet, so it points to a valid
                        // AVPacket.
                        let (pts, duration) = unsafe { ((*packet).pts, (*packet).duration) };
                        exact_end = exact_end.max(self.timeline.packet_end(pts, duration));
                    }
                    Ok(None) => break,
                    Err(e) => {
                        scan_error = Some(e);
                        break;
                    }
                }
            },
            ScanMode::Frame => loop {
                match self.receive_frame() {
                    Ok(Some(frame)) => {
                        total_duration_fallback =
                            total_duration_fallback.saturating_add(frame.duration());
                        exact_end = exact_end.max(frame.end());
                    }
                    Ok(None) => break,
                    Err(e) => {
                        scan_error = Some(e);
                        break;
                    }
                }
            },
        }

        let restore_result = self.restore(saved);

        if let Some(e) = scan_error {
            return Err(e);
        }
        restore_result?;

        Ok(exact_end.or_else(|| {
            (mode == ScanMode::Frame && !total_duration_fallback.is_zero())
                .then_some(total_duration_fallback)
        }))
    }

    /// Puts reading back where `saved` stood, including the reported stream position.
    fn restore(&mut self, saved: ReadCursor) -> Result<()> {
        match saved.resume() {
            Resume::Exhausted => self.cursor = saved,
            Resume::Seek { target, mode } => self.seek(target, mode)?,
            Resume::After { raw_pts, next } => self.seek_after(raw_pts, next)?,
        }

        self.cursor.restore_position(saved.stream_position());
        Ok(())
    }

    /// Returns the duration declared by the container, a quick estimate of the exact duration.
    pub fn declared_duration(&self) -> Option<Duration> {
        self.timeline.declared_duration(
            self.demuxer.stream_duration(),
            self.demuxer.container_duration(),
        )
    }

    /// Returns a shared, immutable reference to the underlying demuxer.
    pub(crate) const fn demuxer(&self) -> &Demuxer {
        &self.demuxer
    }

    /// Returns a shared, immutable reference to the underlying decoder.
    pub(crate) const fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    /// Returns the presentation timestamp of the most recently decoded audio frame.
    ///
    /// # Returns
    /// * `Some(Duration)` representing the current playback position.
    /// * `None` if no frames have been successfully decoded yet, or immediately after a seek.
    pub const fn stream_position(&self) -> Option<Duration> {
        self.cursor.stream_position()
    }
}
