//! `preview_revision`, `create_revision`, `edit_revision`: an assistant retouches a note on a copy
//! (SPEC §4.9, `docs/design/revision.md`). The assistant decides, `ardoise-revise` carries out its
//! orders on the strokes, and this module is what makes it safe:
//!
//! - **The source's file is never written.** The tools read the source and write a new note
//!   (`create_revision`), or write a note that is already a revision (`edit_revision`, which
//!   refuses any other with `notARevision`). There is no other way in this module to write ink.
//! - `preview_revision` works in memory and writes nothing.
//! - Every write is atomic and checks the stamp of what it read, like the editor's save.
//! - **Orders are tied to the version seen.** A stroke's number is its place in the note: the same
//!   number in another version is another stroke. So `create_revision` and `edit_revision` take the
//!   `version` the assistant was shown, and refuse with `noteConflict` when the note is no longer
//!   that one.
//!
//! The same operations (new notes from a note, and the edit of a revision with other strokes) serve
//! `straighten_shapes` and `split_note`, which is why they are functions of their own.

use ardoise_core::format::NoteFile;
use ardoise_core::format::revision::MAX_ENTRY_CHARS;
use ardoise_core::library::{Library, LoadedNote, NoteEntry, TrashItem};
use ardoise_core::note::Note;
use ardoise_ink::Stroke;
use ardoise_revise::{Input, Order, Revised, apply};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rmcp::model::ContentBlock;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::create::uuid_v4;
use super::transform::{Words, keep_previous, words};
use super::update::{check_title, now_ms, summary_at, summary_of};
use crate::error::{ToolError, ToolResult, code, content_result, json_result};
use crate::host::Host;
use crate::notes::{find_note, read_note, version_of};
use crate::read::{PictureRequest, Region, draw_picture, label_spots};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PreviewRevisionArgs {
    /// The note's `id`, or its path relative to the library. It is only read.
    pub note: String,
    /// The orders to carry out, one after the other, on the note as it is now (the schema of each `op` is in the type of this list). Send the whole list each time, built from the note as it is, not the changes since the last preview.
    pub orders: Vec<Order>,
    /// Part of the page to draw (default: the whole page); look closer at what changes.
    pub region: Option<Region>,
    /// Dots per inch (default 100, at most 300); lowered so that the image's longest side stays at most 2000 pixels.
    #[schemars(range(min = 25, max = 300))]
    pub dpi: Option<u32>,
    /// Write each stroke's number next to it (default `false`). These are the numbers of the orders: a stroke keeps its number whatever is done to it, and a stroke an order makes gets the next free one, as `report.orders[i].new_strokes` says. Once the revision is saved, strokes are numbered by their place in the note again.
    pub labels: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateRevisionArgs {
    /// The note's `id`, or its path relative to the library. Its file is never written.
    pub note: String,
    /// The orders to carry out on the note as it is now (the same list as in `preview_revision`; preview it first).
    pub orders: Vec<Order>,
    /// The `version` of the note your orders were written for, from `preview_revision` or `get_note`. If the note changed since (the user, another computer), nothing is written and the answer is `noteConflict`: look at it again, its strokes may have other numbers.
    pub expected_version: String,
    /// The account of what changed, in the user's language: every change, and every letter you made, what is uncertain. It stays with the copy and the user reads it in the editor ("What changed").
    pub summary: String,
    /// Title of the copy (default: the note's title with " (clean)", in the app's language).
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditRevisionArgs {
    /// The id or path of a note that is a revision (a copy made by `create_revision` or `straighten_shapes`). Any other note is refused with `notARevision`.
    pub note: String,
    /// The orders to carry out on the revision's current strokes, numbered as `get_note` with `strokes` and `render_note` with `labels` give them now: this keeps what the user changed by hand in the copy.
    pub orders: Vec<Order>,
    /// The `version` of the note your orders were written for, from `preview_revision` or `get_note`. If the note changed since (the user, another computer), nothing is written and the answer is `noteConflict`: look at it again, its strokes may have other numbers.
    pub expected_version: String,
    /// What this retouching changed, in the user's language: it is added, dated, to the copy's account.
    pub summary: String,
}

/// Carries out `orders` on a copy of the strokes of the note just read: nothing is written. The
/// engine's message goes back as `invalidOrders`.
pub(super) fn revise(loaded: &LoadedNote, orders: &[Order]) -> Result<Revised, ToolError> {
    let note = &loaded.file.note;
    let visible_layers: Vec<bool> = note.layers.iter().map(|layer| layer.visible).collect();
    let input = Input {
        strokes: &note.strokes,
        page_mm: [
            note.page.width as f32 * 0.01,
            note.page.height as f32 * 0.01,
        ],
        layers: note.layers.len() as u32,
        visible_layers: &visible_layers,
        narration: loaded.file.audio.is_some(),
    };
    apply(&input, orders).map_err(|error| ToolError::new(code::INVALID_ORDERS, error))
}

/// Refuses orders written for another version of the note just read: a stroke's number would
/// name another stroke, silently.
fn check_version(loaded: &LoadedNote, expected: &str) -> Result<(), ToolError> {
    let version = version_of(&loaded.stamp);
    if version == expected.trim() {
        return Ok(());
    }
    Err(ToolError::new(
        "noteConflict",
        format!(
            "this note changed since you looked at it (it is at version `{version}`, your orders are for `{expected}`): nothing was written. Look at it again (`get_note`, `preview_revision`) and write your orders for what it is now: its strokes may have other numbers"
        ),
    ))
}

fn required_summary(summary: &str) -> Result<&str, ToolError> {
    match summary.trim() {
        "" => Err(ToolError::invalid(
            "`summary` is required: say, in the user's language, what changed (every change, each letter you made, what is uncertain); it stays with the copy",
        )),
        text if text.chars().count() > MAX_ENTRY_CHARS => Err(ToolError::invalid(format!(
            "`summary` has {} characters: at most {MAX_ENTRY_CHARS} are accepted",
            text.chars().count()
        ))),
        text => Ok(text),
    }
}

fn some_orders(orders: &[Order]) -> Result<(), ToolError> {
    if orders.is_empty() {
        return Err(ToolError::invalid(
            "`orders` is empty: there is nothing to change",
        ));
    }
    Ok(())
}

/// The title of a copy of `note`'s title: "<title> (clean)" in the app's language.
pub(super) fn copy_title(words: &Words, title: &str) -> String {
    let base = match title.trim() {
        "" => words.untitled,
        title => title,
    };
    format!("{base} ({})", words.clean)
}

/// A new note next to `entry`'s note, with `strokes` in place of its strokes and the account
/// `account`: everything else of the source is kept (paper, layers, background image, tags,
/// narration, text), with a new id, the same `created`, `favorite` off. The source's file is only
/// read. Returns the copy as written.
pub(super) fn make_copy(
    host: &Host,
    library: &Library,
    entry: &NoteEntry,
    source: &LoadedNote,
    strokes: Vec<Stroke>,
    account: &str,
    title: Option<&str>,
) -> Result<LoadedNote, ToolError> {
    let words = words(host.environment.language());
    let source_note = &source.file.note;
    let title = match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => title.to_owned(),
        None => copy_title(&words, &source_note.title),
    };
    let now = now_ms();
    // The copy's file starts as the source's: the app finishes it (the thumbnail, and the note's
    // "original" by the editor's rule) knowing the ink it is given besides.
    let mut file = source.file.clone();
    file.note.id = uuid_v4();
    file.note.title = title;
    file.note.favorite = false;
    file.note.modified = now.max(file.note.created);
    file.thumbnail = None;
    file.mark_as_revision_of(source_note, now, account);
    let mut revised = file.note.clone();
    revised.strokes = strokes;
    let saved = host
        .write
        .create_revision(library, &entry.binder, file, revised)?;
    Ok(library.read(&saved.path)?)
}

/// A new note next to `entry`'s note holding `part`, what a split took out of `source`: a note of
/// its own clock and date (`ardoise_core::ops`), so it has neither the narration nor the original of
/// `source`; the page's picture goes with it. It is a revision of `source`, with the account
/// `account`. The source's file is only read. Returns the part as written.
pub(super) fn make_part(
    host: &Host,
    library: &Library,
    entry: &NoteEntry,
    source: &LoadedNote,
    part: Note,
    account: &str,
) -> Result<LoadedNote, ToolError> {
    let mut file = NoteFile::new(part);
    if file.note.background_image.is_some() {
        file.background.clone_from(&source.file.background);
    }
    file.mark_as_revision_of(&source.file.note, now_ms(), account);
    let revised = file.note.clone();
    let saved = host
        .write
        .create_revision(library, &entry.binder, file, revised)?;
    Ok(library.read(&saved.path)?)
}

/// The revision just read, with `strokes` in place of its strokes and `account` added to its
/// account, saved over its file with the stamp it was read at (`noteConflict` when it changed
/// meanwhile). The version before goes first to the library's trash as a note of its own,
/// titled "<title> (<before>)" (taken back when the save fails). `notARevision` for any other
/// note, before anything is done.
pub(super) fn edit_in_place(
    host: &Host,
    library: &Library,
    entry: &NoteEntry,
    revision: &LoadedNote,
    strokes: Vec<Stroke>,
    account: &str,
    before: &str,
) -> Result<(LoadedNote, TrashItem), ToolError> {
    if revision.file.revision.is_none() {
        return Err(not_a_revision());
    }
    let mut note = revision.file.note.clone();
    note.strokes = strokes;
    let kept = keep_previous(host, library, revision, before)?;
    if let Err(error) =
        host.write
            .save_note(library, &entry.path, note, &revision.stamp, Some(account))
    {
        let _ = library.purge(&kept.id);
        return Err(error);
    }
    Ok((library.read(&entry.path)?, kept))
}

pub(super) fn not_a_revision() -> ToolError {
    ToolError::new(
        code::NOT_A_REVISION,
        "this note is not a revision: the ink of an ordinary note is never changed. Make a copy with `create_revision` (or `straighten_shapes`), then edit the copy",
    )
}

/// What the result says of the version put in the trash before an edit.
pub(super) fn previous_version(kept: &TrashItem) -> serde_json::Value {
    let title = kept.title.clone().unwrap_or_default();
    let days = ardoise_core::library::TRASH_RETENTION.as_secs() / 86_400;
    json!({
        "in_trash": kept.id,
        "title": title,
        "id": kept.note_id,
        "restore": format!(
            "the version before is in the library's trash as the note \"{title}\": the user restores it from the trash view of the app (kept {days} days)"
        ),
    })
}

pub fn preview_revision(host: &Host, args: PreviewRevisionArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = read_note(&library, &entry)?;
    let revised = revise(&loaded, &args.orders)?;
    // Only a copy in memory is drawn: the file is not touched.
    let mut note = loaded.file.note.clone();
    note.strokes = revised.strokes.clone();
    let labels = args
        .labels
        .unwrap_or(false)
        .then(|| label_spots(&note, &revised.numbers));
    let drawn = draw_picture(PictureRequest {
        labels,
        note: &note,
        background_file: loaded.file.background.as_ref(),
        region: args.region,
        dpi: args.dpi,
        background: true,
    })?;
    let mut info = drawn.info;
    info["preview"] = true.into();
    info["written"] = false.into();
    info["note"] = serde_json::to_value(crate::notes::NoteSummary::of_entry(&library, &entry))
        .map_err(|e| ToolError::new("unexpected", e))?;
    info["version"] = version_of(&loaded.stamp).into();
    info["stroke_count"] = revised.strokes.len().into();
    info["report"] =
        serde_json::to_value(&revised.report).map_err(|e| ToolError::new("unexpected", e))?;
    content_result(vec![
        ContentBlock::image(BASE64.encode(&drawn.png), "image/png"),
        ContentBlock::text(info.to_string()),
    ])
}

pub fn create_revision(host: &Host, args: CreateRevisionArgs) -> ToolResult {
    let account = required_summary(&args.summary)?;
    some_orders(&args.orders)?;
    args.title.as_deref().map_or(Ok(()), check_title)?;
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let source = read_note(&library, &entry)?;
    check_version(&source, &args.expected_version)?;
    let revised = revise(&source, &args.orders)?;
    let stroke_count = revised.strokes.len();
    let copy = make_copy(
        host,
        &library,
        &entry,
        &source,
        revised.strokes,
        account,
        args.title.as_deref(),
    )?;
    json_result(json!({
        "created": true,
        "note": summary_of(&library, &copy),
        "version": version_of(&copy.stamp),
        "original": summary_at(&library, &entry.path)?,
        "original_unchanged": true,
        "stroke_count": stroke_count,
        "report": revised.report,
    }))
}

pub fn edit_revision(host: &Host, args: EditRevisionArgs) -> ToolResult {
    let account = required_summary(&args.summary)?;
    some_orders(&args.orders)?;
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = read_note(&library, &entry)?;
    if loaded.file.revision.is_none() {
        return Err(not_a_revision());
    }
    check_version(&loaded, &args.expected_version)?;
    let revised = revise(&loaded, &args.orders)?;
    let stroke_count = revised.strokes.len();
    let words = words(host.environment.language());
    let (saved, kept) = edit_in_place(
        host,
        &library,
        &entry,
        &loaded,
        revised.strokes,
        account,
        words.before_retouching,
    )?;
    json_result(json!({
        "saved": true,
        "note": summary_of(&library, &saved),
        "version": version_of(&saved.stamp),
        "stroke_count": stroke_count,
        "report": revised.report,
        "previous_version": previous_version(&kept),
        "account_entries": saved.file.revision.as_ref().map_or(0, |r| r.summary.len()),
    }))
}
