//! The write tools (`create_note`, `update_note`, `move_note`, `trash_note`, `merge_notes`,
//! `split_note`, `straighten_shapes`, `export_note`, `set_note_text`, the three of `revision.rs`:
//! `preview_revision`, `create_revision`, `edit_revision`, and `revision_orders`, which describes
//! their orders): SPEC §4.9, design in `docs/design/mcp.md` and `docs/design/revision.md`. Absent
//! with `--read-only`.
//!
//! Every tool gives exactly what the same action gives in the app: the library operations are
//! `ardoise-core`'s (`Library::save` with its stamp check, the trash, the " (2)" rule of moves
//! and creations), and what lives in the app's executable (thumbnails, the note's original,
//! background images, the exports, the handwriting cache) comes through [`WriteServices`].
//! No file is ever overwritten by an export and nothing is deleted for good: a trashed note, and
//! the version of a revision before it is edited, are in the library's trash. The ink of a note
//! that is not a revision is never written: whatever changes it (the orders of `create_revision`,
//! `straighten_shapes`, `split_note`) makes new notes next to it.
//!
//! To add a tool:
//!
//! 1. put the app-side operation it needs in [`WriteServices`] (a method with the default body
//!    [`unavailable`], so that a host without the service fails with a coded error), and
//!    implement it in `app/src-tauri/src/mcp/write.rs`, passing it with
//!    [`Host::with_write`](crate::host::Host::with_write);
//! 2. write the arguments and the function `fn(&Host, Args) -> ToolResult` in the module of its
//!    kind (`create`, `update`, `transform`, `export`, `text`), and the tool below in the
//!    `impl ArdoiseServer` block marked `#[tool_router(router = write_tools, vis = "pub(crate)")]`,
//!    with `annotations(read_only_hint = false, destructive_hint = ..., ...)`: `true` when the
//!    tool changes or removes what is there (the MCP specification keeps `false` for tools that
//!    only add), even if the app can restore it;
//! 3. nothing else: [`crate::server::build_tool_router`] adds `write_router()` unless the server
//!    runs read-only.

mod create;
mod export;
mod orders;
mod revision;
mod text;
mod transform;
mod update;

use std::path::{Path, PathBuf};

use ardoise_core::format::NoteFile;
use ardoise_core::library::{FileStamp, Library, SavedNote};
use ardoise_core::note::Note;
use ardoise_hwr::Language;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};

use crate::error::{ToolError, code};
use crate::server::ArdoiseServer;

pub use create::{CreateNoteArgs, Orientation, PaperArg, PaperName, TemplateName};
pub use export::{
    ExportFormat, ExportKind, MARKDOWN_IMAGES_SUFFIX, PdfPage, VideoOptions, VideoResolution,
    VideoTiming, check_replay_plan, free_file,
};
pub use export::{ExportNoteArgs, ExportOptions, ExportRequest, ExportTarget};
pub use revision::{CreateRevisionArgs, EditRevisionArgs, PreviewRevisionArgs};
pub use text::SetNoteTextArgs;
pub use transform::{MergeLayout, MergeNotesArgs, SplitNoteArgs, StraightenArgs};
pub use update::{MoveNoteArgs, TrashNoteArgs, UpdateNoteArgs};

/// The error of a service the host does not have.
pub fn unavailable(service: &str) -> ToolError {
    ToolError::new(
        code::WRITE_UNAVAILABLE,
        format!("this server has no {service} service"),
    )
}

/// What the files written by an export were, for the result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExportOutcome {
    /// Every file written (a Markdown export also lists its drawings).
    pub files: Vec<PathBuf>,
    /// Facts the format has to report (frames, duration, encoder of a video; whether the layout
    /// was analysed or guessed for a text export), as a JSON object.
    pub details: serde_json::Value,
}

