//! Animated WebP export (SPEC §4.5) through libwebp's `WebPAnimEncoder`, wrapped by the
//! `webp-animation` crate (MIT OR Apache-2.0; libwebp is BSD-3-Clause, compiled from source by
//! `cc`, no cmake).
//!
//! - **Lossless by default.** Handwriting is a few flat colours with anti-aliased edges:
//!   lossless WebP (VP8L, with its colour-cache and palette transforms) keeps every pixel and is
//!   *smaller* than lossy WebP at a quality that keeps thin lines clean, because lossy VP8
//!   codes 4:2:0 YUV in 4×4 blocks and needs many bits to avoid ringing and colour bleeding
//!   around 2-pixel strokes. Measured: a page written for 10 s at 1080p takes 120 kB lossless
//!   against 251 kB at lossy quality 90, which still errs by up to 47 levels on stroke edges
//!   (tests and the `animated_speed_1080p` benchmark in `gif.rs`). Lossy remains available
//!   ([`WebpCompression::Lossy`]).
//! - libwebp stores only the rectangle that changed in each frame, and merges a frame identical
//!   to the previous one into it. Key frames are disabled (smallest files; players decode from
//!   the start anyway).
//! - **Speed**: `WebPAnimEncoder` analyses the whole canvas for every frame, whatever the
//!   compression: about 24 frames per second at 1080p on the development PC (a 2-minute replay
//!   at 30 fps takes about 2.5 minutes; GIF takes 11 s). Lower resolutions scale accordingly.
//! - **Timing** in milliseconds, each frame's end rounded on the absolute timeline (see
//!   `gif::Retimer`). Browsers show frames of 10 ms or less for 100 ms, so frames shorter than
//!   11 ms are merged into the next one.
//!
//! [`probe_webp`] reads a file back on its own: the RIFF chunks (canvas, `ANMF` frames and their
//! durations), then every frame decoded by libwebp's `WebPAnimDecoder`, which must agree.

use std::path::Path;
use std::time::Duration;

use webp_animation::{
    AnimParams, Decoder, Encoder, EncoderOptions, EncodingConfig, EncodingType, LossyEncodingConfig,
};

use crate::gif::{Retimer, validate_settings};
use crate::{AnimationFormat, AnimationProbe, MediaError, VideoSettings, VideoSink, VideoSummary};

/// Options of a WebP export.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebpOptions {
    /// Plays the animation again and again (`true`, the default) or once.
    pub loop_forever: bool,
    pub compression: WebpCompression,
}

impl Default for WebpOptions {
    fn default() -> Self {
        Self {
            loop_forever: true,
            compression: WebpCompression::Lossless,
        }
    }
}

/// How WebP frames are compressed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WebpCompression {
    /// Exact pixels (VP8L): the default, sharp and small for handwriting.
    Lossless,
    /// VP8 at `quality` (0 to 100), with sharp RGB to YUV conversion.
    Lossy { quality: f32 },
}

/// WebP durations are in milliseconds.
const WEBP_TICKS_PER_SECOND: u64 = 1000;
/// Browsers show frames of 10 ms or less for 100 ms.
const WEBP_MIN_DURATION_MS: u64 = 11;
/// Largest duration of one `ANMF` frame (24 bits).
const WEBP_MAX_DURATION_MS: u64 = (1 << 24) - 1;
/// Largest canvas side of a WebP file.
const WEBP_MAX_DIMENSION: u32 = 16_383;
/// Lossless effort (libwebp's `quality` for VP8L: 0 fastest, 100 smallest) and method.
const LOSSLESS_EFFORT: f32 = 50.0;
const LOSSLESS_METHOD: usize = 2;
/// Lossy method (0 fastest, 6 smallest).
const LOSSY_METHOD: usize = 4;
/// Largest canvas `webp-animation`'s decoder accepts (it refuses bigger ones).
const DECODER_MAX_PIXELS: u64 = 3840 * 2160;

