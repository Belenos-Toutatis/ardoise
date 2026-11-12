//! Touching up a page during live mode (SPEC §4.2, `docs/questions.md` Q9;
//! docs/design/tablet-live-classroom.md, "M12"): what the interface asks for, once checked, and
//! the history of a page's edits.
//!
//! Strokes have no identity but their place in the list of the page's finished strokes. Two facts
//! keep every place stored here right:
//!
//! - **The pens only append.** A stroke that ends, the tablet's or the computer's (M14), goes at
//!   the tail: no place changes, so a selection stays valid while they write, and a stroke written
//!   after an edit stays after the places of the strokes that edit gives back.
//! - **Steps come off their stack in reverse order.** When a step is undone, every later step was
//!   undone first: what stands before its places is what stood there when it was done, whatever
//!   the pens added at the tail since. The same goes for redo. So a place in a step is simply
//!   the place the stroke had, and is never translated.
//!
//! An edit moves no place but by taking strokes out of the list or putting them back: a change of
//! colour, and a move or resize of the selection, change strokes where they stand (a move or resize
//! replaces each chosen stroke by its moved copy at the same place, `Replaced`), so both facts hold
//! for them as they are.
//!
//! The computer's strokes are steps (`Added`); the tablet's never are. Steps of one gesture of the
//! computer's eraser share a group, undone and redone together (M14).
//!
//! One step comes off out of order: « Undo the clearing » ([`History::unclear`]) takes the last
//! clearing out from under the steps done since, which stay; every place they name is moved for
//! the strokes that come back, so that the second fact holds again for them.
//!
//! No lock here and no I/O: [`LiveSession`](super::session::LiveSession) applies all this to its
//! pages under the hub's lock, which is why nothing below costs more than a pass over the page's
//! list (moves, no copy of points), with one exception: a move or resize makes its moved strokes
//! from the chosen ones there ([`StrokeTransform::apply`]), a pass over their points.

use std::collections::{HashMap, VecDeque};
use std::mem::size_of;
use std::sync::Arc;

use ardoise_core::format::strokes::MAX_COORD;
use ardoise_core::note::Layer;
use ardoise_device::pages::turn_point;
use ardoise_ink::{Point, Rgba, Stroke};
use serde::{Deserialize, Deserializer};

use super::model::{PlacedStroke, RecolouredStrokes};
use crate::share::page::is_drawn;

/// Most strokes one edit may name. Far more than a page holds (at one stroke per second, twelve
/// days of writing); it bounds what is sorted and checked for a list that makes no sense.
pub const MAX_EDIT_STROKES: usize = 1 << 20;
/// Smallest and largest scale of one move or resize ([`StrokesEdit::Transform`]): a twentieth of
/// the size, twenty times it. Beyond, a drag of a corner has gone wrong (a frame dragged onto its
/// opposite corner), and a few of them in a row would take the ink out of what a note holds.
pub const MIN_TRANSFORM_SCALE: f64 = 0.05;
pub const MAX_TRANSFORM_SCALE: f64 = 20.0;
/// Largest shift of one move, along each axis, in page units (the units of the strokes' points:
/// 10 m of a page whose unit is 10 µm, the tablet's).
pub const MAX_TRANSFORM_SHIFT: f64 = 1e6;
/// Most steps a page keeps to undo; the oldest goes first.
pub const MAX_UNDO_STEPS: usize = 100;
/// Most memory the undo steps of a page may keep, in bytes; the oldest goes first, except the last
/// step left, which holds no more than the page itself held.
pub const MAX_UNDO_BYTES: usize = 64 << 20;

