//! Page objects: what a page holds besides its strokes (format 1.4; design and how to add a kind:
//! `docs/design/page-objects.md`; bytes: `docs/file-format.md` §2.2).
//!
//! Every object has the same **placement** ([`PageObject`]'s own fields), which is all that page
//! operations need: a page turned, cropped, resized, merged or split moves an object through its
//! placement alone, whatever its kind. What it shows is its [`ObjectBody`], drawn in the object's
//! own coordinates (origin at the top-left corner of its content, y down) and placed on the page by
//! [`PageObject::page_point`]. A kind this version does not know is kept with all its keys
//! ([`ObjectBody::Unknown`]), moved with the page like the others, and drawn nowhere.
//!
//! Kinds today: `text` ([`TextBody`], SPEC §4.4 « Texte tapé au clavier »), `equation`
//! ([`EquationBody`], format 1.5, SPEC §4.4 « Équation », `docs/design/equations.md`) and
//! `bookmark` ([`BookmarkBody`], SPEC §4.4 « Signets », format 1.6): a short label drawn in the
//! editor only.

use ardoise_ink::Rgba;
use ardoise_text::{FontFamily, Layout, Path, TextStyle};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// The kind of a text frame (`"kind": "text"`).
pub const TEXT_KIND: &str = "text";

/// The kind of a bookmark (`"kind": "bookmark"`).
pub const BOOKMARK_KIND: &str = "bookmark";

/// Keys of the placement, common to every kind.
const PLACEMENT_KEYS: [&str; 10] = [
    "kind",
    "id",
    "layer",
    "t",
    "x",
    "y",
    "scale",
    "quarterTurns",
    "width",
    "height",
];

/// Keys of a text frame's body.
const TEXT_KEYS: [&str; 5] = ["text", "font", "bold", "size", "color"];

/// Keys of a bookmark's body.
const BOOKMARK_KEYS: [&str; 1] = ["label"];

/// How many characters a bookmark's label may have (the editor's field stops there; a longer label read
/// from a file is kept and drawn whole).
pub const MAX_BOOKMARK_LABEL: usize = 60;

/// Size of the font of a bookmark's label (its em), device units: 5 mm. A bookmark keeps this size on the
/// page, like a text frame: it is a small label in the margin.
pub const BOOKMARK_FONT_SIZE: f32 = 500.0;
/// Room around a bookmark's content, device units.
pub const BOOKMARK_PAD: f32 = 150.0;
/// Width of the flag at the left of the label, device units; the label starts after it and one `PAD`.
pub const BOOKMARK_FLAG_WIDTH: f32 = 300.0;

/// Sizes the text tool offers, in device units (the size of the font, its em): 4, 6 and 9 mm.
pub const TEXT_SIZES: [f32; 3] = [400.0, 600.0, 900.0];

/// The kind of an equation (`"kind": "equation"`, format 1.5).
pub const EQUATION_KIND: &str = "equation";

/// Keys of an equation's body.
const EQUATION_KEYS: [&str; 3] = ["latex", "path", "color"];

/// Units of an equation's content per em of its type (MathJax's): the placement's `scale` is the
/// em on the page divided by this (0.6 for the 6 mm of the tool's middle size).
pub const EQUATION_UNITS_PER_EM: f32 = 1000.0;

/// Longest LaTeX source of an equation, in characters (Unicode scalar values). A system of a few
/// lines is a few hundred; beyond, the body is not read (kept, not drawn).
pub const EQUATION_MAX_SOURCE_CHARS: usize = 4000;

/// Largest path data of an equation, bytes. A line of chemistry is about 10 KB; the editor refuses
/// to put down an equation whose drawing is larger, and a reader keeps one it finds without
/// drawing it (so that `note.json`, at most 16 MiB, holds dozens of the largest).
pub const EQUATION_MAX_PATH_BYTES: usize = 256 * 1024;

/// Largest coordinate of an equation's path, in its units (a thousand ems).
pub const EQUATION_MAX_COORDINATE: f32 = 1.0e6;

/// An equation (SPEC §4.4 « Équation »): the LaTeX source the user typed, which the editor turns
/// into a drawing with MathJax, and that drawing as filled outlines, so that every output draws it
/// without MathJax. Its content's units are 1/1000 em ([`EQUATION_UNITS_PER_EM`]), y down, origin
/// at the top-left corner of the drawing's box.
#[derive(Debug, Clone, PartialEq)]
pub struct EquationBody {
    /// The source, in TeX's math mode (chemistry with `\ce{…}`); never read by Rust but to keep it.
    pub latex: String,
    /// SVG path data: absolute `M`, `L`, `Q`, `C`, `Z` ([`Path::from_svg`]), to fill with the
    /// non-zero rule.
    pub path: String,
    pub color: Rgba,
}

impl EquationBody {
    /// The drawing, in the object's units; empty when the path data cannot be read (a body read
    /// from a file always can: one that cannot is not an equation to this version).
    pub fn outline(&self) -> Path {
        read_equation_path(&self.path).unwrap_or_default()
    }

