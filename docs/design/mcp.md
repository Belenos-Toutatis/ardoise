# Driving Ardoise from an assistant (MCP) — milestone M9

SPEC §4.9 (retouching a note on a copy, M13: `docs/design/revision.md`). Ardoise contains no AI and stays offline; it exposes its library and its windows to the user's own
assistant (Claude Code, Claude Desktop, any MCP client) through the Model Context Protocol. The assistant brings
the intelligence: it reads the recognised text with positions *and sees the page as an image*, so it can recognise
diagrams, formulas and layout far better than the Windows recogniser alone, then write clean documents.

## Shape

- **`ardoise.exe --mcp [--read-only]`**: an MCP server on standard input/output (JSON-RPC, one message per line),
  started by the assistant as a child process. No network port. It finds the library like the app does (portable
  settings, §3.1: the library of this PC), works whether the app is open or not, and exits when stdin closes.
  `--read-only` exposes only the read tools.
- **Crate `crates/ardoise-mcp`**: the server (tools, prompts, their schemas and texts) on top of `ardoise-core`,
  `ardoise-render` and `ardoise-hwr`. It owns no state beyond the open library; every write goes through the same
  code as the app (`Library::save` with its stamp check, `fsutil::write_atomic`, the trash), so the app and the
  server can run at the same time (the app already copes with files changed by Nextcloud).
- **SDK**: `rmcp` (the official Rust SDK, Apache-2.0), features `server` + `transport-io`. Tokio current-thread
  runtime, created only in `--mcp` mode.
- **Windows subsystem**: `ardoise.exe` is a GUI-subsystem program. A parent that starts it with redirected handles
  (`STARTF_USESTDHANDLES`, what Node and Claude Code do) gives it working pipes: check it for real, and never write
  anything but protocol messages to stdout (logs go to stderr and the diagnostic log).
- **Bridge to the open app** (`ardoise-mcp` client side + app server side): a named pipe
  `\\.\pipe\ardoise-<user name>` created by the running app, with a DACL for the current user only and
  `PIPE_REJECT_REMOTE_CLIENTS`; one JSON request / response per line. When no app is running, the bridge tools answer
  an error that says so ("Ardoise n'est pas ouverte"). The app executes bridge requests through the same commands as
  its interface (open a note in the editor, start/stop the presentation, next/previous page, current state).

## Identifying notes

A note is named by its `id` (UUID from `note.json`) or by its path relative to the library (`/`-separated, as the
library listing gives it); tools accept either (`note` argument). Results always give both. Coordinates exposed to
the assistant are **millimetres** from the top-left of the page (device units / 100), rounded to 0.1 mm.

## Tools

Every tool has a short description (English, the protocol's lingua franca; the assistant answers in the user's
language anyway), a JSON schema, and MCP annotations: `readOnlyHint` for the read tools, `destructiveHint: true` on
the tools that change or remove what is there (`update_note`, `move_note`, `trash_note`, `split_note`,
`straighten_shapes`: the MCP specification keeps `false` for tools that only add, even though nothing is ever
deleted permanently), `false` on the others, `idempotentHint` where true. Errors are tool results with
`isError: true` and the same `{ code, message }` codes as the interface (`errorCodes.*`), message in English.

### Read

