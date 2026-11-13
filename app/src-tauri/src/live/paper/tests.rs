//! The paper and the image of the live page (docs/design/paper-projection-narration.md §1.8, package
//! A): the checks, the decoded pictures, what the session does with each operation, and what the hub
//! writes.

use std::sync::Arc;

use ardoise_core::format::BackgroundFile;
use ardoise_core::note::{
    Background, BackgroundImage, ImageFit, ImageRect, PageSize, Paper, Template,
};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_device::pages::oriented_size;
use ardoise_proto::live::LiveRecord;
use ardoise_render::{NoteView, Picture, RenderOptions, decode_png, encode_png, render_note};

use super::super::hub::{LiveError, LiveHub};
use super::super::model::{LiveCommandKind, LiveEvent, LiveSessionState, LiveStyle, LiveViewState};
use super::super::resume::PenMapping;
use super::super::session::LiveSession;
use super::super::tests::{
    A5, Window, break_folder, config, fixed, notes_in, options, pen, start, stroke, wait_until,
};
use super::{
    DECODED_KEPT, DecodedPictures, ImageChange, LivePageImageOp, MAX_IMAGE_NAME, PageImage,
    PaperError, checked_image, current_fit, refitted, within_reach,
};
use crate::background::tests::halves;
use crate::background::{Staged, Staging, key_of};
use crate::library::tests::library;

const T0: f64 = 1_792_843_200_000.0;

/// A5 held in portrait.
const A5_PORTRAIT: PageSize = PageSize {
    width: A5.height,
    height: A5.width,
};

/// The PNG member of a `width × height` image, red on its left half and blue on its right.
fn member(width: u32, height: u32) -> BackgroundFile {
    BackgroundFile {
        extension: "png".to_owned(),
        bytes: encode_png(&halves(width, height)).unwrap(),
    }
}

fn pixels_of(width: u32, height: u32) -> Arc<PageImage> {
    Arc::new(PageImage::of_member(&member(width, height)).unwrap())
}

/// The image `name` placed on `page` with `fit`.
fn placed(name: &str, page: PageSize, pixels: (u32, u32), fit: ImageFit) -> BackgroundImage {
    BackgroundImage {
        name: name.to_owned(),
        pdf_page: None,
        rect: ImageRect::fit(page, pixels, fit),
        quarter_turns: 0,
        opacity: 1.0,
    }
}

/// A session on an A5 tablet held in landscape, streaming.
fn running() -> LiveSession {
    let mut session = LiveSession::new(
        1,
        A5,
        LiveStyle::default(),
        LiveViewState::whole(A5),
        true,
        T0 as i64 - 60_000,
    );
    let mut out = Vec::new();
    session.set_state(LiveSessionState::Waiting, &mut out);
    session.started(A5.width, A5.height, Orientation::Landscape, &mut out);
    assert_eq!(session.state(), LiveSessionState::Live);
    session
}

fn snapshots(events: &[LiveEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, LiveEvent::Snapshot { .. }))
        .count()
}

fn board(session: &mut LiveSession, on: bool) {
    let view = LiveViewState {
        board: on,
        ..session.view()
    };
    session.set_view(view, &mut Vec::new());
}

fn command(session: &mut LiveSession, kind: LiveCommandKind) {
    session.command(kind, LiveButton::NewPage, T0 as i64, &mut Vec::new());
}

/// One point of pressure then the pen lifted, at `at` (Unix ms).
fn write_at(session: &mut LiveSession, x: u16, y: u16, at: f64) {
    let mut out = Vec::new();
    session.records(&[pen(x, y, 500)], at, &mut out);
    session.records(&[LiveRecord::ProximityOut], at + 5.0, &mut out);
}

fn seyes(paper: Paper) -> Background {
    Background {
        paper,
        template: Template::Seyes,
        texture: true,
    }
}

fn set_image(session: &mut LiveSession, image: BackgroundImage, pixels: Arc<PageImage>) {
    let page = session.current();
    session
        .set_page_image(page, ImageChange::Set { image, pixels }, &mut Vec::new())
        .unwrap();
}

// --- The checks ----------------------------------------------------------------------------------

