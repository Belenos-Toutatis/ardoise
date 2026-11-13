//! Synced pages become notes of the library (SPEC §4.1). Every synchronisation goes through
//! [`LibrarySink::store`]: it writes the note, reads it back to verify it, and only then lets the
//! session delete the page from the tablet; then the after-save hook runs (automatic export).
//!
//! A synced note's id is derived from the page ([`page_note_id`]), so that a page synced again
//! (sync without deletion, or a deletion that failed after the save) is recognised
//! ([`ImportedPages`]) instead of being stored twice. A copy that failed its check never counts
//! as imported ([`LibrarySink::store`]). Points are turned to the way the tablet is held ([`orient`]: landscape
//! or portrait, either way up), in synced pages and in live mode alike.
//!
//! A page of the Xiaomi blackboard ([`DownloadedPage::kind`]) is read as its 7-byte records
//! ([`blackboard_note`]): its page is the blackboard's own, portrait, never turned, and its times
//! are synthesised from the record rate (the tablet sends none).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ardoise_core::format::{self, NoteFile};
use ardoise_core::fsutil::remove_file_with_retry;
use ardoise_core::library::{Library, LoadedNote, SavedNote};
use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_core::settings::Orientation;
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_proto::SLATE_MAX_PRESSURE;
use ardoise_proto::blackboard::{self, BOARD_HEIGHT, BOARD_MAX_PRESSURE, BOARD_WIDTH};
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::{self, INTUOS_PRO_MAGIC, SLATE_MAGIC, StrokeError, StrokeTime};

use crate::session::DownloadedPage;
use crate::transport::TabletKind;

/// The blackboard's page, portrait, in the notes' units (10 µm, taken 1:1 from its units): the
/// size of the notes it writes now.
pub const BLACKBOARD_PAGE: PageSize = PageSize {
    width: BOARD_WIDTH,
    height: BOARD_HEIGHT,
};
/// Every page size the blackboard has ever written with, [`BLACKBOARD_PAGE`] first. A
/// calibration of `BOARD_WIDTH` and `BOARD_HEIGHT` (docs/design/blackboard.md §3) adds the new
/// size here and keeps the old ones: the notes written before stay blackboard pages (resumed
/// with the blackboard, never with the Slate).
pub const BLACKBOARD_PAGES: [PageSize; 1] = [BLACKBOARD_PAGE];

/// Whether `size` is a page the blackboard wrote ([`BLACKBOARD_PAGES`]).
pub fn is_blackboard_page(size: PageSize) -> bool {
    BLACKBOARD_PAGES.contains(&size)
}
/// A blackboard page dated before this (2020-01-01 00:00:00 UTC, Unix seconds) has a clock that
/// was never set: its note is dated by the import ([`blackboard_note`]). The tablet was sold from
/// December 2021 (research note §1).
pub const EARLIEST_BOARD_PAGE: i64 = 1_577_836_800;

/// Nominal diameter of the tablet's pen, in device units (0.6 mm, the editor's default pen).
pub const TABLET_PEN_WIDTH: f32 = 60.0;
/// Interval between two samples of the tablet (protocol.md §6).
const SAMPLE_MS: u64 = 5;
/// The Slate's stroke clock wraps every 327.68 s (a 16-bit count of 5 ms ticks).
const SLATE_CLOCK_WRAP_MS: u64 = 327_680;
/// Pause assumed before a stroke whose header gives no usable time.
const UNKNOWN_GAP_MS: u64 = 100;

/// Why a page could not become a note. The page then stays on the tablet.
#[derive(Debug, thiserror::Error)]
pub enum PageError {
    #[error("the page cannot be decoded: {0}")]
    Decode(#[from] StrokeError),
    #[error("cannot make a note id: {0}")]
    Random(String),
}

/// A random (version 4) UUID.
pub fn new_note_id() -> Result<String, PageError> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| PageError::Random(e.to_string()))?;
    Ok(uuid_text(b, 0x40))
}

/// `b` as a UUID of `version` (high nibble of byte 6), RFC 9562 variant.
fn uuid_text(mut b: [u8; 16], version: u8) -> String {
    b[6] = (b[6] & 0x0f) | version;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex = |range: std::ops::Range<usize>| -> String {
        b[range].iter().map(|byte| format!("{byte:02x}")).collect()
    };
    format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    )
}

/// 128-bit FNV-1a (deterministic on every platform; not a cryptographic hash, which an id
/// does not need).
struct Fnv128(u128);

impl Fnv128 {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u128::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }
}

/// The id of the note made from a synced page: a version 8 (custom) UUID hashed from the
/// page's time and bytes as the tablet sent them. The same page synced again gets the same id;
/// two different pages practically never do (the stroke headers hold the tablet's time).
pub fn page_note_id(page: &DownloadedPage) -> String {
    let mut hash = Fnv128::new();
    hash.write(b"ardoise synced page 1");
    match page.timestamp {
        Some(seconds) => {
            hash.write(&[1]);
            hash.write(&seconds.to_le_bytes());
        }
        None => hash.write(&[0]),
    }
    hash.write(&(page.bytes.len() as u64).to_le_bytes());
    hash.write(&page.bytes);
    uuid_text(hash.0.to_be_bytes(), 0x80)
}

/// A point of the tablet in page coordinates: turned by the quarter turns of `orientation`
/// (clockwise) so that what the user writes is upright on the page. `width` × `height` is the
/// tablet's drawing area; the page is `orientation.page_size(width, height)`. Landscape keeps
/// the point, flipped landscape turns it back half a turn (`x' = width - x`,
/// `y' = height - y`), a portrait position makes `(height - y, x)` or `(y, width - x)`
/// (SPEC §4.1; docs/design/tablet-live-classroom.md, "Orientation"). Points beyond the area
/// are clamped.
pub fn orient(x: u16, y: u16, width: u32, height: u32, orientation: Orientation) -> (u16, u16) {
    let (x, y) = (u32::from(x), u32::from(y));
    let (x, y) = match orientation.quarter_turns() {
        0 => (x, y),
        1 => (height.saturating_sub(y), x),
        2 => (width.saturating_sub(x), height.saturating_sub(y)),
        _ => (y, width.saturating_sub(x)),
    };
    (
        u16::try_from(x).unwrap_or(u16::MAX),
        u16::try_from(y).unwrap_or(u16::MAX),
    )
}

/// A point of a page of `width` × `height` after the page is turned `turns` clockwise quarter
/// turns (the result lives on a page with `width` and `height` swapped when `turns` is odd):
/// the same turn as [`orient`], for ink already on a page (the live page when the orientation
/// is switched).
pub fn turn_point(x: f32, y: f32, width: f32, height: f32, turns: u8) -> (f32, f32) {
    match turns % 4 {
        0 => (x, y),
        1 => (height - y, x),
        2 => (width - x, height - y),
        _ => (y, width - x),
    }
}

/// The page of a tablet whose drawing area is `size`, held as `orientation` says.
pub fn oriented_size(size: PageSize, orientation: Orientation) -> PageSize {
    let (width, height) = orientation.page_size(size.width, size.height);
    PageSize { width, height }
}

/// [`orient`] applied to a live record ("pen left" records stay as they are).
pub fn orient_record(
    record: LiveRecord,
    width: u32,
    height: u32,
    orientation: Orientation,
) -> LiveRecord {
    match record {
        LiveRecord::Pen(mut point) => {
            (point.x, point.y) = orient(point.x, point.y, width, height, orientation);
            LiveRecord::Pen(point)
        }
        LiveRecord::ProximityOut => LiveRecord::ProximityOut,
    }
}

