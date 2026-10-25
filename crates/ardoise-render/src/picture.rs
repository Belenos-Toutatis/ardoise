//! Background images (SPEC §4.4, P2): an image or a PDF page imported under the strokes.
//!
//! A note keeps where its image goes ([`BackgroundImage`]: rectangle, quarter turns, opacity)
//! and its file keeps the pixels (the `background.png` or `background.jpg` member). Every format
//! draws them from a [`Picture`], decoded once from that member and handed to the renderer
//! together with the note ([`NoteView`]). Without a picture, or without the note's
//! `background_image`, no image is drawn. The image is part of the page background: it is drawn
//! over the paper colour, the template and the texture, under the strokes, clipped to the page,
//! and only when the background is drawn at all.
//!
//! Import ([`import_image`], [`import_page`]) turns a file chosen by the user into the member
//! Ardoise stores: decoded in pure Rust (PNG and JPEG), turned upright when a photo's EXIF data
//! says so, at most [`MAX_STORED_SIDE`] pixels on its long side, kept as JPEG for opaque photos
//! and as PNG otherwise (transparency, few colours, rendered PDF pages). Hostile files (huge
//! dimensions, broken data) end in a [`PictureError`]; nothing larger than
//! [`MAX_SOURCE_PIXELS`] is ever decoded.

use std::borrow::Cow;
use std::collections::HashSet;
use std::io::Cursor;

use ardoise_core::note::{BackgroundImage, ImageRect, Note};
use tiny_skia::{FilterQuality, Paint, Pattern, Pixmap, Rect, SpreadMode, Transform};

use crate::{Image, check_pixels, encode_png, jpeg};

/// Longest side of a stored background image, in pixels.
pub const MAX_STORED_SIDE: u32 = 4096;
/// Largest image decoded, in pixels (50 megapixels: a 8160 × 6120 photo). Larger files are
/// refused before any pixel is decoded.
pub const MAX_SOURCE_PIXELS: u64 = 50_000_000;
/// JPEG quality of the photos Ardoise stores (a photo that needs no change keeps its bytes).
pub const STORED_JPEG_QUALITY: u8 = 90;
/// An opaque image with at most this many colours (a drawing, a scanned sheet turned to a few
/// tones) is stored as PNG, which keeps its edges sharp.
const FEW_COLOURS: usize = 256;
/// An image drawn more than this many times smaller than its pixels is first reduced by area
/// averaging, so that it does not shimmer (thumbnails, small exports).
const RESAMPLE_ABOVE: f32 = 1.5;

/// Why an image cannot be imported or drawn.
#[derive(Debug, thiserror::Error)]
pub enum PictureError {
    /// Neither PNG nor JPEG.
    #[error("the file is not a PNG or JPEG image")]
    Unsupported,
    /// More than [`MAX_SOURCE_PIXELS`] pixels, or an empty image.
    #[error("the image is too large or empty ({width} x {height} px)")]
    TooLarge { width: u32, height: u32 },
    /// The file cannot be decoded.
    #[error("the image is damaged: {0}")]
    Damaged(String),
    /// The image cannot be encoded again.
    #[error("the image cannot be stored: {0}")]
    Encode(String),
}

/// How a picture's bytes are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureFormat {
    Png,
    Jpeg,
}

impl PictureFormat {
    /// Extension of the note file's member (`background.<extension>`).
    pub fn extension(self) -> &'static str {
        match self {
            PictureFormat::Png => "png",
            PictureFormat::Jpeg => "jpg",
        }
    }

    /// MIME type, for data URIs.
    pub fn mime(self) -> &'static str {
        match self {
            PictureFormat::Png => "image/png",
            PictureFormat::Jpeg => "image/jpeg",
        }
    }

    /// The format of `bytes`, from their signature (the member's extension is not trusted).
    pub fn sniff(bytes: &[u8]) -> Option<PictureFormat> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(PictureFormat::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(PictureFormat::Jpeg)
        } else {
            None
        }
    }
}

/// Colour model of a JPEG that a PDF can take as it is (`/DCTDecode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DctColor {
    Gray,
    Rgb,
}

/// The pixels of a background image: its encoded bytes (embedded as they are by SVG, and by
/// PDF for a JPEG) and the decoded image (drawn by the raster formats).
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    format: PictureFormat,
    bytes: Vec<u8>,
    image: Image,
    /// Set for a JPEG a PDF can embed without decoding it.
    dct: Option<DctColor>,
}

impl Picture {
    /// Decodes a PNG or JPEG file (a note file's background member). Refuses anything else,
    /// damaged data and images of more than [`MAX_SOURCE_PIXELS`] pixels.
    pub fn decode(bytes: Vec<u8>) -> Result<Picture, PictureError> {
        let format = PictureFormat::sniff(&bytes).ok_or(PictureError::Unsupported)?;
        let (image, dct) = match format {
            PictureFormat::Png => (decode_png_limited(&bytes)?, None),
            PictureFormat::Jpeg => {
                let decoded = decode_jpeg_limited(&bytes, None)?;
                (decoded.image, decoded.dct)
            }
        };
        Ok(Picture {
            format,
            bytes,
            image,
            dct,
        })
    }

    pub fn format(&self) -> PictureFormat {
        self.format
    }

    /// The encoded file.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The decoded pixels (straight alpha).
    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn width(&self) -> u32 {
        self.image.width
    }

    pub fn height(&self) -> u32 {
        self.image.height
    }

    pub(crate) fn dct(&self) -> Option<DctColor> {
        self.dct
    }
}

/// A note with the pixels of its background image: what every format draws. A plain `&Note`
/// converts into a view without pixels (no image is drawn).
#[derive(Debug, Clone, Copy)]
pub struct NoteView<'a> {
    pub note: &'a Note,
    /// The decoded `background.*` member of the note's file.
    pub picture: Option<&'a Picture>,
}

