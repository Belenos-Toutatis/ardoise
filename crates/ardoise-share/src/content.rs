//! What the viewers are shown (`docs/design/lan-share.md`, "Content, always chosen by the
//! Ardoise PC"): the viewers and their queues, what is shown, the pause, the live
//! subscriptions handed to the [`LiveFeed`].
//!
//! Locks: the content's lock ([`Content`]) is taken before a viewer's queue lock, never the
//! other way round. The [`LiveFeed`] is called with the content's lock held (it may take the
//! hub's lock); [`LiveViewer::send`] takes only its viewer's queue lock, so the hub's flusher
//! never waits for the share.

use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

use crate::{LiveFeed, LiveViewer, NoteUpdate, Shown, ViewerGone};

/// Messages a viewer's queue holds before the viewer is disconnected as too slow, counting those
/// taken by its stream and not yet written.
pub(crate) const QUEUE_MESSAGES: usize = 1024;
/// Bytes a viewer's queue holds before the viewer is disconnected as too slow, counted the same
/// way (a single message larger than this still goes through an empty queue).
pub(crate) const QUEUE_BYTES: usize = 64 * 1024 * 1024;

/// One Server-Sent Event, ready to write.
pub(crate) type Frame = Arc<[u8]>;

/// The event whose data is `parts` put together. A line break inside the data (none in compact
/// JSON) starts a new `data:` line, as the format requires; the browser joins them with `\n`.
pub(crate) fn data_frame(parts: &[&str]) -> Frame {
    let len: usize = parts.iter().map(|part| part.len()).sum();
    let mut out = String::with_capacity(len + 8);
    out.push_str("data: ");
    for part in parts {
        let mut rest = *part;
        while let Some(at) = rest.find(['\r', '\n']) {
            out.push_str(&rest[..at]);
            out.push_str("\ndata: ");
            let skip = if rest[at..].starts_with("\r\n") { 2 } else { 1 };
            rest = &rest[at + skip..];
        }
        out.push_str(rest);
    }
    out.push_str("\n\n");
    Arc::from(out.into_bytes())
}

