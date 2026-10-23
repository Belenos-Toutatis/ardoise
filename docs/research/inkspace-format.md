# Wacom Inkspace 3.0.11 local note storage: format and migration

Status: reverse-engineered on 2026-10-24 from a copy of this computer's Inkspace data (22 notes). All 22 notes were decoded and rendered. The renders match Inkspace's own `preview_*.png` files (see §7).

Proof-of-concept code (in the session scratchpad, not in the project):

- `C:\Users\prof\AppData\Local\Temp\claude\C--Users-prof-Bamboo-Wacom\64a68b47-fd6c-4f40-9d55-812b4c73a1fa\scratchpad\leveldb-probe\`: a Rust crate built on `rusty-leveldb` 4.0.2, `tiny-skia` 0.11 and `uuid` 1.
  - `src/main.rs` (`leveldb-probe <db> <out>`) dumps every key and value.
  - `src/bin/inkspace_export.rs` (`inkspace_export <WacomCloudDb\_ copy> <out> [width_scale]`) walks the note tree, decodes strokes, renders one PNG per note, and writes `summary.tsv` and `sample_stroke.json`.
  - `src/bin/pngdiff.rs` compares a render against the Inkspace preview.
- `...\scratchpad\analysis\pb.py` and `inkspace.py` hold the same decoder in Python (schema-less protobuf dumper and tree walker). `inkspace.py` writes `notes.json`.

Always run these tools on a **copy**: opening a LevelDB with `rusty-leveldb` may replay the `.log` file and compact the database.

## 1. Where the data is

`%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_38kynpdw5g1aw\LocalState\`

| Path | Content |
|---|---|
| `WacomCloudDb\_\` | **The note store.** A LevelDB (`*.ldb` tables, `*.log`, `MANIFEST-*`, `CURRENT`) holding a Wacom DiffSync replica. It contains every note, page, layer and stroke. |
| `db\` | A LevelDB with **app settings only**: 4 keys (`device`, `profile`, `queue`, `settings`). It holds no notes. |
| `db\notes\<note-uuid>\preview_<lastModifiedMs>.png` | Inkspace's render of the page at 1 px = 1 page unit (1188x864 landscape or 864x1188 portrait). |
| `db\notes\<note-uuid>\thumb_<lastModifiedMs>.png` | The same render as a 275x200 or 200x275 thumbnail. |

The `<lastModifiedMs>` in a preview's file name equals the note's `lastModifiedDate` (and the page's). It can serve as a cache key.

`WacomCloudDb\_\LOG` is a standard LevelDB log. The table files are snappy-compressed. The standard LevelDB format is readable with `rusty-leveldb`; no Windows-specific variant was found.

## 2. `db\` (settings LevelDB)

Keys and values are **UTF-16LE** strings. Values are JSON.

| Key | Value (JSON, UTF-16LE) | Notes |
|---|---|---|
| `device` | `{"id":"8KZ…","type":"COLUMBIA_CONSUMER","size":{"width":864,"height":1188},"protocol":"BLE","serialNumber":…,"BLE":{"id":"BluetoothLE#BluetoothLE<mac>-<mac>",…},"name":"Mon Bamboo","firmwareVersion":{…},"orientation":0}` | The paired Slate. `size` is in Inkspace page units (see §6). |
| `profile` | Wacom ID account, including an **access token (JWT)** and the e-mail address | **The importer must not read or copy this key.** |
| `queue` | `[]` | Upload queue (empty). |
| `settings` | `{"appID":[…],"appUserID":"…","version":"3.0.11","locale":"fr-FR","migrationCompleted":true,"lastLogin":…,"defaultNoteLocale":"fr_FR","window":{…},"selectedColor":"#BB3E9A"}` | `selectedColor` is the last-used pen colour. |

The `.ldb` and `.log` files also contain older versions of these same 4 keys. No note content was found in `db\`.

## 3. `WacomCloudDb\_\`: key schema

This database has 217 live entries. Keys are raw bytes: an ASCII prefix followed by a protobuf-encoded **NodeRef**.

```
NodeRef := protobuf { 1: varint kind ; 2: bytes id }
   kind 1 -> id is a node id: 16 raw UUID bytes in big-endian/textual order (NOT .NET Guid.ToByteArray mixed-endian;
             use Uuid::from_slice / uuid.UUID(bytes=...)) or an ASCII name ("__mate__")
   kind 2 -> "id" is an inline value (used for strokes inside a doc value, never in keys)
