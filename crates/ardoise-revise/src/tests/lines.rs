//! `ink_lines` on synthetic writing: tilted lines, descenders, dots and accents, a drawing.

use super::tidy::letter;
use super::*;

/// A small pseudo-random generator: the tests must not change from one run to the next.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    /// From `-reach` to `reach`.
    fn wobble(&mut self, reach: f32) -> f32 {
        (self.next() % 2001) as f32 / 1000.0 * reach - reach
    }
}

/// Handwriting along a line, and which stroke belongs where: letters, some with an ascender or a
/// descender, some with a dot above, words apart, a comma at the end of some.
struct Page {
    strokes: Vec<Stroke>,
    /// The numbers of each line, as written.
    lines: Vec<Vec<usize>>,
    noise: Noise,
    clock: u32,
}

impl Page {
    fn new() -> Self {
        Self {
            strokes: Vec::new(),
            lines: Vec::new(),
            noise: Noise(7),
            clock: 0,
        }
    }

    fn push(&mut self, stroke: Stroke, line: usize) {
        self.lines[line].push(self.strokes.len());
        self.strokes.push(stroke);
    }

    /// A line from `x0` to about `x1`, with its baseline through `base` at x = 0.
    fn line(&mut self, x0: f32, x1: f32, base: f32, slope: f32, body: f32) -> usize {
        let index = self.lines.len();
        self.lines.push(Vec::new());
        let mut x = x0;
        let mut in_word = 0;
        while x < x1 {
            let kind = self.noise.next() % 8;
            let width = body * 0.8;
            let drift = self.noise.wobble(body * 0.08);
            let here = base + drift;
            self.clock += 120;
            let t = self.clock;
            match kind {
                // An ascender: an l, a d.
                0 | 1 => self.push(letter(x, width, here, slope, body * 2.2, t), index),
                // A descender: a g, a p; the same stroke goes below the line.
                2 => self.push(
                    letter(x, width, here + body * 0.9, slope, body * 1.9, t),
                    index,
                ),
                // A letter and a dot above: an i.
                3 => {
                    self.push(letter(x, width * 0.5, here, slope, body, t), index);
                    let (dx, dy) = (x + width * 0.25, here + slope * x - body * 1.5);
                    self.push(
                        line_stroke([dx, dy], [dx + 0.2, dy + 0.1], t, t + 10),
                        index,
                    );
                }
                // An accent over a letter.
                4 => {
                    self.push(letter(x, width, here, slope, body, t), index);
                    let (ax, ay) = (x + width * 0.3, here + slope * x - body * 1.6);
                    self.push(
                        line_stroke([ax, ay], [ax + 0.5, ay - 0.5], t, t + 15),
                        index,
                    );
                }
                _ => self.push(letter(x, width, here, slope, body, t), index),
            }
            x += width + body * 0.3;
            in_word += 1;
            if in_word > 2 + (self.noise.next() % 5) as usize {
                in_word = 0;
                if self.noise.next().is_multiple_of(4) {
                    // A comma.
                    let (cx, cy) = (x, base + slope * x + 0.3);
                    self.push(
                        line_stroke([cx, cy], [cx - 0.3, cy + 0.7], t, t + 15),
                        index,
                    );
                }
                x += body * 1.8;
            }
        }
        index
    }

    /// All strokes, in the order they were written.
    fn visible(&self) -> Vec<usize> {
        (0..self.strokes.len()).collect()
    }

    fn check(&self, found: &InkLines) {
        assert_eq!(
            found.lines.len(),
            self.lines.len(),
            "{:?}",
            found
                .lines
                .iter()
                .map(|l| (l.strokes.len(), l.x0, l.x1, l.baseline_left))
                .collect::<Vec<_>>()
        );
        for (expected, line) in self.lines.iter().zip(&found.lines) {
            let missing: Vec<usize> = expected
                .iter()
                .copied()
                .filter(|n| !line.strokes.contains(n))
                .collect();
            let extra: Vec<usize> = line
                .strokes
                .iter()
                .copied()
                .filter(|n| !expected.contains(n))
                .collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "missing {missing:?}, extra {extra:?}"
            );
        }
    }
}

const BODY: f32 = 3.0;

#[test]
fn level_lines_are_found_in_their_order() {
    let mut page = Page::new();
    for k in 0..6 {
        page.line(20.0, 180.0, 30.0 + k as f32 * 10.0, 0.0, BODY);
    }
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
    assert!(found.apart.is_empty(), "{:?}", found.apart);
    for (k, line) in found.lines.iter().enumerate() {
        assert_near(line.baseline_left, 30.0 + k as f32 * 10.0, 0.6);
        assert_near(line.slope, 0.0, 0.3);
        assert_near(line.x0, 20.0, 0.5);
        assert_near(line.x1, 180.0, 6.0);
    }
}

