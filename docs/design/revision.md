# An assistant retouches a copy of a note — milestone M13

SPEC §4.9, `docs/questions.md` Q10. Ardoise still contains no AI: the assistant looks at the page and decides, Ardoise
carries out a list of **orders** on the strokes. It never sends every point of a page: a dense page has thousands of
strokes, that would be slow, costly and fragile.

Tried by hand first, with a scratch bench, on four real notes (a diagram with forces, an object-interaction diagram and
a boat, a page of plain handwriting, an incident report on ruled paper). What that trial taught is in "Lessons" below;
keep them in mind, they are why the tools look like this.

## The guarantees

1. **The source's ink is never written.** The tools that write ink take the source only to read it, or take a note that is
   already a revision (`notARevision` otherwise). This is enforced in code, not by the prompt.
2. **Nothing is written before the assistant asked for it**: `preview_revision` works in memory.
3. **An order cannot name a stroke of another note.** Its stroke numbers are read on the note being revised. The rule "never a
   letter from another note" (the notebook is sometimes lent to a pupil) goes beyond what the code can hold: `add` takes free
   points, so an assistant could retype the points of another note it has read, and nothing can tell them from any others. The
   prompt and the schema forbid it, and the account says each letter that was made.
4. Every write is atomic, with the usual stamp check and the " (2)" rule; a revision of a revision is allowed.
5. **Orders are tied to the version seen.** A stroke's number is its place in the note, so `create_revision` and `edit_revision`
   take the `version` the assistant was shown, and refuse with `noteConflict` when the note is no longer that one.

## Crate `ardoise-revise`

Pure: no I/O, no library, depends on `ardoise-ink` (strokes), `serde`, `schemars`, `thiserror`. Millimetres from the top
left of the page everywhere in its interface, device units (10 µm) only inside.

