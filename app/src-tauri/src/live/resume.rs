//! Resuming a sheet in live mode (SPEC §4.2, `docs/questions.md` Q9;
//! docs/design/tablet-live-classroom.md, "M12"): what a session keeps of the note it starts from,
//! how the tablet's pen lands on a sheet that was not made for it, and where the new ink and the
//! new narration go in the note's time.
//!
//! No clock and no file here: the hub reads the note, the session writes on it.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use ardoise_core::format::NoteFile;
use ardoise_core::library::LoadedNote;
use ardoise_core::note::{Note, PageSize};
use ardoise_core::settings::Orientation;
use ardoise_device::pages::{BLACKBOARD_PAGE, turn_point};
use ardoise_device::transport::TabletKind;
use ardoise_media::replay::{ReplayOptions, Timeline};
use ardoise_media::{Narration, NarrationHeader, narration_header};

use super::hub::file_fingerprint;
use super::paper::PageImage;
use super::session::SavedFile;

/// Note time left between what a resumed note already holds and what is written on it: the time
/// since the resume counts from there. Any small value will do (nothing is synchronised with
/// it); the replay shows it as a pause, which its dead-time removal shortens like any other.
pub const RESUME_GAP_MS: u32 = 1000;

/// The file a resumed sheet comes from, as it was read: every write of the page starts from it,
/// so that what live mode does not change stays in the file (`original.bin`, `original.ardoise`,
/// the background image, `text.json`, unknown fields and members). Its `note` is the version on
/// disk before the resume, which `original::keep_original` keeps.
///
/// In memory for the whole session, not read again: a note deleted meanwhile must still give the
/// new note its background image. Its pixels, to draw, are the page's ([`ResumeSeed::image`]).
pub struct ResumeBase {
    pub file: NoteFile,
}

impl ResumeBase {
    pub fn new(file: NoteFile) -> ResumeBase {
        ResumeBase { file }
    }
}

impl fmt::Debug for ResumeBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResumeBase")
            .field("id", &self.file.note.id)
            .finish_non_exhaustive()
    }
}

/// What a session starts from when it resumes a note.
#[derive(Debug, Clone)]
pub struct ResumeSeed {
    pub base: Arc<ResumeBase>,
    /// Where the note is: its path, stamp and fingerprint as read, so that the first write checks
    /// them like any other.
    pub saved: SavedFile,
    /// Note time at which the time since the resume starts ([`time_base`]).
    pub time_base: u32,
    /// The pixels of the note's background image, the page's to draw and to write (M17): `None`
    /// without an image, or when its member is missing or its header cannot be read (the member
    /// then stays in the file as it is, and nothing is drawn).
    pub image: Option<Arc<PageImage>>,
}

impl ResumeSeed {
    /// Before the hub's lock is taken: hashing the image's bytes and reading its header take a
    /// moment.
    pub fn of(loaded: LoadedNote) -> ResumeSeed {
        let saved = SavedFile {
            content: file_fingerprint(&loaded.file),
            path: loaded.path,
            stamp: loaded.stamp,
        };
        let time_base = time_base(&loaded.file.note, loaded.file.audio.as_deref());
        let image = loaded
            .file
            .note
            .background_image
            .as_ref()
            .and(loaded.file.background.as_ref())
            .and_then(PageImage::of_member)
            .map(Arc::new);
        ResumeSeed {
            base: Arc::new(ResumeBase::new(loaded.file)),
            saved,
            time_base,
            image,
        }
    }
}

/// How the tablet is held to write on a sheet of size `sheet` when it is set to `setting`: the
/// setting itself when the sheet is held the same way round (a square sheet always is); else the
/// position a quarter turn away on the same side, so that the choice made among the four
/// positions still counts:
///
/// | Setting | Sheet the other way round |
/// |---|---|
/// | `landscape` (button at the bottom, right hand) | `portraitLeft` (button on the left) |
/// | `landscapeFlipped` (button at the top, left hand) | `portraitRight` (button on the right) |
/// | `portraitLeft` | `landscape` |
/// | `portraitRight` | `landscapeFlipped` |
///
/// (The pairs keep the button away from the writing hand: `docs/questions.md` Q9 leaves the
/// choice open, and this table is the only place to change it.)
pub fn hold_for(sheet: PageSize, setting: Orientation) -> Orientation {
    let other_way = if setting.is_portrait() {
        sheet.width > sheet.height
    } else {
        sheet.height > sheet.width
    };
    if !other_way {
        return setting;
    }
    match setting {
        Orientation::Landscape => Orientation::PortraitLeft,
        Orientation::LandscapeFlipped => Orientation::PortraitRight,
        Orientation::PortraitLeft => Orientation::Landscape,
        Orientation::PortraitRight => Orientation::LandscapeFlipped,
    }
}

