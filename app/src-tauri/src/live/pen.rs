//! The computer's pen during live mode (SPEC §4.2 « Corriger pendant le direct »,
//! `docs/questions.md` Q11; docs/design/tablet-live-classroom.md, "M14", "The computer's strokes"):
//! what the control window sends while its pen or mouse writes, once checked, and how the
//! window's times become the session's.
//!
//! Everything here is done before the hub's lock is taken: what the tablet waits for under it is a
//! copy of at most [`MAX_PEN_SAMPLES`] points, never a check or a sort of what the window sent.

use serde::Deserialize;

use super::edits::LiveEditError;
use super::model::LiveStyle;

/// Most samples one call may bring. The window sends a call every few milliseconds, each with what
/// came since the last one: a few samples, a few hundred after a pause of the IPC.
pub const MAX_PEN_SAMPLES: usize = 4096;
/// Most points one computer stroke holds: reached, the stroke ends there (kept) and the window
/// stops sending for that gesture. Far beyond a gesture (minutes of writing without lifting).
pub const MAX_COMPUTER_POINTS: usize = 100_000;
/// A sample's time is at most this long before the call that brings it (ms): a window that kept
/// samples for longer, or a clock that jumped, gives no time before that.
pub const MAX_SAMPLE_AGE_MS: f64 = 10_000.0;

/// One sample of the computer's pen, as the window sends it (`ComputerSample` in `live.ts`).
/// Plain numbers, checked here.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ComputerSample {
    /// Page units of the page shown (on a resumed sheet, its own units).
    pub x: f64,
    pub y: f64,
    /// Pressure, 0..=1 (0.5 for a pressed mouse).
    pub p: f64,
    /// The event's `timeStamp` (`performance.now()`'s clock).
    pub ms: f64,
}

/// What the control window sends for one gesture of the computer's pen (`LiveComputerPenInput` in
/// `live.ts`). `stroke` is the window's number for the gesture (a counter from 1); `now` is
/// `performance.now()` when the call is made, the clock of the samples' `ms`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ComputerPenInput {
    Begin {
        stroke: u32,
        page_index: usize,
        style: LiveStyle,
        samples: Vec<ComputerSample>,
        now: f64,
    },
    Points {
        stroke: u32,
        samples: Vec<ComputerSample>,
        now: f64,
    },
    End {
        stroke: u32,
        samples: Vec<ComputerSample>,
        now: f64,
    },
    Cancel {
        stroke: u32,
    },
}

/// A sample once checked: finite coordinates (not yet kept within the page, which needs the
/// page), a pressure in 0..=1, and how long before the call it was taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenSample {
    pub x: f32,
    pub y: f32,
    pub p: f32,
    /// Milliseconds before the call (`now − ms`), at least 0.
    pub age: f64,
}

impl PenSample {
    /// The sample within one page size around a page of `width` × `height`, as the marks are
    /// ([`sanitize_marks`](super::model::sanitize_marks)).
    pub fn within(self, width: f32, height: f32) -> (f32, f32) {
        (
            self.x.clamp(-width, 2.0 * width),
            self.y.clamp(-height, 2.0 * height),
        )
    }
}

/// What a call does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PenAction {
    /// A new stroke on page `page_index` (the one on screen, or the call is stale), in `style`.
    Begin {
        page_index: usize,
        style: LiveStyle,
    },
    Points,
    End,
    Cancel,
}

/// A [`ComputerPenInput`] that makes sense whatever the session holds.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedPen {
    pub stroke: u32,
    pub action: PenAction,
    pub samples: Vec<PenSample>,
}

impl ComputerPenInput {
    /// Whether the call may start ink: refused once the session is stopped, like any change of a
    /// note. The others only say whether the stroke goes on.
    pub fn begins(&self) -> bool {
        matches!(self, ComputerPenInput::Begin { .. })
    }

    /// Checked before the hub's lock: at most [`MAX_PEN_SAMPLES`] samples, those without finite
    /// coordinates dropped, the pressure within 0..=1 (1 when it is not a number), the style one
    /// the tools can draw. A `begin` needs one sample at least.
    pub fn checked(self) -> Result<CheckedPen, LiveEditError> {
        let (stroke, action, samples, now) = match self {
            ComputerPenInput::Begin {
                stroke,
                page_index,
                style,
                samples,
                now,
            } => (
                stroke,
                PenAction::Begin {
                    page_index,
                    style: style.sanitized(),
                },
                samples,
                now,
            ),
            ComputerPenInput::Points {
                stroke,
                samples,
                now,
            } => (stroke, PenAction::Points, samples, now),
            ComputerPenInput::End {
                stroke,
                samples,
                now,
            } => (stroke, PenAction::End, samples, now),
            ComputerPenInput::Cancel { stroke } => (stroke, PenAction::Cancel, Vec::new(), 0.0),
        };
        if samples.len() > MAX_PEN_SAMPLES {
            return Err(LiveEditError::TooManySamples {
                count: samples.len(),
            });
        }
        let samples: Vec<PenSample> = samples
            .into_iter()
            .filter(|sample| sample.x.is_finite() && sample.y.is_finite())
            .map(|sample| PenSample {
                x: sample.x as f32,
                y: sample.y as f32,
                p: if sample.p.is_finite() {
                    sample.p.clamp(0.0, 1.0) as f32
                } else {
                    1.0
                },
                age: age(now, sample.ms),
            })
            .collect();
        if matches!(action, PenAction::Begin { .. }) && samples.is_empty() {
            return Err(LiveEditError::NoSample);
        }
        Ok(CheckedPen {
            stroke,
            action,
            samples,
        })
    }
}

/// How long before the call a sample was taken: `now − ms`, never negative, 0 when either is not a
/// number.
fn age(now: f64, ms: f64) -> f64 {
    let age = now - ms;
    if age.is_finite() { age.max(0.0) } else { 0.0 }
}

/// The wall-clock time (Unix ms, the clock of the tablet's stamps) of a sample taken `age` ms
/// before a call received at `wall_now`: anchored at each call, so that the window's clock and the
/// session's never need to agree (only the call's own delay, about a millisecond, is not counted).
/// At least the stroke's previous sample (`previous`), at most `wall_now`, at least
/// `wall_now −` [`MAX_SAMPLE_AGE_MS`].
pub fn sample_wall(wall_now: f64, age: f64, previous: Option<f64>) -> f64 {
    let wall = wall_now - age;
    let wall = previous.map_or(wall, |previous| wall.max(previous));
    wall.min(wall_now).max(wall_now - MAX_SAMPLE_AGE_MS)
}

/// A sample's time in the note, ms since `origin` (the note's `created`, or a resumed sheet's
/// shifted origin): floored, within what a point's time holds, never before the stroke's previous
/// time. Never through the tablet's spacing of 5 ms: neither pen pushes the other's times.
pub fn note_time(wall: f64, origin: i64, previous: Option<u32>) -> u32 {
    let since = (wall - origin as f64).floor();
    let t = if since.is_nan() {
        0
    } else {
        since.clamp(0.0, f64::from(u32::MAX)) as u32
    };
    previous.map_or(t, |previous| t.max(previous))
}

#[cfg(test)]
pub(crate) mod tests;