| Tool | Arguments | Result |
|---|---|---|
| `list_notes` | `binder?`, `tag?`, `favorite?`, `from?`/`to?` (ISO dates), `sort?` (`newest`/`oldest`/`title`), `limit?` (default 50, max 500), `offset?` | summaries: id, path, binder, title, created, modified (ISO), tags, favorite, page size in mm and orientation, stroke count, has background image; total count |
| `list_binders` | — | binders with note counts |
| `list_tags` | — | tags with counts |
| `search_notes` | `query`, `limit?` | like the app's search (title, tags, recognised text, accent-insensitive), with the snippet that matched; says how many notes are not read yet |
| `get_note` | `note`, `strokes?` (`none` default / `summary` / `points`), `max_points?` | metadata, layers, background (paper, template, image name/page), duration; `summary`: per stroke tool, colour, width, bounding box (mm), start/end time; `points`: decimated polylines in mm (Douglas–Peucker, bounded by `max_points`, default 5000 in total) |
| `get_note_text` | `note`, `language?` (`fr`/`en`, default the app's) | recognised text: lines with their bounding boxes (mm), and the structure when Windows' ink analysis is available (paragraphs, list items, drawings with bounds and stroke indices — `ardoise_hwr::structure`, M8); recognises on demand (and fills the per-PC cache) when the note is not read yet |
| `render_note` | `note`, `region?` ({x, y, width, height} in mm), `dpi?` (default 100, max 300), `background?` (default true), `layers?`, `labels?` (M13: each stroke's number written next to it) | a PNG image content (MCP `image`), longest side at most 2000 px, the whole page drawn at most 24 Mpx (dpi lowered to fit, the effective dpi and what lowered it returned in a text content next to it) so the assistant can *see* the note |
| `get_strokes` | `note`, `strokes` (numbers), `step?` | M13: the exact samples `[i, x_mm, y_mm, pressure, t_ms]` of the strokes, at most 20,000 a call |
| `ink_lines` | `note` | M13: the written lines found from the ink (`ardoise-revise`), with their strokes, baseline, slope and words |

### Write (absent with `--read-only`)

| Tool | Arguments | Effect |
|---|---|---|
| `create_note` | `title?`, `binder?`, `tags?`, `paper?` (`a4`/`a5`/`letter`/`tablet`/{width, height} mm), `orientation?`, `template?`, `from_file?` (absolute path of a PDF/image, `pdf_page?`) | a new note (blank, or on an image / PDF page as in the app); returns its summary |
| `update_note` | `note`, `title?`, `tags?` (replace) / `add_tags?` / `remove_tags?`, `favorite?` | metadata only |
| `move_note` | `note`, `binder` (created if missing) | moves the file, never overwrites (" (2)" rule) |
| `trash_note` | `note` | to the library trash, restorable from the app |
| `merge_notes` | `notes` (≥ 2), `layout` (`overlay`/`endToEnd`), `title?` | the M5 merge, a new note |
| `split_note` | `note`, `at_ms` or `polygon` (mm) | the M5 split |
| `straighten_shapes` | `note`, `strokes?` (indices; default the drawings found by Windows' ink analysis on visible layers, required without it) | the M7 straightening **on a copy** of a note that is not a revision (M13, `revision.md`: the original is not written), in place on a revision (its version before goes to the library's trash as a note of its own) |
| `preview_revision`, `create_revision`, `edit_revision` | `note`, `orders`, `summary` (`create_revision` also `title?`; the preview also `region?`, `dpi?`, `labels?`) | M13, see `docs/design/revision.md`: a list of orders retouches the strokes in memory (preview, nothing written), on a new copy next to the note, or in place on a note that already is a revision |
| `export_note` | `notes`, `format` (`png`/`jpg`/`webp`/`svg`/`pdf`/`txt`/`md`/`docx`/`gif`/`webm`/`mp4`), `path` (absolute file or folder), options of each format (dpi, background, quality, …) | the app's exports; never overwrites (" (2)" added), returns the written paths |
| `set_note_text` | `note`, `text`, `language?` | stores a corrected transcription in the per-PC handwriting cache, used by the search and the text exports instead of the recogniser's text until the ink changes; never written into the note file |

### Bridge (need the app open)

| Tool | Arguments | Effect |
|---|---|---|
| `app_state` | — | whether the app is open, what it shows (library / editor note / live mode state, presentation open, current page) |
| `open_note` | `note` | shows the note in the editor |
| `presentation` | `action` (`start`/`stop`/`next`/`previous`/`page`), `page?` | drives the live-mode presentation |
| `last_live_note` | — | id of the note written during the current or last live session (to offer a clean version at the end of a lesson) |

## Prompts

MCP prompts (they become slash commands in Claude Code), texts in French and English following the app's language
setting, each naming the tools to use:

- `mettre_au_propre` / `clean_up` (`note`, `format`: `markdown`/`word`/`latex`): render the note, read its text and
  structure, write a faithful clean document (titles, lists, tables, formulas; diagrams redrawn — for LaTeX:
  `circuitikz`, `mhchem`, `siunitx`, `pgfplots`), export or save it next to an export, and list what was uncertain.
- `titrer_et_etiqueter` / `title_and_tag` (`binder?`): for notes without a title, propose a title and tags from the
  text and the image, apply them with `update_note` after showing the list.
- `resume_seance` / `lesson_summary` (`date?` or `binder?`): a short summary of the notes of a lesson, for the class
  log (cahier de textes).
- `ameliorer_note` / `improve_note` (`note?`, `goals?`; M13, `revision.md`): improve a note on a copy, in six steps (look, say what
  is planned and list the spelling mistakes, letters only from the same handwriting in this note, `after` and `retime`, preview and
  correct, `create_revision` and an account). It names the write tools, so a `--read-only` server does not serve it.

## Privacy and safety

- What the assistant reads (text, images) goes to its provider under the user's own account settings. The guide
  says so in plain words (students' names on notes), and recommends `--read-only` when only reading is needed.
- The server reads the library of this PC only; `from_file` and export `path` must be absolute and are used as given
  (the assistant's client usually asks the user before a call that writes, depending on its permission settings).
- No tool deletes permanently, empties the trash, changes settings or touches the tablet.

## Tests

- `ardoise-mcp`: unit tests of each tool on a temporary library (synthetic notes), through rmcp's in-memory duplex
  transport: schemas valid, arguments validated, errors coded, writes atomic, `--read-only` hides the write tools.
- App: `--mcp` argument parsing; the bridge (pipe server answers, DACL, unknown requests).
- End to end (`cargo xtask e2e`, new journey kind): a Node script starts `target\release\ardoise.exe --mcp` on the
  demo library, sends `initialize`, `tools/list`, `prompts/list`, then calls every tool (render returns a real PNG,
  text of "Cours d'optique" contains its words, a write tool changes the file, the bridge tools answer "app not open"
  then work once the app is started by the journey).
- Real trial from Claude Code (with the user's consent to add the server to its configuration).

## As built — server and read tools (M9, part 1)

- **SDK**: `rmcp` 3.8 (`server`, `macros`, `transport-io`; `client` only in the tests). Tools are written with
  `#[tool_router]` / `#[tool]`, so each tool's JSON schema comes from the doc-commented argument struct
  (`schemars`); an argument that does not fit the schema is refused by rmcp as a failed tool result
  (`isError: true`, text "failed to deserialize parameters: …"), the others by our code with
  `{ code: "invalidArguments", message }`. Results are JSON objects sent both as text and as
  `structuredContent`; keys are `snake_case` like the arguments; `render_note` answers an `image` content
  (PNG, base64) followed by a text content with the sizes. The server name is `ardoise`.
- **Codes**: the interface's `errorCodes.*` (`noteNotFound`, `noLibrary`, `handwritingUnsupported`,
  `handwritingNoRecognizer`, `render`, …) plus `invalidArguments`, `appNotRunning` and `readOnly` (reserved
  for the next parts).
- **Crate layout** (`crates/ardoise-mcp/src`): `server.rs` (handler, `build_tool_router(options)` = the one place
  where groups are chosen and `--read-only` is enforced), `host.rs` (the services the app gives the server),
  `read.rs`, `write.rs`, `bridge.rs`, `prompts.rs` (one file per group; the last three are stubs),
  `notes.rs` (id-or-path lookup, mm, ISO dates), `strokes.rs` (stroke summaries, Douglas–Peucker),
  `error.rs`. The crate cannot depend on the app, which owns the handwriting cache and the exports: the app
  implements the traits of `host.rs` in `app/src-tauri/src/mcp.rs`.
- **Adding to it**: a tool is a method in an `#[tool_router(router = <name>, vis = "pub(crate)")] impl ArdoiseServer`
  block, returning `Result<CallToolResult, ErrorData>` through `self.blocking(|host| …)` (blocking pool, errors
  turned into failed results); the new router is added in `ArdoiseServer::write_router()` / `bridge_router()`.
  Prompts: `prompt_router()` in `prompts.rs`. App-side needs go in `WriteServices` / `AppBridge`
  (`host.rs` carries them, `Host::with_write` / `with_app` set them in `mcp.rs`).
- **Finding the library**: `--mcp` builds an `AppContext` like the app's (`SettingsStore::open_default()`:
  `ardoise.toml` next to the executable, section of this machine's name) and calls `commands::current_state`
  at every tool call, so a library changed in the app is followed; `noLibrary` when none is chosen. The
  automatic export folder is left out of `list_binders` while the automatic export is on. The language of
  the app comes from `diagnostic::report_language`.
- **Handwriting**: the app's `Indexer` (M7) is started at the first search or text request (a session that only
  lists notes never wakes the engine) with `busy = false`: the process cannot see live mode, which runs in the
  app's process, so the background pass keeps the low priority of the background engine. **The search follows
  the library as it is now** (after review): `search_notes` gives the listing it made to
  `Handwriting::search_in`; `--mcp` has no folder watcher, so it compares a fingerprint of that listing (paths,
  stamps, ids) with the one the indexer last looked at and calls `Indexer::refresh` when it differs (a note
  written by the app, a tool, Nextcloud...). `Indexer::search_in` counts a note only with the text of the
  version listed: its record (path + stamp, `files.json`) must give the strokes of an entry in memory in the
  language recognised; otherwise it is unread, never searched with an older text. The wait is bounded: until
  the pass of that refresh has listed the library and taken what the cache knows (`Indexer::ready`, 10 s at
  most, a fraction of a second in practice), then for the notes left to recognise only when they are at most
  10 (15 s at most: a note just written or edited), else 1 s; then it answers with what is read and
  `unread_notes` (the hint says that `get_note_text` reads one now). `get_note_text` recognises on demand
  through `Indexer::recognition_at` (forces a language, and keeps the note's record), so the next search finds
  that note at once. The write tools need no call of their own: the files they change change the listing.
  Structure: `ardoise_hwr::structure::structure_of` (Windows' ink analysis, else layout heuristics;
  `structure.source` says which). Outside Windows `SystemRecognizer` answers `Unsupported`, so the text tools
  answer `handwritingUnsupported`, and the search works on titles, tags and binders.
- **Choices the design left open**: `list_notes` filters and sorts on the *creation* date (`from` / `to` are
  local dates; `to` includes its whole day); `limit` is clamped to 1..500 (50 by default); the stroke count and
  page size need the note read, which is done for the returned page only. `search_notes` follows the
  interface's rule (every word in title, tags, binder or recognised text; a word begins a recognised word),
  gives `matched_in`, a `snippet` and `text_search.unread_notes`. `get_note` `points`: tolerance doubles
  from 0.05 mm until the points fit `max_points`; if even the coarsest simplification is over budget the
  last strokes come without points (`strokes_without_points`). `render_note` renders the whole page at the
  dpi (25..300, default 100; lowered so that the region's longest side is at most 2000 px) and crops the
  region (`ardoise-render` draws pages, not parts of them); the dpi is also lowered so that the page drawn
  holds at most 24 Mpx and 16 000 px a side, which matters only for very large pages (2 m custom paper, an A0
  PDF: about 60 to 120 dpi at most); `dpi_limited_by` says `image_size` or `page_size`. `layers` (indices or
  names) draws the chosen layers even if hidden. `--read-only` serves the read
  group only (bridge tools too, `app_state` included).
- **Tests**: `crates/ardoise-mcp/tests/read_tools.rs` (in-memory duplex transport, temporary library, fake
  handwriting service), unit tests in the modules, `app/src-tauri/src/mcp.rs` (argument parsing), and the
  journey `app/e2e/mcp.mjs` (real executable over pipes; not wired into `run.mjs`).
- **GUI subsystem and pipes — checked for real**: the release `ardoise.exe` (PE subsystem 2, "windows") started by
  Node with `stdio: pipe` (what Claude Code does) answers `initialize`, `tools/list` and every read tool, and
  exits with code 0 when its stdin is closed; nothing else reaches stdout (the journey fails on any stdout line
  that is not a JSON-RPC message). No console strategy is needed for that case: the parent's pipes are the
  process's standard handles. `console::attach_parent_console()` is still called first, for a server started by
  hand from a terminal without redirection (GUI programs then have no standard handles; it attaches only when
  stdout is null, as the other command-line modes do). The banner goes to stderr, best effort.
  Journey: `node app/e2e/mcp.mjs target/release/ardoise.exe [fr|en]` (13 checks, "MCP OK"); the French
  handwriting recognizer of Windows is needed for the text and search checks.

## As built — write tools and prompts (M9, part 2)

- **Where the code is**: `crates/ardoise-mcp/src/write.rs` (the `WriteServices` trait, the nine tools, their
  annotations) with one module per kind in `src/write/` (`create`, `update` = update/move/trash, `transform` =
  merge/split/straighten, `export`, `text`); `src/prompts.rs`; the app's side in `app/src-tauri/src/mcp/write.rs`
  (`AppWrite`, passed with `Host::with_write`). The crate does the choosing and the checking (arguments, names of
  files, defaults), the app does what only its own code does.
- **What each tool reuses** (nothing is re-implemented):
  - `update_note`, `move_note`, `trash_note`: `ardoise-core` alone (`Library::save` with the stamp check for tags and
    favourite in one save, `retitle` for the title, `move_to` with the " (2)" rule, `trash`). Nothing is deleted: the
    note is in `_ardoise/trash`, restorable from the app's trash view (30 days).
  - `create_note`: a blank note is the app's new note (A4-sized tablet page, one layer named `Calque 1` / `Layer 1`
    after the app's language) through `WriteServices::create_blank` (thumbnail made like the app does); `from_file`
    is `background::notes_from_file_with_progress` (image or one PDF page; page 1 when `pdf_page` is omitted, never
    all the pages), then the title and tags asked for are applied. `paper`, `orientation` and `template` are refused
    with `from_file` (the page takes the shape of the file). Papers: `a4` 210x297, `a5` 148x210, `letter`
    215.9x279.4, `tablet` 297x216 (the default), or `{ width, height }` in mm (10 to 2000); `orientation` turns the
    page, without it the paper's own is kept. Templates: `blank lined seyes grid5mm millimetric dotted music`.
  - `merge_notes`: `operations::merge_in` (a new note in the binder of the first one; its background image or PDF
    page is kept, the others' are not). The notes are merged **in the order given** (the library dialog sorts
    oldest first). Default title: the first note's title, or its date, with " (assemblage)" / " (assembled)".
  - `split_note`: `operations::split_in` with the file's own content and stamp (narration and background go with
    the new note). `at_ms` is on the clock of the strokes' `start_ms`/`end_ms`; `polygon` is `[[x, y], ...]` in mm.
    Default title for the new note: "<title> (suite)" / "(extrait)", "(continued)" / "(excerpt)", as the editor
    proposes.
  - `straighten_shapes`: `ardoise_ink::shapes::straighten` on the strokes the editor would let the user select as
    drawings, the new strokes taking the place of the first stroke they replace exactly as `straightenSelection`
    does in the editor. Default strokes (after review): those of the drawings Windows' ink analysis finds on the
    visible layers (`structure_of` with `analyzed: true`; the recognised text is not needed to find them, so an
    empty recognition is used when there is none); the layout heuristics are not trusted (they guess drawings from
    what the recogniser did not read, which may be letters), so without the analysis `strokes` is required
    (`invalidArguments` says so). Checked for real on the demo library: pure handwriting notes give no stroke,
    the drawing notes a few. **The version before is kept**: before the save, the file as read is put into the
    library's trash as a note of its own (`Library::trash_copy`, new in `ardoise-core`: the record is written
    first, the note never passes through a binder) with a new id (it must never stand for the note once restored)
    and the title "<title> (before straightening)" / "(avant redressement)"; the app's trash view restores it next
    to the note (30 days). Then one `notes::save_with_key` (the editor's own save: `original.ardoise` the first
    time the ink changes, the thumbnail again) with the stamp read at the start: a note changed meanwhile gives
    `noteConflict`, is left alone, and the copy is taken back (`Library::purge`). The result gives
    `previous_version` (`in_trash`, `title`, `id`, a sentence for the user), `strokes_tried` and `chosen_by`
    (`ink_analysis` / `given`). Nothing recognised: no save, no copy (`saved: false`).
  - `split_note` changes the source too, but the app's own split keeps no previous version either (the kept part
    is saved over the note with `save_in`; the other part is in the new note, so no ink is lost): mirrored,
    `destructiveHint: true` only.
  - `export_note`: the crate resolves `path`: absolute only (`invalidArguments` otherwise); a folder (existing, or
    ending with a separator, or without extension: created if missing) gets `<note file name>.<ext>`; a file name
    must end with the format's extension (`.jpeg` is accepted for jpg); several notes in an image format need a
    folder (one file per note), pdf/txt/md/docx put all the notes in one file (`Ardoise export.<ext>` in a folder),
    a replay takes one note. The first free name is chosen (" (2)", " (3)"…; `exportFailed` after 999), and the app
    writes images, PDF, txt and docx with `write_atomic_new` (a name taken in the meantime moves on to the next free
    one). **No existing file is replaced, reused or removed** (after review): a Markdown name is chosen with its
    `<name>_images` folder free too, and written by `textdoc::write_markdown_new` (the folder made with
    `create_dir`, `<name>_images (2)` when taken meanwhile; the `.md` with `write_atomic_new`; nothing reused or
    removed, its own drawings taken back if the `.md` was taken meanwhile; the GUI's `write_markdown`, whose dialog
    confirmed a replacement, still reuses a folder holding only its document's previous drawings); a replay is
    encoded under a name of its own next to the target (`.<name>.<pid>-<n>.mcp-part`), then moved with
    `rename_no_replace` to the first free name, so a file made during the minutes of encoding stays. Every check
    comes before any folder is made, and the folders made for an export that fails are removed again (when empty).
    Defaults are the app's dialog's: 150 dpi, JPG quality 90, background on, PDF page `actual`; replays 1080p
    (720p for gif), 30 fps (15 for gif), speed 1, no pause cut, 3 s on the last frame, narration on. Image and PDF
    exports are `export::image_bytes` / `notes_to_pdf`; txt is `handwriting::text_document`; md and docx are
    `textdoc::document_of` + `write_markdown` / `to_docx` with the labels of the interface in the language asked;
    replays are `video::export_video_in` (MP4 may use the OpenH264 fallback only if the user allowed it on this PC).
    **Video**: the call runs on the blocking pool and answers when the file is complete (seconds to minutes); no
    progress notification and no cancellation from the assistant's side (decision: not needed for a first
    version; the connection stays quiet meanwhile, so a client with a short timeout may give up). Hence bounds
    (after review), `invalidArguments` with the limits: `duration_seconds` more than 0 and at most 600, `speed` 0.25
    to 16, `frame_rate` 10/15/24/30/60, `hold_end_seconds` 0 to 60 (`options_of`); then the app plans the replay
    (`video::estimate_in`, no file written) and refuses one over 11 minutes (narration included), 39 600 frames or
    20 minutes of estimated encoding (`check_replay_plan`), before encoding anything.
    The text exports use the corrected transcription when there is one, as the app's (changed on 2026-10-29 with the
    Markdown and Word fidelity fix, `docs/design/extras.md`, M8): the `.txt` gives it as it is, Markdown and Word lay it
    over the recognised lines.
  - `set_note_text` / `get_note_text`: see below.
- **Corrected transcription** (`set_note_text`): kept in the per-PC handwriting cache
  (`%LOCALAPPDATA%\Ardoise\handwriting\`) as `<note id>-<strokes hash>-fix.json`
  (`{ format, id, strokes, language, text }`), next to the entries of M7 and keyed by the same hash of the
  recognised strokes. Hence it is **dropped when the ink changes** (another hash is no longer found, and the cache's
  pruning removes the file with the older versions of the note), nothing is written into the note file, and old
  caches stay readable (entries are not touched). The search (`Indexer::search` / `search_in`, so the app's search
  as well as `search_notes`) reads the corrections folder at each search and, for a listed note that has one,
  searches the corrected text (read as lines of words without positions) *instead of* the recogniser's. After
  review: a correction counts for **any listed note** whose current strokes it fits, also one never recognised (no
  handwriting pack, or not read yet): the note is read once and its record kept; the background pass lists the
  library even when recognition is unavailable, for the interface's search. The pruning at the end of a pass
  re-reads a note before removing a correction of other strokes than the pass saw (it may be for an edit made
  during the pass, by the other process): only a correction that no longer fits the note goes. Written with a plain file write
  (no engine woken); read by the other process at its next search. `get_note_text` returns the corrected text as
  `text`, `is_correction: true` and `recognised_text`; the lines and the structure stay the recogniser's (they carry
  the positions). An empty `text` removes the correction. Limit: 200 000 characters.
- **When the app has the note open**: nothing special is done, as for a change made by Nextcloud. The editor sees
  the file's stamp change and shows the "changed elsewhere" banner (reload offered when there are no unsaved
  changes); if it has unsaved changes its save is refused with `noteConflict` instead of overwriting the tool's
  write (`Library::save` checks the stamp), and a moved or trashed note gives the "missing" banner. The tools
  themselves refuse to overwrite a version they did not read (`straighten_shapes`, `update_note`: stamp check).
- **Codes**: the interface's `errorCodes.*` (`noteNotFound`, `noteConflict`, `invalidName`, `nothingToMerge`,
  `nothingToSplit`, `exportFailed`, `invalidVideoSettings`, `codecUnavailable`, `backgroundUnsupported`,
  `pdfEncrypted`…) and `invalidArguments`; the new `writeUnavailable` (a host without the app's service: only for
  tests and embedding, never reaches the interface, so no `errorCodes` entry). Annotations: all nine
  `readOnlyHint: false`; `destructiveHint: true` for `update_note` (it replaces tags and renames), `move_note`,
  `trash_note`, `split_note` and `straighten_shapes`, `false` for `create_note`, `merge_notes`, `export_note` and
  `set_note_text`; `idempotentHint: true` for `update_note`, `move_note` and `set_note_text` only.
- **Prompts**: the router is built once at server start from `Environment::language()`: a French app serves
  `mettre_au_propre`, `titrer_et_etiqueter`, `resume_seance`; an English one `clean_up`, `title_and_tag`,
  `lesson_summary` (the six are `#[prompt]` functions and the other language's three routes are removed: no
  contortion). They are served with `--read-only` too (they only read). Their messages:
  - `clean_up` (`note?`, `format?` = `markdown` (default) / `word` / `latex`): names the note (id and path) when it
    exists; without `note`, tells to call `last_live_note` and to ask the user when there is none; five steps
    (`get_note` summary + `render_note` page, zoom with `region` on dense parts; `get_note_text` with its structure;
    a faithful document, nothing invented, mistakes of substance flagged; save with the assistant's own file tools or
    give it, `export_note` md/docx as a starting point; the list of what is uncertain, with positions in mm). For
    LaTeX: LuaLaTeX with `circuitikz`, `mhchem`, `siunitx`, `pgfplots`, `chemfig`, diagrams redrawn in code.
  - `title_and_tag` (`binder?`): an *untitled* note is one whose title is empty after trimming (the library then shows
    its creation date: `displayTitle` in the interface; the demo library has one untitled note). The message carries the list of those notes (40 newest, with count),
    tells to look at the image and the text, to reuse the existing tags (`list_tags`), to show the whole list and to
    call `update_note` (`title`, `add_tags`) **only after the user agrees**.
  - `lesson_summary` (`date?`, `binder?`): the notes of the day (default today; a date alone, or with a binder),
    or of the most recent day of the binder, are listed in the message in the order they were taken; the summary asked
    is a title line, 3 to 6 bullets of content, "homework" / "to remember" only if written, about 80 words, no student
    names, nothing saved; a date that is not ISO is a request error (`invalid_params`).
  Argument names are the same in both languages. **A read-only server** (after review: the texts take the
  server's options) names none of the tools it lacks: `clean_up` without a note asks the user which one (no
  `last_live_note`), and its format rules and step 4 point to the app's own text exports instead of `export_note`;
  `title_and_tag` shows the list for the user to apply in Ardoise (each note's title and tags) instead of calling
  `update_note`.
- **Tests**: `crates/ardoise-mcp/tests/write_tools.rs` (22 tests: every tool, atomic files, the stamp check, " (2)",
  trash and restore, the version before straightening in the trash (and taken back on a conflict), the default
  strokes from the ink analysis and the refusal without it, the destructive hints, a Markdown name whose drawings'
  folder is taken, replay limits, no folder left by a failed export, relative paths refused, coded errors, a
  read-only server lists none of them and refuses each call, a host without services), `tests/prompts.rs` (6, with
  the read-only prompts in French and English), `tests/support/mod.rs` (the library, fakes made of `ardoise-core`'s
  operations, an optional "ink analysis", the recorded export requests), `write/export.rs` unit tests (free names,
  limits, folders undone); `ardoise-core` `library/tests.rs` (`trash_copy`); the app's side: `mcp/write.rs` tests
  (thumbnails, `save_in`'s stamp check and original, split and merge, a note on an image, images/folders/PDF without
  replacing a file, a real GIF replay moved to a free name, a replay too long refused before encoding, the option
  mapping), `textdoc.rs` (`write_markdown_new` reuses, replaces and removes nothing), `mcp.rs` (the search follows
  the library after a write tool, a long first pass costs a search 1 s), `handwriting/cache.rs` (corrections bound
  to the ink, the re-check before pruning, old caches readable) and `handwriting/indexer.rs` (the search uses the
  correction, a listing of now is searched with the text of each version, a correction without any recognizer, a
  correction written during a pass survives its pruning); the journey `app/e2e/mcp-write.mjs`
  (`node app/e2e/mcp.mjs target/debug/ardoise.exe fr`, then `en`).
## As built — bridge to the open app and guide (M9, parts 3 and 4)

- **Layout**: `crates/ardoise-mcp/src/bridge.rs` (the `AppBridge` trait, `NoApp`, `PipeBridge`, the four tools),
  `bridge/protocol.rs` (platform-free: lines, version, limits, dispatch), `bridge/pipe.rs` (the named pipe, both
  ends; `cfg(not(windows))` stubs answer `Unsupported`, so `--mcp` uses `NoApp` there). The app implements
  `AppBridge` in `app/src-tauri/src/app_bridge.rs` and serves it with `PipeServer`; `--mcp` gets a `PipeBridge`
  through `Host::with_app(app_bridge::client(..))`. The same trait on both ends: the pipe is transparent.
- **Pipe name**: `\\.\pipe\ardoise-<SID>-<hash>`: the current user's SID (no user name to sanitise, unique across
  domains; any character outside `[A-Za-z0-9._-]` would be replaced) and 8 hex digits of FNV-1a 64 (folded) of the
  **settings file location** (`SettingsStore::path()`, its folder canonicalised, `\\?\` dropped, lower case). So
  `--mcp` started from one portable copy talks to the app started from that same copy, the e2e copies in temporary
  folders never meet the user's own, and copies that share `%APPDATA%\Ardoise\ardoise.toml` share one bridge.
- **Protocol** (one JSON object per line, at most 64 KiB, newline excluded): request
  `{"v":1,"id":7,"request":{"kind":"app_state"|"open_note"|"presentation"|"last_live_note", …}}`, answer
  `{"v":1,"id":7,"ok":…}` or `{"v":1,"id":7,"error":{"code","message"}}`. Another `v` gives `bridgeVersion`; an
  unknown kind, an unknown field, a missing `id`, unreadable JSON or a line over the limit gives `invalidRequest`, and
  the app is not asked anything (an over-long line also closes the connection). Codes received pass through when
  they are plain identifiers (`wire_code`, `^[a-zA-Z][a-zA-Z0-9]{0,47}$`: a live command's error, a note that cannot
  be read… reach the assistant; the usual ones are static, any other is allocated once, at most 64 per process);
  anything else becomes `unexpected`. The mapping is for safety, not a closed list. New codes, at the end of
  `error::code`:
  `appNotResponding`, `appBusy`, `bridgeVersion`, `invalidRequest` (they never reach the interface's screens).
- **Security**: DACL `D:P(D;;GA;;;NU)(A;;GA;;;<SID>)` (protected; network logons denied; the current user only);
  `PIPE_REJECT_REMOTE_CLIENTS`; the first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE` (`AlreadyServed` when the
  name is taken: the app logs it and runs without the bridge) and each next instance created *before* the previous
  one is handed to its connection, so the name is never free for another process; the client connects with
  `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION` and refuses a pipe whose serving process
  (`GetNamedPipeServerProcessId`) does not run as the same user; overlapped I/O with timeouts everywhere (client:
  connect 2 s, `app_state` / `last_live_note` 8 s, `open_note` / `presentation` 20 s; server: 30 s idle per
  connection, 5 s per write); at most 4 connections at once (the next one gets `appBusy`). One connection per request
  on the client side (an app opened or closed meanwhile is followed); the server accepts several requests per
  connection. No request deletes, changes a setting or touches the tablet.
- **Answers before a close** (review fixes): a connection over the limit is turned away by the accepting thread
  *after* reading its request (at most 1 s, `BUSY_READ`), with `appBusy` under the request's id: closing at once made
  the client's write fail (`ERROR_NO_DATA`) before it read anything, so it reported a lost connection. And when the
  client's write fails because the app closed its end (a request far over the limit, a connection turned away), the
  client still reads the answer left in the pipe (data written before `CloseHandle` stays readable; checked by
  `tests/bridge_pipe.rs`, which fails without it).
- **The accepting thread never gives up**: a failure of `ConnectNamedPipe`, or of the next instance, is logged once
  per run of failures, kept for `PipeServer::trouble()` and tried again after a pause (`retry_delay`: 250 ms, doubled
  each time, at most 5 s, waiting on the stop event: no busy loop). An instance that failed to connect is replaced by
  a fresh one created before it is closed; without a spare instance the connection is served on the accepting thread
  itself, then its instance reused. One instance stays open throughout, so the name is never free.
- **Who serves**: only the windowed app, from `setup` (not `--mcp`, `--self-test`, nor the other command-line modes,
  which exit before Tauri starts). Failures are logged on stderr (`assistant bridge: …`) and shown in Settings >
  Assistant: `mcp_launch_info` gives `bridge: { state, detail }` (`serving`, `retrying` + detail, `anotherCopy` when
  another Ardoise of the same copy holds the pipe, `unavailable` + detail, `off` outside Windows or without a settings
  location), shown as the line "Liaison avec Ardoise ouverte : …". `app_state` cannot report it: when the bridge does
  not serve, nothing reaches the app.
- **Requests are carried out like clicks**: what the Svelte interface owns goes to the main window as the event
  `bridge-request` `{ id, request: { kind: "view" | "openNote" | "presentation", … } }`; the interface answers with
  the command `bridge_reply(id, { ok } | { error: { code, message } })` (main window only), correlated by id. The
  window has 3 s for `view` (then `app_state` answers `screen: "unknown"` with the rest) and 15 s for an action
  (`appNotResponding`). In the interface, `lib/app-bridge.svelte.ts`: the screens register what they offer while
  mounted (`bridgeScreens`: `home` = `Home.svelte`, `editor` = `Editor.svelte`, `live` = `LiveView.svelte`;
  `App.svelte` says `loading` / `setup` / `error` and wires the event); `answerBridge` and `drivePresentation` are
  pure apart from them (vitest). Messages for the assistant are English, like Rust's `message`. The interface keeps
  at an action at most 12 s (`ACTION_BUDGET_MS`, under Rust's 15 s), and a newer `presentation` request replaces the
  one under way: `page` checks before each step whether its request is abandoned (`appNotResponding` past the time,
  `appBusy` when replaced), so the window never goes on paging after Rust has answered.
- **`app_state`**: the window's view (screen `library` / `editor` / `live` / `setup` / `loading`, the editor's note
  and unsaved changes) joined with Rust's: version, library, and the live session (`LiveHub::overview`, new: state,
  running, page and pages, presentation screen). The tool resolves the editor's note to id, path and title.
- **`open_note`**: the tool looks the note up in the library first (a wrong name answers `noteNotFound` before the
  app is asked), then sends the absolute path; the app accepts only an existing `.ardoise` file inside its library
  and outside `_ardoise/` (`note_in_library`, canonical paths). The interface refuses with `appBusy` when the editor
  cannot be left (unsaved changes, a narration recording, a video export, a save under way: the user's "Back" would
  hold too), while live mode is on screen (opening a note would take the teacher out of the lesson), while a library
  dialog or the tablet test is open, and before a library exists. Otherwise `Home` shows the library for a tick,
  then the note (the editor loads it afresh), and the answer waits until the editor has **read** the note
  (`EditorSlot.loaded`, `NoteLoads` in `app-bridge.svelte.ts`): a note that cannot be read answers its error and
  code (a damaged file is not reported "opened"), a load replaced by another note answers `appBusy`, an editor that
  does not show up within 3 s `appNotResponding`. The note already in the editor is not reloaded, but the outcome of
  its load is still answered.
- **What "presentation" is** (M4, M8): the full-screen window of **live mode** (`presentation.rs`,
  `Presentation.svelte`), opened from the live window ("Project" / "Stop projecting"), whose page is the live
  session's current page. Without a tablet, live mode starts in `waiting` (the simulator streams with
  `--simulated-tablet`). The tool maps: `start` = the "Project" button (the screen chosen in the live window, else
  the saved or default one; `noMonitor` from Rust), except that the screen is **not saved** as this PC's choice
  (`open_presentation(monitor, remember: false)`: no bridge request changes a setting; the button still saves it);
  `stop` = "Stop projecting" (live mode goes on; idempotent, and a
  success when live mode is not on screen); `next` / `previous` = `live_command` `nextPage` / `previousPage`, as the
  arrow keys (`next` from the last page adds a blank page, like →; `previous` on page 1 does nothing); `page` = one
  step at a time up to page *n* (1-based, an existing page: `invalidArguments` beyond), each step waiting for the
  session's batch. The assistant **never starts or stops live mode** (that talks to the tablet): every action but
  `stop` answers `liveNotRunning` until the teacher opens live mode.
