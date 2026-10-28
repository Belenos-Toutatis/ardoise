//! The structure of a page of handwriting: paragraphs, list items and drawings in reading
//! order, platform-free.
//!
//! Two sources feed it, both giving the same [`Item`]s:
//!
//! - [`from_regions`]: Windows' ink analysis ([`Recognizer::analyze`](crate::Recognizer::analyze),
//!   `InkAnalyzer`) says which strokes make a paragraph, a list item or a drawing; the words
//!   come from the recognition the app already keeps (so the text is in the language the user
//!   chose, and agrees with the search), found by their position inside each region;
//! - [`from_recognition`]: layout heuristics on the recognised lines, when the analysis is not
//!   available or fails: a vertical gap bigger than a line starts a paragraph, a leading dash,
//!   bullet or number (or a small mark drawn to the left of a line) makes a list item, and ink
//!   that the recognition did not read as text, grouped by proximity, is a drawing.

use ardoise_core::note::Note;
use ardoise_ink::Tool;

use ardoise_ink::Stroke;

use crate::lines::group_lines;
use crate::model::{Bounds, Line, Recognition, Word};
use crate::{Language, Recognizer};

/// A vertical gap larger than this many line heights separates two paragraphs.
const PARAGRAPH_GAP: f32 = 1.0;
/// A small mark this many line heights wide at most, left of a line, is its bullet.
const BULLET_WIDTH: f32 = 1.6;
/// ... and this many line heights tall at most.
const BULLET_HEIGHT: f32 = 0.6;
/// ... and at most this many line heights from the start of the line.
const BULLET_DISTANCE: f32 = 2.5;
/// A line goes on a list item when it starts at least this far (in line heights) to the left
/// of the item's text at most.
const CONTINUATION_INDENT: f32 = 0.5;
/// Ink closer than this many millimetres belongs to the same drawing (page units: 10 µm).
const DRAWING_LINK: f32 = 400.0;
/// A drawing smaller than this on both sides (2 mm) is a stray mark, not a drawing.
const MIN_DRAWING: f32 = 200.0;
/// Line height when the page has no recognised text (4 mm).
const DEFAULT_LINE_HEIGHT: f32 = 400.0;

/// What the ink analysis found, in page units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Paragraph,
    ListItem,
    Drawing,
}

/// A region of the page, as the ink analysis gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub kind: RegionKind,
    pub bounds: Bounds,
    /// The analysis' own reading of the region (the fallback when the recognition has no word
    /// there).
    pub text: String,
    /// The strokes of the region: positions in the list of strokes that was analysed.
    pub strokes: Vec<usize>,
}

/// Reads again the strokes of each paragraph and list item of `regions` on their own, with the
/// recognizer of `language`, and puts the result in the region's `text`. Windows' ink analysis
/// reads with the language of the system, and a recognition of the whole page splits some words
/// wrongly ("nousirons"): the engine does better on one paragraph at a time. A region that
/// cannot be read again keeps the text it had. `strokes` are the strokes that were analysed.
pub fn read_regions(
    recognizer: &dyn Recognizer,
    strokes: &[Stroke],
    regions: &mut [Region],
    language: Language,
) {
    for region in regions.iter_mut().filter(|r| r.kind != RegionKind::Drawing) {
        let own: Vec<Stroke> = region
            .strokes
            .iter()
            .filter_map(|&position| strokes.get(position).cloned())
            .collect();
        if own.is_empty() {
            continue;
        }
        if let Ok(read) = recognizer.recognize(&own, language)
            && !read.is_empty()
        {
            region.text = joined(&read.lines);
        }
    }
}

/// The structure of a note and how it was found.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub items: Vec<Item>,
    /// True when Windows' ink analysis gave the layout; false when it was guessed from the
    /// recognised lines ([`from_recognition`]) because the analysis was not available.
    pub analyzed: bool,
}

