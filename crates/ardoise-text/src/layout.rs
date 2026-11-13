//! Lines of a text frame and the outlines of their glyphs.
//!
//! The rules, which the editor's text field follows too (`app/ui/src/lib/editor/text.ts`), so that
//! its caret falls between the glyphs drawn:
//!
//! - a line of the text is a line of the frame: only `\n` (or `\r\n`, or a lone `\r`) starts a new
//!   one, nothing wraps;
//! - each character takes the advance width of its glyph (`hmtx`), from the frame's family, else
//!   from the other family (Greek letters in Latin Modern come from Andika), else the family's
//!   `.notdef`; no kerning, no ligature, no other substitution;
//! - a tab goes to the next multiple of [`TAB_SPACES`] spaces from the start of the line; other
//!   control characters take no room and draw nothing;
//! - lines are [`LINE_HEIGHT`] × size apart; within a line the baseline sits as CSS puts it: half
//!   the leading above the family's ascent (`hhea`).

use ttf_parser::{GlyphId, OutlineBuilder};

use crate::fonts::{FontFamily, face};
use crate::path::{Path, Segment};

/// Distance between two lines, in units of the text's size (CSS `line-height: 1.25`).
pub const LINE_HEIGHT: f32 = 1.25;

/// A tab goes to the next multiple of this many spaces (CSS `tab-size: 4`).
pub const TAB_SPACES: f32 = 4.0;

/// Largest size of a text, in its own units (a frame's scale makes it larger on the page).
const MAX_SIZE: f32 = 1.0e6;

/// How a frame's text looks: one style for the whole frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub family: FontFamily,
    pub bold: bool,
    /// Size of the font (its em), in the frame's units; not a number or not positive draws nothing.
    pub size: f32,
}

impl TextStyle {
    fn size(&self) -> f32 {
        if self.size.is_finite() {
            self.size.clamp(0.0, MAX_SIZE)
        } else {
            0.0
        }
    }
}

/// A glyph on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedGlyph {
    /// The family whose glyph it is (the other one when the frame's family lacks the character).
    pub family: FontFamily,
    pub glyph: u16,
    /// Pen position on the line, from the left edge of the frame.
    pub x: f32,
    pub advance: f32,
    /// Offset of its character in the text, in UTF-16 code units (the text field's positions).
    pub index: usize,
}

/// One line of the frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Top of the line's box, from the top of the frame.
    pub top: f32,
    pub baseline: f32,
    /// Advance width of the line (its trailing spaces included).
    pub width: f32,
    /// UTF-16 offsets of the line's first character and of the end of the line (its line break
    /// excluded).
    pub start: usize,
    pub end: usize,
    pub glyphs: Vec<PlacedGlyph>,
}

/// A frame's text laid out, in the frame's units, origin at its top-left corner, y down.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub style: TextStyle,
    pub lines: Vec<Line>,
    /// The widest line.
    pub width: f32,
    /// Every line's box: `lines × line_height`.
    pub height: f32,
    pub line_height: f32,
    /// The family's ascent and descent (both positive), at the text's size.
    pub ascent: f32,
    pub descent: f32,
}

impl Layout {
    /// The UTF-16 offset nearest to `(x, y)` (frame units): the line under `y` (the first or
    /// the last beyond them), the gap between two glyphs nearest to `x` on it.
    pub fn offset_at(&self, x: f32, y: f32) -> usize {
        let Some(last) = self.lines.len().checked_sub(1) else {
            return 0;
        };
        let row = if self.line_height > 0.0 {
            ((y / self.line_height).floor().max(0.0) as usize).min(last)
        } else {
            0
        };
        let line = &self.lines[row];
        for glyph in &line.glyphs {
            if x < glyph.x + glyph.advance / 2.0 {
                return glyph.index;
            }
        }
        line.end
    }
}

/// The advance of `glyph` of `face`, scaled.
fn advance(face: &ttf_parser::Face<'_>, glyph: GlyphId, scale: f32) -> f32 {
    f32::from(face.glyph_hor_advance(glyph).unwrap_or(0)) * scale
}

fn units_scale(face: &ttf_parser::Face<'_>, size: f32) -> f32 {
    size / f32::from(face.units_per_em().max(1))
}

/// Characters that take no room and draw nothing (control characters other than the tab).
fn is_invisible(c: char) -> bool {
    c != '\t' && c.is_control()
}

