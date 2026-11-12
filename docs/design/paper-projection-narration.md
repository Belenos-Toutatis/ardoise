# Live paper and background, editor projection, sound while writing (milestone M17)

Decisions: `docs/questions.md` Q13 (2026-11-11: the three M17 bullets), SPEC §4.2 « Papier et fond pendant le direct »,
§4.4 « Projeter la note de l'éditeur », §4.5 « Narration au micro » (« pendant qu'on écrit dans l'éditeur »), ROADMAP M17
and its checkpoint H7. Look: the mockup the user validated, `target/maquettes/maquette-barre-laterale.html` (not in git;
its blocks marked « nouveau »: live Page and Fond, the editor's Projection, the editor's Son « Pendant que j'écris » /
« Pendant une relecture »). M16 prepared the ground: [side-panel.md](side-panel.md) §11 "M17 fit" and "As built".
Written on 2026-11-11 from reading the code at commit b9eaa8a (branch `m17`), nothing built or run: every "today"
cites where it was read; "(supposé)" marks what was not checked.

**Words.** The **page** of live mode is the current page of the session (`LiveSession.current`). Its **paper** is its
`Background` (colour, template, texture); its **own paper** is the colour it has without the blackboard. Its
**image** is a background image or a page of a PDF (`Note.background_image` and the pixels of the `background.*`
member). The **projection** is the editor's page full screen on a screen (SPEC §4.4), in the **projection window**
(a webview of its own, like live's presentation window). The **feed** is what the editor sends to it. A **writing
recording** is a narration recorded while writing in the editor (« Pendant que j'écris »), as opposed to a **replay
recording** (« Pendant une relecture », today's). **Note time** is `Point::t`, ms since `Note::created`.

## Rules

- One owner per file (§10); the contracts of package 0 are fixed: a package that needs to change a file it does not
  own, or a contract, says so in its report.
- Every `data-section`, `data-slot`, `data-action` and `data-field` that exists today stays on the same control. The
  new hooks are listed here.
- The tablet is never slowed: nothing slow under the hub's lock (hub.rs module doc, lines 15–16); imports, decoding,
  rendering and writing happen outside it.
- **No change to the `.ardoise` format** for the three features: paper, template, texture, `backgroundImage` and the
  `background.*` member exist since 1.0–1.2 (`docs/file-format.md` §2, §4), and a narration in note time
  (`ARDOISE_BASE=note`) exists since M6. Package G (optional, §3.6) adds a **playback rule** for note-time narrations,
  stored nowhere (§4).
- SPEC/Q13 win over the mockup; every choice this document makes that the user may want to make is in §11.

---

## 1. Live: the paper and the image of the current page

### 1.1 Today

- **New pages are white, or slate with the blackboard**: `LiveSession::new_page` (`live/session.rs:896–924`) builds
  `Background { paper: White | Slate, ..Default }` and `background_image: None`. It is called by
  `LiveSession::new` (673–704, with no page yet), `NextPage` past the last page (1634–1650) and `NewPage` (1665–1675).
- **Only a resumed sheet has an image**, whose pixels are the resumed file's: `ResumeBase` (`live/resume.rs:30–59`,
  decoded once per session, `OnceLock`), found by `LiveHub::page_image` (`live/hub.rs:1187–1194`: `page.base()?`, so
  `None` for every other page). A page of the session is written as `NoteFile::new(note)` (`page_file`, hub.rs:526): no
  `background.*` member at all; its thumbnail is `thumbnail_png` (hub.rs:533–543).
- **A resumed sheet's file hears of its paper only through chalk**: `written_sheet` (hub.rs:485–502) puts back the
  paper read (`if same_ink || !sheet.on_board { paper = before.paper }`); template, texture and image are written as the
  page has them, which nothing in live mode changes today. `SheetChanges` (188–202) holds `on_board` and
  `layers_edited` (M14).
- **The blackboard** (`set_view`, session.rs:1975–2010): turning it on keeps the page's paper in
  `Page.paper_before_board` (123–125) and paints the page slate, chalk replacing the style; turning it off gives the
  paper back (white for a page made under it). A resumed sheet shown again with the board off gets its paper back
  (`settle_paper`, 1591–1604). New pages under the board are slate.
- **A page without ink is never written**: `Page::needs_write` (session.rs:370–372) wants a file or a stroke.
- **`merge_outside`** (hub.rs:652–702), which writes a page into its note retitled elsewhere, copies the note's fields,
  the narration and `original_note`, but **not** the `background.*` member (the file keeps the one on disk).
- **Windows.** `LiveCanvas` (`components/live/LiveCanvas.svelte:353–397`) renders the paper through
  `live_page_background` (live.rs:482–517, the hub's image) when the snapshot's page has an image, else
  `render_background`; the paper is keyed by `JSON.stringify([page, background, sheetImage, live.noteId])` (363): two
  files of the same name and placement look the same to it. `LiveState.pageImage` is the snapshot's
  `backgroundImage` (`lib/live-state.svelte.ts:145`, 461–462); `effectiveBackground` paints slate on the board
  (`lib/live-render.ts:147–150`).
- **Share.** `paper_key` (`share/live_paper.rs:47–64`) names the paper of any snapshot whose effective background is
  not plain (`is_plain_paper`, `share/page.rs:25–28`: blank template, no texture, no image); `LivePapers::picture`
  (live_paper.rs:208–243) renders it from `hub.page_image` and the base's member (`key_of(&member.bytes)`, 217). The
  key digests the note id, page, effective background and image description, **not the pixels** (`key_for`, 66–76).
  The module doc (1–14) and lan-share.md say ordinary pages never get a key: true today only because they are white.
- **UI.** The live window's `SidePanel` has no `page` nor `background` snippet (`LiveView.svelte:1077–1166`), as M16
  planned (side-panel.md §1 "What each window lacks until M17", §11). `PaperBlock` (`components/editor/PaperBlock.svelte`)
  and `BackgroundImageSection` (`components/editor/BackgroundImageSection.svelte`) are standalone blocks since M16. The
  section stages a file with `api.stageBackgroundImage` (86) into Rust's process-wide staging area
  (`background.rs:248–338`, keyed by `key_of`), and offers to switch the note's orientation (88–92), which live pages
  cannot do (a session page follows the tablet, a resumed sheet never turns: M12).
- **Keys.** `isTypingTarget` (`lib/live-keys.ts:77–86`) treats a `select` and a range `input` as typing: with one
  focused, Page Down from the class's remote is the control's, not live mode's.

### 1.2 Data model (package A)

`live/paper.rs` (new) holds the page's image:

```rust
/// The pixels of a live page's background image: the member written with the page, and its key.
pub struct PageImage {
    /// What `background.png` / `background.jpg` holds (as staged: at most 4096 px a side).
    pub member: BackgroundFile,
    pub width: u32,
    pub height: u32,
    /// `background::key_of(&member.bytes)`: tells two files of the same name apart (windows, share).
    pub key: String,
}
```

- `Page` gets `image: Option<Arc<PageImage>>`. `Page::resumed` seeds it from the base's member when the note has a
  `background_image` (dimensions by `ardoise_render::picture::dimensions`, a header read); `ResumeBase` keeps the file
  only and loses its `picture` (`OnceLock`).
- **Decoded pixels** (4096 × 4096 RGBA is 64 MB) are not kept per page: `live/paper.rs` keeps the last
  `DECODED_KEPT = 2` decoded pictures by `PageImage.key`, as `background::Staging` does (`DECODED_KEPT`, background.rs:61).
  `PageImage::picture()` decodes on a miss. Users: the thumbnails, `live_page_background`, the share's `LivePapers`.
- `Resumed` (session.rs:63–82) gets `paper_edited: bool` and `image_edited: bool`, set by the operations below (a
  deliberate change of the sheet, as `layers_edited` in M14); `SheetChanges` carries both and the page's own paper.
- **Own paper**: `Page::own_paper(board: bool) -> Paper` = `paper_before_board.unwrap_or(White)` while the board is on,
  else `note.background.paper`.
- `LiveSnapshot` (`live/model.rs:257–294`) gets two fields, **not** in the share's allow list (`SharedSnapshot`,
  `share/feed.rs:385–425`, lists its fields one by one):

  ```rust
  /// The current page's own paper (what it has without the blackboard): the live window's Page tab shows it.
  pub page_paper: Paper,
  /// The key and size of the pixels of the page's image; `None` without one. The windows key their paper on it, the
  /// Fond tab places the image with it.
  pub image_pixels: Option<ImagePixels>,   // ImagePixels { key: String, width: u32, height: u32 }, camelCase
  ```

### 1.3 Operations (package A)

Two commands in `live.rs`, the work in `LiveSession` and `LiveHub` as M14's layer ops (`LiveHub::layers`,
hub.rs:1544): refused as a whole, under the lock only cheap work, one snapshot.

```rust
#[tauri::command] pub async fn live_page_paper(runtime: State<'_, LiveRuntime>, page_index: usize,
    background: Background) -> CommandResult<()>
#[tauri::command] pub async fn live_page_image(runtime: State<'_, LiveRuntime>, page_index: usize,
    op: LivePageImageOp) -> CommandResult<()>

// live/paper.rs, serde tag "kind", camelCase as LayerOp
pub enum LivePageImageOp {
    /// The image staged under `key` (`stage_background_image`), placed as `image` says.
    Set { key: String, image: BackgroundImage },
    /// The same pixels placed otherwise (fit, quarter turn, opacity).
    Place { image: BackgroundImage },
    Remove,
}
```

- **Refusals** (codes that exist): `liveEditStale` when `page_index` is not the page on screen; `liveNotRunning`
  before the first start or once stopped (`LiveHub::change` with `rewrites` true); `liveEditInvalid` for an image
  whose `name` is empty or over 255 characters, a `pdfPage` of 0, a `rect` that is not finite or not of positive size
  or reaches more than 16 page sizes away, a non-finite opacity (else clamped to 0..=1; `quarterTurns` taken modulo 4),
  a `place` whose `name` or `pdfPage` differ from the page's image or with no image; `backgroundMissing` when `key` is
  not staged. A `Custom` paper's channels are `u8` (Tauri refuses others: never sent, the custom swatch is disabled).
- **`live_page_paper`**: board off → `note.background = background`, `paper_before_board = Some(background.paper)` (so
  that a later blackboard gives this paper back, slate included); board on → template and texture at once, the colour
  only into `paper_before_board` (the page stays slate; the Page tab says so, §1.6). Same value: nothing, no event.
  `page.touch()`, `Resumed.paper_edited = true`, snapshot.
- **`live_page_image`**: `Set` first takes the staged pixels out of the staging area **before the lock**
  (`Staging::take(key) -> Option<Arc<Staged>>`, new in `background.rs`, which also drops its decoded copy: the editor
  is never mounted with live, and the staging must not keep a lesson's PDFs), builds the `PageImage`, then under the
  lock sets `note.background_image` and `page.image`, `touch()`, `Resumed.image_edited = true`, snapshot. A refused `Set`
  puts the staged image back (`Staging::put`). `Place` changes the description only; `Remove` drops both.
- **Not undone by Ctrl+Z** (Q13: « on revient en arrière en rechoisissant l'ancien réglage, pas avec Ctrl+Z »): no
  history step, `canUndo` untouched; no stroke in progress is ended; the revision does not change (no index moves).
- **New pages keep the paper, not the image** (Q13). `new_page(now_ms, from: Option<&Page>, board)`: the page it is
  made from is the page on screen when it is made (`NextPage` past the last, `NewPage`, the tablet's button set to «
  nouvelle page »; §11 O4); its template and texture, and its own paper: as `note.background.paper` with the board off,
  as `paper_before_board` with it on (the page itself slate). No image. `LiveSession::new` passes `None`: a new session
  starts white (§11 O2). An existing next page keeps its own.
- **`needs_write`**: also true for a page with an image and a change (`version != saved_version`), so that a worksheet
  shown and not written on is kept (§11 O3). Paper alone, without ink or image, is still nothing.
- **A page with an image and the tablet's orientation.** Today an ordinary page turns with the tablet, ink and all
  (`hold`, `follow`, session.rs:1079–1161; `Page::turn`, 338–364), and a turn "knows nothing of its background image"
  (its doc comment): the image would stay where it was while the ink turned. M17 (§11 O21): a page with an image
  **and no ink** (no stroke, empty history) takes the tablet's new size, as a blank page does today (1118–1120, without
  the `file.is_none()` condition, since such a page is now written: O3), and its image is placed again on it with the
  fit it has (contain, cover or stretch, recognised as `currentFit` does in `lib/background.ts`; contain when none
  matches), upright. A page with an image **and ink** is held like a resumed sheet: never turned, the tablet follows it
  (`hold_for`, `PenMapping::for_sheet`, `pen_mapping` 1163–1176), `can_turn_page` false (the live window's quick switch
  is already disabled then, `LiveView.svelte:224`). One predicate, `Page::held_fixed()` (resumed, or image and ink),
  replaces `is_resumed()` in `turn`, `hold`'s turns (1111), `pen_mapping` and the snapshot's `can_turn_page` (936);
  `is_resumed()` keeps its other uses. A page of the session whose image is removed turns again with the tablet (the
  next `follow`); a resumed sheet stays fixed.

### 1.4 Saving (package A)

- `SaveJob` (hub.rs:172–184) gets `image: Option<Arc<PageImage>>` (an `Arc` clone under the lock, no bytes copied).
- `page_file` (hub.rs:511–531): a page of the session now writes `file.background = image.member.clone()` (outside the
  lock, a few MB at most, at most every `save_wait`); a resumed sheet keeps the base's member unless `image_edited`,
  then the page's (or none after `Remove`).
- `written_sheet`: the paper becomes, in order: slate when ink was added under the board (today's rule); else the own
  paper when `paper_edited`; else the paper read. Template, texture and image stay as the page has them (they change
  only on purpose now).
- `page_thumbnail`: with the page's picture whenever it has an image (`thumbnail_view(NoteView::new(note, picture))`).
- `merge_outside` copies the member too (`merged.background = file.background.clone()`): otherwise a page whose image
  changed, written into a note retitled elsewhere, would describe the new image over the old pixels. A test.
- `back_up_unsaved` (1577–1621) goes through `page_file`: it gets the image like any write.
- The fingerprints (`content_fingerprint` 431–452: background and image description; `file_fingerprint` 454–483: the
  member's bytes) see every change: a fork (M12) carries the page's image and paper.
- **Original.** `keep_resumed_original` (`original.rs:82–88`) is about ink: a sheet whose only change is its paper or
  image keeps no original; « Revenir à l'original » gives back the ink, not the paper nor the image
  (`lib/editor/original.ts:15–27`, unchanged: the editor's rule). The guide says so.

### 1.5 The windows and the share (A for Rust, B for the windows)

- `LiveHub::page_image(index) -> Option<(BackgroundImage, Arc<PageImage>)>` (no `base` any more): `live_page_background`
  and `LivePapers` take it as they took the base.
- `share/live_paper.rs`: `paper_key` adds `snapshot.image_pixels.key` to the digest (another file under the same name
  and place is another picture); `picture()` takes `PageImage.key` instead of hashing the member. Ordinary pages with a
  template, a texture or an image now get a key and a picture: the module doc and lan-share.md "The paper of a live
  page" are corrected (H); a white blank page still costs nothing (the existing test stays).
- `LiveState` (B) keeps `imagePixels` and `pagePaper` from the snapshot (viewers: absent, read as null and as the
  background's paper). `LiveCanvas` keys its paper on `[page, background, sheetImage, noteId, imagePixels?.key]`.
- The presentation window, the preview and the viewers already draw the picture under the strokes at every zoom (M12
  "Paper, as built"): nothing else to change.

### 1.6 The live window: Page and Fond tabs (package B)

`LiveView.svelte` adds `{#snippet page()}` and `{#snippet background()}` to its `SidePanel` (between `layers` and
`sound`: `SIDE_TABS` orders them, side-panel.md §11). Both are disabled while the session does not run (as the layers
zone). `pageIndex` is read at the click; `liveEditStale` is told as `live.failed.pageStale`, other refusals as
`live.failed.paper` / `live.failed.image` with the error's sentence (`lib/live-errors.ts`).

**Page** (`section.pane-block[data-section="live-paper"]`, title `editor.page.paperTitle`):
- `p.pane-help` `live.page.help`; `PaperBlock` with `background = { ...live.background, paper: live.pagePaper }` and
  `onbackground` → `api.livePagePaper(live.pageIndex, b)`.
- New `PaperBlock` prop `colourDisabled?: boolean` (default false): the swatches disabled, template and texture not.
  Live passes `live.view.board`, and shows `p.pane-help[data-slot="paper-board"]` `live.page.board` (§11 O1).
- With slate chosen as paper and the board off: `p.pane-help[data-slot="paper-slate"]` `live.page.slate`.
- `p.pane-help` `live.page.back` (how to go back; Q13).

**Fond** (`BackgroundImageSection` as is, `data-section="background-image"`):
- New props: `offerOrientation?: boolean` (default true; live: false: the image is placed on the page as it is, with
  « Ajuster » by default, `importedImage`) and `hint?: string` (default `background.editor.hint`; live:
  `live.background.help`). `image = live.pageImage`, `pixels = live.imagePixels`, `page = live.pageSize`.
- `onset(image, pixels)` → `livePageImage(pageIndex, { kind: 'set', key: pixels.key, image })`; `onfit` →
  `refit(...)`, `onturn` → `turnImageAlone(...)`, `onopacity` → `{ ...image, opacity }`, each as `{ kind: 'place' }`;
  `onremove` → `{ kind: 'remove' }` (`lib/background.ts` helpers, as the editor's `lib/editor/image.ts` uses them).
- The section's busy state covers the import (a PDF page at 300 dpi takes seconds: the tablet goes on meanwhile).

**The class's remote.** In the live window, **Page Up and Page Down always act on live mode**, even with a `select` or a
range input of the side panel focused (`liveKeyAction`: these two keys bypass `isTypingTarget` for `SELECT` and
`input[type=range]`; text fields keep them), and the template select, the fit select and the opacity slider give
the focus back after a change (`blur()`), so that a choice made with the mouse does not keep the remote's keys. The
sweep is unchanged (it uses `target: null`); a test per control.

Badges: none on Page and Fond (side-panel.md §4). Help: no key added (§6).

### 1.7 Edge cases

| Case | Behaviour |
|---|---|
| Paper changed on a page, then Page Down to an existing page | that page keeps its own paper; coming back finds the changed one |
| Board on, then a new page | slate, template and texture of the page left, `paper_before_board` its own paper: board off gives it |
| Board on, a swatch | disabled (O1); Rust would still store the colour as `paper_before_board` |
| Slate chosen in Page, board off | slate paper, the ink unchanged (black on slate: the `live.page.slate` hint) |
| Resumed sheet: paper or image changed, no ink | written (deliberate change), no original kept; Ctrl+Z does nothing |
| Resumed sheet with an image: image removed | `background_image` and the member gone from the file |
| The sheet changed elsewhere meanwhile | fork (M12), the new note gets the page's paper and image |
| An image set on a fresh page, no stroke | the page becomes a note (O3); its `created` is the page's creation |
| The same file placed twice | `key` equal: the windows and the share keep their picture |
| Another file of the same name and page | another `key`: the windows render again, the share has another `paperKey` |
| Image set during a pause of the share | told to nobody until the resume (lan-share.md "Pause") |
| The tablet turned (`hold`, the quick switch) | a page with an image and no ink takes the new size, its image placed again upright; with ink it stays, the tablet follows it, the quick switch disabled (§1.3, O21); a resumed sheet never turns (M12) |
| A portrait worksheet on a landscape page | contained, smaller (no orientation offer in live); turning the tablet to portrait before writing gives a portrait page with the worksheet placed again |
| A narration recorded on the page | unchanged (M12) |
| Live stopped | Page and Fond disabled; the stopped session's pages are not changed (`liveNotRunning`) |

### 1.8 Tests

- **A (Rust)**: `live/paper/tests.rs` and `live/tests.rs`: each op and refusal with its code; board on/off with a chosen
  paper (including slate chosen); new page keeps template, texture and own paper and has no image, under and off the
  board; `LiveSession::new` white; snapshot fields; `needs_write` with an image; the tablet turned on a page with an
  image without ink (new size, image placed again with its fit) and with ink (not turned, `PenMapping` onto it,
  `can_turn_page` false), then the image removed (turns again); `Staging::take` and the put back on a
  refusal; `written_sheet` with `paper_edited` (with and without ink under the board) and `image_edited`; `page_file`
  members; `merge_outside` copies the member; a fork keeps the image; `back_up_unsaved` holds it; the decoded LRU
  (third image decodes again, no growth). `share/tests.rs`: `paperKey` for a Seyès page of the session, another key for
  another pixels key, a white page still none, the PNG served under the key holds the image (a pixel). **The tablet is
  never slowed**: as M14's `the_tablet_is_never_slowed_by_the_computer_s_pen` (`live/tests.rs`), the tablet's input on
  one thread while another sets paper and `Set`/`Place`/`Remove` a 4096 px image every 50 ms: p99 of the tablet's Rust
  part under `NEVER_SLOWED_P99_MS` (10 ms).
- **B (vitest)**: `LiveView.page.test.ts` (new): the six tabs now (Page and Fond present), Page and Fond call the
  commands with the page on screen, disabled at idle, board on → swatches disabled and the hint, slate hint, the
  remote's Page Down with the template select focused turns the page, blur after a change, stale and other refusals'
  sentences; Fond: choose → stage → `set` (no orientation offer), fit/turn/opacity → `place`, remove. `LiveView.side.test.ts`
  (M16: "four tabs, no Page or Fond") becomes six tabs. `live-state` keeps `imagePixels`, `pagePaper`. `LiveCanvas`
  renders again on another pixels key. `PaperBlock.test.ts` (`colourDisabled`), `BackgroundImage.test.ts`
  (`offerOrientation=false`, `hint`). `mock-live` follows A's rules (new page keeps the paper, not the image).

### 1.9 Performance

A paper or image op is a click: one snapshot, whose clone of the page under the lock is the cost M12 measured (a few ms
for a lesson; 58 ms for 50 000 strokes of 100 points in the test build). The windows keep their outlines (same strokes:
`sameStrokes`, live-state.svelte.ts:94–110, 444) and redraw the cache once with the new paper, as for a page change.
The picture is rendered and decoded off the pen's frames (M12). Measure: §8, part `paper`.

---

## 2. Editor: the projection

### 2.1 Today

- The presentation window is live's: label `presentation`, URL `index.html?view=presentation` (`presentation.rs:20–25`),
  made by `show_on` (145–187: undecorated, not focused, hidden until placed, the WebView2 data folder shared, the
  `Activity` flag that holds the handwriting indexer, `live/activity.rs`), opened by `open_presentation` (235–263),
  which saves the screen for this PC and records it in the hub (`set_presentation`, 261): every live snapshot carries
  it, and a new session takes it over (`install`, hub.rs:1342). `on_window_event` (207–225) tells the hub when it goes,
  and takes it along with the main window.
- `Presentation.svelte` subscribes to the hub (`subscribeLive(live, api.liveSubscribe)`, :77), draws with
  `LiveCanvas mode="presentation"` (:124–135), where the pen, a finger or the mouse write temporary marks and two
  fingers pinch the area (LiveCanvas.svelte:17–24), and takes live's keys (:102–113). `main.ts:10–13` mounts it for
  `?view=presentation`. The capability `capabilities/presentation.json` names the window `presentation`.
- `LiveCanvas` draws a `LiveState` through `LiveRenderer`, whose input is the small `LiveSource`
  (`lib/live-render.ts:217–229`); `LiveState` applies `LiveBatch`es: snapshots, strokes in progress (one per pen),
  `strokesEdited` with partial repaints (M12), `view`.
- The editor: the Projection tab holds `ShareBlock` alone (`Editor.svelte:2143–2145`); the badge's blue dot is the share's
  (307–318). The doc is in the UI (`history.present`), drawn by `Canvas.svelte` as an SVG; the stroke in progress is
  local (`drag.points`, Canvas.svelte:121, 331–348, 573–582). During a replay the canvas gets `replayDoc`, the note as
  drawn at the playhead (2058; `ReplayTimeline.noteAt` keeps the objects of the strokes shown whole, `lib/replay.ts:226–251`).
- The editor's share sends the whole note, debounced 100 ms and at most every 300 ms (`lib/share-state.svelte.ts:10–20`,
  123–131; `Editor.svelte:421–430`): fine for a viewer, too slow and too large for a projector « mis à jour à chaque
  trait ».
- `ProjectorBlock` and `ZoneBlock` (`components/projection/`) have window-agnostic props (M16 §8); `ZoneBlock`'s width
  button titles `live.zoom.fitWidthTitle`, which ends with live's key « (W) ».
- The assistant's `presentation` tool drives live mode only (`lib/app-bridge.svelte.ts:192–219`).

### 2.2 Design in one paragraph

A second window label, `projection`, shows the editor's page with the same drawing stack as live's presentation
(`LiveState` + `LiveCanvas mode="presentation"`). Rust keeps the projected page (`projection.rs`) and speaks to the
window in live's own vocabulary (`LiveBatch`: snapshot, `strokesEdited`, the stroke in progress as a computer stroke,
`view`), so the window, its partial repaints and its latency figures are live's. The editor feeds Rust with small
updates computed by object identity (`lib/projection-feed.ts`): the whole page when its layout changes, else "keep the
first n strokes, then these", plus the stroke in progress point by point, with one call in flight.

```
Editor.svelte ──show(note, key)/stroke()/visible()──▶ EditorProjection (lib/editor/projection.svelte.ts)
                                                         │ ProjectionFeed: diff by identity, one call in flight
                                                         ▼
                              projection_update(update) ─▶ projection.rs: Scene (note, image key, view, stroke)
                                                         │ LiveEvents, one LiveBatch per update
                                                         ▼
               window "projection" (?view=projection) ◀─ Channel<LiveBatch> (projection_subscribe)
               EditorPresentation.svelte: LiveState + LiveCanvas (presentation, interactive=false)
```

### 2.3 Rust: `projection.rs` (new) and `presentation.rs` (package C)

```rust
#[derive(Deserialize)] #[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProjectionUpdate {
    /// The whole page (a note opened, its page, paper, image, layers or image key changed).
    Page { note: Note, image_key: Option<String>, view: ProjectionView },
    /// The strokes from `keep` on are replaced by `tail`; `ended`: the stroke in progress `id` is now among them.
    Strokes { keep: f64, tail: Vec<Stroke>, ended: Option<f64> },
    View { view: ProjectionView },
    StrokeBegin { id: f64, tool: Tool, color: Rgba, width: f32, layer: f64, points: Vec<ProjectionPoint> },
    StrokePoints { id: f64, points: Vec<ProjectionPoint> },
    StrokeCancel { id: f64 },
}
#[derive(Deserialize, Serialize, Clone, Copy)] pub struct ProjectionView { pub zoom: f64, pub x: f64, pub y: f64 }
/// `wall`: Unix ms of the pointer sample (the window's latency figures count from it).
#[derive(Deserialize)] pub struct ProjectionPoint { pub x: f64, pub y: f64, pub p: f64, pub t: f64, pub wall: f64 }
#[derive(Serialize, Clone)] #[serde(rename_all = "camelCase")] pub struct ProjectionStatus { pub open: Option<MonitorInfo> }
pub const MAX_PROJECTION_POINTS: usize = 4096;     // per call
pub const MAX_PROJECTION_STROKES: usize = 1 << 20; // per tail

#[tauri::command] pub async fn projection_open(app: AppHandle, context: State<'_, AppContext>,
    runtime: State<'_, LiveRuntime>, projection: State<'_, ProjectionRuntime>, monitor: Option<String>) -> CommandResult<MonitorInfo>
#[tauri::command] pub async fn projection_close(app: AppHandle, projection: State<'_, ProjectionRuntime>) -> CommandResult<()>
#[tauri::command] pub async fn projection_watch(projection: State<'_, ProjectionRuntime>,
    on_event: Channel<ProjectionStatus>) -> CommandResult<ProjectionStatus>
#[tauri::command] pub async fn projection_subscribe(projection: State<'_, ProjectionRuntime>,
    on_event: Channel<LiveBatch>) -> CommandResult<LiveSnapshot>
#[tauri::command] pub async fn projection_update(projection: State<'_, ProjectionRuntime>,
    update: ProjectionUpdate) -> CommandResult<()>
```

- **`ProjectionRuntime`** (managed): a `Mutex<Projection>` with `scene: Option<Scene>` (the note, its image key and
  pixel size, the view, the revision), `current: Option<(u32, LiveStroke)>` (the stroke in progress), `subscriber:
  Option<Channel<LiveBatch>>` (the window's latest subscription), `watcher: Option<Channel<ProjectionStatus>>` (the
  editor's), `open: Option<MonitorInfo>`. No thread: the editor paces its calls (one in flight), each update is applied
  and its batch sent from the command, `sent_at` stamped as the hub does.
- **Events**, built from `live/model.rs` types:
  - `Page` → `Snapshot` of a one-page session: `state: Live`, `page` = the note (its own id: a local window),
    `page_index: 0`, `page_count: 1`, `can_turn_page: false`, `layer`/`computer_layer: 0`, `style` default, `view` =
    `{ zoom, x, y, board: false, laser: false }` (sanitized with `LiveViewState::sanitized`), `hover: None`,
    `presentation: open`, `saved: []`, `revision` = the scene's, no undo, `page_paper` its paper, `image_pixels` from the
    staging (`background::staging().get(key)`: size; `None` when not staged); then the stroke in progress if any.
  - `Strokes` → `StrokesEdited { page_index: 0, revision: +1, removed: keep..old_len, inserted: tail at keep.., recoloured:
    [], can_undo/redo: false, viewers: None }`, then `StrokeCancel { pen: Computer, id }` when `ended` names the stroke in
    progress (the same batch: no flash). Refused with **`projectionStale`** when `keep` exceeds the scene's strokes or there
    is no scene: the feed sends `Page` again.
  - `View` → `View`. `StrokeBegin`/`StrokePoints`/`StrokeCancel` → `StrokeStart`/`Points`/`StrokeCancel` with
    `pen: Computer`, `id`, points as `LivePoint { x, y, p, t, recv: wall }`. A `StrokeBegin` replaces a stroke in
    progress; `StrokePoints` for another id is ignored.
  - Bad numbers (`keep`, `id`, `layer` not whole, non-finite points, more than the limits): `projectionInvalid`
    (logged by the feed, never shown: the next `Page` repairs).
- **Subscription**: `projection_subscribe` returns the snapshot (an `Idle` one with an empty A4 page while no scene: the
  window stays black) and starts the channel with it and the stroke in progress, as `LiveHub::subscribe` (hub.rs:1196–1225).
  A failed send drops the subscriber. Updates while nobody subscribes are applied, not sent.
- **`projection_open`**: the screen as `open_presentation` chooses it (`choose_monitor`, the saved projector, saved
  again: one projector per PC for both windows), then `show_on` with label `projection` and URL
  `index.html?view=projection`; it first destroys a live `presentation` window if one exists (they never coexist in the
  interface; the assistant's bridge drives live only), and **never** calls `hub.set_presentation`. Sets `open`, tells
  the watcher. Errors: `noMonitor`, `unexpected`.
- **`projection_close`**: destroys the `projection` window. **Closed by any means** (Esc in it, `projection_close`, the
  main window going): `on_window_event` gets a `PROJECTION` branch: `ProjectionRuntime::closed()` drops the scene, the
  stroke and the subscriber, sets `open: None` and tells the watcher. `open_presentation` (live) destroys a
  `projection` window first, symmetrically.
- **`presentation.rs`**: `show_on(app, label, url, monitor, activity)` (today's body, parametrised; the same
  `Activity` token, so the indexer waits for this window too); `MAIN` destroyed takes both windows along.
  `capabilities/presentation.json` lists `["presentation", "projection"]` (package 0).

### 2.4 The feed (package D): `lib/projection-feed.ts`

```ts
/** What the editor tells of its stroke in progress (Canvas's `oninprogress`, package E calls it). */
export type InProgress =
  | { kind: 'begin'; id: number; tool: Tool; color: Rgba; width: number; layer: number; points: readonly InkPoint[];
      /** Wall clock (Unix ms) at note time `base`: a point's wall time is `wallAtBase + (p.t - base)`. */
      wallAtBase: number; base: number }
  | { kind: 'grow'; id: number; points: readonly InkPoint[] }     // all the points so far (the feed sends the new ones)
  | { kind: 'end'; id: number; committed: boolean };              // committed: the doc gets the stroke next
/** The update that brings what was sent to `note`, or null when nothing changed (pure; tested alone). */
export function projectionDiff(sent: SentPage | null, note: Note, imageKey: string | null, view: ProjectionView): ProjectionUpdate | null;
export class ProjectionFeed {
  constructor(send: (update: ProjectionUpdate) => Promise<void>);
  show(note: Note, imageKey: string | null): void; view(view: ProjectionView): void; stroke(event: InProgress): void;
  reset(): void;    // the next show sends the whole page (opened again, `projectionStale`)
}
```

- **Diff by identity.** `SentPage` remembers the last page sent: id, page size, background, background image, layers,
  image key, and the stroke array (the objects). A different id, page, background, image, layers (compared by value)
  or image key → `Page`. Else `k` = length of the common prefix by `===`; nothing changed → null; else
  `{ kind: 'strokes', keep: k, tail: note.strokes.slice(k) }`. Undo, redo, erase, move, recolour and the replay's
  `noteAt` all keep the objects they do not change (immutable doc, `lib/editor/state.ts`; replay.ts:226–251), so a
  typical change sends one stroke or none. A tail of more than half the strokes is sent as `Page` (same size, simpler).
- **One call in flight**; while it runs, `show` only records the latest note, the stroke's new points accumulate (at
  most `MAX_PROJECTION_POINTS` a call), and when the call ends the next update is computed against what was sent.
  Order: stroke events before a strokes update that follows them. `end { committed: true }` is held and sent as `ended`
  with the next strokes update; if none comes within 1 s, a `strokeCancel`. `committed: false` → `strokeCancel` at once.
- A rejected call (`projectionStale`, `projectionInvalid`, IPC) → `reset()`, the next show sends `Page`.

### 2.5 The projection window (package D)

`components/projection/EditorPresentation.svelte` (new), mounted by `main.ts` for `?view=projection`:

- `const live = new LiveState()`, `subscribeLive(live, api.projectionSubscribe)` (same signature as `api.liveSubscribe`,
  live-state.svelte.ts:565–575); `LiveCanvas mode="presentation"` with two **new `LiveCanvas` props** (declared by
  package 0, implemented by B): `interactive?: boolean` (default true; false: no marks, no pinch, no wheel, the pointer
  only wakes the cursor) and `paper?: (page: PageSize, background: Background, widthPx: number) => Promise<string>`
  (default today's choice between `live_page_background` and `render_background`). The editor's window passes
  `interactive={false}` and a paper that calls `api.renderBackground(page, background, widthPx, image)` with
  `{ image: live.pageImage, key: live.imagePixels.key }` when both are set: the editor's staged pixels, in the same
  process (`background::source_picture`, background.rs:521–538).
- No interface, black surround, the cursor hidden after 2 s (as Presentation.svelte:33–94); `exposeLatency(latency,
  window, computerLatency)`: the editor's points are computer points (`__ardoiseLatency.computer`).
- Keys: Esc, Ctrl+W, Ctrl+F4, Ctrl+Q → `api.projectionClose()`; nothing else (§11 O6). Accessible names
  `presentation.editorTitle`, `presentation.editorCanvas`.
- `LiveCanvas` test attributes added by B, read by the journeys: `data-drawn` (strokes drawn, visible layers only, the
  one in progress included), `data-paper` (`white|cream|kraft|slate|custom`, the effective paper), `data-template`,
  `data-image` (the image's name, empty without one), `data-paper-ready` (`true` once the picture of the current paper
  is drawn), `data-zoom` (the view's zoom).

### 2.6 The editor (D for its modules, E for the wiring)

`lib/editor/projection.svelte.ts` (D; package 0 commits its API):

```ts
export type ProjectionCommands = Pick<typeof api,
  'listMonitors' | 'projectionOpen' | 'projectionClose' | 'projectionWatch' | 'projectionUpdate'>;
export class EditorProjection {
  monitors = $state.raw<MonitorInfo[]>([]); monitorKey = $state(''); loaded = $state(false);
  /** The screen projected on, or null. */  presentation = $state.raw<MonitorInfo | null>(null);
  busy = $state(false); error = $state<string | null>(null);
  /** The projection's own zoom (1 = the whole page). */  zoom = $state(1);
  /** The projected area on the page (page units) while projecting with a zoom above 1, for the editor's frame. */
  readonly frame: Rect | null;
  constructor(commands?: ProjectionCommands);
  watch(): void; refresh(): Promise<void>; open(): Promise<void>; close(): Promise<void>;
  show(note: Note, imageKey: string | null): void;  // while open: what the editor shows (a replay included)
  stroke(event: InProgress): void;                   // while open: the stroke in progress
  visible(rect: Rect): void;                         // what the editor's canvas shows of the page
  zoomStep(direction: 1 | -1): void; wholePage(): void; pageWidth(): void;
  dispose(): void;                                   // the editor goes: closes the projection
}
```

- **The projected area** (§11 O5): centred on the centre of what the editor shows (`visible`, clamped to the page by
  `clampView`, `lib/live-view.ts:85`), at the projection's own zoom, in the projector's shape (`aspectOf` of the
  monitor's size). « Page entière » is zoom 1; « Largeur » the zoom of `fitWidthView` (live-view.ts:175) at least 1; −
  and + steps of `ZOOM_STEP` (live-view.ts:18) between 1 and 8. Moving or zooming the editor's own view moves the area,
  never changes its zoom. Each change sends `view` (at most one per frame).
- `components/projection/EditorProjectionTab.svelte` (D): `ProjectorBlock` (new optional prop `help`, here
  `editor.projection.help`), `ZoneBlock` (new optional prop `widthTitle`, here `editor.projection.widthTitle`; `hint`
  `editor.projection.zoneHint`; no children: no blackboard, no laser, Q13), then `ShareBlock`. Same hooks as live
  (`project`, `stop-projecting`, `[data-presentation]`, `[data-zoom]`, `whole-page`, `page-width`, `share`).
- **E wires it in `Editor.svelte`**: `const projection = new EditorProjection()`; `projection.watch()` at mount; an
  `$effect` calls `projection.show((replayDoc ?? doc).note, doc.backgroundPixels?.key ?? null)` while
  `projection.presentation`; the badge `projection: { dot: share.status.active || projection.presentation ? 'live' : undefined }`;
  the `projection` snippet becomes `<EditorProjectionTab {projection} />`; `projection.dispose()` in the destroy
  (556–576). Switching notes (`open`, 579) keeps the projection: the next `show` is another note's `Page`.
- **`Canvas.svelte` (E)** gets three props: `oninprogress?: (event: InProgress) => void` (begin at pen down with
  `wallAtBase = performance.timeOrigin + d.t0` and `base = d.base`, grow after each move's points, end in `finish` with
  `committed = !cancelled`, `dropDrag` → end false); `onvisible?: (rect: Rect) => void` (`visibleRect(view, viewport)`,
  `lib/editor/view.ts`, at each change of the view or the size); `projected?: Rect | null` (a dashed rectangle in page
  units, `vector-effect: non-scaling-stroke`, `pointer-events: none`, `data-slot="projected-area"`).

### 2.7 What the projection shows, case by case

| Case | Projection |
|---|---|
| A stroke being written (pen, mouse, finger when it draws) | streamed point by point; joins the page when lifted |
| Undo, redo, erase (released), move, scale, recolour, paste, straighten | the result, one `strokes` update per change |
| An eraser or a move being dragged | the result when released (the drag's transient doc is not fed) |
| Selection, lasso, eraser ring, straighten preview, split preview, crop rectangle | never (as live: the result only) |
| Layer shown or hidden, renamed, moved, merged | `Page` (layers changed); hidden layers never drawn (`LiveRenderer`) |
| Paper, image, page size, rotation, crop, « Revenir à l'original » | `Page` |
| Background image (or PDF page) | the editor's staged pixels; while another note's image is being staged, the paper colour |
| A replay | the replay as the editor shows it (`replayDoc`), one `strokes` update per frame (O7) |
| Another note (Alt + ←/→, the library's next) | the next note's `Page`, the projection stays open (O9) |
| Back to the library, live mode resumed, the window closed | the projection closes (`dispose`) |
| Esc on the projector | closed; the tab and the badge follow (`projection_watch`) |
| Live mode starts | never at once with the editor (Home.svelte); `open_presentation` would close it anyway |
| The share runs | both: the share still shows the note (debounced), not the projected area (O19) |
| The assistant's `presentation` tool | live only, unchanged (O18) |

### 2.8 Latency and performance

- Target for the stroke in progress (no SPEC figure for the editor): as M14's computer pen, a median under 30 ms from
  the pointer sample to the projection's frame, 50 ms accepted (M14 measured 40 ms on the same kind of path, window →
  Rust → window, tablet-live-classroom.md "Latency targets"). Measured by `__ardoiseLatency.computer` in the projection
  window (the points' `recv` is their sample's wall time).
- The editor's pen is not slowed: the feed's work in a pointer move is an array push; diffs are one identity pass over
  the strokes per change (well under a millisecond for 10 000 strokes, supposé: D measures in a vitest).
- A replay at 60 frames a second sends the partial stroke each frame: a long stroke's repaint area grows towards a full
  redraw (over 60 % of the canvas: `planRepaint`, M12). If §8's measure shows late frames on the projector, D sends a
  replay's partial stroke as a stroke in progress instead (R2).

### 2.9 Tests

- **C (Rust)**, `projection/tests.rs`: each update's events (snapshot fields, `strokesEdited` indices, `ended` →
  `strokeCancel` in the same batch, points with `recv`), `projectionStale`/`projectionInvalid`, a late subscriber gets
  the snapshot and the stroke in progress, updates without subscriber kept, `closed()` resets and tells the watcher,
  the snapshot's `image_pixels` from a staged image; `presentation.rs`: the two labels and URLs (pure parts).
- **D (vitest)**: `projection-feed.test.ts` (append, undo, middle edit, recolour all → `Page`, layout change, image key,
  `ended` attached to the next strokes update, 1 s fallback, cancel, coalescing with one call in flight, stale → `Page`,
  replay frames reuse objects); `editor/projection.test.ts` (open/close/watch, follow the visible centre, clamp, aspect
  from the monitor, whole/width/zoom steps, `frame`); `EditorPresentation.test.ts` (subscribes, draws strokes and the
  stroke in progress, Esc closes, no marks with a pen, the paper asks `render_background` with the key);
  `EditorProjectionTab.test.ts`; `mock-projection.ts` follows C's rules; ProjectorBlock/ZoneBlock new props.
- **E (vitest)**: Canvas reports `oninprogress` (begin/grow/end, cancelled), `onvisible`, draws `projected`; the
  editor feeds while open, during a replay too, stops on destroy, keeps it on another note, the blue dot.

---

## 3. Editor: the sound while writing

### 3.1 Today

- **Stroke times.** A new stroke starts 500 ms after the note's last sample: `base = lastTime(doc.note) + 500` at pen
  down, then `t = base + (e.timeStamp − t0)` (`Canvas.svelte:324–334`, 508–512); a paste goes `PASTE_GAP_MS` after the
  note's end (`retimedForPaste`, `lib/editor/ops.ts:212–222`; `pasteStrokes`, `lib/editor/state.ts:161–170`).
- **The narration** is one `audio.opus` per note with its time base (`crates/ardoise-media/src/narration.rs:47–68`,
  `docs/file-format.md` §4.1): `replay` (recorded during a replay, with its speed and trimming) or `note` (live mode:
  speed 1, no trimming), and `start_ms`, the replay instant of its first sample.
- **The editor records only during a replay** (`EditorSound.svelte:68–122`; `Editor.svelte:1627–1664`): target
  `replay` (`narration.rs:86–102`); the first sample is placed from the wall clock at which the replay was at instant
  0 (`replay_start_ms`, 860–866); at stop the sound is written straight into the note file with the editor's stamp
  (`save_replay`, 1138–1186; `save_replay_audio`, 952–968), whatever the editor has not saved
  (`narration.record.saveNote`); a failed save stays pending with a backup copy (`keep_pending`, 712–730).
- **Replacing** a narration is said, not asked (`[data-slot="replaces"]`, `RecordPanel.svelte:123–125`). The narration's
  info is read in a replay only (`loadNarration`, Editor.svelte:1561–1576; M16 O7).
- **Replay with the narration**: the player takes the narration's options and span (Editor.svelte:1578–1599), the sound
  starts at `startMs` (`ReplayBar.svelte:62`, `narration-playback.ts`); the video does the same
  (`plan_export`, `crates/ardoise-media/src/export.rs:151–200`: the narration's options, `start_ms` as video time,
  the tail held until the sound ends).
- **Live mode on a resumed sheet** puts new ink after the last stroke and the narration's end, plus 1 s
  (`time_base`, `live/resume.rs:214–230`; `RESUME_GAP_MS`, 25–28): the origin rule reused below.
- **Microphone**: `NarrationRuntime` opens it through an `Opener` (`narration.rs:554–569`), which the tests replace by
  a fake (`narration/tests.rs:62–100`); `simulated_narration` makes a deterministic voice-like clip
  (`crates/ardoise-media/src/microphone/simulated.rs`). The journeys never record (`app/e2e/side-panel.mjs:56–90`,
  `ARDOISE_E2E_MICROPHONE`).
- **While recording**, leaving, switching notes, resuming live and closing wait (Editor.svelte:973–995, 1040–1075);
  the replay button is disabled (`replayDisabled`, 2137).

### 3.2 The time model

- **Origin** (Q13: « après les traits déjà présents »): `T0 = writingOrigin(note)` = the note's last sample time
  (hidden layers included) + `WRITING_GAP_MS` (1000, `RESUME_GAP_MS`'s value), or 0 for a note without ink. The
  narration being replaced does not count (it goes).
- **Clock** (`lib/editor/writing-clock.ts`, E): when `narration_start` has resolved, `P0 = performance.now()`;
  `noteTime(timeStamp) = T0 + (timeStamp − P0)`; `noteZeroWallMs = performance.timeOrigin + P0 − T0`, the wall clock
  at which note time would be 0.
- **Strokes**: `Canvas.svelte` gets `strokeClock?: { noteTime(timeStamp: number): number } | null`; at pen down,
  `base = strokeClock ? round(strokeClock.noteTime(e.timeStamp)) : lastTime(note) + 500`, then as today. A stroke begun
  before the start keeps the editor's rule; one still drawn at the stop keeps the sound's clock to its end.
- **The sound**: at stop the editor passes `noteZeroWallMs` as `replayStartedAtMs` (the existing fourth argument of
  `narration_stop`); Rust computes `start_ms = replay_start_ms(wall_start_ms, noteZeroWallMs)` (the existing function:
  the first sample's wall clock less note time 0's) and makes `Narration { base: Note, options: default, start_ms }`.
  A point drawn at wall time `w` has `t = T0 + (w − P0wall)`; the sound's sample at `w` plays at
  `start_ms + (w − W0) = T0 + (w − P0wall)`: the same instant. `start_ms` may be a few hundred ms below `T0` (the
  microphone opened before `P0`): within the 1 s gap.
- **No format change**: a note-time narration is what live mode writes (M6); a replay of it uses speed 1 and keeps the
  pauses, and the strokes keep their real rhythm, silences while talking included.
- **After the recording**, the editor's rule puts a stroke 500 ms after the last sample, which may fall inside the
  sound (the teacher spoke after the last stroke): `nextStrokeBase(note, narration)` puts it after the end of a
  note-time narration too (O14).

### 3.3 Rust (package F)

- `NarrationTarget::Writing { path: String }`: `start_in` checks the note exists (as `replay_target`), records with a
  watcher (no page), `Purpose::Writing`. `Stopped::Writing { target, wall_start_ms }`.
- **The sound waits for the note** (O10): at stop with `save`, `save_stopped` encodes it once and keeps it
  **staged** in the runtime (one slot, `Mutex<Option<StagedNarration { key, audio, duration_ms, backup }>>`; a newer
  one replaces it and removes its copy), writes the backup copy (`write_narration_backup`, as `keep_pending`) and returns
  `NarrationSaved { stamp: None, duration_ms, pages: 1, staged: Some(key) }` (`staged` new, skipped when `None`). Empty
  clip → `emptyRecording`; silent microphone → `microphoneSilent` (today's); nothing staged then.
- **`OpenedNote.narration`** (`notes.rs:86–92`; package 0 adds the field as `None`, F fills it): the note's
  `NarrationInfo` (narration.rs:179–203) from the member `open_in` already read.
- **`save_note(…, narration_key: Option<String>)`** (`notes.rs:210–229`; package 0 adds the parameter, F implements;
  the command gets `State<NarrationRuntime>`): with a key, `save_with_key_and_account` writes `file.audio = staged.audio`
  in the same atomic write as the note (`library.save`, with its stamp check); on success the slot and its backup copy
  go. An unknown key → `noStagedNarration`, nothing written. A failure (conflict, disk) leaves the slot.
- `#[tauri::command] narration_drop_staged(key: String) -> ()`: drops the slot when the key matches, removes the copy
  (the user chose « Ne pas enregistrer » or reloaded).
- **Simulated microphone** (`--simulated-microphone`, parsed by package 0 in `lib.rs`; `narration/simulated.rs`, F):
  `NarrationRuntime::with_simulated_microphone(true)` uses an opener whose `Source` has `device_name` « Simulated
  microphone », `elapsed()` = the time since it opened (recording), `level()` from `simulated_narration` at that
  instant, `status()` `Receiving`, and `stop()` = `simulated_narration(elapsed, 1)`; `list_microphones` lists that one
  device, default. Never the PC's microphone. Cheap: about 120 lines with tests.
- Session end and crash: as today for a replay recording, nothing special (only live mode keeps a WAV copy, M12
  "Session end"; the editor tells Windows of unsaved edits through its drafts, `setUnsavedWork`, Editor.svelte:392–397,
  not of a recording under way): a recording under way is lost, a staged sound not yet saved survives as its backup
  copy (R4).

### 3.4 The editor (package E)

**Son tab** (`EditorSound.svelte`), outside a replay:
- a segmented control `div.pane-seg[role="radiogroup"][data-field="narration-mode"]` of two radios
  `input[value="writing"|"replay"]` (`editor.sound.modeWriting`, `editor.sound.modeReplay`; group label
  `editor.sound.mode`), remembered per PC in `ardoise.editor.narrationMode` (browser storage, as the side panel's
  choices), default `writing` (the mockup's; O12);
- the stored narration's line (`narration.stored.info` / `.none`), no longer only in a replay: `open_note` now says it
  (`OpenedNote.narration: NarrationInfo | null`, F: `open_in`, notes.rs:94–106, reads the header of the member it has
  already loaded, `narration_header`, no decoding; a damaged member gives null, the replay still says why), and the
  editor reads it again (`api.narrationInfo`) after a recording or a deletion; `nextStrokeBase` (§3.2) needs it at every
  pen down;
- **writing**: `p.pane-help` `editor.sound.writingHelp`, then `RecordPanel` with `openLabel`
  `editor.sound.recordWriting`, `openHint` `editor.sound.recordWritingHint`, `info` `editor.sound.writingInfo`,
  `saveNote` `editor.sound.writingSave`, `duringInfo` `editor.sound.writingDuring`, `replaces`, then the deletion of the
  stored narration (the replay's, `narration-delete`);
- **replay**: today's M16 content (`editor.sound.replayFirst`, `sound-replay`). During a replay: today's content
  whatever the mode.

**The replace question** (Q13 « demande d'abord si l'on remplace le premier »; both modes, O11): in `RecordPanel`, with
`replaces`, « Démarrer » shows `div[data-slot="replace-question"][role="alert"]` (`narration.record.replaceQuestion`
with the stored duration) and `button[data-action="narration-replace-yes"]` (`replaceYes`) /
`[data-action="narration-replace-no"]` (`replaceNo`); only « Remplacer » calls `onstart`. Live passes no `replaces`: no
change there.

**Start** (writing): `recorder.start(microphone, { kind: 'writing', path: currentPath })`; on success
`clock = new WritingClock(writingOrigin(doc.note), performance.now())` and `Canvas strokeClock={clock}`.

**Stop**: `recorder.stop(true, null, null, clock.noteZeroWallMs)`; on `saved.staged`: `stagedNarration = { key,
durationMs }`, the note is dirty (`dirty` includes it; `writeNote` no longer returns early while one is staged), and
the editor **saves at once** (`save()`; O10) with `api.saveNote(path, note, stamp, imageKey, stagedNarration.key)`.
Success: `stagedNarration = null`, notice `editor.sound.saved`, the narration reread. Failure: notice
`editor.sound.notSaved`, the conflict banner as for any save (« Enregistrer une copie » passes the key too: the strokes
timed on the sound go with it); « Recharger » and « Ne pas enregistrer » call `narrationDropStaged`. **Cancel** drops the
sound and keeps the strokes (O16).

**Gates while a writing recording runs** (O20): today's (leave, switch notes, resume live, close, replay), plus
« Revenir à l'original », the split and the video export (they act on the timeline the recording is writing); each shows
`editor.replay.recordingBusy`. Everything else edits as usual (§3.5).

### 3.5 Edits during a writing recording

| Edit | Times |
|---|---|
| Pen, mouse, finger (with « Dessiner au doigt ») | the sound's clock |
| Undo / redo | the stroke goes or comes back with its time; the sound is untouched |
| Eraser (whole or partial) | removed strokes go; pieces keep their samples' times |
| Move, scale, recolour, change tool, delete, copy | times unchanged |
| Paste (Ctrl + V) | the copies' first sample at the sound's current time, their rhythm kept: `pasteStrokes(doc, strokes, at)` and `retimedForPaste(note, strokes, at?)` (O15) |
| Duplicate | as today (copies at the originals' times) |
| Straighten (F) | the shapes keep the times of the strokes they replace (`ardoise-ink` shapes, supposé: E tests one) |
| Layers, paper, image, page size, rotation, crop | times unchanged |
| Original, split, video, replay | refused while recording (§3.4) |

« Remplacer » means: the old sound goes; the strokes that were timed on it stay, as ink at their times (their pauses
are then ordinary dead time, trimmed like any other when « supprimer les temps morts » is on, and before the new sound
by package G). A recording longer than the strokes: the replay and the video last until the sound ends (the player's
`extent`, `plan_export`'s tail: unchanged).

### 3.6 The replay and the video, and the dead time before the sound (package G, optional)

A note-time narration is played at speed 1 without trimming, **the whole note**: a page written in class (real
tablet times, minutes of pauses) and commented later replays all its old ink in real time before the voice, in the app
and in the video. G adds a playback rule (O13), stored nowhere: **with a narration in note time, the pauses of the
ink before the sound's first sample are cut after `TRIMMED_PAUSE_MS` (500 ms), as « supprimer les temps morts »
does; from the sound on, note time is kept.** Old readers play in plain note time: in sync, longer.

- Rust (`crates/ardoise-media`): `ReplayOptions { speed, max_pause, lead: Option<Lead> }`, `Lead { until_ms: f64,
  max_pause: Duration }` (`Default`: `None`, so `options == ReplayOptions::default()` stays true for stored headers,
  `live/resume.rs:260, 282`); `Clock::new` cuts only the pauses before `until_ms` (a pause straddling it: only its part
  before); `Narration::replay_options()` and `NarrationHeader::replay_options()` give `lead: Some(start_ms, 500 ms)` for
  `base == Note && start_ms > 0`, the stored options otherwise; `Narration::replay_start(&Timeline) -> i64`: the replay
  instant of the first sample (`timeline.to_replay(start_ms)` with a lead, `start_ms` otherwise). `plan_export`
  (export.rs:158, 167) takes both. M12's merges stay in note time.
- TS (`lib/replay.ts`): `ReplayOptions.lead?: { untilMs: number; maxPauseMs: number } | null`, `ReplayTimeline` cuts the
  same; `narrationReplay(info): ReplayOptions` and `narrationSoundStart(timeline, info): number` (package 0 stubs: the
  stored options and `info.startMs`). E uses them where the editor plays along (Editor.svelte:1591–1593) and in
  `ReplayBar.svelte:62`.
- It also shortens the replay of M12's live narrations on resumed sheets, the same case. Tests: the TS cases mirror the
  Rust ones (as `replay.test.ts` does, its line 1).
- The Son tab says it under the stored narration's line when the narration is in note time and the note has ink before
  it (`p.pane-help[data-slot="narration-lead"]`, `editor.sound.leadTrimmed`; E, once G is in).

### 3.7 Edge cases

| Case | Behaviour |
|---|---|
| A note without ink | `T0 = 0`; the sound may start a few ms before note time 0 (a lead-in) |
| The note has a narration (any base) | the question; « Garder l'ancienne » starts nothing |
| A staged sound not yet saved, and a new recording | it counts as the note's narration: the question; replacing drops the staged one |
| Nothing written during the recording | the sound is saved alone, after the ink (a commentary) |
| Every stroke of the recording undone, then saved | the sound stays (one narration); the user deletes it if they want |
| Microphone blocked by Windows (silent) | `microphoneSilent` at stop, nothing staged, the strokes stay |
| The note changed on disk meanwhile | the save at stop fails as `noteConflict`: the banner; a copy gets the sound |
| The app crashes after stop, before the save | the draft holds the strokes, the backup folder the sound (R4) |
| The editor goes away during a recording (forced) | `salvageRecording` stops and stages it: backup copy only |
| Projection on | the strokes stream as any; the sound is the editor's |
| Shortcuts | none added (§6) |

### 3.8 Tests, without the PC's microphone

- **F (Rust)**: the writing target starts and stops with the fake opener (`narration/tests.rs`); `start_ms` from
  `noteZeroWallMs` and the watcher's anchor; `base: Note`, default options; the staged slot (replaced, dropped, its
  backup removed); `save_note` with a key writes note and audio in one save, refuses an unknown key, keeps the slot on a
  conflict; the simulated microphone (elapsed grows with time, the clip's length, one device listed, a monitor's level).
- **E (vitest)**: `writing-clock.test.ts` (`writingOrigin` with hidden layers, empty note, `noteTime`,
  `noteZeroWallMs`, `nextStrokeBase` with a note-time narration); `Canvas` (strokes follow `strokeClock`, the old rule
  without it); `pasteStrokes` at a time; `RecordPanel` (the question, both answers); `Editor.sound.test.ts` (new: the mode
  switch and its storage, the stored line outside a replay, the question with a stored or a staged narration, start
  with the writing target, stop → `saveNote` with the key, a failed save keeps it and « Ne pas enregistrer » drops it, the
  gates, cancel keeps the strokes); `mock-narration` (writing target, staged key, drop), `mock-notes` (`narrationKey`).
- **G**: timeline cases (cut before, kept after, a pause straddling the start, round trips), `plan_export` of a note
  with old ink and a note-time narration: shorter video, the sound's start at the trimmed instant; the same numbers in TS.
- **e2e** (§7) with `--simulated-microphone`: the PC's microphone is never switched on.

---

## 4. File format

None of the three features changes the `.ardoise` format nor its version (§Rules). H updates `docs/file-format.md`:
§4 `audio.opus` « recorded during a replay, in live mode or while writing in the editor »; §4.1 `ARDOISE_BASE=note`
« in live mode or while writing in the editor (note time) » and, with G, the playback rule of §3.6 (a rule of the
replay, like the existing sentence on video exports; no comment is added, nothing to read back).

## 5. Translations (package 0, both catalogs)

New (French in the user's tone: short, plain, « » and plain spaces):

| Key | Français | English |
|---|---|---|
| `live.page.help` | Pour la page en cours, au vidéoprojecteur et sur le partage aussi. Une nouvelle page garde ce papier. | For the current page, on the projector and on the share too. A new page keeps this paper. |
| `live.page.board` | Tableau noir : la couleur du papier reviendra quand il sera éteint (B). Le modèle et la texture s'appliquent tout de suite. | Blackboard on: the paper's colour comes back when it is turned off (B). The template and the texture apply at once. |
| `live.page.slate` | Pour écrire à la craie, allumez plutôt le tableau noir (B). | To write in chalk, turn the blackboard on instead (B). |
| `live.page.back` | Pour revenir en arrière, rechoisissez l'ancien réglage : Ctrl+Z n'annule que ce qu'écrit l'ordinateur. | To go back, choose the previous setting again: Ctrl+Z only undoes what the computer writes. |
| `live.background.help` | Sous l'écriture de la page en cours : une fiche ou un document à compléter devant la classe. La page suivante n'a pas d'image. | Under the writing of the current page: a worksheet or a document to fill in front of the class. The next page has no image. |
| `live.failed.paper` | Le papier de la page n'a pas pu être changé. | The page's paper could not be changed. |
| `live.failed.image` | L'image de fond n'a pas pu être changée. | The background image could not be changed. |
| `live.failed.pageStale` | La page a changé entre-temps : refaites le choix. | The page changed in the meantime: choose again. |
| `editor.projection.help` | La page de l'éditeur en plein écran sur le vidéoprojecteur, mise à jour à chaque trait. Elle s'arrête quand vous quittez l'éditeur. | The editor's page full screen on the projector, updated at every stroke. It stops when you leave the editor. |
| `editor.projection.zoneHint` | La zone projetée suit ce que montre l'éditeur ; un cadre en pointillés la montre sur la page. | The projected area follows what the editor shows; a dashed frame shows it on the page. |
| `editor.projection.widthTitle` | Afficher la page sur toute la largeur du vidéoprojecteur | Show the page across the projector's width |
| `presentation.editorTitle` | Projection de l'éditeur | Editor projection |
| `presentation.editorCanvas` | Page projetée de l'éditeur | The editor's projected page |
| `editor.sound.mode` | Enregistrer le son | Record the sound |
| `editor.sound.modeWriting` | Pendant que j'écris | While I write |
| `editor.sound.modeReplay` | Pendant une relecture | During a replay |
| `editor.sound.writingHelp` | Le son et ce que vous écrivez sur l'ordinateur (stylet, souris, ou doigt si « Dessiner au doigt » est activé) sont enregistrés ensemble : la relecture les rejoue en même temps, après ce que la note contient déjà. | The sound and what you write on the computer (pen, mouse, or finger when “Draw with a finger” is on) are recorded together: the replay plays them back together, after what the note already holds. |
| `editor.sound.recordWriting` | Enregistrer en écrivant | Record while writing |
| `editor.sound.recordWritingHint` | Parler en écrivant : les traits prennent l'heure du son | Talk while writing: the strokes take the time of the sound |
| `editor.sound.writingInfo` | Les traits écrits pendant l'enregistrement prennent l'heure du son. | The strokes written during the recording take the time of the sound. |
| `editor.sound.writingSave` | À l'arrêt, la note est enregistrée avec le son et ce que vous avez écrit. | When you stop, the note is saved with the sound and what you wrote. |
| `editor.sound.writingDuring` | Écrivez : vos traits suivent le son. | Write: your strokes follow the sound. |
| `editor.sound.saved` | Son enregistré ({duration}) avec vos traits. | Sound saved ({duration}) with your strokes. |
| `editor.sound.notSaved` | Le son est gardé en mémoire : il ira dans la note à son prochain enregistrement. | The sound is kept in memory: it goes into the note the next time it is saved. |
| `narration.record.replaceQuestion` | Cette note a déjà une narration ({duration}). La remplacer par ce nouvel enregistrement ? | This note already has a narration ({duration}). Replace it with this new recording? |
| `narration.record.replaceYes` | Remplacer | Replace |
| `narration.record.replaceNo` | Garder l'ancienne | Keep the old one |
| `errorCodes.noStagedNarration` | le son enregistré n'est plus en mémoire ; une copie est dans le dossier de secours des narrations | the recorded sound is no longer in memory; a copy is in the narrations' backup folder |
| `errorCodes.projectionStale` | la projection a pris du retard ; elle se remet à jour | the projection fell behind; it catches up |
| `errorCodes.projectionInvalid` | la projection a reçu une mise à jour incorrecte ; elle se remet à jour | the projection got a wrong update; it catches up |
| `editor.sound.leadTrimmed` (G) | Avec la narration, les temps morts avant le son sont raccourcis. | With the narration, the dead time before the sound is shortened. |

Changed: none (`narration.record.replaces` and `editor.sound.replayFirst` stay as they are). Reused as they are:
`editor.page.paperTitle`, `editor.papers.*`, `editor.templates.*`, `editor.page.template|texture`,
`background.editor.*`, `background.fits.*`, `live.projection.*`, `live.zoom.*`, `narration.stored.*`,
`narration.record.*`, `editor.replay.recordingBusy`, `side.dot.live`. A key found missing is added by the lead on
`m17` (packages do not edit the catalogs). `errorCodes.projection*` are never shown to the user by the interface (the
feed repairs); they exist because every code has a sentence (the error tests).

## 6. Shortcuts

No key is added in either window: no row in `EDITOR_SHORTCUTS` nor `LIVE_SHORTCUTS`, and the sweeps of M16 stay as
they are. The projection window takes Esc, Ctrl+W, Ctrl+F4 and Ctrl+Q to close itself, like live's presentation window
takes Esc, unlisted in either help as today. Live's Page Up / Page Down rule of §1.6 changes no chord's action, only
the targets where it applies (the sweep uses no target).

## 7. End-to-end journeys

Driven as the others (`app/e2e/run.mjs`, WebView2 over CDP, French and English), registered by package 0 as
`export const … = []`. Helpers (package 0, `app/e2e/m17-helpers.mjs`, complete): `penStroke(app, points, opts)` (the
`journeys-m8-input.mjs:43` one, exported), `BROWSER_ARGS` (journeys-m14-live.mjs:36), `projectionWindow(app)`
(`attach(app.port, (t) => t.url.includes('view=projection'))`, journeys-m12-resume.mjs:321), `canvasFacts(target)`
(the `data-*` of `canvas.live-canvas`, §2.5), `noteOnDisk(path)` (`readZip`, journeys-m12-resume.mjs:48; `note.json`,
`decodeStrokes`, journeys-m12-touchup.mjs:736, and the members' names), `opusTags(bytes)` (the `ARDOISE_*` comments read
as ASCII from the member). `side-panel.mjs` `checkRecordingDot(app, l, { simulated = false } = {})`: with `simulated`
(the journey runs with `--simulated-microphone`) it records for real; without it, today's skip.

- **`journeys-m17-live.mjs`** (B) « direct : papier et fond de la page en cours » (`--simulated-tablet`, the share on
  loopback, `BROWSER_ARGS`): open live, wait for the tablet's ink; project (`open_presentation` through
  `__TAURI_INTERNALS__`), attach the presentation; join the share's stream (`EventStream`); Page: Crème, Seyès, texture →
  the presentation's `data-paper="cream"`, `data-template="seyes"`, the stream's snapshot has a `paperKey` and a
  `livePaper` follows; `GET /paper/<key>` is a PNG (`readPng`, journeys-m7-background.mjs:53) with the Seyès lines' colour
  on a line; Fond: `window.__ardoiseE2eOpenPath` = `twoPagePdf()` (journeys-m7-background.mjs:27), page 1, Placer → the
  presentation's `data-image="fiche-e2e.pdf"`, `data-paper-ready="true"`, its pixels dark where the PDF's square lands
  (contained), cream elsewhere; another `paperKey`; opacity 50 %; with the template select focused, Page Down turns the
  page (`data-page` 2/2): the new page is cream Seyès with texture, `data-image=""`; ← back: the image is there; B →
  the swatches disabled and `paper-board` shown, Lined chosen under the board → `data-template="lined"`, B → cream back;
  Terminer: the first note's `note.json` has `background { paper: cream, template: lined, texture: true }`,
  `backgroundImage { name: fiche-e2e.pdf, pdfPage: 1, opacity: 0.5 }` and a `background.png` member; the second has the
  paper, no `backgroundImage`, no member. The M16 live journey is updated (six tabs, Page and Fond shown).
- **`journeys-m17-projection.mjs`** (D) « éditeur projeté : un trait de l'éditeur au vidéoprojecteur » (the share on
  loopback, `BROWSER_ARGS`): import `twoPagePdf()` as notes (as journeys-m7-background.mjs:104–127) and open page 1;
  Projection, Projeter → attach the projection window: `data-drawn` equals the editor's strokes
  (`.editor g.ink path[data-stroke]`), `data-image` the PDF's name; draw with the pen (`penStroke`) slowly: before it
  is lifted `__ardoiseLatency.computer.samples()` grows and `data-drawn` counts it; lifted: one more; median latency
  under 50 ms; Ctrl+Z → one less, Ctrl+Y → back; hide the chosen layer → fewer drawn, show it; + twice → `data-zoom`
  1.56 and the editor shows `[data-slot="projected-area"]`; scroll the editor (wheel) → the projection's view moves
  (its `data-zoom` unchanged); « Page entière » → zoom 1, no frame; Relecture (R) → `data-drawn` follows the replay
  (fewer, then back to all); Alt + → → the next note's strokes; start the share → the blue dot stays with both, the
  stream shows the note; Esc in the projection window → it is gone, `[data-presentation]` empty, no blue dot with the
  share stopped; Projeter again, back to the library → the window is gone.
- **`journeys-m17-sound.mjs`** (E) « son pendant qu'on écrit dans l'éditeur » (`--simulated-microphone`): open a demo
  note with ink (its last sample time read from the file); Son: the mode « Pendant que j'écris » is shown by default;
  open the recording: the picker lists « Simulated microphone »; Démarrer; two strokes 1.5 s apart; 1 s more; Arrêter →
  `editor.sound.saved`, no unsaved marker; the file: `audio.opus` with `ARDOISE_BASE=note`, `ARDOISE_START_MS` within
  [last + 500, last + 1300]; the first new stroke's first `t` − START_MS within 150 ms of its pen-down wall time −
  the recording's start wall time; the strokes' order after the old ink; Relecture with « Lire avec la narration »: the
  timeline lasts at least until the sound's end, and seeking just before / after the first new stroke's replay instant
  shows it absent / present; the video estimate (`estimate_video_export` with narration, MP4) reports `narrationMs` and
  a length to the sound's end; record again → the replace question; « Garder l'ancienne » → nothing recorded; again,
  « Remplacer » → a new `audio.opus` whose START_MS is after the newest stroke. The M16 editor journey's Son check
  becomes the mode switch, and both M16 journeys pass `--simulated-microphone` to `checkRecordingDot`.

## 8. Performance (`cargo xtask perf`, package D owns `app/e2e/perf.mjs` and `xtask/src/perf.rs`)

Two new parts in `PARTS` (`xtask/src/perf.rs:47`) and `perf.mjs`:
- **`paper`**: as `live` (`measureLive`, perf.mjs:407) with the presentation open (`BROWSER_ARGS`) and a page of
  `twoPagePdf()` under the live page, plus Seyès; the frame intervals of both windows while the simulated tablet
  streams. Row « Mode direct sur une page de PDF (≥ 60 i/s, ≤ 1 % d'images en retard) », judged with `LIVE_MIN_FPS` and
  `LIVE_DROPPED_MAX_PERCENT`.
- **`projection`**: a demo note open in the editor, the projection open, the pen through CDP in the editor as
  `measureLivePen` does (perf.mjs:614–726: a move every 8 ms, strokes of 1.5 s); rows « Projection de l'éditeur :
  latence du trait (médiane < 30 ms, 50 ms acceptés) » from `__ardoiseLatency.computer` in the projection window, and
  « Projection de l'éditeur : images par seconde au vidéoprojecteur (≥ 60 i/s) », plus a replay of a 1 000-stroke note
  projected (frames late on the projector, R2).

The hub test of §1.8 guards the tablet in `cargo test`. `docs/performance.md` is rewritten by `cargo xtask perf
--write` on the PC (H).

## 9. H7 procedure (for `docs/hardware-tests.md`, written by H)

```markdown
## H7 — Papier et fond en direct, projection de l'éditeur, son pendant qu'on écrit

**Durée** : 40 à 50 minutes. **Matériel** : le PC d'Ardoise (**la Surface et son stylet**), la vraie tablette
(chargée, dongle branché, téléphone Bluetooth **coupé**), le **vidéoprojecteur** en bureau étendu, le **PC du
vidéoprojecteur** sur le même réseau (le partage, voir H4), un élève ou un collègue qui écrit sur la tablette, le **micro
de la Surface**, et sur la Surface une **fiche en PDF de deux pages** et une **photo**. Ce test suppose que H1 à H6 ont
réussi.

Ce test vérifie SPEC §4.2 « Papier et fond pendant le direct », §4.4 « Projeter la note de l'éditeur » et §4.5 « pendant
qu'on écrit dans l'éditeur » (`docs/questions.md` Q13) avec le vrai matériel.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser `--simulated-tablet` ni `--simulated-microphone`.
3. Copier la fiche PDF et la photo dans un dossier de la Surface (pas dans la bibliothèque).
4. Noter la version d'Ardoise et celle de Windows.

### Au travail

#### A. Le papier en direct
1. Ouvrir le **Mode direct**, **Projeter**, **Partager sur le réseau** vers le PC du vidéoprojecteur. L'élève écrit.
2. Onglet **Page** : **Crème**, modèle **Seyès**, **Texture du papier**. Noter si le vidéoprojecteur et l'écran du
   partage changent, en combien de temps, et si l'écriture de l'élève continue **sans à-coup** pendant le changement.
3. Appuyer sur `B` (tableau noir) : les pastilles de couleur sont grisées, une phrase le dit ; choisir le modèle
   **Ligné** : il apparaît sur l'ardoise. `B` à nouveau : le crème revient, ligné.
4. Faire passer la page avec le **bouton de la tablette** : la nouvelle page est crème lignée.

#### B. Une fiche sous la page
1. Onglet **Fond** : **Choisir un PDF ou une image…**, la fiche, **page 2**, **Placer**. Noter le temps avant qu'elle
   s'affiche au vidéoprojecteur et sur le partage, et si l'élève peut écrire pendant ce temps.
2. L'élève complète la fiche : l'écriture tombe-t-elle **au bon endroit** sur la fiche, au vidéoprojecteur et sur le
   partage ? Essayer **Opacité** 50 %, **Remplir**, **Ajuster**, tourner l'image.
3. Cliquer dans la liste **Modèle**, puis appuyer sur la **télécommande** (Page suivante) : la page doit tourner, le
   modèle ne doit pas changer.
4. Page suivante : pas de fiche, le papier est gardé. Page précédente : la fiche est là. Mettre la photo sur une autre
   page.
5. **Terminer.** Dans la bibliothèque, ouvrir les notes : la fiche sous la première, la photo sous la sienne, le papier
   sur toutes.

#### C. Une feuille reprise
1. Reprendre en direct une note qui a déjà une image de fond. Remplacer l'image par la fiche page 1, écrire, Terminer.
2. Ouvrir la note : la nouvelle image. **Revenir à l'original** : l'encre d'avant revient, l'image reste (c'est voulu).

#### D. Projeter une note de l'éditeur
1. Ouvrir une note, onglet **Projection**, **Projeter**. Écrire au stylet de la Surface : le trait apparaît-il au
   vidéoprojecteur **pendant qu'on écrit** ? Noter le retard ressenti.
2. `Ctrl+Z`, `Ctrl+Y`, masquer un calque, changer le papier : le vidéoprojecteur suit-il à chaque fois ?
3. **+** deux fois, puis faire défiler et zoomer l'éditeur : la zone projetée suit-elle ? Le cadre en pointillés
   montre-t-il ce que voit la classe ? **Page entière**, **Largeur**.
4. **Relecture** (`R`) : la relecture passe-t-elle au vidéoprojecteur sans saccade ?
5. Note suivante (`Alt+→`) : le vidéoprojecteur montre la nouvelle note. Partager en même temps : les deux marchent.
6. `Échap` sur le vidéoprojecteur : il se ferme, l'onglet le dit. Projeter à nouveau, puis revenir à la bibliothèque :
   la projection se ferme.

#### E. Le son pendant qu'on écrit
1. Ouvrir une note déjà écrite en classe. Onglet **Son**, **Pendant que j'écris**, **Enregistrer en écrivant**, choisir
   le micro de la Surface, **Démarrer**. Parler une minute en écrivant au stylet, avec des moments où l'on parle sans
   écrire et où l'on écrit sans parler. **Arrêter** : « Son enregistré ».
2. **Relecture** avec **Lire avec la narration** : les traits apparaissent-ils avec la voix ? Noter un décalage, en
   secondes, au début et à la fin. Combien de temps passe avant la voix (l'encre déjà là défile-t-elle vite) ?
3. **Exporter la vidéo** en MP4 avec la narration, la lire dans le lecteur de Windows : même vérification.
4. Enregistrer à nouveau : Ardoise demande s'il faut **remplacer** ; essayer **Garder l'ancienne**, puis **Remplacer**.
5. Avec **Dessiner au doigt** activé dans les réglages, écrire au doigt pendant un enregistrement.

### Ce qui revient par Nextcloud
Les notes de la séance (A à C) et celles de D et E, dans la bibliothèque ; les journaux du partage dans
`_ardoise/diagnostics/`.

### À me signaler
- tout à-coup de l'écriture de l'élève pendant un changement de papier ou de fond, et le temps d'apparition de la fiche ;
- un décalage de l'écriture sur la fiche, au vidéoprojecteur ou sur le partage ;
- le retard du trait de l'éditeur au vidéoprojecteur, et la relecture projetée ;
- le décalage entre la voix et l'écriture, dans Ardoise et dans la vidéo ;
- toute phrase incompréhensible, et ce que Windows Ink affiche par-dessus la page.
```

---

## 10. Work packages

One owner per file, as in M4, M14 and M16. Each package in its own worktree and branch, merged into `m17` by the
lead. Sizes count lines written or changed, tests included.

| # | Package (profile) | Owns | Size | Starts |
|---|---|---|---|---|
| 0 | Contract (the lead, one commit) | see below | M: ~700 lines, mostly types, stubs, catalogs | first |
| A | Live paper and image, Rust (specialiste) | `app/src-tauri/src/live/{session,hub,model,resume}.rs`, `live/paper.rs` (after 0) and new `live/paper/tests.rs`, `live/tests.rs`, `live/resume/tests.rs`, `live.rs` (after 0: the two commands, `live_page_background`), `share/{live_paper,feed,tests}.rs`, `background.rs` (`Staging::take`, its test) | L: ~1,000 | after 0 |
| B | Live Page and Fond, interface (developpeur) | `components/live/{LiveView,LiveCanvas}.svelte` (after 0) and every `components/live/LiveView*.test.ts`, `LiveCanvas*.test.ts`, new `LiveView.page.test.ts`; `lib/live-state.svelte.ts` (+ tests), `lib/live-keys.ts` (+ tests), `lib/live-errors.ts` (+ test), `lib/mock-live.ts` (+ tests); `components/editor/{PaperBlock,BackgroundImageSection}.svelte`, `PaperBlock.test.ts`, `BackgroundImage.test.ts`; `app/e2e/journeys-m17-live.mjs`, `journeys-m16-live.mjs` | L: ~1,300 | after 0; journey after A |
| C | Editor projection, Rust (specialiste) | `app/src-tauri/src/projection.rs` (after 0) and new `projection/tests.rs`, `presentation.rs` | M: ~650 | after 0 |
| D | Editor projection, interface (specialiste) | `lib/projection-feed.ts`, `lib/editor/projection.svelte.ts`, `components/projection/{EditorPresentation,EditorProjectionTab}.svelte` (all after 0) and their tests; `components/projection/{ProjectorBlock,ZoneBlock}.svelte` (+ tests); `lib/mock-projection.ts` (after 0, + test); `main.ts`; `app/e2e/journeys-m17-projection.mjs`, `app/e2e/perf.mjs`, `xtask/src/perf.rs` | L: ~1,400 | after 0; journey and perf after B, C, E |
| E | Editor: sound while writing, and the wiring of both editor features (specialiste) | `components/editor/{Editor,Canvas,EditorSound,ReplayBar}.svelte`, every `components/editor/Editor*.test.ts` and `Canvas*.test.ts` and harness, new `Editor.sound.test.ts`; `lib/editor/{state,ops}.ts` (+ tests), new `lib/editor/writing-clock.ts` (+ test); `components/narration/{RecordPanel.svelte,recorder.svelte.ts}` (+ tests); `lib/mock-narration.ts`, `lib/mock-notes.ts` (+ tests); `app/e2e/journeys-m17-sound.mjs`, `journeys-m16-editor.mjs` | L: ~1,600 | after 0; journey after F |
| F | Narration, Rust: writing target, staged sound, simulated microphone (specialiste) | `app/src-tauri/src/narration.rs` (after 0), `narration/tests.rs`, new `narration/simulated.rs`; `notes.rs` (after 0: `save_note`'s narration, `OpenedNote.narration`) and its tests | M: ~800 | after 0 |
| G | Dead time before a note-time narration (specialiste; optional, O13) | `crates/ardoise-media/src/replay/{timeline,tests}.rs`, `replay.rs`, `narration.rs`, `export.rs` and the crate's tests; `app/ui/src/lib/replay.ts` (after 0) and `replay.test.ts` | M: ~700 | after 0 |
| H | Guides and closing (developpeur) | `docs/guide/guide.fr.md`, `guide.en.md`, `CHANGELOG.md`, `docs/notes-de-version.md`, `docs/hardware-tests.md` (H7, §9), `docs/file-format.md` (§4), `docs/design/lan-share.md` ("The paper of a live page": ordinary pages now get a key), `docs/design/side-panel.md` (§1 and §11: a pointer here), `docs/design/interface.md` (one line), this file's "As built" (from the packages' reports: they do not edit it), the catalogs after the packages (removals), `docs/performance.md` (by `cargo xtask perf --write` on the PC) | M: ~900 | journeys run after all merges; guides last |
| R | Independent review (relecteur); remarks fixed by their package's owner | – | – | last |

**Order.** 0; then A, C, F, G (Rust, disjoint) and B, D, E (interface, against package 0's stubs and the mocks) in
parallel. Merge G, F, A, C, then B (D's window uses `LiveCanvas`'s new props), then D and E in either order (disjoint
files: E imports D's modules through package 0's skeletons); the three journeys and `cargo xtask perf --only
paper,projection` once all are in; H; R. `ROADMAP.md` is ticked only after `cargo xtask check` and `cargo xtask e2e` on
the PC (CLAUDE.md). Without G, M17 is complete and correct; only the replay of an already written note with a new
narration is longer (O13).

### Package 0, the contract (committed by the lead before anything else)

1. **Rust stubs**, compiling, every new command registered in `lib.rs`:
   - `live.rs`: `pub mod paper;`, `live_page_paper` and `live_page_image` returning `CommandError::new("unexpected",
     "not built yet")`; `live/paper.rs`: `LivePageImageOp` (§1.3) and the `PageImage` struct (§1.2), no logic.
   - `projection.rs` (new): `ProjectionUpdate`, `ProjectionView`, `ProjectionPoint`, `ProjectionStatus`, the two
     limits, `ProjectionRuntime` (`Default`), the five commands (§2.3) as stubs; `lib.rs`: `mod projection;`,
     `.manage(projection::ProjectionRuntime::default())`.
   - `narration.rs`: `NarrationTarget::Writing { path }` (refused with `unexpected` until F),
     `NarrationSaved.staged: Option<String>` (skipped when `None`), `narration_drop_staged` stub,
     `NarrationRuntime::with_simulated_microphone(simulated: bool) -> Self` (the default runtime until F).
   - `notes.rs`: `save_note`'s `narration_key: Option<String>` (ignored until F); `OpenedNote.narration:
     Option<NarrationInfo>` (always `None` until F).
   - `lib.rs`: `Cli.simulated_microphone` (`--simulated-microphone`, with a parse test),
     `.manage(narration::NarrationRuntime::with_simulated_microphone(cli.simulated_microphone))`, the eight commands.
   - `capabilities/presentation.json`: `"windows": ["presentation", "projection"]`.
2. **TypeScript types and wrappers**: `lib/live.ts` (`LiveSnapshot.pagePaper?: Paper`, `imagePixels?: { key: string;
   width: number; height: number } | null`, `LivePageImageOp`); `lib/projection.ts` (new: `ProjectionUpdate`,
   `ProjectionView`, `ProjectionPoint`, `ProjectionStatus`); `lib/video.ts` (`NarrationTarget` `{ kind: 'writing'; path:
   string }`, `NarrationSaved.staged?: string`); `lib/note.ts` (`OpenedNote.narration?: NarrationInfo | null`);
   `lib/api.ts`:

   ```ts
   livePagePaper: (pageIndex: number, background: Background) => call<void>('live_page_paper', { pageIndex, background }),
   livePageImage: (pageIndex: number, op: LivePageImageOp) => call<void>('live_page_image', { pageIndex, op }),
   projectionOpen: (monitor: string | null) => call<MonitorInfo>('projection_open', { monitor }),
   projectionClose: () => call<void>('projection_close'),
   projectionWatch: (onStatus: (status: ProjectionStatus) => void) => Promise<ProjectionStatus>,   // Channel, as shareSubscribe
   projectionSubscribe: (onBatch: (batch: LiveBatch) => void) => Promise<LiveSnapshot>,            // Channel, as liveSubscribe
   projectionUpdate: (update: ProjectionUpdate) => call<void>('projection_update', { update }),
   saveNote: (path: string, note: Note, stamp: FileStamp, imageKey?: string, narrationKey?: string) =>
     call<FileStamp>('save_note', { path, note, stamp, imageKey, narrationKey }),
   narrationDropStaged: (key: string) => call<void>('narration_drop_staged', { key }),
   ```

   `lib/mock.ts` chains a new `lib/mock-projection.ts` (skeleton: `projection_*` answered as closed; D completes it).
3. **Skeletons with their final API** (§2.4–2.6): `lib/projection-feed.ts` (types, `projectionDiff` returning null,
   `ProjectionFeed` doing nothing), `lib/editor/projection.svelte.ts` (`EditorProjection`, no-ops),
   `components/projection/EditorProjectionTab.svelte` (props `{ projection: EditorProjection }`, the three blocks
   wired to the no-ops), `components/projection/EditorPresentation.svelte` (a black page), `main.ts` mounting it for
   `?view=projection`; `LiveCanvas.svelte`: the `interactive` and `paper` props declared (today's behaviour); `lib/replay.ts`:
   `ReplayOptions.lead?` and `narrationReplay(info)`, `narrationSoundStart(timeline, info)` returning the stored options
   and `info.startMs`.
4. **Catalogs**: every key of §5 in `locales/fr.json` and `locales/en.json`.
5. **e2e**: `app/e2e/m17-helpers.mjs` (complete, §7), `side-panel.mjs`'s `checkRecordingDot` option,
   `journeys-m17-{live,projection,sound}.mjs` as empty lists registered in `run.mjs`.

`cargo xtask check` passes after package 0 (nothing calls the stubs; the new keys are unused but present in both
catalogs, which the catalog tests accept: supposé, the lead checks).

#### Package 0, as built

What the commit holds, and where it differs from the list above (every name, signature and key not mentioned is as
written):

- **`LiveSnapshot` has its two new fields already** (`page_paper: Paper`, `image_pixels: Option<ImagePixels>`,
  `live/model.rs`; `ImagePixels { key, width, height }` is in `live/model.rs`, not re-exported from `live.rs`: use
  `crate::live::model::ImagePixels`). Package C builds `LiveSnapshot`s and cannot wait for A. `Session::snapshot` fills
  them with the page's `background.paper` and `None`; A makes them real (the own paper under the board, the image's
  key). The share's allow list is untouched.
- **Rust stubs.** `live_page_paper` and `live_page_image` answer `unexpected` / « not built yet »; `PageImage` and
  `LivePageImageOp` are in `live/paper.rs`. In `projection.rs` `projection_open` and `projection_subscribe` answer the
  same error, `projection_close` and `projection_update` answer `Ok(())`, `projection_watch` answers `{ open: None }`.
  `ProjectionRuntime` is `Mutex<Projection>` with an `open` field C replaces. The items nothing reads yet carry
  `#[allow(dead_code)]` (C and A remove it); `ProjectionUpdate` also allows `clippy::large_enum_variant` (`Page` holds a
  whole note: not boxed, so the contract keeps `note: Note`).
- **`narration.rs`**: `NarrationTarget::Writing { path }` is refused by `start_in` as `unexpected`;
  `NarrationSaved.staged` is skipped when `None`; `with_simulated_microphone(bool)` builds the real runtime whatever the
  flag until F; `narration_drop_staged(key)` answers `Ok(())`. **`notes.rs`**: `save_note` takes
  `State<NarrationRuntime>` (named `_narration`) and `narration_key` (ignored); `OpenedNote.narration` is `None`.
- **`capabilities/presentation.json`** lists both windows. `lib.rs` has `--simulated-microphone` (parse test) and the
  eight commands. `contract_tests.rs` (new, `#[cfg(test)]`) guards the wire format of the new Rust types.
- **TypeScript.** The types and wrappers are as listed (`LivePageImageOp` and `ImagePixels` in `lib/live.ts`). The
  skeletons carry their final API and do nothing: `lib/projection-feed.ts` (`SentPage` is the interface of §2.4's
  « SentPage »), `lib/editor/projection.svelte.ts`, `EditorProjectionTab.svelte` (the blocks are wired without
  `ProjectorBlock`'s `help` nor `ZoneBlock`'s `widthTitle`: D adds those two props and passes the keys),
  `EditorPresentation.svelte` (a black page that does not close itself yet), `main.ts`. `LiveCanvas.svelte` declares
  `interactive` and `paper`, unused until B. `lib/mock-projection.ts` exports `mockProjectionInvoke` (chained in
  `mock.ts`), `mockProjectionWatch` and `mockProjectionSubscribe` (which `api.ts` uses outside the desktop app).
- **Package G is not done (§11): skipped here** `ReplayOptions.lead`, `narrationReplay`, `narrationSoundStart` in
  `lib/replay.ts`, and the key `editor.sound.leadTrimmed`. No package A to F needs them.
- **Catalogs**: every key of §5 except `editor.sound.leadTrimmed`, both languages, appended at the end of its group.
- **e2e**: `m17-helpers.mjs` as §7 (`penStroke`, `BROWSER_ARGS` are exported by their files and re-exported;
  `noteOnDisk` also gives `strokes` and `audio`; `opusTags` reads the OpusTags comments properly, names without the
  `ARDOISE_` prefix, e.g. `opusTags(bytes).START_MS`; `canvasFacts` gives numbers and booleans). The three journey
  files export `livePaperJourneys`, `editorProjectionJourneys` and `writingSoundJourneys` (empty). `checkRecordingDot`
  has its `{ simulated }` option; the M16 journeys are not changed (B and E). `run.mjs` also ignores a `view=projection`
  page when it looks for the main window.

## 11. Open points and risks

Open points: the user's call. Each has a recommendation, which this document follows by default; none changes what
the user asked (Q13).

**Decided by the lead (2026-11-11): every recommendation below is taken, except O13: package G is NOT done in M17**
(it would also shorten the replays of notes written before, which the user did not ask for; it can come later if the
dead time before a narration recorded while writing proves a nuisance). The user is told in the M17 report.

- **O1. Paper colour under the blackboard** (live). Recommendation: the swatches are disabled and a sentence says the
  colour comes back with the board off; template and texture apply. Alternative: a swatch turns the board off and
  applies the colour (one click, but the tablet's chalk goes back to its ink at the same time).
- **O2. The first page of a new session**: white, as today (recommendation: each lesson starts plain). Alternative:
  the last session's paper, as the ink and the blackboard carry over (`carried_ink`).
- **O3. A live page with an image and no ink**: written as a note (recommendation: a worksheet shown in class is
  kept). Paper alone, without ink, writes nothing.
- **O4. Which page a new page takes its paper from**: the page on screen when it is made (recommendation; with « Nouvelle
  page » from an earlier page, that page, not the last one).
- **O5. Where the editor's projected area goes**: centred on what the editor shows, at its own zoom, the editor drawing
  a dashed frame (recommendation: the teacher zooms the editor to write small, the class keeps its view, scrolling the
  editor moves it). Alternatives: mirror the editor's view exactly (the editor's zoom is the class's); or move the area
  by hand (a second control the editor lacks).
- **O6. The projection window is display-only**: no temporary marks, no pinch, Esc closes (recommendation; marks are
  live's, the teacher writes in the editor itself).
- **O7. A replay is projected** (recommendation: yes, the class sees it; the projector follows the editor).
- **O8. The stroke in progress is projected while it is written** (recommendation: yes, as in live mode; « mis à jour
  à chaque trait » read as "follows every stroke as it is drawn").
- **O9. Another note opened in the editor stays projected** (recommendation: yes; leaving the editor stops it, Q13).
- **O10. « Arrêter » saves the note with the sound at once** (recommendation: consistent with the replay recording,
  which writes at stop, and the strokes timed on the sound never sit in a file without it). Alternative: the note
  becomes unsaved and the user saves it.
- **O11. The replace question in both modes** (recommendation: yes, one rule; today's replay recording replaces with a
  notice only).
- **O12. Son's default mode**: « Pendant que j'écris » (the mockup's; recommendation). Remembered per PC.
- **O13. Dead time before a note-time narration trimmed** (package G; recommendation: in M17, so that commenting a page
  written in class does not replay the whole lesson in real time before the voice; it also shortens M12's resumed sheets
  with a live narration). Alternative: later, after use.
- **O14. Strokes written after a writing recording go after the end of its sound** (recommendation, as live mode's
  resume does, `time_base`), so that they do not appear during the narration; replay narrations keep today's rule.
- **O15. A paste during a recording takes the sound's time** (recommendation: it appears when it was pasted).
- **O16. « Annuler » during a writing recording keeps the strokes** (recommendation: ink is never thrown away with a
  sound).
- **O17. A finger writes during a recording only with « Dessiner au doigt »**, as today (recommendation; the help says
  so).
- **O18. The assistant's `presentation` tool stays live-only** (recommendation; not asked).
- **O19. The editor's share keeps showing the note** (not the projected area; recommendation, as SPEC §4.10 says).
- **O20. Blocked while a writing recording runs**: today's gates plus « Revenir à l'original », the split and the video
  export (recommendation); every other edit allowed.
- **O21. A live page with an image and the tablet turned** (§1.3): without ink the page follows the tablet and the image
  is placed again upright; with ink the page stays and the tablet follows it, as a resumed sheet (recommendation: a
  worksheet written on is never turned under the pupil's writing). Not in M17: a live page that takes the image's
  orientation by itself (the editor's offer), since a page of live mode follows the tablet.

Risks:

- **R1. Two presentation windows.** Labels `presentation` and `projection` never coexist: each open destroys the
  other's window; both use the same `Activity` flag (activity.rs tracks one token: the second window's creation takes
  the flag, a late destruction of the first is ignored, `a_late_report_of_an_old_window_keeps_the_new_one`). The journeys
  attach by URL (`view=presentation` / `view=projection`).
- **R2. Large or frequent feed messages.** Recolouring every stroke sends the page (megabytes for a big note: one
  message); a projected replay repaints the partial stroke's area each frame, near a full redraw for a long stroke.
  §8 measures it; the fallback is the stroke-in-progress path for the replay's partial stroke.
- **R3. Clocks of the writing recording.** The strokes follow `performance.now()`, the sound the sound card: the offset
  is fixed once (`P0`, as the replay recording's `replayStartedAt`, proven since M6), a drift of the card's crystal is
  not corrected (supposé ≤ 0.1 s over 10 minutes; H7 asks for the start and end offsets).
- **R4. A staged sound lost by a crash** between « Arrêter » and the save (a few hundred ms): its backup copy remains in
  `%LOCALAPPDATA%\Ardoise\narration-backup` (`backup_dir`, narration.rs:1064–1077), the draft keeps the strokes;
  nothing re-attaches them (logged).
- **R5. `merge_outside` and the member** (§1.4): without the copy, a page retitled elsewhere would write a new image's
  description over the old pixels. A test in A.
- **R6. Memory of a lesson's images**: members per page (compressed, MBs), decoded pictures bounded to two (§1.2). Ten
  4096 px pages decoded would be 640 MB.
- **R7. The remote's keys in the new tabs** (§1.6): a select or slider keeping the focus would take Page Down and change
  the paper in front of the class; tested in vitest and the journey.
- **R8. G changes how existing files replay**: live narrations on resumed sheets get shorter replays and videos (same
  sync); tests of M12 that measure durations with a narration may need new numbers.
- **R9. `Canvas.svelte` is the editor's pen path**: three new props and the clock; the existing Canvas tests guard it,
  plus E's.

## Not done on purpose

A pause button for the recording (not asked: a silence is a pause); undoing a paper or image change with Ctrl+Z (Q13);
marks, blackboard, laser and pinch on the editor's projection (Q13, O6); the projected area moved by hand; the
editor's eraser and moves shown while dragged; a narration added after the old one instead of replacing it (Q13: one
narration); re-attaching a backup sound after a crash; the assistant driving the editor's projection; a key to project.

## To check on Windows

The two presentation windows' labels with the real projector and Windows' full screen; the projection's latency with the
Surface's pen (`__ardoiseLatency.computer`); WebView2 throttling the editor while the projection covers it on one screen;
Page Down from a real remote with the template select focused; a 300 dpi PDF page under a live page (import time,
memory, the share's picture); the simulated microphone listed in a real window; the sync of voice and strokes in an MP4
read by the Windows player (Media Foundation's AAC) and by VLC.

---

## As built

What differs from the design above, package by package, from the packages' reports (they do not edit this file) and
as merged on `m17`; the contract of package 0 has its own "Package 0, as built" in §10. Package G was not done (§11), so
nothing below concerns the dead time before a note-time narration.

**Package A (live paper and image, Rust).**
- **O21 as designed, plus the removal.** When the tablet turns, a page with an image and no ink takes the tablet's new
  size and its image is placed again with the fit it had (contain, cover or stretch, recognised on the old page), keeping its
  own quarter turns; a page with an image and ink is held fixed like a resumed sheet (`can_turn_page` false, the tablet follows
  the page). Removing the image from such a page turns it to the tablet at once, ink and all, and refits the view.
- **Saving.** A page with an image and a change is written as a note even without ink (O3); paper alone writes nothing.
- **New pages** take the template, texture and own paper of the page on screen, never its image; a new session starts white.
  Under the blackboard the colour is kept and comes back when the board is off (O1).
- **`paperKey`** digests the image's pixels key too, so any page with a template, a texture or an image gets a key, a
  `livePaper` message and `/paper/<key>` on the share; a white blank page still gets none (lan-share.md is corrected).
- **Not in the design**: the share skips a picture if the hub's image changed after the snapshot asked for it (the next
  snapshot asks again).
- **Behaviour that changed**: a new page made from a slate sheet now stays slate after the board is turned off.

**Package B (live Page and Fond tabs).** As designed: `section.pane-block[data-section="live-paper"]` for Page,
`BackgroundImageSection` (`data-section="background-image"`) wrapped for Fond, the remote's Page Up / Page Down acting
even with a `select` or a range input focused, and the focus given back after a change.

**Package C (editor projection, Rust).** The five commands as in §2.3, with these differences:
- The projection window's destruction is handled by **the window's own callback**, carrying a number given at each
  open or move, not by a `PROJECTION` branch in the global `on_window_event`: Tauri removes a window from its manager
  before its `Destroyed` handlers run, so a reopen could be wiped by the old window's late event.
- `projection_subscribe` returns `SnapshotReply` (the same JSON as `LiveSnapshot`).
- Limits: 4096 points a call, 2^20 strokes, 100,000 points for the stroke in progress. A `strokeBegin` while a stroke is in
  progress cancels it first.

**Package D (projection window and feed, interface).**
- The feed (`projectionDiff`) sends the whole page when nothing was sent yet or when the note id, page size,
  background, image placement, layers or image key change; otherwise the common prefix of strokes (by `===`) gives
  `strokes {keep, tail}`. A tail of more than half the strokes goes as a page, but only when it holds more than one
  stroke (so a single new stroke on a tiny note goes as a tail, not as a page and a cancel).
- One call in flight; inputs only record state and schedule one pump. Order: page, ending the stroke Rust shows
  (`ended` if the editor kept it, else `strokeCancel`), `strokeBegin`/`strokePoints`, strokes, view. A kept stroke waits
  up to 1 s (`END_WAIT_MS`) for the document to bring it, then is cancelled.
- A refusal sends the page next, cancels the held stroke and restarts the stroke being drawn from its first point. A
  refused page is tried again after 250 ms, doubling up to 8 s, or at once when another note is shown. Refusals are
  counted, never shown.
- `EditorProjection` takes `show` and `visible` at any time and forwards them only while open. The projected area is
  centred on the editor's visible centre, kept on the page, in the projector's aspect; « Largeur » uses the
  `fitWidthView` zoom (at least 1). The view goes at most once a frame. `frame` (the dashed rectangle) is set only at a
  zoom above 1.
- The window (`EditorPresentation`) draws with `LiveCanvas mode="presentation" interactive={false}`; its paper is
  rendered with the staged pixels when there are some, otherwise the paper alone. Esc, Ctrl+W, Ctrl+F4 and Ctrl+Q close
  it; no other key does anything.
- `ProjectorBlock` gets `help`, `ZoneBlock` gets `widthTitle`; live's use of both is unchanged.
- The browser stand-in relays batches between tabs over a `BroadcastChannel` (not in the tests), so a tab at
  `?view=projection` draws the editor's page; live's presentation opening closes the stand-in's projection (added by
  the lead).
- `cargo xtask perf` gains the parts `paper` (live mode with the presentation, Seyès and a PDF page set through the
  commands) and `projection` (a 1,000-stroke note projected while the editor's pen writes; then the replay). The
  latency row is met under 30 ms, accepted from 30 to 50 ms, missed from 50 ms; the projector's frame rate needs 57 fps;
  the replay row is information only (R2 is decided from it).
- Not done: a second `strokeCancel` refused by Rust can leave a stale stroke on the projector until the next stroke
  begins (rare; closing clears it).

**Package E (the editor's sound UI and the wiring).**
- `noteZeroWallMs` is `Date.now()` at `P0` minus `T0`, not `performance.timeOrigin + P0 − T0` (`timeOrigin` drifts
  from the wall clock in a page left open for hours).
- O14 also applies to a paste outside a recording: it goes after the end of a note-time narration.
- The video export is also blocked during a **replay** recording, not only a writing one.
- The original's gate shows its notice only after its confirmation.
- The mode is stored in `ardoise.editor.narrationMode`.

**Package F (narration, Rust).**
- A stop without a clock places the sound after the note's ink on disk plus the writing gap.
- A newer recording replaces the staged sound in memory; the older one's backup copy always stays (changed after the
  review: the editor may not know that sound, after it was closed during a recording). Only a save with the note and
  `narration_drop_staged` remove a copy.
- `save_note` writes the note and the sound in one atomic save.
- `--simulated-microphone` lists only « Simulated microphone » (the journeys use it; the real microphone is never used
  in tests).
- Not done: nothing re-attaches a backup copy after a crash; a writing recording under way when the app closes is lost (as a
  replay recording is).

**Package H (guides and closing).** Written from this file and the packages' reports; not a line of code. The guide
chapters: FR and EN « Le papier et l'image de la page en cours » (ch. 6), « Projeter la note au vidéoprojecteur » (ch. 8),
« Narration » (ch. 9). Differences between the design's wording and the interface, which the docs follow: the buttons are
« Mettre en fond » (§9 said « Placer »), the fits « Entière » / « Pleine page » / « Étirée » (« Ajuster » / « Remplir ») and the
recording's stop « Arrêter et enregistrer » (« Arrêter »). What a user is told of what G would have changed: the replay and
the video of a note-time narration keep the real pauses of the ink that was there before the sound (guide ch. 9, release
notes « Limites connues »).

**After the independent review (lead).**
- A sound kept after a failed save is no longer dropped when « Remplacer » starts a new recording: Rust replaces it only
  once the new sound is stopped and kept, so a recording that cannot start (microphone unplugged) or is cancelled loses
  nothing. A replay narration saved in its place drops it after its save.
- The replace question puts the focus on « Garder l'ancienne » (a held Enter replaces nothing); keeping gives the focus back
  to « Démarrer ».
- The « kept in memory » line no longer shows during the save that follows « Arrêter et enregistrer ».
- `live_stop` clears the background staging area (images imported in the Fond tab and never set).
- The live mock keeps an image's quarter turns when the tablet turns, turns a page at once when its image is removed,
  and live's presentation opening closes the editor's projection in the browser stand-in.
- The tooltip of a page that does not turn names a page written over an image too (`live.resume.cannotTurn`).
- Left as they are: a live page whose image is removed after it was written stays as an empty note (as a page whose ink
  is all erased; the guide says so); no way to drop only a kept sound and keep the strokes; the editor's projection shows
  the paper colour if its image key is no longer staged, until the layout or the key changes.

**To check on Windows, as listed above** (and H7 in `docs/hardware-tests.md`): the two presentation windows with the real
projector and Windows' full screen; the projection's latency with the Surface's pen; WebView2 throttling the editor while the
projection covers it on one screen; Page Down from a real remote with the template select focused; a 300 dpi PDF page under a
live page; the simulated microphone in a real window; the sync of voice and strokes in an MP4 read by Windows' player and VLC.
