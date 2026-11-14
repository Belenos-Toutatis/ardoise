# The `.ardoise` file format

> Developer document, written in English (the planning documents are in French). It describes format
> **version 1.6** byte by byte (1.1 = 1.0 plus the optional `favorite` field; 1.2 = 1.1 plus the optional
> `backgroundImage` field and `original.ardoise` member; 1.3 = 1.2 plus the optional `revision.json` member; 1.4 = 1.3
> plus the optional `objects` field, the page objects such as text frames, §2.2; 1.5 = 1.4 plus page objects of kind
> `equation`, §2.3; 1.6 = 1.5 plus page objects of kind `bookmark`, §2.2). Requirements: SPEC §3.3 and §3.4. Code:
> `crates/ardoise-core/src/format.rs` (container, `note.json`), `crates/ardoise-core/src/objects.rs` (page objects) and
> `crates/ardoise-core/src/format/strokes.rs` (`strokes.bin`). Model: `note.rs` (`ardoise-core`) and `model.rs`
> (`ardoise-ink`). Committed sample files: `testdata/format/v1/`.

One note = one file `YYYY-MM-DD Title.ardoise`. The file is an ordinary ZIP archive, so it can be inspected with any
unzip tool:

| Member | Required | Content | Compression |
|---|---|---|---|
| `note.json` | yes | format version and note metadata (§2) | deflate |
| `strokes.bin` | yes | every stroke, compact binary (§3) | deflate |
| `original.bin` | no | the bytes received from the tablet, untouched ("back to the original") | deflate |
| `original.ardoise` | no (1.2) | the note before its first edit in Ardoise, for notes without `original.bin` (§4) | stored |
| `thumb.png` | no | thumbnail, PNG | stored |
| `audio.opus` | no | narration, Ogg Opus (§4.1) | stored |
| `background.<ext>` | no | background image (pixels of `backgroundImage`); `<ext>` = 1–10 lowercase ASCII letters or digits; Ardoise writes `png` or `jpg` | by extension (see below) |
| `revision.json` | no (1.3) | the note is a **revision**, a retouched copy of another note made by an assistant: whose copy, and the account of what changed (§4.2) | deflate |
| `text.json` | no | reserved for recognised handwriting (not written: the recognition keeps a cache on each PC; opaque to the format) | deflate |
| anything else | no | members of newer versions: kept byte for byte when the note is rewritten | by extension (see below) |

The optional members are opaque bytes for the format code: it reads and writes them without looking inside.
Members whose name ends in `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.opus`, `.ogg`, `.mp3`, `.mp4`, `.zip` or
`.ardoise` (any case) are already compressed and are stored; all others are deflated.

## 1. ZIP container

- Plain ZIP (PKWARE APPNOTE): local file headers, central directory, end of central directory record. No encryption,
  no archive comment, no spanning. Writers use only **stored** (method 0) and **deflate** (method 8); readers refuse
  other methods (the member cannot be decoded: "damaged file").
- Member order written: `note.json`, `strokes.bin`, then `original.bin`, `original.ardoise`, `thumb.png`, `audio.opus`, `text.json`,
  `background.<ext>`, `revision.json`, then unknown members in the order they were read. **Readers must not depend on the order**;
  they find members by name through the central directory. Names are case-sensitive, `/`-separated, UTF-8 (the
  "language encoding" flag, bit 11, is set when a name is not ASCII).
- Every member's DOS date and time is the note's `modified` time in UTC (1980-01-01 00:00 when out of the DOS range),
  so that writing the same note twice gives the same bytes.
- Sizes and CRC-32 are in the local headers (no data descriptor). ZIP64 is only used for a member of 4 GiB or more,
  which the limits below exclude.
- A name ending in `/` with no data is a directory entry. None is written by version 1; unknown ones are kept.
- Names are unique. A file in which a name appears twice in the central directory (damaged or hostile file; also
  two spellings that decode to the same name) is refused as damaged (code `noteDamaged`), when listing as well as
  when opening, and is never rewritten.

Limits enforced by readers on the *decompressed* size, whatever the archive claims (protection against ZIP bombs):

| What | Limit |
|---|---|
| `note.json` | 16 MiB |
| `strokes.bin` | 128 MiB (an hour of continuous live writing is about 6 MB) |
| any other member | 1 GiB |
| all members together | 1.5 GiB |
| number of members | 4096 (counted in the central directory, repeated names included) |

A member that crosses a limit, its own or what is left of the total, makes the file "damaged" (`MemberTooLarge`).

## 2. `note.json`

A UTF-8 JSON object. Versions 1.2 to 1.4 write it indented with two spaces, `format` first and the other keys in
alphabetical order; **key order and whitespace are not significant**.

