//! The note model: what one `.ardoise` file holds (SPEC §3.3).
//!
//! Strokes are in page coordinates, device units (10 µm), origin top-left of the page as
//! displayed (see `ardoise_ink::model`). Rotating a page by a quarter turn rewrites the
//! points and swaps the page size; the tablet's raw data stays untouched in `original.bin`
//! (or, for a note that does not come from the tablet's memory, the note as it was before its
//! first edit stays in `original.ardoise`: "back to the original").

use ardoise_ink::{Rgba, Stroke};

pub use crate::objects::{BookmarkBody, EquationBody, ObjectBody, PageObject, TextBody};
/// The families of a text frame, for the crates that build notes and do not depend on the fonts.
pub use ardoise_text::FontFamily;

/// A bookmark as the library's listing gives it (`BookmarkRef` in `note.ts`): enough for the binder's
/// summary and the search, and to open the note at its spot, without reading anything but `note.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BookmarkRef {
    /// The object's id: what the editor finds it by.
    pub id: String,
    pub label: String,
    /// The centre of the bookmark's box on the page, device units, rounded.
    pub x: i32,
    pub y: i32,
}

/// Page sizes of the Bamboo Slate Large (A4) in device units.
pub const A4_LANDSCAPE: PageSize = PageSize {
    width: 29_700,
    height: 21_600,
};
pub const A4_PORTRAIT: PageSize = PageSize {
    width: 21_600,
    height: 29_700,
};

/// Page size in device units (10 µm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageSize {
    pub width: u32,
    pub height: u32,
}

/// Paper colour (SPEC §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Paper {
    White,
    Cream,
    Kraft,
    /// Blackboard ("tableau noir"), for chalk in class.
    Slate,
    /// Any other colour.
    Custom(Rgba),
}

/// Printed guide lines (SPEC §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Template {
    Blank,
    /// Ruled lines, 8 mm apart.
    Lined,
    /// French "Seyès" ruling: 8 mm squares with 3 thin lines in between.
    Seyes,
    /// 5 mm squares.
    Grid5mm,
    /// Millimetre graph paper.
    Millimetric,
    /// Dots every 5 mm.
    Dotted,
    /// Music staves (P3).
    Music,
}

/// What is under the strokes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    pub paper: Paper,
    pub template: Template,
    /// Subtle paper grain.
    pub texture: bool,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            paper: Paper::White,
            template: Template::Blank,
            texture: false,
        }
    }
}

/// Where a background image goes, in page coordinates (device units). It may reach outside
/// the page: what is outside is not drawn.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// How an image is placed when it is imported (the note then keeps only the resulting
/// [`ImageRect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageFit {
    /// Whole image visible, centred, aspect ratio kept (the paper shows around it).
    Contain,
    /// Page covered, centred, aspect ratio kept (the image may be cut).
    Cover,
    /// Page covered exactly, aspect ratio not kept.
    Stretch,
}

impl ImageRect {
    /// The rectangle where an image `pixels` = (width, height) wide and high, as displayed
    /// (after its quarter turns), goes on `page` with `fit`. An empty image gets the whole page.
    pub fn fit(page: PageSize, pixels: (u32, u32), fit: ImageFit) -> ImageRect {
        let (page_w, page_h) = (page.width as f32, page.height as f32);
        let whole = ImageRect {
            x: 0.0,
            y: 0.0,
            width: page_w,
            height: page_h,
        };
        let (image_w, image_h) = (pixels.0 as f32, pixels.1 as f32);
        if pixels.0 == 0 || pixels.1 == 0 || fit == ImageFit::Stretch {
            return whole;
        }
        let (by_width, by_height) = (page_w / image_w, page_h / image_h);
        let scale = match fit {
            ImageFit::Contain => by_width.min(by_height),
            _ => by_width.max(by_height),
        };
        let (width, height) = (image_w * scale, image_h * scale);
        ImageRect {
            x: (page_w - width) / 2.0,
            y: (page_h - height) / 2.0,
            width,
            height,
        }
    }
}

