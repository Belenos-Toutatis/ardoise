# Page objects, and the Text tool

> Developer document (English). Requirements: SPEC §4.4 (« Texte tapé au clavier », « Équation », « Signets »), §4.3
> (« Sommaire du classeur »); decisions: `docs/questions.md` Q20, items 5 to 7. Bytes: `docs/file-format.md` §2.2.
> Bookmarks: §7. Code: `crates/ardoise-core/src/objects.rs` (model, placement, JSON), `crates/ardoise-text` (fonts, layout, glyphs),
> `crates/ardoise-render` (`draw_items`, `text.rs`), `crates/ardoise-media/src/replay` (replay), `app/ui/src/lib/editor/objects.ts`
> and `app/ui/src/lib/text.ts` (interface), `app/ui/src/components/editor/TextField.svelte` (typing).

## 1. What a page object is

A note's page held strokes only. It now holds **page objects** too (`Note.objects`, format 1.4): things placed on the page that
are not ink. Text frames are the first kind; equations (format 1.5: an object that keeps its LaTeX source and the vector drawing
made from it, `docs/design/equations.md`) the second; bookmarks (a short label at a spot, shown in the editor only: §7, format
1.6) the third. The layer was designed so that they are added as kinds, not as new mechanisms (§6).

Every object is two parts:

- **The placement**, the same for every kind: `id`, `layer`, `t`, `x`, `y`, `scale`, `quarterTurns`, `width`, `height`. The
  content of an object is drawn in its own coordinates (origin at the top-left corner of its content, y down, its own units) and
  put on the page by `page = (x, y) + turn(scale × (u, v))`. `width` × `height` is the content's box.
- **The body**, the keys of its kind (`kind` names it). Rust: `ObjectBody::Text(TextBody)`, `ObjectBody::Equation(EquationBody)`,
  or `ObjectBody::Unknown { kind, fields }` for a kind this version does not know (or a known kind with a value it does not
  know, such as a font of a later version). `ObjectBody::outline` gives what every output draws of a body, `is_drawn` whether it
  draws. TS: `PageObject` (placement plus any keys), `TextObject` and `EquationObject` with their guards `isTextObject` and
  `isEquationObject`, and `isDrawnObject` for both.

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
| Page size, orientation, « Adapter l'écriture à la feuille » | `ops.ts` `resizePage` | moved inside like the ink, or scaled with it (`scale` multiplied; a text frame's `wrapWidth`, in its own units, scales with it: the same lines) |
| Move / resize the selection | `state.ts` `transformSelection` | the selected objects follow the affine map (`transformObject`); a frame scaled keeps its lines |
| Side handles of one selected text frame | `Canvas.svelte`, `objects.ts` `rewrapped` | its `wrapWidth` changes (the left one moves `x`, `y` too): the text wraps again, the box is measured again (§4) |
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
| Writing clock (M17) | `writing-clock.ts`, `narration.rs` | a recording, and a new stroke, start after the last object's time too (a bookmark's time does not count) |
| Layers: hide | `state.ts`, `objects.ts` | a bookmark is not hidden with its layer (§7); it stays selectable and drawn |
| Back to the original | `original.ts`, `original.rs` | a bookmark is not ink: not part of the first version kept, and it stays when going back (§7) |

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
- **Network share**: only text frames and equations of visible layers are sent (`share/page.rs`), keys of later versions
  stripped.
- **Equations** are drawn by the same code from the outline they store (`ObjectBody::outline`; `equations.md` §1): no
  MathJax outside the editor's equation tool.
- **Projection feed**: a change of objects sends the whole page again (`projection-feed.ts` `sameLayout`).

## 4. Text frames

- **Fonts** (`crates/ardoise-text/fonts/`, `README.md`): Latin Modern Roman 10 regular and bold (GUST e-foundry 2.004, GUST
  Font License = LPPL 1.3c), Andika regular and bold (SIL 6.200, OFL 1.1 with Reserved Font Names) and Latin Modern Math (GUST
  e-foundry 1.959, GUST Font License): the official files, unmodified (subsetting would have meant renaming them under the
  licences), 2.4 MB in all. They are embedded in the executable (exports) and in the WebAssembly module of the interface (screen,
  and the bytes of the web fonts of the text field). `cargo xtask licenses` checks their SHA-256 and lists them in
  `THIRD-PARTY-NOTICES` and in « À propos ».
