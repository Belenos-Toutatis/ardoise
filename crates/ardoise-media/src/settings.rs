//! Export settings and read-back results, shared by every platform.

use std::fmt;
use std::time::Duration;

use crate::MediaError;

/// AAC bitrate of the MP4 audio track, in bits per second.
///
/// Microsoft's AAC encoder only accepts 96, 128, 160 or 192 kbit/s; 128 kbit/s is
/// transparent for a voice and still small (about 1 MB per minute).
pub const AAC_BITRATE: u32 = 128_000;

/// Largest width or height accepted (Microsoft's software H.264 encoder stops at 4096).
const MAX_DIMENSION: u32 = 4096;

/// Format of the PCM audio given to [`crate::Mp4Writer::push_audio`]: interleaved signed
/// 16-bit samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSettings {
    /// 44 100 or 48 000 Hz (the only rates the AAC encoder accepts).
    pub sample_rate: u32,
    /// 1 (mono) or 2 (stereo).
    pub channels: u16,
}

impl AudioSettings {
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    pub fn validate(&self) -> Result<(), MediaError> {
        if !matches!(self.sample_rate, 44_100 | 48_000) {
            return Err(MediaError::InvalidSettings(format!(
                "audio sample rate {} Hz (AAC needs 44100 or 48000 Hz)",
                self.sample_rate
            )));
        }
        if !matches!(self.channels, 1 | 2) {
            return Err(MediaError::InvalidSettings(format!(
                "{} audio channels (mono or stereo only)",
                self.channels
            )));
        }
        Ok(())
    }
}

/// Settings of an MP4 export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp4Settings {
    /// Frame width in pixels (even).
    pub width: u32,
    /// Frame height in pixels (even).
    pub height: u32,
    /// Nominal frames per second (1 to 60). Frames carry their own timestamps, so the real
    /// cadence may vary; this sets the encoder's rate control and each frame's duration.
    pub frame_rate: u32,
    /// Average H.264 bitrate in bits per second.
    pub video_bitrate: u32,
    /// Audio track, if any.
    pub audio: Option<AudioSettings>,
    /// Lets Media Foundation pick the GPU's hardware H.264 encoder when there is one.
    /// `false` forces Microsoft's software encoder.
    pub hardware_acceleration: bool,
}

impl Mp4Settings {
    /// Video-only settings with a bitrate suited to handwriting and hardware acceleration on.
    pub fn new(width: u32, height: u32, frame_rate: u32) -> Self {
        Self {
            width,
            height,
            frame_rate,
            video_bitrate: default_video_bitrate(width, height, frame_rate),
            audio: None,
            hardware_acceleration: true,
        }
    }

    /// Adds an audio track.
    pub fn with_audio(mut self, audio: AudioSettings) -> Self {
        self.audio = Some(audio);
        self
    }

    pub fn validate(&self) -> Result<(), MediaError> {
        for (name, value) in [("width", self.width), ("height", self.height)] {
            if !(16..=MAX_DIMENSION).contains(&value) || value % 2 != 0 {
                return Err(MediaError::InvalidSettings(format!(
                    "{name} {value} (must be even, from 16 to {MAX_DIMENSION})"
                )));
            }
        }
        if !(1..=60).contains(&self.frame_rate) {
            return Err(MediaError::InvalidSettings(format!(
                "frame rate {} (must be from 1 to 60)",
                self.frame_rate
            )));
        }
        if !(100_000..=50_000_000).contains(&self.video_bitrate) {
            return Err(MediaError::InvalidSettings(format!(
                "video bitrate {} bit/s (must be from 100 kbit/s to 50 Mbit/s)",
                self.video_bitrate
            )));
        }
        if let Some(audio) = &self.audio {
            audio.validate()?;
        }
        Ok(())
    }

    /// Size in bytes of one RGBA frame.
    pub fn rgba_frame_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// About 0.08 bit per pixel and frame: plenty for strokes on a plain page.
fn default_video_bitrate(width: u32, height: u32, frame_rate: u32) -> u32 {
    let bits = u64::from(width) * u64::from(height) * u64::from(frame_rate) * 8 / 100;
    u32::try_from(bits.clamp(500_000, 20_000_000)).unwrap_or(20_000_000)
}

/// An encoder Media Foundation chose or offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderInfo {
    /// Friendly name registered by the encoder, when it has one.
    pub name: Option<String>,
    /// `true` for a GPU hardware encoder.
    pub hardware: bool,
}

