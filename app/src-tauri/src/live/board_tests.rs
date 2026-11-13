//! The Xiaomi blackboard's position switch in live mode (`docs/questions.md` Q19), through the
//! whole stack: the simulated blackboard, the tablet service, the live hub and the command of the
//! live window (`tablet_set_board_position_in`), down to the note written at the end. A hand holds
//! the tablet one way while it is set another and writes a "Γ" (up, then right, as the user sees
//! it): the switch turns the page with that ink, what the hand writes next comes upright, and the
//! note saved at the end is upright. On a resumed sheet only the two positions of its way round
//! are taken, the sheet never turns.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ardoise_core::format::NoteFile;
use ardoise_core::library::Library;
use ardoise_core::note::{Note, PageSize};
use ardoise_core::settings::{BoardPosition, LiveButton, Orientation, SettingsStore};
use ardoise_device::identity::{self, BLACKBOARD_FORMAT, BLACKBOARD_PROTOCOL, BlackboardRecord};
use ardoise_device::pages::{BLACKBOARD_PAGE, board_page, orient_board};
use ardoise_device::service::{
    Connection, ServiceConfig, ServiceHooks, TabletOptions, TabletService,
};
use ardoise_ink::Stroke;
use ardoise_proto::stroke::Point;
use ardoise_sim::{PenScript, SimBoard, SimBoardConfig, SimDevice, SimFactory};

use super::hub::{LiveHub, Start, StartOptions};
use super::model::{LiveCommandKind, LiveEvent, LiveSessionState};
use super::tests::{Window, config, fixed, wait_until};
use crate::AppContext;
use crate::library::blank_note;
use crate::library::tests::library;
use crate::tablet::{
    TabletRuntime, board_position_allowed, tablet_set_board_position_in, tablet_set_options_in,
};

use BoardPosition::{ButtonsBottom, ButtonsLeft, ButtonsRight, ButtonsTop};

/// Where the user's "up" and "right" point in the blackboard's own frame (x across its screen, y
/// down to its buttons), held as `position`: from where its buttons are, not from the code under
/// test (the same table as `ardoise-sim/tests/blackboard.rs`).
fn user_axes(position: BoardPosition) -> ((i32, i32), (i32, i32)) {
    match position {
        // (up, right)
        ButtonsBottom => ((0, -1), (1, 0)),
        ButtonsTop => ((0, 1), (-1, 0)),
        ButtonsLeft => ((-1, 0), (0, -1)),
        ButtonsRight => ((1, 0), (0, 1)),
    }
}

/// The "Γ" of a hand holding the blackboard as `position`, starting at the user's `(dx, dy)` from
/// the middle of the screen (in steps of 200 units): up 20 points, then right 10, two strokes.
fn gamma_at(position: BoardPosition, (dx, dy): (i32, i32)) -> PenScript {
    let ((ux, uy), (rx, ry)) = user_axes(position);
    let (cx, cy) = (
        10_000 + (rx * dx - ux * dy) * 200,
        13_000 + (ry * dx - uy * dy) * 200,
    );
    let at = |x: i32, y: i32| Point {
        x: u16::try_from(x).unwrap(),
        y: u16::try_from(y).unwrap(),
        pressure: 900,
    };
    let up: Vec<Point> = (0..20)
        .map(|i| at(cx + ux * i * 200, cy + uy * i * 200))
        .collect();
    let (tx, ty) = (cx + ux * 19 * 200, cy + uy * 19 * 200);
    let right: Vec<Point> = (0..10)
        .map(|i| at(tx + rx * i * 200, ty + ry * i * 200))
        .collect();
    PenScript::new()
        .near(up)
        .away(Duration::from_millis(300))
        .near(right)
        .away(Duration::from_millis(300))
}

fn gamma(position: BoardPosition) -> PenScript {
    gamma_at(position, (0, 0))
}