```rust
pub struct Input<'a> {
    pub strokes: &'a [Stroke],
    pub page_mm: [f32; 2],
    pub layers: u32,
    /// For each layer, whether the note shows it (beyond the list: shown).
    pub visible_layers: &'a [bool],
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

**Hidden layers.** The picture, its labels and `ink_lines` show drawn strokes only, so what the assistant reasons on has no hidden
stroke. A selection by `rect`, or with no place at all, takes drawn strokes only, unless `layer` names a layer (hidden or not);
`ids` takes what it names. The report warns whenever a hidden stroke is touched (`select`, `trim`, a `glyph` piece, which keeps its
stroke's layer), and an empty selection says how many hidden strokes it left out. `replace` takes its place and look from the first
stroke in the drawing order among those shown; when it names hidden strokes only, the shapes stay with them. `add` without `layer`
goes on the layer of the first writing shown (which also gives its pen; any writing does when none is shown), else on the first
layer shown; a new stroke on a hidden layer, named or because all are hidden, is a warning.

### Orders (JSON, `op` is the tag, unknown fields refused)

`Select`: `{ ids?, rect?: [x,y,w,h] (centre of the stroke inside), color?: "#rrggbb" (any opacity) or "#rrggbbaa" (exactly, as
`get_note` writes a translucent one; as given), layer?, min_size?, max_size? (mm, larger side of the box), not?: [ids] }`. `ids` and `rect` add up; none of them means every drawn stroke (see "Hidden layers"); the
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
| `trim` | `stroke`, `from`, `to?` (included) | keeps only these points of a stroke (from `get_strokes`; numbered from 0 again after an earlier `trim` of it), which stays where it is in time. Refused on a stroke an earlier order placed in time (see "Time"). To reuse a letter of a word written in one stroke: `glyph` with `from` and `to` |
| `replace` | `select`, `with: [Shape]`, `color?`, `width?` | the shapes take the place, tool, colour, width, layer and median pressure of the strokes they replace, at the place of the first in the drawing order; for their time, see "Time" |
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
new stroke comes at the end of the note.

`replace` starts when the first of its strokes began and lasts what the pen spent on them all, stopping before the next stroke
of the note: nothing else moves, so a narration stays right. It used to take the whole span, from the first start to the last
end, and the real trial showed why that is wrong: an arrow whose head was written ten minutes after its tail became a stroke
drawn over ten minutes, on top of everything written in between (one dash of 95 s on a real note), and the report said nothing.
Now its `time` says when the shapes are drawn, a warning says when other strokes were written between the replaced ones, and
the replay leaves a pause at the later moments (`retime` the shapes to draw them elsewhere).

**A piece of a stroke.** A letter taken out of a word written in one stroke is a `glyph` with `from` and `to`: only that piece is
timed, so what follows moves by what it needs and, with a narration, it is drawn at the pace it needs. `copy` then `trim` used to
be the way, and came out wrong: the place was reserved for the whole word (a hole on each side of the letter, everything after it
pushed by the word's length; with a narration, the letter in about 10 ms). Giving the slot back would mean undoing what the order
did to every other stroke, so `trim` now refuses a stroke an earlier order placed in time (`copy`, `glyph`, `add`, `retime`), with
a message that says so. `trim` keeps its use on a stroke that was not placed: it stays in its own time, with nothing else moved,
and `retime` afterwards places the piece at its own length. Tested against `Timeline`, with and without a narration.

Check it against the real replay (`ardoise_media::replay::Timeline`), not only against the numbers:
`crates/ardoise-media/tests/revise_replay.rs` (there because `ardoise-media` needs `nasm`, which this crate must not).

### Limits

Refused with a message, nothing comes out of the list: numbers that are not finite (JSON cannot write them, but `1e39` comes
out of `serde` infinite), unknown stroke numbers, `trim` out of range, a layer that does not exist, more than 5,000 orders, more
than 1,000,000 points made by the orders (a note's own points do not count: a dense note can still be retouched; shapes are
counted before any work, without building them, copies and glyphs as they are made), a polyline or a curve given more than
100,000 points, more than 5,000,000 samples smoothed by one list (each sample each time it is smoothed, `smooth` and `tidy_line`'s
`smooth`, counted before the work; about 2 s per million at the worst, supposé as a limit), a shape larger than 5 m, a dash or gap
under 0.2 mm, `sigma` over 5 mm, a factor outside 0.05..20, a place or distance beyond 100 m (the arithmetic stays finite; repeated scaling is caught too), a
pen width over 100 mm, a `slope` over 45°, shapes of no size (an arrow from a point to itself, a polyline of one point). Stroke
numbers and point ranges are checked when their order runs, the numbers of made strokes depending on what came before. A stroke
left without points is removed and counted. Strokes moved or made that end up off the page (by more than 0.5 mm) are a warning
in the report. A slope the fit finds steeper than 45° is no written line: the line is taken as level (`slope_from: too_steep`), with
a warning, as a slope given beyond 45° is refused.

`smooth` averages each sample with at most 64 neighbours a side (`geom.rs`), found by a binary search along the arc length; beyond,
it takes them evenly, each counting for those around it. At the tablet's rate a usual smoothing never reaches that, so the result
does not change on normal strokes (tested against a smoothing with no limit, to 0.1 µm; within 10 µm on a stroke ten times denser).
104,000 samples at `sigma` 5 took 97 s when every neighbour was visited.

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

The work grows with the lines times the strokes. It is fine on what a note holds (measured in release: 210,000 strokes on 3,000 lines,
100 merged pages, in 3.9 s; 100,000 strokes scribbled on one page in 4.7 s), but only separate strokes that each make a line reach the
square (20,000 of them: 3.7 s), and an assistant can make such a note. Beyond 10,000 lines (333 pages of 30 lines) the lines are not
looked for: every stroke is in `apart`.

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
- **`create_revision`** `{ note, orders, expected_version, summary, title? }`: a new note next to the source. `summary` (required, in the user's
  language) is the account of what changed. Default title: "<title> (propre)" / "(clean)" after the app's language. New id;
  same `created` (the clock of the points, and the note sits next to its source), `modified` now; everything else of the source
  kept: paper, layers, background image, tags, narration, text; `favorite` off; new thumbnail. Its `original` /
  `original_note` follow the editor's rule (`save_with_key`), so "Revenir à l'original" works.
- **`edit_revision`** `{ note, orders, expected_version, summary }`: orders on a revision's current strokes (numbers from `get_note` on it), which
  keeps what the user changed by hand. `notARevision` on any other note. Stamp check; the version before goes to the trash
  as `straighten_shapes` did (`Library::trash_copy`); `summary` is added to the account, dated.
- **`straighten_shapes`** changes: on a note that is not a revision it makes a revision (title as above) and leaves the note
  alone; on a revision it edits it. No more copy of the source in the trash.
- **`split_note`** changes the same way (see "As built: what writes ink").

Annotations: `create_revision` not destructive (it adds), `edit_revision` and `straighten_shapes` as today. Errors: the
usual `{ code, message }`, plus `invalidOrders` (message from `ReviseError`) and `notARevision`.

### Prompt `ameliorer_note` / `improve_note` (`note?`, `goals?`)

Absent with `--read-only`. It says, in order:

1. Look: `get_note`, `render_note`, `ink_lines`, zoom with `labels` on what will change.
2. Say what is planned, and list the spelling mistakes separately. **Correct only those the user validates.** Never change
   the meaning; nothing corrected or moved silently. A document whose words matter (a report, a record) stays sober: no
   colour unless asked.
3. Letters: first from the same handwriting in this note (compare the letters: a note can hold two hands; prefer close,
   same colour; `copy` for a letter that is a stroke of its own, `glyph` with `from`/`to` for one out of a word in one stroke), else
   pieces of that handwriting (`glyph`: q + tail of j = g, bigger w = W, u turned = n), else mark the word in colour (`add` with
   the highlighter) and say so. Never from another note, not even by retyping its points.
4. Give `after` to every copied letter, and `retime` a word moved into its sentence.
5. `preview_revision`, look at the picture, read the report (a selection of the wrong size is the usual mistake), correct, again.
   Always from the note as it is: the list is sent whole each time.
6. `create_revision` with `expected_version` (the `version` of the last preview) and the `summary`; on `noteConflict`, look again
   and redo the list. Then tell the user everything that changed, each letter made, what is uncertain.

## Format 1.3: member `revision.json`

Optional member, written only on a revision; `NoteFile.revision: Option<RevisionInfo>`. Older versions keep it verbatim
(unknown members are kept), newer minor rule as usual.

```json
{ "of": "<source id>", "of_title": "<source title at that time>", "created": 1793948883407,
  "summary": [ { "at": 1793948883407, "text": "…" } ] }
