//! The text export with the structure drawn from the ink (milestone M8): a note's handwriting as
//! Markdown (`.md`, the drawings as PNG files next to it) or as a Word document (`.docx`, the
//! drawings embedded). Contract: `docs/design/extras.md`, "M8: text export with structure".
//!
//! The structure (paragraphs, list items, drawings) comes from Windows' ink analysis, or from
//! layout heuristics on the recognised lines when it is not available
//! (`ardoise_hwr::structure`, platform-free). The files are built by `ardoise_hwr::markdown` and
//! `ardoise_hwr::docx`; this module reads the notes, crops the drawings out of them and writes
//! the files atomically.

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

/// Writes the Markdown of `documents` to `destination` and the drawings next to it
/// (`<name>-1.png`…), atomically; returns the files, the document first.
pub fn write_markdown(
    destination: &Path,
    documents: &[Document],
    labels: &Labels,
) -> CommandResult<Vec<PathBuf>> {
    let prefix = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let markdown = to_markdown(documents, labels, &prefix);
    let folder = destination.parent().unwrap_or(Path::new("."));
    let fail =
        |path: &Path, error: std::io::Error| export_error(format!("{}: {error}", path.display()));
    let mut written = Vec::new();
    // The images first: a text file never points at an image that is not there.
    for image in &markdown.images {
        let path = folder.join(&image.name);
        write_atomic(&path, &image.png).map_err(|e| fail(&path, e))?;
        written.push(path);
    }
    write_atomic(destination, markdown.text.as_bytes()).map_err(|e| fail(destination, e))?;
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

    #[test]
    fn markdown_is_written_with_its_images_next_to_it() {
        let (document, _) = document_of(
            &source(),
            &page_text(Language::Fr),
            &Fake { analysis: true },
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("Mon journal (1).md");
        let written = write_markdown(&destination, &[document], &labels()).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(written[0], destination);
        let image = dir.path().join("Mon journal (1)-1.png");
        assert_eq!(written[1], image);
        let text = fs::read_to_string(&destination).unwrap();
        assert!(text.starts_with("# Journal\n\nCreated: "), "{text}");
        assert!(text.contains("Tags: classe"));
        assert!(
            text.contains("\nlu\n\n- lu\n\n![Drawing 1](Mon%20journal%20%281%29-1.png)\n"),
            "{text}"
        );
        assert!(ardoise_render::decode_png(&fs::read(&image).unwrap()).is_ok());
        // Written again over the same files: atomic, no leftover.
        write_markdown(&destination, &[], &labels()).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
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
