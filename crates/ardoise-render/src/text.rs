//! Text frames (page objects of kind `text`, SPEC §4.4 « Texte tapé au clavier ») in every
//! output. A frame draws the glyph outlines of `ardoise-text` (the ones the editor draws, from
//! the same layout), placed on the page by the frame's placement
//! ([`PageObject::page_point`]: scale, quarter turns, origin), which maps the control points and so
//! keeps the curves exact. They are filled with the non-zero rule in the frame's colour, its alpha
//! as the opacity, normal blending and no grain: in pixels by [`crate::PageCanvas::draw_object`],
//! as paths by the SVG and PDF writers ([`TextFill`]).
//!
//! Only kinds this version knows are drawn: an [`ObjectBody::Unknown`](ardoise_core::note::ObjectBody)
//! (a newer kind, or a text frame this version cannot read) draws nothing.

use ardoise_core::note::PageObject;
use ardoise_ink::Rgba;
use ardoise_text::{LINE_HEIGHT, Layout, Path, outline};

/// Whether this version draws `object` (a text frame it can read).
pub(crate) fn is_drawn(object: &PageObject) -> bool {
    object.as_text().is_some()
}

/// The glyphs of a text frame on the page (device units, y down) and its colour; `None` for a
/// kind this version does not draw.
pub(crate) fn page_outline(object: &PageObject) -> Option<(Path, Rgba)> {
    let body = object.as_text()?;
    let path = outline(&body.layout()).map(|p| object.page_point(p));
    Some((path, body.color))
}

/// A text frame ready for the vector formats.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextFill {
    /// The frame's text: the words the file carries besides the outlines (SVG `<title>`, PDF
    /// invisible text).
    pub text: String,
    /// Its lines, in the frame's own units.
    pub layout: Layout,
    /// Frame units to page units: the content point `(u, v)` is at
    /// `(a u + c v + e, b u + d v + f)` for `[a, b, c, d, e, f]`.
    pub placement: [f32; 6],
    /// The glyphs' outlines on the page; never empty.
    pub path: Path,
    pub rgb: [u8; 3],
    /// 0..1, the colour's alpha.
    pub opacity: f32,
}

impl TextFill {
    /// The frame `object` as the vector formats draw it; `None` for a kind this version does not
    /// draw, or a frame without any outline (blank text).
    pub(crate) fn of(object: &PageObject) -> Option<TextFill> {
        let body = object.as_text()?;
        let layout = body.layout();
        let path = outline(&layout).map(|p| object.page_point(p));
        if path.is_empty() {
            return None;
        }
        let [e, f] = object.page_point([0.0, 0.0]);
        let [ux, uy] = object.page_point([1.0, 0.0]);
        let [vx, vy] = object.page_point([0.0, 1.0]);
        let c = body.color;
        Some(TextFill {
            text: body.text.clone(),
            layout,
            placement: [ux - e, uy - f, vx - e, vy - f, e, f],
            path,
            rgb: [c.r, c.g, c.b],
            opacity: f32::from(c.a) / 255.0,
        })
    }

    /// The size the text was laid out at (its em, frame units), as the layout used it: clamped,
    /// zero when not usable.
    pub(crate) fn size(&self) -> f32 {
        self.layout.line_height / LINE_HEIGHT
    }

    /// Where the frame point `(u, v)` is on the page.
    pub(crate) fn page_point(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.placement;
        [a * u + c * v + e, b * u + d * v + f]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::{ObjectBody, TextBody};
    use ardoise_text::FontFamily;

    fn frame(quarter_turns: u8, scale: f32) -> PageObject {
        let mut object = PageObject::text(
            "a",
            0,
            0,
            1000.0,
            2000.0,
            TextBody {
                text: "Ab\nc".into(),
                font: FontFamily::Andika,
                bold: false,
                size: 600.0,
                color: Rgba {
                    a: 51,
                    ..Rgba::rgb(10, 20, 30)
                },
            },
        );
        object.quarter_turns = quarter_turns;
        object.scale = scale;
        object
    }

    #[test]
    fn the_placement_is_the_objects() {
        for turns in 0..4 {
            let object = frame(turns, 1.5);
            let fill = TextFill::of(&object).unwrap();
            for p in [[0.0, 0.0], [10.0, 0.0], [0.0, 7.0], [123.0, -45.0]] {
                let [x, y] = fill.page_point(p);
                let [ex, ey] = object.page_point(p);
                assert!(
                    (x - ex).abs() < 1e-3 && (y - ey).abs() < 1e-3,
                    "{turns} {p:?}"
                );
            }
            assert_eq!((fill.rgb, fill.opacity), ([10, 20, 30], 0.2));
            assert!((fill.size() - 600.0).abs() < 1e-3);
            // The outline is the frame's, moved onto the page.
            let (path, color) = page_outline(&object).unwrap();
            assert_eq!(path, fill.path);
            assert_eq!(color.a, 51);
        }
    }

    #[test]
    fn unknown_kinds_and_blank_text_draw_nothing() {
        let mut object = frame(0, 1.0);
        object.body = ObjectBody::Unknown {
            kind: "equation".into(),
            fields: Default::default(),
        };
        assert!(!is_drawn(&object));
        assert!(page_outline(&object).is_none() && TextFill::of(&object).is_none());
        let mut blank = frame(0, 1.0);
        if let ObjectBody::Text(body) = &mut blank.body {
            body.text = "  \n\t".into();
        }
        assert!(is_drawn(&blank));
        assert!(TextFill::of(&blank).is_none());
        assert!(page_outline(&blank).unwrap().0.is_empty());
    }
}
