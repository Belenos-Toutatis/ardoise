# An assistant retouches a copy of a note — milestone M13

SPEC §4.9, `docs/questions.md` Q10. Ardoise still contains no AI: the assistant looks at the page and decides, Ardoise
carries out a list of **orders** on the strokes. It never sends every point of a page: a dense page has thousands of
strokes, that would be slow, costly and fragile.

Tried by hand first, with a scratch bench, on four real notes (a diagram with forces, an object-interaction diagram and
a boat, a page of plain handwriting, an incident report on ruled paper). What that trial taught is in "Lessons" below;
keep them in mind, they are why the tools look like this.

## The guarantees

1. **The source's ink is never written.** The tools that write ink take the source only to read it, or take a note that
   is already a revision (`notARevision` otherwise). This is enforced in code, not by the prompt.
2. **Nothing is written before the assistant asked for it**: `preview_revision` works in memory.
3. **A letter only comes from the note being revised.** An order can only name strokes of that note, so the rule "never
   from another note" (the notebook is sometimes lent to a pupil) holds by construction.
4. Every write is atomic, with the usual stamp check and the " (2)" rule; a revision of a revision is allowed.

## Crate `ardoise-revise`

Pure: no I/O, no library, depends on `ardoise-ink` (strokes), `serde`, `schemars`, `thiserror`. Millimetres from the top
left of the page everywhere in its interface, device units (10 µm) only inside.

```rust
pub struct Input<'a> {
    pub strokes: &'a [Stroke],
    pub page_mm: [f32; 2],
    pub layers: u32,
    /// A narration exists: nothing may move in time.
    pub narration: bool,
}
pub fn apply(input: &Input, orders: &[Order]) -> Result<Revised, ReviseError>;
pub struct Revised { pub strokes: Vec<Stroke>, pub numbers: Vec<usize>, pub report: Report }
pub fn ink_lines(strokes: &[Stroke], visible: &[usize]) -> InkLines;
pub struct InkLines { pub lines: Vec<InkLine>, pub apart: Vec<usize> }
```

`ReviseError` names the order (index from 0) and says what is wrong in plain English: the message goes back to the
assistant. `Revised.numbers` gives each stroke of the result its number in the orders: the labels of a preview must show
these (once saved, strokes are numbered by their place again). `visible` of `ink_lines` is the numbers of the strokes on
visible layers. `Report` and `InkLines` are `Serialize`; `Order` is `Deserialize + JsonSchema`, its doc comments are the
schema the assistant reads.

### Stroke numbers

A stroke keeps its number for the whole list of orders: its index in `input.strokes`, whatever is done to it. A stroke
an order creates (`copy`, `glyph`, `add`, `replace`) gets the next free number, reported in `Report.orders[i].new_strokes`,
so a later order can name it (the assistant can predict them too: numbers are given in order, starting at
`strokes.len()`).

**Selections are read on the note as given** (`rect`, `color`, `layer`, sizes), so that the assistant reasons on the
picture it saw, not on a state it must imagine. A stroke created by an earlier order is selected on what it was when it
was created. Geometry changes accumulate: `move` then `tidy_line` on the same strokes composes. Naming a stroke an earlier order
removed is a warning in a selection, an error in `after`, `trim` and `glyph`, which need it.

### Orders (JSON, `op` is the tag, unknown fields refused)

`Select`: `{ ids?, rect?: [x,y,w,h] (centre of the stroke inside), color?: "#rrggbb" (any opacity) or "#rrggbbaa" (exactly, as
`get_note` writes a translucent one; as given), layer?, min_size?, max_size? (mm, larger side of the box), not?: [ids] }`. `ids` and `rect` add up; none of them means every stroke; the
filters then narrow. An empty selection is an error for `replace`, `retime`, and a warning in the report otherwise: a
wrong selection must show.