/// What the write tools use from the app. The library operations of `ardoise-core` need no
/// service; these live in the app's executable (thumbnails, the original of a note,
/// background images and PDF pages, narration, exports, the handwriting cache), and every
/// method is the app's own code. A method that is not given fails with `writeUnavailable`.
pub trait WriteServices: Send + Sync {
    /// Creates the blank `note` in `binder` (relative to the library), with its thumbnail;
    /// the file name takes " (2)"… when taken.
    fn create_blank(
        &self,
        _library: &Library,
        _binder: &Path,
        _note: Note,
    ) -> Result<SavedNote, ToolError> {
        Err(unavailable("note creation"))
    }

    /// Creates a note in `binder` from the image or the PDF `file` (page `pdf_page`, from 1,
    /// for a PDF), as the app does for a file it imports: the page takes the size of the file
    /// and shows it as background image. `layer_name` names the first layer.
    fn create_from_file(
        &self,
        _library: &Library,
        _binder: &Path,
        _file: &Path,
        _pdf_page: u32,
        _layer_name: &str,
    ) -> Result<SavedNote, ToolError> {
        Err(unavailable("background import"))
    }

    /// Saves `note` over the file at `path` as the editor does: refused with `noteConflict`
    /// when the file is no longer the version of `stamp`; every other member of the file is
    /// kept, `modified` is now, the thumbnail is made again and the original of the tablet's
    /// note is kept the first time the ink changes. With `account`, the file (a revision) gets
    /// that line, dated now, at the end of its account, in the same write.
    fn save_note(
        &self,
        _library: &Library,
        _path: &Path,
        _note: Note,
        _stamp: &FileStamp,
        _account: Option<&str>,
    ) -> Result<FileStamp, ToolError> {
        Err(unavailable("note saving"))
    }

    /// Creates the new note `file` (a copy of another note, or a part of one, marked as a revision
    /// of it, with a new id and no thumbnail) in `binder`, with `revised` as its ink in place of
    /// the ink `file` carries (the source's): as the editor's save would write `revised` over
    /// `file`, the note's original is kept by the editor's rule (a tablet note keeps the tablet's
    /// bytes, any other note gets the version before its first edit, here the source's ink; none when
    /// `revised` is the ink `file` carries) and the thumbnail is drawn. The file name takes " (2)"…
    /// when taken. Nothing but the new file is written.
    fn create_revision(
        &self,
        _library: &Library,
        _binder: &Path,
        _file: NoteFile,
        _revised: Note,
    ) -> Result<SavedNote, ToolError> {
        Err(unavailable("revision"))
    }

    /// Merges the notes at `paths`, in this order, into a new note titled `title` in the
    /// binder of the first one, as the library does. Returns its path.
    fn merge_notes(
        &self,
        _library: &Library,
        _paths: &[PathBuf],
        _layout: MergeLayout,
        _title: &str,
    ) -> Result<PathBuf, ToolError> {
        Err(unavailable("merge"))
    }

    /// Exports the notes of the request as the app does. A [`ExportTarget::File`] is free (the
    /// tool checked); inside a [`ExportTarget::Folder`] each file is named after its note, with
    /// " (2)"… added when taken. Never overwrites.
    fn export(
        &self,
        _library: &Library,
        _request: &ExportRequest,
    ) -> Result<ExportOutcome, ToolError> {
        Err(unavailable("export"))
    }

    /// Stores `text` as the transcription of `note` in the handwriting cache of this PC,
    /// bound to the note's current ink; an empty `text` removes it. Never written into the
    /// note file.
    fn set_note_text(
        &self,
        _note: &Note,
        _text: &str,
        _language: Language,
    ) -> Result<(), ToolError> {
        Err(unavailable("handwriting cache"))
    }
}

/// The services of a host that has none: every write that needs the app fails with
/// `writeUnavailable`.
pub struct NoWriteServices;

impl WriteServices for NoWriteServices {}

