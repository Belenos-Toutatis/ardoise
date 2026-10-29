//! Lossless WebP through libwebp (`libwebp-sys2`): the picture is handed over as RGB or RGBA,
//! encoded with the lossless preset and written into a `Vec<u8>` by a callback.
//!
//! Why not the pure-Rust encoder of `image-webp`? Measured on the reference note of
//! `tests/export_formats.rs` it gave files 2.1 times the PNG's (paper and Seyès texture) and
//! 1.3 times (ink alone): not worth offering. libwebp's lossless mode is lighter than the PNG.

use std::ffi::{c_int, c_void};

use libwebp_sys as webp;

use crate::{Image, RenderError};

/// Lossless effort for libwebp's preset, 0 (fastest) to 9 (smallest). 4 is libwebp's default.
pub(crate) const EFFORT: c_int = 4;

fn failure(step: &str, picture: &webp::WebPPicture) -> RenderError {
    RenderError::Encode(format!(
        "WebP {step} (libwebp error {})",
        picture.error_code as i32
    ))
}

/// The bytes libwebp writes, collected as it goes.
extern "C" fn collect(data: *const u8, size: usize, picture: *const webp::WebPPicture) -> c_int {
    // SAFETY: libwebp passes `size` readable bytes, and the picture whose `custom_ptr` is the
    // `Vec<u8>` set by `encode_lossless`, alive for the whole call to `WebPEncode`.
    unsafe {
        let out = &mut *(*picture).custom_ptr.cast::<Vec<u8>>();
        out.extend_from_slice(std::slice::from_raw_parts(data, size));
    }
    1
}

/// Lossless WebP of `image` (already checked: not empty, the right number of pixels, sides within
/// WebP's limit) at the given effort. RGB when every pixel is opaque, RGBA otherwise.
pub(crate) fn encode_lossless(image: &Image, effort: c_int) -> Result<Vec<u8>, RenderError> {
    let opaque = image.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let rgb: Vec<u8>;
    let (pixels, stride) = if opaque {
        rgb = image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        (&rgb, image.width as usize * 3)
    } else {
        (&image.pixels, image.width as usize * 4)
    };
    let mut out: Vec<u8> = Vec::new();
    // SAFETY: the structures are initialised by libwebp's own functions before use; `pixels` holds
    // `height` rows of `stride` bytes, as the import functions read them; the picture's buffers
    // are freed on every path; `out` outlives `WebPEncode`, which is the only caller of `collect`.
    unsafe {
        let mut config: webp::WebPConfig = std::mem::zeroed();
        if webp::WebPConfigInit(&mut config) == 0
            || webp::WebPConfigLosslessPreset(&mut config, effort) == 0
            || webp::WebPValidateConfig(&config) == 0
        {
            return Err(RenderError::Encode("WebP: invalid encoder settings".into()));
        }
        let mut picture: webp::WebPPicture = std::mem::zeroed();
        if webp::WebPPictureInit(&mut picture) == 0 {
            return Err(RenderError::Encode("WebP: libwebp version mismatch".into()));
        }
        picture.use_argb = 1;
        picture.width = image.width as c_int;
        picture.height = image.height as c_int;
        let imported = if opaque {
            webp::WebPPictureImportRGB(&mut picture, pixels.as_ptr(), stride as c_int)
        } else {
            webp::WebPPictureImportRGBA(&mut picture, pixels.as_ptr(), stride as c_int)
        };
        if imported == 0 {
            let error = failure("import", &picture);
            webp::WebPPictureFree(&mut picture);
            return Err(error);
        }
        picture.writer = Some(collect);
        picture.custom_ptr = (&mut out as *mut Vec<u8>).cast::<c_void>();
        let encoded = webp::WebPEncode(&config, &mut picture);
        let error = (encoded == 0).then(|| failure("encoding", &picture));
        webp::WebPPictureFree(&mut picture);
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(out)
}
