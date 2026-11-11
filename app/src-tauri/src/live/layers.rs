//! The layers of a page during live mode (SPEC §4.4 « Calques », `docs/questions.md` Q11;
//! docs/design/tablet-live-classroom.md, "M14", "Layer operations during live"): what the layers
//! zone of the live window asks, once checked, and how a move or a merge changes the page.
//!
//! The same results as the editor's `ops.moveLayer` and `ops.mergeDown` (`lib/editor/ops.ts`): a
//! page touched in either place ends up alike. No lock and no I/O here.

use ardoise_core::note::{Layer, Note};
use serde::Deserialize;

use super::edits::LiveEditError;

/// Most layers a page may get from live mode (our bound: the tablet's button set to « nouveau
/// calque » does nothing beyond it either).
pub const MAX_LAYERS: usize = 100;
/// Longest name of a layer, in characters, once trimmed.
pub const MAX_LAYER_NAME: usize = 200;

/// What the layers zone asks (`LiveLayerOp` in `live.ts`). Indices are plain numbers, checked
/// here: a bad one gets the coded error rather than the rejection of the command's arguments.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayerOp {
    /// « Ajouter un calque »: a new layer on top, where the computer goes.
    Add,
    Rename {
        layer: f64,
        name: String,
    },
    Visible {
        layer: f64,
        visible: bool,
    },
    /// Layer `from` goes to place `to` (0 = bottom).
    Move {
        from: f64,
        to: f64,
    },
    /// Layer `layer` goes into the one below it.
    Merge {
        layer: f64,
    },
    /// The computer acts on `layer`.
    Computer {
        layer: f64,
    },
    /// The tablet writes on `layer`.
    Tablet {
        layer: f64,
    },
}

/// A [`LayerOp`] whose numbers are indices and whose name is one a layer may have; whether those
/// layers exist is for the page to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckedLayerOp {
    Add,
    Rename { layer: u32, name: String },
    Visible { layer: u32, visible: bool },
    Move { from: u32, to: u32 },
    Merge { layer: u32 },
    Computer { layer: u32 },
    Tablet { layer: u32 },
}

impl LayerOp {
    /// Whether the op only says where a pen acts: it changes no note, so it is allowed once the
    /// session is stopped (as paging is).
    pub fn changes_note(&self) -> bool {
        !matches!(self, LayerOp::Computer { .. } | LayerOp::Tablet { .. })
    }

    /// Checked before the hub's lock (see [`CheckedLayerOp`]).
    pub fn checked(self) -> Result<CheckedLayerOp, LiveEditError> {
        Ok(match self {
            LayerOp::Add => CheckedLayerOp::Add,
            LayerOp::Rename { layer, name } => CheckedLayerOp::Rename {
                layer: index(layer)?,
                name: layer_name(&name)?,
            },
            LayerOp::Visible { layer, visible } => CheckedLayerOp::Visible {
                layer: index(layer)?,
                visible,
            },
            LayerOp::Move { from, to } => CheckedLayerOp::Move {
                from: index(from)?,
                to: index(to)?,
            },
            LayerOp::Merge { layer } => CheckedLayerOp::Merge {
                layer: index(layer)?,
            },
            LayerOp::Computer { layer } => CheckedLayerOp::Computer {
                layer: index(layer)?,
            },
            LayerOp::Tablet { layer } => CheckedLayerOp::Tablet {
                layer: index(layer)?,
            },
        })
    }
}

/// A whole number that can name a layer.
fn index(value: f64) -> Result<u32, LiveEditError> {
    if value.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&value) {
        Ok(value as u32)
    } else {
        Err(LiveEditError::BadLayer)
    }
}

/// The name trimmed: refused empty or longer than [`MAX_LAYER_NAME`] characters.
fn layer_name(name: &str) -> Result<String, LiveEditError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_LAYER_NAME {
        return Err(LiveEditError::BadLayerName);
    }
    Ok(name.to_owned())
}

/// Layer `layer` of `layers`, which must exist.
pub fn existing(layers: &[Layer], layer: u32) -> Result<usize, LiveEditError> {
    let index = layer as usize;
    if index < layers.len() {
        Ok(index)
    } else {
        Err(LiveEditError::BadLayer)
    }
}

/// Where layer index `index` goes when layer `from` moves to place `to` (as the editor's
/// `ops.moveIndex`): `from` becomes `to`, the ones between shift by one towards `from`.
pub fn follow_move(index: u32, from: u32, to: u32) -> u32 {
    if index == from {
        to
    } else if from < to && index > from && index <= to {
        index - 1
    } else if from > to && index >= to && index < from {
        index + 1
    } else {
        index
    }
}

/// Where layer index `index` goes when layer `merged` goes into the one below it: it and the ones
/// above go down by one.
pub fn follow_merge(index: u32, merged: u32) -> u32 {
    if index >= merged && merged > 0 {
        index - 1
    } else {
        index
    }
}

/// Moves layer `from` (an index of `note.layers`) to place `to` (another one): the strokes follow
/// their layer and keep their places, so that the revision of the page and a selection stay. A
/// stroke of a layer that does not exist keeps its number, as in the editor.
pub fn move_layer(note: &mut Note, from: u32, to: u32) {
    let count = note.layers.len();
    let layer = note.layers.remove(from as usize);
    note.layers.insert(to as usize, layer);
    for stroke in &mut note.strokes {
        if (stroke.layer as usize) < count {
            stroke.layer = follow_move(stroke.layer, from, to);
        }
    }
}

/// Merges layer `merged` (1 or more, an index of `note.layers`) into the one below it, as the
/// editor's `ops.mergeDown`: the lower layer keeps its name and shows if either did; the strokes
/// of `merged` take its index and move after all the others in the list, so that they stay drawn
/// above the lower layer's ink (drawing order is layer, then list); every stroke above shifts down.
/// Moves the strokes, copies no point.
pub fn merge_layer(note: &mut Note, merged: u32) {
    let index = merged as usize;
    let upper = note.layers.remove(index);
    if let Some(lower) = note.layers.get_mut(index - 1) {
        lower.visible |= upper.visible;
    }
    let strokes = std::mem::take(&mut note.strokes);
    let (mut moved, mut kept): (Vec<_>, Vec<_>) = strokes
        .into_iter()
        .partition(|stroke| stroke.layer == merged);
    for stroke in &mut kept {
        if stroke.layer > merged {
            stroke.layer -= 1;
        }
    }
    for stroke in &mut moved {
        stroke.layer = merged - 1;
    }
    kept.append(&mut moved);
    note.strokes = kept;
}

#[cfg(test)]
mod tests;