/// Why an edit is refused. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiveEditError {
    /// Another page is shown, or strokes left or came back since these ones were chosen.
    #[error("the page changed since these strokes were chosen")]
    Stale,
    #[error("no stroke was chosen")]
    Empty,
    #[error("too many strokes were chosen ({count})")]
    TooMany { count: usize },
    /// Not a whole number that can name a stroke.
    #[error("a stroke's index is not valid")]
    NotAnIndex,
    #[error("stroke {index} was chosen twice")]
    Duplicate { index: u32 },
    #[error("there is no stroke {index} on a page of {strokes} strokes")]
    OutOfRange { index: u32, strokes: usize },
    /// A stroke of a hidden layer: the interface shows none, so it was not chosen on purpose.
    #[error("stroke {index} is on a hidden layer")]
    Hidden { index: u32 },
    /// Channels out of 0..=255, or fully transparent (the ink would vanish without being deleted).
    #[error("this colour is not valid")]
    BadColor,
    /// A move or resize whose scale is not finite or outside [`MIN_TRANSFORM_SCALE`] ..=
    /// [`MAX_TRANSFORM_SCALE`], whose shift is not finite or beyond [`MAX_TRANSFORM_SHIFT`], or
    /// that would take a point of the chosen strokes beyond what a note holds
    /// ([`StrokeTransform::apply`]).
    #[error("this move or resize is not valid")]
    BadTransform,
    /// The group of an eraser's gesture is not a whole number that fits.
    #[error("the group of the edit is not valid")]
    BadGroup,
    /// A `begin` of the computer's pen without any sample with finite coordinates.
    #[error("the pen gave no sample")]
    NoSample,
    #[error("too many samples in one call ({count})")]
    TooManySamples { count: usize },
    /// The computer's layer is hidden: its pen does not write there (show it first).
    #[error("the chosen layer is hidden")]
    LayerHidden,
    /// A layer that does not exist, a number that is no index, a move onto itself or outside the
    /// list, a merge of the bottom layer.
    #[error("there is no such layer, or it cannot go there")]
    BadLayer,
    /// Empty once trimmed, or too long.
    #[error("this name cannot be the name of a layer")]
    BadLayerName,
    #[error("the page has as many layers as it may have")]
    TooManyLayers,
}

impl LiveEditError {
    /// The error's code for the interface (`errorCodes.*`): a stale edit asks for a new selection,
    /// a hidden layer for the chosen one to be shown, a layer operation that makes no sense says
    /// so; any other is a mistake of the interface.
    pub fn code(&self) -> &'static str {
        match self {
            LiveEditError::Stale => "liveEditStale",
            LiveEditError::LayerHidden => "liveLayerHidden",
            LiveEditError::BadLayer
            | LiveEditError::BadLayerName
            | LiveEditError::TooManyLayers => "liveLayerInvalid",
            _ => "liveEditInvalid",
        }
    }
}

/// A colour as the interface sends it. Plain numbers, checked here: a bad value then gets the coded
/// error of an edit rather than the rejection of the command's arguments.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct EditColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl EditColor {
    fn checked(self) -> Result<Rgba, LiveEditError> {
        let channel = |value: f64| {
            if value.fract() == 0.0 && (0.0..=255.0).contains(&value) {
                Ok(value as u8)
            } else {
                Err(LiveEditError::BadColor)
            }
        };
        let color = Rgba {
            r: channel(self.r)?,
            g: channel(self.g)?,
            b: channel(self.b)?,
            a: channel(self.a)?,
        };
        if color.a == 0 {
            return Err(LiveEditError::BadColor);
        }
        Ok(color)
    }
}

/// A number of the interface where `null` stands for one that is not finite: what JSON makes of
/// `NaN` and `Infinity` (`JSON.stringify`), as a resize of a selection with no width can give. Taken
/// as NaN, so that it gets the coded refusal of an edit rather than the rejection of the command's
/// arguments. A missing number is still a rejection.
fn number_or_null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    Ok(Option::<f64>::deserialize(deserializer)?.unwrap_or(f64::NAN))
}

/// What the interface asks of strokes it chose (`LiveStrokesEdit` in `live.ts`). The indices are
/// places in the list the window has, in any order; plain numbers for the same reason as
/// [`EditColor`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StrokesEdit {
    Delete {
        indices: Vec<f64>,
        /// The gesture of the computer's eraser this delete is part of (M14): the deletes of one
        /// gesture are undone and redone together. Absent for a delete of the selection.
        #[serde(default)]
        group: Option<f64>,
    },
    Recolour {
        indices: Vec<f64>,
        color: EditColor,
    },
    /// The selection moved or resized (SPEC §4.2, « Retoucher pendant le direct »): each point
    /// (px, py) of each stroke becomes (px·scale + x, py·scale + y), in page units; see
    /// [`StrokeTransform`].
    Transform {
        indices: Vec<f64>,
        #[serde(deserialize_with = "number_or_null")]
        scale: f64,
        #[serde(deserialize_with = "number_or_null")]
        x: f64,
        #[serde(deserialize_with = "number_or_null")]
        y: f64,
    },
}

/// A move and resize of strokes, checked ([`StrokesEdit::Transform`]): each point (px, py) becomes
/// (px·`scale` + `x`, py·`scale` + `y`), in the units of the points, and the width is multiplied by
/// `scale`; pressures, times, tool, colour and layer stay. Worked out in `f64`, stored in `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeTransform {
    scale: f64,
    x: f64,
    y: f64,
}

