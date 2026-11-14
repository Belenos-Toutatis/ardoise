//! Lines of a text frame and the outlines of their glyphs.
//!
//! The rules, which the editor's text field follows too (`app/ui/src/lib/text.ts`,
//! `TextField.svelte`), so that its caret falls between the glyphs drawn:
//!
//! - a line break (`\n`, `\r\n` or a lone `\r`) starts a new line. A frame with a wrap width
//!   ([`TextStyle::wrap`], format 1.7) also starts one where the next word would cross it: greedy,
//!   first fit. A line may break after a run of white space ([`is_break_space`]: the space, the
//!   tab, the other breaking spaces of Unicode, the zero width space); the white space stays at
//!   the end of the line it ends ("hangs", even beyond the width) and never makes a line break. A
//!   word wider than the width alone on its line breaks before the first character that crosses
//!   it (a line holds at least one character). No hyphenation, no other break opportunity: a
//!   no-break space (U+00A0, U+202F, U+2007) and a hyphen do not break. A frame without a wrap
//!   width never wraps: frames made before format 1.7 keep their lines exactly;
//! - each character takes the advance width of its glyph (`hmtx`) from the first face of
//!   [`fallback_chain`] that serves it and has it, the face the browser takes it from too (the
//!   field lists the same faces in the same order); no kerning, no ligature, no other
//!   substitution. Three exceptions draw another glyph in that same advance: in a bold frame a
//!   glyph of Latin Modern Math, which has one weight, is emboldened ([`GlyphLook::Emboldened`]);
//!   in a Latin Modern frame a superscript or subscript digit or sign Latin Modern lacks (it has
//!   only ¹ ² ³) is its own digit reduced and raised like its ¹ ² ³ ([`GlyphLook::Superscript`],
//!   [`GlyphLook::Subscript`]) rather than Andika's; a character no face has is a white square
//!   □ ([`GlyphLook::Missing`]), which the editor reports;
//! - a tab goes to the next multiple of [`TAB_SPACES`] spaces from the start of the line; other
//!   control characters and the default-ignorable ones (zero width space, soft hyphen, joiners,
//!   variation selectors…) take no room and draw nothing, as in a browser;
//! - lines are [`LINE_HEIGHT`] × size apart; within a line the baseline sits as CSS puts it: half
//!   the leading above the frame's face's ascent (`hhea`).

use std::sync::OnceLock;

use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::fonts::{FontFace, FontFamily, face, fallback_chain};
use crate::path::{Path, Segment};

/// Distance between two lines, in units of the text's size (CSS `line-height: 1.25`).
pub const LINE_HEIGHT: f32 = 1.25;

/// A tab goes to the next multiple of this many spaces (CSS `tab-size: 4`).
pub const TAB_SPACES: f32 = 4.0;

/// What stands for a character no face has: a white square, from Latin Modern Math.
pub const MISSING_CHAR: char = '\u{25A1}';

/// How far an emboldened glyph's outline moves out, on each side, in units of the text's size
/// (Latin Modern Bold's stems are about 1/25 em thicker than the regular's).
pub const EMBOLDEN: f32 = 1.0 / 50.0;

/// Largest size of a text, in its own units (a frame's scale makes it larger on the page).
const MAX_SIZE: f32 = 1.0e6;

/// Largest wrap width, in the text's own units.
const MAX_WRAP: f32 = 1.0e8;

/// A line crosses its wrap width only beyond it by more than this share of it (a millionth: a few
/// units of the last place of an `f32`), so that a width read back from a page position (the side
/// handle of the editor, which goes through a `f64` and back) never wraps a line that fitted.
const WRAP_TOLERANCE: f32 = 1.0e-6;

/// Whether a line whose ink would end at `end` crosses the wrap width `limit`.
fn crosses(end: f32, limit: f32) -> bool {
    end > limit * (1.0 + WRAP_TOLERANCE)
}