#[test]
fn an_image_that_makes_no_sense_is_refused_and_the_rest_kept_within_bounds() {
    let good = placed("fiche.pdf", A5, (40, 20), ImageFit::Contain);
    assert_eq!(checked_image(good.clone()), Ok(good.clone()));
    let with = |change: fn(&mut BackgroundImage)| {
        let mut image = good.clone();
        change(&mut image);
        checked_image(image)
    };
    assert_eq!(with(|i| i.name.clear()), Err(PaperError::BadName));
    assert_eq!(
        with(|i| i.name = "é".repeat(MAX_IMAGE_NAME + 1)),
        Err(PaperError::BadName)
    );
    assert!(with(|i| i.name = "é".repeat(MAX_IMAGE_NAME)).is_ok());
    assert_eq!(with(|i| i.pdf_page = Some(0)), Err(PaperError::BadPdfPage));
    assert!(with(|i| i.pdf_page = Some(3)).is_ok());
    let bad_places: [fn(&mut BackgroundImage); 5] = [
        |i| i.rect.x = f32::NAN,
        |i| i.rect.y = f32::INFINITY,
        |i| i.rect.width = 0.0,
        |i| i.rect.height = -5.0,
        |i| i.rect.width = f32::NEG_INFINITY,
    ];
    for bad in bad_places {
        assert_eq!(with(bad), Err(PaperError::BadRect));
    }
    assert_eq!(with(|i| i.opacity = f32::NAN), Err(PaperError::BadOpacity));
    let clamped = with(|i| {
        i.opacity = 7.0;
        i.quarter_turns = 6;
    })
    .unwrap();
    assert_eq!((clamped.opacity, clamped.quarter_turns), (1.0, 2));
    assert_eq!(with(|i| i.opacity = -1.0).unwrap().opacity, 0.0);

    // Beyond the page, within 16 page sizes on each side.
    let reach = |x: f32, y: f32, width: f32, height: f32| {
        within_reach(
            &ImageRect {
                x,
                y,
                width,
                height,
            },
            A5,
        )
    };
    let (w, h) = (A5.width as f32, A5.height as f32);
    assert!(reach(-16.0 * w, -16.0 * h, 33.0 * w, 33.0 * h));
    assert!(!reach(-16.5 * w, 0.0, w, h));
    assert!(!reach(0.0, -17.0 * h, w, h));
    assert!(!reach(0.0, 0.0, 17.5 * w, h));
    assert!(!reach(0.0, 0.0, w, 18.0 * h));
    // A rectangle of huge numbers does not overflow into a pass.
    assert!(!reach(f32::MAX, 0.0, f32::MAX, h));

    // The codes of the interface.
    assert_eq!(PaperError::Stale.code(), "liveEditStale");
    assert_eq!(PaperError::Missing("k".into()).code(), "backgroundMissing");
    for invalid in [
        PaperError::BadName,
        PaperError::BadPdfPage,
        PaperError::BadRect,
        PaperError::BadOpacity,
        PaperError::NotThisImage,
    ] {
        assert_eq!(invalid.code(), "liveEditInvalid");
    }
    assert_eq!(
        LiveError::Paper(PaperError::Missing("abc-12".into()))
            .command_error()
            .code(),
        "backgroundMissing"
    );
}

#[test]
fn the_fit_of_an_image_is_recognised_and_kept_on_a_page_turned() {
    let pixels = (40, 20);
    for fit in [ImageFit::Contain, ImageFit::Cover, ImageFit::Stretch] {
        let image = placed("demi.png", A5, pixels, fit);
        assert_eq!(current_fit(&image, A5, pixels), Some(fit));
        let rect = refitted(&image, A5, A5_PORTRAIT, pixels);
        assert_eq!(rect, ImageRect::fit(A5_PORTRAIT, pixels, fit), "{fit:?}");
    }
    // Contained on a portrait page: as wide as the page, centred.
    let contained = refitted(
        &placed("demi.png", A5, pixels, ImageFit::Contain),
        A5,
        A5_PORTRAIT,
        pixels,
    );
    assert_eq!(
        (contained.x, contained.width, contained.height),
        (0.0, 14_800.0, 7_400.0)
    );
    assert_eq!(contained.y, (21_600.0 - 7_400.0) / 2.0);
    // Turned a quarter by the Fond tab: its own turns are kept, the fit counts them.
    let turned = BackgroundImage {
        quarter_turns: 1,
        rect: ImageRect::fit(A5, (20, 40), ImageFit::Cover),
        ..placed("demi.png", A5, pixels, ImageFit::Cover)
    };
    assert_eq!(current_fit(&turned, A5, pixels), Some(ImageFit::Cover));
    assert_eq!(
        refitted(&turned, A5, A5_PORTRAIT, pixels),
        ImageRect::fit(A5_PORTRAIT, (20, 40), ImageFit::Cover)
    );
    // An image of the page's proportions fits all three ways: contained on the page turned.
    let page_like = placed("a5.png", A5, (216, 148), ImageFit::Stretch);
    assert_eq!(
        current_fit(&page_like, A5, (216, 148)),
        Some(ImageFit::Contain)
    );
    // Placed by hand: contained.
    let by_hand = BackgroundImage {
        rect: ImageRect {
            x: 100.0,
            y: 200.0,
            width: 3000.0,
            height: 1500.0,
        },
        ..placed("demi.png", A5, pixels, ImageFit::Contain)
    };
    assert_eq!(current_fit(&by_hand, A5, pixels), None);
    assert_eq!(
        refitted(&by_hand, A5, A5_PORTRAIT, pixels),
        ImageRect::fit(A5_PORTRAIT, pixels, ImageFit::Contain)
    );
}

