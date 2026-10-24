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

    // --- Microphone capture (`microphone`) ---
    /// The microphone delivered nothing but digital silence (exact zeros) for a second or
    /// more. On Windows this is what a desktop app gets when "Let desktop apps access your
    /// microphone" is off (Settings > Privacy & security > Microphone); a hardware mute
    /// switch gives the same. A quiet room is not silence in this sense: it has low,
    /// non-zero noise.
    #[error(
        "the microphone delivers only silence: Windows may be blocking desktop apps from using it"
    )]
    MicrophoneSilent,

    /// No input device matches the requested id or name.
    #[error("no microphone matches \"{0}\"")]
    MicrophoneNotFound(String),

    /// Listing, opening or reading the microphone failed, or the audio could not be converted.
    #[error("microphone: {step} failed: {message}")]
    Microphone { step: &'static str, message: String },

    /// Drawing a replay frame failed (a page too small or too large for the frame size).
    #[error("cannot draw the replay: {0}")]
    Render(#[from] ardoise_render::RenderError),

    // Animated formats (GIF, WebP, WebM) and the Opus encoder.
    /// Reading or writing the file failed.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),

    /// An encoder or muxer of the animated formats failed (`codec`: "GIF", "WebP", "AV1",
    /// "Opus", "WebM").
    #[error("{codec} encoding failed: {message}")]
    Encoding {
        codec: &'static str,
        message: String,
    },

    // MP4 back ends (`mp4`).
    /// No usable H.264 encoder for the MP4: Media Foundation is missing and the OpenH264
    /// fallback is not installed, not allowed or not working.
    #[error("no H.264 encoder for the MP4: {0}")]
    CodecUnavailable(String),

    // Export pipeline (`export`).
    /// The user stopped the export.
    #[error("export cancelled")]
    Cancelled,
}
