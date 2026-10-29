# Driving Ardoise from an assistant (MCP) — milestone M9

SPEC §4.9. Ardoise contains no AI and stays offline; it exposes its library and its windows to the user's own
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
language anyway), a JSON schema, and MCP annotations: `readOnlyHint` for the read tools, `destructiveHint: false`
everywhere (nothing is ever deleted permanently), `idempotentHint` where true. Errors are tool results with
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
| `render_note` | `note`, `region?` ({x, y, width, height} in mm), `dpi?` (default 100, max 300), `background?` (default true), `layers?` | a PNG image content (MCP `image`), longest side at most 2000 px (dpi lowered to fit, the effective dpi returned in a text content next to it) so the assistant can *see* the note |

### Write (absent with `--read-only`)

| Tool | Arguments | Effect |
|---|---|---|
| `create_note` | `title?`, `binder?`, `tags?`, `paper?` (`a4`/`a5`/`letter`/`tablet`/{width, height} mm), `orientation?`, `template?`, `from_file?` (absolute path of a PDF/image, `pdf_page?`) | a new note (blank, or on an image / PDF page as in the app); returns its summary |
| `update_note` | `note`, `title?`, `tags?` (replace) / `add_tags?` / `remove_tags?`, `favorite?` | metadata only |
| `move_note` | `note`, `binder` (created if missing) | moves the file, never overwrites (" (2)" rule) |
| `trash_note` | `note` | to the library trash, restorable from the app |
| `merge_notes` | `notes` (≥ 2), `layout` (`overlay`/`endToEnd`), `title?` | the M5 merge, a new note |
| `split_note` | `note`, `at_ms` or `polygon` (mm) | the M5 split |
| `straighten_shapes` | `note`, `strokes?` (indices; default all closed or straight shapes) | the M7 straightening, one save |
| `export_note` | `notes`, `format` (`png`/`jpg`/`webp`/`svg`/`pdf`/`txt`/`md`/`docx`/`gif`/`webm`/`mp4`), `path` (absolute file or folder), options of each format (dpi, background, quality, …) | the app's exports; never overwrites (" (2)" added), returns the written paths |
| `set_note_text` | `note`, `text`, `language?` | stores a corrected transcription in the per-PC handwriting cache, used by the search instead of the recogniser's text until the ink changes; never written into the note file |

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

## Privacy and safety

- What the assistant reads (text, images) goes to its provider under the user's own account settings. The guide
  says so in plain words (students' names on notes), and recommends `--read-only` when only reading is needed.
- The server reads the library of this PC only; `from_file` and export `path` must be absolute and are used as given
  (the assistant's client asks the user before each call that writes).
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