#[test]
fn a_page_image_knows_its_size_and_key_without_decoding() {
    let member = member(40, 20);
    let image = PageImage::of_member(&member).unwrap();
    assert_eq!((image.width, image.height), (40, 20));
    assert_eq!(image.key, key_of(&member.bytes));
    assert_eq!(image.member, member);
    let pixels = image.pixels();
    assert_eq!(
        (pixels.key, pixels.width, pixels.height),
        (image.key.clone(), 40, 20)
    );
    // Not an image: nothing to draw.
    let junk = BackgroundFile {
        extension: "png".to_owned(),
        bytes: vec![1, 2, 3],
    };
    assert!(PageImage::of_member(&junk).is_none());
    // Its debug text is not its bytes.
    assert!(format!("{image:?}").len() < 200);
    // From and back to the staging area, as it was.
    let staged = Staged {
        file: member.clone(),
        width: 40,
        height: 20,
    };
    let back = PageImage::staged(key_of(&member.bytes), staged.clone()).into_staged();
    assert_eq!(back, staged);
}

#[test]
fn two_decoded_pictures_are_kept_and_a_third_decodes_again() {
    let decoded = DecodedPictures::default();
    let images: Vec<PageImage> = [(40, 20), (30, 30), (20, 40)]
        .into_iter()
        .map(|(w, h)| PageImage::of_member(&member(w, h)).unwrap())
        .collect();
    let first = decoded.picture(&images[0]).unwrap();
    assert_eq!((first.width(), first.height()), (40, 20));
    // Kept: the same pixels, not decoded again.
    let again = decoded.picture(&images[0]).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(decoded.decodes(), 1);
    decoded.picture(&images[1]).unwrap();
    decoded.picture(&images[2]).unwrap();
    assert_eq!(decoded.decodes(), 3);
    assert_eq!(decoded.kept(), DECODED_KEPT);
    // The first went: decoded again, and still no more than two kept.
    decoded.picture(&images[0]).unwrap();
    assert_eq!(decoded.decodes(), 4);
    assert_eq!(decoded.kept(), DECODED_KEPT);
    // The last two used stay.
    decoded.picture(&images[2]).unwrap();
    assert_eq!(decoded.decodes(), 4);
    // Pixels that cannot be decoded are tried once.
    let damaged = PageImage {
        member: BackgroundFile {
            extension: "png".to_owned(),
            bytes: b"\x89PNG\r\n\x1a\nnot really".to_vec(),
        },
        width: 10,
        height: 10,
        key: "damaged-18".to_owned(),
    };
    assert!(decoded.picture(&damaged).is_none());
    assert!(decoded.picture(&damaged).is_none());
    assert_eq!(decoded.decodes(), 5);
}

// --- The paper of the page on screen --------------------------------------------------------------

#[test]
fn the_paper_of_the_page_changes_at_once_and_nothing_else_does() {
    let mut session = running();
    // A stroke in progress, and the computer's history: neither moves.
    session.records(
        &[pen(1000, 1000, 500), pen(1010, 1010, 500)],
        T0,
        &mut Vec::new(),
    );
    let revision = session.snapshot().revision;
    let mut out = Vec::new();
    session
        .set_page_paper(0, seyes(Paper::Cream), &mut out)
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    let LiveEvent::Snapshot { snapshot } = &out[0] else {
        panic!("not a snapshot: {:?}", out[0]);
    };
    assert_eq!(snapshot.page.background, seyes(Paper::Cream));
    assert_eq!(snapshot.page_paper, Paper::Cream);
    assert_eq!(snapshot.revision, revision);
    assert!(
        !snapshot.can_undo && !snapshot.can_redo,
        "not a step of Ctrl+Z"
    );
    // The stroke in progress follows the snapshot, still in progress.
    assert!(matches!(out[1], LiveEvent::StrokeStart { .. }));
    assert!(!out.iter().any(|e| matches!(e, LiveEvent::StrokeEnd { .. })));
    // Paper alone is not worth a note.
    assert!(!session.pages()[0].needs_write());

    // The same again: nothing, no event.
    let mut out = Vec::new();
    session
        .set_page_paper(0, seyes(Paper::Cream), &mut out)
        .unwrap();
    assert!(out.is_empty());
    // Another page than the one on screen: stale, nothing changed.
    assert_eq!(
        session.set_page_paper(1, Background::default(), &mut out),
        Err(PaperError::Stale)
    );
    assert!(out.is_empty());
    // Going back is choosing the old paper again.
    session
        .set_page_paper(0, Background::default(), &mut out)
        .unwrap();
    assert_eq!(session.pages()[0].note.background, Background::default());
}