```

Example key: `5f 5f 64 6f 63 5f 5f | 08 01 12 10 01 23 45 67 89 ab 4c de 8f 01 23 45 67 89 ab cd` = `__doc__` + {kind 1, uuid `01234567-89ab-4cde-8f01-23456789abcd`}.

The 16 bytes are the UUID in plain big-endian (textual) order. Note folder `01234567-…` matches key bytes `01 23 45 67 89 ab …`.

| Key prefix | Count | Value |
|---|---|---|
| `__changed__` + NodeRef | 72 | 1 byte, `0x01`, on all 72 nodes. This is a dirty flag (node changed locally since the last sync). |
| `__child_to_parent__` + NodeRef | 71 | NodeRef of the parent (12 bytes when the parent is `__mate__`, 20 bytes for a UUID parent). |
| `__db_uid__` | 1 | 16 random bytes (replica id). |
| `__db_ver__` | 1 | ASCII `1.1`. |
| `__doc__` + NodeRef | 72 | The node's **child list** (see §4). |

There are 72 nodes: 1 root `__mate__`, 22 documents, 22 pages and 27 layers. Strokes are not nodes: they are stored inline in their layer's `__doc__` value. No orphan docs and no tombstones were present.

## 4. `__doc__` value: child list

```
Doc := protobuf {
  1: repeated ChildEntry           (one per child, in order)
  3: varint 0, 4: varint 0, 6: varint 0, 7: { 1: varint 0 }, 8: varint 0, 9: varint 1
                                   (constant on all 72 docs; DiffSync version/shadow counters, meaning unknown)
}
ChildEntry := protobuf {
  1: NodeRef                       kind 1 = child node (uuid); kind 2 = inline stroke payload
  3: repeated Property             Property := { 1: string name ; 2: string value }  (all values are strings)
}
```

Byte example, the start of a layer doc (one note): `0a c5 02` (field 1, ChildEntry, 325 bytes) → `0a 8b 02` (field 1, NodeRef, 267 bytes) → `08 02` (kind 2 = inline) → `12 86 02` (field 2, 262-byte stroke payload) `84 02 0d 00 00 00 00 …`. After the payload come the Property entries, for example `1a 16 0a 0f "pointsPerSecond" 12 03 "200"` and `1a 1d 0a 0c "creationDate" 12 0d "1700000000000"`. The doc ends with `18 00 20 00 30 00 3a 02 08 00 40 00 48 01`.

A node's own properties are stored in its **parent's** ChildEntry, not in its own doc.

### 4.1 Tree and properties

```
__mate__                       (root; name = the "stream", value "mate")
└─ document   nodeType=document  stream=mate  title=""  creationDate  lastModifiedDate
   └─ page    nodeType=page  stream=mate  locale=fr_FR  boundingBox  transform  [dpi=100]  creationDate  lastModifiedDate
      └─ layer (1..4)  nodeType=layer  stream=mate  layerType=vector  lastModifiedDate
         └─ stroke (inline, kind 2)  [pointsPerSecond]  [creationDate]