```

It is what makes a note a revision. Saves keep it (like the other members); the editor's merge and split do not carry it: what they
make is not a revision (`split_note`, the assistant's, marks what it makes). A note restored from the trash keeps it.

## Editor

When the open note has `revision`: a line under the top bar, "Copie retouchée de « Titre »", with:

- **Ouvrir l'original**: the note found by id in the library, opened in this editor, same unsaved-changes question as the page
  buttons. Greyed with "L'original n'est pas dans la bibliothèque" when it is gone.
- **Ce qui a changé**: a dialog with the account, each entry dated.
- **Comparer**: a dialog with the original and this copy side by side (one above the other on a narrow window), drawn by Rust at
  the size of the dialog; the copy as last saved.

All through the translations, both languages. The share and live mode are not concerned.

## As built: format 1.3 and the editor

What differs from the two sections above, or was decided while building them. Byte-level description of the member:
`docs/file-format.md` §4.2.

- **Keys.** `revision.json` is camelCase like `note.json`: `ofTitle`, not `of_title`. In Rust: `ardoise_core::format::{RevisionInfo,
  RevisionEntry}` (`format/revision.rs`), `NoteFile.revision`, `REVISION_JSON`, and `FormatVersion::CURRENT` is 1.3.
- **What the MCP tools call.** To make a note a revision: `file.mark_as_revision_of(&source.note, now_ms, summary)` before
  `Library::create` (it replaces any revision the file had: a revision of a revision starts a new account). To add a line to the
  account (`edit_revision`): `file.add_revision_entry(now_ms, summary)`, which returns `false` on a note that is not a revision
  (`notARevision`); a blank text adds nothing and a text is cut at `revision::MAX_ENTRY_CHARS` (20,000 characters). Both
  need the file to be saved afterwards (`Library::save` with the stamp). `RevisionInfo::of_note` and `push_entry` do the same
  on the bare struct. A file whose `revision.of` is empty is refused on write (`InvalidNote`).
- **A member that cannot be read** is not an error: the note is read as not being a revision and the member stays an unknown
  member, written back byte for byte, until `mark_as_revision_of` replaces it (a note whose `revision.json` was damaged by a
  sync accident therefore still opens, and nothing is silently destroyed). Tested with six kinds of damage.
- **Saves.** Every path that rewrites a note starts from the file on disk (`save_in` of the editor, `Library::rewrite` for rename,
  tags and favourite, live mode's `merge_outside` and its resumed sheets, narration, the MCP's `update_note`), so the member is kept by construction; tests: `notes.rs`,
  `library/tests.rs`, and the live resume tests, whose `assert_keeps_all_but_ink` now lists `revision` too. The live mode's file
  fingerprint includes it (a retouching made elsewhere during a live session is a change that a write must not overwrite).
  `split` keeps it on the half that stays, not on the new note; `merge`, "save as a copy" and the copy made from a recovered
  draft are not revisions. The trash and its restore keep the bytes. A resumed revision that live mode forks (the file changed
  elsewhere: `live/hub.rs`, `page_file`, which starts from the file as it was read) stays a revision under its new id: harmless, the
  link to its source is still true.
- **A limit of its own.** A `revision.json` over 16 MiB (`MAX_REVISION_JSON_BYTES`) is read as damaged, as above; the writer refuses to
  write one (`FormatError::TooLarge`, `noteInvalid`), so that a note never stops being a revision by being saved. An entry is 20,000
  characters at most, and `edit_revision` refuses more.
- **To the interface.** `open_note` answers `{ note, stamp, revision }`; `revision` is `null`, or the content of `revision.json`
  plus `source: { path, title } | null`, found by id in the library's listing (`app/src-tauri/src/revision.rs`; a sync client's
  conflict copy has the id of the note it copies, the original file is preferred). The editor looks again by id with
  `revision_source(id)` when the user clicks « Ouvrir l'original » (the note may have moved since) and follows the library's change
  events (`checkDisk`), so « L'original n'est pas dans la bibliothèque » appears and goes away with the source.
- **Editor.** `RevisionBar.svelte` (the line and the three buttons), `RevisionAccount.svelte` (« Ce qui a changé »),
  `RevisionCompare.svelte` (« Comparer »). The line names the source by the title it has now, the one of the time while it is
  gone. « Comparer » is greyed with « L'original n'est pas dans la bibliothèque » like « Ouvrir l'original » (it needs the source); « Ce qui a changé »
  stays available. « Ouvrir l'original » is `goTo`, the move that the page buttons share (`goPage` is now `neighbour` + `goTo`).
  The page position simply disappears when the library's list does not hold the source.
- **Comparer.** `note_picture(path, maxWidth, maxHeight)` draws the whole page of a note of the library as last saved, paper and
  background image included, as large as fits in the box (clamped to 120..2400 px), as a PNG data URL: the box is measured in the dialog
  (rounded to 50 px, times the screen's pixel ratio up to 2) and asked again, after a pause, when the window is resized. Side
  by side from 640 px of dialog width, one above the other below. No zoom.
- **Plain text.** The account is interpolated as text (`white-space: pre-wrap` keeps its line breaks), never `{@html}`; a test
  puts markup and a script in it.
- **The browser stand-in** (`lib/mock-data.ts`): `addMockRevision({ of?, title?, summary? })` adds a revision to the demo library
  on demand (the library has none by default, so the tests that count its three notes are unchanged); `lib/mock-notes.ts` answers
  `revision_source` and `note_picture` (an SVG).
- **Not done here**, by the brief: the tools that write revisions, the end-to-end journey, the user guide and the changelog (done: see the next section).

## As built: the tools, the prompt and the end-to-end journey

What differs from "Tools (`ardoise-mcp`)" above, or was decided while building it.

- **Where the code is.** `crates/ardoise-mcp/src/write/revision.rs` (`preview_revision`, `create_revision`, `edit_revision`, and the
  two operations `straighten_shapes` shares with them: `make_copy` and `edit_in_place`), `src/read.rs` (`get_strokes`, `ink_lines`,
  `render_note`'s labels, `get_note`'s ruling), `src/prompts.rs` (`improve_text`), `crates/ardoise-render/src/labels.rs` and
  `background.rs` (`ruling`). The app's side: `WriteServices::create_revision` and the `account` argument of `save_note`
  (`app/src-tauri/src/mcp/write.rs`, `notes::save_in_with_account`).
- **The guarantee is in the shape of the code.** The only functions that write ink are `make_copy` and `make_part` (new files:
  `Library::create`, which never replaces one) and `edit_in_place` (which begins with `notARevision` on a file with no `revision`). The
  source's file is read, never passed to a save. `crates/ardoise-mcp/tests/revision_tools.rs` takes the bytes and the stamp of the source
  before and after `create_revision`, `preview_revision`, `straighten_shapes`, `split_note`, `merge_notes`, `set_note_text` and a refused
  `edit_revision`, and compares. See "As built: what writes ink" for every other tool.
- **`get_note` says what a revision is.** Beyond the design: `revision: { of, of_title, created, account: [{ at, text }] }` on a retouched
  copy, so that an assistant knows to use `edit_revision` (and the prompt says so when the note it is given is one). Absent on any other note.
- **`background.ruling`** is given for `lined`, `seyes` and `grid5mm` only (`ardoise_render::ruling`, which reads the constants the template
  drawing uses): the strong lines of Seyès, the horizontal lines of squared paper (whose first line is centred, so `first_mm` depends on the
  page height). Millimetric paper, dots and music staves have none: the key is absent.
- **Labels** (`ardoise_render::draw_labels`): digits made of lines (3 x 5 units, 11 px high at any zoom, 1.4 px thick, red on a pale
  backing), drawn on the picture after the region is cut out, next to each drawn stroke's box: the first free place among above, below,
  right, left, inside; a number that falls outside the picture is left out. `render_note` numbers by place (the index of `get_note`);
  `preview_revision` by `Revised.numbers` (tested: after a `delete` the others keep their numbers). Off by default in both.
- **`preview_revision`** answers an image and a text, whose JSON is the picture's facts (`dpi`, `region_mm`, `labels`…) plus `preview: true`,
  `written: false`, `stroke_count` and the engine's `report`. An empty list of orders is allowed (it draws the note); it is refused by
  `create_revision` and `edit_revision` (`invalidArguments`: nothing to change), as is a blank `summary`.
- **`create_revision`** keeps everything of the source (the file is cloned: paper, layers, background image and its pixels, tags,
  narration, text member, unknown members) and changes the id, the title (`"<title> (clean)" / "(propre)"`; an untitled source gives
  `Untitled note (clean)` / `Note sans titre (propre)`), `favorite` (off), `modified` (now), the thumbnail (drawn by the app) and
  `revision` (replaced: a revision of a revision starts a new account). **"Back to the original"** follows the editor's rule exactly, because
  the app's `create_revision` runs `original::keep_original` on the source's file with the new ink, as `save_in` does: a tablet note keeps
  `original.bin`; a note without an original keeps its ink at the time of the copy as `original.ardoise`; a note already edited keeps the
  one it has; a source with no stroke has none (`app/src-tauri/src/mcp/write.rs`, three tests). The account's `at` is the time of the copy.
- **`edit_revision`** and **`straighten_shapes` on a revision** put the version before in the trash (`Library::trash_copy`, titled
  "<title> (before retouching)" / "(avant retouche)", or "(before straightening)" / "(avant redressement)" for the second) and add a line to
  the account in the same write (`"Shapes straightened: N"` / `"Formes redressées : N"` for the second): `WriteServices::save_note`
  takes an `account`, and the app's `save_in_with_account` adds it after the stamp check. A conflict gives `noteConflict` and takes the
  trash copy back.
- **`straighten_shapes` on an ordinary note** makes the copy with no trash copy of anything, and answers `made_a_copy: true`, the copy as
  `note`, the source as `original` and `original_unchanged: true`; nothing recognised makes no copy (`saved: false`) as before.
- **Annotations**: as the design says; `preview_revision` is in the write group, so `--read-only` does not serve it (nor `create_revision` or
  `edit_revision`). `--read-only` serves nine read tools.
- **The prompt** `ameliorer_note` / `improve_note` (`note?`, `goals?`): the six steps, then the lessons of the trial that help an assistant
  (a selection of the wrong size, the colour of a new stroke, the slope with exponents, a page of diagram, `scale: 0.9` on ruled paper). Without
  `note` it says to call `last_live_note`; without `goals` it says to ask; a note that is a revision is edited with `edit_revision`. Absent
  with `--read-only` (the prompts that only read are still served).
- **Tests**: `tests/revision_tools.rs` (the guarantee byte for byte, `notARevision`, the copy keeps paper, layers, image, tags,
  narration, text, new id, same `created`, favourite off, new thumbnail, titles in both languages, the account, the original, a narration
  reaching the engine, the stamp, `invalidOrders` carrying the engine's message, the labels' numbers, `get_strokes`, `ink_lines`, the
  ruling, annotations, `--read-only`, and the four real notes of `testdata/private/revise` when they are there: skipped otherwise,
  `ARDOISE_SAMPLES_DIR` writes the pictures); `tests/write_tools.rs` (the new `straighten_shapes`); `tests/prompts.rs`; `labels.rs` and
  `background.rs` in `ardoise-render`; the app's `mcp/write.rs`. End to end: `app/e2e/journeys-m13-revision.mjs` (wired in `run.mjs`; run it
  alone with `ARDOISE_E2E_ONLY="assistant : copie"`) and the checks added to `mcp.mjs` and `mcp-write.mjs`.
- **Not done here**: ticking `ROADMAP.md` (after the Windows verification), the trial with a real assistant.

## As built: after the independent review

### What writes ink

Every write tool, checked: `create_revision`, `straighten_shapes` and `split_note` go through `make_copy`, `make_part` and
`edit_in_place`; `edit_revision` through `edit_in_place`. `create_note` makes a new blank note or one on an image. `merge_notes` makes a
new note and only reads its sources. `update_note` rewrites the file with `Library::save` and `retitle`, for its metadata: the strokes,
the narration and the originals come back as they were read (tested), and the encoding of `strokes.bin` gives the same values.
`move_note` and `trash_note` move the file whole. `set_note_text` writes to the per-PC handwriting cache. `export_note` writes
outside the note and never replaces a file. The bridge tools only drive the app. `trash_note` can still take an original out of the
library, as it could before: it stays restorable for 30 days, and its ink is not changed.

### `split_note`

An assistant refused by `edit_revision` could split a word off an ordinary note and throw it away: the kept part was saved over the
note. So the tool no longer calls the app's split (`WriteServices::split_note` is gone); it computes the parts with `ardoise_core::ops`,
the code the editor's split uses, and writes them through the operations above. The editor's own split, a user action, does not change.

- **On an ordinary note** the file is not written. Both parts are new notes next to it, each marked as a revision of it, with a line in
  its account saying which part it is. What stays goes through `make_copy`: everything of the note is kept, the narration and its clock
  included, and "Back to the original" gives the note's whole ink. What moves goes through `make_part`: like the editor's new note, it
  has a clock of its own and the page's picture but no narration (the editor's split does not give it one either, whatever `mcp.md`
  used to say). Titles: "<title> (start)" / "(début)" at an instant, "(rest)" / "(reste)" by a zone, for what stays; "(continued)" /
  "(suite)", "(excerpt)" / "(extrait)", or `title`, for what moves. Answer: `made_copies: true`, `kept`, `created`, `original`,
  `original_unchanged: true`. When the second part cannot be written the first goes to the trash again.
- **On a revision** what stays is saved over it by `edit_in_place`: the version before goes to the trash, titled "(before splitting)" /
  "(avant découpe)", which it did not before; a line is added to the account; a conflict gives `noteConflict`. What moves is new, made first
  and taken back on a failure. It is marked as a revision of that revision, which it was not before: its ink may hold what an assistant
  made, the user must be able to see that, and the assistant to go on with `edit_revision`. Answer: `made_copies: false`, `kept`,
  `created`, `previous_version`.
- **`destructiveHint` stays `true`**, like `straighten_shapes`: it only adds on an ordinary note, but it rewrites a revision, and the
  specification keeps `false` for tools that only add.

### `version`

`get_note` and `preview_revision` answer `version`, `notes::version_of`: the file's stamp as a string, `<seconds>.<nanoseconds>-<length>`.
`create_revision` and `edit_revision` require `expected_version` and compare it with the file they just read, after `notARevision`, before
anything else: `noteConflict`, with a message that says to look again and that the strokes may have other numbers. Without it, a preview
followed by a wait for the user's answer could delete another stroke than the one shown, silently, if the user or Nextcloud changed the note
meanwhile. The stamp check of a save only covers the instant between the read and the write of one call. Their answers give the `version` of
what they wrote. `straighten_shapes`'s `strokes` have no such check: the worst it does is redraw another stroke of a copy, whose version
before is kept.

### Hidden layers

The rules are under "Stroke numbers". In code: `Input.visible_layers`, which `write/revision.rs` (`revise`) reads from the note's layers; a
layer beyond the list is shown, as the drawing does. Before, `rect` and an empty selection also took the strokes the picture does not show,
and `replace` and `add` could put new strokes where the picture would not show them: the assistant saw nothing wrong, and the report
said nothing.

### The limits

- The writer refuses a file the reader would refuse (`format.rs`, `check_read_limits`, `FormatError::TooLarge`, code `noteInvalid`), before
  any file is touched: `note.json` 16 MiB, `strokes.bin` 128 MiB, `revision.json` 16 MiB, any other member 1 GiB, 1.5 GiB in all, 4,096 members.
  Before, a 17 MiB title wrote a note that could never be read again, and repeated `edit_revision` could do the same to `strokes.bin`.
  An `edit_in_place` refused that way takes its trash copy back.
- The tools cap `title` at 1,000 characters (`create_note`, `update_note`, `merge_notes`, `split_note`, `create_revision`) and `summary` at
  20,000 (`invalidArguments`; it used to be cut silently).
- A curve's vertices are counted from the lengths between its points before any is built (`shapes::least_samples`): a curve of 1,000,000
  points made 3.2 GB of vertices before its refusal. `tests/shape_memory.rs` measures the peak with a counting allocator.

### Labels

A long stroke crossing a zoomed region has no place around its box on the picture: its number now goes next to its ink there
(`labels.rs`, `place_on_ink`, up to 32 points tried for a free place), and a stroke with no ink on the picture has no number. What numbers cover
is a grid of 4 px cells (`Taken`), not a list scanned for each candidate: the cost no longer grows with the numbers written, and numbers stay
up to 4 px further apart than they must.

## Lessons of the trial

- The first orders are wrong: a rectangle selected "yon" of a red "rayon" lying just above (hence the colour filter and the
  report's counts), new strokes kept an old colour (put colours first, or give `color`), the slope was bad with exponents and on
  short lines. So: preview, look, correct, always from the source.
- `tidy_line` shows little on writing already regular, and much on a messy page. Ruled paper may need `scale: 0.9`: the
  writer's own line pitch was wider than the rules, and descenders touched.
- Copying letters is nearly invisible when they are separate and from the same page. A word in one stroke needs `glyph` with `from`
  and `to`.
- Made letters read well (g, k, w, z), y less so; side by side with real writing they look a little tight. Fine for a letter, less
  for a word. That is why the account names them.
- Layers: a note may have several; new strokes go on the layer of what they replace.