/// Lays `text` out (see the module documentation).
pub fn layout(text: &str, style: TextStyle) -> Layout {
    let size = style.size();
    let line_height = size * LINE_HEIGHT;
    let primary = face(style.family, style.bold);
    let fallback = face(style.family.other(), style.bold);
    let (ascent, descent) = primary.map_or((0.8 * size, 0.2 * size), |f| {
        let s = units_scale(f, size);
        (f32::from(f.ascender()) * s, -f32::from(f.descender()) * s)
    });
    // CSS: half the leading above the content area, whose top is the ascent above the baseline.
    let baseline_in_line = (line_height - (ascent + descent)) / 2.0 + ascent;
    let space = primary
        .and_then(|f| {
            f.glyph_index(' ')
                .map(|g| advance(f, g, units_scale(f, size)))
        })
        .unwrap_or(size / 4.0);
    let tab = space * TAB_SPACES;

    let mut lines = Vec::new();
    let mut glyphs = Vec::new();
    let mut pen = 0.0_f32;
    let mut index = 0usize;
    let mut start = 0usize;
    let mut chars = text.chars().peekable();
    let finish_line =
        |lines: &mut Vec<Line>, glyphs: &mut Vec<PlacedGlyph>, pen: f32, start: usize, end| {
            let top = lines.len() as f32 * line_height;
            lines.push(Line {
                top,
                baseline: top + baseline_in_line,
                width: pen,
                start,
                end,
                glyphs: std::mem::take(glyphs),
            });
        };
    while let Some(c) = chars.next() {
        let at = index;
        index += c.len_utf16();
        if c == '\n' || c == '\r' {
            finish_line(&mut lines, &mut glyphs, pen, start, at);
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
                index += 1;
            }
            start = index;
            pen = 0.0;
            continue;
        }
        if c == '\t' {
            pen = if tab > 0.0 {
                ((pen / tab).floor() + 1.0) * tab
            } else {
                pen
            };
            continue;
        }
        if is_invisible(c) {
            continue;
        }
        let found = [(style.family, primary), (style.family.other(), fallback)]
            .into_iter()
            .find_map(|(family, f)| {
                let f = f?;
                f.glyph_index(c)
                    .filter(|g| g.0 != 0)
                    .map(|g| (family, f, g))
            })
            .or_else(|| primary.map(|f| (style.family, f, GlyphId(0))));
        let Some((family, f, glyph)) = found else {
            continue;
        };
        let width = advance(f, glyph, units_scale(f, size));
        glyphs.push(PlacedGlyph {
            family,
            glyph: glyph.0,
            x: pen,
            advance: width,
            index: at,
        });
        pen += width;
    }
    finish_line(&mut lines, &mut glyphs, pen, start, index);
    let width = lines.iter().map(|l| l.width).fold(0.0_f32, f32::max);
    Layout {
        style,
        height: lines.len() as f32 * line_height,
        lines,
        width,
        line_height,
        ascent,
        descent,
    }
}

/// Collects a glyph's outline into a [`Path`], from font units (y up) to frame units (y down).
struct Collect<'a> {
    path: &'a mut Path,
    x: f32,
    baseline: f32,
    scale: f32,
}

impl Collect<'_> {
    fn point(&self, x: f32, y: f32) -> [f32; 2] {
        [self.x + x * self.scale, self.baseline - y * self.scale]
    }
}

impl OutlineBuilder for Collect<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.path.segments.push(Segment::MoveTo(p));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.path.segments.push(Segment::LineTo(p));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.point(x1, y1), self.point(x, y));
        self.path.segments.push(Segment::QuadTo(c, p));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        self.path.segments.push(Segment::CubicTo(c1, c2, p));
    }

    fn close(&mut self) {
        self.path.segments.push(Segment::Close);
    }
}

/// The outlines of every glyph of `layout`, in frame units.
pub fn outline(layout: &Layout) -> Path {
    let mut path = Path::default();
    let size = layout.style.size();
    for line in &layout.lines {
        for glyph in &line.glyphs {
            let Some(f) = face(glyph.family, layout.style.bold) else {
                continue;
            };
            let mut collect = Collect {
                path: &mut path,
                x: glyph.x,
                baseline: line.baseline,
                scale: units_scale(f, size),
            };
            let _ = f.outline_glyph(GlyphId(glyph.glyph), &mut collect);
        }
    }
    path
}