#[test]
fn under_the_blackboard_the_colour_waits_and_the_rest_applies_at_once() {
    let mut session = running();
    board(&mut session, true);
    assert_eq!(session.snapshot().page_paper, Paper::White);
    let mut out = Vec::new();
    session
        .set_page_paper(0, seyes(Paper::Kraft), &mut out)
        .unwrap();
    let page = &session.pages()[0];
    // Still slate, already Seyès and textured; its own paper is kraft.
    assert_eq!(page.note.background, seyes(Paper::Slate));
    assert_eq!(page.own_paper(true), Paper::Kraft);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.page_paper, Paper::Kraft);
    assert_eq!(snapshot.page.background.paper, Paper::Slate);
    // The colour alone, under the board: told, the page unchanged on screen.
    let mut out = Vec::new();
    session
        .set_page_paper(0, seyes(Paper::Cream), &mut out)
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    assert_eq!(session.pages()[0].note.background, seyes(Paper::Slate));
    // Off: the colour chosen comes back.
    board(&mut session, false);
    assert_eq!(session.pages()[0].note.background, seyes(Paper::Cream));
    assert_eq!(session.snapshot().page_paper, Paper::Cream);

    // Slate chosen without the blackboard: slate it is, and the blackboard gives it back.
    let slate = Background {
        paper: Paper::Slate,
        ..Background::default()
    };
    session.set_page_paper(0, slate, &mut out).unwrap();
    assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
    assert_eq!(
        session.style(),
        LiveStyle::default(),
        "the ink stays the pen's"
    );
    board(&mut session, true);
    board(&mut session, false);
    assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
    assert_eq!(session.snapshot().page_paper, Paper::Slate);
}

#[test]
fn a_new_page_keeps_the_paper_of_the_page_shown_but_not_its_image() {
    let mut session = running();
    // A new session starts white.
    assert_eq!(session.pages()[0].note.background, Background::default());
    session
        .set_page_paper(0, seyes(Paper::Cream), &mut Vec::new())
        .unwrap();
    set_image(
        &mut session,
        placed("fiche.png", A5, (40, 20), ImageFit::Contain),
        pixels_of(40, 20),
    );
    command(&mut session, LiveCommandKind::NextPage);
    let page = &session.pages()[1];
    assert_eq!(page.note.background, seyes(Paper::Cream));
    assert!(page.note.background_image.is_none() && page.image().is_none());
    let snapshot = session.snapshot();
    assert_eq!(snapshot.image_pixels, None);
    assert_eq!(snapshot.page_paper, Paper::Cream);

    // Changed on page 2, back to page 1: each keeps its own.
    session
        .set_page_paper(1, Background::default(), &mut Vec::new())
        .unwrap();
    command(&mut session, LiveCommandKind::PreviousPage);
    assert_eq!(session.pages()[0].note.background, seyes(Paper::Cream));
    command(&mut session, LiveCommandKind::NextPage);
    assert_eq!(session.pages()[1].note.background, Background::default());

    // « Nouvelle page » from page 1: page 1's paper, not the last page's.
    command(&mut session, LiveCommandKind::PreviousPage);
    command(&mut session, LiveCommandKind::NewPage);
    assert_eq!(session.current(), 2);
    assert_eq!(session.pages()[2].note.background, seyes(Paper::Cream));

    // Under the blackboard: slate, its template and texture, and its own paper back when it goes.
    board(&mut session, true);
    command(&mut session, LiveCommandKind::NewPage);
    assert_eq!(session.pages()[3].note.background, seyes(Paper::Slate));
    assert_eq!(session.snapshot().page_paper, Paper::Cream);
    board(&mut session, false);
    assert_eq!(session.pages()[3].note.background, seyes(Paper::Cream));
    // The tablet's button set to « nouvelle page » does the same.
    session.command(
        LiveCommandKind::Button,
        LiveButton::NewPage,
        T0 as i64,
        &mut Vec::new(),
    );
    assert_eq!(session.pages()[4].note.background, seyes(Paper::Cream));

    // A new session: white again (O2).
    let fresh = LiveSession::new(
        2,
        A5,
        LiveStyle::default(),
        LiveViewState::whole(A5),
        true,
        0,
    );
    assert_eq!(fresh.pages()[0].note.background, Background::default());
}

#[test]
fn the_snapshot_tells_the_windows_the_own_paper_and_the_pixels_as_live_ts_reads_them() {
    let mut session = running();
    session
        .set_page_paper(0, seyes(Paper::Kraft), &mut Vec::new())
        .unwrap();
    let pixels = pixels_of(40, 20);
    set_image(
        &mut session,
        placed("fiche.png", A5, (40, 20), ImageFit::Contain),
        pixels.clone(),
    );
    board(&mut session, true);
    let value = serde_json::to_value(session.snapshot()).unwrap();
    assert_eq!(value["pagePaper"], "kraft");
    assert_eq!(value["page"]["background"]["paper"], "slate");
    assert_eq!(
        value["imagePixels"],
        serde_json::json!({ "key": pixels.key, "width": 40, "height": 20 })
    );
    session
        .set_page_image(0, ImageChange::Remove, &mut Vec::new())
        .unwrap();
    let value = serde_json::to_value(session.snapshot()).unwrap();
    assert_eq!(value["imagePixels"], serde_json::Value::Null);
}

