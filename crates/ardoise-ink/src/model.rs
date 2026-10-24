//! The stroke model shared by notes, rendering, import and (later) the live display.
//!
//! Coordinates are **page coordinates in device units** (10 µm, the Bamboo Slate's unit),
//! origin at the top-left corner of the page as displayed, y pointing down. They are `f32`
//! so that transformed or imported points keep sub-unit precision; quarter-turn rotations
//! stay exact. Pressure is normalised to 0..=1. Time is in milliseconds since the note's
//! time origin ([`Point::t`]).

/// An sRGB colour with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// `0xRRGGBBAA`, as Inkspace stores colours.
    pub const fn from_rgba_u32(value: u32) -> Self {
        let [r, g, b, a] = value.to_be_bytes();
        Self { r, g, b, a }
    }

    pub const fn to_rgba_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }
}

/// Ink tools (SPEC §4.4). Each has its own width response to pressure and its own look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    /// Ballpoint pen: the tablet's own pen and Inkspace's only tool.
    Ballpoint,
    FountainPen,
    FeltTip,
    /// Pencil, with a grainy texture.
    Pencil,
    Brush,
    /// Highlighter: wide, translucent, flat ends (P2).
    Highlighter,
    /// Chalk: for the blackboard background (P2).
    Chalk,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Ballpoint,
        Tool::FountainPen,
        Tool::FeltTip,
        Tool::Pencil,
        Tool::Brush,
        Tool::Highlighter,
        Tool::Chalk,
    ];
}

/// One pen sample.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    /// Page coordinates in device units (10 µm).
    pub x: f32,
    pub y: f32,
    /// 0 = lightest touch, 1 = full pressure.
    pub pressure: f32,
    /// Milliseconds since the note's time origin (`Note::created`).
    pub t: u32,
}

/// One continuous pen-down trace.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stroke {
    pub tool: Tool,
    pub color: Rgba,
    /// Nominal diameter at full pressure, in device units. The tool turns it into a width
    /// per point (see `Tool::width_at` in the geometry module).
    pub width: f32,
    /// Index into the note's layers.
    pub layer: u32,
    pub points: Vec<Point>,
}
