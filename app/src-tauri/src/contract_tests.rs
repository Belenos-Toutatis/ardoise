//! The wire format of the M17 contract (docs/design/paper-projection-narration.md §10, package 0): what the interface
//! sends and receives for the live paper and image, the editor's projection and the sound while writing
//! (`lib/live.ts`, `lib/projection.ts`, `lib/video.ts`). The packages that build the commands keep their own tests;
//! these only guard the shapes they all rely on. The second tablet (M18) has its own block at the end: the status of the
//! tablet service as `lib/tablet.ts` reads it.

use serde_json::json;

use ardoise_device::service::{
    BoardSearch, Connection, DeviceDetails, DeviceMemory, FoundTablet, KnownTablet, TabletOptions,
    TabletStatus,
};
use ardoise_device::transport::TabletKind;

use crate::live::LivePageImageOp;
use crate::narration::{NarrationSaved, NarrationTarget};
use crate::projection::{ProjectionStatus, ProjectionUpdate, ProjectionView};

fn image() -> serde_json::Value {
    json!({ "name": "fiche.pdf", "pdfPage": 2, "rect": { "x": 0, "y": 0, "width": 100, "height": 200 },
            "quarterTurns": 1, "opacity": 0.5 })
}

#[test]
fn a_live_page_image_op_is_tagged_by_kind() {
    let set: LivePageImageOp =
        serde_json::from_value(json!({ "kind": "set", "key": "abc", "image": image() })).unwrap();
    assert!(
        matches!(&set, LivePageImageOp::Set { key, image } if key == "abc" && image.name == "fiche.pdf")
    );
    let place: LivePageImageOp =
        serde_json::from_value(json!({ "kind": "place", "image": image() })).unwrap();
    assert!(matches!(place, LivePageImageOp::Place { .. }));
    let remove: LivePageImageOp = serde_json::from_value(json!({ "kind": "remove" })).unwrap();
    assert_eq!(remove, LivePageImageOp::Remove);
    assert!(serde_json::from_value::<LivePageImageOp>(json!({ "kind": "place" })).is_err());
}

#[test]
fn a_projection_update_is_tagged_by_kind() {
    let view = json!({ "zoom": 1.5, "x": 100.0, "y": 50.0 });
    let kinds = [
        json!({ "kind": "view", "view": view }),
        json!({ "kind": "strokes", "keep": 3, "tail": [], "ended": null }),
        json!({ "kind": "strokes", "keep": 0, "tail": [], "ended": 7 }),
        json!({ "kind": "strokeBegin", "id": 1, "tool": "ballpoint",
                "color": { "r": 0, "g": 0, "b": 0, "a": 255 }, "width": 60.0, "layer": 0,
                "points": [{ "x": 1.0, "y": 2.0, "p": 0.5, "t": 10.0, "wall": 1.7e12 }] }),
        json!({ "kind": "strokePoints", "id": 1, "points": [] }),
        json!({ "kind": "strokeCancel", "id": 1 }),
    ];
    for kind in kinds {
        serde_json::from_value::<ProjectionUpdate>(kind.clone())
            .unwrap_or_else(|error| panic!("{kind}: {error}"));
    }
    let view: ProjectionView = serde_json::from_value(view).unwrap();
    assert_eq!(view.zoom, 1.5);
    assert!(serde_json::from_value::<ProjectionUpdate>(json!({ "kind": "nope" })).is_err());
    assert_eq!(
        serde_json::to_value(ProjectionStatus { open: None }).unwrap(),
        json!({ "open": null })
    );
}

#[test]
fn the_writing_narration_is_a_target_and_may_be_staged() {
    let target: NarrationTarget =
        serde_json::from_value(json!({ "kind": "writing", "path": "n.ardoise" })).unwrap();
    assert_eq!(
        target,
        NarrationTarget::Writing {
            path: "n.ardoise".into()
        }
    );
    let staged = NarrationSaved {
        stamp: None,
        duration_ms: 1500,
        pages: 1,
        staged: Some("k1".into()),
    };
    let value = serde_json::to_value(staged).unwrap();
    assert_eq!(value["staged"], "k1");
    let plain = NarrationSaved {
        stamp: None,
        duration_ms: 1500,
        pages: 1,
        staged: None,
    };
    assert!(serde_json::to_value(plain).unwrap().get("staged").is_none());
}

// --- M18: the Xiaomi blackboard, the second tablet (docs/design/blackboard.md §5, `lib/tablet.ts`) -------------------

