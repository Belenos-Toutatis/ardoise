//! The frames of a replay, drawn incrementally (SPEC §8: a 2-minute 1080p replay must export
//! in under 2 minutes, so a frame cannot redraw the note).
//!
//! Two pages are kept: the cache (the background and the finished strokes at the front of the
//! drawing order) and the picture (the current frame's page: the cache with the other strokes
//! begun so far on top). For each frame, the region where something changed (the strokes
//! that grew, their previous and new bounds) is grown until every stroke reaching into it lies
//! inside; that region of the picture is restored from the cache and the strokes in it are
//! drawn again, in drawing order. A stroke only changes pixels within its bounds, so every
//! pixel of the picture is drawn from the same strokes in the same order as by `render_note`.
//!
//! Strokes are composited in the exports' drawing order, not in the order they were written:
//! a finished stroke joins the cache only once every stroke before it in drawing order is
//! finished too; until then it waits with the strokes in progress (its geometry is kept, and
//! it is only drawn again when the changing region reaches it). Each frame is therefore
//! exactly `render_note` of [`Timeline::note_at`], except that a grain stroke in progress
//! (pencil, chalk) already has the grain position and opacity of the complete stroke
//! ([`ardoise_render::Look::partial_fills`]) instead of shimmering as it grows.

use std::collections::HashMap;
use std::time::Duration;

use ardoise_render::{
    Backdrop, NoteView, PageCanvas, Picture, PixelRect, PreparedStroke, image_size, paper_color,
};

use super::timeline::{Timeline, frame_time};
use crate::{MediaError, VideoSettings};

/// Largest frame side, in pixels (the largest image `ardoise-render` draws).
const MAX_FRAME_SIDE: u32 = 16_384;
/// Most finished strokes already shown whole that join the cache in one frame.
const CATCH_UP_PER_FRAME: usize = 16;

/// The frames to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSettings {
    /// Frame size in pixels. The page keeps its proportions: it is drawn as large as fits,
    /// centred, and the bands left on two sides are filled with the plain paper colour
    /// (white or transparent for those backdrops), without the template or the texture.
    pub width: u32,
    pub height: u32,
    /// Frames per second.
    pub frame_rate: u32,
    /// What is under the strokes. `Backdrop::Paper` and `Backdrop::White` give opaque frames;
    /// `Backdrop::Transparent` gives straight alpha.
    pub backdrop: Backdrop,
}

impl FrameSettings {
    /// The frames of a video export: its size and frame rate, on the note's paper.
    pub fn for_video(video: &VideoSettings) -> Self {
        Self {
            width: video.width,
            height: video.height,
            frame_rate: video.frame_rate,
            backdrop: Backdrop::Paper,
        }
    }
}

/// Where the page lies in the frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePlacement {
    /// Top-left corner, in pixels from the frame's top-left corner.
    pub x: u32,
    pub y: u32,
    /// Size of the page in pixels: `ardoise_render::image_size(page, scale)`.
    pub width: u32,
    pub height: u32,
    /// Pixels per device unit (10 µm): `render_note` at this scale draws the same page.
    pub scale: f32,
}

/// A stroke on the picture beyond the cache.
#[derive(Debug, Clone)]
struct Drawn {
    /// Index into `note.strokes`.
    index: usize,
    /// Samples drawn.
    samples: usize,
    /// Its geometry, to draw it again.
    stroke: PreparedStroke,
}

/// Draws the frames of a replay, in RGBA.
///
/// Frames are fastest in increasing time order (an export); going back in time draws the
/// page again from the background.
pub struct FrameRenderer<'n> {
    timeline: Timeline<'n>,
    settings: FrameSettings,
    placement: PagePlacement,
    /// The background alone.
    paper: PageCanvas,
    /// The background and the first `cached` strokes of the drawing order, all finished.
    cache: PageCanvas,
    cached: usize,
    /// The page of the current frame: the cache with the strokes of `drawn` on top.
    picture: PageCanvas,
    /// The strokes begun beyond the cache, in drawing order.
    drawn: Vec<Drawn>,
    /// The current frame: the picture at its place, the bands around it.
    frame: Vec<u8>,
    /// False until the picture has been written into the frame (first frame, after going
    /// back in time).
    up_to_date: bool,
    /// Replay instant of the last frame drawn.
    last: Duration,
}

