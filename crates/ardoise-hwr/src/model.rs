//! What a recognition gives, platform-free: words (text, alternatives, bounding box in page
//! units) grouped in lines. Serialised as is in the app's per-PC cache.

use serde::{Deserialize, Serialize};

use crate::Language;

/// A rectangle in page units (10 µm), origin at the top-left corner of the page, y down.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Bounds) -> Bounds {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Bounds {
            x,
            y,
            width: self.right().max(other.right()) - x,
            height: self.bottom().max(other.bottom()) - y,
        }
    }

    /// The smallest rectangle holding every one of `all` (`None` when empty).
    pub fn enclosing<'a>(all: impl IntoIterator<Item = &'a Bounds>) -> Option<Bounds> {
        all.into_iter().fold(None, |acc, b| match acc {
            None => Some(*b),
            Some(a) => Some(a.union(b)),
        })
    }
}

/// One recognised word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// The engine's best guess.
    pub text: String,
    /// The other candidates, best first (the best guess not repeated).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
    pub bounds: Bounds,
}

/// Words on one line, left to right.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub words: Vec<Word>,
    pub bounds: Bounds,
}

impl Line {
    /// A line of `words` (in reading order); its bounds hold them all.
    pub fn new(words: Vec<Word>) -> Line {
        let bounds = Bounds::enclosing(words.iter().map(|w| &w.bounds)).unwrap_or_default();
        Line { words, bounds }
    }

    /// The words separated by spaces.
    pub fn text(&self) -> String {
        let words: Vec<&str> = self.words.iter().map(|w| w.text.as_str()).collect();
        words.join(" ")
    }
}

/// The text recognised in a note, top to bottom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recognition {
    pub language: Language,
    /// Name of the recognizer used (empty when there was nothing to recognise).
    pub recognizer: String,
    pub lines: Vec<Line>,
}

impl Recognition {
    /// A recognition of nothing (a note without strokes).
    pub fn empty(language: Language) -> Recognition {
        Recognition {
            language,
            recognizer: String::new(),
            lines: Vec::new(),
        }
    }

    /// The lines, one per line of text.
    pub fn text(&self) -> String {
        let lines: Vec<String> = self.lines.iter().map(Line::text).collect();
        lines.join("\n")
    }

    /// Every word, in reading order.
    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.lines.iter().flat_map(|line| line.words.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.words.is_empty())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn word(text: &str, x: f32, y: f32, width: f32, height: f32) -> Word {
        Word {
            text: text.into(),
            alternatives: Vec::new(),
            bounds: Bounds {
                x,
                y,
                width,
                height,
            },
        }
    }

    #[test]
    fn a_recognition_reads_line_by_line() {
        let recognition = Recognition {
            language: Language::Fr,
            recognizer: "Français".into(),
            lines: vec![
                Line::new(vec![
                    word("le", 0.0, 0.0, 100.0, 50.0),
                    word("chat", 150.0, 10.0, 200.0, 60.0),
                ]),
                Line::new(vec![word("dort", 0.0, 100.0, 180.0, 50.0)]),
            ],
        };
        assert_eq!(recognition.text(), "le chat\ndort");
        assert_eq!(recognition.words().count(), 3);
        assert_eq!(
            recognition.lines[0].bounds,
            Bounds {
                x: 0.0,
                y: 0.0,
                width: 350.0,
                height: 70.0
            }
        );
        assert!(!recognition.is_empty());
        assert!(Recognition::empty(Language::En).is_empty());
        assert_eq!(Recognition::empty(Language::En).text(), "");
    }

    #[test]
    fn the_serialised_form_is_compact() {
        let w = word("chat", 1.0, 2.0, 3.0, 4.0);
        let json = serde_json::to_value(&w).unwrap();
        assert!(json.get("alternatives").is_none(), "{json}");
        let back: Word = serde_json::from_value(json).unwrap();
        assert_eq!(back, w);
    }
}