/// How a frame's text looks and wraps: one style for the whole frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub family: FontFamily,
    pub bold: bool,
    /// Size of the font (its em), in the frame's units; not a number or not positive draws nothing.
    pub size: f32,
    /// Width at which lines wrap, in the frame's units; `None` (or a value that is not a
    /// positive number): only line breaks start a line.
    pub wrap: Option<f32>,
}

impl TextStyle {
    fn size(&self) -> f32 {
        if self.size.is_finite() {
            self.size.clamp(0.0, MAX_SIZE)
        } else {
            0.0
        }
    }

    /// The wrap width the layout uses: a positive number, or `None`.
    pub fn wrap_width(&self) -> Option<f32> {
        self.wrap
            .filter(|w| w.is_finite() && *w > 0.0)
            .map(|w| w.min(MAX_WRAP))
    }
}

/// How a glyph is drawn (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphLook {
    /// The face's glyph as it is.
    Plain,
    /// A glyph of Latin Modern Math in a bold frame: its outline moved out by [`EMBOLDEN`] em on
    /// each side, its advance kept.
    Emboldened,
    /// A superscript digit or sign Latin Modern lacks, in a Latin Modern frame: `glyph` is Latin
    /// Modern's digit (or sign), reduced and raised like its own ¹ ² ³, centred in the advance.
    Superscript,
    /// The same for a subscript, lowered.
    Subscript,
    /// No face has the character: `glyph` is [`MISSING_CHAR`] of Latin Modern Math.
    Missing,
}

/// A glyph on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedGlyph {
    /// The face whose glyph it is.
    pub face: FontFace,
    pub glyph: u16,
    /// Pen position on the line, from the left edge of the frame.
    pub x: f32,
    pub advance: f32,
    /// Offset of its character in the text, in UTF-16 code units (the text field's positions).
    pub index: usize,
    pub look: GlyphLook,
}

/// One line of the frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Top of the line's box, from the top of the frame.
    pub top: f32,
    pub baseline: f32,
    /// Advance width of the line (its trailing white space included).
    pub width: f32,
    /// UTF-16 offsets of the line's first character and of the end of the line: its line break
    /// excluded; for a wrapped line, where the next line starts.
    pub start: usize,
    pub end: usize,
    /// The line ends where the text wrapped, not at a line break: the next line goes on with
    /// the same paragraph from `end`.
    pub wrapped: bool,
    pub glyphs: Vec<PlacedGlyph>,
}

/// A frame's text laid out, in the frame's units, origin at its top-left corner, y down.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub style: TextStyle,
    pub lines: Vec<Line>,
    /// The frame's width: without a wrap width, the widest line (its trailing white space
    /// included); with one, the widest line's ink, white space counting up to the wrap width
    /// only (white space hanging beyond it does not widen the frame).
    pub width: f32,
    /// Every line's box: `lines × line_height`.
    pub height: f32,
    pub line_height: f32,
    /// The face's ascent and descent (both positive), at the text's size.
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

    /// The characters of `text` (the text laid out) that no face has, each once, in order.
    pub fn missing_chars(&self, text: &str) -> Vec<char> {
        let mut offsets: Vec<usize> = self
            .lines
            .iter()
            .flat_map(|line| line.glyphs.iter())
            .filter(|g| g.look == GlyphLook::Missing)
            .map(|g| g.index)
            .collect();
        offsets.sort_unstable();
        let mut found = Vec::new();
        let mut at = 0usize;
        let mut wanted = offsets.into_iter().peekable();
        for c in text.chars() {
            if wanted.peek() == Some(&at) {
                wanted.next();
                if !found.contains(&c) {
                    found.push(c);
                }
            }
            at += c.len_utf16();
        }
        found
    }
}

/// The advance of `glyph` of `face`, scaled.
fn advance(face: &Face<'_>, glyph: GlyphId, scale: f32) -> f32 {
    f32::from(face.glyph_hor_advance(glyph).unwrap_or(0)) * scale
}

