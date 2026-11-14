//! The `.ardoise` file: a ZIP archive holding one note (SPEC §3.3).
//!
//! Members (byte-level description in `docs/file-format.md`):
//!
//! - `note.json` (required): `{"format": {"version": 1, "minor": 0, …}, …}` plus the
//!   [`Note`] metadata without its strokes. Top-level fields this version does not know (added
//!   by a newer minor version) are kept and written back.
//! - `strokes.bin` (required): the strokes, see [`strokes`].
//! - `original.bin`, `original.ardoise`, `thumb.png`, `audio.opus`, `background.<ext>`,
//!   `text.json` (optional): opaque bytes.
//! - page objects (format 1.4; equations 1.5): the `objects` key of `note.json`, see
//!   [`crate::objects`]; a list that cannot be read stays an unknown key, and the note reads
//!   without objects.
//! - `revision.json` (optional, format 1.3): the note is a retouched copy of another, see
//!   [`RevisionInfo`]. A damaged one (or one over [`MAX_REVISION_JSON_BYTES`]) is kept verbatim as
//!   an unknown member and the note is not a revision.
//! - anything else: kept verbatim when the note is rewritten.
//!
//! Versioning: a reader accepts every older version and every minor version of its own major
//! version; a file whose major version is newer is refused with
//! [`FormatError::NewerVersion`] and is never rewritten.
//!
//! What is written can be read: the writer refuses, with [`FormatError::TooLarge`], a file over
//! the limits of the reader, before anything is written.

pub mod revision;
pub mod strokes;

use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Seek, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::fsutil::write_atomic;
use crate::note::Note;

pub use revision::{RevisionEntry, RevisionInfo};
pub use strokes::StrokesError;

/// Required member: format version and note metadata.
pub const NOTE_JSON: &str = "note.json";
/// Required member: the strokes.
pub const STROKES_BIN: &str = "strokes.bin";
/// Optional member: the bytes received from the tablet, untouched.
pub const ORIGINAL_BIN: &str = "original.bin";
/// Optional member (format 1.2): the note as it was before its first edit in Ardoise, as a
/// complete `.ardoise` archive holding only `note.json` and `strokes.bin`. Kept for notes
/// without `original.bin` (live mode, Inkspace import, notes made in Ardoise), for "back to
/// the original".
pub const ORIGINAL_NOTE: &str = "original.ardoise";
/// Optional member: thumbnail (PNG).
pub const THUMB_PNG: &str = "thumb.png";
/// Optional member: narration (Opus).
pub const AUDIO_OPUS: &str = "audio.opus";
/// Optional member, reserved for recognised text (JSON). Ardoise does not write it: the
/// handwriting recognition keeps its results in a cache on each PC (`app/src-tauri/src/
/// handwriting.rs`), so that two PCs never rewrite a note through Nextcloud for it. Kept as read.
pub const TEXT_JSON: &str = "text.json";
/// Optional member (format 1.3): the note is a revision, a retouched copy of another note.
pub const REVISION_JSON: &str = "revision.json";
/// Prefix of the optional background member (`background.png`, `background.pdf`…).
pub const BACKGROUND_PREFIX: &str = "background.";

/// Largest `note.json` accepted.
pub const MAX_NOTE_JSON_BYTES: u64 = 16 << 20;
/// Largest `strokes.bin` accepted (several hours of continuous writing take a few MB).
pub const MAX_STROKES_BYTES: u64 = 128 << 20;
/// Largest `revision.json` read as a revision (an account is a few paragraphs per retouching;
/// supposé: the same as `note.json`). Beyond it the member is treated as damaged: kept as an
/// unknown member, and the note is not a revision.
pub const MAX_REVISION_JSON_BYTES: u64 = 16 << 20;
/// Largest optional or unknown member accepted.
pub const MAX_MEMBER_BYTES: u64 = 1 << 30;
/// Largest total of all the members of one archive, decompressed (1.5 GiB).
pub const MAX_TOTAL_BYTES: u64 = 3 << 29;
/// Largest number of members in one archive.
pub const MAX_MEMBERS: usize = 4096;

/// Version of the file format: `major.minor`.
///
/// The major version changes when older readers could not read a file, or could not rewrite
/// it without losing data; the minor version changes when something is only added (a new
/// top-level field of `note.json`, a new optional member) that older readers keep verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FormatVersion {
    pub major: u32,
    pub minor: u32,
}

impl FormatVersion {
    /// The version this code writes, and the newest it reads.
    /// 1.5: page objects of kind `equation`.
    pub const CURRENT: FormatVersion = FormatVersion { major: 1, minor: 5 };
}

