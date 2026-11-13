//! `create_note`: a blank note, or a note on an image or a page of a PDF.

use std::path::Path;

use ardoise_core::library::normalize_tags;
use ardoise_core::note::{
    A4_LANDSCAPE, Background, Layer, Note, Origin, PageSize, Template, Timing,
};
use ardoise_hwr::Language;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::update::{MetadataChange, apply_metadata, check_title, now_ms, summary_at};
use crate::error::{ToolError, ToolResult, json_result};
use crate::host::Host;
use crate::read::clean_binder;

/// Paper sizes in device units (10 µm): the sheet's own orientation, portrait for the
/// printed sizes and landscape for the tablet's page (the app's new notes).
const A4: (u32, u32) = (21_000, 29_700);
const A5: (u32, u32) = (14_800, 21_000);
const LETTER: (u32, u32) = (21_590, 27_940);
/// Custom pages, millimetres.
const MIN_SIDE_MM: f64 = 10.0;
const MAX_SIDE_MM: f64 = 2_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PaperName {
    /// 210 x 297 mm, portrait.
    A4,
    /// 148 x 210 mm, portrait.
    A5,
    /// 215.9 x 279.4 mm, portrait.
    Letter,
    /// The page of the Bamboo Slate Large, 297 x 216 mm, landscape (the app's default).
    Tablet,
}

/// A paper size.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PaperArg {
    Named(PaperName),
    /// A page of this width and height in millimetres (10 to 2000).
    Custom {
        width: f64,
        height: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// The printed guide lines under the ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TemplateName {
    Blank,
    /// Ruled lines 8 mm apart.
    Lined,
    /// French "Seyès" ruling.
    Seyes,
    /// 5 mm squares.
    Grid5mm,
    /// Millimetre graph paper.
    Millimetric,
    /// Dots every 5 mm.
    Dotted,
    /// Music staves.
    Music,
}

