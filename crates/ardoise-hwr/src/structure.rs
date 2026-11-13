//! The structure of a page of handwriting: paragraphs, list items and drawings in reading
//! order, platform-free.
//!
//! **The words are those of the recognition of the whole note**, the text the search, the `.txt`
//! export and the clipboard give, or the user's corrected transcription when there is one
//! ([`lay_correction`]); the structure only says where they go. Two sources give it, both giving
//! the same [`Item`]s:
//!
//! - [`from_regions`]: Windows' ink analysis ([`Recognizer::analyze`](crate::Recognizer::analyze),
//!   `InkAnalyzer`) says which strokes make a paragraph, a list item or a drawing. Each
//!   recognised line goes, in its order, to the paragraph or list item that holds it (or overlaps
//!   it most); only a region in which the recognition has no line is read on its own
//!   ([`read_regions`]). Reading each paragraph again on its own was tried first (M8): on real
//!   notes it misread words the recognition of the whole page had right, and it ignored the
//!   user's corrections;
//! - [`from_recognition`]: layout heuristics on the recognised lines, when the analysis is not
//!   available or fails: a vertical gap bigger than a line starts a paragraph, a leading dash,
//!   bullet or number (or a small mark drawn to the left of a line) makes a list item, and ink
//!   that the recognition did not read as text, grouped by proximity, is a drawing.
//!
//! Underlines: a thin, wide group of strokes right under a line of text, or through it, is not a
//! drawing but a [`Mark`] on the words it covers (Word underlines or strikes them, Markdown
//! emphasises or strikes them). Drawings go between the blocks of text they sit between; one
//! beside a block of text comes after it when it is on its right, before it on its left
//! ([`reading_order`]).

use ardoise_core::note::Note;
use ardoise_ink::Tool;

use ardoise_ink::Stroke;

use crate::Recognizer;
use crate::document::{Mark, Text};
use crate::model::{Bounds, Line, Recognition, Word};

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
/// A recognised line goes to the paragraph or list item that holds at least this share of it
/// (of its area; the one that holds the most), or that holds its middle.
const LINE_IN_REGION: f32 = 0.2;
/// The analysis' rectangles and the recognition's are drawn around the same ink by two engines:
/// a region is taken this many line heights larger on each side.
const REGION_MARGIN: f32 = 0.25;
/// An underline or a strikethrough is at most this many heights of its line tall...
const DECORATION_HEIGHT: f32 = 0.5;
/// ... or, sloping, up to a line tall when it is at least this many times wider than tall;
const DECORATION_SLOPE: f32 = 6.0;
/// ... at least this many times wider than tall,
const DECORATION_ASPECT: f32 = 3.0;
/// ... and at least this many line heights wide.
const DECORATION_MIN_WIDTH: f32 = 0.4;
/// The middle of a strikethrough is at least this share of its line's height below the top
/// of the line (higher, it is over the words)...
const STRIKE_TOP: f32 = 0.2;
/// ... and above this share; lower, it is an underline. With ascenders and descenders, the
/// middle of the small letters is at about half the line and the baseline at two thirds; a
/// line without descenders has its small letters lower (a strikethrough there is taken for
/// an underline: the safer mistake).
const STRIKE_LIMIT: f32 = 0.64;
/// An underline starts at most this many line heights below its line.
const UNDERLINE_GAP: f32 = 0.5;
/// A word is marked when the line covers at least this share of its width...
const COVERED: f32 = 0.5;
/// ... and the words marked are under (or over) at least this share of the line's length (a
/// line drawn much longer than the words is a rule or an arrow, not an underline).
const COVERED_SPAN: f32 = 0.5;
/// A "word" whose rectangle is this much the mark's (intersection over union) is the
/// recognition's reading of the mark itself ("_", "—"), not a word.
const OWN_READING: f32 = 0.5;
/// A recognised word is dropped when drawings cover at least this share of its area (and no
/// text block holds its middle).
const WORD_IN_DRAWINGS: f32 = 0.5;
/// A drawing is beside a block of text when they overlap vertically by at least this share
/// of the smaller height...
const BESIDE_OVERLAP: f32 = 0.3;
/// ... and horizontally by at most this share of the narrower width.
const BESIDE_SEPARATION: f32 = 0.2;

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
    /// The analysis' own reading of the region (in the language of the system): the last
    /// resort, when the recognition has no line in it and reading it on its own fails.
    pub text: String,
    /// The strokes of the region: positions in the list of strokes that was analysed.
    pub strokes: Vec<usize>,
}

/// `recognition` with, for each paragraph and list item of `regions` in which it has no word,
/// the strokes of that region read on their own (in `recognition.language`): their lines are
/// added at the end. A region that cannot be read keeps nothing (its `text`, the analysis'
/// reading, stands in). `strokes` are the strokes that were analysed.
pub fn read_regions(
    recognizer: &dyn Recognizer,
    strokes: &[Stroke],
    regions: &[Region],
    recognition: &Recognition,
) -> Recognition {
    let texts: Vec<&Region> = text_regions(regions);
    let touched = touched(&texts, recognition.words());
    let mut read = recognition.clone();
    for (region, _) in texts.iter().zip(touched).filter(|(_, touched)| !touched) {
        let own: Vec<Stroke> = region
            .strokes
            .iter()
            .filter_map(|&position| strokes.get(position).cloned())
            .collect();
        if own.is_empty() {
            continue;
        }
        if let Ok(found) = recognizer.recognize(&own, recognition.language) {
            read.lines
                .extend(found.lines.into_iter().filter(|l| !l.words.is_empty()));
        }
    }
    read
}

/// A corrected transcription laid over the recognised lines ([`lay_correction`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Corrected {
    /// As many lines as the recognition: the corrected words where the recognised lines are.
    Placed(Vec<Line>),
    /// The lines do not match: the corrected text, without positions.
    Unplaced(String),
}

/// Lays the user's corrected transcription `text` over the lines of `recognition`, which it
/// was written from: they match line by line when both have as many lines (blank lines of
/// `text` aside). A corrected line keeps the rectangle of its recognised line; its words keep
/// those of the recognised words when there are as many, else share the line's width in
/// proportion to their length.
pub fn lay_correction(recognition: &Recognition, text: &str) -> Corrected {
    let corrected: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let recognised: Vec<&Line> = recognition
        .lines
        .iter()
        .filter(|line| !line.words.is_empty())
        .collect();
    if corrected.is_empty() || corrected.len() != recognised.len() {
        return Corrected::Unplaced(text.trim().to_owned());
    }
    let lines = recognised
        .iter()
        .zip(corrected)
        .map(|(line, text)| {
            let texts: Vec<&str> = text.split_whitespace().collect();
            let words = if texts.len() == line.words.len() {
                texts
                    .iter()
                    .zip(&line.words)
                    .map(|(text, word)| corrected_word(text, word.bounds))
                    .collect()
            } else {
                // Each word as wide as its letters, a letter between two words.
                let letters = |text: &str| text.chars().count().max(1) as f32;
                let total: f32 =
                    texts.iter().map(|t| letters(t)).sum::<f32>() + texts.len() as f32 - 1.0;
                let unit = line.bounds.width / total.max(1.0);
                let mut x = line.bounds.x;
                texts
                    .iter()
                    .map(|text| {
                        let width = letters(text) * unit;
                        let bounds = Bounds {
                            x,
                            width,
                            ..line.bounds
                        };
                        x += width + unit;
                        corrected_word(text, bounds)
                    })
                    .collect()
            };
            Line {
                words,
                bounds: line.bounds,
            }
        })
        .collect();
    Corrected::Placed(lines)
}