```

| Field | Where | Format | Observed |
|---|---|---|---|
| Note id | document node UUID | UUID | Equal to the `db\notes\<uuid>` folder name (22/22). |
| Title | document `title` | string | Always `""` or absent. Inkspace shows dates instead of titles. |
| Created / modified | document and page `creationDate`, `lastModifiedDate` | string, Unix **milliseconds** | Notes imported from the device show whole-second values (`…000`). |
| Page size | page `boundingBox` | `rect(x y w h)` | `rect(0.0 0.0 1188.0 864.0)` (18 landscape notes) or `rect(0.0 0.0 864.0 1188.0)` (4 portrait notes). |
| Orientation | page `transform` | SVG `matrix(a b c d e f)` mapping stroke space to page space | Landscape: `matrix(0.0 -1.0 1.0 0.0 0.0 864.0)`, i.e. page_x = y, page_y = 864 − x. Portrait: identity. |
| dpi | page `dpi` | string | `100` (present on 8 pages only; nominal, see §6). |
| Locale | page `locale` | string | `fr_FR` (handwriting-recognition language). |
| Paper / background | none | none | **No background or paper template property exists.** Pages are plain white. |
| Layers | layer nodes, ordered | `layerType=vector` | 1 layer on 19 notes, 2 on 2 notes, 4 on 1 note. Layers draw in child order. |

Stroke coordinates are **always in portrait device space** (864 wide, 1188 high, as the Slate reports it). Landscape pages rotate that space into place with the page transform.

## 5. Stroke payload (WILL 2.0-style "Path" protobuf)

The inline NodeRef `2:` bytes hold **one varint-length-delimited protobuf message**. The leading varint equals the remaining length, for example `84 02` = 260. The message fields are:

| Field | Wire | Meaning | Observed values |
|---|---|---|---|
| 1 | fixed32 float | startParameter (Catmull-Rom t at the first segment) | `0.0` always |
| 2 | fixed32 float | endParameter (t at the last segment) | `1.0` always |
| 3 | varint | decimalPrecision *p*: all integers below are value × 10^p | `2` always |
| 4 | packed **sint32 (zigzag)** | x,y interleaved, **delta-encoded**: first pair absolute, then differences | 2·N values |
| 5 | packed sint32 (zigzag) | per-point **width**, delta-encoded, × 10^p | N values (same count as points, 3874/3874 strokes) |
| 6 | packed sint32 (zigzag) | colour as one **RGBA** int32 (0xRRGGBBAA), a single value | e.g. `0x51A7F9FF` |
| 9 | varint | Unknown. Probably blendMode (as in WILL 2). | `2` always |

Fields 7 and 8 never occur. All 3874 strokes have exactly fields {1,2,3,4,5,6,9}.

Byte example (first stroke of one note): `84 02` len=260 · `0d 00 00 00 00` f1=0.0 · `15 00 00 80 3f` f2=1.0 · `18 02` f3=2 · `22 ac 01 f2 e6 09 f6 ff 01 00 00 5f 00 …` f4 (172 bytes; zigzag 80313, 16379, 0, 0, −48, 0 …) · `2a …` f5 · `32 05 fe e7 bf 9a 0a` f6 → `#BB3E9AFF` · `48 02` f9=2.

**The points are Catmull-Rom control points.** The first and last points are duplicated in 3873 of 3874 strokes. Curve segment *k* runs from P[k] to P[k+1] for k = 1 … N−3. Its cubic Bézier controls are P[k] + (P[k+1] − P[k−1])/6 and P[k+1] − (P[k+2] − P[k])/6. Width[k] is the stroke diameter at point *k*, in page units.

The coordinates are **raw sensor samples, not smoothed**. All x/y values are exact multiples of 0.04, which is one device unit (see §6). Median spacing is 0.62 units, which matches 200 Hz sampling.

### 5.1 Stroke properties (ChildEntry Property list)

| Property | Values | Strokes |
|---|---|---|
| `pointsPerSecond` | `200` | 3146 (Slate strokes, synced live or from memory) |
| | `60` | 56 (all on 3 notes written the same day, blue; probably drawn or edited in the app) |
| | `0` | 158 (2 notes; all their strokes share one identical `creationDate`) |
| | absent | 514 (5 notes written on two days; no `creationDate` either) |
| `creationDate` | Unix ms, string | Same 3360 strokes as above |

Timing check: strokes are stored in `creationDate` order in every layer. For consecutive strokes, the gap between `creationDate` values is ≥ the previous stroke's duration ((N−2)/pps) in 99% of cases, with a few overlaps of up to 0.3 s. So **`creationDate` is the pen-down time**, and point *i* (1 ≤ i ≤ N−2) is at about `creationDate + (i−1)·1000/pps` ms. **There are no per-point timestamps.** Uniform sampling has to be assumed, and pen lifts inside a stroke cannot be detected (the device never produces them).