impl<'n> FrameRenderer<'n> {
    /// A renderer of the frames of `timeline`. Fails with [`MediaError::InvalidSettings`]
    /// for an empty or larger than 16 384 px frame or a zero frame rate, and with
    /// [`MediaError::Render`] when the page cannot be drawn at that size (an empty page, or
    /// one so elongated that a side would round to nothing).
    pub fn new(timeline: Timeline<'n>, settings: &FrameSettings) -> Result<Self, MediaError> {
        Self::with_picture(timeline, settings, None)
    }

    /// Like [`FrameRenderer::new`], with the pixels of the note's background image (drawn
    /// under the strokes in every frame on [`Backdrop::Paper`]).
    pub fn with_picture(
        timeline: Timeline<'n>,
        settings: &FrameSettings,
        picture: Option<&Picture>,
    ) -> Result<Self, MediaError> {
        let (width, height) = (settings.width, settings.height);
        if width == 0 || height == 0 || width > MAX_FRAME_SIDE || height > MAX_FRAME_SIDE {
            return Err(MediaError::InvalidSettings(format!(
                "frames of {width} x {height} px (1 to {MAX_FRAME_SIDE} px a side)"
            )));
        }
        if settings.frame_rate == 0 {
            return Err(MediaError::InvalidSettings("a frame rate of 0".into()));
        }
        let note = timeline.note();
        let page = note.page;
        // As large as fits; the rounded page size must not exceed the frame.
        let mut scale = (width as f32 / page.width as f32).min(height as f32 / page.height as f32);
        for _ in 0..4 {
            let (w, h) = image_size(page, scale);
            if w <= width && h <= height {
                break;
            }
            scale = scale.next_down();
        }
        let paper = PageCanvas::new(NoteView::new(note, picture), scale, settings.backdrop)?;
        let (Some(free_x), Some(free_y)) = (
            width.checked_sub(paper.width()),
            height.checked_sub(paper.height()),
        ) else {
            return Err(MediaError::InvalidSettings(format!(
                "the page does not fit in frames of {width} x {height} px"
            )));
        };
        let placement = PagePlacement {
            x: free_x / 2,
            y: free_y / 2,
            width: paper.width(),
            height: paper.height(),
            scale,
        };
        let band = match settings.backdrop {
            Backdrop::Paper => {
                let c = paper_color(note.background.paper);
                [c.r, c.g, c.b, 255]
            }
            Backdrop::White => [255; 4],
            Backdrop::Transparent => [0; 4],
        };
        let frame = band.repeat(width as usize * height as usize);
        Ok(Self {
            timeline,
            settings: *settings,
            placement,
            cache: paper.clone(),
            cached: 0,
            picture: paper.clone(),
            paper,
            drawn: Vec::new(),
            frame,
            up_to_date: false,
            last: Duration::ZERO,
        })
    }

