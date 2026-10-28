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
| `note_thumbnail` | `path`, `stamp` | PNG data URL or null. Cached in `%LOCALAPPDATA%\Ardoise\thumbnails\` (never in the library, SPEC §3.1), keyed by note id, place in the library and stamp (entries unused for 60 days are removed); from the file's `thumb.png`, else rendered |
| `create_note` | `binder`, `title` | `NoteSummary` of a new blank A4 landscape note |
| `create_binder` | `binder` | – |
| `move_notes` | `paths`, `binder` | new paths |
| `trash_notes` | `paths` | – |
| `trash_list` / `trash_restore` / `trash_purge` / `trash_empty` | – / `id` / `id` / – | entries / path / – / count |
| `set_note_tags` / `set_note_favorite` | `path`, `tags` or `favorite`, `stamp` | new stamp |
| `retitle_note` | `path`, `title`, `stamp` | `{ path, stamp }` |
| `open_note` | `path` | `{ note, stamp }` |
| `save_note` | `path`, `note`, `stamp`, `imageKey?` | new stamp; keeps `original.bin`, audio and unknown members; writes the background image staged under `imageKey` (M7, `extras.md`); regenerates `thumb.png`; `noteConflict` if changed meanwhile |
| `render_background` | `page`, `background`, `widthPx`, `image?` | PNG data URL of the paper/template/texture, with the background image `{ image, key }` (M7) |
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

## M8: pen and touch

Feedback from a Surface with its pen and touch screen. Pure logic in `lib/gestures.ts` (tested without the DOM);
the components only feed it Pointer Events fields and apply what it returns.

**Classification** (`pointerKind`): `pointerType: 'touch'` is a finger; `'pen'` is a pen tip, or the **eraser end**
when `buttons & 32` (or `button === 5`); anything else is a mouse (its behaviour does not change).

**Pen** (editor, `Canvas.svelte`):

- The eraser end erases with the current eraser settings (whole stroke or partial, radius) whatever the selected tool,
  and shows the eraser circle while it hovers (`pointermove` with `buttons & 32`). It does nothing in a replay
  (`readonly` prop) or while the split panel is open. *Not verified on hardware*: whether WebView2 reports
  `buttons & 32` for a hovering inverted pen as well as for contact (`docs/hardware-tests.md`, H3).
- The stroke width follows the pen's pressure (it already did: the editor records `pointer.pressure`, the geometry
  maps it to the diameter). A mouse keeps its constant 0.5, a finger a constant 0.6 (`inkPressure`), a pen reporting
  0 counts as 0.6.
- **Barrel button** (`buttons & 2` on a pen): a **temporary lasso**, whatever the tool; the selection it makes is the
  one the Select tool would make (Shift adds, Ctrl removes), and the tool does not change. A mouse's right button is
  not the barrel. The canvas shows no context menu for a pen (Windows turns the barrel button into a right click).
- **Palm rejection** (`PenPresence`): while a pen is in contact or has been seen hovering less than 1.2 s ago (or until
  `pointerleave`), touch is ignored on the page; a pen arriving drops the fingers being tracked.

**Touch** (`TouchGestures`): by default a finger never draws.

- One finger pans (after an 8 px slop that is not lost as a jump), two fingers pinch (zoom around their centre and
  pan together, `pinchView`), a double tap (two quick still taps within 40 px) fits the page.
- **Draw with a finger** (`touchPrefs.drawWithFinger`, `lib/touch-prefs.svelte.ts`): a finger then acts like the
  mouse with the current tool (drawing, lasso, crop…); the two-finger pinch still works, and a second finger
  dropping while the first one draws **drops the stroke without reporting it**. It is an interface preference kept in
  the browser storage (`ardoise.touch.drawWithFinger`, like the custom colours and the export choices), not a
  setting of `ardoise.toml`: it describes the screen of one PC. Switch: Settings > Pen and touch, and a "Finger"
  toggle in the editor's zoom bar. With the Pan tool, or in a replay, a finger always pans.
- The same gestures apply to the replay viewer (the editor's canvas in `readonly` mode), to the live control window's
  preview (`LiveCanvas` `preview`: one finger moves the projected area as the mouse does, two fingers pinch it, a
  double tap shows the whole page) and to the presentation window (see `tablet-live-classroom.md`).

**Fit the width** (`fitWidthView` in `lib/editor/view.ts`, `lib/live-view.ts`): next to every "fit the page":
editor and replay "Width" button in the zoom bar and **Ctrl+2** (matched by key code, so French keyboards work), live
mode "Width" button and the **W** key (listed in the keyboard help), presentation window the **W** key. A page taller
than the window shows its top.