/// An imported image or PDF page drawn under the strokes, over the paper and its template
/// (SPEC §4.4, P2). Its pixels are the note file's `background.png` or `background.jpg` member
/// (a PDF page becomes an image when it is imported); without that member nothing is drawn.
/// Page operations move it with the strokes: a quarter turn turns `rect` and adds one to
/// `quarter_turns`, cropping shifts `rect`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundImage {
    /// Name of the file it was imported from, shown to the user (`fiche.pdf`).
    pub name: String,
    /// Page of the PDF it comes from, from 1; absent for an image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_page: Option<u32>,
    /// Where the image is stretched to, after its quarter turns.
    pub rect: ImageRect,
    /// Clockwise quarter turns applied to the stored pixels (0 to 3; taken modulo 4).
    #[serde(default)]
    pub quarter_turns: u8,
    /// 0 (invisible) to 1 (as imported); values outside are clamped when drawing.
    #[serde(default = "full_opacity")]
    pub opacity: f32,
}

fn full_opacity() -> f32 {
    1.0
}

/// A layer: strokes refer to layers by index.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub name: String,
    pub visible: bool,
}

/// Whether point times were measured or made up (imported notes without timing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Timing {
    Recorded,
    /// Times were synthesised (uniform sampling, fixed gaps): replay is approximate.
    Synthesized,
}

/// Where a note comes from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// A page stored on the tablet and downloaded.
    Tablet,
    /// Written in live mode.
    Live,
    /// Imported from Wacom Inkspace.
    Inkspace {
        /// Inkspace's note identifier (UUID).
        id: String,
    },
    /// Made in Ardoise (split, merge, blank note…).
    Ardoise,
}

/// One note: a page of strokes with its metadata.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// UUID, stable for the life of the note (survives renames and moves).
    pub id: String,
    pub title: String,
    /// Unix milliseconds. Also the time origin of every point (`Point::t`).
    pub created: i64,
    /// Unix milliseconds of the last change.
    pub modified: i64,
    pub page: PageSize,
    pub background: Background,
    /// Imported image or PDF page under the strokes (format 1.2; written only when set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_image: Option<BackgroundImage>,
    pub tags: Vec<String>,
    /// Marked as a favourite (format 1.1; absent from older files, written only when set).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
    /// At least one layer.
    pub layers: Vec<Layer>,
    pub strokes: Vec<Stroke>,
    /// What the page holds besides its strokes: text frames (format 1.4; written only when there
    /// is one). A list this version cannot read is kept as an unknown key of `note.json` and reads
    /// as empty (`objects::deserialize_objects`).
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::objects::deserialize_objects"
    )]
    pub objects: Vec<PageObject>,
    pub timing: Timing,
    pub origin: Origin,
}

impl Note {
    /// Whether the page holds anything to draw: a stroke with a sample, or an object that is not a
    /// bookmark (a bookmark is a mark on the page, drawn in the editor only).
    pub fn has_content(&self) -> bool {
        self.objects.iter().any(|o| !o.is_bookmark())
            || self.strokes.iter().any(|s| !s.points.is_empty())
    }

    /// Whether `layer` is shown (a layer the note does not have is not).
    pub fn layer_visible(&self, layer: u32) -> bool {
        self.layers
            .get(layer as usize)
            .is_some_and(|layer| layer.visible)
    }

    /// The text frames this version can read, in reading order (top to bottom, then left to
    /// right, by the top-left corner of their box on the page), with their text. Every layer;
    /// the caller picks with [`Note::layer_visible`].
    pub fn text_frames(&self) -> Vec<(&PageObject, &TextBody)> {
        let mut frames: Vec<_> = self
            .objects
            .iter()
            .filter_map(|object| Some((object, object.as_text()?)))
            .collect();
        frames.sort_by(|(a, _), (b, _)| reading_order(a, b));
        frames
    }

    /// The equations this version can read, in reading order (as [`Note::text_frames`]), with
    /// their source and drawing. Every layer.
    pub fn equations(&self) -> Vec<(&PageObject, &EquationBody)> {
        let mut equations: Vec<_> = self
            .objects
            .iter()
            .filter_map(|object| Some((object, object.as_equation()?)))
            .collect();
        equations.sort_by(|(a, _), (b, _)| reading_order(a, b));
        equations
    }

