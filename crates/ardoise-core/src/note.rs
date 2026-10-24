//! The note model: what one `.ardoise` file holds (SPEC §3.3).
//!
//! Strokes are in page coordinates, device units (10 µm), origin top-left of the page as
//! displayed (see `ardoise_ink::model`). Rotating a page by a quarter turn rewrites the
//! points and swaps the page size; the tablet's raw data stays untouched in `original.bin`.

use ardoise_ink::{Rgba, Stroke};

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
    pub tags: Vec<String>,
    /// At least one layer.
    pub layers: Vec<Layer>,
    pub strokes: Vec<Stroke>,
    pub timing: Timing,
    pub origin: Origin,
}
