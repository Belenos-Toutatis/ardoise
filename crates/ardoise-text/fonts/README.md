# Fonts of the text tool

The four files below are embedded in Ardoise (`crates/ardoise-text/src/fonts.rs`) for the text frames
(SPEC §4.4), on screen and in every export. They are the **unmodified** files of their official
releases: same bytes, same names (no subsetting, no conversion). Downloaded on 2026-11-13 with the
user's consent (`docs/questions.md`, Q20, item 6).

| File | Size (bytes) | SHA-256 |
|---|---|---|
| `lmroman10-regular.otf` | 111536 | `1aa18cfefa58132c52ce5de70db1fd1154201c19cd2b2cdaffba4906a33e6852` |
| `lmroman10-bold.otf` | 111240 | `102fe06c430a8b681b2bf6876b7cd967ae4d47b4b6b41d915eb7913b726d9fb1` |
| `Andika-Regular.ttf` | 738780 | `694d12d0f3fb2be696dbbde93eee3ccbdee766751d836eb1fbe8aab2d439d38a` |
| `Andika-Bold.ttf` | 748748 | `568e48915cd0ebbadfc3309e0550d050ccf9df526e809dffea189c68a54fb760` |

`cargo xtask licenses` checks these sizes and digests (and `THIRD-PARTY-NOTICES` lists the fonts).

## Latin Modern Roman 10 (regular, bold)

- **Version** 2.004 (OpenType release of the Latin Modern family, by Bogusław Jackowski and Janusz M.
  Nowacki, GUST e-foundry).
- **Source**: <https://www.gust.org.pl/projects/e-foundry/latin-modern/download/lm2.004otf.zip>
  (3 907 819 bytes, SHA-256 `5b0236051d3728be6616f1b274e3b910473875b5a3ef9382f0ef00384ddb161b`), the
  two files taken from it as they are. The complete work (every Latin Modern font) is at
  <https://www.gust.org.pl/projects/e-foundry/latin-modern> and on CTAN (`fonts/lm`).
- **Licence**: GUST Font License 1.0 (`GUST-FONT-LICENSE.txt`), which is the LaTeX Project Public
  License 1.3c or later (`LPPL-1.3c.txt`, from <https://www.latex-project.org/lppl.txt>, SHA-256
  `c1ae6e179a6d28bbd5c483fbacc744acc75e922d598281dc3390aab26126e163`). Ardoise distributes two of the
  work's files, unmodified, with the address of the complete work above (LPPL clause 6d).

## Andika (regular, bold)

- **Version** 6.200 (SIL International, 2023).
- **Source**: <https://software.sil.org/downloads/r/andika/Andika-6.200.zip> (4 771 652 bytes,
  SHA-256 `cc9a7523fcdecf5d2e1858f8ed43bca8f1e1be1ad5f20ac81475d816a9dcf87c`), from
  <https://software.sil.org/andika/download/>; the two files taken from `Andika-6.200/` as they are.
- **Licence**: SIL Open Font License 1.1 (`OFL-Andika.txt`, the release's `OFL.txt`), with the
  Reserved Font Names "Andika" and "SIL": the files are not modified (a subset would have to be
  renamed), so they keep their name.

The licence texts are the releases' own, with their line ends written as LF.