/// The structure of `note`, whose handwriting `recognition` read: through the ink analysis of
/// `recognizer` when it works (each paragraph and list item read again on its own in
/// `recognition.language`), else from the recognised lines.
pub fn structure_of(
    recognizer: &dyn Recognizer,
    recognition: &Recognition,
    note: &Note,
) -> Structure {
    let ink = ink_boxes(note);
    let analyzed = crate::ink::recognizable_indices(note);
    let strokes = crate::ink::recognizable_strokes(note);
    match recognizer.analyze(&strokes) {
        Ok(mut regions) => {
            read_regions(recognizer, &strokes, &mut regions, recognition.language);
            Structure {
                items: from_regions(&regions, &analyzed, recognition, &ink),
                analyzed: true,
            }
        }
        Err(_) => Structure {
            items: from_recognition(recognition, &ink),
            analyzed: false,
        },
    }
}

/// A visible stroke of the note and the rectangle it covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InkBox {
    /// Position in `note.strokes`.
    pub stroke: usize,
    pub bounds: Bounds,
    /// Whether the recognition considered it ([`crate::ink::recognizable_indices`]): everything
    /// visible but highlighter marks.
    pub recognizable: bool,
}

/// The visible strokes of `note` with their bounds in page units.
pub fn ink_boxes(note: &Note) -> Vec<InkBox> {
    note.strokes
        .iter()
        .enumerate()
        .filter(|(_, stroke)| {
            !stroke.points.is_empty()
                && note
                    .layers
                    .get(stroke.layer as usize)
                    .is_some_and(|layer| layer.visible)
        })
        .map(|(index, stroke)| {
            let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
            let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
            for p in &stroke.points {
                min_x = min_x.min(p.x);
                max_x = max_x.max(p.x);
                min_y = min_y.min(p.y);
                max_y = max_y.max(p.y);
            }
            InkBox {
                stroke: index,
                bounds: Bounds {
                    x: min_x,
                    y: min_y,
                    width: max_x - min_x,
                    height: max_y - min_y,
                },
                recognizable: stroke.tool != Tool::Highlighter,
            }
        })
        .collect()
}

/// The strokes of a drawing and the rectangle that holds them.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingArea {
    pub bounds: Bounds,
    /// Positions in `note.strokes`.
    pub strokes: Vec<usize>,
}

/// One block of the page, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A paragraph: its lines joined by single spaces.
    Paragraph(String),
    /// A list item, without its bullet or number.
    ListItem(String),
    Drawing(DrawingArea),
}

/// An item and the rectangle it covers (to put the items in reading order).
struct Placed {
    bounds: Bounds,
    item: Item,
}

fn into_items(mut placed: Vec<Placed>) -> Vec<Item> {
    placed.sort_by(|a, b| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
    });
    placed.into_iter().map(|p| p.item).collect()
}

