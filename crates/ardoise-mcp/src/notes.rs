//! Naming notes and measuring them for the assistant: a note is given by its `id` or by its path
//! relative to the library; sizes and positions are millimetres; dates are ISO 8601.

use std::path::Path;
use std::time::UNIX_EPOCH;

use ardoise_core::library::{FileStamp, Library, LoadedNote, NoteEntry};
use ardoise_core::note::{EquationBody, Note, PageObject, PageSize, TextBody};
use ardoise_core::objects::EQUATION_UNITS_PER_EM;
use ardoise_hwr::Bounds;
use ardoise_ink::Rgba;
use chrono::{
    DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, TimeZone,
};
use serde::Serialize;

use crate::error::ToolError;

/// Millimetres in a device unit (10 µm).
const MM_PER_UNIT: f64 = 0.01;

/// `units` (device units, 10 µm) in millimetres, rounded to 0.1 mm.
pub fn mm(units: f32) -> f64 {
    (f64::from(units) * MM_PER_UNIT * 10.0).round() / 10.0
}

/// A rectangle in millimetres from the top-left corner of the page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RectMm {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<Bounds> for RectMm {
    fn from(bounds: Bounds) -> Self {
        RectMm {
            x: mm(bounds.x),
            y: mm(bounds.y),
            width: mm(bounds.width),
            height: mm(bounds.height),
        }
    }
}

/// The page of a note.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageMm {
    pub width_mm: f64,
    pub height_mm: f64,
    /// `landscape` (at least as wide as tall) or `portrait`.
    pub orientation: &'static str,
}

impl From<PageSize> for PageMm {
    fn from(page: PageSize) -> Self {
        PageMm {
            width_mm: mm(page.width as f32),
            height_mm: mm(page.height as f32),
            orientation: if page.width >= page.height {
                "landscape"
            } else {
                "portrait"
            },
        }
    }
}

/// `#rrggbb`, or `#rrggbbaa` when the colour is not opaque.
pub fn hex(color: Rgba) -> String {
    if color.a == 255 {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color.r, color.g, color.b, color.a
        )
    }
}

/// Unix milliseconds as an ISO 8601 date and time in this PC's time zone
/// (`2026-10-12T14:03:22+02:00`).
pub fn iso(unix_ms: i64) -> String {
    DateTime::from_timestamp_millis(unix_ms)
        .map(|utc| {
            utc.with_timezone(&Local)
                .to_rfc3339_opts(SecondsFormat::Secs, false)
        })
        .unwrap_or_default()
}

/// Unix milliseconds of an ISO date (`2026-10-12`, local midnight; with `end_of_day`, the start
/// of the next day, so that a `to` date includes its whole day) or date and time
/// (`2026-10-12T14:00:00`, with or without an offset).
pub fn parse_date(text: &str, end_of_day: bool) -> Result<i64, ToolError> {
    let bad = || {
        ToolError::invalid(format!(
            "`{text}` is not an ISO date (2026-10-12) or date and time (2026-10-12T14:00:00)"
        ))
    };
    let text = text.trim();
    let local_ms = |naive: NaiveDateTime| match Local.from_local_datetime(&naive) {
        LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => Ok(at.timestamp_millis()),
        LocalResult::None => Ok(naive.and_utc().timestamp_millis()),
    };
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let date = if end_of_day {
            date.succ_opt().ok_or_else(bad)?
        } else {
            date
        };
        return local_ms(date.and_time(NaiveTime::MIN));
    }
    if let Ok(at) = DateTime::parse_from_rfc3339(text) {
        return Ok(at.timestamp_millis());
    }
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .map_err(|_| bad())
        .and_then(local_ms)
}