- **`last_live_note`**: this run's session once it wrote a note (`running`, pages in order), else the session
  remembered from an earlier run. **It survives a restart**: `%LOCALAPPDATA%\Ardoise\last-live-session-<hash>.json`
  (per-PC state, never in the library; atomic; `<hash>` names the copy as in the pipe's name, `settings_hash`, so
  copies with their own settings no longer overwrite each other's record) holds the library, the end time and each
  page's note id and path, written when live mode stops (`live_stop`) and when the main window closes; it is given
  only while the same library is open. While the copy has no file of its own, the former shared
  `last-live-session.json` is read (same library rule). The tool finds each note by id in the library now (a note moved since is followed; one gone is
  `found: false`) and gives the last page still there as `note`.
- **Annotations**: `app_state` and `last_live_note` (idempotent) read-only; `open_note` (idempotent), `presentation`
  `read_only_hint: false`; all `destructive_hint: false`. `--read-only` keeps all four out (part 1's choice). With no
  app, each answers `appNotRunning` / "Ardoise is not open" (argument errors, e.g. an unknown note, come first).
- **Settings > Assistant (MCP)** (`AssistantSection.svelte`, `lib/assistant-setup.ts`, locales section
  `assistant`): the command `mcp_launch_info` gives `current_exe()`; the panel shows
  `claude mcp add --scope user ardoise "--" "<exe>" --mcp` (syntax checked in the Claude Code documentation: options
  before the name, the server's command after `--`) and the Claude Desktop `mcpServers` entry, both with
  `--read-only` when the box is ticked, with copy buttons (`navigator.clipboard`), the bridge's status line, the
  privacy reminder and a pointer to the guide. The app never edits a Claude configuration. **Why `"--"` is quoted**:
  PowerShell drops a bare `--` when it starts a *script*, and an npm install of Claude Code makes `claude` the
  script `claude.ps1` (which passes `$args` to node): `--mcp` would then reach `claude mcp add` as one of its options.
  Checked on this PC with stand-ins (a `claude.ps1` shaped like npm's, a `claude.cmd`, and `node` as a native
  program printing its arguments), Windows PowerShell 5.1 and cmd.exe: a bare `--` is lost through the `.ps1` and
  kept for native programs and `.cmd`; `"--"` arrives as `--` in every case. `pwsh` 7 is not installed here (its
  parameter binder treats `--` the same way, documented, not checked); the native `claude.exe` itself was not run.
  The privacy reminder says assistants *usually* ask before a change, depending on their permission settings, that
  the tools that change or remove notes are flagged destructive, and points to `--read-only`.
- **Guide**: `docs/guide/assistant.fr.md` / `.en.md` (folder `docs/guide/` for M10's user guide): what an assistant
  can do, Claude Code (user scope) and Claude Desktop (`%APPDATA%\Claude\claude_desktop_config.json`; Microsoft
  Store / MSIX under `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\`; *Edit Config* opens
  the right one), `--read-only`, prompts (`/ardoise:name (MCP)` or `/mcp__ardoise__name`), the bridge tools,
  troubleshooting, privacy in plain words.
- **Tests**: `bridge/protocol.rs` (round trip, wire spelling, size limit, unknown requests and fields, versions,
  lines, safe codes passed through, the busy line), `bridge/pipe.rs` (name and hash, retry delays; on Windows the
  accepting thread surviving injected instance failures: served inline, trouble reported then cleared),
  `tests/bridge_tools.rs` (the tools through rmcp with a fake app: annotations, `--read-only`, `appNotRunning`, note
  resolution, argument checks, `last_live_note` by id), `tests/bridge_pipe.rs` (Windows, a real pipe per test: round
  trip, several requests per connection, the DACL read back with `GetSecurityInfo` (the user's SID allowed, network
  denied, nothing else), one server per name, "not open" at once, the client's timeout, malformed and over-long
  lines, a 1 MiB request still answered, more connections than the limit answered `appBusy` then served again),
  `app_bridge.rs` (window answers, path checks, correlation by id, the remembered session, one record per copy, the
  former shared record, the bridge status), vitest `lib/app-bridge.test.ts` (also: `open_note` answered once the
  note is read or with its load error, `page` stopping when abandoned, newer requests replacing older ones),
  `lib/assistant-setup.test.ts`, `lib/mock-live.test.ts` (`remember: false`), `components/AssistantSection.test.ts`;
  e2e `app/e2e/journeys-m9-bridge.mjs` (wired in `run.mjs`: `--mcp` of the runner's copy drives the open app) and
  the part `bridgeWithoutApp` of `app/e2e/mcp.mjs`.

## As built — retouching on a copy (M13)

Design and guarantees: `docs/design/revision.md` (its "As built — tools and prompt" section says what the code does).
What changes in this document's tools:

- `straighten_shapes` no longer writes the note it is given unless that note is a revision. A note that is not one gets a
  **copy** (a new note next to it, a revision of it, titled "<title> (clean)" / "(propre)", with "Shapes straightened: N"
  as its account) and is left byte for byte as it was, so **nothing is put in the trash for it** (the paragraph on
  `straighten_shapes` in "write tools" above describes the earlier behaviour: the version before in the trash). On a
  revision the shapes are straightened in it, the version before goes to the trash as before (title "(before
  straightening)" / "(avant redressement)") and a line is added to the account. The result says which
  (`made_a_copy`).
- `edit_revision` keeps the version before in the trash too ("(before retouching)" / "(avant retouche)").
- Annotations: `preview_revision` `readOnlyHint: true`, `idempotentHint: true` (it is in the write group, so `--read-only`
  does not serve it, because what it prepares is what `create_revision` writes); `create_revision` not destructive (it only
  adds); `edit_revision` destructive (it changes a note); `get_strokes` and `ink_lines` are read tools.
- Codes: `invalidOrders` (the engine's message) and `notARevision`.
- The prompts `ameliorer_note` / `improve_note` are the only ones that need the write tools: a `--read-only` server does not
  serve them.
