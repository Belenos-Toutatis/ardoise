# Split, merge, export (milestone M5)

Developer document (English). Requirements: SPEC §3.2 (automatic exports), §4.3 (multiple selection: export,
merge), §4.4 (split a page, assemble pages), §4.6 (PNG, JPG, SVG, PDF, clipboard), §6.2 ("one drawing of the
strokes"), §9 (licences). Models: `docs/design/notes-and-rendering.md`, `docs/design/interface.md`.

```
ardoise-ink     outline(&Stroke)  ── the only stroke geometry, for every format below
     ^
ardoise-render  look: what each tool paints (fills, opacity, blend, grain)  ─┬─ raster (tiny-skia): PNG, JPG,
     |          background: paper + template as primitives                  │    clipboard image
     |                                                                      ├─ svg: one <path> per fill
     |                                                                      └─ pdf: one page per note
ardoise-core    ops: split at an instant / by zone, merge (overlay, end to end) — pure functions on notes
ardoise-device  pages: a synced page becomes a note of the library (LibrarySink), then an after-save hook
     ^
app/src-tauri   export.rs, clipboard.rs (commands), operations.rs (split/merge commands),
                autoexport.rs (the after-save hook: PDF/PNG into <library>/<folder>/)
app/ui          export dialog, split panel in the editor, merge in the library selection, settings
```

## Shared rules

Page objects (text frames, format 1.4) are drawn by every format and carried by split and merge: `docs/design/page-objects.md`
(§2 for the operations, §3 for the drawing).

- One drawing of the strokes: every format fills `ardoise_ink::outline(stroke)` with the non-zero rule. What
  a tool paints (felt-tip halo, highlighter blending, pencil and chalk grain, opacities) is described once, in
  `ardoise-render` (`look`), and consumed by the raster painter, the SVG writer and the PDF writer. Vector
  formats cannot reproduce the grain exactly: they fill with the grain's mean coverage as opacity.
- Layers: strokes on hidden layers are not exported; a stroke whose layer has no entry is drawn on top
  (as `render_note` does).
- Every file written into the library (automatic exports, split and merged notes) goes through
  `ardoise_core::fsutil::write_atomic` or `Library::create` / `Library::save` (SPEC §3.4). Exports written
  elsewhere (user-chosen destination) are atomic too.
- No `unwrap`/`expect` on user or file data; typed errors in libraries; `{ code, message }` to the interface.
- New dependencies: licences compatible with GPL v3 only (MIT, Apache-2.0, BSD, zlib, ISC, BSL-1.0,
  MPL-2.0…), never AGPL (M10 checks it automatically). Prefer small pure-Rust crates.

## ardoise-render (exports)

- `RenderOptions` and `render_note` keep their signature (thumbnails, Inkspace comparison and golden images use
  them); the reference images must stay identical (`cargo test -p ardoise-render`).
- Transparent PNG: a separate entry point (no paper at all, alpha kept), e.g. `render_note_transparent`.
- JPG: `encode_jpeg(&Image, quality 1..=100)`, alpha flattened over white, baseline, 8-bit, sRGB.
- SVG: `note_to_svg(&Note, &SvgOptions) -> String`: `width`/`height` in millimetres, `viewBox` in device units
  (10 µm), background (paper colour, template as vector shapes, texture as an embedded raster image when on)
  unless disabled, one `<path fill-rule="nonzero">` per fill of each visible stroke, highlighter with
  `mix-blend-mode`. Opens in Inkscape and in browsers.
- PDF: `notes_to_pdf(&[&Note], &PdfOptions) -> Result<Vec<u8>, RenderError>`: one page per note, in order;
  page size `Actual` (the note's size: 1 device unit = 72 / 2540 pt) or `A4` (210 × 297 mm, orientation of the
  note, the note scaled to fit and centred); strokes as vector paths (non-zero fill), opacity through
  ExtGState, highlighter with `/BM /Multiply` (or `/Screen` on dark paper); background as for SVG.
- Clipboard data: `clipboard_image(&Image) -> ClipboardImage { png, dib }`: `png` for the registered "PNG"
  clipboard format (Word, OneNote keep transparency and quality), `dib` for `CF_DIB` (BITMAPINFOHEADER, 24-bit
  BI_RGB, bottom-up rows padded to 4 bytes, alpha flattened over white) for every other program. Pure function,
  tested on Linux; the clipboard itself is only touched in `app/src-tauri/src/clipboard.rs` (Windows).
- Test that proves the milestone (`cargo test -p ardoise-render --test export_formats -- --nocapture`): a
  reference note is exported in the 5 formats and each file is validated by an independent reader (decoder or
  parser used only in tests), printing a table: format, size in bytes, valid, dimensions, pages.

## ardoise-core::ops (split and merge)

Pure functions on `Note` (no I/O); ids, titles and dates of new notes come from the caller.

- `split_at_time(note, at_ms) -> Split { kept, extracted }`: samples with `t < at` stay, samples with
  `t >= at` go to the new note; a stroke that spans the instant is cut into two strokes. The new note's
  `created` is `note.created + first extracted t`, and its times are shifted so that they start at 0.
- `split_by_zone(note, polygon) -> Split`: a stroke goes to the new note when at least half of its samples are
  inside the polygon (even-odd point-in-polygon), only strokes on visible layers — the rule of the editor's lasso
  (`selectInLasso` in `app/ui/src/lib/editor/hit.ts`, share 0.5), so what the user sees highlighted is what
  moves. Positions and times unchanged.
- Both refuse (`OpsError::NothingToSplit`) when one side would be empty. The new note copies page size,
  background, layers, tags and timing; it is `Origin::Ardoise`, not a favourite, has no `original.bin`.
- `merge(notes, MergeLayout) -> Note`: at least 2 notes (`OpsError::NotEnough`), in the order given (the
  interface passes them oldest first). `Overlay`: one page as large as the largest (max width × max height),
  every note at the top-left; each source note's layers are appended (names prefixed with the note's title when
  it has several layers, else the title alone). `EndToEnd`: notes stacked vertically (a long page: max width ×
  sum of heights), layers merged by index. Background, tags (union) and timing (`Synthesized` if any is) follow;
  times are made sequential (each note starts 1 s after the previous one ends) so the replay plays the notes in
  order. Page sides above `u32` or above 100 m are refused (`OpsError::PageTooLarge`).

