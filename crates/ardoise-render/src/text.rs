//! Page objects in every output: text frames (kind `text`, SPEC §4.4 « Texte tapé au clavier »)
//! and equations (kind `equation`, « Équation »). A text frame draws the glyph outlines of
//! `ardoise-text` (the ones the editor draws, from the same layout); an equation draws the
//! outlines it stores, made by MathJax in the editor (`docs/design/equations.md`): no MathJax runs
//! here. Both are placed on the page by the object's placement ([`PageObject::page_point`]: scale,
//! quarter turns, origin), which maps the control points and so keeps the curves exact. They are
//! filled with the non-zero rule in the object's colour, its alpha as the opacity, normal blending
//! and no grain: in pixels by [`crate::PageCanvas::draw_object`], as paths by the SVG and PDF
//! writers ([`TextFill`]).
//!
//! Only kinds this version knows are drawn: an [`ObjectBody::Unknown`] (a newer kind, or an
//! object this version cannot read) draws nothing.

use ardoise_core::note::{ObjectBody, PageObject};
use ardoise_ink::Rgba;
use ardoise_text::{LINE_HEIGHT, Layout, Path, outline};

/// Whether this version draws `object` (a text frame or an equation it can read).
pub(crate) fn is_drawn(object: &PageObject) -> bool {
    object.body.is_drawn()
}

/// What `object` draws on the page (device units, y down) and its colour; `None` for a kind this
/// version does not draw.
pub(crate) fn page_outline(object: &PageObject) -> Option<(Path, Rgba)> {
    let (path, color) = object.body.outline()?;
    Some((path.map(|p| object.page_point(p)), color))
}

/// A page object ready for the vector formats: a text frame or an equation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextFill {
    /// The words the file carries besides the outlines (SVG `<title>`; for a text frame, the PDF's
    /// invisible text too): a frame's text, an equation's LaTeX source.
    pub text: String,
    /// A text frame's lines, in its own units (the PDF lays its invisible text on them); `None`
    /// for an equation, which has no text to search in a PDF.
    pub layout: Option<Layout>,
    /// Object units to page units: the content point `(u, v)` is at
    /// `(a u + c v + e, b u + d v + f)` for `[a, b, c, d, e, f]`.
    pub placement: [f32; 6],
    /// The outlines on the page; never empty.
    pub path: Path,
    pub rgb: [u8; 3],
    /// 0..1, the colour's alpha.
    pub opacity: f32,
}

impl TextFill {
    /// The object as the vector formats draw it; `None` for a kind this version does not draw, or
    /// an object without any outline (blank text).
    pub(crate) fn of(object: &PageObject) -> Option<TextFill> {
        let (text, layout, outline, color) = match &object.body {
            ObjectBody::Text(body) => {
                let layout = body.layout();
                let path = outline(&layout);
                (body.text.clone(), Some(layout), path, body.color)
            }
            ObjectBody::Equation(body) => (body.latex.clone(), None, body.outline(), body.color),
            ObjectBody::Unknown { .. } => return None,
        };
        let path = outline.map(|p| object.page_point(p));
        if path.is_empty() {
            return None;
        }
        let [e, f] = object.page_point([0.0, 0.0]);
        let [ux, uy] = object.page_point([1.0, 0.0]);
        let [vx, vy] = object.page_point([0.0, 1.0]);
        Some(TextFill {
            text,
            layout,
            placement: [ux - e, uy - f, vx - e, vy - f, e, f],
            path,
            rgb: [color.r, color.g, color.b],
            opacity: f32::from(color.a) / 255.0,
        })
    }

    /// The size a text frame was laid out at (its em, frame units), as the layout used it:
    /// clamped, zero when not usable or for an equation.
    pub(crate) fn size(&self) -> f32 {
        self.layout
            .as_ref()
            .map_or(0.0, |layout| layout.line_height / LINE_HEIGHT)
    }

    /// Where the object point `(u, v)` is on the page.
    pub(crate) fn page_point(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.placement;
        [a * u + c * v + e, b * u + d * v + f]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::{EquationBody, TextBody};
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
    fn an_equation_draws_its_stored_outline_at_its_place() {
        let mut object = PageObject::equation(
            "e",
            0,
            0,
            [1000.0, 2000.0],
            0.5,
            EquationBody {
                latex: r"\ce{H2O}".into(),
                path: "M0 0L100 0L100 40Z".into(),
                color: Rgba::rgb(200, 0, 0),
            },
        );
        object.quarter_turns = 1;
        let fill = TextFill::of(&object).unwrap();
        assert_eq!(fill.text, r"\ce{H2O}");
        assert!(fill.layout.is_none() && fill.size() == 0.0);
        assert_eq!(fill.rgb, [200, 0, 0]);
        // A clockwise quarter turn: right becomes down.
        assert_eq!(fill.path.bounds(), Some([980.0, 2000.0, 1000.0, 2050.0]));
        assert_eq!(page_outline(&object).unwrap().0, fill.path);
        assert!(is_drawn(&object));
    }

    #[test]
    fn unknown_kinds_and_blank_text_draw_nothing() {
        let mut object = frame(0, 1.0);
        object.body = ObjectBody::Unknown {
            kind: "stamp".into(),
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
