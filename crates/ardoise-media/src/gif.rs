//! Animated GIF export (SPEC §4.5) with the `gif` crate (image-rs, pure Rust) and the NeuQuant
//! quantiser of `color_quant` (MIT); never `gifski`, whose licence is the AGPL (SPEC §9).
//!
//! A replay is a drawing that grows, on a plain page, in a few ink colours. The writer uses it:
//!
//! - **Only what changed** is stored: each frame is the rectangle around the pixels that differ
//!   from what is already on screen, the unchanged pixels inside it transparent, and the frame
//!   kept on screen afterwards (disposal "keep"). Strokes appearing on paper cost a few hundred
//!   bytes per frame.
//! - **Palette per frame**: the colours of the changed pixels, exactly, when there are at most
//!   255 of them (paper, inks and their anti-aliasing usually are); otherwise NeuQuant picks
//!   255. Each frame carries its own colour table, sized to what it uses.
//! - **Timing**: GIF delays are whole centiseconds. Each frame's end is rounded on the absolute
//!   timeline (not its duration), so rounding never accumulates and the total duration stays
//!   right. Browsers show frames shorter than 2 cs for 10 cs, so a frame that would last less
//!   than 2 cs is merged into the next one (frames are cumulative drawings: nothing is lost,
//!   the cadence just tops out at 50 frames per second). A frame identical to the previous one
//!   only lengthens it.
//! - **Transparency**: pixels with alpha below 128 are transparent (the page background shows
//!   through). Frames are expected opaque (`VideoSink`); with a transparent background, a pixel
//!   that becomes transparent again after being drawn keeps its previous colour.
//!
//! [`probe_gif`] decodes a file with the `gif` decoder: canvas size, decoded frames and the sum
//! of their delays.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::time::Duration;

use crate::{AnimationFormat, AnimationProbe, MediaError, VideoSettings, VideoSink, VideoSummary};

/// Options of a GIF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GifOptions {
    /// Plays the animation again and again (`true`, the default) or once.
    pub loop_forever: bool,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self { loop_forever: true }
    }
}

/// GIF delays are in centiseconds.
const GIF_TICKS_PER_SECOND: u64 = 100;
/// Shortest delay browsers honour (they show 0 and 1 cs as 10 cs).
const GIF_MIN_DELAY: u64 = 2;
/// NeuQuant sampling factor: 1 is best and slowest, 10 is the usual compromise.
const NEUQUANT_SAMPLE_FACTOR: i32 = 10;
/// Largest canvas side a GIF can describe.
const GIF_MAX_DIMENSION: u32 = u16::MAX as u32;

/// Starts a GIF export to `path` (created or replaced).
pub fn create_gif(
    path: &Path,
    settings: &VideoSettings,
    options: &GifOptions,
) -> Result<Box<dyn VideoSink>, MediaError> {
    validate_settings(settings, 1, GIF_MAX_DIMENSION, false)?;
    let width = settings.width as u16;
    let height = settings.height as u16;
    let file = BufWriter::new(File::create(path)?);
    let mut encoder = gif::Encoder::new(file, width, height, &[]).map_err(gif_error)?;
    if options.loop_forever {
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(gif_error)?;
    }
    Ok(Box::new(GifSink {
        retimer: Retimer::new(
            settings,
            GIF_TICKS_PER_SECOND,
            GIF_MIN_DELAY,
            u64::from(u16::MAX),
        ),
        out: GifOut {
            encoder,
            width: usize::from(width),
            height: usize::from(height),
            canvas: None,
        },
        frames_pushed: 0,
    }))
}

struct GifSink {
    retimer: Retimer,
    out: GifOut,
    frames_pushed: u64,
}

