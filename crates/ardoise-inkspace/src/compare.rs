//! Comparing an imported note with Inkspace's own render, `preview_<lastModifiedDate>.png`
//! (1 px = 1 page unit, research §7).
//!
//! Both images are reduced to ink masks (luminance < 200 once composited on white), then
//! scored: intersection over union, and the share of each side's ink lying within 1 px of the
//! other side's ink (geometry check, insensitive to a slightly different stroke thickness).

use std::io::Cursor;
use std::path::Path;

use ardoise_render::{Image, RenderError, RenderOptions};

use crate::ImportedNote;
use crate::reader::TrackedReader;

/// Render scale of the comparison: 0.04 px per device unit = 1 px per page unit.
pub const PREVIEW_SCALE: f32 = 0.04;
/// Luminance below which a pixel counts as ink.
pub const INK_LUMINANCE: f32 = 200.0;
/// Largest preview file read.
const MAX_PREVIEW_BYTES: u64 = 64 << 20;
/// Largest preview side, in pixels.
const MAX_PREVIEW_SIDE: u32 = 16_384;

/// Minimum scores for an import to count as faithful.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Minimum share of ink within 1 px of the other image's ink, both ways.
    pub within_1px: f64,
    /// Minimum intersection over union of the ink masks.
    pub iou: f64,
}

/// Thresholds of the import test: geometry exact (research §7) and thickness close. With the
/// real renderer the 22 notes of this PC score IoU 0.942–0.969, so 0.90 leaves a margin.
pub const THRESHOLDS: Thresholds = Thresholds {
    within_1px: 0.99,
    iou: 0.90,
};

/// Scores of one comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scores {
    /// Intersection over union of the ink masks (1 when both are empty).
    pub iou: f64,
    /// Share of the render's ink within 1 px of preview ink (1 when the render has none).
    pub render_within_1px: f64,
    /// Share of the preview's ink within 1 px of render ink (1 when the preview has none).
    pub preview_within_1px: f64,
    pub render_ink: u64,
    pub preview_ink: u64,
}

impl Scores {
    pub fn passes(&self, thresholds: &Thresholds) -> bool {
        self.iou >= thresholds.iou
            && self.render_within_1px >= thresholds.within_1px
            && self.preview_within_1px >= thresholds.within_1px
    }
}

/// Errors of a comparison.
#[derive(Debug, thiserror::Error)]
pub enum CompareError {
    #[error("Inkspace has no preview image for this note")]
    NoPreview,
    #[error("cannot read the preview {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("the preview {path} is not a usable PNG: {reason}")]
    Png {
        path: std::path::PathBuf,
        reason: String,
    },
    #[error(
        "the render ({render_w} x {render_h}) and the preview ({preview_w} x {preview_h}) differ in size"
    )]
    SizeMismatch {
        render_w: u32,
        render_h: u32,
        preview_w: u32,
        preview_h: u32,
    },
    #[error("cannot render the note: {0}")]
    Render(#[from] RenderError),
}

