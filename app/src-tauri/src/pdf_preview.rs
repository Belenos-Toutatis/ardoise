//! Thumbnails of the pages of a PDF, for the question « which page goes under the writing? »
//! (SPEC §4.4, P2: the editor's and live mode's Fond tab, `PdfPagePicker.svelte`).
//!
//! The pages are drawn by the renderer the backgrounds already use ([`crate::pdf_pages`],
//! Windows' own PDF renderer): nothing is added. A question opens **a preview** — the PDF read
//! and loaded once, its page sizes kept — and the interface asks for the thumbnail of a page
//! only when it comes into view, a few at a time; each thumbnail is a small PNG sent as a data
//! URL and kept by the interface alone (Rust keeps none). Closing the preview (the question
//! answered, cancelled, or another file chosen) frees the loaded PDF. A preview the interface
//! forgot to close (a window closed mid-question) is dropped once [`MAX_OPEN`] newer ones exist.

use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use crate::background::{SourceKind, pdf_error, read_source};
use crate::commands::{CommandError, CommandResult};
use crate::notes::blocking;
use crate::pdf_pages::{PdfError, PdfFile, PdfPageSize};
use crate::thumbnails::png_data_url;

/// Previews kept at once (one per open question; a few spare for windows that vanished).
const MAX_OPEN: usize = 4;
/// Narrowest and widest thumbnail asked for, in pixels (the interface asks for about
/// 120 CSS pixels at the screen's pixel ratio).
const MIN_WIDTH: u32 = 32;
const MAX_WIDTH: u32 = 512;
/// A page taller than this many times its width is drawn smaller, in its own proportions (a
/// scroll of 1 : 20 would give an image of thousands of pixels).
const MAX_HEIGHT_RATIO: f32 = 2.5;

/// Open items by token, the oldest dropped when more than [`MAX_OPEN`] are held.
struct Registry<T> {
    next: u32,
    items: Vec<(u32, Arc<T>)>,
}

impl<T> Registry<T> {
    const fn new() -> Self {
        Registry {
            next: 1,
            items: Vec::new(),
        }
    }

    fn put(&mut self, item: T) -> u32 {
        let token = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.items.push((token, Arc::new(item)));
        if self.items.len() > MAX_OPEN {
            self.items.remove(0);
        }
        token
    }

    fn get(&self, token: u32) -> Option<Arc<T>> {
        self.items
            .iter()
            .find(|(held, _)| *held == token)
            .map(|(_, item)| Arc::clone(item))
    }

    fn remove(&mut self, token: u32) {
        self.items.retain(|(held, _)| *held != token);
    }
}

/// A PDF loaded for its thumbnails.
struct Preview {
    file: PdfFile,
    sizes: Vec<PdfPageSize>,
    /// The renderer draws one page at a time (the interface asks for two at once to hide the
    /// round trip, not to render in parallel).
    rendering: Mutex<()>,
}

static PREVIEWS: LazyLock<Mutex<Registry<Preview>>> = LazyLock::new(|| Mutex::new(Registry::new()));

fn closed(token: u32) -> CommandError {
    CommandError::new(
        "pdfPreviewClosed",
        format!("the preview {token} of the PDF is no longer open"),
    )
}

/// The pixel size of the thumbnail of a page of `size` points, `width` pixels wide at most
/// (clamped to [`MIN_WIDTH`]..=[`MAX_WIDTH`]): in the page's proportions, narrower when the page
/// is taller than [`MAX_HEIGHT_RATIO`] times its width.
pub fn thumbnail_size(size: PdfPageSize, width: u32) -> (u32, u32) {
    let width = width.clamp(MIN_WIDTH, MAX_WIDTH) as f32;
    let ratio = (size.height.max(1.0) / size.width.max(1.0)).max(0.01);
    let (w, h) = if ratio > MAX_HEIGHT_RATIO {
        (width * MAX_HEIGHT_RATIO / ratio, width * MAX_HEIGHT_RATIO)
    } else {
        (width, width * ratio)
    };
    (w.round().max(1.0) as u32, h.round().max(1.0) as u32)
}

/// Loads the PDF at `path` and keeps it under a token.
fn open_preview(path: &Path) -> CommandResult<u32> {
    let (bytes, kind) = read_source(path)?;
    if kind != SourceKind::Pdf {
        return Err(CommandError::new(
            "backgroundUnsupported",
            format!("{} is not a PDF", path.display()),
        ));
    }
    let file = PdfFile::load(&bytes).map_err(pdf_error)?;
    drop(bytes);
    let sizes = file.page_sizes().map_err(pdf_error)?;
    let preview = Preview {
        file,
        sizes,
        rendering: Mutex::new(()),
    };
    Ok(PREVIEWS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .put(preview))
}