/// The tablet status as the interface reads it: the new fields, in camelCase, with the kinds as words.
#[test]
fn the_tablet_status_carries_the_second_tablet() {
    let mut status = TabletStatus::initial(true, TabletOptions::default());
    status.connection = Connection::Adding;
    status.known = vec![KnownTablet {
        kind: TabletKind::Blackboard,
        name: "Mijia LCD blackboard".into(),
        address: "00:00:5E:00:53:01".into(),
        added: "2026-11-13T10:00:00+01:00".into(),
        width: 20_500,
        height: 27_200,
        model: "Xiaomi Mijia LCD blackboard".into(),
    }];
    status.board_search = Some(BoardSearch {
        found: vec![
            FoundTablet {
                address: "00:00:5E:00:53:01".into(),
                name: Some("xiaomi.dev.ble".into()),
                rssi: Some(-52),
            },
            FoundTablet {
                address: "00:00:5E:00:53:02".into(),
                name: None,
                rssi: None,
            },
        ],
        until: 1_794_000_000_000,
    });
    let value = serde_json::to_value(&status).unwrap();
    assert_eq!(value["connection"], "adding");
    assert_eq!(value["active"], json!(null));
    assert_eq!(value["registered"], json!(null));
    assert_eq!(
        value["known"][0],
        json!({ "kind": "blackboard", "name": "Mijia LCD blackboard", "address": "00:00:5E:00:53:01",
                "added": "2026-11-13T10:00:00+01:00", "width": 20500, "height": 27200,
                "model": "Xiaomi Mijia LCD blackboard" })
    );
    assert_eq!(
        value["boardSearch"],
        json!({ "found": [
            { "address": "00:00:5E:00:53:01", "name": "xiaomi.dev.ble", "rssi": -52 },
            { "address": "00:00:5E:00:53:02", "name": null, "rssi": null } ],
            "until": 1_794_000_000_000_i64 })
    );
    // The fields of the other states are there too, empty.
    assert_eq!(
        TabletStatus::initial(false, TabletOptions::default()).board_search,
        None
    );
    let idle =
        serde_json::to_value(TabletStatus::initial(false, TabletOptions::default())).unwrap();
    assert_eq!(idle["known"], json!([]));
    assert_eq!(idle["boardSearch"], json!(null));
}

/// What the blackboard says about itself: its kind, the memory, no firmware.
#[test]
fn the_details_of_the_blackboard_have_a_kind_and_a_memory() {
    let details = DeviceDetails {
        kind: TabletKind::Blackboard,
        name: "Mijia LCD blackboard".into(),
        firmware: String::new(),
        battery_percent: 90,
        charging: false,
        width: 20_500,
        height: 27_200,
        stored_pages: 3,
        model: "Xiaomi Mijia LCD blackboard".into(),
        read_at: 1_794_000_000_000,
        memory: Some(DeviceMemory {
            total: 8_000_000,
            free: 7_460_000,
        }),
    };
    let value = serde_json::to_value(&details).unwrap();
    assert_eq!(value["kind"], "blackboard");
    assert_eq!(
        value["memory"],
        json!({ "total": 8_000_000, "free": 7_460_000 })
    );
    assert_eq!(value["firmware"], "");
    assert_eq!(value["batteryPercent"], 90);
    assert_eq!(value["storedPages"], 3);
    let slate = DeviceDetails {
        kind: TabletKind::Slate,
        memory: None,
        ..details
    };
    let value = serde_json::to_value(slate).unwrap();
    assert_eq!(value["kind"], "slate");
    assert_eq!(value["memory"], json!(null));
}

// --- M18, after the review: sync confirmation, refused resume, error codes --------------------------------------------

/// `tablet_sync` takes one optional argument; Tauri turns `confirm_blackboard_delete` into the `confirmBlackboardDelete`
/// that `lib/api.ts` sends (nothing at all when it is false). Without a running service, both forms fail alike.
#[test]
fn tablet_sync_takes_the_optional_confirmation_of_the_blackboard_deletion() {
    let source = include_str!("tablet.rs");
    assert!(
        source.contains("confirm_blackboard_delete: Option<bool>"),
        "the argument `confirmBlackboardDelete` of lib/api.ts is `confirm_blackboard_delete: Option<bool>`"
    );
    let runtime = crate::tablet::TabletRuntime::default();
    for confirmed in [false, true] {
        let error = crate::tablet::tablet_sync_in(&runtime, confirmed).unwrap_err();
        assert_eq!(error.code(), "tablet");
    }
}

/// The errors of the Xiaomi paths reach the interface as `{ code, message }`, `code` selecting `errorCodes.*`.
#[test]
fn the_errors_of_the_blackboard_are_a_code_and_a_message() {
    use crate::commands::CommandError;
    for code in [
        "blackboardConfirmDelete",
        "pageUnreadable",
        "sheetOtherTablet",
    ] {
        let value = serde_json::to_value(CommandError::new(code, "detail")).unwrap();
        assert_eq!(value, json!({ "code": code, "message": "detail" }));
    }
}

