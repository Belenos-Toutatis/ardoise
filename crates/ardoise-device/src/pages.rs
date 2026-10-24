//! Synced pages become notes of the library (SPEC §4.1). Every synchronisation goes through
//! [`LibrarySink::store`]: it writes the note, reads it back to verify it, and only then lets the
//! session delete the page from the tablet; then the after-save hook runs (automatic export).

use std::path::PathBuf;

use ardoise_core::format::{self, NoteFile};
use ardoise_core::library::{Library, SavedNote};
use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_proto::SLATE_MAX_PRESSURE;
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
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex = |range: std::ops::Range<usize>| -> String {
        b[range].iter().map(|byte| format!("{byte:02x}")).collect()
    };
    Ok(format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    ))
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
/// `created` at the page's timestamp (or `now_ms`), `original.bin` = the bytes received.
pub fn note_from_page(
    page: &DownloadedPage,
    size: PageSize,
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
                .map(|(i, p)| Point {
                    x: f32::from(p.x),
                    y: f32::from(p.y),
                    pressure: (f32::from(p.pressure) / max_pressure).clamp(0.0, 1.0),
                    t: u32::try_from(start + i as u64 * SAMPLE_MS).unwrap_or(u32::MAX),
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

/// Stores synced pages as new notes of a library binder.
pub struct LibrarySink<'a> {
    library: &'a Library,
    binder: PathBuf,
    size: PageSize,
    layer_name: String,
    now_ms: Box<dyn Fn() -> i64 + 'a>,
    after_save: Option<AfterSave<'a>>,
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
            layer_name: layer_name.into(),
            now_ms: Box::new(now_ms),
            after_save: None,
            saved: Vec::new(),
        }
    }

    /// Runs `hook` after each note is saved and verified.
    pub fn with_after_save(mut self, hook: impl FnMut(&SavedNote, &Note) + 'a) -> Self {
        self.after_save = Some(Box::new(hook));
        self
    }

    /// Saves `page` as a new note and checks it reads back identical. An `Err` keeps the page
    /// on the tablet (the session does not delete it). Fits `Session::sync`'s sink:
    /// `&mut |page, _, _| sink.store(page).map(|_| ())`.
    pub fn store(&mut self, page: DownloadedPage) -> Result<SavedNote, String> {
        let id = new_note_id().map_err(|e| e.to_string())?;
        let file = note_from_page(&page, self.size, id, &self.layer_name, (self.now_ms)())
            .map_err(|e| e.to_string())?;
        let saved = self
            .library
            .create(&self.binder, &file)
            .map_err(|e| e.to_string())?;
        // What the file must hold: the note as the format stores it (pressures and positions
        // are quantised), and the tablet's bytes untouched.
        let expected = format::to_bytes(&file)
            .and_then(|bytes| format::read_from_bytes(&bytes))
            .map_err(|e| e.to_string())?;
        let back = self.library.read(&saved.path).map_err(|e| e.to_string())?;
        if back.file.note != expected.note || back.file.original != file.original {
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
        let file = note_from_page(&page, A4_LANDSCAPE, "id".into(), "Calque 1", 5).unwrap();
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
}