## ardoise-device::pages (synced page → note)

- `note_from_page(&DownloadedPage, page: PageSize, id, layer_name, now_ms) -> Result<NoteFile, PageError>`:
  decodes the tablet file, black ballpoint strokes (`TABLET_PEN_WIDTH`), pressure / 2047, times from the stroke
  headers (Slate clock since power-up, wrapping every 327.68 s, unwrapped), 5 ms per sample; `created` = the
  page's timestamp (or `now_ms`), `Origin::Tablet`, `original.bin` = the raw bytes.
- `LibrarySink`: stores each page as a new note in a binder of the library (`Library::create`), reads it back to
  verify it (only then may the session delete the page from the tablet), then calls the after-save hook
  `FnMut(&SavedNote, &Note)`. The hook cannot fail the sync (an automatic export that fails is logged by the hook,
  the note is safe). `sink.store(page)` fits `Session::sync`'s sink closure.
- This is the place M4's automatic sync goes through; M5 only adds the hook.

## app/src-tauri

| Command | Arguments | Returns |
|---|---|---|
| `export_notes` | `sources: ExportSource[]`, `format: 'png'\|'jpg'\|'svg'\|'pdf'`, `options: ExportOptions`, `destination` | written paths |
| `copy_note_image` | `source: ExportSource`, `dpi` | – (Windows clipboard; `clipboardUnavailable` elsewhere) |
| `split_note` | `path`, `note`, `stamp`, `split: SplitSpec`, `title` | `SplitResult { note, stamp, created: NoteSummary }` |
| `merge_notes` | `paths`, `layout: 'overlay'\|'endToEnd'`, `title` | `NoteSummary` of the new note |
| `set_auto_export` | `autoExport: { format: 'off'\|'pdf'\|'png', folder }` | `AppState` (with `autoExport`) |

