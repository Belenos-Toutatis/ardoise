//! The text export with the structure drawn from the ink (milestone M8): a note's handwriting as
//! Markdown (`.md`, the drawings as PNG files in a folder next to it) or as a Word document (`.docx`, the
//! drawings embedded). Contract: `docs/design/extras.md`, "M8: text export with structure".
//!
//! The structure (paragraphs, list items, drawings) comes from Windows' ink analysis, or from
//! layout heuristics on the recognised lines when it is not available
//! (`ardoise_hwr::structure`, platform-free). The files are built by `ardoise_hwr::markdown` and
//! `ardoise_hwr::docx`; this module reads the notes, crops the drawings out of them and writes
//! the files atomically.

use std::fs;
use std::path::{Path, PathBuf};

use ardoise_core::fsutil::write_atomic;
use ardoise_core::note::Note;
use ardoise_hwr::document::{Block, Document, Drawing, Labels};
use ardoise_hwr::docx::to_docx;
use ardoise_hwr::markdown::to_markdown;
use ardoise_hwr::structure::{DrawingArea, Item, structure_of};
use ardoise_hwr::{Recognition, Recognizer};
use ardoise_render::{
    Image, NoteView, RenderOptions, encode_png_at, paper_color, render_note, scale_for_dpi,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::commands::CommandResult;
use crate::export::{
    ExportSource, LoadedSource, export_error, library_for, load_sources, render_error,
};
use crate::handwriting::{blocking, hwr_error};

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
    let mut note: Note = source.note.clone();
    note.strokes = strokes
        .iter()
        .filter_map(|&index| source.note.strokes.get(index).cloned())
        .collect();
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

/// The document of one note: its title (or file name), date and tags, then its structure.
/// The flag tells whether Windows' ink analysis gave the structure.
pub fn document_of(
    source: &LoadedSource,
    recognition: &Recognition,
    recognizer: &dyn Recognizer,
) -> CommandResult<(Document, bool)> {
    let structure = structure_of(recognizer, recognition, &source.note);
    let areas: Vec<&DrawingArea> = structure
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Drawing(area) => Some(area),
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
            Item::Drawing(_) => images.next().map(Block::Drawing),
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
    Ok((document, structure.analyzed))
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

/// Exports the recognised text of `sources` with its structure, as Markdown or Word, to
/// `destination` (one note after the other); returns the files written.
#[tauri::command]
pub async fn export_structured_text(
    app: AppHandle,
    sources: Vec<ExportSource>,
    format: StructuredFormat,
    destination: String,
    labels: TextLabels,
) -> CommandResult<StructuredExport> {
    blocking(app, move |indexer, context| {
        let library = library_for(context, &sources)?;
        let loaded = load_sources(library.as_ref(), sources)?;
        let engine = indexer.on_demand();
        let mut documents = Vec::new();
        let mut analyzed = true;
        for source in &loaded {
            let recognition = indexer.recognition_of(&source.note).map_err(hwr_error)?;
            let (document, by_analysis) = document_of(source, &recognition, &*engine)?;
            analyzed &= by_analysis;
            documents.push(document);
        }
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

    use ardoise_hwr::model::{Bounds, Line, Word};
    use ardoise_hwr::structure::{Region, RegionKind};
    use ardoise_hwr::{HwrError, Language, RecognizerInfo};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    use super::*;
    use crate::library::tests::sample_note;

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

    /// Reads "lu" for any group of strokes; analyses a page into a paragraph (stroke 0), a list item
    /// (stroke 1) and a drawing (stroke 2), unless `analysis` is false.
    struct Fake {
        analysis: bool,
    }

    impl Recognizer for Fake {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            Ok(Vec::new())
        }

        /// Reading a paragraph or a list item on its own: "lu".
        fn recognize(&self, _: &[Stroke], language: Language) -> Result<Recognition, HwrError> {
            let mut read = page_text(language);
            read.lines.truncate(1);
            Ok(read)
        }

        fn analyze(&self, _: &[Stroke]) -> Result<Vec<Region>, HwrError> {
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
            Ok(vec![
                region(RegionKind::Paragraph, 1000.0, 1000.0, 0),
                region(RegionKind::ListItem, 1000.0, 3000.0, 1),
                region(RegionKind::Drawing, 6000.0, 4000.0, 2),
            ])
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

    #[test]
    fn a_note_becomes_a_document_with_its_drawings_cropped() {
        let (document, analyzed) = document_of(
            &source(),
            &page_text(Language::Fr),
            &Fake { analysis: true },
        )
        .unwrap();
        assert!(analyzed);
        assert_eq!(document.title, "Journal");
        assert_eq!(document.tags, ["classe"]);
        assert_eq!(document.date.len(), 16, "{}", document.date);
        assert_eq!(document.blocks.len(), 3);
        // The paragraph and the list item are read again on their own ("lu").
        assert_eq!(document.blocks[0], Block::Paragraph("lu".into()));
        assert_eq!(document.blocks[1], Block::ListItem("lu".into()));
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
    fn without_the_analysis_the_structure_is_guessed_from_the_text() {
        let (document, analyzed) = document_of(
            &source(),
            &page_text(Language::Fr),
            &Fake { analysis: false },
        )
        .unwrap();
        assert!(!analyzed);
        assert_eq!(document.blocks[0], Block::Paragraph("lu".into()));
        assert_eq!(document.blocks[1], Block::ListItem("pommes".into()));
        assert!(matches!(document.blocks[2], Block::Drawing(_)));
    }

    /// The document of the sample note, with one drawing.
    fn sample_document() -> Document {
        document_of(
            &source(),
            &page_text(Language::Fr),
            &Fake { analysis: true },
        )
        .unwrap()
        .0
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

- lu

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
        assert_eq!(written, [destination.clone()]);
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
        let (document, _) = document_of(
            &source(),
            &page_text(Language::En),
            &Fake { analysis: true },
        )
        .unwrap();
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