/// `path` relative to the library root, `/`-separated.
pub fn relative_path(library: &Library, path: &Path) -> String {
    let relative = path.strip_prefix(library.root()).unwrap_or(path);
    relative
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A binder as `a/b` (empty for the root).
pub fn binder_text(binder: &Path) -> String {
    binder
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The note named `reference`: its `id` (case does not matter), or its path relative to the
/// library (`/` or `\`, case does not matter, `.ardoise` optional), or its absolute path. When
/// several files share an id, a sync client's conflict copies give way to the original; but two
/// files that both look like originals (a copy made in the Explorer) are refused: the error
/// lists their paths, and the path of one of them names it.
pub fn find_note(library: &Library, reference: &str) -> Result<NoteEntry, ToolError> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(ToolError::invalid(
            "`note` is empty: give a note's id or its path in the library",
        ));
    }
    let listing = library.list();
    let mut same_id: Vec<&NoteEntry> = listing
        .notes
        .iter()
        .filter(|note| note.id.eq_ignore_ascii_case(reference))
        .collect();
    // Conflict copies only count when there is no original.
    if same_id.iter().any(|note| note.conflict.is_none()) {
        same_id.retain(|note| note.conflict.is_none());
    }
    match same_id.as_slice() {
        [] => {}
        [note] => return Ok((*note).clone()),
        several => {
            let mut paths: Vec<String> = several
                .iter()
                .map(|note| relative_path(library, &note.path))
                .collect();
            paths.sort();
            return Err(ToolError::invalid(format!(
                "several notes have the id `{reference}` ({}): give the path of the one you mean",
                paths.join(", ")
            )));
        }
    }

    // An absolute path inside the library is turned into a relative one; comparisons ignore case
    // and the kind of slash, as Windows does.
    let given = reference.replace('\\', "/").to_lowercase();
    let root = library
        .root()
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    let wanted = match given.strip_prefix(root.trim_end_matches('/')) {
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/'),
        _ => given.trim_start_matches('/'),
    };
    let with_extension = format!("{wanted}.{}", ardoise_core::library::NOTE_EXTENSION);
    listing
        .notes
        .iter()
        .find(|note| {
            let path = relative_path(library, &note.path).to_lowercase();
            path == wanted || path == with_extension
        })
        .cloned()
        .ok_or_else(|| {
            ToolError::new(
                "noteNotFound",
                format!("no note has the id or the path `{reference}` (see list_notes)"),
            )
        })
}

/// The name of a version of a note's file, from its stamp: two versions never share one. The
/// assistant reads it (`get_note`, `preview_revision`) and gives it back with its orders.
pub fn version_of(stamp: &FileStamp) -> String {
    let (sign, since) = match stamp.modified.duration_since(UNIX_EPOCH) {
        Ok(since) => ("", since),
        Err(before) => ("-", before.duration()),
    };
    format!(
        "{sign}{}.{:09}-{}",
        since.as_secs(),
        since.subsec_nanos(),
        stamp.len
    )
}

/// Reads the whole note of `entry`.
pub fn read_note(library: &Library, entry: &NoteEntry) -> Result<LoadedNote, ToolError> {
    Ok(library.read(&entry.path)?)
}

/// A text frame as the assistant reads it: its text and style, and its box on the page in
/// millimetres (turned and scaled like the page shows it). Typed on the keyboard, so exact.
pub fn text_frame_json(note: &Note, object: &PageObject, body: &TextBody) -> serde_json::Value {
    let [left, top, right, bottom] = object.page_bounds();
    serde_json::json!({
        "id": object.id,
        "layer": object.layer,
        "visible": note.layer_visible(object.layer),
        "text": body.text,
        "font": serde_json::to_value(body.font).unwrap_or_default(),
        "bold": body.bold,
        "size_mm": mm(body.size * object.scale),
        "color": hex(body.color),
        "bounds_mm": RectMm {
            x: mm(left),
            y: mm(top),
            width: mm(right - left),
            height: mm(bottom - top),
        },
        "quarter_turns": object.quarter_turns % 4,
        "t_ms": object.t,
    })
}

/// An equation as the assistant reads it: its LaTeX source (what was typed, exact; chemistry in
/// `\ce{…}`), its colour, the size of its type (its em) and its box on the page in millimetres.
pub fn equation_json(note: &Note, object: &PageObject, body: &EquationBody) -> serde_json::Value {
    let [left, top, right, bottom] = object.page_bounds();
    serde_json::json!({
        "id": object.id,
        "layer": object.layer,
        "visible": note.layer_visible(object.layer),
        "latex": body.latex,
        "size_mm": mm(object.scale * EQUATION_UNITS_PER_EM),
        "color": hex(body.color),
        "bounds_mm": RectMm {
            x: mm(left),
            y: mm(top),
            width: mm(right - left),
            height: mm(bottom - top),
        },
        "quarter_turns": object.quarter_turns % 4,
        "t_ms": object.t,
    })
}

/// The page objects this version cannot read (a kind of a later version): how many, and their
/// kinds. `None` when there is none.
pub fn other_objects_json(note: &Note) -> Option<serde_json::Value> {
    let mut kinds: Vec<&str> = note
        .objects
        .iter()
        .filter(|object| !object.body.is_drawn())
        .map(|object| object.body.kind())
        .collect();
    if kinds.is_empty() {
        return None;
    }
    let count = kinds.len();
    kinds.sort_unstable();
    kinds.dedup();
    Some(serde_json::json!({
        "count": count,
        "kinds": kinds,
        "note": "objects of kinds this version of Ardoise does not know: they are kept as they are and drawn nowhere here, so `render_note` does not show them",
    }))
}