/// The direction a stroke goes on the page, as (sign of dx, sign of dy).
fn heading(stroke: &Stroke) -> (i32, i32) {
    let (first, last) = (stroke.points[0], stroke.points[stroke.points.len() - 1]);
    let sign = |d: f32| {
        if d.abs() < 1.0 {
            0
        } else if d > 0.0 {
            1
        } else {
            -1
        }
    };
    (sign(last.x - first.x), sign(last.y - first.y))
}

/// Every "Γ" of `note` goes up then right on its page (its strokes in pairs), inside the page.
fn upright(note: &Note, what: &str) {
    assert!(!note.strokes.is_empty(), "{what}: no ink");
    for (i, stroke) in note.strokes.iter().enumerate() {
        let expected = if i % 2 == 0 { (0, -1) } else { (1, 0) };
        assert_eq!(heading(stroke), expected, "{what}: stroke {i}");
        for point in &stroke.points {
            assert!(
                point.x <= note.page.width as f32 && point.y <= note.page.height as f32,
                "{what}: stroke {i} beyond the page"
            );
        }
    }
}

/// Writes `script` each time live mode starts on the blackboard (a new link's included).
struct Writer {
    board: SimBoard,
    script: PenScript,
}

impl ServiceHooks for Writer {
    fn blackboard_live_started(&self) {
        self.board.draw_live(self.script.clone());
    }
}

/// The app around a simulated blackboard: its settings, its library, the tablet service in a
/// runtime, the live hub.
struct Stack {
    _settings: tempfile::TempDir,
    _notes: tempfile::TempDir,
    context: AppContext,
    library: Library,
    board: SimBoard,
    runtime: TabletRuntime,
    hub: LiveHub,
    /// A window subscribed to the hub: what it was sent.
    window: Arc<Window>,
}

impl Stack {
    /// The blackboard known, set to `set`, a hand on it writing `script` whenever live mode
    /// starts; `real_time`: the blackboard follows the computer's clock (else its own virtual one).
    fn new(set: BoardPosition, script: PenScript, real_time: bool) -> Stack {
        let settings = tempfile::tempdir().unwrap();
        let context = AppContext {
            store: Ok(SettingsStore::locate(
                settings.path(),
                settings.path(),
                "PC-TEST",
            )),
            system_locale: Some("fr-FR".to_owned()),
            self_test: None,
            open_diagnostic: false,
        };
        let saved = context
            .store
            .as_ref()
            .unwrap()
            .update(|s| s.tablet.board_position = set)
            .unwrap();
        let (notes, library) = library();
        let board = if real_time {
            SimBoard::real_time(SimBoardConfig::blackboard())
        } else {
            SimBoard::blackboard()
        };
        identity::save_blackboard(
            library.root(),
            &BlackboardRecord {
                format: BLACKBOARD_FORMAT,
                address: board.config().address,
                name: "Mijia LCD blackboard".into(),
                protocol: BLACKBOARD_PROTOCOL.into(),
                model: "jiqid.sxb.v1".into(),
                added: "2026-11-13T09:00:00+01:00".into(),
                extra: Default::default(),
            },
        )
        .unwrap();
        let service = TabletService::start(
            ServiceConfig {
                library: Some(library.root().to_owned()),
                identity_root: Some(library.root().to_owned()),
                options: TabletOptions::from(&saved.tablet),
                simulated: true,
                rescue_fallback: None,
            },
            Box::new(SimFactory::of(
                vec![SimDevice::Board(board.clone())],
                real_time,
            )),
            Arc::new(Writer {
                board: board.clone(),
                script,
            }),
            Box::new(|_: &ardoise_device::service::TabletStatus| {}),
            Box::new(|_, _, _| Err("no stored page in these tests".into())),
        );
        let runtime = TabletRuntime::holding(service);
        let hub = LiveHub::new(config());
        let window = Arc::new(Window::default());
        hub.subscribe("main", window.clone());
        let stack = Stack {
            _settings: settings,
            _notes: notes,
            context,
            library,
            board,
            runtime,
            hub,
            window,
        };
        stack.wait_for_service("the blackboard connected", |options, connection| {
            options.board_position == set && connection == Connection::Connected
        });
        stack
    }

