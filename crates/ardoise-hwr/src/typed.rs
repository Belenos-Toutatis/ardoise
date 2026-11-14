//! The text typed on the page in the text exports: the text frames and the equations (page
//! objects, SPEC §4.4), which Windows' recognition never reads and which the user wants in the
//! `.txt`, Markdown and Word exports, the clipboard and what the assistant reads. Platform-free.
//!
//! - [`typed_blocks`]: the frames and equations of the visible layers, in reading order;
//! - [`crate::structure::Structure::with_typed`]: put among the handwriting's items (Markdown, Word);
//! - [`document_blocks`]: a block as the blocks of a [`crate::document::Document`];
//! - [`interleaved_text`]: the plain text (`.txt`, clipboard, preview): the handwriting's lines
//!   with the blocks among them.
//!
//! Where a block goes among the handwriting is the rule of the drawings
//! ([`crate::structure::comes_after`]): after the last item or line it comes after, beside one
//! on its right, below it otherwise.

use ardoise_core::note::{Note, ObjectBody};

use crate::document::{Block, Text, equation_line};
use crate::model::{Bounds, Line, Recognition};
use crate::structure::{
    BESIDE_OVERLAP, Corrected, comes_after, lay_correction, overlap, strip_bullet,
};

/// What a [`TypedBlock`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKind {
    /// A text frame: the user's own line breaks (the wrapping of a frame is display only).
    Text,
    /// An equation: its LaTeX source.
    Equation,
}

/// A text frame or an equation of the page, as the text exports read it.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedBlock {
    pub kind: TypedKind,
    /// The frame's text, or the equation's source, as typed (never blank).
    pub text: String,
    /// The object's box on the page, in page units.
    pub bounds: Bounds,
}

impl TypedBlock {
    /// The lines of the block as plain text: a frame's lines as the user typed them (no
    /// trailing spaces, no blank line at either end), an equation's source on one line
    /// ([`equation_line`]).
    pub fn lines(&self) -> Vec<String> {
        match self.kind {
            TypedKind::Equation => vec![equation_line(&self.text)],
            TypedKind::Text => corrected_text(&self.text)
                .lines()
                .map(str::to_owned)
                .collect(),
        }
    }
}

/// The text frames and equations of the visible layers of `note` (hidden layers are left out, as
/// the recognised handwriting of a hidden layer is; a bookmark is never text, nor is an object of
/// a kind this version does not know), those with nothing but white space aside, in reading order:
/// by rows, top to bottom, each row left to right. A block joins the row above when it overlaps
/// its band vertically by at least [`BESIDE_OVERLAP`] of the smaller height: two frames side by
/// side read left then right even when the left one starts a little lower.
pub fn typed_blocks(note: &Note) -> Vec<TypedBlock> {
    let blocks = note
        .objects
        .iter()
        .filter(|object| note.layer_visible(object.layer))
        .filter_map(|object| {
            let (kind, text) = match &object.body {
                ObjectBody::Text(body) => (TypedKind::Text, &body.text),
                ObjectBody::Equation(body) => (TypedKind::Equation, &body.latex),
                ObjectBody::Bookmark(_) | ObjectBody::Unknown { .. } => return None,
            };
            if text.trim().is_empty() {
                return None;
            }
            let [left, top, right, bottom] = object.page_bounds();
            Some(TypedBlock {
                kind,
                text: text.clone(),
                bounds: Bounds {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                },
            })
        })
        .collect();
    in_rows(blocks)
}

/// A band of the page and the blocks that share it.
struct Row {
    top: f32,
    bottom: f32,
    blocks: Vec<TypedBlock>,
}