    /// Whether this version reads the body: a source and a path within the limits, the path in
    /// the grammar.
    pub fn is_readable(&self) -> bool {
        self.latex.chars().count() <= EQUATION_MAX_SOURCE_CHARS
            && read_equation_path(&self.path).is_ok()
    }
}

/// An equation's path data read with its limits.
fn read_equation_path(data: &str) -> Result<Path, ardoise_text::PathDataError> {
    Path::from_svg(data, EQUATION_MAX_PATH_BYTES, EQUATION_MAX_COORDINATE)
}

/// A text frame: one line of the page per line of `text`, in one style (SPEC §4.4: two families,
/// three sizes, a colour, bold or not, no formatting inside a frame).
#[derive(Debug, Clone, PartialEq)]
pub struct TextBody {
    pub text: String,
    pub font: FontFamily,
    pub bold: bool,
    /// Size of the font (its em), in the object's units: device units at scale 1.
    pub size: f32,
    pub color: Rgba,
}

impl TextBody {
    pub fn style(&self) -> TextStyle {
        TextStyle {
            family: self.font,
            bold: self.bold,
            size: self.size,
        }
    }

    /// The frame's lines, in the object's units.
    pub fn layout(&self) -> Layout {
        ardoise_text::layout(&self.text, self.style())
    }
}

/// A bookmark: a word or a short phrase, one line, at a spot of the page (SPEC §4.4 « Signets »).
/// Drawn in the editor, with a flag, and nowhere else: not in the exports, nor the replay, nor the
/// projection or the share. It makes the binder's summary and is found by the library's search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkBody {
    pub label: String,
}

impl BookmarkBody {
    /// The label's text style: Andika, regular, [`BOOKMARK_FONT_SIZE`].
    pub fn style() -> TextStyle {
        TextStyle {
            family: FontFamily::Andika,
            bold: false,
            size: BOOKMARK_FONT_SIZE,
        }
    }

    /// The label's line, in its own units (the glyph path is this layout's).
    pub fn layout(&self) -> Layout {
        ardoise_text::layout(&self.label, Self::style())
    }

    /// The box of the bookmark's content (the flag, the label and room around them), device units.
    pub fn content_box(&self) -> (f32, f32) {
        let laid = self.layout();
        (
            BOOKMARK_PAD + BOOKMARK_FLAG_WIDTH + BOOKMARK_PAD + laid.width + BOOKMARK_PAD,
            laid.height.max(laid.line_height) + 2.0 * BOOKMARK_PAD,
        )
    }
}

/// A label as a bookmark keeps it: one line (line breaks and tabs become spaces, runs of white space
/// one space), no white space at either end, at most [`MAX_BOOKMARK_LABEL`] characters. Empty when
/// there is nothing left: the bookmark is then removed.
pub fn clean_bookmark_label(label: &str) -> String {
    let one_line = label.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line
        .chars()
        .take(MAX_BOOKMARK_LABEL)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// What an object shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectBody {
    Text(TextBody),
    Equation(EquationBody),
    Bookmark(BookmarkBody),
    /// A kind this version does not know (or an object of a known kind it cannot read: a value it
    /// does not know, a wrong type): every key but the placement's, kept verbatim and written back.
    Unknown {
        kind: String,
        fields: Map<String, Value>,
    },
}

impl ObjectBody {
    /// The `kind` written in the file.
    pub fn kind(&self) -> &str {
        match self {
            ObjectBody::Text(_) => TEXT_KIND,
            ObjectBody::Equation(_) => EQUATION_KIND,
            ObjectBody::Bookmark(_) => BOOKMARK_KIND,
            ObjectBody::Unknown { kind, .. } => kind,
        }
    }

    /// What every output draws of the body, in the object's own units, and its colour: a text
    /// frame's glyphs, an equation's drawing; `None` for a kind no output draws (a bookmark: the
    /// editor's only) or this version does not know.
    pub fn outline(&self) -> Option<(Path, Rgba)> {
        match self {
            ObjectBody::Text(body) => Some((ardoise_text::outline(&body.layout()), body.color)),
            ObjectBody::Equation(body) => Some((body.outline(), body.color)),
            ObjectBody::Bookmark(_) | ObjectBody::Unknown { .. } => None,
        }
    }

    /// Whether every output draws the body: a text frame or an equation this version reads (never
    /// a bookmark, which only the editor shows).
    pub fn is_drawn(&self) -> bool {
        matches!(self, ObjectBody::Text(_) | ObjectBody::Equation(_))
    }
}

/// Something on the page that is not a stroke.
///
/// Placement: the point `(u, v)` of the object's content (its units, y down) is on the page at
/// `(x, y) + turn(scale × (u, v))`, `turn` being `quarter_turns` clockwise quarter turns. The content
/// fills `[0, width] × [0, height]`.
#[derive(Debug, Clone, PartialEq)]
pub struct PageObject {
    /// Unique within the note; kept by every operation that keeps the object.
    pub id: String,
    /// Index into the note's layers: objects are drawn with their layer, above its strokes.
    pub layer: u32,
    /// Milliseconds since the note's `created`: when the replay shows it (all at once).
    pub t: u32,
    /// Where the content's origin (its top-left corner before turning) is on the page.
    pub x: f32,
    pub y: f32,
    /// Page units per unit of the content; greater than zero.
    pub scale: f32,
    /// Clockwise quarter turns of the content about its origin, 0 to 3.
    pub quarter_turns: u8,
    /// Size of the content, in its own units (before scale and turns).
    pub width: f32,
    pub height: f32,
    pub body: ObjectBody,
    /// Keys of the object this version does not know (added by a newer minor version), kept verbatim.
    pub extra: Map<String, Value>,
}

