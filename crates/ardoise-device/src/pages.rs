//! Synced pages become notes of the library (SPEC §4.1). Every synchronisation goes through
//! [`LibrarySink::store`]: it writes the note, reads it back to verify it, and only then lets the
//! session delete the page from the tablet; then the after-save hook runs (automatic export).
//!
//! A synced note's id is derived from the page ([`page_note_id`]), so that a page synced again
//! (sync without deletion, or a deletion that failed after the save) is recognised
//! ([`ImportedPages`]) instead of being stored twice. A copy that failed its check never counts
//! as imported ([`LibrarySink::store`]). Points are turned for a left-handed user ([`orient`]),
//! in synced pages and in live mode alike.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ardoise_core::format::{self, NoteFile};
use ardoise_core::fsutil::remove_file_with_retry;
use ardoise_core::library::{Library, LoadedNote, SavedNote};
use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_core::settings::Hand;
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_proto::SLATE_MAX_PRESSURE;
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::{self, StrokeError, StrokeTime};

use crate::session::DownloadedPage;

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

/// A point of the tablet in page coordinates. A left-handed user turns the tablet half a turn,
/// so the point is turned back: `x' = width - x`, `y' = height - y` (SPEC §4.1;
/// docs/design/tablet-live-classroom.md, "Settings"). Points beyond the area are clamped.
pub fn orient(x: u16, y: u16, width: u32, height: u32, hand: Hand) -> (u16, u16) {
    let turn = |value: u16, size: u32| {
        u16::try_from(size.saturating_sub(u32::from(value))).unwrap_or(u16::MAX)
    };
    match hand {
        Hand::Right => (x, y),
        Hand::Left => (turn(x, width), turn(y, height)),
    }
}

/// [`orient`] applied to a live record ("pen left" records stay as they are).
pub fn orient_record(record: LiveRecord, width: u32, height: u32, hand: Hand) -> LiveRecord {
    match record {
        LiveRecord::Pen(mut point) => {
            (point.x, point.y) = orient(point.x, point.y, width, height, hand);
            LiveRecord::Pen(point)
        }
        LiveRecord::ProximityOut => LiveRecord::ProximityOut,
    }
}

/// The synced pages already in a library, found by their note id ([`page_note_id`]).
#[derive(Debug, Default)]
pub struct ImportedPages {
    listed: HashMap<String, PathBuf>,
    /// Files of the trash by note id (a note may have been trashed more than once).
    trashed: HashMap<String, Vec<PathBuf>>,
}

impl ImportedPages {
    /// Reads the ids of every note of `library` and of its trash (once per sync).
    pub fn scan(library: &Library) -> Self {
        let listed = library
            .list()
            .notes
            .into_iter()
            .map(|entry| (entry.id, entry.path))
            .collect();
        let mut trashed: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for item in library.trash_items().unwrap_or_default() {
            if let Some(id) = item.note_id {
                trashed.entry(id).or_default().push(item.path);
            }
        }
        Self { listed, trashed }
    }