    /// The bookmarks of the page that say something (a blank label is not listed), in reading order
    /// (top to bottom, then left to right, by the top-left corner of their box on the page), every
    /// layer: a bookmark is not hidden with its layer (SPEC §4.4 « Signets »).
    pub fn bookmark_objects(&self) -> Vec<(&PageObject, &BookmarkBody)> {
        let mut found: Vec<(&PageObject, &BookmarkBody)> = self
            .objects
            .iter()
            .filter_map(|object| Some((object, object.as_bookmark()?)))
            .filter(|(_, body)| !body.label.trim().is_empty())
            .collect();
        found.sort_by(|(a, _), (b, _)| {
            let (a, b) = (a.page_bounds(), b.page_bounds());
            a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0]))
        });
        found
    }

    /// [`Note::bookmark_objects`] as the listing gives them: the position is the centre of the
    /// bookmark's box on the page, in device units, where the editor goes to show it.
    pub fn bookmarks(&self) -> Vec<BookmarkRef> {
        self.bookmark_objects()
            .into_iter()
            .map(|(object, body)| {
                let [x, y] = object.page_centre();
                BookmarkRef {
                    id: object.id.clone(),
                    label: body.label.trim().to_owned(),
                    x: x.round() as i32,
                    y: y.round() as i32,
                }
            })
            .collect()
    }

    /// What the library search reads of the page's typed text: the text of the frames and the
    /// LaTeX source of the equations of the visible layers, in reading order, joined by a line
    /// break (an equation reads as its source: `\ce{H2O}` is found by « H2O »); one with nothing
    /// but white space says nothing. Empty when the page has neither. Hidden layers are left out
    /// like they are for the recognised handwriting: the search finds what the page shows.
    pub fn typed_text(&self) -> String {
        let mut typed: Vec<(&PageObject, &str)> = self
            .objects
            .iter()
            .filter(|object| self.layer_visible(object.layer))
            .filter_map(|object| match &object.body {
                ObjectBody::Text(body) => Some((object, body.text.as_str())),
                ObjectBody::Equation(body) => Some((object, body.latex.as_str())),
                // A bookmark is searched apart (the binder's summary): not typed text.
                ObjectBody::Bookmark(_) | ObjectBody::Unknown { .. } => None,
            })
            .filter(|(_, text)| !text.trim().is_empty())
            .collect();
        typed.sort_by(|(a, _), (b, _)| reading_order(a, b));
        typed
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Top to bottom, then left to right, by the top-left corner of the objects' boxes on the page.
fn reading_order(a: &PageObject, b: &PageObject) -> std::cmp::Ordering {
    let (a, b) = (a.page_bounds(), b.page_bounds());
    a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_is_fitted_into_the_page() {
        // A portrait A4 sheet scanned at 100 dpi on the landscape page.
        let rect = ImageRect::fit(A4_LANDSCAPE, (827, 1169), ImageFit::Contain);
        assert!((rect.height - 21_600.0).abs() < 0.01, "{rect:?}");
        assert!((rect.x - (29_700.0 - rect.width) / 2.0).abs() < 0.01);
        assert_eq!(rect.y, 0.0);
        let rect = ImageRect::fit(A4_LANDSCAPE, (827, 1169), ImageFit::Cover);
        assert!((rect.width - 29_700.0).abs() < 0.01, "{rect:?}");
        assert!(rect.y < 0.0 && rect.height > 21_600.0);
        let whole = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 29_700.0,
            height: 21_600.0,
        };
        assert_eq!(
            ImageRect::fit(A4_LANDSCAPE, (827, 1169), ImageFit::Stretch),
            whole
        );
        assert_eq!(
            ImageRect::fit(A4_LANDSCAPE, (0, 10), ImageFit::Contain),
            whole
        );
    }

    fn frame(id: &str, layer: u32, x: f32, y: f32, text: &str) -> PageObject {
        PageObject::text(
            id,
            layer,
            0,
            x,
            y,
            TextBody {
                text: text.to_owned(),
                font: ardoise_text::FontFamily::LatinModern,
                bold: false,
                size: 600.0,
                color: Rgba::BLACK,
            },
        )
    }

    #[test]
    fn typed_text_is_read_in_reading_order_on_visible_layers_only() {
        let mut note = crate::testutil::blank_note("n", "Texte");
        note.layers.push(Layer {
            name: "Cachée".into(),
            visible: false,
        });
        note.objects = vec![
            frame("low", 0, 100.0, 5_000.0, "Bas"),
            frame("right", 0, 9_000.0, 1_000.0, "Droite"),
            frame("left", 0, 100.0, 1_000.0, "Gauche\nsuite"),
            frame("hidden", 1, 100.0, 3_000.0, "Secret"),
            frame("blank", 0, 100.0, 4_000.0, "  \n "),
        ];
        assert_eq!(note.typed_text(), "Gauche\nsuite\nDroite\nBas");
        // Every frame, every layer, in the same order, for the callers that choose.
        let ids: Vec<&str> = note
            .text_frames()
            .iter()
            .map(|(object, _)| object.id.as_str())
            .collect();
        assert_eq!(ids, ["left", "right", "hidden", "blank", "low"]);
        assert!(note.layer_visible(0) && !note.layer_visible(1) && !note.layer_visible(2));
        assert_eq!(crate::testutil::blank_note("m", "").typed_text(), "");
    }

    fn bookmark(id: &str, layer: u32, x: f32, y: f32, label: &str) -> PageObject {
        PageObject::bookmark(id, layer, 0, x, y, label)
    }

    #[test]
    fn bookmarks_are_listed_in_reading_order_whatever_the_layer_shows() {
        let mut note = crate::testutil::blank_note("n", "Signets");
        note.layers.push(Layer {
            name: "Cachée".into(),
            visible: false,
        });
        note.objects = vec![
            bookmark("low", 0, 100.0, 5_000.0, "Bas"),
            bookmark("right", 0, 9_000.0, 1_000.0, "  Droite "),
            bookmark("left", 0, 100.0, 1_000.0, "Gauche"),
            bookmark("hidden", 1, 100.0, 3_000.0, "Calque caché"),
            bookmark("blank", 0, 100.0, 4_000.0, "   "),
            frame("text", 0, 100.0, 100.0, "Pas un signet"),
        ];
        let refs = note.bookmarks();
        let labels: Vec<&str> = refs.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Gauche", "Droite", "Calque caché", "Bas"]);
        assert_eq!(refs[0].id, "left");
        // The spot is the centre of the box, rounded.
        let [x, y] = note.objects[2].page_centre();
        assert_eq!((refs[0].x, refs[0].y), (x.round() as i32, y.round() as i32));
        // Bookmarks are not typed text, and make no content to draw.
        assert_eq!(note.typed_text(), "Pas un signet");
        let mut only = crate::testutil::blank_note("m", "");
        assert!(!only.has_content());
        only.objects = vec![bookmark("a", 0, 0.0, 0.0, "Seul")];
        assert!(!only.has_content());
        only.objects.push(frame("t", 0, 0.0, 0.0, "x"));
        assert!(only.has_content());
    }

    #[test]
    fn equations_read_as_their_source_among_the_typed_text() {
        let equation = |id: &str, layer: u32, x: f32, y: f32, latex: &str| {
            PageObject::equation(
                id,
                layer,
                0,
                [x, y],
                0.6,
                EquationBody {
                    latex: latex.to_owned(),
                    path: "M0 0L100 0L100 100Z".to_owned(),
                    color: Rgba::BLACK,
                },
            )
        };
        let mut note = crate::testutil::blank_note("n", "Chimie");
        note.layers.push(Layer {
            name: "Cachée".into(),
            visible: false,
        });
        note.objects = vec![
            frame("title", 0, 100.0, 500.0, "Combustion"),
            equation("ce", 0, 100.0, 2_000.0, r"\ce{CH4 + 2O2 -> CO2 + 2H2O}"),
            equation("hidden", 1, 100.0, 1_000.0, r"\ce{H2}"),
            equation("blank", 0, 100.0, 3_000.0, " "),
            equation("first", 0, 5_000.0, 500.0, "E = mc^2"),
        ];
        assert_eq!(
            note.typed_text(),
            "Combustion\nE = mc^2\n\\ce{CH4 + 2O2 -> CO2 + 2H2O}"
        );
        let ids: Vec<&str> = note
            .equations()
            .iter()
            .map(|(object, _)| object.id.as_str())
            .collect();
        assert_eq!(ids, ["first", "hidden", "ce", "blank"]);
        // The text frames alone stay what they were.
        assert_eq!(note.text_frames().len(), 1);
    }

    #[test]
    fn a_background_image_reads_with_its_defaults() {
        let image: BackgroundImage = serde_json::from_str(
            r#"{"name": "fiche.png", "rect": {"x": 0, "y": 0, "width": 10, "height": 20}}"#,
        )
        .unwrap();
        assert_eq!(
            (image.pdf_page, image.quarter_turns, image.opacity),
            (None, 0, 1.0)
        );
        let json = serde_json::to_value(&image).unwrap();
        assert!(json.get("pdfPage").is_none(), "{json}");
    }
}