impl VideoSink for GifSink {
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
        let out = &mut self.out;
        self.retimer
            .push(rgba, timestamp, |rgba, delay| out.write(rgba, delay))?;
        self.frames_pushed += 1;
        Ok(())
    }

    fn push_audio(&mut self, _samples: &[i16]) -> Result<(), MediaError> {
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
        let GifSink {
            mut retimer,
            mut out,
            frames_pushed,
        } = *self;
        if frames_pushed == 0 {
            return Err(MediaError::InvalidSettings(
                "no video frame was pushed".into(),
            ));
        }
        let duration = retimer.finish(|rgba, delay| out.write(rgba, delay))?;
        out.encoder
            .into_inner()
            .map_err(gif_error)?
            .into_inner()
            .map_err(|e| MediaError::Io(e.into_error()))?
            .sync_all()?;
        Ok(VideoSummary {
            format: AnimationFormat::Gif,
            frames: frames_pushed,
            duration,
            audio: false,
            encoder: "gif (NeuQuant palettes when needed)".into(),
        })
    }
}

/// The encoder and what the viewer currently sees.
struct GifOut {
    encoder: gif::Encoder<BufWriter<File>>,
    width: usize,
    height: usize,
    /// The last frame written (the GIF canvas, before quantisation), `None` before the first.
    canvas: Option<Vec<u8>>,
}

impl GifOut {
    /// Writes `rgba` shown for `delay` centiseconds (at most `u16::MAX`).
    fn write(&mut self, rgba: &[u8], delay: u64) -> Result<(), MediaError> {
        let delay = u16::try_from(delay).unwrap_or(u16::MAX);
        let frame = match &self.canvas {
            None => FramePixels::new(rgba, None, self.width, Rect::full(self.width, self.height)),
            Some(canvas) => match changed_rect(canvas, rgba, self.width, self.height) {
                Some(rect) => FramePixels::new(rgba, Some(canvas), self.width, rect),
                None => FramePixels::unchanged(),
            },
        };
        let mut gif_frame = frame.into_gif_frame();
        gif_frame.delay = delay;
        gif_frame.dispose = gif::DisposalMethod::Keep;
        self.encoder.write_frame(&gif_frame).map_err(gif_error)?;
        match &mut self.canvas {
            Some(canvas) => canvas.copy_from_slice(rgba),
            None => self.canvas = Some(rgba.to_vec()),
        }
        Ok(())
    }
}

/// A rectangle of pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

impl Rect {
    fn full(width: usize, height: usize) -> Self {
        Self {
            left: 0,
            top: 0,
            width,
            height,
        }
    }
}

/// The smallest rectangle holding every pixel of `next` that differs from `previous`, or
/// `None` if the frames are identical.
fn changed_rect(previous: &[u8], next: &[u8], width: usize, height: usize) -> Option<Rect> {
    let stride = width * 4;
    let rows = || previous.chunks_exact(stride).zip(next.chunks_exact(stride));
    let top = rows().position(|(a, b)| a != b)?;
    let bottom = height - 1 - rows().rev().position(|(a, b)| a != b).unwrap_or(0);
    let (mut left, mut right) = (width, 0);
    for (a, b) in rows().skip(top).take(bottom + 1 - top) {
        if a == b {
            continue;
        }
        let pixels = || a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0);
        if let Some(first) = pixels().position(|(p, q)| p != q) {
            left = left.min(first);
            let last = width - 1 - pixels().rev().position(|(p, q)| p != q).unwrap_or(0);
            right = right.max(last);
        }
    }
    Some(Rect {
        left,
        top,
        width: right + 1 - left,
        height: bottom + 1 - top,
    })
}

/// The pixels of one GIF frame before palette mapping: `None` for transparent.
struct FramePixels {
    rect: Rect,
    /// `[r, g, b]` per pixel of the rectangle, row by row; `None` is transparent.
    pixels: Vec<Option<[u8; 3]>>,
}

impl FramePixels {
    /// The pixels of `rect` in `rgba`; those equal to `canvas` (if any) become transparent.
    fn new(rgba: &[u8], canvas: Option<&Vec<u8>>, width: usize, rect: Rect) -> Self {
        let mut pixels = Vec::with_capacity(rect.width * rect.height);
        for y in rect.top..rect.top + rect.height {
            let start = (y * width + rect.left) * 4;
            let end = start + rect.width * 4;
            let row = rgba[start..end].as_chunks::<4>().0;
            match canvas {
                Some(canvas) => {
                    for (p, c) in row.iter().zip(canvas[start..end].as_chunks::<4>().0) {
                        pixels.push(visible(p).filter(|_| p != c));
                    }
                }
                None => pixels.extend(row.iter().map(visible)),
            }
        }
        Self { rect, pixels }
    }