/// [`layout`] and [`outline`] at once.
pub fn text_outline(text: &str, style: TextStyle) -> (Layout, Path) {
    let laid = layout(text, style);
    let path = outline(&laid);
    (laid, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LM: TextStyle = TextStyle {
        family: FontFamily::LatinModern,
        bold: false,
        size: 1000.0,
    };

    #[test]
    fn lines_break_only_at_line_breaks() {
        let laid = layout("Bonjour\nle monde\r\n\rfin", LM);
        assert_eq!(laid.lines.len(), 4);
        assert_eq!(
            laid.lines
                .iter()
                .map(|l| (l.start, l.end))
                .collect::<Vec<_>>(),
            [(0, 7), (8, 16), (18, 18), (19, 22)]
        );
        assert_eq!(laid.height, 4.0 * 1250.0);
        assert_eq!(laid.lines[2].glyphs.len(), 0);
        assert_eq!(laid.lines[1].top, 1250.0);
        assert!(laid.lines[0].baseline > 0.0 && laid.lines[0].baseline < 1250.0);
        assert_eq!(laid.lines[1].baseline - laid.lines[0].baseline, 1250.0);
        let widest = laid.lines.iter().map(|l| l.width).fold(0.0, f32::max);
        assert_eq!(laid.width, widest);
    }

    #[test]
    fn glyphs_follow_each_other_by_their_advances_without_kerning() {
        let laid = layout("AVAV", LM);
        let glyphs = &laid.lines[0].glyphs;
        assert_eq!(glyphs.len(), 4);
        for pair in glyphs.windows(2) {
            assert_eq!(pair[1].x, pair[0].x + pair[0].advance);
        }
        // Twice the same pair: no kerning makes them differ.
        assert_eq!(glyphs[2].x, 2.0 * glyphs[1].x - glyphs[0].x);
        // Double the size, double every distance.
        let twice = layout("AVAV", TextStyle { size: 2000.0, ..LM });
        assert!((twice.width - 2.0 * laid.width).abs() < 0.01);
    }

    #[test]
    fn utf16_offsets_count_surrogate_pairs() {
        // U+1D400 (mathematical bold A) takes two UTF-16 units.
        let laid = layout("a\u{1D400}b", LM);
        let indices: Vec<usize> = laid.lines[0].glyphs.iter().map(|g| g.index).collect();
        assert_eq!(indices, [0, 1, 3]);
        assert_eq!(laid.lines[0].end, 4);
    }

    #[test]
    fn greek_in_latin_modern_comes_from_andika() {
        let laid = layout("aα", LM);
        let families: Vec<FontFamily> = laid.lines[0].glyphs.iter().map(|g| g.family).collect();
        assert_eq!(families, [FontFamily::LatinModern, FontFamily::Andika]);
        assert!(
            laid.lines[0]
                .glyphs
                .iter()
                .all(|g| g.glyph != 0 && g.advance > 0.0)
        );
    }

    #[test]
    fn tabs_go_to_the_next_stop_and_control_characters_vanish() {
        let space = layout(" ", LM).width;
        let laid = layout("a\tb\u{7}c", LM);
        let glyphs = &laid.lines[0].glyphs;
        assert_eq!(glyphs.len(), 3);
        assert!((glyphs[1].x - 4.0 * space).abs() < 0.01, "{glyphs:?}");
        assert_eq!(glyphs[2].x, glyphs[1].x + glyphs[1].advance);
        assert_eq!(glyphs[2].index, 4);
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        let laid = layout("", LM);
        assert_eq!(laid.lines.len(), 1);
        assert_eq!((laid.width, laid.height), (0.0, 1250.0));
        assert!(outline(&laid).is_empty());
        for size in [f32::NAN, -5.0, 0.0] {
            let laid = layout("abc", TextStyle { size, ..LM });
            assert_eq!(laid.width, 0.0);
        }
    }

    #[test]
    fn outlines_lie_within_the_lines() {
        for family in FontFamily::ALL {
            for bold in [false, true] {
                let style = TextStyle {
                    family,
                    bold,
                    size: 600.0,
                };
                let (laid, path) = text_outline("Élève ça\nœuvre « 12 € »", style);
                assert!(!path.is_empty());
                let [x0, y0, x1, y1] = path.bounds().unwrap();
                assert!(
                    x0 >= -60.0 && x1 <= laid.width + 60.0,
                    "{family:?} {x0} {x1}"
                );
                assert!(
                    y0 >= -0.5 * laid.line_height && y1 <= laid.height,
                    "{family:?} {y0} {y1}"
                );
                // The second line is below the first.
                assert!(y1 > laid.lines[1].top);
            }
        }
    }

    #[test]
    fn bold_is_wider_and_the_families_differ() {
        let text = "Les élèves";
        let regular = layout(text, LM).width;
        let bold = layout(text, TextStyle { bold: true, ..LM }).width;
        let andika = layout(
            text,
            TextStyle {
                family: FontFamily::Andika,
                ..LM
            },
        )
        .width;
        assert!(bold > regular, "{bold} {regular}");
        assert!((andika - regular).abs() > 1.0);
    }

    #[test]
    fn the_offset_under_a_point_is_the_nearest_gap() {
        let laid = layout("ab\ncd", LM);
        let a = laid.lines[0].glyphs[0];
        assert_eq!(laid.offset_at(-10.0, 10.0), 0);
        assert_eq!(laid.offset_at(a.advance * 0.4, 10.0), 0);
        assert_eq!(laid.offset_at(a.advance * 0.6, 10.0), 1);
        assert_eq!(laid.offset_at(1.0e6, 10.0), 2);
        assert_eq!(laid.offset_at(0.0, 1300.0), 3);
        assert_eq!(laid.offset_at(1.0e6, 1.0e6), 5);
        assert_eq!(laid.offset_at(0.0, -100.0), 0);
    }
}
