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
//! The computer's strokes are steps (`Added`); the tablet's never are. Steps of one gesture of the
//! computer's eraser share a group, undone and redone together (M14).
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
        /// Memory the strokes hold, worked out once.
        bytes: usize,
    },
    Recoloured(Vec<ColourChange>),
    /// Strokes the computer wrote (M14), at these places (ascending): they are on the page, the
    /// step holds nothing else.
    Added {
        indices: Vec<u32>,
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
    /// A removal of `strokes`.
    pub fn removed(strokes: Vec<PlacedStroke>, cleared: bool) -> UndoStep {
        let bytes = stored_size(&strokes);
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

    /// The strokes the next undo gives back (all the steps of the group on top), to copy before
    /// the lock is taken.
    pub fn next_restored(&self) -> Vec<Arc<Vec<PlacedStroke>>> {
        let Some(top) = self.undo.back() else {
            return Vec::new();
        };
        self.undo
            .iter()
            .rev()
            .take_while(|entry| std::ptr::eq(*entry, top) || same_group(entry.group, top.group))
            .filter_map(|entry| match &entry.step {
                UndoStep::Removed { strokes, .. } => Some(strokes.clone()),
                _ => None,
            })
            .collect()
    }

    /// The strokes the next redo gives back (the computer's strokes of the group on top).
    pub fn next_redone(&self) -> Vec<Arc<Vec<PlacedStroke>>> {
        let Some(top) = self.redo.last() else {
            return Vec::new();
        };
        self.redo
            .iter()
            .rev()
            .take_while(|entry| std::ptr::eq(*entry, top) || same_group(entry.group, top.group))
            .filter_map(|entry| match &entry.step {
                RedoStep::Add { strokes } => Some(strokes.clone()),
                _ => None,
            })
            .collect()
    }

    /// A « Clear the page » can still be undone.
    pub fn holds_clearing(&self) -> bool {
        self.undo.iter().any(|entry| entry.step.is_clearing())
    }

    /// The next undo is that of a « Clear the page ».
    pub fn next_is_clearing(&self) -> bool {
        self.undo
            .back()
            .is_some_and(|entry| entry.step.is_clearing())
    }

    /// Every stroke kept to come back (by an undo or a redo), for `change`.
    fn kept_strokes(&mut self, mut change: impl FnMut(&mut Stroke)) {
        let undone = self
            .undo
            .iter_mut()
            .filter_map(|entry| match &mut entry.step {
                UndoStep::Removed { strokes, .. } => Some(strokes),
                _ => None,
            });
        let redone = self
            .redo
            .iter_mut()
            .filter_map(|entry| match &mut entry.step {
                RedoStep::Add { strokes } => Some(strokes),
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
