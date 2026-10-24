//! Estimated export time, shown before exporting (SPEC §8): from the speeds measured on the
//! development PC at 1080p, scaled by the pixel count (`docs/design/media.md`).
//!
//! Contract stub: the body is filled in by the media pipeline work package.

use std::time::Duration;

use crate::AnimationFormat;

/// How long exporting `frames` frames of `width × height` to `format` should take.
pub fn estimate_export_time(
    format: AnimationFormat,
    width: u32,
    height: u32,
    frames: u64,
) -> Duration {
    let _ = (format, width, height, frames);
    Duration::ZERO
}