    /// A frame that changes nothing: one transparent pixel.
    fn unchanged() -> Self {
        Self {
            rect: Rect {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
            },
            pixels: vec![None],
        }
    }

    /// Builds the palette (exact when possible, NeuQuant otherwise) and the indexed frame.
    fn into_gif_frame(self) -> gif::Frame<'static> {
        let has_transparent = self.pixels.iter().any(Option::is_none);
        let max_colours = if has_transparent { 255 } else { 256 };

        // Exact palette, if the frame has few enough colours.
        let mut index_of: HashMap<[u8; 3], u8> = HashMap::new();
        let mut palette: Vec<u8> = Vec::new();
        let mut exact = true;
        for rgb in self.pixels.iter().flatten() {
            if index_of.contains_key(rgb) {
                continue;
            }
            if index_of.len() == max_colours {
                exact = false;
                break;
            }
            index_of.insert(*rgb, index_of.len() as u8);
            palette.extend_from_slice(rgb);
        }

        if !exact {
            let opaque: Vec<u8> = self
                .pixels
                .iter()
                .flatten()
                .flat_map(|&[r, g, b]| [r, g, b, 255])
                .collect();
            let quantiser =
                color_quant::NeuQuant::new(NEUQUANT_SAMPLE_FACTOR, max_colours, &opaque);
            palette = quantiser.color_map_rgb();
            index_of.clear();
            for &[r, g, b] in self.pixels.iter().flatten() {
                index_of
                    .entry([r, g, b])
                    .or_insert_with(|| quantiser.index_of(&[r, g, b, 255]) as u8);
            }
        }

        let transparent = has_transparent.then_some((palette.len() / 3) as u8);
        if has_transparent {
            palette.extend_from_slice(&[0, 0, 0]);
        }
        let transparent_index = transparent.unwrap_or(0);
        let buffer: Vec<u8> = self
            .pixels
            .iter()
            .map(|pixel| match pixel {
                Some(rgb) => index_of.get(rgb).copied().unwrap_or(0),
                None => transparent_index,
            })
            .collect();
        gif::Frame {
            left: self.rect.left as u16,
            top: self.rect.top as u16,
            width: self.rect.width as u16,
            height: self.rect.height as u16,
            transparent,
            palette: Some(palette),
            buffer: buffer.into(),
            ..gif::Frame::default()
        }
    }
}

/// `[r, g, b]` of an RGBA pixel, or `None` if it is mostly transparent.
fn visible(&[r, g, b, a]: &[u8; 4]) -> Option<[u8; 3]> {
    (a >= 128).then_some([r, g, b])
}

/// Reads a GIF back with the `gif` decoder: every frame is decoded and its delay summed.
pub fn probe_gif(path: &Path) -> Result<AnimationProbe, MediaError> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let mut decoder = options
        .read_info(BufReader::new(File::open(path)?))
        .map_err(|e| MediaError::InvalidFile(format!("GIF: {e}")))?;
    let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
    let mut frames = 0u64;
    let mut centiseconds = 0u64;
    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|e| MediaError::InvalidFile(format!("GIF frame {}: {e}", frames + 1)))?
    {
        frames += 1;
        centiseconds += u64::from(frame.delay);
    }
    Ok(AnimationProbe {
        format: AnimationFormat::Gif,
        width,
        height,
        frames,
        duration: Duration::from_millis(centiseconds * 10),
        audio: None,
    })
}

fn gif_error(error: gif::EncodingError) -> MediaError {
    match error {
        gif::EncodingError::Io(e) => MediaError::Io(e),
        other => MediaError::Encoding {
            codec: "GIF",
            message: other.to_string(),
        },
    }
}

