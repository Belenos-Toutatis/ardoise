//! H.264 encoding by Cisco's OpenH264 library, loaded at run time from the DLL the user
//! downloaded (`openh264` crate in `libloading` mode: the library is checked against the crate's
//! list of known hashes before it is loaded, besides our own pinned SHA-256).
//!
//! Settings, chosen to match the Media Foundation writer: no frame ever skipped by the rate
//! control (every pushed frame is in the file), a key frame every two seconds, the average
//! bitrate of `Mp4Settings` (0.08 bit per pixel and frame unless the caller sets one) in
//! quality mode, and the BT.709 limited-range colour description in the SPS (the colours
//! `nv12.rs` produces). OpenH264 writes the Constrained Baseline profile: no B-frames, so
//! decoding order is display order.
//!
//! Usage type: OpenH264 2.6.0 accepts only its two real-time modes. Measured at 1080p, 30 fps
//! on the reference note of `tests/animated_exports.rs` (development PC, release build,
//! `mp4_speed_1080p`): camera mode 116 to 124 frames per second, 756 kB for 11.5 s, last frame
//! at 31.0 dB PSNR; screen-content mode 65 frames per second, 458 kB, 30.5 dB. Camera mode is
//! used: twice as fast for the same quality, and files stay ten times smaller than those of
//! the hardware encoder (7.4 MB).

use std::path::Path;
use std::time::Duration;

use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod,
    RateControlMode, UsageType, VuiConfig,
};
use openh264::formats::YUVSlices;
use openh264::{OpenH264API, Timestamp};

use crate::{MediaError, nv12};

/// Seconds between key frames (as the Media Foundation writer).
const KEY_FRAME_INTERVAL_SECONDS: u32 = 2;

/// An OpenH264 encoder for frames of one size.
pub(crate) struct H264Encoder {
    encoder: Encoder,
    width: usize,
    height: usize,
    nv12: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl H264Encoder {
    /// Loads `library` and prepares an encoder (`width` and `height` even).
    pub(crate) fn new(
        library: &Path,
        width: u32,
        height: u32,
        frame_rate: u32,
        bitrate: u32,
    ) -> Result<Self, MediaError> {
        let api = OpenH264API::from_blob_path(library).map_err(|e| {
            MediaError::CodecUnavailable(format!("cannot load {}: {e}", library.display()))
        })?;
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(frame_rate as f32))
            .rate_control_mode(RateControlMode::Quality)
            .skip_frames(false)
            .usage_type(UsageType::CameraVideoRealTime)
            .complexity(Complexity::Medium)
            .intra_frame_period(IntraFramePeriod::from_num_frames(
                frame_rate * KEY_FRAME_INTERVAL_SECONDS,
            ))
            .vui(VuiConfig::bt709())
            .num_threads(0);
        let encoder = Encoder::with_api_config(api, config).map_err(|e| h264_error(&e))?;
        let (width, height) = (width as usize, height as usize);
        let chroma = (width / 2) * (height / 2);
        Ok(Self {
            encoder,
            width,
            height,
            nv12: vec![0; nv12::nv12_len(width, height)],
            u: vec![0; chroma],
            v: vec![0; chroma],
        })
    }

    /// Encodes one opaque RGBA frame; returns the access unit in Annex B.
    pub(crate) fn encode(
        &mut self,
        rgba: &[u8],
        timestamp: Duration,
    ) -> Result<Vec<u8>, MediaError> {
        nv12::rgba_to_nv12(rgba, self.width, self.height, &mut self.nv12)?;
        let (y, uv) = self.nv12.split_at(self.width * self.height);
        for ((pair, u), v) in uv
            .as_chunks::<2>()
            .0
            .iter()
            .zip(self.u.iter_mut())
            .zip(self.v.iter_mut())
        {
            *u = pair[0];
            *v = pair[1];
        }
        let half = self.width / 2;
        let yuv = YUVSlices::new(
            (y, &self.u, &self.v),
            (self.width, self.height),
            (self.width, half, half),
        );
        let millis = u64::try_from(timestamp.as_millis()).unwrap_or(u64::MAX);
        let stream = self
            .encoder
            .encode_at(&yuv, Timestamp::from_millis(millis))
            .map_err(|e| h264_error(&e))?;
        match stream.frame_type() {
            FrameType::IDR | FrameType::I | FrameType::P | FrameType::IPMixed => {}
            other => {
                return Err(h264_error(&format!(
                    "OpenH264 returned a frame of type {other:?}"
                )));
            }
        }
        Ok(stream.to_vec())
    }
}

fn h264_error(error: &dyn std::fmt::Display) -> MediaError {
    MediaError::Encoding {
        codec: "H.264",
        message: format!("OpenH264: {error}"),
    }
}
