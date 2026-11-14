# Ardoise and your assistant (Claude Code, Claude Desktop)

Ardoise contains no artificial intelligence and works offline. It can, however, be connected to **your own
assistant** — Claude Code, Claude Desktop or any other program that speaks the **MCP** protocol (Model Context
Protocol). The assistant then reads your notes (the handwriting, but also the **typed text**, the **equations** and the
**bookmarks**), **sees them as images** (diagrams, formulas and layout included), and can clean them up, title them,
file them or drive Ardoise during a lesson.

The assistant brings the intelligence; Ardoise only gives it access to your library, when it asks.

> **Finding this guide in Ardoise.** In **Settings > Assistant (MCP)**, the **Open the "Assistant" guide** button
> opens it in your browser, offline; if it is missing next to Ardoise (the `guide` folder), the guide on GitHub opens
> instead. The `F1` key (or the **Guide** button) opens the user guide.

## What the assistant can do for you

A few requests, as you would write them:

- "Clean up my note *Optics lesson* as **Markdown**" — or **Word**, or **LaTeX** (with `circuitikz` for circuit
  diagrams, `mhchem` for chemical equations, `siunitx` for units); the assistant lists what it was unsure of.
- "Give a title and tags to this week's untitled notes."
- "Summarise Tuesday's lesson with class 8B for the class log."
- "Find the note where I wrote Ohm's law." (the assistant also searches your typed text, your equations and your
  bookmarks)
- "Copy the equation of my note *Dilution* out in LaTeX": it reads the LaTeX source you typed, exact, without guessing
  it from the picture.
- "List the bookmarks of the note *Chapter 3*, with their place on the page."
- "File the chemistry notes in the binder *Chemistry/Year 9*."
- "Merge my three notes on circuits into one"; "split this note in two where I changed subject"; "put yesterday's test
  note in the trash".