### 5.2 Colours seen

`#000000` black (832; every untimed stroke is black), `#51A7F9` blue (2304), `#84BD00` green (326), `#BB3E9A` magenta (314), `#D80000` red (58), `#6B6B6B` grey (27), `#EEEEEE` very light grey (11), `#FF6E03` orange (2). Alpha is always FF. There is **no tool/brush id** in the stroke. Inkspace 3 has one pen type, so the tool is implicit: "ballpoint", width from pressure.

### 5.3 Width (the only pressure information)

Pressure is **not stored**. Only the rendered width is stored, and it is a saturating function of pressure:

| pps class | width range | distribution |
|---|---|---|
| 200 | 1.06 – 2.12 | **57 % of points at 2.1** (clipped), the rest spread from 1.1 to 2.0 |
| absent / 0 | 1.00 – 2.00 | bell-shaped around 1.7 – 2.0 |
| 60 | 1.00 – 1.74 | spread |

The original 0–2047 Slate pressure therefore cannot be recovered exactly. A monotone inverse, pressure ≈ (w − w_min)/(w_max − w_min), gives a usable proxy, but over half of the 200-pps points will read as "full pressure".

## 6. Units and the Bamboo Slate

- Page units are **1/25 of a Slate device unit**. Device units are 10 µm (`protocol.md` §4.5), so **1 page unit = 0.25 mm** (101.6 dpi; Inkspace labels this `dpi=100`).
- Evidence: coordinate quantisation is exactly 0.04 page units. The page is 864 x 1188, which equals 21600/25 x 29700/25, the **Bamboo Slate Large (A4)** sensor of 29700 x 21600 units (297 x 216 mm), turned portrait. The `device.size` of 864x1188 in `db\` is consistent.
- Conversion: `device_short_axis = x_page × 25`, `device_long_axis = y_page × 25`. Inkspace's x is the sensor's 216 mm axis and its y is the 297 mm axis. The exact axis flip relative to the raw BLE packets still has to be checked against a live capture.
- Width is also in page units: a 2.1-unit width is about 0.5 mm.

## 7. Verification (render vs `preview_*.png`)

`inkspace_export` rendered all 22 notes: white page of `boundingBox` size, page `transform` applied, Catmull-Rom curves, per-segment width = mean of the two endpoint widths, round caps, RGBA colour. `pngdiff` then compared each render to Inkspace's preview (ink = luminance < 200):

- **Geometry is exact on 22/22 notes.** 100 % of render ink lies within 1 px of preview ink, and 100 % of preview ink lies within 1 px of render ink. There is no scale error, no rotation error, no offset, and no element is missing from either side.
- Pixel-mask IoU at width × 1.0 is 0.97–0.98 for the 8 notes without 200-pps timing and 0.81–0.92 for the 200-pps notes. On those, the naive renderer draws about 15–20 % more ink. A width factor of about 0.8 brings the 200-pps notes to IoU 0.93–0.95. Inkspace's brush for those strokes is slightly thinner than "diameter = width". This is a stroke-thickness nuance only.
- Visual checks (images read side by side): one landscape note in four ink colours and one portrait note in black ink are indistinguishable from their previews.

Suggested import test metric for ROADMAP M2: "ink within 1 px both ways ≥ 0.99" plus "IoU ≥ 0.80" (or ≥ 0.9 once the brush width is tuned).

## 8. Decoding procedure (for the Rust importer)

1. Locate `%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_*\LocalState\WacomCloudDb\_`. **Copy** the whole directory to a temporary folder, then open the copy read-only in intent with `rusty-leveldb` (`create_if_missing=false`). Never open the original, and never read `db\profile`.
2. Iterate the keys. Keep the `__doc__` entries in a map from NodeRef id to value. Parse the id from the NodeRef that follows the 7-byte prefix.
3. Parse `__doc__/__mate__`. Each ChildEntry with kind 1 is a document. Read its properties (`creationDate`, `lastModifiedDate`, `title`).
4. For each document, read its doc's single child, the page (`boundingBox`, `transform`, `locale`, dates). For each page, read its children in order: the layers. For each layer, every ChildEntry with kind 2 is a stroke, in drawing order.
5. Decode each stroke: read the varint length, parse the protobuf, and un-delta fields 4 and 5 with zigzag. Divide by 10^decimalPrecision. Field 6[0] is the RGBA colour. Drop the duplicated first and last control points to get the N−2 real samples.
6. Timestamps: if `pointsPerSecond > 0`, then t_i = creationDate + i·1000/pps. Otherwise, synthesise timestamps: for example, use 200 Hz and keep strokes in stored order, with a fixed gap between strokes.
7. Pressure: derive it from width (§5.3) and keep the original width as well, so that rendering stays faithful.
8. Store the page orientation from `transform` (landscape = rotate 90°) rather than rewriting the coordinates. Alternatively, convert the coordinates to the app's device units with ×25.
9. Validate: render and compare against `db\notes\<uuid>\preview_<lastModifiedDate>.png`.

Sample decoded output (the shape of a first stroke; the values are illustrative; each point is `[x, y, width]` in page units):

```json
{"doc":"00000000-0000-4000-8000-000000000000","precision":2,"startParameter":0,"endParameter":1,
 "rgba":"#000000FF","field9":2,"creationDate":1700000000000,"pointsPerSecond":200,
 "points":[[100.0,100.0,1.3],[100.0,100.0,1.3],[100.4,100.0,1.4],[100.9,100.2,1.5],
           [101.4,100.5,1.5],[101.8,100.8,1.6],[102.2,101.0,1.7],[102.6,101.2,1.8], …]}