fn center(bounds: &Bounds) -> (f32, f32) {
    (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
}

fn contains(bounds: &Bounds, x: f32, y: f32, tolerance: f32) -> bool {
    x >= bounds.x - tolerance
        && x <= bounds.right() + tolerance
        && y >= bounds.y - tolerance
        && y <= bounds.bottom() + tolerance
}

/// The mark that starts a list item, taken off the front of `text`: a dash, bullet or star
/// followed by a space or directly by a letter, or a number with a dot or a parenthesis. `None`
/// when `text` does not start a list item.
pub fn strip_bullet(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let mut chars = text.chars();
    let first = chars.next()?;
    let rest = chars.as_str();
    if matches!(
        first,
        '-' | '–' | '—' | '•' | '·' | '*' | '+' | '●' | '○' | '▪' | '◦'
    ) {
        // "-5" is a negative number, not a bullet.
        return match rest.chars().next() {
            None => Some(""),
            Some(c) if c.is_whitespace() => Some(rest.trim_start()),
            Some(c) if c.is_alphabetic() => Some(rest),
            _ => None,
        };
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let after = &text[digits..];
        let mut after_chars = after.chars();
        if matches!(after_chars.next(), Some('.' | ')')) {
            let tail = after_chars.as_str();
            return match tail.chars().next() {
                None => Some(""),
                Some(c) if c.is_whitespace() => Some(tail.trim_start()),
                Some(c) if c.is_alphabetic() => Some(tail),
                _ => None,
            };
        }
    }
    None
}

/// The text of `lines`, joined by single spaces.
fn joined(lines: &[Line]) -> String {
    let parts: Vec<String> = lines.iter().map(Line::text).collect();
    parts.join(" ").trim().to_owned()
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied()
}

/// The typical height of a line of text on the page.
fn line_height(recognition: &Recognition) -> f32 {
    median(
        recognition
            .lines
            .iter()
            .map(|line| line.bounds.height)
            .filter(|h| *h > 0.0)
            .collect(),
    )
    .unwrap_or(DEFAULT_LINE_HEIGHT)
}

/// The strokes of a drawing: its own, plus the highlighter marks (never read as text) that lie
/// over it.
fn drawing_strokes(
    bounds: &Bounds,
    own: impl Iterator<Item = usize>,
    ink: &[InkBox],
) -> Vec<usize> {
    let mut strokes: Vec<usize> = own.collect();
    for stroke in ink.iter().filter(|b| !b.recognizable) {
        let (x, y) = center(&stroke.bounds);
        if contains(bounds, x, y, 0.0) {
            strokes.push(stroke.stroke);
        }
    }
    strokes.sort_unstable();
    strokes.dedup();
    strokes
}

/// The structure from the ink analysis' `regions` (of the strokes `analyzed`: positions in
/// `note.strokes`, in the order given to the analysis) and the recognised words. A region's
/// text is its `text` when it has one, else the recognised words found inside it. Words no
/// region claims are laid out by [`from_recognition`]'s rules; a drawing of stray size is
/// dropped.
pub fn from_regions(
    regions: &[Region],
    analyzed: &[usize],
    recognition: &Recognition,
    ink: &[InkBox],
) -> Vec<Item> {
    let words: Vec<&Word> = recognition.words().collect();
    let mut used = vec![false; words.len()];
    let mut placed = Vec::new();
    for region in regions {
        let claimed: Vec<usize> = words
            .iter()
            .enumerate()
            .filter(|(i, word)| {
                let (x, y) = center(&word.bounds);
                !used[*i] && contains(&region.bounds, x, y, word.bounds.height * 0.25)
            })
            .map(|(i, _)| i)
            .collect();
        for &i in &claimed {
            used[i] = true;
        }
        match region.kind {
            RegionKind::Drawing => {
                if region.bounds.width < MIN_DRAWING && region.bounds.height < MIN_DRAWING {
                    continue;
                }
                let own = region
                    .strokes
                    .iter()
                    .filter_map(|&position| analyzed.get(position).copied());
                placed.push(Placed {
                    bounds: region.bounds,
                    item: Item::Drawing(DrawingArea {
                        bounds: region.bounds,
                        strokes: drawing_strokes(&region.bounds, own, ink),
                    }),
                });
            }
            RegionKind::Paragraph | RegionKind::ListItem => {
                // The region's own text (read again on its own by `read_regions`, else the
                // analysis'), else the words of the page recognition found there.
                let text = if region.text.trim().is_empty() {
                    let region_words: Vec<Word> =
                        claimed.iter().map(|&i| words[i].clone()).collect();
                    joined(&group_lines(region_words))
                } else {
                    region.text.trim().to_owned()
                };
                let item = if region.kind == RegionKind::ListItem {
                    // The analysis puts the bullet in the item; the words may hold it too.
                    let text = strip_bullet(&text).unwrap_or(&text).trim().to_owned();
                    Item::ListItem(text)
                } else {
                    Item::Paragraph(text)
                };
                if !matches!(&item, Item::Paragraph(t) | Item::ListItem(t) if t.is_empty()) {
                    placed.push(Placed {
                        bounds: region.bounds,
                        item,
                    });
                }
            }
        }
    }
    // What the analysis did not cover.
    let rest: Vec<Word> = words
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(word, _)| (*word).clone())
        .collect();
    if !rest.is_empty() {
        let lines = group_lines(rest);
        placed.extend(text_items(&lines, &[], line_height(recognition)));
    }
    into_items(placed)
}

