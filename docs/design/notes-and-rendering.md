# Notes, library, rendering and Inkspace import (milestone M2)

Developer document (English). Requirements: SPEC §3.2–§3.4, §4.3, §4.4, §6.1–§6.3, §7, §9. Inkspace facts:
[inkspace-format.md](../research/inkspace-format.md) (read it entirely before touching the importer).

```
ardoise-ink       model.rs (fixed): Point, Stroke, Tool, Rgba  +  geometry: width per tool, outlines
     ^
ardoise-core      note.rs (fixed): Note, PageSize, Background, Paper, Template, Layer, Timing, Origin
     ^            format: the .ardoise ZIP + strokes.bin;  library: folders, tags, trash, watch, conflicts
ardoise-render    render_note(&Note, &RenderOptions) -> Image, encode_png  (signature fixed, tiny-skia)
     ^
ardoise-inkspace  locate, read (tracked file access), decode, convert to Note, compare with previews
     ^
app/src-tauri     ardoise.exe --import-inkspace [LocalState] [--library DIR] [--compare]
```

## Shared rules

- Units: page coordinates in device units (10 µm), `f32`, origin top-left, y down. A4 landscape page = 29700 × 21600.
  1 Inkspace page unit = 25 device units. Times: `Point::t` = ms since `Note::created` (unix ms).
- No `unwrap`/`expect`/panic on file or user data; typed errors (`thiserror`).
- Every write into the library goes through `ardoise_core::fsutil::write_atomic` (SPEC §3.4).
- Privacy: `testdata/private/` (git-ignored) holds the user's Inkspace copy; nothing from it is ever committed —
  no rendered PNG, no fixture, no excerpt. Publishable test data is synthetic and lives in `testdata/` elsewhere.

## ardoise-ink: geometry

- `Tool::width_at(stroke_width, pressure) -> f32`: diameter of the line at a point, monotone in pressure, and its
  exact inverse `Tool::pressure_for_width(stroke_width, diameter) -> f32` (the Inkspace import stores widths as
  pressures, so the round trip must reproduce the widths).
- `outline(&Stroke) -> Outline`: closed contour(s) in page coordinates, variable width along the stroke, centreline
  smoothed through the samples (Catmull-Rom or equivalent; the samples themselves are kept intact), round caps by
  default, tool-specific shapes (fountain pen: width depends on direction; highlighter: flat ends; brush: tapered).
  Robust to 0, 1 or 2 points, duplicated points, sharp turns, NaN-free.
- Pure Rust, no platform or I/O dependencies, so that the same code serves the exports (native) and the screen
  (through a Tauri command now; possibly WebAssembly later — the `wasm32-unknown-unknown` target is not installed
  on this PC and installing it needs the user's consent).

## ardoise-render

- `render_note(&Note, &RenderOptions) -> Result<Image, RenderError>` (signature fixed) with tiny-skia: background
  (paper colour, template in millimetres: lined 8 mm, Seyès, grid 5 mm, millimetric, dotted 5 mm; optional
  deterministic paper texture), then visible layers in order, strokes filled from `ardoise_ink::outline`, each tool
  with its look (pencil grain, felt-tip, brush, translucent highlighter, chalk), anti-aliased. `encode_png(&Image)`.
- Deterministic output (no randomness without a fixed seed) so that reference images are stable.
- Reference images: synthetic notes covering every tool, colour, paper and template, rendered to `testdata/golden/`;
  a test compares fresh renders with a tolerance written in the test; `cargo xtask golden` regenerates them.

## ardoise-core: the `.ardoise` file and the library

- ZIP with `note.json` (format version, the `Note` metadata without strokes, plus unknown fields kept for forward
  compatibility), `strokes.bin` (compact binary: varint/zigzag deltas, documented byte by byte in
  `docs/file-format.md`), optional `original.bin` (tablet bytes, untouched), `thumb.png`, `audio.opus`,
  `background.*`, `text.json` (optional members are preserved when a note is rewritten).
- Versioned: a reader accepts every older version (committed synthetic fixtures per version in `testdata/format/`),
  refuses a newer major version with a clear error, and keeps unknown members/fields of newer minor versions.
- Library (SPEC §3.2, §3.4): notes are `YYYY-MM-DD Title.ardoise` files in a folder tree (sub-folders = binders);
  list/read, create (unique file names), save with a modification-time check (a note changed on another PC is not
  overwritten silently), rename (renames the file), move, tags, trash in `.ardoise/trash/` with restore and purge after
  30 days, folder watching (debounced events), detection of Nextcloud conflict copies
  (`… (conflicted copy YYYY-MM-DD HHMMSS).ardoise` and the localised variants), temporary `.part` files ignored.

## ardoise-inkspace

- Locate `%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_*\LocalState`. Every file read goes through one tracked
  reader (it records each path), so a test can prove that nothing under `LocalState\db\` is read except
  `db\notes\<id>\preview_*.png` (used only for the comparison). Never open the `db\` LevelDB: its `profile` key holds
  an access token and an e-mail address.
- Read `WacomCloudDb\_` into memory (rusty-leveldb on an in-memory environment), so nothing is ever written next to
  the data and the original folder is never modified (SPEC §6.3).
- Decode per research §8; convert to `Note`: page coordinates ×25 with the page transform applied (landscape =
  29700 × 21600), Catmull-Rom duplicated end points dropped, ballpoint tool, colour, per-point width turned into
  pressure with `Tool::pressure_for_width` (a width factor may be tuned to match Inkspace's slightly thinner brush),
  timing from `pointsPerSecond`/`creationDate` or synthesised (`Timing::Synthesized`), `Origin::Inkspace`.
- Comparison with `preview_<lastModifiedDate>.png`: render at 0.04 px/unit (1 px = 1 Inkspace unit), ink mask =
  luminance < 200, scores: IoU and "ink within 1 px" in both directions. Thresholds fixed in the test (research
  suggests within-1-px ≥ 0.99 and IoU ≥ 0.80).
- `ardoise.exe --import-inkspace [LocalState] [--library DIR] [--compare]`: imports every note into the library
  (binder `Inkspace`, original dates, Inkspace id kept to avoid duplicates on a second import) and prints a table and
  `N/N notes importées`.