// --- The image of the page on screen --------------------------------------------------------------

#[test]
fn set_place_and_remove_change_the_image_and_say_so_once() {
    let mut session = running();
    let pixels = pixels_of(40, 20);
    let image = placed("fiche.png", A5, (40, 20), ImageFit::Contain);
    assert!(!session.pages()[0].needs_write());
    let mut out = Vec::new();
    session
        .set_page_image(
            0,
            ImageChange::Set {
                image: image.clone(),
                pixels: pixels.clone(),
            },
            &mut out,
        )
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.page.background_image.as_ref(), Some(&image));
    assert_eq!(snapshot.image_pixels, Some(pixels.pixels()));
    assert!(!snapshot.can_undo);
    // A worksheet shown is worth a note, ink or not (O3).
    assert!(session.pages()[0].needs_write());
    assert!(!session.pages()[0].held_fixed(), "no ink yet");

    // The same pixels placed the same way: nothing.
    let mut out = Vec::new();
    session
        .set_page_image(
            0,
            ImageChange::Set {
                image: image.clone(),
                pixels: pixels.clone(),
            },
            &mut out,
        )
        .unwrap();
    session
        .set_page_image(
            0,
            ImageChange::Place {
                image: image.clone(),
            },
            &mut out,
        )
        .unwrap();
    assert!(out.is_empty());

    // Placed otherwise.
    let lighter = BackgroundImage {
        opacity: 0.4,
        ..placed("fiche.png", A5, (40, 20), ImageFit::Cover)
    };
    session
        .set_page_image(
            0,
            ImageChange::Place {
                image: lighter.clone(),
            },
            &mut out,
        )
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    assert_eq!(
        session.pages()[0].note.background_image.as_ref(),
        Some(&lighter)
    );
    assert_eq!(session.snapshot().image_pixels, Some(pixels.pixels()));

    // Another image under the same place: refused (new pixels come with a set), nothing changed.
    let mut out = Vec::new();
    for other in [
        BackgroundImage {
            name: "autre.png".to_owned(),
            ..lighter.clone()
        },
        BackgroundImage {
            pdf_page: Some(2),
            ..lighter.clone()
        },
    ] {
        assert_eq!(
            session.set_page_image(0, ImageChange::Place { image: other }, &mut out),
            Err(PaperError::NotThisImage)
        );
    }
    // Too far away from this page, set or placed.
    let far = BackgroundImage {
        rect: ImageRect {
            x: 20.0 * A5.width as f32,
            ..lighter.rect
        },
        ..lighter.clone()
    };
    assert_eq!(
        session.set_page_image(0, ImageChange::Place { image: far.clone() }, &mut out),
        Err(PaperError::BadRect)
    );
    assert_eq!(
        session.set_page_image(
            0,
            ImageChange::Set {
                image: far,
                pixels: pixels_of(30, 30)
            },
            &mut out
        ),
        Err(PaperError::BadRect)
    );
    assert_eq!(
        session.set_page_image(1, ImageChange::Remove, &mut out),
        Err(PaperError::Stale)
    );
    assert!(out.is_empty());
    assert_eq!(
        session.pages()[0].note.background_image.as_ref(),
        Some(&lighter)
    );

    // Another file under the same name: other pixels, another key.
    let other = pixels_of(30, 30);
    session
        .set_page_image(
            0,
            ImageChange::Set {
                image: lighter.clone(),
                pixels: other.clone(),
            },
            &mut out,
        )
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    assert_ne!(other.key, pixels.key);
    assert_eq!(session.snapshot().image_pixels, Some(other.pixels()));

    // Removed: both go, once.
    let mut out = Vec::new();
    session
        .set_page_image(0, ImageChange::Remove, &mut out)
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    let page = &session.pages()[0];
    assert!(page.note.background_image.is_none() && page.image().is_none());
    assert_eq!(session.snapshot().image_pixels, None);
    let mut out = Vec::new();
    session
        .set_page_image(0, ImageChange::Remove, &mut out)
        .unwrap();
    assert!(out.is_empty());
    // Never written, no ink: nothing to write any more.
    assert!(!session.pages()[0].needs_write());
    // A place with no image to place.
    assert_eq!(
        session.set_page_image(0, ImageChange::Place { image: lighter }, &mut out),
        Err(PaperError::NotThisImage)
    );
}

