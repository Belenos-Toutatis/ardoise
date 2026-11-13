//! Tests of a session that resumes a note: the session alone (the pen on the sheet, its time, its
//! layers, its paper), and the hub with a real library (what is written, kept, refused).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ardoise_core::format::{BackgroundFile, Member, NoteFile, RevisionEntry, RevisionInfo};
use ardoise_core::library::{FileStamp, Library, LoadedNote};
use ardoise_core::note::{
    A4_LANDSCAPE, A4_PORTRAIT, Background, BackgroundImage, ImageFit, ImageRect, Layer, Note,
    Origin, PageSize, Paper, Template,
};
use ardoise_core::settings::{LiveButton, Orientation, Settings};
use ardoise_device::pages::orient;
use ardoise_device::service::{LiveInput, LiveSink};
use ardoise_device::transport::TabletKind;
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_media::microphone::simulated_narration;
use ardoise_media::replay::ReplayOptions;
use ardoise_media::{Narration, NarrationBase, decode_narration, encode_narration};
use ardoise_proto::live::LiveRecord;
use ardoise_render::encode_png;
use serde_json::json;

use super::super::edits::LiveEditError;
use super::super::edits::tests::{delete, recolour, transform};
use super::super::hub::{
    HubConfig, HubSink, LiveError, LiveHub, NarrationApart, Start, StartOptions, file_fingerprint,
};
use super::super::model::{
    ForkReason, LiveCommandKind, LiveEvent, LiveMark, LiveSessionState, LiveStyle, LiveViewState,
    MarkPoint,
};
use super::super::paper::LivePageImageOp;
use super::super::session::LiveSession;
use super::super::tests::{
    A5, Window, audio_of, break_folder, change, config, fixed, keys, note_narration, notes_in,
    options, pen, send, stream_simulated_start, stroke, stroke_count, text_frame, wait_until,
};
use super::{
    PenMapping, RESUME_GAP_MS, ResumeSeed, held_for, hold_for, merged_keeping_both,
    placed_for_merge, tablet_fits, time_base,
};
use crate::background::tests::halves;
use crate::background::{Staged, Staging, key_of};
use crate::drafts::{DRAFT_VERSION, DraftRecord, restore_copy_in};
use crate::library::tests::library;
use crate::library::{blank_note, now_ms, path_text};
use crate::narration::{PageLog, give_to_hub};
use crate::original::{NoteOriginal, TabletContext, original_of};
use crate::share::feed::batch_json;
use crate::share::live_paper::paper_key;

/// Note written a long time ago (2 months before the resume of the tests below).
const CREATED: i64 = 1_787_000_000_000;
/// When the session of the session-level tests resumes the note, wall clock.
const RESUMED_AT: i64 = CREATED + 60 * 24 * 3600 * 1000;

fn ink(x: f32, y: f32, times: &[u32], layer: u32) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer,
        points: times
            .iter()
            .enumerate()
            .map(|(i, &t)| Point {
                x: x + i as f32 * 10.0,
                y,
                pressure: 0.5,
                t,
            })
            .collect(),
    }
}

/// A note of `page` with two strokes, the last point at 5 000 ms.
fn sheet(page: PageSize) -> Note {
    let mut note = blank_note("Cours", "Calque 1", CREATED);
    note.page = page;
    note.strokes = vec![
        ink(100.0, 100.0, &[0, 5], 0),
        ink(200.0, 100.0, &[4000, 5000], 0),
    ];
    note
}

fn layer(name: &str, visible: bool) -> Layer {
    Layer {
        name: name.to_owned(),
        visible,
    }
}

/// A file holding everything a note file can hold, on a sheet of `page`: a text frame too (shown at
/// 4.5 s, within the ink's time: the time base stays the strokes').
fn full_file(page: PageSize) -> NoteFile {
    let mut note = sheet(page);
    note.tags = vec!["4e".to_owned()];
    note.favorite = true;
    note.objects = vec![text_frame("consigne", 0, 4_500, 1_000.0, 500.0)];
    note.background = Background {
        paper: Paper::Cream,
        template: Template::Seyes,
        texture: true,
    };
    note.background_image = Some(BackgroundImage {
        name: "fiche.pdf".to_owned(),
        pdf_page: Some(2),
        rect: ImageRect::fit(page, (40, 20), ImageFit::Contain),
        quarter_turns: 1,
        opacity: 0.8,
    });
    let mut file = NoteFile::new(note);
    file.background = Some(BackgroundFile {
        extension: "png".to_owned(),
        bytes: encode_png(&halves(40, 20)).unwrap(),
    });
    file.audio = Some(encode_narration(&note_narration(0.5, 0)).unwrap());
    file.text = Some(b"{\"recognised\":\"bonjour\"}".to_vec());
    file.revision = Some(RevisionInfo {
        of: "source-note".to_owned(),
        of_title: "Brouillon".to_owned(),
        created: CREATED,
        summary: vec![RevisionEntry {
            at: CREATED,
            text: "Écriture rangée".to_owned(),
        }],
    });
    file.thumbnail = Some(encode_png(&halves(8, 4)).unwrap());
    file.unknown_fields
        .insert("fromTheFuture".to_owned(), json!({ "a": [1, 2] }));
    file.unknown_members.push(Member {
        name: "future/data.bin".to_owned(),
        bytes: vec![1, 2, 3, 4],
    });
    file
}

/// What a resume seeds from `file`, without a library.
fn seed_of(file: NoteFile) -> ResumeSeed {
    ResumeSeed::of(LoadedNote {
        path: PathBuf::from("Classe/note.ardoise"),
        file,
        stamp: FileStamp {
            modified: std::time::SystemTime::UNIX_EPOCH,
            len: 0,
        },
    })
}

/// A session resuming `note` at [`RESUMED_AT`], streaming from an A4 tablet set to `setting`.
fn resumed(note: Note, setting: Orientation) -> LiveSession {
    resumed_file(NoteFile::new(note), setting)
}

fn resumed_file(file: NoteFile, setting: Orientation) -> LiveSession {
    let size = ardoise_device::pages::oriented_size(A4_LANDSCAPE, setting);
    let mut session = LiveSession::new(
        1,
        size,
        LiveStyle::default(),
        LiveViewState::whole(size),
        true,
        RESUMED_AT,
    )
    .with_orientation(setting)
    .resuming(seed_of(file), RESUMED_AT);
    let mut out = Vec::new();
    session.set_state(LiveSessionState::Waiting, &mut out);
    session.started(A4_LANDSCAPE.width, A4_LANDSCAPE.height, setting, &mut out);
    assert_eq!(session.state(), LiveSessionState::Live);
    session
}

/// One point of pressure then the pen lifted, at `at` (Unix ms): returns the point stored.
fn write_at(session: &mut LiveSession, x: u16, y: u16, at: i64) -> Point {
    let mut out = Vec::new();
    session.records(&[pen(x, y, 500)], at as f64, &mut out);
    session.records(&[LiveRecord::ProximityOut], at as f64 + 5.0, &mut out);
    let page = &session.pages()[session.current()];
    page.note.strokes.last().unwrap().points[0]
}

// --- The pen on the sheet ------------------------------------------------------------------------

#[test]
fn on_a_sheet_of_the_tablets_size_the_points_stay_exactly_one_to_one() {
    let mapping = PenMapping::for_sheet(A4_LANDSCAPE, 0, A4_LANDSCAPE);
    assert!(mapping.is_direct());
    let mut session = resumed(sheet(A4_LANDSCAPE), Orientation::Landscape);
    for (x, y) in [
        (0, 0),
        (1, 1),
        (12_345, 6_789),
        (29_699, 21_599),
        (65_535, 65_535),
    ] {
        let point = write_at(&mut session, x, y, RESUMED_AT + 10_000);
        assert_eq!((point.x, point.y), (f32::from(x), f32::from(y)));
    }
    // The hovering pen too.
    let mut out = Vec::new();
    session.records(&[pen(777, 888, 0)], RESUMED_AT as f64 + 20_000.0, &mut out);
    assert!(out.contains(&LiveEvent::Hover { x: 777.0, y: 888.0 }));
}

#[test]
fn on_a_sheet_of_another_size_the_tablet_is_fitted_centred_without_distortion() {
    // A5, cropped wide, cropped tall, larger than the tablet (A3).
    let a3 = PageSize {
        width: 42_000,
        height: 29_700,
    };
    let wide = PageSize {
        width: 20_000,
        height: 3_000,
    };
    let tall = PageSize {
        width: 3_000,
        height: 20_000,
    };
    for page in [A5, wide, tall, a3] {
        let mapping = PenMapping::for_sheet(A4_LANDSCAPE, 0, page);
        let (x0, y0) = mapping.map(0, 0);
        let (x1, y1) = mapping.map(29_700, 21_600);
        let (w, h) = (page.width as f32, page.height as f32);
        // Within the page, touching it on one side at least, centred.
        assert!(
            x0 >= -0.01 && y0 >= -0.01 && x1 <= w + 0.01 && y1 <= h + 0.01,
            "{page:?}"
        );
        assert!(x0.abs() < 0.01 || y0.abs() < 0.01, "{page:?}");
        assert!(
            (x0 - (w - x1)).abs() < 0.01 && (y0 - (h - y1)).abs() < 0.01,
            "{page:?}"
        );
        // The same scale across and down: no distortion.
        let (across, down) = ((x1 - x0) / 29_700.0, (y1 - y0) / 21_600.0);
        assert!((across - down).abs() < 1e-5, "{page:?}: {across} {down}");
        // The centre of the tablet is the centre of the page.
        let (cx, cy) = mapping.map(14_850, 10_800);
        assert!(
            (cx - w / 2.0).abs() < 0.01 && (cy - h / 2.0).abs() < 0.01,
            "{page:?}"
        );
    }

    // On a real A5 sheet: scale 14 800 / 21 600, so 20 350 wide, 625 on each side.
    let mut session = resumed(sheet(A5), Orientation::Landscape);
    let point = write_at(&mut session, 0, 0, RESUMED_AT + 1_000);
    assert_eq!((point.x, point.y), (625.0, 0.0));
    let point = write_at(&mut session, 29_700, 21_600, RESUMED_AT + 2_000);
    assert_eq!((point.x, point.y), (20_975.0, 14_800.0));
    // The hover and the stroke in progress are sent in page units too.
    let mut out = Vec::new();
    session.records(&[pen(0, 21_600, 0)], RESUMED_AT as f64 + 3_000.0, &mut out);
    assert!(out.contains(&LiveEvent::Hover {
        x: 625.0,
        y: 14_800.0
    }));
    session.records(
        &[pen(14_850, 10_800, 300)],
        RESUMED_AT as f64 + 4_000.0,
        &mut out,
    );
    let sent = out.iter().find_map(|e| match e {
        LiveEvent::StrokeStart { stroke, .. } => Some(stroke.points[0]),
        _ => None,
    });
    let sent = sent.unwrap();
    assert_eq!((sent.x, sent.y), (10_800.0, 7_400.0));
    // A sheet of another size held the other way round: turned, then fitted. A5 upright on the A4
    // tablet in landscape: scale 14 800 / 21 600, 625 above and below; the tablet's top left is the
    // sheet's top right.
    let upright = PageSize {
        width: A5.height,
        height: A5.width,
    };
    let mut other = resumed(sheet(upright), Orientation::Landscape);
    let point = write_at(&mut other, 0, 0, RESUMED_AT + 1_000);
    assert_eq!((point.x, point.y), (14_800.0, 625.0));
    let point = write_at(&mut other, 29_700, 21_600, RESUMED_AT + 2_000);
    assert_eq!((point.x, point.y), (0.0, 20_975.0));

    // A tablet that turns out to be another one (known only when it streams): the fit follows.
    session.started(21_600, 14_800, Orientation::Landscape, &mut out);
    let point = write_at(&mut session, 21_600, 14_800, RESUMED_AT + 5_000);
    assert_eq!((point.x, point.y), (21_600.0, 14_800.0));
    assert_eq!(session.pages()[0].note.page, A5, "never resized");
}

#[test]
fn the_tablet_is_held_the_way_the_sheet_is_keeping_the_side_chosen() {
    use Orientation::{Landscape, LandscapeFlipped, PortraitLeft, PortraitRight};
    // The other way round: a quarter turn on the same side.
    assert_eq!(hold_for(A4_PORTRAIT, Landscape), PortraitLeft);
    assert_eq!(hold_for(A4_PORTRAIT, LandscapeFlipped), PortraitRight);
    assert_eq!(hold_for(A4_LANDSCAPE, PortraitLeft), Landscape);
    assert_eq!(hold_for(A4_LANDSCAPE, PortraitRight), LandscapeFlipped);
    // The same way round, or a square sheet: the setting.
    let square = PageSize {
        width: 10_000,
        height: 10_000,
    };
    for setting in Orientation::ALL {
        assert_eq!(hold_for(square, setting), setting);
    }
    assert_eq!(hold_for(A4_LANDSCAPE, Landscape), Landscape);
    assert_eq!(hold_for(A4_LANDSCAPE, LandscapeFlipped), LandscapeFlipped);
    assert_eq!(hold_for(A4_PORTRAIT, PortraitLeft), PortraitLeft);
    assert_eq!(hold_for(A4_PORTRAIT, PortraitRight), PortraitRight);
}

/// The points of the pen, however the service oriented them, land where the tablet held as the
/// sheet says would put them.
#[test]
fn a_sheet_held_the_other_way_round_is_written_as_if_the_tablet_were_held_so() {
    let (w, h) = (A4_LANDSCAPE.width, A4_LANDSCAPE.height);
    for (setting, sheet_size) in [
        (Orientation::Landscape, A4_PORTRAIT),
        (Orientation::LandscapeFlipped, A4_PORTRAIT),
        (Orientation::PortraitLeft, A4_LANDSCAPE),
        (Orientation::PortraitRight, A4_LANDSCAPE),
    ] {
        let mut session = resumed(sheet(sheet_size), setting);
        let held = hold_for(sheet_size, setting);
        assert_ne!(held, setting);
        let snapshot = session.snapshot();
        assert_eq!(
            snapshot.orientation, held,
            "the interface shows the way to hold it"
        );
        assert!(!snapshot.can_turn_page);
        assert_eq!(snapshot.page.page, sheet_size);
        // The raw tablet point (5 000, 3 000), as the service hands it over with the setting...
        let (x, y) = orient(5_000, 3_000, w, h, setting);
        let point = write_at(&mut session, x, y, RESUMED_AT + 1_000);
        // ...is written where the same point goes when the tablet is held as the sheet says.
        let (x, y) = orient(5_000, 3_000, w, h, held);
        assert_eq!(
            (point.x, point.y),
            (f32::from(x), f32::from(y)),
            "{setting:?}"
        );
    }
}

#[test]
fn started_and_a_switch_leave_a_resumed_sheet_alone() {
    let mut note = sheet(A4_PORTRAIT);
    let rect = ImageRect {
        x: 10.0,
        y: 20.0,
        width: 1000.0,
        height: 2000.0,
    };
    note.background_image = Some(BackgroundImage {
        name: "fiche.png".to_owned(),
        pdf_page: None,
        rect,
        quarter_turns: 0,
        opacity: 1.0,
    });
    let before = note.clone();
    let mut session = resumed(note, Orientation::Landscape);
    let unchanged = |session: &LiveSession| {
        let page = &session.pages()[0];
        assert_eq!(page.note.page, before.page);
        assert_eq!(page.note.strokes, before.strokes);
        assert_eq!(page.note.background_image, before.background_image);
        assert_eq!(
            (page.version, page.needs_write()),
            (0, false),
            "nothing to write"
        );
    };
    unchanged(&session);

    // Streaming again after a lost link: the same way.
    let mut out = Vec::new();
    session.set_state(LiveSessionState::Reconnecting, &mut out);
    session.started(29_700, 21_600, Orientation::Landscape, &mut out);
    unchanged(&session);

    // The quick switch (or a change of the settings) to the other portrait position: the sheet
    // is held that way now, its size is the tablet's as it is held: one to one.
    out.clear();
    session.reorient(29_700, 21_600, Orientation::PortraitRight, &mut out);
    unchanged(&session);
    assert_eq!(session.snapshot().orientation, Orientation::PortraitRight);
    assert!(session.snapshot().view.zoom == 1.0);
    let point = write_at(&mut session, 1_234, 5_678, RESUMED_AT + 1_000);
    assert_eq!((point.x, point.y), (1_234.0, 5_678.0));
    let written = session.pages()[0].note.strokes.clone();

    // To landscape, the other way round again: still not turned, the pen follows.
    session.reorient(29_700, 21_600, Orientation::Landscape, &mut out);
    assert_eq!(session.pages()[0].note.page, before.page);
    assert_eq!(session.pages()[0].note.strokes, written);
    assert_eq!(session.snapshot().orientation, Orientation::PortraitLeft);
    assert!(!session.snapshot().can_turn_page);
}