/// The synced pages already in a library, found by their note id ([`page_note_id`]).
#[derive(Debug, Default)]
pub struct ImportedPages {
    listed: HashMap<String, PathBuf>,
    /// Notes by creation time (Unix ms): a blackboard note updated by a later store of its
    /// screen keeps the creation time of its first page ([`LibrarySink::store_page`]).
    created: HashMap<i64, Vec<PathBuf>>,
    /// Files of the trash by note id (a note may have been trashed more than once).
    trashed: HashMap<String, Vec<PathBuf>>,
}

impl ImportedPages {
    /// Reads the ids of every note of `library` and of its trash (once per sync).
    pub fn scan(library: &Library) -> Self {
        let mut listed = HashMap::new();
        let mut created: HashMap<i64, Vec<PathBuf>> = HashMap::new();
        for entry in library.list().notes {
            created
                .entry(entry.created)
                .or_default()
                .push(entry.path.clone());
            listed.insert(entry.id, entry.path);
        }
        let mut trashed: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for item in library.trash_items().unwrap_or_default() {
            if let Some(id) = item.note_id {
                trashed.entry(id).or_default().push(item.path);
            }
        }
        Self {
            listed,
            created,
            trashed,
        }
    }

    /// The note `page` was already saved as: a note of the library with its id that holds
    /// exactly its bytes (`original.bin`), so the page is safe in the library. For a page of
    /// the blackboard, also a note created at the page's time whose `original.bin` starts with
    /// the page's bytes: the note of its screen, updated since by a later store that holds it
    /// whole (docs/design/blackboard.md §9.6).
    pub fn find(&self, library: &Library, page: &DownloadedPage) -> Option<PathBuf> {
        if let Some(path) = self.listed.get(&page_note_id(page))
            && let Ok(loaded) = library.read(path)
            && loaded.file.original.as_deref() == Some(page.bytes.as_slice())
        {
            return Some(loaded.path);
        }
        if page.kind != TabletKind::Blackboard || page.bytes.is_empty() {
            return None;
        }
        let created = page.timestamp?.checked_mul(1000)?;
        self.created.get(&created)?.iter().find_map(|path| {
            let loaded = library.read(path).ok()?;
            let original = loaded.file.original.as_deref()?;
            (loaded.file.note.page == BLACKBOARD_PAGE && original.starts_with(&page.bytes))
                .then_some(loaded.path)
        })
    }

    /// Whether the note of `page` is in the trash: the user deleted it, so a page kept on the
    /// tablet is not imported again. Only a complete copy counts: a trashed file that does not
    /// read back with the page's id and exactly its bytes (`original.bin`), such as a copy that
    /// failed its check and was put aside ([`LibrarySink::store`]), never makes the page count
    /// as imported (it would never be imported, and without deletion it would hide every later
    /// page of the tablet).
    pub fn in_trash(&self, page: &DownloadedPage) -> bool {
        let id = page_note_id(page);
        let Some(paths) = self.trashed.get(&id) else {
            return false;
        };
        paths.iter().any(|path| {
            format::read_note(path).is_ok_and(|file| {
                file.note.id == id && file.original.as_deref() == Some(page.bytes.as_slice())
            })
        })
    }

    /// Records the note just saved at `path` from the page whose [`page_note_id`] is `id`.
    pub fn add(&mut self, id: String, path: &Path) {
        self.listed.insert(id, path.to_path_buf());
    }

    /// Records the note just saved (or updated) at `path` from `page`: by its id, and by its
    /// creation time.
    pub fn record(&mut self, page: &DownloadedPage, path: &Path, created: i64) {
        self.add(page_note_id(page), path);
        let paths = self.created.entry(created).or_default();
        if !paths.iter().any(|p| p == path) {
            paths.push(path.to_path_buf());
        }
    }
}

/// Start time of each stroke in milliseconds since the first one, from the stroke headers.
/// The Slate's clock counts from power-up and wraps every 327.68 s: it is unwrapped assuming
/// strokes are in order and less than one wrap apart.
fn stroke_starts(file: &stroke::StrokeFile) -> Vec<u64> {
    let mut starts = Vec::with_capacity(file.strokes.len());
    let mut elapsed = 0u64;
    let mut previous: Option<(StrokeTime, u64)> = None;
    for s in &file.strokes {
        let samples = s.points.len() as u64 * SAMPLE_MS;
        let start = match (previous, s.time) {
            (None, _) => 0,
            (
                Some((StrokeTime::SincePowerUp { millis: before }, _)),
                StrokeTime::SincePowerUp { millis },
            ) => {
                let delta = (u64::from(millis) + SLATE_CLOCK_WRAP_MS
                    - u64::from(before) % SLATE_CLOCK_WRAP_MS)
                    % SLATE_CLOCK_WRAP_MS;
                elapsed + delta
            }
            (
                Some((
                    StrokeTime::Unix {
                        seconds: before, ..
                    },
                    _,
                )),
                StrokeTime::Unix { seconds, .. },
            ) if seconds >= before => elapsed + (u64::from(seconds - before) * 1000),
            (Some((_, end)), _) => end + UNKNOWN_GAP_MS,
        };
        // Never before the end of the previous stroke (clock glitches, lost wraps).
        let start = previous.map_or(start, |(_, end)| start.max(end));
        starts.push(start);
        elapsed = start;
        previous = Some((s.time, start + samples));
    }
    starts
}

/// The tablet whose page `bytes` are, for bytes kept without it (`original.bin`): a Slate (or
/// Intuos Pro) file starts with its header; whole 7-byte records starting with a pen status of
/// the blackboard are a blackboard page. Anything else is taken for a Slate file (whose decoder
/// then says what is wrong).
pub fn page_kind(bytes: &[u8]) -> TabletKind {
    let header = bytes.starts_with(&SLATE_MAGIC) || bytes.starts_with(&INTUOS_PRO_MAGIC);
    let records = !bytes.is_empty()
        && bytes.len().is_multiple_of(blackboard::record::RECORD_LEN)
        && bytes.first().is_some_and(|status| {
            [
                blackboard::record::TOUCH,
                blackboard::record::HOVER,
                blackboard::record::OUT,
            ]
            .contains(status)
        });
    if !header && records {
        TabletKind::Blackboard
    } else {
        TabletKind::Slate
    }
}

/// The note of one downloaded page: black ballpoint strokes on a page of the tablet's size
/// (`size`, swapped in portrait), turned to the way the tablet is held ([`orient`]), `created` at the page's timestamp (or `now_ms`),
/// `original.bin` = the bytes received (untouched).
///
/// A page of the blackboard (`page.kind`) ignores `size` and `orientation`: see
/// [`blackboard_note`].
pub fn note_from_page(
    page: &DownloadedPage,
    size: PageSize,
    orientation: Orientation,
    id: String,
    layer_name: &str,
    now_ms: i64,
) -> Result<NoteFile, PageError> {
    if page.kind == TabletKind::Blackboard {
        return Ok(blackboard_note(page, id, layer_name, now_ms));
    }
    let file = stroke::decode(&page.bytes)?;
    let starts = stroke_starts(&file);
    let max_pressure = f32::from(SLATE_MAX_PRESSURE);
    let strokes = file
        .strokes
        .iter()
        .zip(starts)
        .filter(|(s, _)| !s.points.is_empty())
        .map(|(s, start)| Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: TABLET_PEN_WIDTH,
            layer: 0,
            points: s
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let (x, y) = orient(p.x, p.y, size.width, size.height, orientation);
                    Point {
                        x: f32::from(x),
                        y: f32::from(y),
                        pressure: (f32::from(p.pressure) / max_pressure).clamp(0.0, 1.0),
                        t: u32::try_from(start + i as u64 * SAMPLE_MS).unwrap_or(u32::MAX),
                    }
                })
                .collect(),
        })
        .collect();
    let created = page
        .timestamp
        .and_then(|seconds| seconds.checked_mul(1000))
        .unwrap_or(now_ms);
    let note = Note {
        id,
        title: String::new(),
        created,
        modified: now_ms.max(created),
        page: oriented_size(size, orientation),
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
        favorite: false,
        layers: vec![Layer {
            name: layer_name.to_owned(),
            visible: true,
        }],
        strokes,
        timing: Timing::Recorded,
        origin: Origin::Tablet,
    };
    let mut note_file = NoteFile::new(note);
    note_file.original = Some(page.bytes.clone());
    Ok(note_file)
}