/// An affine map of the page `p' = (a·x + c·y + e, b·x + d·y + f)`, as the editor writes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translation(dx: f32, dy: f32) -> Affine {
        Affine {
            e: dx,
            f: dy,
            ..Affine::IDENTITY
        }
    }

    /// Uniform scaling by `k` around `(ox, oy)`, then a move by `(dx, dy)`.
    pub fn scaling(k: f32, ox: f32, oy: f32, dx: f32, dy: f32) -> Affine {
        Affine {
            a: k,
            b: 0.0,
            c: 0.0,
            d: k,
            e: ox * (1.0 - k) + dx,
            f: oy * (1.0 - k) + dy,
        }
    }

    /// A clockwise quarter turn of a page `height` high: `(x, y) → (height − y, x)`.
    pub fn quarter_cw(height: f32) -> Affine {
        Affine {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: height,
            f: 0.0,
        }
    }

    /// A counter-clockwise quarter turn of a page `width` wide: `(x, y) → (y, width − x)`.
    pub fn quarter_ccw(width: f32) -> Affine {
        Affine {
            a: 0.0,
            b: -1.0,
            c: 1.0,
            d: 0.0,
            e: 0.0,
            f: width,
        }
    }

    pub fn apply(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        ]
    }

    /// Factor by which lengths change (square root of the determinant).
    pub fn length_scale(&self) -> f32 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }

    /// The clockwise quarter turns the map makes, rounded to the nearest one.
    pub fn quarter_turns(&self) -> u8 {
        let angle = self.b.atan2(self.a);
        let turns = (angle / std::f32::consts::FRAC_PI_2).round() as i32;
        turns.rem_euclid(4) as u8
    }
}

impl PageObject {
    /// A text frame at `(x, y)`, its box measured.
    pub fn text(id: impl Into<String>, layer: u32, t: u32, x: f32, y: f32, body: TextBody) -> Self {
        let mut object = PageObject {
            id: id.into(),
            layer,
            t,
            x,
            y,
            scale: 1.0,
            quarter_turns: 0,
            width: 0.0,
            height: 0.0,
            body: ObjectBody::Text(body),
            extra: Map::new(),
        };
        object.measure();
        object
    }

    /// An equation at `(x, y)` (the top-left corner of its drawing), `scale` page units per unit
    /// of its content ([`EQUATION_UNITS_PER_EM`]), its box measured.
    pub fn equation(
        id: impl Into<String>,
        layer: u32,
        t: u32,
        [x, y]: [f32; 2],
        scale: f32,
        body: EquationBody,
    ) -> Self {
        let mut object = PageObject {
            id: id.into(),
            layer,
            t,
            x,
            y,
            scale,
            quarter_turns: 0,
            width: 0.0,
            height: 0.0,
            body: ObjectBody::Equation(body),
            extra: Map::new(),
        };
        object.measure();
        object
    }

    /// A bookmark with its top-left corner at `(x, y)`, its box measured. The label is kept as given:
    /// callers clean it with [`clean_bookmark_label`].
    pub fn bookmark(
        id: impl Into<String>,
        layer: u32,
        t: u32,
        x: f32,
        y: f32,
        label: impl Into<String>,
    ) -> Self {
        let mut object = PageObject {
            id: id.into(),
            layer,
            t,
            x,
            y,
            scale: 1.0,
            quarter_turns: 0,
            width: 0.0,
            height: 0.0,
            body: ObjectBody::Bookmark(BookmarkBody {
                label: label.into(),
            }),
            extra: Map::new(),
        };
        object.measure();
        object
    }

    /// The text of a text frame.
    pub fn as_text(&self) -> Option<&TextBody> {
        match &self.body {
            ObjectBody::Text(body) => Some(body),
            _ => None,
        }
    }

    /// The source and drawing of an equation.
    pub fn as_equation(&self) -> Option<&EquationBody> {
        match &self.body {
            ObjectBody::Equation(body) => Some(body),
            _ => None,
        }
    }

    /// The label of a bookmark.
    pub fn as_bookmark(&self) -> Option<&BookmarkBody> {
        match &self.body {
            ObjectBody::Bookmark(body) => Some(body),
            _ => None,
        }
    }

    /// Whether the object is a bookmark: a kind only the editor shows, which the clocks of the replay
    /// and of the writing do not count.
    pub fn is_bookmark(&self) -> bool {
        matches!(self.body, ObjectBody::Bookmark(_))
    }