| Order | Fields | Effect |
|---|---|---|
| `recolor` | `select`, `color` | colour |
| `restyle` | `select`, `width?` (mm), `tool?` | pen width, tool |
| `delete` | `select` | removes |
| `move` | `select`, `dx`, `dy` | moves |
| `scale` | `select`, `factor` or `sx`+`sy`, `origin: [x,y]` | size |
| `rotate` | `select`, `degrees`, `origin` | turns (a tilted block) |
| `smooth` | `select`, `sigma` (mm, 0.2 is light) | removes the tremble: Gaussian along the arc length, the ends stay |
| `trim` | `stroke`, `from`, `to?` (included) | keeps only these points of a stroke (from `get_strokes`; numbered from 0 again after an earlier `trim` of it): cuts a letter off a word written in one line |
| `replace` | `select`, `with: [Shape]`, `color?`, `width?` | the shapes take the place, tool, colour, width, layer, median pressure and time span of the strokes they replace, at the place of the first in the drawing order |
| `add` | `shapes`, `color`, `width?`, `tool?` (`highlighter`…), `layer?`, `behind?`, `after?` | new strokes; `behind` under the others (a highlight) |
| `copy` | `select`, `to_x?` or `dx?`, `dy?` (default 0), `factor?`, `color?`, `after?` | a copy of strokes as they are now |
| `glyph` | `parts: [Part]`, `color?`, `after?` | a letter made of pieces |
| `retime` | `select`, `after` | these strokes come right after `after` in the replay |
| `tidy_line` | `select`, `baseline?`, `left?`, `mode?`, `scale?`, `slope?`, `gaps?`, `word_gap?`, `smooth?` | see below |

`Shape`: `{"line":{"from":[x,y],"to":[x,y]}}`, `polyline {points, closed?}`, `rect {x,y,w,h}`, `circle {c,r}`,
`ellipse {c,rx,ry,angle?}`, `arrow {from,to,head?,both?}`, `dashed {from,to,dash,gap,head?}` (`head`: both ends),
`curve {points}` (Catmull-Rom). An arrow is one stroke: tail, tip, barb, tip, other barb, as the editor's straightening (with
`both`, one stroke too, which starts with the tail's head). `dashed` makes one stroke per dash and per head; every other shape
one stroke. Samples are 0.5 mm apart (`ardoise_ink::shapes::SAMPLE_SPACING`).

`Part` of a `glyph`: `{ stroke, from?, to?, flip?: "h"|"v"|"rot", sx?, sy?, x, y }`: this stroke (or these points of it),
mirrored or turned, resized, its box's left at `x` and its bottom at `y`.

### `tidy_line`

Level 1 of the user's choice: **the same handwriting, better laid out**. Letter shapes do not change, except for
`smooth`.