/// A note in a listing, or the identity of a note in another result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteSummary {
    pub id: String,
    /// Relative to the library, `/`-separated: also a valid `note` argument.
    pub path: String,
    pub binder: String,
    pub title: String,
    /// ISO 8601, this PC's time zone.
    pub created: String,
    pub modified: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    /// Absent when the file could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<PageMm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_count: Option<usize>,
    pub has_background_image: bool,
    /// Set when the file is a sync client's conflict copy: the name of the note it copies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_copy_of: Option<String>,
}

impl NoteSummary {
    /// The summary of `entry`, without reading the strokes (no page size, no stroke count).
    pub fn of_entry(library: &Library, entry: &NoteEntry) -> NoteSummary {
        NoteSummary {
            id: entry.id.clone(),
            path: relative_path(library, &entry.path),
            binder: binder_text(&entry.binder),
            title: entry.title.clone(),
            created: iso(entry.created),
            modified: iso(entry.modified),
            tags: entry.tags.clone(),
            favorite: entry.favorite,
            page: None,
            stroke_count: None,
            has_background_image: false,
            conflict_copy_of: entry.conflict.as_ref().map(|c| c.original_name.clone()),
        }
    }

    /// The summary of `entry`, with what reading the note gives (page, stroke count,
    /// background image); the listing's fields when the file cannot be read.
    pub fn read(library: &Library, entry: &NoteEntry) -> NoteSummary {
        let mut summary = NoteSummary::of_entry(library, entry);
        if let Ok(loaded) = library.read(&entry.path) {
            summary.add_note(&loaded.file.note);
        }
        summary
    }

    /// Adds what `note` says of itself.
    pub fn add_note(&mut self, note: &Note) {
        self.page = Some(note.page.into());
        self.stroke_count = Some(note.strokes.len());
        self.has_background_image = note.background_image.is_some();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_millimetres_to_a_tenth() {
        assert_eq!(mm(29_700.0), 297.0);
        assert_eq!(mm(21_600.0), 216.0);
        assert_eq!(mm(1_234.0), 12.3);
        assert_eq!(mm(-56.0), -0.6);
    }

    #[test]
    fn a_version_names_the_stamp_of_the_file() {
        use std::time::Duration;
        let stamp = |seconds: u64, nanos: u32, len: u64| FileStamp {
            modified: UNIX_EPOCH + Duration::new(seconds, nanos),
            len,
        };
        let version = version_of(&stamp(1_792_843_200, 123_456_700, 4096));
        assert_eq!(version, "1792843200.123456700-4096");
        assert_eq!(
            version,
            version_of(&stamp(1_792_843_200, 123_456_700, 4096))
        );
        // A modification time or a length of its own: another version.
        assert_ne!(
            version,
            version_of(&stamp(1_792_843_200, 123_456_800, 4096))
        );
        assert_ne!(
            version,
            version_of(&stamp(1_792_843_201, 123_456_700, 4096))
        );
        assert_ne!(
            version,
            version_of(&stamp(1_792_843_200, 123_456_700, 4097))
        );
        // A file dated before 1970 still has a name.
        let old = FileStamp {
            modified: UNIX_EPOCH - Duration::from_secs(5),
            len: 1,
        };
        assert_eq!(version_of(&old), "-5.000000000-1");
    }

    #[test]
    fn colours_are_hex() {
        assert_eq!(hex(Rgba::rgb(255, 0, 16)), "#ff0010");
        assert_eq!(
            hex(Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 128
            }),
            "#00000080"
        );
    }

    #[test]
    fn dates_are_local_and_the_end_date_includes_its_day() {
        let start = parse_date("2026-10-12", false).unwrap();
        let end = parse_date("2026-10-12", true).unwrap();
        // A day is 24 hours, give or take the hour of a time change.
        assert!((23 * 3_600_000..=25 * 3_600_000).contains(&(end - start)));
        assert_eq!(
            parse_date("2026-10-12T12:00:00+00:00", false).unwrap(),
            DateTime::parse_from_rfc3339("2026-10-12T12:00:00+00:00")
                .unwrap()
                .timestamp_millis()
        );
        assert!(parse_date("12/10/2026", false).is_err());
        assert!(
            iso(start).starts_with("2026-10-12T00:00:00") || iso(start).starts_with("2026-10-1")
        );
    }
}
