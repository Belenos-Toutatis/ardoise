# Ardoise and your assistant (Claude Code, Claude Desktop)

Ardoise contains no artificial intelligence and works offline. It can, however, be connected to **your own
assistant** — Claude Code, Claude Desktop or any other program that speaks the **MCP** protocol (Model Context
Protocol). The assistant then reads your notes, **sees them as images** (diagrams, formulas and layout included), and
can clean them up, title them, file them or drive Ardoise during a lesson.

The assistant brings the intelligence; Ardoise only gives it access to your library, when it asks.

## What the assistant can do for you

A few requests, as you would write them:

- "Clean up my note *Optics lesson* as **Markdown**" — or **Word**, or **LaTeX** (with `circuitikz` for circuit
  diagrams, `mhchem` for chemical equations, `siunitx` for units); the assistant lists what it was unsure of.
- "Give a title and tags to this week's untitled notes."
- "Summarise Tuesday's lesson with class 8B for the class log."
- "Find the note where I wrote Ohm's law."
- "File the chemistry notes in the binder *Chemistry/Year 9*."
- "Export the dilution method sheet as a PDF to my Documents folder."
- During a lesson, with Ardoise open: "Open the answers note", "Project the page", "Next page".
- At the end of a lesson: "Make a clean version of the note I just wrote in live mode."
- "Tidy the handwriting of my note and correct my spelling mistakes": the assistant retouches a **copy**, never the
  original (see "Retouching a note on a copy" below).

## Before you start

- Ardoise has been opened at least once on this PC, and **a library is chosen** there (the setting belongs to each
  PC).
- For the assistant to read **your handwriting**, Windows' handwriting recognition must be installed for the
  language: Windows Settings > Time & language > Language & region > (the language) > Language options >
  Handwriting. Without it, the assistant still sees your notes as images.
- In Ardoise, **Settings > Assistant (MCP)** shows the exact text to copy, with the path of *your* `ardoise.exe`.
  Copy it from there rather than typing it: that is the safest.

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
**only read**: list and search notes, read their recognised text, see them as images. It can neither create,
rename, file or export notes, retouch a copy, nor drive Ardoise. The `title_and_tag` prompt then gives a list of titles and tags
for you to apply in Ardoise.

**Recommended whenever the assistant only needs to read** (summarising, finding a note, preparing a document from
your notes).

## Ready-made prompts

Ardoise offers ready-made prompts. In Claude Code, type `/`: they appear as `/ardoise:name (MCP)`; typing
`/mcp__ardoise__name` runs them too. Arguments follow, separated by spaces. Their names follow Ardoise's language:

| In French | In English | Purpose |
|---|---|---|
| `mettre_au_propre` | `clean_up` | a note cleaned up as Markdown, Word or LaTeX |
| `titrer_et_etiqueter` | `title_and_tag` | titles and tags for untitled notes |
| `resume_seance` | `lesson_summary` | a lesson summary for the class log |
| `ameliorer_note` | `improve_note` | a note improved on a copy (tidier writing, straightened diagram, spelling corrected); not there in read-only mode |

You can also simply ask in your own words, without a prompt: the assistant picks Ardoise's tools by itself.

## Retouching a note on a copy

