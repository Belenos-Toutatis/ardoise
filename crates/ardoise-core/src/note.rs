//! The note model: what one `.ardoise` file holds (SPEC §3.3).
//!
//! Strokes are in page coordinates, device units (10 µm), origin top-left of the page as
//! displayed (see `ardoise_ink::model`). Rotating a page by a quarter turn rewrites the
//! points and swaps the page size; the tablet's raw data stays untouched in `original.bin`
//! (or, for a note that does not come from the tablet's memory, the note as it was before its
//! first edit stays in `original.ardoise`: "back to the original").

use ardoise_ink::{Rgba, Stroke};

pub use crate::objects::{ObjectBody, PageObject, TextBody};

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
    /// Whether the page holds anything to draw: a stroke with a sample, or an object.
    pub fn has_content(&self) -> bool {
        !self.objects.is_empty() || self.strokes.iter().any(|s| !s.points.is_empty())
    }
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
