//! MP4 export behind the same [`VideoSink`] as the other formats: Windows Media Foundation by
//! default (H.264 + AAC, nothing to download), Cisco's OpenH264 2.6.0 as the fallback when
//! Media Foundation is missing (H.264 by OpenH264, sound in Opus), SPEC §6.2 "Vidéo MP4".
//!
//! M6 contract: the Media Foundation path works; the OpenH264 path and
//! [`OpenH264Codec`]'s download are filled in by the MP4 work package (see
//! `docs/design/media.md`). Until then they answer [`MediaError::CodecUnavailable`].

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::{
    AnimationFormat, AnimationProbe, AudioProbe, AudioSettings, Codec, MediaError, Mp4Settings,
    Mp4Writer, NARRATION_SAMPLE_RATE, ProbeOptions, VideoSettings, VideoSink, VideoSummary,
    probe_mp4,
};

/// The notice Cisco's licence requires wherever the user controls the codec (SPEC §6.2).
pub const CISCO_NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";

/// The only OpenH264 release the `openh264` crate accepts by hash (docs/research/media-stack.md §1).
pub const OPENH264_VERSION: &str = "2.6.0";

/// Which H.264 encoder writes the MP4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mp4Backend {
    /// Media Foundation when it is there, else OpenH264 when it is installed and allowed.
    #[default]
    Auto,
    MediaFoundation,
    /// Forces the fallback (tests, and PCs whose Media Foundation misbehaves).
    OpenH264,
}

/// MP4 options on top of [`VideoSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4Options {
    pub backend: Mp4Backend,
    /// Lets Media Foundation use the GPU's encoder (ignored by OpenH264).
    pub hardware_acceleration: bool,
    /// Average H.264 bitrate in bits per second; `None` for a default suited to the size.
    pub video_bitrate: Option<u32>,
}

impl Default for Mp4Options {
    fn default() -> Self {
        Self {
            backend: Mp4Backend::Auto,
            hardware_acceleration: true,
            video_bitrate: None,
        }
    }
}

/// State of the OpenH264 library on this PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecStatus {
    NotInstalled,
    /// Present, with the expected SHA-256.
    Installed {
        path: PathBuf,
        bytes: u64,
    },
    /// A file is there but does not match the expected hash (it is never loaded).
    Damaged(String),
}

/// The OpenH264 library, downloaded from Cisco on the user's request into `dir` (the app uses
/// `%LOCALAPPDATA%\Ardoise\codecs`). Whether the user allows its use is an app setting: pass
/// `None` to [`create_mp4`] when it is disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenH264Codec {
    dir: PathBuf,
}

impl OpenH264Codec {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Folder holding the library.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether the library is installed and intact.
    pub fn status(&self) -> CodecStatus {
        CodecStatus::NotInstalled
    }

    /// Downloads the library from Cisco's server, checks its SHA-256 and installs it
    /// atomically. `progress(received, total)` is called as bytes arrive; `cancel` stops it.
    pub fn download(
        &self,
        _progress: &mut dyn FnMut(u64, Option<u64>),
        _cancel: &AtomicBool,
    ) -> Result<(), MediaError> {
        Err(MediaError::CodecUnavailable(
            "the OpenH264 download is not implemented yet".into(),
        ))
    }

    /// Deletes the library (the user can remove it at any time).
    pub fn remove(&self) -> Result<(), MediaError> {
        Err(MediaError::CodecUnavailable(
            "removing OpenH264 is not implemented yet".into(),
        ))
    }
}