/// The note of a page of the Xiaomi blackboard: black ballpoint strokes, the runs of touching
/// records, on the blackboard's page ([`BLACKBOARD_PAGE`], portrait, never turned: the
/// blackboard has no orientation, and its x and y are the notes' units 1:1); each record one
/// period of [`blackboard::record::RECORDS_PER_SECOND`] after the previous one, half a second
/// more after the pen left the range (`Timing::Synthesized`: the tablet keeps no time per
/// point); `created` at the page's time (when it was stored, by the tablet's clock), else (no
/// time, or one before [`EARLIEST_BOARD_PAGE`]) `now_ms`; `original.bin` = the bytes received.
/// Never fails: unknown records are skipped.
pub fn blackboard_note(
    page: &DownloadedPage,
    id: String,
    layer_name: &str,
    now_ms: i64,
) -> NoteFile {
    let decoded = blackboard::decode_page(&page.bytes);
    let max_pressure = f32::from(BOARD_MAX_PRESSURE);
    let strokes = decoded
        .strokes
        .iter()
        .filter(|s| !s.points.is_empty())
        .map(|s| Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: TABLET_PEN_WIDTH,
            layer: 0,
            points: s
                .points
                .iter()
                .map(|p| Point {
                    x: f32::from(p.x),
                    y: f32::from(p.y),
                    pressure: (f32::from(p.pressure) / max_pressure).clamp(0.0, 1.0),
                    t: p.t,
                })
                .collect(),
        })
        .collect();
    // A tablet whose clock was never set (after its battery ran out, say) dates its pages near
    // 1970: such a note is dated by the import instead. Its id still comes from the page's time
    // and bytes (`page_note_id`), so the page is recognised all the same.
    let created = page
        .timestamp
        .filter(|seconds| *seconds >= EARLIEST_BOARD_PAGE)
        .and_then(|seconds| seconds.checked_mul(1000))
        .unwrap_or(now_ms);
    let note = Note {
        id,
        title: String::new(),
        created,
        modified: now_ms.max(created),
        page: BLACKBOARD_PAGE,
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
        favorite: false,
        layers: vec![Layer {
            name: layer_name.to_owned(),
            visible: true,
        }],
        strokes,
        timing: Timing::Synthesized,
        origin: Origin::Tablet,
    };
    let mut note_file = NoteFile::new(note);
    note_file.original = Some(page.bytes.clone());
    note_file
}

/// Called once a synced note is safely in the library (automatic export).
pub type AfterSave<'a> = Box<dyn FnMut(&SavedNote, &Note) + 'a>;

/// Makes the PNG thumbnail (`thumb.png`) of a synced note, `None` if it cannot be made.
pub type ThumbnailMaker<'a> = Box<dyn Fn(&Note) -> Option<Vec<u8>> + 'a>;

/// Test hook run on the file just written, before it is read back.
#[cfg(test)]
type TamperHook<'a> = Box<dyn Fn(&std::path::Path) + 'a>;

/// How far around the time of an earlier page the notes made from it are looked for
/// ([`LibrarySink::store_page`]): a note is modified (imported) after its page was stored, give or
/// take the drift between the tablet's clock and the computer's (**supposed** at most a day; the
/// clock is set at each connection and was right an hour later: research note §6).
pub const CANDIDATE_MARGIN_MS: i64 = 86_400_000;

/// What an update of an earlier note came to ([`LibrarySink::store_page`]).
enum Update {
    /// Updated: its place, its creation time (kept).
    Done(SavedNote, i64),
    /// Not to be updated (why): a new note is added.
    Refused(String),
    /// Written, but it does not read back: the page is not safe.
    Failed(String),
}

/// What the page store did with a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    /// A new note.
    New,
    /// A new note, although the page replaces an earlier store of its screen: why the note of
    /// that store was not updated (for the log).
    NewNotUpdated(String),
    /// The note of the earlier store of the screen (the page of time `replaced`) was updated.
    Updated { replaced: u32 },
}

/// The note a page became ([`LibrarySink::store_page`], the service's page store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPage {
    pub path: PathBuf,
    /// The note's creation time (Unix ms), for [`ImportedPages::record`].
    pub created: i64,
    pub how: Stored,
}

impl StoredPage {
    /// A new note at `path`.
    pub fn new(path: PathBuf, created: i64) -> Self {
        Self {
            path,
            created,
            how: Stored::New,
        }
    }
}

/// The one note of the library made from the page of time `previous` (its id is that page's,
/// its `original.bin` that page), read; `Err` says why there is none to update.
fn earlier_note(library: &Library, previous: u32) -> Result<LoadedNote, String> {
    let time_ms = i64::from(previous) * 1000;
    let mut found = Vec::new();
    for entry in library.list().notes {
        if entry.created > time_ms || entry.modified < time_ms - CANDIDATE_MARGIN_MS {
            continue;
        }
        let Ok(loaded) = library.read(&entry.path) else {
            continue;
        };
        let Some(original) = loaded.file.original.clone() else {
            continue;
        };
        let page = DownloadedPage {
            announced_size: u32::try_from(original.len()).unwrap_or(u32::MAX),
            bytes: original,
            timestamp: Some(i64::from(previous)),
            kind: TabletKind::Blackboard,
            previous: None,
        };
        if page_note_id(&page) == loaded.file.note.id {
            found.push(loaded);
        }
    }
    match found.len() {
        0 => Err("no note of it in the library (deleted, in the trash or not imported)".into()),
        1 => Ok(found.remove(0)),
        n => Err(format!("{n} notes of it in the library")),
    }
}

/// Whether `file` is still what importing its own `original.bin` gives (`layer_name` for its
/// layer), after the format's rounding: its ink, layers, paper, background and image, timing
/// and origin, and nothing added (narration, background file, text, revision, original note).
/// Title, tags, favourite mark, thumbnail and `modified` do not count.
fn unedited_since_import(file: &NoteFile, layer_name: &str) -> Result<(), String> {
    let Some(original) = file.original.clone() else {
        return Err("it keeps no page".into());
    };
    if file.audio.is_some()
        || file.background.is_some()
        || file.text.is_some()
        || file.revision.is_some()
        || file.original_note.is_some()
    {
        return Err("something was added to it since its import".into());
    }
    let page = DownloadedPage {
        announced_size: u32::try_from(original.len()).unwrap_or(u32::MAX),
        bytes: original,
        timestamp: Some(file.note.created / 1000),
        kind: TabletKind::Blackboard,
        previous: None,
    };
    let again = blackboard_note(&page, file.note.id.clone(), layer_name, file.note.created);
    let again = format::to_bytes(&again)
        .and_then(|bytes| format::read_from_bytes(&bytes))
        .map_err(|e| e.to_string())?
        .note;
    let note = &file.note;
    let same = note.page == again.page
        && note.background == again.background
        && note.background_image == again.background_image
        && note.layers == again.layers
        && note.strokes == again.strokes
        && note.timing == again.timing
        && note.origin == again.origin;
    if same {
        Ok(())
    } else {
        Err("it was edited since its import".into())
    }
}

