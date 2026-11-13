# The library's note-taking features of 2026-11-13 (Q20)

> Developer document, written in English. Requirements: SPEC §4.3 and §4.6, `docs/questions.md` Q20. Code:
> `crates/ardoise-core/src/library/{order,duplicate}.rs` (the library side), `app/src-tauri/src/{library,export,print}.rs`
> (the commands), `app/ui/src/lib/library-{logic,state,activity}*.ts` and `app/ui/src/components/library/` (the screen).
> The PDF outline is `crates/ardoise-render/src/pdf.rs`. The journeys are `app/e2e/journeys-q20-library.mjs`.

Four things, all on the library screen: duplicating a note and changing its date (§1), the free order of a binder (§2),
exporting a binder as one PDF (§3), and printing 1, 2 or 4 notes per sheet (§4). §5 says what each of them does to the
recognition of the tablet's pages, which is what could have gone wrong.

## 1. Duplicating a note, changing its date

`Library::duplicate(path, new_id, copy_word)` reads the note's whole file (`NoteFile`) and writes it again with
`Library::create`, in the same binder. Reading and writing a `NoteFile` is the file format's own round trip, so
**everything the file holds is carried unless a line below changes it**:

| What | In the copy | Why |
|---|---|---|
| `id` | new (UUID v4, made by the app: the core has no random source) | an id names one note for the life of the file; the order file, the revisions' `of` and the page recognition all use it |
| `title` | `title (copie)`, then `(copie 2)`, `(copie 3)`… for the copy of a copy; an untitled note gives `(copie)` | the user's rule. The word comes from the interface's translation (`library.duplicate.word`), so Rust holds no visible text. An untitled copy is *not* left untitled: the file name is made from the date and the title, and a title is the only mark a card can show that tells two notes of the same minute apart |
| `created` | the original's | the user's choice. It is also the time origin of every point and of the narration's offset: a copy with another date would not be out of step, but the two notes would stop being twins |
| `modified` | now | it is a new file |
| `favorite` | off | the user's rule |
| tags, layers, strokes, page, paper, `backgroundImage` and its pixels (`background.*`) | kept | it is the same note |
| `original.bin` (the tablet's page) | kept | so « Revenir à l'original » works on the copy |
| `original.ardoise` (the note before its first edit) | kept | the same reason, for notes without a tablet page |
| `audio.opus` (the narration) | kept | the user's rule (about 0.5 MB a minute) |
| `thumb.png` | kept | same ink |
| `text.json` (recognised text, not written by Ardoise) | kept if present | opaque member, kept verbatim like every unknown one |
| `origin` (`tablet`, `live`, `inkspace` + id, `ardoise`) | kept | where the ink came from is a fact about the ink. An Inkspace import that looks for notes by that id stays happy with either |
| `boardPosition` and every other unknown key of `note.json` | kept verbatim | `unknown_fields` |
| unknown members | kept verbatim | `unknown_members` |
| `revision.json` | kept: the copy of a revision is a revision of the same note | the account of changes describes the ink, which is the same. Dropping it would silently lose it |

In a binder that has a free order (§2) the copy is listed right after its original. In a binder without one nothing is
written: the copy has the original's date and a title that sorts after it, so it is next to it in the date sorts and in the
free order's « unlisted, by date » rule alike.

The interface calls `duplicate_note` once per note (progress, « Arrêter » between two notes, the failures listed), then
offers « Annuler »: the copies go to the **trash**, not to oblivion, so that undoing is itself undoable.

`Library::set_created(path, created, expected)` rewrites the note with its new `created` and renames the file to
`YYYY-MM-DD Title.ardoise` (the same rename as a retitle, `rename_to_match`). The ink and the narration are not touched:
their times are offsets from `created`. The same date writes nothing. Dates before 1970 and after 9999-12-31 are refused
(`invalidDate`), which keeps the year in a file name at four digits. The dialog (`DateDialog.svelte`) has a
`datetime-local` field and « Aujourd'hui », which fills the field with now; « Annuler » puts the old date back, refused
(`noteConflict`) if the file changed since.

## 2. The free order of a binder

### The file

`<binder folder>/_ardoise-order.json`:

```json
{ "version": 1, "changed": 1792843200000, "notes": ["6f0c9a52-…", "…"] }
```

- **Name.** Visible: Nextcloud's default settings do not synchronise names starting with a dot. Not `.ardoise` (so never a
  note: the walk lists notes by extension), and not inside `_ardoise/` because it belongs to the binder: renaming or moving
  the folder takes it along. The library's root can have one too (the API accepts it; no view shows the root alone).
- **Content.** `notes` lists note ids, first to last, of the notes of **this** binder only. Ids, not file names: a retitle or a
  change of date renames the file and must not lose the place. The listing already carries the id of every note
  (`NoteEntry::id`, read from `note.json` and cached by stamp), so nothing more is opened. `changed` is the writer's clock
  (Unix ms). Unknown top-level keys are kept when the file is rewritten.
