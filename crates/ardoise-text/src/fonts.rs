//! The five font files shipped with Ardoise, embedded in the binary (provenance, versions,
//! licences and SHA-256: `fonts/README.md`), and the order a frame looks a character up in them.

use std::sync::OnceLock;

use ttf_parser::Face;

/// A family of the text tool. Serialised as `"latinModern"` or `"andika"` (`docs/file-format.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FontFamily {
    /// Latin Modern Roman 10, the font of LaTeX (GUST e-foundry, version 2.004).
    LatinModern,
    /// Andika, a sans serif designed for people learning to read (SIL, version 6.200).
    Andika,
}

impl FontFamily {
    pub const ALL: [FontFamily; 2] = [FontFamily::LatinModern, FontFamily::Andika];

    /// The other family of the tool.
    pub fn other(self) -> FontFamily {
        match self {
            FontFamily::LatinModern => FontFamily::Andika,
            FontFamily::Andika => FontFamily::LatinModern,
        }
    }

    /// The code of the family in the WebAssembly interface (its index in [`FontFamily::ALL`]).
    pub fn code(self) -> u32 {
        match self {
            FontFamily::LatinModern => 0,
            FontFamily::Andika => 1,
        }
    }

    /// The family of a code; an unknown code means Latin Modern.
    pub fn from_code(code: u32) -> FontFamily {
        match code {
            1 => FontFamily::Andika,
            _ => FontFamily::LatinModern,
        }
    }
}

/// One embedded font file: a face of a family, or Latin Modern Math, the face both families take
/// the characters they lack from (Greek, mathematical symbols).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontFace {
    LatinModern,
    LatinModernBold,
    Andika,
    AndikaBold,
    /// Latin Modern Math (GUST e-foundry, version 1.959): one weight only. In a bold frame its
    /// glyphs are emboldened (`layout`, [`crate::GlyphLook::Emboldened`]).
    LatinModernMath,
}

impl FontFace {
    /// Every face, in the order of [`FONT_FILES`] (its index is [`FontFace::code`]).
    pub const ALL: [FontFace; 5] = [
        FontFace::LatinModern,
        FontFace::LatinModernBold,
        FontFace::Andika,
        FontFace::AndikaBold,
        FontFace::LatinModernMath,
    ];

    /// The face of `family` in that weight.
    pub fn of(family: FontFamily, bold: bool) -> FontFace {
        match (family, bold) {
            (FontFamily::LatinModern, false) => FontFace::LatinModern,
            (FontFamily::LatinModern, true) => FontFace::LatinModernBold,
            (FontFamily::Andika, false) => FontFace::Andika,
            (FontFamily::Andika, true) => FontFace::AndikaBold,
        }
    }

    /// Its index in [`FONT_FILES`], also its code in the WebAssembly interface.
    pub fn code(self) -> u32 {
        match self {
            FontFace::LatinModern => 0,
            FontFace::LatinModernBold => 1,
            FontFace::Andika => 2,
            FontFace::AndikaBold => 3,
            FontFace::LatinModernMath => 4,
        }
    }

    /// The face of a code (`None` beyond [`FontFace::ALL`]).
    pub fn from_code(code: u32) -> Option<FontFace> {
        FontFace::ALL.get(code as usize).copied()
    }

    /// Whether the face is bold (Latin Modern Math has no bold: false).
    pub fn is_bold(self) -> bool {
        matches!(self, FontFace::LatinModernBold | FontFace::AndikaBold)
    }

    /// Whether the face is looked into for `c`. Andika is not, for the Greek and Coptic block
    /// (U+0370–U+03FF): its Greek is the few letters of linguistics (no ε, η, ν, τ nor Σ), and a
    /// word mixing its letters with Latin Modern Math's would mix two designs, so a frame takes
    /// all of its Greek from Latin Modern Math. The text field registers Andika's files with the
    /// same range left out (`unicodeRange` in `app/ui/src/lib/text.ts`), so that the browser
    /// falls back as this does.
    pub fn serves(self, c: char) -> bool {
        match self {
            FontFace::Andika | FontFace::AndikaBold => !GREEK_BLOCK.contains(&u32::from(c)),
            _ => true,
        }
    }
}

/// The Greek and Coptic block, which Andika does not serve ([`FontFace::serves`]).
pub const GREEK_BLOCK: std::ops::RangeInclusive<u32> = 0x0370..=0x03FF;

/// The faces a frame of `family` in that weight takes a character from, in order: the first that
/// serves it and has a glyph for it. Its own face; then Latin Modern Math, the companion of Latin
/// Modern Roman by the same foundry (all of Greek, ϵ ϑ ϕ ϖ ϱ, the mathematical symbols and
/// arrows); then the other family (Andika has the superscript and subscript digits Latin Modern
/// lacks). The editor's text field lists its CSS families in the same order (`textFieldStyle`),
/// so that the browser takes each character from the same file, with the same advance.
pub fn fallback_chain(family: FontFamily, bold: bool) -> [FontFace; 3] {
    [
        FontFace::of(family, bold),
        FontFace::LatinModernMath,
        FontFace::of(family.other(), bold),
    ]
}