/// Stores synced pages as new notes of a library binder.
pub struct LibrarySink<'a> {
    library: &'a Library,
    binder: PathBuf,
    size: PageSize,
    orientation: Orientation,
    layer_name: String,
    now_ms: Box<dyn Fn() -> i64 + 'a>,
    after_save: Option<AfterSave<'a>>,
    thumbnail: Option<ThumbnailMaker<'a>>,
    /// Test hook run on the file just written, before it is read back.
    #[cfg(test)]
    tamper: Option<TamperHook<'a>>,
    /// Test hook: this many reads of the file just written fail, as when a sync client or an
    /// antivirus holds it a moment.
    #[cfg(test)]
    locked_reads: std::cell::Cell<u32>,
    /// Notes written so far, in order.
    pub saved: Vec<SavedNote>,
}

impl<'a> LibrarySink<'a> {
    /// Pages of a tablet of `size` go into `binder` (relative to the library root; empty for the
    /// root); their single layer is called `layer_name` (translated by the caller).
    pub fn new(
        library: &'a Library,
        binder: impl Into<PathBuf>,
        size: PageSize,
        layer_name: impl Into<String>,
        now_ms: impl Fn() -> i64 + 'a,
    ) -> Self {
        Self {
            library,
            binder: binder.into(),
            size,
            orientation: Orientation::Landscape,
            layer_name: layer_name.into(),
            now_ms: Box::new(now_ms),
            after_save: None,
            thumbnail: None,
            #[cfg(test)]
            tamper: None,
            #[cfg(test)]
            locked_reads: std::cell::Cell::new(0),
            saved: Vec::new(),
        }
    }

    /// Turns the points to the way the tablet is held ([`orient`]).
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Runs `hook` after each note is saved and verified.
    pub fn with_after_save(mut self, hook: impl FnMut(&SavedNote, &Note) + 'a) -> Self {
        self.after_save = Some(Box::new(hook));
        self
    }

    /// Gives each note a thumbnail made by `maker` (SPEC §3.3: `thumb.png`).
    pub fn with_thumbnail(mut self, maker: impl Fn(&Note) -> Option<Vec<u8>> + 'a) -> Self {
        self.thumbnail = Some(Box::new(maker));
        self
    }

    /// [`store_page`](Self::store_page), the note's place and stamp.
    pub fn store(&mut self, page: DownloadedPage) -> Result<SavedNote, String> {
        self.store_page(page)?;
        self.saved
            .last()
            .cloned()
            .ok_or_else(|| "the page store wrote nothing".to_owned())
    }

    /// Saves `page` as a note and checks it reads back identical: the one page store of the
    /// sync, of the rescue copies and of `--import-blackboard-pages`.
    ///
    /// A page of the blackboard that replaces an earlier store of its screen
    /// ([`DownloadedPage::previous`]) updates the note of that store when there is exactly one,
    /// unedited since its import, and the new page holds it whole (SPEC §4.1,
    /// docs/design/blackboard.md §9.6); otherwise, and for every other page, a new note (id:
    /// [`page_note_id`]). An `Err` keeps the page on the tablet (the session does not delete
    /// it); a new note that failed its check is put aside ([`discard`](Self::discard)) so that
    /// it never counts as imported: the next sync writes the page again.
    pub fn store_page(&mut self, page: DownloadedPage) -> Result<StoredPage, String> {
        let not_updated = match page.previous {
            Some(previous) if page.kind == TabletKind::Blackboard => {
                match self.update(&page, previous) {
                    Update::Done(saved, created) => {
                        return Ok(StoredPage {
                            path: saved.path,
                            created,
                            how: Stored::Updated { replaced: previous },
                        });
                    }
                    Update::Refused(why) => Some(why),
                    Update::Failed(error) => return Err(error),
                }
            }
            _ => None,
        };
        let (saved, created) = self.create(page)?;
        let how = match not_updated {
            Some(why) => Stored::NewNotUpdated(why),
            None => Stored::New,
        };
        Ok(StoredPage {
            path: saved.path,
            created,
            how,
        })
    }

    /// Updates the note of the page of time `previous` with `page` (docs/design/blackboard.md
    /// §9.6).
    fn update(&mut self, page: &DownloadedPage, previous: u32) -> Update {
        let why =
            |reason: String| Update::Refused(format!("its earlier store ({previous}): {reason}"));
        let loaded = match earlier_note(self.library, previous) {
            Ok(loaded) => loaded,
            Err(reason) => return why(reason),
        };
        if let Err(reason) = unedited_since_import(&loaded.file, &self.layer_name) {
            return why(reason);
        }
        let earlier = loaded.file.original.as_deref().unwrap_or_default();
        if !page.bytes.starts_with(earlier) {
            return why("the new page does not hold it whole".into());
        }
        let made = blackboard_note(page, page_note_id(page), &self.layer_name, (self.now_ms)());
        let mut file = loaded.file.clone();
        file.note.id = made.note.id;
        file.note.page = made.note.page;
        file.note.layers = made.note.layers;
        file.note.strokes = made.note.strokes;
        file.note.timing = made.note.timing;
        file.note.modified = made.note.modified.max(file.note.modified);
        file.original = made.original;
        file.thumbnail = self.thumbnail.as_ref().and_then(|maker| maker(&file.note));
        let expected = match format::to_bytes(&file).and_then(|b| format::read_from_bytes(&b)) {
            Ok(expected) => expected,
            Err(error) => return why(error.to_string()),
        };
        // The library's checked save: a file changed since it was read is never overwritten.
        let stamp = match self.library.save(&loaded.path, &file, &loaded.stamp) {
            Ok(stamp) => stamp,
            Err(error @ ardoise_core::library::LibraryError::Conflict { .. }) => {
                return why(error.to_string());
            }
            Err(error) => return Update::Failed(error.to_string()),
        };
        let saved = SavedNote {
            path: loaded.path,
            stamp,
        };
        #[cfg(test)]
        if let Some(tamper) = &self.tamper {
            tamper(&saved.path);
        }
        // Never discarded: the file is the note the user had (the new page holds it whole).
        let back = match self.read_back(&saved.path) {
            Ok(back) => back,
            Err(error) => return Update::Failed(error),
        };
        if back.file.note != expected.note
            || back.file.original != file.original
            || back.file.thumbnail != file.thumbnail
        {
            return Update::Failed(format!(
                "{} does not read back as written",
                saved.path.display()
            ));
        }
        if let Some(hook) = self.after_save.as_mut() {
            hook(&saved, &back.file.note);
        }
        self.saved.push(saved.clone());
        Update::Done(saved, back.file.note.created)
    }

