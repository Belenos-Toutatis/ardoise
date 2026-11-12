//! The wire format of the M17 contract (docs/design/paper-projection-narration.md §10, package 0): what the interface
//! sends and receives for the live paper and image, the editor's projection and the sound while writing
//! (`lib/live.ts`, `lib/projection.ts`, `lib/video.ts`). The packages that build the commands keep their own tests;
//! these only guard the shapes they all rely on. The second tablet (M18) has its own block at the end: the status of the
//! tablet service as `lib/tablet.ts` reads it.

use serde_json::json;

use ardoise_device::service::{
    BoardSearch, Connection, DeviceDetails, DeviceMemory, FoundTablet, KnownTablet, TabletOptions, TabletStatus,
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
    assert_eq!(TabletStatus::initial(false, TabletOptions::default()).board_search, None);
    let idle = serde_json::to_value(TabletStatus::initial(false, TabletOptions::default())).unwrap();
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
    assert_eq!(value["memory"], json!({ "total": 8_000_000, "free": 7_460_000 }));
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