- Baseline: lower outline of the strokes (1 mm bins, largest y), Theil-Sen line through it (pairs at least 12 mm apart).
- `mode: "words"` (default): strokes cut into words where the gap is wider than `word_gap` (default `max(1.2 × body,
  2.5 mm)`: the bench's first value, 0.6, split "1 250" and "l'éolienne"). Each word is sheared to remove the slope, put on
  the baseline (its own median bottom, at most 0.4 body from the line's; words under 4 bins or lower than 0.45 body, such as
  punctuation, only follow the line), then placed in turn from `left`.
- `mode: "rigid"`: the whole selection is one block (formulas: an exponent must stay above).
- `slope` (degrees) overrides the fit. **Without it, a line under 40 mm, or of fewer than 8 bins, takes slope 0**: the fit is
  wrong on short lines. Exponents also fool it, in `rigid` mode the assistant should give the slope it sees.
- `gaps: "even"` (default: `0.3 g + 0.7 median`, clamped to 0.7..1.4 median) or `"keep"` (an intentional blank: a name left
  out).
- `baseline`: new y; default: where the line sits at its left end. `scale` around the start of each word and the baseline.
- The report gives, per line: strokes, words, slope used and where it comes from (`given`, `fitted`, `short_line`), body height,
  baseline, left, word gap: the assistant checks them. `ink_lines` uses the same fit and the same cut: `tidy_line` on the
  `strokes` of a line finds the same slope and words.

### Time

`after: stroke` on `copy`, `glyph`, `add`, and `retime`. `t_after` = last time of that stroke; `next` = first start among
the other strokes at or after `t_after`. The strokes come one after the other with their own duration and 40 ms pauses (before
each one and after the last): those of `copy` and `retime` in the order they were written, those of `glyph` in the order of the
parts. A new shape lasts its length at 150 mm/s, at least 60 ms (supposé).
- it fits between `t_after` and `next`: nothing else moves;
- it does not fit, no narration: every stroke starting at or after `next` moves on by what is missing;
- it does not fit, narration: **nothing moves**, they are drawn faster to fit.

`Report` totals what moved. A `retime` leaves its old slot as a pause (the replay can cut pauses). Without `after`, a
new stroke comes at the end of the note. `replace` keeps the time span of what it replaces.

Check it against the real replay (`ardoise_media::replay::Timeline`), not only against the numbers:
`crates/ardoise-media/tests/revise_replay.rs` (there because `ardoise-media` needs `nasm`, which this crate must not).

### Limits

Refused with a message, nothing comes out of the list: numbers that are not finite (JSON cannot write them, but `1e39` comes
out of `serde` infinite), unknown stroke numbers, `trim` out of range, a layer that does not exist, more than 5,000 orders, more
than 1,000,000 points made by the orders (a note's own points do not count: a dense note can still be retouched; shapes are
counted before any work, copies and glyphs as they are made), a shape larger than 5 m, a dash or gap under 0.2 mm, `sigma` over
5 mm, a factor outside 0.05..20, a place or distance beyond 100 m (the arithmetic stays finite; repeated scaling is caught too), a
pen width over 100 mm, a `slope` over 45°, shapes of no size (an arrow from a point to itself, a polyline of one point). Stroke
numbers and point ranges are checked when their order runs, the numbers of made strokes depending on what came before. A stroke
left without points is removed and counted. Strokes moved or made that end up off the page (by more than 0.5 mm) are a warning
in the report.

### `ink_lines`

Finds the written lines **from the ink**: the handwriting recogniser merges lines (it read seven lines of the incident
report as one region). Strokes of the visible layers, clustered on their vertical position, tall strokes (drawings, over
3 × the usual height) left out and listed apart in `apart`, with rules, highlights and marks too far from any line. Each line:
`strokes`, `x0`, `x1`, `baseline_left`, `baseline_right`, `slope` (degrees), `body`, `words: [{strokes, x0, x1}]`, top to bottom. Test
it on synthetic lines, tilted, with descenders; on the real notes when they are there (`testdata/private/revise/`, git-ignored;
skipped otherwise).

How: a stroke's vertical position is its median sample, as a letter's mass sits in its body whatever its ascender or
descender. The usual height of a stroke is measured first, then, on the strokes that can lead a line, the direction of the page
(the slope that lines them up best) and the pitch (the first strong peak of the autocorrelation of their positions). This matters:
strokes of one line differ by more than a letter's height, so "near" must come from the pitch. The strokes are then followed
left to right (a stroke goes on the nearest line expected near it, within 0.45 pitch), which stands lines tilted so much that
they overlap across the page; each goes to its nearest line; lines level with each other are joined, a few strokes on their own
join the line around them; a blank over 30 mm (or 8 × the usual height) breaks a line: two blocks side by side are not one line.
Dots, accents, commas and dashes come last, and letters over 3 × the usual height (an l, a p, a capital) when they sit on a line.

## Tools (`ardoise-mcp`)

### Read group

- **`get_strokes`** `{ note, strokes: [..], step? }`: the exact samples `[i, x, y, pressure, t]`, for `trim` and `from`/`to`.
  At most 20,000 points a call. `get_note`'s `points` mode is simplified and has no indices, so it cannot serve for this.
- **`ink_lines`** `{ note }`.
- **`render_note`** gets `labels?: bool`: each stroke's number written next to it (small red digits drawn as lines, in
  `ardoise-render`), meant with `region`. This is what links the picture to the numbers.
- **`get_note`** gets `background.ruling: { first_mm, pitch_mm }` for the templates with horizontal lines, so that
  writing can be put on them (from the constants of `ardoise-render/src/background.rs`).

### Write group (absent with `--read-only`)

- **`preview_revision`** `{ note, orders, region?, dpi?, labels? }`: applies in memory; answers the PNG and the report. Writes
  nothing (`readOnlyHint: true`).