/// Checks the settings shared by the animated formats: size within `min..=max` (even if
/// `even`), frame rate from 1 to 60.
pub(crate) fn validate_settings(
    settings: &VideoSettings,
    min: u32,
    max: u32,
    even: bool,
) -> Result<(), MediaError> {
    for (name, value) in [("width", settings.width), ("height", settings.height)] {
        if !(min..=max).contains(&value) || (even && value % 2 != 0) {
            let parity = if even { "even, " } else { "" };
            return Err(MediaError::InvalidSettings(format!(
                "{name} {value} (must be {parity}from {min} to {max})"
            )));
        }
    }
    if !(1..=60).contains(&settings.frame_rate) {
        return Err(MediaError::InvalidSettings(format!(
            "frame rate {} (must be from 1 to 60)",
            settings.frame_rate
        )));
    }
    Ok(())
}

/// Gives frames with [`Duration`] timestamps whole-tick durations (GIF: centiseconds, WebP:
/// milliseconds; also used by `webp.rs`).
///
/// A frame is written once the next one arrives (its end is then known). Each frame's end is
/// rounded on the absolute timeline, so durations never drift. A frame that would last less
/// than `min_ticks`, or that is identical to the next one, is merged into the next one: the
/// next frame takes its place and its start (replay frames are cumulative drawings, so the
/// next frame contains it). A frame longer than `max_ticks` is followed by copies of itself.
pub(crate) struct Retimer {
    frame_len: usize,
    ticks_per_second: u64,
    min_ticks: u64,
    max_ticks: u64,
    frame_period: Duration,
    /// The frame waiting for its end.
    pending: Option<Vec<u8>>,
    last_timestamp: Option<Duration>,
    /// End of what has been written, in ticks (also the start of the pending frame).
    written_end: u64,
}

impl Retimer {
    pub(crate) fn new(
        settings: &VideoSettings,
        ticks_per_second: u64,
        min_ticks: u64,
        max_ticks: u64,
    ) -> Self {
        Self {
            frame_len: settings.width as usize * settings.height as usize * 4,
            ticks_per_second,
            min_ticks,
            max_ticks,
            frame_period: Duration::from_secs(1) / settings.frame_rate.max(1),
            pending: None,
            last_timestamp: None,
            written_end: 0,
        }
    }

    /// Takes a frame; calls `write(rgba, duration_ticks)` for the previous one when its
    /// duration is settled. Frames are written back to back from tick zero.
    pub(crate) fn push(
        &mut self,
        rgba: &[u8],
        timestamp: Duration,
        mut write: impl FnMut(&[u8], u64) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        if rgba.len() != self.frame_len {
            return Err(MediaError::FrameSize {
                expected: self.frame_len,
                actual: rgba.len(),
            });
        }
        if let Some(previous) = self.last_timestamp
            && timestamp <= previous
        {
            return Err(MediaError::TimestampOrder {
                previous,
                timestamp,
            });
        }
        self.last_timestamp = Some(timestamp);
        let end = self.ticks(timestamp);
        let Some(pending) = self.pending.as_mut() else {
            self.pending = Some(rgba.to_vec());
            return Ok(());
        };
        if pending.as_slice() == rgba {
            return Ok(());
        }
        let duration = end.saturating_sub(self.written_end);
        if duration >= self.min_ticks {
            Self::emit(pending, duration, self.max_ticks, &mut write)?;
            self.written_end = end;
        }
        pending.copy_from_slice(rgba);
        Ok(())
    }

    /// Writes the last frame (it lasts one frame period, and at least `min_ticks`); returns
    /// the total duration written.
    pub(crate) fn finish(
        &mut self,
        mut write: impl FnMut(&[u8], u64) -> Result<(), MediaError>,
    ) -> Result<Duration, MediaError> {
        if let (Some(pending), Some(last)) = (self.pending.take(), self.last_timestamp) {
            let end = self.ticks(last.saturating_add(self.frame_period));
            let duration = end.saturating_sub(self.written_end).max(self.min_ticks);
            Self::emit(&pending, duration, self.max_ticks, &mut write)?;
            self.written_end += duration;
        }
        let nanos =
            u128::from(self.written_end) * 1_000_000_000 / u128::from(self.ticks_per_second);
        Ok(Duration::from_nanos(
            u64::try_from(nanos).unwrap_or(u64::MAX),
        ))
    }