// Checked finite: no NaN, so `==` is an equivalence.
impl Eq for StrokeTransform {}

impl StrokeTransform {
    /// The transform the interface asked for, if it makes sense whatever the strokes.
    pub fn checked(scale: f64, x: f64, y: f64) -> Result<StrokeTransform, LiveEditError> {
        let shift = |value: f64| value.is_finite() && value.abs() <= MAX_TRANSFORM_SHIFT;
        // `contains` is false for NaN, and the bounds are finite.
        if !(MIN_TRANSFORM_SCALE..=MAX_TRANSFORM_SCALE).contains(&scale) || !shift(x) || !shift(y) {
            return Err(LiveEditError::BadTransform);
        }
        Ok(StrokeTransform { scale, x, y })
    }

    /// Moves nothing: no step, no event.
    pub fn is_identity(self) -> bool {
        self.scale == 1.0 && self.x == 0.0 && self.y == 0.0
    }

    /// `stroke` moved and resized: a new stroke, a pass over its points. `None` when a point would
    /// go beyond ±[`MAX_COORD`] or the width would not be finite: the note's file could no longer
    /// store it (`strokes::encode`), and a page that cannot be written loses everything after.
    pub fn apply(self, stroke: &Stroke) -> Option<Stroke> {
        let limit = f64::from(MAX_COORD);
        let coordinate = |value: f32, shift: f64| {
            let moved = f64::from(value) * self.scale + shift;
            // `<=` is false for NaN.
            (moved.abs() <= limit).then_some(moved as f32)
        };
        let width = (f64::from(stroke.width) * self.scale) as f32;
        if !width.is_finite() {
            return None;
        }
        let points = stroke
            .points
            .iter()
            .map(|point| {
                Some(Point {
                    x: coordinate(point.x, self.x)?,
                    y: coordinate(point.y, self.y)?,
                    ..*point
                })
            })
            .collect::<Option<Vec<Point>>>()?;
        Some(Stroke {
            tool: stroke.tool,
            color: stroke.color,
            width,
            layer: stroke.layer,
            points,
        })
    }
}

/// What an edit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Delete,
    Recolour(Rgba),
    Transform(StrokeTransform),
}

/// A [`StrokesEdit`] that makes sense whatever the page holds: at least one stroke, ascending, no
/// duplicate. Made before the hub's lock is taken: sorting a long list has no reason to hold up the
/// tablet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedEdit {
    pub indices: Vec<u32>,
    pub action: EditAction,
    /// The step's group ([`StrokesEdit::Delete`]).
    pub group: Option<u32>,
}

impl StrokesEdit {
    pub fn checked(self) -> Result<CheckedEdit, LiveEditError> {
        let (indices, action, group) = match self {
            StrokesEdit::Delete { indices, group } => {
                let group = match group {
                    Some(group)
                        if group.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&group) =>
                    {
                        Some(group as u32)
                    }
                    Some(_) => return Err(LiveEditError::BadGroup),
                    None => None,
                };
                (indices, EditAction::Delete, group)
            }
            StrokesEdit::Recolour { indices, color } => {
                (indices, EditAction::Recolour(color.checked()?), None)
            }
            StrokesEdit::Transform {
                indices,
                scale,
                x,
                y,
            } => (
                indices,
                EditAction::Transform(StrokeTransform::checked(scale, x, y)?),
                None,
            ),
        };
        if indices.is_empty() {
            return Err(LiveEditError::Empty);
        }
        if indices.len() > MAX_EDIT_STROKES {
            return Err(LiveEditError::TooMany {
                count: indices.len(),
            });
        }
        let mut sorted = indices
            .iter()
            .map(|&index| {
                if index.fract() == 0.0 && (0.0..f64::from(u32::MAX)).contains(&index) {
                    Ok(index as u32)
                } else {
                    Err(LiveEditError::NotAnIndex)
                }
            })
            .collect::<Result<Vec<u32>, _>>()?;
        sorted.sort_unstable();
        if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(LiveEditError::Duplicate { index: pair[0] });
        }
        Ok(CheckedEdit {
            indices: sorted,
            action,
            group,
        })
    }
}

/// The colour a stroke had and the one it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourChange {
    pub index: u32,
    pub before: Rgba,
    pub after: Rgba,
}