    /// Sets `width` and `height` from the content, for the kinds this version can measure (the
    /// others keep the box they were written with): a text frame's lines; an equation's drawing,
    /// from its origin to the farthest point of its outline (control points included, as the
    /// editor measures it).
    pub fn measure(&mut self) {
        match &self.body {
            ObjectBody::Text(body) => {
                let laid = body.layout();
                self.width = laid.width;
                self.height = laid.height;
            }
            ObjectBody::Equation(body) => {
                let [_, _, right, bottom] = body.outline().bounds().unwrap_or([0.0; 4]);
                self.width = right.max(0.0);
                self.height = bottom.max(0.0);
            }
            ObjectBody::Bookmark(body) => (self.width, self.height) = body.content_box(),
            ObjectBody::Unknown { .. } => {}
        }
    }

    /// Where the content's point `(u, v)` is on the page.
    pub fn page_point(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let (u, v) = (u * self.scale, v * self.scale);
        let (dx, dy) = match self.quarter_turns % 4 {
            0 => (u, v),
            1 => (-v, u),
            2 => (-u, -v),
            _ => (v, -u),
        };
        [self.x + dx, self.y + dy]
    }

    /// The box `[min x, min y, max x, max y]` of the content on the page.
    pub fn page_bounds(&self) -> [f32; 4] {
        let corners = [
            [0.0, 0.0],
            [self.width, 0.0],
            [self.width, self.height],
            [0.0, self.height],
        ]
        .map(|p| self.page_point(p));
        corners.iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            },
        )
    }

    /// The centre of the content's box on the page (what a lasso or a zone must hold).
    pub fn page_centre(&self) -> [f32; 2] {
        self.page_point([self.width / 2.0, self.height / 2.0])
    }

    /// Moves the object with the page under `m` (a move, a uniform change of scale, quarter
    /// turns): its origin follows `m`, its scale and turns take `m`'s.
    pub fn transform(&mut self, m: &Affine) {
        let [x, y] = m.apply([self.x, self.y]);
        self.x = x;
        self.y = y;
        let factor = m.length_scale();
        if factor.is_finite() && factor > 0.0 {
            self.scale *= factor;
        }
        self.quarter_turns = (self.quarter_turns + m.quarter_turns()) % 4;
    }
}

/// `objects` with `m` applied to each (page operations).
pub fn transform_all(objects: &mut [PageObject], m: &Affine) {
    for object in objects {
        object.transform(m);
    }
}

/// Gives a fresh id to every object whose id is empty or already taken by an earlier one (two
/// copies of a note merged, a hand-edited file).
pub fn make_ids_unique(objects: &mut [PageObject]) {
    let mut seen = std::collections::HashSet::new();
    for index in 0..objects.len() {
        if objects[index].id.is_empty() || seen.contains(&objects[index].id) {
            let base = if objects[index].id.is_empty() {
                "object".to_owned()
            } else {
                objects[index].id.clone()
            };
            let mut n = 2usize;
            let mut id = format!("{base}-{n}");
            while seen.contains(&id) || objects.iter().any(|o| o.id == id) {
                n += 1;
                id = format!("{base}-{n}");
            }
            objects[index].id = id;
        }
        seen.insert(objects[index].id.clone());
    }
}

// ---------------------------------------------------------------------------------------
// JSON: one flat object per page object (`docs/file-format.md` §2.2).

/// `value` as a JSON number written with the fewest digits that read back as the same `f32`
/// (`4363.2`, not `4363.2001953125`).
fn number(value: f32) -> Value {
    let shortest = value.to_string().parse::<f64>().unwrap_or(f64::from(value));
    serde_json::Number::from_f64(shortest).map_or(Value::Null, Value::Number)
}

impl Serialize for PageObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = self.extra.clone();
        if let ObjectBody::Unknown { fields, .. } = &self.body {
            map.extend(fields.clone());
        }
        map.insert("kind".into(), Value::String(self.body.kind().to_owned()));
        map.insert("id".into(), Value::String(self.id.clone()));
        map.insert("layer".into(), Value::from(self.layer));
        map.insert("t".into(), Value::from(self.t));
        map.insert("x".into(), number(self.x));
        map.insert("y".into(), number(self.y));
        map.insert("scale".into(), number(self.scale));
        map.insert("quarterTurns".into(), Value::from(self.quarter_turns % 4));
        map.insert("width".into(), number(self.width));
        map.insert("height".into(), number(self.height));
        let color = |c: Rgba| serde_json::to_value(c).map_err(serde::ser::Error::custom);
        match &self.body {
            ObjectBody::Text(body) => {
                map.insert("text".into(), Value::String(body.text.clone()));
                map.insert(
                    "font".into(),
                    serde_json::to_value(body.font).map_err(serde::ser::Error::custom)?,
                );
                map.insert("bold".into(), Value::Bool(body.bold));
                map.insert("size".into(), number(body.size));
                map.insert("color".into(), color(body.color)?);
            }
            ObjectBody::Equation(body) => {
                map.insert("latex".into(), Value::String(body.latex.clone()));
                map.insert("path".into(), Value::String(body.path.clone()));
                map.insert("color".into(), color(body.color)?);
            }
            ObjectBody::Bookmark(body) => {
                map.insert("label".into(), Value::String(body.label.clone()));
            }
            ObjectBody::Unknown { .. } => {}
        }
        map.serialize(serializer)
    }
}