#[test]
fn the_next_page_is_a_new_note_held_as_the_setting_and_the_sheet_stays_as_it_is() {
    let before = sheet(A4_PORTRAIT);
    let mut session = resumed(before.clone(), Orientation::Landscape);
    assert_eq!((session.view().x, session.view().y), (10_800.0, 14_850.0));
    let mut out = Vec::new();
    session.command(
        LiveCommandKind::NextPage,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    let snapshot = session.snapshot();
    assert_eq!((snapshot.page_index, snapshot.page_count), (1, 2));
    assert_ne!(snapshot.page.id, before.id);
    assert_eq!(snapshot.page.page, A4_LANDSCAPE);
    assert_eq!(snapshot.page.origin, Origin::Live);
    assert_eq!(snapshot.orientation, Orientation::Landscape);
    assert!(snapshot.can_turn_page);
    assert_eq!(snapshot.saved, Vec::<String>::new());
    // The whole page is still shown: the view follows the page's size.
    assert_eq!((snapshot.view.x, snapshot.view.y), (14_850.0, 10_800.0));
    let point = write_at(&mut session, 321, 654, RESUMED_AT + 1_000);
    assert_eq!((point.x, point.y), (321.0, 654.0));

    // Back on the sheet: not turned, the pen follows it again.
    session.command(
        LiveCommandKind::PreviousPage,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    let snapshot = session.snapshot();
    assert_eq!(snapshot.page.page, A4_PORTRAIT);
    assert_eq!(snapshot.page.strokes, before.strokes);
    assert_eq!(snapshot.orientation, Orientation::PortraitLeft);
    assert!(!snapshot.can_turn_page);
    assert_eq!((snapshot.view.x, snapshot.view.y), (10_800.0, 14_850.0));
    assert_eq!(session.pages()[0].version, 0);
    let point = write_at(&mut session, 5_000, 3_000, RESUMED_AT + 2_000);
    assert_eq!((point.x, point.y), (21_600.0 - 3_000.0, 5_000.0));

    // A switch while on the new page, then back on the sheet: never turned either.
    let strokes = session.pages()[0].note.strokes.clone();
    session.command(
        LiveCommandKind::NextPage,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    session.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
    session.command(
        LiveCommandKind::PreviousPage,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    assert_eq!(session.pages()[0].note.strokes, strokes);
    assert_eq!(session.snapshot().orientation, Orientation::PortraitLeft);
    let point = write_at(&mut session, 6_000, 2_000, RESUMED_AT + 3_000);
    assert_eq!((point.x, point.y), (6_000.0, 2_000.0), "one to one now");
}

// --- Time ----------------------------------------------------------------------------------------

#[test]
fn new_strokes_continue_after_the_last_one_counting_only_the_time_since_the_resume() {
    let base = 5_000 + RESUME_GAP_MS;
    let mut session = resumed(sheet(A4_LANDSCAPE), Orientation::Landscape);
    assert_eq!(
        session.pages()[0].time_origin(),
        Some(RESUMED_AT - i64::from(base))
    );

    // 2 s after the resume, 2 months after the note was written.
    let point = write_at(&mut session, 10, 10, RESUMED_AT + 2_000);
    assert_eq!(point.t, base + 2_000);
    // The note keeps its date; times go on from there, and never go back.
    let page = &session.pages()[0];
    assert_eq!(page.note.created, CREATED);
    let point = write_at(&mut session, 10, 10, RESUMED_AT + 2_001);
    assert_eq!(point.t, base + 2_000 + 5);
    let point = write_at(&mut session, 10, 10, RESUMED_AT + 60_000);
    assert_eq!(point.t, base + 60_000);

    // The same whatever the age of the note: nothing collapses at 49.7 days (u32 of milliseconds).
    let mut old = sheet(A4_LANDSCAPE);
    old.created = RESUMED_AT - 365 * 24 * 3600 * 1000;
    let mut session = resumed(old, Orientation::Landscape);
    assert_eq!(
        write_at(&mut session, 1, 1, RESUMED_AT + 2_000).t,
        base + 2_000
    );

    // A note without any stroke or narration starts at the resume; its date stays too.
    let mut blank = sheet(A4_LANDSCAPE);
    blank.strokes.clear();
    let mut session = resumed(blank, Orientation::Landscape);
    assert_eq!(session.pages()[0].time_origin(), Some(RESUMED_AT));
    assert_eq!(write_at(&mut session, 1, 1, RESUMED_AT + 30).t, 30);
    assert_eq!(session.pages()[0].note.created, CREATED);

    // Times already at the end of the range stay there, without a panic.
    let mut full = sheet(A4_LANDSCAPE);
    full.strokes = vec![ink(1.0, 1.0, &[u32::MAX - 3], 0)];
    let mut session = resumed(full, Orientation::Landscape);
    assert_eq!(write_at(&mut session, 1, 1, RESUMED_AT + 2_000).t, u32::MAX);
}

#[test]
fn the_time_base_is_after_the_last_stroke_and_after_the_narration() {
    let note = sheet(A4_LANDSCAPE);
    assert_eq!(time_base(&note, None), 5_000 + RESUME_GAP_MS);
    // A stroke of a hidden layer counts: its time is taken.
    let mut hidden = sheet(A4_LANDSCAPE);
    hidden.layers.push(layer("Réponses", false));
    hidden.strokes.push(ink(1.0, 1.0, &[9_000], 1));
    assert_eq!(time_base(&hidden, None), 9_000 + RESUME_GAP_MS);
    // A narration that goes on after the last stroke.
    let audio = encode_narration(&note_narration(1.0, 7_000)).unwrap();
    assert_eq!(time_base(&note, Some(&audio)), 8_000 + RESUME_GAP_MS);
    // One that ends before it changes nothing.
    let audio = encode_narration(&note_narration(1.0, 0)).unwrap();
    assert_eq!(time_base(&note, Some(&audio)), 5_000 + RESUME_GAP_MS);
    // One recorded during a replay at speed 2: 5 s of ink last 2.5 s; 1.5 s of sound beyond it
    // stand for 3 s of note time.
    let fast = ReplayOptions {
        speed: 2.0,
        max_pause: None,
    };
    let mut replayed = note_narration(1.0, 3_000);
    replayed.base = NarrationBase::Replay;
    replayed.options = fast;
    let audio = encode_narration(&replayed).unwrap();
    assert_eq!(time_base(&note, Some(&audio)), 8_000 + RESUME_GAP_MS);
    // A narration that cannot be read counts for nothing.
    assert_eq!(time_base(&note, Some(b"damaged")), 5_000 + RESUME_GAP_MS);
    // Neither stroke nor narration.
    assert_eq!(time_base(&blank_note("", "Calque 1", 0), None), 0);
}

#[test]
fn a_late_narration_lands_after_the_older_sound() {
    let earlier = note_narration(1.0, 0);
    // A replay that trims nothing at speed 1 is note time: nothing moves.
    let note = sheet(A4_LANDSCAPE);
    let later = placed_for_merge(note_narration(1.0, 6_000), &earlier, &note);
    assert_eq!(later.start_ms, 6_000);

    // At speed 2: 5 s of ink last 2.5 s, 1 s of note time after them lasts 0.5 s.
    let mut fast = note_narration(1.0, 0);
    fast.base = NarrationBase::Replay;
    fast.options.speed = 2.0;
    let later = placed_for_merge(note_narration(1.0, 6_000), &fast, &note);
    assert_eq!(later.start_ms, 3_000);
    // Within the ink, it follows the replay's clock.
    let later = placed_for_merge(note_narration(1.0, 4_000), &fast, &note);
    assert_eq!(later.start_ms, 2_000);
    // Never over the older sound.
    let later = placed_for_merge(note_narration(1.0, 0), &fast, &note);
    assert_eq!(later.start_ms, 1_000);

    // Too far apart for one track: `merge` alone would keep the later one only.
    let far = note_narration(1.0, 5 * 3600 * 1000);
    assert!(!earlier.fits_with(&far));
    let merged = merged_keeping_both(earlier.clone(), far).unwrap();
    assert_eq!(merged.start_ms, 0);
    assert_eq!(merged.duration(), Duration::from_secs(2));
    assert_eq!(merged.base, NarrationBase::Note);
}

// --- Layers --------------------------------------------------------------------------------------

#[test]
fn new_ink_goes_on_the_last_visible_layer_never_on_a_hidden_one() {
    let mut note = sheet(A4_LANDSCAPE);
    note.layers = vec![
        layer("Cours", true),
        layer("Dessin", true),
        layer("Réponses", false),
        layer("Brouillon", false),
    ];
    let mut session = resumed(note, Orientation::Landscape);
    assert_eq!(session.snapshot().layer, 1);
    write_at(&mut session, 1, 1, RESUMED_AT + 1_000);
    assert_eq!(session.pages()[0].note.strokes.last().unwrap().layer, 1);
    // A new layer is on top and visible: it receives the ink.
    let mut out = Vec::new();
    session.command(
        LiveCommandKind::NewLayer,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    assert_eq!(session.snapshot().layer, 4);
    write_at(&mut session, 1, 1, RESUMED_AT + 2_000);
    assert_eq!(session.pages()[0].note.strokes.last().unwrap().layer, 4);
    assert!(!session.pages()[0].note.layers[2].visible);
}

/// Every layer hidden: nothing is shown, and none may be: the sheet gets a new visible layer,
/// which costs no write until something is written on it.
#[test]
fn a_sheet_with_every_layer_hidden_gets_a_new_layer_to_write_on() {
    let mut note = sheet(A4_LANDSCAPE);
    note.layers = vec![layer("Cours", false), layer("Réponses", false)];
    let mut session = resumed(note, Orientation::Landscape);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.layer, 2);
    assert_eq!(snapshot.page.layers.len(), 3);
    assert_eq!(snapshot.page.layers[2], layer("Calque 3", true));
    assert!(!snapshot.page.layers[0].visible && !snapshot.page.layers[1].visible);
    assert!(!session.pages()[0].needs_write());
    write_at(&mut session, 1, 1, RESUMED_AT + 1_000);
    assert_eq!(session.pages()[0].note.strokes.last().unwrap().layer, 2);
    assert!(session.pages()[0].needs_write());
}

// --- Blackboard ----------------------------------------------------------------------------------

fn set_board(session: &mut LiveSession, board: bool) {
    let view = LiveViewState {
        board,
        ..session.view()
    };
    session.set_view(view, &mut Vec::new());
}

#[test]
fn the_blackboard_gives_the_sheets_paper_back() {
    for paper in [
        Paper::Cream,
        Paper::Kraft,
        Paper::Custom(Rgba::rgb(1, 2, 3)),
        Paper::White,
    ] {
        let mut note = sheet(A4_LANDSCAPE);
        let background = Background {
            paper,
            template: Template::Grid5mm,
            texture: true,
        };
        note.background = background;
        let mut session = resumed(note, Orientation::Landscape);
        assert!(!session.view().board);
        let pen_style = session.style();
        set_board(&mut session, true);
        assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
        assert_eq!(session.style(), LiveStyle::chalk());
        // Its template and texture stay.
        assert_eq!(
            session.pages()[0].note.background.template,
            Template::Grid5mm
        );
        set_board(&mut session, false);
        assert_eq!(session.pages()[0].note.background, background);
        assert_eq!(session.style(), pen_style);
        // And again.
        set_board(&mut session, true);
        set_board(&mut session, false);
        assert_eq!(session.pages()[0].note.background, background);
    }
}

#[test]
fn a_slate_sheet_stays_a_slate_sheet_and_the_next_page_follows_the_blackboard() {
    let mut note = sheet(A4_LANDSCAPE);
    note.background.paper = Paper::Slate;
    let mut session = resumed(note, Orientation::Landscape);
    // Written on with chalk, as on the blackboard.
    assert!(session.view().board);
    assert_eq!(session.style(), LiveStyle::chalk());
    assert!(!session.pages()[0].needs_write());
    // Off: the pen comes back, the sheet keeps its paper; nothing to write.
    set_board(&mut session, false);
    assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
    assert_eq!(session.style(), LiveStyle::default());
    assert!(!session.pages()[0].needs_write());
    set_board(&mut session, true);
    set_board(&mut session, false);
    assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
    // A new page made while the blackboard is on keeps the sheet's own paper (M17: a new page
    // takes the paper of the page shown), slate here, with the blackboard on or off.
    set_board(&mut session, true);
    let mut out = Vec::new();
    session.command(
        LiveCommandKind::NewPage,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    assert_eq!(session.pages()[1].note.background.paper, Paper::Slate);
    set_board(&mut session, false);
    assert_eq!(session.pages()[1].note.background.paper, Paper::Slate);
    assert!(session.pages()[1].note.background_image.is_none());
}

#[test]
fn a_new_page_takes_the_sheets_paper_but_not_its_image() {
    let file = full_file(A5);
    let background = file.note.background;
    let mut session = resumed_file(file, Orientation::Landscape);
    assert!(session.snapshot().image_pixels.is_some());
    let mut out = Vec::new();
    session.command(
        LiveCommandKind::NextPage,
        LiveButton::NewPage,
        RESUMED_AT,
        &mut out,
    );
    let snapshot = session.snapshot();
    assert_eq!(snapshot.page.background, background);
    assert_eq!(snapshot.page.background_image, None);
    assert_eq!(snapshot.image_pixels, None);
    assert!(snapshot.can_turn_page);
}

// --- The hub, with a real library ----------------------------------------------------------------

/// Writes `file` in the binder `binder` of `library` and returns its path.
fn filed(library: &Library, binder: &str, file: &NoteFile) -> PathBuf {
    library.create(Path::new(binder), file).unwrap().path
}

/// Starts a session on the note at `path`, streaming from a tablet of `tablet`, held in landscape.
fn resume(hub: &LiveHub, library: &Library, path: &Path, tablet: PageSize) -> HubSink {
    let start = StartOptions {
        size: tablet,
        ..options(library, fixed(LiveButton::NewPage))
    };
    let Start::New { mut sink, snapshot } = hub.resume(start, path).unwrap() else {
        panic!("a session is already running");
    };
    assert_eq!(snapshot.state, LiveSessionState::Waiting);
    sink.input(LiveInput::Started {
        width: tablet.width,
        height: tablet.height,
        orientation: Orientation::Landscape,
        kind: TabletKind::Slate,
    });
    sink
}

/// What the note must still hold after live mode wrote on it (the ink and the narration apart,
/// which each test looks at, and the original, kept by this very write). Spelled out member by
/// member: a member added to the format does not compile here until it is looked at.
fn assert_keeps_all_but_ink(written: &NoteFile, before: &NoteFile) {
    let NoteFile {
        note: _,
        original,
        original_note: _,
        thumbnail: _,
        audio: _,
        background,
        text,
        revision,
        unknown_fields,
        unknown_members,
    } = written;
    let (a, b) = (&written.note, &before.note);
    assert_eq!(
        (&a.id, &a.title, a.created, a.page, a.background),
        (&b.id, &b.title, b.created, b.page, b.background)
    );
    assert_eq!(a.background_image, b.background_image);
    assert_eq!(
        (&a.tags, a.favorite, &a.layers),
        (&b.tags, b.favorite, &b.layers)
    );
    assert_eq!(a.objects, b.objects, "its text frames, where they were");
    assert_eq!((a.timing, &a.origin), (b.timing, &b.origin));
    assert_eq!(*original, before.original);
    assert_eq!(*background, before.background);
    assert_eq!(*text, before.text);
    assert_eq!(*revision, before.revision);
    assert_eq!(*unknown_fields, before.unknown_fields);
    assert_eq!(*unknown_members, before.unknown_members);
}

fn tablet_context() -> TabletContext {
    TabletContext::of(Path::new("unused"), &Settings::default(), None)
}

fn forks(window: &Window) -> Vec<(usize, ForkReason, String, String)> {
    window
        .events()
        .iter()
        .filter_map(|event| match event {
            LiveEvent::SheetForked {
                page_index,
                reason,
                original,
                path,
            } => Some((*page_index, *reason, original.clone(), path.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_resumed_note_keeps_every_member_through_a_real_save_and_reload() {
    let (dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    assert!(before.note.modified > 0);

    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    assert_eq!(hub.snapshot().page.strokes, before.note.strokes);
    stroke(&mut sink, 3000, 5);
    wait_until("the note rewritten", || {
        stroke_count(&library, &path) == Some(3)
    });
    assert_eq!(
        notes_in(&library),
        std::slice::from_ref(&path),
        "no other note"
    );
    // More ink, more writes: everything is still there.
    stroke(&mut sink, 4000, 5);
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&path)]);
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes.len(), 4);
    assert_eq!(written.note.strokes[..2], before.note.strokes[..]);
    assert_keeps_all_but_ink(&written, &before);
    assert_eq!(written.audio, before.audio, "its narration, untouched");
    assert!(written.note.modified >= before.note.modified);
    // The picture is one 1:1 of the tablet: the new points are as they came.
    let new_point = written.note.strokes[2].points[0];
    assert_eq!((new_point.x, new_point.y), (3000.0, 1000.0));
    // The thumbnail is new, and draws the image.
    assert_ne!(written.thumbnail, before.thumbnail);
    assert!(written.thumbnail.is_some());
    assert_ne!(
        written.thumbnail,
        crate::thumbnails::thumbnail_png(&written.note)
    );
    // The atomic writes leave nothing behind.
    let names: Vec<_> = std::fs::read_dir(dir.path().join("Classe"))
        .unwrap()
        .collect();
    assert_eq!(names.len(), 1);

    // « Back to the original » gives the sheet as it was before the resume.
    let NoteOriginal::FirstVersion {
        strokes,
        page,
        layers,
        objects,
        paper,
        background_image,
    } = original_of(&written, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    assert_eq!(strokes, before.note.strokes);
    assert_eq!(objects, before.note.objects);
    assert_eq!(
        (page, layers),
        (before.note.page, before.note.layers.clone())
    );
    assert_eq!(paper, before.note.background.paper);
    assert_eq!(background_image, before.note.background_image);
    // No fork, no notice.
    assert!(forks(&window).is_empty());
}

#[test]
fn a_resumed_note_that_receives_no_change_is_not_written_at_all() {
    let (_dir, library) = library();
    let path = filed(&library, "", &full_file(A5));
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    // Hovering, paging through, leaving: no change. The note is not one this session wrote.
    send(&mut sink, &[pen(10, 10, 0), LiveRecord::ProximityOut]);
    hub.command(LiveCommandKind::NextPage).unwrap();
    hub.command(LiveCommandKind::PreviousPage).unwrap();
    std::thread::sleep(config().save_interval * 2);
    assert_eq!(hub.snapshot().saved, Vec::<String>::new());
    assert_eq!(hub.stop().unwrap(), Vec::<String>::new());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(FileStamp::of(&path).unwrap(), stamp, "same modified date");
    assert_eq!(notes_in(&library).len(), 1);
    assert!(!window.has(|e| matches!(e, LiveEvent::Saved { .. })));
    assert!(
        hub.overview().unwrap().notes.is_empty(),
        "nothing for the assistant"
    );

    // What changes nothing in the end does not write either: clearing an empty sheet.
    let blank = NoteFile::new(blank_note("Vide", "Calque 1", CREATED));
    let empty = filed(&library, "", &blank);
    let (bytes, stamp) = (
        std::fs::read(&empty).unwrap(),
        FileStamp::of(&empty).unwrap(),
    );
    let _sink = resume(&hub, &library, &empty, A5);
    hub.command(LiveCommandKind::ClearPage).unwrap();
    std::thread::sleep(config().save_interval * 2);
    assert_eq!(hub.stop().unwrap(), Vec::<String>::new());
    assert_eq!(std::fs::read(&empty).unwrap(), bytes);
    assert_eq!(FileStamp::of(&empty).unwrap(), stamp);
}

#[test]
fn what_an_original_becomes_when_a_note_is_resumed() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let write_once = |path: &Path| -> NoteFile {
        let mut sink = resume(&hub, &library, path, A5);
        stroke(&mut sink, 3000, 5);
        hub.stop().unwrap();
        library.read(path).unwrap().file
    };

    // A tablet note keeps the tablet's bytes: its original stays the synced page.
    let mut tablet_note = full_file(A5);
    tablet_note.note.origin = Origin::Tablet;
    tablet_note.original = Some(vec![1, 2, 3]);
    let path = filed(&library, "", &tablet_note);
    let before = library.read(&path).unwrap().file;
    let written = write_once(&path);
    assert_keeps_all_but_ink(&written, &before);
    assert_eq!(written.original, Some(vec![1, 2, 3]));
    assert_eq!(written.original_note, None);

    // A note edited before keeps the first version it has: not the one before the resume.
    let mut edited = full_file(A5);
    edited.original_note = Some(b"first version".to_vec());
    edited.note.title = "Déjà modifiée".to_owned();
    let path = filed(&library, "", &edited);
    let before = library.read(&path).unwrap().file;
    let written = write_once(&path);
    assert_keeps_all_but_ink(&written, &before);
    assert_eq!(written.original_note, Some(b"first version".to_vec()));

    // A blank note goes back to being blank: see the worksheet below.
    let blank = NoteFile::new(blank_note("Vide", "Calque 1", CREATED));
    let path = filed(&library, "", &blank);
    let written = write_once(&path);
    assert_eq!(written.note.strokes.len(), 1);
    assert!(matches!(
        original_of(&written, &tablet_context()).unwrap(),
        NoteOriginal::FirstVersion { strokes, .. } if strokes.is_empty()
    ));

    // A first write without new ink (its narration) keeps no original; the next one does, with
    // the ink of before the resume.
    let plain = full_file(A5);
    let path = filed(&library, "", &plain);
    let before = library.read(&path).unwrap().file;
    let mut sink = resume(&hub, &library, &path, A5);
    assert!(hub.set_page_audio(0, b"OggS new".to_vec()));
    wait_until("the narration written", || {
        audio_of(&library, &path) == Some(b"OggS new".to_vec())
    });
    assert_eq!(library.read(&path).unwrap().file.original_note, None);
    stroke(&mut sink, 3000, 5);
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    let NoteOriginal::FirstVersion { strokes, .. } =
        original_of(&written, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    assert_eq!(strokes, before.note.strokes);
}

/// A sheet with a page of a PDF under it and no ink, as a teacher prepares one.
fn blank_worksheet(page: PageSize) -> NoteFile {
    let mut file = full_file(page);
    file.note.strokes.clear();
    // Blank: nothing drawn (a sheet of text frames only has its own test).
    file.note.objects.clear();
    file.note.background.paper = Paper::Kraft;
    file.audio = None;
    file
}

/// What the editor does with « Back to the original »: the original's ink in the note, saved, as
/// `lib/editor/original.ts` does.
fn back_to_the_original(library: &Library, path: &Path) -> NoteFile {
    let loaded = library.read(path).unwrap();
    let NoteOriginal::FirstVersion {
        page,
        layers,
        strokes,
        objects,
        paper,
        background_image,
    } = original_of(&loaded.file, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    let mut note = loaded.file.note.clone();
    (note.page, note.layers, note.strokes, note.objects) = (page, layers, strokes, objects);
    note.background_image = background_image;
    if note.background.paper == Paper::Slate && paper != Paper::Slate {
        note.background.paper = paper;
    }
    crate::notes::save_in(library, path, note, &loaded.stamp).unwrap();
    library.read(path).unwrap().file
}

#[test]
fn a_blank_worksheet_written_on_can_go_back_to_being_blank() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &blank_worksheet(A5));
    let before = library.read(&path).unwrap().file;
    assert!(before.note.strokes.is_empty());
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    wait_until("the first write", || {
        stroke_count(&library, &path) == Some(1)
    });

    // After a reload: the original is the sheet before the resume, with no stroke at all.
    let written = library.read(&path).unwrap().file;
    let kept = written.original_note.clone().expect("original.ardoise");
    let NoteOriginal::FirstVersion {
        strokes,
        layers,
        page,
        objects,
        paper,
        background_image,
    } = original_of(&written, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    assert!(strokes.is_empty() && objects.is_empty());
    assert_eq!(
        (page, layers, paper),
        (before.note.page, before.note.layers.clone(), Paper::Kraft)
    );
    assert_eq!(background_image, before.note.background_image);

    // More ink: the original stays what it was.
    stroke(&mut sink, 4000, 5);
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes.len(), 2);
    assert_eq!(written.original_note, Some(kept.clone()));
    assert_eq!(written.background, before.background);

    // Back to it, saved, and read again: the blank sheet, its picture and paper as they were.
    let restored = back_to_the_original(&library, &path);
    assert!(restored.note.strokes.is_empty());
    assert_eq!(restored.note.layers, before.note.layers);
    assert_eq!(restored.note.background, before.note.background);
    assert_eq!(restored.note.background_image, before.note.background_image);
    assert_eq!(restored.background, before.background);
    assert_eq!(restored.original_note, Some(kept), "it stays recoverable");

    // A new blank note filled in the editor keeps today's rule: see `original/tests.rs`.
}

#[test]
fn a_note_changed_elsewhere_is_left_alone_and_the_page_goes_on_in_its_binder() {
    let (dir, library) = library();
    let path = filed(&library, "Classe/4e", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);

    // Another PC changes its ink (through Nextcloud), before the session wrote anything.
    let mut elsewhere = library.read(&path).unwrap();
    elsewhere.file.note.strokes.pop();
    library
        .save(&path, &elsewhere.file, &elsewhere.stamp)
        .unwrap();
    let kept = std::fs::read(&path).unwrap();

    stroke(&mut sink, 3000, 5);
    wait_until("a new note", || notes_in(&library).len() == 2);
    wait_until("the notice", || !forks(&window).is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), kept, "not overwritten");

    let new_path = notes_in(&library).into_iter().find(|p| *p != path).unwrap();
    assert_eq!(
        new_path.parent(),
        Some(dir.path().join("Classe").join("4e")).as_deref()
    );
    assert_eq!(
        forks(&window),
        [(
            0,
            ForkReason::Changed,
            path_text(&path),
            path_text(&new_path)
        )]
    );
    // Everything went with it, and its original is the sheet before the resume.
    let written = library.read(&new_path).unwrap().file;
    assert_ne!(written.note.id, before.note.id);
    assert_eq!(written.note.strokes.len(), 3);
    assert_eq!(written.note.objects, before.note.objects);
    assert_eq!(written.background, before.background);
    assert_eq!(written.audio, before.audio);
    assert_eq!(written.text, before.text);
    assert_eq!(written.unknown_fields, before.unknown_fields);
    assert_eq!(written.unknown_members, before.unknown_members);
    assert_eq!(written.note.background_image, before.note.background_image);
    assert!(matches!(
        original_of(&written, &tablet_context()).unwrap(),
        NoteOriginal::FirstVersion { .. }
    ));

    // The page goes on there: the next strokes land in the new note, the event is not repeated.
    stroke(&mut sink, 4000, 5);
    wait_until("the new note rewritten", || {
        stroke_count(&library, &new_path) == Some(4)
    });
    // A window opened later is told too, while the session runs.
    let late = Arc::new(Window::default());
    hub.subscribe("presentation", late.clone());
    wait_until("the notice in the late window", || forks(&late).len() == 1);

    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&new_path)]);
    assert_eq!(forks(&window).len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), kept);
    let overview = hub.overview().unwrap();
    assert_eq!(overview.notes[0].1, written.note.id, "the note's real id");
    assert_eq!(overview.notes[0].2, new_path);

    // The view of the next lesson does not open on a notice about this one.
    let next = Arc::new(Window::default());
    hub.subscribe("main", next.clone());
    wait_until("the first batch", || !next.events().is_empty());
    std::thread::sleep(config().flush_interval * 5);
    assert_eq!(forks(&next), []);
    let _sink = {
        let Start::New { sink, .. } = hub
            .start(options(&library, fixed(LiveButton::NewPage)))
            .unwrap()
        else {
            panic!("already running");
        };
        sink
    };
    let fresh = Arc::new(Window::default());
    hub.subscribe("main", fresh.clone());
    wait_until("the first batch", || !fresh.events().is_empty());
    assert_eq!(forks(&fresh), []);
    assert_eq!(hub.forked_sheets(), []);
}

#[test]
fn a_fork_changes_nothing_on_screen_nor_for_the_viewers() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let before = hub.snapshot();
    let mark = LiveMark {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        points: vec![MarkPoint {
            x: 10.0,
            y: 10.0,
            p: 1.0,
        }],
    };
    hub.set_marks(0, vec![mark.clone()]).unwrap();

    std::fs::remove_file(&path).unwrap();
    stroke(&mut sink, 3000, 5);
    wait_until("the notice", || !forks(&window).is_empty());

    // The note has its own id now; the page is the same page for everyone who shows it.
    let new_path = PathBuf::from(&forks(&window)[0].3);
    let written = library.read(&new_path).unwrap().file;
    assert_ne!(written.note.id, before.page.id);
    let after = hub.snapshot();
    assert_eq!(after.page.id, before.page.id);
    assert_eq!(after.page.strokes.len(), before.page.strokes.len() + 1);
    assert_eq!(paper_key(&after), paper_key(&before), "no new picture");
    // The marks stay, here and for the windows (which keep theirs while the id stays).
    let ids: Vec<String> = window
        .events()
        .iter()
        .filter_map(|event| match event {
            LiveEvent::Snapshot { snapshot } => Some(snapshot.page.id.clone()),
            _ => None,
        })
        .collect();
    assert!(ids.iter().all(|id| *id == before.page.id), "{ids:?}");
    assert!(matches!(
        hub.current_events().last(),
        Some(LiveEvent::Marks { marks }) if *marks == [mark]
    ));
    // Later writes still go through the note's own id.
    stroke(&mut sink, 4000, 5);
    hub.stop().unwrap();
    assert_eq!(notes_in(&library), std::slice::from_ref(&new_path));
    assert_eq!(hub.overview().unwrap().notes[0].1, written.note.id);
    assert_eq!(
        library.read(&new_path).unwrap().file.note.id,
        written.note.id
    );
}

#[test]
fn a_change_after_the_first_write_is_noticed_too_narration_and_members_included() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    wait_until("the first write", || {
        stroke_count(&library, &path) == Some(3)
    });

    // Its narration is replaced elsewhere: the ink is the same, but the file is not the one the
    // page wrote.
    let mut elsewhere = library.read(&path).unwrap();
    elsewhere.file.audio = Some(b"OggS from the other PC".to_vec());
    library
        .save(&path, &elsewhere.file, &elsewhere.stamp)
        .unwrap();
    stroke(&mut sink, 4000, 5);
    wait_until("the notice", || !forks(&window).is_empty());
    assert_eq!(
        audio_of(&library, &path),
        Some(b"OggS from the other PC".to_vec())
    );
    assert_eq!(forks(&window)[0].1, ForkReason::Changed);
    hub.stop().unwrap();
}