/// Paragraphs and list items from recognised `lines` in reading order. `bullets` are the
/// small marks drawn to the left of the lines (they make a line a list item even when the
/// recognition did not read them).
fn text_items(lines: &[Line], bullets: &[Bounds], height: f32) -> Vec<Placed> {
    let has_bullet = |line: &Line| {
        bullets.iter().any(|mark| {
            let (_, y) = center(mark);
            let distance = line.bounds.x - mark.right();
            y >= line.bounds.y - height * 0.3
                && y <= line.bounds.bottom() + height * 0.3
                && distance > -height * 0.3
                && distance < BULLET_DISTANCE * height
        })
    };
    let mut placed: Vec<Placed> = Vec::new();
    // The block being built: its lines, and whether it is a list item with the left edge of its
    // text.
    struct Open {
        lines: Vec<Line>,
        item: Option<f32>,
    }
    let mut open: Option<Open> = None;
    let close = |open: &mut Option<Open>, placed: &mut Vec<Placed>| {
        if let Some(block) = open.take() {
            let text = joined(&block.lines);
            let bounds =
                Bounds::enclosing(block.lines.iter().map(|l| &l.bounds)).unwrap_or_default();
            if text.is_empty() {
                return;
            }
            placed.push(Placed {
                bounds,
                item: if block.item.is_some() {
                    Item::ListItem(text)
                } else {
                    Item::Paragraph(text)
                },
            });
        }
    };
    for line in lines {
        let text = line.text();
        let stripped = strip_bullet(&text);
        let drawn_bullet = has_bullet(line);
        if stripped.is_some() || drawn_bullet {
            close(&mut open, &mut placed);
            // The text of the item without its mark (a mark read as a word leaves the line's
            // other words).
            let words: Vec<Word> = if stripped.is_some() {
                strip_first_word(line)
            } else {
                line.words.clone()
            };
            let text_left = words.first().map_or(line.bounds.x, |w| w.bounds.x);
            open = Some(Open {
                lines: vec![Line::new(words)],
                item: Some(text_left),
            });
            continue;
        }
        let continues =
            open.as_ref()
                .is_some_and(|block| match (&block.lines.last(), block.item) {
                    (Some(last), Some(left)) => {
                        line.bounds.y - last.bounds.bottom() <= PARAGRAPH_GAP * height
                            && line.bounds.x >= left - CONTINUATION_INDENT * height
                    }
                    (Some(last), None) => {
                        line.bounds.y - last.bounds.bottom() <= PARAGRAPH_GAP * height
                    }
                    _ => false,
                });
        if continues {
            if let Some(block) = open.as_mut() {
                block.lines.push(line.clone());
            }
        } else {
            close(&mut open, &mut placed);
            open = Some(Open {
                lines: vec![line.clone()],
                item: None,
            });
        }
    }
    close(&mut open, &mut placed);
    placed
}

/// `line` without its bullet: the first word when it is only the mark, else the mark cut off
/// the front of the first word.
fn strip_first_word(line: &Line) -> Vec<Word> {
    let mut words = line.words.clone();
    let several = words.len() > 1;
    if let Some(first) = words.first_mut() {
        match strip_bullet(&first.text) {
            Some("") if several => first.text.clear(),
            Some(rest) => {
                let rest = rest.to_owned();
                first.text = rest;
            }
            // The mark is spread over several words ("- " then the text): the whole line is
            // read as text.
            None => {}
        }
    }
    words.retain(|word| !word.text.is_empty());
    words
}

