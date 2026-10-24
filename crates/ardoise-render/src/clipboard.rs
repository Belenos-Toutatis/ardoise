//! The data of an image copied to the Windows clipboard (SPEC §4.6): a PNG for the
//! registered "PNG" format (Word and OneNote keep its quality and transparency) and a
//! device-independent bitmap for `CF_DIB` (every other program). Pure functions; the
//! clipboard itself is only touched by the application (`app/src-tauri/src/clipboard.rs`).

use crate::{Image, RenderError, check_pixels, encode_png_at, over_white, pixels_per_metre};

/// Size of a `BITMAPINFOHEADER`.
const INFO_HEADER_SIZE: u32 = 40;
/// `BI_RGB`: uncompressed.
const BI_RGB: u32 = 0;

/// What goes on the clipboard for one image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImage {
    /// PNG file, alpha kept.
    pub png: Vec<u8>,
    /// `CF_DIB` data: a `BITMAPINFOHEADER` then 24-bit BGR rows, bottom-up, each padded to
    /// 4 bytes, alpha flattened over white.
    pub dib: Vec<u8>,
}

/// Both clipboard formats of `image`, without a resolution.
pub fn clipboard_image(image: &Image) -> Result<ClipboardImage, RenderError> {
    clipboard_image_at(image, None)
}

/// Both clipboard formats of `image`; `dpi` is recorded in both, so that programs that
/// honour it paste the note at its real size.
pub fn clipboard_image_at(image: &Image, dpi: Option<u32>) -> Result<ClipboardImage, RenderError> {
    Ok(ClipboardImage {
        png: encode_png_at(image, dpi)?,
        dib: dib(image, dpi)?,
    })
}

/// `CF_DIB` data of `image`.
fn dib(image: &Image, dpi: Option<u32>) -> Result<Vec<u8>, RenderError> {
    check_pixels(image)?;
    let (width, height) = (image.width, image.height);
    let too_large = || RenderError::Size { width, height };
    let stride = (width as usize)
        .checked_mul(3)
        .map(|row| row.div_ceil(4) * 4)
        .ok_or_else(too_large)?;
    let data_size = stride.checked_mul(height as usize).ok_or_else(too_large)?;
    let image_size = u32::try_from(data_size).map_err(|_| too_large())?;
    let signed = |v: u32| i32::try_from(v).map_err(|_| too_large());
    let ppm = dpi
        .map(pixels_per_metre)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(0);

    let mut out = Vec::with_capacity(INFO_HEADER_SIZE as usize + data_size);
    out.extend_from_slice(&INFO_HEADER_SIZE.to_le_bytes());
    out.extend_from_slice(&signed(width)?.to_le_bytes());
    // A positive height: rows go bottom-up.
    out.extend_from_slice(&signed(height)?.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&BI_RGB.to_le_bytes());
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&ppm.to_le_bytes());
    out.extend_from_slice(&ppm.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // colours used
    out.extend_from_slice(&0u32.to_le_bytes()); // important colours
    let row_bytes = width as usize * 4;
    let padding = stride - width as usize * 3;
    for row in image.pixels.chunks_exact(row_bytes).rev() {
        for p in row.as_chunks::<4>().0 {
            let [r, g, b] = over_white(*p);
            out.extend_from_slice(&[b, g, r]);
        }
        out.extend(std::iter::repeat_n(0, padding));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_png;

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn dib_rows_are_bottom_up_bgr_and_padded() {
        // 2 × 2: red, half-transparent black / green, transparent.
        let image = Image {
            width: 2,
            height: 2,
            pixels: vec![255, 0, 0, 255, 0, 0, 0, 128, 0, 255, 0, 255, 9, 9, 9, 0],
        };
        let data = clipboard_image_at(&image, Some(254)).unwrap();
        let dib = &data.dib;
        assert_eq!(u32_at(dib, 0), 40);
        assert_eq!(u32_at(dib, 4), 2);
        assert_eq!(u32_at(dib, 8), 2);
        assert_eq!(&dib[12..16], [1, 0, 24, 0]);
        assert_eq!(u32_at(dib, 16), BI_RGB);
        // Rows of 6 bytes padded to 8.
        assert_eq!(u32_at(dib, 20), 16);
        // 254 dpi = 10 000 pixels per metre.
        assert_eq!(u32_at(dib, 24), 10_000);
        assert_eq!(u32_at(dib, 28), 10_000);
        assert_eq!(dib.len(), 40 + 16);
        // Bottom row first: green, then transparent → white.
        assert_eq!(&dib[40..48], [0, 255, 0, 255, 255, 255, 0, 0]);
        // Top row: red, then black at half opacity over white → grey.
        assert_eq!(&dib[48..56], [0, 0, 255, 127, 127, 127, 0, 0]);
        // The PNG keeps the alpha.
        assert_eq!(decode_png(&data.png).unwrap(), image);
    }

    #[test]
    fn inconsistent_images_are_refused() {
        let bad = Image {
            width: 3,
            height: 1,
            pixels: vec![0; 4],
        };
        assert!(clipboard_image(&bad).is_err());
    }
}
