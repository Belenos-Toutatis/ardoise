//! A note (or several) as Markdown: the title as a heading, the date and the tags, then the
//! paragraphs, the list items and the drawings as images saved next to the `.md` file.

use crate::document::{Block, Document, Labels};

/// A drawing's PNG file, to be written next to the Markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownImage {
    /// File name, without folder (`prefix-1.png`).
    pub name: String,
    pub png: Vec<u8>,
}

/// The Markdown text and the image files it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markdown {
    pub text: String,
    pub images: Vec<MarkdownImage>,
}

/// Escapes what Markdown would take for formatting in running text.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~' | '|' | '&'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    // At the start of a block, a mark that would make a heading, a list or a quote.
    let digits = out.chars().take_while(char::is_ascii_digit).count();
    let numbered = digits > 0 && matches!(out[digits..].chars().next(), Some('.' | ')'));
    if numbered {
        out.insert(digits, '\\');
    } else if out.starts_with(['#', '+', '-', '=']) {
        out.insert(0, '\\');
    }
    out
}

/// A file name as the target of a link: spaces, parentheses and the other characters that would
/// end or break the link are percent-encoded.
fn link_target(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~') || !c.is_ascii() {
            out.push(c);
        } else {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// The Markdown of `documents`, one note after the other (a rule between two notes). The
/// drawings are `prefix-1.png`, `prefix-2.png`… over all the notes. Lines end with `\n`.
pub fn to_markdown(documents: &[Document], labels: &Labels, prefix: &str) -> Markdown {
    let mut text = String::new();
    let mut images = Vec::new();
    for (index, document) in documents.iter().enumerate() {
        if index > 0 {
            text.push_str("\n---\n\n");
        }
        text.push_str(&format!("# {}\n\n", escape(document.title.trim())));
        if !document.date.is_empty() {
            // Two trailing spaces: a line break inside the paragraph.
            let end = if document.tags.is_empty() { "" } else { "  " };
            text.push_str(&format!(
                "{} {}{end}\n",
                escape(&labels.date),
                escape(&document.date)
            ));
        }
        if !document.tags.is_empty() {
            let tags: Vec<String> = document.tags.iter().map(|tag| escape(tag)).collect();
            text.push_str(&format!("{} {}\n", escape(&labels.tags), tags.join(", ")));
        }
        if !document.date.is_empty() || !document.tags.is_empty() {
            text.push('\n');
        }
        let mut previous_item = false;
        for block in &document.blocks {
            let item = matches!(block, Block::ListItem(_));
            if previous_item && !item {
                text.push('\n');
            }
            match block {
                Block::Paragraph(paragraph) => {
                    text.push_str(&format!("{}\n\n", escape(paragraph)));
                }
                Block::ListItem(line) => text.push_str(&format!("- {}\n", escape(line))),
                Block::Drawing(drawing) => {
                    let number = images.len() + 1;
                    let name = format!("{prefix}-{number}.png");
                    text.push_str(&format!(
                        "![{} {number}]({})\n\n",
                        escape(&labels.drawing),
                        link_target(&name)
                    ));
                    images.push(MarkdownImage {
                        name,
                        png: drawing.png.clone(),
                    });
                }
            }
            previous_item = item;
        }
        if previous_item {
            text.push('\n');
        }
    }
    // One blank line at most at the end.
    let trimmed = text.trim_end_matches('\n').len();
    text.truncate(trimmed);
    text.push('\n');
    Markdown { text, images }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::{labels, sample};

    #[test]
    fn running_text_is_escaped() {
        assert_eq!(escape("a*b_c [d] <e>"), r"a\*b\_c \[d\] \<e\>");
        assert_eq!(escape("# titre"), r"\# titre");
        assert_eq!(escape("- tiret"), r"\- tiret");
        assert_eq!(escape("12. douze"), r"12\. douze");
        assert_eq!(escape("3 pommes"), "3 pommes");
        assert_eq!(escape("é à ç 2 + 2"), "é à ç 2 + 2");
    }

    #[test]
    fn link_targets_are_percent_encoded() {
        assert_eq!(link_target("notes-1.png"), "notes-1.png");
        assert_eq!(
            link_target("Mon journal (1)-2.png"),
            "Mon%20journal%20%281%29-2.png"
        );
        assert_eq!(link_target("café.png"), "café.png");
        assert_eq!(link_target("a#b?c.png"), "a%23b%3Fc.png");
    }

    #[test]
    fn a_note_becomes_a_heading_a_header_a_list_and_images() {
        let markdown = to_markdown(&[sample()], &labels(), "notes");
        let expected = "\
# Réunion \\& \\*plan\\* #1

Created: 2026-10-22 14:05<br>
Tags: école, classe 3e

Premier paragraphe avec\\_des \\[signes\\] \\<spéciaux\\>.

- pommes
- \\- poires

![Drawing 1](notes-1.png)

1\\. fin
"
        // Two spaces at the end of a line are a line break.
        .replace("<br>", "  ");
        assert_eq!(markdown.text, expected);
        assert_eq!(markdown.images.len(), 1);
        assert_eq!(markdown.images[0].name, "notes-1.png");
        assert_eq!(&markdown.images[0].png[1..4], b"PNG");
    }

    #[test]
    fn several_notes_are_separated_and_their_drawings_numbered_in_a_row() {
        let mut plain = sample();
        plain.date.clear();
        plain.tags.clear();
        plain.title = "Deux".into();
        let markdown = to_markdown(&[sample(), plain], &labels(), "x");
        assert_eq!(markdown.images.len(), 2);
        assert_eq!(markdown.images[1].name, "x-2.png");
        assert!(markdown.text.contains("\n---\n\n# Deux\n\nPremier"));
        assert!(markdown.text.contains("![Drawing 2](x-2.png)"));
        assert!(markdown.text.ends_with("fin\n"));
        // A note with a header but no body ends after its tags.
        let empty = Document {
            blocks: Vec::new(),
            ..sample()
        };
        let text = to_markdown(&[empty], &labels(), "x").text;
        assert!(text.ends_with("Tags: école, classe 3e\n"), "{text:?}");
        assert_eq!(to_markdown(&[], &labels(), "x").text, "\n");
    }
}
