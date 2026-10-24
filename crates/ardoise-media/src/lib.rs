//! Replay frames, MP4/GIF/WebP/WebM export, microphone capture and audio encoding.
//!
//! What exists today:
//!
//! - [`Mp4Writer`]: writes an `.mp4` file holding H.264 video and, optionally, AAC audio through
//!   Windows Media Foundation (`IMFSinkWriter`). Media Foundation provides the encoders (the
//!   GPU's hardware encoder when allowed and present, Microsoft's software encoder otherwise)
//!   and the MPEG-4 muxer, so nothing has to be downloaded (SPEC §6.2, row "Vidéo MP4").
//! - [`probe_mp4`]: reads a media file back through `IMFSourceReader` (container duration,
//!   codecs, dimensions, decoded frame and audio sample counts), for tests and diagnostics.
//! - [`available_encoders`]: the H.264 and AAC encoders Media Foundation offers on this PC.
//! - [`nv12`]: the RGBA to NV12 (BT.709, limited range) conversion the H.264 encoders need.
//!
//! On platforms other than Windows the Media Foundation entry points return
//! [`MediaError::Unsupported`]; the types and the colour conversion are available everywhere.

mod error;
pub mod microphone;
pub mod nv12;
pub mod replay;
mod settings;
mod sink;
// Only the Media Foundation back end needs it (tested everywhere).
#[cfg(any(windows, test))]
mod timing;

#[cfg(windows)]
mod mf;
#[cfg(not(windows))]
mod unsupported;
// Keeps the placeholder compiling (and tested) on the Windows development machine.
#[cfg(all(windows, test))]
#[path = "unsupported.rs"]
mod unsupported;

pub use error::MediaError;
pub use settings::{
    AAC_BITRATE, AudioSettings, AudioTrackProbe, AvailableEncoders, Codec, EncoderInfo, Mp4Probe,
    Mp4Settings, Mp4Summary, ProbeOptions, RgbaFrame, VideoTrackProbe,
};
pub use sink::{
    AnimationFormat, AnimationProbe, AudioClip, AudioProbe, NARRATION_SAMPLE_RATE, VideoSettings,
    VideoSink, VideoSummary,
};

#[cfg(windows)]
pub use mf::{Mp4Writer, available_encoders, probe_mp4};
#[cfg(not(windows))]
pub use unsupported::{Mp4Writer, available_encoders, probe_mp4};
