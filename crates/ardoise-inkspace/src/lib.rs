//! Import of Wacom Inkspace notes (SPEC §6.3, research `docs/research/inkspace-format.md`).
//!
//! Pipeline: [`locate_local_state`] finds `%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_*\
//! LocalState` (on Windows; the rules are platform-free in [`locate_with_env`]), and
//! [`local_state_in`] turns a folder chosen by hand into its `LocalState`;
//! [`import_local_state`] reads `WacomCloudDb\_` into memory (rusty-leveldb on its in-memory
//! environment, so nothing is written next to Inkspace's data), walks the note tree, decodes
//! the strokes and converts every page into an [`ardoise_core::note::Note`];
//! [`compare::compare_with_preview`] renders an imported note and scores it against Inkspace's
//! own `preview_*.png`.
//!
//! # Privacy
//!
//! Every file access goes through a [`TrackedReader`]. The importer reads only
//! `WacomCloudDb\_\{CURRENT, MANIFEST-*, *.ldb, *.sst, *.log}` and, to find and compare
//! previews, `db\notes\<uuid>\preview_*.png` (plus a listing of `db\notes\<uuid>` when the
//! expected preview name is missing). It never opens the `db\` LevelDB, whose `profile` key
//! holds an access token and an e-mail address; a test proves it with a canary.
//!
//! # Conversion rules
//!
//! - **Note id**: UUID v5 of the Inkspace document id under [`convert::NOTE_ID_NAMESPACE`]
//!   (`<id>/page/<n>` for extra pages, never seen so far), so a second import of the same
//!   note yields the same id. The Inkspace id is kept in `Origin::Inkspace { id }`.
//! - **Title**: Inkspace's (always empty). **Dates**: the document's `creationDate` and
//!   `lastModifiedDate` (Unix ms), falling back to the page's.
//! - **Page**: `boundingBox` × 25 (landscape 29700 × 21600, portrait 21600 × 29700), plain
//!   white background (Inkspace has no paper or template).
//! - **Points**: the Catmull-Rom control points without the duplicated first and last ones,
//!   mapped through the page `transform` (stroke space is the Slate's portrait space; landscape
//!   pages use `matrix(0 -1 1 0 0 864)`: page_x = y, page_y = 864 − x), minus the
//!   `boundingBox` origin, × 25: page coordinates as displayed, device units.
//! - **Layers**: in stored order, all visible, named "Calque 1", "Calque 2"…
//! - **Ink**: `Tool::Ballpoint`, colour from the RGBA int. The stored diameter of each point
//!   is scaled by [`width::width_factor`] (1.0 for timed strokes, 1.1 for untimed ones, tuned
//!   against Inkspace's previews) and × 25, then stored as a pressure through the ballpoint's
//!   inverse response ([`width::pressure_for_width`]), with the stroke's nominal width = its
//!   largest diameter / full-pressure ratio, so every point maps into 0..=1.
//! - **Timing**: when every stroke has `pointsPerSecond > 0` and a `creationDate`, point i of
//!   a stroke is at `creationDate − created + i·1000/pps` ms (`Timing::Recorded`; `created`
//!   moves back to the first stroke if a stroke predates the document). Otherwise every
//!   stroke of the note is timed at 200 Hz, in stored order, with a 250 ms gap
//!   (`Timing::Synthesized`).

pub mod compare;
pub mod convert;
mod decode;
mod locate;
mod protobuf;
pub mod reader;
mod store;
#[cfg(feature = "synthetic")]
pub mod synthetic;
pub mod width;

use std::path::{Path, PathBuf};

use ardoise_core::note::{Note, Timing};

pub use compare::{CompareError, Scores, THRESHOLDS, Thresholds, compare_with_preview};
pub use locate::{
    PACKAGE_PREFIX, local_state_in, locate_in_packages, locate_local_state, locate_with_env,
    packages_dirs,
};
pub use reader::{Access, AccessKind, TrackedReader};

/// Errors that stop the whole import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the Inkspace note database is too large ({bytes} bytes)")]
    TooLarge { bytes: u64 },
    #[error("cannot open the Inkspace note database: {0}")]
    Database(String),
    #[error("the Inkspace note database is malformed: {0}")]
    Malformed(#[from] DecodeError),
}