- **Missing, damaged, too large (8 MB), not an object, wrong types**: no order. The listing never fails because of it and
  the notes go by date. A file of a newer `version` is read as well as possible and never rewritten (`orderNewer`).
- **What an order says about a binder's notes.** A listed id that is not a note of the binder (deleted, moved, not synced yet)
  is ignored. A note the file does not list (arrived by the tablet or a sync, new, restored, moved in) goes **after the
  listed ones, oldest first**: « a note that arrives goes at the end », and a copy sits after its original. The interface does the
  arranging (`inBinderOrder` in `library-logic.ts`); the file only holds ids.
- **Writing.** `Library::set_binder_order(binder, ids)` stores exactly the ids it is given (blank, over-long, repeated ones
  dropped, at most 100 000): the interface sends the notes of the binder in the order shown. An id of a note that is not
  there any more is therefore not kept: a note restored from the trash arrives at the end, like any arrival. Atomic
  (temporary file in the folder, then rename), and checked against the file it replaces (`write_atomic_checked`,
  three attempts, then `orderConflict`). Storing the order the file already holds writes nothing.

### Conflicts

Two computers writing the file at once make Nextcloud keep a copy `_ardoise-order (conflicted copy …).json`.
`State::read` reads the file and every copy of it (`is_order_file_name` accepts the same wordings as the note conflicts) and
**uses the one with the greatest `changed`; the main file wins a tie, then the later modification time**. A note the winner does
not list is placed at the end by date, so **no note is lost whatever the winner**. Reading never writes. The next write is
newer than every file it read (`changed` is at least the winner's plus one, even if a clock is behind) and deletes the copies
it read, if they did not change since. A copy that cannot be understood is left alone.
The copies are not notes and never appear among the library's conflict cards (`Listing::conflicts` looks at note files only).

### Listing, watching

- `Listing::orders` (binder → ids) is read during the walk the listing already makes: the walk reports an order file
  (`Found::Order`) like a note file, and only binders that have one cost a read. `LibraryListing.orders` carries it to the
  interface.
- The watcher snapshots the order files next to the notes and reports `LibraryChange::OrderChanged { folder }` when one
  appears, changes or goes (an order brought by Nextcloud from another computer shows at once). The interface refreshes
  its listing on any change, as it already does.
- Deleting a binder trashes its notes, then removes the folders left empty. A folder that holds nothing but its order file
  (and copies of it) counts as empty: the order files go with it (`remove_if_only_order_files`). A folder with another file
  keeps its order.

### On screen

- A fifth sort, « Ordre du classeur » (`binder`), **offered only in the view of a binder**. In the other views (all notes,
  favourites, a tag) the choice is not in the menu and the notes go by date, newest first; the user's choice is kept and comes
  back in a binder. A binder's view shows its own notes first, then each sub-binder's in that sub-binder's order, the
  sub-binders as the sidebar lists them (`arrangeByBinderOrder`). During a **search** the notes keep that order but cannot be moved
  (the cards that match are a part of the binder; where a drop would go among the others would be a guess).
- **Drag between the cards** (`NoteCard.svelte`, `LibraryState.hoverSpot/dropAtSpot`, `moveToSpot`): over the left half of a card a bar
  shows before it, over the right half after it. The cards dragged (the whole selection when the dragged card is part of it) must be
  of one binder, and the spot must be next to a card of that binder (in a parent binder's grid, a drop between two cards of a
  sub-binder only moves a note of that sub-binder): otherwise nothing is accepted. They move among their binder's notes, keeping their
  relative order, and that binder's file is written. Dropping on a binder of the sidebar still moves the notes there, in any sort.
- **Writes are queued** (`LibraryState.applyOrder`): the grid follows a move at once, the file is written after the move before it
  (a key held down moves faster than a file is written, and two writes at once could end with the older order), and the listing is read
  again when the last write is over.
- **The keyboard** (required): `Alt+←` / `Alt+→` move the card (or the selection) one place earlier or later in its binder,
  `Alt+Début` / `Alt+Fin` to the start or the end (`moveByStep`). The focus stays on the card, and an `aria-live` region says
  « Beta déplacée en position 1 sur 3 » or that it already is at the end.
- The editor's page buttons and `Alt+←/→` walk through `lib.visible`, and the export of several notes takes the selection in
  display order: both follow the free order with no change to the editor.

## 3. Exporting a binder as one PDF

Right-click (or the ⋯ menu) on a binder of the sidebar: « Exporter le classeur en PDF… ». It is the usual export dialog
(`ExportDialog.svelte`) with the format fixed to PDF, no text export, and a checkbox « Inclure les sous-classeurs » (**off by
default**: the user said the sub-binders are an option; the box is only there for a binder that has some). The notes are
`LibraryState.binderNotes(binder, includeSub)`: the binder's own in its order, then each sub-binder's, **whatever sort is on
screen**.

**Outline (bookmarks).** `pdf-writer` writes outlines, so one is built (`notes_to_pdf_with_outline`, flat `OutlineEntry { title,
level, page }`, all open, `PageMode UseOutlines`): a bookmark per note (its title, or its date when untitled), and with the
sub-binders a bookmark per sub-binder (to its first note) that holds its notes and its own sub-binders. The interface sends it
beside the sources (`outline` of `export_notes`: for each source the binders it is in below the exported one, and the title
shown). The later feature of bookmarks in notes (SPEC §4.4) would add its entries to the same list. An export without the
outline, or with one that is not one entry per note, is the PDF of before, byte for byte. A single-note PDF (the editor's
export) is unchanged.

## 4. Printing, pages per sheet

Printing is GDI through Windows' print dialog (`print.rs`, `print_windows.rs`): the job draws the pages itself, so the
**printer driver's own « pages per sheet » cannot be used** (it would act on sheets that already are our pages, and is not
read back). The N-up is therefore ours: « Notes par feuille: 1 / 2 / 4 » in the dialog (remembered per PC), notes laid out in
reading order in cells of the sheet, each note fitted to its cell with its proportions kept and the usual « taille réelle » or
« ajustée » rule, about 4 mm between cells. **The sheet turns with the layout**: two portrait notes stand side by side on a landscape
sheet, two landscape notes one above the other on a portrait sheet; four keep the first note's orientation (2 × 2). **The dialog's pages are
the sheets**: 5 notes two to a sheet make 3 pages, and « 2 » prints notes 3 and 4. The same dialog serves « Imprimer le classeur… » (binder menu),
with the same « Inclure les sous-classeurs » option and the binder's order. The part that cuts the sheet (`cells`,
`sheet_orientation`, `selected_sheets`) is platform-free and tested; the job is tested by printing to « Microsoft Print to PDF » and
reading the page count and sizes back.

