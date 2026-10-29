//! `merge_notes`, `split_note`, `straighten_shapes`: the operations that make notes out of
//! notes, and the one that redraws ink. Merging and splitting are the app's own code (they
//! carry the narration and the background image along); straightening is
//! `ardoise_ink::shapes::straighten`, applied the way the editor applies it and saved once.

use std::collections::{HashMap, HashSet};

use ardoise_hwr::Language;
use ardoise_ink::{Shape, Straightened, Stroke, straighten};
use chrono::{DateTime, Local};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::SplitRequest;
use super::update::summary_at;
use crate::error::{ToolError, ToolResult, json_result};
use crate::host::Host;
use crate::notes::{find_note, iso};

/// How merged notes are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MergeLayout {
    /// All the ink on one page (as large as the largest).
    Overlay,
    /// The pages one after the other, on a longer page.
    EndToEnd,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MergeNotesArgs {
    /// At least two notes, by `id` or path, in the order they are merged (the first one gives the binder and the background image or PDF page; the others' images are not kept).
    pub notes: Vec<String>,
    /// `overlay` or `endToEnd`.
    pub layout: MergeLayout,
    /// Title of the new note (default: the first note's title with " (assembled)").
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SplitNoteArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// Everything written from this instant on goes to the new note: milliseconds on the clock of the strokes (`start_ms` and `end_ms` in `get_note` with `strokes`). Give this or `polygon`.
    pub at_ms: Option<u32>,
    /// A polygon of the page in millimetres from its top-left corner, `[[x, y], ...]` (at least 3 points): the strokes with at least half of their samples inside go to the new note. Give this or `at_ms`.
    pub polygon: Option<Vec<[f64; 2]>>,
    /// Title of the new note (default: the note's title with " (continued)" or " (excerpt)").
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct StraightenArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// Indices of the strokes to try (from `get_note` with `strokes`), default all of them.
    pub strokes: Option<Vec<usize>>,
}

/// The words the app adds to a title (`library.assemble.defaultTitle`, `editor.split.titleTime`,
/// `editor.split.titleZone`, `editor.untitled`), in the language of the app.
struct Words {
    assembled: &'static str,
    continued: &'static str,
    excerpt: &'static str,
    untitled: &'static str,
}

fn words(language: Language) -> Words {
    match language {
        Language::Fr => Words {
            assembled: "assemblage",
            continued: "suite",
            excerpt: "extrait",
            untitled: "Note sans titre",
        },
        Language::En => Words {
            assembled: "assembled",
            continued: "continued",
            excerpt: "excerpt",
            untitled: "Untitled note",
        },
    }
}