#[test]
fn a_worksheet_without_ink_follows_the_tablet_and_one_written_on_stays() {
    let mut session = running();
    let pixels = pixels_of(40, 20);
    for fit in [ImageFit::Contain, ImageFit::Cover, ImageFit::Stretch] {
        set_image(
            &mut session,
            placed("fiche.png", A5, (40, 20), fit),
            pixels.clone(),
        );
        // The tablet turned to portrait before anything is written: a portrait page, the image
        // placed again upright with its fit.
        let mut out = Vec::new();
        session.reorient(A5.width, A5.height, Orientation::PortraitLeft, &mut out);
        assert_eq!(snapshots(&out), 1);
        let page = &session.pages()[0];
        assert_eq!(page.note.page, A5_PORTRAIT);
        assert_eq!(
            page.note.background_image.as_ref().unwrap().rect,
            ImageRect::fit(A5_PORTRAIT, (40, 20), fit),
            "{fit:?}"
        );
        assert_eq!(
            page.note.background_image.as_ref().unwrap().quarter_turns,
            0
        );
        assert!(session.snapshot().can_turn_page);
        // And back to landscape.
        session.reorient(A5.width, A5.height, Orientation::Landscape, &mut Vec::new());
        assert_eq!(session.pages()[0].note.page, A5);
        assert_eq!(
            session.pages()[0]
                .note
                .background_image
                .as_ref()
                .unwrap()
                .rect,
            ImageRect::fit(A5, (40, 20), fit)
        );
    }
    // A page with an image and no ink behind the one on screen follows too.
    command(&mut session, LiveCommandKind::NextPage);
    session.reorient(
        A5.width,
        A5.height,
        Orientation::PortraitLeft,
        &mut Vec::new(),
    );
    assert_eq!(session.pages()[0].note.page, A5_PORTRAIT);
    session.reorient(A5.width, A5.height, Orientation::Landscape, &mut Vec::new());
    command(&mut session, LiveCommandKind::PreviousPage);

    // Written on: never turned, the tablet follows it, the quick switch is off.
    write_at(&mut session, 3_000, 2_000, T0);
    let strokes = session.pages()[0].note.strokes.clone();
    let rect = session.pages()[0]
        .note
        .background_image
        .as_ref()
        .unwrap()
        .rect;
    assert!(session.pages()[0].held_fixed());
    assert!(!session.snapshot().can_turn_page);
    let mut out = Vec::new();
    session.reorient(A5.width, A5.height, Orientation::PortraitLeft, &mut out);
    let page = &session.pages()[0];
    assert_eq!(page.note.page, A5);
    assert_eq!(page.note.strokes, strokes);
    assert_eq!(page.note.background_image.as_ref().unwrap().rect, rect);
    let snapshot = session.snapshot();
    assert_eq!(
        snapshot.orientation,
        Orientation::Landscape,
        "held as the page is"
    );
    assert!(!snapshot.can_turn_page);
    // The pen lands on it as on a resumed sheet held so.
    let size = oriented_size(A5, Orientation::PortraitLeft);
    let mapping = PenMapping::for_sheet(
        size,
        Orientation::Landscape.turns_from(Orientation::PortraitLeft),
        A5,
    );
    assert!(!mapping.is_direct());
    write_at(&mut session, 4_000, 6_000, T0 + 1_000.0);
    let point = session.pages()[0].note.strokes.last().unwrap().points[0];
    assert_eq!((point.x, point.y), mapping.map(4_000, 6_000));

    // The image removed: a page of the session again, turned at once with its ink to the way the
    // tablet is held; the pen's points land as they come.
    let mut out = Vec::new();
    session
        .set_page_image(0, ImageChange::Remove, &mut out)
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    let page = &session.pages()[0];
    assert!(!page.held_fixed());
    assert_eq!(page.note.page, A5_PORTRAIT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.orientation, Orientation::PortraitLeft);
    assert!(snapshot.can_turn_page);
    assert_eq!(snapshot.view, LiveViewState::whole(A5_PORTRAIT));
    write_at(&mut session, 4_000, 6_000, T0 + 2_000.0);
    let point = session.pages()[0].note.strokes.last().unwrap().points[0];
    assert_eq!((point.x, point.y), (4_000.0, 6_000.0));
}

// --- The hub, with a real library -----------------------------------------------------------------

/// `bytes` staged as the live window does.
fn stage(staging: &Staging, width: u32, height: u32) -> (String, BackgroundFile) {
    let member = member(width, height);
    let key = staging.put(Staged {
        file: member.clone(),
        width,
        height,
    });
    (key, member)
}

fn code(error: LiveError) -> &'static str {
    error.command_error().code()
}

fn set_op(key: &str, image: &BackgroundImage) -> LivePageImageOp {
    LivePageImageOp::Set {
        key: key.to_owned(),
        image: image.clone(),
    }
}

