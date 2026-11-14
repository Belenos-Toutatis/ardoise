# Fonts of the text tool

The five files below are embedded in Ardoise (`crates/ardoise-text/src/fonts.rs`) for the text frames
(SPEC §4.4), on screen and in every export. They are the **unmodified** files of their official
releases: same bytes, same names (no subsetting, no conversion). The first four were downloaded on
2026-11-13 with the user's consent (`docs/questions.md`, Q20, item 6); Latin Modern Math on 2026-11-14,
when the user asked for the whole Greek alphabet in the text frames (the same consent: a font of the
same foundry from its official source).

| File | Size (bytes) | SHA-256 |
|---|---|---|
| `lmroman10-regular.otf` | 111536 | `1aa18cfefa58132c52ce5de70db1fd1154201c19cd2b2cdaffba4906a33e6852` |
| `lmroman10-bold.otf` | 111240 | `102fe06c430a8b681b2bf6876b7cd967ae4d47b4b6b41d915eb7913b726d9fb1` |
| `Andika-Regular.ttf` | 738780 | `694d12d0f3fb2be696dbbde93eee3ccbdee766751d836eb1fbe8aab2d439d38a` |
| `Andika-Bold.ttf` | 748748 | `568e48915cd0ebbadfc3309e0550d050ccf9df526e809dffea189c68a54fb760` |
| `latinmodern-math.otf` | 733736 | `6075562b771f8b82f0c179e363389684f2dd09de30038269e2628e504bd7be0f` |

`cargo xtask licenses` checks these digests (and `THIRD-PARTY-NOTICES` lists the fonts).

## What each face lacks, and where it is taken from

A frame looks each character up in its own face, then in Latin Modern Math, then in the other family
(`fallback_chain`; Andika is never used for the Greek and Coptic block, whose few letters it has would
mix with Latin Modern Math's). The test `the_faces_lack_what_latin_modern_math_and_andika_give`
(`src/layout/tests.rs`) lists what each file lacks of the Greek alphabet and of the symbols of physics
and chemistry; in short:

- Latin Modern Roman (both weights) has the Greek capitals, µ, Ω, Δ, °, ±, ×, ·, →, ←, ∞, √, ‰, −,
  ¹ ² ³, but no small Greek letter, neither ϵ ϑ ϕ ϖ ϱ, ≈ ≠ ≤ ≥ ⇌ ∑ ∫ ∂ ∝ ′ ″ ⇒ ⇔ ↔ ∇, nor the other
  superscript and subscript digits and signs.
- Latin Modern Math has all of Greek and every symbol of the list, but no superscript or subscript
  digit; one weight only (a bold frame emboldens its glyphs: `GlyphLook::Emboldened`).
- Andika (both weights) lacks 18 Greek capitals (Α Β Ε Ζ Η Ι Κ Λ Μ Ν Ο Ρ Σ Τ Υ Φ Χ), ε ζ η ι κ ν ξ ο
  ς τ υ ψ, ϵ ϕ ϖ ϱ, ⇌ ∝ ∇; it has every superscript and subscript digit and sign. A Latin Modern frame
  draws those from its own digits, reduced and raised like its ¹ ² ³, in Andika's advance (the
  advance of the face the browser takes them from).
- A character no face has is drawn as □ (from Latin Modern Math) and the editor names it under the
  frame being typed.

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

## Latin Modern Math

- **Version** 1.959 (5 September 2014; by Bogusław Jackowski, Piotr Strzelczyk and Piotr Pianowski,
  GUST e-foundry), the mathematical companion of Latin Modern.
- **Source**: <https://www.gust.org.pl/projects/e-foundry/lm-math/download/latinmodern-math-1959.zip>
  (576 413 bytes, SHA-256 `aaaa060b4ffc091461e875efb9498b9abfa7c7a48f38eb33882868839903a4f8`), from
  <https://www.gust.org.pl/projects/e-foundry/lm-math>; `otf/latinmodern-math.otf` taken from it as it
  is. The complete work (the font and its documentation, listed in the release's
  `doc/MANIFEST-Latin-Modern-Math.txt`) is at that address and on CTAN (`fonts/lm-math`).
- **Licence**: GUST Font License (`GUST-FONT-LICENSE-math.txt`, the release's own
  `doc/GUST-FONT-LICENSE.txt`, a version dated 2006-09-30 of the same licence), an instance of the
  LaTeX Project Public License (`LPPL-1.3c.txt`). Ardoise distributes one of the work's files,
  unmodified and under its name, with the address of the complete work above (LPPL clause 6d).

## Andika (regular, bold)

- **Version** 6.200 (SIL International, 2023).
- **Source**: <https://software.sil.org/downloads/r/andika/Andika-6.200.zip> (4 771 652 bytes,
  SHA-256 `cc9a7523fcdecf5d2e1858f8ed43bca8f1e1be1ad5f20ac81475d816a9dcf87c`), from
  <https://software.sil.org/andika/download/>; the two files taken from `Andika-6.200/` as they are.
- **Licence**: SIL Open Font License 1.1 (`OFL-Andika.txt`, the release's `OFL.txt`), with the
  Reserved Font Names "Andika" and "SIL": the files are not modified (a subset would have to be
  renamed), so they keep their name.

The licence texts are the releases' own, with their line ends written as LF.
