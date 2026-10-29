//! The read tools: `list_notes`, `list_binders`, `list_tags`, `search_notes`, `get_note`,
//! `get_note_text`, `render_note`. None of them changes anything (`readOnlyHint`), except that
//! `get_note_text` may fill the per-PC handwriting cache.
//!
//! Each tool is a thin method of [`ArdoiseServer`] (arguments in, `self.blocking` out) over a
//! plain function `fn(&Host, Args) -> ToolResult` that the tests also call.

use std::collections::HashMap;

use ardoise_core::library::NoteEntry;
use ardoise_core::note::Note;
use ardoise_hwr::Language;
use ardoise_hwr::structure::Item;
use ardoise_hwr::text::fold;
use ardoise_render::{Image, NoteView, Picture, RenderOptions, encode_png_at, render_note};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::error::{ToolError, ToolResult, content_result, json_result};
use crate::host::Host;
use crate::notes::{
    NoteSummary, PageMm, RectMm, binder_text, find_note, iso, mm, parse_date, read_note,
    relative_path,
};
use crate::server::ArdoiseServer;
use crate::strokes::{DEFAULT_MAX_POINTS, MAX_POINTS, StrokeInfo, polylines};

/// Notes per page of `list_notes`.
const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 500;
/// Results of `search_notes`.
const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 200;
/// Resolution of `render_note`, dots per inch, and the longest side of its image, pixels.
const DEFAULT_DPI: u32 = 100;
const MIN_DPI: u32 = 25;
const MAX_DPI: u32 = 300;
const MAX_RENDER_SIDE: u32 = 2_000;
const MM_PER_INCH: f64 = 25.4;