```json
{
  "format": { "version": 1, "minor": 2, "writer": "Ardoise 0.1.0" },
  "background": { "paper": "cream", "template": "seyes", "texture": true },
  "created": 1792843200000,
  "id": "6f0c9a52-3d7e-4f0a-9a43-1c2b5e8d7f10",
  "layers": [ { "name": "Calque 1", "visible": true } ],
  "modified": 1792846800000,
  "origin": { "kind": "tablet" },
  "page": { "width": 29700, "height": 21600 },
  "tags": [ "maths", "3e B" ],
  "timing": "recorded",
  "title": "Théorème de Pythagore"
}
```

| Key | Type | Meaning |
|---|---|---|
| `format.version` | integer ≥ 1 | **major** format version. The one field every future version keeps, with this meaning. |
| `format.minor` | integer ≥ 0 | minor format version (absent = 0). |
| `format.writer` | string, optional | application that wrote the file, for diagnostics. |
| `id` | string | UUID, stable for the life of the note (survives renames and moves). |
| `title` | string | title shown in the app; the file name is derived from it (§6). |
| `created` | integer | Unix time in milliseconds; also the time origin of every point's `t`. |
| `modified` | integer | Unix time in milliseconds of the last change. |
| `page` | `{"width": u32, "height": u32}` | page size in device units (10 µm), both ≥ 1. Slate Large: 29700 × 21600 (landscape). |
| `background.paper` | `"white"`, `"cream"`, `"kraft"`, `"slate"` or `{"custom": {"r": u8, "g": u8, "b": u8, "a": u8}}` | paper colour. |
| `background.template` | `"blank"`, `"lined"`, `"seyes"`, `"grid5mm"`, `"millimetric"`, `"dotted"`, `"music"` | printed guide lines. |
| `background.texture` | boolean | subtle paper grain. |
| `backgroundImage` | object, optional (1.2) | imported image or PDF page drawn over the paper and template, under the strokes; absent when there is none (see below). |
| `tags` | array of strings | tags, in the user's order. |
| `favorite` | boolean, optional (1.1) | `true` when the note is a favourite; absent otherwise. Absent in 1.0 files: not a favourite. |
| `layers` | array of `{"name": string, "visible": boolean}` | at least one; strokes refer to layers by index (0 = first). |
| `objects` | array, optional (1.4) | what the page holds besides its strokes: text frames, bookmarks (1.6) (§2.2), equations (1.5, §2.3); absent when there is none. |
| `timing` | `"recorded"` or `"synthesized"` | whether point times were measured or made up (imported notes without timing). |
| `origin` | `{"kind": "tablet"}`, `{"kind": "live"}`, `{"kind": "ardoise"}` or `{"kind": "inkspace", "id": string}` | where the note comes from (`id` = Inkspace's note UUID). |
| `boardPosition` | `"buttonsBottom"`, `"buttonsTop"`, `"buttonsRight"` or `"buttonsLeft"`, optional | for a note made from a page of the Xiaomi blackboard (its `original.bin`): how the blackboard was held, i.e. how the page's points were turned to make the note (docs/design/blackboard.md, « Positions, the Rust side »). Absent: `buttonsBottom` (notes made before 2026-11). Not a key of the format's model: `ardoise-device` writes and reads it, and every reader, of any 1.x version, keeps it verbatim as an unknown key; it describes `original.bin`, which no edit changes, so it stays true after an older version edited the note. |

### 2.1 `backgroundImage` (1.2)

| Key | Type | Meaning |
|---|---|---|
| `name` | string | name of the imported file, for display (`fiche.pdf`). |
| `pdfPage` | integer ≥ 1, optional | page of the PDF the image was made from; absent for an image file. |
| `rect` | `{"x": f32, "y": f32, "width": f32, "height": f32}` | where the image is stretched to, page coordinates in device units; may reach outside the page (not drawn there). |
| `quarterTurns` | integer, optional (default 0) | clockwise quarter turns applied to the stored pixels before stretching (taken modulo 4). |
| `opacity` | number, optional (default 1) | 0 = invisible, 1 = as imported; clamped to 0–1 when drawing. |

The pixels are the `background.png` or `background.jpg` member; when it is missing (or is not an image Ardoise can
decode), nothing is drawn. Page operations move the image with the strokes: a quarter turn turns `rect` and adds one
to `quarterTurns`, cropping shifts `rect`. (A 1.1 reader keeps the key verbatim but does not move it: a note with a
background image rotated or cropped by Ardoise 1.1 has its image misplaced.)

Strokes are not in `note.json` (a `strokes` key, if present, is ignored). Any other top-level key is **unknown**: a
reader keeps it with its value and writes it back unchanged when it saves the note (a key it knows always takes the
value of its own model). Unknown keys *inside* known objects are not kept (see §5), except inside page objects (§2.2).

A reader refuses the file ("invalid note") if there is no layer, the page size is zero, or a stroke refers to a layer
that does not exist. Writers refuse to write such a note.

### 2.2 Page objects (`objects`, 1.4)

What a page holds besides its strokes: **text frames** (SPEC §4.4, « Texte tapé au clavier »), **equations** (1.5, §2.3),
**bookmarks** (1.6, SPEC §4.4, « Signets »), and other kinds later (`docs/design/page-objects.md`, which also says how to
add a kind). Each element of the array is one flat JSON object: the **placement**, the same for every kind, and the keys
of its **kind**.

```json
"objects": [
  {
    "kind": "text", "id": "5c1e…", "layer": 0, "t": 61250,
    "x": 2500.0, "y": 16000.0, "scale": 1.0, "quarterTurns": 0, "width": 7350.9, "height": 750.0,
    "text": "Cadeaux\nDes chocolats", "font": "andika", "bold": false, "size": 600.0,
    "color": { "r": 26, "g": 58, "b": 138, "a": 255 }
  }
]
```

Placement (every kind):

| Key | Type | Meaning |
|---|---|---|
| `kind` | string, not empty | the kind: `"text"` in 1.4, `"equation"` in 1.5, `"bookmark"` in 1.6. |
| `id` | string | unique within the note, kept by every operation that keeps the object (a reader gives a new one to an object without id, or with the id of an earlier one: `<id>-2`, `object-2`…). |
| `layer` | integer ≥ 0 | index into `layers`: the object is drawn with its layer, **above the strokes of that layer**, objects in array order. A reader puts an object on a missing layer on the last one (a reader before 1.4 may have merged layers without knowing objects). |
| `t` | integer ≥ 0 | milliseconds since `created` (like a point's `t`): when the replay shows the object, all at once. |
| `x`, `y` | number | where the content's origin (the top-left corner of its content before turning) is on the page, device units. |
| `scale` | number > 0, optional (default 1) | page units per unit of the content. |
| `quarterTurns` | integer, optional (default 0) | clockwise quarter turns of the content about its origin (taken modulo 4). |
| `width`, `height` | number ≥ 0, optional (default 0) | size of the content, in its own units (before scale and turns). |

The content's point `(u, v)` (its units, y down) is on the page at `(x, y) + turn(scale × (u, v))`, where one clockwise
quarter turn maps `(u, v)` to `(−v, u)`; the content fills `[0, width] × [0, height]`. **Page operations move objects through
the placement alone**, whatever their kind: a move adds to `x`, `y`; a uniform change of scale about a point maps `x`, `y` and
multiplies `scale`; a quarter turn of the page maps `x`, `y` like a point and adds 1 (clockwise) or 3 (counter-clockwise) to
`quarterTurns`; a crop shifts `x`, `y` and drops a visible object whose box is wholly outside; a split by zone moves an object
of a visible layer whose box centre is inside the zone; a split at an instant moves the objects with `t` at or after it; a
merge maps layers, positions and times as for strokes.

Text frame (`"kind": "text"`):

| Key | Type | Meaning |
|---|---|---|
| `text` | string | the text; only `\n` (or `\r\n`, or a lone `\r`) starts a new line: nothing wraps. |
| `font` | `"latinModern"` or `"andika"` | the family: Latin Modern Roman 10 or Andika, shipped with Ardoise (`crates/ardoise-text/fonts/`). |
| `bold` | boolean | the bold face of the family. |
| `size` | number > 0 | size of the font (its em), in the content's units: device units at scale 1 (the tool offers 400, 600 and 900: 4, 6 and 9 mm). |
| `color` | `{"r", "g", "b", "a"}` (u8 each) | colour, straight alpha. |

Bookmark (`"kind": "bookmark"`, 1.6): a word or a short phrase at a spot of the page, one line, with a flag. It is **the editor's
only**: image, SVG, video and print outputs, the replay, the projection and the share on the network do not show it. The library's
listing gives its label and its spot (for the summary of a binder and the search), and the PDF of a binder or of one note makes
its outline of the bookmarks.

| Key | Type | Meaning |
|---|---|---|
| `label` | string | the text: one line, at most 60 characters when the editor writes it (a longer or multi-line label read from a file is kept and shown as it is, the line breaks being spaces). An empty or blank label says nothing: the editor removes such a bookmark, and no listing shows it. |

Its content is the flag, the label (Andika regular, `ardoise_core::objects::BOOKMARK_FONT_SIZE` = 500 units, 5 mm, laid out as a
text frame's line) and room around them (`BOOKMARK_PAD` = 150 units on each side, the flag `BOOKMARK_FLAG_WIDTH` = 300 units
wide and one pad before the label): `width` = 3 × pad + flag + the label's advance width, `height` = 2 × pad + the line height
(1.25 × the font size). It is placed like any object (`x`, `y` the top-left corner of the content, `scale`, `quarterTurns`),
so a turned page turns it. Writers write that box, readers measure it again. A bookmark has a `layer` like every object (the
layer of the page when it was put), but **no layer hides it**: it is drawn, selectable and listed whatever the layer shows. Its
`t` is when it was put (a split at an instant cuts by it) and nothing else: no replay waits for it, and the clock of a writing
recording ignores it. A bookmark whose `label` is not a string is kept as an object of a kind this version does not know.

The layout of a frame is `ardoise-text`'s (`crates/ardoise-text/src/layout.rs`): glyphs placed by their advance widths, no
kerning nor ligature, a character missing from the family taken from the other one, a tab to the next multiple of four spaces,
lines `1.25 × size` apart. Writers write `width` and `height` as this layout measures them (the widest line, the lines'
height), and a reader measures them again: they are there for readers that do not know the kind.

Rules for readers and writers:

- A list that cannot be read (not an array, an element that is not an object, a missing `kind`, a placement key of the wrong
  type, a `scale` that is not positive) makes the note read **without objects**, and the whole `objects` value is kept as an
  unknown key (§5), written back as it was until the note is given objects of its own.
- An object of a kind this version does not know, or of a known kind with a value it does not know or a wrong type (a font of a
  later version), is kept with all its keys, moved with the page by its placement, and drawn nowhere. So a new kind, or a new
  value of a kind, is a **minor** version: a reader of 1.4 keeps it and does not lose it when it rotates or crops the page.
- Keys of an object this version does not know are kept with the object and written back.
- Writers refuse (the note is not written, "invalid note") an object on a layer that does not exist, a placement that is not a
  finite number, a `scale` or a text `size` that is not positive.
- `objects` is written only when there is at least one object, numbers with the fewest digits that read back the same.

A reader of 1.3 or older keeps `objects` verbatim (an unknown top-level key) but neither draws nor moves the objects: a note
whose page such a version turned or cropped has its objects misplaced, and one whose layers it merged may have them on the
wrong layer (a 1.4 reader moves an object of a missing layer to the last one).

### 2.3 Equations (`"kind": "equation"`, 1.5)

An equation (SPEC §4.4 « Équation », `docs/design/equations.md`): the LaTeX source the user typed and the drawing MathJax
made of it in the editor, kept as filled outlines, so that every reader draws it without MathJax. Its content's units are
**1/1000 em** of its type (MathJax's), y down, origin at the top-left corner of its drawing; the placement's `scale` is the em
on the page divided by 1000 (the tool's sizes 4, 6 and 9 mm give 0.4, 0.6 and 0.9).

```json
{
  "kind": "equation", "id": "9b2f…", "layer": 0, "t": 1200,
  "x": 1500.0, "y": 2500.0, "scale": 0.6, "quarterTurns": 0, "width": 7811.3, "height": 862.4,
  "latex": "\\ce{2H2 + O2 -> 2H2O}",
  "path": "M218.4 641.2C…ZM…Z",
  "color": { "r": 26, "g": 58, "b": 138, "a": 255 }
}
```

| Key | Type | Meaning |
|---|---|---|
| `latex` | string, at most 4000 characters (Unicode scalar values) | the source, in TeX's math mode; kept to be edited again, never read to draw. |
| `path` | string, at most 262 144 bytes | the drawing: SVG path data (below), filled with the non-zero rule. |
| `color` | `{"r", "g", "b", "a"}` (u8 each) | colour of the whole drawing, straight alpha. |

The path data's grammar is a strict subset of SVG's, ASCII only: commands `M`, `L`, `Q`, `C` (absolute, upper case) and `Z`;
`M` first, and only `M` after a `Z`; a command may be followed by further groups of numbers (an `M` by line ends, as in SVG);
numbers `[-+]?(digits(.digits?)?|.digits)` without exponent, separated by white space or commas, each coordinate within
±1 000 000; nothing else. Writers write one `M` … `Z` contour after another, numbers with at most one decimal (`M218.4
641.2C…Z`), the drawing moved so that the smallest x and y of its points (control points included) are 0.

`width` and `height` are measured from the drawing: from the origin to the largest x and y of its points, control points
included. A reader measures them again; they are there for the readers that do not know the kind.

An equation this version cannot read — a `latex` or `path` that is missing, not a string or beyond its limit, path data
outside the grammar, a `color` missing or of a wrong type — is kept with all its keys and drawn nowhere, like an unknown kind (§2.2). So a
hostile `note.json` costs at most one linear pass over a quarter of a megabyte per equation; nothing in it ever reaches a
renderer. Writers refuse (the note is not written) an equation they could not read back.

A reader of 1.4 keeps equations as objects of a kind it does not know: moved with the page, drawn nowhere.

## 3. `strokes.bin`

### 3.1 Primitive types

- `u8`: one byte.
- `f32le`: IEEE 754 single precision, little-endian, 4 bytes.
- `varint`: unsigned LEB128: 7 bits per byte, least significant group first, high bit set on every byte but the
  last. At most 10 bytes; the 10th byte may only be `0x00` or `0x01`. Non-minimal encodings are accepted on read and
  never written.
- `svarint`: a signed 64-bit integer `n` zigzag-encoded then written as a `varint`:
  `z = (n << 1) ^ (n >> 63)` (0 → 0, −1 → 1, 1 → 2, −2 → 3…); decoding: `n = (z >> 1) ^ −(z & 1)`.

### 3.2 Grammar

```text
strokes.bin  = magic version stroke_count stroke*
magic        = "ASTK"                      ; 0x41 0x53 0x54 0x4B
version      = u8                          ; 1
stroke_count = varint
stroke       = tool color width layer point_count point*
tool         = u8                          ; see table
color        = u8 u8 u8 u8                 ; r, g, b, a (sRGB, straight alpha)
width        = f32le                       ; nominal diameter at full pressure, device units, finite, >= 0
layer        = varint                      ; index into note.json "layers", < 2^32
point_count  = varint
point        = dx dy dpressure dt          ; four svarint
```

Nothing may follow the last stroke.

| `tool` | Tool |
|---|---|
| 0 | ballpoint |
| 1 | fountain pen |
| 2 | felt-tip |
| 3 | pencil |
| 4 | brush |
| 5 | highlighter |
| 6 | chalk |

Codes are never reused; an unknown code makes the file unreadable for that version (a new tool is a new major
version, §5).

### 3.3 Points

Each point is stored in integer units:

| Value | Stored integer | Range |
|---|---|---|
| x, y | `X = round(x × 100)`: 1/100 device unit (0.1 µm) | `|X| ≤ 10^11` (`|x| ≤ 10^9` device units = 10 km) |
| pressure | `P = round(clamp(p, 0, 1) × 65535)` | 0 ≤ P ≤ 65535 |
| t | `T = t`, milliseconds since `created`, exact | 0 ≤ T ≤ 2^32 − 1 |

`round` rounds half away from zero, computed in 64-bit floating point. Each point stores the difference from the
previous point **of the whole stream**, crossing stroke boundaries; the first point of the file is relative to
`(0, 0, 0, 0)`:

```text
dx = X[i] - X[i-1]    dy = Y[i] - Y[i-1]    dpressure = P[i] - P[i-1]    dt = T[i] - T[i-1]
```

Times may decrease (between strokes, or in edited notes), hence signed deltas for every field. Decoding:
`x = f32(X / 100)`, `pressure = f32(P / 65535)` (divisions in 64-bit floating point, then rounded to `f32`),
`t = T`. A reader refuses the file if a running value leaves its range.

### 3.4 Precision guarantees

- **x, y**: `|decoded − original| ≤ 0.005 + ½ ulp(original)` device units, where `ulp` is the spacing of `f32`
  values at that magnitude (on an A4 page, < 0.006 units = 0.06 µm). Values that are whole or have two decimals and
  fit `f32` exactly (0, 29700, 12.25…) come back unchanged.
- **pressure**: `|decoded − clamp(original, 0, 1)| ≤ 1/131070 + f32 rounding` (< 8·10⁻⁶). The tablet's 2048 levels
  (`level / 2047`) are recovered exactly by `round(pressure × 2047)`.
- **t, tool, colour, layer**: exact. **width**: exact (the `f32` bits are copied).
- **Stable**: a decoded value encodes to the same integer, so loading and saving a note any number of times never
  moves its points (tested over thousands of random values).
- Writers refuse (the note is not written) a coordinate or pressure that is not finite, a coordinate beyond
  ±10^9, or a width that is negative or not finite. Pressures outside 0..1 are clamped.

### 3.5 Example

One pencil stroke, colour `#123456`, width 40, layer 1, two points (100, 200.5, pressure 0.5, t 0) and
(101.25, 200, pressure 1, t 5):

```text
41 53 54 4B 01        magic "ASTK", version 1
01                    1 stroke
03                    tool 3 (pencil)
12 34 56 FF           colour r g b a
00 00 20 42           width 40.0 (f32 LE)
01                    layer 1
02                    2 points
A0 9C 01              dx = +10000          (x = 100.00)
A4 B9 02              dy = +20050          (y = 200.50)
80 80 04              dpressure = +32768   (0.5 × 65535 = 32767.5, rounded to 32768)
00                    dt = 0
FA 01                 dx = +125            (x = 101.25)
63                    dy = −50             (y = 200.00)
FE FF 03              dpressure = +32767   (P = 65535)
0A                    dt = +5
```

An empty note's `strokes.bin` is `41 53 54 4B 01 00`.

### 3.6 Reading hostile data

A reader never trusts counts: a stroke takes at least 11 bytes and a point at least 4, so `stroke_count` and
`point_count` are refused when larger than the remaining bytes could hold, before anything is allocated. Every
addition is overflow-checked. Every malformed input ends in an error, never a crash (tested with truncations at every
length and thousands of random corruptions).

## 4. Optional members

- `original.bin`: the tablet's raw data for this page, as received, never modified after import.
- `original.ardoise` (1.2): for a note without `original.bin` (live mode, Inkspace import, made in Ardoise), the note as
  it was before its first edit of the ink in Ardoise: a complete `.ardoise` archive (this format, current version)
  holding only `note.json` and `strokes.bin`. Written once, when a save first changes the ink (page, layers or
  strokes) of a note whose version on disk has ink, and never changed afterwards; "back to the original" restores its
  page, layers and strokes. Its `note.json` is the whole note of that version, so it may describe a
  `backgroundImage` whose member (`background.*`) is not inside it: readers never draw that image from the inner
  archive; they take only its placement (`rect`, `quarterTurns`), for the image of the outer file when it is the
  same image (same `name` and `pdfPage`).
- `thumb.png`: thumbnail, any size (the app decides).
- `audio.opus`: narration recorded during a replay, in live mode or while writing in the editor (§4.1).
- `background.<ext>`: pixels of `backgroundImage` (Ardoise writes `png` or `jpg`; a PDF page is turned into an image
  when it is imported; files of 1.0/1.1 may hold any extension). At most one is recognised (the first in the central
  directory); further `background.*` members are kept as unknown members. When a note is written, an unknown member
  with the very name of the background being written is dropped: the known background wins (a file read with
  `background.png` and an unknown `background.pdf`, then given a PDF background, is written with the new PDF only).
- `text.json`: reserved for recognised text; its schema would belong to the recognition feature. Ardoise does not
  write it: the handwriting recognition (M7) keeps its results in a cache on each PC (`%LOCALAPPDATA%\Ardoise\handwriting`),
  so that two PCs never rewrite the same note through Nextcloud for it. A member found in a file is kept as it is.

- `revision.json` (1.3): see §4.2.

### 4.1 `audio.opus`

The narration is an ordinary **Ogg Opus** stream (RFC 3533 container, RFC 7845 mapping), so any player (VLC, a
browser) plays the member once extracted. Ardoise writes 48 kHz mono (readers also accept stereo), 20 ms packets:

- page 0: `OpusHead` alone (beginning of stream); page 1: `OpusTags`; then audio pages. Granule positions count
  48 kHz samples including the `OpusHead` pre-skip; the last page (end of stream) carries pre-skip + the narration's
  length, so a decoder trims the end exactly. Pages carry their CRC-32; a reader refuses a page whose CRC is wrong.
- `OpusTags` vendor string `Ardoise`, and these user comments, which place the sound on the replay (all optional;
  an Ogg Opus file from elsewhere, without them, plays from the start of a replay at speed 1):

| Comment | Values | Default | Meaning |
|---|---|---|---|
| `ARDOISE_BASE` | `replay` or `note` | `replay` | recorded during a replay (replay time), or in live mode or while writing in the editor (note time) |
| `ARDOISE_SPEED` | decimal number, 0.25 to 16 | 1 | speed of the replay it was recorded with (`note`: 1) |
| `ARDOISE_MAX_PAUSE_MS` | integer, or `none` | `none` | pauses cut after this many milliseconds ("supprimer les temps morts") |
| `ARDOISE_START_MS` | signed integer | 0 | replay instant, in that time base, of the first sample; negative when the recording began before the note's time origin (`created`) |

The time base is the one of the replay: note time (`Point::t`) mapped through the speed and the cut pauses
(`crates/ardoise-media/src/replay/timeline.rs`). A video export that includes the narration replays the note with the
narration's speed and pauses, so that voice and drawing stay together; when the note is edited afterwards, the sound
keeps its instants.

A narration recorded while writing in the editor (M17, `docs/design/paper-projection-narration.md` §3) is a note-time
narration like live mode's: `ARDOISE_BASE=note`, speed 1, no cut pauses, and `ARDOISE_START_MS` the note time of its
first sample. The strokes written during the recording get their `Point::t` from the sound's clock, after the strokes the
note already held (and a stroke written later goes after the end of the sound), so a replay that plays the narration
shows strokes and sound together. A replay of a note-time narration is the note's time at speed 1 with its real pauses (no
playback rule is stored). Nothing in the file tells the two note-time cases apart, and nothing needs to.

The member has been listed since version 1.0 as opaque bytes, and readers of 1.0 and 1.1 keep it verbatim when they
rewrite a note: specifying its content changes neither the major nor the minor version. Like the rest of the note,
the narration is written atomically with the whole file and with the usual check that the file has not changed since
it was read (§6).

### 4.2 `revision.json`

Written only on a revision (SPEC §4.9, `docs/design/revision.md`); a note without it is an ordinary note. A UTF-8 JSON
object, indented with two spaces:

```json
{
  "of": "1d1e7c9a-…",
  "ofTitle": "Cours d'optique",
  "created": 1793948883407,
  "summary": [
    { "at": 1793948883407, "text": "Lignes redressées." },
    { "at": 1793949000000, "text": "Le « g » de « longueur » est refait." }
  ]
}
```

| Key | Type | Meaning |
|---|---|---|
| `of` | string | `id` (`note.json`) of the note this one is a copy of. Required, never empty. |
| `ofTitle` | string | that note's title when the copy was made, to say whose copy this is when the source is gone. |
| `created` | integer | when the copy was made, Unix milliseconds. |
| `summary` | array | the account of the changes, oldest first: one `{ "at", "text" }` per retouching (`at` in Unix milliseconds, `text` plain text written by the assistant, in the user's language). Absent = empty. |

The source is found by id in the library (titles and folders change, ids do not); it can be gone. Every save of the note
keeps the member (the editor, the library's rename, tags and favourite, live mode on a resumed sheet, restoring from the
trash); a merge or the editor's split does not carry it to the notes they make, nor does "save as a copy" (an assistant's
split of an ordinary note makes two new notes, both marked as copies of it: `docs/design/revision.md`). Keys this version does
not know inside the object are ignored (and not written back). **A member that cannot be read** (not JSON, not an
object, a missing or empty `of`, a wrong type) does not make the note unreadable: the note is read as not being a
revision, and the member stays among the unknown members, written back byte for byte, until the note is given a
revision, which replaces it. A reader of 1.2 or older keeps the member as an unknown one.

A live-mode fork of a resumed revision stays a revision: when the resumed sheet changed or went elsewhere meanwhile, the
page continues in a new note beside it that is written from the file that was resumed, so it carries the same
`revision.json` (`live/hub.rs`, `page_file` and `write_beside`); the other notes of the session (new pages) are ordinary
notes. The member has a read limit of about 16 MiB, decompressed, like `note.json`.

### 4.3 Operations of the library that write a file again

The library's duplicate (`Library::duplicate`, SPEC §4.3) and change of date (`Library::set_created`) are ordinary rewrites of a
`NoteFile`: they add no member and no key, so they do not change the format version. A duplicate keeps every member and every unknown
key or member verbatim (it is a new `id`, a title followed by the copy word, no `favorite`, and `modified` now; `created`, the tablet's
`original.bin`, the narration, `revision.json` and `boardPosition` are the original's); a change of date writes only `created` (and
`modified`), the points' times and the narration's offset being offsets from it. Their effect on the recognition of synced pages is in
`docs/design/library-order.md` §5. The free order of a binder is **not** in the note: it is `_ardoise-order.json` in the binder's folder
(same document, §2).

## 5. Versioning rules

The version is `format.version` (major) and `format.minor` in `note.json`.

- **Readers accept every older version** and every minor version of their own major version. Each version's sample
  files stay committed in `testdata/format/v<major>/` with a test that reads them and checks their content, so a new
  version cannot silently stop reading old notes.
- **A file whose major version is newer is refused** with the error "this note was made by a newer version of
  Ardoise" (code `noteNewerVersion`), and is never rewritten. Readers look at `format.version` before anything else,
  so a newer major version may change everything else, including the rest of `note.json` and `strokes.bin`.
- **Minor version** (old readers keep working): only additions that an older reader can carry along unchanged —
  new **top-level** keys of `note.json`, new optional members, and (from 1.4) new kinds of page objects, new keys of a page
  object and new values of a page object's keys (§2.2: a reader keeps an object it cannot read). Older readers keep them verbatim, and write the file
  back with *their* version number. So new data must stay meaningful when the rest of the note has been edited by
  a version that ignores it (or the new version must re-check it when reading an older minor version).
- **Major version** (old readers refuse the file): anything an older reader would misread or lose on save — a change
  to `strokes.bin` (a new tool, a new per-point value…), a new value of an existing enumeration (paper, template,
  timing, origin kind), a new key inside an existing object, a change of meaning.
- `strokes.bin` carries its own layout version byte (1 in format 1.x). Readers refuse a layout version they do not
  know.

Sample files of version 1 (regenerate only when adding one:
`cargo test -p ardoise-core -- --ignored generate_v1_fixtures`):

| File | Content |
|---|---|
| `testdata/format/v1/minimal.ardoise` | empty note, one layer |
| `testdata/format/v1/full.ardoise` | every tool, two layers, custom paper, tags, Inkspace origin, every optional member |
| `testdata/format/v1/future-minor.ardoise` | an unknown top-level key and unknown members (it was written when "1.3" stood for a later version, hence its number; it is only read, never rewritten) |
| `testdata/format/v1/full-1.2.ardoise` | format 1.2: a turned, translucent background image from a PDF page, and `original.ardoise` |
| `testdata/format/v1/revision-1.3.ardoise` | format 1.3: a revision with an account of two entries |
| `testdata/format/v1/objects-1.4.ardoise` | format 1.4: text frames in both families, one bold, turned and scaled with a key of a later version, on two layers; a text frame in a font this version does not know and an object of an unknown kind (`equation`), both kept |
| `testdata/format/v1/bookmarks-1.6.ardoise` | format 1.6: bookmarks (one with a key of a later version, one scaled and turned on a hidden layer, one whose label is not a string), a text frame and an object of an unknown kind (`sticker`), all kept |

## 6. Library conventions (not part of the file)

- File name: `YYYY-MM-DD Title.ardoise`, the date being the creation date in the computer's time zone. The title
  part is made valid for Windows: `/ \ | :` become `-`, `"` becomes `'`, `< > ? *` are dropped, control characters
  and runs of spaces become one space, leading and trailing dots and spaces are removed, at most 80 characters, and
  `_` follows a reserved device name (`CON`, `NUL`, `COM1`…). A taken name gets ` (2)`, ` (3)`… An empty title gives
  `YYYY-MM-DD.ardoise`. Renaming the note renames the file.
- Names Nextcloud's desktop client never synchronises (its default `sync-exclude.lst` and built-in rules, compared
  ignoring case) are refused for binders and avoided in titles: names ending with `~`, `.part`, `.filepart`,
  `.crdownload`, `.kate-swp`, `.unison`, `.idlk`, `.prlock`, `.dwl`, `.dwl2` or `.textClipping`; starting with `~$`;
  `~….tmp`; `#…#`; containing `.~`, `.sb-`, `.gnucash.tmp-`, `_conflict-`, `(conflicted copy` or
  `(case clash from`; `Thumbs.db`, `photothumb.db`, `desktop.ini`, `System Volume Information`; more than 254
  UTF-16 units. In a title, the smallest change is made: `Plan.part` → `Plan_part`, `Brouillon~` → `Brouillon`,
  `~$Budget` → `$Budget`, `a.~b` → `a-~b`, `Thumbs.db` → `Thumbs_.db`, `#idée#` → `#idée` (rules in
  `crates/ardoise-core/src/library/names.rs`).
- Atomic writes go through a temporary file `.<name>.<process id>.<counter>.part` next to the target (hidden, and
  ignored by Nextcloud); `<name>` is shortened when needed so that the temporary name stays within 255 UTF-16 units.
- Saving compares the file's modification time and size with those of the version loaded, before writing and again
  right before every attempt to rename the new version over the note, and refuses to overwrite a version that came
  meanwhile (conflict).
- Ignored when listing: the library's own folder `_ardoise/` (any case; no binder may take that name), hidden names
  (leading dot: the old `.ardoise/`, the `.part` temporary files of atomic writes, Nextcloud's `.~` downloads and
  `.sync_*.db`), names ending in `.part`, folders marked hidden or system by Windows, links and junctions.
- `_ardoise/` was first named `.ardoise/`; being hidden, Nextcloud's default settings did not sync it. Opening a
  library (and `init`) moves an old `.ardoise/` to `_ardoise/`, or merges it into an existing `_ardoise/` without
  overwriting anything (on a clash the `_ardoise/` file wins and the old one stays in `.ardoise/`).
- Trash: `_ardoise/trash/<deleted ms> <file name>` (unique), the note's bytes untouched, plus `<that name>.json`:
  `{"original": "Classe/2026-10-24 Titre.ardoise", "deleted": 1792843200000, "title": "Titre", "id": "…"}`
  (`original` is relative to the library and `/`-separated; it is ignored if it is absolute or contains `..`).
  Purged after 30 days.
- Conflict copies made by sync clients are recognised by name (see `crates/ardoise-core/src/library/conflicts.rs`):
  `Title (conflicted copy [user ]YYYY-MM-DD HHMMSS).ardoise`, `Title_conflict-YYYYMMDD-HHMMSS.ardoise`,
  `Title (case clash from YYYY-MM-DD HHMMSS).ardoise`, and other wordings such as
  `Title (copie en conflit de Nom YYYY-MM-DD).ardoise`.
- Tag colours (milestone M10): `_ardoise/tags.json`, owned by the library (a note only carries tag names, so the
  note format does not change):
  `{"version": 1, "tags": {"physique": {"color": "blue", "changed": 1792843200000}, "3e b": {"color": null, "changed": …}}}`.
  Keys are tags with spaces collapsed, in lower case; `color` is a palette id of the interface (`[a-z][a-z0-9-]{0,31}`;
  an id the interface does not know is not shown), `null` a removal kept as a tombstone; `changed` is the Unix time of
  the change. Written atomically with the check that the file did not change meanwhile. Nextcloud conflict copies of it
  in `_ardoise/` are merged per tag, the latest `changed` winning (on a tie a colour beats a removal, then the greater
  id), when reading; the next write folds them into `tags.json` and deletes them. Unknown fields are kept; a file that
  cannot be understood is moved aside as `tags.damaged-<ms>.json` before the next write; a file of a newer version is
  read but never rewritten (`crates/ardoise-core/src/library/tag_colors.rs`).