    /// Saves `page` as a new note and checks it reads back identical; its place and its
    /// creation time.
    fn create(&mut self, page: DownloadedPage) -> Result<(SavedNote, i64), String> {
        let id = page_note_id(&page);
        let mut file = note_from_page(
            &page,
            self.size,
            self.orientation,
            id,
            &self.layer_name,
            (self.now_ms)(),
        )
        .map_err(|e| e.to_string())?;
        if let Some(maker) = &self.thumbnail {
            file.thumbnail = maker(&file.note);
        }
        // The bytes `Library::create` writes (the encoding is deterministic), and what the file
        // must hold: the note as the format stores it (pressures and positions are quantised),
        // the tablet's bytes untouched, the thumbnail given. Encoded first: a note that cannot
        // be encoded leaves no file.
        let written = format::to_bytes(&file).map_err(|e| e.to_string())?;
        let expected = format::read_from_bytes(&written).map_err(|e| e.to_string())?;
        let saved = self
            .library
            .create(&self.binder, &file)
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        if let Some(tamper) = &self.tamper {
            tamper(&saved.path);
        }
        let back = match self.read_back(&saved.path) {
            Ok(back) => back,
            Err(error) => {
                self.discard(&saved, &written);
                return Err(error);
            }
        };
        if back.file.note != expected.note
            || back.file.original != file.original
            || back.file.thumbnail != file.thumbnail
        {
            self.discard(&saved, &written);
            return Err(format!(
                "{} does not read back as written",
                saved.path.display()
            ));
        }
        if let Some(hook) = self.after_save.as_mut() {
            hook(&saved, &back.file.note);
        }
        self.saved.push(saved.clone());
        Ok((saved, back.file.note.created))
    }

    /// Reads back the note just written.
    fn read_back(&self, path: &Path) -> Result<LoadedNote, String> {
        #[cfg(test)]
        if self.locked() {
            return Err(format!("{} is locked", path.display()));
        }
        self.library.read(path).map_err(|e| e.to_string())
    }

    /// The bytes of the file just written, as they are on the disk.
    fn bytes_on_disk(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        #[cfg(test)]
        if self.locked() {
            return Err(std::io::Error::other("locked"));
        }
        std::fs::read(path)
    }

    /// Test hook: whether this read fails as if the file were held by another program.
    #[cfg(test)]
    fn locked(&self) -> bool {
        let left = self.locked_reads.get();
        self.locked_reads.set(left.saturating_sub(1));
        left > 0
    }

    /// Puts aside a copy that failed its check (`written`: the bytes it was written with), so
    /// that it never counts as imported. Best effort: the page stays on the tablet either way.
    ///
    /// - The file holds exactly `written` (the read-back failed, e.g. a sync client held the
    ///   file a moment): it is this copy, deleted for good; nothing is lost.
    /// - It holds something else (replaced by another program, or damaged): moved to the
    ///   trash, never destroyed; it does not count as imported there
    ///   ([`ImportedPages::in_trash`] wants the page's id and bytes).
    /// - It cannot be read at all: left where it is. The next sync recognises it if it turns out
    ///   to be the complete copy ([`ImportedPages::find`]), else writes the page again.
    fn discard(&self, saved: &SavedNote, written: &[u8]) {
        match self.bytes_on_disk(&saved.path) {
            Ok(bytes) if bytes == written => {
                let _ = remove_file_with_retry(&saved.path);
            }
            Ok(_) => {
                let _ = self.library.trash(&saved.path);
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::{A4_LANDSCAPE, A4_PORTRAIT};
    use ardoise_proto::stroke::{Point as RawPoint, Stroke as RawStroke};

    fn raw_stroke(millis: u32, x: u16, n: u16) -> RawStroke {
        RawStroke {
            time: StrokeTime::SincePowerUp { millis },
            pen: None,
            points: (0..n)
                .map(|i| RawPoint {
                    x: x + i * 10,
                    y: 5000,
                    pressure: 1024 + i,
                })
                .collect(),
            lost_points: 0,
        }
    }

    fn page(strokes: &[RawStroke]) -> DownloadedPage {
        let bytes = stroke::encode_slate(strokes);
        DownloadedPage {
            announced_size: bytes.len() as u32,
            bytes,
            // 2026-10-24 09:30:00 UTC.
            timestamp: Some(1_792_834_200),
            kind: TabletKind::Slate,
            previous: None,
        }
    }

    #[test]
    fn a_page_becomes_a_tablet_note_with_unwrapped_times() {
        // The second stroke starts after the clock wrapped (327 000 ms → 1 000 ms).
        let page = page(&[raw_stroke(327_000, 1000, 4), raw_stroke(1_000, 2000, 3)]);
        let file = note_from_page(
            &page,
            A4_LANDSCAPE,
            Orientation::Landscape,
            "id".into(),
            "Calque 1",
            5,
        )
        .unwrap();
        let note = &file.note;
        assert_eq!(note.created, 1_792_834_200_000);
        assert_eq!(note.origin, Origin::Tablet);
        assert_eq!(note.page, A4_LANDSCAPE);
        assert_eq!(note.layers[0].name, "Calque 1");
        assert_eq!(file.original.as_deref(), Some(page.bytes.as_slice()));
        assert_eq!(note.strokes.len(), 2);
        let first = &note.strokes[0];
        assert_eq!(first.tool, Tool::Ballpoint);
        assert_eq!(
            first.points.iter().map(|p| p.t).collect::<Vec<_>>(),
            [0, 5, 10, 15]
        );
        assert_eq!((first.points[0].x, first.points[0].y), (1000.0, 5000.0));
        assert!((first.points[0].pressure - 1024.0 / 2047.0).abs() < 1e-6);
        // 327 000 → 327 680 (wrap) → 1 000: 1 680 ms later.
        assert_eq!(note.strokes[1].points[0].t, 1_680);
    }

    #[test]
    fn the_sink_saves_verifies_and_calls_the_hook() {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        let mut seen = Vec::new();
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7)
            .with_after_save(|saved, note| seen.push((saved.path.clone(), note.strokes.len())));
        let first = sink.store(page(&[raw_stroke(0, 100, 5)])).unwrap();
        let second = sink
            .store(page(&[raw_stroke(0, 100, 5), raw_stroke(900, 300, 2)]))
            .unwrap();
        assert_ne!(first.path, second.path);
        assert_eq!(sink.saved.len(), 2);
        drop(sink);
        assert_eq!(seen, [(first.path.clone(), 1), (second.path.clone(), 2)]);
        let listed = library.list();
        assert_eq!(listed.notes.len(), 2);
        let back = library.read(&first.path).unwrap();
        assert_eq!(back.file.note.origin, Origin::Tablet);

        // An undecodable page is refused: it stays on the tablet.
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        let bad = DownloadedPage {
            bytes: vec![1, 2, 3],
            timestamp: None,
            announced_size: 3,
            kind: TabletKind::Slate,
            previous: None,
        };
        assert!(sink.store(bad).is_err());
        assert_eq!(library.list().notes.len(), 2);
    }

    #[test]
    fn notes_get_a_thumbnail_when_a_maker_is_given() {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7)
            .with_thumbnail(|note| Some(vec![0x89, b'P', note.strokes.len() as u8]));
        let saved = sink.store(page(&[raw_stroke(0, 100, 5)])).unwrap();
        let back = library.read(&saved.path).unwrap();
        assert_eq!(back.file.thumbnail.as_deref(), Some(&[0x89, b'P', 1][..]));
        // Without a maker, no thumbnail.
        let mut plain = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        let saved = plain.store(page(&[raw_stroke(0, 100, 5)])).unwrap();
        assert!(library.read(&saved.path).unwrap().file.thumbnail.is_none());
    }

    #[test]
    fn a_note_that_fails_verification_goes_to_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        // Another program replaces the file between the write and the read-back.
        sink.tamper = Some(Box::new(|path| {
            let mut other = note_from_page(
                &page(&[raw_stroke(0, 100, 5)]),
                A4_LANDSCAPE,
                Orientation::Landscape,
                "id".into(),
                "L",
                1,
            )
            .unwrap();
            other.note.title = "Other".into();
            std::fs::write(path, format::to_bytes(&other).unwrap()).unwrap();
        }));
        let error = sink.store(page(&[raw_stroke(0, 100, 5)])).unwrap_err();
        assert!(error.contains("does not read back"), "{error}");
        assert!(sink.saved.is_empty());
        // No duplicate left in the library; the file, which is not ours, is kept in the trash.
        assert!(library.list().notes.is_empty());
        assert_eq!(library.trash_items().unwrap().len(), 1);
    }

