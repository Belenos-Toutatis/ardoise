//! `revision.json` (format 1.3): what makes a note a **revision**, a retouched copy of another
//! note made by an assistant (SPEC §4.9, `docs/design/revision.md`).
//!
//! The member names the note the copy comes from (by id: titles and paths change), the title it
//! had then (to say something when it is gone) and the account of what was changed, one dated
//! entry per retouching. It is written only on a revision. The text of the account comes from an
//! assistant: whoever shows it draws it as plain text.

use serde::{Deserialize, Serialize};

use crate::note::Note;

/// Largest account entry kept when one is added by [`RevisionInfo::push_entry`], in characters:
/// a summary is a few paragraphs, not a document.
pub const MAX_ENTRY_CHARS: usize = 20_000;

/// One line of the account: what a retouching changed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevisionEntry {
    /// When (Unix milliseconds).
    pub at: i64,
    /// What changed, in the user's language, plain text.
    pub text: String,
}

/// The content of `revision.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionInfo {
    /// Id of the note this one is a copy of. Never empty in a note this version reads.
    pub of: String,
    /// Title of that note when the copy was made.
    pub of_title: String,
    /// When the copy was made (Unix milliseconds).
    pub created: i64,
    /// The account of what was changed, oldest first. A revision of a revision starts a new
    /// account: it describes the changes from the copy it was made from.
    #[serde(default)]
    pub summary: Vec<RevisionEntry>,
}

impl RevisionInfo {
    /// The info of a copy of `source` made at `at` (Unix milliseconds), with `summary` as the
    /// first line of the account.
    pub fn of_note(source: &Note, at: i64, summary: impl Into<String>) -> Self {
        let mut info = RevisionInfo {
            of: source.id.clone(),
            of_title: source.title.clone(),
            created: at,
            summary: Vec::new(),
        };
        info.push_entry(at, summary);
        info
    }

    /// Adds a line to the account, at the end. Text beyond [`MAX_ENTRY_CHARS`] is cut. A blank
    /// text adds nothing.
    pub fn push_entry(&mut self, at: i64, text: impl Into<String>) {
        let text: String = text.into();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.summary.push(RevisionEntry {
            at,
            text: text.chars().take(MAX_ENTRY_CHARS).collect(),
        });
    }

    /// Reads the bytes of a `revision.json`; `None` when they are not a usable one (not JSON, a
    /// wrong shape, no source id): the note is then not a revision and the bytes are kept as an
    /// unknown member.
    pub(super) fn parse(bytes: &[u8]) -> Option<RevisionInfo> {
        let info: RevisionInfo = serde_json::from_slice(bytes).ok()?;
        (!info.of.is_empty()).then_some(info)
    }

    pub(super) fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }
}