The assistant can **improve a note without ever touching the original**: it makes a *copy* next to it and works on
the copy. Ardoise enforces this (it is not only the assistant's promise): the tools that write ink can only change a
copy of that kind, never an ordinary note.

**What you can ask**, in your own words:

- "Tidy the handwriting of my note *Incident report*: straight lines, letters of the same size, writing set on the
  rules of the paper. Do not change a word."
- "Redraw the diagram of the note *Forces on a boat* cleanly: straight arrows, equal forces the same length, aligned
  axes."
- "Colour the words to remember and put a frame round the result in the lesson note on dilution."
- "Correct the spelling mistakes in my note, using my own handwriting."
- "Straighten the shapes of my physics note" (this too is done on a copy now: the original is no longer changed).
- In Claude Code, the prompt `improve_note` (`ameliorer_note` in French) goes through all this step by step; it is not
  there in read-only mode.

**What happens**, in order:

1. The assistant looks at the page (the picture, the written lines, the recognised text) and tells you what it plans.
2. It **tries**: it prepares a list of changes and sees the picture of the result **without writing anything**
   anywhere. It starts again as often as needed, looking at the picture each time.
3. When the picture is right it **creates the copy** (titled "<title> (clean)", or "<title> (propre)" in French) and
   tells you everything it changed. That account stays with the copy.

**Spelling mistakes come first, as a list.** The assistant never corrects silently: it gives you the list of the
mistakes it sees, *you* say which ones to correct, and only those are corrected. It never changes the meaning of what
you wrote. For a document in which every word matters (a report, an incident record) it stays sober: no colour unless
you ask for it.

**Where the letters of a corrected word come from.** To replace a letter, the assistant takes, in this order:

1. the same letter *in your handwriting, in this very note* (it compares the letters before choosing: a note can hold
   two hands, yours and a student's or a colleague's);
2. otherwise pieces of that handwriting (the bowl of a "q" and the tail of a "j" make a "g", a bigger "w" makes a
   "W");
3. otherwise it makes nothing: it **marks the word in colour** and tells you.

**Never a letter taken from another note.** You sometimes lend your notebook to a student: a letter from another note
might be theirs, or from another day. Ardoise forbids it by construction: the changes of a note can only name strokes
of that note. A made letter reads well but stays a little tight next to real writing: the account names every one of
them.

**The replay stays right.** An added letter, or a word moved into its sentence, takes its place in the time of the
writing (that of the replay and the video); the strokes after it move on if necessary. If the note has a
*narration*, nothing moves (the voice and the writing would no longer agree): the letter is drawn faster.

**In the editor**, open the copy: a line under the top bar says "Retouched copy of "Optics lesson"", with three
buttons — **Open the original**, **What changed** (the assistant's account, one dated line per retouching) and
**Compare** (the original and the copy side by side). See chapter 8 of the user guide.

**Retouching again.** You can ask for another retouching of the copy ("make the title red too"): the assistant changes
it *in place*, keeping what you changed by hand, and adds a line to the account. The version before goes to Ardoise's
trash (see "Privacy"). Asking for a retouching of the *original note* makes a new copy.

**What this does not replace.** The assistant can be wrong: look at the copy (and **Compare**) before you rely on it.
And the reminder of the "Privacy" chapter holds here too: to retouch a note the assistant sees it, so it goes to its
provider.

The Ardoise tools behind all this (you do not need to know them, the assistant picks them): **read** (`get_note`
with the rules of the paper, `get_strokes` for the points of a stroke, `ink_lines` for the written lines, `render_note`
with the strokes' numbers written next to them); **write** (`preview_revision` to try, `create_revision` to make the
copy, `edit_revision` to retouch a copy, `straighten_shapes`).

## Driving Ardoise while it is open

Four tools act on Ardoise **while it is open** (they do nothing when it is closed):

- **state** (`app_state`): what Ardoise shows — the library, the editor (which note, unsaved changes or not), live
  mode (current page, presentation open or not);
- **open a note** (`open_note`) in the editor, as a click on its card does. Ardoise refuses while the open note has
  unsaved changes, while live mode is on screen or while a dialog is open: nothing is ever lost;
- **presentation** (`presentation`): project the live page on the screen chosen in the live window, stop
  projecting, next, previous or a given page. **You start live mode** (tablet panel > Live mode): the assistant never
  touches the tablet;
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
  remove notes (move to the trash, change the title or tags, split, retouch or straighten a copy, move) as **destructive**,
  which prompts the assistant to ask. To be sure nothing changes, declare Ardoise **read only** (`--read-only`).
- **Nothing is ever deleted for good**: a note the assistant deletes goes to Ardoise's trash (kept 30 days), where you
  can restore it. When it retouches a copy that was already made (or straightens its shapes), the version before goes there too, as
  "<title> (before retouching)" or "<title> (before straightening)". The ink of an ordinary note is never changed: the
  assistant only retouches it on a copy, next to it. Exports never overwrite an existing file.
