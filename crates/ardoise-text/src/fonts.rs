//! The four font files shipped with Ardoise, embedded in the binary (provenance, versions,
//! licences and SHA-256: `fonts/README.md`).

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

    /// The family a character missing from this one is taken from.
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

/// One of the embedded font files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFile {
    pub family: FontFamily,
    pub bold: bool,
    /// Its name in `fonts/` (the name it is distributed under).
    pub file_name: &'static str,
    pub bytes: &'static [u8],
}

/// The embedded files, in the order of [`slot`].
pub const FONT_FILES: [FontFile; 4] = [
    FontFile {
        family: FontFamily::LatinModern,
        bold: false,
        file_name: "lmroman10-regular.otf",
        bytes: include_bytes!("../fonts/lmroman10-regular.otf"),
    },
    FontFile {
        family: FontFamily::LatinModern,
        bold: true,
        file_name: "lmroman10-bold.otf",
        bytes: include_bytes!("../fonts/lmroman10-bold.otf"),
    },
    FontFile {
        family: FontFamily::Andika,
        bold: false,
        file_name: "Andika-Regular.ttf",
        bytes: include_bytes!("../fonts/Andika-Regular.ttf"),
    },
    FontFile {
        family: FontFamily::Andika,
        bold: true,
        file_name: "Andika-Bold.ttf",
        bytes: include_bytes!("../fonts/Andika-Bold.ttf"),
    },
];

fn slot(family: FontFamily, bold: bool) -> usize {
    family.code() as usize * 2 + usize::from(bold)
}

/// The file of `family` in that weight.
pub fn font_file(family: FontFamily, bold: bool) -> &'static FontFile {
    &FONT_FILES[slot(family, bold)]
}

/// The parsed faces, once for the life of the program.
fn faces() -> &'static [Option<Face<'static>>; 4] {
    static FACES: OnceLock<[Option<Face<'static>>; 4]> = OnceLock::new();
    FACES.get_or_init(|| FONT_FILES.map(|file| Face::parse(file.bytes, 0).ok()))
}

/// The face of `family` in that weight; `None` only if an embedded file could not be parsed,
/// which the tests rule out.
pub(crate) fn face(family: FontFamily, bold: bool) -> Option<&'static Face<'static>> {
    faces()[slot(family, bold)].as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_file_parses_and_has_the_characters_french_needs() {
        for file in FONT_FILES {
            let face = face(file.family, file.bold).expect(file.file_name);
            assert!(face.units_per_em() > 0, "{}", file.file_name);
            for c in "AZaz09 éèêëàâäîïôöùûüçÉÈÊÀÇœŒæÆ«»€’‘“”–—…°×÷±".chars()
            {
                assert!(
                    face.glyph_index(c).is_some_and(|g| g.0 != 0),
                    "{} lacks {c:?} (U+{:04X})",
                    file.file_name,
                    c as u32
                );
            }
        }
    }

    #[test]
    fn andika_has_the_greek_letters_of_science_for_latin_modern_to_fall_back_on() {
        // Andika's Greek is the one of linguistics: neither ε, η, ν, τ nor Σ (U+03A3), for instance.
        for bold in [false, true] {
            let face = face(FontFamily::Andika, bold).unwrap();
            for c in "αβγδθλμπρσφχωΔΩ∞√∑→≈≠≤≥∂∫".chars() {
                assert!(face.glyph_index(c).is_some(), "Andika lacks {c:?}");
            }
        }
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
