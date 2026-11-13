//! A page drawn stroke by stroke: the drawing primitive of animations (the replay frames of
//! `ardoise-media`). [`render_note`](crate::render_note) is built on it, so a canvas given
//! the strokes `render_note` draws ([`drawing_order`](crate::drawing_order)), in the same
//! order, holds exactly the same pixels (SPEC §6.2, "un seul dessin des traits").
//!
//! An animation keeps one canvas with what no longer changes (the background and the finished
//! strokes) and a second one with the current picture. Drawing a stroke only touches the
//! pixels inside its bounds ([`PreparedStroke::bounds`]), so a frame can restore a region of
//! the picture from the first canvas ([`PageCanvas::copy_rect_from`]), draw again the strokes
//! that reach into it ([`PageCanvas::draw_prepared`], a stroke in progress with
//! [`PageCanvas::prepare_stroke`]) and send only that region on
//! ([`PageCanvas::write_rgba_rect`]).

use ardoise_core::note::PageSize;
use ardoise_ink::Stroke;
use tiny_skia::{Color, Pixmap, Rect};

use crate::ink::{InkPainter, Prepared};
use crate::picture::{self, NoteView};
use crate::{Image, MAX_SIDE, RenderError, background, image_from};

/// A rectangle of pixels: columns `x .. x + width`, rows `y .. y + height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// The smallest rectangle holding both (an empty rectangle adds nothing).
    pub fn union(self, other: PixelRect) -> PixelRect {
        if other.is_empty() {
            return self;
        }
        if self.is_empty() {
            return other;
        }
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        PixelRect {
            x,
            y,
            width: self.right().max(other.right()) - x,
            height: self.bottom().max(other.bottom()) - y,
        }
    }

    /// True if both hold a common pixel.
    pub fn intersects(self, other: PixelRect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// True if every pixel of `other` is in `self` (an empty rectangle is in any).
    pub fn contains(self, other: PixelRect) -> bool {
        other.is_empty()
            || (self.x <= other.x
                && self.y <= other.y
                && other.right() <= self.right()
                && other.bottom() <= self.bottom())
    }

    /// The part of `self` inside `width × height`.
    fn clipped(self, width: u32, height: u32) -> PixelRect {
        let (x, y) = (self.x.min(width), self.y.min(height));
        PixelRect {
            x,
            y,
            width: self.right().min(width) - x,
            height: self.bottom().min(height) - y,
        }
    }
}

/// A stroke ready to be drawn on a [`PageCanvas`]: its fills with their paths in pixels,
/// computed once ([`PageCanvas::prepare_stroke`]) to be drawn as often as needed, on that
/// canvas or on any canvas of the same scale and backdrop (a copy of it).
#[derive(Debug, Clone)]
pub struct PreparedStroke {
    prepared: Prepared,
    bounds: Option<PixelRect>,
}

impl PreparedStroke {
    /// The pixels drawing it can change (anti-aliasing included, clipped to the page); `None`
    /// when nothing of it is drawn (no usable sample, or off the page).
    pub fn bounds(&self) -> Option<PixelRect> {
        self.bounds
    }
}

/// What is under the strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backdrop {
    /// The note's paper colour, template and texture, and its background image when the view
    /// has its pixels (`RenderOptions::background` on).
    Paper,
    /// A plain white page (`RenderOptions::background` off).
    White,
    /// Nothing: the strokes on transparent pixels
    /// ([`render_note_transparent`](crate::render_note_transparent)).
    Transparent,
}

/// Size in pixels of a page drawn at `scale` pixels per device unit (each side rounded): the
/// size of the images of [`render_note`](crate::render_note) and of a [`PageCanvas`].
pub fn image_size(page: PageSize, scale: f32) -> (u32, u32) {
    // `as` saturates: a NaN or negative scale gives 0, which callers refuse.
    (
        (page.width as f32 * scale).round() as u32,
        (page.height as f32 * scale).round() as u32,
    )
}

