//! Exports of notes to PNG, JPG, WebP, SVG and PDF (SPEC §4.6), for the editor and the library.
//! Contract: `docs/design/exports-and-operations.md`.
//!
//! - PDF: one file holding every source, one page each.
//! - PNG, JPG, WebP, SVG: a file for one source; for several, a folder with one file per note,
//!   named like the note file, ` (2)` added rather than overwriting anything.
//!
//! Every file is written atomically (`ardoise_core::fsutil`). Rendering, encoding and writing
//! failures are `exportFailed` (`imageTooLarge` when the image would exceed the size limit); no note
//! at all to export is `nothingToExport`; reading a note of the library fails with the library's codes. The export can be cancelled
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
    LayerMode, MAX_DPI, MIN_DPI, NoteView, OutlineEntry, PdfOptions, PdfPage, Picture, RenderError,
    RenderOptions, Summary, SummaryEntry, SummaryKind, SvgOptions, bookmark_outline,
    encode_jpeg_at, encode_png_at, encode_webp, note_to_svg, notes_to_pdf,
    notes_to_pdf_with_summary, render_note, render_note_transparent, scale_for_dpi,
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
/// Error code of an export or a print job without any note (a binder with no note, say).
pub const NOTHING_TO_EXPORT: &str = "nothingToExport";
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

/// Layers of an SVG or PDF export: `ExportOptions.layers` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportLayers {
    /// One layer of the file per layer of the note (Inkscape layers, PDF optional content).
    Separate,
    /// One drawing.
    Merged,
}

impl From<ExportLayers> for LayerMode {
    fn from(layers: ExportLayers) -> Self {
        match layers {
            ExportLayers::Separate => LayerMode::Separate,
            ExportLayers::Merged => LayerMode::Merged,
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
    /// SVG and PDF: separate or merged layers; `None` is the format's default (SVG separate, PDF
    /// merged, the files written before the choice existed). Ignored by the images.
    #[serde(default)]
    pub layers: Option<ExportLayers>,
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

/// Where a note goes in the outline (the bookmarks) of a PDF of several notes (`outline` of
/// `ExportSource` in `note.ts`, sent beside the sources, one per source): the binders it is in below
/// the exported one, and the title its bookmark shows. « Exporter le classeur en PDF » only.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OutlineSpec {
    pub path: Vec<String>,
    pub title: String,
}

/// The summary page(s) put before the notes of a binder's PDF (`summary` of `export_notes`, SPEC §4.6): the text
/// the interface gives, which the rows do not hold: the page's title (the binder's name) and the name of its
/// entry in the PDF's outline.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRequest {
    pub title: String,
    pub outline_title: String,
}

/// What the outline of a binder's PDF, and the summary page made of it, hold (« Dans le sommaire » of the export dialog,
/// SPEC §4.6): the names of the sub-binders as headings, a line per note, the bookmarks of the notes. All three is the
/// outline of before. A note's name is the one its `OutlineSpec` gives (the interface chooses a date or « Note sans titre »).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineShape {
    pub binders: bool,
    pub notes: bool,
    pub bookmarks: bool,
}

impl OutlineShape {
    /// Everything: sub-binders, notes and bookmarks.
    pub const ALL: OutlineShape = OutlineShape {
        binders: true,
        notes: true,
        bookmarks: true,
    };
}

/// What a binder's PDF takes besides its pages: the outline (one spec per source; none, or a number that is not the
/// sources', gives a PDF without outline), what it holds, and the summary page(s) in front of the notes.
#[derive(Debug, Clone, Copy)]
pub struct BinderPdf<'a> {
    pub outline: &'a [OutlineSpec],
    pub shape: OutlineShape,
    pub summary: Option<&'a SummaryRequest>,
}

impl<'a> BinderPdf<'a> {
    /// No outline, no summary: the PDF of notes.
    pub const NONE: BinderPdf<'static> = BinderPdf {
        outline: &[],
        shape: OutlineShape::ALL,
        summary: None,
    };

    /// The outline of before (sub-binders, notes and bookmarks), no summary page.
    #[cfg(test)]
    pub fn outline(outline: &'a [OutlineSpec]) -> Self {
        BinderPdf {
            outline,
            shape: OutlineShape::ALL,
            summary: None,
        }
    }
}

/// The outline of a PDF whose pages are the notes `specs` describe, in order: a bookmark per note,
/// and, for a note below the exported binder, bookmarks for each binder it is in (a binder's
/// bookmark goes to its first note, and holds those of the notes and sub-binders it contains).
/// A binder that comes up again after another one (the sub-binders follow the sidebar's order, so
/// the notes of one are together) is not repeated.
#[cfg(test)]
pub fn outline_entries(specs: &[OutlineSpec]) -> Vec<OutlineEntry> {
    outline_with_bookmarks(specs, &[])
}

/// [`outline_entries`] with, under each note's bookmark, the bookmarks of that note's page
/// (`notes[i]` is the note of `specs[i]`; a shorter list, or none, leaves those notes without):
/// one level deeper than the note, in reading order, each going to its spot on the page
/// (SPEC §4.6 « Exporter un classeur en PDF »).
#[cfg(test)]
pub fn outline_with_bookmarks(specs: &[OutlineSpec], notes: &[&Note]) -> Vec<OutlineEntry> {
    outline_with_kinds(specs, notes, OutlineShape::ALL)
        .into_iter()
        .map(|(entry, _)| entry)
        .collect()
}

