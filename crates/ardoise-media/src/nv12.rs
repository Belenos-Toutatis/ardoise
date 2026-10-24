//! RGBA to NV12 colour conversion for the H.264 encoders.
//!
//! H.264 codes YUV 4:2:0. NV12 is the 4:2:0 layout every Media Foundation H.264 encoder accepts
//! as input, hardware ones included (the Microsoft software encoder takes only NV12, I420,
//! YV12, YUY2 and IYUV; it does not take RGB). The conversion is done here, in Rust, rather
//! than by letting the sink writer insert its own colour converter, so that the matrix and the
//! range are chosen deliberately and match what the writer declares in the stream:
//!
//! - **Matrix BT.709**, the HD standard that players assume for HD H.264.
//! - **Limited ("studio") range**: Y from 16 (black) to 235 (white), U and V from 16 to 240
//!   around 128. This is the H.264 default; full-range video is often shown washed out or
//!   crushed by players that ignore the range flag.
//! - **Chroma**: average of each 2×2 block of pixels.
//! - **Alpha** is ignored: frames must already be composited onto an opaque background.
//!
//! Layout written: the Y plane (`width × height` bytes, stride = `width`), then the UV plane
//! (`width × height / 2` bytes: `height / 2` rows of `width / 2` interleaved U, V pairs, stride
//! = `width`). Width and height must be even.

use crate::MediaError;

/// Size in bytes of an NV12 image of `width × height` pixels.
pub fn nv12_len(width: usize, height: usize) -> usize {
    width * height * 3 / 2
}

// BT.709 luma weights.
const KR: f64 = 0.2126;
const KB: f64 = 0.0722;
const KG: f64 = 1.0 - KR - KB;
// Limited range: 219 steps of luma, 224 steps of chroma, for 255 steps of RGB.
const Y_SCALE: f64 = 219.0 / 255.0;
const C_SCALE: f64 = 224.0 / 255.0;

/// Fixed-point coefficient with 16 fractional bits, rounded to nearest.
const fn fix(x: f64) -> i32 {
    let scaled = x * 65536.0;
    if scaled >= 0.0 {
        (scaled + 0.5) as i32
    } else {
        (scaled - 0.5) as i32
    }
}

const YR: i32 = fix(KR * Y_SCALE);
const YG: i32 = fix(KG * Y_SCALE);
const YB: i32 = fix(KB * Y_SCALE);
const UR: i32 = fix(-KR / (2.0 * (1.0 - KB)) * C_SCALE);
const UG: i32 = fix(-KG / (2.0 * (1.0 - KB)) * C_SCALE);
const UB: i32 = fix(0.5 * C_SCALE);
const VR: i32 = fix(0.5 * C_SCALE);
const VG: i32 = fix(-KG / (2.0 * (1.0 - KR)) * C_SCALE);
const VB: i32 = fix(-KB / (2.0 * (1.0 - KR)) * C_SCALE);

#[inline]
fn luma(px: &[u8]) -> u8 {
    let y = YR * i32::from(px[0]) + YG * i32::from(px[1]) + YB * i32::from(px[2]);
    // + 16 offset, + 0.5 for rounding, then drop the 16 fractional bits.
    ((y + (16 << 16) + (1 << 15)) >> 16).clamp(0, 255) as u8
}

/// Chroma from the sums of four pixels (hence the two extra fractional bits).
#[inline]
fn chroma(cr: i32, cg: i32, cb: i32, r: i32, g: i32, b: i32) -> u8 {
    let c = cr * r + cg * g + cb * b;
    ((c + (128 << 18) + (1 << 17)) >> 18).clamp(0, 255) as u8
}

