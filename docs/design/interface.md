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

## M8: sheet format, image turn, clipboard (agent `page`)

All in the editor's Page panel (`PagePanel.svelte`, `BackgroundImageSection.svelte`) and `lib/editor/` (pure operations,
unit-tested in `page-format.test.ts`; component tests in `Editor.page.test.ts` and `BackgroundImage.test.ts`). The note
format does not change; every change is one undoable step.

- **Portrait / landscape** (`ops.setOrientation`) swaps the page's width and height and does *not* turn the ink: the
  strokes and the background image keep their orientation. "Turn the sheet with the writing" (`rotateQuarter`) stays,
  under its own heading; the panel's texts spell out the difference. A page is landscape when wider than tall (a square
  one counts as portrait).
- **Paper size** (`ops.resizePage`): presets A4 210 × 297, A5 148 × 210, Letter 215.9 × 279.4 and "Tablet" 216 × 297 mm
  (the Bamboo Slate page, which is not a true A4: 29700 × 21600 units), always in the page's current orientation
  (`lib/editor/page-size.ts`), plus two millimetre fields (10 to 2000 mm a side) and an Apply button. The select shows
  "Custom" when the size is none of the presets.
- **What happens to the ink** (decided here): by default the ink keeps its size and place. When some of it would fall
  outside the new page but all of it fits (bounds of the samples, hidden layers included), it is moved back inside by
  the smallest shift; when it does not fit it is left where it is, and a notice says how many strokes now lie beyond the
  page. Such strokes are kept in the note (they are not drawn; the former size brings them back; the crop does not
  delete hidden ones). With "Scale the writing with the sheet", the whole old page is scaled uniformly to fit the new
  one and centred in it: strokes, stroke widths and image alike (no distortion; times and pressures untouched).
- **Background image** follows the ink in both cases (same shift, or same scale and shift), so that annotations stay
  aligned with it; the "Placement" select then shows "custom" and the user can re-fit it.
- **Turn the image alone** (`turnImageAlone`): the stored pixels' `quarterTurns` change and the image is placed again
  with the fit it had (contain, cover or stretch; contain when its proportions made them all the same, so that it is
  never distorted); an image moved or cropped by hand turns in place around its centre. Page and strokes stay.
- **Orientation of notes made from a file**: `create_notes_from_file` already gave a PDF page its own size and an
  image an A4 sheet of its orientation (`page_for_pdf`, `page_for_image`; the PDF page size has its `/Rotate`
  applied by Windows), the library dialog now says so. When an image or PDF page is put under an *existing* note
  and its orientation differs (never for a square image), the section asks whether to switch the note to that
  orientation: the switch and the placement are one undo step, the image fitted on the switched page.
- **Clipboard** (`lib/editor/clipboard.ts`): strokes travel as JSON text with a marker
  (`{"ardoise":"ardoise-strokes","version":1,"strokes":[{tool,color,width,points}]}`) on the system clipboard, so a
  paste works in another note, window or session; anything else is ignored (the paste event is left alone), and
  decoding validates every field and bounds the size. Ctrl+C / Ctrl+X / Ctrl+V use the browser's copy / cut / paste
  events (no permission prompt in WebView2); the selection panel's Copy / Cut / Paste buttons use
  `navigator.clipboard`. Pasted strokes keep colour, tool, width, pressure and points, land on the active layer, are
  selected, and are offset by 4 mm diagonally (repeatedly) while an identical copy already lies at the place; strokes
  that would be wholly outside the page are centred on it. **Timing:** the pasted samples are re-timed to start 0.5 s
  after the last sample of the destination note, keeping their own rhythm, so the replay draws them last, as written
  (Duplicate keeps its old behaviour).

## Tests

- Component tests (vitest + jsdom) for each screen, with the browser stand-in.
- Pure editor logic (`lib/editor/*.ts`: operations, undo/redo, lasso and eraser hit tests, rotation, crop) unit-tested.
- End-to-end (`cargo xtask e2e`): the real release executable on a demo library, driven through WebView2's DevTools
  protocol: open a note, modify it, undo, save, find it by search — in French and in English.