- `ExportSource { path, note? }`: the note at `path` in the library, or the editor's current (unsaved) `note`;
  `path` also names the output file. `ExportOptions { dpi, background, quality, pdfPage: 'actual'|'a4' }`:
  `dpi` for raster formats (72..=600), `background` false = no paper (transparent PNG; white for JPG and the
  clipboard), `quality` for JPG (1..=100).
- Destination: PDF → one file holding every source, one page each. PNG/JPG/SVG → a file when there is one
  source, a folder when there are several (one file per note, named like the note file, ` (2)` added rather than
  overwriting). A file destination is overwritten (the save dialog asked).
- `split_note` first creates the new note in the same binder (title from the interface), then saves the kept
  part over `path` with the stamp check; if that save fails the new note is moved to the trash and the error is
  returned (nothing is lost either way). `merge_notes` never changes its sources; the new note goes into the
  binder of the first one.
- Error codes (translated in `errorCodes.*`): `exportFailed`, `clipboardUnavailable`, `nothingToSplit`,
  `nothingToMerge`, `pageTooLarge`, `invalidFolder`, `imageTooLarge` (an image side above 16 384 px: lower the
  resolution or use PDF/SVG), `cancelled` (the user stopped an export), plus the existing library codes.
- `cancel_export` (no arguments, returns nothing) sets a shared flag; `export_notes` resets it when it starts and
  checks it between notes and before each write. A cancelled export writes nothing more (a single file or a PDF
  is not written at all; the files of a folder already written stay) and fails with `cancelled`.
- Settings: `[auto_export]` in `ardoise.toml` (`format = "off"|"pdf"|"png"`, `folder = "Exports"`), shared by
  every PC (the library is shared). The folder is relative to the library root (no `..`, not `_ardoise`); it is
  not listed as a binder. One file per note: `<folder>/<binder>/<note file stem>.pdf|.png`, rewritten
  atomically when the note is synced again.

## Interface

- Editor toolbar: Split (panel with a timeline slider over the note's time range and a "zone" mode that traces a
  lasso; strokes that will move are highlighted live), Export (dialog), Copy image (clipboard).
- Library selection bar: Export (dialog; PDF puts every selected note in one file), Assemble (≥ 2 notes: overlay
  or end to end, title).
- Settings: automatic export after sync (off / PDF / PNG, folder).
- Browser stand-in: the new commands are routed in `lib/mock.ts` (export returns fake paths, split and merge
  work on the demo store).

## Decisions made while building M5

- **One look per tool.** `ardoise_render::look` turns a stroke into fills (outline, colour, opacity, blend, grain,
  seed); `Look::raster(px_per_unit, dark)` feeds tiny-skia, `Look::vector(dark)` feeds SVG and PDF (grain as its
  mean coverage, felt-tip halo sized for 300 dpi). The golden images did not change by a single pixel.
- **Vector background.** `draw_template` works over a `TemplateCanvas` trait: the raster canvas keeps its pixel
  snapping and fading, the vector one keeps exact rectangles and discs. The paper texture is embedded as a 100 dpi
  JPEG under the template in SVG and PDF.
- **JPEG.** No GPL-compatible pure-Rust encoder without the IJG DCT code was found, so a baseline encoder was
  written from ITU-T T.81 (Annex K tables): error within 1 % of libjpeg at quality 10–95, files ~9 % larger.
- **PDF.** `pdf-writer` (MIT/Apache-2.0). A4: margins stay white, the note is clipped to its page; pages longer
  than ~5 m use `/UserUnit`. **SVG**: one Inkscape layer per note layer, the background in a locked layer.
