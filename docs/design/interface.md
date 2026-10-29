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
  decoding validates every field and bounds the size and the ranges (what a saved note could not hold is refused, never pasted: `t` an integer in `0..=u32::MAX` with at most 24 h between samples, `x`/`y` within ± `MAX_PAGE_SIDE`, width in `(0, 100 000]`; a pressure outside 0..1 is clamped). Ctrl+C / Ctrl+X / Ctrl+V use the browser's copy / cut / paste
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
  not the barrel. Pressing the barrel button of a pen that is not drawing may arrive as a `pointermove` whose `buttons`
  changed, with no `pointerdown` (this is what WebView2 does with the DevTools protocol): it starts the lasso too, and
  releasing the button ends it. The DevTools protocol cannot send the eraser button (it drops bit 32), so the eraser
  end is checked by unit tests and by hand only. The canvas shows no context menu for a pen (Windows turns the barrel button into a right click).
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

## M10: drafts and session end (agent `m10-fix-hard`)

Review finding: the editor kept edits in memory until Ctrl+S, and Windows ending the session lost them silently.
Checked in tao 0.37.2 (`platform_impl/windows/event_loop.rs`, `thread_event_target_callback`): `WM_QUERYENDSESSION`
is not handled (Windows is told yes), and `WM_ENDSESSION` ends the process at once (`std::process::exit(0)`); no
`CloseRequested` reaches the interface, so the leave dialog never shows at shutdown, logoff or a Windows Update restart.

- **Drafts** (`app/src-tauri/src/drafts.rs`, `lib/editor/drafts.ts`): while the note is dirty, the editor hands its
  present state to `draft_write` 2 s after the last change, and no later than 10 s after the first unwritten one
  (`DraftKeeper`). One JSON file per note in `%LOCALAPPDATA%\Ardoise\drafts\<copy>\` (temp folder when unset), the copy
  being `pipe::settings_hash` of the settings location as for the bridge pipe: `draft-<FNV-1a of library + note id>.json`
  holding the note as edited, the library, the file's path and id, the stamp the edits started from, the time and the
  process token (`session`). A background image staged in memory only (imported, not saved) is written beside it once
  per staging key. Every write is atomic (`fsutil::write_atomic`); nothing goes into the library, so drafts never sync.
  The key is the id **of the file** (`noteId`, from `open_note`, or the copy's after "save as a copy"): the edited note
  keeps the old id after a copy.
- **Removed** when the note is no longer dirty (saved, or undone back to the saved version) if this editor wrote it, on
  "leave without saving", on "Discard" in the recovery dialog (asked twice), or once restored as a copy. Never by age.
- **Offered** when the note is opened (`draft_find` by the file's id): `recoveryOf` compares the draft's base stamp with
  the file's: equal → "Recover the changes" puts the draft's note over the opened one as **one undoable step** (Ctrl+Z
  gives the saved version back; the note is then dirty and saved as usual); different (another PC saved it through
  Nextcloud) or unknown → only "Recover into a new note" (`draft_restore_copy`: a new note "<title> (récupérée)" in the
  same binder, else the root, new id, the draft's image; the newer note is not touched), which the editor then opens.
  A draft holding exactly what the file holds (a save whose draft could not be removed) is removed quietly.
  `RecoveryDialog.svelte` is modal; "Later" goes back to the library and keeps the draft. While it is open, the
  keeper holds its writes (`hold`) and the editing keys are off, so nothing written now replaces the earlier draft.
- **On the library screen** (`DraftsBanner.svelte`, one line in `Home.svelte`): `draft_list` gives the drafts of the
  current library written by an earlier run (another `session` token), most recent first: "Open" (the note found by
  path, else by id) or, when the note is gone, "Recover into a new note"; "Discard" asks once more. Reloaded when the
  library listing changes.
- **Session end** (`app/src-tauri/src/session_end.rs`): the editor reports dirtiness through `set_unsaved_work(reason)`
  (the sentence `drafts.shutdownReason`, in the interface language); while set, the main window has a
  `ShutdownBlockReasonCreate` reason (destroyed when nothing is unsaved). The main window is subclassed
  (`SetWindowSubclass`, installed in Tauri's setup on the window's thread): on `WM_QUERYENDSESSION` it emits
  `session-ending` (from another thread, after the procedure returns), on which the editor writes its draft at once,
  and answers FALSE while work is unsaved unless `ENDSESSION_CRITICAL` (Windows then shows "Ardoise is preventing
  shutdown" with the reason; "Shut down anyway" sends `WM_ENDSESSION`, which tao turns into `exit(0)`, the draft
  being on disk by then). A forced end (`EWX_FORCE`, critical) or a crash keeps the last periodic draft.

## M10: one window per copy (agent `m10-fix-hard`)

`app/src-tauri/src/instance.rs`. Rather than `tauri-plugin-single-instance` (MIT/Apache, but keyed by the
configuration's identifier, so one per PC and not per copy, and a second instance that does not find the first one's
window carries on as a second app), a named mutex and a window class:

- The windowed launches (`guarded`: no argument, `--diagnostic`, `--simulated-tablet`) claim
  `Local\ardoise-instance-<user SID>-<copy hash>` (`CreateMutexW`, owned by the main thread for the life of the
  process); the copy hash is the bridge pipe's `settings_hash(settings_key(settings file))`, so two folders with their
  own `ardoise.toml` run side by side, and a copy whose settings live in `%APPDATA%` (unwritable folder) is one copy.
  `--mcp [--read-only]`, `--self-test`, `--create-demo-library`, `--import-inkspace` and `--diagnostic --simulated`
  never claim it.
- The main window's class is `Ardoise-<copy hash>` (`window_classname`). A second launch whose mutex already exists
  looks for that window every 100 ms (`FindWindowW`) and brings it to the front (`ShowWindow` restore,
  `SetForegroundWindow`: the new process was started by the user and may give the focus away), then exits with 0. If
  the mutex is released meanwhile (the first app is closing, or crashed: the mutex is abandoned), it becomes the app;
  after 20 s without a window it exits quietly (the first one is still starting). A second `--diagnostic` launch only
  shows the open window (the tablet test is one click away there).
- Without a settings location or a SID (non-Windows builds), no guard.
