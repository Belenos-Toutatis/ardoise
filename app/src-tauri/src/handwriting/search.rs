//! Search in the recognised text. The library search of the interface (`matchesSearch` in
//! `app/ui/src/lib/library-logic.ts`) matches every word of the query in the note's title, tags
//! or binder; it asks this module which query words the handwriting of each note has, and an
//! excerpt that shows why a note matched.
//!
//! Like the titles, accents and case are ignored (the query is split and folded the same way as
//! in the interface). A query word is found where a recognised word begins with it (`lum` finds
//! "lumière", not "allumer"): recognised text holds stray words from drawings, which matching
//! anywhere inside words would let through too easily. The engine's other candidates for a word
//! count too, since its first guess is sometimes wrong.

use ardoise_hwr::Recognition;
use ardoise_hwr::text::fold;
use serde::Serialize;

/// Words of context kept on each side of the found word.
const CONTEXT_WORDS: usize = 4;

/// The query split as the interface splits it: folded, at white space.
pub fn query_words(query: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in fold(query).split_whitespace() {
        if !words.iter().any(|w| w == word) {
            words.push(word.to_owned());
        }
    }
    words
}

/// A line of recognised text with its folded form, where each folded character comes from.
struct FoldedLine {
    text: Vec<char>,
    folded: Vec<char>,
    /// Index in `text` of each character of `folded`.
    origin: Vec<usize>,
    /// Each word's range in `text` and its candidates folded (first guess included).
    words: Vec<(usize, usize, Vec<String>)>,
}

impl FoldedLine {
    fn new(words: &[ardoise_hwr::Word]) -> FoldedLine {
        let mut text: Vec<char> = Vec::new();
        let mut ranges = Vec::new();
        for (i, word) in words.iter().enumerate() {
            if i > 0 {
                text.push(' ');
            }
            let start = text.len();
            text.extend(word.text.chars());
            let candidates = std::iter::once(&word.text)
                .chain(word.alternatives.iter())
                .map(|c| fold(c))
                .collect();
            ranges.push((start, text.len(), candidates));
        }
        let mut folded = Vec::with_capacity(text.len());
        let mut origin = Vec::with_capacity(text.len());
        for (index, c) in text.iter().enumerate() {
            for f in fold(&c.to_string()).chars() {
                folded.push(f);
                origin.push(index);
            }
        }
        FoldedLine {
            text,
            folded,
            origin,
            words: ranges,
        }
    }

    /// Where `word` (folded) starts a word of the line: the range in `text` of the word it is
    /// found in.
    fn find(&self, word: &str) -> Option<(usize, usize)> {
        let needle: Vec<char> = word.chars().collect();
        if needle.is_empty() || needle.len() > self.folded.len() {
            return self.find_in_candidates(word);
        }
        for at in 0..=self.folded.len() - needle.len() {
            let word_start = at == 0 || !self.folded[at - 1].is_alphanumeric();
            if word_start && self.folded[at..at + needle.len()] == needle[..] {
                let start = self.origin[at];
                // The whole recognised word it is in.
                let range = self
                    .words
                    .iter()
                    .find(|(s, e, _)| (*s..*e).contains(&start))
                    .map_or((start, start + 1), |(s, e, _)| (*s, *e));
                return Some(range);
            }
        }
        self.find_in_candidates(word)
    }

    /// A word whose other candidates begin with `word`.
    fn find_in_candidates(&self, word: &str) -> Option<(usize, usize)> {
        self.words
            .iter()
            .find(|(_, _, candidates)| candidates.iter().skip(1).any(|c| c.starts_with(word)))
            .map(|(s, e, _)| (*s, *e))
    }

    /// The excerpt around `range`: a few words on each side, `…` where the line is cut.
    fn excerpt(&self, (start, end): (usize, usize)) -> Excerpt {
        let before: String = self.text[..start].iter().collect();
        let after: String = self.text[end..].iter().collect();
        let before_words: Vec<&str> = before.split_whitespace().collect();
        let after_words: Vec<&str> = after.split_whitespace().collect();
        let mut before_text =
            before_words[before_words.len().saturating_sub(CONTEXT_WORDS)..].join(" ");
        if before_words.len() > CONTEXT_WORDS {
            before_text.insert_str(0, "… ");
        }
        if !before_text.is_empty() {
            before_text.push(' ');
        }
        let mut after_text = after_words[..after_words.len().min(CONTEXT_WORDS)].join(" ");
        if after_words.len() > CONTEXT_WORDS {
            after_text.push_str(" …");
        }
        if !after_text.is_empty() {
            after_text.insert(0, ' ');
        }
        Excerpt {
            before: before_text,
            word: self.text[start..end].iter().collect(),
            after: after_text,
        }
    }
}