/// Reads and decodes a preview PNG into an RGBA image.
pub fn load_preview(reader: &TrackedReader, path: &Path) -> Result<Image, CompareError> {
    let bytes = reader
        .read(path, MAX_PREVIEW_BYTES)
        .map_err(|source| CompareError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    decode_png(&bytes).map_err(|reason| CompareError::Png {
        path: path.to_path_buf(),
        reason,
    })
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if width == 0 || height == 0 || width > MAX_PREVIEW_SIDE || height > MAX_PREVIEW_SIDE {
        return Err(format!("unexpected size {width} x {height}"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "image too large".to_string())?;
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let data = buffer.get(..frame.buffer_size()).unwrap_or(&buffer);
    let pixel_count = width as usize * height as usize;
    let mut pixels = Vec::with_capacity(pixel_count * 4);
    let line = frame.line_size;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("palette not expanded".into()),
    };
    if frame.bit_depth != png::BitDepth::Eight {
        return Err("unexpected bit depth".into());
    }
    for row in data.chunks(line).take(height as usize) {
        for px in row.chunks_exact(channels).take(width as usize) {
            let rgba = match *px {
                [g] => [g, g, g, 255],
                [g, a] => [g, g, g, a],
                [r, g, b] => [r, g, b, 255],
                [r, g, b, a] => [r, g, b, a],
                _ => [255, 255, 255, 0],
            };
            pixels.extend_from_slice(&rgba);
        }
    }
    if pixels.len() != pixel_count * 4 {
        return Err("truncated image data".into());
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// Ink mask of an RGBA image (straight alpha), composited on white.
pub fn ink_mask(image: &Image) -> Vec<bool> {
    image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            let alpha = f32::from(a) / 255.0;
            let on_white = |c: u8| f32::from(c) * alpha + 255.0 * (1.0 - alpha);
            let luminance = 0.299 * on_white(r) + 0.587 * on_white(g) + 0.114 * on_white(b);
            luminance < INK_LUMINANCE
        })
        .collect()
}

/// Share of `a`'s ink with `b` ink in its 3×3 neighbourhood, and `a`'s ink count.
fn within_1px(a: &[bool], b: &[bool], width: usize, height: usize) -> (f64, u64) {
    let mut ink = 0u64;
    let mut near = 0u64;
    for y in 0..height {
        for x in 0..width {
            if !a[y * width + x] {
                continue;
            }
            ink += 1;
            let found = (y.saturating_sub(1)..=(y + 1).min(height - 1)).any(|yy| {
                (x.saturating_sub(1)..=(x + 1).min(width - 1)).any(|xx| b[yy * width + xx])
            });
            near += u64::from(found);
        }
    }
    let share = if ink == 0 {
        1.0
    } else {
        near as f64 / ink as f64
    };
    (share, ink)
}

/// Scores `rendered` against `preview` (same size).
pub fn compare_images(rendered: &Image, preview: &Image) -> Result<Scores, CompareError> {
    let expected = rendered.width as usize * rendered.height as usize * 4;
    if rendered.width != preview.width
        || rendered.height != preview.height
        || rendered.pixels.len() != expected
        || preview.pixels.len() != expected
    {
        return Err(CompareError::SizeMismatch {
            render_w: rendered.width,
            render_h: rendered.height,
            preview_w: preview.width,
            preview_h: preview.height,
        });
    }
    let (w, h) = (rendered.width as usize, rendered.height as usize);
    let a = ink_mask(rendered);
    let b = ink_mask(preview);
    let (intersection, union) = a.iter().zip(&b).fold((0u64, 0u64), |(i, u), (&x, &y)| {
        (i + u64::from(x && y), u + u64::from(x || y))
    });
    let (render_within_1px, render_ink) = within_1px(&a, &b, w, h);
    let (preview_within_1px, preview_ink) = within_1px(&b, &a, w, h);
    Ok(Scores {
        iou: if union == 0 {
            1.0
        } else {
            intersection as f64 / union as f64
        },
        render_within_1px,
        preview_within_1px,
        render_ink,
        preview_ink,
    })
}

/// Options of the comparison render: 1 px per page unit, plain white page.
pub fn preview_render_options() -> RenderOptions {
    RenderOptions {
        scale: PREVIEW_SCALE,
        background: false,
    }
}

/// Renders the imported note with `ardoise_render::render_note` and scores it against its
/// Inkspace preview.
pub fn compare_with_preview(
    reader: &TrackedReader,
    imported: &ImportedNote,
) -> Result<Scores, CompareError> {
    let path = imported.preview.as_deref().ok_or(CompareError::NoPreview)?;
    let preview = load_preview(reader, path)?;
    let rendered = ardoise_render::render_note(&imported.note, &preview_render_options())?;
    compare_images(&rendered, &preview)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, ink: &[(u32, u32)]) -> Image {
        let mut pixels = vec![255u8; (width * height * 4) as usize];
        for &(x, y) in ink {
            let i = ((y * width + x) * 4) as usize;
            pixels[i..i + 3].copy_from_slice(&[0, 0, 0]);
        }
        Image {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn identical_images_score_one() {
        let a = image(8, 8, &[(1, 1), (2, 1), (3, 1)]);
        let s = compare_images(&a, &a).unwrap();
        assert_eq!(
            (s.iou, s.render_within_1px, s.preview_within_1px),
            (1.0, 1.0, 1.0)
        );
        assert_eq!((s.render_ink, s.preview_ink), (3, 3));
        assert!(s.passes(&THRESHOLDS));
    }

    #[test]
    fn one_pixel_shift_is_within_1px_but_lowers_iou() {
        let a = image(8, 8, &[(1, 1), (2, 1)]);
        let b = image(8, 8, &[(2, 2), (3, 2)]);
        let s = compare_images(&a, &b).unwrap();
        assert_eq!(s.render_within_1px, 1.0);
        assert_eq!(s.preview_within_1px, 1.0);
        assert_eq!(s.iou, 0.0);
        assert!(!s.passes(&THRESHOLDS));
        let far = image(8, 8, &[(6, 6)]);
        let s = compare_images(&a, &far).unwrap();
        assert_eq!((s.render_within_1px, s.preview_within_1px), (0.0, 0.0));
    }

    #[test]
    fn blank_render_fails_against_inked_preview() {
        let blank = image(4, 4, &[]);
        let inked = image(4, 4, &[(0, 0), (3, 3)]);
        let s = compare_images(&blank, &inked).unwrap();
        assert_eq!(s.render_within_1px, 1.0);
        assert_eq!(s.preview_within_1px, 0.0);
        assert!(!s.passes(&THRESHOLDS));
        let s = compare_images(&blank, &blank).unwrap();
        assert!(s.passes(&THRESHOLDS));
    }

    #[test]
    fn transparent_pixels_are_paper() {
        let mut a = image(2, 1, &[(0, 0), (1, 0)]);
        a.pixels[3] = 0; // first pixel transparent black
        assert_eq!(ink_mask(&a), [false, true]);
    }

    #[test]
    fn size_mismatch() {
        let a = image(2, 2, &[]);
        let b = image(2, 3, &[]);
        assert!(matches!(
            compare_images(&a, &b),
            Err(CompareError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn png_round_trip_and_garbage() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 3, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[
                    0, 0, 0, 255, 255, 255, 10, 20, 30, 1, 2, 3, 4, 5, 6, 7, 8, 9,
                ])
                .unwrap();
        }
        let image = decode_png(&bytes).unwrap();
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(&image.pixels[..8], &[0, 0, 0, 255, 255, 255, 255, 255]);
        assert!(decode_png(b"not a png").is_err());
        assert!(decode_png(&bytes[..bytes.len() / 2]).is_err());
    }
}