    /// The copies that failed their check, as a sync without deletion meets them next time:
    /// none counts as imported, so the page is written again (it would otherwise never be
    /// imported, and hide every later page of the tablet).
    #[test]
    fn a_copy_that_failed_its_check_never_counts_as_imported() {
        let fresh = || {
            let dir = tempfile::tempdir().unwrap();
            ardoise_core::library::init(dir.path()).unwrap();
            let library = Library::open(dir.path()).unwrap();
            (dir, library)
        };
        let oldest = page(&[raw_stroke(0, 100, 5)]);

        // The read-back fails (a sync client holds the file a moment): the file holds exactly
        // what was written, so it is deleted for good, not trashed...
        let (_dir, library) = fresh();
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        sink.locked_reads.set(1);
        let error = sink.store(oldest.clone()).unwrap_err();
        assert!(error.contains("locked"), "{error}");
        assert!(library.list().notes.is_empty());
        assert!(library.trash_items().unwrap().is_empty());
        let imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &oldest), None);
        assert!(!imported.in_trash(&oldest));
        // ...and the next sync writes the page again.
        let saved = sink.store(oldest.clone()).unwrap();
        let imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &oldest), Some(saved.path));

        // Replaced meanwhile by a note with the page's id but other bytes: kept in the trash,
        // where it does not count.
        let (_dir, library) = fresh();
        let other = page(&[raw_stroke(0, 900, 9)]);
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        let id = page_note_id(&oldest);
        sink.tamper = Some(Box::new(move |path| {
            let mut file = note_from_page(
                &other,
                A4_LANDSCAPE,
                Orientation::Landscape,
                id.clone(),
                "L",
                1,
            )
            .unwrap();
            file.note.title = "Other".into();
            std::fs::write(path, format::to_bytes(&file).unwrap()).unwrap();
        }));
        assert!(sink.store(oldest.clone()).is_err());
        let trashed = library.trash_items().unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(
            trashed[0].note_id.as_deref(),
            Some(page_note_id(&oldest).as_str())
        );
        assert!(!ImportedPages::scan(&library).in_trash(&oldest));

        // Neither the read-back nor the look at the bytes on disk can read the file: it stays
        // in the library, and the next sync recognises it once it can be read.
        let (_dir, library) = fresh();
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        sink.locked_reads.set(2);
        assert!(sink.store(oldest.clone()).is_err());
        let left = library.list().notes;
        assert_eq!(left.len(), 1);
        assert!(library.trash_items().unwrap().is_empty());
        let imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &oldest), Some(left[0].path.clone()));
    }

    #[test]
    fn flipped_landscape_turns_points_half_a_turn() {
        assert_eq!(
            orient(1000, 5000, 29_700, 21_600, Orientation::Landscape),
            (1000, 5000)
        );
        assert_eq!(
            orient(1000, 5000, 29_700, 21_600, Orientation::LandscapeFlipped),
            (28_700, 16_600)
        );
        assert_eq!(
            orient(0, 0, 29_700, 21_600, Orientation::LandscapeFlipped),
            (29_700, 21_600)
        );
        // Outside the area: clamped, never wrapped.
        assert_eq!(
            orient(
                30_000,
                21_600,
                29_700,
                21_600,
                Orientation::LandscapeFlipped
            ),
            (0, 0)
        );
        let pen = LiveRecord::Pen(RawPoint {
            x: 2500,
            y: 500,
            pressure: 700,
        });
        assert_eq!(
            orient_record(pen, 21_600, 14_800, Orientation::LandscapeFlipped),
            LiveRecord::Pen(RawPoint {
                x: 19_100,
                y: 14_300,
                pressure: 700
            })
        );
        assert_eq!(
            orient_record(
                LiveRecord::ProximityOut,
                21_600,
                14_800,
                Orientation::LandscapeFlipped
            ),
            LiveRecord::ProximityOut
        );

        // A synced page held flipped.
        let page = page(&[raw_stroke(0, 1000, 2)]);
        let file = note_from_page(
            &page,
            A4_LANDSCAPE,
            Orientation::LandscapeFlipped,
            "id".into(),
            "Calque 1",
            5,
        )
        .unwrap();
        assert_eq!(file.note.page, A4_LANDSCAPE);
        let first = &file.note.strokes[0].points;
        assert_eq!((first[0].x, first[0].y), (28_700.0, 16_600.0));
        assert_eq!((first[1].x, first[1].y), (28_690.0, 16_600.0));
        // The tablet's bytes are kept untouched.
        assert_eq!(file.original.as_deref(), Some(page.bytes.as_slice()));
    }

    /// The tablet's four corners, as the user sees them, for each position. The button is at the
    /// bottom of the tablet's own frame: it ends up on the left (a quarter turn clockwise) or on
    /// the right (anticlockwise) in portrait.
    #[test]
    fn the_four_positions_map_corners_to_corners() {
        const W: u32 = 29_700;
        const H: u32 = 21_600;
        let (w, h) = (W as u16, H as u16);
        // (position, page size, images of the tablet's top-left, top-right, bottom-left and
        // bottom-right corners)
        let cases = [
            (
                Orientation::Landscape,
                (W, H),
                [(0, 0), (w, 0), (0, h), (w, h)],
            ),
            (
                Orientation::LandscapeFlipped,
                (W, H),
                [(w, h), (0, h), (w, 0), (0, 0)],
            ),
            (
                Orientation::PortraitLeft,
                (H, W),
                [(h, 0), (h, w), (0, 0), (0, w)],
            ),
            (
                Orientation::PortraitRight,
                (H, W),
                [(0, w), (0, 0), (h, w), (h, 0)],
            ),
        ];
        for (orientation, (page_w, page_h), expected) in cases {
            assert_eq!(
                orientation.page_size(W, H),
                (page_w, page_h),
                "{orientation:?}"
            );
            let corners = [(0, 0), (w, 0), (0, h), (w, h)];
            for (corner, want) in corners.into_iter().zip(expected) {
                let got = orient(corner.0, corner.1, W, H, orientation);
                assert_eq!(got, want, "{orientation:?} {corner:?}");
                // Always inside the page.
                assert!(u32::from(got.0) <= page_w && u32::from(got.1) <= page_h);
                // The float version, used on ink already on a page, does the same.
                let turned = turn_point(
                    f32::from(corner.0),
                    f32::from(corner.1),
                    W as f32,
                    H as f32,
                    orientation.quarter_turns(),
                );
                assert_eq!(turned, (f32::from(want.0), f32::from(want.1)));
            }
        }
        // A point that is not a corner: 5 000 across and 3 000 down the tablet.
        assert_eq!(
            orient(5000, 3000, W, H, Orientation::PortraitLeft),
            (h - 3000, 5000)
        );
        assert_eq!(
            orient(5000, 3000, W, H, Orientation::PortraitRight),
            (3000, w - 5000)
        );
    }

    #[test]
    fn turning_a_page_round_trips_and_moves_between_positions() {
        let (mut width, mut height) = (29_700.0_f32, 21_600.0_f32);
        let start = (1234.0_f32, 5678.0_f32);
        let mut point = start;
        for _ in 0..4 {
            point = turn_point(point.0, point.1, width, height, 1);
            (width, height) = (height, width);
            assert!((0.0..=width).contains(&point.0) && (0.0..=height).contains(&point.1));
        }
        assert_eq!(point, start);
        let once = turn_point(start.0, start.1, 29_700.0, 21_600.0, 1);
        assert_eq!(turn_point(once.0, once.1, 21_600.0, 29_700.0, 3), start);

        // From one position to another is the difference of their quarter turns.
        for from in Orientation::ALL {
            for to in Orientation::ALL {
                let first = orient(5000, 3000, 29_700, 21_600, from);
                let (page_w, page_h) = from.page_size(29_700, 21_600);
                let moved = turn_point(
                    f32::from(first.0),
                    f32::from(first.1),
                    page_w as f32,
                    page_h as f32,
                    to.turns_from(from),
                );
                let direct = orient(5000, 3000, 29_700, 21_600, to);
                assert_eq!(
                    moved,
                    (f32::from(direct.0), f32::from(direct.1)),
                    "{from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn a_portrait_position_makes_a_portrait_note() {
        let page = page(&[raw_stroke(0, 1000, 2)]);
        for orientation in [Orientation::PortraitLeft, Orientation::PortraitRight] {
            let file = note_from_page(&page, A4_LANDSCAPE, orientation, "id".into(), "Calque 1", 5)
                .unwrap();
            assert_eq!(file.note.page, A4_PORTRAIT, "{orientation:?}");
            for stroke in &file.note.strokes {
                for p in &stroke.points {
                    assert!((0.0..=A4_PORTRAIT.width as f32).contains(&p.x), "{p:?}");
                    assert!((0.0..=A4_PORTRAIT.height as f32).contains(&p.y), "{p:?}");
                }
            }
            assert_eq!(file.original.as_deref(), Some(page.bytes.as_slice()));
        }
    }

    #[test]
    fn page_ids_are_deterministic_uuids() {
        let a = page(&[raw_stroke(0, 100, 5)]);
        let b = page(&[raw_stroke(0, 100, 6)]);
        let id = page_note_id(&a);
        assert_eq!(id, page_note_id(&a.clone()));
        assert_ne!(id, page_note_id(&b));
        let mut later = a.clone();
        later.timestamp = Some(1_792_834_201);
        assert_ne!(id, page_note_id(&later));
        later.timestamp = None;
        assert_ne!(id, page_note_id(&later));
        // 8-4-4-4-12 hex digits, version 8, RFC 9562 variant.
        let parts: Vec<_> = id.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12], "{id}");
        assert_eq!(&id[14..15], "8", "{id}");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"), "{id}");
        // Pinned, so that a change of the hash (which would import again every page kept on a
        // tablet) is deliberate. The announced size is not part of the page.
        let small = |announced_size| DownloadedPage {
            bytes: vec![1, 2, 3],
            timestamp: Some(0),
            announced_size,
            kind: TabletKind::Slate,
            previous: None,
        };
        assert_eq!(
            page_note_id(&small(3)),
            "eb8a2121-4b58-8f30-9181-c47e03a5133c"
        );
        assert_eq!(page_note_id(&small(3)), page_note_id(&small(99)));
        let random = new_note_id().unwrap();
        assert_eq!(&random[14..15], "4", "{random}");
    }

    fn board_page() -> DownloadedPage {
        use ardoise_proto::blackboard::{Record, RecordStatus, record::encode_records};
        let rec = |status, x, y, pressure| Record {
            status,
            x,
            y,
            pressure,
        };
        let bytes = encode_records(&[
            rec(RecordStatus::Hover, 100, 200, 0),
            rec(RecordStatus::Touch, 100, 200, 1024),
            rec(RecordStatus::Touch, 110, 210, 2047),
            rec(RecordStatus::Out, 110, 210, 0),
            rec(RecordStatus::Touch, 20_401, 27_067, 500),
        ]);
        DownloadedPage {
            announced_size: bytes.len() as u32,
            bytes,
            timestamp: Some(1_792_834_200),
            kind: TabletKind::Blackboard,
            previous: None,
        }
    }

    #[test]
    fn a_blackboard_page_becomes_a_portrait_note_with_synthesised_times() {
        let page = board_page();
        // The size and orientation given are the Slate's: a blackboard page ignores them.
        let file = note_from_page(
            &page,
            A4_LANDSCAPE,
            Orientation::PortraitLeft,
            "id".into(),
            "Calque 1",
            5,
        )
        .unwrap();
        let note = &file.note;
        assert_eq!(note.page, BLACKBOARD_PAGE);
        assert!(note.page.width < note.page.height, "portrait");
        assert_eq!(note.timing, Timing::Synthesized);
        assert_eq!(note.origin, Origin::Tablet);
        assert_eq!(note.created, 1_792_834_200_000);
        assert_eq!(file.original.as_deref(), Some(page.bytes.as_slice()));
        assert_eq!(note.strokes.len(), 2);
        let first = &note.strokes[0];
        assert_eq!((first.points[0].x, first.points[0].y), (100.0, 200.0));
        assert!((first.points[0].pressure - 1024.0 / 2047.0).abs() < 1e-6);
        assert_eq!(
            first.points.iter().map(|p| p.t).collect::<Vec<_>>(),
            [8, 16]
        );
        // Untouched coordinates, half a second after the pen left.
        let second = &note.strokes[1].points[0];
        assert_eq!((second.x, second.y), (20_401.0, 27_067.0));
        assert_eq!(second.t, 33 + 500);
        // Kept bytes are recognised without their tablet.
        assert_eq!(page_kind(&page.bytes), TabletKind::Blackboard);
        assert_eq!(
            page_kind(&stroke::encode_slate(&[raw_stroke(0, 100, 5)])),
            TabletKind::Slate
        );
        assert_eq!(page_kind(&[]), TabletKind::Slate);
        assert_eq!(page_kind(&[0xa1; 6]), TabletKind::Slate);
        assert_eq!(page_kind(&[0x07; 7]), TabletKind::Slate);
    }

    #[test]
    fn a_blackboard_page_of_a_clock_never_set_is_dated_by_the_import() {
        let mut page = board_page();
        for seconds in [0, EARLIEST_BOARD_PAGE - 1] {
            page.timestamp = Some(seconds);
            let file = note_from_page(
                &page,
                A4_LANDSCAPE,
                Orientation::Landscape,
                page_note_id(&page),
                "Calque 1",
                1_792_834_200_123,
            )
            .unwrap();
            assert_eq!(file.note.created, 1_792_834_200_123);
            // The id still comes from the time and the bytes: the page is recognised again.
            assert_eq!(file.note.id, page_note_id(&page));
        }
        page.timestamp = Some(EARLIEST_BOARD_PAGE);
        let file = note_from_page(
            &page,
            A4_LANDSCAPE,
            Orientation::Landscape,
            "id".into(),
            "L",
            9,
        )
        .unwrap();
        assert_eq!(file.note.created, EARLIEST_BOARD_PAGE * 1000);
    }

    #[test]
    fn a_blackboard_page_is_stored_and_recognised_like_a_slate_page() {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        let page = board_page();
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        let saved = sink.store(page.clone()).unwrap();
        let back = library.read(&saved.path).unwrap().file;
        assert_eq!(back.note.page, BLACKBOARD_PAGE);
        assert_eq!(back.note.id, page_note_id(&page));
        let imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &page), Some(saved.path));
    }

    #[test]
    fn imported_pages_are_recognised_by_id_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        let first = page(&[raw_stroke(0, 100, 5)]);
        let second = page(&[raw_stroke(0, 300, 5)]);
        let mut sink = LibrarySink::new(&library, "", A4_LANDSCAPE, "Layer 1", || 7);
        let saved = sink.store(first.clone()).unwrap();
        let back = library.read(&saved.path).unwrap();
        assert_eq!(back.file.note.id, page_note_id(&first));

        let mut imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &first), Some(saved.path.clone()));
        assert_eq!(imported.find(&library, &second), None);
        assert!(!imported.in_trash(&first));
        let other = sink.store(second.clone()).unwrap();
        imported.add(page_note_id(&second), &other.path);
        assert_eq!(imported.find(&library, &second), Some(other.path.clone()));

        // A deleted note: in the trash, no longer in the library.
        library.trash(&saved.path).unwrap();
        let imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &first), None);
        assert!(imported.in_trash(&first));
        assert!(!imported.in_trash(&second));
    }
}