/// [`outline_with_bookmarks`], each entry with what it is (a sub-binder, a note, a bookmark): the rows of the
/// summary page are these entries. `shape` leaves out what the user did not ask for: without the notes, the bookmarks
/// stand directly under their sub-binder (or at the top level), each still going to its page and spot; without the
/// sub-binders there are no headings and no level for them; a note with nothing to show gives no line, and a sub-binder
/// with nothing under it no heading. Levels count only the entries shown above, so there is never an empty level.
fn outline_with_kinds(
    specs: &[OutlineSpec],
    notes: &[&Note],
    shape: OutlineShape,
) -> Vec<(OutlineEntry, SummaryKind)> {
    let mut entries = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for (page, spec) in specs.iter().enumerate() {
        let marks = match notes.get(page) {
            Some(note) if shape.bookmarks => bookmark_outline(note, page, 0),
            _ => Vec::new(),
        };
        // Nothing of this note is shown: no line, and no heading for a sub-binder only it was in.
        if !shape.notes && marks.is_empty() {
            continue;
        }
        let path: &[String] = if shape.binders { &spec.path } else { &[] };
        let common = open
            .iter()
            .zip(path)
            .take_while(|(open, wanted)| **open == wanted.as_str())
            .count();
        open.truncate(common);
        for (level, name) in path.iter().enumerate().skip(common) {
            entries.push((
                OutlineEntry {
                    title: name.clone(),
                    level,
                    page,
                    at: None,
                },
                SummaryKind::Binder,
            ));
            open.push(name);
        }
        let mut level = path.len();
        if shape.notes {
            entries.push((
                OutlineEntry {
                    title: spec.title.clone(),
                    level,
                    page,
                    at: None,
                },
                SummaryKind::Note,
            ));
            level += 1;
        }
        entries.extend(
            marks
                .into_iter()
                .map(|entry| (OutlineEntry { level, ..entry }, SummaryKind::Bookmark)),
        );
    }
    entries
}

