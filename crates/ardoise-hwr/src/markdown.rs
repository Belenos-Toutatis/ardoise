//! A note (or several) as Markdown: the title as a heading, the date and the tags, then the
//! paragraphs, the list items and the drawings as images saved in a folder next to the `.md` file.

use crate::document::{Block, Document, Labels, Mark, Text};

/// A drawing's PNG file, to be written next to the Markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownImage {
    /// File name, without folder (`prefix-1.png`); the file goes in the folder given to
    /// `to_markdown`.
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
    guard_block_start(escape_inline(text))
}

/// `text` with a backslash before each character Markdown would take for inline formatting.
fn escape_inline(text: &str) -> String {
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
    out
}

/// At the start of a block, a mark that would make a heading, a list or a quote is escaped.
fn guard_block_start(mut out: String) -> String {
    let digits = out.chars().take_while(char::is_ascii_digit).count();
    let numbered = digits > 0 && matches!(out[digits..].chars().next(), Some('.' | ')'));
    if numbered {
        out.insert(digits, '\\');
    } else if out.starts_with(['#', '+', '-', '=']) {
        out.insert(0, '\\');
    }
    out
}

/// A paragraph's or a list item's text, its marked words wrapped: strong emphasis for the
/// underlined ones (Markdown has no underline, and a hand underline marks what matters), `~~`
/// for the struck ones. Spans are separated by spaces, so each delimiter run opens or closes.
pub fn inline(text: &Text) -> String {
    let parts: Vec<String> = text
        .spans
        .iter()
        .filter(|span| !span.text.trim().is_empty())
        .map(|span| {
            let escaped = escape_inline(span.text.trim());
            match span.mark {
                Mark::None => escaped,
                Mark::Underline => format!("**{escaped}**"),
                Mark::Strike => format!("~~{escaped}~~"),
            }
        })
        .collect();
    guard_block_start(parts.join(" "))
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

/// The target of a link to `name` inside `folder` (a folder name next to the Markdown file; empty
/// for the same folder): both parts percent-encoded.
fn image_link(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        link_target(name)
    } else {
        format!("{}/{}", link_target(folder), link_target(name))
    }
}

/// The Markdown of `documents`, one note after the other (a rule between two notes). The
/// drawings are `prefix-1.png`, `prefix-2.png`… over all the notes, linked as lying in `folder`
/// (a name next to the Markdown file, empty for the same folder). Lines end with `\n`.
pub fn to_markdown(
    documents: &[Document],
    labels: &Labels,
    prefix: &str,
    folder: &str,
) -> Markdown {
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
                    text.push_str(&format!("{}\n\n", inline(paragraph)));
                }
                Block::ListItem(line) => text.push_str(&format!("- {}\n", inline(line))),
                Block::Drawing(drawing) => {
                    let number = images.len() + 1;
                    let name = format!("{prefix}-{number}.png");
                    text.push_str(&format!(
                        "![{} {number}]({})\n\n",
                        escape(&labels.drawing),
                        image_link(folder, &name)
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
    fn marked_words_are_emphasised_or_struck() {
        let text = Text::from_words([
            ("la", Mark::None),
            ("lumière", Mark::Underline),
            ("se", Mark::Underline),
            ("*propage*", Mark::None),
            ("en", Mark::Strike),
        ]);
        assert_eq!(inline(&text), r"la **lumière se** \*propage\* ~~en~~");
        // A block that starts with a list or heading mark is still escaped, a marked one needs
        // nothing more.
        assert_eq!(inline(&Text::plain("- tiret")), r"\- tiret");
        assert_eq!(inline(&Text::plain("12. douze")), r"12\. douze");
        let marked = Text::from_words([("1.", Mark::Underline), ("un", Mark::None)]);
        assert_eq!(inline(&marked), "**1.** un");
        assert_eq!(inline(&Text::default()), "");
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
        assert_eq!(image_link("", "a b.png"), "a%20b.png");
        assert_eq!(
            image_link("a b_images (2)", "a b-1.png"),
            "a%20b_images%20%282%29/a%20b-1.png"
        );
    }

    #[test]
    fn a_note_becomes_a_heading_a_header_a_list_and_images() {
        let markdown = to_markdown(&[sample()], &labels(), "notes", "");
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
        let markdown = to_markdown(&[sample(), plain], &labels(), "x", "");
        assert_eq!(markdown.images.len(), 2);
        assert_eq!(markdown.images[1].name, "x-2.png");
        let in_folder = to_markdown(&[sample()], &labels(), "x", "x_images");
        assert!(in_folder.text.contains("![Drawing 1](x_images/x-1.png)"));
        assert_eq!(in_folder.images[0].name, "x-1.png");
        assert!(markdown.text.contains("\n---\n\n# Deux\n\nPremier"));
        assert!(markdown.text.contains("![Drawing 2](x-2.png)"));
        assert!(markdown.text.ends_with("fin\n"));
        // A note with a header but no body ends after its tags.
        let empty = Document {
            blocks: Vec::new(),
            ..sample()
        };
        let text = to_markdown(&[empty], &labels(), "x", "").text;
        assert!(text.ends_with("Tags: école, classe 3e\n"), "{text:?}");
        assert_eq!(to_markdown(&[], &labels(), "x", "").text, "\n");
    }
}
