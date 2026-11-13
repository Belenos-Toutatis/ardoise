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
//! Kinds today: `text` ([`TextBody`], SPEC §4.4 « Texte tapé au clavier »).

use ardoise_ink::Rgba;
use ardoise_text::{FontFamily, Layout, TextStyle};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// The kind of a text frame (`"kind": "text"`).
pub const TEXT_KIND: &str = "text";

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

/// Sizes the text tool offers, in device units (the size of the font, its em): 4, 6 and 9 mm.
pub const TEXT_SIZES: [f32; 3] = [400.0, 600.0, 900.0];

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

/// What an object shows.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectBody {
    Text(TextBody),
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
            ObjectBody::Unknown { kind, .. } => kind,
        }
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

    /// The text of a text frame.
    pub fn as_text(&self) -> Option<&TextBody> {
        match &self.body {
            ObjectBody::Text(body) => Some(body),
            ObjectBody::Unknown { .. } => None,
        }
    }

    /// Sets `width` and `height` from the content, for the kinds this version can measure (the
    /// others keep the box they were written with).
    pub fn measure(&mut self) {
        if let ObjectBody::Text(body) = &self.body {
            let laid = body.layout();
            self.width = laid.width;
            self.height = laid.height;
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
        if let ObjectBody::Text(body) = &self.body {
            map.insert("text".into(), Value::String(body.text.clone()));
            map.insert(
                "font".into(),
                serde_json::to_value(body.font).map_err(serde::ser::Error::custom)?,
            );
            map.insert("bold".into(), Value::Bool(body.bold));
            map.insert("size".into(), number(body.size));
            map.insert(
                "color".into(),
                serde_json::to_value(body.color).map_err(serde::ser::Error::custom)?,
            );
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
        let body = match (kind.as_str(), text_body(&map)) {
            (TEXT_KIND, Some(body)) => {
                for key in TEXT_KEYS {
                    map.remove(key);
                }
                ObjectBody::Text(body)
            }
            _ => ObjectBody::Unknown {
                kind,
                fields: std::mem::take(&mut map),
            },
        };
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
    PLACEMENT_KEYS.contains(&key) || (kind == TEXT_KIND && TEXT_KEYS.contains(&key))
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
            "kind": "equation", "id": "e", "layer": 0, "t": 3, "x": 10, "y": 20, "scale": 2,
            "quarterTurns": 5, "width": 30, "height": 40, "latex": "x^2", "paths": ["M0 0Z"]
        });
        let object: PageObject = serde_json::from_value(value).unwrap();
        assert_eq!(object.body.kind(), "equation");
        assert_eq!(object.quarter_turns, 1);
        assert_eq!(object.scale, 2.0);
        let back = serde_json::to_value(&object).unwrap();
        assert_eq!(back["latex"], "x^2");
        assert_eq!(back["paths"], json!(["M0 0Z"]));
        assert_eq!(back["quarterTurns"], 1);
        assert_eq!(back["x"], 10.0);
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