fn corrected_word(text: &str, bounds: Bounds) -> Word {
    Word {
        text: text.to_owned(),
        alternatives: Vec::new(),
        bounds,
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

impl Structure {
    /// How many drawings (pictures) it holds.
    pub fn drawings(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, Item::Drawing(_)))
            .count()
    }

    /// The structure without its drawings (the export's "Inclure les dessins" off).
    pub fn without_drawings(mut self) -> Structure {
        self.items.retain(|item| !matches!(item, Item::Drawing(_)));
        self
    }
}

/// The structure of `note`, whose handwriting `recognition` read (the whole note at once):
/// through the ink analysis of `recognizer` when it works, else from the recognised lines.
/// `correction` is the user's corrected transcription of the note, which then gives every
/// word ([`lay_correction`]; one that does not match the recognised lines is the text as it
/// is, in plain paragraphs, with the drawings around it).
pub fn structure_of(
    recognizer: &dyn Recognizer,
    recognition: &Recognition,
    correction: Option<&str>,
    note: &Note,
) -> Structure {
    let ink = ink_boxes(note);
    let analyzed = crate::ink::recognizable_indices(note);
    let strokes = crate::ink::recognizable_strokes(note);
    let laid = correction
        .filter(|text| !text.trim().is_empty())
        .map(|text| lay_correction(recognition, text));
    match recognizer.analyze(&strokes) {
        Ok(regions) => {
            let read;
            let source = match &laid {
                Some(laid) => Source::corrected(recognition, laid),
                None => {
                    read = read_regions(recognizer, &strokes, &regions, recognition);
                    Source::recognised(&read)
                }
            };
            Structure {
                items: regions_layout(&regions, &analyzed, &source, &ink),
                analyzed: true,
            }
        }
        Err(_) => {
            let source = match &laid {
                Some(laid) => Source::corrected(recognition, laid),
                None => Source::recognised(recognition),
            };
            Structure {
                items: recognition_layout(&source, &ink),
                analyzed: false,
            }
        }
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
    Paragraph(Text),
    /// A list item, without its bullet or number.
    ListItem(Text),
    Drawing(DrawingArea),
}

/// Blocks and the rectangle they cover (to put them in reading order).
struct Placed {
    bounds: Bounds,
    items: Vec<Item>,
}

impl Placed {
    fn one(bounds: Bounds, item: Item) -> Placed {
        Placed {
            bounds,
            items: vec![item],
        }
    }
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

/// Length of the overlap of `[a, a_end]` and `[b, b_end]` (negative when apart).
fn overlap(a: f32, a_end: f32, b: f32, b_end: f32) -> f32 {
    a_end.min(b_end) - a.max(b)
}

/// Area of the intersection of two rectangles.
fn intersection(a: &Bounds, b: &Bounds) -> f32 {
    overlap(a.x, a.right(), b.x, b.right()).max(0.0)
        * overlap(a.y, a.bottom(), b.y, b.bottom()).max(0.0)
}

/// Intersection over union of two rectangles (0 for empty ones).
fn iou(a: &Bounds, b: &Bounds) -> f32 {
    let common = intersection(a, b);
    let union = a.width * a.height + b.width * b.height - common;
    if union > 0.0 { common / union } else { 0.0 }
}

/// The text block a line goes to: among `regions`, the one that holds the largest share of
/// it ([`LINE_IN_REGION`] at least) or its middle; the smaller region on a tie.
fn region_of(line: &Bounds, regions: &[&Region]) -> Option<usize> {
    let margin = line.height * REGION_MARGIN;
    let area = line.width * line.height;
    let (x, y) = center(line);
    let mut best: Option<(f32, f32, usize)> = None;
    for (index, region) in regions.iter().enumerate() {
        let grown = Bounds {
            x: region.bounds.x - margin,
            y: region.bounds.y - margin,
            width: region.bounds.width + 2.0 * margin,
            height: region.bounds.height + 2.0 * margin,
        };
        let middle = contains(&grown, x, y, 0.0);
        let share = if area > 0.0 {
            intersection(line, &grown) / area
        } else if middle {
            1.0
        } else {
            0.0
        };
        if share < LINE_IN_REGION && !middle {
            continue;
        }
        let size = region.bounds.width * region.bounds.height;
        let better = best.is_none_or(|(best_share, best_size, _)| {
            share > best_share || (share == best_share && size < best_size)
        });
        if better {
            best = Some((share, size, index));
        }
    }
    best.map(|(_, _, index)| index)
}

/// The paragraphs and list items of `regions`.
fn text_regions(regions: &[Region]) -> Vec<&Region> {
    regions
        .iter()
        .filter(|r| r.kind != RegionKind::Drawing)
        .collect()
}

/// For each of `regions`, whether the middle of one of `words` is in it: its text is in the
/// recognition, maybe on a line that went to another region (a title and a date on one row
/// make one recognised line and two regions). Such a region is never read again and never takes
/// the analysis' reading: its words would come twice.
fn touched<'a>(regions: &[&Region], words: impl Iterator<Item = &'a Word>) -> Vec<bool> {
    let mut touched = vec![false; regions.len()];
    for word in words {
        let (x, y) = center(&word.bounds);
        let margin = word.bounds.height * REGION_MARGIN;
        for (index, region) in regions.iter().enumerate() {
            if contains(&region.bounds, x, y, margin) {
                touched[index] = true;
            }
        }
    }
    touched
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

/// What the layout is built from.
struct Source<'a> {
    /// The recognition of the whole note: where the ink of the text is.
    recognition: &'a Recognition,
    /// The lines whose words the document shows: the recognition's, or a correction laid over
    /// them.
    lines: &'a [Line],
    /// The words are the user's: every one is kept as it is.
    corrected: bool,
    /// A correction that could not be laid over the lines: it is the text, in plain paragraphs.
    unplaced: Option<&'a str>,
}

impl<'a> Source<'a> {
    fn recognised(recognition: &'a Recognition) -> Source<'a> {
        Source {
            recognition,
            lines: &recognition.lines,
            corrected: false,
            unplaced: None,
        }
    }

    fn corrected(recognition: &'a Recognition, laid: &'a Corrected) -> Source<'a> {
        match laid {
            Corrected::Placed(lines) => Source {
                recognition,
                lines,
                corrected: true,
                unplaced: None,
            },
            Corrected::Unplaced(text) => Source {
                recognition,
                lines: &recognition.lines,
                corrected: true,
                unplaced: Some(text),
            },
        }
    }
}

/// A line of text with the mark of each word.
#[derive(Debug, Clone)]
struct MarkedLine {
    words: Vec<(Word, Mark)>,
    bounds: Bounds,
}

impl MarkedLine {
    fn new(words: Vec<(Word, Mark)>) -> MarkedLine {
        let bounds = Bounds::enclosing(words.iter().map(|(w, _)| &w.bounds)).unwrap_or_default();
        MarkedLine { words, bounds }
    }

    /// The words separated by spaces, without marks.
    fn plain(&self) -> String {
        let words: Vec<&str> = self.words.iter().map(|(w, _)| w.text.as_str()).collect();
        words.join(" ")
    }

    /// The line without the bullet or number its text starts with: the first word when it is
    /// only the mark, else the mark cut off the front of the first word. With `signs`, a first
    /// word of one or two signs goes too: the bullet the analysis found, as the recognition read
    /// it ("→", "°"; not an opening quote or bracket).
    fn strip_bullet(&mut self, signs: bool) {
        if let Some((first, _)) = self.words.first_mut() {
            if let Some(rest) = strip_bullet(&first.text) {
                first.text = rest.to_owned();
            } else if signs && is_bullet_sign(&first.text) {
                first.text.clear();
            }
        }
        self.words.retain(|(word, _)| !word.text.is_empty());
    }
}

/// Whether `word` is one or two signs that can be a bullet: no letter or digit, no opening
/// quote or bracket.
fn is_bullet_sign(word: &str) -> bool {
    (1..=2).contains(&word.chars().count())
        && word
            .chars()
            .all(|c| !c.is_alphanumeric() && !"«\"'“‘([{¿¡".contains(c))
}

/// The text of `lines`, joined by single spaces.
fn text_of(lines: &[MarkedLine]) -> Text {
    Text::from_words(
        lines
            .iter()
            .flat_map(|line| line.words.iter())
            .map(|(word, mark)| (word.text.as_str(), *mark)),
    )
}

/// The lines of the text with, for each word, its mark and whether it stays.
struct Marked<'a> {
    lines: &'a [Line],
    marks: Vec<Vec<Mark>>,
    kept: Vec<Vec<bool>>,
}