#[test]
fn the_hub_refuses_as_a_whole_and_gives_a_refused_image_back() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let staging = Staging::default();
    let (key, _) = stage(&staging, 40, 20);
    let image = placed("fiche.png", A5, (40, 20), ImageFit::Contain);

    // Before the first start.
    let refused = hub
        .change_page_image(0, set_op(&key, &image), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "liveNotRunning");
    assert!(staging.get(&key).is_some(), "given back");
    assert_eq!(
        code(hub.set_page_paper(0, seyes(Paper::Cream)).unwrap_err()),
        "liveNotRunning"
    );

    let _sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    // Another page: stale, given back.
    let refused = hub
        .change_page_image(1, set_op(&key, &image), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "liveEditStale");
    assert!(staging.get(&key).is_some());
    // Too far for this page (checked under the lock): given back.
    let far = BackgroundImage {
        rect: ImageRect {
            y: -40.0 * A5.height as f32,
            ..image.rect
        },
        ..image.clone()
    };
    let refused = hub
        .change_page_image(0, set_op(&key, &far), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "liveEditInvalid");
    assert!(staging.get(&key).is_some());
    // Nonsense: refused before anything is taken.
    let nameless = BackgroundImage {
        name: String::new(),
        ..image.clone()
    };
    let refused = hub
        .change_page_image(0, set_op(&key, &nameless), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "liveEditInvalid");
    assert!(staging.get(&key).is_some());
    // Not staged.
    let refused = hub
        .change_page_image(0, set_op("0000000000000000-1", &image), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "backgroundMissing");
    let refused = hub
        .change_page_image(
            0,
            LivePageImageOp::Place {
                image: image.clone(),
            },
            &staging,
        )
        .unwrap_err();
    assert_eq!(code(refused), "liveEditInvalid", "no image to place");
    assert_eq!(
        code(hub.set_page_paper(3, Background::default()).unwrap_err()),
        "liveEditStale"
    );
    assert!(hub.page_image(0).is_none());

    // Set: taken out of the staging area, the page keeps it.
    hub.change_page_image(0, set_op(&key, &image), &staging)
        .unwrap();
    assert!(staging.get(&key).is_none());
    let (shown, pixels) = hub.page_image(0).unwrap();
    assert_eq!(shown, image);
    assert_eq!(pixels.key, key);
    assert_eq!(hub.snapshot().image_pixels.unwrap().key, key);
    // The same key again is no longer staged.
    let refused = hub
        .change_page_image(0, set_op(&key, &image), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "backgroundMissing");
    wait_until("the snapshot", || {
        window.has(|event| {
            matches!(event, LiveEvent::Snapshot { snapshot }
                if snapshot.image_pixels.as_ref().is_some_and(|p| p.key == key))
        })
    });

    // Stopped: its notes are in the library, nothing changes them.
    hub.stop().unwrap();
    let (other, _) = stage(&staging, 30, 30);
    let refused = hub
        .change_page_image(0, set_op(&other, &image), &staging)
        .unwrap_err();
    assert_eq!(code(refused), "liveNotRunning");
    assert!(staging.get(&other).is_some(), "given back");
    assert_eq!(
        code(
            hub.change_page_image(0, LivePageImageOp::Remove, &staging)
                .unwrap_err()
        ),
        "liveNotRunning"
    );
    assert_eq!(
        code(hub.set_page_paper(0, seyes(Paper::Cream)).unwrap_err()),
        "liveNotRunning"
    );
    assert!(hub.page_image(0).is_some());
}

/// The red half of the image shows on the left of `thumbnail`, the blue half on the right.
fn shows_the_halves(png: &[u8]) -> bool {
    let image = decode_png(png).unwrap();
    let rgb = |x: u32, y: u32| {
        let [r, g, b, _] = image.pixel(x, y).unwrap();
        [r, g, b]
    };
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 12);
    let (w, h) = (image.width, image.height);
    near(rgb(w / 4, h / 2), [220, 30, 30]) && near(rgb(3 * w / 4, h / 2), [30, 40, 220])
}

#[test]
fn a_worksheet_shown_is_kept_as_a_note_with_its_pixels_and_paper_alone_is_not() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let staging = Staging::default();
    let _sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let created = hub.snapshot().page.created;

    // Paper alone: nothing written.
    hub.set_page_paper(0, seyes(Paper::Cream)).unwrap();
    std::thread::sleep(config().save_interval * 3);
    assert!(notes_in(&library).is_empty());

    // An image: a note, with the member, its description and the paper, ink or not (O3).
    let (key, member) = stage(&staging, 40, 20);
    let image = placed("fiche.png", A5, (40, 20), ImageFit::Stretch);
    hub.change_page_image(0, set_op(&key, &image), &staging)
        .unwrap();
    wait_until("the note", || notes_in(&library).len() == 1);
    let path = notes_in(&library).remove(0);
    let file = library.read(&path).unwrap().file;
    assert_eq!(file.background.as_ref(), Some(&member));
    assert_eq!(file.note.background_image.as_ref(), Some(&image));
    assert_eq!(file.note.background, seyes(Paper::Cream));
    assert!(file.note.strokes.is_empty());
    assert_eq!(file.note.created, created, "the page's creation");
    // Its thumbnail shows the image.
    assert!(shows_the_halves(file.thumbnail.as_ref().unwrap()));

    // Placed otherwise: the description follows, the pixels stay.
    let faint = BackgroundImage {
        opacity: 0.25,
        ..image.clone()
    };
    hub.change_page_image(
        0,
        LivePageImageOp::Place {
            image: faint.clone(),
        },
        &staging,
    )
    .unwrap();
    wait_until("the new place", || {
        library
            .read(&path)
            .unwrap()
            .file
            .note
            .background_image
            .as_ref()
            == Some(&faint)
    });
    assert_eq!(
        library.read(&path).unwrap().file.background.as_ref(),
        Some(&member)
    );

    // Removed: the note keeps its paper, without member nor image.
    hub.change_page_image(0, LivePageImageOp::Remove, &staging)
        .unwrap();
    wait_until("the image gone", || {
        library
            .read(&path)
            .unwrap()
            .file
            .note
            .background_image
            .is_none()
    });
    assert_eq!(library.read(&path).unwrap().file.background, None);
    hub.stop().unwrap();
    assert_eq!(notes_in(&library), [path]);
}