    /// Writes `rgba` for `duration` ticks, in pieces of at most `max_ticks`.
    fn emit(
        rgba: &[u8],
        mut duration: u64,
        max_ticks: u64,
        write: &mut impl FnMut(&[u8], u64) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        while duration > max_ticks {
            write(rgba, max_ticks)?;
            duration -= max_ticks;
        }
        write(rgba, duration)
    }

    /// `timestamp` in ticks, rounded to the nearest.
    fn ticks(&self, timestamp: Duration) -> u64 {
        let ticks = (timestamp.as_nanos() * u128::from(self.ticks_per_second) + 500_000_000)
            / 1_000_000_000;
        u64::try_from(ticks).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Paper colour of the synthetic animations.
    pub(crate) const PAPER: [u8; 3] = [250, 247, 240];
    /// Ink colour of the synthetic animations.
    pub(crate) const INK: [u8; 3] = [20, 40, 120];

    /// Frame `index` of a synthetic replay: a line that grows from left to right on paper,
    /// with anti-aliased edges, slightly wavy so rows differ.
    pub(crate) fn growing_line(width: u32, height: u32, index: u32, frames: u32) -> Vec<u8> {
        let (w, h) = (width as usize, height as usize);
        let mut rgba: Vec<u8> = PAPER
            .iter()
            .copied()
            .chain([255])
            .cycle()
            .take(w * h * 4)
            .collect();
        let length = (w as f64 * 0.9) * f64::from(index + 1) / f64::from(frames);
        let x0 = w as f64 * 0.05;
        for x in 0..w {
            let fx = x as f64;
            if fx < x0 || fx > x0 + length {
                continue;
            }
            let centre = h as f64 / 2.0 + (fx / 40.0).sin() * h as f64 / 8.0;
            for y in 0..h {
                let distance = (y as f64 - centre).abs();
                let coverage = (2.5 - distance).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    let i = (y * w + x) * 4;
                    for c in 0..3 {
                        let paper = f64::from(PAPER[c]);
                        let ink = f64::from(INK[c]);
                        rgba[i + c] = (paper + (ink - paper) * coverage).round() as u8;
                    }
                }
            }
        }
        rgba
    }

    /// A page being written: loops along lines, like cursive writing, with a pen of about
    /// 3 pixels at 1080p. Strokes accumulate on the canvas as in a replay.
    pub(crate) struct Handwriting {
        width: usize,
        height: usize,
        pub(crate) canvas: Vec<u8>,
        theta: f64,
    }

    impl Handwriting {
        pub(crate) fn new(width: usize, height: usize) -> Self {
            let paper = PAPER;
            Self {
                width,
                height,
                canvas: paper
                    .iter()
                    .copied()
                    .chain([255])
                    .cycle()
                    .take(width * height * 4)
                    .collect(),
                theta: 0.0,
            }
        }

        /// Moves the pen `steps` times and inks where it goes.
        pub(crate) fn write(&mut self, steps: usize) {
            let scale = self.height as f64 / 1080.0;
            let (a, b) = (5.0 * scale, 11.0 * scale); // loops: b > a
            let margin = 80.0 * scale;
            let line_gap = 60.0 * scale;
            let line_length = self.width as f64 - 2.0 * margin;
            let radius = 1.6 * scale;
            for _ in 0..steps {
                self.theta += 0.12;
                let along = a * self.theta;
                let line = (along / line_length).floor();
                let x = margin + along % line_length - b * self.theta.sin();
                let y = margin + line_gap * (line + 1.0) - b * (1.0 - self.theta.cos());
                if y + radius + 2.0 >= self.height as f64 {
                    continue;
                }
                self.stamp(x, y, radius);
            }
        }