- **Which face draws a character** (`fallback_chain`, `FontFace::serves`): the frame's face, then Latin Modern Math (all of
  Greek, ϵ ϑ ϕ ϖ ϱ, the mathematical symbols and arrows), then the other family (Andika has the superscript and subscript digits).
  Andika is never used for the Greek and Coptic block: it has only the Greek of linguistics (no ε, η, ν, τ, Σ…), and a word
  mixing its letters with Latin Modern Math's would mix two designs. What each file lacks is listed by the test
  `the_faces_lack_what_latin_modern_math_and_andika_give`. Three exceptions draw another glyph **in the same advance** (the
  field's caret relies on the advances): in a bold frame a glyph of Latin Modern Math (one weight) is **emboldened** (its
  outline moved out by 1/50 em on each side, FreeType's synthetic bold); in a Latin Modern frame the superscript and subscript
  digits and signs Latin Modern lacks (it has only ¹ ² ³) are its own digits reduced and raised like its ¹ ² ³ (lowered by
  its OS/2 subscript offset), centred in Andika's advance; a character no face has is a **white square** □ (Latin Modern
  Math's), flagged (`GlyphLook::Missing`) so that the editor names it under the frame while it is typed. Default-ignorable
  characters (zero width space, soft hyphen, joiners, variation selectors) take no room and draw nothing, as in a browser.
- **Layout** (`ardoise_text::layout`): a line break starts a line; a frame with a **wrap width** (`wrapWidth`, format 1.7)
  also wraps, greedy, first fit: a line may break after a run of breaking white space (space, tab, the other breaking spaces
  of Unicode, the zero width space; never at a no-break space U+00A0, U+202F, U+2007, nor after a hyphen), the white space
  staying at the end of the line it ends (it hangs, even beyond the width, and never makes a line); a word wider than the line
  alone on it breaks before the character that crosses the width (a line holds at least one character); a line crosses the
  width only beyond a millionth of it (`WRAP_TOLERANCE`), so that a width read back from a page position never re-flows a line
  that fitted; a wrapped line is placed from its start again (the same positions as the same text alone). A frame without a
  wrap width (every frame made before 1.7) does not wrap: its lines stay exactly what they were. Glyphs by their advance widths
  (`hmtx`), **no kerning, no ligature**; a tab to the next multiple of four spaces; lines `1.25 × size` apart; within a line the
  baseline is where CSS puts it (half the leading, then the ascent of `hhea`). The box: without a wrap width the widest line
  with its trailing white space (as before); with one, the widest line's ink, white space counting up to the wrap width only
  (`Layout::width`): a short text has a narrow box (auto-crop, selection, reading order), however wide its column.
- **Wrap width of a new frame**: from where it is put to the page's right edge less 1 cm (`TEXT_WRAP_MARGIN`), at least six
  ems (`newFrameWrap`). **Side handles**: when one text frame is selected alone with the selection tool, two small bars in the
  middle of the content's left and right edges (turned with the frame) change its wrap width (`rewrapped`: the right one where
  the lines end, the left one moves the left edge and keeps the right boundary; never narrower than two ems); the text wraps
  again and the height follows; one undoable step per drag. A drag that starts at the edge of a frame narrower than its wrap
  width changes nothing until it passes the old width (any width between the ink and the wrap width lays out the same lines).
  The corner handles still scale the whole frame (its lines kept), and so does a resize of a selection of several objects. A
  size button keeps the frame's width on the page (`wrapWidth × scale`, then `scale` 1) and re-wraps.
- **The same glyphs everywhere**: `text_outline` gives the outlines as a `Path` with its curves; the WebAssembly module exposes
  `text_layout`, `text_path` (SVG path data) and the font bytes (`crates/ardoise-ink-wasm/src/text.rs`, `lib/text.ts`).
- **Typing** (`TextField.svelte`): a `<textarea>` laid over the frame, scaled and turned like it, styled with the same font
  files (registered as `FontFace`s from the module's bytes, Andika without the Greek block: `unicodeRange`, Latin Modern Math
  with one weight, which a bold field takes as it is under `font-synthesis: none`) listed in the layout's order, and the same
  rules (`textFieldStyle`: `white-space: pre`, `font-kerning: none`, `font-variant-ligatures: none`, the line height,
  `tab-size: 4`), its text transparent: what shows is the frame drawn from the module's glyphs as it is typed; the field gives
  the caret, the selection, the dead keys and input methods, and Windows' on-screen keyboard. Its lines are put on the frame's
  baselines from the font's ascent and descent as the browser measures them (`measureText`). Being a real field, the keys typed
  are the field's: the editor's shortcuts ignore keys typed in a field (Ctrl + S still saves what is typed).
