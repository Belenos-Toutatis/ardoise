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
claude mcp add --scope user ardoise -- "C:\Tools\Ardoise\ardoise.exe" --mcp
```

- `--scope user`: Ardoise is available in **all your Claude Code projects** (the declaration is kept in your
  profile, not in a project).
- Keep the **double quotes** around the path (it may contain spaces).
- `--` separates Claude Code's own options from Ardoise's command: everything after it is given to Ardoise as is.
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
rename, file or export notes, nor drive Ardoise.

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

You can also simply ask in your own words, without a prompt: the assistant picks Ardoise's tools by itself.

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
| `appNotRunning`: "Ardoise is not open" | Open Ardoise — the copy whose path is declared. |
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
- **The assistant asks for your consent before each change**: Claude Code and Claude Desktop ask for permission
  before using a tool that writes; you can refuse.
- **Nothing is ever deleted for good**: a note the assistant deletes goes to Ardoise's trash (kept 30 days), where you
  can restore it. Exports never overwrite an existing file.