impl<'a> Marked<'a> {
    fn new(lines: &'a [Line]) -> Marked<'a> {
        Marked {
            lines,
            marks: lines
                .iter()
                .map(|l| vec![Mark::None; l.words.len()])
                .collect(),
            kept: lines.iter().map(|l| vec![true; l.words.len()]).collect(),
        }
    }

    /// Marks the words `decoration` covers. Its own reading goes, unless the words are the
    /// user's (`corrected`).
    fn apply(&mut self, decoration: &Decoration, corrected: bool) {
        for &word in &decoration.words {
            self.marks[decoration.line][word] = decoration.mark;
        }
        if !corrected {
            for &(line, word) in &decoration.own {
                self.kept[line][word] = false;
            }
        }
    }

    /// The line `index` with its words kept, `None` when none is.
    fn line(&self, index: usize) -> Option<MarkedLine> {
        let words: Vec<(Word, Mark)> = self.lines[index]
            .words
            .iter()
            .enumerate()
            .filter(|(i, _)| self.kept[index][*i])
            .map(|(i, word)| (word.clone(), self.marks[index][i]))
            .collect();
        (!words.is_empty()).then(|| MarkedLine::new(words))
    }

    /// Every line with its words kept, in order.
    fn all(&self) -> Vec<MarkedLine> {
        (0..self.lines.len()).filter_map(|i| self.line(i)).collect()
    }
}

/// An underline or a strikethrough: the words of line `line` it marks, and the "words" (line,
/// word) that are the recognition's reading of the mark itself.
#[derive(Debug, Clone, PartialEq)]
struct Decoration {
    line: usize,
    words: Vec<usize>,
    own: Vec<(usize, usize)>,
    mark: Mark,
}

/// Whether the ink in `bounds` (a group of strokes the analysis or the heuristics took for a
/// drawing) is an underline or a strikethrough of words of `lines`: thin and wide, right under a
/// line or through it, over words for a good part of its length.
fn decoration_of(bounds: &Bounds, lines: &[Line]) -> Option<Decoration> {
    let (_, middle) = center(bounds);
    let own: Vec<(usize, usize)> = lines
        .iter()
        .enumerate()
        .flat_map(|(l, line)| (0..line.words.len()).map(move |w| (l, w)))
        .filter(|&(l, w)| iou(&lines[l].words[w].bounds, bounds) >= OWN_READING)
        .collect();
    let mut best: Option<(f32, Decoration)> = None;
    for (index, line) in lines.iter().enumerate() {
        let height = line.bounds.height;
        if height <= 0.0 || line.words.is_empty() {
            continue;
        }
        let thin = bounds.height <= DECORATION_HEIGHT * height
            || (bounds.height <= height && bounds.width >= DECORATION_SLOPE * bounds.height);
        let wide = bounds.width >= DECORATION_ASPECT * bounds.height
            && bounds.width >= DECORATION_MIN_WIDTH * height;
        if !thin || !wide {
            continue;
        }
        // 0 at the top of the line, 1 at its bottom.
        let depth = (middle - line.bounds.y) / height;
        let mark = if (STRIKE_TOP..STRIKE_LIMIT).contains(&depth) {
            Mark::Strike
        } else if depth >= STRIKE_LIMIT && bounds.y <= line.bounds.bottom() + UNDERLINE_GAP * height
        {
            Mark::Underline
        } else {
            continue;
        };
        let words: Vec<usize> = (0..line.words.len())
            .filter(|&i| !own.contains(&(index, i)))
            .filter(|&i| {
                let word = &line.words[i].bounds;
                word.width > 0.0
                    && overlap(word.x, word.right(), bounds.x, bounds.right())
                        >= COVERED * word.width
            })
            .collect();
        let (Some(&first), Some(&last)) = (words.first(), words.last()) else {
            continue;
        };
        let span = overlap(
            line.words[first].bounds.x,
            line.words[last].bounds.right(),
            bounds.x,
            bounds.right(),
        );
        if span < COVERED_SPAN * bounds.width {
            continue;
        }
        let distance = (line.bounds.y - middle)
            .max(middle - line.bounds.bottom())
            .max(0.0);
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((
                distance,
                Decoration {
                    line: index,
                    words,
                    own: own.clone(),
                    mark,
                },
            ));
        }
    }
    best.map(|(_, decoration)| decoration)
}

/// Whether `drawing` comes after the block of text `block` in reading order: when it is beside
/// the block, after it when on its right; otherwise when it is lower.
fn comes_after(drawing: &Bounds, block: &Bounds) -> bool {
    let vertical = overlap(drawing.y, drawing.bottom(), block.y, block.bottom());
    let horizontal = overlap(drawing.x, drawing.right(), block.x, block.right());
    let beside = vertical >= BESIDE_OVERLAP * drawing.height.min(block.height)
        && horizontal <= BESIDE_SEPARATION * drawing.width.min(block.width);
    let (drawing_x, drawing_y) = center(drawing);
    let (block_x, block_y) = center(block);
    if beside {
        drawing_x > block_x
    } else {
        block_y < drawing_y
    }
}

