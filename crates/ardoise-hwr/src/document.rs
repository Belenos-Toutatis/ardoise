//! A note's recognised content as a structured document: title, date and tags, then paragraphs,
//! list items and drawings (cropped images). Platform-free; [`crate::markdown`] and
//! [`crate::docx`] write it. The structure itself comes from [`crate::structure`].

/// One block of a note's body, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A paragraph: its lines joined by single spaces.
    Paragraph(String),
    /// A list item (its bullet or number mark removed). Consecutive items make one list.
    ListItem(String),
    /// Ink that is not text, as an image.
    Drawing(Drawing),
}

/// A drawing cropped from the note, as a PNG file.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub png: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
    /// Size on the page in millimetres (the size a Word picture takes).
    pub width_mm: f32,
    pub height_mm: f32,
}

/// One note.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub title: String,
    /// The creation date, already formatted for the reader (empty: not shown).
    pub date: String,
    pub tags: Vec<String>,
    pub blocks: Vec<Block>,
}

/// The words the files themselves contain, in the interface language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels {
    /// Before the date ("Created:", the colon included: French puts a space before it).
    pub date: String,
    /// Before the tags ("Tags:").
    pub tags: String,
    /// The alternative text of a drawing, followed by its number ("Drawing").
    pub drawing: String,
}

impl Document {
    /// Whether the body has no block at all.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// The drawings of the body, in order.
    pub fn drawings(&self) -> impl Iterator<Item = &Drawing> {
        self.blocks.iter().filter_map(|block| match block {
            Block::Drawing(drawing) => Some(drawing),
            _ => None,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn labels() -> Labels {
        Labels {
            date: "Created:".into(),
            tags: "Tags:".into(),
            drawing: "Drawing".into(),
        }
    }

    /// Bytes that look like a PNG file (the writers only copy the file).
    pub(crate) fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x7b, 0x40, 0xe8, 0xdd, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92,
            0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }

    pub(crate) fn sample() -> Document {
        Document {
            title: "Réunion & *plan* #1".into(),
            date: "2026-10-22 14:05".into(),
            tags: vec!["école".into(), "classe 3e".into()],
            blocks: vec![
                Block::Paragraph("Premier paragraphe avec_des [signes] <spéciaux>.".into()),
                Block::ListItem("pommes".into()),
                Block::ListItem("- poires".into()),
                Block::Drawing(Drawing {
                    png: tiny_png(),
                    width_px: 2,
                    height_px: 1,
                    width_mm: 60.0,
                    height_mm: 30.0,
                }),
                Block::Paragraph("1. fin".into()),
            ],
        }
    }

    #[test]
    fn a_document_lists_its_drawings() {
        let document = sample();
        assert_eq!(document.drawings().count(), 1);
        assert!(!document.is_empty());
        let empty = Document {
            blocks: Vec::new(),
            ..document
        };
        assert!(empty.is_empty());
    }
}