#[cfg(test)]
mod update_tests {
    //! A screen of the blackboard stored again updates the note of its earlier store (SPEC
    //! §4.1, docs/design/blackboard.md §9.6).
    use super::*;
    use ardoise_proto::blackboard::record::encode_records;
    use ardoise_proto::blackboard::{Record, RecordStatus};

    /// The records of a screen: `n` touching points along a line.
    fn screen(n: u16) -> Vec<u8> {
        let mut records = vec![Record {
            status: RecordStatus::Hover,
            x: 100,
            y: 200,
            pressure: 0,
        }];
        records.extend((0..n).map(|i| Record {
            status: RecordStatus::Touch,
            x: 100 + i * 30,
            y: 200 + i * 7,
            pressure: 900,
        }));
        records.push(Record {
            status: RecordStatus::Out,
            x: 100,
            y: 200,
            pressure: 0,
        });
        encode_records(&records)
    }

    fn page(time: i64, bytes: Vec<u8>, previous: Option<u32>) -> DownloadedPage {
        DownloadedPage {
            announced_size: bytes.len() as u32,
            bytes,
            timestamp: Some(time),
            kind: TabletKind::Blackboard,
            previous,
        }
    }

    /// 2026-10-24 09:30:00 UTC, and the second store a minute later.
    const FIRST: i64 = 1_792_834_200;
    const SECOND: i64 = FIRST + 60;
    const NOW: i64 = 1_792_843_200_000;

