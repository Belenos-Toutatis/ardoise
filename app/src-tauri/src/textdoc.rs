//! The text export with the structure drawn from the ink (milestone M8): a note's handwriting as
//! Markdown (`.md`, the drawings as PNG files in a folder next to it) or as a Word document (`.docx`, the
//! drawings embedded). Contract: `docs/design/extras.md`, "M8: text export with structure".
//!
//! The words are those of the `.txt` export and the clipboard: the recognition of the whole note,
//! or the transcription the user corrected ([`NoteReading`]). The structure (paragraphs, list
//! items, drawings, underlined words) comes from Windows' ink analysis, or from layout
//! heuristics on the recognised lines when it is not available (`ardoise_hwr::structure`,
//! platform-free). The files are built by `ardoise_hwr::markdown` and `ardoise_hwr::docx`; this
//! module reads the notes, crops the drawings out of them and writes the files atomically.

use std::collections::VecDeque;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ardoise_core::fsutil::{write_atomic, write_atomic_new};
use ardoise_core::note::Note;
use ardoise_hwr::Recognizer;
use ardoise_hwr::document::{Block, Document, Drawing, Labels};
use ardoise_hwr::docx::to_docx;
use ardoise_hwr::markdown::to_markdown;
use ardoise_hwr::structure::{DrawingArea, Item, Structure, structure_of};
use ardoise_render::{
    Image, NoteView, RenderOptions, encode_png_at, paper_color, render_note, scale_for_dpi,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::AppContext;
use crate::commands::CommandResult;
use crate::export::{
    ExportSource, LoadedSource, export_error, library_for, load_sources, render_error,
};
use crate::handwriting::{Indexer, NoteReading, blocking, hwr_error, reading_of};

/// Resolution of the drawings' images, in dots per inch.
const DRAWING_DPI: u32 = 200;
/// Margin kept around a drawing, in page units (2 mm).
const DRAWING_MARGIN: f32 = 200.0;
/// Millimetres in an inch.
const MM_PER_INCH: f32 = 25.4;

/// `StructuredFormat` in `handwriting.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StructuredFormat {
    Md,
    Docx,
}

/// `TextLabels` in `handwriting.ts`: the words of the files, in the interface language.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLabels {
    pub date: String,
    pub tags: String,
    pub drawing: String,
}

impl From<TextLabels> for Labels {
    fn from(labels: TextLabels) -> Self {
        Labels {
            date: labels.date,
            tags: labels.tags,
            drawing: labels.drawing,
        }
    }
}

/// `StructuredExport` in `handwriting.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredExport {
    /// The files written: the document, then the images next to a Markdown file.
    pub paths: Vec<String>,
    /// False when the layout was guessed from the text because Windows' ink analysis was not
    /// available for at least one note.
    pub analyzed: bool,
}

/// The creation date as the files show it: local time, `2026-10-22 14:05`.
fn format_date(created_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(created_ms)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The rows `from..to` of `image`, columns `left..right`, as an image of its own.
fn crop(image: &Image, left: u32, top: u32, right: u32, bottom: u32) -> Image {
    let (right, bottom) = (right.min(image.width), bottom.min(image.height));
    let (left, top) = (
        left.min(right.saturating_sub(1)),
        top.min(bottom.saturating_sub(1)),
    );
    let width = right - left;
    let mut pixels = Vec::with_capacity(width as usize * (bottom - top) as usize * 4);
    for y in top..bottom {
        let start = (y as usize * image.width as usize + left as usize) * 4;
        pixels.extend_from_slice(&image.pixels[start..start + width as usize * 4]);
    }
    Image {
        width,
        height: bottom - top,
        pixels,
    }
}

/// `source` reduced to the strokes `strokes` (indices into its strokes, sorted): what a drawing's
/// image is cropped from. The page's objects (text frames) are taken off: the text next to a
/// drawing does not creep into it, typed text no more than handwriting.
fn drawings_only(source: &Note, strokes: &[usize]) -> Note {
    let mut note: Note = source.clone();
    note.strokes = strokes
        .iter()
        .filter_map(|&index| source.strokes.get(index).cloned())
        .collect();
    note.objects.clear();
    note
}

/// The images of `areas`, cropped from one rendering of the note that holds only their strokes
/// (the text next to a drawing does not creep into it). Ink on white, except on a dark paper,
/// where the paper stays so that light ink remains visible.
fn render_drawings(source: &LoadedSource, areas: &[&DrawingArea]) -> CommandResult<Vec<Drawing>> {
    if areas.is_empty() {
        return Ok(Vec::new());
    }
    let mut strokes: Vec<usize> = areas
        .iter()
        .flat_map(|a| a.strokes.iter().copied())
        .collect();
    strokes.sort_unstable();
    strokes.dedup();
    let note = drawings_only(&source.note, &strokes);
    let paper = paper_color(note.background.paper);
    let luminance =
        0.299 * f32::from(paper.r) + 0.587 * f32::from(paper.g) + 0.114 * f32::from(paper.b);
    let options = RenderOptions::at_dpi(DRAWING_DPI, luminance < 110.0);
    let image = render_note(NoteView::new(&note, source.picture.as_deref()), &options)
        .map_err(render_error)?;
    let scale = scale_for_dpi(DRAWING_DPI);
    areas
        .iter()
        .map(|area| {
            let b = &area.bounds;
            let pixel = |units: f32| (units * scale).max(0.0);
            let cropped = crop(
                &image,
                pixel(b.x - DRAWING_MARGIN).floor() as u32,
                pixel(b.y - DRAWING_MARGIN).floor() as u32,
                pixel(b.right() + DRAWING_MARGIN).ceil() as u32,
                pixel(b.bottom() + DRAWING_MARGIN).ceil() as u32,
            );
            let png = encode_png_at(&cropped, Some(DRAWING_DPI)).map_err(render_error)?;
            Ok(Drawing {
                png,
                width_px: cropped.width,
                height_px: cropped.height,
                width_mm: cropped.width as f32 / DRAWING_DPI as f32 * MM_PER_INCH,
                height_mm: cropped.height as f32 / DRAWING_DPI as f32 * MM_PER_INCH,
            })
        })
        .collect()
}

/// How many structures [`Structures`] keeps.
const KEPT_STRUCTURES: usize = 8;

/// The structures found lately: the export window first asks whether the notes have drawings
/// (its "Inclure les dessins" is on only when they do), then the export uses them. Windows' ink
/// analysis of a page takes about two seconds; it is done once.
#[derive(Default)]
pub struct Structures(Mutex<VecDeque<(u64, Arc<Structure>)>>);

impl Structures {
    fn get(&self, key: u64) -> Option<Arc<Structure>> {
        let kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, structure)| Arc::clone(structure))
    }

    fn keep(&self, key: u64, structure: Arc<Structure>) {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|(k, _)| *k != key);
        kept.push_back((key, structure));
        while kept.len() > KEPT_STRUCTURES {
            kept.pop_front();
        }
    }
}