/// Page `page` (from 1) of the preview `token` as a `data:image/png` URL of about `width` pixels.
fn render_thumbnail(token: u32, page: u32, width: u32) -> CommandResult<String> {
    let preview = PREVIEWS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(token)
        .ok_or_else(|| closed(token))?;
    let size = page
        .checked_sub(1)
        .and_then(|index| preview.sizes.get(index as usize))
        .copied()
        .ok_or_else(|| pdf_error(PdfError::NoSuchPage(page)))?;
    let (w, h) = thumbnail_size(size, width);
    let _one_at_a_time = preview
        .rendering
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let png = preview.file.render_page(page, w, h).map_err(pdf_error)?;
    Ok(png_data_url(&png))
}

fn close_preview(token: u32) {
    PREVIEWS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(token);
}

/// Loads the PDF at `path` (chosen by the user anywhere) for its thumbnails; answers the token
/// to give back.
#[tauri::command]
pub async fn open_pdf_preview(path: String) -> CommandResult<u32> {
    blocking(move || open_preview(Path::new(&path))).await
}

/// The thumbnail of page `page` (from 1) of the open preview, as a PNG data URL.
#[tauri::command]
pub async fn render_pdf_thumbnail(token: u32, page: u32, width: u32) -> CommandResult<String> {
    blocking(move || render_thumbnail(token, page, width)).await
}