- "Create a note on page 3 of this PDF, so I can annotate it" (with the file's path).
- "Export the dilution method sheet as a PDF to my Documents folder."
- During a lesson, with Ardoise open: "Open the answers note", "Project the page", "Next page".
- At the end of a lesson: "Make a clean version of the note I just wrote in live mode."
- "Tidy the handwriting of my note and correct my spelling mistakes": the assistant retouches a **copy**, never the
  original (see "Retouching a note on a copy" below).

## What the assistant reads in a note

An Ardoise page can hold handwriting, typed text, equations and bookmarks. The assistant receives each in its own way:

- **Handwriting**: the picture of the page (`render_note`) and the text Windows recognised, line by line, with its
  position in millimetres (`get_note_text`). The recognition makes mistakes, especially on formulas and diagrams: the
  assistant should compare with the picture.
- **Typed text** (the **Text** tool): its text is **exact**, nothing is guessed; the assistant also has its font, bold,
  size, colour and place on the page, in reading order (`get_note` and `get_note_text`). `get_note_text` gives every
  frame, saying for each whether its layer is visible; the overall "typed text" it returns only holds those of the
  visible layers.
- **Equations**: their **LaTeX** source, exact (chemistry in `\ce{…}`), with size, colour and place; `render_note`
  shows how they look. They are also in the overall typed text, as their source.
- **Bookmarks**: the words you put at spots of the page, with their position (`get_note`). They only serve the editor:
  the picture of `render_note` does not draw them.
- An object of a kind this version of Ardoise does not know is reported to the assistant (`other_objects`), but
  `render_note` does not show it.

**Search** (`search_notes`) searches like the app: titles, tags, binders, typed text (equations included), bookmarks
and recognised handwriting, ignoring accents and case. Each result says **where** the word was found (title, tags,
binder, typed text, bookmarks, handwriting) and shows a short snippet. Notes whose handwriting Windows has not read yet
are not searched by their handwriting: the result counts them, and the assistant can have one read (`get_note_text`) so
that it is found next time.

## Before you start

- Ardoise has been opened at least once on this PC, and **a library is chosen** there (the setting belongs to each
  PC).
- For the assistant to read **your handwriting**, Windows' handwriting recognition must be installed for the
  language: Windows Settings > Time & language > Language & region > (the language) > Language options >
  Handwriting. Without it, the assistant still sees your notes as images.
- In Ardoise, **Settings > Assistant (MCP)** shows the exact text to copy, with the path of *your* `ardoise.exe`.
  Copy it from there rather than typing it: that is the safest. That page also has the **Read only** box, the **Link
  with the open Ardoise** line and the button that opens this guide.

> Ardoise is portable: if you move its folder, declare it again with the new path. If you keep several copies of
> Ardoise, the assistant uses the one whose path is declared.

## Declaring Ardoise in Claude Code

Once, in **PowerShell** or the **command prompt** (the path is an example: take the one Settings > Assistant gives):

```
claude mcp add --scope user ardoise "--" "C:\Tools\Ardoise\ardoise.exe" --mcp
```

- `--scope user`: Ardoise is available in **all your Claude Code projects** (the declaration is kept in your
  profile, not in a project).
- Keep the **double quotes** around the path (it may contain spaces).
- `"--"` separates Claude Code's own options from Ardoise's command: everything after it is given to Ardoise as is.
  **Keep its double quotes too**: when Claude Code was installed with npm, `claude` is a script that PowerShell
  starts after dropping an unquoted `--`, and `--mcp` would then be taken for an option of Claude Code. With the
  quotes, the same command works in PowerShell and in the command prompt, however Claude Code was installed.
- Check: `claude mcp list` (Ardoise should be "Connected"), or `/mcp` inside Claude Code.
- Remove: `claude mcp remove ardoise`.

## Declaring Ardoise in Claude Desktop

1. In Claude Desktop: **Settings > Developer > Edit Config**. This opens `claude_desktop_config.json`, whatever the
   kind of installation:
   - usual installation: `%APPDATA%\Claude\claude_desktop_config.json`;
   - installed from the Microsoft Store (or the MSIX package): the app's folder is then under
     `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\`; the *Edit Config* button always opens
     the right file.
2. Add the `ardoise` entry to `mcpServers` (copy it from Settings > Assistant; in this file every `\` of the path is
   doubled):

   ```json
   {
     "mcpServers": {
       "ardoise": {
         "command": "C:\\Tools\\Ardoise\\ardoise.exe",
         "args": ["--mcp"]
       }
     }
   }
   ```

   If `mcpServers` already holds other servers, add only the `"ardoise": { … }` block next to them, separated by a
   comma.
3. Save, then **quit Claude Desktop completely** (from the notification area too) and start it again.

If something goes wrong, Claude Desktop's log for Ardoise is `%APPDATA%\Claude\logs\mcp-server-ardoise.log`.

## The read-only option (`--read-only`)

With `--read-only` (the **Read only** box in Settings > Assistant, which adds it to both texts), the assistant can
**only read**: list and search notes, read their content (recognised handwriting, typed text, equations, bookmarks),
see them as images and look closely at their strokes (`get_strokes`, `ink_lines`). Ardoise then gives it **no other
tool**: it can neither create, rename, file, trash, merge, split or export notes, nor store a corrected transcription,
nor try or make a retouching, nor read Ardoise's state or the last live session, nor drive Ardoise. The
`title_and_tag` prompt then gives a list of titles and tags for you to apply in Ardoise; `clean_up` works without
`export_note` (the drawings are then for you to export from the app); `improve_note` does not exist.

**Recommended whenever the assistant only needs to read** (summarising, finding a note, preparing a document from
your notes).

## Ready-made prompts

Ardoise offers ready-made prompts. In Claude Code, type `/`: they appear as `/ardoise:name (MCP)`; typing
`/mcp__ardoise__name` runs them too. Arguments follow, separated by spaces. Their names follow Ardoise's language:

| In French | In English | Purpose |
|---|---|---|
| `mettre_au_propre` | `clean_up` | a note cleaned up as Markdown, Word or LaTeX (arguments: the note, the format) |
| `titrer_et_etiqueter` | `title_and_tag` | titles and tags for untitled notes (argument: a binder) |
| `resume_seance` | `lesson_summary` | a lesson summary for the class log (arguments: the day, the class's binder) |
| `ameliorer_note` | `improve_note` | a note improved on a copy (tidier writing, straightened diagram, spelling corrected); not there in read-only mode (arguments: the note, what you want improved) |

With no note given, `clean_up` and `improve_note` take the last note of live mode (see "Driving Ardoise while it is
open" below).

You can also simply ask in your own words, without a prompt: the assistant picks Ardoise's tools by itself.

## Ardoise's tools, by what you ask for

You do not need to know them: the assistant picks. This list says what is possible, and helps you understand what it
shows when it asks permission to use a tool. A note is named by its identifier or by its path in the library;
positions are in millimetres from the top-left corner of the page.

**Read, search, look** (the only tools left in read-only mode):

| You want… | A request | Tools |
|---|---|---|
| to browse the library | "Which notes did I write this week in the binder *Class 8B*?" | `list_notes` (by binder, tag, favourite, dates; sorting), `list_binders`, `list_tags` |
| to find a note | "Find the note where I wrote Ohm's law." | `search_notes` |
| to read a note | "What does my note *Optics* say?" | `get_note` (paper, layers, text frames, equations, bookmarks; strokes on request), `get_note_text` (recognised handwriting and typed text) |
| it to look at a note | "Look at the diagram at the bottom of *Series circuit*." | `render_note` (the whole page or a region, with or without the strokes' numbers) |
| fine work on the ink | "Tell me where the written lines of this page are." | `get_strokes` (the points of a stroke), `ink_lines` (the written lines, their slope, their height) |

**Create, title, file** (the ink is never changed in the note itself):

| You want… | A request | Tools |
|---|---|---|
| a new note | "Create a squared note in the binder *Chemistry/Year 9*."; "Create a note on page 3 of this PDF." | `create_note` (blank, or on an image or a page of a PDF) |
| to title, tag, mark as favourite | "Give a title to my untitled notes." | `update_note` (the title changes the file's name; the old title and tags are not kept) |
| to file | "File the chemistry notes in *Chemistry/Year 9*." | `move_note` (the binder is created if missing; a note of the same name is never overwritten) |
| to delete | "Put the test note in the trash." | `trash_note` (restorable from Ardoise's trash, 30 days) |
| to merge | "Merge these three notes one after the other." | `merge_notes` (on one page or end to end; the original notes do not change) |
| to split | "Split this note in two at this point." | `split_note` (at an instant or around a zone; see "Retouching a note on a copy") |

**Export**:

| You want… | A request | Tool |
|---|---|---|
| files | "Export the dilution method sheet as a PDF to my Documents folder." | `export_note` |

`export_note` makes the same exports as the app: PNG, JPG, WebP, SVG, PDF (one page per note), `.txt`, Markdown and
Word, and the replay of the writing of **one** note as GIF, WebM or MP4 (a few seconds to several minutes). It never
overwrites a file: a name that is taken gets " (2)". A few details:

- The **PDF of a single note** has the note's bookmarks as its outline (the bookmarks panel of your PDF reader). The
  PDF of several notes has none: for a binder's PDF, with its outline and its summary page, use **Export the binder as
  a PDF** in Ardoise.
- The **images** do not have the bookmarks (they serve the editor).
- The `.txt`, Markdown and Word files hold **the text of the recognised handwriting** (with its drawings for Markdown
  and Word), like the app's; typed text and equations are not in them: an assistant writing a complete document reads
  them separately and adds them itself.

**Correct the transcription**:

| You want… | A request | Tool |
|---|---|---|
| search and the text exports to use a right text | "Windows read *ohn* instead of *Ohm*: correct my note's transcription." | `set_note_text` |

The correction stays in the handwriting memory of **this PC**, never in the note, and only holds while the note's ink
does not change; an empty text removes it. It only concerns handwriting, not typed text.

**Retouch the ink, on a copy** (next chapter):

| You want… | A request | Tools |
|---|---|---|
| to tidy the writing, redraw a diagram, correct mistakes | "Tidy the handwriting of my note *Report*." | `revision_orders` (the list of possible retouchings), `preview_revision` (try without writing anything), `create_revision` (write the copy), `edit_revision` (retouch a copy again) |
| clean shapes | "Straighten the shapes of my physics note." | `straighten_shapes` |

**Drive Ardoise while it is open** (see below; none of these tools exists in read-only mode):

| You want… | A request | Tool |
|---|---|---|
| to know what Ardoise shows | "Which note is open?" | `app_state` |
| to open a note | "Open the answers note." | `open_note` |
| to project in class | "Project the page.", "Next page." | `presentation` |
| to find what was just written | "Make a clean version of the note I just wrote." | `last_live_note` |

## Retouching a note on a copy

The assistant can **improve a note without touching the original's ink**: it makes a *copy* next to it and works on
the copy. Keep apart what **Ardoise guarantees** (the program enforces it, whatever the assistant is and whatever you
tell it) and what is **only asked of the assistant** (it may fall short).

**What Ardoise guarantees:**

- **The original's ink is never written by the assistant**: the tools that write ink can only change a copy of that
  kind, never an ordinary note.
- **Nothing is written during a trial**, anywhere.
- **The copy is a new note**, next to the original. When the assistant splits a note, likewise, you get two new notes,
  marked as copies: the note it started from stays whole.
- **If the note changed since the assistant looked at it** (you retouched it by hand, say), Ardoise refuses to write
  the copy or the retouching: the assistant has to look again.
- **A retouching can only name strokes of the note it retouches**, never those of another note. The strokes of a hidden
  layer are only touched if the assistant names them expressly.
- **Retouchings only act on strokes**: the page's typed text, equations and bookmarks are never changed; the copy
  keeps them as they are, and a split gives them to the part they belong to according to the instant or the zone chosen
  (a typed text goes to the second part if it was written from that instant on, or if its centre is inside the zone).
- **Nothing is deleted for good**: the version before a retouching goes to the trash (see "Privacy").

**What is only asked of the assistant**, by the prompt `improve_note` (`ameliorer_note` in French), and which Ardoise
cannot check: list the spelling mistakes first and correct only those you validate; take letters only from the same
handwriting, in the same note; mark in colour what it cannot write; tell you everything it changed; keep the replay
right. **Without that prompt, none of this is asked**: you have to say it ("give me the list of mistakes first", "use
only my handwriting from this note"). In every case, **always look at the copy** (and **Compare**) before you rely on
it.

**What you can ask**, in your own words:

- "Tidy the handwriting of my note *Incident report*: straight lines, letters of the same size, writing set on the
  rules of the paper. Do not change a word."
- "Redraw the diagram of the note *Forces on a boat* cleanly: straight arrows, equal forces the same length, aligned
  axes."
- "Colour the words to remember and put a frame round the result in the lesson note on dilution."
- "Correct the spelling mistakes in my note, using my own handwriting."
- "Straighten the shapes of my physics note" (this too is done on a copy now: the original is no longer changed). The
  assistant straightens with **the same values as the setting** next to the editor's "Straighten shapes" button
  (tolerance, minimum size, automatic alignment) and tells you which it used, in the copy's account too when they are
  not the original values. You can ask for others for one time ("with a stricter tolerance", "even the 6 mm strokes")
  without touching your setting; below 10 mm, letters may be straightened.
- "Split my note in two at this point" (the two parts are new notes, the note it started from stays as it was).
- In Claude Code, the prompt `improve_note` (`ameliorer_note` in French) goes through all this step by step, with the
  requests for care above; it is not there in read-only mode.

**What happens**, in order:

1. The assistant looks at the page (the picture, the written lines, the recognised text) and tells you what it plans.
2. It **tries**: it prepares a list of changes and sees the picture of the result **without writing anything**
   anywhere. It starts again as often as needed, looking at the picture each time.
3. When the picture is right it **creates the copy** (titled "`title` (clean)", or "`title` (propre)" in French) and,
   as the prompt asks of it, tells you everything it changed. That account, written by the assistant, stays with the
   copy.

**Spelling mistakes come first, as a list** (asked by the prompt). The assistant should not correct silently: it gives
you the list of the mistakes it sees, *you* say which ones to correct, and only those should be corrected. It should
not change the meaning of what you wrote. For a document in which every word matters (a report, an incident record) it
should stay sober: no colour unless you ask for it. Ardoise does not check the corrected text: read it over.

**Where the letters of a corrected word come from** (asked by the prompt). To replace a letter, the assistant must
take, in this order:

1. the same letter *in your handwriting, in this very note* (it compares the letters before choosing: a note can hold
   two hands, yours and a student's or a colleague's);
2. otherwise pieces of that handwriting (the bowl of a "q" and the tail of a "j" make a "g", a bigger "w" makes a
   "W");
3. otherwise it makes nothing: it **marks the word in colour** and tells you.

**No letter taken from another note.** You sometimes lend your notebook to a student: a letter from another note
might be theirs, or from another day. What Ardoise does: a retouching cannot name a stroke of another note. What it
cannot prevent: an assistant retyping by hand the points of a stroke it saw elsewhere; only the prompt forbids that. A
made letter reads well but stays a little tight next to real writing: the assistant must name every one of them in its
account.

**The replay: what is asked of the assistant.** For each letter it adds, or word it moves into its sentence, it must
say where it takes its place in the time of the writing (that of the replay and the video); Ardoise then moves the
strokes after it on if necessary. If the note has a *narration*, nothing moves (the voice and the writing would no
longer agree): the letter is drawn faster. If the assistant did not do this, the replay may be wrong: watch it.

**In the editor**, open the copy: a line under the top bar says "Retouched copy of "Optics lesson"", with three
buttons — **Open the original**, **What changed** (the assistant's account, one dated line per retouching) and
**Compare** (the original and the copy side by side). The two "original" buttons show the note it was made from *as it
is now*; if it is no longer in the library, they are greyed. See chapter 8 of the user guide.

**Retouching again.** You can ask for another retouching of the copy ("make the title red too"): the assistant changes
it *in place*, keeping what you changed by hand, and adds a line to the account. The version before goes to Ardoise's
trash (see "Privacy"). If you changed the copy meanwhile, the assistant has to look at it again first: Ardoise refuses
otherwise. Asking for a retouching of the *original note* makes a new copy.

**What this does not replace.** The assistant can be wrong, and Ardoise does not check what is only "asked" above
(mistakes validated, letters from the right handwriting, a complete account, colour): always look at the copy (and
**Compare**) before you rely on it. And the reminder of the "Privacy" chapter holds here too: to retouch a note the assistant sees it, so it goes to its
provider.

The Ardoise tools behind all this (you do not need to know them, the assistant picks them): **read** (`get_note`
with the rules of the paper, `get_strokes` for the points of a stroke, `ink_lines` for the written lines, `render_note`
with the strokes' numbers written next to them); **write** (`revision_orders` to learn the orders, `preview_revision`
to try, `create_revision` to make the copy, `edit_revision` to retouch a copy, `straighten_shapes`).

## Driving Ardoise while it is open

Four tools act on Ardoise **while it is open** (they do nothing when it is closed, and do not exist in read-only
mode). None deletes anything, changes a setting or touches the tablet:

- **state** (`app_state`): what Ardoise shows — the library, the editor (which note, unsaved changes or not), live
  mode (current page, presentation open or not);
- **open a note** (`open_note`) in the editor, as a click on its card does. Ardoise refuses while the open note has
  unsaved changes, while live mode is on screen or while a dialog is open: nothing is ever lost;
- **presentation** (`presentation`): project the live page on the screen chosen in the live window, stop
  projecting, next, previous or a given page. **You start live mode** (tablet panel > Live mode): the assistant never
  touches the tablet. "Next page" from the last page adds a blank one, like the `→` key;
- **last live note** (`last_live_note`): the notes written during the current or last live session (one note per
  page), to offer a clean version at the end of a lesson. Ardoise remembers it on this PC even after it was closed.

The assistant must use **the same copy** of Ardoise as the one you open (same folder).

## Troubleshooting

| The assistant says… | What to do |
|---|---|
| `noLibrary`: no library | Open Ardoise on this PC and choose the library. |
| `handwritingNoRecognizer` / `handwritingUnsupported` | Install Windows' handwriting for the language (see above). Images stay readable. |
| `appNotRunning`: "Ardoise is not open" | Open Ardoise — the copy whose path is declared. If it is open already, look at the "Link with the open Ardoise" line in Settings > Assistant: it says whether this window can be driven. |
| `appNotResponding` | Ardoise is busy (an export, a dialog) or still starting: try again. |
| `appBusy` | First finish in Ardoise what is under way (save the note, close the dialog, finish live mode). |
| `liveNotRunning` | Start live mode in Ardoise before asking for the presentation. |
| `noteConflict` | The note changed since the assistant looked at it (you retouched it, or another computer did): ask it to look at it again, then start over. |
| `notARevision` | The assistant tried to retouch an ordinary note "in place": Ardoise refuses, because the ink of an ordinary note never changes. Ask it to make a copy first. |
| The assistant says it can neither create, file, export nor drive Ardoise | Ardoise is declared **read only**: untick **Read only** in Settings > Assistant, copy the declaration again and restart the assistant. |
| Ardoise does not appear in the assistant | Check the path (folder moved?), `claude mcp list`, Claude Desktop's log; restart the assistant. |

## Privacy: read this first

- **What the assistant reads goes to its provider.** The recognised text and the images of your notes — which may
  hold **students' names**, marks or comments — are sent to the assistant's provider (Anthropic for Claude) and
  handled according to **your account's settings** (how long conversations are kept, whether they help improve the
  models…). Check those settings, and follow your school's rules (GDPR) before letting it read notes that hold
  information about students.
- Ardoise itself sends nothing over the Internet: the assistant reads, when you ask it to.
- **Read only is recommended** when the assistant only needs to read (`--read-only`).
- **The assistant usually asks for your consent before changing anything**: Claude Code and Claude Desktop ask for
  permission before using a tool that writes, and you can refuse. But that depends on their settings: a tool you
  "always allowed", or a mode that accepts everything, asks nothing any more. Ardoise flags the tools that change or
  remove notes (move to the trash, change the title, tags or favourite mark, move, split, straighten shapes, retouch a
  copy that was already made) as **destructive**, which prompts the assistant to ask. To be sure nothing changes, declare Ardoise **read only** (`--read-only`).
- **Nothing is ever deleted for good**: a note the assistant deletes goes to Ardoise's trash (kept 30 days), where you
  can restore it. When it retouches a copy that was already made (or straightens its shapes, or splits it), the version before
  goes there too, as "`title` (before retouching)", "`title` (before straightening)" or "`title` (before splitting)". The assistant never writes the ink of an ordinary
  note: it only retouches a copy, next to it. Merging notes makes a new one without changing the original notes; exports
  never overwrite an existing file; a corrected transcription stays in this PC's handwriting memory, never in the
  note. On the other hand, **changing a note's title or tags** (`update_note`) does not keep the old ones: it is one
  of the changes to look at before allowing it.
