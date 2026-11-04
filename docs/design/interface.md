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

## M10: About and distribution

**The « À propos » screen** (SPEC §9). Settings > *À propos* (`AboutSection.svelte`) shows "Ardoise, version X" and
one line on the licence, with a button that opens `AboutDialog.svelte` in the shared `Modal` (Escape closes it, Tab
stays inside, the focus goes back to the button). It replaces the old version line of Settings. It is not repeated in
the library header, which only holds the Settings button.

- **Content**: name and version; the licence GPL-3.0-or-later with one plain sentence (use, study, change, share; a
  distributed modified version stays free; no warranty) and the full `LICENSE` behind a disclosure button; the credit
  to Tuhi (same wording as `THIRD-PARTY-NOTICES`: its documentation of the protocol guided Ardoise, which
  reimplements it and contains no Tuhi code); Cisco's notice as `OpenH264State.notice` sends it (fallback: the same
  sentence from the locale), the sentence saying that the user downloads OpenH264 from Cisco and that it is never
  shipped, and whether it is installed here; the components (name, version, licence) with a filter field that
  matches every word against name, version and licence (`lib/about.ts`, accents and case ignored) and a "shown of
  total" counter; the full notices behind a second disclosure button.
- **Data**: the exe embeds the repository's own `LICENSE` and `THIRD-PARTY-NOTICES` with `include_str!`
  (`app/src-tauri/src/about.rs`); the component list is parsed back from the notices' two package lists, so the
  screen and the file cannot disagree (a test checks the counts announced by the headings). Two commands:
  `about_info` (version, SPDX licence, components) and `about_text("license" | "notices")` (long texts, fetched only
  when the user asks and kept; an unknown name is a `{ code: "unexpected", message }` error). The notices add about
  0.8 MB to the exe, so they are not compressed.
- **Accessibility and themes**: disclosure buttons with `aria-expanded` and `aria-controls`; the scrollable list and
  texts are focusable (`tabindex="0"`, labelled) so the keyboard can scroll them; the table has column and row
  headers; colours come from the theme variables only, so dark mode follows.
- **Tests**: `AboutDialog.test.ts` (both languages, filter, disclosures, errors), `lib/about.test.ts`, the Rust tests
  of `about.rs`, and the journey `app/e2e/journeys-m10-about.mjs` (`aboutJourneys`, not wired into `run.mjs`).

**`THIRD-PARTY-NOTICES` is complete** (`xtask/src/licenses.rs`): a package that ships no licence text (or only a
NOTICE file) gets the standard text of each licence of its expression, from `xtask/licenses/` (Apache-2.0, MPL-2.0,
and MIT, BSD-2-Clause, BSD-3-Clause templates with a `{holders}` placeholder). The holders on the "Copyright (c)"
line are the manifest authors (names only, e-mail addresses dropped), otherwise "the <repository> contributors" with
the repository address; with neither, the generation fails. Texts identical to one already in the file are merged
into its "Covers" list (Apache-2.0 and MPL-2.0 were), the others are new texts. The file also has a section on the
Cisco OpenH264 binary, which is not shipped. The automatic check (`cargo xtask licenses`, part of `check`) already
covered every package of the resolved Cargo graph (all platforms, dev and build dependencies) and of
`package-lock.json`, a superset of what ends up in the exe, so no gap was found there.

**`cargo xtask dist [--reuse]`** (`xtask/src/dist.rs`): checks the licences, builds the release exe like `check` does
(`--features custom-protocol`; `--reuse` takes `target/release/ardoise.exe` as it is), refuses an exe of 30 MB or
more, and writes `target/dist/ardoise-<version>-windows-x64.zip` (version of `[workspace.package]`) with, under
`ardoise-<version>/`: `ardoise.exe`, `LICENSE`, `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` (French then English, CRLF)
and `guide/*.html`. Each `docs/guide/*.md` becomes a standalone page with `pulldown-cmark` (tables and
strikethrough; links to another guide page rewritten from `.md` to `.html`, fragment kept; a link to a file outside
the guide is left as is and reported as a warning; headings get GitHub-style anchors; one embedded stylesheet,
light and dark, with a print version). The archive is deterministic (entries sorted, fixed timestamps and
permissions, same bytes for the same inputs); the size and SHA-256 are printed. Nothing is uploaded or published.
Tests build an archive from a fake exe and a throwaway guide.

