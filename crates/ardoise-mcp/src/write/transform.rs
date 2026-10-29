//! `merge_notes`, `split_note`, `straighten_shapes`: the operations that make notes out of
//! notes, and the one that redraws ink. Merging and splitting are the app's own code (they
//! carry the narration and the background image along); straightening is
//! `ardoise_ink::shapes::straighten`, applied the way the editor applies it and saved once,
//! after the version on disk was put in the library's trash as a note of its own.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ardoise_core::library::{
    Library, LoadedNote, NOTE_EXTENSION, TRASH_RETENTION, TrashItem, note_file_stem,
};
use ardoise_core::note::Note;
use ardoise_hwr::structure::Item;
use ardoise_hwr::{Language, Recognition};
use ardoise_ink::{Shape, Straightened, Stroke, straighten};
use chrono::{DateTime, Local};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::SplitRequest;
use super::create::uuid_v4;
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
    /// Indices of the strokes to try (from `get_note` with `strokes`, or the `strokes` of the drawings in `get_note_text`'s structure). Default: the strokes of the drawings that Windows' ink analysis finds on the visible layers (an error tells when the analysis is not available on this PC: then give them).
    pub strokes: Option<Vec<usize>>,
}

/// The words the app adds to a title (`library.assemble.defaultTitle`, `editor.split.titleTime`,
/// `editor.split.titleZone`, `editor.untitled`), in the language of the app.
struct Words {
    assembled: &'static str,
    continued: &'static str,
    excerpt: &'static str,
    untitled: &'static str,
    /// The title of the version kept before straightening (no key of the interface: the MCP
    /// server alone makes it).
    before_straightening: &'static str,
}

fn words(language: Language) -> Words {
    match language {
        Language::Fr => Words {
            assembled: "assemblage",
            continued: "suite",
            excerpt: "extrait",
            untitled: "Note sans titre",
            before_straightening: "avant redressement",
        },
        Language::En => Words {
            assembled: "assembled",
            continued: "continued",
            excerpt: "excerpt",
            untitled: "Untitled note",
            before_straightening: "before straightening",
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

/// The strokes the editor would let the user pick as drawings: those of the drawings Windows' ink
/// analysis finds, on visible layers. Refused when the analysis is not available here (the
/// layout heuristics guess drawings from what the recogniser did not read, which may be
/// letters).
fn drawing_strokes(host: &Host, note: &Note) -> Result<Vec<usize>, ToolError> {
    // The analysis does not need the recognised text to find the drawings: a PC without the
    // handwriting pack may still have it.
    let recognition = host
        .handwriting
        .recognition_of(note, None)
        .unwrap_or_else(|_| Arc::new(Recognition::empty(host.environment.language())));
    let structure = host.handwriting.structure_of(note, &recognition);
    if !structure.analyzed {
        return Err(ToolError::invalid(
            "Windows' ink analysis is not available on this computer, so the drawings cannot be told from the handwriting: pass `strokes` (indices from `get_note` with `strokes`, after looking at the note with `render_note`) to say which strokes to straighten",
        ));
    }
    let visible = |index: &usize| {
        note.strokes.get(*index).is_some_and(|stroke| {
            note.layers
                .get(stroke.layer as usize)
                .is_some_and(|layer| layer.visible)
        })
    };
    let mut strokes: Vec<usize> = structure
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Drawing(area) => Some(area.strokes.iter().copied()),
            _ => None,
        })
        .flatten()
        .filter(visible)
        .collect();
    strokes.sort_unstable();
    strokes.dedup();
    Ok(strokes)
}

/// Puts the version of the note read in `loaded` into the library's trash as a note of its
/// own: a new id (it must never stand for the note itself once restored), its title followed by
/// "(before straightening)", named as if it had been next to the note. The user restores it from
/// the trash view of the app.
fn keep_previous(
    host: &Host,
    library: &Library,
    loaded: &LoadedNote,
) -> Result<TrashItem, ToolError> {
    let words = words(host.environment.language());
    let mut copy = loaded.file.clone();
    copy.note.id = uuid_v4();
    let base = match copy.note.title.trim() {
        "" => words.untitled.to_owned(),
        title => title.to_owned(),
    };
    copy.note.title = format!("{base} ({})", words.before_straightening);
    let name = format!(
        "{}.{NOTE_EXTENSION}",
        note_file_stem(copy.note.created, &copy.note.title)
    );
    let beside = loaded
        .path
        .parent()
        .map_or_else(|| library.root().join(&name), |dir| dir.join(&name));
    Ok(library.trash_copy(&copy, &beside)?)
}

pub fn straighten_shapes(host: &Host, args: StraightenArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = library.read(&entry.path)?;
    let mut note = loaded.file.note.clone();
    let count = note.strokes.len();

    let (selected, chosen_by): (Vec<usize>, &str) = match &args.strokes {
        None => (drawing_strokes(host, &note)?, "ink_analysis"),
        Some(indices) => {
            if let Some(bad) = indices.iter().find(|&&i| i >= count) {
                return Err(ToolError::invalid(format!(
                    "no stroke {bad}: this note has {count} stroke(s), numbered from 0"
                )));
            }
            let mut sorted = indices.clone();
            sorted.sort_unstable();
            sorted.dedup();
            (sorted, "given")
        }
    };
    let given: Vec<Stroke> = selected.iter().map(|&i| note.strokes[i].clone()).collect();
    let groups = straighten(&given);
    if groups.is_empty() {
        // Nothing is a clear shape: the note is left as it is, not even saved.
        return json_result(json!({
            "straightened": 0,
            "saved": false,
            "strokes_tried": selected,
            "chosen_by": chosen_by,
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
    // The version on disk first, so that nothing is lost whatever happens next.
    let kept = keep_previous(host, &library, &loaded)?;
    // One save, with the stamp of the version that was read: a note changed meanwhile (another
    // PC, the app) is refused with `noteConflict` and left alone, and the copy of a version
    // that was not replaced is taken back.
    if let Err(error) = host
        .write
        .save_note(&library, &entry.path, note, &loaded.stamp)
    {
        let _ = library.purge(&kept.id);
        return Err(error);
    }
    let saved = library.read(&entry.path)?;
    let days = TRASH_RETENTION.as_secs() / 86_400;
    let title = kept.title.clone().unwrap_or_default();
    json_result(json!({
        "straightened": groups.len(),
        "saved": true,
        "shapes": shapes,
        "strokes_tried": selected,
        "chosen_by": chosen_by,
        "stroke_count": new_count,
        "modified": iso(saved.file.note.modified),
        "note": super::update::summary_of(&library, &saved),
        "previous_version": {
            "in_trash": kept.id,
            "title": title,
            "id": kept.note_id,
            "restore": format!(
                "the version before straightening is in the library's trash as the note \"{title}\": the user restores it from the trash view of the app (kept {days} days)"
            ),
        },
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