- **Clipboard.** `copy_note_image` also receives the window (its HWND owns the clipboard, as `OpenClipboard`
  recommends); the steps (open with 10 tries, empty, "PNG" best effort, `CF_DIB` required, free what the system
  refused, always close) are written once over a small trait and tested with a fake on Linux.
- **Split.** Each sample goes by `t < at`, so non-monotonic times lose nothing; a stroke without samples stays in
  `kept`. The zone rule is `inside > 0 && 2 × inside ≥ samples`, computed in f64 like `hit.ts`. `split_note` checks
  the stamp before creating anything, copies the page background file to the new note, and returns the note and
  stamp read back from disk.
- **Merge.** `created` of the first note; overlay layers named `Title - Layer` (several layers) or `Title`; end to
  end: left-aligned, layer names from the first note having that index, a layer visible when it is visible in any
  note that has it (no note's strokes vanish from the merged note); a stroke on a missing layer
  goes to its note's last layer; tags merged by `normalize_tags`.
- **Auto-export folder.** Hidden from the binders of the interface listing only while the export is on (format
  pdf or png) and the folder is valid, and never when it holds notes (a user's own binder with that name stays
  visible). `set_auto_export` refuses (`invalidFolder`), with the export on, a folder that is an existing binder
  holding notes. A format this version does not know (written by a newer one) loads as off, is reported as `off`
  to the interface and is written back verbatim. A PNG too large at 150 dpi is exported at a lower resolution
  that fits (`AutoExportReport::lowered`). `autoexport::after_save_hook` is ready; M4 plugs it into the
  automatic sync (the app has no sync command yet), and its simulator test proves the chain
  (`autoexport::tests::synced_pages_are_exported_automatically`).

## M8: WebP and direct printing

### WebP in the image export

- `ardoise_render::encode_webp(&Image)`: lossless WebP (VP8L) by **libwebp** (`libwebp-sys2`, BSD-3-Clause / MIT, built
  from source with `cc`; `ardoise-media` already builds the same library for animated WebP through `webp-animation`),
  lossless preset at libwebp's default effort 4, hand-written FFI in `webp.rs` (a picture imported as RGB or RGBA, a
  writer callback filling a `Vec<u8>`). The pure-Rust encoder of `image-webp` (already in the workspace through
  `resvg`'s dev dependency) was tried first and **measured**: it gives files 2.1 times the PNG's on the reference note
  (paper and Seyès texture) and 1.3 times the PNG's for the ink alone, so a WebP would have been the heavier choice;
  `image-webp` stays a dev dependency, as the independent decoder of the tests. RGB when every pixel is opaque (a note
  with its paper), RGBA otherwise (no paper: transparent, like the PNG; the colour under a fully transparent pixel is
  not kept). WebP stores no resolution (no `pHYs` equivalent), and a side is limited to 16 383 px (`WEBP_MAX_SIDE`, one
  less than the other image formats): a larger page gives `imageTooLarge`, and the dialog hides the resolutions that
  would produce it (`dpiFits(pages, dpi, 'webp')`).
- `ExportFormat::Webp` behaves like PNG everywhere: resolution 72 to 600 dpi, `background` false = transparent, one
  file per note in a folder for several notes (` (2)` rather than overwriting). The format help has its card
  (`export.formatPros/formatCons.webp`) and its column in "Quel format choisir ?". The automatic export after sync
  stays PDF or PNG.
- Measured on the reference note of `export_formats` (`cargo test -p ardoise-render --test export_formats --
  --nocapture`, 150 dpi): with the paper 889 382 bytes against 1 107 957 for the PNG (80 %), without the paper 67 504
  against 102 571 (66 %). The format help says "about a quarter smaller".

### Direct printing

- Mechanism: **GDI through the system print dialog**, not the web view's print and not a PDF handed to a PDF reader.
  `PrintDlgEx` (shown, with the job, on a thread of its own in a single-threaded COM apartment: a pool thread may already be
  in the multithreaded one) gives the printer, its settings (copies, duplex, paper tray…) and the page range; the job
  (`print_windows::print_job`) then draws one page per note: the device context's orientation follows the note
  (`ResetDC` with `dmOrientation` between two pages, so a portrait note and a landscape note of the same job each fill
  their sheet), the note is rendered as an image (`ardoise_render::render_note`, the same drawing as the PNG and PDF
  exports) at the resolution of the sheet, 300 dpi at most, and stretched over the printable area
  (`StretchDIBits`, halftone). Reasons: the print dialog is the real Windows one, the orientation can change page by
  page (the web view's `window.print()` shows Edge's own preview and takes one orientation per job in practice), no
  third-party reader is needed, and the job can be aimed at "Microsoft Print to PDF" with an output file in the test.
  The cost is a raster page (ink and textures at 300 dpi) instead of vectors.
- Platform-free and tested on Linux (`print.rs`): `Orientation::of(page)` (landscape when wider than tall),
  `place(page, sheet, fit)` (centred, proportions kept; `Actual` = real size, reduced only when the note is larger than
  the printable area; `Fit` = as large as fits), `render_scale`, `selected_notes` (the ranges of the dialog),
  `bitmap_of` (RGBA to opaque BGRA). `print_windows.rs` is Windows only; elsewhere `print_notes` answers
  `printUnavailable`.
- Command `print_notes(sources: ExportSource[], options: PrintOptions { background, fit: 'actual'|'fit' }) ->
  PrintResult { printed, pages }`. `printed` false = the user closed the dialog (not an error). The dialog runs on a
  blocking thread, owned by the app's window. Errors: `printFailed`, `printUnavailable`.
- Interface: "Imprimer…" in the editor's toolbar (the note as it is in memory) and in the library selection bar, both
  opening `PrintDialog` (paper printed or not, real size or fitted to the sheet; the choices are remembered like the
  export's). It does not offer the A4 / actual choice of the PDF as such: the sheet is the printer's, chosen in
  Windows' dialog; "real size" and "fitted" are the two ways a note meets a sheet.
- Verification on this PC (Windows 11): `cargo test -p ardoise print_windows -- --nocapture` prints three notes
  (portrait, landscape, portrait) to the installed "Microsoft Print to PDF" with an output file, no dialog and nothing
  sent to a real printer, and reads the PDF back: three pages, A4 in the orientation of each note. The test skips
  itself with a message when that printer is not installed. The dialog itself needs a person (see the checklist in the
  M8 report).

## Layers at export (M14)

Decisions: `docs/questions.md` Q11, SPEC §4.6 « Calques à l'export », ROADMAP M14. Built by package D of
[tablet-live-classroom.md](tablet-live-classroom.md), "M14" (its work packages, order and file ownership). Written on
2026-11-11 from reading the code, nothing built or run; "(supposé)" marks what was not checked.

**Today.** `vector::Scene::of` groups the fills of the visible strokes by layer, bottom first; hidden layers and empty
outlines give nothing (`visible_strokes`, `drawing_order` in lib.rs). `svg::note_to_svg` writes one Inkscape layer per
group (`<g id="layer{n}" inkscape:groupmode="layer" inkscape:label="{name}">`) above a locked « Background » layer;
`pdf::write_page` paints every fill in one content stream, without optional content. So the SVG is "separate" and the
PDF "merged" already, and a hidden layer is in neither: its strokes are not in the scene and its name is not written.

### Render crate (`crates/ardoise-render`)

- `pub enum LayerMode { Separate, Merged }` (lib.rs, exported). `SvgOptions { background, layers: LayerMode }`, default
  `Separate`; `PdfOptions { page, background, layers: LayerMode }`, default `Merged`. The defaults are today's files.
- **SVG `Separate`**: today's output, byte for byte. **SVG `Merged`**: one Inkscape layer,
  `<g id="ink" inkscape:groupmode="layer" inkscape:label="…">`, holding every fill in drawing order (`Scene::fills`);
  its label is the names of the layers merged, bottom first, joined by « + » (a layer without entry: its number, as
  today's label), so that Inkscape says what was merged without a word of either language in the crate. The background
  layer is unchanged.
- **PDF `Merged`**: today's output, byte for byte (the catalog written first, no optional content).
- **PDF `Separate`**: optional content (PDF 1.5; pdf-writer writes 1.7 by default, `Pdf::set_version`).
  - One OCG per distinct layer name over all the pages of the document (exact string), in order of first appearance,
    the top layer first within a page (Acrobat's and Firefox's panels list them top first, as the layers zone does). Two
    layers of one name on a page share their OCG. Only layers with fills get one: a hidden layer has none and its name
    appears nowhere in the file. Strokes whose layer has no entry, the paper, the template, the texture and the
    background image stay outside optional content: always shown.
  - Content: for each `LayerFills` of the page, `/OC /L{k} BDC`, its fills, `EMC`, where `k` is the OCG's number in the
    document; the page's resources map `/L{k}` to the OCG in `/Properties`. Marked content does not touch the graphics
    state (it is not `q`/`Q`), so the `Painter`'s tracking of colour and ExtGState stays right.
  - Catalog: `/OCProperties << /OCGs [all] /D << /BaseState /ON /Order [all] >> >>` and `/PageMode /UseOC`, which opens
    the layers panel in Acrobat Reader ((supposé) and Firefox's layers view). The OCGs are known only once every page is
    written: in this mode the catalog is written last (pdf-writer takes indirect objects in any order; its reference was
    bumped first and stays).
  - pdf-writer 0.15 has no typed writer for optional content (its sources have no `OCG`, `OCProperties` nor `/OC`
    helper; `types::PageMode::UseOC` exists). The calls:
    - an OCG: `pdf.indirect(id).dict()` (`Pdf` dereferences to `Chunk`, `Chunk::indirect` gives an `Obj`), then
      `.pair(Name(b"Type"), Name(b"OCG"))`, `.pair(Name(b"Name"), TextStr(name))`, `.finish()`;
    - in a page's content: `painter.content.begin_marked_content_with_properties(Name(b"OC")).properties_named(Name(b"L3"))`
      (`MarkContent::properties_named` writes `/OC /L3 BDC` as the operation drops), then
      `painter.content.end_marked_content()` (`EMC`);
    - in the page's resources: `resources.insert(Name(b"Properties")).dict()`, then `.pair(Name(b"L3"), ocg_ref)` per
      OCG of the page (`Resources` dereferences to `Dict`; `Resources::properties()` gives a `TypedDict<PropertyList>`,
      whose `pair` wants a primitive property list: it cannot take a reference);
    - the catalog: `let mut catalog = pdf.catalog(catalog_id); catalog.pages(tree); catalog.page_mode(PageMode::UseOC);`
      then on `catalog.insert(Name(b"OCProperties")).dict()`: `.insert(Name(b"OCGs")).array().items(refs)`, and on its
      `.insert(Name(b"D")).dict()`: `.pair(Name(b"BaseState"), Name(b"ON"))`, `.insert(Name(b"Order")).array().items(refs)`
      (each writer finished before its parent).
  - A reader that ignores optional content draws everything (the dialog says so).

### App (`app/src-tauri/src/export.rs`)

- `ExportOptions.layers: Option<ExportLayers>` (`#[serde(default)]`; `ExportLayers { Separate, Merged }`, camelCase),
  TS `ExportOptions.layers?: 'separate' | 'merged'` (`lib/note.ts`). `None` is the format's default (SVG separate, PDF
  merged). Read for SVG (`image_bytes`) and PDF (`pdf_options`), ignored by the images.
- Unchanged, Q11 and SPEC §4.6 asking nothing of them: the automatic export (`autoexport::export_bytes`: its PDF says
  `LayerMode::Merged`), direct printing and the copy (rasters, `render_note`), dragging a note out (a PNG), and the
  assistant's `export_note` (`mcp/write.rs`: `image_options` gives `layers: None`, its own `PdfOptions` says
  `LayerMode::Merged`: the files are today's). Struct literals to complete: `export.rs` (and its tests), `autoexport.rs`,
  `mcp/write.rs`, `drag.rs`, `background.rs` (tests).

### Interface

- `lib/export-options.ts`: `ExportChoice` gets `layers: { svg: 'separate' | 'merged'; pdf: 'separate' | 'merged' }`,
  default `{ svg: 'separate', pdf: 'merged' }`, stored under `ardoise.export` with the rest; an older stored choice
  without it, or another word, reads the default. One remembered choice per format: the defaults differ, and one shared
  value would turn the PDF's merged default into the SVG's separate one. Saved after an export that succeeded, as now
  (`ExportDialog.run` → `saveChoice`).
- `ExportDialog.svelte`: for SVG and PDF a fieldset `[data-slot="export-layers"]` (legend `export.layers.legend`;
  `input[type=radio][name="export-layers"][value="separate"|"merged"]` with their hints,
  `export.layers.separateHint.svg|pdf` and `export.layers.mergedHint`); for PNG, JPG and WebP the sentence
  `p[data-slot="export-layers-flat"]` (`export.layers.flat`); under either, `p[data-slot="export-layers-hidden"]`
  (`export.layers.hiddenNever`). The request carries `layers: choice.layers[format]` for SVG and PDF, nothing for images.
- Shown only when it matters: the fieldset and the image sentence when at least one exported note has two or more
  visible layers holding ink, the hidden sentence when one has a hidden layer holding ink. The dialog reads the notes
  already, for their page sizes (`onMount`: `source.note`, or `api.openNote`): the same read gives their layers; until
  it answers, nothing is shown. Most notes have one layer: for them the dialog does not change. (The mockup showed the
  block on a note of two layers; this is our reading of it, easy to turn into "always".)
- `lib/mock-export.ts` accepts `layers` and refuses another value, as Rust's serde would.

### Translations (added by package 0)

| Key | Français | English |
|---|---|---|
| `export.layers.legend` | Calques | Layers |
| `export.layers.separate` | Séparés | Separate |
| `export.layers.merged` | Fusionnés | Merged |
| `export.layers.separateHint.svg` | Un calque Inkscape par calque de la note, à afficher ou masquer dans Inkscape. | One Inkscape layer per layer of the note, to show or hide in Inkscape. |
| `export.layers.separateHint.pdf` | Des calques PDF, à afficher ou masquer dans Acrobat Reader ou Firefox. Certains lecteurs les montrent tous, sans choix. | PDF layers, to show or hide in Acrobat Reader or Firefox. Some readers show them all, with no choice. |
| `export.layers.mergedHint` | Un seul dessin, comme à l'écran. | One drawing, as on screen. |
| `export.layers.flat` | Une image n'a pas de calques : ils sont toujours fusionnés. | An image has no layers: they are always merged. |
| `export.layers.hiddenNever` | Les calques masqués dans Ardoise ne sont jamais exportés : un calque masqué peut contenir les réponses. | Layers hidden in Ardoise are never exported: a hidden layer may hold the answers. |

### Tests (package D)

- Render crate (`tests/export_formats.rs`, the unit tests of `svg.rs` and `pdf.rs`; `lopdf` is a dev dependency
  already): the existing tests pass unchanged (SVG separate and PDF merged are today's files; to be sure of the bytes,
  a digest of each for the reference note taken before the change and kept in a test); SVG merged has one ink layer
  holding every fill and no hidden layer's name; PDF merged has no `/OCProperties`, `/OC` nor `BDC`; PDF separate, over a document of two notes that share « Calque 1 »
  (names 2 + 1, one shared: 3 OCGs, not 4): `/OCProperties` with them all in `/OCGs` and `/D /Order`, each page's
  `/Properties` naming its own, `BDC`/`EMC` balanced in the inflated content with one sequence per layer of the page,
  the fill count unchanged, the hidden layer's name nowhere in the bytes nor in the inflated streams; a page whose layers
  are all hidden has no OCG.
- App (`export.rs` tests): options without `layers` read `None`; SVG with `"merged"` writes one ink layer; a PDF of two
  notes with `"separate"` has one OCG per name; the assistant's `export_note` to PDF has no `/OCProperties`.
- Interface: `export-options` (defaults, per format, an old stored choice, garbage); `ExportDialog` (the fieldset for SVG
  and PDF with a note of two visible layers, none with one layer, the defaults, the remembered choice per format, the
  image sentence, the hidden sentence, `layers` in the request, nothing for an image).
- e2e `journeys-m14-export.mjs` « export : calques séparés ou fusionnés, choix retenu par format »: open the export
  dialog on a note of two layers (made in the editor first), SVG shows « Séparés » chosen, PDF « Fusionnés »; choose
  « Séparés », export the PDF to a temporary file, which holds `/OCProperties` and two OCGs; open the dialog again: PDF
  still says « Séparés », SVG still « Séparés »; PNG shows the image sentence.

### As built (package D, 2026-11-11)

Built as designed; the points where the code says more or something else than the text above:

- **Byte-identical files.** `tests/export_formats.rs` keeps the length and an FNV-1a 64 of the reference note's SVG
  (separate) and of its two-page PDF (merged, actual size and A4), taken on the code before the change; the PDF
  is built with `LayerMode::Merged` spelled out and the defaults are asserted. The catalog is written first in merged mode and last in
  separate mode (pdf-writer takes the objects in any order; its reference is the one bumped first).
- **No group at all, no optional content.** A separate PDF whose pages have no group (every layer hidden, or only strokes
  of layers without an entry) is written like a merged one: no `/OCProperties`, no `/PageMode /UseOC`, no
  `/Properties`. A group exists only for a layer with fills.
- **Blank layer names.** A layer named with blanks only gets its number as its group's name (`"3"` for the third layer),
  as an Inkscape label already does for a layer without entry. The SVG separate output is untouched (it keeps the blank label it always had).
- **SVG merged with nothing to draw** writes no ink layer (as separate writes none): the only Inkscape layer is the
  background's. The label joins the names of the layers that hold fills, bottom first, with « + » (a name twice stays
  twice).
- **Struct literals outside the list.** `SvgOptions` and `PdfOptions` gained a field, so the literal
  `SvgOptions { background: false }` of `crates/ardoise-render/tests/background_image.rs` no longer compiled: it got
  `..SvgOptions::default()`. One line in a file package D does not own.
- **App.** `ExportLayers` (`Separate`, `Merged`) is in `export.rs` with `From<ExportLayers> for LayerMode`; `None` reads
  `Separate` for SVG and `Merged` for PDF. `autoexport.rs`, `mcp/write.rs` (its own `PdfOptions` and `image_options`) and
  the struct literals of `drag.rs`, `background.rs` and the `export.rs` tests say `None` / `LayerMode::Merged`; the
  assistant's PDF test checks there is no `/OCProperties`.
- **Dialog.** "A layer holding ink" is a stroke with at least one point; a stroke whose layer has no entry counts as a
  visible layer (the renderer draws it). The notes read for the page sizes (`notes`, `pages` derived from them) give
  the layers too. The request carries `layers` for SVG and PDF whether or not the block is shown (a remembered
  « Séparés » therefore also applies to a one-layer note exported to PDF: one group, harmless); the sentence about hidden
  layers is shown for any format when a hidden layer holds ink, and does not depend on the block. The choices are
  disabled while an export runs, like the other options. Styles are scoped in the component (`app.css` is not package
  D's).
- **Journey.** `journeys-m14-export.mjs` writes its two-layer sheet (plus a hidden layer named `Hidden answers`, ASCII so
  that a search of the bytes means something) into the library with `dropSheet` of `journeys-m12-resume.mjs`, instead of
  drawing it in the editor: the editor's layers zone is package A's and was not there to select. It also exports an SVG
  « Fusionnés » and checks the per-format memory both ways.