## M10: review fixes (interface)

- **The library stays mounted.** `Home.svelte` keeps the library shell in the DOM (class `shell-hidden`, `display: none`) while the editor or live mode shows over it. A tablet test in progress, the registration wizard, the OpenH264 download, an unsaved Settings draft, the binders opened in the sidebar and the 1,000 cards all survive. `leaveLibrary` notes the scroll position and the card that was opened; `backToLibrary` gives both back. The assistant's `open_note` no longer closes the open panel.
- **Shortcuts** of the library (Ctrl+N, Ctrl+F) are ignored under the editor, live mode and any library dialog; `createdFromFile` ignores a late answer when the dialog is no longer the current one.
- **Editor.** `blocker()` (for the assistant) also refuses during an export, print or video dialog, the split panel and the leave question, and counts a title typed but not renamed as unsaved. The leave question is a `Modal` (`role="alertdialog"`: Tab stays inside, focus returns to the button). A path that changes from outside while the note has unsaved edits asks first (`switching`); a failed rename keeps the editor open with its error. A background image that cannot be loaded says so.
- **Modal.** Escape is heard on `window` by the topmost modal only, wherever the focus is; the card takes the focus back when its focused control becomes disabled (`MutationObserver`).
- **Live mode.** Space is always « next page » (buttons do not keep it; `keyup` is cancelled too so that a focused button is not clicked); Esc closes the projection when open, otherwise asks (a `Modal`, « Continuer » focused); « Clear the page » offers « Annuler l'effacement » for 8 s (the clearing is a step of the page's history in Rust: command `live_restore_cleared`, only while that page is on screen; the ink kept turns with the page); « Fermer quand même » stops the projection and the session; « Terminer » waits for `live_start`.
- **Errors.** `genericError()` (api.ts) composes « Une erreur est survenue. » with the coded phrase made a sentence; `saveFailed` events carry the library error's `code`.
- **Long PDF.** `cancel_create_notes` sets a flag that `notes_from_file_cancellable` checks before and after each rendered page; the notes already made are removed and the call fails with the code `cancelled`. The dialog shows a progress bar and a « Cancel » button while notes are made.
- **Selection with the pen or a finger.** After a lasso made with the pen's barrel button (the tool is still the pen), the tip and a finger move and resize the selection instead of drawing (`barrelPicked` in `Canvas.svelte`; handles are shown); with the selection tool, a finger that would navigate takes hold of the selection when pressed on a handle or inside the frame. A `lostpointercapture` coming from a child (implicit capture of a finger pressed on ink) no longer cancels the gesture.

## M10: library features

Requirements: SPEC §3.4, §4.3 (tag colours, multiple selection), §5 (feedback for long operations, undo). The note
format stays 1.2.

| Command (`organize.rs` unless said) | Arguments | Returns |
|---|---|---|
| `rename_binder` | `binder`, `name` | the new binder path; `binderExists` rather than replacing a folder or file, `binderNotFound` |
| `delete_binder` | `binder` | `BinderDeleted { trashed: TrashEntry[], failed: { path, error: {code, message} }[], kept: string[] }` |
| `tag_colors` / `set_tag_color` | – / `tag`, `color \| null` | `{ [tag in lower case]: colour id }`; `invalidTagColor`, `tagColorsNewer`, `tagColorsConflict` |
| `trash_notes` (`library.rs`, body in `organize.rs`) | `paths` | now `{ trashed: { path, id }[], failed: { path, error }[] }`: every note that can go is trashed, the trash ids serve « Annuler » |
| `inspect_library` (`commands.rs`) | `path` | `{ existing, notes, complete }`: notes counted by a bounded walk (`count_notes`, 20 000 entries) |