```

## 9. The 22 notes

Summary (the per-note details stay out of git, in `testdata/private/`):

- 18 landscape and 4 portrait pages.
- Layers: 1 layer in 19 notes, 2 layers in 2 notes, 4 layers in 1 note.
- `pointsPerSecond`: 200 on most strokes, 60 on a few strokes of the multi-layer notes, `0` in 2 notes, absent in 5 notes.
- 13 to 426 strokes and 576 to 26 840 control points per note.

Totals: 3874 strokes and 238 062 control points.

## 10. Is this UIM or WILL?

- It is **not** the Universal Ink Model: there is no RIFF/`UINK` container, no ink tree, no sensor data channels. `universal_ink_library` cannot read it.
- The stroke payload has the shape of the **WILL 2.0 path/stroke encoding**: start/end parameters, decimalPrecision, zigzag delta-packed points, per-point widths, RGBA colour, blend mode. It is wrapped in Wacom DiffSync's node/property protobuf (`Wacom.Cloud.DiffSync.dll`). The field names above are inferred from observation and could not be confirmed against a published `.proto`.

## 11. Open questions

1. **Field 9 = 2**: blend mode or brush id? It does not affect the rendering of these notes.
2. **Doc-level fields 3/4/6/7/8/9** (constant): DiffSync revision counters. They are not needed for import.
3. **Exact Inkspace brush** for 200-pps strokes: the preview is about 20 % thinner than diameter = width. Width × 0.8 fits better, but the true pressure→width curve and the stroke-outline algorithm are unknown.
4. **pps `0` / absent** notes (8 of 22, 1 017 strokes): there is no timing at all. Where did they come from? Three of them have whole-second dates (device memory import?). The others were created or migrated on a single day in 2026. Replay will need synthetic timing for these.
5. The **sensor axis mapping** (Inkspace x/y vs raw Slate BLE x/y and the margins in `protocol.md` §4.5) still needs to be confirmed with one live capture of a known point.
6. The `__changed__ = 01` flag on every node suggests nothing has synced to Wacom Cloud since the last change. There may be cloud-only notes; this local store holds 22 notes, which matches the 22 preview folders.
7. Older stroke versions (before erasing) could linger in `WacomCloudDb\_\*.ldb` until compaction. Only the live view was examined.