/// Whether a tablet of `kind` may write on a resumed sheet of size `sheet`, without its ink being
/// shrunk, turned or lost: the blackboard only on its own page ([`BLACKBOARD_PAGE`]), which it
/// covers exactly at its own scale (it is never turned, nor are its records); the Slate on every
/// other sheet, as before the blackboard (turned to how the sheet is held and fitted in it), never
/// on the blackboard's page (its ink would be shrunk to the narrower portrait page). Any other pair
/// ends the resumed run (`sheetOtherTablet`), nothing written on the sheet.
pub fn tablet_fits(kind: TabletKind, sheet: PageSize) -> bool {
    let board_page = sheet == BLACKBOARD_PAGE;
    match kind {
        TabletKind::Blackboard => board_page,
        TabletKind::Slate => !board_page,
    }
}

/// How the tablet is held to write on a page held fixed (a resumed sheet, a worksheet): the Slate
/// follows the sheet ([`hold_for`]); the blackboard cannot (its records are never turned): it is
/// held as it is, `setting` being its own way.
pub fn held_for(sheet: PageSize, setting: Orientation, kind: TabletKind) -> Orientation {
    match kind {
        TabletKind::Slate => hold_for(sheet, setting),
        TabletKind::Blackboard => setting,
    }
}

/// From the pen's points (as the tablet service hands them over) to the page being written: a
/// quarter-turn correction when the sheet is held otherwise than the service orients the points,
/// then a uniform scale and an offset that fit the tablet's whole area in the sheet, centred,
/// without distortion. On a sheet of the tablet's size held the same way there is nothing to do:
/// [`map`](Self::map) gives the point as it came, exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenMapping {
    /// The area the points arrive in: the tablet as the service orients them.
    width: f32,
    height: f32,
    /// Clockwise quarter turns from that area to the way the sheet is held.
    turns: u8,
    scale: f64,
    dx: f64,
    dy: f64,
}

