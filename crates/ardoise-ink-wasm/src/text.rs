//! WebAssembly entry points of `ardoise-text`: the editor lays out and draws text frames with the
//! very glyphs of the exports, and gets the font files to style its text field with them (no font
//! installed on the PC is used). The interface side is `app/ui/src/lib/text.ts`.
//!
//! Protocol, for one text:
//! 1. `text_alloc(n)` reserves `n` bytes, where the caller writes the text in UTF-8.
//! 2. `text_layout(ptr, n, family, bold, size)` returns the number of f32 values of the layout, at
//!    `text_layout_result()` until the next call: `[width, height, line height, ascent, descent,
//!    line count, then for each line: top, baseline, width, start, end, glyph count, then for each
//!    glyph: x, advance, index]` (start, end and index: UTF-16 offsets in the text).
//! 3. `text_path(ptr, n, family, bold, size)` returns the length in bytes of the SVG path data of
//!    its glyphs (UTF-8, at `text_path_result()` until the next call), in the text's own
//!    coordinates (origin top-left, y down), to fill with the non-zero rule.
//! 4. `text_free(ptr, n)` releases the bytes.
//!
//! Family codes: `FontFamily::code` (0 Latin Modern, 1 Andika; an unknown code means Latin
//! Modern); `bold` is 0 or 1. `text_font(family, bold)` and `text_font_len(family, bold)` give the
//! bytes of the font file.

use std::cell::RefCell;

use ardoise_text::{FontFamily, TextStyle, font_file, layout, text_outline};

thread_local! {
    static LAYOUT: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    static PATH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// The style of the codes passed by the interface.
pub fn style_from(family: u32, bold: u32, size: f32) -> TextStyle {
    TextStyle {
        family: FontFamily::from_code(family),
        bold: bold != 0,
        size,
    }
}

/// The layout of `text` as the flat list of [`text_layout`] (see the module documentation).
pub fn layout_floats(text: &str, style: TextStyle) -> Vec<f32> {
    let laid = layout(text, style);
    let mut out = vec![
        laid.width,
        laid.height,
        laid.line_height,
        laid.ascent,
        laid.descent,
        laid.lines.len() as f32,
    ];
    for line in &laid.lines {
        out.extend([
            line.top,
            line.baseline,
            line.width,
            line.start as f32,
            line.end as f32,
            line.glyphs.len() as f32,
        ]);
        for glyph in &line.glyphs {
            out.extend([glyph.x, glyph.advance, glyph.index as f32]);
        }
    }
    out
}

/// The SVG path data of the glyphs of `text`.
pub fn path_svg(text: &str, style: TextStyle) -> String {
    text_outline(text, style).1.to_svg()
}

/// The text written at `ptr` (`len` bytes of UTF-8; invalid sequences are replaced).
///
/// # Safety
/// `ptr` must point to `len` initialised bytes (from [`text_alloc`]), or be null with `len` 0.
unsafe fn text_at<'a>(ptr: *const u8, len: usize) -> std::borrow::Cow<'a, str> {
    if ptr.is_null() || len == 0 {
        return std::borrow::Cow::Borrowed("");
    }
    // SAFETY: the caller guarantees `len` initialised bytes at `ptr`.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes)
}

/// Reserves `bytes` bytes and returns their address (null for 0).
#[unsafe(no_mangle)]
pub extern "C" fn text_alloc(bytes: usize) -> *mut u8 {
    if bytes == 0 {
        return std::ptr::null_mut();
    }
    let mut buffer = Vec::<u8>::with_capacity(bytes);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Releases bytes reserved by [`text_alloc`].
///
/// # Safety
/// `ptr` must come from `text_alloc(bytes)` with the same `bytes`, and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn text_free(ptr: *mut u8, bytes: usize) {
    if !ptr.is_null() && bytes > 0 {
        // SAFETY: the caller guarantees ptr/bytes come from `text_alloc`.
        drop(unsafe { Vec::from_raw_parts(ptr, 0, bytes) });
    }
}

/// Lays out the text at `ptr`; returns the length of the result (see [`text_layout_result`]).
///
/// # Safety
/// `ptr` must point to `len` initialised bytes (from [`text_alloc`]), or be null with `len` 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn text_layout(
    ptr: *const u8,
    len: usize,
    family: u32,
    bold: u32,
    size: f32,
) -> usize {
    // SAFETY: forwarded from the caller.
    let text = unsafe { text_at(ptr, len) };
    let flat = layout_floats(&text, style_from(family, bold, size));
    let count = flat.len();
    LAYOUT.with(|layout| *layout.borrow_mut() = flat);
    count
}

