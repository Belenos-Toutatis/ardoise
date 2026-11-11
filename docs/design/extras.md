# Extra features (milestone M7, P2)

Developer document (English). Requirements: SPEC §4.3 (search in recognised text), §4.4 (straighten shapes,
background image or PDF, back to the original), §4.6 (drag a note out of the app), §4.7 (offline handwriting
recognition, French and English), §6.2 ("Écriture" row). The Windows pen (tablet as a system pen) was dropped at the
user's request on 2026-10-25: there is no `ardoise-winpen` any more.

Done criteria (ROADMAP M7): `CHECK OK`; every feature tested; recognition checked on reference notes with the rate of
recognised words printed.

## Shared contract (committed before the work started)

- **Format 1.2** (`docs/file-format.md`, fixture `testdata/format/v1/full-1.2.ardoise`):
  - `Note::background_image: Option<BackgroundImage>` (`crates/ardoise-core/src/note.rs`): file name, PDF page,
    `rect` (where the image is stretched to, page coordinates, may reach outside the page), `quarter_turns`
    (clockwise, applied to the stored pixels first), `opacity`. The pixels are `NoteFile::background`
    (`background.png` or `background.jpg`). `ImageRect::fit(page, pixels, ImageFit)` computes the rectangle of an
    import (contain / cover / stretch); the note keeps only the rectangle, so that page operations can move the image
    exactly like the strokes.
  - `NoteFile::original_note` (member `original.ardoise`): the note before its first edit of the ink, a complete
    `.ardoise` archive (read it with `read_from_bytes`), for notes that have no `original.bin`.
- TypeScript mirror: `BackgroundImage`, `ImageRect`, `Note.backgroundImage?` in `app/ui/src/lib/note.ts`. The
  interface never holds the image's pixels: Rust renders the page background with them.

## Ownership (parallel work, one worktree each)

| Part | Owner | Files (main ones) |
|---|---|---|
| Handwriting recognition, search, text export | agent `hwr` | `crates/ardoise-hwr/`, `app/src-tauri/src/handwriting.rs` (new), settings field, library search, text export |
| Straighten shapes; back to the original | agent `edit` | `crates/ardoise-ink/src/shapes.rs` (new), `crates/ardoise-ink-wasm/`, editor actions, `save_in` snapshot, `note_original` command |
| Background image or PDF | agent `background` | import (image decode, PDF pages), `ardoise-render` (PNG/JPG/SVG/PDF/canvas), `ardoise-media` frames, thumbnails, page operations (Rust `ops.rs` and `lib/editor/ops.ts`), page panel, "new note from a PDF or an image" |
| Drag a note out | agent `drag` | native drag source, library card handle |

Shared files everyone may append to (merged by hand): `app/src-tauri/src/lib.rs` (command list), `app/ui/src/lib/api.ts`
and the `mock-*.ts` stand-ins, `locales/fr.json` and `locales/en.json` (each part adds keys under its own section),
`CHANGELOG.md` (one line per feature). Nobody else edits another part's main files.

## Handwriting recognition (`hwr`)

- Engine: Windows' `InkRecognizerContainer` (`Windows.UI.Input.Inking`, works in desktop apps). Strokes are built
  with `InkStrokeBuilder` from the note's points (page units → DIPs; pressure kept), recognised with
  `InkRecognitionTarget::All`; each result gives words with candidates and a bounding box. Language: the installed
  recognizers (`GetRecognizers`) are listed; the user picks French or English in the settings (default: the interface
  language when its recognizer is installed, else the first installed one). The English recognizer exists only if
  Windows' English handwriting pack is installed: say so, with where to install it, never download anything.
- `ardoise-hwr`: a `Recognizer` trait (so that the indexing logic is tested on Linux with a fake), the Windows
  implementation, `cfg(not(windows))` → "unsupported". Recognition runs on a worker thread of its own (WinRT
  async completed with `.get()`), never on the UI thread (threads and priorities: below).