// ---------------------------------------------------------------------------------------------
// Arguments

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    /// Most recently created first (the default).
    Newest,
    Oldest,
    /// Alphabetical, accents and case ignored.
    Title,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListNotesArgs {
    /// Only the notes of this binder and its sub-binders (path relative to the library, `/`-separated; empty for all).
    pub binder: Option<String>,
    /// Only the notes with this tag (case and accents ignored).
    pub tag: Option<String>,
    /// `true`: only favourites; `false`: only the others.
    pub favorite: Option<bool>,
    /// Only notes created on or after this ISO date (`2026-10-12`) or date and time, in this PC's time zone.
    pub from: Option<String>,
    /// Only notes created before the end of this ISO date (`2026-10-12` includes the whole day) or before this date and time.
    pub to: Option<String>,
    /// Order of the list (default `newest`, by creation date).
    pub sort: Option<SortOrder>,
    /// How many notes to return (default 50, at most 500). Reading many notes takes time.
    #[schemars(range(min = 1, max = 500))]
    pub limit: Option<usize>,
    /// How many notes to skip (to read the next page).
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchNotesArgs {
    /// Words to find: a note matches when every word is in its title, tags or binder, or begins a word of its recognised handwriting. Accents and case are ignored.
    pub query: String,
    /// How many notes to return (default 20, at most 200).
    #[schemars(range(min = 1, max = 200))]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StrokeDetail {
    /// No strokes (the default).
    None,
    /// Per stroke: tool, colour, width, bounding box, start and end time.
    Summary,
    /// The summary and each stroke's polyline in millimetres, simplified.
    Points,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetNoteArgs {
    /// The note's `id`, or its path relative to the library as `list_notes` gives it.
    pub note: String,
    /// How much of the ink to return (default `none`).
    pub strokes: Option<StrokeDetail>,
    /// With `points`: most points over the whole note (default 5000, at most 50000); strokes are simplified (Douglas-Peucker) to fit.
    #[schemars(range(min = 2, max = 50000))]
    pub max_points: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextLanguage {
    Fr,
    En,
}

impl From<TextLanguage> for Language {
    fn from(language: TextLanguage) -> Language {
        match language {
            TextLanguage::Fr => Language::Fr,
            TextLanguage::En => Language::En,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetNoteTextArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// Recognition language (default: the one in Ardoise's settings). Needs the Windows handwriting pack of that language.
    pub language: Option<TextLanguage>,
}

/// A rectangle of the page in millimetres from its top-left corner.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A layer, by position (from 0) or by name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LayerRef {
    Index(u32),
    Name(String),
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RenderNoteArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// Part of the page to render (default: the whole page). Use it to look closer at a diagram.
    pub region: Option<Region>,
    /// Dots per inch (default 100, at most 300). Lowered so that the image's longest side stays at most 2000 pixels; the dpi used is returned.
    #[schemars(range(min = 25, max = 300))]
    pub dpi: Option<u32>,
    /// Draw the paper, template and background image (default `true`); otherwise a plain white page.
    pub background: Option<bool>,
    /// Layers to draw, by index or name, even if hidden in the note (default: the layers visible in the note).
    pub layers: Option<Vec<LayerRef>>,
}

// ---------------------------------------------------------------------------------------------
// Tools

#[tool_router(router = read_router, vis = "pub(crate)")]
impl ArdoiseServer {
    /// List the notes of the library, newest first, with their id, path, binder, title, dates, tags, page size and stroke count. Filter by binder, tag, favourite or creation date; page with `limit` and `offset`.
    #[tool(
        name = "list_notes",
        annotations(
            title = "List notes",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_notes(
        &self,
        Parameters(args): Parameters<ListNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| list_notes(host, args)).await
    }

    /// List the binders (folders) of the library with the number of notes in each, sub-binders included.
    #[tool(
        name = "list_binders",
        annotations(
            title = "List binders",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_binders(&self) -> Result<CallToolResult, ErrorData> {
        self.blocking(list_binders).await
    }

    /// List the tags used in the library with the number of notes that carry each.
    #[tool(
        name = "list_tags",
        annotations(
            title = "List tags",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_tags(&self) -> Result<CallToolResult, ErrorData> {
        self.blocking(list_tags).await
    }

    /// Search the notes like the app does: titles, tags, binders and the recognised handwriting, accents and case ignored. Each result says where it matched and shows a snippet of the text. Notes whose handwriting has not been read yet are not searched by text; the result counts them (`get_note_text` reads one).
    #[tool(
        name = "search_notes",
        annotations(
            title = "Search notes",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn search_notes(
        &self,
        Parameters(args): Parameters<SearchNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| search_notes(host, args)).await
    }

    /// Read a note's data: metadata, page, layers, paper and template, background image, duration and, on request, its strokes (a summary of each, or simplified polylines in millimetres). Use `render_note` to see it and `get_note_text` to read its handwriting.
    #[tool(
        name = "get_note",
        annotations(
            title = "Get a note",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn get_note(
        &self,
        Parameters(args): Parameters<GetNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| get_note(host, args)).await
    }

    /// The handwriting of a note as text: the recognised lines with their bounding boxes in millimetres, and its structure (paragraphs, list items, drawings with their bounds and stroke indices) when it can be found. Reads the note now if it was not read yet, which can take a few seconds. The recognition makes mistakes, especially on formulas and diagrams: compare with `render_note`. If a corrected transcription was stored with `set_note_text` for the ink as it is now, `text` is that one, `is_correction` is true and `recognised_text` is the recogniser's.
    #[tool(
        name = "get_note_text",
        annotations(
            title = "Get a note's text",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn get_note_text(
        &self,
        Parameters(args): Parameters<GetNoteTextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| get_note_text(host, args)).await
    }

    /// Render a note (or a region of its page) as a PNG image so that you can see it: handwriting, drawings, diagrams, layout, paper and background image. The image's longest side is at most 2000 pixels; the dpi actually used is returned in the text next to it.
    #[tool(
        name = "render_note",
        annotations(
            title = "Render a note as an image",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn render_note(
        &self,
        Parameters(args): Parameters<RenderNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| render_note_tool(host, args))
            .await
    }
}

// ---------------------------------------------------------------------------------------------
// list_notes, list_binders, list_tags

/// Whether `binder` is `ancestor` or lies inside it (names compared without regard to case).
pub(crate) fn in_binder(binder: &str, ancestor: &str) -> bool {
    let (binder, ancestor) = (binder.to_lowercase(), ancestor.to_lowercase());
    ancestor.is_empty() || binder == ancestor || binder.starts_with(&format!("{ancestor}/"))
}

/// A binder given by the assistant: `/`-separated, without the slashes around it.
pub(crate) fn clean_binder(binder: &str) -> String {
    binder
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn list_notes(host: &Host, args: ListNotesArgs) -> ToolResult {
    let from = args
        .from
        .as_deref()
        .map(|d| parse_date(d, false))
        .transpose()?;
    let to = args
        .to
        .as_deref()
        .map(|d| parse_date(d, true))
        .transpose()?;
    let limit = args
        .limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let offset = args.offset.unwrap_or(0);
    let binder = args.binder.as_deref().map(clean_binder);
    let tag = args.tag.as_deref().map(fold);

    let library = host.library()?;
    let listing = library.list();
    let mut notes: Vec<&NoteEntry> = listing
        .notes
        .iter()
        .filter(|note| {
            binder
                .as_deref()
                .is_none_or(|b| in_binder(&binder_text(&note.binder), b))
                && tag
                    .as_deref()
                    .is_none_or(|t| note.tags.iter().any(|n| fold(n) == t))
                && args.favorite.is_none_or(|f| note.favorite == f)
                && from.is_none_or(|from| note.created >= from)
                && to.is_none_or(|to| note.created < to)
        })
        .collect();
    match args.sort.unwrap_or(SortOrder::Newest) {
        SortOrder::Newest => {
            notes.sort_by(|a, b| b.created.cmp(&a.created).then(a.path.cmp(&b.path)))
        }
        SortOrder::Oldest => {
            notes.sort_by(|a, b| a.created.cmp(&b.created).then(a.path.cmp(&b.path)))
        }
        SortOrder::Title => notes.sort_by(|a, b| {
            fold(&a.title)
                .cmp(&fold(&b.title))
                .then(a.path.cmp(&b.path))
        }),
    }
    let total = notes.len();
    let page: Vec<NoteSummary> = notes
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|entry| NoteSummary::read(&library, entry))
        .collect();
    let problems: Vec<_> = listing
        .problems
        .iter()
        .map(
            |p| json!({ "path": relative_path(&library, &p.path), "message": p.error.to_string() }),
        )
        .collect();
    let mut result = json!({
        "total": total,
        "offset": offset,
        "limit": limit,
        "notes": page,
    });
    if !problems.is_empty() {
        result["unreadable_files"] = problems.into();
    }
    json_result(result)
}

/// Whether the binder `name` is left out of the list: `hidden` (the automatic export folder)
/// and what is inside it hold exports, unless a note is in it.
fn is_hidden(name: &str, hidden: Option<&str>, notes: &[NoteEntry]) -> bool {
    let Some(hidden) = hidden else { return false };
    let hidden = hidden.to_lowercase();
    let name = name.to_lowercase();
    let within =
        |binder: &str, parent: &str| binder == parent || binder.starts_with(&format!("{parent}/"));
    within(&name, &hidden)
        && !notes
            .iter()
            .any(|note| within(&binder_text(&note.binder).to_lowercase(), &name))
}

pub fn list_binders(host: &Host) -> ToolResult {
    let library = host.library()?;
    let listing = library.list();
    let hidden = host.environment.hidden_binder();
    let mut binders: Vec<String> = library
        .binders()
        .iter()
        .map(|binder| binder_text(binder))
        .filter(|name| !name.is_empty() && !is_hidden(name, hidden.as_deref(), &listing.notes))
        .collect();
    binders.sort();
    let counts: Vec<_> = binders
        .iter()
        .map(|name| {
            let own = listing
                .notes
                .iter()
                .filter(|n| binder_text(&n.binder).eq_ignore_ascii_case(name))
                .count();
            let inside = listing
                .notes
                .iter()
                .filter(|n| in_binder(&binder_text(&n.binder), name))
                .count();
            json!({ "path": name, "note_count": inside, "own_note_count": own })
        })
        .collect();
    let at_root = listing
        .notes
        .iter()
        .filter(|n| n.binder.as_os_str().is_empty())
        .count();
    json_result(
        json!({ "binders": counts, "notes_at_root": at_root, "total_notes": listing.notes.len() }),
    )
}

pub fn list_tags(host: &Host) -> ToolResult {
    let library = host.library()?;
    let tags = library.list().tags();
    json_result(json!({ "tags": tags }))
}

// ---------------------------------------------------------------------------------------------
// search_notes

/// The query as the interface splits it: folded, at white space, without repeats.
fn query_words(query: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in fold(query).split_whitespace() {
        if !words.iter().any(|w| w == word) {
            words.push(word.to_owned());
        }
    }
    words
}

pub fn search_notes(host: &Host, args: SearchNotesArgs) -> ToolResult {
    let words = query_words(&args.query);
    if words.is_empty() {
        return Err(ToolError::invalid("`query` has no word to look for"));
    }
    let limit = args
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT);
    let library = host.library()?;
    let listing = library.list();
    let text = host.handwriting.search_in(&args.query, &listing.notes);
    let hits: HashMap<_, _> = text
        .hits
        .iter()
        .map(|hit| (hit.path.clone(), hit))
        .collect();

    let mut found: Vec<(&NoteEntry, serde_json::Value)> = Vec::new();
    for note in &listing.notes {
        let title = fold(&note.title);
        let tags = fold(&note.tags.join("\n"));
        let binder = fold(&binder_text(&note.binder));
        let hit = hits.get(&note.path);
        let mut places: Vec<&str> = Vec::new();
        let mut all = true;
        for word in &words {
            let mut seen = false;
            for (place, haystack) in [("title", &title), ("tags", &tags), ("binder", &binder)] {
                if haystack.contains(word.as_str()) {
                    seen = true;
                    if !places.contains(&place) {
                        places.push(place);
                    }
                }
            }
            if hit.is_some_and(|h| h.words.contains(word)) {
                seen = true;
                if !places.contains(&"text") {
                    places.push("text");
                }
            }
            all &= seen;
        }
        if !all {
            continue;
        }
        let mut entry = serde_json::to_value(NoteSummary::of_entry(&library, note))
            .map_err(|e| ToolError::new("unexpected", e))?;
        entry["matched_in"] = places.into();
        if let Some(hit) = hit.filter(|h| !h.word.is_empty()) {
            entry["snippet"] =
                json!({ "before": hit.before, "word": hit.word, "after": hit.after });
        }
        found.push((note, entry));
    }
    found.sort_by(|a, b| b.0.created.cmp(&a.0.created).then(a.0.path.cmp(&b.0.path)));
    let total = found.len();
    let notes: Vec<_> = found
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect();

    let status = &text.status;
    let mut text_search = json!({ "available": status.available, "unread_notes": status.unread });
    if let Some(reason) = &status.reason {
        text_search["reason"] = reason.clone().into();
    }
    if status.available && status.unread > 0 {
        text_search["hint"] = format!(
            "{} note(s) have not been read by the handwriting recognition yet, so their text was not searched. get_note_text reads one now (it is then found by the next search); the others are read in the background while this server runs.",
            status.unread
        )
        .into();
    }
    json_result(json!({
        "query": args.query,
        "total": total,
        "notes": notes,
        "text_search": text_search,
    }))
}

// ---------------------------------------------------------------------------------------------
// get_note

pub fn get_note(host: &Host, args: GetNoteArgs) -> ToolResult {
    let detail = args.strokes.unwrap_or(StrokeDetail::None);
    let budget = args
        .max_points
        .unwrap_or(DEFAULT_MAX_POINTS)
        .clamp(2, MAX_POINTS);
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = read_note(&library, &entry)?;
    let file = &loaded.file;
    let note = &file.note;

    let mut summary = NoteSummary::of_entry(&library, &entry);
    summary.add_note(note);
    let layers: Vec<_> = note
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            json!({
                "index": index,
                "name": layer.name,
                "visible": layer.visible,
                "stroke_count": note.strokes.iter().filter(|s| s.layer as usize == index).count(),
            })
        })
        .collect();
    let image = note.background_image.as_ref().map(|image| {
        json!({
            "name": image.name,
            "pdf_page": image.pdf_page,
            "x_mm": mm(image.rect.x),
            "y_mm": mm(image.rect.y),
            "width_mm": mm(image.rect.width),
            "height_mm": mm(image.rect.height),
            "quarter_turns": image.quarter_turns % 4,
            "opacity": image.opacity,
            "has_pixels": file.background.is_some(),
        })
    });
    let duration_ms = note
        .strokes
        .iter()
        .filter_map(|s| s.points.last().map(|p| p.t))
        .max()
        .unwrap_or(0);
    let mut result = json!({
        "note": summary,
        "origin": serde_json::to_value(&note.origin).unwrap_or_default(),
        "timing": serde_json::to_value(note.timing).unwrap_or_default(),
        "layers": layers,
        "background": {
            "paper": serde_json::to_value(note.background.paper).unwrap_or_default(),
            "template": serde_json::to_value(note.background.template).unwrap_or_default(),
            "texture": note.background.texture,
            "image": image,
        },
        "duration_ms": duration_ms,
        "has_narration": file.audio.is_some(),
    });
    if detail != StrokeDetail::None {
        let mut infos: Vec<StrokeInfo> = note
            .strokes
            .iter()
            .enumerate()
            .map(|(index, stroke)| StrokeInfo::of(index, stroke))
            .collect();
        if detail == StrokeDetail::Points {
            let lines = polylines(&note.strokes, budget);
            let left_out = lines.iter().filter(|l| l.is_none()).count();
            for (info, line) in infos.iter_mut().zip(lines) {
                info.points_mm = line;
            }
            if left_out > 0 {
                result["strokes_without_points"] = left_out.into();
                result["points_note"] = format!(
                    "the budget of {budget} points ran out: the last {left_out} stroke(s) have no points; raise max_points or use render_note"
                )
                .into();
            }
        }
        result["strokes"] =
            serde_json::to_value(infos).map_err(|e| ToolError::new("unexpected", e))?;
    }
    json_result(result)
}

// ---------------------------------------------------------------------------------------------
// get_note_text

pub fn get_note_text(host: &Host, args: GetNoteTextArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = read_note(&library, &entry)?;
    let note = &loaded.file.note;
    // Kept with the note's path and version: the next search finds its text.
    let recognition = host.handwriting.recognition_at(
        &loaded.path,
        &loaded.stamp,
        note,
        args.language.map(Language::from),
    )?;
    let structure = host.handwriting.structure_of(note, &recognition);
    let lines: Vec<_> = recognition
        .lines
        .iter()
        .map(|line| json!({ "text": line.text(), "bounds_mm": RectMm::from(line.bounds) }))
        .collect();
    let items: Vec<_> = structure
        .items
        .iter()
        .map(|item| match item {
            Item::Paragraph(text) => json!({ "kind": "paragraph", "text": text }),
            Item::ListItem(text) => json!({ "kind": "list_item", "text": text }),
            Item::Drawing(area) => json!({
                "kind": "drawing",
                "bounds_mm": RectMm::from(area.bounds),
                "strokes": area.strokes,
            }),
        })
        .collect();
    // A transcription corrected with `set_note_text` replaces the recogniser's text (the lines
    // and the structure, with their positions, stay the recogniser's).
    let correction = host.handwriting.correction_of(note);
    let recognised = recognition.text();
    let mut result = json!({
        "note": NoteSummary::of_entry(&library, &entry),
        "language": recognition.language.code(),
        "recognizer": recognition.recognizer,
        "text": correction.as_ref().map_or(&recognised, |c| &c.text),
        "is_correction": correction.is_some(),
        "lines": lines,
        "structure": {
            "source": if structure.analyzed { "ink_analysis" } else { "layout_heuristics" },
            "items": items,
        },
    });
    if correction.is_some() {
        result["recognised_text"] = recognised.into();
    }
    json_result(result)
}

// ---------------------------------------------------------------------------------------------
// render_note

/// The layers to draw: `layers` of `note` made visible (the others hidden), or the note's own
/// visibility when `wanted` is `None`. Returns the names drawn.
fn choose_layers(note: &mut Note, wanted: Option<&[LayerRef]>) -> Result<(), ToolError> {
    let Some(wanted) = wanted else { return Ok(()) };
    let mut visible = vec![false; note.layers.len()];
    for layer in wanted {
        let position = match layer {
            LayerRef::Index(index) => {
                ((*index as usize) < note.layers.len()).then_some(*index as usize)
            }
            LayerRef::Name(name) => {
                note.layers
                    .iter()
                    .position(|l| l.name == *name)
                    .or_else(|| {
                        note.layers
                            .iter()
                            .position(|l| l.name.eq_ignore_ascii_case(name))
                    })
            }
        };
        let position = position.ok_or_else(|| {
            let names: Vec<&str> = note.layers.iter().map(|l| l.name.as_str()).collect();
            ToolError::invalid(format!(
                "no layer {layer:?}: this note has {} layer(s), numbered from 0: {names:?}",
                note.layers.len()
            ))
        })?;
        visible[position] = true;
    }
    for (layer, visible) in note.layers.iter_mut().zip(visible) {
        layer.visible = visible;
    }
    Ok(())
}

/// The part of `image` that shows `region` of a page `page` millimetres wide and high.
fn crop(image: &Image, page: (f64, f64), region: Region) -> Image {
    let to_x = |mm: f64| {
        ((mm / page.0) * f64::from(image.width))
            .round()
            .clamp(0.0, f64::from(image.width)) as u32
    };
    let to_y = |mm: f64| {
        ((mm / page.1) * f64::from(image.height))
            .round()
            .clamp(0.0, f64::from(image.height)) as u32
    };
    let (x0, y0) = (to_x(region.x), to_y(region.y));
    let x1 = to_x(region.x + region.width).max(x0 + 1).min(image.width);
    let y1 = to_y(region.y + region.height).max(y0 + 1).min(image.height);
    let (x0, y0) = (x0.min(x1 - 1), y0.min(y1 - 1));
    let width = (x1 - x0).min(MAX_RENDER_SIDE);
    let height = (y1 - y0).min(MAX_RENDER_SIDE);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in y0..y0 + height {
        let start = (y as usize * image.width as usize + x0 as usize) * 4;
        pixels.extend_from_slice(&image.pixels[start..start + width as usize * 4]);
    }
    Image {
        width,
        height,
        pixels,
    }
}

pub fn render_note_tool(host: &Host, args: RenderNoteArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = read_note(&library, &entry)?;
    let mut note = loaded.file.note;
    let background = args.background.unwrap_or(true);
    choose_layers(&mut note, args.layers.as_deref())?;

    let page_mm = PageMm::from(note.page);
    let page = (
        f64::from(note.page.width) * 0.01,
        f64::from(note.page.height) * 0.01,
    );
    let region = match args.region {
        None => Region {
            x: 0.0,
            y: 0.0,
            width: page.0,
            height: page.1,
        },
        Some(r) => {
            if ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
                || r.width <= 0.0
                || r.height <= 0.0
            {
                return Err(ToolError::invalid(
                    "`region` needs a positive width and height in millimetres",
                ));
            }
            // What lies outside the page is not drawn: keep the part inside.
            let (x0, y0) = (r.x.max(0.0), r.y.max(0.0));
            let (x1, y1) = ((r.x + r.width).min(page.0), (r.y + r.height).min(page.1));
            if x1 <= x0 || y1 <= y0 {
                return Err(ToolError::invalid(format!(
                    "`region` is outside the page, which is {} x {} mm",
                    page_mm.width_mm, page_mm.height_mm
                )));
            }
            Region {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            }
        }
    };

    let requested = args.dpi.unwrap_or(DEFAULT_DPI).clamp(MIN_DPI, MAX_DPI);
    let longest_mm = region.width.max(region.height);
    let wanted_side = longest_mm / MM_PER_INCH * f64::from(requested);
    let dpi = if wanted_side > f64::from(MAX_RENDER_SIDE) {
        f64::from(MAX_RENDER_SIDE) * MM_PER_INCH / longest_mm
    } else {
        f64::from(requested)
    };

    let picture = if background && note.background_image.is_some() {
        loaded
            .file
            .background
            .as_ref()
            .and_then(|file| Picture::decode(file.bytes.clone()).ok())
    } else {
        None
    };
    let options = RenderOptions {
        scale: (dpi / 2540.0) as f32,
        background,
    };
    let page_image = render_note(NoteView::new(&note, picture.as_ref()), &options)?;
    let image = if args.region.is_some() {
        crop(&page_image, page, region)
    } else {
        page_image
    };
    let png = encode_png_at(&image, Some(dpi.round() as u32))?;

    let drawn: Vec<&str> = note
        .layers
        .iter()
        .filter(|l| l.visible)
        .map(|l| l.name.as_str())
        .collect();
    let info = json!({
        "note": NoteSummary::of_entry(&library, &entry),
        "width_px": image.width,
        "height_px": image.height,
        "dpi": (dpi * 10.0).round() / 10.0,
        "requested_dpi": requested,
        "dpi_lowered": dpi < f64::from(requested),
        "region_mm": {
            "x": (region.x * 10.0).round() / 10.0,
            "y": (region.y * 10.0).round() / 10.0,
            "width": (region.width * 10.0).round() / 10.0,
            "height": (region.height * 10.0).round() / 10.0,
        },
        "page": page_mm,
        "background": background,
        "layers_drawn": drawn,
        "created": iso(note.created),
    });
    content_result(vec![
        ContentBlock::image(BASE64.encode(&png), "image/png"),
        ContentBlock::text(info.to_string()),
    ])
}