/// Converts one RGBA frame (`width × height × 4` bytes, top row first) to NV12 in `nv12`
/// (at least [`nv12_len`] bytes; only that many are written).
pub fn rgba_to_nv12(
    rgba: &[u8],
    width: usize,
    height: usize,
    nv12: &mut [u8],
) -> Result<(), MediaError> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(MediaError::InvalidSettings(format!(
            "NV12 needs even, non-zero dimensions (got {width}×{height})"
        )));
    }
    let expected = width * height * 4;
    if rgba.len() != expected {
        return Err(MediaError::FrameSize {
            expected,
            actual: rgba.len(),
        });
    }
    let out_len = nv12_len(width, height);
    let Some(nv12) = nv12.get_mut(..out_len) else {
        return Err(MediaError::FrameSize {
            expected: out_len,
            actual: nv12.len(),
        });
    };

    let (y_plane, uv_plane) = nv12.split_at_mut(width * height);
    let rgba_rows = rgba.chunks_exact(width * 8); // two RGBA rows at a time
    let y_rows = y_plane.chunks_exact_mut(width * 2); // two Y rows at a time
    let uv_rows = uv_plane.chunks_exact_mut(width); // one UV row per pair of pixel rows
    for ((rgba_pair, y_pair), uv_row) in rgba_rows.zip(y_rows).zip(uv_rows) {
        let (top, bottom) = rgba_pair.split_at(width * 4);
        let (y_top, y_bottom) = y_pair.split_at_mut(width);
        // Blocks of 2×2 pixels: two RGBA pixels (8 bytes) from each row.
        let blocks = top
            .as_chunks::<8>()
            .0
            .iter()
            .zip(bottom.as_chunks::<8>().0)
            .zip(y_top.as_chunks_mut::<2>().0)
            .zip(y_bottom.as_chunks_mut::<2>().0)
            .zip(uv_row.as_chunks_mut::<2>().0);
        for ((((t, b), yt), yb), uv) in blocks {
            yt[0] = luma(&t[0..4]);
            yt[1] = luma(&t[4..8]);
            yb[0] = luma(&b[0..4]);
            yb[1] = luma(&b[4..8]);
            let r = i32::from(t[0]) + i32::from(t[4]) + i32::from(b[0]) + i32::from(b[4]);
            let g = i32::from(t[1]) + i32::from(t[5]) + i32::from(b[1]) + i32::from(b[5]);
            let bl = i32::from(t[2]) + i32::from(t[6]) + i32::from(b[2]) + i32::from(b[6]);
            uv[0] = chroma(UR, UG, UB, r, g, bl);
            uv[1] = chroma(VR, VG, VB, r, g, bl);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: usize, height: usize, rgb: [u8; 3]) -> Vec<u8> {
        (0..width * height)
            .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
            .collect()
    }

    fn convert_solid(rgb: [u8; 3]) -> (u8, u8, u8) {
        let rgba = solid(4, 2, rgb);
        let mut out = vec![0; nv12_len(4, 2)];
        rgba_to_nv12(&rgba, 4, 2, &mut out).unwrap();
        assert!(out[..8].iter().all(|&y| y == out[0]));
        assert_eq!(out[8..10], out[10..12]);
        (out[0], out[8], out[9])
    }

    #[test]
    fn coefficients_match_bt709_limited_range() {
        // Luma weights add up to 219/255, chroma weights to zero.
        assert!((YR + YG + YB - fix(Y_SCALE)).abs() <= 1);
        assert!((UR + UG + UB).abs() <= 1);
        assert!((VR + VG + VB).abs() <= 1);
    }

    #[test]
    fn reference_colours() {
        // Values from the BT.709 equations, rounded.
        assert_eq!(convert_solid([255, 255, 255]), (235, 128, 128));
        assert_eq!(convert_solid([0, 0, 0]), (16, 128, 128));
        assert_eq!(convert_solid([255, 0, 0]), (63, 102, 240));
        assert_eq!(convert_solid([0, 255, 0]), (173, 42, 26));
        assert_eq!(convert_solid([0, 0, 255]), (32, 240, 118));
        assert_eq!(convert_solid([128, 128, 128]), (126, 128, 128));
    }

    #[test]
    fn layout_and_chroma_averaging() {
        // 4×2: left 2×2 block white, right 2×2 block black.
        let mut rgba = solid(4, 2, [255, 255, 255]);
        for row in 0..2 {
            for col in 2..4 {
                let i = (row * 4 + col) * 4;
                rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let mut out = vec![0xAA; nv12_len(4, 2) + 3];
        rgba_to_nv12(&rgba, 4, 2, &mut out).unwrap();
        assert_eq!(out[..8], [235, 235, 16, 16, 235, 235, 16, 16]);
        assert_eq!(out[8..12], [128, 128, 128, 128]);
        assert_eq!(out[12..], [0xAA; 3], "bytes past the image are left alone");

        // A 2×2 block of half red, half blue averages to purple chroma.
        let mut rgba = solid(2, 2, [255, 0, 0]);
        rgba[4..7].copy_from_slice(&[0, 0, 255]);
        rgba[12..15].copy_from_slice(&[0, 0, 255]);
        let mut out = vec![0; nv12_len(2, 2)];
        rgba_to_nv12(&rgba, 2, 2, &mut out).unwrap();
        assert_eq!(out[..4], [63, 32, 63, 32]);
        assert_eq!(out[4..6], [171, 179]); // (102 + 240) / 2, (240 + 118) / 2
    }

    #[test]
    fn ignores_alpha() {
        let mut rgba = solid(2, 2, [10, 200, 30]);
        let mut a = vec![0; 6];
        rgba_to_nv12(&rgba, 2, 2, &mut a).unwrap();
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = 0;
        }
        let mut b = vec![0; 6];
        rgba_to_nv12(&rgba, 2, 2, &mut b).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_bad_sizes() {
        let mut out = vec![0; 100];
        assert!(matches!(
            rgba_to_nv12(&[0; 12], 3, 1, &mut out),
            Err(MediaError::InvalidSettings(_))
        ));
        assert!(matches!(
            rgba_to_nv12(&[0; 15], 2, 2, &mut out),
            Err(MediaError::FrameSize {
                expected: 16,
                actual: 15
            })
        ));
        assert!(matches!(
            rgba_to_nv12(&[0; 16], 2, 2, &mut [0; 5]),
            Err(MediaError::FrameSize {
                expected: 6,
                actual: 5
            })
        ));
    }
}