/// A step that can be undone.
#[derive(Debug, Clone)]
pub enum UndoStep {
    /// Strokes taken off the page, each with the place it had (ascending). Shared with the event
    /// that gives them back to the windows, whose queues outlive the step. `cleared`: « Clear the
    /// page » did it.
    Removed {
        strokes: Arc<Vec<PlacedStroke>>,
        cleared: bool,
        /// Strokes left in the list just after the removal: what « Undo the clearing » needs to
        /// tell them from the strokes written since ([`History::unclear`]).
        left: u32,
        /// Memory the strokes hold, worked out once.
        bytes: usize,
    },
    Recoloured(Vec<ColourChange>),
    /// Strokes the computer wrote (M14), at these places (ascending): they are on the page, the
    /// step holds nothing else.
    Added {
        indices: Vec<u32>,
    },
    /// Strokes as they were before a move or resize, each with its place (ascending): the page
    /// holds their moved copies there. An undo swaps them back ([`RedoStep::Replace`] gets what
    /// the page held); shared with the event that gives them back, as `Removed`'s.
    Replaced {
        strokes: Arc<Vec<PlacedStroke>>,
        /// Memory the strokes hold, worked out once.
        bytes: usize,
    },
}

/// Memory `strokes` hold, as the undo steps count it.
fn stored_size(strokes: &[PlacedStroke]) -> usize {
    strokes
        .iter()
        .map(|placed| size_of::<PlacedStroke>() + placed.stroke.points.len() * size_of::<Point>())
        .sum()
}

impl UndoStep {
    /// A removal of `strokes`, which left `left` strokes in the list.
    pub fn removed(strokes: Vec<PlacedStroke>, cleared: bool, left: usize) -> UndoStep {
        let bytes = stored_size(&strokes);
        UndoStep::Removed {
            strokes: Arc::new(strokes),
            cleared,
            left: u32::try_from(left).unwrap_or(u32::MAX),
            bytes,
        }
    }

    /// A move or resize of strokes that were `strokes` before.
    pub fn replaced(strokes: Vec<PlacedStroke>) -> UndoStep {
        let bytes = stored_size(&strokes);
        UndoStep::Replaced {
            strokes: Arc::new(strokes),
            bytes,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            UndoStep::Removed { bytes, .. } | UndoStep::Replaced { bytes, .. } => *bytes,
            UndoStep::Recoloured(changes) => changes.len() * size_of::<ColourChange>(),
            UndoStep::Added { indices } => indices.len() * size_of::<u32>(),
        }
    }

    fn is_clearing(&self) -> bool {
        matches!(self, UndoStep::Removed { cleared: true, .. })
    }
}

/// A step that was undone. A removal keeps only its places: the strokes are back on the page.
#[derive(Debug, Clone, PartialEq)]
pub enum RedoStep {
    Remove {
        indices: Vec<u32>,
        cleared: bool,
    },
    Recolour(Vec<ColourChange>),
    /// The computer's strokes an undo took off the page, each with its place: given back there.
    /// Shared with the event that took them, as [`UndoStep::Removed`] is with the one that gives
    /// them back.
    Add {
        strokes: Arc<Vec<PlacedStroke>>,
    },
    /// Strokes as a move or resize left them, each with its place, which the undo of that move
    /// took off the page and replaced: a redo swaps them in again.
    Replace {
        strokes: Arc<Vec<PlacedStroke>>,
    },
}

/// A step and the group it belongs to.
#[derive(Debug, Clone)]
struct Entry<S> {
    step: S,
    group: Option<u32>,
}

/// Whether a step of group `a` and one of group `b` are undone together: only steps that name the
/// same group (the deletes of one gesture of the computer's eraser).
fn same_group(a: Option<u32>, b: Option<u32>) -> bool {
    a.is_some() && a == b
}

