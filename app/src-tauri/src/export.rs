//! Exports of notes to PNG, JPG, WebP, SVG and PDF (SPEC §4.6), for the editor and the library.
//! Contract: `docs/design/exports-and-operations.md`.
//!
//! - PDF: one file holding every source, one page each.
//! - PNG, JPG, WebP, SVG: a file for one source; for several, a folder with one file per note,
//!   named like the note file, ` (2)` added rather than overwriting anything.
//!
//! Every file is written atomically (`ardoise_core::fsutil`). Rendering, encoding and writing
//! failures are `exportFailed` (`imageTooLarge` when the image would exceed the size limit);
//! reading a note of the library fails with the library's codes. The export can be cancelled
//! (`cancel_export`): it then stops between two notes and before any write, keeping the files
//! already written in a folder, and fails with `cancelled`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ardoise_core::fsutil;
use ardoise_core::library::{Library, LibraryError, note_file_stem};
use ardoise_core::note::Note;
use ardoise_render::{
    MAX_DPI, MIN_DPI, NoteView, PdfOptions, PdfPage, Picture, RenderError, RenderOptions,
    SvgOptions, encode_jpeg_at, encode_png_at, encode_webp, note_to_svg, notes_to_pdf, render_note,
    render_note_transparent, scale_for_dpi,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::background;
use crate::commands::{CommandError, CommandResult};
use crate::library::{library_error, library_of, path_text};

/// Error code of every rendering, encoding or writing failure.
pub const EXPORT_FAILED: &str = "exportFailed";
/// Error code when an image would be larger than the renderer allows.
pub const IMAGE_TOO_LARGE: &str = "imageTooLarge";
/// Error code of an export stopped by the user.
pub const CANCELLED: &str = "cancelled";
/// Highest ` (n)` tried for a free file name in a folder.
const MAX_SUFFIX: u32 = 999;

/// `ExportFormat` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Png,
    Jpg,
    Webp,
    Svg,
    Pdf,
}

impl ExportFormat {
    /// File extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpg => "jpg",
            ExportFormat::Webp => "webp",
            ExportFormat::Svg => "svg",
            ExportFormat::Pdf => "pdf",
        }
    }
}

/// Page size of a PDF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PdfPageSize {
    /// The note's own size.
    Actual,
    /// 210 × 297 mm, in the note's orientation, the note fitted and centred.
    A4,
}

impl From<PdfPageSize> for PdfPage {
    fn from(size: PdfPageSize) -> Self {
        match size {
            PdfPageSize::Actual => PdfPage::Actual,
            PdfPageSize::A4 => PdfPage::A4,
        }
    }
}

/// `ExportOptions` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    /// Resolution of raster formats (72..=600).
    pub dpi: u32,
    /// Draw the paper (colour, template, texture); without it PNG is transparent, JPG white.
    pub background: bool,
    /// JPG quality, 1..=100.
    pub quality: u8,
    pub pdf_page: PdfPageSize,
}

/// A note to export: the note at `path`, or the editor's current `note` (unsaved changes
/// included). `path` also names the output file. `image_key` names the staged pixels of the
/// editor note's background image (`background::Staging`); without it they come from the file
/// at `path`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSource {
    pub path: String,
    #[serde(default)]
    pub note: Option<Note>,
    #[serde(default)]
    pub image_key: Option<String>,
}

/// A note ready to export, with the file name (without extension) its export takes and the
/// pixels of its background image.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSource {
    pub note: Note,
    pub stem: String,
    pub picture: Option<Arc<Picture>>,
}

impl LoadedSource {
    /// The note with its pixels, as the renderer takes it.
    pub fn view(&self) -> NoteView<'_> {
        NoteView::new(&self.note, self.picture.as_deref())
    }
}

pub(crate) fn export_error(message: impl ToString) -> CommandError {
    CommandError::new(EXPORT_FAILED, message)
}

/// A rendering or encoding failure: `imageTooLarge` when the image side would exceed the limit,
/// else `exportFailed`.
pub(crate) fn render_error(error: RenderError) -> CommandError {
    match error {
        RenderError::Size { width, height } if width > 0 && height > 0 => {
            CommandError::new(IMAGE_TOO_LARGE, RenderError::Size { width, height })
        }
        other => export_error(other),
    }
}