impl<'a> NoteView<'a> {
    pub fn new(note: &'a Note, picture: Option<&'a Picture>) -> Self {
        Self { note, picture }
    }

    /// The image to draw, placed on the page; `None` when there is none to draw (no image, no
    /// pixels, an invisible or degenerate placement).
    pub(crate) fn placed(&self) -> Option<PlacedPicture<'a>> {
        let image = self.note.background_image.as_ref()?;
        PlacedPicture::of(image, self.picture?)
    }
}

impl<'a> From<&'a Note> for NoteView<'a> {
    fn from(note: &'a Note) -> Self {
        Self {
            note,
            picture: None,
        }
    }
}

/// An affine map `[a, b, c, d, e, f]`: `x' = a x + c y + e`, `y' = b x + d y + f` (the order of
/// SVG's `matrix()`, PDF's `cm` and tiny-skia's `Transform::from_row`).
pub(crate) type Matrix = [f32; 6];

/// A picture with where it goes on the page.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedPicture<'a> {
    pub picture: &'a Picture,
    /// From the stored pixels (`0..width × 0..height`) to page coordinates.
    pub matrix: Matrix,
    /// Where it is drawn, page coordinates (the image of the pixels by `matrix`).
    pub rect: ImageRect,
    /// In `0 < opacity <= 1`.
    pub opacity: f32,
    /// Turned by an odd number of quarter turns: the stored width runs down the page.
    pub turned: bool,
}

impl<'a> PlacedPicture<'a> {
    fn of(image: &BackgroundImage, picture: &'a Picture) -> Option<Self> {
        let opacity = if image.opacity.is_finite() {
            image.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if opacity <= 0.0 {
            return None;
        }
        let matrix = placement(image, picture.width(), picture.height())?;
        Some(Self {
            picture,
            matrix,
            rect: image.rect,
            opacity,
            turned: image.quarter_turns % 2 == 1,
        })
    }
}

/// The map from the stored pixels of an image `width × height` to the page: turned by
/// `image.quarter_turns` clockwise, then stretched to `image.rect`. `None` for an empty image
/// or a rectangle that is empty, inverted or not finite.
pub(crate) fn placement(image: &BackgroundImage, width: u32, height: u32) -> Option<Matrix> {
    let r = image.rect;
    let finite = [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite());
    if !finite || r.width <= 0.0 || r.height <= 0.0 || width == 0 || height == 0 {
        return None;
    }
    let (w, h) = (width as f32, height as f32);
    // The turn alone: stored pixel (u, v) to the turned image, and the turned image's size.
    let (turn, turned_w, turned_h): (Matrix, f32, f32) = match image.quarter_turns % 4 {
        0 => ([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], w, h),
        // (u, v) -> (h - v, u): the top row becomes the right column.
        1 => ([0.0, 1.0, -1.0, 0.0, h, 0.0], h, w),
        2 => ([-1.0, 0.0, 0.0, -1.0, w, h], w, h),
        _ => ([0.0, -1.0, 1.0, 0.0, 0.0, w], h, w),
    };
    let (sx, sy) = (r.width / turned_w, r.height / turned_h);
    let [a, b, c, d, e, f] = turn;
    let matrix = [sx * a, sy * b, sx * c, sy * d, r.x + sx * e, r.y + sy * f];
    matrix.iter().all(|v| v.is_finite()).then_some(matrix)
}

/// Inverse of an affine map whose linear part only scales and swaps axes (the maps of
/// [`placement`]); `None` when it is not invertible.
fn invert(m: Matrix) -> Option<Matrix> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    let inverse = [ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)];
    inverse.iter().all(|v| v.is_finite()).then_some(inverse)
}

fn apply(m: Matrix, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

// --- Drawing (raster) -----------------------------------------------------------------------

/// Draws the picture onto a page pixmap drawn at `scale` pixels per device unit: stretched,
/// turned, with its opacity, clipped to the pixmap.
pub(crate) fn paint(pixmap: &mut Pixmap, placed: &PlacedPicture<'_>, scale: f32) {
    let (page_w, page_h) = (pixmap.width() as f32, pixmap.height() as f32);
    let r = placed.rect;
    let (x0, y0) = ((r.x * scale).max(0.0), (r.y * scale).max(0.0));
    let (x1, y1) = (
        ((r.x + r.width) * scale).min(page_w),
        ((r.y + r.height) * scale).min(page_h),
    );
    let Some(visible) = Rect::from_ltrb(x0, y0, x1, y1) else {
        return;
    };
    let source = placed.picture.image();
    // Size the picture is drawn at, in pixels, in the orientation of the stored pixels.
    let (drawn_w, drawn_h) = if placed.turned {
        (r.height * scale, r.width * scale)
    } else {
        (r.width * scale, r.height * scale)
    };
    let reduce = |stored: u32, drawn: f32| -> u32 {
        if stored as f32 > drawn * RESAMPLE_ABOVE {
            (drawn.ceil() as u32).clamp(1, stored)
        } else {
            stored
        }
    };
    let (w, h) = (
        reduce(source.width, drawn_w),
        reduce(source.height, drawn_h),
    );
    let reduced = (w, h) != (source.width, source.height);
    let pixels: Cow<'_, Image> = if reduced {
        Cow::Owned(area_resize(source, w, h))
    } else {
        Cow::Borrowed(source)
    };
    let Some(texture) = premultiplied(&pixels) else {
        return;
    };
    // Reduced pixels cover the same area as the stored ones.
    let (kx, ky) = (
        source.width as f32 / w as f32,
        source.height as f32 / h as f32,
    );
    let [a, b, c, d, e, f] = placed.matrix;
    let transform = Transform::from_row(
        a * kx * scale,
        b * kx * scale,
        c * ky * scale,
        d * ky * scale,
        e * scale,
        f * scale,
    );
    let enlarged = drawn_w > w as f32 || drawn_h > h as f32;
    let quality = if enlarged && !reduced {
        FilterQuality::Bicubic
    } else {
        FilterQuality::Bilinear
    };
    let paint = Paint {
        shader: Pattern::new(
            texture.as_ref(),
            SpreadMode::Pad,
            quality,
            placed.opacity,
            transform,
        ),
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_rect(visible, &paint, Transform::identity(), None);
}

/// A premultiplied pixmap of `image`; `None` when it cannot be made.
fn premultiplied(image: &Image) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(image.width, image.height)?;
    let premultiply = |v: u8, a: u8| ((u16::from(v) * u16::from(a) + 127) / 255) as u8;
    for (out, p) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(image.pixels.as_chunks::<4>().0)
    {
        let a = p[3];
        *out = [
            premultiply(p[0], a),
            premultiply(p[1], a),
            premultiply(p[2], a),
            a,
        ];
    }
    Some(pixmap)
}

