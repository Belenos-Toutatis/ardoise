//! Touching up a page during live mode (SPEC §4.2, `docs/questions.md` Q9;
//! docs/design/tablet-live-classroom.md, "M12"): what the interface asks for, once checked, and
//! the history of a page's edits.
//!
//! Strokes have no identity but their place in the list of the page's finished strokes. Two facts
//! keep every place stored here right:
//!
//! - **The tablet only appends.** A stroke that ends goes at the tail: no place changes, so a
//!   selection stays valid while the tablet writes, and a stroke written after an edit stays after
//!   the places of the strokes that edit gives back.
//! - **Steps come off their stack in reverse order.** When a step is undone, every later step was
//!   undone first: what stands before its places is what stood there when it was done, whatever
//!   the tablet added at the tail since. The same goes for redo. So a place in a step is simply
//!   the place the stroke had, and is never translated.
//!
//! No lock here and no I/O: [`LiveSession`](super::session::LiveSession) applies all this to its
//! pages under the hub's lock, which is why nothing below costs more than a pass over the page's
//! list (moves, no copy of points).

use std::collections::{HashMap, VecDeque};
use std::mem::size_of;
use std::sync::Arc;

use ardoise_core::note::Layer;
use ardoise_device::pages::turn_point;
use ardoise_ink::{Point, Rgba, Stroke};
use serde::Deserialize;

use super::model::{PlacedStroke, RecolouredStrokes};
use crate::share::page::is_drawn;

/// Most strokes one edit may name. Far more than a page holds (at one stroke per second, twelve
/// days of writing); it bounds what is sorted and checked for a list that makes no sense.
pub const MAX_EDIT_STROKES: usize = 1 << 20;
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
}

impl LiveEditError {
    /// The error's code for the interface (`errorCodes.*`): a stale edit asks for a new selection,
    /// any other is a mistake of the interface.
    pub fn code(&self) -> &'static str {
        match self {
            LiveEditError::Stale => "liveEditStale",
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
    Delete { indices: Vec<f64> },
    Recolour { indices: Vec<f64>, color: EditColor },
}

/// What an edit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Delete,
    Recolour(Rgba),
}

/// A [`StrokesEdit`] that makes sense whatever the page holds: at least one stroke, ascending, no
/// duplicate. Made before the hub's lock is taken: sorting a long list has no reason to hold up the
/// tablet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedEdit {
    pub indices: Vec<u32>,
    pub action: EditAction,
}

impl StrokesEdit {
    pub fn checked(self) -> Result<CheckedEdit, LiveEditError> {
        let (indices, action) = match self {
            StrokesEdit::Delete { indices } => (indices, EditAction::Delete),
            StrokesEdit::Recolour { indices, color } => {
                (indices, EditAction::Recolour(color.checked()?))
            }
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
        /// Memory the strokes hold, worked out once.
        bytes: usize,
    },
    Recoloured(Vec<ColourChange>),
}

impl UndoStep {
    /// A removal of `strokes`.
    pub fn removed(strokes: Vec<PlacedStroke>, cleared: bool) -> UndoStep {
        let bytes = strokes
            .iter()
            .map(|placed| {
                size_of::<PlacedStroke>() + placed.stroke.points.len() * size_of::<Point>()
            })
            .sum();
        UndoStep::Removed {
            strokes: Arc::new(strokes),
            cleared,
            bytes,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            UndoStep::Removed { bytes, .. } => *bytes,
            UndoStep::Recoloured(changes) => changes.len() * size_of::<ColourChange>(),
        }
    }

    fn is_clearing(&self) -> bool {
        matches!(self, UndoStep::Removed { cleared: true, .. })
    }
}

/// A step that was undone. A removal keeps only its places: the strokes are back on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedoStep {
    Remove { indices: Vec<u32>, cleared: bool },
    Recolour(Vec<ColourChange>),
}

/// What can be undone and redone on a page. The tablet's writing is not in it, and does not empty
/// the redo steps either: it moves no place, and only a new edit changes what they would redo.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: VecDeque<UndoStep>,
    redo: Vec<RedoStep>,
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
    }

    /// A new edit: what was undone can no longer be redone.
    pub fn record(&mut self, step: UndoStep) {
        self.redo.clear();
        self.push_undo(step);
    }

    /// Back on the undo stack, without touching the redo steps.
    pub fn push_undo(&mut self, step: UndoStep) {
        self.undo.push_back(step);
        while self.undo.len() > MAX_UNDO_STEPS
            || (self.undo.len() > 1 && self.stored_bytes() > MAX_UNDO_BYTES)
        {
            self.undo.pop_front();
        }
    }

    pub fn pop_undo(&mut self) -> Option<UndoStep> {
        self.undo.pop_back()
    }

    pub fn push_redo(&mut self, step: RedoStep) {
        self.redo.push(step);
    }

    pub fn pop_redo(&mut self) -> Option<RedoStep> {
        self.redo.pop()
    }

    /// Memory the undo steps keep, in bytes.
    pub fn stored_bytes(&self) -> usize {
        self.undo.iter().map(UndoStep::bytes).sum()
    }

    /// The strokes the next undo gives back, if it gives any.
    pub fn next_restored(&self) -> Option<Arc<Vec<PlacedStroke>>> {
        match self.undo.back() {
            Some(UndoStep::Removed { strokes, .. }) => Some(strokes.clone()),
            _ => None,
        }
    }

    /// A « Clear the page » can still be undone.
    pub fn holds_clearing(&self) -> bool {
        self.undo.iter().any(UndoStep::is_clearing)
    }

    /// The next undo is that of a « Clear the page ».
    pub fn next_is_clearing(&self) -> bool {
        self.undo.back().is_some_and(UndoStep::is_clearing)
    }

    /// The page, of size `width` × `height`, turns: the strokes kept to come back turn with it, so
    /// that they return to where they would be if they had stayed. Colours and places do not change.
    pub fn turn(&mut self, width: f32, height: f32, turns: u8) {
        for step in &mut self.undo {
            if let UndoStep::Removed { strokes, .. } = step {
                for placed in Arc::make_mut(strokes) {
                    for point in &mut placed.stroke.points {
                        (point.x, point.y) = turn_point(point.x, point.y, width, height, turns);
                    }
                }
            }
        }
    }
}

/// A copy of the strokes an undo gives back, for the page (the windows get the stored ones): made
/// by the hub before it takes its lock, since those points can be many.
#[derive(Debug)]
pub struct Prepared {
    source: Arc<Vec<PlacedStroke>>,
    copy: Vec<Stroke>,
}

impl Prepared {
    pub fn of(source: Arc<Vec<PlacedStroke>>) -> Prepared {
        let copy = source.iter().map(|placed| placed.stroke.clone()).collect();
        Prepared { source, copy }
    }

    /// The copy of `stored`: `prepared` when it was made from them, else a new one (another
    /// command got in between).
    pub fn copy_of(prepared: Option<Prepared>, stored: &Arc<Vec<PlacedStroke>>) -> Vec<Stroke> {
        match prepared {
            Some(prepared) if Arc::ptr_eq(&prepared.source, stored) => prepared.copy,
            _ => stored.iter().map(|placed| placed.stroke.clone()).collect(),
        }
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

/// `places` of a page in the numbering of a viewer (see [`hidden_before`]).
pub fn viewer_places(hidden_before: &[u32], places: impl Iterator<Item = u32>) -> Vec<u32> {
    places
        .map(|place| {
            let hidden = hidden_before.get(place as usize).copied().unwrap_or(0);
            place.saturating_sub(hidden)
        })
        .collect()
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
