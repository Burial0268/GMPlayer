use thiserror::Error;

#[derive(Error, Debug)]
pub enum AudioError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decode error: {0}")]
    Decode(String),

    #[error("Audio output error: {0}")]
    Output(String),

    #[error("Unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("No audio track found in the media")]
    NoAudioTrack,

    #[error("Player not initialized")]
    NotInitialized,

    #[error("Thread communication error: {0}")]
    ThreadError(String),

    #[error("FFT error: {0}")]
    FftError(String),

    #[error("Download failed: {0}")]
    Download(String),

    #[error("Queue is empty")]
    QueueEmpty,

    #[error("Queue item not found: id={0}")]
    QueueItemNotFound(u64),

    #[error("Invalid queue index: {0}")]
    InvalidQueueIndex(usize),

    #[error("Crossfade in progress")]
    CrossfadeInProgress,

    #[error("Player busy: {0}")]
    PlayerBusy(String),
}

pub type AudioResult<T> = Result<T, AudioError>;