/// The date of `created_ms` without a time, as a title (the app proposes one for untitled
/// notes).
fn date_title(created_ms: i64) -> String {
    DateTime::from_timestamp_millis(created_ms)
        .map(|at| at.with_timezone(&Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

pub fn merge_notes(host: &Host, args: MergeNotesArgs) -> ToolResult {
    if args.notes.len() < 2 {
        return Err(ToolError::new(
            "nothingToMerge",
            "at least two notes are needed to merge them",
        ));
    }
    let library = host.library()?;
    let entries = args
        .notes
        .iter()
        .map(|reference| find_note(&library, reference))
        .collect::<Result<Vec<_>, _>>()?;
    let title = match args.title.as_deref().map(str::trim) {
        Some(title) => title.to_owned(),
        None => {
            let first = &entries[0];
            let base = match first.title.trim() {
                "" => date_title(first.created),
                title => title.to_owned(),
            };
            format!("{base} ({})", words(host.environment.language()).assembled)
        }
    };
    let paths: Vec<_> = entries.iter().map(|entry| entry.path.clone()).collect();
    let merged = host
        .write
        .merge_notes(&library, &paths, args.layout, &title)?;
    json_result(json!({
        "note": summary_at(&library, &merged)?,
        "sources": entries.len(),
    }))
}

pub fn split_note(host: &Host, args: SplitNoteArgs) -> ToolResult {
    let request = match (args.at_ms, &args.polygon) {
        (Some(at), None) => SplitRequest::AtMs(at),
        (None, Some(polygon)) => {
            if polygon.len() < 3 || polygon.iter().flatten().any(|v| !v.is_finite()) {
                return Err(ToolError::invalid(
                    "`polygon` needs at least 3 points `[x, y]` in millimetres",
                ));
            }
            // Millimetres to device units (10 µm).
            SplitRequest::Polygon(
                polygon
                    .iter()
                    .map(|[x, y]| [(x * 100.0) as f32, (y * 100.0) as f32])
                    .collect(),
            )
        }
        _ => {
            return Err(ToolError::invalid(
                "give exactly one of `at_ms` and `polygon`",
            ));
        }
    };
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let title = match args.title.as_deref().map(str::trim) {
        Some(title) => title.to_owned(),
        None => {
            let words = words(host.environment.language());
            let base = match entry.title.trim() {
                "" => words.untitled,
                title => title,
            };
            let suffix = match request {
                SplitRequest::AtMs(_) => words.continued,
                SplitRequest::Polygon(_) => words.excerpt,
            };
            format!("{base} ({suffix})")
        }
    };
    let created = host
        .write
        .split_note(&library, &entry.path, &request, &title)?;
    json_result(json!({
        "kept": summary_at(&library, &entry.path)?,
        "created": summary_at(&library, &created)?,
    }))
}

/// The strokes of `strokes` with the shapes of `groups` (recognised among the `selected` ones)
/// replaced by their straightened version, as the editor does: a group's new strokes take the
/// place of the first stroke it replaces, so that they stay above and below the same ink.
pub(super) fn replace_straightened(
    strokes: &[Stroke],
    selected: &[usize],
    groups: &[Straightened],
) -> Vec<Stroke> {
    let mut removed: HashSet<usize> = HashSet::new();
    let mut placed: HashMap<usize, &[Stroke]> = HashMap::new();
    for group in groups {
        let indices: Vec<usize> = group
            .sources
            .iter()
            .filter_map(|&source| selected.get(source).copied())
            .collect();
        let Some(&first) = indices.iter().min() else {
            continue;
        };
        removed.extend(indices);
        placed.insert(first, &group.strokes);
    }
    let mut out = Vec::with_capacity(strokes.len());
    for (index, stroke) in strokes.iter().enumerate() {
        if let Some(made) = placed.get(&index) {
            out.extend(made.iter().cloned());
        } else if !removed.contains(&index) {
            out.push(stroke.clone());
        }
    }
    out
}

fn shape_name(shape: Shape) -> &'static str {
    match shape {
        Shape::Line => "line",
        Shape::Ellipse => "ellipse",
        Shape::Rectangle => "rectangle",
        Shape::Arrow => "arrow",
    }
}

pub fn straighten_shapes(host: &Host, args: StraightenArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = library.read(&entry.path)?;
    let mut note = loaded.file.note;
    let count = note.strokes.len();

    let selected: Vec<usize> = match &args.strokes {
        None => (0..count).collect(),
        Some(indices) => {
            if let Some(bad) = indices.iter().find(|&&i| i >= count) {
                return Err(ToolError::invalid(format!(
                    "no stroke {bad}: this note has {count} stroke(s), numbered from 0"
                )));
            }
            let mut sorted = indices.clone();
            sorted.sort_unstable();
            sorted.dedup();
            sorted
        }
    };
    let given: Vec<Stroke> = selected.iter().map(|&i| note.strokes[i].clone()).collect();
    let groups = straighten(&given);
    if groups.is_empty() {
        // Nothing is a clear shape: the note is left as it is, not even saved.
        return json_result(json!({
            "straightened": 0,
            "saved": false,
            "note": summary_at(&library, &entry.path)?,
        }));
    }
    let shapes: Vec<_> = groups
        .iter()
        .map(|group| {
            json!({
                "shape": shape_name(group.shape),
                "strokes": group
                    .sources
                    .iter()
                    .filter_map(|&s| selected.get(s))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    note.strokes = replace_straightened(&note.strokes, &selected, &groups);
    let new_count = note.strokes.len();
    // One save, with the stamp of the version that was read: a note changed meanwhile (another
    // PC, the app) is refused with `noteConflict` and left alone.
    host.write
        .save_note(&library, &entry.path, note, &loaded.stamp)?;
    let saved = library.read(&entry.path)?;
    json_result(json!({
        "straightened": groups.len(),
        "saved": true,
        "shapes": shapes,
        "stroke_count": new_count,
        "modified": iso(saved.file.note.modified),
        "note": super::update::summary_of(&library, &saved),
    }))
}

#[cfg(test)]
mod tests {
    use ardoise_ink::{Point, Rgba, Tool};

    use super::*;

    fn stroke(layer: u32, x: f32) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 50.0,
            layer,
            points: vec![Point {
                x,
                y: 0.0,
                pressure: 0.5,
                t: 0,
            }],
        }
    }

    #[test]
    fn straightened_strokes_take_the_place_of_the_first_they_replace() {
        let strokes: Vec<Stroke> = (0..5).map(|i| stroke(0, i as f32)).collect();
        // Strokes 1, 2 and 4 were tried (positions 0, 1 and 2 of the selection); the group made
        // of the selection's first two strokes becomes one stroke at the place of stroke 1.
        let selected = [1, 2, 4];
        let groups = [Straightened {
            sources: vec![0, 1],
            shape: Shape::Arrow,
            strokes: vec![stroke(0, 100.0)],
        }];
        let out = replace_straightened(&strokes, &selected, &groups);
        let xs: Vec<f32> = out.iter().map(|s| s.points[0].x).collect();
        assert_eq!(xs, [0.0, 100.0, 3.0, 4.0]);
    }
}
