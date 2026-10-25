//! Split and merge notes (SPEC §4.4): pure functions on [`Note`], no I/O.
//!
//! Contract: `docs/design/exports-and-operations.md`. Ids, titles and dates of the notes made
//! here come from the caller ([`NewNote`]); the source notes are never modified. Writing the
//! results into the library (atomically) is up to the caller.

use ardoise_ink::{Point, Stroke};

use crate::library::normalize_tags;
use crate::note::{Layer, Note, Origin, PageSize, Timing};

/// Longest side of a page [`merge`] accepts: 100 m, in device units (10 µm).
pub const MAX_PAGE_SIDE: u32 = 10_000_000;

/// Gap between two merged notes in the replay: each note starts this long after the previous
/// one ends.
pub const MERGE_GAP_MS: u32 = 1000;

/// Why an operation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpsError {
    /// One side of the split would be empty: nothing would move, or nothing would stay.
    #[error("the split would leave one of the two notes empty")]
    NothingToSplit,
    /// A merge needs at least two notes.
    #[error("merging needs at least two notes")]
    NotEnough,
    /// The merged page would be larger than [`MAX_PAGE_SIDE`] on one side.
    #[error("the merged page would be larger than 100 m")]
    PageTooLarge,
}

/// What the caller decides for the note an operation creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNote {
    /// UUID of the new note.
    pub id: String,
    pub title: String,
    /// Current time, Unix milliseconds: becomes `modified`.
    pub now: i64,
}

/// The two notes a split makes.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// The source note without what moved: same id, title, origin, everything else.
    pub kept: Note,
    /// What moved: a new note.
    pub extracted: Note,
}

/// How [`merge`] lays the notes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeLayout {
    /// One page as large as the largest, every note at its top-left corner.
    Overlay,
    /// One long page: the notes one below the other.
    EndToEnd,
}

/// First and last sample time of the note, in milliseconds since `created`; `None` without
/// any sample.
pub fn time_range(note: &Note) -> Option<(u32, u32)> {
    let mut times = note
        .strokes
        .iter()
        .flat_map(|stroke| stroke.points.iter().map(|p| p.t));
    let first = times.next()?;
    Some(times.fold((first, first), |(lo, hi), t| (lo.min(t), hi.max(t))))
}

fn has_points(strokes: &[Stroke]) -> bool {
    strokes.iter().any(|s| !s.points.is_empty())
}

/// The note made of what moved: a new, non-favourite note of this app, in the page, background,
/// layers, tags and timing of `source`.
fn extracted_note(source: &Note, new: NewNote, created: i64, strokes: Vec<Stroke>) -> Note {
    Note {
        id: new.id,
        title: new.title,
        created,
        modified: new.now,
        page: source.page,
        background: source.background,
        background_image: source.background_image.clone(),
        tags: source.tags.clone(),
        favorite: false,
        layers: source.layers.clone(),
        strokes,
        timing: source.timing,
        origin: Origin::Ardoise,
    }
}

fn kept_note(source: &Note, new_now: i64, strokes: Vec<Stroke>) -> Note {
    Note {
        modified: new_now.max(source.modified),
        strokes,
        ..source.clone()
    }
}

/// Splits at an instant: samples with `t < at_ms` stay, samples with `t >= at_ms` go to the
/// new note. A stroke that spans the instant is cut into two strokes. The new note's `created`
/// is `note.created` plus its first sample time, and its times are shifted to start at 0, so
/// that its replay starts right away. Strokes without any sample stay where they are.
///
/// Refused with [`OpsError::NothingToSplit`] when no sample is before the instant, or none is
/// at or after it.
pub fn split_at_time(note: &Note, at_ms: u32, new: NewNote) -> Result<Split, OpsError> {
    let first_moved = note
        .strokes
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.t))
        .filter(|&t| t >= at_ms)
        .min()
        .ok_or(OpsError::NothingToSplit)?;
    let mut kept = Vec::new();
    let mut moved = Vec::new();
    for stroke in &note.strokes {
        let (before, after): (Vec<Point>, Vec<Point>) =
            stroke.points.iter().partition(|p| p.t < at_ms);
        // A stroke entirely after the instant leaves nothing behind.
        if !before.is_empty() || stroke.points.is_empty() {
            kept.push(Stroke {
                points: before,
                ..stroke.clone()
            });
        }
        if !after.is_empty() {
            let points = after
                .into_iter()
                .map(|p| Point {
                    t: p.t - first_moved,
                    ..p
                })
                .collect();
            moved.push(Stroke {
                points,
                ..stroke.clone()
            });
        }
    }
    if !has_points(&kept) {
        return Err(OpsError::NothingToSplit);
    }
    let created = note.created.saturating_add(i64::from(first_moved));
    let now = new.now;
    Ok(Split {
        extracted: extracted_note(note, new, created, moved),
        kept: kept_note(note, now, kept),
    })
}

