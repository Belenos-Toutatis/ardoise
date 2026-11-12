//! The wire format of the M17 contract (docs/design/paper-projection-narration.md §10, package 0): what the interface
//! sends and receives for the live paper and image, the editor's projection and the sound while writing
//! (`lib/live.ts`, `lib/projection.ts`, `lib/video.ts`). The packages that build the commands keep their own tests;
//! these only guard the shapes they all rely on.

use serde_json::json;

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