/// Why a page object's placement cannot be read.
fn bad(what: &str) -> String {
    format!("page object: {what}")
}

fn take_f32(map: &mut Map<String, Value>, key: &str, default: f32) -> Result<f32, String> {
    match map.remove(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .map(|v| v as f32)
            .filter(|v| v.is_finite())
            .ok_or_else(|| bad(&format!("{key} is not a number"))),
    }
}

fn take_u32(map: &mut Map<String, Value>, key: &str) -> Result<u32, String> {
    match map.remove(key) {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| bad(&format!("{key} is not a whole number"))),
    }
}

/// A text frame's body from its keys, or `None` when this version cannot read it.
fn text_body(map: &Map<String, Value>) -> Option<TextBody> {
    let get = |key: &str| map.get(key);
    Some(TextBody {
        text: match get("text") {
            None => String::new(),
            Some(value) => value.as_str()?.to_owned(),
        },
        font: match get("font") {
            None => FontFamily::LatinModern,
            Some(value) => serde_json::from_value(value.clone()).ok()?,
        },
        bold: match get("bold") {
            None => false,
            Some(value) => value.as_bool()?,
        },
        size: match get("size") {
            None => TEXT_SIZES[1],
            Some(value) => {
                let size = value.as_f64()? as f32;
                (size.is_finite() && size > 0.0).then_some(size)?
            }
        },
        color: match get("color") {
            None => Rgba::BLACK,
            Some(value) => serde_json::from_value(value.clone()).ok()?,
        },
    })
}

/// An equation's body from its keys, or `None` when this version cannot read it: a source or a
/// path that is not a string, beyond its limit, or path data outside the grammar (a hostile or
/// damaged note: kept as it is, drawn nowhere).
fn equation_body(map: &Map<String, Value>) -> Option<EquationBody> {
    let body = EquationBody {
        latex: map.get("latex")?.as_str()?.to_owned(),
        path: map.get("path")?.as_str()?.to_owned(),
        // Required, as the interface requires it (`isEquationObject`): both read the same objects.
        color: serde_json::from_value(map.get("color")?.clone()).ok()?,
    };
    body.is_readable().then_some(body)
}

/// A bookmark's body from its keys, or `None` when this version cannot read it.
fn bookmark_body(map: &Map<String, Value>) -> Option<BookmarkBody> {
    Some(BookmarkBody {
        label: match map.get("label") {
            None => String::new(),
            Some(value) => value.as_str()?.to_owned(),
        },
    })
}

impl PageObject {
    /// Reads one object. An error means its placement cannot be read (the caller then keeps the
    /// whole list verbatim, see [`deserialize_objects`]); a body that cannot be read makes an
    /// [`ObjectBody::Unknown`].
    pub fn from_value(value: Value) -> Result<PageObject, String> {
        let Value::Object(mut map) = value else {
            return Err(bad("not a JSON object"));
        };
        let kind = match map.remove("kind") {
            Some(Value::String(kind)) if !kind.is_empty() => kind,
            _ => return Err(bad("no kind")),
        };
        let id = match map.remove("id") {
            None => String::new(),
            Some(Value::String(id)) => id,
            Some(_) => return Err(bad("id is not a string")),
        };
        let layer = take_u32(&mut map, "layer")?;
        let t = take_u32(&mut map, "t")?;
        let x = take_f32(&mut map, "x", 0.0)?;
        let y = take_f32(&mut map, "y", 0.0)?;
        let scale = take_f32(&mut map, "scale", 1.0)?;
        if scale <= 0.0 {
            return Err(bad("scale is not positive"));
        }
        let quarter_turns = (take_u32(&mut map, "quarterTurns")? % 4) as u8;
        let width = take_f32(&mut map, "width", 0.0)?.max(0.0);
        let height = take_f32(&mut map, "height", 0.0)?.max(0.0);
        let known = match kind.as_str() {
            TEXT_KIND => text_body(&map).map(|body| {
                for key in TEXT_KEYS {
                    map.remove(key);
                }
                ObjectBody::Text(body)
            }),
            EQUATION_KIND => equation_body(&map).map(|body| {
                for key in EQUATION_KEYS {
                    map.remove(key);
                }
                ObjectBody::Equation(body)
            }),
            BOOKMARK_KIND => bookmark_body(&map).map(|body| {
                for key in BOOKMARK_KEYS {
                    map.remove(key);
                }
                ObjectBody::Bookmark(body)
            }),
            _ => None,
        };
        let body = known.unwrap_or_else(|| ObjectBody::Unknown {
            kind,
            fields: std::mem::take(&mut map),
        });
        Ok(PageObject {
            id,
            layer,
            t,
            x,
            y,
            scale,
            quarter_turns,
            width,
            height,
            body,
            extra: map,
        })
    }
}

impl<'de> Deserialize<'de> for PageObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        PageObject::from_value(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// `Note::objects` from `note.json`: a list whose objects can all be placed, or an empty list. In
/// the second case nothing is lost: the reader keeps the key it could not read verbatim, as an
/// unknown key (`format.rs`), and writes it back unless the note is given objects meanwhile.
pub fn deserialize_objects<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PageObject>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let Value::Array(items) = value else {
        return Ok(Vec::new());
    };
    let objects: Result<Vec<PageObject>, String> =
        items.into_iter().map(PageObject::from_value).collect();
    Ok(objects.unwrap_or_default())
}