/// What can be undone and redone on a page: the edits, and the computer's strokes (M14). The
/// tablet's writing is not in it, and does not empty the redo steps either: it moves no place, and
/// only a new edit or computer stroke changes what they would redo.
///
/// Steps next to each other on a stack that name the same group are one group: undone and redone
/// together, counted and dropped together by the limits ([`MAX_UNDO_STEPS`] groups,
/// [`MAX_UNDO_BYTES`]). A step without a group is a group of its own.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: VecDeque<Entry<UndoStep>>,
    redo: Vec<Entry<RedoStep>>,
    /// Groups on the undo stack, kept up to date: a gesture of the eraser may make many steps,
    /// and the stack is not walked at each of them under the hub's lock.
    groups: usize,
    /// Memory the undo steps keep, kept up to date likewise.
    bytes: usize,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty() && self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.groups = 0;
        self.bytes = 0;
    }

    /// A new edit or computer stroke: what was undone can no longer be redone.
    pub fn record(&mut self, step: UndoStep, group: Option<u32>) {
        self.redo.clear();
        self.push_undo(step, group);
    }

    /// Back on the undo stack, without touching the redo steps. The oldest groups go beyond the
    /// limits, except the last group left, which holds no more than the page held.
    pub fn push_undo(&mut self, step: UndoStep, group: Option<u32>) {
        let joins = self
            .undo
            .back()
            .is_some_and(|top| same_group(top.group, group));
        if !joins {
            self.groups += 1;
        }
        self.bytes += step.bytes();
        self.undo.push_back(Entry { step, group });
        while self.groups > MAX_UNDO_STEPS || (self.groups > 1 && self.bytes > MAX_UNDO_BYTES) {
            self.drop_oldest_group();
        }
    }

    fn drop_oldest_group(&mut self) {
        let Some(first) = self.undo.pop_front() else {
            return;
        };
        self.bytes -= first.step.bytes();
        self.groups -= 1;
        while self
            .undo
            .front()
            .is_some_and(|next| same_group(next.group, first.group))
        {
            if let Some(next) = self.undo.pop_front() {
                self.bytes -= next.step.bytes();
            }
        }
    }

    /// The last step done, and its group.
    pub fn pop_undo(&mut self) -> Option<(UndoStep, Option<u32>)> {
        let Entry { step, group } = self.undo.pop_back()?;
        self.bytes -= step.bytes();
        if !self.undo_joins(group) {
            self.groups -= 1;
        }
        Some((step, group))
    }

    /// The next step to undo belongs to `group` (a group, not `None`).
    pub fn undo_joins(&self, group: Option<u32>) -> bool {
        self.undo
            .back()
            .is_some_and(|top| same_group(top.group, group))
    }

    pub fn push_redo(&mut self, step: RedoStep, group: Option<u32>) {
        self.redo.push(Entry { step, group });
    }

    /// The last step undone, and its group.
    pub fn pop_redo(&mut self) -> Option<(RedoStep, Option<u32>)> {
        self.redo.pop().map(|Entry { step, group }| (step, group))
    }

    /// The next step to redo belongs to `group` (a group, not `None`).
    pub fn redo_joins(&self, group: Option<u32>) -> bool {
        self.redo
            .last()
            .is_some_and(|top| same_group(top.group, group))
    }

    /// Memory the undo steps keep, in bytes.
    #[cfg(test)]
    pub fn stored_bytes(&self) -> usize {
        self.bytes
    }

    /// Groups on the undo stack.
    #[cfg(test)]
    pub fn undo_groups(&self) -> usize {
        self.groups
    }

    /// The strokes the next undo gives back (all the steps of the group on top: removed, or as
    /// they were before a move), to copy before the lock is taken.
    pub fn next_restored(&self) -> Vec<Arc<Vec<PlacedStroke>>> {
        let Some(top) = self.undo.back() else {
            return Vec::new();
        };
        self.undo
            .iter()
            .rev()
            .take_while(|entry| std::ptr::eq(*entry, top) || same_group(entry.group, top.group))
            .filter_map(|entry| match &entry.step {
                UndoStep::Removed { strokes, .. } | UndoStep::Replaced { strokes, .. } => {
                    Some(strokes.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The strokes the next redo gives back (the computer's strokes of the group on top, and
    /// strokes as a move left them).
    pub fn next_redone(&self) -> Vec<Arc<Vec<PlacedStroke>>> {
        let Some(top) = self.redo.last() else {
            return Vec::new();
        };
        self.redo
            .iter()
            .rev()
            .take_while(|entry| std::ptr::eq(*entry, top) || same_group(entry.group, top.group))
            .filter_map(|entry| match &entry.step {
                RedoStep::Add { strokes } | RedoStep::Replace { strokes } => Some(strokes.clone()),
                _ => None,
            })
            .collect()
    }

    /// The strokes the page's last « Clear the page » took, to copy before the lock is taken.
    pub fn last_cleared(&self) -> Option<Arc<Vec<PlacedStroke>>> {
        self.undo.iter().rev().find_map(|entry| match &entry.step {
            UndoStep::Removed {
                strokes,
                cleared: true,
                ..
            } => Some(strokes.clone()),
            _ => None,
        })
    }

    /// « Undo the clearing », out of order (M14): takes the page's last « Clear the page » out of
    /// the history, and gives its strokes with the places they go back to in the list as it is now,
    /// under what was written since, whichever pen wrote it. The steps done since stay, every place
    /// they name moved for the strokes that come back, so that Ctrl+Z still undoes the last of
    /// them, and then the steps from before the clearing as before. The clearing itself can no
    /// longer be redone, and the redo steps go, as for any new action. `None`: no clearing to undo.
    pub fn unclear(&mut self) -> Option<(Arc<Vec<PlacedStroke>>, Vec<u32>)> {
        let at = self
            .undo
            .iter()
            .rposition(|entry| entry.step.is_clearing())?;
        let Entry { step, group } = self.undo.remove(at)?;
        let (strokes, left) = match step {
            UndoStep::Removed { strokes, left, .. } => (strokes, left),
            step => {
                self.undo.insert(at, Entry { step, group });
                return None;
            }
        };
        let mut moving = Unclearing::new(strokes.iter().map(|placed| placed.index), left);
        for entry in self.undo.iter_mut().skip(at) {
            moving.follow(&mut entry.step);
        }
        self.redo.clear();
        self.recount();
        let places = moving.places_now();
        Some((strokes, places))
    }

    /// Groups and bytes counted again, after a step left from the middle of the stack.
    fn recount(&mut self) {
        let mut previous: Option<&Entry<UndoStep>> = None;
        self.groups = 0;
        for entry in &self.undo {
            if !previous.is_some_and(|previous| same_group(previous.group, entry.group)) {
                self.groups += 1;
            }
            previous = Some(entry);
        }
        self.bytes = self.undo.iter().map(|entry| entry.step.bytes()).sum();
    }

    /// Every stroke kept to come back (by an undo or a redo), for `change`.
    fn kept_strokes(&mut self, mut change: impl FnMut(&mut Stroke)) {
        let undone = self
            .undo
            .iter_mut()
            .filter_map(|entry| match &mut entry.step {
                UndoStep::Removed { strokes, .. } | UndoStep::Replaced { strokes, .. } => {
                    Some(strokes)
                }
                _ => None,
            });
        let redone = self
            .redo
            .iter_mut()
            .filter_map(|entry| match &mut entry.step {
                RedoStep::Add { strokes } | RedoStep::Replace { strokes } => Some(strokes),
                _ => None,
            });
        for strokes in undone.chain(redone) {
            for placed in Arc::make_mut(strokes) {
                change(&mut placed.stroke);
            }
        }
    }

    /// The page, of size `width` × `height`, turns: the strokes kept to come back turn with it, so
    /// that they return to where they would be if they had stayed. Colours and places do not change.
    pub fn turn(&mut self, width: f32, height: f32, turns: u8) {
        self.kept_strokes(|stroke| {
            for point in &mut stroke.points {
                (point.x, point.y) = turn_point(point.x, point.y, width, height, turns);
            }
        });
    }

    /// The page's layers were reordered (M14, « monter », « descendre »): the strokes kept to come
    /// back take the new index of their layer (`follow`). Places do not change.
    pub fn remap_layers(&mut self, follow: impl Fn(u32) -> u32) {
        self.kept_strokes(|stroke| stroke.layer = follow(stroke.layer));
    }
}

/// Copies of the strokes an undo or a redo gives back, for the page (the windows get the stored
/// ones): made by the hub before it takes its lock, since those points can be many.
#[derive(Debug)]
pub struct Prepared {
    copies: Vec<(Arc<Vec<PlacedStroke>>, Vec<Stroke>)>,
}

impl Prepared {
    /// Copies of each of `sources` (the steps of a group).
    pub fn of_all(sources: Vec<Arc<Vec<PlacedStroke>>>) -> Prepared {
        let copies = sources
            .into_iter()
            .map(|source| {
                let copy = source.iter().map(|placed| placed.stroke.clone()).collect();
                (source, copy)
            })
            .collect();
        Prepared { copies }
    }

    /// Strokes copied, in all.
    #[cfg(test)]
    pub fn copied(&self) -> Vec<&Stroke> {
        self.copies.iter().flat_map(|(_, copy)| copy).collect()
    }

    /// The copy of `stored`: the one `prepared` holds when it was made from them (taken out of
    /// it), else a new one (another command got in between).
    pub fn copy_of(
        prepared: &mut Option<Prepared>,
        stored: &Arc<Vec<PlacedStroke>>,
    ) -> Vec<Stroke> {
        let made = prepared.as_mut().and_then(|prepared| {
            let at = prepared
                .copies
                .iter()
                .position(|(source, _)| Arc::ptr_eq(source, stored))?;
            Some(prepared.copies.swap_remove(at).1)
        });
        made.unwrap_or_else(|| stored.iter().map(|placed| placed.stroke.clone()).collect())
    }
}

/// How the places named by the steps done after a « Clear the page » move when its strokes come
/// back under them ([`History::unclear`]).
///
/// After a clearing the list is the strokes it left (`kept`: those of hidden layers, and any written
/// while it was undone), in their order, then the strokes written since, in theirs: a pen only
/// appends, an edit only takes strokes out or changes them where they stand (a colour, a move), its
/// undo puts them back where they were, and a step done since never gives back a stroke from
/// before the clearing (that would undo the clearing first). So a place below the number of kept strokes still there is a kept stroke, whose rank
/// among them says which cleared strokes stood before it; any other place is a stroke written
/// since, which comes after all of them. Which kept strokes are still there changes only when a
/// step takes one out (its layer shown since): followed step by step, in order.
struct Unclearing {
    /// For each cleared stroke, in order, how many kept strokes stood before it.
    kept_before: Vec<u32>,
    /// How many strokes the clearing left.
    kept: u32,
    /// The ranks among them of those still in the list; `None` while all are (the usual case:
    /// nothing to follow).
    present: Option<Vec<u32>>,
}

impl Unclearing {
    /// For a clearing that took the strokes at `places` (ascending) and left `kept`.
    fn new(places: impl Iterator<Item = u32>, kept: u32) -> Unclearing {
        let kept_before = places
            .enumerate()
            .map(|(i, place)| place.saturating_sub(u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        Unclearing {
            kept_before,
            kept,
            present: None,
        }
    }

    /// How many strokes come back.
    fn count(&self) -> u32 {
        u32::try_from(self.kept_before.len()).unwrap_or(u32::MAX)
    }

    /// How many of the kept strokes are in the list now.
    fn kept_now(&self) -> usize {
        self.present.as_ref().map_or(self.kept as usize, Vec::len)
    }

    /// Where the stroke at `place` of the list stands once the cleared strokes are back.
    fn moved(&self, place: u32) -> u32 {
        let at = place as usize;
        if at < self.kept_now() {
            let rank = self.present.as_ref().map_or(place, |present| present[at]);
            let before = self.kept_before.partition_point(|&kept| kept <= rank);
            place + u32::try_from(before).unwrap_or(u32::MAX)
        } else {
            place.saturating_add(self.count())
        }
    }

    /// The strokes at `places` (ascending) leave the list.
    fn removed(&mut self, places: &[u32]) {
        let kept_now = self.kept_now();
        if !places.iter().any(|&place| (place as usize) < kept_now) {
            return;
        }
        let kept = self.kept;
        let present = self.present.get_or_insert_with(|| (0..kept).collect());
        let mut at = 0;
        let mut taken = places.iter().peekable();
        present.retain(|_| {
            let gone = taken.next_if(|&&place| place as usize == at).is_some();
            at += 1;
            !gone
        });
    }

    /// A step done after the clearing, in order: its places move, and what it takes out is followed.
    fn follow(&mut self, step: &mut UndoStep) {
        match step {
            UndoStep::Removed { strokes, left, .. } => {
                let places: Vec<u32> = strokes.iter().map(|placed| placed.index).collect();
                let moved: Vec<u32> = places.iter().map(|&place| self.moved(place)).collect();
                if moved != places {
                    for (placed, place) in Arc::make_mut(strokes).iter_mut().zip(moved) {
                        placed.index = place;
                    }
                }
                self.removed(&places);
                *left = left.saturating_add(self.count());
            }
            UndoStep::Recoloured(changes) => {
                for change in changes {
                    change.index = self.moved(change.index);
                }
            }
            // As for a recolour: the strokes stay in the list, so which kept strokes are there
            // does not change.
            UndoStep::Replaced { strokes, .. } => {
                let moved: Vec<u32> = strokes
                    .iter()
                    .map(|placed| self.moved(placed.index))
                    .collect();
                if strokes
                    .iter()
                    .map(|placed| placed.index)
                    .ne(moved.iter().copied())
                {
                    for (placed, place) in Arc::make_mut(strokes).iter_mut().zip(moved) {
                        placed.index = place;
                    }
                }
            }
            // A stroke written since: after every stroke that comes back.
            UndoStep::Added { indices } => {
                for index in indices {
                    *index = index.saturating_add(self.count());
                }
            }
        }
    }

    /// Where the cleared strokes go back in the list as it is now (ascending): each after the kept
    /// strokes that stood before it and are still there, and before the strokes written since.
    fn places_now(&self) -> Vec<u32> {
        self.kept_before
            .iter()
            .enumerate()
            .map(|(i, &before)| {
                let kept = match &self.present {
                    None => before.min(self.kept),
                    Some(present) => u32::try_from(present.partition_point(|&rank| rank < before))
                        .unwrap_or(u32::MAX),
                };
                u32::try_from(i).unwrap_or(u32::MAX).saturating_add(kept)
            })
            .collect()
    }
}

/// Takes the strokes at `indices` (ascending, all within the list) out of `list`. Only the strokes
/// after the first one taken move, and no point is copied.
pub fn remove_at(list: &mut Vec<Stroke>, indices: &[u32]) -> Vec<PlacedStroke> {
    let Some(first) = indices.first().map(|&first| first as usize) else {
        return Vec::new();
    };
    let mut place = first;
    let mut upcoming = indices.iter().peekable();
    list.extract_if(first.., |_| {
        let taken = upcoming
            .next_if(|&&index| index as usize == place)
            .is_some();
        place += 1;
        taken
    })
    .zip(indices)
    .map(|(stroke, &index)| PlacedStroke { index, stroke })
    .collect()
}

/// Puts `strokes` back in `list`, each at the place given (ascending, in the list as it ends up).
/// Only the strokes after the first place move, and no point is copied.
pub fn insert_at(list: &mut Vec<Stroke>, strokes: impl ExactSizeIterator<Item = (u32, Stroke)>) {
    let mut strokes = strokes.peekable();
    let Some(first) = strokes.peek().map(|(first, _)| *first as usize) else {
        return;
    };
    let tail = list.split_off(first.min(list.len()));
    list.reserve(strokes.len() + tail.len());
    let mut tail = tail.into_iter();
    for (index, stroke) in strokes {
        while list.len() < index as usize {
            let Some(kept) = tail.next() else { break };
            list.push(kept);
        }
        list.push(stroke);
    }
    list.extend(tail);
}

/// Numbers of hidden strokes before each place of `list`, on a page with a hidden layer (`None`
/// otherwise: a viewer's list and the page's are numbered alike). A page's place less this is its
/// place in a viewer's list, which holds only the strokes [`is_drawn`] lets through.
pub fn hidden_before(layers: &[Layer], list: &[Stroke]) -> Option<Vec<u32>> {
    if layers.iter().all(|layer| layer.visible) {
        return None;
    }
    let mut hidden = 0;
    Some(
        list.iter()
            .map(|stroke| {
                let before = hidden;
                if !is_drawn(layers, stroke) {
                    hidden += 1;
                }
                before
            })
            .collect(),
    )
}

/// Where the stroke at `place` of `list` stands in a viewer's list (see [`hidden_before`]):
/// `None` for a hidden stroke, which a viewer never has (M14: an undo may give back, a redo take, a
/// stroke whose layer was hidden since it was done), or for a place outside the list.
pub fn viewer_place(
    hidden_before: &[u32],
    layers: &[Layer],
    list: &[Stroke],
    place: u32,
) -> Option<u32> {
    let stroke = list.get(place as usize)?;
    if !is_drawn(layers, stroke) {
        return None;
    }
    let hidden = hidden_before.get(place as usize).copied().unwrap_or(0);
    Some(place.saturating_sub(hidden))
}

/// Whether ink of layer `layer` is hidden on a page of `layers` (a layer that does not exist
/// shows, as [`is_drawn`] has it).
pub fn layer_hidden(layers: &[Layer], layer: u32) -> bool {
    layers
        .get(layer as usize)
        .is_some_and(|layer| !layer.visible)
}

/// The new colour of strokes as the windows get it: one entry per colour, indices in the order
/// given. (A linear search would do for a real page; the file of a note can hold any colours.)
pub fn grouped(changes: impl Iterator<Item = (u32, Rgba)>) -> Vec<RecolouredStrokes> {
    let mut groups: Vec<RecolouredStrokes> = Vec::new();
    let mut by_color: HashMap<Rgba, usize> = HashMap::new();
    for (index, color) in changes {
        let group = *by_color.entry(color).or_insert_with(|| {
            groups.push(RecolouredStrokes {
                color,
                indices: Vec::new(),
            });
            groups.len() - 1
        });
        groups[group].indices.push(index);
    }
    groups
}

#[cfg(test)]
pub(crate) mod tests;
