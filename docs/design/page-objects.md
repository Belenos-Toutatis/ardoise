# Page objects, and the Text tool

> Developer document (English). Requirements: SPEC §4.4 (« Texte tapé au clavier », « Équation », « Signets »), §4.3
> (« Sommaire du classeur »); decisions: `docs/questions.md` Q20, items 5 to 7. Bytes: `docs/file-format.md` §2.2.
> Code: `crates/ardoise-core/src/objects.rs` (model, placement, JSON), `crates/ardoise-text` (fonts, layout, glyphs),
> `crates/ardoise-render` (`draw_items`, `text.rs`), `crates/ardoise-media/src/replay` (replay), `app/ui/src/lib/editor/objects.ts`
> and `app/ui/src/lib/text.ts` (interface), `app/ui/src/components/editor/TextField.svelte` (typing).

## 1. What a page object is

A note's page held strokes only. It now holds **page objects** too (`Note.objects`, format 1.4): things placed on the page that
are not ink. Text frames are the first kind; equations (an object that keeps its LaTeX source and the vector drawing made from
it) and bookmarks (a short label at a point, shown in the editor only) are planned as the next two kinds. The layer was designed
so that they are added as kinds, not as new mechanisms (§6).

Every object is two parts:

- **The placement**, the same for every kind: `id`, `layer`, `t`, `x`, `y`, `scale`, `quarterTurns`, `width`, `height`. The
  content of an object is drawn in its own coordinates (origin at the top-left corner of its content, y down, its own units) and
  put on the page by `page = (x, y) + turn(scale × (u, v))`. `width` × `height` is the content's box.
- **The body**, the keys of its kind (`kind` names it). Rust: `ObjectBody::Text(TextBody)`, or `ObjectBody::Unknown { kind,
  fields }` for a kind this version does not know (or a known kind with a value it does not know, such as a font of a later
  version). TS: `PageObject` (placement plus any keys) and `TextObject` with its guard `isTextObject`.

Why a placement common to every kind: **every operation that rewrites a page moves objects through the placement alone**. A
turn, a crop, a resize of the page, a move or a resize of the selection, a merge, a split, never need to know what an object
shows. A version that meets a kind it does not know keeps it, moves it with the page, and draws nothing; so a new kind is a
minor version of the format, and no operation written today has to change when equations and bookmarks come.

Why `width` and `height` are stored though a text frame's box can be measured: for the readers that cannot measure a kind (an
older version meeting an equation; the assistant's server reading a note), so that they can still crop, split by zone and select
by the box. Writers write the box they measure; readers measure again the kinds they know (`PageObject::measure`, called when a
note is read and written).

Drawing order (every output, `ardoise_render::draw_items`, `drawItems` in TS): visible layers bottom first; within a layer its
strokes in note order, **then its objects** in note order; what has no layer entry last. The time `t` places an object in the
replay: it appears all at once at `t`.

## 2. What each operation does to objects

Rule: nothing that rewrites a note may drop an object; wherever strokes move with the page, objects move the same way.

| Operation | Where | Objects |
|---|---|---|
| Turn the page (editor, library batch) | `ops.ts` `rotateQuarter` | `x, y` mapped like a point, `quarterTurns` + 1 (cw) or + 3 (ccw) |
| Crop (manual, automatic) | `ops.ts` `cropTo`, `autoCropRect` | shifted; a visible one wholly outside goes (hidden layers kept); the automatic crop counts their boxes |
| Page size, orientation, « Adapter l'écriture à la feuille » | `ops.ts` `resizePage` | moved inside like the ink, or scaled with it (`scale` multiplied) |
| Move / resize the selection | `state.ts` `transformSelection` | the selected objects follow the affine map (`transformObject`) |
| Layers: move, merge, hide | `ops.ts` `moveLayer`, `mergeDown`; `state.ts` | layer index remapped; a merged layer's objects after the lower layer's; hidden: unselected |
| Delete, duplicate, copy, paste | `state.ts`, `clipboard.ts` | with the strokes; copies get new ids; a paste is retimed with the strokes (`retimedPaste`) |
| Eraser | `state.ts` `erase` | untouched (an object is deleted by selecting it) |
| Split at an instant / by zone | Rust `ops::split_at_time`, `split_by_zone`; preview `split.ts` | `t ≥ at` moves (times shifted); by zone: an object of a visible layer whose box centre is inside moves |
| Merge (overlay, end to end) | Rust `ops::merge` | layers rebased, `y` offset, times made sequential like the strokes; ids made unique |
| Back to the original | `original.rs`, `lib/editor/original.ts` | the original's objects come back (a tablet page has none: the frames added since go) |
| Duplicate a note, change its date, trash, restore, drafts | `ardoise-core::library`, `drafts.rs` | whole files: kept (times are offsets from `created`) |
| Live mode on a resumed sheet | `live/session.rs`, `hub.rs`, `resume.rs` | kept and written; turned with the page; layer moves and merges follow; « Effacer la page » leaves them; new live strokes start after their times |
| Xiaomi update of an unedited note | `ardoise-device` `pages.rs` | a note with objects counts as edited: never overwritten |
| Assistant (MCP) | `ardoise-mcp` | every write tool keeps them (revisions, straightening, split, merge); revision orders touch strokes only |
| Writing clock (M17) | `writing-clock.ts`, `narration.rs` | a recording, and a new stroke, start after the last object's time too |