/// Starts a WebP export to `path` (written when the sink finishes: libwebp assembles the
/// file in memory).
pub fn create_webp(
    path: &Path,
    settings: &VideoSettings,
    options: &WebpOptions,
) -> Result<Box<dyn VideoSink>, MediaError> {
    validate_settings(settings, 1, WEBP_MAX_DIMENSION, false)?;
    let encoding_config = match options.compression {
        WebpCompression::Lossless => EncodingConfig {
            encoding_type: EncodingType::Lossless,
            quality: LOSSLESS_EFFORT,
            method: LOSSLESS_METHOD,
        },
        WebpCompression::Lossy { quality } => EncodingConfig {
            encoding_type: EncodingType::Lossy(LossyEncodingConfig {
                use_sharp_yuv: true,
                ..LossyEncodingConfig::new_from_drawing_preset()
            }),
            quality: quality.clamp(0.0, 100.0),
            method: LOSSY_METHOD,
        },
    };
    let encoder = Encoder::new_with_options(
        (settings.width, settings.height),
        EncoderOptions {
            anim_params: AnimParams {
                loop_count: if options.loop_forever { 0 } else { 1 },
            },
            // No key frames (`kmax` 0): only changed rectangles after the first frame.
            kmin: 0,
            kmax: 0,
            encoding_config: Some(encoding_config),
            ..EncoderOptions::default()
        },
    )
    .map_err(webp_error)?;
    Ok(Box::new(WebpSink {
        path: path.to_path_buf(),
        retimer: Retimer::new(
            settings,
            WEBP_TICKS_PER_SECOND,
            WEBP_MIN_DURATION_MS,
            WEBP_MAX_DURATION_MS,
        ),
        out: WebpOut {
            encoder,
            written_ms: 0,
        },
        frames_pushed: 0,
        lossless: options.compression == WebpCompression::Lossless,
    }))
}

struct WebpSink {
    path: std::path::PathBuf,
    retimer: Retimer,
    out: WebpOut,
    frames_pushed: u64,
    lossless: bool,
}

struct WebpOut {
    encoder: Encoder,
    /// Start of the next frame, in milliseconds.
    written_ms: u64,
}

impl WebpOut {
    fn write(&mut self, rgba: &[u8], duration_ms: u64) -> Result<(), MediaError> {
        self.encoder
            .add_frame(rgba, timestamp_ms(self.written_ms)?)
            .map_err(webp_error)?;
        self.written_ms += duration_ms;
        Ok(())
    }
}

impl VideoSink for WebpSink {
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
        let out = &mut self.out;
        self.retimer
            .push(rgba, timestamp, |rgba, ms| out.write(rgba, ms))?;
        self.frames_pushed += 1;
        Ok(())
    }

    fn push_audio(&mut self, _samples: &[i16]) -> Result<(), MediaError> {
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
        let WebpSink {
            path,
            mut retimer,
            mut out,
            frames_pushed,
            lossless,
        } = *self;
        if frames_pushed == 0 {
            return Err(MediaError::InvalidSettings(
                "no video frame was pushed".into(),
            ));
        }
        let duration = retimer.finish(|rgba, ms| out.write(rgba, ms))?;
        let data = out
            .encoder
            .finalize(timestamp_ms(out.written_ms)?)
            .map_err(webp_error)?;
        std::fs::write(&path, &*data)?;
        Ok(VideoSummary {
            format: AnimationFormat::WebP,
            frames: frames_pushed,
            duration,
            audio: false,
            encoder: format!("libwebp ({})", if lossless { "lossless" } else { "lossy" }),
        })
    }
}

fn timestamp_ms(ms: u64) -> Result<i32, MediaError> {
    i32::try_from(ms).map_err(|_| MediaError::InvalidSettings(format!("WebP longer than {ms} ms")))
}

fn webp_error(error: webp_animation::Error) -> MediaError {
    MediaError::Encoding {
        codec: "WebP",
        message: error.to_string(),
    }
}