impl fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Background image or PDF stored as `background.<extension>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundFile {
    /// Lowercase ASCII letters and digits, 1 to 10 characters (`png`, `jpg`, `pdf`…).
    pub extension: String,
    pub bytes: Vec<u8>,
}

impl BackgroundFile {
    fn member_name(&self) -> String {
        format!("{BACKGROUND_PREFIX}{}", self.extension)
    }
}

/// A ZIP member this version does not know about, kept verbatim. A name ending in `/` with
/// no bytes is a directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Everything one `.ardoise` file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteFile {
    pub note: Note,
    /// The bytes received from the tablet, untouched ("back to the original").
    pub original: Option<Vec<u8>>,
    /// The note before its first edit, a complete `.ardoise` archive ([`ORIGINAL_NOTE`]); only
    /// for notes without [`original`](Self::original). Read it with [`read_from_bytes`].
    pub original_note: Option<Vec<u8>>,
    /// PNG thumbnail.
    pub thumbnail: Option<Vec<u8>>,
    /// Narration (Opus).
    pub audio: Option<Vec<u8>>,
    pub background: Option<BackgroundFile>,
    /// Reserved for recognised text ([`TEXT_JSON`]): never written by Ardoise, kept as read.
    pub text: Option<Vec<u8>>,
    /// Present when the note is a revision ([`REVISION_JSON`]): the copy an assistant made of
    /// another note. Every save keeps it; a merge or a split does not carry it to the notes
    /// they make.
    pub revision: Option<RevisionInfo>,
    /// Top-level fields of `note.json` this version does not know, kept verbatim.
    pub unknown_fields: Map<String, Value>,
    /// Members this version does not know, kept verbatim in archive order. When the file is
    /// written, one named like [`background`](Self::background) is dropped: the known
    /// background wins; so is a `revision.json` when [`revision`](Self::revision) is set. A
    /// `revision.json` that could not be read stays here.
    pub unknown_members: Vec<Member>,
}

impl NoteFile {
    /// A file holding only `note`.
    pub fn new(note: Note) -> Self {
        NoteFile {
            note,
            original: None,
            original_note: None,
            thumbnail: None,
            audio: None,
            background: None,
            text: None,
            revision: None,
            unknown_fields: Map::new(),
            unknown_members: Vec::new(),
        }
    }

    /// Makes this file a revision of `source`, copied at `at` (Unix milliseconds), with
    /// `summary` as the first line of the account. What the file already had as a revision is
    /// replaced: a revision of a revision describes the changes from the copy it was made from.
    pub fn mark_as_revision_of(&mut self, source: &Note, at: i64, summary: impl Into<String>) {
        self.revision = Some(RevisionInfo::of_note(source, at, summary));
    }

    /// Adds a line to the account of a revision, at the end. Returns `false`, and changes
    /// nothing, when the note is not a revision.
    pub fn add_revision_entry(&mut self, at: i64, text: impl Into<String>) -> bool {
        match &mut self.revision {
            Some(revision) => {
                revision.push_entry(at, text);
                true
            }
            None => false,
        }
    }
}

/// `note.json` alone: enough to list a note without decoding its strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteHeader {
    /// Format version found in the file.
    pub format: FormatVersion,
    /// Application that wrote the file (`"Ardoise 0.1.0"`), if recorded.
    pub writer: Option<String>,
    /// The note's metadata. `strokes` is empty: strokes live in `strokes.bin`.
    pub note: Note,
}

/// A `.ardoise` file that cannot be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The file could not be opened, read or written.
    #[error("cannot access {}: {source}", path.display())]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// Not a ZIP archive, or a damaged one.
    #[error("not a valid .ardoise file (damaged ZIP archive): {0}")]
    Archive(String),
    /// A required member is missing.
    #[error("not a valid .ardoise file: {0} is missing")]
    MissingMember(&'static str),
    /// A member is larger than this version accepts, or than what is left of the archive's
    /// total budget ([`MAX_TOTAL_BYTES`]) (damaged or hostile file).
    #[error("member {name} is larger than {limit} bytes")]
    MemberTooLarge {
        /// Member name.
        name: String,
        /// Limit that applied, in bytes: the member's own limit, or what was left of the
        /// archive's total budget once the members before it were read.
        limit: u64,
    },
    /// The archive has more members than this version accepts.
    #[error("the archive has {0} members, more than this version accepts")]
    TooManyMembers(usize),
    /// `note.json` is not valid.
    #[error("invalid note.json: {0}")]
    Metadata(String),
    /// The file uses a newer major version of the format.
    #[error(
        "this note was made by a newer version of Ardoise (file format {found}); \
         update Ardoise to open it (this version reads format {} and older)",
        FormatVersion::CURRENT.major
    )]
    NewerVersion {
        /// Version found in the file.
        found: FormatVersion,
    },
    /// `strokes.bin` is not valid, or the strokes cannot be encoded.
    #[error("invalid strokes: {0}")]
    Strokes(#[from] StrokesError),
    /// The note breaks an invariant of the model (no layer, stroke on a missing layer…).
    #[error("invalid note: {0}")]
    InvalidNote(String),
    /// The note is over a limit of the reader: a file that could not be read again is not
    /// written. Nothing was written.
    #[error("this note is too large to be saved: {what} is {size}, at most {limit}")]
    TooLarge {
        /// What is too large: a member's name, or the archive as a whole.
        what: String,
        /// Bytes, or members for their number.
        size: u64,
        /// The reader's limit, in the same unit.
        limit: u64,
    },
}

impl FormatError {
    /// Stable identifier for the interface (`errorCodes.*`).
    pub fn code(&self) -> &'static str {
        match self {
            FormatError::Io { .. } => "noteAccess",
            FormatError::NewerVersion { .. } => "noteNewerVersion",
            FormatError::InvalidNote(_) | FormatError::TooLarge { .. } => "noteInvalid",
            FormatError::Strokes(
                StrokesError::InvalidPoint { .. } | StrokesError::InvalidStroke { .. },
            ) => "noteInvalid",
            _ => "noteDamaged",
        }
    }
}

fn archive_error(error: zip::result::ZipError) -> FormatError {
    FormatError::Archive(error.to_string())
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> FormatError + '_ {
    move |source| FormatError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Runs a decoder, turning a panic inside a dependency into an error: a damaged file in the
/// library must never bring the application down.
fn guarded<T>(decode: impl FnOnce() -> Result<T, FormatError>) -> Result<T, FormatError> {
    catch_unwind(AssertUnwindSafe(decode)).unwrap_or_else(|_| {
        Err(FormatError::Archive(
            "unexpected failure while decoding".to_owned(),
        ))
    })
}

// ---------------------------------------------------------------------------------------
// note.json

/// The `format` object of `note.json`.
#[derive(Debug, Serialize, Deserialize)]
struct FormatHeader {
    /// Major version. The only field every future version must keep, with this meaning.
    version: u32,
    #[serde(default)]
    minor: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    writer: Option<String>,
}

#[derive(Serialize)]
struct NoteJsonOut<'a> {
    format: &'a FormatHeader,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

fn writer_name() -> String {
    format!("Ardoise {}", env!("CARGO_PKG_VERSION"))
}

/// The note without its strokes, which are not part of `note.json`.
fn metadata_only(note: &Note) -> Note {
    Note {
        id: note.id.clone(),
        title: note.title.clone(),
        created: note.created,
        modified: note.modified,
        page: note.page,
        background: note.background,
        background_image: note.background_image.clone(),
        tags: note.tags.clone(),
        favorite: note.favorite,
        layers: note.layers.clone(),
        strokes: Vec::new(),
        // Page objects are part of `note.json`; their boxes are written as this version measures
        // them.
        objects: note
            .objects
            .iter()
            .map(|object| {
                let mut object = object.clone();
                object.measure();
                object
            })
            .collect(),
        timing: note.timing,
        origin: note.origin.clone(),
    }
}

fn note_json(file: &NoteFile, version: FormatVersion) -> Result<Vec<u8>, FormatError> {
    let serialized = serde_json::to_value(metadata_only(&file.note))
        .map_err(|e| FormatError::InvalidNote(e.to_string()))?;
    let Value::Object(mut fields) = serialized else {
        return Err(FormatError::InvalidNote(
            "the note does not serialise to an object".to_owned(),
        ));
    };
    fields.remove("strokes");
    for (key, value) in &file.unknown_fields {
        if key != "format" && key != "strokes" && !fields.contains_key(key) {
            fields.insert(key.clone(), value.clone());
        }
    }
    let header = FormatHeader {
        version: version.major,
        minor: version.minor,
        writer: Some(writer_name()),
    };
    serde_json::to_vec_pretty(&NoteJsonOut {
        format: &header,
        fields,
    })
    .map_err(|e| FormatError::InvalidNote(e.to_string()))
}

struct ParsedJson {
    format: FormatVersion,
    writer: Option<String>,
    note: Note,
    unknown_fields: Map<String, Value>,
}

fn parse_note_json(bytes: &[u8]) -> Result<ParsedJson, FormatError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| FormatError::Metadata(e.to_string()))?;
    let Value::Object(mut fields) = value else {
        return Err(FormatError::Metadata("not a JSON object".to_owned()));
    };
    let header = fields
        .remove("format")
        .ok_or_else(|| FormatError::Metadata("the \"format\" object is missing".to_owned()))?;
    // Only the major version is read first: a newer major version may change anything else,
    // including the rest of the "format" object.
    let major = header
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| FormatError::Metadata("format.version is not a number".to_owned()))?;
    let found_minor = header.get("minor").and_then(Value::as_u64).unwrap_or(0);
    let clamp = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
    let found = FormatVersion {
        major: clamp(major),
        minor: clamp(found_minor),
    };
    if found.major > FormatVersion::CURRENT.major {
        return Err(FormatError::NewerVersion { found });
    }
    if found.major == 0 {
        return Err(FormatError::Metadata(format!(
            "unknown format version {found}"
        )));
    }
    let header: FormatHeader =
        serde_json::from_value(header).map_err(|e| FormatError::Metadata(e.to_string()))?;

    fields.remove("strokes");
    let mut with_strokes = fields.clone();
    with_strokes.insert("strokes".to_owned(), Value::Array(Vec::new()));
    let mut note: Note = serde_json::from_value(Value::Object(with_strokes))
        .map_err(|e| FormatError::Metadata(e.to_string()))?;
    repair_objects(&mut note);

    // Whatever the model does not serialise back is unknown to this version.
    let known: HashSet<String> = match serde_json::to_value(&note) {
        Ok(Value::Object(map)) => map.into_iter().map(|(k, _)| k).collect(),
        _ => HashSet::new(),
    };
    let unknown_fields = fields
        .into_iter()
        .filter(|(key, _)| !known.contains(key))
        .collect();
    Ok(ParsedJson {
        format: found,
        writer: header.writer,
        note,
        unknown_fields,
    })
}

/// Makes the page objects read from a file fit the note: an object on a layer the note does not
/// have (a version before 1.4 merged layers without knowing objects) goes on the last layer, ids
/// are made unique, and text frames are measured as this version lays them out.
fn repair_objects(note: &mut Note) {
    let last = note.layers.len().saturating_sub(1) as u32;
    for object in &mut note.objects {
        object.layer = object.layer.min(last);
        object.measure();
    }
    crate::objects::make_ids_unique(&mut note.objects);
}

/// Checks the invariants the model documents and the rest of the code relies on.
fn validate(note: &Note) -> Result<(), FormatError> {
    if note.layers.is_empty() {
        return Err(FormatError::InvalidNote("the note has no layer".to_owned()));
    }
    if note.page.width == 0 || note.page.height == 0 {
        return Err(FormatError::InvalidNote("the page size is zero".to_owned()));
    }
    let layers = note.layers.len();
    if let Some((index, stroke)) = note
        .strokes
        .iter()
        .enumerate()
        .find(|(_, s)| usize::try_from(s.layer).map_or(true, |l| l >= layers))
    {
        return Err(FormatError::InvalidNote(format!(
            "stroke {index} is on layer {} but the note has {layers} layer(s)",
            stroke.layer
        )));
    }
    for (index, object) in note.objects.iter().enumerate() {
        if usize::try_from(object.layer).map_or(true, |l| l >= layers) {
            return Err(FormatError::InvalidNote(format!(
                "object {index} is on layer {} but the note has {layers} layer(s)",
                object.layer
            )));
        }
        let finite = [
            object.x,
            object.y,
            object.width,
            object.height,
            object.scale,
        ]
        .iter()
        .all(|v| v.is_finite());
        let size_ok = object
            .as_text()
            .is_none_or(|text| text.size.is_finite() && text.size > 0.0);
        if !finite || object.scale <= 0.0 || !size_ok {
            return Err(FormatError::InvalidNote(format!(
                "object {index} has a placement or a size that is not a finite positive number"
            )));
        }
        // What is written can be read: an equation's source and drawing within their limits.
        if object.as_equation().is_some_and(|e| !e.is_readable()) {
            return Err(FormatError::InvalidNote(format!(
                "equation {index} has a source or a drawing this version cannot read back"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Reading

fn is_valid_background_extension(extension: &str) -> bool {
    (1..=10).contains(&extension.len())
        && extension
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Reads member `index` entirely, refusing more than `limit` bytes whatever the archive
/// claims (protection against ZIP bombs).
fn read_member_at<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limit: u64,
) -> Result<(String, bool, Vec<u8>), FormatError> {
    let member = archive.by_index(index).map_err(archive_error)?;
    let name = member.name().map_err(archive_error)?.into_owned();
    let is_dir = member.is_dir();
    let declared = member.size();
    let mut bytes =
        Vec::with_capacity(usize::try_from(declared.min(limit).min(1 << 24)).unwrap_or(0));
    member
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| FormatError::Archive(format!("{name}: {e}")))?;
    if bytes.len() as u64 > limit {
        return Err(FormatError::MemberTooLarge { name, limit });
    }
    Ok((name, is_dir, bytes))
}

/// What may still be decompressed from one archive: with [`MAX_MEMBERS`] members of up to
/// [`MAX_MEMBER_BYTES`] each, an archive of a few megabytes could otherwise expand to
/// terabytes.
struct Budget(u64);

impl Budget {
    /// [`read_member_at`], with `limit` lowered to what is left of the budget.
    fn read<R: Read + Seek>(
        &mut self,
        archive: &mut ZipArchive<R>,
        index: usize,
        limit: u64,
    ) -> Result<(String, bool, Vec<u8>), FormatError> {
        let member = read_member_at(archive, index, limit.min(self.0))?;
        self.0 = self.0.saturating_sub(member.2.len() as u64);
        Ok(member)
    }
}

fn read_named<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &'static str,
    limit: u64,
    budget: &mut Budget,
) -> Result<Option<Vec<u8>>, FormatError> {
    match archive.index_for_name(name) {
        Some(index) => Ok(Some(budget.read(archive, index, limit)?.2)),
        None => Ok(None),
    }
}

fn duplicate_name(name: &str) -> FormatError {
    FormatError::Archive(format!("the member name {name:?} appears twice"))
}

/// Signature of a file header in the central directory.
const CENTRAL_HEADER_SIGNATURE: [u8; 4] = 0x0201_4b50_u32.to_le_bytes();

/// Reads the central directory's file headers, starting at `directory_start`, and refuses a
/// name that appears twice. The zip library keeps only one entry per name (the position of
/// the first, the data of the last), so a repeated name can only be seen here. Counting the
/// headers also applies [`MAX_MEMBERS`] to the members as written, repeats included.
fn check_unique_names<R: Read + Seek>(
    reader: &mut R,
    directory_start: u64,
) -> Result<(), FormatError> {
    let damaged = |e: io::Error| FormatError::Archive(format!("central directory: {e}"));
    reader
        .seek(io::SeekFrom::Start(directory_start))
        .map_err(damaged)?;
    let mut names: HashSet<Vec<u8>> = HashSet::new();
    loop {
        let mut signature = [0; 4];
        match reader.read_exact(&mut signature) {
            Ok(()) if signature == CENTRAL_HEADER_SIGNATURE => {}
            // The end of central directory record (or a ZIP64 one) follows the last header.
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(damaged(e)),
        }
        // Fixed part of the header after the signature (APPNOTE 4.3.12); the lengths of the
        // name, extra field and comment are at offsets 24, 26 and 28.
        let mut fixed = [0; 42];
        reader.read_exact(&mut fixed).map_err(damaged)?;
        let length = |at: usize| u16::from_le_bytes([fixed[at], fixed[at + 1]]);
        let mut name = vec![0; usize::from(length(24))];
        reader.read_exact(&mut name).map_err(damaged)?;
        let skip = u64::from(length(26)) + u64::from(length(28));
        let skipped =
            io::copy(&mut reader.by_ref().take(skip), &mut io::sink()).map_err(damaged)?;
        if skipped != skip {
            return Err(FormatError::Archive(
                "central directory: truncated header".to_owned(),
            ));
        }
        if names.contains(&name) {
            return Err(duplicate_name(&String::from_utf8_lossy(&name)));
        }
        names.insert(name);
        if names.len() > MAX_MEMBERS {
            return Err(FormatError::TooManyMembers(names.len()));
        }
    }
}

fn open_archive<R: Read + Seek>(mut reader: R) -> Result<ZipArchive<R>, FormatError> {
    // The central directory is parsed twice (it is small): the zip library lends its reader to
    // nobody, and the check of repeated names needs it.
    let directory_start = {
        let archive = ZipArchive::new(&mut reader).map_err(archive_error)?;
        if archive.len() > MAX_MEMBERS {
            return Err(FormatError::TooManyMembers(archive.len()));
        }
        archive.central_directory_start()
    };
    check_unique_names(&mut reader, directory_start)?;
    let archive = ZipArchive::new(reader).map_err(archive_error)?;
    // Different bytes may decode to the same name (a CP437 and a UTF-8 spelling).
    let mut names: HashSet<String> = HashSet::new();
    for index in 0..archive.len() {
        if let Some(name) = archive.name_for_index(index) {
            let name = name.map_err(archive_error)?;
            if !names.insert(name.clone().into_owned()) {
                return Err(duplicate_name(&name));
            }
        }
    }
    Ok(archive)
}

fn read_header_from_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    budget: &mut Budget,
) -> Result<ParsedJson, FormatError> {
    let json = read_named(archive, NOTE_JSON, MAX_NOTE_JSON_BYTES, budget)?
        .ok_or(FormatError::MissingMember(NOTE_JSON))?;
    parse_note_json(&json)
}

fn read_archive<R: Read + Seek>(reader: R) -> Result<NoteFile, FormatError> {
    read_archive_within(reader, MAX_TOTAL_BYTES)
}

/// [`read_archive`], decompressing at most `budget` bytes in all.
fn read_archive_within<R: Read + Seek>(reader: R, budget: u64) -> Result<NoteFile, FormatError> {
    let mut budget = Budget(budget);
    let mut archive = open_archive(reader)?;
    let parsed = read_header_from_archive(&mut archive, &mut budget)?;
    let strokes_bytes = read_named(&mut archive, STROKES_BIN, MAX_STROKES_BYTES, &mut budget)?
        .ok_or(FormatError::MissingMember(STROKES_BIN))?;
    let mut note = parsed.note;
    note.strokes = strokes::decode(&strokes_bytes)?;
    validate(&note)?;

    let mut file = NoteFile::new(note);
    file.unknown_fields = parsed.unknown_fields;
    for index in 0..archive.len() {
        // Names are unique ([`open_archive`]); these two are already read.
        match archive.name_for_index(index) {
            Some(Ok(name)) if name != NOTE_JSON && name != STROKES_BIN => {}
            Some(Err(error)) => return Err(archive_error(error)),
            _ => continue,
        }
        let (name, is_dir, bytes) = budget.read(&mut archive, index, MAX_MEMBER_BYTES)?;
        if name == REVISION_JSON
            && !is_dir
            && bytes.len() as u64 <= MAX_REVISION_JSON_BYTES
            && let Some(revision) = RevisionInfo::parse(&bytes)
        {
            file.revision = Some(revision);
            continue;
        }
        let slot = match name.as_str() {
            _ if is_dir => None,
            ORIGINAL_BIN => Some(&mut file.original),
            ORIGINAL_NOTE => Some(&mut file.original_note),
            THUMB_PNG => Some(&mut file.thumbnail),
            AUDIO_OPUS => Some(&mut file.audio),
            TEXT_JSON => Some(&mut file.text),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = Some(bytes);
            continue;
        }
        if let Some(extension) = name.strip_prefix(BACKGROUND_PREFIX)
            && file.background.is_none()
            && !is_dir
            && is_valid_background_extension(extension)
        {
            file.background = Some(BackgroundFile {
                extension: extension.to_owned(),
                bytes,
            });
            continue;
        }
        file.unknown_members.push(Member { name, bytes });
    }
    Ok(file)
}

/// Decodes a whole `.ardoise` file held in memory.
pub fn read_from_bytes(bytes: &[u8]) -> Result<NoteFile, FormatError> {
    guarded(|| read_archive(Cursor::new(bytes)))
}

/// Decodes only `note.json` of a `.ardoise` file held in memory.
pub fn read_header_from_bytes(bytes: &[u8]) -> Result<NoteHeader, FormatError> {
    guarded(|| header_of(Cursor::new(bytes)))
}

fn header_of<R: Read + Seek>(reader: R) -> Result<NoteHeader, FormatError> {
    let mut archive = open_archive(reader)?;
    let parsed = read_header_from_archive(&mut archive, &mut Budget(MAX_TOTAL_BYTES))?;
    Ok(NoteHeader {
        format: parsed.format,
        writer: parsed.writer,
        note: parsed.note,
    })
}

fn open_file(path: &Path) -> Result<BufReader<File>, FormatError> {
    File::open(path).map(BufReader::new).map_err(io_error(path))
}

/// Reads a `.ardoise` file.
pub fn read_note(path: &Path) -> Result<NoteFile, FormatError> {
    let file = open_file(path)?;
    guarded(|| read_archive(file))
}

/// Reads only `note.json` of a `.ardoise` file: fast, for listing a library.
pub fn read_header(path: &Path) -> Result<NoteHeader, FormatError> {
    let file = open_file(path)?;
    guarded(|| header_of(file))
}

/// Reads only the thumbnail of a `.ardoise` file, if it has one.
pub fn read_thumbnail(path: &Path) -> Result<Option<Vec<u8>>, FormatError> {
    let file = open_file(path)?;
    guarded(|| {
        let mut archive = open_archive(file)?;
        read_named(
            &mut archive,
            THUMB_PNG,
            MAX_MEMBER_BYTES,
            &mut Budget(MAX_TOTAL_BYTES),
        )
    })
}

// ---------------------------------------------------------------------------------------
// Writing

/// Extensions of already-compressed data, stored without deflate.
const STORED_EXTENSIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "webp", "gif", "opus", "ogg", "mp3", "mp4", "zip", "ardoise",
];

fn compression_for(name: &str) -> CompressionMethod {
    let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match extension {
        Some(e) if STORED_EXTENSIONS.contains(&e.as_str()) => CompressionMethod::Stored,
        _ => CompressionMethod::Deflated,
    }
}

/// ZIP timestamp of the members: the note's last change, in UTC (deterministic output).
fn zip_time(unix_ms: i64) -> zip::DateTime {
    use chrono::{Datelike, Timelike};
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .and_then(|t| {
            let year = u16::try_from(t.year()).ok()?;
            zip::DateTime::from_date_and_time(
                year,
                t.month() as u8,
                t.day() as u8,
                t.hour() as u8,
                t.minute() as u8,
                t.second() as u8,
            )
            .ok()
        })
        .unwrap_or_default()
}

/// The unknown members written with `file`. One named like the background the file has is
/// dropped: the known background wins. (A file read with `background.png` and an unknown
/// `background.pdf` must stay writable once the app gives it a PDF background.) The same goes
/// for a damaged `revision.json` once the note is given a revision.
fn unknown_members_to_write(file: &NoteFile) -> impl Iterator<Item = &Member> {
    let background = file.background.as_ref().map(BackgroundFile::member_name);
    let revision = file.revision.is_some();
    file.unknown_members.iter().filter(move |member| {
        background.as_deref() != Some(member.name.as_str())
            && !(revision && member.name == REVISION_JSON)
    })
}

fn check_member_names(file: &NoteFile) -> Result<(), FormatError> {
    if let Some(background) = &file.background
        && !is_valid_background_extension(&background.extension)
    {
        return Err(FormatError::InvalidNote(format!(
            "invalid background extension {:?}",
            background.extension
        )));
    }
    if file.revision.as_ref().is_some_and(|r| r.of.is_empty()) {
        return Err(FormatError::InvalidNote(
            "a revision must name the note it comes from".to_owned(),
        ));
    }
    let mut names: HashSet<&str> = [
        NOTE_JSON,
        STROKES_BIN,
        ORIGINAL_BIN,
        ORIGINAL_NOTE,
        THUMB_PNG,
        AUDIO_OPUS,
        TEXT_JSON,
    ]
    .into_iter()
    .collect();
    let background_name = file.background.as_ref().map(BackgroundFile::member_name);
    if let Some(name) = &background_name {
        names.insert(name);
    }
    if file.revision.is_some() {
        names.insert(REVISION_JSON);
    }
    for member in unknown_members_to_write(file) {
        if member.name.is_empty() || !names.insert(&member.name) {
            return Err(FormatError::InvalidNote(format!(
                "member name {:?} is empty or used twice",
                member.name
            )));
        }
    }
    Ok(())
}

fn add_member<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    name: &str,
    bytes: &[u8],
    time: zip::DateTime,
) -> Result<(), FormatError> {
    let options = SimpleFileOptions::default()
        .compression_method(compression_for(name))
        .last_modified_time(time)
        .large_file(bytes.len() as u64 >= u64::from(u32::MAX));
    zip.start_file(name, options).map_err(archive_error)?;
    zip.write_all(bytes)
        .map_err(|e| FormatError::Archive(e.to_string()))
}

/// Refuses what the reader would refuse: each member against its limit, the whole against
/// [`MAX_TOTAL_BYTES`] and [`MAX_MEMBERS`]. `members` gives, for each member to write, its name,
/// its size and its limit.
fn check_read_limits<'a>(
    members: impl Iterator<Item = (&'a str, u64, u64)>,
) -> Result<(), FormatError> {
    let too_large = |what: &str, size: u64, limit: u64| FormatError::TooLarge {
        what: what.to_owned(),
        size,
        limit,
    };
    let mut total = 0u64;
    let mut count = 0u64;
    for (name, size, limit) in members {
        if size > limit {
            return Err(too_large(name, size, limit));
        }
        total = total.saturating_add(size);
        count += 1;
    }
    if total > MAX_TOTAL_BYTES {
        return Err(too_large("the whole file", total, MAX_TOTAL_BYTES));
    }
    if count > MAX_MEMBERS as u64 {
        return Err(too_large(
            "its number of members",
            count,
            MAX_MEMBERS as u64,
        ));
    }
    Ok(())
}

/// Encodes a note into the bytes of a `.ardoise` file (format [`FormatVersion::CURRENT`]).
/// A note the reader would refuse is refused here ([`FormatError::TooLarge`]).
pub fn to_bytes(file: &NoteFile) -> Result<Vec<u8>, FormatError> {
    to_bytes_as(file, FormatVersion::CURRENT)
}

fn to_bytes_as(file: &NoteFile, version: FormatVersion) -> Result<Vec<u8>, FormatError> {
    validate(&file.note)?;
    check_member_names(file)?;
    let strokes = strokes::encode(&file.note.strokes)?;
    let json = note_json(file, version)?;
    let revision = file
        .revision
        .as_ref()
        .map(RevisionInfo::to_json)
        .transpose()
        .map_err(|e| FormatError::InvalidNote(e.to_string()))?;
    let optional = [
        (ORIGINAL_BIN, &file.original),
        (ORIGINAL_NOTE, &file.original_note),
        (THUMB_PNG, &file.thumbnail),
        (AUDIO_OPUS, &file.audio),
        (TEXT_JSON, &file.text),
    ];
    let background = file
        .background
        .as_ref()
        .map(|background| (background.member_name(), background));
    let size = |bytes: &[u8]| bytes.len() as u64;
    let members = [
        (NOTE_JSON, size(&json), MAX_NOTE_JSON_BYTES),
        (STROKES_BIN, size(&strokes), MAX_STROKES_BYTES),
    ]
    .into_iter()
    .chain(
        revision
            .iter()
            .map(|json| (REVISION_JSON, size(json), MAX_REVISION_JSON_BYTES)),
    )
    .chain(optional.iter().filter_map(|(name, bytes)| {
        bytes
            .as_ref()
            .map(|bytes| (*name, size(bytes), MAX_MEMBER_BYTES))
    }))
    .chain(
        background
            .iter()
            .map(|(name, member)| (name.as_str(), size(&member.bytes), MAX_MEMBER_BYTES)),
    )
    .chain(
        unknown_members_to_write(file)
            .map(|member| (member.name.as_str(), size(&member.bytes), MAX_MEMBER_BYTES)),
    );
    check_read_limits(members)?;
    let time = zip_time(file.note.modified);

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    add_member(&mut zip, NOTE_JSON, &json, time)?;
    add_member(&mut zip, STROKES_BIN, &strokes, time)?;
    for (name, bytes) in optional {
        if let Some(bytes) = bytes {
            add_member(&mut zip, name, bytes, time)?;
        }
    }
    if let Some((name, background)) = &background {
        add_member(&mut zip, name, &background.bytes, time)?;
    }
    if let Some(json) = &revision {
        add_member(&mut zip, REVISION_JSON, json, time)?;
    }
    for member in unknown_members_to_write(file) {
        if member.name.ends_with('/') && member.bytes.is_empty() {
            let options = SimpleFileOptions::default().last_modified_time(time);
            zip.add_directory(member.name.as_str(), options)
                .map_err(archive_error)?;
        } else {
            add_member(&mut zip, &member.name, &member.bytes, time)?;
        }
    }
    let cursor = zip.finish().map_err(archive_error)?;
    Ok(cursor.into_inner())
}

/// Writes a `.ardoise` file atomically: a reader (or Nextcloud) sees the old file or the new
/// one, never a mix. Nothing is written if the note cannot be encoded.
pub fn write_note(path: &Path, file: &NoteFile) -> Result<(), FormatError> {
    let bytes = to_bytes(file)?;
    write_atomic(path, &bytes).map_err(io_error(path))
}

#[cfg(test)]
pub(crate) fn to_bytes_with_version(
    file: &NoteFile,
    version: FormatVersion,
) -> Result<Vec<u8>, FormatError> {
    to_bytes_as(file, version)
}

#[cfg(test)]
mod tests;
