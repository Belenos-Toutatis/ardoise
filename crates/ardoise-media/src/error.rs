//! Errors of the media crate.

use std::time::Duration;

/// Everything that can go wrong while writing or reading a media file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MediaError {
    /// Media Foundation export is only implemented on Windows.
    #[error("MP4 export through Media Foundation is only available on Windows")]
    Unsupported,

    /// `MFStartup` failed: Media Foundation is missing or broken (for example a Windows "N"
    /// edition without the Media Feature Pack).
    #[error(
        "Windows Media Foundation is not available on this PC: {message} (HRESULT {code:#010x})"
    )]
    MediaFoundationUnavailable { code: u32, message: String },

    /// The sink writer found no encoder for this codec.
    #[error("Windows Media Foundation has no {codec} encoder on this PC (HRESULT {code:#010x})")]
    EncoderUnavailable { codec: &'static str, code: u32 },

    /// The settings are outside what the writer supports.
    #[error("invalid export settings: {0}")]
    InvalidSettings(String),

    /// A frame buffer does not hold exactly `width × height` RGBA pixels.
    #[error("frame buffer holds {actual} bytes, expected {expected}")]
    FrameSize { expected: usize, actual: usize },

    /// Frame timestamps must strictly increase.
    #[error("frame timestamp {timestamp:?} does not come after the previous one ({previous:?})")]
    TimestampOrder {
        previous: Duration,
        timestamp: Duration,
    },

    /// An interleaved audio buffer must hold whole frames (one sample per channel).
    #[error("audio buffer of {len} samples does not hold whole frames of {channels} channel(s)")]
    AudioLength { len: usize, channels: u16 },

    /// `push_audio` was called on a writer created without an audio track.
    #[error("the writer has no audio track")]
    NoAudioTrack,

    /// A Media Foundation call failed.
    #[error("Media Foundation failed during {step}: {message} (HRESULT {code:#010x})")]
    MediaFoundation {
        step: &'static str,
        code: u32,
        message: String,
    },

    /// The file read back does not have the expected structure.
    #[error("unexpected media file content: {0}")]
    InvalidFile(String),
}