#[test]
fn a_note_deleted_during_the_resume_becomes_a_new_note_in_its_binder_with_its_image() {
    let (dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);

    std::fs::remove_file(&path).unwrap();
    stroke(&mut sink, 3000, 5);
    wait_until("the notice", || !forks(&window).is_empty());
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 1);
    let new_path = PathBuf::from(&saved[0]);
    assert_eq!(
        new_path.parent(),
        Some(dir.path().join("Classe")).as_deref()
    );
    assert_eq!(
        forks(&window),
        [(0, ForkReason::Deleted, path_text(&path), saved[0].clone())]
    );
    let written = library.read(&new_path).unwrap().file;
    assert_eq!(written.note.strokes.len(), 3);
    assert_eq!(written.note.objects, before.note.objects);
    assert_eq!(written.background, before.background);
    assert_eq!(written.note.background_image, before.note.background_image);
    assert_eq!(written.note.created, before.note.created);

    // Even when its binder went with it.
    let path = filed(&library, "Disparu", &full_file(A5));
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    std::fs::remove_dir_all(dir.path().join("Disparu")).unwrap();
    stroke(&mut sink, 3000, 5);
    let saved = hub.stop().unwrap();
    assert_eq!(
        Path::new(&saved[0]).parent(),
        Some(dir.path().join("Disparu")).as_deref()
    );
}