    fn library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        (dir, library)
    }

    fn store(library: &Library, page: DownloadedPage) -> StoredPage {
        LibrarySink::new(library, "", BLACKBOARD_PAGE, "Calque 1", || NOW)
            .store_page(page)
            .unwrap()
    }

    /// The second store holding the first whole updates its note in place: path, title, tags,
    /// favourite, creation date kept; ink, kept page and id become the new page's; both pages
    /// count as imported.
    #[test]
    fn the_note_of_the_earlier_store_is_updated() {
        let (_dir, library) = library();
        let first = page(FIRST, screen(5), None);
        let stored = store(&library, first.clone());
        assert_eq!(stored.how, Stored::New);
        let path = library.retitle(&stored.path, "Leçon", None).unwrap().path;
        library.set_tags(&path, &["classe".into()], None).unwrap();
        let mut more = screen(5);
        more.extend(screen(8));
        let second = page(SECOND, more.clone(), Some(FIRST as u32));
        let updated = store(&library, second.clone());
        assert_eq!(
            updated.how,
            Stored::Updated {
                replaced: FIRST as u32
            }
        );
        assert_eq!(updated.path, path);
        assert_eq!(updated.created, FIRST * 1000);
        let file = library.read(&path).unwrap().file;
        assert_eq!(file.note.title, "Leçon");
        assert_eq!(file.note.tags, ["classe"]);
        assert_eq!(file.note.created, FIRST * 1000);
        assert!(file.note.modified >= NOW);
        assert_eq!(file.note.id, page_note_id(&second));
        assert_eq!(file.original, Some(more));
        assert_eq!(file.note.strokes.len(), 2);
        assert_eq!(library.list().notes.len(), 1);
        // Both pages count as imported (a rescue copy of either does not come back).
        let mut imported = ImportedPages::scan(&library);
        assert_eq!(imported.find(&library, &second), Some(path.clone()));
        assert_eq!(imported.find(&library, &first), Some(path.clone()));
        imported.record(&second, &path, updated.created);
        assert_eq!(imported.find(&library, &first), Some(path));
        // A third store of the same screen finds the updated note (its id is the second page's).
        let mut most = second.bytes.clone();
        most.extend(screen(3));
        let third = store(&library, page(SECOND + 60, most, Some(SECOND as u32)));
        assert_eq!(
            third.how,
            Stored::Updated {
                replaced: SECOND as u32
            }
        );
        assert_eq!(library.list().notes.len(), 1);
    }

    /// The guards: an edited note, a note in the trash or none, a page that does not hold the
    /// earlier one whole, a layer named in another language: a new note, and why.
    #[test]
    fn otherwise_a_new_note_is_added_and_the_earlier_one_left_as_it_is() {
        let refused = |prepare: &dyn Fn(&Library, &Path), second: Vec<u8>| -> (Stored, usize) {
            let (_dir, library) = library();
            let stored = store(&library, page(FIRST, screen(5), None));
            prepare(&library, &stored.path);
            let before = library.read(&stored.path).ok().map(|l| l.file);
            let added = store(&library, page(SECOND, second, Some(FIRST as u32)));
            if let Some(before) = before {
                assert_eq!(
                    library.read(&stored.path).unwrap().file,
                    before,
                    "untouched"
                );
            }
            (added.how, library.list().notes.len())
        };
        let mut whole = screen(5);
        whole.extend(screen(8));
        let reason = |how: &Stored| match how {
            Stored::NewNotUpdated(why) => why.clone(),
            other => panic!("{other:?}"),
        };
        // Edited ink.
        let (how, notes) = refused(
            &|library, path| {
                let loaded = library.read(path).unwrap();
                let mut file = loaded.file;
                file.note.strokes[0].points.pop();
                library.save(path, &file, &loaded.stamp).unwrap();
            },
            whole.clone(),
        );
        assert!(reason(&how).contains("edited"), "{how:?}");
        assert_eq!(notes, 2);
        // A renamed layer.
        let (how, _) = refused(
            &|library, path| {
                let loaded = library.read(path).unwrap();
                let mut file = loaded.file;
                file.note.layers[0].name = "Mon calque".into();
                library.save(path, &file, &loaded.stamp).unwrap();
            },
            whole.clone(),
        );
        assert!(reason(&how).contains("edited"), "{how:?}");
        // In the trash.
        let (how, notes) = refused(
            &|library, path| {
                library.trash(path).unwrap();
            },
            whole.clone(),
        );
        assert!(reason(&how).contains("no note"), "{how:?}");
        assert_eq!(notes, 1);
        // Not held whole: the new page does not start with the earlier one.
        let mut other = screen(8);
        other.extend(screen(5));
        let (how, notes) = refused(&|_, _| {}, other);
        assert!(reason(&how).contains("does not hold it whole"), "{how:?}");
        assert_eq!(notes, 2);
        // Title, tags and favourite do not count: updated.
        let (_dir, library) = library();
        let stored = store(&library, page(FIRST, screen(5), None));
        library.set_favorite(&stored.path, true, None).unwrap();
        let updated = store(&library, page(SECOND, whole, Some(FIRST as u32)));
        assert!(matches!(updated.how, Stored::Updated { .. }));
        assert!(library.read(&updated.path).unwrap().file.note.favorite);
    }
}