    fn wait_for_service(&self, what: &str, until: impl Fn(&TabletOptions, Connection) -> bool) {
        wait_until(what, || {
            self.runtime
                .status()
                .is_ok_and(|status| until(&status.options, status.connection))
        });
    }

    fn start_options(&self) -> StartOptions {
        StartOptions {
            library: self.library.clone(),
            french: true,
            size: BLACKBOARD_PAGE,
            orientation: Orientation::Landscape,
            button: fixed(LiveButton::NewPage),
        }
    }

    /// Live mode on a new page, as `live_start` does.
    fn start_live(&self) {
        let Start::New { sink, .. } = self.hub.start(self.start_options()).unwrap() else {
            panic!("a session is already running");
        };
        self.runtime
            .with_service(move |service| service.start_live(Box::new(sink)))
            .unwrap();
    }

    /// Live mode on the note at `path`, as `live_resume` does.
    fn resume_live(&self, path: &Path) {
        let Start::New { sink, .. } = self.hub.resume(self.start_options(), path).unwrap() else {
            panic!("a session is already running");
        };
        self.runtime
            .with_service(move |service| service.start_live(Box::new(sink)))
            .unwrap();
    }

    /// The finished strokes of the page on screen, once there are `n`.
    fn strokes(&self, n: usize) -> Vec<Stroke> {
        wait_until(&format!("{n} strokes on the page"), || {
            self.hub.snapshot().page.strokes.len() >= n
        });
        self.hub.snapshot().page.strokes
    }

    /// The live window's switch, as the interface calls it.
    fn switch(&self, position: BoardPosition) {
        tablet_set_board_position_in(&self.runtime, &self.context, &self.hub, position).unwrap();
        wait_until(&format!("the live page held {position:?}"), || {
            self.hub.snapshot().board_position == Some(position)
        });
    }

    fn saved_position(&self) -> BoardPosition {
        self.context
            .store
            .as_ref()
            .unwrap()
            .load()
            .unwrap()
            .tablet
            .board_position
    }

    /// Ends live mode as `live_stop` does; returns the notes written.
    fn stop(&self) -> Vec<Note> {
        self.runtime.with_service(TabletService::stop_live).unwrap();
        let saved = self.hub.stop().unwrap();
        self.runtime.stop();
        saved
            .iter()
            .map(|path| self.library.read(Path::new(path)).unwrap().file.note)
            .collect()
    }
}

/// The page turns with its ink by the quarter turns from `from` to `to` (a quarter turn swaps the
/// page's shape), as `LiveSession::hold` is meant to: the same points, turned.
fn turned(strokes: &[Stroke], from: BoardPosition, to: BoardPosition) -> Vec<(f32, f32)> {
    let page = board_page(from);
    let turns = (to.quarter_turns() + 4 - from.quarter_turns()) % 4;
    strokes
        .iter()
        .flat_map(|stroke| &stroke.points)
        .map(|point| {
            ardoise_device::pages::turn_point(
                point.x,
                point.y,
                page.width as f32,
                page.height as f32,
                turns,
            )
        })
        .collect()
}

fn points(strokes: &[Stroke]) -> Vec<(f32, f32)> {
    strokes
        .iter()
        .flat_map(|stroke| &stroke.points)
        .map(|point| (point.x, point.y))
        .collect()
}