fn units_scale(face: &Face<'_>, size: f32) -> f32 {
    size / f32::from(face.units_per_em().max(1))
}

/// Whether a line may break after `c` (white space that breaks, and the zero width space).
pub fn is_break_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200B}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// Whether `c` is a default-ignorable code point (Unicode's `Default_Ignorable_Code_Point`): a
/// browser draws nothing for it and gives it no room.
fn is_default_ignorable(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFF8
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

/// Characters that take no room and draw nothing (control characters other than the tab, and the
/// default-ignorable ones).
fn is_invisible(c: char) -> bool {
    (c != '\t' && c.is_control()) || is_default_ignorable(c)
}

/// The character a superscript or subscript digit or sign is drawn from, and whether it is a
/// superscript.
fn script_base(c: char) -> Option<(char, bool)> {
    let digit = |d: u32| char::from_digit(d, 10);
    match u32::from(c) {
        0x2070 => Some(('0', true)),
        0x00B9 => Some(('1', true)),
        0x00B2 => Some(('2', true)),
        0x00B3 => Some(('3', true)),
        code @ 0x2074..=0x2079 => digit(code - 0x2070).map(|d| (d, true)),
        0x207A => Some(('+', true)),
        0x207B => Some(('\u{2212}', true)),
        0x207C => Some(('=', true)),
        0x207D => Some(('(', true)),
        0x207E => Some((')', true)),
        code @ 0x2080..=0x2089 => digit(code - 0x2080).map(|d| (d, false)),
        0x208A => Some(('+', false)),
        0x208B => Some(('\u{2212}', false)),
        0x208C => Some(('=', false)),
        0x208D => Some(('(', false)),
        0x208E => Some((')', false)),
        _ => None,
    }
}

/// How a face draws its superscripts: Latin Modern's ¹ is its 1 reduced by `scale` and raised by
/// `raise`; a subscript is lowered by `drop` (font units, y up).
#[derive(Debug, Clone, Copy)]
struct ScriptMetrics {
    scale: f32,
    raise: f32,
    drop: f32,
}

fn script_metrics(face_id: FontFace) -> Option<ScriptMetrics> {
    static METRICS: OnceLock<[Option<ScriptMetrics>; 5]> = OnceLock::new();
    METRICS.get_or_init(|| {
        FontFace::ALL.map(|id| {
            let f = face(id)?;
            let one = f.glyph_bounding_box(f.glyph_index('1')?)?;
            let sup = f.glyph_bounding_box(f.glyph_index('\u{B9}')?)?;
            let height = f32::from(one.y_max) - f32::from(one.y_min);
            if height <= 0.0 {
                return None;
            }
            let scale = (f32::from(sup.y_max) - f32::from(sup.y_min)) / height;
            let raise = f32::from(sup.y_min) - scale * f32::from(one.y_min);
            // The subscript's baseline: as far below the baseline as the font's own subscripts
            // go (OS/2), else 15 % of the em (TeX's).
            let drop = f
                .subscript_metrics()
                .map(|m| f32::from(m.y_offset))
                .filter(|d| *d > 0.0)
                .unwrap_or(0.15 * f32::from(f.units_per_em()));
            Some(ScriptMetrics { scale, raise, drop })
        })
    })[face_id.code() as usize]
}

/// The glyph a character takes in a frame of `style`'s family and weight: the face, the glyph,
/// its advance (frame units) and how it is drawn.
fn resolve(c: char, style: &TextStyle, size: f32) -> Option<(FontFace, GlyphId, f32, GlyphLook)> {
    let chain = fallback_chain(style.family, style.bold);
    let found = chain.into_iter().find_map(|id| {
        let f = face(id).filter(|_| id.serves(c))?;
        let glyph = f.glyph_index(c).filter(|g| g.0 != 0)?;
        Some((id, f, glyph))
    });
    let Some((id, f, glyph)) = found else {
        let math = face(FontFace::LatinModernMath)?;
        let square = math.glyph_index(MISSING_CHAR)?;
        let width = advance(math, square, units_scale(math, size));
        return Some((FontFace::LatinModernMath, square, width, GlyphLook::Missing));
    };
    let width = advance(f, glyph, units_scale(f, size));
    // A Latin Modern frame draws the scripts its face lacks from its own digits.
    if style.family == FontFamily::LatinModern
        && id != chain[0]
        && let Some((base, superscript)) = script_base(c)
        && let Some(own) = face(chain[0]).and_then(|f| f.glyph_index(base))
        && script_metrics(chain[0]).is_some()
    {
        let look = if superscript {
            GlyphLook::Superscript
        } else {
            GlyphLook::Subscript
        };
        return Some((chain[0], own, width, look));
    }
    let look = if style.bold && id == FontFace::LatinModernMath {
        GlyphLook::Emboldened
    } else {
        GlyphLook::Plain
    };
    Some((id, glyph, width, look))
}

/// The lines being made, and the line under way.
struct Lines {
    line_height: f32,
    baseline_in_line: f32,
    lines: Vec<Line>,
    /// The ink width of each finished line (its trailing white space left out).
    inks: Vec<f32>,
    glyphs: Vec<PlacedGlyph>,
    start: usize,
    pen: f32,
    /// The pen after the last character that is not white space.
    ink: f32,
}

impl Lines {
    fn finish(&mut self, end: usize, wrapped: bool) {
        let top = self.lines.len() as f32 * self.line_height;
        self.lines.push(Line {
            top,
            baseline: top + self.baseline_in_line,
            width: self.pen,
            start: self.start,
            end,
            wrapped,
            glyphs: std::mem::take(&mut self.glyphs),
        });
        self.inks.push(self.ink);
        self.start = end;
        self.pen = 0.0;
        self.ink = 0.0;
    }
}

/// Where the line under way may break: before its glyph `glyph`, at the text offset `offset`, the
/// pen and the ink being then `pen` and `ink`.
#[derive(Debug, Clone, Copy)]
struct Break {
    glyph: usize,
    offset: usize,
    pen: f32,
    ink: f32,
}

/// Lays `text` out (see the module documentation).
pub fn layout(text: &str, style: TextStyle) -> Layout {
    let size = style.size();
    let wrap = style.wrap_width();
    let line_height = size * LINE_HEIGHT;
    let primary = face(FontFace::of(style.family, style.bold));
    let (ascent, descent) = primary.map_or((0.8 * size, 0.2 * size), |f| {
        let s = units_scale(f, size);
        (f32::from(f.ascender()) * s, -f32::from(f.descender()) * s)
    });
    // CSS: half the leading above the content area, whose top is the ascent above the baseline.
    let baseline_in_line = (line_height - (ascent + descent)) / 2.0 + ascent;
    let space = resolve(' ', &style, size).map_or(size / 4.0, |(_, _, width, _)| width);
    let tab = space * TAB_SPACES;

    let mut out = Lines {
        line_height,
        baseline_in_line,
        lines: Vec::new(),
        inks: Vec::new(),
        glyphs: Vec::new(),
        start: 0,
        pen: 0.0,
        ink: 0.0,
    };
    // Where the line under way may break (after white space), and whether white space ends it.
    let mut opportunity: Option<Break> = None;
    let mut after_space = false;
    let mut index = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let at = index;
        index += c.len_utf16();
        if c == '\n' || c == '\r' {
            out.finish(at, false);
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
                index += 1;
            }
            out.start = index;
            opportunity = None;
            after_space = false;
            continue;
        }
        if is_break_space(c) {
            after_space = true;
            if c == '\t' {
                out.pen = if tab > 0.0 {
                    ((out.pen / tab).floor() + 1.0) * tab
                } else {
                    out.pen
                };
                continue;
            }
            if is_invisible(c) {
                continue;
            }
            if let Some((face, glyph, width, look)) = resolve(c, &style, size) {
                out.glyphs.push(PlacedGlyph {
                    face,
                    glyph: glyph.0,
                    x: out.pen,
                    advance: width,
                    index: at,
                    look,
                });
                out.pen += width;
            }
            continue;
        }
        if is_invisible(c) {
            continue;
        }
        let Some((face, glyph, width, look)) = resolve(c, &style, size) else {
            continue;
        };
        if after_space {
            after_space = false;
            opportunity = Some(Break {
                glyph: out.glyphs.len(),
                offset: at,
                pen: out.pen,
                ink: out.ink,
            });
        }
        if let Some(limit) = wrap
            && crosses(out.pen + width, limit)
        {
            // The word under way goes to the next line, the white space before it stays.
            if let Some(at_break) = opportunity.take().filter(|b| b.glyph > 0) {
                let tail = out.glyphs.split_off(at_break.glyph);
                out.pen = at_break.pen;
                out.ink = at_break.ink;
                out.finish(at_break.offset, true);
                // Placed again from the start of the line (no tab among them: a tab is a break), so
                // that a line has the same positions whichever way it was reached.
                for glyph in tail {
                    out.glyphs.push(PlacedGlyph {
                        x: out.pen,
                        ..glyph
                    });
                    out.pen += glyph.advance;
                }
                out.ink = out.pen;
            }
            // A word wider than the line breaks before the character that crosses it.
            if crosses(out.pen + width, limit) && !out.glyphs.is_empty() {
                out.finish(at, true);
            }
        }
        out.glyphs.push(PlacedGlyph {
            face,
            glyph: glyph.0,
            x: out.pen,
            advance: width,
            index: at,
            look,
        });
        out.pen += width;
        out.ink = out.pen;
    }
    out.finish(index, false);
    let width = match wrap {
        None => out.lines.iter().map(|l| l.width).fold(0.0_f32, f32::max),
        Some(limit) => out
            .lines
            .iter()
            .zip(&out.inks)
            .map(|(line, &ink)| ink.max(line.width.min(limit)))
            .fold(0.0_f32, f32::max),
    };
    Layout {
        style,
        height: out.lines.len() as f32 * line_height,
        lines: out.lines,
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
            let Some(f) = face(glyph.face) else {
                continue;
            };
            let scale = units_scale(f, size);
            let id = GlyphId(glyph.glyph);
            match glyph.look {
                GlyphLook::Plain | GlyphLook::Missing => {
                    let mut collect = Collect {
                        path: &mut path,
                        x: glyph.x,
                        baseline: line.baseline,
                        scale,
                    };
                    let _ = f.outline_glyph(id, &mut collect);
                }
                GlyphLook::Emboldened => {
                    let mut own = Path::default();
                    let mut collect = Collect {
                        path: &mut own,
                        x: glyph.x,
                        baseline: line.baseline,
                        scale,
                    };
                    let _ = f.outline_glyph(id, &mut collect);
                    path.extend(embolden(&own, EMBOLDEN * size));
                }
                GlyphLook::Superscript | GlyphLook::Subscript => {
                    let Some(metrics) = script_metrics(glyph.face) else {
                        continue;
                    };
                    let reduced = scale * metrics.scale;
                    let shift = if glyph.look == GlyphLook::Superscript {
                        metrics.raise
                    } else {
                        -metrics.drop
                    };
                    let own_advance = advance(f, id, reduced);
                    let mut collect = Collect {
                        path: &mut path,
                        x: glyph.x + (glyph.advance - own_advance) / 2.0,
                        baseline: line.baseline - shift * scale,
                        scale: reduced,
                    };
                    let _ = f.outline_glyph(id, &mut collect);
                }
            }
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

/// The contours of `path` grown by `by` on each side (synthetic bold, after FreeType's
/// `FT_Outline_EmboldenXY`): each point, control points included, moves out along the bisector
/// of the edges that meet at it, so that each edge moves out by `by`; the holes shrink as the
/// outer contours grow. A point where the outline turns back on itself does not move, and no
/// point moves farther than twice `by`.
pub fn embolden(path: &Path, by: f32) -> Path {
    // The contours as point lists (every point of a segment, control points included).
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    for segment in &path.segments {
        match *segment {
            Segment::MoveTo(p) => contours.push(vec![p]),
            Segment::LineTo(p) => contours.last_mut().into_iter().for_each(|c| c.push(p)),
            Segment::QuadTo(c1, p) => contours
                .last_mut()
                .into_iter()
                .for_each(|c| c.extend([c1, p])),
            Segment::CubicTo(c1, c2, p) => contours
                .last_mut()
                .into_iter()
                .for_each(|c| c.extend([c1, c2, p])),
            Segment::Close => {}
        }
    }
    // Which way is out: the outer contours' turn decides (their area outweighs the holes').
    let area: f32 = contours
        .iter()
        .map(|points| {
            (0..points.len())
                .map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % points.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum::<f32>()
        })
        .sum();
    let out = if area >= 0.0 { 1.0 } else { -1.0 };
    let moved: Vec<Vec<[f32; 2]>> = contours
        .iter()
        .map(|points| {
            let n = points.len();
            (0..n)
                .map(|i| {
                    let p = points[i];
                    // The nearest other points before and after (repeated points skipped).
                    let other = |step: isize| {
                        (1..n).find_map(|k| {
                            let j = (i as isize + step * k as isize).rem_euclid(n as isize);
                            let q = points[j as usize];
                            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
                            let length = dx.hypot(dy);
                            (length > 1e-6).then_some((dx / length, dy / length, length))
                        })
                    };
                    let (Some(before), Some(after)) = (other(-1), other(1)) else {
                        return p;
                    };
                    // Directions in and out of the point, and their outward normals.
                    let incoming = (-before.0, -before.1);
                    let outgoing = (after.0, after.1);
                    let normal = |d: (f32, f32)| (out * d.1, -out * d.0);
                    let (na, nb) = (normal(incoming), normal(outgoing));
                    let cos = incoming.0 * outgoing.0 + incoming.1 * outgoing.1;
                    if cos <= -0.9375 {
                        return p;
                    }
                    let k = by / (1.0 + na.0 * nb.0 + na.1 * nb.1).max(1e-3);
                    let (mut sx, mut sy) = ((na.0 + nb.0) * k, (na.1 + nb.1) * k);
                    let length = sx.hypot(sy);
                    if length > 2.0 * by && length > 0.0 {
                        let f = 2.0 * by / length;
                        sx *= f;
                        sy *= f;
                    }
                    [p[0] + sx, p[1] + sy]
                })
                .collect()
        })
        .collect();
    // The same segments with the moved points.
    fn next(points: &mut std::slice::Iter<'_, [f32; 2]>) -> [f32; 2] {
        points.next().copied().unwrap_or([0.0; 2])
    }
    let mut result = Path::default();
    let mut contour = moved.iter();
    let mut points: std::slice::Iter<'_, [f32; 2]> = [].iter();
    for segment in &path.segments {
        match *segment {
            Segment::MoveTo(_) => {
                points = contour.next().map_or([].iter(), |c| c.iter());
                result.segments.push(Segment::MoveTo(next(&mut points)));
            }
            Segment::LineTo(_) => result.segments.push(Segment::LineTo(next(&mut points))),
            Segment::QuadTo(..) => {
                let c = next(&mut points);
                result.segments.push(Segment::QuadTo(c, next(&mut points)));
            }
            Segment::CubicTo(..) => {
                let c1 = next(&mut points);
                let c2 = next(&mut points);
                result
                    .segments
                    .push(Segment::CubicTo(c1, c2, next(&mut points)));
            }
            Segment::Close => result.segments.push(Segment::Close),
        }
    }
    result
}

#[cfg(test)]
mod tests;