#[test]
fn tilted_lines_are_found_though_they_overlap_across_the_page() {
    // 3.4 degrees over 240 mm: each line ends 14 mm lower than it starts, more than the 9 mm from
    // one line to the next. Clustering on heights alone cannot do it.
    let slope = 0.06;
    let mut page = Page::new();
    for k in 0..7 {
        page.line(20.0, 260.0, 20.0 + k as f32 * 9.0, slope, BODY);
    }
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
    for (k, line) in found.lines.iter().enumerate() {
        assert_near(line.slope, slope.atan().to_degrees(), 0.5);
        assert_near(
            line.baseline_left,
            20.0 + k as f32 * 9.0 + slope * 20.0,
            1.0,
        );
        assert!(line.baseline_right > line.baseline_left + 10.0);
    }
}

#[test]
fn lines_tilted_the_other_way_too() {
    let slope = -0.04;
    let mut page = Page::new();
    for k in 0..5 {
        page.line(30.0, 270.0, 40.0 + k as f32 * 11.0, slope, BODY);
    }
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
}

#[test]
fn lines_of_different_lengths_and_indents_are_found() {
    let mut page = Page::new();
    page.line(20.0, 200.0, 20.0, 0.01, BODY);
    page.line(20.0, 60.0, 30.0, 0.01, BODY);
    page.line(40.0, 200.0, 40.0, 0.01, BODY);
    page.line(40.0, 80.0, 50.0, 0.01, BODY);
    page.line(120.0, 190.0, 60.0, 0.01, BODY);
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
}

#[test]
fn a_tall_drawing_is_left_out_and_listed_apart() {
    let mut page = Page::new();
    for k in 0..4 {
        page.line(20.0, 180.0, 20.0 + k as f32 * 10.0, 0.0, BODY);
    }
    let circle: Vec<Point> = (0..=100)
        .map(|i| {
            let a = i as f32 / 100.0 * std::f32::consts::TAU;
            pt(100.0 + 30.0 * a.cos(), 40.0 + 30.0 * a.sin(), 90_000 + i)
        })
        .collect();
    let mut drawing = line_stroke([0.0, 0.0], [1.0, 1.0], 0, 1);
    drawing.points = circle;
    let number = page.strokes.len();
    page.strokes.push(drawing);
    // A vertical line, narrow but far taller than a letter, and crossing the writing.
    page.strokes
        .push(line_stroke([150.0, 5.0], [150.0, 70.0], 91_000, 91_500));
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
    assert_eq!(found.apart, [number, number + 1]);
}

#[test]
fn rules_and_highlights_are_not_writing() {
    let mut page = Page::new();
    for k in 0..3 {
        page.line(20.0, 180.0, 20.0 + k as f32 * 10.0, 0.0, BODY);
    }
    let rule = page.strokes.len();
    page.strokes
        .push(line_stroke([20.0, 22.0], [120.0, 22.0], 90_000, 90_100));
    let mut highlight = line_stroke([20.0, 28.0], [120.0, 29.0], 90_200, 90_300);
    highlight.tool = Tool::Highlighter;
    page.strokes.push(highlight);
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
    assert_eq!(found.apart, [rule, rule + 1]);
}

#[test]
fn a_tall_letter_stays_with_its_line() {
    let mut page = Page::new();
    let first = page.line(20.0, 180.0, 30.0, 0.0, BODY);
    page.line(20.0, 180.0, 45.0, 0.0, BODY);
    // A capital three times the size of the rest, 12 mm for a 15 mm pitch.
    let stroke = letter(190.0, 3.0, 30.0, 0.0, 12.0, 80_000);
    let number = page.strokes.len();
    page.push(stroke, first);
    let found = ink_lines(&page.strokes, &page.visible());
    assert!(
        found.lines[0].strokes.contains(&number),
        "{:?}",
        found.apart
    );
    assert!(found.apart.is_empty(), "{:?}", found.apart);
}

#[test]
fn only_the_strokes_said_visible_count() {
    let mut page = Page::new();
    page.line(20.0, 180.0, 20.0, 0.0, BODY);
    let second = page.line(20.0, 180.0, 30.0, 0.0, BODY);
    let shown: Vec<usize> = page.lines[second].clone();
    let found = ink_lines(&page.strokes, &shown);
    assert_eq!(found.lines.len(), 1);
    assert_eq!(found.lines[0].strokes, shown);
    // Numbers given twice or that are not strokes do no harm.
    let mut twice = shown.clone();
    twice.extend_from_slice(&shown);
    twice.push(1_000_000);
    assert_eq!(ink_lines(&page.strokes, &twice), found);
}