/// What the structure of `note` read as `reading` depends on: its strokes (points, tools,
/// layers and their visibility), the recognition (language, engine, words and their places) and
/// the correction.
fn structure_key(note: &Note, reading: &NoteReading) -> u64 {
    let mut hasher = DefaultHasher::new();
    note.id.hash(&mut hasher);
    note.strokes.len().hash(&mut hasher);
    for stroke in &note.strokes {
        stroke.tool.hash(&mut hasher);
        stroke.layer.hash(&mut hasher);
        stroke.points.len().hash(&mut hasher);
        for point in &stroke.points {
            point.x.to_bits().hash(&mut hasher);
            point.y.to_bits().hash(&mut hasher);
            point.pressure.to_bits().hash(&mut hasher);
        }
    }
    for layer in &note.layers {
        layer.visible.hash(&mut hasher);
    }
    reading.recognition.language.hash(&mut hasher);
    reading.recognition.recognizer.hash(&mut hasher);
    for word in reading.recognition.words() {
        word.text.hash(&mut hasher);
        for side in [
            word.bounds.x,
            word.bounds.y,
            word.bounds.width,
            word.bounds.height,
        ] {
            side.to_bits().hash(&mut hasher);
        }
    }
    reading.correction.hash(&mut hasher);
    hasher.finish()
}

/// The structure of `note`, read as `reading`, by `recognizer`'s ink analysis (or the layout
/// heuristics); found again in `known` when it holds it, and kept there.
pub fn structure_for(
    note: &Note,
    reading: &NoteReading,
    recognizer: &dyn Recognizer,
    known: Option<&Structures>,
) -> Arc<Structure> {
    let key = structure_key(note, reading);
    if let Some(found) = known.and_then(|known| known.get(key)) {
        return found;
    }
    let structure = Arc::new(structure_of(
        recognizer,
        &reading.recognition,
        reading.correction.as_deref(),
        note,
    ));
    if let Some(known) = known {
        known.keep(key, Arc::clone(&structure));
    }
    structure
}

/// The document of one note: its title (or file name), date and tags, then `structure`, without
/// its drawings unless `drawings`.
pub fn document_of(
    source: &LoadedSource,
    structure: &Structure,
    drawings: bool,
) -> CommandResult<Document> {
    let areas: Vec<&DrawingArea> = structure
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Drawing(area) if drawings => Some(area),
            _ => None,
        })
        .collect();
    let mut images = render_drawings(source, &areas)?.into_iter();
    let blocks = structure
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Paragraph(text) => Some(Block::Paragraph(text.clone())),
            Item::ListItem(text) => Some(Block::ListItem(text.clone())),
            Item::Drawing(_) if drawings => images.next().map(Block::Drawing),
            Item::Drawing(_) => None,
        })
        .collect();
    let title = source.note.title.trim();
    let document = Document {
        title: if title.is_empty() {
            source.stem.clone()
        } else {
            title.to_owned()
        },
        date: format_date(source.note.created),
        tags: source.note.tags.clone(),
        blocks,
    };
    Ok(document)
}

/// Suffix of the folder that holds a Markdown file's drawings: `<name>_images`.
const IMAGES_SUFFIX: &str = "_images";
/// Highest number tried in a folder name (` (2)`…).
const MAX_FOLDER_SUFFIX: u32 = 999;

/// Whether `name` is a drawing file of a previous export of `stem` (`<stem>-<number>.png`).
fn is_export_image(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.strip_suffix(".png"))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The files of `folder` when it can be used for the drawings of `stem`'s export: it does not
/// exist (nothing to list) or holds nothing but the drawings of a previous export of the same
/// document. `None` when it holds anything else (the user's own files, a sub-folder), or is not
/// a folder.
fn previous_export_files(folder: &Path, stem: &str) -> Option<Vec<PathBuf>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.ok()?;
        let name = entry.file_name();
        if !entry.file_type().ok()?.is_file() || !is_export_image(&name.to_string_lossy(), stem) {
            return None;
        }
        files.push(entry.path());
    }
    Some(files)
}

