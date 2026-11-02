//! The QR code of the share's address, as SVG path data (`docs/design/lan-share.md`).

use serde::Serialize;

/// A QR code: `size` modules a side, drawn by `path` in a `viewBox="0 0 size size"` (no border).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QrCode {
    pub size: u32,
    pub path: String,
}

/// The QR code of `text` (error correction Medium); `None` when it cannot be encoded.
pub fn qr_code(text: &str) -> Option<QrCode> {
    let _ = text;
    unimplemented!("package A")
}