/// The user holds the tablet as `held`, set to `set` by mistake (or forgotten): what is written is
/// sideways or upside down. One click on the switch turns the page and that ink upright (a quarter
/// turn swaps the page's shape, a half turn keeps it); what is written next comes upright; the
/// choice is saved in `[tablet] board_position`; a lost link and its reconnection keep it; the note
/// saved at the end is upright, on the page of the position the tablet is held in.
#[test]
fn the_switch_turns_the_page_and_its_ink_and_what_follows_comes_upright() {
    for (set, held) in [
        (ButtonsBottom, ButtonsLeft),
        (ButtonsBottom, ButtonsRight),
        (ButtonsBottom, ButtonsTop),
        (ButtonsLeft, ButtonsBottom),
        (ButtonsRight, ButtonsLeft),
    ] {
        let case = format!("set {set:?}, held {held:?}");
        let stack = Stack::new(set, gamma(held), false);
        stack.start_live();
        let written = stack.strokes(2);
        let snapshot = stack.hub.snapshot();
        assert_eq!(snapshot.page.page, board_page(set), "{case}");
        assert_eq!(snapshot.board_position, Some(set), "{case}");

        stack.switch(held);
        let snapshot = stack.hub.snapshot();
        assert_eq!(
            snapshot.page.page,
            board_page(held),
            "{case}: the page's shape"
        );
        assert_eq!(
            points(&snapshot.page.strokes),
            turned(&written, set, held),
            "{case}: the ink turned with the page"
        );
        upright(&snapshot.page, &format!("{case}: the ink already written"));
        assert_eq!(stack.saved_position(), held, "{case}: saved");
        stack.wait_for_service("the service following", |options, _| {
            options.board_position == held
        });

        // What follows comes the new way.
        stack.board.draw_live(gamma_at(held, (-20, 10)));
        stack.strokes(4);
        // A lost link and its reconnection keep the position (the hand writes again then).
        stack.board.disconnect_after(Duration::from_millis(50));
        wait_until("the reconnection", || {
            stack.window.has(|event| {
                matches!(event, LiveEvent::Snapshot { snapshot }
                    if snapshot.state == LiveSessionState::Reconnecting)
            })
        });
        stack.strokes(6);
        assert_eq!(stack.hub.snapshot().state, LiveSessionState::Live, "{case}");
        assert_eq!(stack.hub.snapshot().board_position, Some(held), "{case}");

        let notes = stack.stop();
        assert_eq!(notes.len(), 1, "{case}");
        let note = &notes[0];
        assert_eq!(note.page, board_page(held), "{case}: the note's page");
        assert_eq!(note.strokes.len(), 6, "{case}");
        upright(note, &format!("{case}: the note"));
        println!("{case}: {} × {}", note.page.width, note.page.height);
    }
}

/// Switched, then back: the page and its ink are as they were, exactly, and so is the note.
#[test]
fn switching_back_gives_the_page_and_its_ink_as_they_were() {
    for (set, other) in [(ButtonsBottom, ButtonsLeft), (ButtonsTop, ButtonsBottom)] {
        let case = format!("{set:?}, {other:?} and back");
        let stack = Stack::new(set, gamma(set), false);
        stack.start_live();
        let written = stack.strokes(2);
        stack.switch(other);
        assert_eq!(stack.hub.snapshot().page.page, board_page(other), "{case}");
        stack.switch(set);
        let snapshot = stack.hub.snapshot();
        assert_eq!(snapshot.page.page, board_page(set), "{case}");
        assert_eq!(points(&snapshot.page.strokes), points(&written), "{case}");
        assert_eq!(stack.saved_position(), set, "{case}");
        let notes = stack.stop();
        assert_eq!(
            points(&notes[0].strokes),
            points(&written),
            "{case}: the note"
        );
        upright(&notes[0], &case);
    }
}