/// Creates an MP4 writer. `openh264` is the fallback library when the user allows it.
pub fn create_mp4(
    path: &Path,
    settings: &VideoSettings,
    options: &Mp4Options,
    openh264: Option<&OpenH264Codec>,
) -> Result<Box<dyn VideoSink>, MediaError> {
    let _ = openh264;
    match options.backend {
        Mp4Backend::Auto | Mp4Backend::MediaFoundation => {
            let mut mp4 = Mp4Settings::new(settings.width, settings.height, settings.frame_rate);
            if let Some(channels) = settings.audio_channels {
                mp4 = mp4.with_audio(AudioSettings::new(NARRATION_SAMPLE_RATE, channels));
            }
            if let Some(bitrate) = options.video_bitrate {
                mp4.video_bitrate = bitrate;
            }
            mp4.hardware_acceleration = options.hardware_acceleration;
            let writer = Mp4Writer::create(path, &mp4)?;
            Ok(Box::new(MediaFoundationSink { writer }))
        }
        Mp4Backend::OpenH264 => Err(MediaError::CodecUnavailable(
            "the OpenH264 fallback is not implemented yet".into(),
        )),
    }
}

/// Reads an MP4 back independently of its writer (either back end).
pub fn probe_mp4_animation(path: &Path) -> Result<AnimationProbe, MediaError> {
    let probe = probe_mp4(path, ProbeOptions::default())?;
    let video = probe
        .video
        .ok_or_else(|| MediaError::InvalidFile("no video track".into()))?;
    Ok(AnimationProbe {
        format: AnimationFormat::Mp4,
        width: video.width,
        height: video.height,
        frames: video.frames,
        duration: probe.duration,
        audio: probe.audio.map(|audio| AudioProbe {
            codec: match audio.codec {
                Codec::Aac => "AAC".into(),
                other => other.to_string(),
            },
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            duration: audio.duration,
        }),
    })
}

/// [`Mp4Writer`] as a [`VideoSink`].
struct MediaFoundationSink {
    writer: Mp4Writer,
}

impl VideoSink for MediaFoundationSink {
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
        self.writer.push_frame(rgba, timestamp)
    }

    fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError> {
        self.writer.push_audio(samples)
    }

    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
        let summary = self.writer.finish()?;
        let encoder = &summary.video_encoder;
        let name = encoder.name.as_deref().unwrap_or("software encoder");
        let kind = if encoder.hardware {
            "hardware"
        } else {
            "software"
        };
        Ok(VideoSummary {
            format: AnimationFormat::Mp4,
            frames: summary.video_frames,
            duration: summary.duration,
            audio: summary.audio_frames > 0,
            encoder: format!("Media Foundation: {name} ({kind})"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_openh264_parts_are_not_there_yet() {
        let codec = OpenH264Codec::new("unused");
        assert_eq!(codec.status(), CodecStatus::NotInstalled);
        let settings = VideoSettings {
            width: 64,
            height: 64,
            frame_rate: 10,
            audio_channels: None,
        };
        let options = Mp4Options {
            backend: Mp4Backend::OpenH264,
            ..Mp4Options::default()
        };
        let error = create_mp4(Path::new("unused.mp4"), &settings, &options, Some(&codec))
            .err()
            .unwrap();
        assert!(matches!(error, MediaError::CodecUnavailable(_)));
    }

    /// The Media Foundation path through the shared sink, read back by the probe.
    #[cfg(windows)]
    #[test]
    fn media_foundation_writes_through_the_video_sink() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sink.mp4");
        let settings = VideoSettings {
            width: 320,
            height: 240,
            frame_rate: 10,
            audio_channels: Some(1),
        };
        let mut sink = create_mp4(&path, &settings, &Mp4Options::default(), None).unwrap();
        let frame = vec![200u8; 320 * 240 * 4];
        let audio = vec![0i16; 4_800];
        for k in 0..10u64 {
            sink.push_audio(&audio).unwrap();
            sink.push_frame(&frame, Duration::from_millis(100 * k))
                .unwrap();
        }
        let summary = sink.finish().unwrap();
        assert_eq!(
            (summary.format, summary.frames, summary.audio),
            (AnimationFormat::Mp4, 10, true)
        );
        assert!(summary.encoder.starts_with("Media Foundation: "));
        let probe = probe_mp4_animation(&path).unwrap();
        assert_eq!((probe.width, probe.height, probe.frames), (320, 240, 10));
        let audio = probe.audio.unwrap();
        assert_eq!((audio.codec.as_str(), audio.channels), ("AAC", 1));
    }
}