impl fmt::Display for EncoderInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.hardware {
            "hardware"
        } else {
            "software"
        };
        match &self.name {
            Some(name) => write!(f, "{name} ({kind})"),
            None => write!(f, "unnamed {kind} encoder"),
        }
    }
}

/// Encoders registered with Media Foundation, best first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AvailableEncoders {
    pub h264: Vec<EncoderInfo>,
    pub aac: Vec<EncoderInfo>,
}

/// What [`crate::Mp4Writer::finish`] wrote, according to the writer's own counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp4Summary {
    pub video_frames: u64,
    /// Audio frames (one sample per channel) written.
    pub audio_frames: u64,
    /// End of the last video frame or audio sample, whichever is later.
    pub duration: Duration,
    /// H.264 encoder the sink writer loaded.
    pub video_encoder: EncoderInfo,
}

/// Codec of a track read back from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    H264,
    Aac,
    /// Any other Media Foundation subtype, as its GUID.
    Other(String),
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Codec::H264 => f.write_str("H.264"),
            Codec::Aac => f.write_str("AAC"),
            Codec::Other(guid) => write!(f, "other ({guid})"),
        }
    }
}

/// What [`crate::probe_mp4`] should keep besides the counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProbeOptions {
    /// Index of a decoded video frame to return as RGBA.
    pub capture_frame: Option<u64>,
    /// Return every decoded audio sample (interleaved i16).
    pub capture_audio: bool,
}

/// A decoded video frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    /// `width × height` RGBA pixels, top row first.
    pub pixels: Vec<u8>,
}

impl RgbaFrame {
    /// The `[r, g, b, a]` value at (`x`, `y`), if inside the frame.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels.get(i..i + 4)?;
        Some([p[0], p[1], p[2], p[3]])
    }
}

/// Video track of a file read back by [`crate::probe_mp4`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoTrackProbe {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    /// Frames actually decoded.
    pub frames: u64,
    /// Timestamp of the first decoded frame.
    pub first_timestamp: Duration,
    /// Timestamp of the last decoded frame.
    pub last_timestamp: Duration,
}

/// Audio track of a file read back by [`crate::probe_mp4`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrackProbe {
    pub codec: Codec,
    pub channels: u16,
    pub sample_rate: u32,
    /// Audio frames (one sample per channel) actually decoded.
    pub frames: u64,
    /// `frames / sample_rate`.
    pub duration: Duration,
}

/// A media file read back through Media Foundation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp4Probe {
    /// Duration declared by the container.
    pub duration: Duration,
    pub video: Option<VideoTrackProbe>,
    pub audio: Option<AudioTrackProbe>,
    /// The frame asked for in [`ProbeOptions::capture_frame`].
    pub captured_frame: Option<RgbaFrame>,
    /// Decoded audio, if [`ProbeOptions::capture_audio`] was set.
    pub audio_samples: Option<Vec<i16>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid() {
        let settings = Mp4Settings::new(1280, 720, 30).with_audio(AudioSettings::new(48_000, 2));
        settings.validate().unwrap();
        assert_eq!(settings.video_bitrate, 2_211_840);
        assert_eq!(settings.rgba_frame_len(), 1280 * 720 * 4);
        assert!(settings.hardware_acceleration);
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(Mp4Settings::new(1279, 720, 30).validate().is_err());
        assert!(Mp4Settings::new(1280, 8, 30).validate().is_err());
        assert!(Mp4Settings::new(8192, 720, 30).validate().is_err());
        assert!(Mp4Settings::new(1280, 720, 0).validate().is_err());
        assert!(Mp4Settings::new(1280, 720, 120).validate().is_err());
        let mut s = Mp4Settings::new(1280, 720, 30);
        s.video_bitrate = 10;
        assert!(s.validate().is_err());
        let s = Mp4Settings::new(1280, 720, 30).with_audio(AudioSettings::new(16_000, 1));
        assert!(s.validate().is_err());
        let s = Mp4Settings::new(1280, 720, 30).with_audio(AudioSettings::new(48_000, 6));
        assert!(s.validate().is_err());
        let s = Mp4Settings::new(1280, 720, 30).with_audio(AudioSettings::new(44_100, 1));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn default_bitrate_is_clamped() {
        assert_eq!(default_video_bitrate(64, 64, 1), 500_000);
        assert_eq!(default_video_bitrate(4096, 4096, 60), 20_000_000);
    }

    #[test]
    fn rgba_frame_pixel() {
        let frame = RgbaFrame {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(frame.pixel(1, 0), Some([5, 6, 7, 8]));
        assert_eq!(frame.pixel(2, 0), None);
    }
}
