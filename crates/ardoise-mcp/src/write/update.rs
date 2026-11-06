//! `update_note`, `move_note`, `trash_note`: metadata, binder and trash, all over
//! `ardoise-core`'s library operations (the same code as the library screen).

use std::path::{Path, PathBuf};

use ardoise_core::library::{Library, LoadedNote, normalize_tags, parse_conflict_name};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::error::{ToolError, ToolResult, json_result};
use crate::host::Host;
use crate::notes::{NoteSummary, binder_text, find_note, iso, relative_path};
use crate::read::clean_binder;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateNoteArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// New title (empty for none: the app then shows the creation date). The file is renamed to match.
    pub title: Option<String>,
    /// Replace all the note's tags with these.
    pub tags: Option<Vec<String>>,
    /// Tags to add (after `tags`, if both are given).
    pub add_tags: Option<Vec<String>>,
    /// Tags to remove (case ignored).
    pub remove_tags: Option<Vec<String>>,
    /// Mark or unmark the note as a favourite.
    pub favorite: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MoveNoteArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// The binder to move it to: a path relative to the library, `/`-separated, created if missing. Empty for the root of the library.
    pub binder: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TrashNoteArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
}

/// The summary of the note just read.
pub(super) fn summary_of(library: &Library, loaded: &LoadedNote) -> NoteSummary {
    let note = &loaded.file.note;
    let binder = loaded
        .path
        .parent()
        .and_then(|parent| parent.strip_prefix(library.root()).ok())
        .map(binder_text)
        .unwrap_or_default();
    let mut summary = NoteSummary {
        id: note.id.clone(),
        path: relative_path(library, &loaded.path),
        binder,
        title: note.title.clone(),
        created: iso(note.created),
        modified: iso(note.modified),
        tags: note.tags.clone(),
        favorite: note.favorite,
        page: None,
        stroke_count: None,
        has_background_image: false,
        conflict_copy_of: loaded
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_conflict_name)
            .map(|conflict| conflict.original_name),
    };
    summary.add_note(note);
    summary
}

/// The summary of the note at `path`, read now.
pub(super) fn summary_at(library: &Library, path: &Path) -> Result<NoteSummary, ToolError> {
    Ok(summary_of(library, &library.read(path)?))
}

/// Longest title the tools accept, characters: a title is a line of a list and a file name, not a
/// document (supposé; the app's own title field has no limit, the file format's is 16 MiB).
const MAX_TITLE_CHARS: usize = 1_000;

/// Refuses a title no list could show: `invalidArguments`, before anything is done.
pub(super) fn check_title(title: &str) -> Result<(), ToolError> {
    let count = title.chars().count();
    if count > MAX_TITLE_CHARS {
        return Err(ToolError::invalid(format!(
            "`title` has {count} characters: at most {MAX_TITLE_CHARS} are accepted"
        )));
    }
    Ok(())
}

/// Current time in Unix milliseconds.
pub(super) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// What to change in a note's metadata.
#[derive(Default)]
pub(super) struct MetadataChange<'a> {
    pub title: Option<&'a str>,
    pub tags: Option<&'a [String]>,
    pub add_tags: &'a [String],
    pub remove_tags: &'a [String],
    pub favorite: Option<bool>,
}

impl MetadataChange<'_> {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.tags.is_none()
            && self.add_tags.is_empty()
            && self.remove_tags.is_empty()
            && self.favorite.is_none()
    }
}

/// Changes the metadata of the note at `path`: tags and favourite mark in one save with the
/// stamp check (the other members of the file are kept as they are), then the title, which
/// renames the file. Returns where the note is now and which fields changed.
pub(super) fn apply_metadata(
    library: &Library,
    path: &Path,
    change: &MetadataChange<'_>,
) -> Result<(PathBuf, Vec<&'static str>), ToolError> {
    let loaded = library.read(path)?;
    let mut file = loaded.file;
    let mut changed = Vec::new();

    let mut tags = match change.tags {
        Some(tags) => normalize_tags(tags),
        None => file.note.tags.clone(),
    };
    tags.extend(change.add_tags.iter().cloned());
    let mut tags = normalize_tags(&tags);
    let removed = normalize_tags(change.remove_tags);
    tags.retain(|tag| {
        !removed
            .iter()
            .any(|r| r.to_lowercase() == tag.to_lowercase())
    });
    if tags != file.note.tags {
        file.note.tags = tags;
        changed.push("tags");
    }
    if let Some(favorite) = change.favorite
        && favorite != file.note.favorite
    {
        file.note.favorite = favorite;
        changed.push("favorite");
    }

    let mut path = loaded.path;
    let mut stamp = loaded.stamp;
    if !changed.is_empty() {
        file.note.modified = now_ms().max(file.note.modified);
        stamp = library.save(&path, &file, &stamp)?;
    }
    if let Some(title) = change.title {
        let title = title.trim();
        if title != file.note.title {
            path = library.retitle(&path, title, Some(&stamp))?.path;
            changed.push("title");
        }
    }
    Ok((path, changed))
}

pub fn update_note(host: &Host, args: UpdateNoteArgs) -> ToolResult {
    let change = MetadataChange {
        title: args.title.as_deref(),
        tags: args.tags.as_deref(),
        add_tags: args.add_tags.as_deref().unwrap_or_default(),
        remove_tags: args.remove_tags.as_deref().unwrap_or_default(),
        favorite: args.favorite,
    };
    if change.is_empty() {
        return Err(ToolError::invalid(
            "nothing to change: give `title`, `tags`, `add_tags`, `remove_tags` or `favorite`",
        ));
    }
    change.title.map_or(Ok(()), check_title)?;
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let (path, changed) = apply_metadata(&library, &entry.path, &change)?;
    json_result(json!({
        "note": summary_at(&library, &path)?,
        "changed": changed,
    }))
}

pub fn move_note(host: &Host, args: MoveNoteArgs) -> ToolResult {
    let binder = clean_binder(&args.binder);
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let path = library.move_to(&entry.path, Path::new(&binder))?;
    json_result(json!({
        "note": summary_at(&library, &path)?,
        "moved": path != entry.path,
        // The file name changed when the binder already had a note of that name.
        "renamed": path.file_name() != entry.path.file_name(),
    }))
}

pub fn trash_note(host: &Host, args: TrashNoteArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let summary = NoteSummary::of_entry(&library, &entry);
    let item = library.trash(&entry.path)?;
    json_result(json!({
        "trashed": summary,
        "trash_id": item.id,
        "restore": "Ardoise's trash view restores it (the note stays there 30 days).",
    }))
}