#[tool_router(router = write_tools, vis = "pub(crate)")]
impl ArdoiseServer {
    /// Create a note: blank (page size, orientation and template of your choice) or on an image or one page of a PDF given by its absolute path. It lands in `binder` (created if missing; default: the root of the library). Returns the note's summary; use its `id` or `path` with the other tools.
    #[tool(
        name = "create_note",
        annotations(
            title = "Create a note",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn create_note(
        &self,
        Parameters(args): Parameters<CreateNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| create::create_note(host, args))
            .await
    }

    /// Change a note's metadata: its title (the file is renamed to match), its tags (replace them, or add or remove some) and its favourite mark. The ink is not touched; the old title and tags are not kept.
    #[tool(
        name = "update_note",
        annotations(
            title = "Update a note's title, tags or favourite mark",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn update_note(
        &self,
        Parameters(args): Parameters<UpdateNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| update::update_note(host, args))
            .await
    }

    /// Move a note to another binder (created if missing; empty for the root of the library). A note of the same name there is never overwritten: " (2)" is added to the moved file's name.
    #[tool(
        name = "move_note",
        annotations(
            title = "Move a note to a binder",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn move_note(
        &self,
        Parameters(args): Parameters<MoveNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| update::move_note(host, args))
            .await
    }

    /// Put a note in the library's trash. Nothing is deleted for good: the user restores it from the trash view of the app (notes stay there 30 days).
    #[tool(
        name = "trash_note",
        annotations(
            title = "Move a note to the trash",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn trash_note(
        &self,
        Parameters(args): Parameters<TrashNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| update::trash_note(host, args))
            .await
    }

    /// Merge two or more notes into a new note, in the given order: `overlay` puts their ink on one page, `endToEnd` lays the pages one after the other. The source notes are not changed. The new note is in the binder of the first one.
    #[tool(
        name = "merge_notes",
        annotations(
            title = "Merge notes",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn merge_notes(
        &self,
        Parameters(args): Parameters<MergeNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| transform::merge_notes(host, args))
            .await
    }

    /// Split a note in two. Give `at_ms` (what is written from that instant on is the second part; milliseconds on the clock of the strokes' `start_ms` and `end_ms` in `get_note`) or `polygon` (the strokes mostly inside this polygon of the page, in millimetres, are the second part). The ink of a note that is not a revision is never changed: its file is not written, and both parts are new notes next to it, each a revision of it (titled "<title> (start)" or "(rest)" for the first, "(continued)" or "(excerpt)" for the second, or `title`); the result names both, `kept` and `created`, and says `original_unchanged`. On a note that already is a revision the first part is saved over it, its version before goes to the library's trash as a note of its own that the user can restore, and the second is a new note, a revision of it.
    #[tool(
        name = "split_note",
        annotations(
            title = "Split a note into new notes",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn split_note(
        &self,
        Parameters(args): Parameters<SplitNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| transform::split_note(host, args))
            .await
    }

    /// Straighten hand-drawn shapes (lines, circles and ellipses, rectangles, arrows), as the editor's "Straighten shapes" does on a selection. The ink of a note that is not a revision is never changed: the shapes are straightened on a copy made next to it (a revision, titled "<title> (clean)"), and the result gives both notes. On a note that already is a revision they are straightened in it, in one save, and its version before goes to the library's trash as a note of its own that the user can restore. By default only the strokes of the drawings found by Windows' ink analysis on the visible layers are tried (without the analysis, `strokes` is required); pass `strokes` (indices from `get_note`, or the `strokes` of the drawings in `get_note_text`'s structure) to choose them.
    #[tool(
        name = "straighten_shapes",
        annotations(
            title = "Straighten drawn shapes",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn straighten_shapes(
        &self,
        Parameters(args): Parameters<StraightenArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| transform::straighten_shapes(host, args))
            .await
    }