pub(crate) fn waiting_frame() -> Frame {
    data_frame(&[r#"{"type":"waiting"}"#])
}

pub(crate) fn pause_frame(paused: bool) -> Frame {
    data_frame(&[if paused {
        r#"{"type":"pause","paused":true}"#
    } else {
        r#"{"type":"pause","paused":false}"#
    }])
}

pub(crate) fn end_frame() -> Frame {
    data_frame(&[r#"{"type":"end"}"#])
}

/// A `live` message around a `LiveBatch` (JSON).
pub(crate) fn live_frame(reset: bool, batch_json: &str) -> Frame {
    data_frame(&[
        r#"{"type":"live","reset":"#,
        if reset { "true" } else { "false" },
        r#","batch":"#,
        batch_json,
        "}",
    ])
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One event stream: its queue of messages and its live subscription.
pub(crate) struct Viewer {
    id: u64,
    /// Where it connected from (the streams per address are limited).
    address: IpAddr,
    queue: Mutex<Queue>,
    /// Messages were queued.
    data: Notify,
    /// The queue overflowed: close the stream.
    kick: Notify,
}

#[derive(Default)]
struct Queue {
    frames: VecDeque<Frame>,
    bytes: usize,
    /// Messages (and their bytes) taken by the stream and not yet written: they still count
    /// against the limits, so that a viewer never holds more than them in memory.
    in_flight: usize,
    in_flight_bytes: usize,
    /// The current live subscription, if live batches are expected.
    live: Option<LiveSubscription>,
    /// The picture of the paper last queued for this viewer, which its browser keeps: the same one
    /// is not sent twice to it (compared by identity, see [`State::last_paper`]).
    paper: Option<Frame>,
    /// The queue overflowed: the stream is closed without `end`.
    kicked: bool,
    /// The viewer left (or the share stopped): nothing is queued any more.
    closed: bool,
}

struct LiveSubscription {
    id: u64,
    /// A first batch was queued (the next ones get `reset: false`).
    started: bool,
    /// The picture of the paper to queue right after the first batch: after its snapshot.
    paper: Option<Frame>,
}

impl Queue {
    /// Adds `frame`; a full queue is emptied and marked, and false tells the caller to wake the
    /// stream ([`Viewer::settled`]).
    fn add(&mut self, frame: Frame) -> bool {
        let messages = self.frames.len() + self.in_flight;
        let bytes = self.bytes.saturating_add(self.in_flight_bytes);
        let full = messages >= QUEUE_MESSAGES
            || (messages > 0 && bytes.saturating_add(frame.len()) > QUEUE_BYTES);
        if full {
            self.kicked = true;
            self.frames.clear();
            self.bytes = 0;
            return false;
        }
        self.bytes += frame.len();
        self.frames.push_back(frame);
        true
    }

    /// Adds the picture of the paper, unless this viewer got that one last.
    fn add_paper(&mut self, paper: &Frame) -> bool {
        if self
            .paper
            .as_ref()
            .is_some_and(|known| Arc::ptr_eq(known, paper))
        {
            return true;
        }
        self.paper = Some(Arc::clone(paper));
        self.add(Arc::clone(paper))
    }
}

/// What a stream has to write.
pub(crate) struct Pending {
    pub frames: Vec<Frame>,
    /// The viewer was too slow: close without `end`.
    pub kicked: bool,
}

impl Viewer {
    fn new(id: u64, address: IpAddr) -> Self {
        Self {
            id,
            address,
            queue: Mutex::new(Queue::default()),
            data: Notify::new(),
            kick: Notify::new(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Wakes when messages were queued.
    pub async fn queued(&self) {
        self.data.notified().await
    }

    /// Wakes when the viewer was disconnected as too slow.
    pub async fn kicked(&self) {
        self.kick.notified().await
    }

    /// Takes the queued messages; they count as in flight until [`Viewer::written`].
    pub fn take(&self) -> Pending {
        let mut queue = lock(&self.queue);
        queue.in_flight += queue.frames.len();
        queue.in_flight_bytes += std::mem::take(&mut queue.bytes);
        Pending {
            frames: queue.frames.drain(..).collect(),
            kicked: queue.kicked,
        }
    }

    /// The messages taken were written.
    pub fn written(&self) {
        let mut queue = lock(&self.queue);
        queue.in_flight = 0;
        queue.in_flight_bytes = 0;
    }

    /// Queues a message; a full queue disconnects the viewer.
    pub fn push(&self, frame: Frame) -> Result<(), ViewerGone> {
        let queue = lock(&self.queue);
        self.enqueue(queue, frame)
    }

    fn enqueue(&self, mut queue: MutexGuard<'_, Queue>, frame: Frame) -> Result<(), ViewerGone> {
        if queue.closed || queue.kicked {
            return Err(ViewerGone);
        }
        let added = queue.add(frame);
        self.settled(queue, added)
    }

    /// Lets the queue go and wakes the stream: messages to write, or (`added` false) a queue that
    /// overflowed.
    fn settled(&self, queue: MutexGuard<'_, Queue>, added: bool) -> Result<(), ViewerGone> {
        drop(queue);
        if added {
            self.data.notify_one();
            Ok(())
        } else {
            self.kick.notify_one();
            Err(ViewerGone)
        }
    }

    /// A live batch of the subscription `subscription` ([`LiveViewer::send`]).
    pub fn send_live(&self, subscription: u64, batch_json: &str) -> Result<(), ViewerGone> {
        let mut queue = lock(&self.queue);
        if queue.closed || queue.kicked {
            return Err(ViewerGone);
        }
        let (reset, paper) = match queue.live.as_mut() {
            Some(live) if live.id == subscription => (
                !std::mem::replace(&mut live.started, true),
                live.paper.take(),
            ),
            // An earlier subscription that the share stopped: its batches must not arrive
            // after what replaced it.
            _ => return Err(ViewerGone),
        };
        // Built under the lock: `reset` must be true for the first batch in the queue.
        let frame = live_frame(reset, batch_json);
        let mut added = queue.add(frame);
        // The first batch holds the snapshot: the paper comes right after it.
        if let Some(paper) = paper {
            added = added && queue.add_paper(&paper);
        }
        self.settled(queue, added)
    }

    /// Starts expecting the live batches of `subscription`, with `paper` to follow the first one;
    /// false if the viewer is gone.
    fn begin_live(&self, subscription: u64, paper: Option<Frame>) -> bool {
        let mut queue = lock(&self.queue);
        if queue.closed || queue.kicked {
            return false;
        }
        queue.live = Some(LiveSubscription {
            id: subscription,
            started: false,
            paper,
        });
        true
    }

    /// The paper of the live page changed: a viewer waiting for its snapshot gets it after that,
    /// a viewer following gets it now. No message for plain paper: the snapshot says that the page
    /// wants no picture. Nothing for a viewer out of live mode: its next subscription brings it.
    fn live_paper(&self, paper: Option<&Frame>) {
        let mut queue = lock(&self.queue);
        if queue.closed || queue.kicked {
            return;
        }
        let Some(live) = queue.live.as_mut() else {
            return;
        };
        if !live.started {
            live.paper = paper.cloned();
            return;
        }
        if let Some(paper) = paper {
            let added = queue.add_paper(paper);
            let _ = self.settled(queue, added);
        }
    }

    /// The paper of the frozen image, right after it.
    fn frozen_paper(&self, paper: &Frame) {
        let mut queue = lock(&self.queue);
        if queue.closed || queue.kicked {
            return;
        }
        let added = queue.add_paper(paper);
        let _ = self.settled(queue, added);
    }

    /// Stops expecting live batches; true if a subscription was running.
    fn end_live(&self) -> bool {
        lock(&self.queue).live.take().is_some()
    }

    /// The viewer left: nothing more is queued. True if a live subscription was running.
    fn close(&self) -> bool {
        let mut queue = lock(&self.queue);
        queue.closed = true;
        queue.frames.clear();
        queue.bytes = 0;
        queue.live.take().is_some()
    }
}

/// Why a stream cannot be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JoinRefused {
    /// The streams allowed are open (in all, or from this address).
    TooMany,
    /// The share is stopping.
    Stopped,
}

/// What is shown, to whom, and the pause.
pub(crate) struct Content {
    feed: Arc<dyn LiveFeed>,
    /// Event streams open at once, in all and per address.
    max_streams: usize,
    max_streams_per_address: usize,
    state: Mutex<State>,
    /// The viewers, the pause or what is shown changed (`on_change`).
    changed: Notify,
}

struct State {
    stopped: bool,
    shown: Shown,
    /// The last `note` message, while a note is shown.
    note: Option<Frame>,
    /// The `livePaper` message of the live page, `None` on plain paper.
    live_paper: Option<Frame>,
    /// The last `livePaper` message there was. A picture sent again (the teacher came back to the
    /// sheet) keeps this frame: viewers compare frames by identity, under their queue's lock, which
    /// the hub's flusher takes; the bytes are compared once, here.
    last_paper: Option<Frame>,
    paused: Option<Paused>,
    /// Counts the changes of content, to know at resume whether anything changed.
    version: u64,
    viewers: BTreeMap<u64, Arc<Viewer>>,
    next_viewer: u64,
    next_subscription: u64,
}

struct Paused {
    /// What viewers show while paused (and what a viewer joining during the pause gets).
    frozen: Frame,
    /// The paper of the frozen image.
    paper: Option<Frame>,
    /// [`State::version`] when paused.
    version: u64,
}

impl Content {
    pub fn new(
        feed: Arc<dyn LiveFeed>,
        max_streams: usize,
        max_streams_per_address: usize,
    ) -> Self {
        Self {
            feed,
            max_streams,
            max_streams_per_address,
            state: Mutex::new(State {
                stopped: false,
                shown: Shown::Waiting,
                note: None,
                live_paper: None,
                last_paper: None,
                paused: None,
                version: 0,
                viewers: BTreeMap::new(),
                next_viewer: 1,
                next_subscription: 1,
            }),
            changed: Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Wakes when the viewers, the pause or what is shown changed.
    pub async fn changed(&self) {
        self.changed.notified().await
    }

    fn notify_change(&self) {
        self.changed.notify_one();
    }

    /// Viewers connected, paused, what is shown.
    pub fn summary(&self) -> (usize, bool, Shown) {
        let state = self.lock();
        (state.viewers.len(), state.paused.is_some(), state.shown)
    }

    pub fn is_stopped(&self) -> bool {
        self.lock().stopped
    }

    /// Hands `viewer` to the feed under a new subscription.
    fn start_live(&self, state: &mut State, viewer: &Arc<Viewer>) {
        let subscription = state.next_subscription;
        state.next_subscription += 1;
        if viewer.begin_live(subscription, state.live_paper.clone()) {
            self.feed.start(LiveViewer {
                id: viewer.id,
                subscription,
                viewer: Arc::clone(viewer),
            });
        }
    }

    fn stop_live(&self, viewer: &Viewer) {
        if viewer.end_live() {
            self.feed.stop(viewer.id);
        }
    }

    /// Sends `viewer` what is shown, as to a viewer that just connected (pause excepted).
    fn send_current(&self, state: &mut State, viewer: &Arc<Viewer>) {
        match state.shown {
            Shown::Waiting => {
                let _ = viewer.push(waiting_frame());
            }
            Shown::Note => {
                let frame = state.note.clone().unwrap_or_else(waiting_frame);
                let _ = viewer.push(frame);
            }
            Shown::Live => self.start_live(state, viewer),
        }
    }

    fn push_all(state: &State, frame: &Frame) {
        for viewer in state.viewers.values() {
            // A full queue disconnects that viewer; its stream removes it.
            let _ = viewer.push(Arc::clone(frame));
        }
    }

    fn stop_all_live(&self, state: &State) {
        for viewer in state.viewers.values() {
            self.stop_live(viewer);
        }
    }

    /// A new event stream from `address`: it gets what is shown (or the frozen image and the
    /// pause).
    pub fn join(&self, address: IpAddr) -> Result<Arc<Viewer>, JoinRefused> {
        let mut state = self.lock();
        if state.stopped {
            return Err(JoinRefused::Stopped);
        }
        let from_address = state
            .viewers
            .values()
            .filter(|viewer| viewer.address == address)
            .count();
        if state.viewers.len() >= self.max_streams || from_address >= self.max_streams_per_address {
            return Err(JoinRefused::TooMany);
        }
        let id = state.next_viewer;
        state.next_viewer += 1;
        let viewer = Arc::new(Viewer::new(id, address));
        state.viewers.insert(id, Arc::clone(&viewer));
        if let Some(paused) = &state.paused {
            let _ = viewer.push(Arc::clone(&paused.frozen));
            if let Some(paper) = &paused.paper {
                viewer.frozen_paper(paper);
            }
            let _ = viewer.push(pause_frame(true));
        } else {
            self.send_current(&mut state, &viewer);
        }
        drop(state);
        self.notify_change();
        Ok(viewer)
    }

    /// The stream of `id` closed.
    pub fn leave(&self, id: u64) {
        let mut state = self.lock();
        let Some(viewer) = state.viewers.remove(&id) else {
            return;
        };
        if viewer.close() {
            self.feed.stop(id);
        }
        let stopped = state.stopped;
        drop(state);
        if !stopped {
            self.notify_change();
        }
    }

    pub fn show_waiting(&self) {
        let mut state = self.lock();
        if state.stopped || state.shown == Shown::Waiting {
            return;
        }
        if state.paused.is_none() {
            if state.shown == Shown::Live {
                self.stop_all_live(&state);
            }
            Self::push_all(&state, &waiting_frame());
        }
        state.shown = Shown::Waiting;
        state.note = None;
        state.version += 1;
        drop(state);
        self.notify_change();
    }

    pub fn show_live(&self) {
        let mut state = self.lock();
        if state.stopped || state.shown == Shown::Live {
            return;
        }
        state.shown = Shown::Live;
        state.note = None;
        state.version += 1;
        if state.paused.is_none() {
            let viewers: Vec<Arc<Viewer>> = state.viewers.values().cloned().collect();
            for viewer in &viewers {
                self.start_live(&mut state, viewer);
            }
        }
        drop(state);
        self.notify_change();
    }

    pub fn show_note(&self, update: NoteUpdate) {
        let full = data_frame(&[&update.full]);
        let mut state = self.lock();
        if state.stopped {
            return;
        }
        let was = state.shown;
        if was == Shown::Note && state.note.as_deref() == Some(&*full) {
            return;
        }
        if state.paused.is_none() {
            if was == Shown::Live {
                self.stop_all_live(&state);
            }
            let message = match (&update.append, was) {
                (Some(append), Shown::Note) => data_frame(&[append]),
                _ => Arc::clone(&full),
            };
            Self::push_all(&state, &message);
        }
        state.shown = Shown::Note;
        state.note = Some(full);
        state.version += 1;
        drop(state);
        if was != Shown::Note {
            self.notify_change();
        }
    }

    /// The paper of the live page (`docs/design/lan-share.md`, "The paper of a live page"): a
    /// complete `livePaper` message, `None` on plain paper. Sent outside the live batches, once to
    /// each viewer; a viewer that joins gets it after its snapshot. Recorded and sent to nobody
    /// while the share is paused or shows something else.
    pub fn show_live_paper(&self, message: Option<&str>) {
        // Built outside the lock: a picture may weigh megabytes.
        let frame = message.map(|message| data_frame(&[message]));
        let mut state = self.lock();
        if state.stopped {
            return;
        }
        let frame = frame.map(|frame| match &state.last_paper {
            Some(last) if *last == frame => Arc::clone(last),
            _ => frame,
        });
        if frame.is_some() {
            state.last_paper.clone_from(&frame);
        }
        let same = match (&state.live_paper, &frame) {
            (None, None) => true,
            (Some(current), Some(new)) => Arc::ptr_eq(current, new),
            _ => false,
        };
        if same {
            return;
        }
        state.live_paper = frame;
        if state.shown == Shown::Live && state.paused.is_none() {
            for viewer in state.viewers.values() {
                viewer.live_paper(state.live_paper.as_ref());
            }
        }
    }

    pub fn set_paused(&self, paused: bool) {
        let mut state = self.lock();
        if state.stopped || state.paused.is_some() == paused {
            return;
        }
        if paused {
            let (frozen, paper) = match state.shown {
                Shown::Live => {
                    self.stop_all_live(&state);
                    let (frozen, paper) = match self.feed.freeze() {
                        Some(batch) => (live_frame(true, &batch), state.live_paper.clone()),
                        None => (waiting_frame(), None),
                    };
                    Self::push_all(&state, &frozen);
                    if let Some(paper) = &paper {
                        for viewer in state.viewers.values() {
                            viewer.frozen_paper(paper);
                        }
                    }
                    (frozen, paper)
                }
                // The viewers show it already.
                Shown::Note => (state.note.clone().unwrap_or_else(waiting_frame), None),
                Shown::Waiting => (waiting_frame(), None),
            };
            Self::push_all(&state, &pause_frame(true));
            let version = state.version;
            state.paused = Some(Paused {
                frozen,
                paper,
                version,
            });
        } else {
            let Some(paused) = state.paused.take() else {
                return;
            };
            Self::push_all(&state, &pause_frame(false));
            if state.shown == Shown::Live || state.version != paused.version {
                let viewers: Vec<Arc<Viewer>> = state.viewers.values().cloned().collect();
                for viewer in &viewers {
                    self.send_current(&mut state, viewer);
                }
            }
        }
        drop(state);
        self.notify_change();
    }

    /// The share stops: live subscriptions end, nothing more is queued, no viewer can join.
    pub fn stop(&self) {
        let mut state = self.lock();
        if state.stopped {
            return;
        }
        state.stopped = true;
        let viewers = std::mem::take(&mut state.viewers);
        for viewer in viewers.values() {
            if viewer.close() {
                self.feed.stop(viewer.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(frame: &Frame) -> &str {
        std::str::from_utf8(frame).unwrap()
    }

    #[test]
    fn frames() {
        assert_eq!(text(&waiting_frame()), "data: {\"type\":\"waiting\"}\n\n");
        assert_eq!(
            text(&pause_frame(true)),
            "data: {\"type\":\"pause\",\"paused\":true}\n\n"
        );
        assert_eq!(text(&end_frame()), "data: {\"type\":\"end\"}\n\n");
        assert_eq!(
            text(&live_frame(true, r#"{"events":[]}"#)),
            "data: {\"type\":\"live\",\"reset\":true,\"batch\":{\"events\":[]}}\n\n"
        );
    }

    #[test]
    fn line_breaks_become_data_lines() {
        assert_eq!(
            text(&data_frame(&["a\nb\r\nc\rd"])),
            "data: a\ndata: b\ndata: c\ndata: d\n\n"
        );
        assert_eq!(text(&data_frame(&["{", "\n}"])), "data: {\ndata: }\n\n");
    }

    const HERE: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    #[test]
    fn queue_limits() {
        let viewer = Viewer::new(1, HERE);
        for _ in 0..QUEUE_MESSAGES {
            viewer.push(waiting_frame()).unwrap();
        }
        assert_eq!(viewer.push(waiting_frame()), Err(ViewerGone));
        let pending = viewer.take();
        assert!(pending.kicked);
        assert!(pending.frames.is_empty());
        assert_eq!(viewer.push(waiting_frame()), Err(ViewerGone));

        // A single message larger than the byte limit passes through an empty queue.
        let viewer = Viewer::new(2, HERE);
        let big: Frame = Arc::from(vec![b'x'; QUEUE_BYTES + 1]);
        viewer.push(Arc::clone(&big)).unwrap();
        assert_eq!(viewer.push(waiting_frame()), Err(ViewerGone));
    }

    #[test]
    fn messages_in_flight_count_until_written() {
        // Taken but not yet written: still counted.
        let viewer = Viewer::new(4, HERE);
        for _ in 0..1000 {
            viewer.push(waiting_frame()).unwrap();
        }
        assert_eq!(viewer.take().frames.len(), 1000);
        for _ in 0..QUEUE_MESSAGES - 1000 {
            viewer.push(waiting_frame()).unwrap();
        }
        assert_eq!(viewer.push(waiting_frame()), Err(ViewerGone));

        // Written: only the queued ones count.
        let viewer = Viewer::new(7, HERE);
        for _ in 0..1000 {
            viewer.push(waiting_frame()).unwrap();
        }
        viewer.take();
        viewer.written();
        for _ in 0..QUEUE_MESSAGES {
            viewer.push(waiting_frame()).unwrap();
        }
        assert_eq!(viewer.push(waiting_frame()), Err(ViewerGone));

        // The same for bytes.
        let viewer = Viewer::new(5, HERE);
        let half: Frame = Arc::from(vec![b'x'; QUEUE_BYTES / 2 + 1]);
        viewer.push(Arc::clone(&half)).unwrap();
        assert_eq!(viewer.take().frames.len(), 1);
        assert_eq!(viewer.push(Arc::clone(&half)), Err(ViewerGone));
        let viewer = Viewer::new(6, HERE);
        viewer.push(Arc::clone(&half)).unwrap();
        viewer.take();
        viewer.written();
        viewer.push(Arc::clone(&half)).unwrap();
    }

    struct NoFeed;

    impl LiveFeed for NoFeed {
        fn start(&self, _: LiveViewer) {}
        fn stop(&self, _: u64) {}
        fn freeze(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn streams_per_address() {
        let content = Content::new(Arc::new(NoFeed), 5, 2);
        let a = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 2));
        let b = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 3));
        let first = content.join(a).unwrap();
        content.join(a).unwrap();
        assert_eq!(content.join(a).err(), Some(JoinRefused::TooMany));
        content.join(b).unwrap();
        content.join(b).unwrap();
        content.join(HERE).unwrap();
        assert_eq!(
            content.join(HERE).err(),
            Some(JoinRefused::TooMany),
            "5 in all"
        );
        content.leave(first.id());
        content.join(a).unwrap();
        content.stop();
        assert_eq!(content.join(b).err(), Some(JoinRefused::Stopped));
    }

    #[test]
    fn live_subscriptions() {
        let viewer = Viewer::new(3, HERE);
        assert_eq!(viewer.send_live(1, "{}"), Err(ViewerGone));
        assert!(viewer.begin_live(1, None));
        viewer.send_live(1, "{}").unwrap();
        viewer.send_live(1, "{}").unwrap();
        assert!(viewer.end_live());
        assert!(viewer.begin_live(2, None));
        assert_eq!(
            viewer.send_live(1, "{}"),
            Err(ViewerGone),
            "stale subscription"
        );
        viewer.send_live(2, "{}").unwrap();
        let frames: Vec<String> = viewer
            .take()
            .frames
            .iter()
            .map(|f| text(f).to_owned())
            .collect();
        assert_eq!(
            frames,
            [
                "data: {\"type\":\"live\",\"reset\":true,\"batch\":{}}\n\n",
                "data: {\"type\":\"live\",\"reset\":false,\"batch\":{}}\n\n",
                "data: {\"type\":\"live\",\"reset\":true,\"batch\":{}}\n\n",
            ]
        );
        assert!(viewer.close());
        assert_eq!(viewer.send_live(2, "{}"), Err(ViewerGone));
        assert!(!viewer.begin_live(3, None));
    }

    #[test]
    fn the_paper_follows_the_first_batch_and_goes_once() {
        let paper = data_frame(&[r#"{"type":"livePaper"}"#]);
        let viewer = Viewer::new(8, HERE);
        assert!(viewer.begin_live(1, Some(Arc::clone(&paper))));
        let taken = |viewer: &Viewer| {
            let frames = viewer.take().frames;
            viewer.written();
            frames.len()
        };
        assert_eq!(taken(&viewer), 0);
        viewer.send_live(1, "{}").unwrap();
        assert_eq!(taken(&viewer), 2, "the snapshot, then the paper");
        viewer.send_live(1, "{}").unwrap();
        assert_eq!(taken(&viewer), 1);
        // The same picture, whatever comes through: only once for this viewer.
        viewer.live_paper(Some(&paper));
        viewer.frozen_paper(&paper);
        assert_eq!(taken(&viewer), 0);
        assert!(viewer.end_live());
        assert!(viewer.begin_live(2, Some(Arc::clone(&paper))));
        viewer.send_live(2, "{}").unwrap();
        assert_eq!(taken(&viewer), 1, "the snapshot alone");

        // A paper that changes before the first batch replaces the one waiting; none cancels it.
        let viewer = Viewer::new(9, HERE);
        assert!(viewer.begin_live(1, Some(Arc::clone(&paper))));
        viewer.live_paper(None);
        viewer.send_live(1, "{}").unwrap();
        assert_eq!(taken(&viewer), 1);
    }

    #[test]
    fn a_paper_too_large_for_the_queue_disconnects_the_viewer() {
        let big: Frame = Arc::from(vec![b'x'; QUEUE_BYTES / 2 + 1]);
        let other: Frame = Arc::from(vec![b'y'; QUEUE_BYTES / 2 + 1]);
        let viewer = Viewer::new(10, HERE);
        assert!(viewer.begin_live(1, Some(big)));
        viewer.send_live(1, "{}").unwrap();
        viewer.live_paper(Some(&other));
        assert!(viewer.take().kicked);
    }
}
