//! Accent- and case-blind text, as the library search of the interface compares titles
//! (`normalize` in `app/ui/src/lib/library-logic.ts`: Unicode canonical decomposition, combining
//! marks removed, lower case).

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Lower case without accents: `"Réunion ÉCOLE"` → `"reunion ecole"`.
pub fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The words of `text`: runs of letters and digits (apostrophes and hyphens split words, as
/// punctuation does).
pub fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The words of `text`, folded.
pub fn folded_words(text: &str) -> Vec<String> {
    words(text).into_iter().map(fold).collect()
}

/// `text` without punctuation around it and in lower case, accents kept: how a recognised word
/// is compared with the expected one when accents count.
pub fn bare(text: &str) -> String {
    text.trim_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_drops_accents_and_case() {
        assert_eq!(fold("Réunion ÉCOLE"), "reunion ecole");
        assert_eq!(fold("garçon forêt où île Noël"), "garcon foret ou ile noel");
        // Like the interface: a letter that does not decompose stays as it is.
        assert_eq!(fold("Œuvre"), "œuvre");
    }

    #[test]
    fn words_are_runs_of_letters_and_digits() {
        assert_eq!(
            words("L'élève, 3e B — peut-être."),
            ["L", "élève", "3e", "B", "peut", "être"]
        );
        assert_eq!(folded_words("Forêt  CALME"), ["foret", "calme"]);
        assert!(words(" ,; ").is_empty());
    }

    #[test]
    fn bare_words_keep_their_accents() {
        assert_eq!(bare("«Forêt»,"), "forêt");
        assert_eq!(bare("Été."), "été");
    }
}
