//! Tests of a session that resumes a note: the session alone (the pen on the sheet, its time, its
//! layers, its paper), and the hub with a real library (what is written, kept, refused).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ardoise_core::format::{BackgroundFile, Member, NoteFile};
use ardoise_core::library::{FileStamp, Library, LoadedNote};
use ardoise_core::note::{
    A4_LANDSCAPE, A4_PORTRAIT, Background, BackgroundImage, ImageFit, ImageRect, Layer, Note,
    Origin, PageSize, Paper, Template,
};
use ardoise_core::settings::{LiveButton, Orientation, Settings};
use ardoise_device::pages::orient;
use ardoise_device::service::{LiveInput, LiveSink};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_media::microphone::simulated_narration;
use ardoise_media::replay::ReplayOptions;
use ardoise_media::{Narration, NarrationBase, decode_narration, encode_narration};
use ardoise_proto::live::LiveRecord;
use ardoise_render::encode_png;
use serde_json::json;

use super::super::edits::LiveEditError;
use super::super::edits::tests::{delete, recolour};
use super::super::hub::{
    HubConfig, HubSink, LiveError, LiveHub, Start, StartOptions, file_fingerprint,
};
use super::super::model::{
    ForkReason, LiveCommandKind, LiveEvent, LiveSessionState, LiveStyle, LiveViewState,
};
use super::super::session::LiveSession;
use super::super::tests::{
    A5, Window, audio_of, break_folder, change, config, fixed, keys, note_narration, notes_in,
    options, pen, send, stream_simulated_start, stroke, stroke_count, wait_until,
};
use super::{
    PenMapping, RESUME_GAP_MS, ResumeSeed, hold_for, merged_keeping_both, placed_for_merge,
    time_base,
};
use crate::background::tests::halves;
use crate::drafts::{DRAFT_VERSION, DraftRecord, restore_copy_in};
use crate::library::tests::library;
use crate::library::{blank_note, now_ms, path_text};
use crate::narration::{PageLog, give_to_hub};
use crate::original::{NoteOriginal, TabletContext, original_of};
use crate::share::feed::batch_json;

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

/// A file holding everything a note file can hold, on a sheet of `page`.
fn full_file(page: PageSize) -> NoteFile {
    let mut note = sheet(page);
    note.tags = vec!["4e".to_owned()];
    note.favorite = true;
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
    let merged = merged_keeping_both(earlier.clone(), far);
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
    // A new page made while the blackboard is on goes back to white, as any page of a session.
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
    assert_eq!(session.pages()[1].note.background.paper, Paper::White);
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
    assert_eq!((a.timing, &a.origin), (b.timing, &b.origin));
    assert_eq!(*original, before.original);
    assert_eq!(*background, before.background);
    assert_eq!(*text, before.text);
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
        background_image,
    } = original_of(&written, &tablet_context()).unwrap()
    else {
        panic!("no first version");
    };
    assert_eq!(strokes, before.note.strokes);
    assert_eq!(
        (page, layers),
        (before.note.page, before.note.layers.clone())
    );
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

    // A blank note has nothing to go back to.
    let blank = NoteFile::new(blank_note("Vide", "Calque 1", CREATED));
    let path = filed(&library, "", &blank);
    let written = write_once(&path);
    assert_eq!(written.note.strokes.len(), 1);
    assert_eq!(written.original_note, None);
    assert_eq!(
        original_of(&written, &tablet_context()).unwrap(),
        NoteOriginal::None
    );

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
    let saved = hub.stop().unwrap();
    assert_eq!(saved, [path_text(&new_path)]);
    assert_eq!(forks(&window).len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), kept);
    assert_eq!(hub.snapshot().page.id, written.note.id);
    assert_eq!(hub.overview().unwrap().notes[0].2, new_path);

    // A window opened later is told too.
    let late = Arc::new(Window::default());
    hub.subscribe("presentation", late.clone());
    wait_until("the notice in the late window", || forks(&late).len() == 1);
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

#[test]
fn the_blackboard_on_a_resumed_note_is_written_and_gives_its_paper_back() {
    let (_dir, library) = library();
    let mut file = full_file(A5);
    file.note.background.paper = Paper::Kraft;
    let path = filed(&library, "", &file);
    let hub = LiveHub::new(config());
    let _sink = resume(&hub, &library, &path, A5);
    let paper = || library.read(&path).unwrap().file.note.background.paper;
    let mut view = hub.snapshot().view;
    view.board = true;
    hub.set_view(view).unwrap();
    wait_until("the slate written", || paper() == Paper::Slate);
    view.board = false;
    hub.set_view(view).unwrap();
    wait_until("the kraft written", || paper() == Paper::Kraft);
    hub.stop().unwrap();
    // Only the ink counts for the original: the paper is what the sheet has.
    assert_eq!(library.read(&path).unwrap().file.original_note, None);
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
    break_folder(&root, true);
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
    break_folder(&root, false);
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
            snapshot: Box::new(session.snapshot()),
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
        session.edit_strokes(0, revision, second, out).unwrap();
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