/// The blocks of text top to bottom (then left to right), each drawing between them: after the
/// last block it comes after ([`comes_after`]). Markdown and Word have one column: a drawing
/// beside a paragraph can only come before or after it.
fn reading_order(mut blocks: Vec<Placed>, mut drawings: Vec<Placed>) -> Vec<Item> {
    let by_position = |a: &Placed, b: &Placed| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
    };
    blocks.sort_by(by_position);
    drawings.sort_by(by_position);
    let mut slots: Vec<Vec<Item>> = (0..=blocks.len()).map(|_| Vec::new()).collect();
    for drawing in drawings {
        let slot = blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| comes_after(&drawing.bounds, &block.bounds))
            .map(|(index, _)| index + 1)
            .max()
            .unwrap_or(0);
        slots[slot].extend(drawing.items);
    }
    let mut slots = slots.into_iter();
    let mut items: Vec<Item> = slots.next().unwrap_or_default();
    for (block, slot) in blocks.into_iter().zip(slots) {
        items.extend(block.items);
        items.extend(slot);
    }
    items
}

/// The text blocks replaced by a correction that could not be laid over the lines (its plain
/// paragraphs, where the text was), then everything in reading order.
fn finish(blocks: Vec<Placed>, drawings: Vec<Placed>, source: &Source) -> Vec<Item> {
    let Some(text) = source.unplaced else {
        return reading_order(blocks, drawings);
    };
    let bounds = Bounds::enclosing(blocks.iter().map(|b| &b.bounds))
        .or_else(|| Bounds::enclosing(source.recognition.lines.iter().map(|l| &l.bounds)))
        .unwrap_or_default();
    let text = Placed {
        bounds,
        items: plain_paragraphs(text),
    };
    reading_order(vec![text], drawings)
}

/// A text without positions as blocks: blank lines separate paragraphs, a line that starts
/// with a dash, a bullet or a number starts a list item, the other lines go on the block
/// before them.
fn plain_paragraphs(text: &str) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut open: Option<(bool, Vec<&str>)> = None;
    let close = |open: &mut Option<(bool, Vec<&str>)>, items: &mut Vec<Item>| {
        if let Some((item, words)) = open.take() {
            let text = Text::plain(&words.join(" "));
            if !text.is_empty() {
                items.push(if item {
                    Item::ListItem(text)
                } else {
                    Item::Paragraph(text)
                });
            }
        }
    };
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            close(&mut open, &mut items);
        } else if let Some(rest) = strip_bullet(line) {
            close(&mut open, &mut items);
            open = Some((true, vec![rest]));
        } else if let Some((_, lines)) = open.as_mut() {
            lines.push(line);
        } else {
            open = Some((false, vec![line]));
        }
    }
    close(&mut open, &mut items);
    items
}

/// The structure from the ink analysis' `regions` (of the strokes `analyzed`: positions in
/// `note.strokes`, in the order given to the analysis) and the recognised words: each line goes
/// to the paragraph or list item that holds it, in its order; a region without any line has the
/// analysis' own reading. Lines no region holds are laid out by [`from_recognition`]'s rules; the
/// recognition's reading of a drawing's ink is dropped (the drawing shows it); a drawing of stray
/// size is dropped, an underline or a strikethrough marks the words it covers.
pub fn from_regions(
    regions: &[Region],
    analyzed: &[usize],
    recognition: &Recognition,
    ink: &[InkBox],
) -> Vec<Item> {
    regions_layout(regions, analyzed, &Source::recognised(recognition), ink)
}

fn regions_layout(
    regions: &[Region],
    analyzed: &[usize],
    source: &Source,
    ink: &[InkBox],
) -> Vec<Item> {
    let texts = text_regions(regions);
    let mut marked = Marked::new(source.lines);
    let mut drawings = Vec::new();
    let mut covers = Vec::new();
    for region in regions.iter().filter(|r| r.kind == RegionKind::Drawing) {
        if region.bounds.width < MIN_DRAWING && region.bounds.height < MIN_DRAWING {
            continue;
        }
        if let Some(decoration) = decoration_of(&region.bounds, source.lines) {
            marked.apply(&decoration, source.corrected);
            continue;
        }
        if !source.corrected {
            covers.push(region.bounds);
            // What the recognition read in the drawing's ink, outside any text.
            for (line, kept) in source.lines.iter().zip(marked.kept.iter_mut()) {
                for (word, kept) in line.words.iter().zip(kept.iter_mut()) {
                    let (x, y) = center(&word.bounds);
                    let margin = word.bounds.height * REGION_MARGIN;
                    if contains(&region.bounds, x, y, 0.0)
                        && !texts.iter().any(|t| contains(&t.bounds, x, y, margin))
                    {
                        *kept = false;
                    }
                }
            }
        }
        let own = region
            .strokes
            .iter()
            .filter_map(|&position| analyzed.get(position).copied());
        drawings.push(Placed::one(
            region.bounds,
            Item::Drawing(DrawingArea {
                bounds: region.bounds,
                strokes: drawing_strokes(&region.bounds, own, ink),
            }),
        ));
    }
    // A "word" that is mostly over drawings, though its middle is in none (the engine read
    // the ink of two drawings as one letter, and its line then spans the page), is no text.
    for (line, kept) in source.lines.iter().zip(marked.kept.iter_mut()) {
        for (word, kept) in line.words.iter().zip(kept.iter_mut()) {
            let area = word.bounds.width * word.bounds.height;
            let covered: f32 = covers.iter().map(|c| intersection(&word.bounds, c)).sum();
            let (x, y) = center(&word.bounds);
            let margin = word.bounds.height * REGION_MARGIN;
            if area > 0.0
                && covered >= area * WORD_IN_DRAWINGS
                && !texts.iter().any(|t| contains(&t.bounds, x, y, margin))
            {
                *kept = false;
            }
        }
    }
    let mut held: Vec<Vec<MarkedLine>> = texts.iter().map(|_| Vec::new()).collect();
    let mut loose = Vec::new();
    let lines = marked.all();
    let touched = touched(
        &texts,
        lines
            .iter()
            .flat_map(|l| l.words.iter().map(|(word, _)| word)),
    );
    for line in lines {
        match region_of(&line.bounds, &texts) {
            Some(index) => held[index].push(line),
            None => loose.push(line),
        }
    }
    let mut blocks = Vec::new();
    for ((region, mut lines), touched) in texts.iter().zip(held).zip(touched) {
        let list = region.kind == RegionKind::ListItem;
        let text = match lines.first_mut() {
            Some(first) => {
                if list {
                    // The analysis puts the bullet in the item; the words may hold it too.
                    first.strip_bullet(true);
                }
                text_of(&lines)
            }
            // The user's words are all placed, or the words there went with their line to
            // another region; else the analysis' own reading.
            None if source.corrected || touched => continue,
            None => {
                let own = region.text.trim();
                let own = if list {
                    strip_bullet(own).unwrap_or(own)
                } else {
                    own
                };
                Text::plain(own)
            }
        };
        if text.is_empty() {
            continue;
        }
        let item = if list {
            Item::ListItem(text)
        } else {
            Item::Paragraph(text)
        };
        blocks.push(Placed::one(region.bounds, item));
    }
    blocks.extend(text_items(&loose, &[], line_height(source.recognition)));
    finish(blocks, drawings, source)
}