impl From<TemplateName> for Template {
    fn from(name: TemplateName) -> Template {
        match name {
            TemplateName::Blank => Template::Blank,
            TemplateName::Lined => Template::Lined,
            TemplateName::Seyes => Template::Seyes,
            TemplateName::Grid5mm => Template::Grid5mm,
            TemplateName::Millimetric => Template::Millimetric,
            TemplateName::Dotted => Template::Dotted,
            TemplateName::Music => Template::Music,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateNoteArgs {
    /// Title of the note (default: none, the app then shows its creation date).
    pub title: Option<String>,
    /// Binder to create it in: a path relative to the library, `/`-separated, created if missing (default: the root of the library).
    pub binder: Option<String>,
    /// Tags of the note.
    pub tags: Option<Vec<String>>,
    /// Page size of a blank note: `a4`, `a5`, `letter`, `tablet` (default, 297 x 216 mm landscape) or `{ "width": mm, "height": mm }`.
    pub paper: Option<PaperArg>,
    /// Turns the page so that it is portrait or landscape (default: the paper's own orientation).
    pub orientation: Option<Orientation>,
    /// Guide lines of a blank note (default `blank`).
    pub template: Option<TemplateName>,
    /// Absolute path of an image (PNG, JPEG) or of a PDF to make the note on: the page takes the shape of the file and shows it as background, ready to be annotated. `paper`, `orientation` and `template` do not apply.
    pub from_file: Option<String>,
    /// With a PDF in `from_file`: the page to use, from 1 (default 1).
    #[schemars(range(min = 1))]
    pub pdf_page: Option<u32>,
}

/// The page of a blank note, in device units.
pub(super) fn page_size(
    paper: Option<PaperArg>,
    orientation: Option<Orientation>,
) -> Result<PageSize, ToolError> {
    let (width, height) = match paper {
        None | Some(PaperArg::Named(PaperName::Tablet)) => {
            (A4_LANDSCAPE.width, A4_LANDSCAPE.height)
        }
        Some(PaperArg::Named(PaperName::A4)) => A4,
        Some(PaperArg::Named(PaperName::A5)) => A5,
        Some(PaperArg::Named(PaperName::Letter)) => LETTER,
        Some(PaperArg::Custom { width, height }) => {
            let in_range = |mm: f64| mm.is_finite() && (MIN_SIDE_MM..=MAX_SIDE_MM).contains(&mm);
            if !in_range(width) || !in_range(height) {
                return Err(ToolError::invalid(format!(
                    "`paper` is {width} x {height} mm: each side must be between {MIN_SIDE_MM} and {MAX_SIDE_MM} mm"
                )));
            }
            (
                (width * 100.0).round() as u32,
                (height * 100.0).round() as u32,
            )
        }
    };
    let (short, long) = (width.min(height), width.max(height));
    let (width, height) = match orientation {
        None => (width, height),
        Some(Orientation::Portrait) => (short, long),
        Some(Orientation::Landscape) => (long, short),
    };
    Ok(PageSize { width, height })
}

/// The name of the first layer, as the app names it in `language`.
pub(super) fn first_layer_name(language: Language) -> &'static str {
    match language {
        Language::Fr => "Calque 1",
        Language::En => "Layer 1",
    }
}

pub fn create_note(host: &Host, args: CreateNoteArgs) -> ToolResult {
    let binder = clean_binder(args.binder.as_deref().unwrap_or_default());
    let title = args.title.as_deref().map(str::trim).unwrap_or_default();
    check_title(title)?;
    let tags = normalize_tags(args.tags.as_deref().unwrap_or_default());
    let layer = first_layer_name(host.environment.language());

    if args.from_file.is_none() && args.pdf_page.is_some() {
        return Err(ToolError::invalid("`pdf_page` needs `from_file`"));
    }
    if args.from_file.is_some()
        && (args.paper.is_some() || args.orientation.is_some() || args.template.is_some())
    {
        return Err(ToolError::invalid(
            "`paper`, `orientation` and `template` do not apply with `from_file`: the page takes the shape of the file",
        ));
    }
    let page = page_size(args.paper, args.orientation)?;

    let library = host.library()?;
    let saved = if let Some(file) = &args.from_file {
        let file = Path::new(file);
        if !file.is_absolute() {
            return Err(ToolError::invalid(format!(
                "`from_file` must be an absolute path, not `{}`",
                file.display()
            )));
        }
        if !file.is_file() {
            return Err(ToolError::invalid(format!(
                "`from_file` is not a file: {}",
                file.display()
            )));
        }
        let saved = host.write.create_from_file(
            &library,
            Path::new(&binder),
            file,
            args.pdf_page.unwrap_or(1),
            layer,
        )?;
        // The note is titled after the file; the title and tags asked for come on top.
        let change = MetadataChange {
            title: (!title.is_empty()).then_some(title),
            tags: (!tags.is_empty()).then_some(&tags[..]),
            ..MetadataChange::default()
        };
        let (path, _) = apply_metadata(&library, &saved.path, &change)?;
        path
    } else {
        let now = now_ms();
        let note = Note {
            id: uuid_v4(),
            title: title.to_owned(),
            created: now,
            modified: now,
            page,
            background: Background {
                template: args.template.map_or(Template::Blank, Template::from),
                ..Background::default()
            },
            background_image: None,
            tags,
            favorite: false,
            layers: vec![Layer {
                name: layer.to_owned(),
                visible: true,
            }],
            strokes: Vec::new(),
            objects: Vec::new(),
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        };
        host.write
            .create_blank(&library, Path::new(&binder), note)?
            .path
    };
    let note = summary_at(&library, &saved)?;
    json_result(json!({ "note": note }))
}

/// A random (version 4) UUID, as the app gives its notes.
pub(super) fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_follow_the_paper_and_the_orientation() {
        let page = |paper, orientation| page_size(paper, orientation).unwrap();
        // The default is the app's new note: the tablet's page.
        assert_eq!(page(None, None), A4_LANDSCAPE);
        assert_eq!(
            page(Some(PaperArg::Named(PaperName::A4)), None),
            PageSize {
                width: 21_000,
                height: 29_700
            }
        );
        assert_eq!(
            page(
                Some(PaperArg::Named(PaperName::A5)),
                Some(Orientation::Landscape)
            ),
            PageSize {
                width: 21_000,
                height: 14_800
            }
        );
        assert_eq!(
            page(
                Some(PaperArg::Named(PaperName::Tablet)),
                Some(Orientation::Portrait)
            ),
            PageSize {
                width: 21_600,
                height: 29_700
            }
        );
        assert_eq!(
            page(
                Some(PaperArg::Custom {
                    width: 100.0,
                    height: 50.5
                }),
                None
            ),
            PageSize {
                width: 10_000,
                height: 5_050
            }
        );
        for bad in [0.0, 5.0, 2_500.0, f64::NAN] {
            let error = page_size(
                Some(PaperArg::Custom {
                    width: bad,
                    height: 100.0,
                }),
                None,
            )
            .unwrap_err();
            assert_eq!(error.code, "invalidArguments", "{bad}");
        }
    }
}
