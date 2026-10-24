//! What every animated export format shares: the sink that replay frames and narration are
//! pushed into, the settings common to all formats, narration audio, and what a file reads
//! back as (for tests). One module per format implements [`VideoSink`]; see
//! `docs/design/media.md`.

use std::time::Duration;

use crate::MediaError;

/// Sample rate of narration everywhere: microphone capture resamples to it, and both AAC
/// (MP4 through Media Foundation) and Opus (WebM, MP4 through OpenH264) accept it.
pub const NARRATION_SAMPLE_RATE: u32 = 48_000;

/// The animated export formats (SPEC §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationFormat {
    Mp4,
    Gif,
    WebP,
    WebM,
}

impl AnimationFormat {
    /// File extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            AnimationFormat::Mp4 => "mp4",
            AnimationFormat::Gif => "gif",
            AnimationFormat::WebP => "webp",
            AnimationFormat::WebM => "webm",
        }
    }

    /// True if the format can carry the narration.
    pub fn has_sound(self) -> bool {
        matches!(self, AnimationFormat::Mp4 | AnimationFormat::WebM)
    }
}

/// Settings shared by every animated format; each format adds its own options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSettings {
    /// Frame width in pixels (even, for the video codecs).
    pub width: u32,
    /// Frame height in pixels (even).
    pub height: u32,
    /// Nominal frames per second. Frames carry their own timestamps; the last frame lasts
    /// `1 / frame_rate`.
    pub frame_rate: u32,
    /// Narration channels (1 or 2) at [`NARRATION_SAMPLE_RATE`], or `None` for no sound.
    /// Formats without sound ignore it.
    pub audio_channels: Option<u16>,
}

/// Where the frames of a replay and its narration go: one implementation per format.
///
/// Frames and audio are pushed interleaved, each frame's audio before or with the frame
/// (Media Foundation is ten times slower when all the video comes first).
pub trait VideoSink {
    /// Adds a frame: opaque RGBA, `width * height * 4` bytes, shown from `timestamp` until
    /// the next frame. Timestamps strictly increase and the first one is zero.
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError>;

    /// Adds narration: interleaved signed 16-bit samples at [`NARRATION_SAMPLE_RATE`] with
    /// [`VideoSettings::audio_channels`] channels, continuing where the previous chunk
    /// stopped (the first sample is at time zero). Formats without sound ignore it.
    fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError>;

    /// Writes what is left and closes the file.
    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError>;
}

/// What a [`VideoSink`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSummary {
    pub format: AnimationFormat,
    pub frames: u64,
    /// End of the last frame or of the narration, whichever is later.
    pub duration: Duration,
    /// True if the file has a sound track.
    pub audio: bool,
    /// Who encoded the video, for the report and diagnostics (e.g. "Media Foundation:
    /// Intel Quick Sync Video H.264 Encoder MFT (hardware)", "OpenH264 2.6.0", "rav1e").
    pub encoder: String,
}

/// An animated file read back independently of the writer (tests, diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationProbe {
    pub format: AnimationFormat,
    pub width: u32,
    pub height: u32,
    /// Frames actually decoded.
    pub frames: u64,
    /// Duration read from the file (sum of frame delays, or the container's duration).
    pub duration: Duration,
    pub audio: Option<AudioProbe>,
}

/// The sound track of an [`AnimationProbe`].
#[derive(Debug, Clone, PartialEq)]
pub struct AudioProbe {
    /// "AAC" or "Opus".
    pub codec: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub duration: Duration,
}

/// Recorded narration: interleaved signed 16-bit samples at [`NARRATION_SAMPLE_RATE`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioClip {
    /// 1 or 2.
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl AudioClip {
    /// Length of the clip.
    pub fn duration(&self) -> Duration {
        let frames = self.samples.len() as u64 / u64::from(self.channels.max(1));
        Duration::from_secs_f64(frames as f64 / f64::from(NARRATION_SAMPLE_RATE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_duration_counts_frames_not_samples() {
        let clip = AudioClip {
            channels: 2,
            samples: vec![0; 2 * 48_000],
        };
        assert_eq!(clip.duration(), Duration::from_secs(1));
        assert_eq!(AudioClip::default().duration(), Duration::ZERO);
    }

    #[test]
    fn only_video_formats_have_sound() {
        assert!(AnimationFormat::Mp4.has_sound());
        assert!(AnimationFormat::WebM.has_sound());
        assert!(!AnimationFormat::Gif.has_sound());
        assert!(!AnimationFormat::WebP.has_sound());
        assert_eq!(AnimationFormat::WebP.extension(), "webp");
    }
}