/// Frees the preview (nothing happens when it is already closed).
#[tauri::command]
pub fn close_pdf_preview(token: u32) {
    close_preview(token);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f32, height: f32) -> PdfPageSize {
        PdfPageSize { width, height }
    }

    #[test]
    fn a_thumbnail_keeps_the_proportions_of_its_page() {
        // A4 portrait and landscape at 128 px wide.
        assert_eq!(thumbnail_size(size(595.0, 842.0), 128), (128, 181));
        assert_eq!(thumbnail_size(size(842.0, 595.0), 128), (128, 90));
        assert_eq!(thumbnail_size(size(100.0, 100.0), 128), (128, 128));
    }

    #[test]
    fn a_thumbnail_is_never_absurdly_large_or_small() {
        // Asked widths are clamped.
        assert_eq!(
            thumbnail_size(size(100.0, 100.0), 1),
            (MIN_WIDTH, MIN_WIDTH)
        );
        assert_eq!(
            thumbnail_size(size(100.0, 100.0), 100_000),
            (MAX_WIDTH, MAX_WIDTH)
        );
        // A scroll 20 times taller than wide: 2.5 times the width high, narrower.
        assert_eq!(thumbnail_size(size(50.0, 1000.0), 128), (16, 320));
        // A degenerate page still gives a drawable size.
        let (w, h) = thumbnail_size(size(0.0, 0.0), 128);
        assert!(w >= 1 && h >= 1);
        let (w, h) = thumbnail_size(size(1e12, 1.0), 128);
        assert!(w >= 1 && h >= 1);
    }

    #[test]
    fn the_registry_forgets_the_oldest_and_the_closed() {
        let mut registry = Registry::new();
        let tokens: Vec<u32> = (0..MAX_OPEN as u32 + 2).map(|n| registry.put(n)).collect();
        // The two first were dropped to keep MAX_OPEN.
        assert!(registry.get(tokens[0]).is_none());
        assert!(registry.get(tokens[1]).is_none());
        assert_eq!(registry.get(tokens[2]).as_deref(), Some(&2));
        registry.remove(tokens[2]);
        assert!(registry.get(tokens[2]).is_none());
        assert_eq!(registry.get(tokens[5]).as_deref(), Some(&5));
        // Tokens are never 0, and never repeat in a row.
        assert!(tokens.iter().all(|t| *t > 0));
        assert!(tokens.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn a_thumbnail_of_a_closed_preview_is_refused_with_its_code() {
        assert_eq!(
            render_thumbnail(u32::MAX, 1, 128).unwrap_err().code(),
            "pdfPreviewClosed"
        );
        // Closing what is not open is harmless.
        close_preview(u32::MAX);
    }

    #[test]
    fn only_a_pdf_can_be_previewed() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("notes.txt");
        std::fs::write(&text, b"pas un PDF").unwrap();
        assert_eq!(
            open_preview(&text).unwrap_err().code(),
            "backgroundUnsupported"
        );
        let missing = dir.path().join("absent.pdf");
        assert_eq!(
            open_preview(&missing).unwrap_err().code(),
            "backgroundUnreadable"
        );
    }

    /// A PDF of `count` pages (A4 portrait, every fifth one landscape), made by the app's own exporter.
    #[cfg(windows)]
    fn pdf_of(count: usize) -> Vec<u8> {
        use crate::library::blank_note;
        use ardoise_core::note::{A4_LANDSCAPE, A4_PORTRAIT};
        use ardoise_ink::{Point, Rgba, Stroke, Tool};
        let notes: Vec<_> = (0..count)
            .map(|i| {
                let mut note = blank_note(&format!("Page {}", i + 1), "Calque 1", 0);
                note.page = if i % 5 == 4 {
                    A4_LANDSCAPE
                } else {
                    A4_PORTRAIT
                };
                // Some ink, so that a page is not blank: a zigzag of 300 points.
                note.strokes.push(Stroke {
                    tool: Tool::FeltTip,
                    color: Rgba::BLACK,
                    width: 1500.0,
                    layer: 0,
                    points: (0..300)
                        .map(|k| Point {
                            x: 2000.0 + (k % 2) as f32 * 12_000.0,
                            y: 1500.0 + k as f32 * 80.0,
                            pressure: 1.0,
                            t: k,
                        })
                        .collect(),
                });
                note
            })
            .collect();
        let refs: Vec<_> = notes.iter().collect();
        ardoise_render::notes_to_pdf(&refs, &ardoise_render::PdfOptions::default()).unwrap()
    }

    /// Windows draws the thumbnails: sizes follow the pages, any page can be asked for, a wrong
    /// page and a closed preview answer their codes.
    #[cfg(windows)]
    #[test]
    fn windows_renders_the_thumbnails_of_a_pdf_of_200_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.pdf");
        std::fs::write(&path, pdf_of(200)).unwrap();
        let token = open_preview(&path).unwrap();
        // The app's pages are 216 x 297 mm (portrait) or 297 x 216 mm (landscape).
        for (page, expected) in [(1u32, (128u32, 176u32)), (5, (128, 93)), (200, (128, 93))] {
            let url = render_thumbnail(token, page, 128).unwrap();
            let encoded = url
                .strip_prefix("data:image/png;base64,")
                .expect("a PNG data URL");
            let png = crate::thumbnails::tests::decode_base64(encoded);
            let (w, h) = ardoise_render::picture::dimensions(&png).unwrap();
            assert!(
                w.abs_diff(expected.0) <= 1 && h.abs_diff(expected.1) <= 1,
                "page {page}: {w} x {h}, expected {expected:?}"
            );
        }
        for page in [0, 201] {
            assert_eq!(
                render_thumbnail(token, page, 128).unwrap_err().code(),
                "pdfNoSuchPage"
            );
        }
        close_preview(token);
        assert_eq!(
            render_thumbnail(token, 1, 128).unwrap_err().code(),
            "pdfPreviewClosed"
        );
    }

    /// The numbers of the report (`cargo test -p ardoise measure -- --ignored --nocapture`): the time to open a PDF of
    /// 200 pages, to draw the first thumbnail, twelve more and all of them, and the process's memory around them.
    #[cfg(windows)]
    #[test]
    #[ignore = "a measure, not a check"]
    fn measure_the_preview_of_a_pdf_of_200_pages() {
        use std::time::Instant;
        let memory = || {
            let output = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "[math]::Round((Get-Process -Id {}).WorkingSet64 / 1MB)",
                        std::process::id()
                    ),
                ])
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.pdf");
        let bytes = pdf_of(200);
        std::fs::write(&path, &bytes).unwrap();
        println!(
            "PDF of 200 pages: {} KiB; memory before: {} MB",
            bytes.len() / 1024,
            memory()
        );
        let started = Instant::now();
        let token = open_preview(&path).unwrap();
        println!("open: {:?}; memory {} MB", started.elapsed(), memory());
        let started = Instant::now();
        let first = render_thumbnail(token, 1, 192).unwrap();
        println!(
            "first thumbnail: {:?} ({} bytes of data URL)",
            started.elapsed(),
            first.len()
        );
        let started = Instant::now();
        for page in 2..=13 {
            render_thumbnail(token, page, 192).unwrap();
        }
        println!("12 more: {:?}; memory {} MB", started.elapsed(), memory());
        let started = Instant::now();
        for page in 14..=200 {
            render_thumbnail(token, page, 192).unwrap();
        }
        println!(
            "the other 187 (the interface never asks for them all): {:?}; memory {} MB",
            started.elapsed(),
            memory()
        );
        close_preview(token);
        println!("closed; memory {} MB", memory());
    }
}