/// A short excerpt of the recognised text: `before`, the found `word`, `after`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Excerpt {
    pub before: String,
    pub word: String,
    pub after: String,
}

/// What the handwriting of one note has of a query (`HandwritingHit` in `handwriting.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    pub path: String,
    /// The query words found (folded, as `query_words` gives them).
    pub words: Vec<String>,
    /// Around the first word found in reading order.
    pub excerpt: Excerpt,
}

/// The query words found in `recognition` and the excerpt, or `None` when none is found.
pub fn search_recognition(
    recognition: &Recognition,
    words: &[String],
) -> Option<(Vec<String>, Excerpt)> {
    let lines: Vec<FoldedLine> = recognition
        .lines
        .iter()
        .map(|line| FoldedLine::new(&line.words))
        .collect();
    let mut found = Vec::new();
    // The earliest match in reading order: (line, start, end).
    let mut first: Option<(usize, usize, usize)> = None;
    for word in words {
        let hit = lines
            .iter()
            .enumerate()
            .find_map(|(index, line)| line.find(word).map(|(s, e)| (index, s, e)));
        if let Some(hit) = hit {
            found.push(word.clone());
            if first.is_none_or(|f| (hit.0, hit.1) < (f.0, f.1)) {
                first = Some(hit);
            }
        }
    }
    let (line, start, end) = first?;
    Some((found, lines[line].excerpt((start, end))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_hwr::{Bounds, Language, Line, Word};

    fn recognition(lines: &[&str]) -> Recognition {
        Recognition {
            language: Language::Fr,
            recognizer: "test".into(),
            lines: lines
                .iter()
                .map(|line| {
                    Line::new(
                        line.split(' ')
                            .map(|w| Word {
                                text: w.into(),
                                alternatives: Vec::new(),
                                bounds: Bounds::default(),
                            })
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    fn excerpt(e: &Excerpt) -> String {
        format!("{}[{}]{}", e.before, e.word, e.after)
    }

    #[test]
    fn the_query_is_split_and_folded_like_the_interface() {
        assert_eq!(
            query_words("  Lumière  ÉCOLE lumiere "),
            ["lumiere", "ecole"]
        );
        assert!(query_words("   ").is_empty());
        assert_eq!(query_words("l'eau km/h"), ["l'eau", "km/h"]);
    }

    #[test]
    fn words_are_found_at_word_starts_without_accents_or_case() {
        let text = recognition(&[
            "La lumière se propage en ligne droite",
            "dans un milieu homogène",
        ]);
        let (found, e) = search_recognition(&text, &query_words("LUMIERE")).unwrap();
        assert_eq!(found, ["lumiere"]);
        assert_eq!(excerpt(&e), "La [lumière] se propage en ligne …");
        // A beginning of word is enough; the middle of a word is not.
        assert!(search_recognition(&text, &query_words("homo")).is_some());
        assert!(search_recognition(&text, &query_words("mogene")).is_none());
        assert!(search_recognition(&text, &query_words("chat")).is_none());
        // Several words: those found are listed, the excerpt is at the first in the text.
        let (found, e) = search_recognition(&text, &query_words("milieu chat droite")).unwrap();
        assert_eq!(found, ["milieu", "droite"]);
        assert_eq!(excerpt(&e), "… se propage en ligne [droite]");
    }

    #[test]
    fn excerpts_keep_a_few_words_around() {
        let text = recognition(&["un deux trois quatre cinq six sept huit neuf dix onze"]);
        let (_, e) = search_recognition(&text, &query_words("six")).unwrap();
        assert_eq!(
            excerpt(&e),
            "… deux trois quatre cinq [six] sept huit neuf dix …"
        );
        let (_, e) = search_recognition(&text, &query_words("un")).unwrap();
        assert_eq!(excerpt(&e), "[un] deux trois quatre cinq …");
        // Punctuation inside the query is kept; the whole recognised word is shown.
        let text = recognition(&["vitesse de 1000 km/h. environ"]);
        let (found, e) = search_recognition(&text, &query_words("km/h")).unwrap();
        assert_eq!(found, ["km/h"]);
        assert_eq!(excerpt(&e), "vitesse de 1000 [km/h.] environ");
    }

    #[test]
    fn other_candidates_of_a_word_count() {
        let mut text = recognition(&["la lumicre du jour"]);
        text.lines[0].words[1].alternatives = vec!["lumière".into(), "lumiere".into()];
        let (found, e) = search_recognition(&text, &query_words("lumière")).unwrap();
        assert_eq!(found, ["lumiere"]);
        assert_eq!(excerpt(&e), "la [lumicre] du jour");
        assert!(search_recognition(&recognition(&[]), &query_words("x")).is_none());
        assert!(search_recognition(&text, &[]).is_none());
    }
}