/// The flag of the export under way, set by `cancel_export`.
#[derive(Debug, Default, Clone)]
pub struct ExportCancel(pub Arc<AtomicBool>);

/// `cancelled` once the user asked to stop.
fn check_cancelled(cancel: &AtomicBool) -> CommandResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(CommandError::new(CANCELLED, "export cancelled"))
    } else {
        Ok(())
    }
}

fn write_error(path: &Path) -> impl FnOnce(io::Error) -> CommandError + '_ {
    move |error| export_error(format!("cannot write {}: {error}", path.display()))
}

/// Refuses a resolution outside 72..=600 dpi.
pub fn check_dpi(dpi: u32) -> CommandResult<()> {
    if (MIN_DPI..=MAX_DPI).contains(&dpi) {
        Ok(())
    } else {
        Err(export_error(format!(
            "resolution {dpi} dpi outside {MIN_DPI}..={MAX_DPI}"
        )))
    }
}

/// Checks the options that `format` uses.
fn check_options(format: ExportFormat, options: &ExportOptions) -> CommandResult<()> {
    if matches!(
        format,
        ExportFormat::Png | ExportFormat::Jpg | ExportFormat::Webp
    ) {
        check_dpi(options.dpi)?;
    }
    if format == ExportFormat::Jpg && !(1..=100).contains(&options.quality) {
        return Err(export_error(format!(
            "JPG quality {} outside 1..=100",
            options.quality
        )));
    }
    Ok(())
}

fn no_library() -> CommandError {
    CommandError::new("noLibrary", "no library is chosen on this computer")
}

/// The notes of `sources`: the note given with a source, or else the note at its path, which
/// must be a note of `library` (`noLibrary` when a note must be read and there is none). The
/// pixels of a note's background image are the staged ones of its key, or else the file's
/// (an editor note not saved yet has none).
pub fn load_sources(
    library: Option<&Library>,
    sources: Vec<ExportSource>,
) -> CommandResult<Vec<LoadedSource>> {
    sources
        .into_iter()
        .map(|source| {
            let path = Path::new(&source.path);
            let key = source.image_key.as_deref();
            let (note, picture) = match source.note {
                Some(note) => {
                    let member = match (&note.background_image, key, library) {
                        (Some(_), None, Some(library)) if !source.path.is_empty() => {
                            match library.read(path) {
                                Ok(loaded) => loaded.file.background,
                                Err(LibraryError::NotFound { .. }) => None,
                                Err(error) => return Err(library_error(error)),
                            }
                        }
                        _ => None,
                    };
                    let picture = background::source_picture(&note, key, member.as_ref())?;
                    (note, picture)
                }
                None => {
                    let file = library
                        .ok_or_else(no_library)?
                        .read(path)
                        .map_err(library_error)?
                        .file;
                    let picture =
                        background::source_picture(&file.note, None, file.background.as_ref())?;
                    (file.note, picture)
                }
            };
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().trim().to_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| note_file_stem(note.created, &note.title));
            Ok(LoadedSource {
                note,
                stem,
                picture,
            })
        })
        .collect()
}

/// The file of a note (`&Note`, or a view with its background image's pixels) in an image
/// format (PNG, JPG, SVG).
pub fn image_bytes<'a>(
    view: impl Into<NoteView<'a>>,
    format: ExportFormat,
    options: &ExportOptions,
) -> CommandResult<Vec<u8>> {
    let view = view.into();
    let dpi = Some(options.dpi);
    match format {
        ExportFormat::Png if options.background => {
            let image = render_note(view, &RenderOptions::at_dpi(options.dpi, true))
                .map_err(render_error)?;
            encode_png_at(&image, dpi).map_err(render_error)
        }
        ExportFormat::Png => {
            // No paper, no background image: the strokes alone.
            let image = render_note_transparent(view.note, scale_for_dpi(options.dpi))
                .map_err(render_error)?;
            encode_png_at(&image, dpi).map_err(render_error)
        }
        ExportFormat::Jpg => {
            // Without the paper, a white page (JPG has no transparency).
            let image = render_note(
                view,
                &RenderOptions::at_dpi(options.dpi, options.background),
            )
            .map_err(render_error)?;
            encode_jpeg_at(&image, options.quality, dpi).map_err(render_error)
        }
        ExportFormat::Webp => {
            // Lossless like PNG; without the paper the pixels keep their transparency.
            let image = if options.background {
                render_note(view, &RenderOptions::at_dpi(options.dpi, true))
            } else {
                render_note_transparent(view.note, scale_for_dpi(options.dpi))
            }
            .map_err(render_error)?;
            encode_webp(&image).map_err(render_error)
        }
        ExportFormat::Svg => Ok(note_to_svg(
            view,
            &SvgOptions {
                background: options.background,
            },
        )
        .into_bytes()),
        ExportFormat::Pdf => notes_to_pdf(&[view], &pdf_options(options)).map_err(render_error),
    }
}

