//! The QR code of the share's address, as SVG path data (`docs/design/lan-share.md`).

use std::fmt::Write as _;

use qrcodegen::QrCodeEcc;
use serde::Serialize;

/// A QR code: `size` modules a side, drawn by `path` in a `viewBox="0 0 size size"` (no border).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QrCode {
    pub size: u32,
    pub path: String,
}

/// The QR code of `text` (error correction Medium); `None` when it cannot be encoded.
pub fn qr_code(text: &str) -> Option<QrCode> {
    // Medium at least: the encoder raises it when that does not make the code larger.
    let code = qrcodegen::QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let size = code.size();
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if code.get_module(x, y) {
                // One unit square per dark module.
                let _ = write!(path, "M{x} {y}h1v1h-1z");
            }
        }
    }
    Some(QrCode {
        size: u32::try_from(size).ok()?,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The dark modules drawn by a path.
    fn modules(path: &str) -> HashSet<(u32, u32)> {
        path.split('M')
            .filter(|square| !square.is_empty())
            .map(|square| {
                let square = square.strip_suffix("h1v1h-1z").unwrap();
                let (x, y) = square.split_once(' ').unwrap();
                (x.parse().unwrap(), y.parse().unwrap())
            })
            .collect()
    }

    /// The 7 × 7 finder pattern with its top-left corner at (x, y): dark ring, light ring,
    /// dark 3 × 3 centre.
    fn has_finder(dark: &HashSet<(u32, u32)>, x: u32, y: u32) -> bool {
        (0..7).all(|dy| {
            (0..7).all(|dx| {
                let ring = dx.min(dy).min(6 - dx).min(6 - dy);
                dark.contains(&(x + dx, y + dy)) == (ring != 1)
            })
        })
    }

    #[test]
    fn address_qr_code() {
        let code = qr_code("http://192.168.1.23:8765/").unwrap();
        // Version 2 (25 modules) at error correction Medium for 25 bytes.
        assert_eq!(code.size, 25);
        let dark = modules(&code.path);
        assert_eq!(dark.len(), code.path.matches('M').count());
        assert!(dark.iter().all(|&(x, y)| x < code.size && y < code.size));
        let s = code.size;
        assert!(has_finder(&dark, 0, 0));
        assert!(has_finder(&dark, s - 7, 0));
        assert!(has_finder(&dark, 0, s - 7));
        // The separators around the finder patterns are light.
        assert!((0..8).all(|i| !dark.contains(&(7, i)) && !dark.contains(&(i, 7))));
        // Timing pattern: alternating modules on row 6 between the finders.
        assert!((8..s - 8).all(|x| dark.contains(&(x, 6)) == (x % 2 == 0)));
        // The dark module next to the bottom-left finder.
        assert!(dark.contains(&(8, s - 8)));
    }

    #[test]
    fn same_text_same_code_and_size_grows_with_text() {
        let a = qr_code("http://10.0.0.5:8765/").unwrap();
        assert_eq!(a, qr_code("http://10.0.0.5:8765/").unwrap());
        let long = qr_code(&format!("http://192.168.1.23:8765/{}", "x".repeat(200))).unwrap();
        assert!(long.size > a.size);
        assert_eq!((long.size - 17) % 4, 0);
    }

    #[test]
    fn too_long_text() {
        assert_eq!(qr_code(&"x".repeat(4000)), None);
    }
}
