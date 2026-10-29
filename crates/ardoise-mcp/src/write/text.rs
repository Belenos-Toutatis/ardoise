//! `set_note_text`: a corrected transcription kept in the handwriting cache of this PC.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::update::summary_of;
use crate::error::{ToolError, ToolResult, json_result};
use crate::host::Host;
use crate::notes::{find_note, read_note};
use crate::read::TextLanguage;

/// Longest transcription kept, in characters (a note is a page or two of handwriting).
const MAX_CHARS: usize = 200_000;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetNoteTextArgs {
    /// The note's `id`, or its path relative to the library.
    pub note: String,
    /// The corrected text of the whole note, lines separated by newlines. Empty removes the correction.
    pub text: String,
    /// Language of the text (default: the app's language). It only labels the correction: the search uses it whatever the recognition language.
    pub language: Option<TextLanguage>,
}

pub fn set_note_text(host: &Host, args: SetNoteTextArgs) -> ToolResult {
    let text = args.text.replace("\r\n", "\n");
    let text = text.trim();
    if text.chars().count() > MAX_CHARS {
        return Err(ToolError::invalid(format!(
            "`text` is longer than {MAX_CHARS} characters"
        )));
    }
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    let loaded = read_note(&library, &entry)?;
    let language = args
        .language
        .map_or_else(|| host.environment.language(), Into::into);
    host.write
        .set_note_text(&loaded.file.note, text, language)?;
    json_result(json!({
        "note": summary_of(&library, &loaded),
        "stored": !text.is_empty(),
        "removed": text.is_empty(),
        "characters": text.chars().count(),
        "scope": "This PC's handwriting cache, until the note's ink changes; the note file is untouched.",
    }))
}