fn pdf_options(options: &ExportOptions) -> PdfOptions {
    PdfOptions {
        page: options.pdf_page.into(),
        background: options.background,
    }
}

/// Writes `bytes` as `stem.extension` in `dir`, or `stem (2).extension`… if taken: never
/// over an existing file.
pub fn write_new_file(
    dir: &Path,
    stem: &str,
    extension: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    for n in 1..=MAX_SUFFIX {
        let name = if n == 1 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({n}).{extension}")
        };
        let path = dir.join(name);
        match fsutil::write_atomic_new(&path, bytes) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("no free name for {stem}.{extension}"),
    ))
}

/// Exports `sources` to `destination` (see the module documentation); returns the paths
/// written, in the order of the sources. `cancel` is checked between notes and before each
/// write: once set, nothing more is written (a single file or a PDF is not written at all;
/// the files of a folder already written stay) and the error is `cancelled`.
pub fn export(
    sources: &[LoadedSource],
    format: ExportFormat,
    options: &ExportOptions,
    destination: &Path,
    cancel: &AtomicBool,
) -> CommandResult<Vec<PathBuf>> {
    check_options(format, options)?;
    check_cancelled(cancel)?;
    match sources {
        [] => Err(export_error("nothing to export")),
        _ if format == ExportFormat::Pdf => {
            let notes: Vec<NoteView<'_>> = sources.iter().map(LoadedSource::view).collect();
            let pdf = notes_to_pdf(&notes, &pdf_options(options)).map_err(render_error)?;
            check_cancelled(cancel)?;
            fsutil::write_atomic(destination, &pdf).map_err(write_error(destination))?;
            Ok(vec![destination.to_path_buf()])
        }
        [single] => {
            let bytes = image_bytes(single.view(), format, options)?;
            check_cancelled(cancel)?;
            fsutil::write_atomic(destination, &bytes).map_err(write_error(destination))?;
            Ok(vec![destination.to_path_buf()])
        }
        several => {
            std::fs::create_dir_all(destination).map_err(write_error(destination))?;
            several
                .iter()
                .map(|source| {
                    check_cancelled(cancel)?;
                    let bytes = image_bytes(source.view(), format, options)?;
                    check_cancelled(cancel)?;
                    write_new_file(destination, &source.stem, format.extension(), &bytes)
                        .map_err(write_error(destination))
                })
                .collect()
        }
    }
}

/// Loads and exports `sources` (the command's work, off the interface thread).
pub fn export_sources(
    library: Option<&Library>,
    sources: Vec<ExportSource>,
    format: ExportFormat,
    options: &ExportOptions,
    destination: &Path,
    cancel: &AtomicBool,
) -> CommandResult<Vec<PathBuf>> {
    // Refuse bad options before reading any note.
    check_options(format, options)?;
    check_cancelled(cancel)?;
    let loaded = load_sources(library, sources)?;
    export(&loaded, format, options, destination, cancel)
}

/// The library, if one of `sources` must be read from it (a note not given, or the pixels of a
/// given note's background image when they are not staged).
pub(crate) fn library_for(
    context: &AppContext,
    sources: &[ExportSource],
) -> CommandResult<Option<Library>> {
    let given = |s: &ExportSource| {
        s.note
            .as_ref()
            .is_some_and(|note| note.background_image.is_none() || s.image_key.is_some())
    };
    if sources.iter().all(given) {
        Ok(None)
    } else {
        library_of(context).map(Some)
    }
}