/// The structure from the recognised lines alone (see the module documentation). `ink` is
/// every visible stroke of the note.
pub fn from_recognition(recognition: &Recognition, ink: &[InkBox]) -> Vec<Item> {
    let height = line_height(recognition);
    let words: Vec<&Word> = recognition.words().collect();
    // Ink that is not under a word.
    let uncovered: Vec<&InkBox> = ink
        .iter()
        .filter(|stroke| stroke.recognizable)
        .filter(|stroke| {
            let (x, y) = center(&stroke.bounds);
            !words
                .iter()
                .any(|word| contains(&word.bounds, x, y, word.bounds.height * 0.2))
        })
        .collect();
    // Small marks to the left of a line: bullets, not drawings.
    let is_bullet = |stroke: &InkBox| {
        let b = &stroke.bounds;
        b.width <= BULLET_WIDTH * height
            && b.height <= BULLET_HEIGHT * height
            && recognition.lines.iter().any(|line| {
                let (_, y) = center(b);
                let distance = line.bounds.x - b.right();
                y >= line.bounds.y
                    && y <= line.bounds.bottom()
                    && distance > -height * 0.3
                    && distance < BULLET_DISTANCE * height
            })
    };
    let (marks, drawn): (Vec<&InkBox>, Vec<&InkBox>) =
        uncovered.into_iter().partition(|stroke| is_bullet(stroke));
    let bullets: Vec<Bounds> = marks.iter().map(|stroke| stroke.bounds).collect();
    let mut placed = text_items(&recognition.lines, &bullets, height);
    placed.extend(drawings(&drawn, ink, height));
    into_items(placed)
}

