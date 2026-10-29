//! What the server needs from the application around it.
//!
//! The recognition cache, the indexer, the exports and the windows live in the app's
//! executable, which depends on this crate: the server asks for them through the traits below,
//! and the app implements them (`app/src-tauri/src/mcp.rs`). The tests of this crate implement
//! them with fakes and a temporary library.
//!
//! One [`Host`] holds one object per service group. A tool group (`read.rs`, `write.rs`,
//! `bridge.rs`) only uses its own service, so groups can grow without touching each other.

use std::path::PathBuf;
use std::sync::Arc;

use ardoise_core::library::Library;
use ardoise_core::note::Note;
use ardoise_hwr::structure::Structure;
use ardoise_hwr::{HwrError, Language, Recognition};

use crate::bridge::{AppBridge, NoApp};
use crate::error::ToolError;
use crate::write::{NoWriteServices, WriteServices};

/// The library of this PC and the settings the server follows.
pub trait Environment: Send + Sync {
    /// This PC's library, opened afresh at each call (the user may change it in the app).
    /// `noLibrary` when none is chosen.
    fn library(&self) -> Result<Library, ToolError>;

    /// A binder relative to the library, `/`-separated, that holds exports and not notes (the
    /// automatic export folder while it is on): it is left out of the binders listed.
    fn hidden_binder(&self) -> Option<String> {
        None
    }

    /// The language of the app (prompts follow it).
    fn language(&self) -> Language;
}

/// Where the text of one note was found by the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHit {
    /// Absolute path of the note file, as the library lists it.
    pub path: PathBuf,
    /// The query words found in the recognised text (folded).
    pub words: Vec<String>,
    /// A few words around the first word found.
    pub before: String,
    pub word: String,
    pub after: String,
}

/// How far the recognition of the library is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextStatus {
    /// False when there is no recognition on this PC (not Windows, no recognizer).
    pub available: bool,
    /// Why it is not available: `unsupported`, `noRecognizer` or `noLibrary`.
    pub reason: Option<String>,
    /// Notes whose text is not read yet (the search does not see them until `get_note_text`
    /// or the indexing reads them).
    pub unread: usize,
}

/// The result of a search in the recognised text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextSearch {
    pub hits: Vec<TextHit>,
    pub status: TextStatus,
}

/// Handwriting recognition, with the per-PC cache of the app.
pub trait Handwriting: Send + Sync {
    /// The recognised text of `note` as it is now: from the cache, or recognised now (and
    /// kept). `language` forces a language (default: the one the app's settings give).
    fn recognition_of(
        &self,
        note: &Note,
        language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError>;

    /// The layout of `note` (paragraphs, list items, drawings) from Windows' ink analysis, or
    /// guessed from the recognised lines when it is not available.
    fn structure_of(&self, note: &Note, recognition: &Recognition) -> Structure;

    /// The notes whose recognised text has the words of `query`.
    fn search(&self, query: &str) -> TextSearch;
}

/// Everything the server uses. Build it with [`Host::new`]; the other groups are added with
/// the `with_*` methods (the read tools work with the first two services alone).
#[derive(Clone)]
pub struct Host {
    pub environment: Arc<dyn Environment>,
    pub handwriting: Arc<dyn Handwriting>,
    /// Services of the write tools (`write.rs`).
    pub write: Arc<dyn WriteServices>,
    /// The link to the open app (`bridge.rs`).
    pub app: Arc<dyn AppBridge>,
}

impl Host {
    pub fn new(environment: Arc<dyn Environment>, handwriting: Arc<dyn Handwriting>) -> Host {
        Host {
            environment,
            handwriting,
            write: Arc::new(NoWriteServices),
            app: Arc::new(NoApp),
        }
    }

    pub fn with_write(mut self, write: Arc<dyn WriteServices>) -> Host {
        self.write = write;
        self
    }

    pub fn with_app(mut self, app: Arc<dyn AppBridge>) -> Host {
        self.app = app;
        self
    }

    /// This PC's library.
    pub fn library(&self) -> Result<Library, ToolError> {
        self.environment.library()
    }
}