#[test]
fn a_note_only_retitled_or_moved_goes_on_receiving_the_page_with_all_it_holds() {
    let (dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);

    // Retitled and tagged from the library, before the first write.
    let renamed = library.retitle(&path, "Optique", None).unwrap().path;
    library
        .set_tags(&renamed, &["optique".to_owned()], None)
        .unwrap();
    stroke(&mut sink, 3000, 5);
    wait_until("the renamed note rewritten", || {
        stroke_count(&library, &renamed) == Some(3)
    });
    assert_eq!(notes_in(&library), std::slice::from_ref(&renamed));
    let written = library.read(&renamed).unwrap().file;
    assert_eq!(
        (written.note.title.as_str(), written.note.tags.clone()),
        ("Optique", vec!["optique".to_owned()])
    );
    // The page whole but for what the library changed: its text frame too.
    assert!(!before.note.objects.is_empty());
    assert_eq!(written.note.objects, before.note.objects);
    assert_eq!(written.note.layers, before.note.layers);
    assert_eq!(written.background, before.background);
    assert_eq!(written.text, before.text);
    assert_eq!(written.unknown_fields, before.unknown_fields);
    assert_eq!(written.unknown_members, before.unknown_members);
    // Its original was kept by that very write.
    let NoteOriginal::FirstVersion { strokes, .. } =
        original_of(&written, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    assert_eq!(strokes, before.note.strokes);

    // Moved: followed.
    let moved = library.move_to(&renamed, Path::new("Physique")).unwrap();
    stroke(&mut sink, 4000, 5);
    wait_until("the moved note rewritten", || {
        stroke_count(&library, &moved) == Some(4)
    });
    assert_eq!(notes_in(&library), std::slice::from_ref(&moved));
    assert_eq!(moved.parent(), Some(dir.path().join("Physique")).as_deref());
    hub.stop().unwrap();
    assert!(forks(&window).is_empty());
}

/// A kraft sheet with ink, resumed by a new hub, its bytes and stamp as a test compares them.
fn kraft_sheet(library: &Library) -> (PathBuf, Vec<u8>, FileStamp) {
    let mut file = full_file(A5);
    file.note.background.paper = Paper::Kraft;
    let path = filed(library, "", &file);
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    (path, bytes, stamp)
}

fn press_board(hub: &LiveHub, on: bool) {
    let mut view = hub.snapshot().view;
    view.board = on;
    hub.set_view(view).unwrap();
}

/// The session stopped, the note reloaded: not one byte of it was written.
fn assert_untouched(hub: &LiveHub, path: &Path, bytes: &[u8], stamp: &FileStamp) {
    assert_eq!(hub.stop().unwrap(), Vec::<String>::new());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(FileStamp::of(path).unwrap(), *stamp);
    assert!(hub.overview().unwrap().notes.is_empty());
}

#[test]
fn the_blackboard_left_on_over_a_sheet_does_not_reach_its_file() {
    let (_dir, library) = library();
    let (path, bytes, stamp) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    press_board(&hub, true);
    assert_eq!(hub.snapshot().page.background.paper, Paper::Slate);
    std::thread::sleep(config().save_interval * 2);
    assert_untouched(&hub, &path, &bytes, &stamp);
}

#[test]
fn the_blackboard_turned_on_and_off_leaves_the_sheet_untouched() {
    let (_dir, library) = library();
    let (path, bytes, stamp) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    press_board(&hub, true);
    press_board(&hub, false);
    assert_eq!(hub.snapshot().page.background.paper, Paper::Kraft);
    std::thread::sleep(config().save_interval * 2);
    assert_untouched(&hub, &path, &bytes, &stamp);
}

#[test]
fn the_blackboard_turned_off_on_the_next_page_leaves_the_sheet_untouched() {
    let (_dir, library) = library();
    let (path, bytes, stamp) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    press_board(&hub, true);
    hub.command(LiveCommandKind::NextPage).unwrap();
    press_board(&hub, false);
    // Shown again, the sheet has its own paper back.
    hub.command(LiveCommandKind::PreviousPage).unwrap();
    assert_eq!(hub.snapshot().page.background.paper, Paper::Kraft);
    std::thread::sleep(config().save_interval * 2);
    assert_untouched(&hub, &path, &bytes, &stamp);
}

#[test]
fn turned_off_from_a_page_it_was_not_turned_on_from_the_sheet_keeps_its_paper() {
    let (_dir, library) = library();
    let (path, bytes, stamp) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.command(LiveCommandKind::NextPage).unwrap();
    press_board(&hub, true);
    hub.command(LiveCommandKind::PreviousPage).unwrap();
    press_board(&hub, false);
    assert_eq!(hub.snapshot().page.background.paper, Paper::Kraft);
    assert_untouched(&hub, &path, &bytes, &stamp);
}

#[test]
fn ink_written_under_the_blackboard_keeps_the_slate_it_needs() {
    let (_dir, library) = library();
    let (path, _, _) = kraft_sheet(&library);
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    press_board(&hub, true);
    stroke(&mut sink, 3000, 5);
    assert_eq!(hub.stop().unwrap(), [path_text(&path)]);
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.background.paper, Paper::Slate);
    assert_eq!(written.note.strokes.len(), 3);
    assert_eq!(written.note.strokes[2].tool, Tool::Chalk);
    // The paper comes back with the ink, in the one case where it should.
    let restored = back_to_the_original(&library, &path);
    assert_eq!(restored.note.background.paper, Paper::Kraft);
    assert_eq!(restored.note.strokes, before.note.strokes);
}

#[test]
fn chalk_from_a_blackboard_turned_on_elsewhere_keeps_the_slate_too() {
    let (_dir, library) = library();
    let (path, _, _) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    hub.command(LiveCommandKind::NextPage).unwrap();
    press_board(&hub, true);
    hub.command(LiveCommandKind::PreviousPage).unwrap();
    stroke(&mut sink, 3000, 5);
    hub.stop().unwrap();
    assert_eq!(
        library.read(&path).unwrap().file.note.background.paper,
        Paper::Slate
    );
}

#[test]
fn ink_without_the_blackboard_keeps_the_sheets_paper_whatever_it_is_pressed_to_after() {
    let (_dir, library) = library();
    let (path, _, _) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    press_board(&hub, true);
    assert_eq!(hub.stop().unwrap(), [path_text(&path)]);
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes.len(), 3);
    assert_eq!(written.note.background.paper, Paper::Kraft);
}

#[test]
fn the_slate_goes_when_the_ink_under_it_goes() {
    let (_dir, library) = library();
    let (path, _, _) = kraft_sheet(&library);
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    press_board(&hub, true);
    stroke(&mut sink, 3000, 5);
    wait_until("the slate written", || {
        stroke_count(&library, &path) == Some(3)
    });
    assert_eq!(
        library.read(&path).unwrap().file.note.background.paper,
        Paper::Slate
    );
    // The chalk is deleted: the sheet is as it was, on its paper, even with the blackboard on.
    let snapshot = hub.snapshot();
    hub.edit_strokes(0, snapshot.revision, delete(&[2]))
        .unwrap();
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes, before.note.strokes);
    assert_eq!(written.note.background, before.note.background);
    assert_eq!(written.original_note, None, "nothing to go back to");
}

#[test]
fn a_new_session_does_not_inherit_the_blackboard_of_a_slate_sheet() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let red = LiveStyle {
        tool: Tool::FeltTip,
        color: Rgba::rgb(200, 0, 0),
        width: 110.0,
    };
    let start_plain = |hub: &LiveHub| {
        let Start::New { sink, .. } = hub
            .start(options(&library, fixed(LiveButton::NewPage)))
            .unwrap()
        else {
            panic!("already running");
        };
        sink
    };
    let _sink = start_plain(&hub);
    hub.set_style(red).unwrap();
    hub.stop().unwrap();

    // The blackboard came on by itself with the sheet; a plain start is back on paper, with the
    // ink from before it.
    let mut slate = NoteFile::new(sheet(A5));
    slate.note.background.paper = Paper::Slate;
    let path = filed(&library, "", &slate);
    let _sink = resume(&hub, &library, &path, A5);
    assert!(hub.snapshot().view.board);
    assert_eq!(hub.snapshot().style, LiveStyle::chalk());
    hub.stop().unwrap();
    let _sink = start_plain(&hub);
    let snapshot = hub.snapshot();
    assert!(!snapshot.view.board);
    assert_eq!(snapshot.page.background.paper, Paper::White);
    assert_eq!(snapshot.style, red);
    hub.stop().unwrap();

    // Asked for on that sheet (off and on again), it is carried like any blackboard.
    let _sink = resume(&hub, &library, &path, A5);
    press_board(&hub, false);
    press_board(&hub, true);
    hub.stop().unwrap();
    let _sink = start_plain(&hub);
    assert!(hub.snapshot().view.board);
    assert_eq!(hub.snapshot().style, LiveStyle::chalk());
    assert_eq!(hub.snapshot().page.background.paper, Paper::Slate);
    // And gives back the ink it replaced, through the new session.
    press_board(&hub, false);
    assert_eq!(hub.snapshot().style, red);
    hub.stop().unwrap();
}

#[test]
fn the_blackboard_left_on_does_not_touch_the_next_sheet() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let _sink = {
        let Start::New { sink, .. } = hub
            .start(options(&library, fixed(LiveButton::NewPage)))
            .unwrap()
        else {
            panic!("already running");
        };
        sink
    };
    let red = LiveStyle {
        tool: Tool::FeltTip,
        color: Rgba::rgb(200, 0, 0),
        width: 110.0,
    };
    hub.set_style(red).unwrap();
    let mut view = hub.snapshot().view;
    view.board = true;
    view.laser = true;
    hub.set_view(view).unwrap();
    hub.stop().unwrap();

    let file = full_file(A5);
    let path = filed(&library, "", &file);
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let _sink = resume(&hub, &library, &path, A5);
    let snapshot = hub.snapshot();
    assert_eq!(snapshot.page.background.paper, Paper::Cream);
    assert!(!snapshot.view.board);
    assert!(snapshot.view.laser, "the laser carries over");
    assert_eq!(snapshot.style, red, "the style from before the blackboard");
    assert_eq!(hub.stop().unwrap(), Vec::<String>::new());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(FileStamp::of(&path).unwrap(), stamp);
}

#[test]
fn hidden_layers_stay_hidden_and_reach_no_viewer() {
    let (_dir, library) = library();
    let mut file = full_file(A5);
    file.note.layers = vec![layer("Cours", false), layer("Réponses", false)];
    file.note.strokes = vec![ink(100.0, 100.0, &[0, 5], 1)];
    let path = filed(&library, "", &file);
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);

    // Without a stroke, the new layer is not written.
    std::thread::sleep(config().save_interval * 2);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(FileStamp::of(&path).unwrap(), stamp);
    // Nor with what leaves the ink as it was: clearing nothing (the ink is on a hidden layer), the
    // blackboard on and off.
    hub.command(LiveCommandKind::ClearPage).unwrap();
    press_board(&hub, true);
    press_board(&hub, false);
    std::thread::sleep(config().save_interval * 2);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(FileStamp::of(&path).unwrap(), stamp);
    assert_eq!(hub.snapshot().saved, Vec::<String>::new());
    // A viewer gets the layers as they are and none of the hidden strokes.
    let shared = batch_json(&super::super::model::LiveBatch {
        events: hub.current_events(),
        sent_at: 1.0,
    })
    .unwrap();
    let shared: serde_json::Value = serde_json::from_str(&shared).unwrap();
    let page = &shared["events"][0]["snapshot"]["page"];
    assert_eq!(page["strokes"], json!([]));
    assert_eq!(page["layers"].as_array().unwrap().len(), 3);

    stroke(&mut sink, 3000, 5);
    let saved = hub.stop().unwrap();
    let written = library.read(Path::new(&saved[0])).unwrap().file;
    assert_eq!(written.note.layers.len(), 3);
    assert!(!written.note.layers[0].visible && !written.note.layers[1].visible);
    assert!(written.note.layers[2].visible);
    assert_eq!(written.note.strokes[1].layer, 2);
    assert_eq!(
        written.note.strokes[0].layer, 1,
        "the hidden ink stays on its layer"
    );
}

#[test]
fn a_note_that_received_no_ink_stays_untouched_whatever_was_added_around_it() {
    let (_dir, library) = library();
    let mut file = full_file(A5);
    file.note.layers = vec![layer("Cours", false)];
    let path = filed(&library, "", &file);
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.command(LiveCommandKind::NewLayer).unwrap();
    assert_eq!(hub.snapshot().page.layers.len(), 3);
    hub.command(LiveCommandKind::ClearPage).unwrap();
    std::thread::sleep(config().save_interval * 2);
    assert_untouched(&hub, &path, &bytes, &stamp);
    let read = library.read(&path).unwrap().file;
    assert_eq!(read.original_note, None);
    assert_eq!(read.note.layers.len(), 1);

    // Ink, then taken back before the end: the file goes back to the layers it had.
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    wait_until("the ink written", || {
        stroke_count(&library, &path) == Some(3)
    });
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.layers.len(), 2);
    assert!(written.original_note.is_some());
    let snapshot = hub.snapshot();
    hub.edit_strokes(0, snapshot.revision, delete(&[2]))
        .unwrap();
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.layers.len(), 1);
    assert_eq!(written.original_note, None);
}

#[test]
fn a_running_session_is_written_out_before_the_resume_and_a_bad_note_leaves_it_running() {
    let (dir, library) = library();
    let hub = LiveHub::new(config());
    let activity = hub.activity();
    let mut first = {
        let Start::New { mut sink, .. } = hub
            .start(options(&library, fixed(LiveButton::NewPage)))
            .unwrap()
        else {
            panic!("already running");
        };
        sink.input(LiveInput::Started {
            width: A5.width,
            height: A5.height,
            orientation: Orientation::Landscape,
            kind: TabletKind::Slate,
        });
        sink
    };
    stroke(&mut first, 3000, 5);
    // A stroke still in progress when the sheet is resumed.
    send(&mut first, &[pen(5000, 1000, 500)]);
    let running = hub.page_position().unwrap();

    // A note that cannot be read: refused, and the session goes on.
    for bad in [Path::new("absent.ardoise"), Path::new("../dehors.ardoise")] {
        let result = hub.resume(options(&library, fixed(LiveButton::NewPage)), bad);
        assert!(matches!(result, Err(LiveError::Library(_))));
    }
    std::fs::write(dir.path().join("abimee.ardoise"), b"not a zip").unwrap();
    let result = hub.resume(
        options(&library, fixed(LiveButton::NewPage)),
        Path::new("abimee.ardoise"),
    );
    assert!(matches!(result, Err(LiveError::Library(_))));
    assert!(activity.live());
    assert_eq!(hub.page_position().unwrap(), running);

    // A good one: the session goes, stroke in progress included, before the sheet is read.
    let path = filed(&library, "Classe", &full_file(A5));
    let mut second = resume(&hub, &library, &path, A5);
    assert_ne!(hub.page_position().unwrap().session, running.session);
    let notes = notes_in(&library);
    let old = notes.iter().find(|p| **p != path).unwrap();
    assert_eq!(stroke_count(&library, old), Some(2));
    // Its input is ignored from now on.
    stroke(&mut first, 7000, 5);
    assert_eq!(hub.snapshot().page.strokes.len(), 2);
    stroke(&mut second, 3000, 5);
    assert_eq!(hub.snapshot().page.strokes.len(), 3);
    assert_eq!(hub.snapshot().page_count, 1);
    assert_eq!(hub.stop().unwrap(), [path_text(&path)]);
    assert_eq!(stroke_count(&library, old), Some(2));

    // Resuming again the note just written starts from what it holds now.
    let mut third = resume(&hub, &library, &path, A5);
    assert_eq!(hub.snapshot().page.strokes.len(), 3);
    assert_eq!(hub.snapshot().saved, Vec::<String>::new());
    stroke(&mut third, 3000, 5);
    assert_eq!(hub.stop().unwrap(), [path_text(&path)]);
    assert_eq!(stroke_count(&library, &path), Some(4));
}

#[test]
fn resuming_the_page_of_the_session_just_shown_finds_all_of_its_ink() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let Start::New { mut sink, .. } = hub
        .start(options(&library, fixed(LiveButton::NewPage)))
        .unwrap()
    else {
        panic!("already running");
    };
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
        kind: TabletKind::Slate,
    });
    stroke(&mut sink, 3000, 5);
    wait_until("the note", || notes_in(&library).len() == 1);
    let path = notes_in(&library).remove(0);
    // Not written yet (the save interval): it must be, before the note is read.
    stroke(&mut sink, 4000, 5);
    let _sink = resume(&hub, &library, &path, A5);
    assert_eq!(hub.snapshot().page.strokes.len(), 2);
    assert_eq!(
        hub.snapshot().page.id,
        library.read(&path).unwrap().file.note.id
    );
    hub.stop().unwrap();
}