/// What the RIFF chunks of a WebP file say.
#[derive(Debug, Default, PartialEq, Eq)]
struct RiffSummary {
    /// Canvas size from the `VP8X` chunk (absent in a still image without extensions).
    canvas: Option<(u32, u32)>,
    /// Number of `ANMF` chunks (0 for a still image).
    frames: u64,
    /// Sum of the `ANMF` durations, in milliseconds.
    duration_ms: u64,
}

fn u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

/// Walks the chunks of a WebP file.
fn read_riff(data: &[u8]) -> Result<RiffSummary, MediaError> {
    let invalid = |what: &str| MediaError::InvalidFile(format!("WebP: {what}"));
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return Err(invalid("not a RIFF WEBP file"));
    }
    let riff_end = 8 + u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let body = data
        .get(12..riff_end)
        .ok_or_else(|| invalid("truncated file"))?;
    let mut summary = RiffSummary::default();
    let mut rest = body;
    while !rest.is_empty() {
        let header = rest
            .get(..8)
            .ok_or_else(|| invalid("truncated chunk header"))?;
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let payload = rest
            .get(8..8 + size)
            .ok_or_else(|| invalid("truncated chunk"))?;
        match &header[..4] {
            b"VP8X" => {
                let fields = payload
                    .get(..10)
                    .ok_or_else(|| invalid("short VP8X chunk"))?;
                summary.canvas = Some((u24(&fields[4..7]) + 1, u24(&fields[7..10]) + 1));
            }
            b"ANMF" => {
                let fields = payload
                    .get(..16)
                    .ok_or_else(|| invalid("short ANMF chunk"))?;
                summary.frames += 1;
                summary.duration_ms += u64::from(u24(&fields[12..15]));
            }
            _ => {}
        }
        // Chunks are padded to an even size.
        let next = (8 + size + 1) & !1;
        rest = rest.get(next..).unwrap_or(&[]);
    }
    Ok(summary)
}