- **Turning several notes** (« Tourner » in the selection bar, `lib/library-batch.ts`, `lib/library-activity.svelte.ts`):
  each note is opened (`open_note`), turned by the editor's own `ops.rotateQuarter` and saved (`save_note`, with the
  stamp read; no image key, so the note's background member is kept), one after the other. One implementation for the
  editor and the library: they cannot disagree. Progress with « Arrêter » (stops between two notes), failures listed
  per note with the translated `errorCodes.*` phrase, then the undo notice; undoing turns each note back the other way
  only if its stamp is still the one the turn returned (a note changed since is listed, not touched). Turning back is
  exact up to the rounding of `h − (h − y)` on f32 samples (a fraction of a micrometre). As in the editor, the first
  save that changes the ink keeps the version before it (`original.ardoise`, « Revenir à l'original ») and `modified`
  changes, also when the turn is undone. Operations on several notes run one at a time: one asked meanwhile (an undo)
  waits for the running one. A note opened in the editor while a turn runs is not skipped: the stamp check keeps
  either version from being overwritten silently (the editor reports the conflict).
- **Undo notice** (`ActivityBanner.svelte`): after a turn, « Mettre à la corbeille » (button or Delete) and a binder
  deletion; 8 s each, paused while the pointer or the focus is on them; up to three stay on display (a notice never
  silently takes the place of another). **Ctrl+Z** in the library undoes the latest one (not in a text field, not
  while an editor is open), so that the keyboard need not reach it in time (WCAG 2.2.1). It sits at the bottom of the
  library's scrolling area, after the content (sticky), so it never pushes the cards under the pointer; the progress
  and the failures appear there too. `trash_notes` trashes every note it can and returns the failures; the notes that
  stayed stay selected.
- **Tag colours**: `_ardoise/tags.json` (format in `docs/file-format.md` §6), loaded by the sidebar
  (`lib/tag-colors.svelte.ts`) and on every `library-changed`. Eight palette ids (`lib/tag-colors.ts`); `tag-colors.css`
  gives each a dot, a chip background and a chip text colour for the light and the dark theme (text contrast 6.2:1 at
  least). Shown on the cards' chips, the sidebar's tags and the tags panel; chosen with `TagColorPicker` (sidebar row,
  tags panel), a menu of `menuitemradio` (arrow keys, one Tab stop on the current colour). The watcher only reports
  notes: the colours are read again at every change of the library, when the window comes back to the front, and every
  minute (a colour changed on another PC arrives through Nextcloud in `tags.json`).
- **Binders**: menu « ⋯ » on each binder row (`BinderEditor.svelte`). Rename: the folder is renamed in place
  (`rename_no_replace`; a change of case only works on Windows), the view and the open branches follow. Delete:
  confirmation with the count of notes (the sidebar's count, sub-binders included) and of sub-binders, « Annuler »
  focused first; the notes go to the trash one by one (`Library::trash`, so each is restorable to its own binder),
  then the folders left empty are removed deepest first (`remove_dir` only: other files, hidden files and links keep
  their folder, and the notice names the deepest folders that could not be removed: they still hold files, or are in
  use). « Annuler » recreates the binder and restores its notes (empty sub-binders are not recreated). The focus goes
  back to the binder (renamed or not) or to « Toutes les notes » after a deletion.
- **Settings › change library** (`LibrarySetting.svelte`): the chosen folder is inspected and shown with
  `LibraryFolderFacts` (also in the wizard): existing library or new, number of notes (or « at least » when the count
  stopped); « Utiliser ce dossier » switches, « Annuler » does nothing; the current library picked again changes nothing.
- **Plurals**: `tn(key, count, params)` picks the `one` or `other` form of a catalog entry `{ "one": …, "other": … }`
  with `Intl.PluralRules` (French: 0 and 1 are `one`); `{count}` is optional in a form. The tests check that every
  plural message has exactly these two forms in both languages.

## M10: performance

`cargo xtask perf` (`xtask/src/perf.rs`, `app/e2e/perf.mjs`, method and last table in `docs/performance.md`) measures the SPEC §8
targets on `target/release/ardoise.exe`. Decisions:

- **Split.** The Node script drives the app (a portable copy per run, fresh `LOCALAPPDATA`, DevTools port, same pattern
  as `run.mjs`) and returns raw numbers (times, one frame interval per frame); Rust computes the statistics (mean fps,
  p95, frames over 25 / 50 ms), judges them and prints the table, so that the pure parts are unit-tested.
- **Ready mark.** The interface sets the User Timing mark `ardoise:library-listed` (`lib/perf.ts`, from `NoteGrid`) two
  animation frames after the grid has cards, and each thumbnail `img` gets `data-drawn-at` (`performance.now()`) on
  `load`. Start = spawn (wall clock) to `timeOrigin + mark.startTime`; no behaviour changes.
- **Large library.** `--create-demo-library DIR --demo-notes N` (`app/src-tauri/src/demo/bulk.rs`): the 8 demo notes, then
  copies of them with unique ids (from `…-000000001000`), titles, dates over 18 months, tags and 24 binders. The default
  demo (8 notes) is unchanged.
- **Live mode** runs the simulated tablet (`--simulated-tablet`: 5 ms samples, a 600 s handwriting script) through the
  registration wizard and the sync, then opens live mode, as the end-to-end journey does. Frame intervals come from a
  `requestAnimationFrame` loop in the live window, with WebView2 started with `--disable-features=CalculateNativeWinOcclusion`,
  `--disable-backgrounding-occluded-windows` and `--disable-renderer-backgrounding` so that a window hidden behind another
  is not throttled.
- **Export** is the release benchmark `export_speed_mp4_2min` of `ardoise-media` (Media Foundation back end forced), run by
  `cargo test --release`; it prints a `PERF-EXPORT` line the task parses.
- The numbers depend on the PC and its load: the table records the CPU, cores, RAM, GPU and Windows version.

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

Checked on the development PC (Windows 11, a debug build with the interface embedded; window messages sent with
`SendMessageTimeout` as Windows does at logoff): before the fix (the exe of main 26da011), a dirty note let
`WM_QUERYENDSESSION` through (answer 1, no reason) and `WM_ENDSESSION` ended the process with 0, the change lost after a
restart. After it: a clean note answers 1; a dirty one answers 0, `ShutdownBlockReasonQuery` returns the French reason,
the draft holds the latest change 166 ms after the query (the periodic write would come 2 s later), `WM_ENDSESSION`
ends the process with 0, and the restarted app lists the draft and offers it ("restore"). A process killed 2 s after an
edit (`taskkill /F /T`) leaves its draft, offered and restored after the restart, and removed once saved. A second
launch of a copy exits with 0 within 0.3 s and restores and brings forward the minimised first window; `--mcp
--read-only`, `--create-demo-library` and `--self-test` of that copy work while it is open; a copy in another folder
runs beside it; two launches at the same moment leave one app. Journeys: `app/e2e/journeys-m10-fix-hard.mjs`. Not
checked: a real shutdown or logoff (Windows' own "apps preventing shutdown" screen).

## M10: the library with a thousand notes

- Listing: `Library::list_with(&mut HeaderCache)` reuses the header of every note whose file kept its stamp (the app
  keeps one cache, `library::HEADERS`; `listed_note_id` gives the thumbnails the id without opening the note), and the
  walk takes each note's stamp from the directory entry (no extra open on Windows). `Listing.binders` comes from the
  same walk. The watcher's first scan runs on its thread (`LibraryWatcher::wait_until_scanned`), so the window and
  `set_library` never wait for it; the commands that touch the library run through `notes::blocking`.
- Thumbnails: a store no longer lists the cache folder; the folder is read once per run (`scan_cache`) and the name last
  written for each note file is remembered.
- Grid: one shared `IntersectionObserver` (`lib/near-viewport.ts`), a card menu listens to the page only while open,
  selection lookups use a `Set`, the thumbnail cache is sized for the library (400 to 2000 pictures, 64 million
  characters at most).
- Unreadable files: each problem carries its error `code` (`errorCodes.*`, like the library errors) and shows in a
  banner above the grid.