#[test]
fn a_note_retitled_meanwhile_gets_the_pixels_with_the_description() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let staging = Staging::default();
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let (key, _) = stage(&staging, 40, 20);
    let image = placed("fiche.png", A5, (40, 20), ImageFit::Contain);
    hub.change_page_image(0, set_op(&key, &image), &staging)
        .unwrap();
    stroke(&mut sink, 3000, 5);
    wait_until("the note", || notes_in(&library).len() == 1);
    let path = notes_in(&library).remove(0);
    wait_until("the stroke written", || {
        library.read(&path).unwrap().file.note.strokes.len() == 1
    });

    // Retitled from the library, then another file is put under the page, of the same name.
    let renamed = library.retitle(&path, "Fiche du jour", None).unwrap().path;
    let (other, other_member) = stage(&staging, 20, 40);
    let upright = placed("fiche.png", A5, (20, 40), ImageFit::Contain);
    hub.change_page_image(0, set_op(&other, &upright), &staging)
        .unwrap();
    wait_until("the retitled note rewritten", || {
        library
            .read(&renamed)
            .is_ok_and(|loaded| loaded.file.note.background_image.as_ref() == Some(&upright))
    });
    let file = library.read(&renamed).unwrap().file;
    assert_eq!(file.note.title, "Fiche du jour", "kept");
    // The new pixels with the new description, not the old pixels under it.
    assert_eq!(file.background.as_ref(), Some(&other_member));
    assert_eq!(notes_in(&library), [renamed]);
    hub.stop().unwrap();
}

#[test]
fn a_page_that_cannot_be_written_is_backed_up_with_its_image() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Notes");
    std::fs::create_dir(&root).unwrap();
    ardoise_core::library::init(&root).unwrap();
    let library = ardoise_core::library::Library::open(&root).unwrap();
    let hub = LiveHub::new(config());
    let staging = Staging::default();
    let _sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    break_folder(&hub, &root, true);
    let (key, member) = stage(&staging, 40, 20);
    let image = placed("fiche.png", A5, (40, 20), ImageFit::Contain);
    hub.change_page_image(0, set_op(&key, &image), &staging)
        .unwrap();
    assert!(hub.stop().is_err());
    let copies = hub.back_up_unsaved(&dir.path().join("live-backup"));
    break_folder(&hub, &root, false);
    assert_eq!(copies.len(), 1);
    let path = copies[0].1.as_ref().unwrap();
    let file = ardoise_core::format::read_note(path).unwrap();
    assert_eq!(file.background.as_ref(), Some(&member));
    assert_eq!(file.note.background_image.as_ref(), Some(&image));
    assert!(shows_the_halves(file.thumbnail.as_ref().unwrap()));
}

/// The pictures the windows get for the page (`live_page_background`) show its image.
#[test]
fn the_windows_draw_the_image_of_any_page_that_has_one() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let staging = Staging::default();
    let _sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let (key, _) = stage(&staging, 40, 20);
    let image = placed("fiche.png", A5, (40, 20), ImageFit::Stretch);
    hub.change_page_image(0, set_op(&key, &image), &staging)
        .unwrap();
    let url = crate::live::page_background_url(hub.page_image(0), A5, Background::default(), 400)
        .unwrap();
    let png = crate::thumbnails::tests::png_of(&url);
    assert!(shows_the_halves(&png));
    // The same scene as the editor draws it.
    let picture = Picture::decode(member(40, 20).bytes).unwrap();
    let mut note = crate::library::blank_note("", "", 0);
    note.page = A5;
    note.background_image = Some(image);
    let editor = render_note(
        NoteView::new(&note, Some(&picture)),
        &RenderOptions::fit_width(A5, 400),
    )
    .unwrap();
    let drawn = decode_png(&png).unwrap();
    assert_eq!((drawn.width, drawn.height), (editor.width, editor.height));
    let differ = (0..drawn.height)
        .flat_map(|y| (0..drawn.width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let (a, b) = (drawn.pixel(x, y).unwrap(), editor.pixel(x, y).unwrap());
            a.iter().zip(b).any(|(p, q)| p.abs_diff(q) > 8)
        })
        .count();
    assert_eq!(differ, 0);
    hub.stop().unwrap();
}
