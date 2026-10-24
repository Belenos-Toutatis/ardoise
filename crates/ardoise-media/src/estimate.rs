//! Estimated export time, shown before exporting (SPEC §8): from the speeds measured on the
//! development PC at 1080p, scaled by the pixel count (`docs/design/media.md`).
//!
//! Encoding dominates (drawing a frame is negligible) and costs about the same per pixel, so a
//! format's frames per second at 1920 × 1080 are scaled by the ratio of pixel counts. MP4 takes
//! the hardware figure of Media Foundation, the slower of its two (the encoder it will pick is
//! not known in advance); a fixed half second covers opening and closing the file.

use std::time::Duration;

use crate::AnimationFormat;

/// Pixels of the frames the speeds were measured on (1080p).
const REFERENCE_PIXELS: f64 = 1920.0 * 1080.0;
/// Opening and closing the file.
const FIXED_COST_SECONDS: f64 = 0.5;

/// Frames per second at 1920 × 1080 on the development PC (`docs/design/media.md`).
fn frames_per_second_at_1080p(format: AnimationFormat) -> f64 {
    match format {
        AnimationFormat::Gif => 310.0,
        AnimationFormat::WebP => 24.0,
        AnimationFormat::WebM => 20.0,
        AnimationFormat::Mp4 => 100.0,
    }
}

/// How long exporting `frames` frames of `width × height` to `format` should take.
pub fn estimate_export_time(
    format: AnimationFormat,
    width: u32,
    height: u32,
    frames: u64,
) -> Duration {
    let pixels = f64::from(width) * f64::from(height);
    let per_frame = pixels / REFERENCE_PIXELS / frames_per_second_at_1080p(format);
    let seconds = FIXED_COST_SECONDS + frames as f64 * per_frame;
    Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMATS: [AnimationFormat; 4] = [
        AnimationFormat::Mp4,
        AnimationFormat::Gif,
        AnimationFormat::WebP,
        AnimationFormat::WebM,
    ];

    fn seconds(format: AnimationFormat, width: u32, height: u32, frames: u64) -> f64 {
        estimate_export_time(format, width, height, frames).as_secs_f64()
    }

    #[test]
    fn measured_speeds_at_1080p() {
        // A 2-minute replay at 30 fps: 3600 frames.
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(close(seconds(AnimationFormat::Gif, 1920, 1080, 3100), 10.5));
        assert!(close(
            seconds(AnimationFormat::WebP, 1920, 1080, 3600),
            150.5
        ));
        assert!(close(
            seconds(AnimationFormat::WebM, 1920, 1080, 3600),
            180.5
        ));
        assert!(close(seconds(AnimationFormat::Mp4, 1920, 1080, 3600), 36.5));
        // A quarter of the pixels, a quarter of the time (720p is 4/9 of 1080p).
        assert!(close(seconds(AnimationFormat::WebM, 960, 540, 3600), 45.5));
        assert!(close(seconds(AnimationFormat::Mp4, 1280, 720, 900), 4.5));
        // 4K: four times the pixels.
        assert!(close(seconds(AnimationFormat::Gif, 3840, 2160, 310), 4.5));
    }

    #[test]
    fn opening_the_file_always_costs_half_a_second() {
        for format in FORMATS {
            assert_eq!(
                estimate_export_time(format, 1920, 1080, 0),
                Duration::from_millis(500)
            );
            assert_eq!(
                estimate_export_time(format, 0, 0, 1000),
                Duration::from_millis(500)
            );
        }
    }

    #[test]
    fn more_frames_or_pixels_never_take_less_time() {
        for format in FORMATS {
            let mut previous = Duration::ZERO;
            for frames in [0, 1, 2, 10, 100, 3600, 1_000_000, u64::MAX] {
                let time = estimate_export_time(format, 1280, 720, frames);
                assert!(time >= previous, "{format:?}: {frames} frames");
                previous = time;
            }
            let mut previous = Duration::ZERO;
            for (width, height) in [(2, 2), (64, 48), (1280, 720), (1920, 1080), (3840, 2160)] {
                let time = estimate_export_time(format, width, height, 600);
                assert!(time > previous, "{format:?}: {width} x {height}");
                previous = time;
            }
        }
        // Slowest to fastest, as measured.
        let at_1080p = |format| estimate_export_time(format, 1920, 1080, 600);
        assert!(at_1080p(AnimationFormat::WebM) > at_1080p(AnimationFormat::WebP));
        assert!(at_1080p(AnimationFormat::WebP) > at_1080p(AnimationFormat::Mp4));
        assert!(at_1080p(AnimationFormat::Mp4) > at_1080p(AnimationFormat::Gif));
        // Absurd inputs saturate instead of panicking.
        assert_eq!(
            estimate_export_time(AnimationFormat::WebM, u32::MAX, u32::MAX, u64::MAX),
            Duration::MAX
        );
    }
}