/// A page at a fixed scale onto which strokes are drawn one at a time, each over what is
/// already there. Layers are not looked at: the caller chooses the strokes and their order.
#[derive(Clone)]
pub struct PageCanvas {
    pixmap: Pixmap,
    painter: InkPainter,
    backdrop: Backdrop,
}

impl PageCanvas {
    /// The background of the note (nothing for [`Backdrop::Transparent`]; with
    /// [`Backdrop::Paper`], its background image over the paper when the view has its pixels)
    /// on a page of [`image_size`] pixels, at most 16 384 px a side.
    pub fn new<'a>(
        view: impl Into<NoteView<'a>>,
        scale: f32,
        backdrop: Backdrop,
    ) -> Result<Self, RenderError> {
        let view = view.into();
        let note = view.note;
        let (width, height) = image_size(note.page, scale);
        let size_error = RenderError::Size { width, height };
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(size_error);
        }
        let mut pixmap = Pixmap::new(width, height).ok_or(size_error)?;
        let dark = match backdrop {
            Backdrop::Paper => {
                background::paint(&mut pixmap, &note.background, scale);
                if let Some(placed) = view.placed() {
                    picture::paint(&mut pixmap, &placed, scale);
                }
                background::is_dark(note.background.paper)
            }
            Backdrop::White => {
                pixmap.fill(Color::WHITE);
                false
            }
            Backdrop::Transparent => false,
        };
        let painter = InkPainter::new(&pixmap, scale, dark);
        Ok(Self {
            pixmap,
            painter,
            backdrop,
        })
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    pub fn backdrop(&self) -> Backdrop {
        self.backdrop
    }

    /// The whole page.
    pub fn rect(&self) -> PixelRect {
        PixelRect {
            x: 0,
            y: 0,
            width: self.width(),
            height: self.height(),
        }
    }

    /// The first `points` samples of `stroke` (all of them when `points` is not smaller than
    /// their number), ready to draw. A cut stroke looks as it does while being written: the
    /// outline of those samples with the grain position and the pencil opacity of the whole
    /// stroke ([`Look::partial_fills`](crate::Look::partial_fills)), so that it does not
    /// shimmer as it grows.
    pub fn prepare_stroke(&self, stroke: &Stroke, points: usize) -> PreparedStroke {
        let fills = self.painter.look().partial_fills(stroke, points);
        let prepared = self.painter.prepare(fills);
        let bounds = prepared.bounds().and_then(|b| self.pixel_bounds(b));
        PreparedStroke { prepared, bounds }
    }

    /// Draws a prepared stroke over what is there. Only pixels within its bounds change.
    pub fn draw_prepared(&mut self, stroke: &PreparedStroke) {
        self.painter.paint(&mut self.pixmap, &stroke.prepared);
    }

    /// Draws `stroke` over what is there, as [`render_note`](crate::render_note) draws it;
    /// returns the pixels it can have changed ([`PreparedStroke::bounds`]).
    pub fn draw_stroke(&mut self, stroke: &Stroke) -> Option<PixelRect> {
        self.draw_stroke_prefix(stroke, stroke.points.len())
    }

    /// Draws the first `points` samples of `stroke` ([`PageCanvas::prepare_stroke`]); returns
    /// the pixels it can have changed.
    pub fn draw_stroke_prefix(&mut self, stroke: &Stroke, points: usize) -> Option<PixelRect> {
        let prepared = self.prepare_stroke(stroke, points);
        self.draw_prepared(&prepared);
        prepared.bounds
    }

    /// Pixels a path with these bounds can touch: one more pixel all around for the
    /// anti-aliasing, clipped to the page.
    fn pixel_bounds(&self, bounds: Rect) -> Option<PixelRect> {
        let (width, height) = (self.width() as f32, self.height() as f32);
        // `as` saturates (and maps NaN to 0).
        let x0 = (bounds.left().floor() - 1.0).clamp(0.0, width) as u32;
        let y0 = (bounds.top().floor() - 1.0).clamp(0.0, height) as u32;
        let x1 = (bounds.right().ceil() + 1.0).clamp(0.0, width) as u32;
        let y1 = (bounds.bottom().ceil() + 1.0).clamp(0.0, height) as u32;
        let rect = PixelRect {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0),
            height: y1.saturating_sub(y0),
        };
        (!rect.is_empty()).then_some(rect)
    }

    /// True if `other` has the same size, scale and backdrop.
    fn same_kind(&self, other: &PageCanvas) -> bool {
        self.pixmap.width() == other.pixmap.width()
            && self.pixmap.height() == other.pixmap.height()
            && self.painter.look() == other.painter.look()
            && self.backdrop == other.backdrop
    }

    /// Makes this canvas an exact copy of `other`, reusing its memory when both have the same
    /// size, scale and backdrop.
    pub fn copy_from(&mut self, other: &PageCanvas) {
        if self.same_kind(other) {
            self.pixmap.data_mut().copy_from_slice(other.pixmap.data());
        } else {
            *self = other.clone();
        }
    }

    /// Copies the pixels of `rect` (clipped to the page) from `other`, a canvas of the same
    /// size, scale and backdrop; from one of another kind, copies all of it
    /// ([`PageCanvas::copy_from`]).
    pub fn copy_rect_from(&mut self, other: &PageCanvas, rect: PixelRect) {
        if !self.same_kind(other) {
            self.copy_from(other);
            return;
        }
        let rect = rect.clipped(self.width(), self.height());
        let stride = self.width() as usize * 4;
        let (start, len) = (rect.x as usize * 4, rect.width as usize * 4);
        let source = other.pixmap.data();
        let target = self.pixmap.data_mut();
        for row in rect.y as usize..rect.bottom() as usize {
            let range = row * stride + start..row * stride + start + len;
            if let (Some(to), Some(from)) = (target.get_mut(range.clone()), source.get(range)) {
                to.copy_from_slice(from);
            }
        }
    }

    /// The page as an image (straight alpha), as [`render_note`](crate::render_note) returns
    /// it.
    pub fn to_image(&self) -> Image {
        image_from(&self.pixmap)
    }

    /// Writes the page into `frame`, an RGBA image (straight alpha, rows top to bottom)
    /// `frame_width` pixels wide, with the page's top-left corner at `(x, y)`. The pixels
    /// written are those of [`PageCanvas::to_image`]; the rest of the frame is left as it is.
    pub fn write_rgba(
        &self,
        frame: &mut [u8],
        frame_width: u32,
        x: u32,
        y: u32,
    ) -> Result<(), RenderError> {
        self.write_rgba_rect(frame, frame_width, x, y, self.rect())
    }

    /// Like [`PageCanvas::write_rgba`], writing only the pixels of the page within `rect`
    /// (page coordinates, clipped to the page).
    pub fn write_rgba_rect(
        &self,
        frame: &mut [u8],
        frame_width: u32,
        x: u32,
        y: u32,
        rect: PixelRect,
    ) -> Result<(), RenderError> {
        let (width, height) = (self.width() as usize, self.height() as usize);
        let frame_width = frame_width as usize;
        let row_bytes = frame_width * 4;
        let frame_height = frame.len().checked_div(row_bytes).unwrap_or(0);
        let fits = row_bytes > 0
            && frame.len().is_multiple_of(row_bytes)
            && x as usize + width <= frame_width
            && y as usize + height <= frame_height;
        if !fits {
            return Err(RenderError::Frame(format!(
                "a page of {width} x {height} px at ({x}, {y}) does not fit in {} bytes of \
                 {frame_width} px rows",
                frame.len()
            )));
        }
        let rect = rect.clipped(self.width(), self.height());
        let (left, span) = (rect.x as usize, rect.width as usize);
        let (data, pixels) = (self.pixmap.data(), self.pixmap.pixels());
        for row in rect.y as usize..rect.bottom() as usize {
            let start = ((y as usize + row) * frame_width + x as usize + left) * 4;
            let out = frame.get_mut(start..start + span * 4);
            let source = row * width + left..row * width + left + span;
            match (self.backdrop, out) {
                // On an opaque page every pixel stays opaque (source-over, multiply and screen
                // all keep alpha at 1), and opaque premultiplied pixels are straight ones.
                (Backdrop::Paper | Backdrop::White, Some(out)) => {
                    if let Some(bytes) = data.get(source.start * 4..source.end * 4) {
                        out.copy_from_slice(bytes);
                    }
                }
                (Backdrop::Transparent, Some(out)) => {
                    let pixels = pixels.get(source).unwrap_or_default();
                    for (out, pixel) in out.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
                        let c = pixel.demultiply();
                        *out = [c.red(), c.green(), c.blue(), c.alpha()];
                    }
                }
                (_, None) => break,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderOptions, drawing_order, render_note, render_note_transparent};
    use ardoise_core::note::{Background, Layer, Note, Origin, Paper, Template, Timing};
    use ardoise_ink::{Point, Rgba, Tool};

    const PAGE: PageSize = PageSize {
        width: 6000,
        height: 4000,
    };

    /// A wavy line of `n` samples, one tool per line.
    fn wave(tool: Tool, layer: u32, y: f32, n: u32) -> Stroke {
        let points = (0..n)
            .map(|i| Point {
                x: 400.0 + i as f32 * (5200.0 / n as f32),
                y: y + 300.0 * (i as f32 * 0.3).sin(),
                pressure: 0.3 + 0.6 * (i as f32 / n as f32),
                t: i * 5,
            })
            .collect();
        Stroke {
            tool,
            color: Rgba::rgb(30, 60, 150),
            width: if tool == Tool::Highlighter {
                400.0
            } else {
                tool.default_width()
            },
            layer,
            points,
        }
    }

    fn note() -> Note {
        let mut strokes: Vec<Stroke> = Tool::ALL
            .iter()
            .enumerate()
            .map(|(i, &tool)| wave(tool, (i % 2) as u32, 500.0 + i as f32 * 450.0, 40))
            .collect();
        // Crossing every other line, on the bottom layer but last in the note.
        strokes.push(Stroke {
            points: (0..30)
                .map(|i| Point {
                    x: 3000.0 + i as f32 * 10.0,
                    y: 200.0 + i as f32 * 120.0,
                    pressure: 1.0,
                    t: 400 + i,
                })
                .collect(),
            ..wave(Tool::Highlighter, 0, 0.0, 1)
        });
        Note {
            id: "00000000-0000-4000-8000-000000000000".into(),
            title: "canvas".into(),
            created: 0,
            modified: 0,
            page: PAGE,
            background: Background {
                paper: Paper::Kraft,
                template: Template::Seyes,
                texture: true,
            },
            background_image: None,
            tags: vec![],
            favorite: false,
            layers: vec![
                Layer {
                    name: "1".into(),
                    visible: true,
                },
                Layer {
                    name: "2".into(),
                    visible: true,
                },
            ],
            strokes,
            objects: Vec::new(),
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
    }

    fn draw_all(note: &Note, scale: f32, backdrop: Backdrop) -> PageCanvas {
        let mut canvas = PageCanvas::new(note, scale, backdrop).unwrap();
        for i in drawing_order(note) {
            canvas.draw_stroke(&note.strokes[i]);
        }
        canvas
    }

    #[test]
    fn the_canvas_draws_what_render_note_draws() {
        let note = note();
        for (backdrop, background) in [(Backdrop::Paper, true), (Backdrop::White, false)] {
            let options = RenderOptions {
                scale: 0.07,
                background,
            };
            let canvas = draw_all(&note, 0.07, backdrop);
            let expected = render_note(&note, &options).unwrap();
            assert_eq!(canvas.to_image(), expected, "{backdrop:?}");
            assert_eq!(
                (canvas.width(), canvas.height()),
                image_size(note.page, 0.07)
            );
            // Written into a larger frame: the same pixels at the given place.
            let (fw, fh) = (expected.width + 7, expected.height + 3);
            let mut frame = vec![9u8; (fw * fh * 4) as usize];
            canvas.write_rgba(&mut frame, fw, 5, 2).unwrap();
            for y in 0..fh {
                for x in 0..fw {
                    let i = ((y * fw + x) * 4) as usize;
                    let inside = (5..5 + expected.width).contains(&x)
                        && (2..2 + expected.height).contains(&y);
                    let want = if inside {
                        expected.pixel(x - 5, y - 2).unwrap()
                    } else {
                        [9; 4]
                    };
                    assert_eq!(frame[i..i + 4], want, "({x}, {y})");
                }
            }
        }
        let transparent = draw_all(&note, 0.05, Backdrop::Transparent);
        let expected = render_note_transparent(&note, 0.05).unwrap();
        assert_eq!(transparent.to_image(), expected);
        let mut frame = vec![0u8; expected.pixels.len()];
        transparent
            .write_rgba(&mut frame, expected.width, 0, 0)
            .unwrap();
        assert_eq!(frame, expected.pixels);
    }

    #[test]
    fn a_whole_prefix_is_the_stroke_and_copies_are_exact() {
        let note = note();
        let order = drawing_order(&note);
        let mut whole = PageCanvas::new(&note, 0.05, Backdrop::Paper).unwrap();
        let mut prefixes = whole.clone();
        for &i in &order {
            let stroke = &note.strokes[i];
            whole.draw_stroke(stroke);
            prefixes.draw_stroke_prefix(stroke, stroke.points.len());
        }
        assert_eq!(whole.to_image(), prefixes.to_image());

        // A copy of another size, scale or backdrop takes them over.
        let mut other = PageCanvas::new(&note, 0.02, Backdrop::Transparent).unwrap();
        other.copy_from(&whole);
        assert_eq!(other.to_image(), whole.to_image());
        assert_eq!(other.backdrop(), Backdrop::Paper);
        // A copy of the same kind reuses its memory and restores the pixels.
        let blank = PageCanvas::new(&note, 0.05, Backdrop::Paper).unwrap();
        other.copy_from(&blank);
        assert_eq!(other.to_image(), blank.to_image());
        assert_ne!(other.to_image(), whole.to_image());
    }

    #[test]
    fn a_stroke_only_changes_pixels_within_its_bounds() {
        let note = note();
        let base = PageCanvas::new(&note, 0.06, Backdrop::Paper).unwrap();
        let before = base.to_image();
        for stroke in &note.strokes {
            for points in [1, 7, stroke.points.len()] {
                let mut canvas = base.clone();
                let bounds = canvas.draw_stroke_prefix(stroke, points).unwrap();
                let after = canvas.to_image();
                let mut changed = 0;
                for (i, (a, b)) in after
                    .pixels
                    .chunks(4)
                    .zip(before.pixels.chunks(4))
                    .enumerate()
                {
                    if a != b {
                        changed += 1;
                        let (x, y) = (i as u32 % after.width, i as u32 / after.width);
                        let pixel = PixelRect {
                            x,
                            y,
                            width: 1,
                            height: 1,
                        };
                        assert!(bounds.contains(pixel), "{:?}: ({x}, {y})", stroke.tool);
                    }
                }
                assert!(changed > 0, "{:?}", stroke.tool);
                // Prepared once, drawn on a copy: the same pixels.
                let prepared = base.prepare_stroke(stroke, points);
                assert_eq!(prepared.bounds(), Some(bounds));
                let mut copy = base.clone();
                copy.draw_prepared(&prepared);
                assert_eq!(copy.to_image(), after, "{:?}", stroke.tool);
            }
        }
        let empty = Stroke {
            points: vec![],
            ..note.strokes[0].clone()
        };
        let mut canvas = base.clone();
        assert_eq!(canvas.draw_stroke(&empty), None);
        assert_eq!(canvas.to_image(), before);
    }

    #[test]
    fn a_region_restored_and_redrawn_is_exact() {
        let note = note();
        let order = drawing_order(&note);
        let paper = PageCanvas::new(&note, 0.05, Backdrop::Paper).unwrap();
        let prepared: Vec<PreparedStroke> = order
            .iter()
            .map(|&i| paper.prepare_stroke(&note.strokes[i], usize::MAX))
            .collect();
        let mut picture = paper.clone();
        for stroke in &prepared {
            picture.draw_prepared(stroke);
        }
        let expected = picture.to_image();
        assert_eq!(
            expected,
            render_note(
                &note,
                &RenderOptions {
                    scale: 0.05,
                    background: true
                }
            )
            .unwrap()
        );
        for start in 0..prepared.len() {
            // The bounds of one stroke, grown until every stroke reaching into it is inside.
            let mut region = prepared[start].bounds().unwrap();
            loop {
                let grown = prepared
                    .iter()
                    .filter_map(PreparedStroke::bounds)
                    .filter(|b| b.intersects(region))
                    .fold(region, PixelRect::union);
                if grown == region {
                    break;
                }
                region = grown;
            }
            let mut redrawn = picture.clone();
            redrawn.copy_rect_from(&paper, region);
            assert_ne!(redrawn.to_image(), expected);
            for stroke in &prepared {
                if stroke.bounds().is_some_and(|b| b.intersects(region)) {
                    redrawn.draw_prepared(stroke);
                }
            }
            assert_eq!(redrawn.to_image(), expected, "region of stroke {start}");
            // Only the region is written into a frame.
            let mut frame = vec![7u8; expected.pixels.len()];
            redrawn
                .write_rgba_rect(&mut frame, expected.width, 0, 0, region)
                .unwrap();
            for (i, (got, want)) in frame.chunks(4).zip(expected.pixels.chunks(4)).enumerate() {
                let (x, y) = (i as u32 % expected.width, i as u32 / expected.width);
                let inside = region.contains(PixelRect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                });
                assert_eq!(got, if inside { want } else { &[7u8; 4][..] }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn pixel_rectangles() {
        let r = |x, y, width, height| PixelRect {
            x,
            y,
            width,
            height,
        };
        let a = r(10, 10, 5, 5);
        assert_eq!(a.union(r(20, 0, 1, 1)), r(10, 0, 11, 15));
        assert_eq!(a.union(PixelRect::default()), a);
        assert_eq!(PixelRect::default().union(a), a);
        assert!(a.intersects(r(14, 14, 3, 3)));
        assert!(
            !a.intersects(r(15, 10, 3, 3)),
            "touching is not overlapping"
        );
        assert!(!a.intersects(r(12, 12, 0, 3)));
        assert!(a.contains(r(11, 11, 4, 4)) && !a.contains(r(11, 11, 5, 4)));
        assert!(a.contains(PixelRect::default()));
        assert_eq!(r(5, 5, 100, 100).clipped(50, 20), r(5, 5, 45, 15));
        assert_eq!(r(60, 5, 10, 10).clipped(50, 20), r(50, 5, 0, 10));
    }

    #[test]
    fn bad_sizes_and_frames_are_refused() {
        let note = note();
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 3.0] {
            assert!(
                matches!(
                    PageCanvas::new(&note, scale, Backdrop::Paper),
                    Err(RenderError::Size { .. })
                ),
                "{scale}"
            );
        }
        let canvas = PageCanvas::new(&note, 0.01, Backdrop::White).unwrap();
        assert_eq!((canvas.width(), canvas.height()), (60, 40));
        let mut frame = vec![0u8; 60 * 40 * 4];
        assert!(canvas.write_rgba(&mut frame, 60, 0, 0).is_ok());
        for (width, x, y) in [(60, 1, 0), (60, 0, 1), (59, 0, 0), (0, 0, 0), (7, 0, 0)] {
            assert!(
                matches!(
                    canvas.write_rgba(&mut frame, width, x, y),
                    Err(RenderError::Frame(_))
                ),
                "{width} {x} {y}"
            );
        }
    }
}