## 3. Drawing

- **Exports** (`ardoise-render`): raster (PNG, JPEG, WebP, thumbnails, print, clipboard, MCP render) fills each frame's glyph
  path with tiny-skia, curves kept, non-zero rule, the colour's alpha. SVG: one `<path>` per frame with its `Q`/`C` curves and a
  `<title>` holding the text, in its layer's Inkscape group. PDF: vector paths (quadratics written as the cubics they are), in its
  layer's optional content group; plus the text as **invisible text** (rendering mode 3, the standard fonts Times or Helvetica,
  WinAnsiEncoding, widths set to our glyphs' advances): the PDF can be searched and copied; characters outside WinAnsi are drawn
  but not searchable. No font is embedded: the glyphs are paths, so every viewer shows exactly the shipped fonts.
- **Replay and video** (`ardoise-media`): an object counts as a stroke of one sample at `t` for the clock (duration, trimmed
  pauses); `Timeline::note_at` keeps the objects whose time has come; the incremental frame renderer draws an object whole, in
  drawing order (a later stroke of the same layer still goes under it). The interface's replay (`lib/replay.ts`) follows the same
  rules.
- **Screen**: the editor draws each frame as an SVG `<path>` (the module's path data, `transform` = the placement), cached per
  frame object. The live renderer (live control window and projector on a resumed sheet, the editor's projection window, the
  share's viewers) draws them on its cache with a 2D context; a stroke appended under a frame, or a repaint that would cross one,
  redraws the whole cache instead.
- **Network share**: only text frames of visible layers are sent (`share/page.rs`), keys of later versions stripped.
- **Projection feed**: a change of objects sends the whole page again (`projection-feed.ts` `sameLayout`).

## 4. Text frames

- **Fonts** (`crates/ardoise-text/fonts/`, `README.md`): Latin Modern Roman 10 regular and bold (GUST e-foundry 2.004, GUST
  Font License = LPPL 1.3c) and Andika regular and bold (SIL 6.200, OFL 1.1 with Reserved Font Names): the official files,
  unmodified (subsetting would have meant renaming them under both licences), 1.7 MB in all. They are embedded in the executable
  (exports) and in the WebAssembly module of the interface (screen, and the bytes of the web fonts of the text field).
  `cargo xtask licenses` checks their SHA-256 and lists them in `THIRD-PARTY-NOTICES` and in « À propos ».
- **Layout** (`ardoise_text::layout`): one line per line of the text (only a line break starts one: no wrapping, so that the
  frame never re-flows differently on two machines and a resize is a change of scale); glyphs by their advance widths
  (`hmtx`), **no kerning, no ligature**; a character missing from the frame's family taken from the other family (Andika has the
  Greek letters of linguistics, not all of Greek), else `.notdef`; a tab to the next multiple of four spaces; lines `1.25 × size`
  apart; within a line the baseline is where CSS puts it (half the leading, then the ascent of `hhea`).
- **The same glyphs everywhere**: `text_outline` gives the outlines as a `Path` with its curves; the WebAssembly module exposes
  `text_layout`, `text_path` (SVG path data) and the font bytes (`crates/ardoise-ink-wasm/src/text.rs`, `lib/text.ts`).
- **Typing** (`TextField.svelte`): a `<textarea>` laid over the frame, scaled and turned like it, styled with the same font
  files (registered as `FontFace`s from the module's bytes) and the same rules (`textFieldStyle`: `white-space: pre`,
  `font-kerning: none`, `font-variant-ligatures: none`, the line height, `tab-size: 4`), its text transparent: what shows is the
  frame drawn from the module's glyphs as it is typed; the field gives the caret, the selection, the dead keys and input
  methods, and Windows' on-screen keyboard. Its lines are put on the frame's baselines from the font's ascent and descent as the
  browser measures them (`measureText`). Being a real field, the keys typed are the field's: the editor's shortcuts ignore keys
  typed in a field (Ctrl + S still saves what is typed).
- **Editing flow** (`Editor.svelte`): a tap of the text tool (pen, mouse, a finger's tap) asks for a new frame there or for the
  frame under it (its caret at the tapped place, `offsetAt`); a double click with the selection tool too. What is typed replaces
  the present doc without an undo step (`typeText`); the end of the typing (Escape, the field left, another tap or tool, undo)
  makes **one** step from the doc before it, or none when nothing changed; an empty or blank frame goes. The frame stays
  selected afterwards, so that the toolbar's options and the colours act on it. A new frame takes the tool's family, size, bold
  and colour and the time a stroke drawn now would take (`nextStrokeBase`, or the sound's clock during a writing recording).
- **Options**: family (two toggle buttons, each written in its font), size (4, 6, 9 mm: `TEXT_SIZES`), bold; the colour is the
  tool's. They apply to the frame being typed, else to the selected frames (a size given to a frame resets its `scale` to 1,
  its top-left corner staying), and to the next frames. They take the focus from nobody (`pointerdown` default prevented).
- **Selection**: a lasso takes an object whose box centre is inside, a click its box (above the strokes of its layer), Ctrl + A
  the chosen layer's; only kinds the editor draws are selectable. Copy and paste carry text frames (`clipboard.ts`, an optional
  `objects` list in the same clipboard version).
- **Search**: the listing gives each note's typed text (`NoteSummary.typedText`, from `note.json`: no extra read), text frames
  of visible layers in reading order; the library's search and the assistant's `search_notes` look in it.
- **Not in live mode**: the tool is the editor's; a resumed sheet keeps and shows its frames.

## 5. Choices that could have gone another way

- No automatic wrapping: a long line widens the frame (Enter breaks it). A wrapping frame would need the same line breaking in
  Rust and in the browser's field.
- No kerning and no ligatures (« fi » is two glyphs), so that the browser's field and our layout agree to the pixel.
- The eraser does not erase text; a resize scales the text (its size becomes any value until a size button is pressed).
- « Revenir à l'original » of a tablet note removes the frames typed since (the tablet's page has none).
- The search leaves out frames of hidden layers, as the handwriting search does.
- Unknown kinds in the editor are kept but neither drawn nor selectable.