/// A page finished before the switch stays as it was while it is not shown (as with the Slate);
/// shown again, it is held as the tablet is now, its ink turned with it.
#[test]
fn a_page_finished_before_the_switch_turns_when_it_is_shown_again() {
    let stack = Stack::new(ButtonsBottom, gamma(ButtonsLeft), false);
    stack.start_live();
    let first = stack.strokes(2);
    stack.hub.command(LiveCommandKind::NewPage).unwrap();
    wait_until("the second page", || stack.hub.snapshot().page_index == 1);
    stack.switch(ButtonsLeft);
    assert_eq!(stack.hub.snapshot().page.page, board_page(ButtonsLeft));
    stack.board.draw_live(gamma(ButtonsLeft));
    stack.strokes(2);
    // The first page, on disk and in the session: as it was written.
    wait_until("both pages written", || {
        stack.hub.snapshot().saved.len() == 2
    });
    let on_disk = stack
        .library
        .read(Path::new(&stack.hub.snapshot().saved[0]))
        .unwrap()
        .file
        .note;
    assert_eq!(on_disk.page, board_page(ButtonsBottom));
    assert_eq!(points(&on_disk.strokes), points(&first));
    stack.hub.command(LiveCommandKind::PreviousPage).unwrap();
    wait_until("the first page shown", || {
        stack.hub.snapshot().page_index == 0
    });
    let snapshot = stack.hub.snapshot();
    assert_eq!(snapshot.page.page, board_page(ButtonsLeft));
    assert_eq!(
        points(&snapshot.page.strokes),
        turned(&first, ButtonsBottom, ButtonsLeft)
    );
    let notes = stack.stop();
    assert_eq!(notes.len(), 2);
    for note in &notes {
        assert_eq!(note.page, board_page(ButtonsLeft));
        upright(note, "each page");
    }
}

/// A switch while the blackboard streams, at a moment the test does not choose (real time): every
/// point of the stroke is in the note once, turned for the position the tablet is held in, whether
/// it came before the switch (the page turned it) or after (the service did): none turned twice,
/// none missed.
#[test]
fn a_switch_while_the_pen_writes_leaves_every_point_once_and_upright() {
    let written: Vec<Point> = (0..240_u16)
        .map(|i| Point {
            x: 2000 + i * 50,
            y: 3000 + i * 70,
            pressure: 900,
        })
        .collect();
    let script = PenScript::new()
        .near(written.clone())
        .away(Duration::from_millis(200));
    let stack = Stack::new(ButtonsBottom, script, true);
    stack.start_live();
    // Some of the stroke first (the stroke in progress is not in the page: its points in the
    // window's events), then the switch.
    let started = Instant::now();
    wait_until("the pen writing", || {
        stack.hub.snapshot().state == LiveSessionState::Live
    });
    while started.elapsed() < Duration::from_millis(700) {
        std::thread::sleep(Duration::from_millis(10));
    }
    stack.switch(ButtonsLeft);
    wait_until("the whole stroke", || {
        let page = stack.hub.snapshot().page;
        points(&page.strokes).len() >= written.len()
    });
    let notes = stack.stop();
    let got = points(&notes[0].strokes);
    let expected: Vec<(f32, f32)> = written
        .iter()
        .map(|p| {
            let (x, y) = orient_board(p.x, p.y, ButtonsLeft);
            (f32::from(x), f32::from(y))
        })
        .collect();
    // The stroke was cut in two by the switch (the stroke in progress ends before the page turns).
    assert_eq!(notes[0].strokes.len(), 2, "cut once, at the switch");
    let cut = notes[0].strokes[0].points.len();
    println!(
        "{cut} points came before the switch, {} after",
        written.len() - cut
    );
    assert!(cut > 0 && cut < written.len(), "the switch came mid-stroke");
    assert_eq!(got, expected);
    assert_eq!(notes[0].page, board_page(ButtonsLeft));
}

/// A sheet of the blackboard in `page`, with one stroke and a text frame typed in the editor,
/// filed in the library.
fn filed_sheet(library: &Library, page: PageSize) -> (PathBuf, Note) {
    let mut note = blank_note("Feuille", "Calque 1", 1_787_000_000_000);
    note.page = page;
    note.objects = vec![super::tests::text_frame("consigne", 0, 5, 2_000.0, 6_000.0)];
    note.strokes = vec![Stroke {
        tool: ardoise_ink::Tool::Ballpoint,
        color: ardoise_ink::Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
        width: 60.0,
        layer: 0,
        points: [(1000.0, 1000.0), (5000.0, 3000.0)]
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| ardoise_ink::Point {
                x,
                y,
                pressure: 0.5,
                t: u32::try_from(i).unwrap() * 5,
            })
            .collect(),
    }];
    let path = library
        .create(Path::new("Classe"), &NoteFile::new(note.clone()))
        .unwrap()
        .path;
    (path, note)
}