/// `blocks` by rows (see [`typed_blocks`]). Total and deterministic: the rows are built first,
/// then each row is sorted by position (equal positions keep the order of the page's objects).
fn in_rows(mut blocks: Vec<TypedBlock>) -> Vec<TypedBlock> {
    blocks.sort_by(|a, b| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
    });
    let mut rows: Vec<Row> = Vec::new();
    for block in blocks {
        let (top, bottom) = (block.bounds.y, block.bounds.bottom());
        let joins = rows.last().is_some_and(|row| {
            let shared = overlap(top, bottom, row.top, row.bottom);
            let smaller = (bottom - top).min(row.bottom - row.top);
            shared > 0.0 && shared >= BESIDE_OVERLAP * smaller
        });
        match rows.last_mut() {
            Some(row) if joins => {
                row.top = row.top.min(top);
                row.bottom = row.bottom.max(bottom);
                row.blocks.push(block);
            }
            _ => rows.push(Row {
                top,
                bottom,
                blocks: vec![block],
            }),
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.blocks.sort_by(|a, b| {
                a.bounds
                    .x
                    .total_cmp(&b.bounds.x)
                    .then(a.bounds.y.total_cmp(&b.bounds.y))
            });
            row.blocks
        })
        .collect()
}

/// `block` as the blocks of a document. A frame: each of its non-empty lines is a block of its
/// own (the user's line breaks are kept), a list item when the line starts with a dash, a bullet
/// or a number (which is then left out), else a paragraph. An equation: one [`Block::Equation`].
pub fn document_blocks(block: &TypedBlock) -> Vec<Block> {
    match block.kind {
        TypedKind::Equation => vec![Block::Equation(equation_line(&block.text))],
        TypedKind::Text => block
            .lines()
            .iter()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let (item, rest) = match strip_bullet(line) {
                    Some(rest) => (true, rest),
                    None => (false, line.as_str()),
                };
                let text = Text::plain(rest);
                if text.is_empty() {
                    None
                } else if item {
                    Some(Block::ListItem(text))
                } else {
                    Some(Block::Paragraph(text))
                }
            })
            .collect(),
    }
}

/// A corrected transcription as text: lines without trailing spaces or `\r`, no blank line at
/// either end.
pub fn corrected_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_matches('\n').to_owned()
}

/// A line of the handwriting's text (or a group of them) and where it is on the page.
struct Entry {
    /// The lines of text, blank ones of a correction included.
    lines: Vec<String>,
    bounds: Option<Bounds>,
}

/// The handwriting as entries: the recognised lines; or the user's correction, laid over them
/// ([`lay_correction`]) when it has as many lines, so that each keeps the place of its recognised
/// line (a blank line of the correction stays with the line before it); else one entry whose
/// box encloses the recognised lines (none: no box at all, as when nothing can be recognised).
fn entries(recognition: &Recognition, correction: Option<&str>) -> Vec<Entry> {
    let recognised: Vec<&Line> = recognition
        .lines
        .iter()
        .filter(|line| !line.words.is_empty())
        .collect();
    let Some(correction) = correction else {
        return recognised
            .iter()
            .map(|line| Entry {
                lines: vec![line.text()],
                bounds: Some(line.bounds),
            })
            .collect();
    };
    let text = corrected_text(correction);
    if text.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    match lay_correction(recognition, &text) {
        Corrected::Placed(placed) => {
            let mut placed = placed.iter();
            let mut entries: Vec<Entry> = Vec::new();
            for line in lines {
                match entries.last_mut() {
                    Some(last) if line.trim().is_empty() => last.lines.push(String::new()),
                    _ => entries.push(Entry {
                        lines: vec![line.to_owned()],
                        bounds: placed.next().map(|line| line.bounds),
                    }),
                }
            }
            entries
        }
        Corrected::Unplaced(_) => vec![Entry {
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            bounds: Bounds::enclosing(recognised.iter().map(|line| &line.bounds)),
        }],
    }
}