## 6. How to add a kind

The equation and bookmark lots plug in here. For a kind `k`:

1. **Model** (`crates/ardoise-core/src/objects.rs`): a body struct, a variant of `ObjectBody`, its keys (`*_KEYS`), its reading
   in `PageObject::from_value` (return `Unknown` for anything this version cannot read, never an error: an error drops nothing
   but makes the whole list unreadable), its writing in `Serialize`, its box in `measure()` when it can be measured. Checks in
   `format.rs` `validate` if the kind has numbers that must be finite. A fixture `objects-1.5.ardoise` (or the next minor
   version: a new kind is a minor version, since a 1.4 reader keeps it) and `FormatVersion::CURRENT` bumped.
2. **Exports** (`crates/ardoise-render`): draw the body in `canvas.rs` (raster) and in `vector.rs`/`svg.rs`/`pdf.rs`, from its
   content coordinates mapped by `page_point`, in the place `draw_items` gives it. A kind that is **not drawn in exports**
   (bookmarks: « absents des exports image et vidéo ») is skipped there, and kept out of the replay's clock
   (`ardoise-media` `Clock::new`) so that it is not waited for.
3. **Interface**: a type and guard in `lib/note.ts`; its drawing in `Canvas.svelte` (`items`) and in `live-render.ts`
   (`drawObject`, `drawnObjects`) if it shows there; `selectable()` in `lib/editor/objects.ts`; its tool in `Toolbar.svelte`,
   `state.ts` `Mode`, `Canvas.svelte` (taps) and `Editor.svelte` (its editing: an equation reopens its source on a double click);
   its keys on the clipboard (`clipboard.ts`); the share's allow-list (`app/src-tauri/src/share/page.rs` `shared_objects`) if it
   is shown there.
4. **Search and assistant**: what it adds to the listing (`NoteEntry`, `NoteSummary`: a bookmark's label for the binder's table
   of contents and the search; an equation's source), and to the MCP read tools (`crates/ardoise-mcp/src/read.rs`).
5. **Docs**: `docs/file-format.md` §2.2 (its keys), this document (§2 if it moves differently, §4-like section for the kind).

Notes for the two planned kinds:

- **Equation**: keep the LaTeX source and the drawing made from it by MathJax in the interface (SVG path data in its own
  units, with its box), so that Rust draws it like a text frame's glyphs without running MathJax; `scale` resizes it; a double
  click reopens the source. Its paths may be large: if `note.json` would grow too much, they can go to a member
  (`objects/<id>.svg`) referenced by the object, a new optional member being a minor version too.
- **Bookmark**: a label at a point (`width`/`height` the label's box on screen, `scale` 1, its point the origin); drawn in the
  editor only, never in image or video exports nor the replay's clock; a PDF of a binder makes them PDF bookmarks; the listing
  gives them to the binder's table of contents (no note to open), as it gives the typed text.