- **The field never wraps by itself** (`lib/text-field.ts`): making the browser break lines where Rust does (UAX #14, its own
  rounding of advances) cannot be exact, so the field's value is a **display** of the frame's text: a line break of its own
  (« soft ») where the frame wraps, the white square where a character no face has stands, each position mapped to the text.
  The field's lines are thus the layout's lines, glyph for glyph, and its caret falls between the drawn glyphs (the e2e
  journey compares the browser's advances of each line with the frame's box). What the browser makes of the value (a key, a
  paste, an input method at its end) is turned back into the text by comparing the values before and after
  (`applyFieldInput`): removing only a soft break removes the character before it (Backspace, the space at the end of the line
  above) or after it (Delete), as a word processor does; Enter is a line break of the text. The value is set again only when
  the wrapping changes (the selection kept through the map). Setting it loses the browser's undo, so the field has its own
  (`FieldHistory`: Ctrl + Z, Ctrl + Y and the context menu's; letters typed in a row one step); a copy or a cut carries the
  text without the soft breaks. The whole typing is still one step of the editor. The field is as wide as the wrap width (its
  dashed outline shows where the lines end), or the widest line with its hanging white space.
- **« Caractères » tab** (`CharactersTab.svelte`, `lib/editor/greek.ts`, `lib/editor/text-input.svelte.ts`; the user's
  choice of 2026-11-15 after the first pop-up palette, which covered the frame being typed): a tab of the editor's side panel
  (`side-panel.md` §14), before Aide, beside the page. The small letters (ς included), the capitals, ϵ ϑ ϕ ϖ ϱ, symbols, the
  superscripts and subscripts, by titled groups, keys that wrap to the pane's width (six a row in the 290 px pane, four at 240 px),
  drawn in the frame's family and weight (the tool's otherwise), each with its name (FR/EN). A press inserts at the caret of
  the frame being typed (`TextInputTarget.insert`, in place of the selection, as a change of the typing: the same editor step)
  and takes the focus from nobody; from the keyboard (Tab into the tab, arrows, Enter) the character goes in and the focus
  goes back to the frame. The field's blur ignores the keys and the tab's own button on the rail (the focus comes back to the
  field after that press); leaving the tab for elsewhere ends the typing. With no frame being typed the keys are disabled
  and the tab says to touch the page with the Text tool, with « Outil Texte » when another tool is chosen. Escape has no step
  for it: it ends the typing, the tab staying as it is. The button « αβγ » of the text options shows the tab (`toggleCharacters`
  in `Editor.svelte`, rule in `side-panel.md` §14).
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

- Wrapping (format 1.7, the user's request of 2026-11-14): a new frame wraps at the page's right edge less 1 cm; one could
  instead grow with the text up to a maximum, or wrap at a fixed width (10 cm). The box is the text's extent, not the column:
  a short text in a wide column is selected and cropped by its words. Break opportunities are white space only (no hyphen,
  no hyphenation, no CJK rules): what a teacher types in French and English; a word longer than the line is cut where it
  crosses it. A frame made before 1.7 keeps its lines until its width is set by a side handle.
- No kerning and no ligatures (« fi » is two glyphs), so that the browser's field and our layout agree to the pixel.
- Greek: Latin Modern Math rather than CMU Serif (the Computer Modern Unicode fonts, OFL, with a real bold Greek): the same
  foundry and family as Latin Modern, one file within the user's consent; its bold Greek is synthesised (emboldened outlines).
  Andika frames take their Greek from it too (serif Greek in a sans frame), never from Andika's partial Greek.
- The superscripts and subscripts of a Latin Modern frame are drawn from its own digits (Andika's would be sans serif next to
  Latin Modern's ¹ ² ³); in Andika's advance, so that the field's caret stays exact.
- The eraser does not erase text; a resize scales the text (its size becomes any value until a size button is pressed).
- « Revenir à l'original » of a tablet note removes the frames typed since (the tablet's page has none).
- The search leaves out frames of hidden layers, as the handwriting search does.
- Unknown kinds in the editor are kept but neither drawn nor selectable.

## 6. How to add a kind

The equation lot plugs in here (the bookmark lot did: §7). For a kind `k`:

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

Notes for the planned kind:

- **Equation** (done, format 1.5: `docs/design/equations.md`): the LaTeX source and the drawing made from it by MathJax in the
  interface (path data in 1/1000 em, its box measured from it), drawn by Rust like a text frame's glyphs without MathJax;
  `scale` is its size; a double click reopens the source. The drawing stays in `note.json` (at most 256 KiB each); a member of
  its own (`objects/<id>.svg`) remains possible later, as another minor version.

## 7. Bookmarks (SPEC §4.4 « Signets », §4.3 « Sommaire du classeur », format 1.6)

A bookmark is a word or a short phrase, one line, at a spot of the page: a label to find one's way in a binder (« Chapitre 3 »,
« Corrigé de l'exercice 2 »). The user's reason: a search inside one note is useless (a note is one page), what helps is to look in a
whole binder. It is a page object of the kind `bookmark` (`ObjectBody::Bookmark(BookmarkBody { label })`, bytes in
`docs/file-format.md` §2.2), so every operation of §2 carries it with no code of its own, except the few places below.

- **Content and box.** Drawn like a small text frame: a flag, then the label in Andika at 5 mm, on a chip, with room around them
  (`BOOKMARK_*` in `objects.rs`, mirrored by `BOOKMARK` in `objects.ts`: the same numbers, so that Rust and the interface measure the
  same box, `PageObject::measure` and `bookmarkLook`). The size is the page's, not the screen's: it is a label in the margin that
  shrinks with the zoom, like the text frames, and a resize of the selection scales it. The label is cut at 60 characters and kept as
  one line (`clean_bookmark_label`, `cleanBookmarkLabel`: the same rules).
- **Only the editor shows it.** `draw_items` (every export: PNG, JPEG, WebP, SVG, PDF page, print, clipboard image, MCP render,
  thumbnails) leaves it out, so the replay, the video and the replay's clock never see it (`ardoise-media` builds on `draw_items`;
  `replay.ts` counts only text frames and equations); the projection window and the share draw text frames and equations only. `Note::has_content`,
  the clocks (`ops::time_range`, `split_at_time`, `writing_origin`, `last_content_time`, `noteEnd`, `lastSampleTime`) and the split
  panel's time range ignore it: a bookmark never makes a page non-empty, never lengthens a replay, never pushes a recording later.
  The projection feed does not send the page again for a bookmark (`projectedObjects`).