/// Whether `key` is one of the keys a page object of `kind` owns (the placement's and the
/// body's), for the tests of the format.
pub fn is_known_key(kind: &str, key: &str) -> bool {
    PLACEMENT_KEYS.contains(&key)
        || (kind == TEXT_KIND && TEXT_KEYS.contains(&key))
        || (kind == EQUATION_KIND && EQUATION_KEYS.contains(&key))
        || (kind == BOOKMARK_KIND && BOOKMARK_KEYS.contains(&key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hello() -> TextBody {
        TextBody {
            text: "Bonjour".into(),
            font: FontFamily::Andika,
            bold: true,
            size: 600.0,
            color: Rgba::rgb(200, 0, 0),
        }
    }

    #[test]
    fn a_text_frame_round_trips_with_its_unknown_keys() {
        let mut object = PageObject::text("a", 1, 50, 100.0, 200.0, hello());
        object.extra.insert("future".into(), json!({"x": 1}));
        let value = serde_json::to_value(&object).unwrap();
        assert_eq!(value["kind"], "text");
        assert_eq!(value["font"], "andika");
        assert_eq!(value["future"], json!({"x": 1}));
        assert!(value["width"].as_f64().unwrap() > 0.0);
        let back: PageObject = serde_json::from_value(value).unwrap();
        assert_eq!(back, object);
    }

    #[test]
    fn an_unknown_kind_keeps_every_key() {
        let value = json!({
            "kind": "stamp", "id": "e", "layer": 0, "t": 3, "x": 10, "y": 20, "scale": 2,
            "quarterTurns": 5, "width": 30, "height": 40, "label": "x^2", "paths": ["M0 0Z"]
        });
        let object: PageObject = serde_json::from_value(value).unwrap();
        assert_eq!(object.body.kind(), "stamp");
        assert!(!object.body.is_drawn() && object.body.outline().is_none());
        assert_eq!(object.quarter_turns, 1);
        assert_eq!(object.scale, 2.0);
        let back = serde_json::to_value(&object).unwrap();
        assert_eq!(back["label"], "x^2");
        assert_eq!(back["paths"], json!(["M0 0Z"]));
        assert_eq!(back["quarterTurns"], 1);
        assert_eq!(back["x"], 10.0);
    }

    fn fraction() -> EquationBody {
        // A bar and a small triangle, in MathJax units (1/1000 em).
        EquationBody {
            latex: r"\frac{1}{2}".into(),
            path: "M0 220L729 220L729 280L0 280ZM100 0L300 0Q350 100 200 150Z".into(),
            color: Rgba::rgb(0, 0, 200),
        }
    }

    #[test]
    fn an_equation_round_trips_and_is_measured_from_its_drawing() {
        let mut object = PageObject::equation("e", 0, 40, [500.0, 600.0], 0.6, fraction());
        assert_eq!((object.width, object.height), (729.0, 280.0));
        assert_eq!(
            object.page_bounds(),
            [500.0, 600.0, 500.0 + 729.0 * 0.6, 600.0 + 280.0 * 0.6]
        );
        object.extra.insert("later".into(), json!(1));
        let value = serde_json::to_value(&object).unwrap();
        assert_eq!(value["kind"], "equation");
        assert_eq!(value["latex"], r"\frac{1}{2}");
        assert_eq!(value["color"], json!({"r": 0, "g": 0, "b": 200, "a": 255}));
        assert_eq!(value["scale"], 0.6);
        let back: PageObject = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(back, object);
        let (outline, color) = back.body.outline().unwrap();
        assert_eq!(outline.segments.len(), 9);
        assert_eq!(color, Rgba::rgb(0, 0, 200));
        // A box written by another version is measured again.
        let mut moved = value;
        moved["width"] = json!(5);
        let read: PageObject = serde_json::from_value(moved).unwrap();
        let mut measured = read.clone();
        measured.measure();
        assert_eq!((measured.width, measured.height), (729.0, 280.0));
        for key in ["latex", "path", "color", "kind", "scale"] {
            assert!(is_known_key(EQUATION_KIND, key), "{key}");
        }
        assert!(!is_known_key(EQUATION_KIND, "text") && !is_known_key(TEXT_KIND, "latex"));
    }

    #[test]
    fn an_equation_this_version_cannot_read_is_kept_and_not_drawn() {
        let base = json!({
            "kind": "equation", "id": "e", "layer": 0, "t": 0, "x": 1, "y": 2,
            "width": 30, "height": 40, "latex": "x", "path": "M0 0L1 0L1 1Z",
            "color": {"r": 0, "g": 0, "b": 0, "a": 255}
        });
        let read = |change: &dyn Fn(&mut Value)| {
            let mut value = base.clone();
            change(&mut value);
            let object: PageObject = serde_json::from_value(value.clone()).unwrap();
            (object, value)
        };
        let (fine, _) = read(&|_| {});
        assert!(fine.as_equation().is_some() && fine.body.is_drawn());
        let hostile: [(&str, &dyn Fn(&mut Value)); 11] = [
            ("no path", &|v| {
                v.as_object_mut().unwrap().remove("path");
            }),
            ("no colour", &|v| {
                v.as_object_mut().unwrap().remove("color");
            }),
            ("colour beyond a byte", &|v| {
                v["color"] = json!({"r": 300, "g": 0, "b": 0, "a": 255})
            }),
            ("no source", &|v| {
                v.as_object_mut().unwrap().remove("latex");
            }),
            ("paths of a guess", &|v| v["path"] = json!(["M0 0Z"])),
            ("script", &|v| {
                v["path"] = json!("<script>alert(1)</script>")
            }),
            ("arc", &|v| v["path"] = json!("M0 0A5 5 0 0 1 10 10")),
            ("relative", &|v| v["path"] = json!("m0 0l5 5z")),
            ("far", &|v| v["path"] = json!("M0 0L99999999 0Z")),
            ("long", &|v| {
                v["path"] = json!(format!(
                    "M0 0{}",
                    "L1 1".repeat(EQUATION_MAX_PATH_BYTES / 4)
                ));
            }),
            ("long source", &|v| {
                v["latex"] = json!("x".repeat(EQUATION_MAX_SOURCE_CHARS + 1));
            }),
        ];
        for (what, change) in hostile {
            let (object, value) = read(change);
            assert!(object.as_equation().is_none(), "{what}");
            assert!(!object.body.is_drawn(), "{what}");
            assert_eq!(object.body.kind(), EQUATION_KIND, "{what}");
            // Kept as it was, box included (not measured: this version cannot).
            let mut measured = object.clone();
            measured.measure();
            assert_eq!((measured.width, measured.height), (30.0, 40.0), "{what}");
            let back = serde_json::to_value(&object).unwrap();
            assert_eq!(back.get("path"), value.get("path"), "{what}");
            assert_eq!(back.get("latex"), value.get("latex"), "{what}");
        }
        // A source of the largest length is read; a colour of a wrong type is not.
        let (longest, _) = read(&|v| v["latex"] = json!("é".repeat(EQUATION_MAX_SOURCE_CHARS)));
        assert!(longest.as_equation().is_some());
        let (grey, _) = read(&|v| v["color"] = json!("grey"));
        assert!(grey.as_equation().is_none());
    }

    #[test]
    fn an_equation_moves_with_the_page_by_its_placement() {
        let mut object = PageObject::equation("e", 0, 0, [1000.0, 500.0], 0.6, fraction());
        let corner = object.page_point([object.width, object.height]);
        let cw = Affine::quarter_cw(2000.0);
        object.transform(&cw);
        assert_eq!(object.quarter_turns, 1);
        let turned = object.page_point([object.width, object.height]);
        let expected = cw.apply(corner);
        assert!((turned[0] - expected[0]).abs() < 0.01 && (turned[1] - expected[1]).abs() < 0.01);
        object.transform(&Affine::scaling(2.0, 0.0, 0.0, 0.0, 0.0));
        assert!((object.scale - 1.2).abs() < 1e-6);
        assert_eq!(object.as_equation(), Some(&fraction()));
    }

    #[test]
    fn a_text_frame_this_version_cannot_read_is_kept_as_it_is() {
        // A font of a newer version: not drawn, not lost.
        let value = json!({"kind": "text", "id": "a", "text": "x", "font": "notoSans"});
        let object: PageObject = serde_json::from_value(value).unwrap();
        assert!(object.as_text().is_none());
        let back = serde_json::to_value(&object).unwrap();
        assert_eq!(back["font"], "notoSans");
        assert_eq!(back["kind"], "text");
    }

    #[test]
    fn a_list_with_an_unplaceable_object_reads_as_empty() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(deserialize_with = "deserialize_objects")]
            objects: Vec<PageObject>,
        }
        let read = |value: Value| serde_json::from_value::<Holder>(value).unwrap().objects;
        assert_eq!(
            read(json!({"objects": [{"kind": "text"}, {"kind": "text", "x": "left"}]})).len(),
            0
        );
        assert_eq!(
            read(json!({"objects": [{"kind": "text"}, {"x": 1}]})).len(),
            0
        );
        assert_eq!(read(json!({"objects": "nope"})).len(), 0);
        assert_eq!(
            read(json!({"objects": [{"kind": "text", "scale": 0}]})).len(),
            0
        );
        let fine = read(json!({"objects": [{"kind": "text"}, {"kind": "bookmark", "label": "A"}]}));
        assert_eq!(fine.len(), 2);
        assert_eq!(fine[0].scale, 1.0);
        assert_eq!(fine[0].as_text().map(|b| b.size), Some(TEXT_SIZES[1]));
    }

    #[test]
    fn placement_maps_the_content_onto_the_page() {
        let mut object = PageObject::text("a", 0, 0, 1000.0, 2000.0, hello());
        object.width = 100.0;
        object.height = 50.0;
        assert_eq!(object.page_point([10.0, 5.0]), [1010.0, 2005.0]);
        object.scale = 2.0;
        object.quarter_turns = 1;
        // A clockwise turn on screen (y down): right becomes down, down becomes left.
        assert_eq!(object.page_point([10.0, 0.0]), [1000.0, 2020.0]);
        assert_eq!(object.page_point([0.0, 5.0]), [990.0, 2000.0]);
        assert_eq!(object.page_bounds(), [900.0, 2000.0, 1000.0, 2200.0]);
    }

    #[test]
    fn page_turns_move_the_object_with_the_strokes() {
        let height = 2000.0;
        let mut object = PageObject::text("a", 0, 0, 1000.0, 500.0, hello());
        let corner = object.page_point([object.width, object.height]);
        let cw = Affine::quarter_cw(height);
        object.transform(&cw);
        assert_eq!(object.quarter_turns, 1);
        let turned = object.page_point([object.width, object.height]);
        let expected = cw.apply(corner);
        assert!((turned[0] - expected[0]).abs() < 0.01 && (turned[1] - expected[1]).abs() < 0.01);
        // Four turns back where it was.
        let ccw = Affine::quarter_ccw(height);
        object.transform(&ccw);
        assert_eq!(object.quarter_turns, 0);
        assert!((object.x - 1000.0).abs() < 0.01 && (object.y - 500.0).abs() < 0.01);
        object.transform(&Affine::scaling(0.5, 0.0, 0.0, 10.0, 0.0));
        assert_eq!((object.x, object.y, object.scale), (510.0, 250.0, 0.5));
    }

    #[test]
    fn a_bookmark_round_trips_with_its_unknown_keys_and_measures_its_box() {
        let mut object = PageObject::bookmark("m", 1, 70, 300.0, 400.0, "Théorème de Thalès");
        object
            .extra
            .insert("future".into(), json!({"color": "red"}));
        let value = serde_json::to_value(&object).unwrap();
        assert_eq!(value["kind"], "bookmark");
        assert_eq!(value["label"], "Théorème de Thalès");
        assert_eq!(value["future"], json!({"color": "red"}));
        // The box is the flag, the label and room around them: the label's own width counts.
        let wide = object.width;
        let narrow = PageObject::bookmark("n", 0, 0, 0.0, 0.0, "Un").width;
        assert!(wide > narrow && narrow > 2.0 * BOOKMARK_PAD + BOOKMARK_FLAG_WIDTH);
        assert_eq!(
            object.height,
            1.25 * BOOKMARK_FONT_SIZE + 2.0 * BOOKMARK_PAD
        );
        let back: PageObject = serde_json::from_value(value).unwrap();
        assert_eq!(back, object);
        assert!(object.is_bookmark() && object.as_text().is_none());
        assert_eq!(object.as_bookmark().unwrap().label, "Théorème de Thalès");
    }

    #[test]
    fn a_bookmark_this_version_cannot_read_is_kept_as_it_is() {
        // A label that is not a string (a later version's structured label): kept, not drawn, not lost.
        let value = json!({"kind": "bookmark", "id": "m", "label": {"fr": "Début"}, "x": 5});
        let object: PageObject = serde_json::from_value(value).unwrap();
        assert!(object.as_bookmark().is_none() && !object.is_bookmark());
        let back = serde_json::to_value(&object).unwrap();
        assert_eq!(back["label"], json!({"fr": "Début"}));
        assert_eq!(back["kind"], "bookmark");
        // Without a label: an empty one (the editor removes it).
        let bare: PageObject = serde_json::from_value(json!({"kind": "bookmark"})).unwrap();
        assert_eq!(bare.as_bookmark().unwrap().label, "");
    }

    #[test]
    fn a_label_is_kept_as_one_short_line() {
        assert_eq!(
            clean_bookmark_label(
                "  Chap. 3 
	 Les  forces "
            ),
            "Chap. 3 Les forces"
        );
        assert_eq!(
            clean_bookmark_label(
                " 
 "
            ),
            ""
        );
        let long = "é".repeat(MAX_BOOKMARK_LABEL + 20);
        assert_eq!(
            clean_bookmark_label(&long).chars().count(),
            MAX_BOOKMARK_LABEL
        );
        // Cut where a space would end the label: no trailing space.
        let spaced = format!("{} suite", "a".repeat(MAX_BOOKMARK_LABEL - 1));
        assert_eq!(
            clean_bookmark_label(&spaced),
            "a".repeat(MAX_BOOKMARK_LABEL - 1)
        );
    }

    #[test]
    fn a_bookmark_moves_with_the_page_like_any_object() {
        let mut object = PageObject::bookmark("m", 0, 0, 1000.0, 500.0, "Début");
        object.transform(&Affine::quarter_cw(2000.0));
        assert_eq!(object.quarter_turns, 1);
        assert_eq!([object.x, object.y], [1500.0, 1000.0]);
    }

    #[test]
    fn ids_are_made_unique() {
        let mut objects: Vec<PageObject> = ["a", "", "a", "a-2"]
            .iter()
            .map(|id| PageObject::text(*id, 0, 0, 0.0, 0.0, hello()))
            .collect();
        make_ids_unique(&mut objects);
        let ids: Vec<&str> = objects.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["a", "object-2", "a-3", "a-2"]);
    }
}