/// Groups strokes closer than [`DRAWING_LINK`] (or half a line) into drawings.
fn drawings(strokes: &[&InkBox], ink: &[InkBox], height: f32) -> Vec<Placed> {
    let link = DRAWING_LINK.max(height * 0.5);
    let mut parent: Vec<usize> = (0..strokes.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..strokes.len() {
        for j in (i + 1)..strokes.len() {
            let (a, b) = (&strokes[i].bounds, &strokes[j].bounds);
            let gap_x = (a.x - b.right()).max(b.x - a.right()).max(0.0);
            let gap_y = (a.y - b.bottom()).max(b.y - a.bottom()).max(0.0);
            if gap_x.hypot(gap_y) <= link {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<&InkBox>)> = Vec::new();
    for (i, stroke) in strokes.iter().enumerate() {
        let r = root(&mut parent, i);
        match groups.iter_mut().find(|(key, _)| *key == r) {
            Some((_, members)) => members.push(stroke),
            None => groups.push((r, vec![stroke])),
        }
    }
    groups
        .into_iter()
        .filter_map(|(_, members)| {
            let bounds = Bounds::enclosing(members.iter().map(|m| &m.bounds))?;
            if bounds.width < MIN_DRAWING && bounds.height < MIN_DRAWING {
                return None;
            }
            let own = members.iter().map(|m| m.stroke);
            Some(Placed {
                bounds,
                item: Item::Drawing(DrawingArea {
                    bounds,
                    strokes: drawing_strokes(&bounds, own, ink),
                }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use crate::model::tests::word;

    /// A line of words, each `width` wide and 300 tall, from `x` at `y`.
    fn line(text: &str, x: f32, y: f32) -> Line {
        let words = text
            .split(' ')
            .enumerate()
            .map(|(i, w)| word(w, x + i as f32 * 500.0, y, 400.0, 300.0))
            .collect();
        Line::new(words)
    }

    fn recognition(lines: Vec<Line>) -> Recognition {
        Recognition {
            language: Language::Fr,
            recognizer: String::new(),
            lines,
        }
    }

    fn mark(stroke: usize, x: f32, y: f32, w: f32, h: f32) -> InkBox {
        InkBox {
            stroke,
            bounds: Bounds {
                x,
                y,
                width: w,
                height: h,
            },
            recognizable: true,
        }
    }

    fn texts(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Paragraph(t) => format!("P:{t}"),
                Item::ListItem(t) => format!("L:{t}"),
                Item::Drawing(d) => format!("D:{:?}", d.strokes),
            })
            .collect()
    }

    #[test]
    fn bullets_and_numbers_are_taken_off_the_front() {
        assert_eq!(strip_bullet("- pommes"), Some("pommes"));
        assert_eq!(strip_bullet("-pommes"), Some("pommes"));
        assert_eq!(strip_bullet("• pommes"), Some("pommes"));
        assert_eq!(strip_bullet("– deux mots"), Some("deux mots"));
        assert_eq!(strip_bullet("-"), Some(""));
        assert_eq!(strip_bullet("1. pommes"), Some("pommes"));
        assert_eq!(strip_bullet("12) pommes"), Some("pommes"));
        assert_eq!(strip_bullet("2.pommes"), Some("pommes"));
        // Not bullets.
        assert_eq!(strip_bullet("-5 degrés"), None);
        assert_eq!(strip_bullet("3,5 kg"), None);
        assert_eq!(strip_bullet("2026 est là"), None);
        assert_eq!(strip_bullet("1.5 litre"), None);
        assert_eq!(strip_bullet("pommes"), None);
        assert_eq!(strip_bullet(""), None);
    }

    #[test]
    fn a_gap_of_more_than_a_line_starts_a_paragraph() {
        let r = recognition(vec![
            line("le chat dort", 0.0, 0.0),
            line("sur le canape", 0.0, 400.0),
            // A blank line (gap 600 > 300) then a new paragraph.
            line("la pluie tombe", 0.0, 1300.0),
        ]);
        let items = from_recognition(&r, &[]);
        assert_eq!(
            texts(&items),
            ["P:le chat dort sur le canape", "P:la pluie tombe"]
        );
    }

    #[test]
    fn dashes_bullets_and_numbers_make_list_items_with_their_continuation() {
        let r = recognition(vec![
            line("courses", 0.0, 0.0),
            line("- pommes", 0.0, 400.0),
            line("- poires rouges", 0.0, 800.0),
            // Indented, right below: goes on the previous item.
            line("et jaunes", 600.0, 1200.0),
            line("2. du pain", 0.0, 1600.0),
            line("fin", 0.0, 3000.0),
        ]);
        let items = from_recognition(&r, &[]);
        assert_eq!(
            texts(&items),
            [
                "P:courses",
                "L:pommes",
                "L:poires rouges et jaunes",
                "L:du pain",
                "P:fin"
            ]
        );
    }

    #[test]
    fn a_bullet_the_recognition_did_not_read_still_makes_a_list_item() {
        let r = recognition(vec![
            line("pommes", 800.0, 0.0),
            line("poires", 800.0, 400.0),
            line("texte", 0.0, 1500.0),
        ]);
        // Small marks left of the first two lines, a little above their middle.
        let ink = [
            mark(0, 200.0, 130.0, 300.0, 20.0),
            mark(1, 200.0, 530.0, 300.0, 20.0),
        ];
        let items = from_recognition(&r, &ink);
        assert_eq!(texts(&items), ["L:pommes", "L:poires", "P:texte"]);
    }

    #[test]
    fn ink_that_is_not_text_makes_a_drawing_in_reading_order() {
        let r = recognition(vec![line("avant", 0.0, 0.0), line("apres", 0.0, 4000.0)]);
        // Under the first line: a circle of two strokes (a frame and a dot inside), far from
        // the text, and a stray dot elsewhere.
        let ink = [
            mark(0, 0.0, 0.0, 400.0, 300.0), // under "avant" (covered by the word)
            mark(1, 1000.0, 1200.0, 1500.0, 1500.0),
            mark(2, 1500.0, 1700.0, 400.0, 400.0),
            mark(3, 6000.0, 2000.0, 60.0, 60.0),
            InkBox {
                stroke: 4,
                bounds: Bounds {
                    x: 900.0,
                    y: 1100.0,
                    width: 1700.0,
                    height: 1700.0,
                },
                recognizable: false,
            },
        ];
        let items = from_recognition(&r, &ink);
        assert_eq!(texts(&items), ["P:avant", "D:[1, 2, 4]", "P:apres"]);
        let Item::Drawing(area) = &items[1] else {
            panic!("a drawing")
        };
        assert_eq!(area.bounds.x, 1000.0);
        assert_eq!(area.bounds.width, 1500.0);
    }

    #[test]
    fn the_analysis_gives_the_structure_and_the_recognition_the_words() {
        let r = recognition(vec![
            line("titre", 0.0, 0.0),
            line("le chat dort", 0.0, 800.0),
            line("- pommes", 0.0, 1600.0),
            line("poires", 800.0, 2000.0),
            line("orphelin", 0.0, 5000.0),
            // The words the recognition read in the drawing are dropped.
            line("o", 3000.0, 1000.0),
        ]);
        let region = |kind, x, y, w, h, text: &str, strokes: Vec<usize>| Region {
            kind,
            bounds: Bounds {
                x,
                y,
                width: w,
                height: h,
            },
            text: text.into(),
            strokes,
        };
        let regions = [
            region(
                RegionKind::Paragraph,
                0.0,
                0.0,
                500.0,
                300.0,
                "titre",
                vec![0],
            ),
            region(
                RegionKind::Paragraph,
                0.0,
                800.0,
                1400.0,
                300.0,
                "le chat dort",
                vec![1],
            ),
            region(
                RegionKind::Drawing,
                2900.0,
                900.0,
                600.0,
                600.0,
                "",
                vec![2],
            ),
            region(
                RegionKind::ListItem,
                0.0,
                1600.0,
                900.0,
                300.0,
                "-pommes",
                vec![3],
            ),
            region(
                RegionKind::ListItem,
                800.0,
                2000.0,
                400.0,
                300.0,
                "-poires",
                vec![4],
            ),
            // A mark too small to be a drawing.
            region(RegionKind::Drawing, 100.0, 100.0, 50.0, 50.0, "", vec![5]),
            // The analysis read something else than the recognition: the recognition wins.
            region(
                RegionKind::Paragraph,
                9000.0,
                0.0,
                400.0,
                300.0,
                "hors page",
                vec![],
            ),
        ];
        // The analysed strokes are not the first ones of the note: positions are mapped.
        let analyzed = [10, 11, 12, 13, 14, 15];
        let ink = [InkBox {
            stroke: 20,
            bounds: Bounds {
                x: 3000.0,
                y: 1000.0,
                width: 400.0,
                height: 400.0,
            },
            recognizable: false,
        }];
        let items = from_regions(&regions, &analyzed, &r, &ink);
        assert_eq!(
            texts(&items),
            [
                "P:titre",
                // The analysis read it, the recognition has no word there: its own text.
                "P:hors page",
                "P:le chat dort",
                "D:[12, 20]",
                "L:pommes",
                "L:poires",
                "P:orphelin",
            ]
        );
    }

    /// Reads "n strokes" for n strokes, fails for a single stroke.
    struct Counting;

    impl Recognizer for Counting {
        fn recognizers(&self) -> Result<Vec<crate::RecognizerInfo>, crate::HwrError> {
            Ok(Vec::new())
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
        ) -> Result<Recognition, crate::HwrError> {
            match strokes.len() {
                1 => Err(crate::HwrError::Stopped),
                0 => Ok(Recognition::empty(language)),
                n => Ok(recognition(vec![
                    line(&format!("{n} traits"), 0.0, 0.0),
                    line("suite", 0.0, 400.0),
                ])),
            }
        }
    }

    #[test]
    fn each_text_region_is_read_again_on_its_own() {
        let stroke = |tool| Stroke {
            tool,
            color: ardoise_ink::Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points: Vec::new(),
        };
        let strokes: Vec<Stroke> = (0..5)
            .map(|_| stroke(ardoise_ink::Tool::Ballpoint))
            .collect();
        let region = |kind, text: &str, positions: Vec<usize>| Region {
            kind,
            bounds: Bounds::default(),
            text: text.into(),
            strokes: positions,
        };
        let mut regions = [
            region(RegionKind::Paragraph, "analyse", vec![0, 1, 2]),
            // One stroke: the engine fails, the analysis' text stays.
            region(RegionKind::ListItem, "-garde", vec![3]),
            // A drawing is not read.
            region(RegionKind::Drawing, "", vec![0, 1]),
            // No stroke known (positions out of range): nothing to read.
            region(RegionKind::Paragraph, "vide", vec![9]),
        ];
        read_regions(&Counting, &strokes, &mut regions, Language::Fr);
        assert_eq!(regions[0].text, "3 traits suite");
        assert_eq!(regions[1].text, "-garde");
        assert_eq!(regions[2].text, "");
        assert_eq!(regions[3].text, "vide");
    }

    /// A recognizer that reads every group of strokes as "lu" and analyses a page into a
    /// paragraph (the first stroke) and a drawing (the second); with `analysis` false it has
    /// no analysis.
    struct Analyzing {
        analysis: bool,
    }

    impl Recognizer for Analyzing {
        fn recognizers(&self) -> Result<Vec<crate::RecognizerInfo>, crate::HwrError> {
            Ok(Vec::new())
        }

        fn recognize(
            &self,
            _: &[Stroke],
            language: Language,
        ) -> Result<Recognition, crate::HwrError> {
            Ok(Recognition {
                language,
                recognizer: String::new(),
                lines: vec![line("lu", 0.0, 0.0)],
            })
        }

        fn analyze(&self, _: &[Stroke]) -> Result<Vec<Region>, crate::HwrError> {
            if !self.analysis {
                return Err(crate::HwrError::Unsupported);
            }
            let bounds = |x: f32| Bounds {
                x,
                y: 0.0,
                width: 1000.0,
                height: 1000.0,
            };
            Ok(vec![
                Region {
                    kind: RegionKind::Paragraph,
                    bounds: bounds(0.0),
                    text: String::new(),
                    strokes: vec![0],
                },
                Region {
                    kind: RegionKind::Drawing,
                    bounds: bounds(5000.0),
                    text: String::new(),
                    strokes: vec![1],
                },
            ])
        }
    }

    #[test]
    fn the_structure_of_a_note_uses_the_analysis_or_falls_back() {
        use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Timing};
        let stroke = |x: f32| Stroke {
            tool: ardoise_ink::Tool::Ballpoint,
            color: ardoise_ink::Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points: vec![
                ardoise_ink::Point {
                    x,
                    y: 100.0,
                    pressure: 0.5,
                    t: 0,
                },
                ardoise_ink::Point {
                    x: x + 300.0,
                    y: 400.0,
                    pressure: 0.5,
                    t: 5,
                },
            ],
        };
        let note = Note {
            id: "n".into(),
            title: String::new(),
            created: 0,
            modified: 0,
            page: A4_LANDSCAPE,
            background: Background::default(),
            background_image: None,
            tags: Vec::new(),
            favorite: false,
            layers: vec![Layer {
                name: "1".into(),
                visible: true,
            }],
            strokes: vec![stroke(100.0), stroke(5200.0)],
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        };
        // The page recognition read "page" where the paragraph is.
        let page = recognition(vec![line("page", 100.0, 100.0)]);
        let found = structure_of(&Analyzing { analysis: true }, &page, &note);
        assert!(found.analyzed);
        // The paragraph was read again on its own ("lu"); the second stroke is a drawing.
        assert_eq!(texts(&found.items), ["P:lu", "D:[1]"]);
        // Without an analysis: the lines of the page recognition, and no drawing (the second
        // stroke is not under a word, but it is a mark of 300 units: a drawing of its own).
        let guessed = structure_of(&Analyzing { analysis: false }, &page, &note);
        assert!(!guessed.analyzed);
        assert_eq!(texts(&guessed.items), ["P:page", "D:[1]"]);
    }

    #[test]
    fn words_no_region_claims_are_laid_out_by_the_heuristics() {
        let r = recognition(vec![
            line("dans la region", 0.0, 0.0),
            line("hors region", 0.0, 3000.0),
        ]);
        let regions = [Region {
            kind: RegionKind::Paragraph,
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 1500.0,
                height: 300.0,
            },
            text: String::new(),
            strokes: vec![],
        }];
        let items = from_regions(&regions, &[], &r, &[]);
        assert_eq!(texts(&items), ["P:dans la region", "P:hors region"]);
        // Nothing recognised and no region: nothing.
        assert!(from_regions(&[], &[], &recognition(vec![]), &[]).is_empty());
        assert!(from_recognition(&recognition(vec![]), &[]).is_empty());
    }
}