#[test]
fn a_page_that_cannot_be_written_has_a_complete_backup_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Notes");
    std::fs::create_dir(&root).unwrap();
    ardoise_core::library::init(&root).unwrap();
    let library = Library::open(&root).unwrap();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    break_folder(&hub, &root, true);
    stroke(&mut sink, 3000, 5);
    let copies = hub.back_up_unsaved(&dir.path().join("backup"));
    assert_eq!(copies.len(), 1);
    let copy = ardoise_core::format::read_note(copies[0].1.as_ref().unwrap()).unwrap();
    assert_ne!(copy.note.id, before.note.id);
    assert_eq!(copy.note.strokes.len(), 3);
    assert_eq!(copy.background, before.background);
    assert_eq!(copy.audio, before.audio);
    assert_eq!(copy.text, before.text);
    assert_eq!(copy.unknown_fields, before.unknown_fields);
    assert_eq!(copy.unknown_members, before.unknown_members);
    assert_eq!(copy.note.background_image, before.note.background_image);
    assert!(copy.original_note.is_some());
    // Once the library is back the page goes to its note, which was not touched meanwhile.
    break_folder(&hub, &root, false);
    hub.stop().unwrap();
    assert_eq!(stroke_count(&library, &path), Some(3));
}

// --- Narration -----------------------------------------------------------------------------------

/// A note whose ink ends at 5 000 ms, with a narration of its own.
fn narrated(library: &Library, narration: &Narration) -> PathBuf {
    let mut file = NoteFile::new(sheet(A5));
    file.audio = Some(encode_narration(narration).unwrap());
    filed(library, "", &file)
}

/// Gives `seconds` of sound recorded on page 0 from wall clock `wall_start` to the hub's session.
fn narrate(hub: &LiveHub, wall_start: f64, seconds: u64) {
    let session = hub.page_position().unwrap().session;
    let clip = simulated_narration(Duration::from_secs(seconds), 1);
    let log = PageLog {
        changes: vec![change(Duration::ZERO, 0)],
        anchor_ms: None,
    };
    assert_eq!(
        give_to_hub(hub, session, &log, wall_start, clip).unwrap(),
        1
    );
}

#[test]
fn a_new_narration_joins_the_notes_own_after_it_on_the_notes_time() {
    let (_dir, library) = library();
    let path = narrated(&library, &note_narration(1.0, 0));
    let hub = LiveHub::new(config());
    let before = now_ms();
    let mut sink = resume(&hub, &library, &path, A5);
    let session = hub.page_position().unwrap().session;
    let base = 5_000 + i64::from(RESUME_GAP_MS);
    let origin = hub.page_origins(session).unwrap()[0].unwrap();
    assert!(
        (before - base..=now_ms() - base).contains(&origin),
        "{origin}"
    );

    // The seed: nothing else is written until something changes.
    stroke(&mut sink, 3000, 5);
    wait_until("the ink written", || {
        stroke_count(&library, &path) == Some(3)
    });
    let first_new = library.read(&path).unwrap().file.note.strokes[2].points[0].t;
    assert!(i64::from(first_new) >= base, "{first_new}");

    // 2 s of sound from now: in note time, on the same clock as the ink.
    let wall_start = now_ms();
    narrate(&hub, wall_start as f64, 2);
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&path)]);
    let merged = decode_narration(&audio_of(&library, &path).unwrap()).unwrap();
    assert_eq!(merged.base, NarrationBase::Note);
    assert_eq!(
        merged.start_ms, 0,
        "the note's narration is still there, from the start"
    );
    let expected_end = wall_start - origin + 2_000;
    assert!(
        (merged.end_ms() - expected_end).abs() <= 20,
        "{}",
        merged.end_ms()
    );
    // The old sound, silence (give or take what a lossy encoder leaves), the new sound.
    let new_part = usize::try_from(expected_end - 2_000).unwrap() * 48;
    let samples = &merged.clip.samples;
    let (old, gap, new) = (
        loudness(&samples[..48_000]),
        loudness(&samples[48_000 + 4_800..new_part - 4_800]),
        loudness(&samples[new_part..]),
    );
    assert!(gap * 10.0 < old && gap * 10.0 < new, "{old} {gap} {new}");
}

/// Root mean square of `samples`.
fn loudness(samples: &[i16]) -> f64 {
    let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    (sum / samples.len().max(1) as f64).sqrt()
}

#[test]
fn a_narration_recorded_in_a_faster_replay_keeps_its_time_base_and_its_sound() {
    let (_dir, library) = library();
    let mut own = note_narration(1.0, 0);
    own.base = NarrationBase::Replay;
    own.options = ReplayOptions {
        speed: 2.0,
        max_pause: None,
    };
    let path = narrated(&library, &own);
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    narrate(&hub, now_ms() as f64, 1);
    hub.stop().unwrap();
    let merged = decode_narration(&audio_of(&library, &path).unwrap()).unwrap();
    assert_eq!((merged.base, merged.options), (own.base, own.options));
    assert_eq!(merged.start_ms, 0);
    // 6 s of note time before the new sound last 3 s at speed 2, not 6.
    let end = merged.end_ms();
    assert!((4_000..5_000).contains(&end), "{end}");
}

#[test]
fn a_new_narration_too_far_from_the_notes_own_loses_neither() {
    let (_dir, library) = library();
    let mut file = NoteFile::new(sheet(A5));
    // Ink 10 hours after the note's first narration: one track cannot hold both at their places.
    file.note
        .strokes
        .push(ink(300.0, 300.0, &[10 * 3600 * 1000], 0));
    file.audio = Some(encode_narration(&note_narration(1.0, 0)).unwrap());
    let path = filed(&library, "", &file);
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    narrate(&hub, now_ms() as f64, 1);
    hub.stop().unwrap();
    let merged = decode_narration(&audio_of(&library, &path).unwrap()).unwrap();
    assert_eq!(merged.start_ms, 0);
    assert!(
        (1_990..2_100).contains(&merged.end_ms()),
        "{}",
        merged.end_ms()
    );
}

#[test]
fn a_new_sound_too_long_to_join_the_notes_own_comes_back_whole() {
    // 4 hours of silence, never touched (the pages are not even committed), then one second.
    let long = Narration {
        clip: ardoise_media::AudioClip {
            channels: 1,
            samples: vec![0; 4 * 3600 * 48_000],
        },
        ..note_narration(0.0, 0)
    };
    let later = note_narration(1.0, 4 * 3600 * 1000);
    assert!(!long.fits_with(&later));
    let kept = merged_keeping_both(long, later.clone()).unwrap_err();
    assert_eq!(kept.clip, later.clip);
}

/// A page with ink, and strokes for a second page, resumed from `path`: the hub with sound to give.
fn with_a_page_written(library: &Library, hub: &LiveHub, path: &Path) -> HubSink {
    let mut sink = resume(hub, library, path, A5);
    stroke(&mut sink, 3000, 5);
    wait_until("the ink written", || stroke_count(library, path) == Some(3));
    sink
}

#[test]
fn a_sound_that_cannot_join_a_narration_it_cannot_read_goes_in_a_new_note() {
    let (dir, library) = library();
    let mut file = full_file(A5);
    file.audio = Some(b"OggS damaged".to_vec());
    let path = filed(&library, "Classe", &file);
    let hub = LiveHub::new(config());
    let _sink = with_a_page_written(&library, &hub, &path);
    let session = hub.page_position().unwrap().session;
    let sound = note_narration(1.0, 8_000);
    let LiveError::NarrationKeptApart {
        why: NarrationApart::Unreadable,
        kept,
    } = hub.attach_narration(session, 0, sound.clone()).unwrap_err()
    else {
        panic!("not kept apart");
    };

    // Beside the note, with the page as it is and this sound, in time with its ink.
    assert_eq!(kept.parent(), Some(dir.path().join("Classe")).as_deref());
    assert_ne!(kept, path);
    let written = library.read(&kept).unwrap().file;
    let page = hub.snapshot().page;
    assert_eq!(written.note.strokes.len(), page.strokes.len());
    assert_ne!(written.note.id, page.id);
    assert_eq!(written.background, file.background);
    let got = decode_narration(written.audio.as_ref().unwrap()).unwrap();
    assert_eq!(got.start_ms, 8_000);
    let ms = got.duration().as_millis();
    assert!((990..=1010).contains(&ms), "{ms}");

    // The page and its own narration are as they were.
    hub.stop().unwrap();
    assert_eq!(audio_of(&library, &path), file.audio);
    assert_eq!(notes_in(&library).len(), 2);

    // Said to the user: the sentence and where the sound is.
    let error = LiveError::NarrationKeptApart {
        why: NarrationApart::Unreadable,
        kept: kept.clone(),
    }
    .command_error();
    assert_eq!(error.code(), "narrationUnreadable");
    assert_eq!(error.parts().1, path_text(&kept));
    assert_eq!(
        LiveError::NarrationLost {
            why: NarrationApart::TooLong,
            detail: "disk full".to_owned(),
        }
        .command_error()
        .code(),
        "narrationNotKept"
    );
}

#[test]
fn a_sound_kept_apart_has_its_place_in_note_time_on_a_page_of_the_session() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let Start::New { mut sink, .. } = hub
        .start(options(&library, fixed(LiveButton::NewPage)))
        .unwrap()
    else {
        panic!("already running");
    };
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
        kind: TabletKind::Slate,
    });
    stroke(&mut sink, 3000, 5);
    wait_until("the note", || notes_in(&library).len() == 1);
    let page_note = notes_in(&library).remove(0);
    let session = hub.page_position().unwrap().session;
    let error = hub.keep_apart(
        session,
        0,
        &note_narration(1.0, 2_000),
        NarrationApart::TooLong,
    );
    let LiveError::NarrationKeptApart { kept, .. } = &error else {
        panic!("{error}");
    };
    let kept = kept.clone();
    assert_eq!(error.command_error().code(), "narrationTooLong");
    assert_ne!(kept, page_note);
    let written = library.read(&kept).unwrap().file;
    assert_eq!(written.note.strokes.len(), 1);
    let got = decode_narration(written.audio.as_ref().unwrap()).unwrap();
    assert_eq!(got.start_ms, 2_000);
    hub.stop().unwrap();
}

#[test]
fn a_sound_that_goes_in_no_note_is_kept_as_a_copy_and_the_error_says_where() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Notes");
    std::fs::create_dir(&root).unwrap();
    ardoise_core::library::init(&root).unwrap();
    let library = Library::open(&root).unwrap();
    let mut file = full_file(A5);
    file.audio = Some(b"OggS damaged".to_vec());
    let path = filed(&library, "", &file);
    let backup = dir.path().join("narration-backup");
    let hub = LiveHub::new(config());
    hub.set_narration_backup(backup.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    wait_until("the ink written", || {
        stroke_count(&library, &path) == Some(3)
    });
    let session = hub.page_position().unwrap().session;

    break_folder(&hub, &root, true);
    let error = hub
        .attach_narration(session, 0, note_narration(1.0, 8_000))
        .unwrap_err();
    let LiveError::NarrationKeptApart { kept, .. } = &error else {
        panic!("{error}");
    };
    assert_eq!(kept.parent(), Some(backup.as_path()));
    let got = decode_narration(&std::fs::read(kept).unwrap()).unwrap();
    assert_eq!(got.start_ms, 8_000);

    // Nowhere to put it at all: the sound is lost, and that is said too.
    std::fs::remove_dir_all(&backup).unwrap();
    std::fs::write(&backup, b"not a folder").unwrap();
    let error = hub
        .attach_narration(session, 0, note_narration(1.0, 8_000))
        .unwrap_err();
    assert!(matches!(error, LiveError::NarrationLost { .. }), "{error}");
    break_folder(&hub, &root, false);
    hub.stop().unwrap();
}

#[test]
fn a_page_that_cannot_take_its_sound_does_not_cost_the_next_pages_theirs() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let Start::New { mut sink, .. } = hub
        .start(options(&library, fixed(LiveButton::NewPage)))
        .unwrap()
    else {
        panic!("already running");
    };
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
        kind: TabletKind::Slate,
    });
    stroke(&mut sink, 3000, 5);
    wait_until("the first note", || notes_in(&library).len() == 1);
    hub.command(LiveCommandKind::NextPage).unwrap();
    stroke(&mut sink, 4000, 5);
    wait_until("the second note", || notes_in(&library).len() == 2);
    let session = hub.page_position().unwrap().session;
    assert!(hub.set_page_audio(0, b"OggS damaged".to_vec()));

    let log = PageLog {
        changes: vec![
            change(Duration::ZERO, 0),
            change(Duration::from_millis(500), 1),
        ],
        anchor_ms: None,
    };
    let clip = simulated_narration(Duration::from_secs(1), 1);
    let error = give_to_hub(&hub, session, &log, now_ms() as f64, clip).unwrap_err();
    assert_eq!(error.code(), "narrationUnreadable");
    let saved = hub.stop().unwrap();
    // The second page has its piece; the first keeps what it had, its piece in a third note.
    let second = audio_of(&library, Path::new(&saved[1])).unwrap();
    let ms = decode_narration(&second).unwrap().duration().as_millis();
    assert!((490..=510).contains(&ms), "{ms}");
    assert_eq!(
        audio_of(&library, Path::new(&saved[0])),
        Some(b"OggS damaged".to_vec())
    );
    assert_eq!(notes_in(&library).len(), 3);
}

// --- From the simulated tablet, through the tablet service ---------------------------------------

/// The whole way: the simulated A4 tablet streams in real time through the service (which tells
/// its size only once it streams) onto an A5 sheet held upright, with an image, a narration, text.
#[test]
fn a_simulated_tablet_writes_on_a_sheet_of_another_size_held_the_other_way() {
    let (_dir, library) = library();
    let upright = PageSize {
        width: A5.height,
        height: A5.width,
    };
    let path = filed(&library, "Classe", &full_file(upright));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(HubConfig::default());
    let samples = stream_simulated_start(
        &hub,
        &library,
        2,
        || {},
        || {
            hub.resume(options(&library, fixed(LiveButton::NewPage)), &path)
                .unwrap()
        },
    );
    assert!(!samples.is_empty(), "no point reached the window");
    assert!(hub.snapshot().page.strokes.len() > 2);
    let written = library.read(&path).unwrap().file;
    assert_keeps_all_but_ink(&written, &before);
    assert_eq!(written.audio, before.audio);
    let new = &written.note.strokes[2..];
    assert!(!new.is_empty());
    let base = 5_000 + RESUME_GAP_MS;
    let (w, h) = (upright.width as f32, upright.height as f32);
    for point in new.iter().flat_map(|stroke| &stroke.points) {
        assert!(
            point.x >= 0.0 && point.x <= w && point.y >= 0.0 && point.y <= h,
            "{point:?}"
        );
        assert!(point.t >= base, "{point:?}");
    }
    assert_eq!(hub.snapshot().orientation, Orientation::PortraitLeft);
}

// --- What the interface, the share and the assistant see -----------------------------------------

#[test]
fn the_shapes_of_live_ts() {
    let event = LiveEvent::SheetForked {
        page_index: 0,
        reason: ForkReason::Deleted,
        original: "C:\\Notes\\Classe\\a.ardoise".into(),
        path: "C:\\Notes\\Classe\\b.ardoise".into(),
    };
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({
            "type": "sheetForked",
            "pageIndex": 0,
            "reason": "deleted",
            "original": "C:\\Notes\\Classe\\a.ardoise",
            "path": "C:\\Notes\\Classe\\b.ardoise",
        })
    );
    assert_eq!(
        serde_json::to_value(ForkReason::Changed).unwrap(),
        "changed"
    );

    // Nothing of it may leave the PC; alone in a batch, the batch is not even sent.
    let batch = super::super::model::LiveBatch {
        events: vec![event],
        sent_at: 1.0,
    };
    assert_eq!(batch_json(&batch), None);

    // `canTurnPage`: in the snapshot of the windows, not in the viewers'.
    let session = resumed(sheet(A5), Orientation::Landscape);
    let snapshot = serde_json::to_value(session.snapshot()).unwrap();
    assert_eq!(snapshot["canTurnPage"], false);
    assert!(keys(&snapshot).contains(&"canTurnPage"));
    let shared = batch_json(&super::super::model::LiveBatch {
        events: vec![LiveEvent::Snapshot {
            snapshot: Arc::new(session.snapshot()),
        }],
        sent_at: 1.0,
    })
    .unwrap();
    assert!(!shared.contains("canTurnPage"), "{shared}");
    // The note's title, tags, dates and background image stay on the PC, as for any live page.
    assert!(!shared.contains("Cours"), "{shared}");
    assert!(!shared.contains(&CREATED.to_string()), "{shared}");
}