impl PenMapping {
    /// Nothing to do: the page has the size and the orientation of the tablet.
    pub fn direct() -> PenMapping {
        PenMapping {
            width: 0.0,
            height: 0.0,
            turns: 0,
            scale: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }

    /// For a sheet of size `sheet`, when the points arrive in `area` and `turns` clockwise quarter
    /// turns bring them to the way the sheet is held.
    pub fn for_sheet(area: PageSize, turns: u8, sheet: PageSize) -> PenMapping {
        let (held_w, held_h) = if turns % 2 == 1 {
            (f64::from(area.height), f64::from(area.width))
        } else {
            (f64::from(area.width), f64::from(area.height))
        };
        if held_w <= 0.0 || held_h <= 0.0 {
            return PenMapping::direct();
        }
        let (sheet_w, sheet_h) = (f64::from(sheet.width), f64::from(sheet.height));
        // The side that fills the sheet has no offset at all, not a rounding error.
        let (scale, dx, dy) = if sheet_w * held_h <= sheet_h * held_w {
            let scale = sheet_w / held_w;
            (scale, 0.0, (sheet_h - held_h * scale) / 2.0)
        } else {
            let scale = sheet_h / held_h;
            (scale, (sheet_w - held_w * scale) / 2.0, 0.0)
        };
        PenMapping {
            width: area.width as f32,
            height: area.height as f32,
            turns: turns % 4,
            scale,
            dx,
            dy,
        }
    }

    /// A point of the sheet is the point as it came.
    pub fn is_direct(&self) -> bool {
        self.turns == 0 && self.scale == 1.0 && self.dx == 0.0 && self.dy == 0.0
    }

    /// The page position of a pen point. Direct: the point as it came, exactly (a `u16` is exact
    /// in an `f32`). Otherwise points beyond the tablet's area are clamped to it first, as the
    /// service's own orientation does, so that a turned point never lands before the page.
    pub fn map(&self, x: u16, y: u16) -> (f32, f32) {
        let (x, y) = (f32::from(x), f32::from(y));
        if self.is_direct() {
            return (x, y);
        }
        let (x, y) = turn_point(
            x.min(self.width),
            y.min(self.height),
            self.width,
            self.height,
            self.turns,
        );
        (
            (f64::from(x) * self.scale + self.dx) as f32,
            (f64::from(y) * self.scale + self.dy) as f32,
        )
    }
}

/// Note time at which the writing resumes on `note`, whose narration is `audio`: after its last
/// stroke and after the end of its narration (a new narration placed there replaces no sound),
/// plus [`RESUME_GAP_MS`]. Zero on a note with neither. A narration that cannot be read counts for
/// nothing here: it stays in the file as it is.
pub fn time_base(note: &Note, audio: Option<&[u8]>) -> u32 {
    let last_stroke = last_point_time(note);
    let narration_end = audio
        .and_then(|bytes| narration_header(bytes).ok())
        .map(|header| narration_end_in_note_time(note, &header));
    match (last_stroke, narration_end) {
        (None, None) => 0,
        (stroke, narration) => stroke
            .unwrap_or(0)
            .max(narration.unwrap_or(0))
            .saturating_add(RESUME_GAP_MS),
    }
}

/// Time of the note's last point, hidden layers included.
fn last_point_time(note: &Note) -> Option<u32> {
    note.strokes
        .iter()
        .flat_map(|stroke| &stroke.points)
        .map(|point| point.t)
        .max()
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// A time in whole milliseconds, within what a point's time can hold.
fn clamped_ms(ms: f64) -> u32 {
    if ms.is_nan() {
        0
    } else {
        ms.round().clamp(0.0, f64::from(u32::MAX)) as u32
    }
}

/// The note time at which a narration ends, 0 when that is within what the note's ink shows. A
/// narration recorded during a replay is in the time of that replay: the part beyond the replay's
/// end is converted at its speed (the part within it plays over ink the note already has).
fn narration_end_in_note_time(note: &Note, header: &NarrationHeader) -> u32 {
    let length = i64::try_from(header.duration.as_millis()).unwrap_or(i64::MAX);
    let end_ms = header.start_ms.saturating_add(length);
    if header.options == ReplayOptions::default() {
        // Speed 1 without trimming: replay time and note time are the same.
        return clamped_ms(end_ms as f64);
    }
    let Ok(timeline) = Timeline::new(note, &header.options) else {
        return clamped_ms(end_ms as f64);
    };
    let beyond = end_ms as f64 - millis(timeline.duration());
    if beyond <= 0.0 {
        return 0;
    }
    clamped_ms(timeline.note_end() + beyond * header.options.speed)
}

/// `later` (recorded in live mode: note time) placed for merging with `earlier`, the narration
/// the note already has, whose time base it joins ([`Narration::merge`] keeps the first one's). A
/// narration recorded during a replay at another speed or with its pauses trimmed is in the time
/// of that replay, not in note time: the new sound starts where the replay reaches its note time,
/// and never over the older sound. It is aligned at its start only (a replay at another speed
/// draws the new ink at that speed; a pause trimmed inside the new ink is not trimmed from the
/// sound).
pub fn placed_for_merge(later: Narration, earlier: &Narration, note: &Note) -> Narration {
    if earlier.options == ReplayOptions::default() {
        return later;
    }
    let Ok(timeline) = Timeline::new(note, &earlier.options) else {
        return later;
    };
    let note_ms = later.start_ms as f64;
    let beyond = note_ms - timeline.note_end();
    let replay_ms = if beyond > 0.0 {
        millis(timeline.duration()) + beyond / earlier.options.speed
    } else {
        millis(timeline.to_replay(note_ms))
    };
    Narration {
        start_ms: (replay_ms.round() as i64).max(earlier.end_ms()),
        ..later
    }
}

/// `earlier` and `later` on one track, without losing `earlier`: [`Narration::merge`] gives
/// `later` alone when they are too far apart for one track, so `later` is then moved right after
/// `earlier` (its timing is off, its sound is kept). Two narrations too long for one track together
/// (the file format holds 4 hours) cannot be merged: `later` comes back, for the caller to keep it
/// apart, and `earlier` is the caller's still (it has the page's encoded narration).
pub fn merged_keeping_both(earlier: Narration, later: Narration) -> Result<Narration, Narration> {
    if earlier.fits_with(&later) {
        return Ok(earlier.merge(later));
    }
    let moved = Narration {
        start_ms: earlier.end_ms(),
        ..later
    };
    if earlier.fits_with(&moved) {
        Ok(earlier.merge(moved))
    } else {
        Err(moved)
    }
}

#[cfg(test)]
mod tests;
