//! Live mode for the share (docs/design/lan-share.md, "What a viewer receives"): each viewer is a
//! subscriber of the [`LiveHub`] like a window, so it gets the batches the presentation window
//! gets (its first one starting with the snapshot and the stroke in progress), stripped of what
//! must not leave the PC.

use std::sync::Arc;

use ardoise_share::{LiveFeed, LiveViewer};

use crate::live::LiveHub;
use crate::live::hub::BatchSink;
use crate::live::model::{LiveBatch, LiveEvent, LiveSnapshot, wall_now_ms};

/// The hub's subscription label of viewer `id` (never a window's label).
pub fn subscriber_label(id: u64) -> String {
    format!("share:{id}")
}

/// [`LiveFeed`] over the app's hub.
pub struct HubFeed {
    hub: Arc<LiveHub>,
}

impl HubFeed {
    pub fn new(hub: Arc<LiveHub>) -> HubFeed {
        HubFeed { hub }
    }
}

impl LiveFeed for HubFeed {
    fn start(&self, viewer: LiveViewer) {
        let label = subscriber_label(viewer.id());
        self.hub.subscribe(&label, Arc::new(ShareSink { viewer }));
    }

    fn stop(&self, viewer: u64) {
        self.hub.unsubscribe(&subscriber_label(viewer));
    }

    fn freeze(&self) -> Option<String> {
        let batch = LiveBatch {
            events: strip_events(self.hub.current_events()),
            sent_at: wall_now_ms(),
        };
        serde_json::to_string(&batch).ok()
    }
}

/// Where the hub's flusher sends one viewer's batches.
struct ShareSink {
    viewer: LiveViewer,
}

impl BatchSink for ShareSink {
    fn send(&self, batch: LiveBatch) -> Result<(), String> {
        let batch = strip_batch(batch);
        if batch.events.is_empty() {
            // Only what is never shared (a page written to the library).
            return Ok(());
        }
        let json = serde_json::to_string(&batch).map_err(|error| error.to_string())?;
        self.viewer.send(&json).map_err(|error| error.to_string())
    }
}

/// `batch` without anything a viewer must not get.
pub fn strip_batch(batch: LiveBatch) -> LiveBatch {
    LiveBatch {
        events: strip_events(batch.events),
        sent_at: batch.sent_at,
    }
}

/// Drops the events about the library (`saved`: paths of notes; `saveFailed`: the system's
/// message) and strips the snapshots ([`strip_snapshot`]).
pub fn strip_events(events: Vec<LiveEvent>) -> Vec<LiveEvent> {
    events
        .into_iter()
        .filter_map(|event| match event {
            LiveEvent::Saved { .. } | LiveEvent::SaveFailed { .. } => None,
            LiveEvent::Snapshot { mut snapshot } => {
                strip_snapshot(&mut snapshot);
                Some(LiveEvent::Snapshot { snapshot })
            }
            other => Some(other),
        })
        .collect()
}

/// No library paths, no screen name, and of the page only what is drawn (its id, dates, size,
/// paper, layers and strokes): the title, tags, favourite mark and layer names a page may have
/// been given stay on the PC.
pub fn strip_snapshot(snapshot: &mut LiveSnapshot) {
    snapshot.saved.clear();
    snapshot.presentation = None;
    let page = &mut snapshot.page;
    page.title.clear();
    page.tags.clear();
    page.favorite = false;
    page.background_image = None;
    // Layer names are typed by the user; the viewer only needs whether a layer shows.
    for layer in &mut page.layers {
        layer.name.clear();
    }
}