#[test]
fn the_assistant_sees_the_note_once_the_session_wrote_it() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let id = library.read(&path).unwrap().file.note.id;
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    let overview = hub.overview().unwrap();
    assert!(overview.running);
    assert_eq!((overview.page, overview.pages), (0, 1));
    assert!(overview.notes.is_empty(), "nothing written yet");
    assert_eq!(overview.library.as_deref(), Some(library.root()));
    stroke(&mut sink, 3000, 5);
    hub.stop().unwrap();
    assert_eq!(hub.overview().unwrap().notes, [(0, id, path)]);
}

/// A draft left for the note (a crash in the editor) is based on the version before the resume:
/// the interface then offers it only as a new note (`recoveryOf`, `lib/editor/drafts.ts`), never
/// over what live mode wrote.
#[test]
fn a_draft_left_for_the_note_can_only_become_a_new_note_after_the_resume() {
    let (dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap();
    let mut edited = before.file.note.clone();
    edited.title = "Brouillon".to_owned();
    let record = DraftRecord {
        version: DRAFT_VERSION,
        library: path_text(library.root()),
        path: path_text(&path),
        id: before.file.note.id.clone(),
        saved_at: now_ms(),
        base_stamp: Some(before.stamp),
        session: "test".to_owned(),
        image: None,
        note: edited,
    };
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    stroke(&mut sink, 3000, 5);
    hub.stop().unwrap();

    let after = library.read(&path).unwrap();
    assert_ne!(
        Some(after.stamp),
        record.base_stamp,
        "the draft is no longer current"
    );
    let copy = restore_copy_in(&library, &record, None, "Brouillon").unwrap();
    assert_eq!(notes_in(&library).len(), 2);
    assert!(Path::new(&copy.path).starts_with(dir.path().join("Classe")));
    assert_eq!(library.read(&path).unwrap().file, after.file);
}

// --- Touching up a resumed sheet ------------------------------------------------------------------

const RED: Rgba = Rgba::rgb(200, 0, 0);

#[test]
fn edits_on_the_old_ink_of_a_sheet_keep_all_it_holds_through_a_real_save_and_reload() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;

    // The note's first stroke goes, the second takes a colour; the tablet writes meanwhile.
    let revision = hub.edit_strokes(0, revision, delete(&[0])).unwrap();
    stroke(&mut sink, 3000, 5);
    hub.edit_strokes(0, revision, recolour(&[0], RED)).unwrap();
    wait_until("the edits in the file", || {
        let written = library.read(&path).unwrap().file.note.strokes;
        written.len() == 2 && written[0].color == RED
    });
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&path)]);
    assert_eq!(notes_in(&library), std::slice::from_ref(&path));
    assert!(forks(&window).is_empty());

    let written = library.read(&path).unwrap().file;
    assert_eq!(
        written.note.strokes[0].points,
        before.note.strokes[1].points
    );
    assert_eq!(written.note.strokes[1].points[0].x, 3000.0);
    assert_keeps_all_but_ink(&written, &before);
    assert_eq!(written.audio, before.audio);
    // « Back to the original » gives the sheet as it was before the resume, colours included.
    let NoteOriginal::FirstVersion { strokes, .. } =
        original_of(&written, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    assert_eq!(strokes, before.note.strokes);
}

#[test]
fn a_change_of_colour_alone_is_a_change_of_the_sheet() {
    let (_dir, library) = library();
    let path = filed(&library, "", &full_file(A5));
    let before = library.read(&path).unwrap();
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;
    hub.edit_strokes(0, revision, recolour(&[1], RED)).unwrap();
    wait_until("the colour in the file", || {
        library.read(&path).unwrap().file.note.strokes[1].color == RED
    });
    let written = library.read(&path).unwrap();
    assert_ne!(
        file_fingerprint(&written.file),
        file_fingerprint(&before.file)
    );
    assert_ne!(written.stamp, before.stamp);
    assert!(written.file.original_note.is_some(), "an original is kept");
    assert_eq!(hub.snapshot().saved, [path_text(&path)]);
    hub.stop().unwrap();
}

#[test]
fn a_move_of_the_old_ink_of_a_sheet_is_a_change_of_the_sheet_and_its_undo_gives_it_back() {
    let (_dir, library) = library();
    let path = filed(&library, "", &full_file(A5));
    let before = library.read(&path).unwrap();
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;
    // The note's second stroke, bigger and further right: as for a change of colour, the moved
    // old ink is written into the note itself.
    let was = before.file.note.strokes[1].clone();
    hub.edit_strokes(0, revision, transform(&[1], 1.5, 250.0, -125.0))
        .unwrap();
    let expected = (
        f64::from(was.points[0].x) * 1.5 + 250.0,
        f64::from(was.width) * 1.5,
    );
    let near = |a: f32, b: f64| (f64::from(a) - b).abs() < 0.01;
    let moved_in_file = |loaded: &LoadedNote| {
        let stroke = &loaded.file.note.strokes[1];
        near(stroke.points[0].x, expected.0) && near(stroke.width, expected.1)
    };
    wait_until("the move in the file", || {
        library
            .read(&path)
            .is_ok_and(|loaded| moved_in_file(&loaded))
    });
    let written = library.read(&path).unwrap();
    assert_eq!(written.file.note.strokes[0], before.file.note.strokes[0]);
    let stroke = &written.file.note.strokes[1];
    assert_eq!(
        (stroke.tool, stroke.color, stroke.layer, stroke.points.len()),
        (was.tool, was.color, was.layer, was.points.len())
    );
    let times = |stroke: &Stroke| stroke.points.iter().map(|p| p.t).collect::<Vec<u32>>();
    assert_eq!(times(stroke), times(&was));
    assert_keeps_all_but_ink(&written.file, &before.file);
    assert_ne!(
        file_fingerprint(&written.file),
        file_fingerprint(&before.file)
    );
    assert!(written.file.original_note.is_some(), "an original is kept");
    assert_eq!(hub.snapshot().saved, [path_text(&path)]);

    // Undone: the sheet as it was read, with nothing to go back to.
    assert!(hub.undo_edit(0).unwrap());
    wait_until("the ink back in the file", || {
        library
            .read(&path)
            .is_ok_and(|loaded| loaded.file.note.strokes == before.file.note.strokes)
    });
    assert_eq!(hub.stop().unwrap(), [path_text(&path)]);
    let written = library.read(&path).unwrap().file;
    assert_keeps_all_but_ink(&written, &before.file);
    assert_eq!(written.original_note, None);
    assert_eq!(file_fingerprint(&written), file_fingerprint(&before.file));
}

#[test]
fn a_sheet_whose_only_change_was_undone_before_a_write_is_not_written() {
    let (_dir, library) = library();
    let path = filed(&library, "", &full_file(A5));
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;
    hub.change_at_once(|session, out| {
        let first = delete(&[0]).checked().unwrap();
        let revision = session.edit_strokes(0, revision, first, out).unwrap();
        let second = recolour(&[0], RED).checked().unwrap();
        let revision = session.edit_strokes(0, revision, second, out).unwrap();
        let third = transform(&[0], 2.0, -30.0, 40.0).checked().unwrap();
        session.edit_strokes(0, revision, third, out).unwrap();
        assert!(session.undo_edit(0, None, out).unwrap());
        assert!(session.undo_edit(0, None, out).unwrap());
        assert!(session.undo_edit(0, None, out).unwrap());
    });
    std::thread::sleep(config().save_interval * 2);
    assert_eq!(hub.snapshot().saved, Vec::<String>::new());
    assert_eq!(hub.stop().unwrap(), Vec::<String>::new());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(FileStamp::of(&path).unwrap(), stamp, "same modified date");
    assert!(hub.overview().unwrap().notes.is_empty());
}

#[test]
fn a_sheet_undone_back_after_a_write_is_written_again_as_it_was() {
    let (_dir, library) = library();
    let path = filed(&library, "", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;
    hub.edit_strokes(0, revision, delete(&[0])).unwrap();
    wait_until("the delete in the file", || {
        stroke_count(&library, &path) == Some(1)
    });
    assert!(library.read(&path).unwrap().file.original_note.is_some());
    assert!(hub.undo_edit(0).unwrap());
    wait_until("the ink back in the file", || {
        stroke_count(&library, &path) == Some(2)
    });
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&path)], "it was written all the same");
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes, before.note.strokes);
    assert_keeps_all_but_ink(&written, &before);
    // Nothing differs from the sheet before the resume: no original to go back to.
    assert_eq!(written.original_note, None);
    assert_eq!(file_fingerprint(&written), file_fingerprint(&before));
}

#[test]
fn a_note_changed_elsewhere_after_an_edit_is_left_alone_and_the_edit_goes_on_in_a_new_note() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let _sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;
    let revision = hub.edit_strokes(0, revision, delete(&[0])).unwrap();
    wait_until("the delete in the file", || {
        stroke_count(&library, &path) == Some(1)
    });
    wait_until("the page saved", || !hub.snapshot().saved.is_empty());

    // Only retitled elsewhere: the next edit goes in, and the title stays.
    let mut retitled = library.read(&path).unwrap();
    retitled.file.note.title = "Titre donné ailleurs".to_owned();
    library
        .save(&path, &retitled.file, &retitled.stamp)
        .unwrap();
    hub.edit_strokes(0, revision, recolour(&[0], RED)).unwrap();
    wait_until("the colour in the file", || {
        library
            .read(&path)
            .is_ok_and(|loaded| loaded.file.note.strokes[0].color == RED)
    });
    let same = library.read(&path).unwrap().file;
    assert_eq!(same.note.title, "Titre donné ailleurs");
    assert!(forks(&window).is_empty());
    // Past the write that follows, whose time is the page's own.
    std::thread::sleep(config().save_interval * 2);

    // Its ink changed elsewhere: left as it is, and the edit does not fight with it.
    let mut elsewhere = library.read(&path).unwrap();
    elsewhere.file.note.strokes.clear();
    library
        .save(&path, &elsewhere.file, &elsewhere.stamp)
        .unwrap();
    let kept = std::fs::read(&path).unwrap();
    assert!(hub.undo_edit(0).unwrap());
    wait_until("the notice", || !forks(&window).is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), kept, "not overwritten");
    let saved = hub.stop().unwrap();
    let new_note = library.read(Path::new(&saved[0])).unwrap().file;
    assert_eq!(new_note.note.strokes.len(), 1);
    assert_eq!(new_note.note.strokes[0].color, Rgba::BLACK, "with the undo");
    assert_eq!(std::fs::read(&path).unwrap(), kept);
}

#[test]
fn what_a_hidden_layer_holds_stays_in_the_file_through_edits_and_clearing() {
    let (_dir, library) = library();
    let mut file = full_file(A5);
    file.note.layers = vec![layer("Cours", true), layer("Réponses", false)];
    file.note.strokes = vec![
        ink(100.0, 100.0, &[0, 5], 1),
        ink(200.0, 100.0, &[10, 15], 0),
        ink(300.0, 100.0, &[20, 25], 1),
        ink(400.0, 100.0, &[30, 35], 0),
    ];
    let path = filed(&library, "", &file);
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let revision = hub.snapshot().revision;
    assert!(matches!(
        hub.edit_strokes(0, revision, delete(&[0])),
        Err(LiveError::Edit(LiveEditError::Hidden { index: 0 }))
    ));
    let revision = hub.edit_strokes(0, revision, delete(&[1])).unwrap();
    assert_eq!(hub.snapshot().revision, revision);
    hub.command(LiveCommandKind::ClearPage).unwrap();
    wait_until("the clearing in the file", || {
        stroke_count(&library, &path) == Some(2)
    });
    let in_file = library.read(&path).unwrap().file;
    assert!(in_file.note.strokes.iter().all(|s| s.layer == 1));
    // All back, in the order it was.
    assert!(hub.undo_edit(0).unwrap());
    assert!(hub.undo_edit(0).unwrap());
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes, before.note.strokes);
    assert_eq!(written.note.layers, before.note.layers);
}

#[test]
fn the_sheet_is_never_turned_and_what_can_come_back_stays_as_it_is() {
    let mut session = resumed(sheet(A5), Orientation::Landscape);
    let before = session.pages()[0].note.strokes.clone();
    let mut out = Vec::new();
    let revision = session.snapshot().revision;
    let edit = delete(&[0, 1]).checked().unwrap();
    session.edit_strokes(0, revision, edit, &mut out).unwrap();
    session.reorient(
        A4_LANDSCAPE.width,
        A4_LANDSCAPE.height,
        Orientation::PortraitLeft,
        &mut out,
    );
    assert!(session.undo_edit(0, None, &mut out).unwrap());
    assert_eq!(session.pages()[0].note.strokes, before);
}

#[test]
fn new_ink_goes_on_counting_time_after_the_last_stroke_was_deleted() {
    let mut session = resumed(sheet(A4_LANDSCAPE), Orientation::Landscape);
    let mut out = Vec::new();
    let revision = session.snapshot().revision;
    let edit = delete(&[1]).checked().unwrap();
    session.edit_strokes(0, revision, edit, &mut out).unwrap();
    // Written a minute later: still after what the note held, the deleted stroke included (an undo
    // brings it back, in the order of the times).
    let point = write_at(&mut session, 10, 10, RESUMED_AT + 60_000);
    assert_eq!(point.t, 5000 + RESUME_GAP_MS + 60_000);
    assert!(session.undo_edit(0, None, &mut out).unwrap());
    let times: Vec<u32> = session.pages()[0]
        .note
        .strokes
        .iter()
        .flat_map(|stroke| stroke.points.iter().map(|point| point.t))
        .collect();
    assert!(times.is_sorted(), "{times:?}");
}

// --- Correcting a resumed sheet on one's own layer (M14) -----------------------------------------

fn layer_op(value: serde_json::Value) -> crate::live::LayerOp {
    serde_json::from_value(value).unwrap()
}

#[test]
fn on_a_resumed_sheet_both_pens_start_on_the_last_visible_layer() {
    let mut note = sheet(A4_LANDSCAPE);
    note.layers = vec![
        layer("Cours", true),
        layer("Dessin", true),
        layer("Réponses", false),
    ];
    let session = resumed(note, Orientation::Landscape);
    let snapshot = session.snapshot();
    assert_eq!((snapshot.layer, snapshot.computer_layer), (1, 1));
    // Every layer hidden: the visible layer the resume adds, for both.
    let mut note = sheet(A4_LANDSCAPE);
    note.layers = vec![layer("Cours", false), layer("Réponses", false)];
    let session = resumed(note, Orientation::Landscape);
    let snapshot = session.snapshot();
    assert_eq!((snapshot.layer, snapshot.computer_layer), (2, 2));
}

#[test]
fn the_computer_writes_on_a_resumed_sheet_after_what_it_holds_and_keeps_the_slate_it_needs() {
    use crate::live::pen::tests::{at_once, begin, end};
    // The note's last point is at 5 000 ms: the time since the resume counts from 6 000.
    let mut session = resumed(sheet(A4_LANDSCAPE), Orientation::Landscape);
    let mut out = Vec::new();
    let at = RESUMED_AT as f64 + 1_000.0;
    session
        .computer_call(begin(1, 0, at_once(&[10.0])), at, &mut out)
        .unwrap();
    session
        .computer_call(end(1, at_once(&[20.0])), at + 40.0, &mut out)
        .unwrap();
    let stroke = session.pages()[0].note.strokes.last().unwrap();
    let times: Vec<u32> = stroke.points.iter().map(|p| p.t).collect();
    assert_eq!(
        times,
        [5000 + RESUME_GAP_MS + 1_000, 5000 + RESUME_GAP_MS + 1_040]
    );
    assert_eq!(
        session.pages()[0].note.created,
        CREATED,
        "the note's date stays"
    );
    // The tablet's next sample is not pushed by the computer's.
    let point = write_at(&mut session, 1, 1, RESUMED_AT + 1_010);
    assert_eq!(point.t, 5000 + RESUME_GAP_MS + 1_010);

    // Under the blackboard, the computer's ink keeps the slate like the tablet's does.
    let mut note = sheet(A4_LANDSCAPE);
    note.background.paper = Paper::Kraft;
    let mut session = resumed(note, Orientation::Landscape);
    set_board(&mut session, true);
    assert!(!session.pages()[0].ink_on_board());
    session
        .computer_call(begin(1, 0, at_once(&[10.0])), at, &mut out)
        .unwrap();
    session
        .computer_call(end(1, Vec::new()), at, &mut out)
        .unwrap();
    assert!(session.pages()[0].ink_on_board());
    assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
}

