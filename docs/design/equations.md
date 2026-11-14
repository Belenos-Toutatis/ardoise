# Equations

> Developer document (English). Requirement: SPEC §4.4 « Équation »; decision: `docs/questions.md` Q20, item 7. The page-object
> layer it is a kind of: `docs/design/page-objects.md` (§6, how a kind is added). Bytes: `docs/file-format.md` §2.3. Code:
> `crates/ardoise-core/src/objects.rs` (`EquationBody`, reading and limits), `crates/ardoise-text/src/path.rs`
> (`Path::from_svg`), `crates/ardoise-render/src/text.rs` (every output), `app/ui/src/lib/equation-path.ts` (the stored
> drawing, in the interface), `app/ui/src/lib/equation.ts` (MathJax's output to our drawing), `app/ui/src/lib/mathjax.ts`
> (MathJax, loaded lazily), `app/ui/src/components/editor/EquationPanel.svelte` (the panel).

## 1. What is stored, and why

An equation is a page object of kind `equation` (format 1.5) with three keys: `latex` (the source), `path` (the drawing) and
`color`. The drawing is MathJax's, made in the editor, kept as filled outlines in the object's own units (1/1000 em, y down,
origin at the top-left corner of the drawing). So:

- **Every output draws it without MathJax.** Rust parses the path data (`Path::from_svg`) into the same `Path` a text frame's
  glyphs are, and draws it exactly where text frames are drawn: raster (PNG, JPEG, WebP, thumbnails, print, clipboard, MCP
  render), SVG (one `<path>`, the source as its `<title>`), PDF (vector fills; no invisible text: LaTeX is not text to search
  in a PDF), videos and replay (all at once at `t`), projection, the share's viewers and resumed live sheets
  (`live-render.ts`, from the stored data). The text frames' code paths were generalised (`ObjectBody::outline`,
  `ObjectBody::is_drawn`, `isDrawnObject`), so an output that draws text frames draws equations.
- **The source is what is edited.** A double click (or a tap of the tool) reopens it; nothing is ever re-derived from the
  drawing.
- **The size is the placement's.** No `size` key: the scale is the em on the page divided by 1000 (`EQUATION_UNITS_PER_EM`);
  the tool's three sizes (4, 6, 9 mm, the text tool's) set it, a resize multiplies it. An equation at « Moyen » has the em of a
  text frame at « Moyen »; MathJax's font (New Computer Modern) is Latin Modern's design, the text tool's first family.
- **The box is the drawing's.** `width`/`height` go from the origin to the farthest point of the outline, control points
  included (`PageObject::measure`, `equationBox`): the lasso, the click, the crop and the split by zone use it. Both sides
  measure the rounded data, so they agree.

The path data is kept in `note.json` (not in a member of its own): a line of chemistry is about 10 KB, the limit 256 KiB, and the
listing already reads `note.json` alone.

## 2. Trust: nothing of MathJax's markup reaches the note or the DOM

MathJax renders into its own light DOM (`liteAdaptor`), never the document's. `lib/mathjax.ts` copies that tree into plain data
(`SvgNode`), and `lib/equation.ts` walks it keeping geometry only:

- `<path>` (filled): its `d` (any SVG command but arcs), made absolute; `<rect>` (filled; a `fill="none"` frame, `\boxed`, as a
  ring of its stroke width); `<line>` (table rules, `\cancel`): a rectangle of its width (70 for table lines, MathJax's style
  sheet), dashed or dotted as `mjx-dashed`/`mjx-dotted` say; `<ellipse>`/`<circle>` likewise; `<g>` and every `transform`
  (`translate`, `scale`, `rotate`, `matrix`, `skew*`); nested `<svg>` (the extenders of stretched delimiters) with their view
  box and their clipping (Sutherland–Hodgman on the flattened contour when a contour crosses the viewport).
- Refused with a reason, so that the equation cannot be placed: `<text>` (a character MathJax has no glyph for: the panel names
  it), a stroked `<path>`, a numbered equation (`\tag`: laid out against a page width; `width="100%"`, `data-labels`), any other
  element.