- **The notes are not written for recognition.** Results are a per-PC cache in `%LOCALAPPDATA%\Ardoise\handwriting\`,
  keyed by note id and a hash of the strokes (+ language), so a note edited on another PC is recognised again and
  two PCs never compete to rewrite the same file through Nextcloud (conflict copies). The `text.json` member stays
  unused (reserved).
- **Strokes reach the engine in the order they were written**, not the file's (`recognizable_indices`): the engine reads them
  in the order it is given them, and a retouched copy keeps a letter put back in its word at the end of the file, with the
  right time. Read in the file's order, a corrected copy lost words and had "dé deva des ré" at the end. A note whose strokes
  overlap too much in time (more than 1 in 20: one pen writes one stroke at a time, so these times are no order of writing,
  such as lines with a clock each) stays in the file's order: sorted, the engine reads nothing. Real notes have 2.4 % at most.
  A note already in order keeps its hash and its entry, the others are recognised again once (`files.json`, format 2, is
  worked out anew). A
  transcription corrected with `set_note_text` on a note out of order is keyed by the old hash, so it is dropped once; there
  were none when this changed on the development PC, and the ink it was written for can be read again.
- Indexing: in the background, notes whose cache entry is missing or stale; paused while live mode or the
  presentation runs (their latency budget is 30 ms); progress visible; cancellable; never blocks the library.
- **Threads and locks** (review fix I1 of M7: a priority inversion between the indexer and live mode):
  - Windows' locks (`SRWLock`, behind `std::sync::Mutex`) have no priority inheritance, and Windows boosts a starved
    thread only after seconds. So a lowered priority is only for a thread that takes no lock shared with others.
  - The indexer's thread (`ardoise-handwriting-index`) runs at **normal** priority: it takes locks that the
    interface's commands take too (its status and cache locks; Tauri's state manager and the settings when it reads
    them; the main window when it reports its progress). The engine's worker thread (`ardoise-handwriting` of
    `SystemRecognizer::background`) runs at background priority (`THREAD_MODE_BACKGROUND_BEGIN`, CPU and disk): it
    only talks through channels.
  - "Live mode or the presentation runs" is read without any lock from `live::Activity` (atomics): the hub sets
    `live` in `Shared::publish`, which every change of the session goes through (start, stop by the user, the
    tablet service leaving live mode, a failed start, the app closing, a stop whose writes fail); the presentation
    window takes a token when it is created and gives it back on its own `Destroyed` event (a late event of an old
    window cannot clear a new one; a new window that cannot be placed is destroyed rather than left hidden). The
    indexer asks before every note (and while reading cache entries back after a restart), and every 500 ms while
    paused.
  - No indexer lock is held across a disk access, a recognition or a call into the app: the cache is split into its
    memory state (`HandwritingCache`, behind the lock) and its disk access (`CacheDir`); `files.json` is read on
    the indexer's thread, not on the main thread at start-up. In debug builds a per-thread lock counter checks it
    before each of these calls (`unlocked`), and the tests run every path. The UI-thread commands
    (`handwriting_status`, `handwriting_stop`, `handwriting_resume`) only take the status lock, never held long.
  - Where the engine works (measured on this PC, Windows 11, 12 logical processors,
    `cargo test -p ardoise-hwr --test engine_threads -- --ignored --nocapture`): `RecognizeAsync` does about 10–20 %
    of the CPU work on the calling worker thread, the rest on a WinRT thread-pool thread and on the engine's own
    inference threads (`session-N-intra-op-K`), all at **normal** priority. A 4-line note takes about 250 ms and
    140 ms of CPU, a dense 16-line page 3–6 s (about 1.2 processors busy on average). The worker's background
    priority therefore does little; what keeps recognition away from live mode is that **no recognition starts
    while live mode or the presentation runs**, so at most the note under way finishes after live mode starts.
    Cancelling it (`IAsyncInfo::Cancel`) was tried: it answers within 100–250 ms, but releasing the
    `InkRecognizerContainer` afterwards crashed the process (access violation), so it is not used. Lowering the
    priority of the engine's threads from outside (they are not ours, and the thread pool is shared with WinRT's
    Bluetooth callbacks) or running the engine in a background-priority child process were not retained; the
    latter remains the option if weaker PCs show contention.
  - Measured (`handwriting::tests`, simulated tablet → tablet service → hub, real engine): with 24 notes being
    indexed when live mode starts, the indexing paused 234 ms later (the note under way, 227 ms of overlap), live
    latency median 0.15 ms, p95 0.31 ms, max 2.6 ms; afterwards the indexing resumed and ended. The ignored
    `live_latency_with_and_without_indexing` compares 10 s of live mode alone (median 0.13 ms, p95 0.19 ms),
    with the indexing paused (0.13 / 0.19 ms) and with the indexing going on regardless (37 notes recognised
    meanwhile: 0.08 / 0.12 ms): on this PC the Rust side does not feel the engine; the pause protects the
    interface's drawing and smaller PCs.
- Search: the library search also matches the recognised text (accents and case ignored, like titles), shows why a
  note matched (a short excerpt), and says when indexing is still running or when no recognizer is installed.
- Text export: a note's (or a selection's) recognised text, as `.txt` (UTF-8) or copied to the clipboard, from the
  same places as the other exports.
- Test: reference notes with known text (publishable, synthetic: handwriting-like single-stroke glyphs, never the
  user's notes), French and English; a Windows test prints the rate of recognised words per language, with a
  threshold written in the test. On a PC without a recognizer for a language, that language is skipped with a message.

## Straighten shapes (`edit`)

- `ardoise_ink::shapes`: recognise a hand-drawn stroke as a straight line, circle or ellipse, rectangle (possibly
  turned) or arrow (shaft and head, in one stroke or in the strokes of the selection), and return the straightened
  stroke(s): same tool, colour, width, layer, a pressure that keeps the stroke's look, times spread over the original
  duration (the replay still draws it). A stroke that is no clear shape is left as it is. Pure Rust, deterministic,
  exposed through `ardoise-ink-wasm` so the editor applies it synchronously and it is one undoable step.
- Handwriting versus drawings (the editor straightens whatever the user selected, as one undoable step, and says how
  many shapes it straightened). A handwritten o, O, 0, Q or D is a closed stroke like a circle, so closed shapes follow
  their own rule (`ardoise_ink::shapes`, "Handwritten loops"): a long side of 13 mm at least (`MIN_CLOSED_SIZE`; lines
  and arrows keep 10 mm); and an ellipse that stands up (long axis within 40° of the vertical), is clearly not round
  (short ÷ long axis below 0.9) and whose long axis is under 25 mm (`LETTER_MAX_AXIS`) is taken for an O or a 0 and left
  alone. Measured on random hands (400 to 500 per case, `cargo test -p ardoise-ink measure -- --ignored --nocapture`):
  no o, O, 0, Q or one-stroke D of 4 to 12 mm is changed; a capital O or Q of 13 to 22 mm in 16 % of the cases (the
  rounder-than-0.9 ones), a 0 and a D never; circles of 15 mm and more are recognised in 99 % of the cases, wide
  ellipses in 94 to 99 %. Accepted on purpose: a round loop of 13 mm or more is a circle (about half of the lower-case
  o written 15 mm high); a tall oval of 13 to 25 mm drawn as a diagram's ellipse is not recognised (the notice for a
  selection without shapes says that the drawings, not the text, should be selected; such an ellipse works drawn wider
  than tall or larger); a Q written above 20 mm gets its 1 cm straight tail straightened like any single straight stroke of 1 cm
  or more (I, l, 1), which is a harmless change. The alternative considered, a flat 15 mm minimum for every closed
  shape, was not enough: a capital O 15 to 20 mm high is still an ellipse by every measure.
- Editor: "Redresser les formes" on the selection (Selection tool), with its help text; nothing selected = the
  action is disabled with an explanation.
- M15 makes the thresholds a user setting (tolerance, minimum size, alignment): see "M15: tuning the straightening"
  at the end of this document.

## Back to the original (`edit`)

- Source of the original: `original.bin` (tablet pages: decoded again with `ardoise_device::pages::note_from_page`,
  the function the synchronisation uses), else `original.ardoise`.
- `save_in` (`app/src-tauri/src/notes.rs`) writes `original.ardoise` once: when it saves over a file that has neither
  member, it first keeps the version on disk. Never rewritten afterwards.
- Command `note_original(path)`: the original's page, layers and strokes, and for a first version the placement of
  the background image it had (`backgroundImage`; title, tags, favourite, paper and narration stay). Editor:
  "Revenir à l'original" with a confirmation, as one undoable step (saving keeps the history's behaviour); absent
  when the note has no original.
- **What the original is.** `save_in` keeps the version on disk when a save changes the ink (page, layers or
  strokes) of a file that has neither member and whose version on disk has ink. So for a note created empty in
  Ardoise (or the blank file of a "save as a copy"), the original is the first version saved with ink, not the
  empty page; the help text says "la première version enregistrée qui en contenait".
- **The background image on the way back** (`lib/editor/original.ts`, `imageForOriginal`). The image itself (its
  pixels) never comes back from the original: `original.ardoise` describes the image the note had, but does not hold
  its member, and a tablet page has none. The current image stays, placed for the restored page:
  - the same image as the original's (same `name` and `pdfPage`): its `rect` and `quarterTurns` become the
    original's, so that a rotation or a crop followed by a save and a revert puts it back exactly; its opacity stays;
  - any other case (another image, an original without one, a tablet original): page turns are the only thing that
    turn an image, and the revert undoes them, so "the same image, unrotated" means the image upright as its file
    shows it, `quarterTurns` 0. When it was turned or the page size changes, it is placed again on the restored
    page with the placement `currentFit` recognises on the current page (contain, cover or stretch), or contained
    when a crop moved it; otherwise it stays where it is. Without the pixels' size (the image could not be staged),
    its rectangle turned back stands for it;
  - an image removed since stays removed.
  The confirmation says so in a sentence.
- **Limit of a tablet original.** `original.bin` is decoded again with today's settings: the hand (orientation) of
  the settings and the drawing area of the tablet registered in the library (the Bamboo Slate's when none is). A
  note synced with the other hand, or from a tablet of another size, comes back turned or scaled in today's
  geometry. The format is not changed for this (the bytes alone do not say how they were decoded); the help text
  says in a few words that the orientation chosen in the settings is used.

## Background image or PDF (`background`)

- Import: PNG and JPEG (decoded in pure Rust so that the tests run on Linux), PDF pages through Windows'
  `Windows.Data.Pdf` (`cfg(not(windows))` → "unsupported"), rasterised once at import. Stored pixels: at most
  4096 px on the long side; `jpg` for photos, `png` otherwise (transparency, few colours). A PDF page keeps its own
  aspect ratio; "new note from a PDF or an image" makes one note per chosen page with the page size of the PDF page
  (1 pt = 1/72 in), and the editor can put an image or a PDF page under an existing note (contain / cover / stretch,
  opacity, remove).
- Drawn everywhere the background is drawn, in this order: paper colour, template, image (with its opacity), strokes:
  the editor and live view (`render_background`), thumbnails, PNG/JPG/SVG/PDF exports (SVG/PDF embed the raster),
  clipboard, automatic export, video frames, presentation if it shows a note's background.
- Page operations move it with the strokes, in Rust (`ardoise_core::ops`, `app/src-tauri/src/operations.rs`) and in
  the editor (`lib/editor/ops.ts`): quarter turn (rect turned, `quarter_turns` + 1), crop (rect shifted), split (the
  new note gets the image and its member), assembly (the first note's image, moved like its strokes; the others' are
  dropped, and the dialog says so). Every note file created from another copies the `background.*` member.
- The editor's undo history stays consistent with the file (no orphan image that reappears, no image lost on save).

### How it is built

- **Pixels and the renderer.** `ardoise_render::picture` decodes a note file's `background.*` member once into a
  `Picture` (the encoded bytes and the RGBA pixels). The renderer takes `NoteView { note, picture }` wherever it took
  a note (`render_note`, `PageCanvas::new`, `note_to_svg`, `notes_to_pdf`, `FrameRenderer::with_picture`,
  `export_animation`); a plain `&Note` converts into a view without pixels, so callers that never have an image
  (golden images, Inkspace comparison, tablet pages, live notes) did not change. The image is part of the page
  background: drawn only with the paper (`Backdrop::Paper`, `background: true`), never on a transparent PNG or
  with the background off. Raster: a tiny-skia pattern over the visible part of the rectangle, after the stored
  pixels are reduced by area averaging when they are more than 1.5× larger than drawn (thumbnails do not
  shimmer). SVG: the stored file (PNG or JPEG) in a data URI, mapped by a `matrix()` from its pixels to the page,
  in a group clipped to the page (drawn back by resvg in the tests). PDF: a JPEG in grey or RGB is embedded as it
  is (`/DCTDecode`), anything else as deflated RGB with an 8-bit `/SMask` when some pixels are not opaque; the
  opacity is the ExtGState `/ca`.
- **Import** (`picture::import_image`, `import_page`): PNG and JPEG are decoded in pure Rust (`png`, `jpeg-decoder`)
  after their header is checked: an empty image or one of more than 50 megapixels is refused before any pixel is
  decoded (`backgroundTooLarge`); large JPEGs are first reduced by the decoder's 1/2–1/8 scaling. EXIF
  orientations are undone. Stored pixels: at most 4096 px on the long side; JPEG (quality 90) for an opaque image
  of more than 256 colours, PNG otherwise; a JPEG that needs no change keeps its own bytes; a rendered PDF page is
  always PNG. A note's member is never larger, so `Picture::decode` (members read from a note file) does not
  decode one of more than 4096 px a side at full size: a JPEG is decoded at 1/2 to 1/8 by the decoder (then
  reduced if still too large, and embedded in a PDF as RGB rather than as its JPEG bytes), a PNG is refused at its
  header. The import keeps its own limits (50 megapixels).
- **PDF pages** (`app/src-tauri/src/pdf_pages.rs`): `Windows.Data.Pdf` reads the PDF from memory. `PdfPage::Size`
  is in DIPs (1/96 in), turned into points. `PdfPageRenderOptions` sizes are DIPs too, multiplied by the
  display's scale: a 100-DIP render first measures that scale, so that a page comes out at 300 dpi (at most
  4096 px) whatever the screen (verified at the PC's scale: 2551 × 3508 px for an A4 portrait page). A PDF that
  needs a password to be opened fails to load: `pdfEncrypted` (`HRESULT_FROM_WIN32(ERROR_WRONG_PASSWORD)`, tested
  with a PDF encrypted by `lopdf`). A PDF protected only by an owner password (permissions) loads and renders, as in
  any reader (tested on Windows 11: `IsPasswordProtected` is false then; the check of that property after the load
  was dropped, it could only refuse such PDFs). A PDF is loaded once (`PdfFile`) for all the pages an import
  renders. Elsewhere than on Windows, `pdfUnsupported`.
- **The editor's staging area and its undo history** (`app/src-tauri/src/background.rs`, `lib/editor/image.ts`).
  The interface never holds pixels. Rust keeps, in memory, every image the editor imported (`stage_background_image`)
  and the image of the note it opened (`stage_note_background`), under a key: a hash of the stored bytes. The
  editor's `Doc` (what undo and redo restore) holds the note and `backgroundPixels { key, width, height }`
  together, so every image in the history comes with its own pixels. The key goes back to Rust with everything
  that needs them: `render_background` (the editor's and the replay's page), `save_note`, `split_note`, and the
  export, copy and video sources (`ExportSource.imageKey`). Nothing is written to the library before the user
  saves. On save, `save_in`'s thumbnail line calls `background::settle`: a note whose image was removed loses the
  member, a note with an image gets the staged member of its key (`backgroundMissing` when the key is no longer
  staged, checked before anything is written; the thumbnail is drawn from the staging area's decoded pixels, not
  by decoding the member again), a save without a key keeps the file's own member, and a member no image
  describes (written by another version) is kept. A save (or split) without a key happens only when the editor
  could not stage the note's image (member missing or unreadable): it may describe the same image as the file
  (same `name` and `pdfPage`) placed anywhere and at any opacity, and is refused with `backgroundMissing` when it
  describes another image or an image the file does not have (a "save as a copy" into the blank file, for
  instance). Replacing an image, saving, undoing and saving again
  therefore writes the first image back from its key. The editor empties the staging area when it opens another
  note and when it goes away, in both cases once its queued writes are done (a "save as a copy" queued just before
  leaving still finds its pixels); going back to the library also waits for them, and an editor opening a note
  waits for the release of the one torn down before it. A copy whose save fails sends its blank new file to the
  trash. The commands that read, decode or write a note (`save_note`, `split_note`, `merge_notes`,
  `note_original`, `open_note`, `note_outlines`, `note_thumbnail`) run on blocking threads. `save_in` keeps its signature (the snapshot block of
  "back to the original" shares it): the command's choice reaches `settle` through `background::with_staged`, a
  thread-local set for the duration of the call; after merging, it can become a parameter of `save_in`.
- **Page operations.** Editor: `rotateQuarter` turns the rectangle like the points and adds a quarter turn,
  `cropTo` shifts it (`lib/background.ts`); a split gives both notes the image (`split_in` writes the staged
  member, or the file's, into the new note, and saves the kept part with the same key; a new note without an
  image, because the editor removed it, gets no member), "save as a copy" saves the copy with the same key. Assembly: the first note's image, which does not move (the first note stays at the
  top-left corner); when it reaches beyond the first note's page on a page that grew (end to end, or an overlay of
  pages of different sizes), its pixels are cut to that page (`picture::crop_to`, rounded out to whole pixels) so
  that it does not cover another note. The dialog says how many other notes' images are left out
  (`background_image_names`).
- **New notes from a file** (`create_notes_from_file`): a PDF gives one note per chosen page (all by default, or a
  list such as `1-3, 5`), at the page's size (1 pt = 2540/72 device units, kept between 1 cm and 100 m), titled
  `<file name> - page <n>` (without the page number for a one-page PDF), one millisecond apart so that the
  library sorts them in page order; an image gives one note as large as fits in an A4 sheet of its orientation,
  in its proportions, titled after the file. The image fills the page (`ImageFit::Contain` on a page of its
  proportions). Stops at the first page that fails; the notes made before stay (each is complete). The command
  reports its progress through a channel (`{ done, total }`, before the first note and after each), shown by the
  dialog for more than one note. One note is opened in the editor right away; several are announced.
- **Error codes** (`errorCodes.*`): `backgroundUnsupported`, `backgroundTooLarge` (an image of more than 50
  megapixels or 256 MB, a PDF of more than 512 MB or 10 000 pages: its sentence names both),
  `backgroundDamaged`, `backgroundUnreadable`, `backgroundMissing`, `pdfUnsupported`, `pdfEncrypted`,
  `pdfNoSuchPage`.

## Drag a note out (`drag`)

- A native drag source (OLE `DoDragDrop`, files `CF_HDROP`), so that a note dropped on the Explorer, the desktop, Word,
  OneNote or a mail becomes a PNG of the note, named after it (rendered to a temporary folder of the app, cleaned up
  later). Several selected notes give several files.
- The library's internal drag (moving notes to a binder) keeps working: the drag out starts from its own handle on
  the card (with a tooltip), or from wherever the agent shows is unambiguous; explain the gesture in the interface.
- `cfg(not(windows))`: the handle is hidden (or does nothing) and the logic that prepares the files is tested on Linux.

### As built

- **Choice: own OLE source, not `tauri-plugin-drag`.** The plugin would add new crates, a JS package and a capability
  (not in the offline cache, so unchecked here), and we want control of the guard against a released button, of the data object and of the drag image. The workspace already compiles `windows` 0.62 (BLE, media): `drag_windows.rs` uses
  it directly (about 150 lines of code), plus `windows-core` for the `implement` macro (same version, no new crate in
  `THIRD-PARTY-NOTICES`).
- **Two commands** (`app/src-tauri/src/drag.rs`). `prepare_note_drag(paths)` renders each note with `export::image_bytes`
  (PNG, 150 dpi, paper and background image; 96 then 72 dpi for a note too large for 150) into a fresh `%TEMP%\Ardoise\drag\<uuid>\`, named
  like the export of several notes (`export::load_sources` stem, `write_new_file`: ` (2)` for duplicates), on a blocking
  thread. `start_note_drag(files)` refuses any file that is not directly inside a drag folder, then runs
  `drag_windows::drag_files` on the main thread (`run_on_main_thread`) and answers `dropped` / `cancelled` when the OLE
  drag ends. Other platforms: `note_drag_supported` is false and `start_note_drag` fails with `dragUnavailable`.
- **Why two steps.** `DoDragDrop` must start while the button is down, and rendering takes time. The interface
  (`lib/drag-out.ts`) calls `prepare_note_drag` when the pointer goes down on the handle, starts the system drag when
  the pointer has moved 6 px **and** the files exist, shows "Préparation…" on the card while it waits, and does
  nothing if the button is released first. `drag_files` itself cancels at once when the main button is not down (and its
  drop source cancels if the first `QueryContinueDrag` sees no button): a drag must never drop under a released pointer.
- **Data object**: the shell's own (`SHCreateDataObject` over the files' item IDs): `CF_HDROP`, "Shell IDList Array",
  file names, and storage for the drag image. Copy only (`DROPEFFECT_COPY`). The pointer carries the first note's picture
  (128 px, `IDragSourceHelper::InitializeFromBitmap`), best effort.
- **Clean-up**: `drag::start_cleaning` removes drag folders older than one hour at the start and every ten minutes.
  Nothing is deleted during or right after a drag. The files are never in the library.
- **Interface**: the handle is a button of its own on the card (not inside the draggable `.card-open`, so the HTML5 drag
  to a binder is untouched), visible on hover/selection, with a tooltip naming the destinations; on a selected card it
  carries the whole selection (`LibraryState.dragOutPaths`). The selection bar's second help line explains it.

#### Checking a drop by hand (Windows)

1. Open the demo library (`ardoise.exe --create-demo-library DIR`), hover a card, press its handle and drag onto an
   Explorer folder: `<note file name>.png` appears, a real image of the note.
2. Select two cards, drag one's handle: two files. Drop on the desktop, into a Word document (inline picture), a mail
   draft (attachments) and OneNote.
3. Escape during the drag, or release over a program that refuses files: nothing is copied, no error shows.
4. Press the handle and release at once: nothing is dragged. Drag a card by its thumbnail onto a binder: still moves it.
5. `%TEMP%\Ardoise\drag\` holds one folder per drag; they disappear within about an hour and a restart.

## Verification on Windows (before ticking M7)

- `cargo xtask check` and `cargo xtask e2e` on the merged branch.
- Recognition: the reference-notes test with the printed rates; a real note of the demo library found by a word in
  its handwriting.
- Background: a PDF of several pages imported, annotated, rotated, exported (PNG, PDF, video); a photo as background.
- Original: a tablet note (simulator) and a live note, edited, then back to the original.
- Drag: a card dropped into an Explorer folder gives the PNG (checked by hand or by a test that reads the data object).

## M8: text export with structure (Markdown and Word)

Next to the plain `.txt` (and the clipboard), the export dialog's "Texte reconnu" section saves a note's handwriting as
**Markdown** (`.md`) or **Word** (`.docx`) with the structure drawn from the ink: the note's title, its date and its
tags at the top, then the paragraphs, the list items (bulleted) and the drawings (pictures), in reading order. Several
notes go one after the other (a rule between two notes in Markdown, a new page in Word).

- **Where the structure comes from.** Windows' ink analysis, `Windows.UI.Input.Inking.Analysis.InkAnalyzer`, **does
  work in a desktop app outside any UWP package** (tried for real on this PC, Windows 11, no package identity): it
  gives a tree `Root > WritingRegion > Paragraph > Line > InkWord`, with `ListItem` (holding an `InkBullet` and a line)
  and `InkDrawing` nodes (a circle and a square were found as such), each with its bounding box and the ids of its
  strokes. `ardoise-hwr` drives it on the same worker thread as the recognizer (`Recognizer::analyze`, answering
  `Unsupported` elsewhere); the tree is flattened to `Region`s (paragraph, list item, drawing; stroke positions mapped
  back through the ids). One analysis of a one-page note takes about 2 s (the first one on a PC a little more).
- **The words are those of the plain text.** The Markdown and Word exports give the words of the `.txt` export and the
  clipboard: the recognition of the whole note (the cache the search uses), or the user's corrected transcription. The
  analysis gives the structure only. M8 first read each paragraph and list item again on its own (`read_regions`), on
  the grounds that the synthetic test page came out better that way ("nousirons" read as "nous irons"); on a real note
  of the user (2026-10-29) the opposite happened: the copied text had the right words and the Markdown misread several
  of them, and the per-region reading also ignored the user's corrections. So (`structure::from_regions`):
  - each recognised line goes, in the engine's order, to the paragraph or list item whose rectangle holds the largest
    share of it (at least 20 %, or its middle; the region taken a quarter of a line larger on each side, as the two
    engines draw their rectangles differently; the smaller region on a tie); a line spread over two regions is not split
    (it stays as the plain text has it). Lines no region holds are laid out by the fallback's rules below;
  - a region in which the recognition has no word at all is read on its own (`read_regions`), with the recognizer of
    the chosen language (`InkAnalyzer` has no language setting and reads with the system's); if that fails too, the
    analyzer's own text stands in. A region with a word of a line that went to another region (a title and a date on
    one row: one recognised line, two regions) is neither read again nor given the analyzer's text: its words would come
    twice. A list item loses its dash or number, and a first word made of signs only (the bullet
    the analyzer found, read as "→" or "°");
  - what the recognition read in a drawing's ink, outside every paragraph and list item ("o" for a circle), is
    dropped: the drawing's image shows it.
- **Corrections** (`set_note_text`, MCP; `handwriting/cache.rs`, `Indexer::correction_of`). The corrected text always
  wins, in the `.txt`, the clipboard and the preview (blank lines kept) as in Markdown and Word (`NoteReading`,
  `handwriting.rs`; the MCP server's `export_note` too). `structure::lay_correction` lays it over the recognised lines:
  when both have as many lines (blank ones aside) each corrected line takes the place of its recognised line, its words
  the rectangles of the recognised words when there are as many, else the line's width shared by their letters; then the
  structure is built as above, nothing is read again, a region the user's lines do not reach is left out, and no
  corrected word is dropped. When the lines do not match, the corrected text is exported as it is, in plain paragraphs
  (blank lines separate them, a line starting with a dash or a number is a list item), placed where the text was, the
  drawings around it. A note with a correction is exported even on a PC without a recognizer (the correction, without
  positions; without the analysis either, nothing tells its handwriting from drawings, so it has none). Limit: lines are
  matched by their number only; a correction that merges two lines and splits another keeps the right text, but some
  of its words may land in the neighbouring block or take a neighbour's underline.
- **Underlines** (`structure::decoration_of`). A thin, wide group of strokes (the analyzer's drawing or unclassified ink,
  or a group of the fallback's uncovered ink) right under a line or through it is no drawing: at most half the line's
  height tall (a line tall when six times wider than tall: a sloping underline), three times wider than tall and wider
  than 0.4 line; its middle below 64 % of the line's height (from the top) and its top at most half a line under the line
  is an **underline**, between 20 % and 64 % a **strikethrough**. It marks the words it covers (each by half its width at
  least, together under at least half of the mark: one much longer than the words is a rule or an arrow), on the nearest
  line; a "word" whose rectangle is the mark's (the recognizer's reading of it, "_") goes. The boundary is set for lines
  with ascenders and descenders (small letters' middle at about half the line, baseline at two thirds); on a line
  without descenders a strikethrough is taken for an underline, the safer mistake. Marks go in the text model (`document::Text`: spans with a `Mark`): Word underlines the
  words (`<w:u w:val="single"/>`) or strikes them (`<w:strike/>`); Markdown has no underline (`<u>` is raw HTML that
  many viewers strip), so underlined words get **strong emphasis** (`**…**`: a hand underline marks what matters) and
  struck ones `~~…~~` (GitHub, Obsidian, Typora). A mark over no word, or far from any line, stays a drawing. Without
  the analysis, a strikethrough is hardly ever found: its middle lies in a word's rectangle, so the fallback counts it
  as read text, not as a mark; underlines (below the words) are found.
- **Where the drawings go** (`structure::reading_order`). The blocks of text top to bottom (then left to right); each
  drawing after the last block it comes after: a drawing **beside** a block (they overlap vertically by 30 % of the
  smaller height and horizontally by at most 20 % of the narrower width) comes after it when it is on its right, before
  it on its left; otherwise after the blocks above its middle. So a figure drawn right of a paragraph follows that
  paragraph (even when its top is higher), one as tall as two paragraphs follows the second, and a list beside a figure
  is not cut by it. Markdown (and the Word export, one column) cannot place a picture beside a paragraph: CommonMark has
  no floats or columns, an HTML table or `<img align>` is raw HTML many viewers strip or render differently, and the
  file must stay plain text the user can edit. After the paragraph is the closest a single column gets.
- **"Inclure les dessins"** (export window, `TextExportSection.svelte`). Markdown and Word include the drawings unless
  the option is off. It is on by default only when the notes have drawings: when the window opens, for five notes at
  most, `structured_text_drawings(sources) -> number` builds their structures and counts the drawings (the option shows
  "Recherche des dessins…" meanwhile, and the Markdown and Word buttons wait, as an export started then would analyse
  the same pages again; none: off and greyed, "Aucun dessin trouvé"). The structures are kept in memory
  (`textdoc::Structures`, the last 8, keyed by note id, strokes, visible layers, recognition and correction), so the
  export that follows does not analyse the page again (about 2 s a page). For more notes, or when the search fails, the
  option stays on (a note without drawings exports the same either way). `export_structured_text` takes
  `includeDrawings` (absent: on); the drawings' words are dropped either way (they are the drawing's ink).
- **Fallback without the analysis** (`structure::from_recognition`, platform-free, tested on Linux): lines whose gap to
  the previous one exceeds one line height start a paragraph; a line starting with a dash, bullet, star or number
  (`strip_bullet`; "-5" is a number, not a bullet), or with a small unread mark drawn to its left, is a list item that
  takes the following indented lines; ink not under any recognised word, grouped by proximity (4 mm, or half a
  line), is a drawing (marks under 2 mm are dropped) or an underline (above). It is weaker: the recognizer reads drawings as stray words
  ("J", "o") that hide them, so a circle drawn among text is often missed. The dialog says so when a note's layout was
  guessed (`handwriting.export.guessed`).
- **Drawings** are cropped from one rendering of the note that holds only the drawings' strokes (the text next to a
  drawing does not creep in): ink on white (the paper stays on a dark paper so that light ink remains visible), 200
  dpi, 2 mm of margin, PNG. Highlighter marks lying over a drawing belong to it. Markdown: see "Where the
  Markdown drawings go" below. Word: embedded, at their real size,
  reduced to the 17 cm text width (and 24 cm height).
- **Files.** `ardoise_hwr::markdown` (escaped text, `-` lists, `![Dessin 1](…)`), `ardoise_hwr::docx` (minimal Office
  Open XML written with the `zip` crate: content types, relationships, styles Title / Subtitle / ListParagraph, one
  bullet list definition, `docProps/core.xml`, A4 portrait, `pageBreakBefore` on each note after the first). The words
  in the files ("Créée le", "Étiquettes", "Dessin") are translated by the interface and sent with the request
  (`TextLabels`); the date is local time, `2026-10-22 14:05`. Platform-free and tested on Linux: the structure, both
  writers (the docx read back with `zip` and `roxmltree`: every part well formed, every relationship resolved).
- **Where the Markdown drawings go.** The save dialog only confirms replacing the `.md`, so no other file of the
  user's is ever replaced (the rule of `export.rs`: ` (2)` rather than overwriting). The drawings are written in a
  **folder next to the `.md`, named `<name>_images`** (language-neutral: no new label), as `<name>-1.png`… (numbered over
  all the notes), before the text, atomically; the links are relative to that folder and percent-encoded
  (`![Dessin 1](Mon%20journal_images/Mon%20journal-1.png)`). The folder is **reused only if it does not exist, or holds
  nothing but `<name>-<number>.png` files** (the drawings of a previous export of the same document: they are
  replaced, and the ones a longer previous export left are removed once the text is written; an export without any
  drawing empties and removes that folder). Otherwise (the user's own files or sub-folders in it, or a file of that
  name) the next free name is taken, `<name>_images (2)`, `(3)`… — the same one again on the next export. The folder
  is created only when there is a drawing. Tests (`textdoc.rs`): existing `<name>-1.png` and a foreign
  `<name>_images` untouched, links matching the files written, fewer drawings leaving no stale image. The
  interface's "Drawings saved…" sentence says "in the folder next to it". Word is unaffected (drawings embedded).
- Command `export_structured_text(sources, format: 'md'|'docx', destination, labels, includeDrawings?) -> { paths,
  analyzed }` (`textdoc.rs`), through the indexer's on-demand engine; same sources as the `.txt` export (the note as it
  is in the editor, or notes of the library). `structured_text_drawings(sources) -> number` (above).
- Tests on Linux (synthetic recognitions and analyses, a fake engine): `ardoise-hwr` `structure::tests` (lines to
  regions in order, a region without a line read on its own and only then, corrections laid line by line or not,
  underlines and strikethroughs in both paths and the mark's own reading, drawings beside and between blocks, the
  structure without drawings), `markdown`/`docx` (marked runs), `textdoc::tests` (the export and its check through the
  indexer, a correction read from the cache, the structures kept once), `handwriting::tests` (the `.txt` text from a
  correction, without an engine), `TextExportSection.test.ts` (the option). On Windows, `cargo test -p ardoise-hwr
  --test structure -- --nocapture` analyses a synthetic page with an underlined title and prints the recognition, the
  regions, the structure and the underlined words.

## M15: tuning the straightening

"Redresser les formes" gets a user setting (SPEC §4.4, `docs/questions.md` Q12, ROADMAP M15). This section is the
recogniser's side (package A: `crates/ardoise-ink`, `crates/ardoise-ink-wasm`, `app/ui/src/lib/ink.ts`); the setting
itself (`[shapes]` of `ardoise.toml`), the editor's panel and the assistant's tool are described with their code.

- **`ardoise_ink::ShapeTuning`** `{ tolerance: u8 (1..=5), min_size_mm (5..=25), snap_angle_deg: Option (1..=10),
  snap_round_percent: Option (2..=20) }`, `clamped()` (out of range: to the limit; not a number: the default),
  `Default` = `3, 10, Some(4), Some(10)`, the thresholds of before. `straighten_with(strokes, &tuning)` clamps the tuning
  and resolves it into a private `Params` (one value per threshold that the tuning moves), threaded through the
  recognisers instead of the constants; `straighten(strokes)` is `straighten_with(strokes, &ShapeTuning::default())`.
- **Same results by default.** The default tuning resolves to `Params::DEFAULT`, built from the constants themselves,
  and `Params::with_factor(default, 1.0)` gives bit for bit the same values (tested, so the steps next to the default
  move from it without a jump). All the earlier tests pass unchanged; a one-off check compiled the code of before beside
  the new one and found bit-identical results on 23,420 inputs (noisy shapes of 5 to 60 mm, the glyphs at 4 to 30 mm,
  the closed letters, 20 pages of 180 strokes; 6,391 shapes).

### What each setting moves

| Setting | Thresholds | Not moved |
|---|---|---|
| Tolerance (steps 1-5) | × the step's factor: `CLOSE_GAP`, `FIT_MEAN`, `FIT_MAX` (closed shapes), `LINE_MAX_DEV`, `LINE_RMS_DEV` (lines), `SEG_EPS`, `MERGE_DEV` (the split into straight pieces: lines and arrows), `BARB_REACH` (a barb starts near the tip); `RECT_COHERENCE`: its shortfall from 1 (0.55 → 0.73 strict, 0.415 lenient) | `MIN_ASPECT`, `RECT_SIDE_SHARE`, `LINE_MAX_PATH`, the hooks, `MERGE_TURN`, the barbs' angles and length shares, the partner strokes' reach and span, `SEG_EPS_MIN` and the other absolute sizes |
| Minimum size | `MIN_SIZE` = `min_size_mm` × 100 units, `MIN_CLOSED_SIZE` = (`min_size_mm` + `CLOSED_EXTRA_MM`, 3) × 100; the shortest barb `BARB_MIN` (250 units) × `min_size_mm` / 10 below 10 mm (1.25 mm at 5 mm: an arrow of 1 cm has barbs of about 1.5 mm) | |
| Horizontal or vertical | `SNAP_ANGLE` = `snap_angle_deg` (lines, shafts, ellipse axes, rectangles); off: never | |
| Circle or square | `SNAP_RATIO` = (100 − `snap_round_percent`) / 100; off: never | the letter rule's `LETTER_MAX_RATIO` (0.9) |

The protections of handwriting are fixed: `letter_like` (an upright oval under `LETTER_MAX_AXIS`, 25 mm, short ÷ long
axis below `LETTER_MAX_RATIO`, 0.9, within 40° of the vertical). `LETTER_MAX_RATIO` was `SNAP_RATIO`; it is now its own
constant, so that snapping within 20 % does not widen what counts as round.

### Choosing the tolerance factors

Factors **0.6, 0.8, 1, 1.15, 1.3** (`TOLERANCE_FACTORS`). Measured with ignored tests (`cargo test --release -p
ardoise-ink measure_tolerance -- --ignored --nocapture`, and `measure_tuned_letters`) on a family of shapes drawn more
and more unsteadily: `noisy(kind, size, noise)` is the random hand of the earlier measurements with its wobble × `noise`
(the pen's jitter unchanged), 15 to 60 mm, ellipses lying (aspect 0.45-1), arrows in one stroke or as a line and a
"V" whose apex misses the tip by `noise` × 4 % of a barb. A first run showed that wobbly lines were rejected by the
split into pieces (`SEG_EPS`, unscaled at first), not by `LINE_*_DEV` (at twice the usual wobble, 224 of 300 lines were
"several pieces" and none failed the deviation tests): so `SEG_EPS` and `MERGE_DEV` are part of the tolerance.

Recognised out of 300 hands, by tolerance step 1 to 5 (`measure_tolerance_steps`); every row is non-decreasing, and a
test (`each_tolerance_step_recognises_at_least_as_much_as_the_stricter_ones`) checks it on a smaller family:

| Shape, wobble | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| Line × 1 (the usual hand) | 184 | 284 | 300 | 300 | 300 |
| Line × 1.5 | 36 | 141 | 254 | 285 | 297 |
| Line × 2 | 8 | 39 | 116 | 198 | 248 |
| Line × 2.5 | 3 | 10 | 40 | 83 | 136 |
| Ellipse × 4 | 233 | 298 | 300 | 300 | 300 |
| Ellipse × 6 | 115 | 203 | 273 | 296 | 300 |
| Rectangle × 3 | 170 | 272 | 298 | 299 | 300 |
| Rectangle × 5 | 37 | 92 | 166 | 225 | 259 |
| Arrow × 2 | 107 | 223 | 258 | 264 | 277 |
| Arrow × 3 | 15 | 72 | 142 | 181 | 204 |

(Ellipses are already very tolerant at step 3: a lenient step helps lines, rectangles and arrows most. 12 arrows of
300 are recognised at no step, 11 of them because their barbs are shorter than 2.5 mm.)

What a more lenient step also straightens (`measure_tolerance_on_curves`, 200 hands each, steps 1 to 5):

- arcs with a 4 cm chord taken for lines: 15°: 2 / 199 / 200 / 200 / 200; 20°: 0 / 1 / 191 / 200 / 200; 25°: 0 / 0 /
  0 / 131 / 200; 30°: 0 / 0 / 0 / 0 / 21; 35° and more: never. So "souple" makes a gentle curve (2 mm of bulge on
  4 cm) straight, "stricte" keeps even 15° arcs;
- loops left open: 36° open: 0 / 0 / 30 / 152 / 197 become closed shapes; 54° open: 1 at step 5; more: never;
- large letters (the glyphs at 18 and 30 mm, 4,800 runs): changed beyond a single straight stroke 0 / 0 / 0 / 0 / 1.

The lenient end stops at 1.3 because handwriting starts to change beyond it: the closed capital D glyph written 3 cm
high becomes an ellipse in 1 % of the hands at 1.3, 12 % at 1.4; from 1.5 a D of 9 to 18 mm becomes a rectangle (the
rectangle has no letter rule), and at 1.7, 196 of the 4,800 large letter runs change. Below 1 the steps are
spaced as widely as the data allow while "stricte" still recognises carefully drawn shapes (all lines at half the usual
wobble, 61 % at the usual one).

### Handwriting with the tuning

`measure_tuned_letters`: the glyphs at 4, 6.5, 9 and 12 mm (28,800 runs) and the closed letters o, O, 0, Q, D at 4,
6, 9 and 12 mm (300 hands each):

| Tuning | Glyph runs changed | Closed letters changed |
|---|---|---|
| default (step 3, 10 mm) | 900: all single straight strokes of 12 mm letters (the stems of K and 4, the strokes of x, which are 10 mm or more) | none |
| step 5, 10 mm | the same 900 | none |
| step 3, 5 mm | 3,109: single straight strokes only (K, 4, x from 6.5 mm, Y from 9 mm, F at 12 mm) | lower-case o of 9 and 12 mm: 156 and 153 of 300 (the round ones become circles); Q of 12 mm: 164 (its 5 mm tail becomes a line) |
| step 5, 5 mm | 3,112 (3 more F) | o: 156 and 153; Q: 196 |
| step 5, 5 mm, 10°, 20 % | the same as step 5, 5 mm | the same |

At the default minimum size the lenient step changes no more handwriting than the default (tested:
`handwriting_stays_as_it_is_at_the_most_lenient_tolerance`). At 5 mm a letter is only ever changed by a stroke that is
itself one straight segment or a round loop: never a capital O, a 0 or a D (tested with upright ovals of 9 to 22 mm,
`a_small_upright_o_is_never_straightened_whatever_the_tuning`), which is what the editor's warning below 10 mm says.

### WebAssembly

- `ink_straighten_tuned(ptr, words, tolerance, min_size_mm, snap_angle_deg, snap_round_percent) -> usize` replaces
  `ink_straighten(ptr, words)` (removed: `ink.ts` was its only caller). The four values are `f64`; a negative angle or
  percentage turns that alignment off; `tuning_from` rounds the tolerance (not a number: step 3) and `straighten_with`
  clamps the rest (not a number: the default). The strokes' words and the result (`ink_straighten_result`) are
  unchanged.
- `Ink.straighten(strokes, tuning = DEFAULT_SHAPE_TUNING)` passes `snapAngleDeg ?? -1` and `snapRoundPercent ?? -1`.
  `DEFAULT_SHAPE_TUNING` (`ink.ts`) equals `toShapeTuning(DEFAULT_SHAPE_SETTINGS)` (tested in `ink.test.ts`, with a 7 mm
  line straightened at 5 mm only, kept tilted without the alignment, and out-of-range values clamped by Rust).

### Speed (the preview runs `straighten` at most once per animation frame)

Native, release, in one process with the code of before M15 compiled beside it (a one-off comparison; `measure_speed`
is the kept measurement), on a selection of handwritten glyphs of 5 to 9 mm and shapes of 2 to 6 cm, medians of 31
runs:

| Selection | Before M15 | Now, default | Now, widest (step 5, 5 mm, 10°, 20 %) |
|---|---|---|---|
| 58 strokes, 4,900 points | 0.63 ms | 0.53 ms | 0.60 ms |
| 341 strokes, 21,000 points | 3.3 ms | 2.1 ms | 2.9 ms |
| 821 strokes, 48,000 points | 11.5 ms | 4.7 ms | 7.3 ms |

(While two other builds ran on the PC, every figure was about 2.5 times higher.) The widest tuning first took 44 and
284 ms on the two large selections: with a 5 mm minimum nearly every letter stroke is an arrow shaft to try, and the
search for its partner strokes measured the farthest point of every other stroke. It now skips the strokes whose
pieces all end outside the reach (a box test with a margin, so that it never decides otherwise than the distances),
then checks the distance of the ends before the farthest point. The decisions are the same: the check above found
bit-identical results after these changes, and the default got faster too.

WebAssembly in Node (vitest), the call alone (encoding the strokes and decoding the result in TypeScript are
negligible), curvy letters of 90 points and dense shapes: 60 strokes (6,000 points) 1.6-1.9 ms, 330 strokes (32,000
points) 7-8 ms, 860 strokes (82,000 points) 17-19 ms, with either tuning (up to twice that on the loaded PC). About
half of it is `f64::hypot`, a software routine in WebAssembly; `(x² + y²).sqrt()` would halve it but changes results
by an ulp, and the default would no longer give exactly the results of before.

So a typical selection (50 strokes of writing and 10 shapes) costs about 2 ms, well within a frame, and a few hundred
strokes about half a frame. A whole dense page (800 strokes and more) takes about one frame: the editor should
recompute only when a value changed, coalesced to one call per frame, and could wait for the slider to settle on such
selections. Possible later work here: a prepared selection (strokes decoded and cleaned once, then straightened with
each new tuning).