#[test]
fn the_computer_s_ink_on_a_resumed_sheet_keeps_all_its_file_holds() {
    use crate::live::pen::tests::{at_once, begin, end};
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    assert!(
        hub.computer_pen(begin(1, 0, at_once(&[300.0, 400.0])))
            .unwrap()
    );
    assert!(hub.computer_pen(end(1, Vec::new())).unwrap());
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&path)]);
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.strokes.len(), 3);
    assert_eq!(written.note.strokes[2].tool, Tool::FeltTip);
    assert_keeps_all_but_ink(&written, &before);
    assert!(
        written.original_note.is_some(),
        "the sheet before it, for « back to the original »"
    );
}

#[test]
fn a_deliberate_change_of_its_layers_is_a_change_of_the_sheet() {
    // Choosing where the pens act writes nothing.
    let (_dir, library) = library();
    let mut file = full_file(A5);
    file.note.layers = vec![layer("Cours", true), layer("Réponses", true)];
    let path = filed(&library, "", &file);
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.layers(0, layer_op(json!({ "kind": "computer", "layer": 0 })))
        .unwrap();
    hub.layers(0, layer_op(json!({ "kind": "tablet", "layer": 0 })))
        .unwrap();
    std::thread::sleep(config().save_interval * 2);
    assert_untouched(&hub, &path, &bytes, &stamp);

    // A layer renamed, without any ink: written, as in the editor.
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.layers(
        0,
        layer_op(json!({ "kind": "rename", "layer": 1, "name": "Correction" })),
    )
    .unwrap();
    wait_until("the name in the file", || {
        library
            .read(&path)
            .is_ok_and(|loaded| loaded.file.note.layers[1].name == "Correction")
    });
    assert_eq!(hub.snapshot().saved, [path_text(&path)]);
    hub.stop().unwrap();

    // On a sheet whose layers are all hidden, the layer the resume added is written with the one
    // added on purpose: the layers as they are.
    let mut file = full_file(A5);
    file.note.layers = vec![layer("Réponses", false)];
    let path = filed(&library, "", &file);
    let before = library.read(&path).unwrap().file.note;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.layers(0, layer_op(json!({ "kind": "add" }))).unwrap();
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&path)]);
    let written = library.read(&path).unwrap().file.note;
    assert_eq!(
        written.layers,
        [
            layer("Réponses", false),
            layer("Calque 2", true),
            layer("Calque 3", true)
        ]
    );
    assert_eq!(written.strokes, before.strokes, "its ink as it was");

    // A merge moves the strokes: a change of the sheet like any other.
    let mut file = full_file(A5);
    file.note.layers = vec![layer("Cours", true), layer("Réponses", false)];
    file.note.strokes[0].layer = 1;
    let path = filed(&library, "", &file);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.layers(0, layer_op(json!({ "kind": "merge", "layer": 1 })))
        .unwrap();
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file.note;
    assert_eq!(written.layers, [layer("Cours", true)]);
    let starts: Vec<f32> = written.strokes.iter().map(|s| s.points[0].x).collect();
    assert_eq!(starts, [200.0, 100.0], "the merged ink above");
}

// --- The paper and the image of a resumed sheet (M17) ---------------------------------------------

/// `width × height` pixels staged as the live window does: their key and member.
fn staged_image(staging: &Staging, width: u32, height: u32) -> (String, BackgroundFile) {
    let member = BackgroundFile {
        extension: "png".to_owned(),
        bytes: encode_png(&halves(width, height)).unwrap(),
    };
    let key = staging.put(Staged {
        file: member.clone(),
        width,
        height,
    });
    (key, member)
}

fn worksheet(page: PageSize, pixels: (u32, u32)) -> BackgroundImage {
    BackgroundImage {
        name: "exercices.pdf".to_owned(),
        pdf_page: Some(4),
        rect: ImageRect::fit(page, pixels, ImageFit::Contain),
        quarter_turns: 0,
        opacity: 1.0,
    }
}

fn seyes_white() -> Background {
    Background {
        paper: Paper::White,
        template: Template::Seyes,
        texture: false,
    }
}

#[test]
fn the_pixels_of_a_resumed_sheet_are_its_files() {
    let file = full_file(A5);
    let member = file.background.clone().unwrap();
    let mut session = resumed_file(file, Orientation::Landscape);
    let snapshot = session.snapshot();
    let pixels = snapshot.image_pixels.clone().unwrap();
    assert_eq!(pixels.key, key_of(&member.bytes));
    assert_eq!((pixels.width, pixels.height), (40, 20));
    assert!(!snapshot.can_turn_page);
    // Its own paper is its paper, under the blackboard too.
    assert_eq!(snapshot.page_paper, Paper::Cream);
    set_board(&mut session, true);
    assert_eq!(session.snapshot().page_paper, Paper::Cream);
    // A member that cannot be read: described, nothing drawn, nothing to key.
    let mut file = full_file(A5);
    file.background = Some(BackgroundFile {
        extension: "png".to_owned(),
        bytes: vec![0; 16],
    });
    let session = resumed_file(file, Orientation::Landscape);
    assert!(session.snapshot().page.background_image.is_some());
    assert_eq!(session.snapshot().image_pixels, None);
}

#[test]
fn a_sheet_whose_paper_alone_changed_is_written_with_it_and_keeps_no_original() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let lined = Background {
        paper: Paper::Kraft,
        template: Template::Lined,
        texture: false,
    };
    hub.set_page_paper(0, lined).unwrap();
    assert_eq!(hub.stop().unwrap(), [path_text(&path)]);
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.background, lined);
    assert_eq!(written.note.strokes, before.note.strokes);
    // Not a change of ink: nothing to go back to; the image stays as it was.
    assert_eq!(written.original_note, None);
    assert_eq!(written.original, before.original);
    assert_eq!(written.background, before.background);
    assert_eq!(written.note.background_image, before.note.background_image);
    // Chosen, then chosen back before a write: the sheet as read, not written again.
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.change_at_once(|session, out| {
        session
            .set_page_paper(0, before.note.background, out)
            .unwrap();
        session.set_page_paper(0, lined, out).unwrap();
    });
    std::thread::sleep(config().save_interval * 2);
    assert_untouched(&hub, &path, &bytes, &stamp);
}

#[test]
fn a_paper_chosen_under_the_blackboard_reaches_the_file_and_chalk_still_wants_slate() {
    let (_dir, library) = library();
    // Chosen while the blackboard is on, nothing written: the sheet gets the colour chosen, not
    // the slate shown.
    let (path, _, _) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    press_board(&hub, true);
    hub.set_page_paper(0, seyes_white()).unwrap();
    assert_eq!(hub.snapshot().page.background.paper, Paper::Slate);
    assert_eq!(hub.snapshot().page_paper, Paper::White);
    hub.stop().unwrap();
    assert_eq!(
        library.read(&path).unwrap().file.note.background,
        seyes_white()
    );

    // Chalk written under it: the slate it needs, whatever was chosen.
    let (path, _, _) = kraft_sheet(&library);
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    hub.set_page_paper(0, seyes_white()).unwrap();
    press_board(&hub, true);
    stroke(&mut sink, 3000, 5);
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file.note;
    assert_eq!(written.background.paper, Paper::Slate);
    assert_eq!(written.background.template, Template::Seyes);
}

#[test]
fn a_sheet_whose_image_changed_is_written_with_the_new_pixels_or_without_any() {
    let (_dir, library) = library();
    let staging = Staging::default();

    // Placed otherwise: the description changes, the pixels stay the file's.
    let path = filed(&library, "Classe", &full_file(A5));
    let before = library.read(&path).unwrap().file;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let faint = BackgroundImage {
        opacity: 0.3,
        ..before.note.background_image.clone().unwrap()
    };
    hub.change_page_image(
        0,
        LivePageImageOp::Place {
            image: faint.clone(),
        },
        &staging,
    )
    .unwrap();
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.background_image.as_ref(), Some(&faint));
    assert_eq!(written.background, before.background);
    assert_eq!(written.original_note, None, "not a change of ink");

    // Another image: its pixels and its description, ink or not.
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let (key, member) = staged_image(&staging, 20, 40);
    let image = worksheet(A5, (20, 40));
    hub.change_page_image(
        0,
        LivePageImageOp::Set {
            key: key.clone(),
            image: image.clone(),
        },
        &staging,
    )
    .unwrap();
    assert_eq!(hub.snapshot().image_pixels.unwrap().key, key);
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.background_image.as_ref(), Some(&image));
    assert_eq!(written.background.as_ref(), Some(&member));
    assert_eq!(written.note.strokes, before.note.strokes);
    assert_eq!(written.text, before.text, "the rest of the file kept");
    assert_eq!(written.unknown_members, before.unknown_members);

    // Removed: neither the description nor the member any more.
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    hub.change_page_image(0, LivePageImageOp::Remove, &staging)
        .unwrap();
    assert!(!hub.snapshot().can_turn_page, "a resumed sheet never turns");
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.background_image, None);
    assert_eq!(written.background, None);
    assert_eq!(written.note.background, before.note.background);
}

#[test]
fn a_sheet_changed_elsewhere_goes_on_in_a_new_note_with_its_paper_and_image() {
    let (_dir, library) = library();
    let staging = Staging::default();
    let path = filed(&library, "Classe", &full_file(A5));
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    let (key, member) = staged_image(&staging, 20, 40);
    let image = worksheet(A5, (20, 40));
    hub.change_page_image(
        0,
        LivePageImageOp::Set {
            key,
            image: image.clone(),
        },
        &staging,
    )
    .unwrap();
    hub.set_page_paper(0, seyes_white()).unwrap();
    // Written at once (a deliberate change of the sheet), then nothing until the next change.
    wait_until("the sheet written", || {
        library.read(&path).is_ok_and(|loaded| {
            loaded.file.note.background_image.as_ref() == Some(&image)
                && loaded.file.note.background == seyes_white()
        })
    });

    // Another PC changes its ink: the next write finds it changed.
    let mut elsewhere = library.read(&path).unwrap();
    elsewhere.file.note.strokes.pop();
    library
        .save(&path, &elsewhere.file, &elsewhere.stamp)
        .unwrap();
    let kept = std::fs::read(&path).unwrap();
    stroke(&mut sink, 3000, 5);
    wait_until("the notice", || !forks(&window).is_empty());
    let saved = hub.stop().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), kept, "left alone");
    let new_path = PathBuf::from(&saved[0]);
    assert_ne!(new_path, path);
    let written = library.read(&new_path).unwrap().file;
    assert_eq!(written.note.background_image.as_ref(), Some(&image));
    assert_eq!(written.background.as_ref(), Some(&member));
    assert_eq!(written.note.background, seyes_white());
    assert_eq!(written.note.strokes.len(), 3);
}

// --- The sheet and the tablet that writes on it (M18) ---------------------------------------------

#[test]
fn the_blackboard_writes_on_its_own_page_as_it_comes_and_only_there() {
    use Orientation::{Landscape, LandscapeFlipped, PortraitLeft};
    use ardoise_device::pages::{BLACKBOARD_PAGE, oriented_size};
    // Who may write where: the blackboard on its page only, the Slate everywhere else.
    assert!(tablet_fits(TabletKind::Blackboard, BLACKBOARD_PAGE));
    for sheet in [A4_LANDSCAPE, A4_PORTRAIT, A5] {
        assert!(!tablet_fits(TabletKind::Blackboard, sheet), "{sheet:?}");
        assert!(tablet_fits(TabletKind::Slate, sheet), "{sheet:?}");
    }
    assert!(!tablet_fits(TabletKind::Slate, BLACKBOARD_PAGE));
    // Every size the blackboard ever wrote with counts (a calibration adds one, keeps the old).
    use ardoise_device::pages::BLACKBOARD_PAGES;
    assert_eq!(BLACKBOARD_PAGES[0], BLACKBOARD_PAGE);
    for page in BLACKBOARD_PAGES {
        assert!(tablet_fits(TabletKind::Blackboard, page), "{page:?}");
        assert!(!tablet_fits(TabletKind::Slate, page), "{page:?}");
    }
    // A landscape page of the blackboard (held on a side, or turned in the editor) is the
    // blackboard's too, never the Slate's.
    let turned = PageSize {
        width: BLACKBOARD_PAGE.height,
        height: BLACKBOARD_PAGE.width,
    };
    assert!(tablet_fits(TabletKind::Blackboard, turned));
    assert!(!tablet_fits(TabletKind::Slate, turned));
    assert_eq!(
        held_for(BLACKBOARD_PAGE, Landscape, TabletKind::Blackboard),
        Landscape
    );
    assert_eq!(
        held_for(BLACKBOARD_PAGE, Landscape, TabletKind::Slate),
        PortraitLeft
    );

    // Its own page resumed with the blackboard: every record lands as it came, never turned nor
    // scaled, whatever the Slate is set to, and whatever area the session was made for.
    let board = BLACKBOARD_PAGE;
    for (setting, made_for) in [
        (Landscape, board),
        (Landscape, oriented_size(A4_LANDSCAPE, Landscape)),
        (PortraitLeft, oriented_size(A4_LANDSCAPE, PortraitLeft)),
        (LandscapeFlipped, board),
    ] {
        let mut session = LiveSession::new(
            1,
            made_for,
            LiveStyle::default(),
            LiveViewState::whole(made_for),
            true,
            RESUMED_AT,
        )
        .with_orientation(setting)
        .resuming(seed_of(NoteFile::new(sheet(board))), RESUMED_AT);
        let mut out = Vec::new();
        session.set_state(LiveSessionState::Waiting, &mut out);
        assert!(session.accepts(TabletKind::Blackboard));
        assert!(!session.accepts(TabletKind::Slate));
        session.set_tablet(TabletKind::Blackboard);
        session.started(board.width, board.height, Landscape, &mut out);
        for (x, y) in [(0, 0), (12_345, 20_000), (20_499, 27_199)] {
            let point = write_at(&mut session, x, y, RESUMED_AT + 1_000);
            assert_eq!(
                (point.x, point.y),
                (f32::from(x), f32::from(y)),
                "{setting:?}, {made_for:?}"
            );
        }
        assert_eq!(session.snapshot().orientation, Landscape, "not turned");
        assert_eq!(session.pages()[0].note.page, board);
    }

    // Its landscape page resumed with the blackboard held on a side (either: the service turns
    // its records for the position, `Started` gives its quarter turns): every record lands as it
    // came, on the sheet as it is. Held upright, the run is refused (the next test).
    use ardoise_core::settings::BoardPosition;
    use ardoise_device::pages::board_page;
    for position in [BoardPosition::ButtonsLeft, BoardPosition::ButtonsRight] {
        let held = Orientation::from_quarter_turns(position.quarter_turns());
        let sheet_page = board_page(position);
        let mut session = LiveSession::new(
            1,
            A4_LANDSCAPE,
            LiveStyle::default(),
            LiveViewState::whole(A4_LANDSCAPE),
            true,
            RESUMED_AT,
        )
        .with_orientation(Landscape)
        .resuming(seed_of(NoteFile::new(sheet(sheet_page))), RESUMED_AT);
        let mut out = Vec::new();
        session.set_state(LiveSessionState::Waiting, &mut out);
        assert_eq!(
            session.refusal(TabletKind::Blackboard, board.width, board.height, held),
            None,
            "{position:?}"
        );
        session.set_tablet(TabletKind::Blackboard);
        session.started(board.width, board.height, held, &mut out);
        for (x, y) in [(0, 0), (20_000, 12_345), (27_199, 20_499)] {
            let point = write_at(&mut session, x, y, RESUMED_AT + 1_000);
            assert_eq!(
                (point.x, point.y),
                (f32::from(x), f32::from(y)),
                "{position:?}"
            );
        }
        assert_eq!(
            session.pages()[0].note.page,
            sheet_page,
            "the sheet as it is"
        );
    }

    // A session that resumes nothing takes any tablet.
    let fresh = LiveSession::new(
        1,
        A4_LANDSCAPE,
        LiveStyle::default(),
        LiveViewState::whole(A4_LANDSCAPE),
        true,
        RESUMED_AT,
    );
    assert!(fresh.accepts(TabletKind::Blackboard) && fresh.accepts(TabletKind::Slate));
}

/// Resumes `path` and streams from a tablet of `kind`, whose area is `area` (not turned).
fn resume_on(
    hub: &LiveHub,
    library: &Library,
    path: &Path,
    area: PageSize,
    kind: TabletKind,
) -> HubSink {
    resume_held(hub, library, path, area, kind, Orientation::Landscape)
}