        fn stamp(&mut self, cx: f64, cy: f64, radius: f64) {
            let ink = INK;
            let x0 = (cx - radius - 1.0).max(0.0) as usize;
            let y0 = (cy - radius - 1.0).max(0.0) as usize;
            let x1 = ((cx + radius + 1.0) as usize).min(self.width - 1);
            let y1 = ((cy + radius + 1.0) as usize).min(self.height - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();
                    let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
                    if coverage > 0.0 {
                        let i = (y * self.width + x) * 4;
                        for (channel, &ink) in self.canvas[i..i + 3].iter_mut().zip(&ink) {
                            let old = f64::from(*channel);
                            *channel = (old + (f64::from(ink) - old) * coverage).round() as u8;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn settings(width: u32, height: u32, frame_rate: u32) -> VideoSettings {
        VideoSettings {
            width,
            height,
            frame_rate,
            audio_channels: None,
        }
    }

    /// Records what a [`Retimer`] writes: (first byte of the frame, ticks).
    fn retime(timestamps_ms: &[u64], frame_rate: u32, min: u64) -> (Vec<(u8, u64)>, Duration) {
        let mut retimer = Retimer::new(&settings(1, 1, frame_rate), 100, min, 1000);
        let mut written = Vec::new();
        for (i, &ms) in timestamps_ms.iter().enumerate() {
            let pixel = [i as u8, 0, 0, 255];
            retimer
                .push(&pixel, Duration::from_millis(ms), |rgba, ticks| {
                    written.push((rgba[0], ticks));
                    Ok(())
                })
                .unwrap();
        }
        let total = retimer
            .finish(|rgba, ticks| {
                written.push((rgba[0], ticks));
                Ok(())
            })
            .unwrap();
        (written, total)
    }

    #[test]
    fn retimer_keeps_the_total_duration() {
        // 30 fps: 3.33 cs per frame, rounded on the timeline: 3, 4, 3, 3, 3, 4...
        let timestamps: Vec<u64> = (0..90).map(|i| i * 1000 / 30).collect();
        let (written, total) = retime(&timestamps, 30, 2);
        assert_eq!(written.len(), 90);
        assert_eq!(written.iter().map(|w| w.1).sum::<u64>(), 300);
        assert_eq!(total, Duration::from_secs(3));
        assert!(written.iter().all(|w| (3..=4).contains(&w.1)));
    }

    #[test]
    fn retimer_merges_frames_that_are_too_short() {
        // 60 fps: 1.67 cs per frame; frames under 2 cs are merged into the next one.
        let timestamps: Vec<u64> = (0..60).map(|i| i * 1000 / 60).collect();
        let (written, total) = retime(&timestamps, 60, 2);
        assert!(
            written.len() < 60 && written.len() >= 25,
            "{}",
            written.len()
        );
        assert!(written.iter().all(|w| w.1 >= 2));
        assert_eq!(total, Duration::from_secs(1));
        // The last frame pushed is always shown.
        assert_eq!(written.last().map(|w| w.0), Some(59));
    }

    #[test]
    fn retimer_merges_identical_frames_and_splits_long_ones() {
        let mut retimer = Retimer::new(&settings(1, 1, 10), 100, 2, 1000);
        let mut written = Vec::new();
        let mut record = |rgba: &[u8], ticks: u64| {
            written.push((rgba[0], ticks));
            Ok(())
        };
        let a = [1, 0, 0, 255];
        let b = [2, 0, 0, 255];
        retimer.push(&a, Duration::ZERO, &mut record).unwrap();
        retimer
            .push(&a, Duration::from_millis(100), &mut record)
            .unwrap();
        retimer
            .push(&b, Duration::from_secs(25), &mut record)
            .unwrap();
        let total = retimer.finish(&mut record).unwrap();
        // `a` lasts 25 s (2500 cs, split in pieces of at most 1000), then `b` 0.1 s.
        assert_eq!(written, vec![(1, 1000), (1, 1000), (1, 500), (2, 10)]);
        assert_eq!(total, Duration::from_millis(25_100));
    }

    #[test]
    fn retimer_checks_frames() {
        let mut retimer = Retimer::new(&settings(2, 2, 10), 100, 2, 1000);
        let ok = |_: &[u8], _: u64| Ok(());
        assert!(matches!(
            retimer.push(&[0; 3], Duration::ZERO, ok),
            Err(MediaError::FrameSize {
                expected: 16,
                actual: 3
            })
        ));
        retimer
            .push(&[0; 16], Duration::from_millis(10), ok)
            .unwrap();
        assert!(matches!(
            retimer.push(&[0; 16], Duration::from_millis(10), ok),
            Err(MediaError::TimestampOrder { .. })
        ));
    }

    #[test]
    fn changed_rect_finds_the_bounding_box() {
        let (w, h) = (8, 6);
        let a = vec![0u8; w * h * 4];
        assert_eq!(changed_rect(&a, &a, w, h), None);
        let mut b = a.clone();
        b[(2 * w + 3) * 4] = 9; // (3, 2)
        b[(4 * w + 6) * 4 + 3] = 9; // (6, 4), alpha only
        assert_eq!(
            changed_rect(&a, &b, w, h),
            Some(Rect {
                left: 3,
                top: 2,
                width: 4,
                height: 3
            })
        );
    }

    #[test]
    fn palettes_are_exact_when_possible() {
        let rgba: Vec<u8> = [[1, 2, 3, 255], [4, 5, 6, 255], [0, 0, 0, 0], [1, 2, 3, 255]].concat();
        let frame = FramePixels::new(&rgba, None, 4, Rect::full(4, 1)).into_gif_frame();
        let palette = frame.palette.unwrap();
        assert_eq!(palette, vec![1, 2, 3, 4, 5, 6, 0, 0, 0]);
        assert_eq!(frame.transparent, Some(2));
        assert_eq!(frame.buffer.as_ref(), &[0, 1, 2, 0]);

        // 300 colours: NeuQuant reduces them to 256 (no transparent pixel).
        let many: Vec<u8> = (0..300u32)
            .flat_map(|i| [(i % 256) as u8, (i / 256 * 100) as u8, 7, 255])
            .collect();
        let frame = FramePixels::new(&many, None, 300, Rect::full(300, 1)).into_gif_frame();
        assert_eq!(frame.palette.unwrap().len(), 256 * 3);
        assert_eq!(frame.transparent, None);
    }

    #[test]
    fn writes_and_probes_a_gif() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("line.gif");
        let (width, height, fps, frames) = (640, 360, 15, 30);
        let started = std::time::Instant::now();
        let mut sink =
            create_gif(&path, &settings(width, height, fps), &GifOptions::default()).unwrap();
        for i in 0..frames {
            let frame = growing_line(width, height, i, frames);
            sink.push_frame(&frame, Duration::from_secs(u64::from(i)) / fps)
                .unwrap();
        }
        let summary = sink.finish().unwrap();
        let elapsed = started.elapsed();
        let probe = probe_gif(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        print_probe_table("GIF", &probe, size, elapsed);

        assert_eq!(summary.frames, 30);
        assert_eq!(summary.duration, Duration::from_secs(2));
        assert_eq!((probe.width, probe.height), (width, height));
        assert_eq!(probe.frames, 30);
        assert_eq!(probe.duration, Duration::from_secs(2));
        assert_eq!(probe.audio, None);
        // Only the growing part of each frame is stored.
        assert!(size < 60_000, "{size} bytes");
    }

    #[test]
    fn gif_frames_decode_to_the_source_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exact.gif");
        let (width, height, frames) = (64, 32, 5);
        let mut sink =
            create_gif(&path, &settings(width, height, 10), &GifOptions::default()).unwrap();
        let sources: Vec<Vec<u8>> = (0..frames)
            .map(|i| growing_line(width, height, i, frames))
            .collect();
        for (i, frame) in sources.iter().enumerate() {
            sink.push_frame(frame, Duration::from_millis(100 * i as u64))
                .unwrap();
        }
        sink.finish().unwrap();

        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options
            .read_info(BufReader::new(File::open(&path).unwrap()))
            .unwrap();
        let mut screen = vec![0u8; (width * height * 4) as usize];
        for source in &sources {
            let frame = decoder.read_next_frame().unwrap().unwrap().clone();
            // Composite the frame (disposal "keep", transparent pixels leave the screen as is).
            for y in 0..usize::from(frame.height) {
                for x in 0..usize::from(frame.width) {
                    let src = (y * usize::from(frame.width) + x) * 4;
                    if frame.buffer[src + 3] == 0 {
                        continue;
                    }
                    let dst = ((y + usize::from(frame.top)) * width as usize
                        + x
                        + usize::from(frame.left))
                        * 4;
                    screen[dst..dst + 4].copy_from_slice(&frame.buffer[src..src + 4]);
                }
            }
            assert!(screen == *source, "decoded frame differs from the source");
        }
    }

    /// GIF and WebP of a page being written at 1080p, 30 fps (`cargo test --release -p
    /// ardoise-media animated_speed_1080p -- --ignored --nocapture`; `ARDOISE_BENCH_FRAMES`
    /// frames, 300 by default).
    #[test]
    #[ignore = "benchmark: run in release"]
    fn animated_speed_1080p() {
        use crate::{WebpCompression, WebpOptions, create_webp, probe_webp};

        let frames: u32 = std::env::var("ARDOISE_BENCH_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let (width, height, fps) = (1920, 1080, 30);
        let dir = tempfile::tempdir().unwrap();
        type Create = Box<dyn Fn(&Path) -> Box<dyn VideoSink>>;
        type Probe = fn(&Path) -> Result<AnimationProbe, MediaError>;
        let formats: [(&str, Create, Probe); 3] = [
            (
                "GIF",
                Box::new(move |p| {
                    create_gif(p, &settings(width, height, fps), &GifOptions::default()).unwrap()
                }),
                probe_gif,
            ),
            (
                "WebP",
                Box::new(move |p| {
                    create_webp(p, &settings(width, height, fps), &WebpOptions::default()).unwrap()
                }),
                probe_webp,
            ),
            (
                "WebP q90",
                Box::new(move |p| {
                    let options = WebpOptions {
                        compression: WebpCompression::Lossy { quality: 90.0 },
                        ..WebpOptions::default()
                    };
                    create_webp(p, &settings(width, height, fps), &options).unwrap()
                }),
                probe_webp,
            ),
        ];
        for (name, create, probe) in formats {
            let path = dir.path().join(name);
            let mut sink = create(&path);
            let mut page = Handwriting::new(width as usize, height as usize);
            let mut spent = Duration::ZERO;
            for i in 0..frames {
                page.write(40);
                let started = std::time::Instant::now();
                sink.push_frame(&page.canvas, Duration::from_secs(u64::from(i)) / fps)
                    .unwrap();
                spent += started.elapsed();
            }
            let started = std::time::Instant::now();
            sink.finish().unwrap();
            spent += started.elapsed();
            let size = std::fs::metadata(&path).unwrap().len();
            let fps_measured = f64::from(frames) / spent.as_secs_f64();
            println!(
                "{name}: {fps_measured:.0} fps, 2 min replay ~{:.0} s, {:.0} kB per 10 s",
                3600.0 / fps_measured,
                size as f64 / 1000.0 / (f64::from(frames) / f64::from(fps)) * 10.0
            );
            let read = probe(&path).unwrap();
            print_probe_table(name, &read, size, spent);
        }
    }

    /// Prints one row of the probe table (`--nocapture`).
    pub(crate) fn print_probe_table(
        format: &str,
        probe: &AnimationProbe,
        size: u64,
        elapsed: Duration,
    ) {
        let audio = probe.audio.as_ref().map_or_else(
            || "-".to_string(),
            |a| {
                format!(
                    "{} {} ch {} Hz {:.3} s",
                    a.codec,
                    a.channels,
                    a.sample_rate,
                    a.duration.as_secs_f64()
                )
            },
        );
        println!(
            "| {format:<5} | {:>4}x{:<4} | {:>4} frames | {:>7.3} s | {:>7} B | written in \
             {:>5.2} s | sound: {audio} |",
            probe.width,
            probe.height,
            probe.frames,
            probe.duration.as_secs_f64(),
            size,
            elapsed.as_secs_f64(),
        );
    }
}