## 5. What a duplicate and a change of date do to the recognition of tablet pages

Pages are recognised by `ardoise-device/src/pages.rs` (`page_note_id`, `ImportedPages::find`, the update of a screen stored again,
`docs/design/blackboard.md` §9.6). **Nothing in the code of that file changed**; `duplicate_tests` was added to it. What holds:

- A synced note's id is derived from its page. A duplicate has **another id**, so `find` (by id, then `original.bin` equal to the page's
  bytes) never takes it for the note of the page; the original is found, and a leftover page is not imported again. With the original
  in the trash the trashed file still has the page's id and bytes (`in_trash`).
- The update of a screen stored again (`earlier_note`) only accepts notes whose **id** is the earlier page's: a duplicate is never one of
  two candidates, so the update never refuses for « 2 notes » because of a copy; the copy is not updated and not touched.
- The fallback for a blackboard page by creation time and `original.bin` prefix **does** see a duplicate (same time, same bytes). It
  only answers « this page is already imported », for the original or for the copy: it keeps a page from coming back when the
  original was deleted for good and the copy remains. Trashing the copy counts as deleting the page.
- With the original deleted for good, a *Slate* page still on the tablet (sync without deletion) has no fallback and is imported again
  as a note of its own; the copy is not touched. Nothing is lost.
- **Changing a date** keeps the id and `original.bin`: a tablet note's page is still recognised by id. The edge that remains is a
  blackboard note **updated** by a later store: it carries the later page's id and is found for the earlier page by its creation
  time, which was changed. That earlier page, offered again by a tablet that keeps its pages, is then not recognised and is imported as
  a note of its own (no note is lost or overwritten). And a blackboard note dated **after** its page's time is not a candidate for
  the update of its screen (`earlier_note` wants a note created no later than the earlier page): a new note is added, the dated one
  untouched.

## 6. Journeys, and what a real drag can be driven with

`app/e2e/journeys-q20-library.mjs` (run by `run.mjs`) covers, on the production executable in both languages: duplicating (right click,
selection bar, undo), changing a date, the free order by keyboard (kept after a reload, followed by the editor's page buttons), exporting a
binder as a PDF with and without sub-binders (pages and bookmarks read from the file), and a drag.

**The drag.** The protocol's mouse events never hold the real button, so the Windows drag-and-drop session (OLE) cannot be started
(`journeys-m7-drag.mjs`). What does work in WebView2: `Input.setInterceptDrags` makes it report the drag the page starts from real mouse
presses and moves (`Input.dragIntercepted`, with the data the page's own `dragstart` set), and `Input.dispatchDragEvent` plays it back
(`dragEnter`, `dragOver`, `drop`) as drag events made by the browser. The journey drives the real handlers that way: the insertion mark on
the left and right halves of a card, the drop between two cards (order written in the binder's folder), the drop on a sidebar binder (the
file moves). **What it does not prove** is that Windows hands a drag to the window at all (the layer commit 55c1551 fixed): that stays a
manual check on the PC. Printing (the Windows dialog) is not driven; its job is tested against « Microsoft Print to PDF ».