- **`create_revision`** `{ note, orders, summary, title? }`: a new note next to the source. `summary` (required, in the user's
  language) is the account of what changed. Default title: "<title> (propre)" / "(clean)" after the app's language. New id;
  same `created` (the clock of the points, and the note sits next to its source), `modified` now; everything else of the source
  kept: paper, layers, background image, tags, narration, text; `favorite` off; new thumbnail. Its `original` /
  `original_note` follow the editor's rule (`save_with_key`), so "Revenir à l'original" works.
- **`edit_revision`** `{ note, orders, summary }`: orders on a revision's current strokes (numbers from `get_note` on it), which
  keeps what the user changed by hand. `notARevision` on any other note. Stamp check; the version before goes to the trash
  as `straighten_shapes` did (`Library::trash_copy`); `summary` is added to the account, dated.
- **`straighten_shapes`** changes: on a note that is not a revision it makes a revision (title as above) and leaves the note
  alone; on a revision it edits it. No more copy of the source in the trash.

Annotations: `create_revision` not destructive (it adds), `edit_revision` and `straighten_shapes` as today. Errors: the
usual `{ code, message }`, plus `invalidOrders` (message from `ReviseError`) and `notARevision`.

### Prompt `ameliorer_note` / `improve_note` (`note?`, `goals?`)

Absent with `--read-only`. It says, in order:

1. Look: `get_note`, `render_note`, `ink_lines`, zoom with `labels` on what will change.
2. Say what is planned, and list the spelling mistakes separately. **Correct only those the user validates.** Never change
   the meaning; nothing corrected or moved silently. A document whose words matter (a report, a record) stays sober: no
   colour unless asked.
3. Letters: first from the same handwriting in this note (compare the letters: a note can hold two hands; prefer close,
   same colour), else pieces of that handwriting (`glyph`: q + tail of j = g, bigger w = W, u turned = n), else mark the word in
   colour (`add` with the highlighter) and say so. Never from another note.
4. Give `after` to every copied letter, and `retime` a word moved into its sentence.
5. `preview_revision`, look at the picture, read the report (a selection of the wrong size is the usual mistake), correct, again.
   Always from the note as it is: the list is sent whole each time.
6. `create_revision` with the `summary`; then tell the user everything that changed, each letter made, what is uncertain.

## Format 1.3: member `revision.json`

Optional member, written only on a revision; `NoteFile.revision: Option<RevisionInfo>`. Older versions keep it verbatim
(unknown members are kept), newer minor rule as usual.

```json
{ "of": "<source id>", "of_title": "<source title at that time>", "created": 1793948883407,
  "summary": [ { "at": 1793948883407, "text": "…" } ] }
```

It is what makes a note a revision. Saves keep it (like the other members); merge and split do not carry it: what they make is
not a revision. A note restored from the trash keeps it.

## Editor

When the open note has `revision`: a line under the top bar, "Copie retouchée de « Titre »", with:

- **Ouvrir l'original**: the note found by id in the library, opened in this editor, same unsaved-changes question as the page
  buttons. Greyed with "L'original n'existe plus" when it is gone.
- **Ce qui a changé**: a dialog with the account, each entry dated.
- **Comparer**: a dialog with the original and this copy side by side (one above the other on a narrow window), drawn by Rust at
  the size of the dialog; the copy as last saved.

All through the translations, both languages. The share and live mode are not concerned.

## Lessons of the trial

- The first orders are wrong: a rectangle selected "yon" of a red "rayon" lying just above (hence the colour filter and the
  report's counts), new strokes kept an old colour (put colours first, or give `color`), the slope was bad with exponents and on
  short lines. So: preview, look, correct, always from the source.
- `tidy_line` shows little on writing already regular, and much on a messy page. Ruled paper may need `scale: 0.9`: the
  writer's own line pitch was wider than the rules, and descenders touched.
- Copying letters is nearly invisible when they are separate and from the same page. A word in one stroke needs `trim`.
- Made letters read well (g, k, w, z), y less so; side by side with real writing they look a little tight. Fine for a letter, less
  for a word. That is why the account names them.
- Layers: a note may have several; new strokes go on the layer of what they replace.
