# The `.ardoise` file format

> Developer document, written in English (the planning documents are in French). It describes format
> **version 1.0** byte by byte. Requirements: SPEC §3.3 and §3.4. Code: `crates/ardoise-core/src/format.rs`
> (container, `note.json`) and `crates/ardoise-core/src/format/strokes.rs` (`strokes.bin`). Model: `note.rs`
> (`ardoise-core`) and `model.rs` (`ardoise-ink`). Committed sample files: `testdata/format/v1/`.

One note = one file `YYYY-MM-DD Title.ardoise`. The file is an ordinary ZIP archive, so it can be inspected with any
unzip tool:

| Member | Required | Content | Compression |
|---|---|---|---|
| `note.json` | yes | format version and note metadata (§2) | deflate |
| `strokes.bin` | yes | every stroke, compact binary (§3) | deflate |
| `original.bin` | no | the bytes received from the tablet, untouched ("back to the original") | deflate |
| `thumb.png` | no | thumbnail, PNG | stored |
| `audio.opus` | no | narration, Opus | stored |
| `background.<ext>` | no | background image or PDF; `<ext>` = 1–10 lowercase ASCII letters or digits (`png`, `jpg`, `pdf`…) | by extension (see below) |
| `text.json` | no | recognised handwriting (written by the P2 recognition; opaque to the format) | deflate |
| anything else | no | members of newer versions: kept byte for byte when the note is rewritten | by extension (see below) |

The optional members are opaque bytes for the format code: it reads and writes them without looking inside.
Members whose name ends in `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`, `.opus`, `.ogg`, `.mp3`, `.mp4` or `.zip` (any
case) are already compressed and are stored; all others are deflated.

## 1. ZIP container

- Plain ZIP (PKWARE APPNOTE): local file headers, central directory, end of central directory record. No encryption,
  no archive comment, no spanning. Writers use only **stored** (method 0) and **deflate** (method 8); readers refuse
  other methods (the member cannot be decoded: "damaged file").
- Member order written: `note.json`, `strokes.bin`, then `original.bin`, `thumb.png`, `audio.opus`, `text.json`,
  `background.<ext>`, then unknown members in the order they were read. **Readers must not depend on the order**;
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

A UTF-8 JSON object. Version 1.0 writes it indented with two spaces, `format` first and the other keys in
alphabetical order; **key order and whitespace are not significant**.

```json
{
  "format": { "version": 1, "minor": 0, "writer": "Ardoise 0.1.0" },
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
| `tags` | array of strings | tags, in the user's order. |
| `layers` | array of `{"name": string, "visible": boolean}` | at least one; strokes refer to layers by index (0 = first). |
| `timing` | `"recorded"` or `"synthesized"` | whether point times were measured or made up (imported notes without timing). |
| `origin` | `{"kind": "tablet"}`, `{"kind": "live"}`, `{"kind": "ardoise"}` or `{"kind": "inkspace", "id": string}` | where the note comes from (`id` = Inkspace's note UUID). |

Strokes are not in `note.json` (a `strokes` key, if present, is ignored). Any other top-level key is **unknown**: a
reader keeps it with its value and writes it back unchanged when it saves the note (a key it knows always takes the
value of its own model). Unknown keys *inside* known objects are not kept (see §5).

A reader refuses the file ("invalid note") if there is no layer, the page size is zero, or a stroke refers to a layer
that does not exist. Writers refuse to write such a note.

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
- `thumb.png`: thumbnail, any size (the app decides).
- `audio.opus`: narration recorded with the replay feature.
- `background.<ext>`: imported background (image or PDF). At most one is recognised (the first in the central
  directory); further `background.*` members are kept as unknown members. When a note is written, an unknown member
  with the very name of the background being written is dropped: the known background wins (a file read with
  `background.png` and an unknown `background.pdf`, then given a PDF background, is written with the new PDF only).
- `text.json`: recognised text; its schema belongs to the recognition feature.

## 5. Versioning rules

The version is `format.version` (major) and `format.minor` in `note.json`.

- **Readers accept every older version** and every minor version of their own major version. Each version's sample
  files stay committed in `testdata/format/v<major>/` with a test that reads them and checks their content, so a new
  version cannot silently stop reading old notes.
- **A file whose major version is newer is refused** with the error "this note was made by a newer version of
  Ardoise" (code `noteNewerVersion`), and is never rewritten. Readers look at `format.version` before anything else,
  so a newer major version may change everything else, including the rest of `note.json` and `strokes.bin`.
- **Minor version** (old readers keep working): only additions that an older reader can carry along unchanged —
  new **top-level** keys of `note.json`, new optional members. Older readers keep them verbatim, and write the file
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
| `testdata/format/v1/future-minor.ardoise` | format 1.3 with an unknown top-level key and unknown members |

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
- Ignored when listing: hidden names (leading dot: `.ardoise/`, the `.part` temporary files of atomic writes,
  Nextcloud's `.~` downloads and `.sync_*.db`), names ending in `.part`, folders marked hidden or system by Windows,
  links and junctions.
- Trash: `.ardoise/trash/<deleted ms> <file name>` (unique), the note's bytes untouched, plus `<that name>.json`:
  `{"original": "Classe/2026-10-24 Titre.ardoise", "deleted": 1792843200000, "title": "Titre", "id": "…"}`
  (`original` is relative to the library and `/`-separated; it is ignored if it is absolute or contains `..`).
  Purged after 30 days.
- Conflict copies made by sync clients are recognised by name (see `crates/ardoise-core/src/library/conflicts.rs`):
  `Title (conflicted copy [user ]YYYY-MM-DD HHMMSS).ardoise`, `Title_conflict-YYYYMMDD-HHMMSS.ardoise`,
  `Title (case clash from YYYY-MM-DD HHMMSS).ardoise`, and other wordings such as
  `Title (copie en conflit de Nom YYYY-MM-DD).ardoise`.