/// Malformed or unexpected content in the note store.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecodeError {
    #[error("truncated data")]
    Truncated,
    #[error("varint longer than 64 bits")]
    VarintTooLong,
    #[error("invalid protobuf field number")]
    FieldNumber,
    #[error("unsupported protobuf wire type {0}")]
    WireType(u8),
    #[error("unexpected wire type for field {field}")]
    WireTypeMismatch { field: u32 },
    #[error("number overflow")]
    Overflow,
    #[error("missing {0}")]
    Missing(&'static str),
    #[error("invalid {what}: {value}")]
    Invalid { what: &'static str, value: String },
    #[error("node {0} is missing from the database")]
    MissingNode(String),
    #[error("stroke {index}: {reason}")]
    Stroke {
        index: usize,
        reason: Box<DecodeError>,
    },
}

/// Counts of one imported note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteStats {
    pub layers: usize,
    pub strokes: usize,
    /// Catmull-Rom control points as stored (before dropping the duplicated ends).
    pub control_points: usize,
    /// Points in the note (samples).
    pub points: usize,
    /// Points whose width the ballpoint response cannot draw exactly (clipped to pressure 0).
    pub clipped_widths: usize,
    pub timing: Timing,
}

/// One imported note.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedNote {
    pub note: Note,
    /// Inkspace's document id (UUID).
    pub inkspace_id: String,
    /// Page of the Inkspace document (always 0 so far: one page per document).
    pub page_index: usize,
    /// Inkspace's render of the page, if found (only for page 0).
    pub preview: Option<PathBuf>,
    pub stats: NoteStats,
}

/// A note that could not be imported.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteFailure {
    pub inkspace_id: String,
    pub error: DecodeError,
}

/// Result of an import: every note that converted, and the ones that did not.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub local_state: PathBuf,
    pub notes: Vec<ImportedNote>,
    pub failures: Vec<NoteFailure>,
}

impl ImportReport {
    /// Notes found (imported or not).
    pub fn total(&self) -> usize {
        self.notes.len() + self.failures.len()
    }
}

/// Imports every note of the Inkspace `LocalState` folder into memory. Reads go through
/// `reader`; nothing is written anywhere.
pub fn import_local_state(
    local_state: &Path,
    reader: &TrackedReader,
) -> Result<ImportReport, ImportError> {
    let db = store::load(reader, local_state)?;
    let (documents, doc_failures) = decode::documents(&db)?;
    let mut notes = Vec::new();
    let mut failures: Vec<NoteFailure> = doc_failures
        .into_iter()
        .map(|f| NoteFailure {
            inkspace_id: f.id.to_string(),
            error: f.error,
        })
        .collect();
    for doc in &documents {
        let inkspace_id = doc.id.to_string();
        for (page_index, page) in doc.pages.iter().enumerate() {
            match convert::convert_page(doc, page_index, page) {
                Ok((note, stats)) => {
                    let preview = if page_index == 0 {
                        find_preview(reader, local_state, doc)
                    } else {
                        None
                    };
                    notes.push(ImportedNote {
                        note,
                        inkspace_id: inkspace_id.clone(),
                        page_index,
                        preview,
                        stats,
                    });
                }
                Err(error) => failures.push(NoteFailure {
                    inkspace_id: inkspace_id.clone(),
                    error,
                }),
            }
        }
    }
    Ok(ImportReport {
        local_state: local_state.to_path_buf(),
        notes,
        failures,
    })
}

/// `db\notes\<uuid>\preview_<lastModifiedDate>.png`, else the newest `preview_<ms>.png` of
/// that folder. Only for UUID ids, so a crafted id can never point elsewhere.
fn find_preview(
    reader: &TrackedReader,
    local_state: &Path,
    doc: &decode::DocumentData,
) -> Option<PathBuf> {
    let store::NodeId::Uuid(uuid) = &doc.id else {
        return None;
    };
    let folder = local_state.join("db").join("notes").join(uuid.to_string());
    if let Some(modified) = doc
        .props
        .get("lastModifiedDate")
        .and_then(|v| v.trim().parse::<i64>().ok())
    {
        let expected = folder.join(format!("preview_{modified}.png"));
        if reader.is_file(&expected) {
            return Some(expected);
        }
    }
    if !reader.is_dir(&folder) {
        return None;
    }
    reader
        .list(&folder)
        .ok()?
        .into_iter()
        .filter(|e| e.is_file)
        .filter_map(|e| {
            let ms = e
                .name
                .strip_prefix("preview_")?
                .strip_suffix(".png")?
                .parse::<i64>()
                .ok()?;
            Some((ms, e.path))
        })
        .max_by_key(|(ms, _)| *ms)
        .map(|(_, path)| path)
}