    /// Export notes with the app's own exports: png, jpg, webp, svg, pdf, txt (recognised text), md and docx (text with its structure and the drawings), gif, webm and mp4 (replay of the writing, one note; takes seconds to minutes). `path` is an absolute folder, or an absolute file name ending with the format's extension. An existing file is never overwritten: " (2)" is added. Returns the paths written.
    #[tool(
        name = "export_note",
        annotations(
            title = "Export notes to files",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn export_note(
        &self,
        Parameters(args): Parameters<ExportNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| export::export_note(host, args))
            .await
    }

    /// Store your corrected transcription of a note's handwriting. The app's search (and `search_notes`) and its text exports (`.txt`, clipboard, Markdown, Word, and `export_note`) then use it instead of the recogniser's text, and `get_note_text` returns it flagged `is_correction`, until the note's ink changes. It is kept in this PC's handwriting cache and never written into the note. An empty `text` removes the correction.
    #[tool(
        name = "set_note_text",
        annotations(
            title = "Store a corrected transcription",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn set_note_text(
        &self,
        Parameters(args): Parameters<SetNoteTextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| text::set_note_text(host, args))
            .await
    }

    /// Every order of `preview_revision`, `create_revision` and `edit_revision`, with what it does and its fields (their types, which are required, what they mean). Call it once before writing orders: the schemas of those tools only name them. It reads nothing of the library.
    #[tool(
        name = "revision_orders",
        annotations(
            title = "The orders that retouch a note",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn revision_orders(&self) -> Result<CallToolResult, ErrorData> {
        self.blocking(|_| orders::revision_orders()).await
    }

    /// Try a list of orders that retouch a note's strokes, in memory: nothing is written. `revision_orders` gives every order and its fields: call it once first. Answers the picture of the result, the note's `version` and the report (what each order selected and made, the lines tidied with their slope, the time moved, warnings). Look at the picture, read the report, correct the orders, and preview again; every time send the whole list, built from the note as it is. Use `labels` and `region` to see the numbers. Then `create_revision` with that `version`.
    #[tool(
        name = "preview_revision",
        annotations(
            title = "Preview a retouching of a note",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn preview_revision(
        &self,
        Parameters(args): Parameters<PreviewRevisionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| revision::preview_revision(host, args))
            .await
    }

    /// Carry out a list of orders on a copy of a note: a new note next to it, a revision, with the retouched strokes and `summary` as the first line of its account, which the editor shows ("Copie retouchée de ...", "What changed", compare, open the original). The original is never written, whatever the orders. Everything else of the note is kept (paper, layers, background image, tags, narration), `favorite` is off, the creation date is the original's. Preview the orders first with `preview_revision`, and give the `version` it answers as `expected_version`: a note changed since is refused with `noteConflict`. Letters of a word that has to be corrected only come from the same handwriting in this same note: never from another note, whose points are not to be retyped either.
    #[tool(
        name = "create_revision",
        annotations(
            title = "Retouch a note on a copy",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn create_revision(
        &self,
        Parameters(args): Parameters<CreateRevisionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| revision::create_revision(host, args))
            .await
    }

    /// Carry out a list of orders on a note that already is a revision (a copy made by `create_revision` or `straighten_shapes`), in place: the strokes it has now, with what the user changed by hand, are the ones the orders number (`get_note` with `strokes`, `render_note` with `labels`). A note that is not a revision is refused with `notARevision`, and left as it is. The version before goes to the library's trash as a note of its own that the user can restore; `summary` is added, dated, to the copy's account. `expected_version` is the `version` you were shown (`preview_revision`, `get_note`): a note changed since is not touched (`noteConflict`), as its strokes may have other numbers.
    #[tool(
        name = "edit_revision",
        annotations(
            title = "Edit a retouched copy",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn edit_revision(
        &self,
        Parameters(args): Parameters<EditRevisionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| revision::edit_revision(host, args))
            .await
    }
}

impl ArdoiseServer {
    /// The write tools.
    pub(crate) fn write_router() -> ToolRouter<ArdoiseServer> {
        Self::write_tools()
    }
}