/// The name of a folder next to `destination` as `<stem>_images`, or `<stem>_images (2)`… if a
/// folder of that name holds anything that is not ours: the drawings never land among the
/// user's files. Returns the name and the files of a previous export it already holds.
fn images_folder(parent: &Path, stem: &str) -> CommandResult<(String, Vec<PathBuf>)> {
    for n in 1..=MAX_FOLDER_SUFFIX {
        let name = if n == 1 {
            format!("{stem}{IMAGES_SUFFIX}")
        } else {
            format!("{stem}{IMAGES_SUFFIX} ({n})")
        };
        if let Some(files) = previous_export_files(&parent.join(&name), stem) {
            return Ok((name, files));
        }
    }
    Err(export_error(format!(
        "no free folder name for {stem}{IMAGES_SUFFIX}"
    )))
}

/// Writes the Markdown of `documents` to `destination` and the drawings in the folder
/// `<name>_images` next to it (`<name>-1.png`…), atomically; returns the files, the document
/// first. No file the user did not confirm is replaced: the save dialog confirmed the `.md`, and
/// the folder is a fresh one, or the one a previous export of the same document made (it holds
/// only its drawings, which are replaced; those of a longer previous version are removed once
/// the text is written). A folder holding anything else is left alone: ` (2)` is added to the
/// name of ours.
pub fn write_markdown(
    destination: &Path,
    documents: &[Document],
    labels: &Labels,
) -> CommandResult<Vec<PathBuf>> {
    let stem = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = destination.parent().unwrap_or(Path::new("."));
    let fail =
        |path: &Path, error: std::io::Error| export_error(format!("{}: {error}", path.display()));
    let drawings = documents
        .iter()
        .flat_map(|document| &document.blocks)
        .any(|block| matches!(block, Block::Drawing(_)));
    let (folder_name, stale) = if drawings {
        images_folder(parent, &stem)?
    } else {
        // No drawing now: the folder of a previous export, if any, is emptied below.
        let name = format!("{stem}{IMAGES_SUFFIX}");
        let files = previous_export_files(&parent.join(&name), &stem).unwrap_or_default();
        (name, files)
    };
    let folder = parent.join(&folder_name);
    let markdown = to_markdown(
        documents,
        labels,
        &stem,
        if drawings { &folder_name } else { "" },
    );
    let mut written = Vec::new();
    // The images first: a text file never points at an image that is not there.
    if !markdown.images.is_empty() {
        fs::create_dir_all(&folder).map_err(|e| fail(&folder, e))?;
    }
    for image in &markdown.images {
        let path = folder.join(&image.name);
        write_atomic(&path, &image.png).map_err(|e| fail(&path, e))?;
        written.push(path);
    }
    write_atomic(destination, markdown.text.as_bytes()).map_err(|e| fail(destination, e))?;
    // Drawings of a previous, longer export are not linked any more (best effort).
    for old in stale.iter().filter(|old| !written.contains(old)) {
        let _ = fs::remove_file(old);
    }
    if markdown.images.is_empty() {
        let _ = fs::remove_dir(&folder);
    }
    written.insert(0, destination.to_path_buf());
    Ok(written)
}

/// [`write_markdown`] for a destination nobody confirmed (the MCP server's `export_note`, which
/// chose a free name): nothing that exists is reused, replaced or removed. The drawings go in a
/// folder made for them (`<name>_images`, or `<name>_images (2)`… when that name is taken, even
/// by a previous export's drawings); the `.md` must still be free when it is written, else the
/// export fails and the drawings written for it are taken back.
pub fn write_markdown_new(
    destination: &Path,
    documents: &[Document],
    labels: &Labels,
) -> CommandResult<Vec<PathBuf>> {
    let stem = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = destination.parent().unwrap_or(Path::new("."));
    let fail =
        |path: &Path, error: std::io::Error| export_error(format!("{}: {error}", path.display()));
    let drawings = documents
        .iter()
        .flat_map(|document| &document.blocks)
        .any(|block| matches!(block, Block::Drawing(_)));
    let folder = if drawings {
        Some(new_images_folder(parent, &stem)?)
    } else {
        None
    };
    let folder_name = folder.as_ref().map_or("", |(name, _)| name.as_str());
    let markdown = to_markdown(documents, labels, &stem, folder_name);
    let mut written = Vec::new();
    let mut write = || -> CommandResult<()> {
        if let Some((_, folder)) = &folder {
            // The images first: a text file never points at an image that is not there.
            for image in &markdown.images {
                let path = folder.join(&image.name);
                write_atomic_new(&path, &image.png).map_err(|e| fail(&path, e))?;
                written.push(path);
            }
        }
        write_atomic_new(destination, markdown.text.as_bytes())
            .map_err(|e| fail(destination, e))?;
        Ok(())
    };
    let outcome = write();
    if outcome.is_err() || markdown.images.is_empty() {
        // Only what this export made: its drawings, then its folder if it is empty.
        if outcome.is_err() {
            for path in &written {
                let _ = fs::remove_file(path);
            }
        }
        if let Some((_, folder)) = &folder {
            let _ = fs::remove_dir(folder);
        }
    }
    outcome?;
    written.insert(0, destination.to_path_buf());
    Ok(written)
}