    /// The note `page` was already saved as: a note of the library with its id that holds
    /// exactly its bytes (`original.bin`), so the page is safe in the library.
    pub fn find(&self, library: &Library, page: &DownloadedPage) -> Option<PathBuf> {
        let path = self.listed.get(&page_note_id(page))?;
        let loaded = library.read(path).ok()?;
        (loaded.file.original.as_deref() == Some(page.bytes.as_slice())).then_some(loaded.path)
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

/// The note of one downloaded page: black ballpoint strokes on a `page` of the tablet's size,
/// turned for a left-handed user ([`orient`]), `created` at the page's timestamp (or `now_ms`),
/// `original.bin` = the bytes received (untouched).
pub fn note_from_page(
    page: &DownloadedPage,
    size: PageSize,
    hand: Hand,
    id: String,
    layer_name: &str,
    now_ms: i64,
) -> Result<NoteFile, PageError> {
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
                    let (x, y) = orient(p.x, p.y, size.width, size.height, hand);
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
        page: size,
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

/// Called once a synced note is safely in the library (automatic export).
pub type AfterSave<'a> = Box<dyn FnMut(&SavedNote, &Note) + 'a>;

/// Makes the PNG thumbnail (`thumb.png`) of a synced note, `None` if it cannot be made.
pub type ThumbnailMaker<'a> = Box<dyn Fn(&Note) -> Option<Vec<u8>> + 'a>;

/// Test hook run on the file just written, before it is read back.
#[cfg(test)]
type TamperHook<'a> = Box<dyn Fn(&std::path::Path) + 'a>;

/// Stores synced pages as new notes of a library binder.
pub struct LibrarySink<'a> {
    library: &'a Library,
    binder: PathBuf,
    size: PageSize,
    hand: Hand,
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
            hand: Hand::Right,
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

    /// Turns the points for a left-handed user ([`orient`]).
    pub fn with_hand(mut self, hand: Hand) -> Self {
        self.hand = hand;
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

    /// Saves `page` as a new note (id: [`page_note_id`]) and checks it reads back identical.
    /// An `Err` keeps the page on the tablet (the session does not delete it) and puts the copy
    /// just written aside ([`discard`](Self::discard)) so that it never counts as imported: the
    /// next sync writes the page again. Fits `Session::sync`'s sink:
    /// `&mut |page, _, _| sink.store(page).map(|_| ())`.
    pub fn store(&mut self, page: DownloadedPage) -> Result<SavedNote, String> {
        let id = page_note_id(&page);
        let mut file = note_from_page(
            &page,
            self.size,
            self.hand,
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
        Ok(saved)
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
    use ardoise_core::note::A4_LANDSCAPE;
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
        }
    }

    #[test]
    fn a_page_becomes_a_tablet_note_with_unwrapped_times() {
        // The second stroke starts after the clock wrapped (327 000 ms → 1 000 ms).
        let page = page(&[raw_stroke(327_000, 1000, 4), raw_stroke(1_000, 2000, 3)]);
        let file =
            note_from_page(&page, A4_LANDSCAPE, Hand::Right, "id".into(), "Calque 1", 5).unwrap();
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
                Hand::Right,
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
            let mut file =
                note_from_page(&other, A4_LANDSCAPE, Hand::Right, id.clone(), "L", 1).unwrap();
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
    fn left_hand_turns_points_half_a_turn() {
        assert_eq!(
            orient(1000, 5000, 29_700, 21_600, Hand::Right),
            (1000, 5000)
        );
        assert_eq!(
            orient(1000, 5000, 29_700, 21_600, Hand::Left),
            (28_700, 16_600)
        );
        assert_eq!(orient(0, 0, 29_700, 21_600, Hand::Left), (29_700, 21_600));
        // Outside the area: clamped, never wrapped.
        assert_eq!(orient(30_000, 21_600, 29_700, 21_600, Hand::Left), (0, 0));
        let pen = LiveRecord::Pen(RawPoint {
            x: 2500,
            y: 500,
            pressure: 700,
        });
        assert_eq!(
            orient_record(pen, 21_600, 14_800, Hand::Left),
            LiveRecord::Pen(RawPoint {
                x: 19_100,
                y: 14_300,
                pressure: 700
            })
        );
        assert_eq!(
            orient_record(LiveRecord::ProximityOut, 21_600, 14_800, Hand::Left),
            LiveRecord::ProximityOut
        );

        // A synced page of a left-handed user.
        let page = page(&[raw_stroke(0, 1000, 2)]);
        let file =
            note_from_page(&page, A4_LANDSCAPE, Hand::Left, "id".into(), "Calque 1", 5).unwrap();
        let first = &file.note.strokes[0].points;
        assert_eq!((first[0].x, first[0].y), (28_700.0, 16_600.0));
        assert_eq!((first[1].x, first[1].y), (28_690.0, 16_600.0));
        // The tablet's bytes are kept untouched.
        assert_eq!(file.original.as_deref(), Some(page.bytes.as_slice()));
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
        };
        assert_eq!(
            page_note_id(&small(3)),
            "eb8a2121-4b58-8f30-9181-c47e03a5133c"
        );
        assert_eq!(page_note_id(&small(3)), page_note_id(&small(99)));
        let random = new_note_id().unwrap();
        assert_eq!(&random[14..15], "4", "{random}");
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