/// Asks the export under way to stop (`export_notes` then fails with `cancelled`).
#[tauri::command]
pub fn cancel_export(cancel: State<'_, ExportCancel>) {
    cancel.0.store(true, Ordering::Relaxed);
}

/// Exports `sources` to `destination` (a file, or a folder for several notes in an image
/// format); returns the paths written.
#[tauri::command]
pub async fn export_notes(
    context: State<'_, AppContext>,
    cancel: State<'_, ExportCancel>,
    sources: Vec<ExportSource>,
    format: ExportFormat,
    options: ExportOptions,
    destination: String,
) -> CommandResult<Vec<String>> {
    let library = library_for(&context, &sources)?;
    let flag = Arc::clone(&cancel.0);
    flag.store(false, Ordering::Relaxed);
    // Rendering at 600 dpi takes seconds: keep it off the async runtime's threads.
    let written = tauri::async_runtime::spawn_blocking(move || {
        export_sources(
            library.as_ref(),
            sources,
            format,
            &options,
            Path::new(&destination),
            &flag,
        )
    })
    .await
    .map_err(export_error)??;
    Ok(written.iter().map(|p| path_text(p)).collect())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);
    use crate::library::tests::{library, sample_note, write_sample};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    fn options(dpi: u32, background: bool) -> ExportOptions {
        ExportOptions {
            dpi,
            background,
            quality: 85,
            pdf_page: PdfPageSize::Actual,
        }
    }

    fn from_library(path: &Path) -> ExportSource {
        ExportSource {
            path: path_text(path),
            note: None,
            image_key: None,
        }
    }

    fn pdf_pages(bytes: &[u8]) -> Vec<Vec<f32>> {
        let document = lopdf::Document::load_mem(bytes).unwrap();
        document
            .get_pages()
            .values()
            .map(|&id| {
                document
                    .get_dictionary(id)
                    .unwrap()
                    .get(b"MediaBox")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_float().unwrap())
                    .collect()
            })
            .collect()
    }

    /// Size of a lossless WebP file, from its RIFF container and the VP8L header (the decoding
    /// itself is tested in `ardoise-render`).
    fn webp_size(bytes: &[u8]) -> (u32, u32) {
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WEBPVP8L");
        assert_eq!(bytes[20], 0x2f, "VP8L signature");
        let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
        ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
    }

    #[test]
    fn webp_notes_behave_like_png() {
        let (_dir, library) = library();
        let a = write_sample(&library, "", "Un");
        let b = write_sample(&library, "Classe", "Deux");
        let out = tempfile::tempdir().unwrap();
        // Several notes: a folder, one file per note, never over an existing one.
        let folder = out.path().join("folder");
        let sources = || vec![from_library(&a.path), from_library(&b.path)];
        for _ in 0..2 {
            let written = export_sources(
                Some(&library),
                sources(),
                ExportFormat::Webp,
                &options(72, true),
                &folder,
                &NOT_CANCELLED,
            )
            .unwrap();
            assert_eq!(written.len(), 2);
            assert!(written.iter().all(|p| p.extension().unwrap() == "webp"));
        }
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 4);
        // Without the paper the file is transparent (alpha channel: VP8L header bit 28).
        let note = sample_note("Libre");
        let clear = image_bytes(&note, ExportFormat::Webp, &options(72, false)).unwrap();
        let paper = image_bytes(&note, ExportFormat::Webp, &options(72, true)).unwrap();
        // Lighter than the PNG of the same page.
        let png = image_bytes(&note, ExportFormat::Png, &options(72, true)).unwrap();
        assert!(
            paper.len() < png.len(),
            "{} against {}",
            paper.len(),
            png.len()
        );
        let alpha = |bytes: &[u8]| bytes[24] & 0x10 != 0;
        assert!(alpha(&clear) && !alpha(&paper));
        // Same resolution rules as PNG.
        let error = check_options(ExportFormat::Webp, &options(10, true)).unwrap_err();
        assert_eq!(error.code(), EXPORT_FAILED);
        let mut huge = sample_note("Immense");
        huge.page.width = 3_000_000;
        let error = image_bytes(&huge, ExportFormat::Webp, &options(300, true)).unwrap_err();
        assert_eq!(error.code(), IMAGE_TOO_LARGE);
    }

    #[test]
    fn options_and_formats_have_the_interface_shape() {
        let options: ExportOptions = serde_json::from_value(serde_json::json!({
            "dpi": 300, "background": false, "quality": 90, "pdfPage": "a4"
        }))
        .unwrap();
        assert_eq!(options.pdf_page, PdfPageSize::A4);
        assert_eq!(options.dpi, 300);
        let format: ExportFormat = serde_json::from_value(serde_json::json!("jpg")).unwrap();
        assert_eq!(format, ExportFormat::Jpg);
        let source: ExportSource =
            serde_json::from_value(serde_json::json!({ "path": "C:\\Notes\\a.ardoise" })).unwrap();
        assert!(source.note.is_none());
        let with_note: ExportSource = serde_json::from_value(serde_json::json!({
            "path": "a.ardoise", "note": serde_json::to_value(sample_note("x")).unwrap()
        }))
        .unwrap();
        assert_eq!(with_note.note.unwrap().title, "x");
    }

    #[test]
    fn one_note_to_png_jpg_webp_svg_and_pdf_files() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Essai");
        let out = tempfile::tempdir().unwrap();
        for format in [
            ExportFormat::Png,
            ExportFormat::Jpg,
            ExportFormat::Webp,
            ExportFormat::Svg,
            ExportFormat::Pdf,
        ] {
            let destination = out.path().join(format!("export.{}", format.extension()));
            let written = export_sources(
                Some(&library),
                vec![from_library(&saved.path)],
                format,
                &options(150, true),
                &destination,
                &NOT_CANCELLED,
            )
            .unwrap();
            assert_eq!(written, std::slice::from_ref(&destination));
            let bytes = fs::read(&destination).unwrap();
            match format {
                ExportFormat::Png => {
                    let image = ardoise_render::decode_png(&bytes).unwrap();
                    // A4 landscape at 150 dpi.
                    assert_eq!((image.width, image.height), (1754, 1276));
                }
                ExportFormat::Jpg => assert_eq!(&bytes[..3], [0xff, 0xd8, 0xff]),
                ExportFormat::Webp => assert_eq!(webp_size(&bytes), (1754, 1276)),
                ExportFormat::Svg => {
                    let text = String::from_utf8(bytes).unwrap();
                    assert!(text.contains("viewBox=\"0 0 29700 21600\""));
                    assert_eq!(text.matches("<path ").count(), 1);
                }
                ExportFormat::Pdf => assert_eq!(pdf_pages(&bytes).len(), 1),
            }
        }
        // No temporary file left behind.
        let names: Vec<String> = fs::read_dir(out.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 5, "{names:?}");
    }

    #[test]
    fn a_file_destination_is_overwritten() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let out = tempfile::tempdir().unwrap();
        let destination = out.path().join("note.svg");
        fs::write(&destination, "old").unwrap();
        export_sources(
            Some(&library),
            vec![from_library(&saved.path)],
            ExportFormat::Svg,
            &options(150, true),
            &destination,
            &NOT_CANCELLED,
        )
        .unwrap();
        assert!(
            fs::read_to_string(&destination)
                .unwrap()
                .starts_with("<?xml")
        );
    }

    #[test]
    fn several_notes_go_to_a_folder_without_overwriting() {
        let (_dir, library) = library();
        let a = write_sample(&library, "", "Un");
        let b = write_sample(&library, "Classe", "Deux");
        // The same file name in another binder.
        let c = write_sample(&library, "Autre", "Un");
        let out = tempfile::tempdir().unwrap();
        let folder = out.path().join("exports");
        let stem = |p: &Path| p.file_stem().unwrap().to_string_lossy().into_owned();
        // A file already there keeps its content.
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{}.png", stem(&b.path))), "keep").unwrap();

        let written = export_sources(
            Some(&library),
            vec![
                from_library(&a.path),
                from_library(&b.path),
                from_library(&c.path),
            ],
            ExportFormat::Png,
            &options(72, false),
            &folder,
            &NOT_CANCELLED,
        )
        .unwrap();
        let names: Vec<String> = written
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                format!("{}.png", stem(&a.path)),
                format!("{} (2).png", stem(&b.path)),
                format!("{} (2).png", stem(&c.path)),
            ]
        );
        assert_eq!(
            fs::read_to_string(folder.join(format!("{}.png", stem(&b.path)))).unwrap(),
            "keep"
        );
        // Background off: transparent PNG.
        let image = ardoise_render::decode_png(&fs::read(&written[0]).unwrap()).unwrap();
        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, 0]));
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 4);
    }

    #[test]
    fn a_pdf_holds_every_note_in_order() {
        let (_dir, library) = library();
        let a = write_sample(&library, "", "Un");
        let mut portrait = sample_note("Deux");
        portrait.page = ardoise_core::note::A4_PORTRAIT;
        let out = tempfile::tempdir().unwrap();
        let destination = out.path().join("deux notes.pdf");
        let mut a4 = options(150, true);
        a4.pdf_page = PdfPageSize::A4;
        let written = export_sources(
            Some(&library),
            vec![
                from_library(&a.path),
                ExportSource {
                    path: String::new(),
                    note: Some(portrait),
                    image_key: None,
                },
            ],
            ExportFormat::Pdf,
            &a4,
            &destination,
            &NOT_CANCELLED,
        )
        .unwrap();
        assert_eq!(written, std::slice::from_ref(&destination));
        let boxes = pdf_pages(&fs::read(&destination).unwrap());
        assert_eq!(boxes.len(), 2);
        // A4 landscape, then A4 portrait.
        assert!((boxes[0][2] - 841.89).abs() < 0.01 && (boxes[0][3] - 595.28).abs() < 0.01);
        assert!((boxes[1][2] - 595.28).abs() < 0.01 && (boxes[1][3] - 841.89).abs() < 0.01);
    }

    #[test]
    fn the_editor_note_is_exported_with_its_unsaved_changes() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let mut edited = library.read(&saved.path).unwrap().file.note;
        edited.strokes.push(Stroke {
            tool: Tool::Highlighter,
            color: Rgba::rgb(255, 230, 0),
            width: 400.0,
            layer: 0,
            points: (0..10)
                .map(|i| Point {
                    x: 2000.0 + 500.0 * i as f32,
                    y: 8000.0,
                    pressure: 1.0,
                    t: 5 * i,
                })
                .collect(),
        });
        let loaded = load_sources(
            None,
            vec![ExportSource {
                path: path_text(&saved.path),
                note: Some(edited),
                image_key: None,
            }],
        )
        .unwrap();
        assert_eq!(loaded[0].note.strokes.len(), 2);
        assert_eq!(
            loaded[0].stem,
            saved.path.file_stem().unwrap().to_string_lossy()
        );
        let svg = image_bytes(loaded[0].view(), ExportFormat::Svg, &options(150, true)).unwrap();
        let svg = String::from_utf8(svg).unwrap();
        assert_eq!(svg.matches("<path ").count(), 2);
        assert!(svg.contains("mix-blend-mode:multiply"));

        // An unsaved note without a file name is named after its date and title.
        let unnamed = load_sources(
            None,
            vec![ExportSource {
                path: String::new(),
                note: Some(sample_note("Brouillon")),
                image_key: None,
            }],
        )
        .unwrap();
        assert!(
            unnamed[0].stem.ends_with(" Brouillon"),
            "{}",
            unnamed[0].stem
        );
    }

    #[test]
    fn bad_options_sources_and_destinations_are_refused() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let out = tempfile::tempdir().unwrap();
        let run = |format, options: &ExportOptions, sources: Vec<ExportSource>| {
            export_sources(
                Some(&library),
                sources,
                format,
                options,
                &out.path().join("x"),
                &NOT_CANCELLED,
            )
            .unwrap_err()
            .code()
        };
        let one = || vec![from_library(&saved.path)];
        assert_eq!(
            run(ExportFormat::Png, &options(50, true), one()),
            EXPORT_FAILED
        );
        assert_eq!(
            run(ExportFormat::Jpg, &options(601, true), one()),
            EXPORT_FAILED
        );
        let mut no_quality = options(150, true);
        no_quality.quality = 0;
        assert_eq!(run(ExportFormat::Jpg, &no_quality, one()), EXPORT_FAILED);
        // Vector formats have no resolution: the dpi does not matter.
        assert!(
            export_sources(
                Some(&library),
                one(),
                ExportFormat::Svg,
                &options(0, true),
                &out.path().join("ok.svg"),
                &NOT_CANCELLED,
            )
            .is_ok()
        );
        assert_eq!(
            run(ExportFormat::Pdf, &options(150, true), vec![]),
            EXPORT_FAILED
        );

        // Library errors keep their codes.
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("2026-10-24 Ailleurs.ardoise");
        assert_eq!(
            run(
                ExportFormat::Svg,
                &options(150, true),
                vec![from_library(&outside)]
            ),
            "notInLibrary"
        );
        let missing = library.root().join("2026-10-24 Absente.ardoise");
        assert_eq!(
            run(
                ExportFormat::Svg,
                &options(150, true),
                vec![from_library(&missing)]
            ),
            "noteNotFound"
        );
        let error = load_sources(None, one()).unwrap_err();
        assert_eq!(error.code(), "noLibrary");

        // A destination that cannot be written.
        let blocked = out.path().join("file");
        fs::write(&blocked, "not a folder").unwrap();
        let error = export_sources(
            Some(&library),
            one(),
            ExportFormat::Svg,
            &options(150, true),
            &blocked.join("inside.svg"),
            &NOT_CANCELLED,
        )
        .unwrap_err();
        assert_eq!(error.code(), EXPORT_FAILED);
        // A page too large to render has its own code (the message is about the resolution).
        let mut huge = sample_note("Énorme");
        huge.page.height = 10_000_000;
        for format in [ExportFormat::Png, ExportFormat::Jpg] {
            let error = image_bytes(&huge, format, &options(600, true)).unwrap_err();
            assert_eq!(error.code(), IMAGE_TOO_LARGE, "{format:?}");
        }
        let error = image_bytes(&huge, ExportFormat::Png, &options(600, false)).unwrap_err();
        assert_eq!(error.code(), IMAGE_TOO_LARGE);
        // An empty page is not "too large".
        let mut empty = sample_note("Vide");
        empty.page.width = 0;
        let error = image_bytes(&empty, ExportFormat::Png, &options(150, true)).unwrap_err();
        assert_eq!(error.code(), EXPORT_FAILED);
    }

    #[test]
    fn a_cancelled_export_writes_nothing_more() {
        let (_dir, library) = library();
        let a = write_sample(&library, "", "Un");
        let b = write_sample(&library, "Classe", "Deux");
        let out = tempfile::tempdir().unwrap();
        let cancelled = AtomicBool::new(true);
        let sources = || vec![from_library(&a.path), from_library(&b.path)];
        for (format, destination) in [
            (ExportFormat::Pdf, "all.pdf"),
            (ExportFormat::Png, "folder"),
        ] {
            let error = export_sources(
                Some(&library),
                sources(),
                format,
                &options(72, true),
                &out.path().join(destination),
                &cancelled,
            )
            .unwrap_err();
            assert_eq!(error.code(), CANCELLED, "{format:?}");
        }
        let error = export_sources(
            Some(&library),
            vec![from_library(&a.path)],
            ExportFormat::Svg,
            &options(72, true),
            &out.path().join("one.svg"),
            &cancelled,
        )
        .unwrap_err();
        assert_eq!(error.code(), CANCELLED);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);

        // The command's flag is a plain shared boolean: reset, the same export succeeds.
        let cancel = ExportCancel::default();
        cancel.0.store(true, Ordering::Relaxed);
        cancel.0.store(false, Ordering::Relaxed);
        let written = export_sources(
            Some(&library),
            sources(),
            ExportFormat::Png,
            &options(72, true),
            &out.path().join("folder"),
            &cancel.0,
        )
        .unwrap();
        assert_eq!(written.len(), 2);
    }

    #[test]
    fn new_files_never_replace_existing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let first = write_new_file(dir.path(), "a", "png", b"1").unwrap();
        let second = write_new_file(dir.path(), "a", "png", b"2").unwrap();
        let third = write_new_file(dir.path(), "a", "png", b"3").unwrap();
        assert_eq!(first.file_name().unwrap(), "a.png");
        assert_eq!(second.file_name().unwrap(), "a (2).png");
        assert_eq!(third.file_name().unwrap(), "a (3).png");
        assert_eq!(fs::read(&first).unwrap(), b"1");
    }
}