/// Reads a WebP file back: chunk walk, then every frame decoded by libwebp (canvases up to
/// 3840 × 2160; above that, the frame count and durations come from the chunks only).
pub fn probe_webp(path: &Path) -> Result<AnimationProbe, MediaError> {
    let data = std::fs::read(path)?;
    let riff = read_riff(&data)?;
    let decodable = riff
        .canvas
        .is_none_or(|(w, h)| u64::from(w) * u64::from(h) <= DECODER_MAX_PIXELS);
    if !decodable {
        let (width, height) = riff.canvas.unwrap_or((0, 0));
        return Ok(AnimationProbe {
            format: AnimationFormat::WebP,
            width,
            height,
            frames: riff.frames,
            duration: Duration::from_millis(riff.duration_ms),
            audio: None,
        });
    }

    let decoder = Decoder::new(&data).map_err(|e| MediaError::InvalidFile(format!("WebP: {e}")))?;
    let (width, height) = decoder.dimensions();
    let mut frames = 0u64;
    let mut end_ms = 0i32;
    for frame in decoder {
        frames += 1;
        // libwebp gives each frame's end time.
        end_ms = frame.timestamp();
    }
    let duration_ms = u64::try_from(end_ms).unwrap_or(0);
    if riff.frames > 0 && (frames != riff.frames || duration_ms != riff.duration_ms) {
        return Err(MediaError::InvalidFile(format!(
            "WebP: {frames} frames decoded ending at {duration_ms} ms, the file lists {} \
             frames lasting {} ms",
            riff.frames, riff.duration_ms
        )));
    }
    Ok(AnimationProbe {
        format: AnimationFormat::WebP,
        width,
        height,
        frames,
        duration: Duration::from_millis(duration_ms),
        audio: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gif::tests::{growing_line, print_probe_table, settings};

    fn write_line(path: &Path, compression: WebpCompression) -> (VideoSummary, Duration) {
        let (width, height, fps, frames) = (640, 360, 15, 30);
        let started = std::time::Instant::now();
        let options = WebpOptions {
            compression,
            ..WebpOptions::default()
        };
        let mut sink = create_webp(path, &settings(width, height, fps), &options).unwrap();
        for i in 0..frames {
            let frame = growing_line(width, height, i, frames);
            sink.push_frame(&frame, Duration::from_secs(u64::from(i)) / fps)
                .unwrap();
        }
        (sink.finish().unwrap(), started.elapsed())
    }

    #[test]
    fn writes_and_probes_a_webp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("line.webp");
        let (summary, elapsed) = write_line(&path, WebpCompression::Lossless);
        let probe = probe_webp(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        print_probe_table("WebP", &probe, size, elapsed);

        assert_eq!(summary.frames, 30);
        assert_eq!(summary.duration, Duration::from_secs(2));
        assert_eq!((probe.width, probe.height), (640, 360));
        assert_eq!(probe.frames, 30);
        assert_eq!(probe.duration, Duration::from_secs(2));
        assert_eq!(probe.audio, None);
    }

    #[test]
    fn lossless_is_exact_and_smaller_than_sharp_lossy() {
        let dir = tempfile::tempdir().unwrap();
        let mut sizes = Vec::new();
        for (name, compression) in [
            ("lossless", WebpCompression::Lossless),
            ("lossy q90", WebpCompression::Lossy { quality: 90.0 }),
            ("lossy q75", WebpCompression::Lossy { quality: 75.0 }),
        ] {
            let path = dir.path().join(format!("{}.webp", sizes.len()));
            let (_, elapsed) = write_line(&path, compression);
            let size = std::fs::metadata(&path).unwrap().len();
            println!(
                "WebP {name:<10} {size:>8} B  {:.2} s",
                elapsed.as_secs_f64()
            );
            sizes.push(size);

            // Decoded frames: exact in lossless mode.
            let data = std::fs::read(&path).unwrap();
            let last = Decoder::new(&data).unwrap().into_iter().last().unwrap();
            let source = growing_line(640, 360, 29, 30);
            let max_error = last
                .data()
                .iter()
                .zip(&source)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            println!("     largest channel error of the last frame: {max_error}");
            if compression == WebpCompression::Lossless {
                assert_eq!(max_error, 0);
            }
        }
        assert!(
            sizes[0] < sizes[1],
            "lossless {} vs lossy q90 {}",
            sizes[0],
            sizes[1]
        );
    }

    /// A RIFF WEBP file made of `chunks` (fourcc, payload), padded as the format requires.
    fn riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut file = b"RIFF\0\0\0\0WEBP".to_vec();
        for (fourcc, payload) in chunks {
            file.extend_from_slice(*fourcc);
            file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            file.extend_from_slice(payload);
            if payload.len() % 2 == 1 {
                file.push(0);
            }
        }
        let size = (file.len() - 8) as u32;
        file[4..8].copy_from_slice(&size.to_le_bytes());
        file
    }

    fn anmf(duration_ms: u8) -> Vec<u8> {
        vec![0, 0, 0, 0, 0, 0, 9, 0, 0, 9, 0, 0, duration_ms, 0, 0, 0]
    }

    #[test]
    fn riff_walk_reads_chunks() {
        // VP8X: flags, reserved, then canvas 640 x 360 minus one, 24 bits each.
        let vp8x = vec![0x02, 0, 0, 0, 0x7f, 0x02, 0, 0x67, 0x01, 0];
        let file = riff(&[
            (b"VP8X", vp8x.clone()),
            (b"EXIF", vec![1, 2, 3]), // odd size: padded
            (b"ANMF", anmf(67)),
            (b"ANMF", anmf(100)),
        ]);
        assert_eq!(
            read_riff(&file).unwrap(),
            RiffSummary {
                canvas: Some((640, 360)),
                frames: 2,
                duration_ms: 167,
            }
        );
        assert!(read_riff(&riff(&[(b"ANMF", vec![0; 15])])).is_err());
        assert!(read_riff(&file[..file.len() - 4]).is_err());
        assert!(read_riff(b"RIFF\0\0\0\0WEBQ").is_err());
    }
}