#[test]
fn nothing_to_find_finds_nothing() {
    let empty = ink_lines(&[], &[]);
    assert!(empty.lines.is_empty() && empty.apart.is_empty());
    // A stroke without points or samples that can be drawn is in no list.
    let mut stroke = line_stroke([0.0, 0.0], [1.0, 1.0], 0, 1);
    stroke.points.clear();
    let mut bad = line_stroke([0.0, 0.0], [1.0, 1.0], 0, 1);
    for p in &mut bad.points {
        p.x = f32::NAN;
    }
    let found = ink_lines(&[stroke, bad], &[0, 1]);
    assert!(found.lines.is_empty() && found.apart.is_empty());
}

#[test]
fn a_single_stroke_is_a_line() {
    let strokes = [letter(20.0, 30.0, 40.0, 0.0, 3.0, 0)];
    let found = ink_lines(&strokes, &[0]);
    assert_eq!(found.lines.len(), 1);
    assert_eq!(found.lines[0].strokes, [0]);
    assert_eq!(found.lines[0].words.len(), 1);
}

#[test]
fn a_single_line_is_one_line() {
    let mut page = Page::new();
    page.line(20.0, 200.0, 40.0, 0.03, BODY);
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
}

#[test]
fn two_blocks_side_by_side_are_not_one_line() {
    let mut page = Page::new();
    page.line(20.0, 80.0, 30.0, 0.0, BODY);
    page.line(160.0, 220.0, 30.0, 0.0, BODY);
    let found = ink_lines(&page.strokes, &page.visible());
    assert_eq!(found.lines.len(), 2, "{:?}", found.lines);
}

#[test]
fn words_are_given_as_tidy_line_cuts_them() {
    let mut page = Page::new();
    for k in 0..4 {
        page.line(20.0, 220.0, 25.0 + k as f32 * 10.0, 0.02, BODY);
    }
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
    for line in &found.lines {
        assert!(line.words.len() > 3, "{}", line.words.len());
        // Every stroke of the line is in a word, words go left to right, without overlapping.
        let mut in_words: Vec<usize> = line.words.iter().flat_map(|w| w.strokes.clone()).collect();
        in_words.sort_unstable();
        assert_eq!(in_words, line.strokes);
        for pair in line.words.windows(2) {
            assert!(pair[0].x1 < pair[1].x0);
        }
        let order = Order::TidyLine {
            select: Select {
                ids: Some(line.strokes.clone()),
                ..Select::default()
            },
            baseline: None,
            left: None,
            mode: None,
            scale: None,
            slope: None,
            gaps: None,
            word_gap: None,
            smooth: None,
        };
        let revised = apply(&input(&page.strokes), &[order]).unwrap();
        let report = revised.report.orders[0].line.as_ref().unwrap();
        assert_eq!(report.words, line.words.len());
        assert_eq!(report.strokes, line.strokes.len());
        assert_near(report.slope, line.slope, 0.01);
        assert_near(report.body, line.body, 0.01);
    }
}

#[test]
fn a_dense_page_and_a_scribble_take_no_long_time() {
    // 60 lines of writing, and 5,000 tiny strokes in one place.
    let mut page = Page::new();
    for k in 0..60 {
        page.line(10.0, 280.0, 10.0 + k as f32 * 5.0, 0.0, 1.5);
    }
    let writing = page.strokes.len();
    assert!(writing > 7000, "{writing}");
    for i in 0..5000 {
        let x = 100.0 + (i % 70) as f32 * 0.3;
        let y = 400.0 + (i % 50) as f32 * 0.2;
        page.strokes
            .push(line_stroke([x, y], [x + 1.5, y + 1.5], i, i + 10));
    }
    let start = std::time::Instant::now();
    let found = ink_lines(&page.strokes, &page.visible());
    let taken = start.elapsed();
    eprintln!(
        "{} strokes, {} lines, {taken:?}",
        page.strokes.len(),
        found.lines.len()
    );
    assert!(found.lines.len() >= 60);
    assert!(taken.as_secs() < 60, "{taken:?}");
}

#[test]
fn tidying_line_by_line_levels_the_page() {
    let slope = 0.05;
    let mut page = Page::new();
    for k in 0..5 {
        page.line(20.0, 260.0, 20.0 + k as f32 * 10.0, slope, BODY);
    }
    let found = ink_lines(&page.strokes, &page.visible());
    page.check(&found);
    let orders: Vec<Order> = found
        .lines
        .iter()
        .map(|line| Order::TidyLine {
            select: Select {
                ids: Some(line.strokes.clone()),
                ..Select::default()
            },
            baseline: None,
            left: None,
            mode: None,
            scale: None,
            slope: None,
            gaps: None,
            word_gap: None,
            smooth: None,
        })
        .collect();
    let revised = apply(&input(&page.strokes), &orders).unwrap();
    let visible: Vec<usize> = (0..revised.strokes.len()).collect();
    let after = ink_lines(&revised.strokes, &visible);
    assert_eq!(after.lines.len(), found.lines.len());
    for line in &after.lines {
        assert_near(line.slope, 0.0, 0.3);
    }
}