/// For each of `dst` cells over `src` cells (`dst <= src`), the source cells it covers with
/// their weights (summing to 1): area averaging.
fn spans(src: u32, dst: u32) -> Vec<Vec<(usize, f32)>> {
    let ratio = f64::from(src) / f64::from(dst.max(1));
    (0..dst)
        .map(|i| {
            let start = f64::from(i) * ratio;
            let end = (f64::from(i) + 1.0) * ratio;
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(src as usize);
            (first..last)
                .filter_map(|s| {
                    let covered = (end.min(s as f64 + 1.0) - start.max(s as f64)) / ratio;
                    (covered > 0.0).then_some((s, covered as f32))
                })
                .collect()
        })
        .collect()
}

/// `image` reduced to `width × height` (each at most the image's own side) by averaging the
/// pixels each new pixel covers, in premultiplied alpha. Memory: the result and two rows.
pub(crate) fn area_resize(image: &Image, width: u32, height: u32) -> Image {
    let (width, height) = (width.clamp(1, image.width), height.clamp(1, image.height));
    if (width, height) == (image.width, image.height) {
        return image.clone();
    }
    let columns = spans(image.width, width);
    let rows = spans(image.height, height);
    let source_width = image.width as usize;
    let mut out = vec![0u8; width as usize * height as usize * 4];
    let mut row = vec![0f32; width as usize * 4];
    let mut sum = vec![0f32; width as usize * 4];
    for (y, contributions) in rows.iter().enumerate() {
        sum.fill(0.0);
        for &(source_y, weight_y) in contributions {
            let start = source_y * source_width * 4;
            let Some(line) = image.pixels.get(start..start + source_width * 4) else {
                continue;
            };
            // The source row, reduced horizontally, premultiplied.
            for (x, cells) in columns.iter().enumerate() {
                let mut px = [0f32; 4];
                for &(source_x, weight) in cells {
                    let p = &line[source_x * 4..source_x * 4 + 4];
                    let alpha = f32::from(p[3]) / 255.0;
                    for c in 0..3 {
                        px[c] += f32::from(p[c]) * alpha * weight;
                    }
                    px[3] += f32::from(p[3]) * weight;
                }
                row[x * 4..x * 4 + 4].copy_from_slice(&px);
            }
            for (s, r) in sum.iter_mut().zip(&row) {
                *s += r * weight_y;
            }
        }
        let target = &mut out[y * width as usize * 4..(y + 1) * width as usize * 4];
        for (out, px) in target
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(sum.as_chunks::<4>().0)
        {
            let alpha = px[3];
            let colour = |v: f32| {
                if alpha <= 0.0 {
                    0
                } else {
                    // `as` saturates; adding 0.5 rounds.
                    (v * 255.0 / alpha + 0.5) as u8
                }
            };
            *out = [
                colour(px[0]),
                colour(px[1]),
                colour(px[2]),
                (alpha + 0.5) as u8,
            ];
        }
    }
    Image {
        width,
        height,
        pixels: out,
    }
}

// --- Decoding -------------------------------------------------------------------------------

fn too_large(width: u32, height: u32) -> PictureError {
    PictureError::TooLarge { width, height }
}

/// Refuses an empty image or one of more than [`MAX_SOURCE_PIXELS`] pixels.
fn check_source_size(width: u32, height: u32) -> Result<(), PictureError> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        return Err(too_large(width, height));
    }
    Ok(())
}

/// Decodes a PNG into 8-bit RGBA, refusing it before decoding when it is too large.
fn decode_png_limited(bytes: &[u8]) -> Result<Image, PictureError> {
    let damaged = |e: png::DecodingError| PictureError::Damaged(e.to_string());
    let limits = png::Limits {
        // The decoder's own buffers (rows, chunks); the image itself is checked below.
        bytes: 64 << 20,
    };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let header = decoder.read_header_info().map_err(damaged)?;
    check_source_size(header.width, header.height)?;
    let mut reader = decoder.read_info().map_err(damaged)?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| too_large(reader.info().width, reader.info().height))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(damaged)?;
    let data = buffer.get(..info.buffer_size()).unwrap_or_default();
    if info.bit_depth != png::BitDepth::Eight {
        return Err(PictureError::Damaged(format!(
            "unexpected bit depth {:?}",
            info.bit_depth
        )));
    }
    let pixels: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => {
            return Err(PictureError::Damaged("unexpanded palette".into()));
        }
    };
    let image = Image {
        width: info.width,
        height: info.height,
        pixels,
    };
    check_pixels(&image).map_err(|e| PictureError::Damaged(e.to_string()))?;
    Ok(image)
}

