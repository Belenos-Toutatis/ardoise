//! Offline handwriting recognition through Windows (SPEC §4.7, §6.2 "Écriture").
//!
//! - [`model`]: what a recognition gives (words with their alternatives and bounding boxes in
//!   page units, grouped in lines), platform-free;
//! - [`Recognizer`]: the engine, a trait so that the app's indexing logic is tested with a fake;
//! - [`SystemRecognizer`]: Windows' own engine (`Windows.UI.Input.Inking`:
//!   `InkStrokeBuilder` → `InkStrokeContainer` → `InkRecognizerContainer::RecognizeAsync`),
//!   running on a worker thread of its own; outside Windows it answers
//!   [`HwrError::Unsupported`];
//! - [`ink`]: which strokes of a note are recognised and how they are scaled for the engine;
//! - [`lines`]: words grouped in lines;
//! - [`text`]: accent- and case-blind comparison of words;
//! - [`synthetic`]: handwriting-like single-stroke glyphs (publishable synthetic notes with a
//!   known text, for the reference test and the demo library).
//!
//! The engine recognises one language at a time: French, or English when Windows' English
//! handwriting pack is installed (the installed recognizers are listed, nothing is ever
//! downloaded).

pub mod document;
pub mod docx;
pub mod ink;
pub mod lines;
pub mod markdown;
pub mod model;
pub mod structure;
pub mod synthetic;
mod system;
pub mod text;

pub use model::{Bounds, Line, Recognition, Word};
pub use system::SystemRecognizer;

use ardoise_ink::Stroke;
use serde::{Deserialize, Serialize};

/// A language the app recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Fr,
    En,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Fr, Language::En];

    /// `"fr"` or `"en"`.
    pub fn code(self) -> &'static str {
        match self {
            Language::Fr => "fr",
            Language::En => "en",
        }
    }
}

/// An installed recognizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognizerInfo {
    /// As Windows names it, in the language of Windows ("Reconnaissance d'écriture Microsoft -
    /// Français").
    pub name: String,
    /// The language it recognises, when it is one the app knows.
    pub language: Option<Language>,
}

/// The language of a recognizer, from its name. Windows gives no language code, only a name in
/// the language of its own interface: "Reconnaissance d'écriture Microsoft - Français",
/// "Microsoft English (US) Handwriting Recognizer", "Microsoft-Handschrifterkennung - Englisch"…
pub fn language_of(name: &str) -> Option<Language> {
    let name = text::fold(name);
    let has = |parts: &[&str]| parts.iter().any(|part| name.contains(part));
    if has(&[
        "francais", "french", "franzos", "frances", "francese", "frans",
    ]) {
        Some(Language::Fr)
    } else if has(&[
        "english", "anglais", "englisch", "ingles", "inglese", "engels",
    ]) {
        Some(Language::En)
    } else {
        None
    }
}

/// The first recognizer of `language` in `recognizers`.
pub fn recognizer_for(
    recognizers: &[RecognizerInfo],
    language: Language,
) -> Option<&RecognizerInfo> {
    recognizers.iter().find(|r| r.language == Some(language))
}

/// Failure of a recognition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HwrError {
    /// Not Windows: there is no recognition engine.
    #[error("handwriting recognition is only available on Windows")]
    Unsupported,
    /// No recognizer of this language is installed on this computer.
    #[error("no {} handwriting recognizer is installed", language.code())]
    NoRecognizer { language: Language },
    /// The engine failed.
    #[error("handwriting recognition failed ({step}): {message}")]
    Engine { step: &'static str, message: String },
    /// The worker thread of the engine stopped.
    #[error("the handwriting recognition thread stopped")]
    Stopped,
}

impl HwrError {
    /// Stable identifier for the interface (`errorCodes.*`).
    pub fn code(&self) -> &'static str {
        match self {
            HwrError::Unsupported => "handwritingUnsupported",
            HwrError::NoRecognizer { .. } => "handwritingNoRecognizer",
            HwrError::Engine { .. } | HwrError::Stopped => "handwritingFailed",
        }
    }
}

/// A handwriting recognition engine.
pub trait Recognizer: Send + Sync {
    /// The recognizers installed on this computer.
    fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError>;

    /// Recognises `strokes` (page coordinates, already chosen with
    /// [`ink::recognizable_strokes`]) with the recognizer of `language`. Blocks until done.
    fn recognize(&self, strokes: &[Stroke], language: Language) -> Result<Recognition, HwrError>;

    /// The layout of `strokes`: which of them make paragraphs, list items and drawings
    /// ([`structure::Region`]; stroke positions are positions in `strokes`). Windows' ink
    /// analysis; engines without one answer [`HwrError::Unsupported`], and the export then
    /// falls back on [`structure::from_recognition`].
    fn analyze(&self, strokes: &[Stroke]) -> Result<Vec<structure::Region>, HwrError> {
        let _ = strokes;
        Err(HwrError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizer_languages_come_from_their_names() {
        for (name, language) in [
            (
                "Reconnaissance d'écriture Microsoft - Français",
                Some(Language::Fr),
            ),
            (
                "Microsoft French Handwriting Recognizer",
                Some(Language::Fr),
            ),
            (
                "Microsoft English (US) Handwriting Recognizer",
                Some(Language::En),
            ),
            (
                "Reconnaissance d'écriture Microsoft - Anglais (Royaume-Uni)",
                Some(Language::En),
            ),
            (
                "Microsoft-Handschrifterkennung - Englisch",
                Some(Language::En),
            ),
            (
                "Reconocimiento de escritura a mano de Microsoft - Español",
                None,
            ),
            ("", None),
        ] {
            assert_eq!(language_of(name), language, "{name}");
        }
    }

    #[test]
    fn the_first_recognizer_of_a_language_is_chosen() {
        let list = vec![
            RecognizerInfo {
                name: "Español".into(),
                language: None,
            },
            RecognizerInfo {
                name: "English (UK)".into(),
                language: Some(Language::En),
            },
            RecognizerInfo {
                name: "English (US)".into(),
                language: Some(Language::En),
            },
        ];
        assert_eq!(
            recognizer_for(&list, Language::En).map(|r| r.name.as_str()),
            Some("English (UK)")
        );
        assert_eq!(recognizer_for(&list, Language::Fr), None);
    }

    #[test]
    fn errors_have_codes_for_the_interface() {
        assert_eq!(HwrError::Unsupported.code(), "handwritingUnsupported");
        let missing = HwrError::NoRecognizer {
            language: Language::En,
        };
        assert_eq!(missing.code(), "handwritingNoRecognizer");
        assert!(missing.to_string().contains("en"));
        assert_eq!(HwrError::Stopped.code(), "handwritingFailed");
        assert_eq!(
            serde_json::to_string(&Language::Fr).unwrap(),
            "\"fr\"".to_owned()
        );
    }
}
