//! Raster rendering of notes: ink tools, page backgrounds, templates and textures (PNG export).
//!
//! The public surface below is the contract used by the Inkspace import comparison, the
//! exports and the thumbnails. The body is a placeholder (blank page) until the renderer
//! lands.

use ardoise_core::note::{Note, PageSize};

/// How to render a note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    /// Pixels per device unit (10 µm). 0.04 gives 1 px = 0.25 mm (Inkspace's previews).
    pub scale: f32,
    /// Draw the paper colour, template and texture; otherwise a plain white page.
    pub background: bool,
}

impl RenderOptions {
    /// Scale so that the page is `width_px` pixels wide.
    pub fn fit_width(page: PageSize, width_px: u32) -> Self {
        Self {
            scale: width_px as f32 / page.width.max(1) as f32,
            background: true,
        }
    }
}

/// An RGBA image, 8 bits per channel, straight alpha, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Errors while rendering or encoding.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the image would be empty or too large ({width} x {height} px)")]
    Size { width: u32, height: u32 },
    #[error("cannot encode the image: {0}")]
    Encode(String),
}

/// Renders `note` to an image of `page × scale` pixels.
pub fn render_note(note: &Note, options: &RenderOptions) -> Result<Image, RenderError> {
    let width = (note.page.width as f32 * options.scale).round() as u32;
    let height = (note.page.height as f32 * options.scale).round() as u32;
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return Err(RenderError::Size { width, height });
    }
    Ok(Image {
        width,
        height,
        pixels: vec![255; width as usize * height as usize * 4],
    })
}

/// PNG bytes of `image`.
pub fn encode_png(image: &Image) -> Result<Vec<u8>, RenderError> {
    let _ = image;
    Err(RenderError::Encode("not implemented yet".into()))
}