/// A resume refused after it was accepted: the channel carries `{ type: 'refused', code, message }` (`lib/live.ts`),
/// then the `idle` snapshot.
#[test]
fn a_refused_resume_is_an_event_with_a_code() {
    use crate::live::model::{LiveBatch, LiveEvent};
    let batch = LiveBatch {
        events: vec![LiveEvent::Refused {
            code: "sheetOtherTablet".into(),
            message: "the blackboard cannot write on this sheet".into(),
        }],
        sent_at: 1.5,
    };
    let value = serde_json::to_value(batch).unwrap();
    assert_eq!(
        value["events"][0],
        json!({ "type": "refused", "code": "sheetOtherTablet",
                "message": "the blackboard cannot write on this sheet" })
    );
}

/// The string literals right after each `marker` in `source` (blanks and line breaks skipped): the codes a call site
/// names. A marker followed by anything else (a variable, a call) is skipped.
fn literals_after<'a>(source: &'a str, marker: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        if let Some(literal) = rest.trim_start().strip_prefix('"')
            && let Some(end) = literal.find('"')
        {
            found.push(&literal[..end]);
        }
    }
    found
}

/// `source` up to its tests.
fn without_tests(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

/// Every code the tablet paths can return, read from the code itself: each arm of `error_code` and `sync_error`
/// (`ardoise-device::service`, every `DeviceError` the service shows), the codes the worker and the service write
/// themselves (`StatusError::new`), those of the tablet commands (`tablet.rs`: `CommandError::new`, `StatusError::new`)
/// and of the live hub's refusal (`code: "…"`).
fn tablet_error_codes() -> std::collections::BTreeSet<&'static str> {
    let service = include_str!("../../../crates/ardoise-device/src/service.rs");
    let mapping = service
        .split("pub fn sync_error")
        .nth(1)
        .and_then(|rest| rest.split("impl From<&DeviceError> for StatusError").next())
        .expect("`sync_error` and `error_code` in ardoise-device/src/service.rs");
    let mut codes: std::collections::BTreeSet<&'static str> =
        literals_after(mapping, "=>").into_iter().collect();
    for source in [
        without_tests(service),
        without_tests(include_str!(
            "../../../crates/ardoise-device/src/service/worker.rs"
        )),
        without_tests(include_str!(
            "../../../crates/ardoise-device/src/service/worker/board.rs"
        )),
        without_tests(include_str!("tablet.rs")),
    ] {
        codes.extend(literals_after(source, "StatusError::new("));
        codes.extend(literals_after(source, "CommandError::new("));
    }
    codes.extend(literals_after(
        without_tests(include_str!("live/hub.rs")),
        "code: ",
    ));
    codes
}

/// Every code the tablet paths can return (the Xiaomi's included) has its sentence in both languages: a code without
/// one would show its technical detail alone. The list is read from the code, so a new code cannot be forgotten here.
#[test]
fn every_error_code_of_the_tablet_paths_has_a_text_in_both_languages() {
    let codes = tablet_error_codes();
    // The reading still finds what it must (a change of shape of `error_code` would leave it empty).
    for code in [
        "bluetoothMissing",
        "tablet",
        "transferIncomplete",
        "blackboardConfirmDelete",
        "sheetOtherTablet",
        "simulationOnly",
        "syncRefused",
    ] {
        assert!(codes.contains(code), "{code} not found in {codes:?}");
    }
    assert!(codes.len() >= 20, "{codes:?}");
    for (language, file) in [
        ("fr", include_str!("../../ui/src/locales/fr.json")),
        ("en", include_str!("../../ui/src/locales/en.json")),
    ] {
        let texts: serde_json::Value = serde_json::from_str(file).unwrap();
        for code in &codes {
            let text = texts["errorCodes"][code].as_str();
            assert!(
                text.is_some_and(|text| !text.is_empty()),
                "{language}: no text for the error code {code}"
            );
        }
    }
}

/// The live snapshot names the tablet its session writes with (`lib/live.ts`: `tablet: 'slate' | 'blackboard' | null`):
/// `null` until a tablet streams, then that tablet (docs/design/blackboard.md §5.5).
#[test]
fn the_live_snapshot_names_its_tablet() {
    use crate::live::session::LiveSession;
    use crate::live::{LiveStyle, LiveViewState};
    let session = || {
        let page = ardoise_core::note::A4_LANDSCAPE;
        LiveSession::new(
            1,
            page,
            LiveStyle::default(),
            LiveViewState::whole(page),
            false,
            0,
        )
    };
    assert_eq!(
        serde_json::to_value(session().snapshot()).unwrap()["tablet"],
        json!(null)
    );
    for (kind, word) in [
        (TabletKind::Slate, "slate"),
        (TabletKind::Blackboard, "blackboard"),
    ] {
        let mut session = session();
        session.set_tablet(kind);
        assert_eq!(
            serde_json::to_value(session.snapshot()).unwrap()["tablet"],
            word
        );
    }
}