/// A folder for the drawings of `stem` that did not exist: made now (`create_dir` fails on a
/// name taken, even at the same moment by another program). Returns its name and path.
fn new_images_folder(parent: &Path, stem: &str) -> CommandResult<(String, PathBuf)> {
    for n in 1..=MAX_FOLDER_SUFFIX {
        let name = if n == 1 {
            format!("{stem}{IMAGES_SUFFIX}")
        } else {
            format!("{stem}{IMAGES_SUFFIX} ({n})")
        };
        let folder = parent.join(&name);
        match fs::create_dir(&folder) {
            Ok(()) => return Ok((name, folder)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(export_error(format!("{}: {error}", folder.display()))),
        }
    }
    Err(export_error(format!(
        "no free folder name for {stem}{IMAGES_SUFFIX}"
    )))
}

/// Writes the Word document of `documents` to `destination`, atomically.
pub fn write_docx(
    destination: &Path,
    documents: &[Document],
    labels: &Labels,
) -> CommandResult<Vec<PathBuf>> {
    let bytes = to_docx(documents, labels).map_err(export_error)?;
    write_atomic(destination, &bytes)
        .map_err(|error| export_error(format!("{}: {error}", destination.display())))?;
    Ok(vec![destination.to_path_buf()])
}

/// The notes of `sources` read and their structures (kept in `known`).
fn structures_of(
    indexer: &Indexer,
    context: &AppContext,
    sources: Vec<ExportSource>,
    known: &Structures,
) -> CommandResult<Vec<(LoadedSource, Arc<Structure>)>> {
    let library = library_for(context, &sources)?;
    let loaded = load_sources(library.as_ref(), sources)?;
    let engine = indexer.on_demand();
    loaded
        .into_iter()
        .map(|source| {
            let reading = reading_of(indexer, &source.note).map_err(hwr_error)?;
            let structure = structure_for(&source.note, &reading, &*engine, Some(known));
            Ok((source, structure))
        })
        .collect()
}

/// The documents of `sources` (their drawings left out unless `drawings`), and whether Windows'
/// ink analysis gave every structure.
fn documents_of(
    indexer: &Indexer,
    context: &AppContext,
    sources: Vec<ExportSource>,
    drawings: bool,
    known: &Structures,
) -> CommandResult<(Vec<Document>, bool)> {
    let mut documents = Vec::new();
    let mut analyzed = true;
    for (source, structure) in structures_of(indexer, context, sources, known)? {
        analyzed &= structure.analyzed;
        documents.push(document_of(&source, &structure, drawings)?);
    }
    Ok((documents, analyzed))
}

/// How many drawings the Markdown and Word exports of `sources` hold when they are included: the
/// export window's "Inclure les dessins" is on only when there are some. The structures found
/// are kept for the export that follows.
#[tauri::command]
pub async fn structured_text_drawings(
    app: AppHandle,
    sources: Vec<ExportSource>,
) -> CommandResult<usize> {
    let handle = app.clone();
    blocking(app, move |indexer, context| {
        let known = handle.state::<Structures>();
        let found = structures_of(indexer, context, sources, &known)?;
        Ok(found
            .iter()
            .map(|(_, structure)| structure.drawings())
            .sum())
    })
    .await
}

/// Exports the recognised text of `sources` with its structure, as Markdown or Word, to
/// `destination` (one note after the other); returns the files written. `include_drawings`
/// false leaves the drawings out (absent: they are in).
#[tauri::command]
pub async fn export_structured_text(
    app: AppHandle,
    sources: Vec<ExportSource>,
    format: StructuredFormat,
    destination: String,
    labels: TextLabels,
    include_drawings: Option<bool>,
) -> CommandResult<StructuredExport> {
    let handle = app.clone();
    blocking(app, move |indexer, context| {
        let known = handle.state::<Structures>();
        let drawings = include_drawings.unwrap_or(true);
        let (documents, analyzed) = documents_of(indexer, context, sources, drawings, &known)?;
        let labels = Labels::from(labels);
        let destination = Path::new(&destination);
        let written = match format {
            StructuredFormat::Md => write_markdown(destination, &documents, &labels)?,
            StructuredFormat::Docx => write_docx(destination, &documents, &labels)?,
        };
        Ok(StructuredExport {
            paths: written
                .iter()
                .map(|p| crate::library::path_text(p))
                .collect(),
            analyzed,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::fs;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use ardoise_core::settings::HandwritingLanguage;
    use ardoise_hwr::document::{Mark, Text};
    use ardoise_hwr::model::{Bounds, Line, Word};
    use ardoise_hwr::structure::{Region, RegionKind};
    use ardoise_hwr::{HwrError, Language, Recognition, RecognizerInfo};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    use super::*;
    use crate::handwriting::{CacheDir, IndexConfig, Services};
    use crate::library::tests::{context, sample_note};

    fn labels() -> Labels {
        Labels {
            date: "Created:".into(),
            tags: "Tags:".into(),
            drawing: "Drawing".into(),
        }
    }

    /// A stroke from (x, y) going 1 000 units right and down.
    fn stroke(x: f32, y: f32) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 60.0,
            layer: 0,
            points: (0..20)
                .map(|i| Point {
                    x: x + 50.0 * i as f32,
                    y: y + 50.0 * i as f32,
                    pressure: 0.6,
                    t: i,
                })
                .collect(),
        }
    }

    /// Has a French recognizer that reads any note as [`page_text`]; analyses a page into a
    /// paragraph (stroke 0), a list item (stroke 1) and a drawing (stroke 2), unless `analysis`
    /// is false, and with `underline` a thin mark under the paragraph's word (stroke 3). Counts
    /// its analyses.
    #[derive(Default)]
    struct Fake {
        analysis: bool,
        underline: bool,
        analyses: AtomicUsize,
    }

    impl Fake {
        fn new(analysis: bool) -> Fake {
            Fake {
                analysis,
                ..Fake::default()
            }
        }
    }

    impl Recognizer for Fake {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            Ok(vec![RecognizerInfo {
                name: "Français".into(),
                language: Some(Language::Fr),
            }])
        }

        fn recognize(&self, _: &[Stroke], language: Language) -> Result<Recognition, HwrError> {
            Ok(page_text(language))
        }

        fn analyze(&self, _: &[Stroke]) -> Result<Vec<Region>, HwrError> {
            self.analyses.fetch_add(1, Ordering::SeqCst);
            if !self.analysis {
                return Err(HwrError::Unsupported);
            }
            let region = |kind, x: f32, y: f32, stroke| Region {
                kind,
                bounds: Bounds {
                    x,
                    y,
                    width: 1000.0,
                    height: 1000.0,
                },
                text: String::new(),
                strokes: vec![stroke],
            };
            let mut regions = vec![
                region(RegionKind::Paragraph, 1000.0, 1000.0, 0),
                region(RegionKind::ListItem, 1000.0, 3000.0, 1),
                region(RegionKind::Drawing, 6000.0, 4000.0, 2),
            ];
            if self.underline {
                regions.push(Region {
                    kind: RegionKind::Drawing,
                    bounds: Bounds {
                        x: 1180.0,
                        y: 1520.0,
                        width: 540.0,
                        height: 40.0,
                    },
                    text: String::new(),
                    strokes: vec![3],
                });
            }
            Ok(regions)
        }
    }

    fn page_text(language: Language) -> Recognition {
        let word = |text: &str, x: f32, y: f32| Word {
            text: text.into(),
            alternatives: Vec::new(),
            bounds: Bounds {
                x,
                y,
                width: 500.0,
                height: 300.0,
            },
        };
        Recognition {
            language,
            recognizer: String::new(),
            lines: vec![
                Line::new(vec![word("lu", 1200.0, 1200.0)]),
                Line::new(vec![word("- pommes", 1200.0, 3200.0)]),
            ],
        }
    }

    fn source() -> LoadedSource {
        let mut note = sample_note("Journal");
        note.tags = vec!["classe".into()];
        note.strokes = vec![
            stroke(1000.0, 1000.0),
            stroke(1000.0, 3000.0),
            stroke(6000.0, 4000.0),
        ];
        LoadedSource {
            note,
            stem: "2026-10-22 Journal".into(),
            picture: None,
        }
    }

    /// How the exports read a note: its recognition, and the user's correction if any.
    fn reading(correction: Option<&str>) -> NoteReading {
        NoteReading {
            recognition: Arc::new(page_text(Language::Fr)),
            correction: correction.map(str::to_owned),
        }
    }

    /// The document of the sample note through `fake`, its drawings included.
    fn document_with(fake: &Fake, correction: Option<&str>) -> (Document, bool) {
        let source = source();
        let structure = structure_for(&source.note, &reading(correction), fake, None);
        let document = document_of(&source, &structure, true).unwrap();
        (document, structure.analyzed)
    }

    #[test]
    fn a_note_becomes_a_document_with_its_drawings_cropped() {
        let (document, analyzed) = document_with(&Fake::new(true), None);
        assert!(analyzed);
        assert_eq!(document.title, "Journal");
        assert_eq!(document.tags, ["classe"]);
        assert_eq!(document.date.len(), 16, "{}", document.date);
        assert_eq!(document.blocks.len(), 3);
        // The words are those of the recognition of the whole note (the plain text), each line
        // in the region that holds it; the list item without its dash.
        assert_eq!(document.blocks[0], Block::Paragraph("lu".into()));
        assert_eq!(document.blocks[1], Block::ListItem("pommes".into()));
        let Block::Drawing(drawing) = &document.blocks[2] else {
            panic!("a drawing")
        };
        // 1 000 units plus 2 x 2 mm of margin = 14 mm at 200 dpi.
        let expected = (1400.0 / 2540.0 * 200.0_f32).round() as i64;
        assert!(
            (i64::from(drawing.width_px) - expected).abs() <= 2,
            "{}",
            drawing.width_px
        );
        assert!(
            (drawing.width_mm - 14.0).abs() < 0.5,
            "{}",
            drawing.width_mm
        );
        // A real PNG with ink in it, and only the drawing's ink: the other strokes are cut.
        let image = ardoise_render::decode_png(&drawing.png).unwrap();
        assert_eq!(
            (image.width, image.height),
            (drawing.width_px, drawing.height_px)
        );
        let dark = |image: &Image| {
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] < 100)
                .count()
        };
        assert!(dark(&image) > 100);
        assert!(
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[..3] == [255, 255, 255])
        );
    }

    #[test]
    fn a_drawing_is_cropped_without_the_text_frames_of_the_page() {
        use ardoise_core::note::{FontFamily, PageObject, TextBody};

        let plain = source();
        let mut typed = source();
        // A frame right on the drawing (stroke 2 is at 6 000, 4 000).
        typed.note.objects = vec![PageObject::text(
            "titre",
            0,
            0,
            6_000.0,
            4_000.0,
            TextBody {
                text: "Légende".into(),
                font: FontFamily::Andika,
                bold: true,
                size: 900.0,
                color: Rgba::BLACK,
            },
        )];
        // What the crop is taken from: the strokes asked for, no object.
        let only = drawings_only(&typed.note, &[2]);
        assert_eq!(only.strokes, [typed.note.strokes[2].clone()]);
        assert!(only.objects.is_empty());
        assert_eq!(typed.note.objects.len(), 1, "the note itself is untouched");

        let drawing_of = |source: &LoadedSource| {
            let fake = Fake::new(true);
            let structure = structure_for(&source.note, &reading(None), &fake, None);
            match document_of(source, &structure, true).unwrap().blocks.pop() {
                Some(Block::Drawing(drawing)) => drawing,
                other => panic!("a drawing, got {other:?}"),
            }
        };
        assert_eq!(drawing_of(&typed), drawing_of(&plain));
    }

    #[test]
    fn without_the_analysis_the_structure_is_guessed_from_the_text() {
        let (document, analyzed) = document_with(&Fake::new(false), None);
        assert!(!analyzed);
        assert_eq!(document.blocks[0], Block::Paragraph("lu".into()));
        assert_eq!(document.blocks[1], Block::ListItem("pommes".into()));
        assert!(matches!(document.blocks[2], Block::Drawing(_)));
    }

    #[test]
    fn the_corrected_text_is_exported_and_drawings_can_be_left_out() {
        let (document, _) = document_with(&Fake::new(true), Some("relu\r\n- poires"));
        assert_eq!(document.blocks[0], Block::Paragraph("relu".into()));
        assert_eq!(document.blocks[1], Block::ListItem("poires".into()));
        assert!(matches!(document.blocks[2], Block::Drawing(_)));
        // Without the drawings: the text alone.
        let source = source();
        let structure = structure_for(&source.note, &reading(None), &Fake::new(true), None);
        assert_eq!(structure.drawings(), 1);
        let plain = document_of(&source, &structure, false).unwrap();
        assert_eq!(
            plain.blocks,
            [
                Block::Paragraph("lu".into()),
                Block::ListItem("pommes".into())
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("plain.md");
        let written = write_markdown(&destination, &[plain], &labels()).unwrap();
        assert_eq!(written, std::slice::from_ref(&destination));
    }

    #[test]
    fn an_underline_is_no_picture_but_underlined_words() {
        let fake = Fake {
            underline: true,
            ..Fake::new(true)
        };
        let (document, _) = document_with(&fake, None);
        let underlined = Text::from_words([("lu", Mark::Underline)]);
        assert_eq!(document.blocks[0], Block::Paragraph(underlined));
        assert_eq!(document.drawings().count(), 1, "only the real drawing");
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("souligné.md");
        write_markdown(&destination, std::slice::from_ref(&document), &labels()).unwrap();
        let text = fs::read_to_string(&destination).unwrap();
        assert!(text.contains("\n**lu**\n\n- pommes\n"), "{text}");
        // Word underlines the run (the run itself: `ardoise-hwr`'s `docx` tests).
        let docx = dir.path().join("souligné.docx");
        assert_eq!(write_docx(&docx, &[document], &labels()).unwrap(), [docx]);
    }

    #[test]
    fn structures_are_found_once_for_the_check_and_the_export() {
        let fake = Fake::new(true);
        let known = Structures::default();
        let note = source().note;
        let first = structure_for(&note, &reading(None), &fake, Some(&known));
        let again = structure_for(&note, &reading(None), &fake, Some(&known));
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(fake.analyses.load(Ordering::SeqCst), 1);
        // Another text, other recognised words, other ink or a hidden layer: found again.
        structure_for(&note, &reading(Some("relu")), &fake, Some(&known));
        let mut misread = page_text(Language::Fr);
        misread.lines[0].words[0].text = "la".into();
        let other = NoteReading {
            recognition: Arc::new(misread),
            correction: None,
        };
        structure_for(&note, &other, &fake, Some(&known));
        let mut moved = note.clone();
        moved.strokes[0].points[0].x += 1.0;
        structure_for(&moved, &reading(None), &fake, Some(&known));
        let mut hidden = note.clone();
        hidden.layers[0].visible = false;
        structure_for(&hidden, &reading(None), &fake, Some(&known));
        assert_eq!(fake.analyses.load(Ordering::SeqCst), 5);
        // A title is not the ink.
        let mut renamed = note.clone();
        renamed.title = "Autre".into();
        structure_for(&renamed, &reading(None), &fake, Some(&known));
        assert_eq!(fake.analyses.load(Ordering::SeqCst), 5);
        // A few are kept, the oldest go first.
        for n in 0..KEPT_STRUCTURES {
            let mut other = note.clone();
            other.id = format!("autre-{n}");
            structure_for(&other, &reading(None), &fake, Some(&known));
        }
        assert_eq!(known.0.lock().unwrap().len(), KEPT_STRUCTURES);
        let before = fake.analyses.load(Ordering::SeqCst);
        structure_for(&note, &reading(None), &fake, Some(&known));
        assert_eq!(fake.analyses.load(Ordering::SeqCst), before + 1);
        // Without a store, nothing is kept.
        structure_for(&note, &reading(None), &fake, None);
        assert_eq!(fake.analyses.load(Ordering::SeqCst), before + 2);
    }

    #[test]
    fn the_export_and_its_check_read_the_correction_through_the_indexer() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        let cache = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::new(true));
        let indexer = Indexer::start(Services {
            background: fake.clone(),
            on_demand: fake.clone(),
            config: Box::new(|| IndexConfig {
                library: None,
                setting: HandwritingLanguage::Auto,
                interface: Language::Fr,
            }),
            busy: Box::new(|| false),
            notify: Box::new(|_| {}),
            cache_dir: cache.path().to_path_buf(),
            pause_poll: Duration::from_millis(5),
        });
        let note = source().note;
        let sources = || {
            vec![ExportSource {
                path: "Journal.ardoise".into(),
                note: Some(note.clone()),
                image_key: None,
            }]
        };
        let known = Structures::default();
        let found = structures_of(&indexer, &context, sources(), &known).unwrap();
        assert_eq!(found[0].1.drawings(), 1);
        let (documents, analyzed) =
            documents_of(&indexer, &context, sources(), false, &known).unwrap();
        assert!(analyzed);
        assert_eq!(documents[0].blocks.len(), 2);
        assert_eq!(fake.analyses.load(Ordering::SeqCst), 1, "analysed once");
        // The user's correction of the note's ink wins.
        CacheDir::new(cache.path())
            .set_correction_for(&note, Language::Fr, "relu\n- poires")
            .unwrap();
        let (documents, _) = documents_of(&indexer, &context, sources(), true, &known).unwrap();
        assert_eq!(documents[0].blocks[0], Block::Paragraph("relu".into()));
        assert_eq!(documents[0].blocks[1], Block::ListItem("poires".into()));
        assert_eq!(documents[0].drawings().count(), 1);
        indexer.shutdown();
    }

    /// The document of the sample note, with one drawing.
    fn sample_document() -> Document {
        document_with(&Fake::new(true), None).0
    }

    #[test]
    fn markdown_is_written_with_its_images_in_a_folder_next_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("Mon journal (1).md");
        let written = write_markdown(&destination, &[sample_document()], &labels()).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(written[0], destination);
        let image = dir
            .path()
            .join("Mon journal (1)_images")
            .join("Mon journal (1)-1.png");
        assert_eq!(written[1], image);
        let text = fs::read_to_string(&destination).unwrap();
        assert!(
            text.starts_with(
                "# Journal

Created: "
            ),
            "{text}"
        );
        assert!(text.contains("Tags: classe"));
        assert!(
            text.contains(
                "
lu

- pommes

![Drawing 1](Mon%20journal%20%281%29_images/Mon%20journal%20%281%29-1.png)
"
            ),
            "{text}"
        );
        assert!(ardoise_render::decode_png(&fs::read(&image).unwrap()).is_ok());
        // Written again over the same files: atomic, no leftover, the same folder.
        let again = write_markdown(&destination, &[sample_document()], &labels()).unwrap();
        assert_eq!(again, written);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert_eq!(fs::read_dir(image.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn the_markdown_export_never_replaces_a_file_of_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("notes.md");
        // The user's own files with the names the old export used, and a folder of that name.
        let mine = dir.path().join("notes-1.png");
        fs::write(&mine, b"mine").unwrap();
        let folder = dir.path().join("notes_images");
        fs::create_dir(&folder).unwrap();
        let theirs = folder.join("notes-1.png");
        fs::write(&theirs, b"theirs").unwrap();
        fs::write(folder.join("readme.txt"), b"keep").unwrap();
        let written = write_markdown(&destination, &[sample_document()], &labels()).unwrap();
        assert_eq!(fs::read(&mine).unwrap(), b"mine");
        assert_eq!(fs::read(&theirs).unwrap(), b"theirs");
        assert_eq!(fs::read(folder.join("readme.txt")).unwrap(), b"keep");
        // The drawings are in a free folder, and the links point at the files written.
        let image = dir.path().join("notes_images (2)").join("notes-1.png");
        assert_eq!(written, [destination.clone(), image.clone()]);
        assert!(ardoise_render::decode_png(&fs::read(&image).unwrap()).is_ok());
        let text = fs::read_to_string(&destination).unwrap();
        assert!(
            text.contains("![Drawing 1](notes_images%20%282%29/notes-1.png)"),
            "{text}"
        );
        // A second export of the document goes to the same free folder again.
        let again = write_markdown(&destination, &[sample_document()], &labels()).unwrap();
        assert_eq!(again, written);
        assert_eq!(fs::read(&theirs).unwrap(), b"theirs");
        assert!(!dir.path().join("notes_images (3)").exists());
    }

    #[test]
    fn a_new_markdown_export_reuses_replaces_and_removes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("notes.md");
        // A previous export's drawings, whose `.md` is gone: the GUI's export would take them
        // back; this one leaves them alone.
        let folder = dir.path().join("notes_images");
        fs::create_dir(&folder).unwrap();
        for n in 1..=2 {
            fs::write(folder.join(format!("notes-{n}.png")), b"old").unwrap();
        }
        let written = write_markdown_new(&destination, &[sample_document()], &labels()).unwrap();
        let image = dir.path().join("notes_images (2)").join("notes-1.png");
        assert_eq!(written, [destination.clone(), image.clone()]);
        assert!(ardoise_render::decode_png(&fs::read(&image).unwrap()).is_ok());
        assert!(
            fs::read_to_string(&destination)
                .unwrap()
                .contains("![Drawing 1](notes_images%20%282%29/notes-1.png)")
        );
        for n in 1..=2 {
            assert_eq!(
                fs::read(folder.join(format!("notes-{n}.png"))).unwrap(),
                b"old"
            );
        }

        // The `.md` taken meanwhile: refused, the file kept, the drawings made for it taken
        // back with their folder.
        let error = write_markdown_new(&destination, &[sample_document()], &labels()).unwrap_err();
        assert_eq!(error.parts().0, "exportFailed");
        assert!(
            fs::read_to_string(&destination)
                .unwrap()
                .starts_with("# Journal")
        );
        assert!(!dir.path().join("notes_images (3)").exists());

        // Without drawings, no folder.
        let plain_md = dir.path().join("plain.md");
        let mut plain = sample_document();
        plain.blocks.retain(|b| !matches!(b, Block::Drawing(_)));
        let written = write_markdown_new(&plain_md, &[plain], &labels()).unwrap();
        assert_eq!(written, std::slice::from_ref(&plain_md));
        assert!(!dir.path().join("plain_images").exists());
    }

    #[test]
    fn exporting_fewer_drawings_leaves_no_stale_image() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("notes.md");
        let two = vec![sample_document(), sample_document()];
        let written = write_markdown(&destination, &two, &labels()).unwrap();
        assert_eq!(written.len(), 3);
        let folder = dir.path().join("notes_images");
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
        // One drawing now: the second image goes.
        let written = write_markdown(&destination, &[sample_document()], &labels()).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        assert!(!folder.join("notes-2.png").exists());
        // No drawing: the folder is gone, and the text has no link.
        let mut plain = sample_document();
        plain.blocks.retain(|b| !matches!(b, Block::Drawing(_)));
        let written = write_markdown(&destination, &[plain], &labels()).unwrap();
        assert_eq!(written, std::slice::from_ref(&destination));
        assert!(!folder.exists());
        assert!(!fs::read_to_string(&destination).unwrap().contains(".png"));
        // A folder with a file of the user is never touched by an export without drawings.
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("notes-1.png"), b"x").unwrap();
        fs::write(folder.join("other.txt"), b"y").unwrap();
        write_markdown(&destination, &[], &labels()).unwrap();
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
    }

    #[test]
    fn only_the_images_of_an_export_of_the_same_document_are_recognised() {
        assert!(is_export_image("notes-1.png", "notes"));
        assert!(is_export_image("notes-12.png", "notes"));
        assert!(!is_export_image("notes-.png", "notes"));
        assert!(!is_export_image("notes-1.jpg", "notes"));
        assert!(!is_export_image("notes-a.png", "notes"));
        assert!(!is_export_image("other-1.png", "notes"));
        assert!(!is_export_image("notes-1.png.bak", "notes"));
    }

    #[test]
    fn a_word_document_embeds_the_drawings() {
        let (document, _) = document_with(&Fake::new(true), None);
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("journal.docx");
        let written = write_docx(&destination, &[document.clone(), document], &labels()).unwrap();
        assert_eq!(written, std::slice::from_ref(&destination));
        // A ZIP file whose member names are stored in clear (the format is read back in
        // `ardoise-hwr`'s tests).
        let bytes = fs::read(&destination).unwrap();
        let holds = |name: &str| bytes.windows(name.len()).any(|w| w == name.as_bytes());
        assert_eq!(&bytes[..2], b"PK");
        assert!(holds("word/document.xml") && holds("word/media/image2.png"));
        assert!(!holds("word/media/image3.png"));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn dates_are_local_and_arguments_have_the_interface_shape() {
        let date = format_date(1_792_843_200_000);
        assert_eq!(date.len(), 16);
        assert!(date.starts_with("2026-10-"), "{date}");
        assert_eq!(format_date(i64::MAX), "");
        let format: StructuredFormat = serde_json::from_value(serde_json::json!("docx")).unwrap();
        assert_eq!(format, StructuredFormat::Docx);
        let labels: TextLabels = serde_json::from_value(
            serde_json::json!({ "date": "Créée", "tags": "Étiquettes", "drawing": "Dessin" }),
        )
        .unwrap();
        assert_eq!(Labels::from(labels).tags, "Étiquettes");
        let json = serde_json::to_value(StructuredExport {
            paths: vec!["a".into()],
            analyzed: false,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "paths": ["a"], "analyzed": false })
        );
    }

    #[test]
    fn cropping_stays_inside_the_image() {
        let image = Image {
            width: 4,
            height: 3,
            pixels: (0..4 * 3 * 4).map(|i| i as u8).collect(),
        };
        let part = crop(&image, 1, 1, 3, 3);
        assert_eq!((part.width, part.height), (2, 2));
        assert_eq!(part.pixels[..4], image.pixels[(4 + 1) * 4..(4 + 1) * 4 + 4]);
        // Past the edges: clamped, never empty.
        let edge = crop(&image, 9, 9, 20, 20);
        assert_eq!((edge.width, edge.height), (1, 1));
    }
}
