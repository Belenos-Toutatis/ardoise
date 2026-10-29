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
  app's process, so the background pass keeps the low priority of the background engine. The search waits at
  most 20 s for the first pass to end (a small library is searched whole), then answers with what is read
  and the number of unread notes; the pass goes on reading the library while the server lives. `get_note_text`
  recognises on demand through `Indexer::recognition_in` (new: forces a language) and fills the same cache.
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
  region; `layers` (indices or names) draws the chosen layers even if hidden. `--read-only` serves the read
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
  the app is not asked anything (an over-long line also closes the connection). Codes received are mapped to the
  server's static codes (`wire_code`; an unknown one becomes `unexpected`). New codes, at the end of `error::code`:
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
- **Who serves**: only the windowed app, from `setup` (not `--mcp`, `--self-test`, nor the other command-line modes,
  which exit before Tauri starts). Failures are logged on stderr (`assistant bridge: …`).
- **Requests are carried out like clicks**: what the Svelte interface owns goes to the main window as the event
  `bridge-request` `{ id, request: { kind: "view" | "openNote" | "presentation", … } }`; the interface answers with
  the command `bridge_reply(id, { ok } | { error: { code, message } })` (main window only), correlated by id. The
  window has 3 s for `view` (then `app_state` answers `screen: "unknown"` with the rest) and 15 s for an action
  (`appNotResponding`). In the interface, `lib/app-bridge.svelte.ts`: the screens register what they offer while
  mounted (`bridgeScreens`: `home` = `Home.svelte`, `editor` = `Editor.svelte`, `live` = `LiveView.svelte`;
  `App.svelte` says `loading` / `setup` / `error` and wires the event); `answerBridge` and `drivePresentation` are
  pure apart from them (vitest). Messages for the assistant are English, like Rust's `message`.
- **`app_state`**: the window's view (screen `library` / `editor` / `live` / `setup` / `loading`, the editor's note
  and unsaved changes) joined with Rust's: version, library, and the live session (`LiveHub::overview`, new: state,
  running, page and pages, presentation screen). The tool resolves the editor's note to id, path and title.
- **`open_note`**: the tool looks the note up in the library first (a wrong name answers `noteNotFound` before the
  app is asked), then sends the absolute path; the app accepts only an existing `.ardoise` file inside its library
  and outside `_ardoise/` (`note_in_library`, canonical paths). The interface refuses with `appBusy` when the editor
  cannot be left (unsaved changes, a narration recording, a video export, a save under way: the user's "Back" would
  hold too), while live mode is on screen (opening a note would take the teacher out of the lesson), while a library
  dialog or the tablet test is open, and before a library exists. The note already in the editor is a no-op.
  Otherwise `Home` shows the library for a tick, then the note (the editor loads it afresh).
- **What "presentation" is** (M4, M8): the full-screen window of **live mode** (`presentation.rs`,
  `Presentation.svelte`), opened from the live window ("Project" / "Stop projecting"), whose page is the live
  session's current page. Without a tablet, live mode starts in `waiting` (the simulator streams with
  `--simulated-tablet`). The tool maps: `start` = the "Project" button (the screen chosen in the live window, else
  the saved or default one; `noMonitor` from Rust); `stop` = "Stop projecting" (live mode goes on; idempotent, and a
  success when live mode is not on screen); `next` / `previous` = `live_command` `nextPage` / `previousPage`, as the
  arrow keys (`next` from the last page adds a blank page, like →; `previous` on page 1 does nothing); `page` = one
  step at a time up to page *n* (1-based, an existing page: `invalidArguments` beyond), each step waiting for the
  session's batch. The assistant **never starts or stops live mode** (that talks to the tablet): every action but
  `stop` answers `liveNotRunning` until the teacher opens live mode.
- **`last_live_note`**: this run's session once it wrote a note (`running`, pages in order), else the session
  remembered from an earlier run. **It survives a restart**: `%LOCALAPPDATA%\Ardoise\last-live-session.json`
  (per-PC state, never in the library; atomic) holds the library, the end time and each page's note id and path,
  written when live mode stops (`live_stop`) and when the main window closes; it is given only while the same
  library is open. The tool finds each note by id in the library now (a note moved since is followed; one gone is
  `found: false`) and gives the last page still there as `note`.
- **Annotations**: `app_state` read-only; `open_note` (idempotent), `presentation`, `last_live_note` (idempotent)
  `read_only_hint: false`; all `destructive_hint: false`. `--read-only` keeps all four out (part 1's choice). With no
  app, each answers `appNotRunning` / "Ardoise is not open" (argument errors, e.g. an unknown note, come first).
- **Settings > Assistant (MCP)** (`AssistantSection.svelte`, `lib/assistant-setup.ts`, locales section
  `assistant`): the command `mcp_launch_info` gives `current_exe()`; the panel shows
  `claude mcp add --scope user ardoise -- "<exe>" --mcp` (syntax checked in the Claude Code documentation: options
  before the name, the server's command after `--`) and the Claude Desktop `mcpServers` entry, both with
  `--read-only` when the box is ticked, with copy buttons (`navigator.clipboard`), the privacy reminder and a pointer
  to the guide. The app never edits a Claude configuration.
- **Guide**: `docs/guide/assistant.fr.md` / `.en.md` (folder `docs/guide/` for M10's user guide): what an assistant
  can do, Claude Code (user scope) and Claude Desktop (`%APPDATA%\Claude\claude_desktop_config.json`; Microsoft
  Store / MSIX under `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\`; *Edit Config* opens
  the right one), `--read-only`, prompts (`/ardoise:name (MCP)` or `/mcp__ardoise__name`), the bridge tools,
  troubleshooting, privacy in plain words.
- **Tests**: `bridge/protocol.rs` (round trip, wire spelling, size limit, unknown requests and fields, versions,
  lines), `bridge/pipe.rs` (name and hash), `tests/bridge_tools.rs` (the tools through rmcp with a fake app:
  annotations, `--read-only`, `appNotRunning`, note resolution, argument checks, `last_live_note` by id),
  `tests/bridge_pipe.rs` (Windows, a real pipe per test: round trip, several requests per connection, the DACL read
  back with `GetSecurityInfo` (the user's SID allowed, network denied, nothing else), one server per name, "not open"
  at once, the client's timeout, malformed and over-long lines), `app_bridge.rs` (window answers, path checks,
  correlation by id, the remembered session), vitest `lib/app-bridge.test.ts`, `lib/assistant-setup.test.ts`,
  `components/AssistantSection.test.ts`; e2e `app/e2e/journeys-m9-bridge.mjs` (wired in `run.mjs`: `--mcp` of the
  runner's copy drives the open app) and the part `bridgeWithoutApp` of `app/e2e/mcp.mjs`.