/// The summary a binder's PDF starts with: the rows are the outline's entries (`outline_with_kinds`).
fn summary_of(request: &SummaryRequest, entries: &[(OutlineEntry, SummaryKind)]) -> Summary {
    Summary {
        title: request.title.clone(),
        entries: entries
            .iter()
            .map(|(entry, kind)| SummaryEntry {
                title: entry.title.clone(),
                level: entry.level,
                page: entry.page,
                at: entry.at,
                kind: *kind,
            })
            .collect(),
    }
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

/// No note to export or to print.
pub(crate) fn nothing_to_export() -> CommandError {
    CommandError::new(NOTHING_TO_EXPORT, "no note to export")
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
                layers: options.layers.map_or(LayerMode::Separate, LayerMode::from),
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
        layers: options.layers.map_or(LayerMode::Merged, LayerMode::from),
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
    export_with_outline(
        sources,
        format,
        options,
        destination,
        cancel,
        &BinderPdf::NONE,
    )
}

/// [`export`], a PDF carrying what `binder` describes: the outline of a binder's PDF (one spec per source; none, or a
/// number that is not the sources', gives a PDF without outline) with what it holds, and the summary page(s) in front
/// of the notes (only when the outline is the sources'). Images ignore it.
pub fn export_with_outline(
    sources: &[LoadedSource],
    format: ExportFormat,
    options: &ExportOptions,
    destination: &Path,
    cancel: &AtomicBool,
    binder: &BinderPdf<'_>,
) -> CommandResult<Vec<PathBuf>> {
    check_options(format, options)?;
    check_cancelled(cancel)?;
    match sources {
        [] => Err(nothing_to_export()),
        _ if format == ExportFormat::Pdf => {
            let notes: Vec<NoteView<'_>> = sources.iter().map(LoadedSource::view).collect();
            // A binder's PDF: a bookmark per note with its own bookmarks under it. A PDF of one note
            // gets that note's bookmarks as its outline.
            let kinds = if binder.outline.len() == sources.len() {
                let notes: Vec<&Note> = sources.iter().map(|source| &source.note).collect();
                outline_with_kinds(binder.outline, &notes, binder.shape)
            } else if let [single] = sources {
                bookmark_outline(&single.note, 0, 0)
                    .into_iter()
                    .map(|entry| (entry, SummaryKind::Bookmark))
                    .collect()
            } else {
                Vec::new()
            };
            // The summary page(s) go with a binder's PDF only: its rows are the outline's entries.
            let front = binder
                .summary
                .filter(|_| binder.outline.len() == sources.len())
                .map(|request| (summary_of(request, &kinds), request.outline_title.as_str()));
            let entries: Vec<OutlineEntry> = kinds.into_iter().map(|(entry, _)| entry).collect();
            let pdf = notes_to_pdf_with_summary(
                &notes,
                &pdf_options(options),
                &entries,
                front.as_ref().map(|(summary, title)| (summary, *title)),
            )
            .map_err(render_error)?;
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

/// Loads and exports `sources` without an outline (the tests' way; the command goes through
/// [`export_sources_with_outline`]).
#[cfg(test)]
pub fn export_sources(
    library: Option<&Library>,
    sources: Vec<ExportSource>,
    format: ExportFormat,
    options: &ExportOptions,
    destination: &Path,
    cancel: &AtomicBool,
) -> CommandResult<Vec<PathBuf>> {
    export_sources_with_outline(
        library,
        sources,
        format,
        options,
        destination,
        cancel,
        &BinderPdf::NONE,
    )
}

/// [`export_sources`] with the outline and summary of a binder's PDF ([`export_with_outline`]).
pub fn export_sources_with_outline(
    library: Option<&Library>,
    sources: Vec<ExportSource>,
    format: ExportFormat,
    options: &ExportOptions,
    destination: &Path,
    cancel: &AtomicBool,
    binder: &BinderPdf<'_>,
) -> CommandResult<Vec<PathBuf>> {
    // Refuse bad options before reading any note.
    check_options(format, options)?;
    check_cancelled(cancel)?;
    let loaded = load_sources(library, sources)?;
    export_with_outline(&loaded, format, options, destination, cancel, binder)
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
#[allow(clippy::too_many_arguments)]
pub async fn export_notes(
    context: State<'_, AppContext>,
    cancel: State<'_, ExportCancel>,
    sources: Vec<ExportSource>,
    format: ExportFormat,
    options: ExportOptions,
    destination: String,
    outline: Option<Vec<OutlineSpec>>,
    summary: Option<SummaryRequest>,
    shape: Option<OutlineShape>,
) -> CommandResult<Vec<String>> {
    let library = library_for(&context, &sources)?;
    let flag = Arc::clone(&cancel.0);
    flag.store(false, Ordering::Relaxed);
    // Rendering at 600 dpi takes seconds: keep it off the async runtime's threads.
    let written = tauri::async_runtime::spawn_blocking(move || {
        export_sources_with_outline(
            library.as_ref(),
            sources,
            format,
            &options,
            Path::new(&destination),
            &flag,
            &BinderPdf {
                outline: &outline.unwrap_or_default(),
                shape: shape.unwrap_or(OutlineShape::ALL),
                summary: summary.as_ref(),
            },
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
            layers: None,
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

    fn spec(path: &[&str], title: &str) -> OutlineSpec {
        OutlineSpec {
            path: path.iter().map(|s| (*s).to_owned()).collect(),
            title: title.to_owned(),
        }
    }

    fn entry(title: &str, level: usize, page: usize) -> OutlineEntry {
        OutlineEntry {
            title: title.to_owned(),
            level,
            page,
            at: None,
        }
    }

    /// The bookmarks of a binder's PDF: one per note, and one per sub-binder holding the notes below it.
    #[test]
    fn the_outline_has_a_bookmark_per_note_and_a_level_per_sub_binder() {
        assert!(outline_entries(&[]).is_empty());
        // The binder's own notes only: a flat list.
        assert_eq!(
            outline_entries(&[spec(&[], "Un"), spec(&[], "Deux")]),
            [entry("Un", 0, 0), entry("Deux", 0, 1)]
        );
        // Its notes, then each sub-binder's, nested sub-binders included.
        let specs = [
            spec(&[], "Cours"),
            spec(&["3e B"], "TP 1"),
            spec(&["3e B"], "TP 2"),
            spec(&["3e B", "Devoirs"], "DM"),
            spec(&["3e C"], "Contrôle"),
        ];
        assert_eq!(
            outline_entries(&specs),
            [
                entry("Cours", 0, 0),
                entry("3e B", 0, 1),
                entry("TP 1", 1, 1),
                entry("TP 2", 1, 2),
                entry("Devoirs", 1, 3),
                entry("DM", 2, 3),
                entry("3e C", 0, 4),
                entry("Contrôle", 1, 4),
            ]
        );
        // A binder that has no note of its own but notes below it.
        assert_eq!(
            outline_entries(&[spec(&["A", "B"], "x")]),
            [entry("A", 0, 0), entry("B", 1, 0), entry("x", 2, 0)]
        );
        let parsed: OutlineSpec =
            serde_json::from_value(serde_json::json!({ "path": ["3e B"], "title": "TP" })).unwrap();
        assert_eq!(parsed, spec(&["3e B"], "TP"));
    }

    #[test]
    fn a_pdf_of_several_notes_carries_its_outline_only_when_asked_and_whole() {
        let (_dir, library) = library();
        let a = write_sample(&library, "Classe", "Cours");
        let b = write_sample(&library, "Classe/3e B", "TP");
        let out = tempfile::tempdir().unwrap();
        let pdf = |name: &str, outline: &[OutlineSpec]| {
            let destination = out.path().join(name);
            export_sources_with_outline(
                Some(&library),
                vec![from_library(&a.path), from_library(&b.path)],
                ExportFormat::Pdf,
                &options(150, true),
                &destination,
                &NOT_CANCELLED,
                &BinderPdf::outline(outline),
            )
            .unwrap();
            let bytes = fs::read(&destination).unwrap();
            assert_eq!(pdf_pages(&bytes).len(), 2);
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let text = pdf("classeur.pdf", &[spec(&[], "Cours"), spec(&["3e B"], "TP")]);
        for title in ["/Title (Cours)", "/Title (3e B)", "/Title (TP)"] {
            assert!(text.contains(title), "{title}");
        }
        assert!(text.contains("/Outlines") && text.contains("/UseOutlines"));
        // None, or a list that is not one per note: no outline, the PDF of before.
        for outline in [&[][..], &[spec(&[], "Cours")][..]] {
            let text = pdf("sans.pdf", outline);
            assert!(!text.contains("/Outlines"), "{outline:?}");
        }
        let without = out.path().join("export.pdf");
        export_sources(
            Some(&library),
            vec![from_library(&a.path), from_library(&b.path)],
            ExportFormat::Pdf,
            &options(150, true),
            &without,
            &NOT_CANCELLED,
        )
        .unwrap();
        assert_eq!(
            fs::read(&without).unwrap(),
            fs::read(out.path().join("sans.pdf")).unwrap()
        );
        // The images ignore it.
        let folder = out.path().join("images");
        let written = export_sources_with_outline(
            Some(&library),
            vec![from_library(&a.path), from_library(&b.path)],
            ExportFormat::Png,
            &options(72, true),
            &folder,
            &NOT_CANCELLED,
            &BinderPdf::outline(&[spec(&[], "Cours"), spec(&["3e B"], "TP")]),
        )
        .unwrap();
        assert_eq!(written.len(), 2);
    }

    /// A note with bookmarks, given to the export as it is in the editor.
    fn marked(title: &str, labels: &[(&str, f32)]) -> Note {
        let mut note = sample_note(title);
        for (i, (label, y)) in labels.iter().enumerate() {
            note.objects.push(ardoise_core::note::PageObject::bookmark(
                format!("b{i}"),
                0,
                0,
                500.0,
                *y,
                *label,
            ));
        }
        note
    }

    /// The outline entries `(level, title, page, has a spot)` of a PDF, in order.
    fn pdf_outline(bytes: &[u8]) -> Vec<(usize, String, usize, bool)> {
        use lopdf::{Document, Object};
        fn text(object: &Object) -> String {
            let Object::String(bytes, _) = object else {
                panic!("a title")
            };
            match bytes.as_slice() {
                [0xfe, 0xff, rest @ ..] => String::from_utf16_lossy(
                    &rest
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>(),
                ),
                plain => String::from_utf8_lossy(plain).into_owned(),
            }
        }
        let document = Document::load_mem(bytes).unwrap();
        let pages: Vec<_> = document.get_pages().values().copied().collect();
        let Ok(root) = document.catalog().unwrap().get(b"Outlines") else {
            return Vec::new();
        };
        let root = document
            .get_dictionary(root.as_reference().unwrap())
            .unwrap();
        let mut out = Vec::new();
        fn walk(
            document: &Document,
            first: lopdf::ObjectId,
            depth: usize,
            pages: &[lopdf::ObjectId],
            out: &mut Vec<(usize, String, usize, bool)>,
        ) {
            let mut current = Some(first);
            while let Some(id) = current {
                let item = document.get_dictionary(id).unwrap();
                let Object::Array(dest) = item.get(b"Dest").unwrap() else {
                    panic!("a destination")
                };
                let page = pages
                    .iter()
                    .position(|p| *p == dest[0].as_reference().unwrap())
                    .unwrap();
                out.push((
                    depth,
                    text(item.get(b"Title").unwrap()),
                    page,
                    dest[1].as_name().unwrap() == b"XYZ",
                ));
                if let Ok(child) = item.get(b"First") {
                    walk(
                        document,
                        child.as_reference().unwrap(),
                        depth + 1,
                        pages,
                        out,
                    );
                }
                current = item.get(b"Next").ok().map(|n| n.as_reference().unwrap());
            }
        }
        walk(
            &document,
            root.get(b"First").unwrap().as_reference().unwrap(),
            0,
            &pages,
            &mut out,
        );
        out
    }

    /// Bookmarks (SPEC §4.4, §4.6): under each note's bookmark in a binder's PDF, the outline of a PDF of
    /// one note, and nothing for a PDF of several notes that is not a binder's.
    #[test]
    fn the_bookmarks_of_a_note_go_into_the_outline_of_its_pdf() {
        let specs = [spec(&[], "Cours"), spec(&["3e B"], "TP")];
        let first = marked("Cours", &[("Seconde", 3000.0), ("Première", 1000.0)]);
        let second = marked("TP", &[("Bilan", 2000.0)]);
        let notes = [&first, &second];
        assert_eq!(
            outline_with_bookmarks(&specs, &notes)
                .iter()
                .map(|e| (e.level, e.title.as_str(), e.page, e.at.is_some()))
                .collect::<Vec<_>>(),
            [
                (0, "Cours", 0, false),
                (1, "Première", 0, true),
                (1, "Seconde", 0, true),
                (0, "3e B", 1, false),
                (1, "TP", 1, false),
                (2, "Bilan", 1, true),
            ]
        );
        // Without notes, the outline of before.
        assert_eq!(outline_with_bookmarks(&specs, &[]), outline_entries(&specs));

        let out = tempfile::tempdir().unwrap();
        let export = |sources: Vec<ExportSource>, outline: &[OutlineSpec]| {
            let destination = out.path().join("sortie.pdf");
            export_sources_with_outline(
                None,
                sources,
                ExportFormat::Pdf,
                &options(150, true),
                &destination,
                &NOT_CANCELLED,
                &BinderPdf::outline(outline),
            )
            .unwrap();
            fs::read(&destination).unwrap()
        };
        let binder = export(
            vec![in_memory(first.clone()), in_memory(second.clone())],
            &specs,
        );
        assert_eq!(
            pdf_outline(&binder),
            [
                (0, "Cours".to_owned(), 0, false),
                (1, "Première".to_owned(), 0, true),
                (1, "Seconde".to_owned(), 0, true),
                (0, "3e B".to_owned(), 1, false),
                (1, "TP".to_owned(), 1, false),
                (2, "Bilan".to_owned(), 1, true),
            ]
        );
        // One note: its bookmarks are the outline.
        let single = export(vec![in_memory(first.clone())], &[]);
        assert_eq!(
            pdf_outline(&single),
            [
                (0, "Première".to_owned(), 0, true),
                (0, "Seconde".to_owned(), 0, true),
            ]
        );
        // Several notes without a binder's outline: none, as before.
        let several = export(vec![in_memory(first), in_memory(second)], &[]);
        assert!(pdf_outline(&several).is_empty());
        // A note without a bookmark: the PDF of before, with no outline.
        let plain = export(vec![in_memory(sample_note("Rien"))], &[]);
        assert!(pdf_outline(&plain).is_empty());
    }

    /// « Ajouter une page de sommaire au début »: the summary pages are in front of a binder's PDF, their rows are the
    /// outline's entries (sub-binder, note, bookmark) with the page numbers counted after them, the names are the
    /// ones given (with or without the dates, which the interface decides), and a single note or images get none.
    #[test]
    fn a_binders_pdf_can_start_with_a_summary_page() {
        let specs = [spec(&[], "Cours"), spec(&["3e B"], "Note sans titre")];
        let request = SummaryRequest {
            title: "Classe".into(),
            outline_title: "Sommaire".into(),
        };
        let out = tempfile::tempdir().unwrap();
        let export = |summary: Option<&SummaryRequest>, specs: &[OutlineSpec]| {
            let destination = out.path().join("sortie.pdf");
            let sources = vec![
                in_memory(marked("Cours", &[("Introduction", 1500.0)])),
                in_memory(marked("TP", &[("Bilan", 800.0)])),
            ];
            export_sources_with_outline(
                None,
                sources,
                ExportFormat::Pdf,
                &options(150, true),
                &destination,
                &NOT_CANCELLED,
                &BinderPdf {
                    outline: specs,
                    shape: OutlineShape::ALL,
                    summary,
                },
            )
            .unwrap();
            fs::read(&destination).unwrap()
        };
        let with = export(Some(&request), &specs);
        let document = lopdf::Document::load_mem(&with).unwrap();
        // One summary page (the first note's own size, the page choice being « actual ») then the two notes.
        assert_eq!(document.get_pages().len(), 3);
        assert_eq!(
            pdf_outline(&with),
            [
                (0, "Sommaire".to_owned(), 0, false),
                (0, "Cours".to_owned(), 1, false),
                (1, "Introduction".to_owned(), 1, true),
                (0, "3e B".to_owned(), 2, false),
                (1, "Note sans titre".to_owned(), 2, false),
                (2, "Bilan".to_owned(), 2, true),
            ]
        );
        // The rows: the binder, the notes, their bookmarks, as the outline has them; the page numbers count the
        // summary page, so the first note is on page 2.
        let text = document.extract_text(&[1]).unwrap();
        for word in [
            "Classe",
            "Cours",
            "Introduction",
            "3e B",
            "Note sans titre",
            "Bilan",
        ] {
            assert!(text.contains(word), "{word} in {text:?}");
        }
        // No summary asked, or no binder's outline: the PDF of before.
        let without = export(None, &specs);
        assert_eq!(
            lopdf::Document::load_mem(&without)
                .unwrap()
                .get_pages()
                .len(),
            2
        );
        let no_outline = export(Some(&request), &[]);
        assert_eq!(
            lopdf::Document::load_mem(&no_outline)
                .unwrap()
                .get_pages()
                .len(),
            2
        );
    }

    /// The notes of the « Dans le sommaire » cases: a note with two bookmarks, one with one in a sub-binder, a note of a
    /// sub-sub-binder and one of another sub-binder with nothing to show, and a last one with a bookmark.
    fn sections() -> (Vec<OutlineSpec>, Vec<Note>) {
        (
            vec![
                spec(&[], "Cours"),
                spec(&["3e B"], "TP"),
                spec(&["3e B", "Devoirs"], "Rien"),
                spec(&["3e C"], "Vide"),
                spec(&["3e C"], "Dernier"),
            ],
            vec![
                marked("Cours", &[("Introduction", 1500.0), ("Fin", 3000.0)]),
                marked("TP", &[("Bilan", 800.0)]),
                sample_note("Rien"),
                sample_note("Vide"),
                marked("Dernier", &[("Zéro", 900.0)]),
            ],
        )
    }

    /// `(level, title, page, kind)` of the entries the shape leaves.
    fn shaped(shape: OutlineShape) -> Vec<(usize, String, usize, SummaryKind)> {
        let (specs, notes) = sections();
        let refs: Vec<&Note> = notes.iter().collect();
        outline_with_kinds(&specs, &refs, shape)
            .into_iter()
            .map(|(entry, kind)| (entry.level, entry.title, entry.page, kind))
            .collect()
    }

    fn row(
        level: usize,
        title: &str,
        page: usize,
        kind: SummaryKind,
    ) -> (usize, String, usize, SummaryKind) {
        (level, title.to_owned(), page, kind)
    }

    /// « Dans le sommaire » (SPEC §4.6): sub-binders, notes and bookmarks are three choices, and what is left keeps the
    /// binder's order, the page and the spot of each bookmark, and never an empty level or an empty heading.
    #[test]
    fn the_outline_follows_what_the_summary_holds() {
        use SummaryKind::{Binder, Bookmark, Note as NoteLine};
        // All three: the outline of before.
        assert_eq!(
            shaped(OutlineShape::ALL),
            [
                row(0, "Cours", 0, NoteLine),
                row(1, "Introduction", 0, Bookmark),
                row(1, "Fin", 0, Bookmark),
                row(0, "3e B", 1, Binder),
                row(1, "TP", 1, NoteLine),
                row(2, "Bilan", 1, Bookmark),
                row(1, "Devoirs", 2, Binder),
                row(2, "Rien", 2, NoteLine),
                row(0, "3e C", 3, Binder),
                row(1, "Vide", 3, NoteLine),
                row(1, "Dernier", 4, NoteLine),
                row(2, "Zéro", 4, Bookmark),
            ]
        );
        // The main case: the bookmarks and the names of the sub-binders. The bookmarks come right under their heading (or at
        // the top for the binder's own note), a note without a bookmark gives no line, and « Devoirs » and the first note of
        // « 3e C » leave no heading nor line.
        let only = OutlineShape {
            binders: true,
            notes: false,
            bookmarks: true,
        };
        assert_eq!(
            shaped(only),
            [
                row(0, "Introduction", 0, Bookmark),
                row(0, "Fin", 0, Bookmark),
                row(0, "3e B", 1, Binder),
                row(1, "Bilan", 1, Bookmark),
                row(0, "3e C", 4, Binder),
                row(1, "Zéro", 4, Bookmark),
            ]
        );
        // Without the sub-binders either: a flat list of the bookmarks.
        assert_eq!(
            shaped(OutlineShape {
                binders: false,
                ..only
            }),
            [
                row(0, "Introduction", 0, Bookmark),
                row(0, "Fin", 0, Bookmark),
                row(0, "Bilan", 1, Bookmark),
                row(0, "Zéro", 4, Bookmark),
            ]
        );
        // The notes and the sub-binders, no bookmark.
        assert_eq!(
            shaped(OutlineShape {
                binders: true,
                notes: true,
                bookmarks: false,
            }),
            [
                row(0, "Cours", 0, NoteLine),
                row(0, "3e B", 1, Binder),
                row(1, "TP", 1, NoteLine),
                row(1, "Devoirs", 2, Binder),
                row(2, "Rien", 2, NoteLine),
                row(0, "3e C", 3, Binder),
                row(1, "Vide", 3, NoteLine),
                row(1, "Dernier", 4, NoteLine),
            ]
        );
        // The notes and their bookmarks, no sub-binder: no level for them.
        assert_eq!(
            shaped(OutlineShape {
                binders: false,
                notes: true,
                bookmarks: true,
            }),
            [
                row(0, "Cours", 0, NoteLine),
                row(1, "Introduction", 0, Bookmark),
                row(1, "Fin", 0, Bookmark),
                row(0, "TP", 1, NoteLine),
                row(1, "Bilan", 1, Bookmark),
                row(0, "Rien", 2, NoteLine),
                row(0, "Vide", 3, NoteLine),
                row(0, "Dernier", 4, NoteLine),
                row(1, "Zéro", 4, Bookmark),
            ]
        );
        // Nothing asked, or only the headings (which need something under them): nothing.
        for shape in [
            OutlineShape {
                binders: false,
                notes: false,
                bookmarks: false,
            },
            OutlineShape {
                binders: true,
                notes: false,
                bookmarks: false,
            },
        ] {
            assert!(shaped(shape).is_empty(), "{shape:?}");
        }
        // The bookmarks keep their spot.
        let (specs, notes) = sections();
        let refs: Vec<&Note> = notes.iter().collect();
        let spots: Vec<bool> = outline_with_kinds(&specs, &refs, only)
            .iter()
            .filter(|(_, kind)| *kind == Bookmark)
            .map(|(entry, _)| entry.at.is_some())
            .collect();
        assert_eq!(spots, [true; 4]);
    }

    /// The links of the first page of a PDF, as the page (0-based) each goes to and whether it has a spot.
    fn first_page_links(document: &lopdf::Document) -> Vec<(usize, bool)> {
        let ids: Vec<lopdf::ObjectId> = document.get_pages().values().copied().collect();
        let page = document.get_dictionary(ids[0]).unwrap();
        let Ok(annots) = page.get(b"Annots") else {
            return Vec::new();
        };
        annots
            .as_array()
            .unwrap()
            .iter()
            .map(|annot| {
                let annot = document
                    .get_dictionary(annot.as_reference().unwrap())
                    .unwrap();
                let action = annot.get(b"A").unwrap().as_dict().unwrap();
                let dest = action.get(b"D").unwrap().as_array().unwrap();
                let target = ids
                    .iter()
                    .position(|id| *id == dest[0].as_reference().unwrap())
                    .unwrap();
                (target, dest[1].as_name().unwrap() == b"XYZ")
            })
            .collect()
    }

    /// The summary page and the outline of a binder's PDF made with a shape: the rows are the bookmarks under their
    /// sub-binders when the notes are left out, their links still go to the page and the spot of each bookmark, and an
    /// empty choice gives no summary page and no outline.
    #[test]
    fn the_summary_page_and_the_outline_follow_the_shape() {
        let (specs, notes) = sections();
        let request = SummaryRequest {
            title: "Classe".into(),
            outline_title: "Sommaire".into(),
        };
        let out = tempfile::tempdir().unwrap();
        let export = |shape: OutlineShape, summary: Option<&SummaryRequest>| {
            let destination = out.path().join("sortie.pdf");
            let sources: Vec<ExportSource> = notes.iter().cloned().map(in_memory).collect();
            export_sources_with_outline(
                None,
                sources,
                ExportFormat::Pdf,
                &options(150, true),
                &destination,
                &NOT_CANCELLED,
                &BinderPdf {
                    outline: &specs,
                    shape,
                    summary,
                },
            )
            .unwrap();
            fs::read(&destination).unwrap()
        };
        let only = OutlineShape {
            binders: true,
            notes: false,
            bookmarks: true,
        };
        let bytes = export(only, Some(&request));
        let document = lopdf::Document::load_mem(&bytes).unwrap();
        // One summary page, then the five notes: the numbers of the pages follow.
        assert_eq!(document.get_pages().len(), 6);
        assert_eq!(
            pdf_outline(&bytes),
            [
                (0, "Sommaire".to_owned(), 0, false),
                (0, "Introduction".to_owned(), 1, true),
                (0, "Fin".to_owned(), 1, true),
                (0, "3e B".to_owned(), 2, false),
                (1, "Bilan".to_owned(), 2, true),
                (0, "3e C".to_owned(), 5, false),
                (1, "Zéro".to_owned(), 5, true),
            ]
        );
        // The rows of the summary page, in the same order: each goes to the page of its note (counted after the summary),
        // the bookmarks to their spot.
        assert_eq!(
            first_page_links(&document),
            [
                (1, true),
                (1, true),
                (2, false),
                (2, true),
                (5, false),
                (5, true)
            ]
        );
        let text = document.extract_text(&[1]).unwrap();
        for word in ["Classe", "Introduction", "3e B", "Bilan", "Zéro"] {
            assert!(text.contains(word), "{word} in {text:?}");
        }
        // The notes' names are not in it: they were left out.
        for word in ["Cours", "Dernier", "Devoirs"] {
            assert!(!text.contains(word), "{word} in {text:?}");
        }
        // Nothing left: no summary page, no outline, the PDF of notes.
        let nothing = OutlineShape {
            binders: true,
            notes: false,
            bookmarks: false,
        };
        let bytes = export(nothing, Some(&request));
        assert_eq!(
            lopdf::Document::load_mem(&bytes).unwrap().get_pages().len(),
            5
        );
        assert!(pdf_outline(&bytes).is_empty());
        assert!(!String::from_utf8_lossy(&bytes).contains("/Outlines"));
    }

    /// The sample note with a second layer holding a stroke of its own, and a hidden one.
    fn layered(title: &str, second: &str) -> Note {
        let mut note = sample_note(title);
        for (name, visible) in [(second, true), ("Reponses", false)] {
            let mut stroke = note.strokes[0].clone();
            stroke.layer = note.layers.len() as u32;
            stroke.points.iter_mut().for_each(|p| p.y += 500.0);
            note.layers.push(ardoise_core::note::Layer {
                name: name.into(),
                visible,
            });
            note.strokes.push(stroke);
        }
        note
    }

    fn in_memory(note: Note) -> ExportSource {
        ExportSource {
            path: String::new(),
            note: Some(note),
            image_key: None,
        }
    }

    /// The names of the PDF's optional content groups; empty without `/OCProperties`.
    fn pdf_groups(bytes: &[u8]) -> Vec<String> {
        let document = lopdf::Document::load_mem(bytes).unwrap();
        let Ok(properties) = document.catalog().unwrap().get(b"OCProperties") else {
            return Vec::new();
        };
        let properties = document
            .dereference(properties)
            .unwrap()
            .1
            .as_dict()
            .unwrap();
        properties
            .get(b"OCGs")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|group| {
                let group = document.dereference(group).unwrap().1.as_dict().unwrap();
                String::from_utf8(group.get(b"Name").unwrap().as_str().unwrap().to_vec()).unwrap()
            })
            .collect()
    }

    #[test]
    fn the_layers_option_is_optional_and_reads_the_interface_words() {
        let read = |layers: Option<&str>| -> Result<ExportOptions, serde_json::Error> {
            let mut value = serde_json::json!({
                "dpi": 150, "background": true, "quality": 90, "pdfPage": "actual"
            });
            if let Some(layers) = layers {
                value["layers"] = layers.into();
            }
            serde_json::from_value(value)
        };
        assert_eq!(read(None).unwrap().layers, None);
        assert_eq!(
            read(Some("separate")).unwrap().layers,
            Some(ExportLayers::Separate)
        );
        assert_eq!(
            read(Some("merged")).unwrap().layers,
            Some(ExportLayers::Merged)
        );
        assert!(read(Some("both")).is_err());
    }

    #[test]
    fn an_svg_has_separate_layers_by_default_and_one_ink_layer_when_merged() {
        let note = layered("Essai", "Correction");
        let layers = |choice: Option<ExportLayers>| {
            let options = ExportOptions {
                layers: choice,
                ..options(150, true)
            };
            let bytes = image_bytes(&note, ExportFormat::Svg, &options).unwrap();
            String::from_utf8(bytes).unwrap()
        };
        // The background layer, then one per visible layer of the note.
        for svg in [layers(None), layers(Some(ExportLayers::Separate))] {
            assert_eq!(svg.matches("inkscape:groupmode=\"layer\"").count(), 3);
            assert!(svg.contains("inkscape:label=\"Correction\""));
        }
        let merged = layers(Some(ExportLayers::Merged));
        assert_eq!(merged.matches("inkscape:groupmode=\"layer\"").count(), 2);
        assert!(merged.contains("<g id=\"ink\""));
        assert_eq!(merged.matches("<path ").count(), 2);
        // A hidden layer, in every mode.
        assert!(!merged.contains("Reponses") && !layers(None).contains("Reponses"));
        // An image has no layers: the choice changes nothing.
        let png = |choice| {
            let options = ExportOptions {
                layers: choice,
                ..options(72, true)
            };
            image_bytes(&note, ExportFormat::Png, &options).unwrap()
        };
        assert_eq!(png(None), png(Some(ExportLayers::Separate)));
    }

    #[test]
    fn a_pdf_has_merged_layers_by_default_and_one_group_per_layer_name_when_separate() {
        let out = tempfile::tempdir().unwrap();
        let pdf = |choice: Option<ExportLayers>| {
            let destination = out.path().join(format!("{choice:?}.pdf"));
            let options = ExportOptions {
                layers: choice,
                ..options(150, true)
            };
            export_sources(
                None,
                vec![
                    in_memory(layered("Un", "Correction")),
                    in_memory(layered("Deux", "Correction")),
                ],
                ExportFormat::Pdf,
                &options,
                &destination,
                &NOT_CANCELLED,
            )
            .unwrap();
            fs::read(&destination).unwrap()
        };
        assert!(pdf_groups(&pdf(None)).is_empty());
        assert!(pdf_groups(&pdf(Some(ExportLayers::Merged))).is_empty());
        // « Calque 1 » and « Correction » on each page, shared: two groups, not four.
        let separate = pdf(Some(ExportLayers::Separate));
        assert_eq!(pdf_groups(&separate), ["Correction", "Calque 1"]);
        assert!(!separate.windows(8).any(|w| w == b"Reponses"));
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
            NOTHING_TO_EXPORT
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

    /// Exporting no note at all is its own error (`nothingToExport`, a sentence of its own in the interface), never
    /// the « could not be written, check the space » of `exportFailed`: the report that started this was a binder
    /// whose notes are all in a sub-binder, so that the dialog had nothing to export.
    #[test]
    fn an_export_of_no_note_is_nothing_to_export_not_a_write_failure() {
        let (_dir, library) = library();
        let out = tempfile::tempdir().unwrap();
        for format in [ExportFormat::Pdf, ExportFormat::Png] {
            let destination = out.path().join("nothing");
            let error = export_sources(
                Some(&library),
                Vec::new(),
                format,
                &options(72, true),
                &destination,
                &NOT_CANCELLED,
            )
            .unwrap_err();
            assert_eq!(error.code(), "nothingToExport", "{format:?}");
            assert!(!destination.exists());
        }
    }

    /// A binder with a space in its name and notes only in its sub-binder, on a real library: the listing gives those
    /// notes the sub-binder's path (`/`-separated, as the sidebar's tree names it), the binder has none of its own,
    /// and the PDF of the notes it does list is written.
    #[test]
    fn a_binder_of_sub_binder_notes_only_lists_none_of_its_own() {
        let (_dir, library) = library();
        write_sample(&library, "oral du dnb/2026", "Candidat 1");
        write_sample(&library, "oral du dnb/2026", "Candidat 2");
        write_sample(&library, "Autre", "Hors classeur");
        let listing = crate::library::list(&library);
        assert_eq!(
            listing.binders,
            ["Autre", "oral du dnb", "oral du dnb/2026"]
        );
        let of = |binder: &str| -> Vec<&str> {
            listing
                .notes
                .iter()
                .filter(|note| note.binder == binder)
                .map(|note| note.path.as_str())
                .collect()
        };
        assert!(of("oral du dnb").is_empty(), "no note of its own");
        let below = of("oral du dnb/2026");
        assert_eq!(below.len(), 2);

        let out = tempfile::tempdir().unwrap();
        let destination = out.path().join("oral du dnb.pdf");
        let sources = below
            .iter()
            .map(|path| ExportSource {
                path: (*path).to_owned(),
                note: None,
                image_key: None,
            })
            .collect();
        let outline = [
            OutlineSpec {
                path: vec!["2026".to_owned()],
                title: "Candidat 1".to_owned(),
            },
            OutlineSpec {
                path: vec!["2026".to_owned()],
                title: "Candidat 2".to_owned(),
            },
        ];
        export_sources_with_outline(
            Some(&library),
            sources,
            ExportFormat::Pdf,
            &options(72, true),
            &destination,
            &NOT_CANCELLED,
            &BinderPdf::outline(&outline),
        )
        .unwrap();
        assert_eq!(pdf_pages(&fs::read(&destination).unwrap()).len(), 2);
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
