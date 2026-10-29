//! A note's recognised content as a structured document: title, date and tags, then paragraphs,
//! list items and drawings (cropped images). Platform-free; [`crate::markdown`] and
//! [`crate::docx`] write it. The structure itself comes from [`crate::structure`].

use std::fmt;

/// A line drawn by hand on words: never a drawing of its own, it decorates the words it covers
/// (see `structure`, "Underlines").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    #[default]
    None,
    /// Under the words: Word underlines them, Markdown gives them strong emphasis (`**…**`).
    Underline,
    /// Through the words: Word strikes them, Markdown `~~…~~`.
    Strike,
}

/// Consecutive words with the same mark, separated by single spaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub mark: Mark,
}

/// The text of a paragraph or a list item: its spans, separated by single spaces. Displayed, it
/// is the plain text (the marks left out).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Text {
    pub spans: Vec<Span>,
}

impl Text {
    /// Text without marks (nothing when `text` is blank).
    pub fn plain(text: &str) -> Text {
        Text::from_words(text.split_whitespace().map(|word| (word, Mark::None)))
    }

    /// The words with their marks, in order: consecutive words with the same mark share a span,
    /// empty words are left out.
    pub fn from_words<'a>(words: impl IntoIterator<Item = (&'a str, Mark)>) -> Text {
        let mut spans: Vec<Span> = Vec::new();
        for (word, mark) in words {
            let word = word.trim();
            if word.is_empty() {
                continue;
            }
            match spans.last_mut() {
                Some(span) if span.mark == mark => {
                    span.text.push(' ');
                    span.text.push_str(word);
                }
                _ => spans.push(Span {
                    text: word.to_owned(),
                    mark,
                }),
            }
        }
        Text { spans }
    }

    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|span| span.text.trim().is_empty())
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, span) in self.spans.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            f.write_str(&span.text)?;
        }
        Ok(())
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Text {
        Text::plain(text)
    }
}

/// One block of a note's body, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A paragraph: its lines joined by single spaces.
    Paragraph(Text),
    /// A list item (its bullet or number mark removed). Consecutive items make one list.
    ListItem(Text),
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
    fn text_keeps_its_marks_in_spans() {
        let text = Text::from_words([
            ("la", Mark::None),
            ("lumière", Mark::None),
            ("en", Mark::Underline),
            (" ", Mark::Strike),
            ("ligne", Mark::Underline),
            ("droite", Mark::Strike),
        ]);
        assert_eq!(
            text.spans,
            [
                Span {
                    text: "la lumière".into(),
                    mark: Mark::None
                },
                Span {
                    text: "en ligne".into(),
                    mark: Mark::Underline
                },
                Span {
                    text: "droite".into(),
                    mark: Mark::Strike
                },
            ]
        );
        assert_eq!(text.to_string(), "la lumière en ligne droite");
        assert_eq!(Text::plain("  deux   mots "), Text::from("deux mots"));
        assert!(Text::plain(" ").is_empty());
        assert!(Text::default().is_empty());
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