/// One of the embedded font files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFile {
    pub face: FontFace,
    /// Its name in `fonts/` (the name it is distributed under).
    pub file_name: &'static str,
    pub bytes: &'static [u8],
}

/// The embedded files, in the order of [`FontFace::ALL`].
pub const FONT_FILES: [FontFile; 5] = [
    FontFile {
        face: FontFace::LatinModern,
        file_name: "lmroman10-regular.otf",
        bytes: include_bytes!("../fonts/lmroman10-regular.otf"),
    },
    FontFile {
        face: FontFace::LatinModernBold,
        file_name: "lmroman10-bold.otf",
        bytes: include_bytes!("../fonts/lmroman10-bold.otf"),
    },
    FontFile {
        face: FontFace::Andika,
        file_name: "Andika-Regular.ttf",
        bytes: include_bytes!("../fonts/Andika-Regular.ttf"),
    },
    FontFile {
        face: FontFace::AndikaBold,
        file_name: "Andika-Bold.ttf",
        bytes: include_bytes!("../fonts/Andika-Bold.ttf"),
    },
    FontFile {
        face: FontFace::LatinModernMath,
        file_name: "latinmodern-math.otf",
        bytes: include_bytes!("../fonts/latinmodern-math.otf"),
    },
];

/// The file of `face`.
pub fn font_file(face: FontFace) -> &'static FontFile {
    &FONT_FILES[face.code() as usize]
}

/// The parsed faces, once for the life of the program.
fn faces() -> &'static [Option<Face<'static>>; 5] {
    static FACES: OnceLock<[Option<Face<'static>>; 5]> = OnceLock::new();
    FACES.get_or_init(|| FONT_FILES.map(|file| Face::parse(file.bytes, 0).ok()))
}

/// The parsed `face`; `None` only if an embedded file could not be parsed, which the tests rule
/// out.
pub(crate) fn face(face: FontFace) -> Option<&'static Face<'static>> {
    faces()[face.code() as usize].as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_file_parses_and_the_families_have_the_characters_french_needs() {
        for file in FONT_FILES {
            let parsed = face(file.face).expect(file.file_name);
            assert!(parsed.units_per_em() > 0, "{}", file.file_name);
            if file.face == FontFace::LatinModernMath {
                continue;
            }
            for c in "AZaz09 éèêëàâäîïôöùûüçÉÈÊÀÇœŒæÆ«»€’‘“”–—…°×÷±".chars()
            {
                assert!(
                    parsed.glyph_index(c).is_some_and(|g| g.0 != 0),
                    "{} lacks {c:?} (U+{:04X})",
                    file.file_name,
                    c as u32
                );
            }
        }
    }

    #[test]
    fn the_files_are_in_the_order_of_their_faces() {
        for (k, face) in FontFace::ALL.into_iter().enumerate() {
            assert_eq!(FONT_FILES[k].face, face);
            assert_eq!(face.code() as usize, k);
            assert_eq!(FontFace::from_code(face.code()), Some(face));
            assert_eq!(font_file(face).face, face);
        }
        assert_eq!(FontFace::from_code(5), None);
        for family in FontFamily::ALL {
            for bold in [false, true] {
                assert_eq!(FontFace::of(family, bold).is_bold(), bold);
                let chain = fallback_chain(family, bold);
                assert_eq!(chain[0], FontFace::of(family, bold));
                assert_eq!(chain[1], FontFace::LatinModernMath);
                assert_eq!(chain[2], FontFace::of(family.other(), bold));
            }
        }
    }

    #[test]
    fn andika_does_not_serve_greek() {
        for face in [FontFace::Andika, FontFace::AndikaBold] {
            assert!(!face.serves('α') && !face.serves('Ω') && !face.serves('ϵ'));
            assert!(face.serves('µ') && face.serves('\u{2126}') && face.serves('a'));
        }
        assert!(FontFace::LatinModern.serves('α') && FontFace::LatinModernMath.serves('α'));
    }

    #[test]
    fn codes_round_trip() {
        for family in FontFamily::ALL {
            assert_eq!(FontFamily::from_code(family.code()), family);
            assert_eq!(family.other().other(), family);
        }
        assert_eq!(FontFamily::from_code(99), FontFamily::LatinModern);
    }

    #[test]
    fn the_files_are_the_ones_recorded_in_the_readme() {
        // The README gives each file's name and size; a file replaced without updating it fails here
        // (its SHA-256 is checked by `cargo xtask licenses`).
        let readme = include_str!("../fonts/README.md");
        for file in FONT_FILES {
            assert!(
                readme.contains(file.file_name),
                "{} is not in fonts/README.md",
                file.file_name
            );
            assert!(
                readme.contains(&file.bytes.len().to_string()),
                "the size of {} ({}) is not in fonts/README.md",
                file.file_name,
                file.bytes.len()
            );
        }
    }
}
