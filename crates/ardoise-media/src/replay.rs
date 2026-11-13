//! The animated replay of a note (SPEC §4.5, `docs/design/media.md`, "Replay semantics"):
//! [`Timeline`] maps replay time to note time, [`FrameRenderer`] draws the frames.
//!
//! # Time
//!
//! Note time is [`Point::t`](ardoise_ink::Point::t): milliseconds from `Note::created`.
//! Replay time is what the viewer sees, from zero (the note's time origin) to
//! [`Timeline::duration`] (its last sample shown):
//!
//! 1. Only the strokes and page objects every format draws count (visible layers: hidden
//!    strokes are not there, so the time spent drawing them is a pause like any other). A page
//!    object (a text frame) appears all at once at its time (`PageObject::t`): it counts as a
//!    stroke of one sample, so a note whose last thing is a text frame replays until it
//!    appears, and a long wait before it is a pause.
//! 2. With [`ReplayOptions::max_pause`] ("supprimer les temps morts"), every pause longer
//!    than it is cut after its first `max_pause`: the time before the first stroke and the
//!    time between strokes (when several strokes overlap in time, such as strokes on two
//!    layers or merged notes, only the time when none is being drawn is a pause). The pen
//!    standing still inside a stroke is not a pause.
//! 3. The time kept is divided by [`ReplayOptions::speed`].
//!
//! The mapping is piecewise linear and pure (a list of kept stretches of note time), so the
//! editor's replay can reproduce it. Note time inside a cut maps to the instant of the cut;
//! the other way, a replay instant maps to the latest note time shown at that instant, so
//! `Timeline::to_replay(Timeline::to_note(r)) == r`.
//!
//! # What is drawn at replay time `r`
//!
//! [`Timeline::note_at`]: every sample whose note time maps to `r` or earlier, so strokes
//! appear in their recorded order of time, point by point; a stroke in progress is cut after
//! its last sample shown (a sample never appears before the one it follows: its time counts
//! as at least theirs); and every page object whose time maps to `r` or earlier, whole. Notes
//! with [`Timing::Synthesized`](ardoise_core::note::Timing) follow the same rules; their
//! replay is approximate. Strokes and objects are drawn in the order every export uses
//! ([`ardoise_render::draw_items`]: a layer's objects over its strokes), whatever the order
//! they were written in, so the last frame is the exported image.

mod frames;
mod timeline;

#[cfg(test)]
mod tests;

pub use frames::{FrameRenderer, FrameSettings, PagePlacement};
pub use timeline::{MAX_SPEED, MIN_SPEED, ReplayOptions, Timeline, frame_time};