/// Address of the last result of [`text_layout`].
#[unsafe(no_mangle)]
pub extern "C" fn text_layout_result() -> *const f32 {
    LAYOUT.with(|layout| layout.borrow().as_ptr())
}

/// The glyphs of the text at `ptr` as SVG path data; returns its length in bytes (see
/// [`text_path_result`]).
///
/// # Safety
/// `ptr` must point to `len` initialised bytes (from [`text_alloc`]), or be null with `len` 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn text_path(
    ptr: *const u8,
    len: usize,
    family: u32,
    bold: u32,
    size: f32,
) -> usize {
    // SAFETY: forwarded from the caller.
    let text = unsafe { text_at(ptr, len) };
    let data = path_svg(&text, style_from(family, bold, size)).into_bytes();
    let count = data.len();
    PATH.with(|path| *path.borrow_mut() = data);
    count
}

/// Address of the last result of [`text_path`].
#[unsafe(no_mangle)]
pub extern "C" fn text_path_result() -> *const u8 {
    PATH.with(|path| path.borrow().as_ptr())
}

/// Address of the bytes of the font file of `family` in that weight.
#[unsafe(no_mangle)]
pub extern "C" fn text_font(family: u32, bold: u32) -> *const u8 {
    font_file(FontFamily::from_code(family), bold != 0)
        .bytes
        .as_ptr()
}

/// Length of the font file of [`text_font`].
#[unsafe(no_mangle)]
pub extern "C" fn text_font_len(family: u32, bold: u32) -> usize {
    font_file(FontFamily::from_code(family), bold != 0)
        .bytes
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn through_memory(text: &str, family: u32, bold: u32, size: f32) -> (Vec<f32>, String) {
        let bytes = text.as_bytes();
        let ptr = text_alloc(bytes.len());
        // SAFETY: ptr holds `bytes.len()` bytes reserved just above (or is null for none).
        unsafe {
            if !bytes.is_empty() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            }
            let n = text_layout(ptr, bytes.len(), family, bold, size);
            let layout = std::slice::from_raw_parts(text_layout_result(), n).to_vec();
            let n = text_path(ptr, bytes.len(), family, bold, size);
            let path =
                String::from_utf8(std::slice::from_raw_parts(text_path_result(), n).to_vec())
                    .unwrap();
            text_free(ptr, bytes.len());
            (layout, path)
        }
    }

    #[test]
    fn gives_exactly_the_native_layout_and_glyphs() {
        for (family, bold) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let text = "Élève « ça »\nα + 2 €";
            let style = style_from(family, bold, 600.0);
            let (layout, path) = through_memory(text, family, bold, 600.0);
            assert_eq!(layout, layout_floats(text, style));
            assert_eq!(path, ardoise_text::text_outline(text, style).1.to_svg());
            assert!(path.starts_with('M'));
            // Two lines.
            assert_eq!(layout[5], 2.0);
        }
    }

    #[test]
    fn the_layout_lists_every_glyph_with_its_utf16_offset() {
        let (layout, _) = through_memory("ab\ncd", 1, 0, 1000.0);
        // [w, h, lh, asc, desc, 2, top, base, w, start, end, n, (x, adv, idx)×n, …]
        assert_eq!(&layout[6..6 + 6][3..], &[0.0, 2.0, 2.0]);
        let first_line_glyphs = 2;
        let second = 6 + 6 + 3 * first_line_glyphs;
        assert_eq!(&layout[second + 3..second + 6], &[3.0, 5.0, 2.0]);
        assert_eq!(layout[second + 6 + 2], 3.0);
    }

    #[test]
    fn empty_text_and_font_bytes() {
        let (layout, path) = through_memory("", 7, 0, 400.0);
        assert_eq!(layout[5], 1.0);
        assert!(path.is_empty());
        for family in [0, 1] {
            for bold in [0, 1] {
                let file = font_file(FontFamily::from_code(family), bold != 0);
                assert_eq!(text_font(family, bold), file.bytes.as_ptr());
                assert_eq!(text_font_len(family, bold), file.bytes.len());
            }
        }
        assert!(text_alloc(0).is_null());
    }
}