- Colours are ignored (one colour per equation, the tool's); `\color` is not even available (the `color` package is not loaded).
  Strokes are ignored on filled paths (MathJax's glyphs are filled; a stroke used to embolden one would be lost, supposé:
  not seen with this font); a nested `<svg>` under a turned or skewed group is not clipped (not seen in MathJax's output).

The result is moved to the origin, rounded to a tenth of a unit, written with absolute `M`/`L`/`Q`/`C`/`Z` only, read back with
the stored grammar, and refused beyond 256 KiB. On the page the editor sets this data as a `<path d>` of its own SVG, the
preview too (the page from the steps read and written out again, `equationPathData`: the browser's SVG parser is less lenient than ours about commas, and must draw what Rust draws); the live renderer traces the steps on a canvas.

A note read from a file is checked the same way on both sides (`equation_body` in Rust, `isEquationObject` in TS): the source
and the drawing within their limits, a colour of four bytes present, the drawing in the grammar (linear, no exponent, coordinates within ±10⁶). An equation that
fails is kept with its keys and drawn nowhere, not selectable (an unknown kind); the other objects of the note are read. Writers
refuse an equation they could not read back. The clipboard checks a pasted equation the same way before anything is pasted.

## 3. MathJax in the interface

- **Version and packages**: `@mathjax/src` 4.1.3 (Apache 2.0) from its sources, with `@mathjax/mathjax-newcm-font` and
  `@mathjax/mathjax-mhchem-font-extension` 4.1.3 (the glyphs of mhchem's arrows). TeX input with `base`, `ams`, `newcommand`,
  `mhchem` (`\ce`, `\pu`), `textmacros`, `gensymb` (`\celsius`, `\ohm`…), `cancel`, `boldsymbol`, `upgreek`; SVG output with
  `fontCache: 'none'` (each glyph a path, no `<use>`), display style. `formatError` throws: an invalid code is MathJax's
  message in the panel (in English: it is MathJax's), never a red formula.
- **Lazy**: `lib/equation.ts` imports `lib/mathjax.ts` dynamically the first time the tool converts; Vite makes it a chunk of
  its own (about 1.4 MB, 0.5 MB compressed, the base font included). The editor's start does not load it.
- **No network**: the font's parts beyond its base are MathJax "dynamic files"; `mathjax.asyncLoad` answers from a fixed map of
  `import()` of the parts shipped (accented Latin in four styles, arrows, math symbols, shapes, accents, double-struck,
  calligraphic, script, fraktur, sans-serif, monospace), each a lazy chunk of the app. A part not shipped (Cyrillic, Hebrew,
  Arabic, accented Greek…) is refused, MathJax has no glyph, and the panel says which character.
- **One TeX input per conversion** (about 2 ms): what an equation defines (`\newcommand`, `\def`, `\label`) never changes how
  another is drawn, nor makes it depend on what was typed earlier in the session.
- **Bounded**: the source at most 4000 characters; MathJax's own limits stop runaway macros (`\def\a{\a}\a`: "maximum macro
  substitution count exceeded"); conversions run one at a time; the copy of the tree stops at 400 levels.

## 4. The editor

- **Tool** `equation` (`Mode`, toolbar « Équation », key `M`; not in live mode). A tap (pen, mouse, a finger's tap) opens the
  panel (`EquationPanel`, placed by the canvas under the tapped point, or above it near the bottom) for a new equation there, or
  for the equation of the chosen layer under the tap; a double click on an equation with the selection tool too. While a new
  one is written, a tap elsewhere moves where it will go.
- **Panel**: the source (a monospace field), the preview (our own drawing, on the note's paper colour, in the colour the
  equation will have), the status (MathJax's error, or « Calcul de l'aperçu… »), the keys, examples, « Annuler » and « Poser » /
  « Mettre à jour ». The conversion waits 150 ms of quiet typing. Enter puts it down (when the drawing of what is typed is
  ready: an Enter typed meanwhile waits for it); Shift + Enter is a new line. Being a field, its keys never reach the editor's
  shortcuts (`isTyping`); Ctrl + S still saves.
- **Steps**: nothing changes on the page while the panel is open; « Poser » is one undoable step (the equation added, then
  selected); « Mettre à jour » is one step (source and drawing replaced, id, time, scale, turns and colour kept, the top-left
  corner of the box where it was). Escape (`lib/editor/escape.ts`: `cancelEquation`, right after ending a text's typing),
  another tool, undo or redo, the replay, the split, another note: the panel closes, the page unchanged. A tap while a new
  equation is written only moves where it will go (on another equation too: the typed source is never thrown away);
  an equation deleted while its panel is open is put back as it was (place, size, colour), rewritten; with the chosen
  layer hidden, the panel stays open.
- **Placement**: a new equation's left end at the tap, its middle at the tap's height (as a text frame's first line); its time
  is the one a stroke drawn now would take (the sound's clock during a writing recording).
- **Size and colour**: the equation rewritten stays selected while its panel is open, so the toolbar's colours and sizes act on
  it as ordinary steps; with the tool or equations selected, the toolbar shows the three sizes. A size sets the scale (top-left
  corner kept); the colour applies like a text frame's.
- **With the strokes**: selection (lasso, click, Ctrl + A), move, resize, delete, duplicate, copy and paste (also into another
  note: `clipboard.ts` carries placement, source, drawing and colour), layers, page turns, crops, resizes, split and merge, back
  to the original: through the placement, as every object (`page-objects.md` §2). The eraser does not erase it.

## 5. Search and assistant

- `Note::typed_text` (Rust) and `typedText` (TS) read an equation as its LaTeX source, in reading order with the text frames,
  visible layers only: the library's search and `search_notes` find `\ce{2H2 + O2 -> 2H2O}` by « H2O ». The listing has no new
  field.
- MCP: `get_note` lists `equations` (source, colour, `size_mm`, `bounds_mm`, layer, visibility, time); `get_note_text` lists them
  too (`source: "equation"`) and has their source in `typed_text`; `other_objects` no longer counts them. Every write tool keeps
  them (they move through the placement).

## 6. Choices that could have gone another way

- Escape **drops** the equation being written (SPEC: « annule l'action en cours »), where it ends a text's typing with the text
  kept: an equation is not on the page until it is put down.
- Enter puts the equation down; Shift + Enter is a new line in the source.
- Display style always (`\frac` full size, limits above and below); no inline style.
- No equation numbers (`\tag` refused), no colours inside an equation, no packages beyond those listed; `\href`, `\class`,
  `\unicode`, `\require` are undefined.
- The PDF carries no searchable text for an equation; the SVG carries its source as the path's title.
- MathJax's error messages are shown as MathJax writes them (English), after a translated prefix.
- `THIRD-PARTY-NOTICES` lists every npm dependency of MathJax's packages (speech-rule-engine, mj-context-menu and the tools the
  font packages depend on), though the bundle holds only the modules `lib/mathjax.ts` imports: the check of the licences follows
  `dependencies`, it does not read the bundle.