/// Paragraphs and list items from recognised `lines` in reading order. `bullets` are the
/// small marks drawn to the left of the lines (they make a line a list item even when the
/// recognition did not read them).
fn text_items(lines: &[MarkedLine], bullets: &[Bounds], height: f32) -> Vec<Placed> {
    let has_bullet = |line: &MarkedLine| {
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
        lines: Vec<MarkedLine>,
        item: Option<f32>,
    }
    let mut open: Option<Open> = None;
    let close = |open: &mut Option<Open>, placed: &mut Vec<Placed>| {
        if let Some(block) = open.take() {
            let text = text_of(&block.lines);
            let bounds =
                Bounds::enclosing(block.lines.iter().map(|l| &l.bounds)).unwrap_or_default();
            if text.is_empty() {
                return;
            }
            placed.push(Placed::one(
                bounds,
                if block.item.is_some() {
                    Item::ListItem(text)
                } else {
                    Item::Paragraph(text)
                },
            ));
        }
    };
    for line in lines {
        let stripped = strip_bullet(&line.plain()).is_some();
        let drawn_bullet = has_bullet(line);
        if stripped || drawn_bullet {
            close(&mut open, &mut placed);
            // The text of the item without its mark (a mark read as a word leaves the line's
            // other words).
            let mut item = line.clone();
            if stripped {
                item.strip_bullet(false);
            }
            let text_left = item
                .words
                .first()
                .map_or(line.bounds.x, |(w, _)| w.bounds.x);
            open = Some(Open {
                lines: vec![item],
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

/// The structure from the recognised lines alone (see the module documentation). `ink` is
/// every visible stroke of the note.
pub fn from_recognition(recognition: &Recognition, ink: &[InkBox]) -> Vec<Item> {
    recognition_layout(&Source::recognised(recognition), ink)
}

fn recognition_layout(source: &Source, ink: &[InkBox]) -> Vec<Item> {
    let recognition = source.recognition;
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
    let mut marked = Marked::new(source.lines);
    let mut drawings = Vec::new();
    // A correction without any recognised word (no recognizer here): nothing tells the
    // handwriting from the drawings, which then stay out.
    let drawn = if source.corrected && words.is_empty() {
        Vec::new()
    } else {
        drawn
    };
    for (bounds, members) in groups(&drawn, height) {
        if let Some(decoration) = decoration_of(&bounds, source.lines) {
            marked.apply(&decoration, source.corrected);
            continue;
        }
        let own = members.iter().map(|m| m.stroke);
        drawings.push(Placed::one(
            bounds,
            Item::Drawing(DrawingArea {
                bounds,
                strokes: drawing_strokes(&bounds, own, ink),
            }),
        ));
    }
    let blocks = text_items(&marked.all(), &bullets, height);
    finish(blocks, drawings, source)
}

/// Groups strokes closer than [`DRAWING_LINK`] (or half a line), leaving out the groups of
/// stray size: each group's rectangle and strokes.
fn groups<'a>(strokes: &[&'a InkBox], height: f32) -> Vec<(Bounds, Vec<&'a InkBox>)> {
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
            Some((bounds, members))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Timing};

    use super::*;
    use crate::Language;
    use crate::model::tests::word;

    /// A line of words, each 400 wide and 300 tall, every 500 from `x`, at `y`.
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

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    fn mark(stroke: usize, x: f32, y: f32, w: f32, h: f32) -> InkBox {
        InkBox {
            stroke,
            bounds: bounds(x, y, w, h),
            recognizable: true,
        }
    }

    fn region(kind: RegionKind, at: Bounds, text: &str, strokes: Vec<usize>) -> Region {
        Region {
            kind,
            bounds: at,
            text: text.into(),
            strokes,
        }
    }

    /// The text with its marks: `_underlined_`, `~struck~`.
    fn shown(text: &Text) -> String {
        let spans: Vec<String> = text
            .spans
            .iter()
            .map(|span| match span.mark {
                Mark::None => span.text.clone(),
                Mark::Underline => format!("_{}_", span.text),
                Mark::Strike => format!("~{}~", span.text),
            })
            .collect();
        spans.join(" ")
    }

    fn texts(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Paragraph(t) => format!("P:{}", shown(t)),
                Item::ListItem(t) => format!("L:{}", shown(t)),
                Item::Drawing(d) => format!("D:{:?}", d.strokes),
            })
            .collect()
    }

    /// A stroke from (x, y) to (x + w, y + h).
    fn stroke(x: f32, y: f32, w: f32, h: f32) -> Stroke {
        let point = |x, y, t| ardoise_ink::Point {
            x,
            y,
            pressure: 0.5,
            t,
        };
        Stroke {
            tool: Tool::Ballpoint,
            color: ardoise_ink::Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points: vec![point(x, y, 0), point(x + w, y + h, 5)],
        }
    }

    fn note_of(strokes: Vec<Stroke>) -> Note {
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
            layers: vec![Layer {
                name: "1".into(),
                visible: true,
            }],
            strokes,
            objects: Vec::new(),
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
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
                bounds: bounds(900.0, 1100.0, 1700.0, 1700.0),
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
        let regions = [
            // The analysis reads in the language of the system: the recognition's words win.
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 500.0, 300.0),
                "title",
                vec![0],
            ),
            region(
                RegionKind::Paragraph,
                bounds(0.0, 800.0, 1400.0, 300.0),
                "the cat sleeps",
                vec![1],
            ),
            region(
                RegionKind::Drawing,
                bounds(2900.0, 900.0, 600.0, 600.0),
                "",
                vec![2],
            ),
            region(
                RegionKind::ListItem,
                bounds(0.0, 1600.0, 900.0, 300.0),
                "-apples",
                vec![3],
            ),
            region(
                RegionKind::ListItem,
                bounds(800.0, 2000.0, 400.0, 300.0),
                "-pears",
                vec![4],
            ),
            // A mark too small to be a drawing.
            region(
                RegionKind::Drawing,
                bounds(100.0, 100.0, 50.0, 50.0),
                "",
                vec![5],
            ),
            // No recognised line there: the analysis' own reading.
            region(
                RegionKind::Paragraph,
                bounds(9000.0, 0.0, 400.0, 300.0),
                "hors page",
                vec![],
            ),
        ];
        // The analysed strokes are not the first ones of the note: positions are mapped.
        let analyzed = [10, 11, 12, 13, 14, 15];
        let ink = [InkBox {
            stroke: 20,
            bounds: bounds(3000.0, 1000.0, 400.0, 400.0),
            recognizable: false,
        }];
        let items = from_regions(&regions, &analyzed, &r, &ink);
        assert_eq!(
            texts(&items),
            [
                "P:titre",
                "P:hors page",
                "P:le chat dort",
                "D:[12, 20]",
                "L:pommes",
                "L:poires",
                "P:orphelin",
            ]
        );
    }

    #[test]
    fn a_list_item_loses_its_bullet_as_the_recognition_read_it() {
        let r = recognition(vec![
            line("→ pommes", 0.0, 0.0),
            line("« poires »", 0.0, 400.0),
            line("2) pain", 0.0, 800.0),
        ]);
        let item = |y: f32| {
            region(
                RegionKind::ListItem,
                bounds(0.0, y, 1400.0, 300.0),
                "",
                vec![],
            )
        };
        let items = from_regions(&[item(0.0), item(400.0), item(800.0)], &[], &r, &[]);
        assert_eq!(texts(&items), ["L:pommes", "L:« poires »", "L:pain"]);
        assert!(is_bullet_sign("°") && is_bullet_sign("->") && is_bullet_sign("•"));
        assert!(!is_bullet_sign("(a)") && !is_bullet_sign("«") && !is_bullet_sign("a"));
        assert!(!is_bullet_sign("") && !is_bullet_sign("==>"));
    }

    #[test]
    fn each_line_goes_to_the_region_that_holds_it_in_its_order() {
        // The engine's reading order, which the export keeps inside a region.
        let r = recognition(vec![
            line("un deux", 0.0, 0.0),
            line("trois", 0.0, 1050.0),
            line("quatre", 0.0, 400.0),
            // Across both regions, mostly in the first one.
            line("cinq", 0.0, 600.0),
            // Mostly in the second one.
            line("six", 0.0, 1300.0),
        ]);
        let regions = [
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 3000.0, 700.0),
                "",
                vec![],
            ),
            region(
                RegionKind::Paragraph,
                bounds(0.0, 1000.0, 3000.0, 700.0),
                "",
                vec![],
            ),
        ];
        let items = from_regions(&regions, &[], &r, &[]);
        assert_eq!(
            texts(&items),
            ["P:un deux quatre cinq", "P:trois six"],
            "the words of the plain text, in its order"
        );
        // The region rule itself: the larger share wins; the middle is enough.
        let texts_of = |regions: &[Region]| text_regions(regions).len();
        assert_eq!(texts_of(&regions), 2);
        let refs: Vec<&Region> = regions.iter().collect();
        assert_eq!(region_of(&bounds(0.0, 600.0, 400.0, 300.0), &refs), Some(0));
        assert_eq!(region_of(&bounds(0.0, 900.0, 400.0, 300.0), &refs), Some(1));
        assert_eq!(region_of(&bounds(0.0, 5000.0, 400.0, 300.0), &refs), None);
        // A line on the edge of a region of the analysis (the two engines draw their
        // rectangles differently) still goes to it.
        assert_eq!(region_of(&bounds(0.0, -60.0, 400.0, 300.0), &refs), Some(0));
    }

    /// A recognizer that reads any group of strokes as one line "relu" where they are (one
    /// stroke alone fails), remembers how many strokes it was asked to read each time, and
    /// analyses a page into `regions` (none: no analysis).
    struct Reader {
        regions: Option<Vec<Region>>,
        asked: Mutex<Vec<usize>>,
    }

    impl Reader {
        fn new(regions: Option<Vec<Region>>) -> Reader {
            Reader {
                regions,
                asked: Mutex::new(Vec::new()),
            }
        }

        fn asked(&self) -> Vec<usize> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Recognizer for Reader {
        fn recognizers(&self) -> Result<Vec<crate::RecognizerInfo>, crate::HwrError> {
            Ok(Vec::new())
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
        ) -> Result<Recognition, crate::HwrError> {
            self.asked.lock().unwrap().push(strokes.len());
            if strokes.len() < 2 {
                return Err(crate::HwrError::Stopped);
            }
            let points = strokes.iter().flat_map(|s| s.points.iter());
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in points {
                (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
            }
            Ok(Recognition {
                language,
                recognizer: String::new(),
                lines: vec![Line::new(vec![word("relu", x0, y0, x1 - x0, y1 - y0)])],
            })
        }

        fn analyze(&self, _: &[Stroke]) -> Result<Vec<Region>, crate::HwrError> {
            self.regions.clone().ok_or(crate::HwrError::Unsupported)
        }
    }

    #[test]
    fn only_a_region_without_a_recognised_line_is_read_on_its_own() {
        let strokes = vec![
            stroke(0.0, 0.0, 1400.0, 300.0),
            stroke(0.0, 1000.0, 600.0, 300.0),
            stroke(700.0, 1000.0, 600.0, 300.0),
            stroke(0.0, 2000.0, 600.0, 300.0),
        ];
        let regions = vec![
            // The recognition of the whole note has a line here: never read again.
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 1400.0, 300.0),
                "the cat",
                vec![0],
            ),
            // Nothing recognised in it: read on its own.
            region(
                RegionKind::Paragraph,
                bounds(0.0, 1000.0, 1300.0, 300.0),
                "ignored",
                vec![1, 2],
            ),
            // Nothing recognised, and reading it on its own fails: the analysis' reading.
            region(
                RegionKind::ListItem,
                bounds(0.0, 2000.0, 600.0, 300.0),
                "- analyse",
                vec![3],
            ),
            // A drawing is never read.
            region(
                RegionKind::Drawing,
                bounds(5000.0, 0.0, 1000.0, 1000.0),
                "",
                vec![0, 1],
            ),
        ];
        let page = recognition(vec![line("le chat dort", 0.0, 0.0)]);
        let reader = Reader::new(Some(regions.clone()));
        let read = read_regions(&reader, &strokes, &regions, &page);
        assert_eq!(reader.asked(), [2, 1]);
        assert_eq!(read.lines.len(), 2);
        assert_eq!(read.text(), "le chat dort\nrelu");

        let note = note_of(strokes);
        let reader = Reader::new(Some(regions));
        let found = structure_of(&reader, &page, None, &note);
        assert!(found.analyzed);
        assert_eq!(
            texts(&found.items),
            ["P:le chat dort", "D:[0, 1]", "P:relu", "L:analyse"]
        );
        assert_eq!(reader.asked(), [2, 1]);
        // With a correction, the user's words are all there is: nothing is read again, and a
        // region they do not reach is left out.
        let reader = Reader::new(reader.regions.clone());
        let corrected = structure_of(&reader, &page, Some("le chien dort"), &note);
        assert_eq!(texts(&corrected.items), ["P:le chien dort", "D:[0, 1]"]);
        assert!(reader.asked().is_empty());
    }

    #[test]
    fn a_line_over_two_regions_is_exported_once() {
        // A title and a date on one row: one recognised line, two regions of the analysis.
        let r = recognition(vec![
            Line::new(vec![
                word("Optique", 0.0, 0.0, 900.0, 300.0),
                word("12/03", 6000.0, 0.0, 600.0, 300.0),
            ]),
            line("la lumiere", 0.0, 1000.0),
        ]);
        let strokes = vec![
            stroke(0.0, 0.0, 900.0, 300.0),
            stroke(6000.0, 0.0, 300.0, 300.0),
            stroke(6300.0, 0.0, 300.0, 300.0),
            stroke(0.0, 1000.0, 900.0, 300.0),
        ];
        let regions = vec![
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 900.0, 300.0),
                "Optics",
                vec![0],
            ),
            region(
                RegionKind::Paragraph,
                bounds(6000.0, 0.0, 600.0, 300.0),
                "12/03",
                vec![1, 2],
            ),
            region(
                RegionKind::Paragraph,
                bounds(0.0, 1000.0, 900.0, 300.0),
                "",
                vec![3],
            ),
        ];
        let reader = Reader::new(Some(regions.clone()));
        let found = structure_of(&reader, &r, None, &note_of(strokes));
        assert!(reader.asked().is_empty(), "nothing read again");
        assert_eq!(texts(&found.items), ["P:Optique 12/03", "P:la lumiere"]);
    }

    #[test]
    fn a_word_over_two_drawings_is_not_text_and_does_not_stretch_its_line() {
        // The engine read the ink of two drawings, right of the text, as one letter ending the
        // last list item: its line must not span the page and fall into another block.
        let r = recognition(vec![
            line("le chat", 0.0, 0.0),
            Line::new(vec![
                word("du", 0.0, 3000.0, 400.0, 300.0),
                word("J", 8000.0, 0.0, 1000.0, 3200.0),
            ]),
        ]);
        let regions = [
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 900.0, 300.0),
                "",
                vec![],
            ),
            region(
                RegionKind::ListItem,
                bounds(0.0, 3000.0, 400.0, 300.0),
                "",
                vec![],
            ),
            region(
                RegionKind::Drawing,
                bounds(8000.0, 0.0, 1000.0, 1500.0),
                "",
                vec![],
            ),
            region(
                RegionKind::Drawing,
                bounds(8000.0, 1700.0, 1000.0, 1500.0),
                "",
                vec![],
            ),
        ];
        let items = from_regions(&regions, &[], &r, &[]);
        assert_eq!(
            texts(&items)
                .into_iter()
                .filter(|t| !t.starts_with("D"))
                .collect::<Vec<_>>(),
            ["P:le chat", "L:du"]
        );
    }

    #[test]
    fn a_correction_without_recognition_does_not_turn_the_writing_into_drawings() {
        let empty = recognition(vec![]);
        let laid = lay_correction(&empty, "le texte corrigé");
        let source = Source::corrected(&empty, &laid);
        let ink = [
            mark(0, 0.0, 0.0, 1500.0, 300.0),
            mark(1, 0.0, 600.0, 900.0, 300.0),
        ];
        assert_eq!(
            texts(&recognition_layout(&source, &ink)),
            ["P:le texte corrigé"]
        );
        // Without a correction, ink the recognition read nothing in is a drawing, as before.
        assert_eq!(texts(&from_recognition(&empty, &ink)).len(), 1);
    }

    #[test]
    fn the_structure_of_a_note_uses_the_analysis_or_falls_back() {
        let note = note_of(vec![
            stroke(100.0, 100.0, 300.0, 300.0),
            stroke(5200.0, 100.0, 300.0, 300.0),
        ]);
        let regions = vec![
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 1000.0, 1000.0),
                "",
                vec![0],
            ),
            region(
                RegionKind::Drawing,
                bounds(5000.0, 0.0, 1000.0, 1000.0),
                "",
                vec![1],
            ),
        ];
        // The recognition of the whole note read "page" where the paragraph is: the words of
        // the export are those (the paragraph is not read again).
        let page = recognition(vec![line("page", 100.0, 100.0)]);
        let reader = Reader::new(Some(regions));
        let found = structure_of(&reader, &page, None, &note);
        assert!(found.analyzed);
        assert_eq!(texts(&found.items), ["P:page", "D:[1]"]);
        assert!(reader.asked().is_empty());
        assert_eq!(found.drawings(), 1);
        let plain = found.without_drawings();
        assert_eq!(texts(&plain.items), ["P:page"]);
        assert_eq!(plain.drawings(), 0);
        // Without an analysis: the lines of the recognition, and the second stroke, not under
        // a word and of 300 units, is a drawing of its own.
        let guessed = structure_of(&Reader::new(None), &page, None, &note);
        assert!(!guessed.analyzed);
        assert_eq!(texts(&guessed.items), ["P:page", "D:[1]"]);
        // A blank correction is none.
        let blank = structure_of(&Reader::new(None), &page, Some(" \n"), &note);
        assert_eq!(blank, guessed);
    }

    #[test]
    fn words_no_region_claims_are_laid_out_by_the_heuristics() {
        let r = recognition(vec![
            line("dans la region", 0.0, 0.0),
            line("hors region", 0.0, 3000.0),
        ]);
        let regions = [region(
            RegionKind::Paragraph,
            bounds(0.0, 0.0, 1500.0, 300.0),
            "",
            vec![],
        )];
        let items = from_regions(&regions, &[], &r, &[]);
        assert_eq!(texts(&items), ["P:dans la region", "P:hors region"]);
        // Nothing recognised and no region: nothing.
        assert!(from_regions(&[], &[], &recognition(vec![]), &[]).is_empty());
        assert!(from_recognition(&recognition(vec![]), &[]).is_empty());
    }

    #[test]
    fn a_correction_is_laid_over_the_recognised_lines() {
        let r = recognition(vec![
            line("la lumiere se", 0.0, 0.0),
            line("pro page", 0.0, 400.0),
        ]);
        // As many lines and words: each word where the recognised one is.
        let Corrected::Placed(lines) = lay_correction(&r, "la lumière se\r\n\r\npropage") else {
            panic!("placed")
        };
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "la lumière se");
        assert_eq!(lines[0].words[1].bounds, r.lines[0].words[1].bounds);
        // Another number of words: they share the line's width by their letters.
        assert_eq!(lines[1].text(), "propage");
        assert_eq!(lines[1].bounds, r.lines[1].bounds);
        assert_eq!(lines[1].words[0].bounds.width, r.lines[1].bounds.width);
        let Corrected::Placed(split) = lay_correction(&r, "la lumière se\npro pa ge") else {
            panic!("placed")
        };
        let widths: Vec<f32> = split[1].words.iter().map(|w| w.bounds.width).collect();
        // 3 + 2 + 2 letters and 2 gaps: 9 units over 900.
        assert_eq!(widths, [300.0, 200.0, 200.0]);
        assert_eq!(split[1].words[2].bounds.right(), r.lines[1].bounds.right());
        // Not as many lines: the text as it is.
        assert_eq!(
            lay_correction(&r, "une seule ligne"),
            Corrected::Unplaced("une seule ligne".into())
        );
        assert_eq!(
            lay_correction(&recognition(vec![]), "texte"),
            Corrected::Unplaced("texte".into())
        );
    }

    #[test]
    fn the_corrected_text_always_wins() {
        let r = recognition(vec![
            line("le chat dort", 0.0, 0.0),
            line("o", 3000.0, 1000.0),
            line("- pommes", 0.0, 2000.0),
        ]);
        let regions = [
            region(
                RegionKind::Paragraph,
                bounds(0.0, 0.0, 1400.0, 300.0),
                "",
                vec![],
            ),
            region(
                RegionKind::Drawing,
                bounds(2900.0, 900.0, 600.0, 600.0),
                "",
                vec![0],
            ),
            region(
                RegionKind::ListItem,
                bounds(0.0, 2000.0, 900.0, 300.0),
                "",
                vec![],
            ),
            // An underline under "chat".
            region(
                RegionKind::Drawing,
                bounds(480.0, 330.0, 440.0, 40.0),
                "",
                vec![1],
            ),
        ];
        let analyzed = [7, 8];
        // Line by line: the corrected words in the structure, the underline on the corrected
        // word, and the user's word in the drawing kept (the user wrote it).
        let laid = lay_correction(&r, "le chien dort\nO\n- poires");
        let source = Source::corrected(&r, &laid);
        let items = regions_layout(&regions, &analyzed, &source, &[]);
        assert_eq!(
            texts(&items),
            ["P:le _chien_ dort", "P:O", "D:[7]", "L:poires"]
        );
        // The lines do not match: the corrected text in plain paragraphs, where the text was,
        // the drawing after it.
        let laid = lay_correction(&r, "Le chien dort.\n\nDeux courses :\n- poires\n- pain");
        let source = Source::corrected(&r, &laid);
        let items = regions_layout(&regions, &analyzed, &source, &[]);
        assert_eq!(
            texts(&items),
            [
                "P:Le chien dort.",
                "P:Deux courses :",
                "L:poires",
                "L:pain",
                "D:[7]"
            ]
        );
        // The same without the analysis (a drawing under no recognised word).
        let ink = [mark(9, 5000.0, 900.0, 600.0, 600.0)];
        let items = recognition_layout(&source, &ink);
        assert_eq!(items.len(), 5);
        assert!(matches!(items[4], Item::Drawing(_)));
        // And a drawing above the text comes before it.
        let high = [mark(9, 0.0, -3000.0, 600.0, 600.0)];
        let items = recognition_layout(&source, &high);
        assert!(matches!(items[0], Item::Drawing(_)));
    }

    #[test]
    fn plain_paragraphs_keep_blank_lines_and_bullets() {
        let items =
            plain_paragraphs("Titre\n\nune phrase\nsur deux lignes\n- un\nsuite\n2. deux\n\n\n");
        assert_eq!(
            texts(&items),
            [
                "P:Titre",
                "P:une phrase sur deux lignes",
                "L:un suite",
                "L:deux"
            ]
        );
        assert!(plain_paragraphs(" \n").is_empty());
    }

    /// "la lumière se propage" (words at 0, 500, 1000, 1500, 400 wide, 0 to 300 high), then
    /// "en ligne droite" below (500 to 800).
    fn sentence() -> Recognition {
        recognition(vec![
            line("la lumière se propage", 0.0, 0.0),
            line("en ligne droite", 0.0, 500.0),
        ])
    }

    fn paragraph_regions() -> Vec<Region> {
        vec![region(
            RegionKind::Paragraph,
            bounds(0.0, 0.0, 1900.0, 800.0),
            "",
            vec![],
        )]
    }

    #[test]
    fn an_underline_marks_the_words_it_covers_and_is_no_drawing() {
        let mut regions = paragraph_regions();
        // Under "lumière se", a little sloping.
        regions.push(region(
            RegionKind::Drawing,
            bounds(480.0, 310.0, 940.0, 90.0),
            "",
            vec![0],
        ));
        // Through "propage".
        regions.push(region(
            RegionKind::Drawing,
            bounds(1500.0, 130.0, 420.0, 30.0),
            "",
            vec![1],
        ));
        // Under the whole second line, wavy and longer than it.
        regions.push(region(
            RegionKind::Drawing,
            bounds(-50.0, 820.0, 1600.0, 140.0),
            "",
            vec![2],
        ));
        // A long rule far below the text, and a figure beside it: drawings.
        regions.push(region(
            RegionKind::Drawing,
            bounds(0.0, 3000.0, 5000.0, 50.0),
            "",
            vec![3],
        ));
        regions.push(region(
            RegionKind::Drawing,
            bounds(2000.0, 100.0, 1000.0, 1000.0),
            "",
            vec![4],
        ));
        let items = from_regions(&regions, &[0, 1, 2, 3, 4], &sentence(), &[]);
        assert_eq!(
            texts(&items),
            [
                "P:la _lumière se_ ~propage~ _en ligne droite_",
                "D:[4]",
                "D:[3]"
            ]
        );
    }

    #[test]
    fn the_recognitions_reading_of_an_underline_is_not_a_word() {
        let mut r = sentence();
        // The engine read the underline as a word of a line of its own.
        r.lines
            .insert(1, Line::new(vec![word("_", 480.0, 330.0, 940.0, 40.0)]));
        let mut regions = paragraph_regions();
        regions.push(region(
            RegionKind::Drawing,
            bounds(480.0, 330.0, 940.0, 40.0),
            "",
            vec![0],
        ));
        let items = from_regions(&regions, &[0], &r, &[]);
        assert_eq!(texts(&items), ["P:la _lumière se_ propage en ligne droite"]);
    }

    #[test]
    fn the_heuristics_find_underlines_too() {
        let ink = [
            // Under "ligne droite", not under any word's middle.
            mark(0, 480.0, 850.0, 900.0, 60.0),
            // A circle far from the text.
            mark(1, 4000.0, 4000.0, 1500.0, 1500.0),
        ];
        let items = from_recognition(&sentence(), &ink);
        assert_eq!(
            texts(&items),
            ["P:la lumière se propage en _ligne droite_", "D:[1]"]
        );
        // Away from any line, the same stroke is a drawing.
        let alone = [mark(0, 480.0, 6000.0, 900.0, 60.0)];
        let items = from_recognition(&sentence(), &alone);
        assert_eq!(texts(&items)[1], "D:[0]");
    }

    #[test]
    fn drawings_go_between_the_blocks_they_sit_between() {
        // Three paragraphs on the right part of the page.
        let r = recognition(vec![
            line("premier", 4000.0, 0.0),
            line("second", 4000.0, 2000.0),
            line("troisieme", 4000.0, 4000.0),
        ]);
        let paragraph = |y: f32| {
            region(
                RegionKind::Paragraph,
                bounds(4000.0, y, 5000.0, 600.0),
                "",
                vec![],
            )
        };
        let drawing = |at: Bounds, stroke: usize| region(RegionKind::Drawing, at, "", vec![stroke]);
        let regions = [
            paragraph(0.0),
            paragraph(2000.0),
            paragraph(4000.0),
            // Right of the first paragraph, its top above the paragraph's: after it.
            drawing(bounds(10_000.0, -100.0, 3000.0, 800.0), 1),
            // Between the second and the third.
            drawing(bounds(4000.0, 2900.0, 3000.0, 600.0), 2),
            // Left of the third: before it.
            drawing(bounds(0.0, 3900.0, 3000.0, 800.0), 3),
            // Right of the second and the third, as tall as both: after the third.
            drawing(bounds(10_000.0, 1900.0, 3000.0, 2800.0), 4),
        ];
        let items = from_regions(&regions, &[0, 1, 2, 3, 4], &r, &[]);
        assert_eq!(
            texts(&items),
            [
                "P:premier",
                "D:[1]",
                "P:second",
                "D:[2]",
                "D:[3]",
                "P:troisieme",
                "D:[4]"
            ]
        );
        // Without any text, the drawings top to bottom.
        let only = [
            drawing(bounds(0.0, 3000.0, 1000.0, 1000.0), 0),
            drawing(bounds(0.0, 0.0, 1000.0, 1000.0), 1),
        ];
        let items = from_regions(&only, &[0, 1], &recognition(vec![]), &[]);
        assert_eq!(texts(&items), ["D:[1]", "D:[0]"]);
    }
}