    pub fn timeline(&self) -> &Timeline<'n> {
        &self.timeline
    }

    pub fn settings(&self) -> FrameSettings {
        self.settings
    }

    /// Where the page lies in the frames.
    pub fn placement(&self) -> PagePlacement {
        self.placement
    }

    /// Number of frames of the replay ([`Timeline::frame_count`]); the last one shows the
    /// whole note.
    pub fn frame_count(&self) -> u64 {
        self.timeline.frame_count(self.settings.frame_rate)
    }

    /// Replay instant (timestamp) of frame `index`.
    pub fn frame_time(&self, index: u64) -> Duration {
        frame_time(index, self.settings.frame_rate)
    }

    /// Frame `index`: `width × height` RGBA pixels, rows top to bottom.
    pub fn render_frame(&mut self, index: u64) -> Result<&[u8], MediaError> {
        self.render_at(self.frame_time(index))
    }

    /// The frame at replay instant `replay`: `width × height` RGBA pixels, rows top to bottom.
    pub fn render_at(&mut self, replay: Duration) -> Result<&[u8], MediaError> {
        if replay < self.last {
            self.cache.copy_from(&self.paper);
            self.picture.copy_from(&self.paper);
            self.cached = 0;
            self.drawn.clear();
            self.up_to_date = false;
        }
        self.last = replay;
        let note = self.timeline.note();
        let order = self.timeline.drawing_order();
        let tau = self.timeline.to_note(replay);
        // Where the picture changes.
        let mut dirty = if self.up_to_date {
            PixelRect::default()
        } else {
            self.picture.rect()
        };
        let mut previous: HashMap<usize, Drawn> =
            self.drawn.drain(..).map(|d| (d.index, d)).collect();

        // Finished strokes at the front of the drawing order join the cache for good. The
        // picture only changes where one was not already drawn whole. Strokes the picture
        // already shows whole (those that waited for an earlier stroke) join a few at a time,
        // so that a long wait ending does not make one slow frame.
        let mut catch_up = CATCH_UP_PER_FRAME;
        while let Some(&index) = order.get(self.cached) {
            if !self.timeline.is_complete(index, tau) {
                break;
            }
            let Some(stroke) = note.strokes.get(index) else {
                self.cached += 1;
                continue;
            };
            match previous.remove(&index) {
                Some(d) if d.samples == stroke.points.len() => {
                    if catch_up == 0 {
                        previous.insert(index, d);
                        break;
                    }
                    catch_up -= 1;
                    self.cache.draw_prepared(&d.stroke);
                }
                old => {
                    let bounds = self.cache.draw_stroke(stroke);
                    let old = old.and_then(|d| d.stroke.bounds());
                    dirty = dirty.union(old.unwrap_or_default());
                    dirty = dirty.union(bounds.unwrap_or_default());
                }
            }
            self.cached += 1;
        }

        // The other strokes begun so far, in drawing order: those that grew are prepared
        // again, the others keep their geometry.
        let mut drawn = Vec::with_capacity(previous.len() + 1);
        for &index in order.get(self.cached..).unwrap_or_default() {
            let samples = self.timeline.samples_shown(index, tau);
            if samples == 0 {
                continue;
            }
            let old = previous.remove(&index);
            match (old, note.strokes.get(index)) {
                (Some(d), _) if d.samples == samples => drawn.push(d),
                (old, Some(stroke)) => {
                    let prepared = self.picture.prepare_stroke(stroke, samples);
                    let old = old.and_then(|d| d.stroke.bounds());
                    dirty = dirty.union(old.unwrap_or_default());
                    dirty = dirty.union(prepared.bounds().unwrap_or_default());
                    drawn.push(Drawn {
                        index,
                        samples,
                        stroke: prepared,
                    });
                }
                (_, None) => {}
            }
        }

        if !dirty.is_empty() {
            // Grown until every stroke reaching into it is inside, so that drawing those
            // strokes again changes nothing outside it.
            loop {
                let grown = drawn
                    .iter()
                    .filter_map(|d| d.stroke.bounds())
                    .filter(|b| b.intersects(dirty))
                    .fold(dirty, PixelRect::union);
                if grown == dirty {
                    break;
                }
                dirty = grown;
            }
            self.picture.copy_rect_from(&self.cache, dirty);
            for d in &drawn {
                if d.stroke.bounds().is_some_and(|b| b.intersects(dirty)) {
                    self.picture.draw_prepared(&d.stroke);
                }
            }
            let (x, y) = (self.placement.x, self.placement.y);
            self.picture
                .write_rgba_rect(&mut self.frame, self.settings.width, x, y, dirty)?;
        }
        self.drawn = drawn;
        self.up_to_date = true;
        Ok(&self.frame)
    }
}