/// [`resume_on`], the tablet held as `held` (its quarter turns, as `Started` gives them).
fn resume_held(
    hub: &LiveHub,
    library: &Library,
    path: &Path,
    area: PageSize,
    kind: TabletKind,
    held: Orientation,
) -> HubSink {
    let start = StartOptions {
        size: area,
        ..options(library, fixed(LiveButton::NewPage))
    };
    let Start::New { mut sink, .. } = hub.resume(start, path).unwrap() else {
        panic!("a session is already running");
    };
    sink.input(LiveInput::Started {
        width: area.width,
        height: area.height,
        orientation: held,
        kind,
    });
    sink
}

/// Positions (docs/design/blackboard.md, « Positions »): a page of the blackboard is resumed with
/// the blackboard held its way round only, in the position set when the run starts (whichever of
/// the two of that way round, not necessarily the one it was made in: the user holds it as set
/// now); held the other way round the run ends at once (`sheetBoardPosition`), the note untouched.
#[test]
fn a_page_of_the_blackboard_is_resumed_only_with_the_blackboard_held_its_way_round() {
    use ardoise_core::settings::BoardPosition::{self, *};
    use ardoise_device::pages::{BLACKBOARD_PAGE, board_page};
    let held = |position: BoardPosition| Orientation::from_quarter_turns(position.quarter_turns());
    for (made, now, accepted) in [
        (ButtonsLeft, ButtonsLeft, true),
        (ButtonsLeft, ButtonsRight, true),
        (ButtonsRight, ButtonsLeft, true),
        (ButtonsBottom, ButtonsTop, true),
        (ButtonsLeft, ButtonsBottom, false),
        (ButtonsRight, ButtonsTop, false),
        (ButtonsBottom, ButtonsLeft, false),
        (ButtonsTop, ButtonsRight, false),
    ] {
        let (_dir, library) = library();
        let path = filed(&library, "Classe", &NoteFile::new(sheet(board_page(made))));
        let before = std::fs::read(library.root().join(&path)).unwrap();
        let hub = LiveHub::new(config());
        let window = Arc::new(Window::default());
        hub.subscribe("main", window.clone());
        let mut sink = resume_held(
            &hub,
            &library,
            &path,
            BLACKBOARD_PAGE,
            TabletKind::Blackboard,
            held(now),
        );
        let case = format!("made {made:?}, held {now:?}");
        assert_eq!(sink.active(), accepted, "{case}");
        if accepted {
            assert_eq!(hub.snapshot().state, LiveSessionState::Live, "{case}");
            stroke(&mut sink, 3000, 5);
            wait_until("the note rewritten", || {
                stroke_count(&library, &path) == Some(3)
            });
            let written = library.read(&path).unwrap().file;
            assert_eq!(
                written.note.page,
                board_page(made),
                "{case}: the sheet as it is"
            );
            let point = written.note.strokes[2].points[0];
            assert_eq!((point.x, point.y), (3000.0, 1000.0), "{case}: as it came");
            hub.stop().unwrap();
            continue;
        }
        wait_until("the refusal", || {
            window
                .events()
                .iter()
                .any(|event| matches!(event, LiveEvent::Refused { .. }))
        });
        let codes: Vec<String> = window
            .events()
            .into_iter()
            .filter_map(|event| match event {
                LiveEvent::Refused { code, .. } => Some(code),
                _ => None,
            })
            .collect();
        assert_eq!(codes, ["sheetBoardPosition"], "{case}");
        assert_eq!(hub.snapshot().state, LiveSessionState::Idle, "{case}");
        stroke(&mut sink, 3000, 5);
        hub.stop().unwrap();
        assert_eq!(
            std::fs::read(library.root().join(&path)).unwrap(),
            before,
            "{case}: the sheet untouched"
        );
        assert_eq!(notes_in(&library), std::slice::from_ref(&path), "{case}");
    }
}

#[test]
fn a_sheet_resumed_with_a_tablet_that_does_not_fit_it_ends_at_once_with_nothing_written() {
    use ardoise_device::pages::BLACKBOARD_PAGE;
    let slate = PageSize {
        width: 29_700,
        height: 21_600,
    };
    // A Slate sheet (landscape or turned) with the blackboard; a blackboard page with the Slate.
    for (sheet_page, area, kind) in [
        (A4_LANDSCAPE, BLACKBOARD_PAGE, TabletKind::Blackboard),
        (A4_PORTRAIT, BLACKBOARD_PAGE, TabletKind::Blackboard),
        (BLACKBOARD_PAGE, slate, TabletKind::Slate),
    ] {
        let (_dir, library) = library();
        let path = filed(&library, "Classe", &NoteFile::new(sheet(sheet_page)));
        let before = std::fs::read(library.root().join(&path)).unwrap();
        let hub = LiveHub::new(config());
        let window = Arc::new(Window::default());
        hub.subscribe("main", window.clone());
        let mut sink = resume_on(&hub, &library, &path, area, kind);
        // The service is told to leave live mode.
        assert!(!sink.active());
        wait_until("the refusal", || {
            window
                .events()
                .iter()
                .any(|event| matches!(event, LiveEvent::Refused { .. }))
        });
        let refused: Vec<(String, String)> = window
            .events()
            .into_iter()
            .filter_map(|event| match event {
                LiveEvent::Refused { code, message } => Some((code, message)),
                _ => None,
            })
            .collect();
        assert_eq!(refused.len(), 1, "{sheet_page:?} with the {}", kind.name());
        assert_eq!(refused[0].0, "sheetOtherTablet");
        assert_eq!(hub.snapshot().state, LiveSessionState::Idle);
        // Whatever comes after is not written anywhere.
        stroke(&mut sink, 3000, 5);
        assert_eq!(hub.snapshot().page.strokes.len(), 2);
        hub.stop().unwrap();
        assert_eq!(
            std::fs::read(library.root().join(&path)).unwrap(),
            before,
            "the sheet untouched"
        );
        assert_eq!(
            notes_in(&library),
            std::slice::from_ref(&path),
            "no other note"
        );
    }

    // The pairs that fit go on: a blackboard page with the blackboard, its ink as it came.
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &NoteFile::new(sheet(BLACKBOARD_PAGE)));
    let hub = LiveHub::new(config());
    let mut sink = resume_on(
        &hub,
        &library,
        &path,
        BLACKBOARD_PAGE,
        TabletKind::Blackboard,
    );
    assert!(sink.active());
    assert_eq!(hub.snapshot().state, LiveSessionState::Live);
    stroke(&mut sink, 3000, 5);
    wait_until("the note rewritten", || {
        stroke_count(&library, &path) == Some(3)
    });
    let written = library.read(&path).unwrap().file;
    let point = written.note.strokes[2].points[0];
    assert_eq!((point.x, point.y), (3000.0, 1000.0));
    hub.stop().unwrap();
}

// --- Text frames of a resumed sheet (format 1.4) -------------------------------------------------

/// A sheet with a text frame on each of two layers, the second one hidden; the visible frame shows
/// at 9 s in the replay, after the ink.
fn sheet_with_frames() -> Note {
    let mut note = sheet(A4_LANDSCAPE);
    note.layers.push(layer("Réponses", false));
    note.objects = vec![
        text_frame("consigne", 0, 9_000, 1_000.0, 500.0),
        text_frame("réponse", 1, 2_000, 1_000.0, 8_000.0),
    ];
    note
}

/// (id, layer) of every page object of `note`, in list order.
fn frames_of(note: &Note) -> Vec<(String, u32)> {
    note.objects
        .iter()
        .map(|object| (object.id.clone(), object.layer))
        .collect()
}

/// The text frames of a resumed sheet show (the control window gets them all, a hidden one with
/// its hidden layer), stay where they are when the tablet is switched (the sheet never turns) and
/// through « Clear the page » (only strokes go, and come back), and the new ink comes after the
/// last of them in the replay.
#[test]
fn a_resumed_sheet_keeps_its_text_frames_in_place() {
    let note = sheet_with_frames();
    assert_eq!(time_base(&note, None), 9_000 + RESUME_GAP_MS);
    let mut session = resumed(note.clone(), Orientation::Landscape);
    assert_eq!(session.snapshot().page.objects, note.objects);
    let point = write_at(&mut session, 10, 10, RESUMED_AT + 2_000);
    assert_eq!(point.t, 9_000 + RESUME_GAP_MS + 2_000);

    // The quick switch to the other orientations: neither the sheet nor its frames move.
    let mut out = Vec::new();
    for orientation in [
        Orientation::PortraitLeft,
        Orientation::LandscapeFlipped,
        Orientation::PortraitRight,
        Orientation::Landscape,
    ] {
        session.reorient(29_700, 21_600, orientation, &mut out);
        let page = &session.pages()[0];
        assert_eq!(page.note.objects, note.objects, "{orientation:?}");
        assert_eq!(page.note.page, note.page, "{orientation:?}");
        assert_eq!(session.snapshot().page.objects, note.objects);
    }

    // Cleared: every visible stroke goes, the frames stay (on both layers); undone: the strokes
    // come back, the frames are still there.
    session.command(
        LiveCommandKind::ClearPage,
        LiveButton::NewPage,
        RESUMED_AT + 3_000,
        &mut out,
    );
    let page = &session.pages()[0];
    assert!(page.note.strokes.is_empty());
    assert_eq!(page.note.objects, note.objects);
    assert_eq!(session.snapshot().page.objects, note.objects);
    assert!(session.restore_cleared_with(None, &mut out));
    assert_eq!(session.pages()[0].note.strokes.len(), 3);
    assert_eq!(session.pages()[0].note.objects, note.objects);
    // The blackboard's screen erased: the frames were never on it; with ink, a new page as ever.
    session.screen_erased(RESUMED_AT + 4_000, &mut out);
    assert_eq!(session.pages().len(), 2);
    assert_eq!(session.pages()[0].note.objects, note.objects);
    assert!(
        session.pages()[1].note.objects.is_empty(),
        "a new page has none"
    );
}

/// A sheet that holds only text frames (a worksheet typed in the editor) is resumed like any other:
/// nothing written while nothing changes; then written with its frames and the new ink, which comes
/// after them in the replay, the frames alone kept as its first version.
#[test]
fn a_sheet_of_text_frames_only_is_resumed_written_and_kept() {
    let (_dir, library) = library();
    let mut worksheet = blank_note("Exercices", "Calque 1", CREATED);
    worksheet.page = A5;
    worksheet.objects = vec![text_frame("consigne", 0, 0, 1_000.0, 500.0)];
    let path = filed(&library, "Classe", &NoteFile::new(worksheet));
    let before = library.read(&path).unwrap().file;
    let (bytes, stamp) = (std::fs::read(&path).unwrap(), FileStamp::of(&path).unwrap());
    let hub = LiveHub::new(config());
    let mut sink = resume(&hub, &library, &path, A5);
    assert_eq!(hub.snapshot().page.objects, before.note.objects);
    // Its blackboard screen erased before any ink: the sheet stays on screen.
    hub.change_at_once(|session, out| session.screen_erased(now_ms(), out));
    assert_eq!(hub.snapshot().page_count, 1);
    std::thread::sleep(config().save_interval * 2);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "nothing changed, nothing written"
    );
    assert_eq!(FileStamp::of(&path).unwrap(), stamp);

    stroke(&mut sink, 3000, 5);
    wait_until("the first write", || {
        stroke_count(&library, &path) == Some(1)
    });
    hub.stop().unwrap();
    let written = library.read(&path).unwrap().file;
    assert_eq!(written.note.objects, before.note.objects);
    assert!(written.note.strokes[0].points[0].t >= RESUME_GAP_MS);
    assert_eq!(notes_in(&library), std::slice::from_ref(&path));
    // Its first version: the frames alone, which « Back to the original » gives back.
    let restored = back_to_the_original(&library, &path);
    assert!(restored.note.strokes.is_empty());
    assert_eq!(restored.note.objects, before.note.objects);
}

/// The layers zone of the live window on a sheet with text frames: a move and a merge take the
/// frames with their layers (a merge puts the merged layer's after the lower layer's, as the
/// editor does), and the sheet is written so; their places on the page do not change. The sheet
/// was retitled from the library meanwhile: it receives the page all the same, frames included
/// (`hub::merge_outside`), and stays one note.
#[test]
fn the_text_frames_of_a_resumed_sheet_follow_its_layers() {
    let (_dir, library) = library();
    let path = filed(&library, "", &NoteFile::new(sheet_with_frames()));
    let before = library.read(&path).unwrap().file.note;
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A4_LANDSCAPE);
    let path = library.retitle(&path, "Optique", None).unwrap().path;
    // « Réponses » goes under « Calque 1 »: their frames swap layers, in the same order.
    hub.layers(0, layer_op(json!({ "kind": "move", "from": 1, "to": 0 })))
        .unwrap();
    assert_eq!(
        frames_of(&hub.snapshot().page),
        [("consigne".to_owned(), 1), ("réponse".to_owned(), 0)]
    );
    // « Calque 1 » merged into « Réponses »: its frame after the other one.
    hub.layers(0, layer_op(json!({ "kind": "merge", "layer": 1 })))
        .unwrap();
    let expected = [("réponse".to_owned(), 0), ("consigne".to_owned(), 0)];
    assert_eq!(frames_of(&hub.snapshot().page), expected);
    hub.stop().unwrap();
    assert_eq!(notes_in(&library), std::slice::from_ref(&path), "one note");
    let written = library.read(&path).unwrap().file.note;
    assert_eq!(written.title, "Optique");
    assert_eq!(written.layers.len(), 1);
    assert_eq!(frames_of(&written), expected);
    for object in &written.objects {
        let was = before.objects.iter().find(|o| o.id == object.id).unwrap();
        assert_eq!((object.x, object.y, object.t), (was.x, was.y, was.t));
    }
}

/// What live mode compares to know that a note changed elsewhere sees its text frames: moved,
/// retyped, added or taken away; not a box the writer measures again, nor a write and a read.
#[test]
fn the_fingerprint_sees_a_change_of_the_text_frames_alone() {
    let file = full_file(A5);
    let print = file_fingerprint(&file).unwrap();
    let with = |change: &dyn Fn(&mut NoteFile)| {
        let mut changed = file.clone();
        change(&mut changed);
        file_fingerprint(&changed).unwrap()
    };
    assert_ne!(with(&|f| f.note.objects[0].x += 1.0), print);
    assert_ne!(with(&|f| f.note.objects[0].t += 1), print);
    assert_ne!(with(&|f| f.note.objects[0].quarter_turns = 1), print);
    assert_ne!(
        with(&|f| {
            if let ardoise_core::note::ObjectBody::Text(body) = &mut f.note.objects[0].body {
                body.text.push('!');
            }
        }),
        print
    );
    assert_ne!(with(&|f| f.note.objects.clear()), print);
    assert_ne!(
        with(&|f| {
            let mut other = f.note.objects[0].clone();
            other.id = "autre".to_owned();
            f.note.objects.push(other);
        }),
        print
    );
    assert_eq!(with(&|f| f.note.objects[0].width = 0.0), print);
    let back =
        ardoise_core::format::read_from_bytes(&ardoise_core::format::to_bytes(&file).unwrap())
            .unwrap();
    assert_eq!(file_fingerprint(&back), Some(print));
}

/// A text frame moved on another PC, nothing else changed: the page is not written over it (nor
/// merged into it as a mere retitling) but goes on as a new note beside it.
#[test]
fn a_text_frame_moved_elsewhere_is_not_overwritten() {
    let (_dir, library) = library();
    let path = filed(&library, "Classe", &full_file(A5));
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = resume(&hub, &library, &path, A5);
    let mut elsewhere = library.read(&path).unwrap();
    elsewhere.file.note.objects[0].x += 2_000.0;
    library
        .save(&path, &elsewhere.file, &elsewhere.stamp)
        .unwrap();
    let kept = std::fs::read(&path).unwrap();

    stroke(&mut sink, 3000, 5);
    wait_until("the notice", || !forks(&window).is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), kept, "not overwritten");
    assert_eq!(forks(&window)[0].1, ForkReason::Changed);
    // The new note has the page's frame, where the page has it.
    let new_path = PathBuf::from(&forks(&window)[0].3);
    let written = library.read(&new_path).unwrap().file.note;
    assert_eq!(
        written.objects[0].x,
        elsewhere.file.note.objects[0].x - 2_000.0
    );
    hub.stop().unwrap();
}
