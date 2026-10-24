# Interface: library and editor (milestone M3)

Developer document (English). Requirements: SPEC §3.1–§3.4, §4.3, §4.4 (P1 items), §5, ROADMAP M3. Models:
`docs/design/notes-and-rendering.md`, `docs/file-format.md`.

## Contracts (fixed; change only with a reason in your report)

- TypeScript models of a note and of the library: `app/ui/src/lib/note.ts` (mirror of the Rust serde shapes).
- Command wrappers: `app/ui/src/lib/api.ts` (section "Library", "Editor", "Settings: tablet") and
  `onLibraryChanged(handler)` (Tauri event `library-changed`).
- Stroke geometry on screen: `app/ui/src/lib/ink.ts` (WebAssembly build of `ardoise-ink`, same outlines as the
  exports; fill with the non-zero rule). Cache outlines per stroke object; recompute only changed strokes.
- Browser stand-in: `lib/mock.ts` routes the M3 commands to `lib/mock-library.ts` (library screen) and
  `lib/mock-notes.ts` (editor), both over the demo store `lib/mock-data.ts` (mutable, `resetMockLibrary` for
  tests, `notifyLibraryChanged` after changes).
- The library screen shows the editor through `components/editor/Editor.svelte` with props
  `{ path, app, onclose, onsaved? }`.

## Rust commands (app/src-tauri)

Tauri maps camelCase arguments to snake_case parameters (`widthPx` → `width_px`). Errors are `{ code, message }`
(codes from `LibraryError::code()` / `FormatError::code()`, plus `noLibrary`), translated by `errorCodes.*`.

| Command | Arguments | Returns |
|---|---|---|
| `library_list` | – | `LibraryListing` (notes, binders, tag counts, problems) |
| `note_thumbnail` | `path`, `stamp` | PNG data URL or null. Cached in `%LOCALAPPDATA%\Ardoise\thumbnails\` (never in the library, SPEC §3.1), keyed by note id + stamp; from the file's `thumb.png`, else rendered |
| `create_note` | `binder`, `title` | `NoteSummary` of a new blank A4 landscape note |
| `create_binder` | `binder` | – |
| `move_notes` | `paths`, `binder` | new paths |
| `trash_notes` | `paths` | – |
| `trash_list` / `trash_restore` / `trash_purge` / `trash_empty` | – / `id` / `id` / – | entries / path / – / count |
| `set_note_tags` / `set_note_favorite` | `path`, `tags` or `favorite`, `stamp` | new stamp |
| `retitle_note` | `path`, `title`, `stamp` | `{ path, stamp }` |
| `open_note` | `path` | `{ note, stamp }` |
| `save_note` | `path`, `note`, `stamp` | new stamp; keeps `original.bin`, audio and unknown members; regenerates `thumb.png`; `noteConflict` if changed meanwhile |
| `render_background` | `page`, `background`, `widthPx` | PNG data URL of the paper/template/texture |
| `tablet_info` / `forget_tablet` | – | `TabletInfo` or null / – |

The library is the one configured for this PC. A `LibraryWatcher` runs while the app is open and emits
`library-changed` (debounced) to the window; it is restarted when the library changes.

## Screens

- **Main window** (`Home.svelte`): sidebar (all notes, favourites, binders as a tree, tags, trash; tablet entry
  that opens the tablet test) + top bar (search, sort, new note, settings) + content (note grid, trash view, or the
  editor full-space when a note is open).
- **Grid**: thumbnails, title (date when untitled), date; sort by date or title, ascending/descending; search by title
  and tags (accent- and case-insensitive); click opens, Ctrl/Shift-click selects; selection actions: move to binder,
  tags, favourite, trash; drag notes onto a binder in the sidebar; conflict copies flagged; refresh on
  `library-changed`.
- **Trash**: restore, delete for good, empty (with confirmation).
- **Editor**: toolbar (tools: select/lasso, pen, eraser whole/partial, pan; ink tool, colour (Inkspace palette +
  remembered custom colours), width; paper, template, texture; layers; rotate ¼ turn; crop auto/manual; undo/redo;
  save) and the page drawn as SVG over the rendered background (`render_background`), zoom with the wheel at the
  cursor, pan by dragging with the middle button or Space; keyboard: Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z, Ctrl+S, Del,
  Ctrl+C/Ctrl+V, Esc. Style applies to the selection, or to the whole note when nothing is selected. Undo/redo is
  unlimited during the session. Leaving with unsaved changes asks for confirmation.
- **Settings**: language, theme, library (existing), tablet (registered tablet, forget it, open the tablet test).

All visible text goes through `locales/fr.json` and `locales/en.json` (parity test).

## Tests

- Component tests (vitest + jsdom) for each screen, with the browser stand-in.
- Pure editor logic (`lib/editor/*.ts`: operations, undo/redo, lasso and eraser hit tests, rotation, crop) unit-tested.
- End-to-end (`cargo xtask e2e`): the real release executable on a demo library, driven through WebView2's DevTools
  protocol: open a note, modify it, undo, save, find it by search — in French and in English.