/// The plain text of a note: the handwriting line after line (the user's `correction` of it when
/// there is one, else the lines of `recognition`) with the `typed` blocks among the lines, each
/// after the last line it comes after ([`comes_after`]; after the blank lines of a correction that
/// follow it), before everything when it comes after none, a frame with its own lines and an
/// equation with its source on one line. Blocks that fall in the same place keep their order.
/// Without any place for the handwriting (nothing was recognised, so only a correction has the
/// words), the typed text follows it; without handwriting, the typed text is the text.
pub fn interleaved_text(
    recognition: &Recognition,
    correction: Option<&str>,
    typed: &[TypedBlock],
) -> String {
    let entries = entries(recognition, correction);
    let placed = entries.iter().any(|entry| entry.bounds.is_some());
    let mut slots: Vec<Vec<&TypedBlock>> = vec![Vec::new(); entries.len() + 1];
    for block in typed {
        let slot = if placed {
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry
                        .bounds
                        .is_some_and(|bounds| comes_after(&block.bounds, &bounds))
                })
                .map(|(index, _)| index + 1)
                .max()
                .unwrap_or(0)
        } else {
            entries.len()
        };
        slots[slot].push(block);
    }
    let mut out: Vec<String> = Vec::new();
    let mut slots = slots.into_iter();
    let add = |blocks: Vec<&TypedBlock>, out: &mut Vec<String>| {
        for block in blocks {
            out.extend(block.lines());
        }
    };
    add(slots.next().unwrap_or_default(), &mut out);
    for entry in entries {
        out.extend(entry.lines);
        add(slots.next().unwrap_or_default(), &mut out);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use ardoise_core::note::{
        A4_LANDSCAPE, Background, EquationBody, FontFamily, Layer, Origin, PageObject, TextBody,
        Timing,
    };
    use ardoise_ink::Rgba;

    use super::*;
    use crate::Language;
    use crate::document::Document;
    use crate::document::tests::labels;
    use crate::markdown::to_markdown;
    use crate::model::tests::word;
    use crate::structure::{Item, Structure};

    fn note(objects: Vec<PageObject>) -> Note {
        Note {
            id: "n".into(),
            title: String::new(),
            created: 0,
            modified: 0,
            page: A4_LANDSCAPE,
            background: Background::default(),
            background_image: None,
            tags: Vec::new(),
            favorite: false,
            layers: vec![
                Layer {
                    name: "1".into(),
                    visible: true,
                },
                Layer {
                    name: "2".into(),
                    visible: false,
                },
            ],
            strokes: Vec::new(),
            objects,
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
    }

    /// A frame at (x, y), `w` by `h` (the box is set, not measured).
    fn frame(id: &str, layer: u32, (x, y, w, h): (f32, f32, f32, f32), text: &str) -> PageObject {
        let mut object = PageObject::text(
            id,
            layer,
            0,
            x,
            y,
            TextBody {
                text: text.into(),
                font: FontFamily::Andika,
                bold: false,
                size: 400.0,
                color: Rgba::BLACK,
                wrap_width: None,
            },
        );
        (object.width, object.height) = (w, h);
        object
    }

    fn equation(
        id: &str,
        layer: u32,
        (x, y, w, h): (f32, f32, f32, f32),
        latex: &str,
    ) -> PageObject {
        let mut object = PageObject::equation(
            id,
            layer,
            0,
            [x, y],
            1.0,
            EquationBody {
                latex: latex.into(),
                path: String::new(),
                color: Rgba::BLACK,
            },
        );
        (object.width, object.height) = (w, h);
        object
    }

    fn texts_of(blocks: &[TypedBlock]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    fn block(kind: TypedKind, text: &str, (x, y, w, h): (f32, f32, f32, f32)) -> TypedBlock {
        TypedBlock {
            kind,
            text: text.into(),
            bounds: Bounds {
                x,
                y,
                width: w,
                height: h,
            },
        }
    }

    fn frame_block(text: &str, at: (f32, f32, f32, f32)) -> TypedBlock {
        block(TypedKind::Text, text, at)
    }

    #[test]
    fn only_visible_frames_and_equations_with_something_in_them_are_typed_blocks() {
        let page = note(vec![
            frame("a", 0, (0.0, 0.0, 1000.0, 300.0), "Titre"),
            frame("hidden", 1, (0.0, 500.0, 1000.0, 300.0), "Réponse cachée"),
            frame("blank", 0, (0.0, 900.0, 1000.0, 300.0), " \n\t "),
            equation("e", 0, (0.0, 1500.0, 800.0, 400.0), "x^2"),
            equation("hidden-e", 1, (0.0, 2500.0, 800.0, 400.0), "y"),
            equation("blank-e", 0, (0.0, 3500.0, 800.0, 400.0), "  "),
            PageObject::bookmark("b", 0, 0, 0.0, 4500.0, "Signet"),
        ]);
        let blocks = typed_blocks(&page);
        assert_eq!(texts_of(&blocks), ["Titre", "x^2"]);
        assert_eq!(blocks[0].kind, TypedKind::Text);
        assert_eq!(blocks[1].kind, TypedKind::Equation);
        // The box is the object's, on the page.
        assert_eq!(
            blocks[1].bounds,
            Bounds {
                x: 0.0,
                y: 1500.0,
                width: 800.0,
                height: 400.0
            }
        );
        assert!(typed_blocks(&note(Vec::new())).is_empty());
    }

    #[test]
    fn blocks_are_read_by_rows_left_to_right() {
        let page = note(vec![
            frame("bottom", 0, (0.0, 5000.0, 1000.0, 300.0), "bas"),
            // Side by side: the left one starts a little lower, it still reads first.
            frame("right", 0, (4000.0, 1000.0, 1000.0, 300.0), "droite"),
            frame("left", 0, (0.0, 1100.0, 1000.0, 300.0), "gauche"),
            frame("top", 0, (9000.0, 0.0, 1000.0, 300.0), "haut"),
            // Barely overlapping the row above (under a third of the height): the next row.
            frame("next", 0, (0.0, 1350.0, 1000.0, 300.0), "suite"),
        ]);
        assert_eq!(
            texts_of(&typed_blocks(&page)),
            ["haut", "gauche", "droite", "suite", "bas"]
        );
        // Same boxes: the order of the page's objects, always the same.
        let same = note(vec![
            frame("1", 0, (0.0, 0.0, 100.0, 100.0), "un"),
            frame("2", 0, (0.0, 0.0, 100.0, 100.0), "deux"),
            frame("3", 0, (0.0, 0.0, 100.0, 100.0), "trois"),
        ]);
        assert_eq!(texts_of(&typed_blocks(&same)), ["un", "deux", "trois"]);
    }

    /// A paragraph item of the handwriting at (x, y), `w` by `h`.
    fn paragraph(text: &str, (x, y, w, h): (f32, f32, f32, f32)) -> (Item, Bounds) {
        (
            Item::Paragraph(text.into()),
            Bounds {
                x,
                y,
                width: w,
                height: h,
            },
        )
    }

    fn structure(items: Vec<(Item, Bounds)>) -> Structure {
        let (items, bounds) = items.into_iter().unzip();
        Structure {
            items,
            bounds,
            analyzed: true,
        }
    }

    fn shown(structure: &Structure) -> Vec<String> {
        structure
            .items
            .iter()
            .map(|item| match item {
                Item::Paragraph(text) => format!("P:{text}"),
                Item::ListItem(text) => format!("L:{text}"),
                Item::Drawing(_) => "D".to_owned(),
                Item::Typed(block) => format!("T:{}", block.text),
            })
            .collect()
    }

    #[test]
    fn typed_blocks_go_where_they_are_read_among_the_handwriting() {
        let base = || {
            structure(vec![
                paragraph("premier", (0.0, 3000.0, 4000.0, 300.0)),
                paragraph("second", (0.0, 6000.0, 4000.0, 300.0)),
            ])
        };
        // Nothing typed: exactly the handwriting's items.
        assert_eq!(base().with_typed(&[]), base());
        let with = |blocks: &[TypedBlock]| shown(&base().with_typed(blocks));
        // Above everything: first.
        let above = frame_block("titre", (0.0, 0.0, 2000.0, 300.0));
        assert_eq!(with(&[above]), ["T:titre", "P:premier", "P:second"]);
        // Beside the first paragraph, on its right: after it; on its left: before it.
        let right = frame_block("à droite", (6000.0, 3000.0, 2000.0, 300.0));
        assert_eq!(with(&[right]), ["P:premier", "T:à droite", "P:second"]);
        let left = frame_block("à gauche", (-5000.0, 3000.0, 2000.0, 300.0));
        assert_eq!(with(&[left]), ["T:à gauche", "P:premier", "P:second"]);
        // Between the two, and below both.
        let between = frame_block("milieu", (0.0, 4500.0, 2000.0, 300.0));
        let below = frame_block("bas", (0.0, 9000.0, 2000.0, 300.0));
        assert_eq!(
            with(&[between.clone(), below.clone()]),
            ["P:premier", "T:milieu", "P:second", "T:bas"]
        );
        // Two blocks in the same place keep the order they were given in.
        let other = frame_block("autre", (0.0, 4600.0, 2000.0, 300.0));
        assert_eq!(
            with(&[between, other, below]),
            ["P:premier", "T:milieu", "T:autre", "P:second", "T:bas"]
        );
        // The rectangles follow: a block carries its own.
        let placed = base().with_typed(&[frame_block("titre", (0.0, 0.0, 2000.0, 300.0))]);
        assert_eq!(placed.bounds.len(), placed.items.len());
        assert_eq!(placed.bounds[0].y, 0.0);
        assert_eq!(placed.bounds[1].y, 3000.0);
        assert!(placed.analyzed);
    }

    #[test]
    fn typed_blocks_follow_items_that_have_no_place_on_the_page() {
        // A correction nothing could be recognised for: plain paragraphs without a rectangle.
        let unplaced = structure(vec![
            paragraph("un", (0.0, 0.0, 0.0, 0.0)),
            paragraph("deux", (0.0, 0.0, 0.0, 0.0)),
        ]);
        let above = frame_block("titre", (-3000.0, -500.0, 1000.0, 300.0));
        let found = unplaced.with_typed(&[above, frame_block("bas", (0.0, 900.0, 100.0, 100.0))]);
        assert_eq!(shown(&found), ["P:un", "P:deux", "T:titre", "T:bas"]);
    }

    #[test]
    fn a_drawing_beside_a_frame_goes_by_the_same_rule() {
        let drawing = |x: f32| {
            (
                Item::Drawing(crate::structure::DrawingArea {
                    bounds: Bounds {
                        x,
                        y: 0.0,
                        width: 2000.0,
                        height: 2000.0,
                    },
                    strokes: vec![0],
                }),
                Bounds {
                    x,
                    y: 0.0,
                    width: 2000.0,
                    height: 2000.0,
                },
            )
        };
        let text = frame_block("légende", (3000.0, 500.0, 2000.0, 400.0));
        // The frame is on the right of the drawing: after it.
        let found = structure(vec![drawing(0.0)]).with_typed(std::slice::from_ref(&text));
        assert_eq!(shown(&found), ["D", "T:légende"]);
        // On its left: before it.
        let found = structure(vec![drawing(6000.0)]).with_typed(&[text]);
        assert_eq!(shown(&found), ["T:légende", "D"]);
        // Without drawings, the rectangles still match the items.
        let found = structure(vec![
            drawing(0.0),
            paragraph("p", (0.0, 3000.0, 100.0, 100.0)),
        ])
        .with_typed(&[frame_block("t", (0.0, 5000.0, 100.0, 100.0))])
        .without_drawings();
        assert_eq!(shown(&found), ["P:p", "T:t"]);
        assert_eq!(found.bounds.len(), 2);
    }

    fn document_with(blocks: Vec<Block>) -> Document {
        Document {
            title: "Note".into(),
            date: String::new(),
            tags: Vec::new(),
            blocks,
        }
    }

    fn markdown(blocks: &[TypedBlock]) -> String {
        let blocks = blocks.iter().flat_map(document_blocks).collect();
        to_markdown(&[document_with(blocks)], &labels(), "n", "").text
    }

    #[test]
    fn a_frame_gives_a_block_per_line_and_an_equation_its_source_between_dollars() {
        let frame = frame_block(
            "Bilan\n- pommes\n\n  1) poires \n• oeufs\n-5 degrés",
            (0.0, 0.0, 1000.0, 1000.0),
        );
        assert_eq!(
            document_blocks(&frame),
            [
                Block::Paragraph("Bilan".into()),
                Block::ListItem("pommes".into()),
                Block::ListItem("poires".into()),
                Block::ListItem("oeufs".into()),
                Block::Paragraph("-5 degrés".into()),
            ]
        );
        let equation = block(
            TypedKind::Equation,
            "\\ce{2H2 + O2 -> 2H2O}\n",
            (0.0, 2000.0, 1000.0, 500.0),
        );
        assert_eq!(
            document_blocks(&equation),
            [Block::Equation("\\ce{2H2 + O2 -> 2H2O}".into())]
        );
        let text = markdown(&[frame_block("Bilan\n- a", (0.0, 0.0, 1.0, 1.0)), equation]);
        assert!(
            text.ends_with("Bilan\n\n- a\n\n$\\ce{2H2 + O2 -> 2H2O}$\n"),
            "{text}"
        );
    }

    #[test]
    fn a_word_document_has_the_typed_paragraphs_and_the_equation_source() {
        let blocks = [
            frame_block("Cours", (0.0, 0.0, 1.0, 1.0)),
            block(TypedKind::Equation, "E = mc^2", (0.0, 2.0, 1.0, 1.0)),
        ];
        let document = document_with(blocks.iter().flat_map(document_blocks).collect());
        let bytes = crate::docx::to_docx(&[document], &labels()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(
            xml.contains(">Cours<") && xml.contains(">E = mc^2<"),
            "{xml}"
        );
    }

    fn page_text() -> Recognition {
        Recognition {
            language: Language::Fr,
            recognizer: String::new(),
            lines: vec![
                Line::new(vec![word("la", 0.0, 3000.0, 400.0, 300.0)]),
                Line::new(vec![word("fin", 0.0, 6000.0, 400.0, 300.0)]),
            ],
        }
    }

    #[test]
    fn the_plain_text_has_the_typed_text_among_the_recognised_lines() {
        let recognition = page_text();
        let typed = [
            frame_block("titre\n  sous-titre  ", (0.0, 0.0, 2000.0, 600.0)),
            block(TypedKind::Equation, "a =\n b", (0.0, 4500.0, 2000.0, 400.0)),
            frame_block("note", (5000.0, 6000.0, 1000.0, 300.0)),
            frame_block("dernier", (0.0, 9000.0, 1000.0, 300.0)),
        ];
        assert_eq!(
            interleaved_text(&recognition, None, &typed),
            "titre\n  sous-titre\nla\na = b\nfin\nnote\ndernier"
        );
        // Without recognition: the typed text alone.
        let nothing = Recognition::empty(Language::Fr);
        assert_eq!(
            interleaved_text(&nothing, None, &typed[..2]),
            "titre\n  sous-titre\na = b"
        );
        assert_eq!(interleaved_text(&nothing, None, &[]), "");
    }

    #[test]
    fn a_correction_keeps_the_place_of_the_lines_it_replaces() {
        let recognition = page_text();
        let typed = [
            frame_block("titre", (0.0, 0.0, 2000.0, 300.0)),
            frame_block("milieu", (0.0, 4500.0, 2000.0, 300.0)),
        ];
        // As many lines as the recognition: placed, a blank line stays with the line before it.
        assert_eq!(
            interleaved_text(&recognition, Some("Là\n\nfin du texte \r\n"), &typed),
            "titre\nLà\n\nmilieu\nfin du texte"
        );
        // Not as many lines: one block enclosing the recognised lines, the frame above goes
        // before it, the one below it after it.
        let around = [
            typed[0].clone(),
            frame_block("bas", (0.0, 7000.0, 2000.0, 300.0)),
        ];
        let unplaced = interleaved_text(&recognition, Some("une seule ligne"), &around);
        assert_eq!(unplaced, "titre\nune seule ligne\nbas");
        // The correction without any recognition: no place, the typed text follows it.
        let nothing = Recognition::empty(Language::Fr);
        assert_eq!(
            interleaved_text(&nothing, Some("mes mots\nsur deux lignes"), &typed),
            "mes mots\nsur deux lignes\ntitre\nmilieu"
        );
        // A blank correction says nothing.
        assert_eq!(
            interleaved_text(&nothing, Some(" \n"), &typed),
            "titre\nmilieu"
        );
    }
}
