//! Reference test of the recognition (ROADMAP M7: "reconnaissance vérifiée sur des notes de
//! référence avec le taux de mots reconnus affiché"). Synthetic notes with a known text
//! (`ardoise_hwr::synthetic`, never anyone's notes), written in French and in English with a
//! few variations of the hand, are recognised by Windows' engine; the rate of recognised words
//! is printed per language and must reach the threshold below. A language whose recognizer is
//! not installed on this PC is skipped with a message.
//!
//! `cargo test -p ardoise-hwr --test reference -- --nocapture` shows the rates;
//! `-- --ignored --nocapture` also compares the scales handed to the engine.

#![cfg(windows)]

use ardoise_hwr::ink::Scale;
use ardoise_hwr::synthetic::Hand;
use ardoise_hwr::text::{bare, fold, words};
use ardoise_hwr::{Language, Recognition, Recognizer, SystemRecognizer, recognizer_for};

/// Minimum share of the expected words recognised (accents ignored, as the search does).
const THRESHOLD: f64 = 0.75;

/// The hands the notes are written with: seed of the synthetic writer and amount of variation
/// (from a tidy hand to a hasty one).
const HANDS: [(u64, f32); 3] = [(11, 0.6), (23, 1.5), (37, 2.5)];

const FRENCH: &[&str] = &[
    "le chat dort sur le canapé",
    "nous étudions la lumière et les ondes",
    "la forêt est très calme ce matin",
    "les élèves préparent une expérience",
    "le garçon mange une pomme verte",
    "il fait beau à la plage",
];

const ENGLISH: &[&str] = &[
    "the quick brown fox jumps over the lazy dog",
    "students measure the speed of sound",
    "we planted seven trees in the garden",
    "my teacher writes on the board",
    "this river flows into the sea",
];

fn sentences(language: Language) -> &'static [&'static str] {
    match language {
        Language::Fr => FRENCH,
        Language::En => ENGLISH,
    }
}

/// Length of the longest common subsequence of two word lists (recognised words in the right
/// order, extra or missing words not counted twice).
fn common(expected: &[String], found: &[String]) -> usize {
    let mut row = vec![0usize; found.len() + 1];
    for e in expected {
        let mut diagonal = 0;
        for (j, f) in found.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if e == f {
                diagonal + 1
            } else {
                row[j + 1].max(row[j])
            };
            diagonal = above;
        }
    }
    row[found.len()]
}

/// Words recognised, accents ignored and accents counted.
#[derive(Debug, Default, Clone, Copy)]
struct Score {
    expected: usize,
    folded: usize,
    exact: usize,
}

impl Score {
    fn add(&mut self, other: Score) {
        self.expected += other.expected;
        self.folded += other.folded;
        self.exact += other.exact;
    }

    fn rate(&self) -> f64 {
        self.folded as f64 / self.expected.max(1) as f64
    }

    fn exact_rate(&self) -> f64 {
        self.exact as f64 / self.expected.max(1) as f64
    }
}

fn score(expected: &str, recognition: &Recognition) -> Score {
    let text = recognition.text();
    let expected_words = words(expected);
    let found_words = words(&text);
    let folded = |list: &[&str]| list.iter().map(|w| fold(w)).collect::<Vec<_>>();
    let exact = |list: &[&str]| list.iter().map(|w| bare(w)).collect::<Vec<_>>();
    Score {
        expected: expected_words.len(),
        folded: common(&folded(&expected_words), &folded(&found_words)),
        exact: common(&exact(&expected_words), &exact(&found_words)),
    }
}

/// Recognises every sentence of `language`, written with each hand as one note of several
/// lines; prints what was read and returns the score.
fn run(
    recognizer: &SystemRecognizer,
    language: Language,
    hands: &[(u64, f32)],
    verbose: bool,
) -> Score {
    let text = sentences(language).join("\n");
    let mut total = Score::default();
    for &(seed, jitter) in hands {
        let hand = Hand {
            seed,
            jitter,
            ..Hand::default()
        };
        let strokes = hand.write(&text);
        let recognition = match recognizer.recognize(&strokes, language) {
            Ok(recognition) => recognition,
            Err(error) => panic!("recognition failed ({}): {error}", language.code()),
        };
        let score = score(&text, &recognition);
        if verbose {
            println!(
                "  [{} seed {seed}, jitter {jitter}] {}/{} words: {:?}",
                language.code(),
                score.folded,
                score.expected,
                recognition.text()
            );
        }
        total.add(score);
    }
    total
}

#[test]
fn reference_notes_are_recognised() {
    let recognizer = SystemRecognizer::new();
    let installed = recognizer.recognizers().expect("list of recognizers");
    println!("Recognizers on this PC:");
    for r in &installed {
        println!(
            "  {} ({})",
            r.name,
            r.language.map_or("other language", Language::code)
        );
    }
    let mut tested = 0;
    for language in Language::ALL {
        if recognizer_for(&installed, language).is_none() {
            println!(
                "[{}] skipped: no recognizer for this language on this PC",
                language.code()
            );
            continue;
        }
        let started = std::time::Instant::now();
        let score = run(&recognizer, language, &HANDS, true);
        println!(
            "[{}] recognised words: {:.1} % ({}/{}), with accents {:.1} %, threshold {:.0} % ({} ms)",
            language.code(),
            score.rate() * 100.0,
            score.folded,
            score.expected,
            score.exact_rate() * 100.0,
            THRESHOLD * 100.0,
            started.elapsed().as_millis()
        );
        assert!(
            score.rate() >= THRESHOLD,
            "{}: {:.1} % of the words recognised, below {:.0} %",
            language.code(),
            score.rate() * 100.0,
            THRESHOLD * 100.0
        );
        tested += 1;
    }
    if tested == 0 {
        println!("no recognizer for French or English on this PC: nothing tested");
    }
}

/// Compares the enlargements of the strokes beyond their physical size (the choice of
/// `ink::RECOGNITION_SCALE`).
#[test]
#[ignore = "measurement: run with --ignored --nocapture"]
fn compare_scales() {
    for enlargement in [0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0] {
        let recognizer = SystemRecognizer::with_scale(Scale::enlarged(enlargement));
        let installed = recognizer.recognizers().expect("list of recognizers");
        for language in Language::ALL {
            if recognizer_for(&installed, language).is_none() {
                continue;
            }
            for jitter in [0.6, 1.5, 2.5, 3.5] {
                let hands: Vec<(u64, f32)> =
                    HANDS.iter().map(|&(seed, _)| (seed, jitter)).collect();
                let score = run(&recognizer, language, &hands, false);
                println!(
                    "scale ×{enlargement}, jitter {jitter}: [{}] {:.1} % (with accents {:.1} %)",
                    language.code(),
                    score.rate() * 100.0,
                    score.exact_rate() * 100.0
                );
            }
        }
    }
}

#[test]
fn the_score_counts_words_in_order() {
    let to = |s: &str| words(s).iter().map(|w| fold(w)).collect::<Vec<_>>();
    assert_eq!(common(&to("le chat dort"), &to("le chat dort")), 3);
    assert_eq!(common(&to("le chat dort"), &to("Le CHAT ici dort")), 3);
    assert_eq!(common(&to("le chat dort"), &to("dort chat le")), 1);
    assert_eq!(common(&to("le chat"), &to("")), 0);
}