/// Even-odd point-in-polygon test, the one of the editor (`pointInPolygon` in `geometry.ts`),
/// computed in double precision like the interface does.
fn in_polygon(polygon: &[[f32; 2]], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for i in 0..polygon.len() {
        let (ax, ay) = (f64::from(polygon[i][0]), f64::from(polygon[i][1]));
        let (bx, by) = (f64::from(polygon[j][0]), f64::from(polygon[j][1]));
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Splits by a zone: a stroke on a visible layer goes to the new note when at least half of its
/// samples (and at least one) are inside `polygon` — the rule of the editor's lasso
/// (`selectInLasso`, `app/ui/src/lib/editor/hit.ts`), so that what the user sees highlighted is
/// what moves. Positions and times are unchanged, and so is `created`.
///
/// Refused with [`OpsError::NothingToSplit`] when no stroke moves (also for a polygon of fewer
/// than three points), or when none would stay.
pub fn split_by_zone(note: &Note, polygon: &[[f32; 2]], new: NewNote) -> Result<Split, OpsError> {
    if polygon.len() < 3 {
        return Err(OpsError::NothingToSplit);
    }
    let visible = |layer: u32| {
        note.layers
            .get(layer as usize)
            .is_none_or(|layer| layer.visible)
    };
    let mut kept = Vec::new();
    let mut moved = Vec::new();
    for stroke in &note.strokes {
        let inside = if visible(stroke.layer) {
            stroke
                .points
                .iter()
                .filter(|p| in_polygon(polygon, f64::from(p.x), f64::from(p.y)))
                .count()
        } else {
            0
        };
        // `inside * 2 >= len` is `inside >= 0.5 * len`, without rounding.
        if inside > 0 && inside * 2 >= stroke.points.len() {
            moved.push(stroke.clone());
        } else {
            kept.push(stroke.clone());
        }
    }
    if !has_points(&moved) || !has_points(&kept) {
        return Err(OpsError::NothingToSplit);
    }
    let now = new.now;
    Ok(Split {
        extracted: extracted_note(note, new, note.created, moved),
        kept: kept_note(note, now, kept),
    })
}

/// The name of a source note's layer in the merged note (overlay): the layer's own name
/// prefixed with the note's title when the note has several layers, the title alone when it has
/// one (the layer's name when the title is empty).
fn overlay_layer_name(note: &Note, layer: &Layer) -> String {
    match (note.title.trim(), note.layers.len()) {
        ("", _) => layer.name.clone(),
        (title, 1) => title.to_owned(),
        (title, _) => format!("{title} - {}", layer.name),
    }
}

/// Merges notes into a new one, in the order given (the interface passes them oldest first).
///
/// - [`MergeLayout::Overlay`]: one page as large as the largest (widest by tallest), every note
///   at its top-left corner; the layers of each note are appended after those of the previous
///   one, named as [`overlay_layer_name`] says.
/// - [`MergeLayout::EndToEnd`]: one long page (widest by the sum of the heights), the notes one
///   below the other, left-aligned; layers merged by index (name from the first note that
///   has the layer; visible if the layer is visible in any note that has it).
///
/// Background (of the first note), tags (union) and `created` (of the first note) follow;
/// timing is [`Timing::Synthesized`] if any note's is. Times are made sequential: the first note
/// keeps its times, each other note starts [`MERGE_GAP_MS`] after the previous one ends, so
/// that the replay plays the notes in order. A stroke on a layer its note does not have is put
/// on the note's last layer. The result is a new note of this app, not a favourite.
///
/// Fails with [`OpsError::NotEnough`] for fewer than two notes and [`OpsError::PageTooLarge`]
/// when a side of the page would exceed [`MAX_PAGE_SIDE`].
pub fn merge(notes: &[Note], layout: MergeLayout, new: NewNote) -> Result<Note, OpsError> {
    let [first, _, ..] = notes else {
        return Err(OpsError::NotEnough);
    };
    let width = notes.iter().map(|n| n.page.width).max().unwrap_or(0);
    let height = match layout {
        MergeLayout::Overlay => u64::from(notes.iter().map(|n| n.page.height).max().unwrap_or(0)),
        MergeLayout::EndToEnd => notes.iter().map(|n| u64::from(n.page.height)).sum(),
    };
    if width > MAX_PAGE_SIDE || height > u64::from(MAX_PAGE_SIDE) {
        return Err(OpsError::PageTooLarge);
    }
    let page = PageSize {
        width,
        height: height as u32,
    };

    let mut layers: Vec<Layer> = Vec::new();
    // Where each note's layers start in the merged list, and where it starts on the page.
    let mut layer_base = Vec::with_capacity(notes.len());
    let mut y_offset = Vec::with_capacity(notes.len());
    let mut y = 0.0_f32;
    for note in notes {
        match layout {
            MergeLayout::Overlay => {
                layer_base.push(layers.len());
                y_offset.push(0.0);
                layers.extend(note.layers.iter().map(|layer| Layer {
                    name: overlay_layer_name(note, layer),
                    visible: layer.visible,
                }));
            }
            MergeLayout::EndToEnd => {
                layer_base.push(0);
                y_offset.push(y);
                y += note.page.height as f32;
                for (i, layer) in note.layers.iter().enumerate() {
                    match layers.get_mut(i) {
                        // Visible if it is in any note that has this layer, so that no note's
                        // strokes vanish from the merged note and its exports.
                        Some(merged) => merged.visible |= layer.visible,
                        None => layers.push(layer.clone()),
                    }
                }
            }
        }
    }
    if layers.is_empty() {
        layers.push(Layer {
            name: new.title.clone(),
            visible: true,
        });
    }
    let last_layer = layers.len() - 1;

    let mut strokes = Vec::new();
    let mut previous_end: Option<i64> = None;
    for (index, note) in notes.iter().enumerate() {
        let shift = match (time_range(note), previous_end) {
            (Some((start, _)), Some(end)) => end + i64::from(MERGE_GAP_MS) - i64::from(start),
            _ => 0,
        };
        if let Some((_, end)) = time_range(note) {
            previous_end = Some(i64::from(end) + shift);
        }
        let own_last = note.layers.len().saturating_sub(1);
        for stroke in &note.strokes {
            let local = (stroke.layer as usize).min(own_last);
            let layer = (layer_base[index] + local).min(last_layer);
            let points = stroke
                .points
                .iter()
                .map(|p| Point {
                    y: p.y + y_offset[index],
                    t: (i64::from(p.t) + shift).clamp(0, i64::from(u32::MAX)) as u32,
                    ..*p
                })
                .collect();
            strokes.push(Stroke {
                layer: layer as u32,
                points,
                ..stroke.clone()
            });
        }
    }

    let tags: Vec<String> = notes.iter().flat_map(|n| n.tags.iter().cloned()).collect();
    Ok(Note {
        id: new.id,
        title: new.title,
        created: first.created,
        modified: new.now,
        page,
        background: first.background,
        // TODO(M7 background agent): offset/clip it for side-by-side assembly.
        background_image: first.background_image.clone(),
        tags: normalize_tags(&tags),
        favorite: false,
        layers,
        strokes,
        timing: if notes.iter().any(|n| n.timing == Timing::Synthesized) {
            Timing::Synthesized
        } else {
            Timing::Recorded
        },
        origin: Origin::Ardoise,
    })
}

#[cfg(test)]
mod tests;