/// A decoded JPEG.
struct DecodedJpeg {
    image: Image,
    /// Width and height in the file (before any reduction while decoding).
    full: (u32, u32),
    dct: Option<DctColor>,
    /// EXIF orientation (1 to 8; 1 when absent).
    orientation: u16,
}

/// Decodes a JPEG into RGBA, refusing it before decoding when it is too large. With
/// `reduce_to`, the decoder skips detail it does not need (1/2, 1/4 or 1/8 of the size, never
/// below that many pixels on the long side).
fn decode_jpeg_limited(bytes: &[u8], reduce_to: Option<u32>) -> Result<DecodedJpeg, PictureError> {
    let damaged = |e: jpeg_decoder::Error| PictureError::Damaged(e.to_string());
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().map_err(damaged)?;
    let info = decoder
        .info()
        .ok_or_else(|| PictureError::Damaged("no image header".into()))?;
    let full = (u32::from(info.width), u32::from(info.height));
    check_source_size(full.0, full.1)?;
    if let Some(side) = reduce_to {
        let long = full.0.max(full.1).max(1);
        if long > side {
            let ratio = side as f32 / long as f32;
            let want = |v: u32| ((v as f32 * ratio).ceil() as u32).clamp(1, u32::from(u16::MAX));
            decoder
                .scale(want(full.0) as u16, want(full.1) as u16)
                .map_err(damaged)?;
        }
    }
    // Bounded by the size check above (8 bytes per pixel covers every pixel format).
    decoder.set_max_decoding_buffer_size(MAX_SOURCE_PIXELS as usize * 8);
    let data = decoder.decode().map_err(damaged)?;
    let info = decoder
        .info()
        .ok_or_else(|| PictureError::Damaged("no image header".into()))?;
    let orientation = decoder.exif_data().map_or(1, exif_orientation);
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let pixels: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| {
                // Big-endian samples: the high byte is the 8-bit value.
                let g = p[0];
                [g, g, g, 255]
            })
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                // Inverted CMYK, as Adobe applications write it (and the decoder returns it).
                let k = u16::from(p[3]);
                let channel = |v: u8| ((u16::from(v) * k + 127) / 255) as u8;
                [channel(p[0]), channel(p[1]), channel(p[2]), 255]
            })
            .collect(),
    };
    let dct = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => Some(DctColor::Rgb),
        jpeg_decoder::PixelFormat::L8 => Some(DctColor::Gray),
        _ => None,
    };
    let image = Image {
        width,
        height,
        pixels,
    };
    check_pixels(&image).map_err(|e| PictureError::Damaged(e.to_string()))?;
    Ok(DecodedJpeg {
        image,
        full,
        dct,
        orientation,
    })
}