- **Layers do not hide it** (a choice the user may want otherwise). It has a `layer` like every object (the chosen layer when it
  was put), which moves with a merge or a reorder, but hiding the layer hides neither the bookmark nor its listing: a table of
  contents that loses entries when a layer is hidden would be wrong. So `drawItems`, `objectDrawingOrder`, `reachable` (lasso,
  click, Ctrl + A's layer filter excepted), the crop and the split by zone treat a bookmark as visible and not tied to the chosen
  layer. The text frames keep the rule of §2.
- **The tool** (`Mode` `'bookmark'`, key B, « Signet » in the toolbar): a tap with the pen, the mouse or a finger puts a bookmark
  (the tap is the middle of its height, at its left end) and opens `BookmarkField.svelte`, a one-line field just under it (above it
  near the bottom), in the interface's font; what is typed changes the present doc without a step (`typeBookmark`), and the end of the
  typing (Enter, Escape, the field left, another tool, undo) is **one undoable step** (`endBookmark`), or none when nothing changed;
  an empty label removes the bookmark. A tap on an existing bookmark with the tool renames it, a double click with the selection tool
  too. Moving, resizing, deleting, duplicating, copying (the clipboard carries the label and the place), pasting are the selection's.
  Why a tool and not a command followed by a tap: it is the same gesture as the text tool's (the user already knows it), it works with
  a pen or a finger with no mode to enter and leave, and the keyboard shortcut and the tool's help line say what to do. The keys typed
  in the field never reach the editor's shortcuts; Escape follows `escape.ts` (`editingText` is true for a bookmark too).
- **Listing and summary.** `Note::bookmarks()` (`BookmarkRef { id, label, x, y }`: reading order, every layer, blank labels left
  out, the centre of the box rounded) goes into `NoteEntry` and `NoteSummary` as the typed text does (`note.json` only). The summary
  of a binder, the search, the opening at a spot and the PDF outline are in `library-order.md` §3 and §6.
- **"Back to the original"** (`original.rs`, `original.ts`): a bookmark is not ink. Adding, renaming or moving one is no change of
  the ink (`same_ink`: no first version kept for it; a Xiaomi page update is not refused for it either, `pages.rs`), the first version
  kept later holds none, and going back to it keeps the bookmarks the note has (on the last layer the original has).
- **Not drawn by the assistant**: `get_note` lists the bookmarks (`bookmarks`, label and `at_mm`), `search_notes` finds them
  (`matched_in: ["bookmarks"]`), `render_note` does not draw them, and every write tool keeps them (they are objects).

Choices that could have gone another way: the bookmarks of a page whose layer is hidden still count (above); a bookmark's size does
not follow the screen (a small label on a zoomed-out page is small); the summary does not reopen when one comes back from the note;
the selection panel counts a selected bookmark in « textes, équations et signets » (one count for every page object); « Revenir à l'original » keeps
the bookmarks although the page may then have another size (the bookmark can lie outside it: it can still be selected from the
listing, not drawn).
