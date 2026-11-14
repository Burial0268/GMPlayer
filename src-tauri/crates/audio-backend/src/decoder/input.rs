//! Codec selection happens once; consumers keep concrete PCM sources in their loops.

use ::symphonia::core::io::MediaSource;

use super::{ffmpeg::FfmpegSource, symphonia::SeekableSymphoniaSource};
use crate::error::{AudioError, AudioResult};

pub(crate) enum OpenedDecoder {
    Symphonia(SeekableSymphoniaSource),
    Ffmpeg(FfmpegSource),
}

impl OpenedDecoder {
    /// The caller owns source policy: only playback may publish a growing-file cursor.
    /// Reopen on fallback because probing consumed and sought the original input.
    pub(crate) fn open_with<R: MediaSource + 'static>(
        mut open: impl FnMut() -> AudioResult<(R, Option<String>)>,
    ) -> AudioResult<Self> {
        let (input, extension) = open()?;
        match SeekableSymphoniaSource::from_input(input, extension.as_deref()) {
            Ok(source) => Ok(Self::Symphonia(source)),
            Err(
                error @ (AudioError::Decode(_)
                | AudioError::UnsupportedFormat(_)
                | AudioError::NoAudioTrack),
            ) => {
                let (input, _) = open()?;
                let source = FfmpegSource::from_input(input).map_err(|fallback| {
                    AudioError::Decode(format!("{error}; fallback failed: {fallback}"))
                })?;
                tracing::debug!("Symphonia 无法解码，使用 FFmpeg：{error}");
                Ok(Self::Ffmpeg(source))
            }
            Err(error) => Err(error),
        }
    }
}