/// A resumed sheet keeps its shape: during its run only the two positions of its way round are
/// taken (the half turn changes how the following ink lands, the sheet and its ink stay as they
/// are); the other two are refused with `boardPositionFixed`, by the switch and by the settings,
/// nothing saved, nothing turned. A change of the other options in the settings still goes
/// through.
#[test]
fn on_a_resumed_sheet_only_the_two_positions_of_its_way_round_are_taken() {
    // A wide sheet, the tablet set with its buttons on the right but held on the left.
    let stack = Stack::new(ButtonsRight, gamma(ButtonsLeft), false);
    let (path, sheet) = filed_sheet(&stack.library, board_page(ButtonsRight));
    stack.resume_live(&path);
    let written = stack.strokes(3);
    assert_eq!(
        stack.hub.board_positions(),
        [ButtonsRight, ButtonsLeft],
        "the two of its way round"
    );
    assert_eq!(
        stack.hub.snapshot().board_positions,
        [ButtonsRight, ButtonsLeft]
    );

    for refused in [ButtonsBottom, ButtonsTop] {
        let error =
            tablet_set_board_position_in(&stack.runtime, &stack.context, &stack.hub, refused)
                .unwrap_err();
        assert_eq!(error.code(), "boardPositionFixed", "{refused:?}");
        let options = TabletOptions {
            board_position: refused,
            ..TabletOptions::from(&stack.context.store.as_ref().unwrap().load().unwrap().tablet)
        };
        let error =
            tablet_set_options_in(&stack.runtime, &stack.context, &stack.hub, options).unwrap_err();
        assert_eq!(
            error.code(),
            "boardPositionFixed",
            "{refused:?}: the settings"
        );
        assert_eq!(
            stack.saved_position(),
            ButtonsRight,
            "{refused:?}: nothing saved"
        );
    }
    // The other options still change while the position stays.
    let options = TabletOptions {
        auto_sync: false,
        ..TabletOptions::from(&stack.context.store.as_ref().unwrap().load().unwrap().tablet)
    };
    tablet_set_options_in(&stack.runtime, &stack.context, &stack.hub, options).unwrap();
    assert_eq!(stack.hub.snapshot().board_position, Some(ButtonsRight));

    // The other position of its way round: the sheet and its ink stay as they are, its text frame
    // where it was typed.
    assert_eq!(stack.hub.snapshot().page.objects, sheet.objects);
    stack.switch(ButtonsLeft);
    let snapshot = stack.hub.snapshot();
    assert_eq!(snapshot.page.page, sheet.page, "the sheet keeps its shape");
    assert_eq!(
        points(&snapshot.page.strokes),
        points(&written),
        "nothing turned"
    );
    assert_eq!(snapshot.page.objects, sheet.objects, "the frame in place");
    assert!(!snapshot.can_turn_page);
    // What is written now lands upright, 1:1.
    stack.board.draw_live(gamma_at(ButtonsLeft, (-20, 10)));
    let strokes = stack.strokes(5);
    let note = Note {
        strokes: strokes[3..].to_vec(),
        ..snapshot.page.clone()
    };
    upright(&note, "after the switch, on the sheet");
    assert_eq!(stack.saved_position(), ButtonsLeft);

    let notes = stack.stop();
    let written_file = stack.library.read(&path).unwrap().file.note;
    assert_eq!(notes.len(), 1);
    assert_eq!(written_file.page, sheet.page);
    assert_eq!(
        points(&written_file.strokes[..1]),
        points(&sheet.strokes),
        "the sheet's own ink"
    );
    assert_eq!(written_file.strokes.len(), 5);
    assert_eq!(
        written_file.objects, sheet.objects,
        "the frame written in place"
    );
    // Live mode over: any position again.
    assert!(board_position_allowed(&stack.hub, ButtonsBottom).is_ok());
}
