//! Words grouped in lines. Windows' engine analyses the layout itself and gives the words in
//! reading order, line after line (checked on real notes: sorting the words by position mixes
//! the lines of a page with formulas or large signs, the engine's order does not). Its lines
//! are only implicit: a new line starts where the next word goes down and back to the start of
//! the line (or far up, to another column).

use crate::model::{Line, Word};

/// A word lower than the previous one starts a line when they overlap vertically by less
/// than this share of the smaller height (`le` then `garçon`: an ascender, then a descender,
/// overlap by more).
const NEW_LINE_OVERLAP: f32 = 0.3;

/// A new line starts at most this many heights to the right of where the current one starts
/// (an indented line still starts a line; a denominator under its fraction bar does not).
const NEW_LINE_INDENT: f32 = 1.5;

/// How many heights the next word must go up to start a line (the top of a second column; a
/// numerator over its fraction bar goes up less).
const NEW_LINE_RISE: f32 = 2.0;

/// Whether `next`, the word after `previous` in reading order on a line that starts at
/// `line_left`, starts a new line: it goes down below `previous` and back to the start of the
/// line, or far up.
fn starts_line(line_left: f32, previous: &Word, next: &Word) -> bool {
    let (p, n) = (&previous.bounds, &next.bounds);
    let height = p.height.max(n.height).max(1.0);
    let smaller = p.height.min(n.height).max(1.0);
    let drop = (n.y + n.height / 2.0) - (p.y + p.height / 2.0);
    let overlap = (p.bottom().min(n.bottom()) - p.y.max(n.y)).max(0.0) / smaller;
    let back = n.x < line_left + NEW_LINE_INDENT * height;
    (back && drop > 0.0 && overlap < NEW_LINE_OVERLAP) || -drop > NEW_LINE_RISE * height
}

/// Splits `words`, in the engine's reading order, into lines (the order is kept).
pub fn group_lines(words: Vec<Word>) -> Vec<Line> {
    let mut lines: Vec<Vec<Word>> = Vec::new();
    for word in words {
        let joins = lines
            .last()
            .is_some_and(|line| match (line.first(), line.last()) {
                (Some(first), Some(last)) => !starts_line(first.bounds.x, last, &word),
                _ => false,
            });
        match lines.last_mut() {
            Some(line) if joins => line.push(word),
            _ => lines.push(vec![word]),
        }
    }
    lines.into_iter().map(Line::new).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::word;

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(Line::text).collect()
    }

    #[test]
    fn a_new_line_starts_back_on_the_left_and_lower() {
        let words = vec![
            // An ascender makes this word taller than the next one.
            word("le", 0.0, -100.0, 200.0, 400.0),
            word("chat", 500.0, 20.0, 300.0, 300.0),
            word("dort", 0.0, 400.0, 300.0, 300.0),
            word("bien", 400.0, 430.0, 300.0, 280.0),
            // Ascender, then descender: still the same line.
            word("le", 0.0, 900.0, 200.0, 560.0),
            word("garçon", 300.0, 1140.0, 600.0, 560.0),
        ];
        let lines = group_lines(words);
        assert_eq!(texts(&lines), ["le chat", "dort bien", "le garçon"]);
        assert!(lines[0].bounds.y < lines[1].bounds.y);
    }

    #[test]
    fn a_line_going_down_or_a_fraction_stays_one_line() {
        // A sloping line.
        let words: Vec<Word> = (0..6)
            .map(|i| {
                word(
                    &format!("w{i}"),
                    i as f32 * 400.0,
                    i as f32 * 60.0,
                    350.0,
                    300.0,
                )
            })
            .collect();
        assert_eq!(texts(&group_lines(words)), ["w0 w1 w2 w3 w4 w5"]);
        // A denominator written under its numerator, then the rest of the line.
        let words = vec![
            word("E", 0.0, 0.0, 200.0, 300.0),
            word("=", 300.0, 100.0, 200.0, 100.0),
            word("1200", 600.0, -200.0, 400.0, 250.0),
            word("60", 650.0, 150.0, 300.0, 250.0),
            word("Wh", 1100.0, 0.0, 300.0, 300.0),
        ];
        assert_eq!(texts(&group_lines(words)), ["E = 1200 60 Wh"]);
    }

    #[test]
    fn the_reading_order_of_the_engine_is_kept() {
        let words = vec![
            word("a", 0.0, 0.0, 100.0, 100.0),
            word("b", 0.0, 1000.0, 100.0, 100.0),
            word("flat", 0.0, 2000.0, 100.0, 0.0),
            // A second column, read after the first.
            word("c", 5000.0, 0.0, 100.0, 100.0),
        ];
        assert_eq!(texts(&group_lines(words)), ["a", "b", "flat", "c"]);
        assert!(group_lines(Vec::new()).is_empty());
    }
}
