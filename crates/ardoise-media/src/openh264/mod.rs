//! The MP4 fallback for PCs without Media Foundation (Windows N editions without the Media
//! Feature Pack, or a Media Foundation that fails): H.264 by Cisco's OpenH264 2.6.0, narration
//! in Opus, MP4 container written in pure Rust (SPEC §6.2 row "Vidéo MP4").
//!
//! - [`install`]: the library itself, downloaded from Cisco on the user's request into the
//!   codec folder, SHA-256 checked, installed atomically, removable (Cisco's conditions:
//!   downloaded by the user's PC from Cisco, never bundled; the user can disable and remove it;
//!   the notice [`crate::CISCO_NOTICE`] and Cisco's licence are shown where it is controlled).
//! - `encoder` (Windows): RGBA frames to H.264 through the library.
//! - [`mux`]: the MP4 file (H.264 and Opus tracks), streamed to disk.
//!
//! Opus-in-MP4 plays in browsers (Chrome, Edge, Firefox, Safari) and VLC, not reliably in the
//! Windows Media Player (docs/research/media-stack.md §2): the reason Media Foundation, with
//! AAC, stays the default.

#[cfg(windows)]
mod encoder;
pub(crate) mod install;
// Used by the Windows sink; tested on every platform.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod mux;

use std::path::Path;

use crate::{MediaError, Mp4Options, VideoSettings, VideoSink};

/// Largest frame OpenH264 encodes (its level 5.2 limit: 3840 × 2160, either way round).
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_LONG_SIDE: u32 = 3840;
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_SHORT_SIDE: u32 = 2160;

/// Starts an MP4 export through OpenH264 loaded from `library` (installed and checked by the
/// caller).
#[cfg(windows)]
pub(crate) fn create(
    path: &Path,
    library: &Path,
    settings: &VideoSettings,
    options: &Mp4Options,
) -> Result<Box<dyn VideoSink>, MediaError> {
    sink::OpenH264Sink::create(path, library, settings, options)
        .map(|sink| Box::new(sink) as Box<dyn VideoSink>)
}

/// The library is a Windows build.
#[cfg(not(windows))]
pub(crate) fn create(
    _path: &Path,
    _library: &Path,
    _settings: &VideoSettings,
    _options: &Mp4Options,
) -> Result<Box<dyn VideoSink>, MediaError> {
    Err(MediaError::CodecUnavailable(
        "the OpenH264 fallback is only available on Windows".into(),
    ))
}

/// Checks what OpenH264 and the MP4 container accept.
#[cfg_attr(not(windows), allow(dead_code))]
fn validate(settings: &VideoSettings) -> Result<(), MediaError> {
    crate::gif::validate_settings(settings, 16, MAX_LONG_SIDE, true)?;
    let (long, short) = (
        settings.width.max(settings.height),
        settings.width.min(settings.height),
    );
    if long > MAX_LONG_SIDE || short > MAX_SHORT_SIDE {
        return Err(MediaError::InvalidSettings(format!(
            "{} x {} px (OpenH264 encodes at most {MAX_LONG_SIDE} x {MAX_SHORT_SIDE})",
            settings.width, settings.height
        )));
    }
    if let Some(channels) = settings.audio_channels
        && !matches!(channels, 1 | 2)
    {
        return Err(MediaError::InvalidSettings(format!(
            "{channels} audio channels (mono or stereo only)"
        )));
    }
    Ok(())
}

#[cfg(windows)]
mod sink {
    use std::path::Path;
    use std::time::Duration;

    use super::encoder::H264Encoder;
    use super::mux::{Mp4Muxer, OpusTrack};
    use crate::opus::{OpusEncoder, default_opus_bitrate};
    use crate::{
        AnimationFormat, MediaError, Mp4Options, Mp4Settings, OPENH264_VERSION, VideoSettings,
        VideoSink, VideoSummary,
    };

    /// OpenH264 and the pure-Rust muxer as a [`VideoSink`].
    pub(super) struct OpenH264Sink {
        encoder: H264Encoder,
        mux: Mp4Muxer,
        opus: Option<OpusEncoder>,
        frame_len: usize,
        frame_period: Duration,
        last_timestamp: Option<Duration>,
        /// Narration pushed, in 48 kHz samples per channel.
        narration_samples: u64,
    }

    impl OpenH264Sink {
        pub(super) fn create(
            path: &Path,
            library: &Path,
            settings: &VideoSettings,
            options: &Mp4Options,
        ) -> Result<Self, MediaError> {
            super::validate(settings)?;
            // The same default bitrate as the Media Foundation writer.
            let bitrate = options.video_bitrate.unwrap_or(
                Mp4Settings::new(settings.width, settings.height, settings.frame_rate)
                    .video_bitrate,
            );
            let encoder = H264Encoder::new(
                library,
                settings.width,
                settings.height,
                settings.frame_rate,
                bitrate,
            )?;
            let opus = settings
                .audio_channels
                .map(|channels| OpusEncoder::new(channels, default_opus_bitrate(channels)))
                .transpose()?;
            let track = opus.as_ref().map(|opus| OpusTrack {
                channels: opus.channels(),
                pre_skip: opus.pre_skip(),
            });
            let mux = Mp4Muxer::create(path, settings.width, settings.height, track)?;
            Ok(Self {
                encoder,
                mux,
                opus,
                frame_len: settings.width as usize * settings.height as usize * 4,
                frame_period: Duration::from_secs(1) / settings.frame_rate,
                last_timestamp: None,
                narration_samples: 0,
            })
        }
    }

    impl VideoSink for OpenH264Sink {
        fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
            if rgba.len() != self.frame_len {
                return Err(MediaError::FrameSize {
                    expected: self.frame_len,
                    actual: rgba.len(),
                });
            }
            if let Some(previous) = self.last_timestamp
                && timestamp <= previous
            {
                return Err(MediaError::TimestampOrder {
                    previous,
                    timestamp,
                });
            }
            let access_unit = self.encoder.encode(rgba, timestamp)?;
            self.mux.push_video(&access_unit, timestamp)?;
            self.last_timestamp = Some(timestamp);
            Ok(())
        }

        fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError> {
            let Some(opus) = self.opus.as_mut() else {
                return Err(MediaError::NoAudioTrack);
            };
            let packets = opus.push(samples)?;
            self.narration_samples += (samples.len() / usize::from(opus.channels())) as u64;
            self.mux.push_audio(packets)
        }

        fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
            let OpenH264Sink {
                mut mux,
                opus,
                frame_period,
                narration_samples,
                ..
            } = *self;
            if let Some(opus) = opus
                && narration_samples > 0
            {
                mux.push_audio(opus.finish()?)?;
            }
            let summary = mux.finish(frame_period, narration_samples)?;
            Ok(VideoSummary {
                format: AnimationFormat::Mp4,
                frames: summary.video_frames,
                duration: summary.duration,
                audio: summary.audio,
                encoder: format!("OpenH264 {OPENH264_VERSION} (Cisco), Opus"),
            })
        }
    }
}