/// The orientation tag (0x0112) of EXIF data (a TIFF structure), 1 when absent or unreadable.
fn exif_orientation(exif: &[u8]) -> u16 {
    let read_u16 = |at: usize, little: bool| -> Option<u16> {
        let b = exif.get(at..at + 2)?;
        Some(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let read_u32 = |at: usize, little: bool| -> Option<u32> {
        let b = exif.get(at..at + 4)?;
        let b = [b[0], b[1], b[2], b[3]];
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let find = || -> Option<u16> {
        let little = match exif.get(..4)? {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => return None,
        };
        let ifd = read_u32(4, little)? as usize;
        let count = read_u16(ifd, little)? as usize;
        (0..count.min(512)).find_map(|i| {
            let entry = ifd + 2 + i * 12;
            (read_u16(entry, little)? == 0x0112).then(|| read_u16(entry + 8, little))?
        })
    };
    find().filter(|o| (1..=8).contains(o)).unwrap_or(1)
}

/// `image` as an upright photo for an EXIF `orientation` (1 to 8): the transposition that
/// undoes the camera's.
fn orient(image: Image, orientation: u16) -> Image {
    if !(2..=8).contains(&orientation) {
        return image;
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let swapped = orientation >= 5;
    let (out_w, out_h) = if swapped { (h, w) } else { (w, h) };
    let mut pixels = vec![0u8; image.pixels.len()];
    for y in 0..out_h {
        for x in 0..out_w {
            // Source pixel of the upright pixel (x, y).
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let from = (sy * w + sx) * 4;
            let to = (y * out_w + x) * 4;
            if let (Some(src), Some(dst)) =
                (image.pixels.get(from..from + 4), pixels.get_mut(to..to + 4))
            {
                dst.copy_from_slice(src);
            }
        }
    }
    Image {
        width: out_w as u32,
        height: out_h as u32,
        pixels,
    }
}

/// Width and height of a PNG or JPEG file, read from its header only (no pixel is decoded).
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), PictureError> {
    match PictureFormat::sniff(bytes).ok_or(PictureError::Unsupported)? {
        PictureFormat::Png => {
            let mut decoder = png::Decoder::new(Cursor::new(bytes));
            let header = decoder
                .read_header_info()
                .map_err(|e| PictureError::Damaged(e.to_string()))?;
            Ok((header.width, header.height))
        }
        PictureFormat::Jpeg => {
            let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            decoder
                .read_info()
                .map_err(|e| PictureError::Damaged(e.to_string()))?;
            let info = decoder
                .info()
                .ok_or_else(|| PictureError::Damaged("no image header".into()))?;
            Ok((u32::from(info.width), u32::from(info.height)))
        }
    }
}

// --- Import ---------------------------------------------------------------------------------

/// A background image ready to be stored as the note file's `background.<extension>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredImage {
    pub format: PictureFormat,
    pub bytes: Vec<u8>,
    /// Size of the stored pixels.
    pub width: u32,
    pub height: u32,
}

impl StoredImage {
    pub fn extension(&self) -> &'static str {
        self.format.extension()
    }
}

/// Size of `(width, height)` reduced so that its long side is at most `max` (kept as it is
/// when it already fits).
fn fitted(width: u32, height: u32, max: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= max {
        return (width, height);
    }
    let ratio = f64::from(max) / f64::from(long);
    let side = |v: u32| ((f64::from(v) * ratio).round() as u32).clamp(1, max);
    (side(width), side(height))
}

/// Imports an image file (PNG or JPEG) as a background: decoded, turned upright (EXIF), reduced
/// to [`MAX_STORED_SIDE`] at most, then stored as JPEG when it is an opaque photo and as PNG
/// otherwise. A JPEG that needs none of this keeps its own bytes.
pub fn import_image(bytes: &[u8]) -> Result<StoredImage, PictureError> {
    match PictureFormat::sniff(bytes).ok_or(PictureError::Unsupported)? {
        PictureFormat::Png => {
            let image = decode_png_limited(bytes)?;
            let (width, height) = fitted(image.width, image.height, MAX_STORED_SIDE);
            store(area_resize(&image, width, height), false)
        }
        PictureFormat::Jpeg => {
            // The decoder reduces a large photo by a power of two first, which bounds memory.
            let decoded = decode_jpeg_limited(bytes, Some(MAX_STORED_SIDE))?;
            let untouched = decoded.orientation == 1
                && (decoded.image.width, decoded.image.height) == decoded.full
                && decoded.full.0.max(decoded.full.1) <= MAX_STORED_SIDE
                && decoded.dct.is_some();
            if untouched {
                return Ok(StoredImage {
                    format: PictureFormat::Jpeg,
                    bytes: bytes.to_vec(),
                    width: decoded.image.width,
                    height: decoded.image.height,
                });
            }
            let image = orient(decoded.image, decoded.orientation);
            let (width, height) = fitted(image.width, image.height, MAX_STORED_SIDE);
            store(area_resize(&image, width, height), false)
        }
    }
}

/// Imports a PDF page the system rendered to a PNG: always stored as PNG (sharp text), at most
/// [`MAX_STORED_SIDE`] pixels on its long side.
pub fn import_page(png: &[u8]) -> Result<StoredImage, PictureError> {
    if PictureFormat::sniff(png) != Some(PictureFormat::Png) {
        return Err(PictureError::Unsupported);
    }
    let image = decode_png_limited(png)?;
    let (width, height) = fitted(image.width, image.height, MAX_STORED_SIDE);
    store(area_resize(&image, width, height), true)
}

/// True when every pixel is opaque and the image has more than [`FEW_COLOURS`] colours: a
/// photo, better stored as JPEG.
fn is_photo(image: &Image) -> bool {
    let mut colours = HashSet::new();
    for p in image.pixels.as_chunks::<4>().0 {
        if p[3] != 255 {
            return false;
        }
        if colours.len() <= FEW_COLOURS {
            colours.insert([p[0], p[1], p[2]]);
        }
    }
    colours.len() > FEW_COLOURS
}

/// Encodes `image` for storage: PNG when `png` is asked or the image is not a photo, JPEG
/// otherwise.
fn store(image: Image, png: bool) -> Result<StoredImage, PictureError> {
    let encode = |e: crate::RenderError| PictureError::Encode(e.to_string());
    let (format, bytes) = if png || !is_photo(&image) {
        (PictureFormat::Png, encode_png(&image).map_err(encode)?)
    } else {
        (
            PictureFormat::Jpeg,
            jpeg::encode_image(&image, STORED_JPEG_QUALITY, None).map_err(encode)?,
        )
    };
    Ok(StoredImage {
        format,
        bytes,
        width: image.width,
        height: image.height,
    })
}

/// The part of a background image inside `area` (page coordinates), for a page that grows
/// around it (an assembly, where what reached beyond the note's own page would cover another
/// note): the pixels outside are cut off and the rectangle shrinks to what is left, rounded
/// out to whole pixels. `None` when nothing of it lies in `area`. An image already inside
/// ([`fits_in`]) needs no crop.
pub fn crop_to(
    image: &BackgroundImage,
    picture: &Picture,
    area: ImageRect,
) -> Result<Option<(BackgroundImage, StoredImage)>, PictureError> {
    let (width, height) = (picture.width(), picture.height());
    let Some(matrix) = placement(image, width, height) else {
        return Ok(None);
    };
    let Some(inverse) = invert(matrix) else {
        return Ok(None);
    };
    let r = image.rect;
    let left = r.x.max(area.x);
    let top = r.y.max(area.y);
    let right = (r.x + r.width).min(area.x + area.width);
    let bottom = (r.y + r.height).min(area.y + area.height);
    if left >= right || top >= bottom {
        return Ok(None);
    }
    // The visible corners in stored pixels, rounded out.
    let (u0, v0) = apply(inverse, left, top);
    let (u1, v1) = apply(inverse, right, bottom);
    let clamp = |v: f32, max: u32| v.clamp(0.0, max as f32);
    let (pu0, pu1) = (
        clamp(u0.min(u1).floor(), width) as u32,
        clamp(u0.max(u1).ceil(), width) as u32,
    );
    let (pv0, pv1) = (
        clamp(v0.min(v1).floor(), height) as u32,
        clamp(v0.max(v1).ceil(), height) as u32,
    );
    if pu1 <= pu0 || pv1 <= pv0 {
        return Ok(None);
    }
    let source = picture.image();
    let (cw, ch) = (pu1 - pu0, pv1 - pv0);
    let mut pixels = Vec::with_capacity(cw as usize * ch as usize * 4);
    for v in pv0..pv1 {
        let start = (v as usize * width as usize + pu0 as usize) * 4;
        pixels.extend_from_slice(
            source
                .pixels
                .get(start..start + cw as usize * 4)
                .unwrap_or_default(),
        );
    }
    let cropped = Image {
        width: cw,
        height: ch,
        pixels,
    };
    check_pixels(&cropped).map_err(|e| PictureError::Encode(e.to_string()))?;
    // Where the kept pixels go: the corners of their box through the image's own map.
    let (ax, ay) = apply(matrix, pu0 as f32, pv0 as f32);
    let (bx, by) = apply(matrix, pu1 as f32, pv1 as f32);
    let rect = ImageRect {
        x: ax.min(bx),
        y: ay.min(by),
        width: (ax - bx).abs(),
        height: (ay - by).abs(),
    };
    let stored = store(cropped, picture.format() == PictureFormat::Png)?;
    Ok(Some((
        BackgroundImage {
            rect,
            ..image.clone()
        },
        stored,
    )))
}

/// True when the image's rectangle lies within `area` (nothing would be cut off).
pub fn fits_in(image: &BackgroundImage, area: ImageRect) -> bool {
    let r = image.rect;
    r.x >= area.x
        && r.y >= area.y
        && r.x + r.width <= area.x + area.width
        && r.y + r.height <= area.y + area.height
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::encode_jpeg;

    /// An image of `width × height` whose pixel `(x, y)` is `f(x, y)`.
    pub(crate) fn image(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
        let pixels = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| f(x, y))
            .collect();
        Image {
            width,
            height,
            pixels,
        }
    }

    /// Opaque noise: many colours, a photo for [`is_photo`].
    fn noise(width: u32, height: u32) -> Image {
        image(width, height, |x, y| {
            let n = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503);
            [n as u8, (n >> 8) as u8, (n >> 16) as u8, 255]
        })
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A PNG chunk with its length and CRC.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut checked = kind.to_vec();
        checked.extend_from_slice(data);
        out.extend_from_slice(&crc32(&checked).to_be_bytes());
        out
    }

    /// A valid PNG header announcing `width × height` (RGBA 8-bit), with no pixels.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = width.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    /// `jpeg` with an EXIF segment holding only an orientation tag, right after SOI.
    pub(crate) fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let mut payload = b"Exif\0\0".to_vec();
        payload.extend(tiff);
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xff, 0xe1]);
        out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// `jpeg` claiming another size in its frame header.
    fn with_size(jpeg: &[u8], width: u16, height: u16) -> Vec<u8> {
        let mut out = jpeg.to_vec();
        let sof = out
            .windows(2)
            .position(|w| w == [0xff, 0xc0])
            .expect("a baseline frame header");
        out[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
        out[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
        out
    }

    #[test]
    fn formats_are_recognised_by_their_signature() {
        let png = encode_png(&noise(3, 2)).unwrap();
        let jpeg = encode_jpeg(&noise(3, 2), 90).unwrap();
        assert_eq!(PictureFormat::sniff(&png), Some(PictureFormat::Png));
        assert_eq!(PictureFormat::sniff(&jpeg), Some(PictureFormat::Jpeg));
        assert_eq!(PictureFormat::sniff(b"%PDF-1.7"), None);
        assert_eq!(PictureFormat::Jpeg.extension(), "jpg");
        assert_eq!(PictureFormat::Png.mime(), "image/png");
        let picture = Picture::decode(png.clone()).unwrap();
        assert_eq!((picture.width(), picture.height()), (3, 2));
        assert_eq!(picture.bytes(), png.as_slice());
        assert_eq!(picture.image(), &noise(3, 2));
        assert_eq!(picture.dct(), None);
        assert_eq!(dimensions(&png).unwrap(), (3, 2));
        assert_eq!(dimensions(&jpeg).unwrap(), (3, 2));
        assert!(matches!(
            dimensions(b"%PDF-1.7"),
            Err(PictureError::Unsupported)
        ));
        assert!(matches!(
            dimensions(&png[..20]),
            Err(PictureError::Damaged(_))
        ));
        let picture = Picture::decode(jpeg).unwrap();
        assert_eq!(picture.format(), PictureFormat::Jpeg);
        assert_eq!(picture.dct(), Some(DctColor::Rgb));
    }

    #[test]
    fn hostile_files_are_refused_without_decoding_them() {
        assert!(matches!(
            Picture::decode(b"GIF89a".to_vec()),
            Err(PictureError::Unsupported)
        ));
        // A header announcing 60 000 × 60 000 pixels (14 GB): refused at the header.
        assert!(matches!(
            Picture::decode(png_header(60_000, 60_000)),
            Err(PictureError::TooLarge {
                width: 60_000,
                height: 60_000
            })
        ));
        assert!(matches!(
            import_image(&png_header(1, 4_000_000_000)),
            Err(PictureError::TooLarge { .. })
        ));
        // Header fine, pixels missing.
        assert!(matches!(
            Picture::decode(png_header(10, 10)),
            Err(PictureError::Damaged(_))
        ));
        let jpeg = encode_jpeg(&noise(16, 16), 90).unwrap();
        assert!(matches!(
            Picture::decode(with_size(&jpeg, 65_535, 65_535)),
            Err(PictureError::TooLarge { .. })
        ));
        assert!(matches!(
            import_image(&with_size(&jpeg, 65_535, 1000)),
            Err(PictureError::TooLarge { .. })
        ));
        // Truncated files, at many lengths: an error, never a panic (a file cut only after its
        // last pixel data, in the closing chunk, still holds the whole image).
        let png = encode_png(&noise(40, 30)).unwrap();
        for len in (0..png.len()).step_by(7) {
            if let Ok(picture) = Picture::decode(png[..len].to_vec()) {
                assert!(len + 12 >= png.len(), "png {len}");
                assert_eq!(picture.image(), &noise(40, 30));
            }
        }
        for len in (0..jpeg.len()).step_by(5) {
            let _ = Picture::decode(jpeg[..len].to_vec());
            let _ = import_image(&jpeg[..len]);
        }
        assert!(import_page(&jpeg).is_err());
    }

    #[test]
    fn exif_orientation_is_read_and_undone() {
        assert_eq!(exif_orientation(b""), 1);
        assert_eq!(exif_orientation(b"II*\0\x08\0\0\0"), 1);
        // A camera held upright (orientation 6) stored the picture turned left: 3 wide, 2 high.
        let stored = image(3, 2, |x, y| [x as u8, y as u8, 0, 255]);
        let upright = orient(stored.clone(), 6);
        assert_eq!((upright.width, upright.height), (2, 3));
        // Its top-left pixel is the stored bottom-left one.
        assert_eq!(upright.pixel(0, 0), Some([0, 1, 0, 255]));
        assert_eq!(upright.pixel(1, 0), Some([0, 0, 0, 255]));
        assert_eq!(upright.pixel(1, 2), Some([2, 0, 0, 255]));
        // Every orientation keeps the pixels; 1 and unknown values change nothing.
        for o in 1..=8 {
            let mut turned = orient(stored.clone(), o).pixels;
            let mut original = stored.pixels.clone();
            turned.sort_unstable();
            original.sort_unstable();
            assert_eq!(turned, original, "{o}");
        }
        assert_eq!(orient(stored.clone(), 1), stored);
        assert_eq!(orient(stored.clone(), 9), stored);
        // Orientation 3 is a half turn: applied twice it gives the image back.
        assert_eq!(orient(orient(stored.clone(), 3), 3), stored);

        let photo = noise(60, 40);
        let jpeg = with_orientation(&encode_jpeg(&photo, 95).unwrap(), 6);
        assert!(
            jpeg_decoder::Decoder::new(Cursor::new(&jpeg))
                .read_info()
                .is_ok()
        );
        let stored = import_image(&jpeg).unwrap();
        assert_eq!((stored.width, stored.height), (40, 60));
        assert_eq!(stored.format, PictureFormat::Jpeg);
        assert_ne!(stored.bytes, jpeg, "turned, so encoded again");
    }

    #[test]
    fn imports_keep_transparency_and_choose_the_format() {
        // Transparency: PNG, pixels unchanged.
        let clear = image(50, 20, |x, _| [200, 10, 10, (x * 5) as u8]);
        let stored = import_image(&encode_png(&clear).unwrap()).unwrap();
        assert_eq!(stored.format, PictureFormat::Png);
        assert_eq!(Picture::decode(stored.bytes).unwrap().image(), &clear);
        // A drawing with few colours: PNG.
        let drawing = image(64, 64, |x, y| {
            if (x / 8 + y / 8) % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let stored = import_image(&encode_png(&drawing).unwrap()).unwrap();
        assert_eq!(stored.format, PictureFormat::Png);
        // A photo saved as PNG: JPEG.
        let stored = import_image(&encode_png(&noise(64, 48)).unwrap()).unwrap();
        assert_eq!(
            (stored.format, stored.width, stored.height),
            (PictureFormat::Jpeg, 64, 48)
        );
        // A JPEG that needs nothing keeps its bytes.
        let jpeg = encode_jpeg(&noise(64, 48), 80).unwrap();
        let stored = import_image(&jpeg).unwrap();
        assert_eq!((stored.format, &stored.bytes), (PictureFormat::Jpeg, &jpeg));
        assert_eq!(stored.extension(), "jpg");
        // A rendered PDF page is always PNG.
        let page = import_page(&encode_png(&noise(30, 40)).unwrap()).unwrap();
        assert_eq!(page.format, PictureFormat::Png);
    }

    #[test]
    fn large_imports_are_reduced_to_the_stored_size() {
        let wide = image(5000, 100, |x, _| [(x % 256) as u8, 0, 0, 255]);
        let stored = import_image(&encode_png(&wide).unwrap()).unwrap();
        assert_eq!((stored.width, stored.height), (MAX_STORED_SIDE, 82));
        // A JPEG first reduced by the decoder (1/2), then to the stored size.
        let tall = noise(600, 9000);
        let jpeg = encode_jpeg(&tall, 75).unwrap();
        let stored = import_image(&jpeg).unwrap();
        assert_eq!((stored.width, stored.height), (273, MAX_STORED_SIDE));
        assert_eq!(fitted(100, 50, 4096), (100, 50));
        assert_eq!(fitted(8192, 1, 4096), (4096, 1));
    }

    #[test]
    fn area_averaging_keeps_the_mean_and_ignores_transparent_colour() {
        let checker = image(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let half = area_resize(&checker, 2, 2);
        assert_eq!((half.width, half.height), (2, 2));
        assert!(
            half.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p == &[128, 128, 128, 255])
        );
        // A non-integer ratio still covers every source pixel once.
        let third = area_resize(&checker, 3, 1);
        assert!(
            third
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0].abs_diff(128) <= 1)
        );
        // A transparent pixel's colour does not leak into its neighbour.
        let edge = image(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 0]
            } else {
                [0, 0, 255, 255]
            }
        });
        let one = area_resize(&edge, 1, 1);
        assert_eq!(one.pixels, [0, 0, 255, 128]);
        // Never larger than the source.
        assert_eq!(area_resize(&checker, 10, 10), checker);
    }

    fn placed(quarter_turns: u8, rect: ImageRect) -> BackgroundImage {
        BackgroundImage {
            name: "fiche.png".into(),
            pdf_page: None,
            rect,
            quarter_turns,
            opacity: 1.0,
        }
    }

    const RECT: ImageRect = ImageRect {
        x: 100.0,
        y: 200.0,
        width: 1000.0,
        height: 500.0,
    };

    #[test]
    fn the_placement_turns_then_stretches() {
        // Stored 40 × 20 (no turn) or 20 × 40 (turned): the image always fills the rectangle.
        let corners = |m: Matrix, w: f32, h: f32| {
            [
                apply(m, 0.0, 0.0),
                apply(m, w, 0.0),
                apply(m, w, h),
                apply(m, 0.0, h),
            ]
        };
        let m = placement(&placed(0, RECT), 40, 20).unwrap();
        assert_eq!(
            corners(m, 40.0, 20.0),
            [
                (100.0, 200.0),
                (1100.0, 200.0),
                (1100.0, 700.0),
                (100.0, 700.0)
            ]
        );
        // A clockwise quarter turn: the stored top-left corner goes to the top right.
        let m = placement(&placed(1, RECT), 20, 40).unwrap();
        assert_eq!(
            corners(m, 20.0, 40.0),
            [
                (1100.0, 200.0),
                (1100.0, 700.0),
                (100.0, 700.0),
                (100.0, 200.0)
            ]
        );
        let m = placement(&placed(2, RECT), 40, 20).unwrap();
        assert_eq!(apply(m, 0.0, 0.0), (1100.0, 700.0));
        let m = placement(&placed(7, RECT), 20, 40).unwrap();
        assert_eq!(apply(m, 0.0, 0.0), (100.0, 700.0), "7 = 3 quarter turns");
        let inverse = invert(m).unwrap();
        let (u, v) = apply(inverse, 100.0, 700.0);
        assert!(u.abs() < 1e-3 && v.abs() < 1e-3, "{u} {v}");
        // Degenerate placements draw nothing.
        for rect in [
            ImageRect { width: 0.0, ..RECT },
            ImageRect {
                height: -5.0,
                ..RECT
            },
            ImageRect {
                x: f32::NAN,
                ..RECT
            },
            ImageRect {
                width: f32::INFINITY,
                ..RECT
            },
        ] {
            assert!(placement(&placed(0, rect), 40, 20).is_none(), "{rect:?}");
        }
        assert!(placement(&placed(0, RECT), 0, 20).is_none());
    }

    #[test]
    fn a_view_places_only_a_visible_image_with_pixels() {
        let picture = Picture::decode(encode_png(&noise(4, 2)).unwrap()).unwrap();
        let mut note = crate::tests::blank_note();
        assert!(
            NoteView::new(&note, Some(&picture)).placed().is_none(),
            "no image"
        );
        note.background_image = Some(placed(0, RECT));
        assert!(NoteView::from(&note).placed().is_none(), "no pixels");
        let shown = NoteView::new(&note, Some(&picture)).placed().unwrap();
        assert_eq!(shown.opacity, 1.0);
        for (opacity, expected) in [
            (0.4, Some(0.4)),
            (7.0, Some(1.0)),
            (f32::NAN, Some(1.0)),
            (0.0, None),
            (-1.0, None),
        ] {
            if let Some(image) = note.background_image.as_mut() {
                image.opacity = opacity;
            }
            let got = NoteView::new(&note, Some(&picture))
                .placed()
                .map(|p| p.opacity);
            assert_eq!(got, expected, "{opacity}");
        }
    }

    #[test]
    fn cropping_keeps_the_visible_pixels_in_place() {
        // 40 × 20 pixels over 1000 × 500 units (25 units a pixel), half of it out of the area.
        let source = image(40, 20, |x, y| [x as u8, y as u8, 7, 255]);
        let picture = Picture::decode(encode_png(&source).unwrap()).unwrap();
        let area = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 600.0,
            height: 450.0,
        };
        let image = placed(0, RECT);
        assert!(!fits_in(&image, area));
        let (cropped, stored) = crop_to(&image, &picture, area).unwrap().unwrap();
        // Columns 0..20 (units 100..600), rows 0..10 (units 200..450).
        assert_eq!((stored.width, stored.height), (20, 10));
        assert_eq!(stored.format, PictureFormat::Png);
        assert_eq!(
            cropped.rect,
            ImageRect {
                x: 100.0,
                y: 200.0,
                width: 500.0,
                height: 250.0
            }
        );
        let kept = Picture::decode(stored.bytes).unwrap();
        assert_eq!(kept.image().pixel(19, 9), Some([19, 9, 7, 255]));
        assert!(fits_in(&cropped, area));
        // Turned a quarter: the stored pixels kept are the ones now in the area.
        let turned = placed(1, RECT);
        let picture = Picture::decode(encode_png(&tall_image()).unwrap()).unwrap();
        let (cropped, stored) = crop_to(&turned, &picture, area).unwrap().unwrap();
        assert_eq!((stored.width, stored.height), (10, 20));
        assert_eq!(cropped.quarter_turns, 1);
        let kept = Picture::decode(stored.bytes).unwrap();
        // The area's top-left corner (100, 200) shows the stored bottom-left pixel (0, 39).
        let m = placement(&cropped, kept.width(), kept.height()).unwrap();
        let (u, v) = apply(invert(m).unwrap(), 101.0, 201.0);
        let at = kept.image().pixel(u as u32, v as u32).unwrap();
        assert_eq!(at, [0, 39, 7, 255]);
        // Nothing in the area: nothing kept.
        let away = ImageRect { x: 5000.0, ..area };
        assert!(crop_to(&image, &picture, away).unwrap().is_none());
    }

    fn tall_image() -> Image {
        image(20, 40, |x, y| [x as u8, y as u8, 7, 255])
    }
}
