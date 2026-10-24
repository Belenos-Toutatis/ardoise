//! The simulated Bamboo Slate: what it stores, what it answers, its button and its pen.
//!
//! [`SimTablet`] is a cheap handle (clone it freely) to one simulated tablet. The connector
//! ([`SimTablet::connector`]) gives the device stack a [`Connector`] over it; the test or the
//! [`SimulatedUser`](crate::SimulatedUser) performs the physical actions (hold or press the
//! button, draw pages, move the pen in live mode) and injects faults.
//!
//! # Behaviour
//!
//! Facts from protocol.md §2–§7, plus the assumptions marked below (the real tablet has not
//! been observed doing these; they are the most plausible reading and are documented in the
//! crate's tests):
//!
//! - One stored host identity. `hold_button` enters registration mode (4-byte manufacturer
//!   data). There, `e7 06 <id>` gets no reply until `press_button`, which stores the id,
//!   answers `e4 01 00` and ends registration mode. `e7` outside registration mode:
//!   `b3 01 01`.
//! - `e6 06 <id>`: `b3 01 00` for the stored id, `b3 01 07` otherwise.
//! - **Assumption:** every other known command sent before a successful `e6` or registration
//!   on the link is refused with `b3 01 02` (invalid state).
//! - **Assumption:** unknown opcodes get `b3 01 04` (command not supported), even before
//!   authentication; known opcodes with malformed arguments get `b3 01 01`.
//! - Time get/set in BCD UTC, kept as an offset from the virtual wall clock; name in `bc`
//!   chunks ending with `0a`; firmware halves; battery; dimensions; `ec`; modes (live 0, paper
//!   1, idle 2).
//! - Pages: `c1` count, `cc` size and BCD time of the oldest page, `c3` -> `c8 01 be`, the page
//!   bytes in notifications of at most 20 bytes on [`Channel::PageData`], then
//!   `c8 05 ed <crc32 LE>` on [`Channel::Command`]; `ca` deletes the oldest page.
//!   **Assumptions:** `cc`, `c3` and `ca` with no stored page get `b3 01 02`; page commands in
//!   live mode get `b3 01 02`; drawing in live mode does not store a page.
//! - Live mode streams the queued pen movement ([`SimTablet::draw_live`]): `a2` when the pen
//!   comes near, `a1` notifications of up to 3 records, one record per 5 ms, an all-`ff`
//!   record when the pen leaves. **Assumption:** the stream starts again only when the host
//!   sends `b1 00` on a new connection; pen movement made while not streaming is lost; when
//!   streaming resumes with the pen already near, an `a2` is sent first.
//! - **Assumption:** a new connection starts in paper mode.
//! - Right after a connection the tablet sends the sysevent `f1 02 13 00` (seen on a real
//!   Slate, issue #212; meaning unknown).
//! - One connection at a time: the tablet does not advertise while connected, and a second
//!   `connect` times out. A host disconnection, a lost link or a dropped transport frees it.
//!
//! # Virtual time
//!
//! Replies are due at the time of the request, page bytes too; live records 5 ms apart from
//! the start of live mode, an `a1` notification being due with its last record. `recv` follows
//! the rule of docs/design/device-stack.md: if the next event is due at or before
//! `now + timeout` the clock jumps to its due time and it is delivered, otherwise the clock
//! jumps by `timeout`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ardoise_device::transport::{Advertisement, Channel, TransportError, TransportEvent};
use ardoise_proto::live::{self, LiveRecord, MAX_RECORDS_PER_NOTIFICATION};
use ardoise_proto::stroke::{self, Stroke, StrokeTime};
use ardoise_proto::{
    BcdTime, Dimension, FirmwarePart, MAX_NOTIFICATION_PAYLOAD, Mode, Reply, Request, StatusCode,
};

use crate::clock::{VirtualClock, millis, unix_seconds};
use crate::connector::SimConnector;
use crate::pen::{self, PenEvent, PenScript, SAMPLE_INTERVAL, TimedPen};

/// Sysevent a real Slate sends right after a connection (issue #212), meaning unknown.
pub const CONNECT_SYSEVENT: [u8; 4] = [0xf1, 0x02, 0x13, 0x00];

/// The Slate stroke-header clock is a 16-bit count of 5 ms ticks: it wraps after this long.
const STROKE_CLOCK_WRAP_MS: u64 = 65_536 * 5;
/// Pause the simulated hand makes between two strokes of a page (for the stroke times).
const STROKE_GAP_MS: u64 = 200;

/// What the simulated tablet is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimConfig {
    /// Advertised and reported name.
    pub name: String,
    /// Bluetooth address (fixed, so runs are reproducible).
    pub address: String,
    /// First half of the firmware version (`b7 01 00`).
    pub firmware_high: String,
    /// Second half of the firmware version (`b7 01 01`).
    pub firmware_low: String,
    /// Drawing area in units of 10 µm.
    pub width: u32,
    pub height: u32,
    pub battery_percent: u8,
    pub charging: bool,
    /// Signal strength reported while scanning, in dBm.
    pub rssi: i16,
    /// Company identifier of the manufacturer data.
    pub company_id: u16,
    /// Manufacturer data in normal mode (7 bytes on a real tablet; content invented).
    pub manufacturer_data: Vec<u8>,
    /// Manufacturer data in registration mode (4 bytes on a real tablet; content invented).
    pub register_manufacturer_data: Vec<u8>,
    /// Tablet clock minus the virtual wall clock before the host sets it, in seconds.
    pub clock_offset: i64,
    /// Sent on [`Channel::SysEvent`] right after each connection, if any.
    pub connect_sysevent: Option<Vec<u8>>,
    /// The tablet drops the link when the host wrote nothing for this long (issue #97), if set.
    pub idle_timeout: Option<Duration>,
}

impl SimConfig {
    /// Bamboo Slate Large (A4): 29 700 × 21 600, firmware `B160805A-C170704A` (issue #117).
    pub fn slate_large() -> Self {
        Self {
            name: "Bamboo Slate".into(),
            address: "D4:5A:2E:7B:3C:91".into(),
            firmware_high: "B160805A".into(),
            firmware_low: "C170704A".into(),
            width: ardoise_proto::SLATE_LARGE_WIDTH,
            height: ardoise_proto::SLATE_LARGE_HEIGHT,
            battery_percent: 87,
            charging: false,
            rssi: -58,
            company_id: 0x4755,
            manufacturer_data: vec![0x91, 0x3c, 0x7b, 0x2e, 0x5a, 0xd4, 0x00],
            register_manufacturer_data: vec![0x91, 0x3c, 0x7b, 0x2e],
            clock_offset: 0,
            connect_sysevent: Some(CONNECT_SYSEVENT.to_vec()),
            idle_timeout: None,
        }
    }

    /// The small Bamboo Slate (A5): 21 600 × 14 800, otherwise like [`SimConfig::slate_large`].
    pub fn slate_small() -> Self {
        Self {
            width: ardoise_proto::SLATE_SMALL_WIDTH,
            height: ardoise_proto::SLATE_SMALL_HEIGHT,
            address: "D4:5A:2E:7B:3C:92".into(),
            ..Self::slate_large()
        }
    }
}

/// A page stored on the simulated tablet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPage {
    /// The strokes as stored, stamped with the tablet's time since power-up.
    pub strokes: Vec<Stroke>,
    /// The file the tablet sends for this page (Slate format).
    pub bytes: Vec<u8>,
    /// When the page was stored, on the tablet clock (unix seconds).
    pub timestamp: i64,
}

/// Counters of what happened on the simulated tablet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SimStats {
    /// Successful connections.
    pub connections: u32,
    /// Links lost by the tablet (faults, idle timeout, power off).
    pub links_lost: u32,
    /// Frames written by hosts.
    pub requests: u32,
    /// Page downloads started (`c3` with a page).
    pub downloads: u32,
    /// Pen samples streamed in live mode (records with a position).
    pub live_samples: u64,
    /// `a2` notifications streamed.
    pub proximity_in: u32,
}

/// Handle to one simulated tablet. Clones share the same tablet.
#[derive(Debug, Clone)]
pub struct SimTablet {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    clock: Arc<VirtualClock>,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    config: SimConfig,
    powered: bool,
    register_mode: bool,
    registered: Option<[u8; 6]>,
    clock_offset: i64,
    pages: VecDeque<StoredPage>,
    /// Strokes drawn on the current page, not stored yet.
    sketch: Vec<Stroke>,
    mode: Mode,
    /// Pen movement waiting for live mode.
    pen_queue: VecDeque<PenScript>,
    /// Pen movement anchored on the virtual clock.
    pen: Vec<TimedPen>,
    /// End of the anchored pen movement.
    pen_end: Duration,
    link: Option<Link>,
    next_link_id: u64,
    /// The link connected at this time is lost (fault injection).
    drop_at: Option<Duration>,
    corrupt_next_transfer: bool,
    stats: SimStats,
}

/// The tablet side of one connection.
#[derive(Debug)]
struct Link {
    id: u64,
    authenticated: bool,
    /// Identity received with `e7`, waiting for the button.
    pending_registration: Option<[u8; 6]>,
    queue: VecDeque<Outgoing>,
    /// Present while live mode streams on this link.
    stream: Option<LiveStream>,
    last_write: Duration,
}

/// A notification waiting to be received.
#[derive(Debug)]
struct Outgoing {
    due: Duration,
    channel: Channel,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct LiveStream {
    /// Next event of `State::pen` to stream.
    cursor: usize,
    /// The host was told the pen is near (an `a2` since the last "pen left" record).
    near: bool,
}

/// The next live notification of a stream.
struct LiveItem {
    due: Duration,
    data: Vec<u8>,
    /// Pen events it covers.
    consumed: usize,
    /// Whether the pen is near after it.
    near: bool,
    samples: u64,
    proximity_in: bool,
}

impl SimTablet {
    /// A tablet with its own virtual clock (monotonic 0, wall clock at
    /// [`SIM_EPOCH_UNIX`](crate::SIM_EPOCH_UNIX)), switched on, never registered, no page.
    pub fn new(config: SimConfig) -> Self {
        Self::with_clock(config, Arc::new(VirtualClock::new()))
    }

    /// A tablet driven by an existing clock. The clock's monotonic 0 is the tablet's power-up.
    pub fn with_clock(config: SimConfig, clock: Arc<VirtualClock>) -> Self {
        let clock_offset = config.clock_offset;
        Self {
            shared: Arc::new(Shared {
                clock,
                state: Mutex::new(State {
                    config,
                    powered: true,
                    register_mode: false,
                    registered: None,
                    clock_offset,
                    pages: VecDeque::new(),
                    sketch: Vec::new(),
                    mode: Mode::Paper,
                    pen_queue: VecDeque::new(),
                    pen: Vec::new(),
                    pen_end: Duration::ZERO,
                    link: None,
                    next_link_id: 1,
                    drop_at: None,
                    corrupt_next_transfer: false,
                    stats: SimStats::default(),
                }),
            }),
        }
    }

    /// The default simulated tablet: a Bamboo Slate Large ([`SimConfig::slate_large`]).
    pub fn slate_large() -> Self {
        Self::new(SimConfig::slate_large())
    }

    /// A [`Connector`](ardoise_device::transport::Connector) over this tablet.
    pub fn connector(&self) -> SimConnector {
        SimConnector::new(self.clone())
    }

    /// The virtual clock of this tablet and of its connector.
    pub fn clock(&self) -> Arc<VirtualClock> {
        self.shared.clock.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> Duration {
        self.shared.clock.monotonic()
    }

    fn wall_seconds(&self) -> i64 {
        unix_seconds(self.shared.clock.now())
    }

    // --- physical actions -----------------------------------------------------------------

    /// Holds the button about 6 s: the tablet enters registration mode (LED blinking blue,
    /// 4-byte manufacturer data) until a host registers.
    pub fn hold_button(&self) {
        self.state().register_mode = true;
    }

    /// Presses the button briefly. During a registration (a host sent `e7`) it confirms it:
    /// the identity is stored and `e4 01 00` sent. In registration mode without `e7` it does
    /// nothing. Otherwise it stores the page drawn so far, if any.
    pub fn press_button(&self) {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut guard = self.state();
        let state = &mut *guard;
        let alive = state.alive(now);
        if alive
            && let Some(link) = state.link.as_mut()
            && let Some(id) = link.pending_registration.take()
        {
            state.registered = Some(id);
            state.register_mode = false;
            link.authenticated = true;
            link.queue.push_back(Outgoing {
                due: now,
                channel: Channel::Command,
                data: encode(&Reply::RegisterButtonPressed),
            });
            return;
        }
        if state.register_mode {
            return;
        }
        state.store_sketch(wall);
    }

    /// Draws `strokes` on the current page (stored by the next button press). Each stroke is
    /// stamped with the tablet's time since power-up, as the tablet does; strokes without
    /// points are ignored.
    pub fn draw(&self, strokes: &[Stroke]) {
        let now = self.now();
        let mut state = self.state();
        let stamped = stamp(strokes, now);
        state.sketch.extend(stamped);
    }

    /// Draws `strokes` on a new page and presses the button to store it.
    pub fn draw_page(&self, strokes: &[Stroke]) {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut state = self.state();
        let stamped = stamp(strokes, now);
        state.sketch.extend(stamped);
        state.store_sketch(wall);
    }

    /// Queues pen movement for live mode. It starts when live mode starts (or right away if
    /// live mode is streaming), after any movement queued before.
    pub fn draw_live(&self, script: PenScript) {
        let now = self.now();
        let mut guard = self.state();
        let state = &mut *guard;
        state.pen_queue.push_back(script);
        if state.alive(now) && state.link.as_ref().is_some_and(|l| l.stream.is_some()) {
            state.anchor_pen(now);
        }
    }

    /// Changes the battery state reported by `b9`.
    pub fn set_battery(&self, percent: u8, charging: bool) {
        let mut state = self.state();
        state.config.battery_percent = percent;
        state.config.charging = charging;
    }

    /// Replaces the stored identity, as when another application (Inkspace, the phone app)
    /// registers the tablet. `None` forgets it.
    pub fn set_registered_id(&self, id: Option<[u8; 6]>) {
        self.state().registered = id;
    }

    /// Sets the tablet clock to the virtual wall clock plus `seconds`.
    pub fn set_clock_offset(&self, seconds: i64) {
        self.state().clock_offset = seconds;
    }

    /// Switches the tablet off: the link (if any) is lost now and the tablet is not seen
    /// any more.
    pub fn power_off(&self) {
        let now = self.now();
        let mut state = self.state();
        state.powered = false;
        if state.link.is_some() {
            state.drop_at = Some(now);
        }
    }

    /// Switches the tablet on again.
    pub fn power_on(&self) {
        self.state().powered = true;
    }

    // --- faults ---------------------------------------------------------------------------

    /// The link connected `after` from now (on the virtual clock) is lost at that moment, as
    /// when the tablet goes out of range: events due before are delivered, then
    /// [`TransportEvent::Disconnected`]. The tablet is free for a new connection right away.
    pub fn disconnect_after(&self, after: Duration) {
        let at = self.now().saturating_add(after);
        self.state().drop_at = Some(at);
    }

    /// The next page download sends one corrupted byte (with the CRC of the right bytes).
    pub fn corrupt_next_transfer(&self) {
        self.state().corrupt_next_transfer = true;
    }

    // --- inspection -----------------------------------------------------------------------

    /// The pages stored on the tablet, oldest first.
    pub fn stored_pages(&self) -> Vec<StoredPage> {
        self.state().pages.iter().cloned().collect()
    }

    pub fn stored_page_count(&self) -> usize {
        self.state().pages.len()
    }

    /// The host identity the tablet accepts, if any.
    pub fn registered_id(&self) -> Option<[u8; 6]> {
        self.state().registered
    }

    /// Current mode (a new connection starts in paper mode).
    pub fn mode(&self) -> Mode {
        self.state().mode
    }

    /// Whether the tablet waits to be registered (after [`SimTablet::hold_button`]).
    pub fn in_register_mode(&self) -> bool {
        self.state().register_mode
    }

    /// Whether a host is connected.
    pub fn is_connected(&self) -> bool {
        let now = self.now();
        self.state().alive(now)
    }

    /// The tablet clock, in unix seconds.
    pub fn tablet_time(&self) -> i64 {
        let wall = self.wall_seconds();
        wall + self.state().clock_offset
    }

    pub fn stats(&self) -> SimStats {
        self.state().stats
    }

    /// The configuration, with the current name and battery state.
    pub fn config(&self) -> SimConfig {
        self.state().config.clone()
    }

    /// What a scan sees now: `None` while switched off or connected.
    pub fn advertisement(&self) -> Option<Advertisement> {
        let now = self.now();
        let state = self.state();
        (state.powered && !state.alive(now)).then(|| state.advertisement())
    }

    // --- link (used by the connector and its transports) ------------------------------------

    /// Opens a link; returns its id and the tablet as advertised.
    pub(crate) fn open_link(
        &self,
        address: &str,
        timeout: Duration,
    ) -> Result<(u64, Advertisement), TransportError> {
        let clock = &self.shared.clock;
        let now = clock.monotonic();
        let mut guard = self.state();
        let state = &mut *guard;
        if !address.eq_ignore_ascii_case(&state.config.address) {
            return Err(TransportError::NotFound(address.to_owned()));
        }
        if !state.powered || state.alive(now) {
            // Switched off, or busy with another host: nothing answers until the timeout.
            clock.advance(timeout);
            return Err(TransportError::Timeout);
        }
        if state.link.is_some() {
            // A link that died without anyone noticing.
            state.lose_link(now);
        }
        if state.drop_at.is_some_and(|at| at <= now) {
            state.drop_at = None;
        }
        let id = state.next_link_id;
        state.next_link_id += 1;
        let mut queue = VecDeque::new();
        if let Some(data) = &state.config.connect_sysevent {
            queue.push_back(Outgoing {
                due: now,
                channel: Channel::SysEvent,
                data: data.clone(),
            });
        }
        state.link = Some(Link {
            id,
            authenticated: false,
            pending_registration: None,
            queue,
            stream: None,
            last_write: now,
        });
        state.mode = Mode::Paper;
        state.stats.connections += 1;
        Ok((id, state.advertisement()))
    }

    /// A host write on link `id`.
    pub(crate) fn link_write(&self, id: u64, bytes: &[u8]) -> Result<(), TransportError> {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut guard = self.state();
        let state = &mut *guard;
        if !state.link.as_ref().is_some_and(|l| l.id == id) || !state.alive(now) {
            return Err(TransportError::NotConnected);
        }
        state.stats.requests += 1;
        if let Some(link) = state.link.as_mut() {
            link.last_write = now;
        }
        // Live data already sent travels before the reply.
        state.materialize_live(now);
        let replies = state.handle(bytes, now, wall);
        if let Some(link) = state.link.as_mut() {
            link.queue
                .extend(replies.into_iter().map(|(channel, data)| Outgoing {
                    due: now,
                    channel,
                    data,
                }));
        }
        Ok(())
    }

    /// Waits up to `timeout` (virtual time) for the next event of link `id`.
    pub(crate) fn link_recv(
        &self,
        id: u64,
        timeout: Duration,
    ) -> Result<Option<TransportEvent>, TransportError> {
        let clock = &self.shared.clock;
        let now = clock.monotonic();
        let horizon = now.saturating_add(timeout);
        let mut guard = self.state();
        let state = &mut *guard;
        let Some(link) = state.link.as_ref().filter(|l| l.id == id) else {
            // The link was lost and replaced (or forgotten) before this transport noticed.
            return Ok(Some(TransportEvent::Disconnected));
        };
        let end = state.link_end(link);
        let queued = link.queue.front().map(|o| o.due);
        let live = link.stream.and_then(|stream| peek_live(&state.pen, stream));
        let next = match (queued, live.as_ref().map(|l| l.due)) {
            (Some(q), Some(l)) => Some(q.min(l)),
            (q, l) => q.or(l),
        };

        if let Some(end) = end
            && end <= horizon
            && next.is_none_or(|due| due >= end)
        {
            clock.advance_to(end);
            state.lose_link(end);
            return Ok(Some(TransportEvent::Disconnected));
        }
        match next {
            Some(due) if due <= horizon => {
                clock.advance_to(due);
                let from_queue = queued.is_some_and(|q| live.as_ref().is_none_or(|l| q <= l.due));
                if from_queue {
                    let outgoing = state.link.as_mut().and_then(|l| l.queue.pop_front());
                    Ok(outgoing.map(|o| TransportEvent::Notification {
                        channel: o.channel,
                        data: o.data,
                    }))
                } else {
                    Ok(live.map(|item| {
                        state.consume_live(&item);
                        TransportEvent::Notification {
                            channel: Channel::Live,
                            data: item.data,
                        }
                    }))
                }
            }
            _ => {
                clock.advance_to(horizon);
                Ok(None)
            }
        }
    }

    /// The host closes link `id` (or drops its transport).
    pub(crate) fn close_link(&self, id: u64) {
        let mut state = self.state();
        if state.link.as_ref().is_some_and(|l| l.id == id) {
            state.link = None;
            state.mode = Mode::Paper;
        }
    }
}

impl State {
    /// When the current link will be lost, if a fault or the idle timeout says so.
    fn link_end(&self, link: &Link) -> Option<Duration> {
        let idle = self.config.idle_timeout.map(|t| link.last_write + t);
        match (self.drop_at, idle) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Whether a link is up at `now`.
    fn alive(&self, now: Duration) -> bool {
        self.link
            .as_ref()
            .is_some_and(|link| self.link_end(link).is_none_or(|end| end > now))
    }

    fn lose_link(&mut self, at: Duration) {
        self.link = None;
        self.mode = Mode::Paper;
        if self.drop_at.is_some_and(|drop| drop <= at) {
            self.drop_at = None;
        }
        self.stats.links_lost += 1;
    }

    fn advertisement(&self) -> Advertisement {
        Advertisement {
            address: self.config.address.clone(),
            name: Some(self.config.name.clone()),
            rssi: Some(self.config.rssi),
            company_id: Some(self.config.company_id),
            manufacturer_data: if self.register_mode {
                self.config.register_manufacturer_data.clone()
            } else {
                self.config.manufacturer_data.clone()
            },
        }
    }

    fn store_sketch(&mut self, wall: i64) {
        if self.sketch.is_empty() {
            return;
        }
        let strokes = std::mem::take(&mut self.sketch);
        let bytes = stroke::encode_slate(&strokes);
        self.pages.push_back(StoredPage {
            strokes,
            bytes,
            timestamp: wall + self.clock_offset,
        });
    }

    /// Gives a time to the pen movement waiting for live mode, after what is already anchored.
    fn anchor_pen(&mut self, now: Duration) {
        while let Some(script) = self.pen_queue.pop_front() {
            let start = self.pen_end.max(now);
            self.pen.extend(pen::timeline(&script, start));
            self.pen_end = start + script.duration();
        }
    }

    /// Moves the live notifications due by `now` from the stream to the queue.
    fn materialize_live(&mut self, now: Duration) {
        loop {
            let Some(stream) = self.link.as_ref().and_then(|l| l.stream) else {
                return;
            };
            let Some(item) = peek_live(&self.pen, stream) else {
                return;
            };
            if item.due > now {
                return;
            }
            self.consume_live(&item);
            if let Some(link) = self.link.as_mut() {
                link.queue.push_back(Outgoing {
                    due: item.due,
                    channel: Channel::Live,
                    data: item.data,
                });
            }
        }
    }

    fn consume_live(&mut self, item: &LiveItem) {
        if let Some(stream) = self.link.as_mut().and_then(|l| l.stream.as_mut()) {
            stream.cursor += item.consumed;
            stream.near = item.near;
        }
        self.stats.live_samples += item.samples;
        if item.proximity_in {
            self.stats.proximity_in += 1;
        }
    }

    fn set_mode(&mut self, mode: Mode, now: Duration) {
        self.mode = mode;
        if mode != Mode::Live {
            if let Some(link) = self.link.as_mut() {
                link.stream = None;
            }
            return;
        }
        if self.link.as_ref().is_none_or(|l| l.stream.is_some()) {
            return;
        }
        self.anchor_pen(now);
        // Movement made before now was not streamed: it is lost.
        self.pen.retain(|event| event.due >= now);
        if matches!(
            self.pen.first(),
            Some(TimedPen {
                event: PenEvent::Record(LiveRecord::ProximityOut),
                ..
            })
        ) {
            self.pen.remove(0);
        }
        if let Some(link) = self.link.as_mut() {
            link.stream = Some(LiveStream {
                cursor: 0,
                near: false,
            });
        }
    }

    /// The replies to one host write: `(channel, bytes)` notifications, due now.
    fn handle(&mut self, bytes: &[u8], now: Duration, wall: i64) -> Vec<(Channel, Vec<u8>)> {
        let request = match Request::decode(bytes) {
            Ok(Request::Unknown(_)) => return status(StatusCode::CommandNotSupported),
            Ok(request) => request,
            Err(_) => return status(StatusCode::GeneralError),
        };
        let authenticated = self.link.as_ref().is_some_and(|l| l.authenticated);
        let page_command = matches!(
            request,
            Request::AvailableFilesCount
                | Request::GetFileInfo
                | Request::DownloadOldestFile
                | Request::DeleteOldestFile
        );
        match request {
            Request::Connect { uuid } => {
                let accepted = self.registered == Some(uuid);
                if let Some(link) = self.link.as_mut() {
                    link.authenticated = accepted;
                }
                status(if accepted {
                    StatusCode::Success
                } else {
                    StatusCode::AuthorizationError
                })
            }
            Request::RegisterPressButton { uuid } => {
                if !self.register_mode {
                    return status(StatusCode::GeneralError);
                }
                if let Some(link) = self.link.as_mut() {
                    link.pending_registration = Some(uuid);
                }
                Vec::new()
            }
            _ if !authenticated => status(StatusCode::InvalidState),
            _ if page_command && self.mode == Mode::Live => status(StatusCode::InvalidState),
            Request::GetName => Reply::name_chunks(&self.config.name)
                .iter()
                .map(command)
                .collect(),
            Request::SetName(name) => {
                self.config.name = name;
                status(StatusCode::Success)
            }
            Request::GetTime => match BcdTime::from_unix(wall + self.clock_offset) {
                Ok(time) => vec![command(&Reply::Time(time))],
                Err(_) => status(StatusCode::GeneralError),
            },
            Request::SetTime(time) => {
                self.clock_offset = time.to_unix() - wall;
                status(StatusCode::Success)
            }
            Request::GetFirmware { part } => {
                let text = match part {
                    FirmwarePart::High => self.config.firmware_high.clone(),
                    FirmwarePart::Low => self.config.firmware_low.clone(),
                };
                vec![command(&Reply::Firmware { part, text })]
            }
            Request::GetBattery => vec![command(&Reply::Battery {
                percent: self.config.battery_percent,
                charging: self.config.charging,
            })],
            Request::GetDimension(which) => {
                let value = match which {
                    Dimension::Width => self.config.width,
                    Dimension::Height => self.config.height,
                };
                vec![command(&Reply::Dimension { which, value })]
            }
            Request::SetFileTransferReportingType => status(StatusCode::Success),
            Request::SetMode(mode) => {
                self.set_mode(mode, now);
                status(StatusCode::Success)
            }
            Request::AvailableFilesCount => {
                let count = u16::try_from(self.pages.len()).unwrap_or(u16::MAX);
                vec![command(&Reply::FilesCount(count))]
            }
            Request::GetFileInfo => match self.pages.front() {
                None => status(StatusCode::InvalidState),
                Some(page) => match BcdTime::from_unix(page.timestamp) {
                    Ok(time) => vec![command(&Reply::FileInfo {
                        size: u32::try_from(page.bytes.len()).unwrap_or(u32::MAX),
                        time,
                    })],
                    Err(_) => status(StatusCode::GeneralError),
                },
            },
            Request::DownloadOldestFile => self.download(),
            Request::DeleteOldestFile => match self.pages.pop_front() {
                Some(_) => status(StatusCode::Success),
                None => status(StatusCode::InvalidState),
            },
            Request::Unknown(_) => status(StatusCode::CommandNotSupported),
        }
    }

    fn download(&mut self) -> Vec<(Channel, Vec<u8>)> {
        let Some(page) = self.pages.front() else {
            return status(StatusCode::InvalidState);
        };
        let crc = ardoise_proto::crc32(&page.bytes);
        let mut data = page.bytes.clone();
        if std::mem::take(&mut self.corrupt_next_transfer)
            && let Some(byte) = data.get_mut(page.bytes.len() / 2)
        {
            *byte ^= 0x5a;
        }
        self.stats.downloads += 1;
        let mut out = vec![command(&Reply::DownloadStart { extra: Vec::new() })];
        out.extend(
            data.chunks(MAX_NOTIFICATION_PAYLOAD)
                .map(|chunk| (Channel::PageData, chunk.to_vec())),
        );
        out.push(command(&Reply::DownloadEnd { crc }));
        out
    }
}

/// The next live notification of `stream`, without consuming it.
fn peek_live(pen: &[TimedPen], stream: LiveStream) -> Option<LiveItem> {
    let rest = pen.get(stream.cursor..)?;
    let first = rest.first()?;
    let proximity_in = |consumed| LiveItem {
        due: first.due,
        data: live::encode_proximity_in(millis(first.due)),
        consumed,
        near: true,
        samples: 0,
        proximity_in: true,
    };
    match first.event {
        PenEvent::Enter => Some(proximity_in(1)),
        // Streaming resumed while the pen was near: say so first.
        PenEvent::Record(_) if !stream.near => Some(proximity_in(0)),
        PenEvent::Record(_) => {
            let mut records = Vec::with_capacity(MAX_RECORDS_PER_NOTIFICATION);
            let mut due = first.due;
            for event in rest.iter().take(MAX_RECORDS_PER_NOTIFICATION) {
                let PenEvent::Record(record) = event.event else {
                    break;
                };
                records.push(record);
                due = event.due;
                if record == LiveRecord::ProximityOut {
                    break;
                }
            }
            let samples = records
                .iter()
                .filter(|r| matches!(r, LiveRecord::Pen(_)))
                .count();
            Some(LiveItem {
                due,
                data: live::encode_samples(&records).into_iter().next()?,
                consumed: records.len(),
                near: records.last() != Some(&LiveRecord::ProximityOut),
                samples: u64::try_from(samples).unwrap_or(u64::MAX),
                proximity_in: false,
            })
        }
    }
}

/// Stamps strokes drawn at `now` with the tablet's time since power-up, as the tablet stores
/// them (5 ms ticks, wrapping like the 16-bit counter of the Slate stroke header).
fn stamp(strokes: &[Stroke], now: Duration) -> Vec<Stroke> {
    let mut at = millis(now) / 5 * 5;
    let interval = millis(SAMPLE_INTERVAL);
    strokes
        .iter()
        .filter(|s| !s.points.is_empty())
        .map(|s| {
            let stamp_ms = u32::try_from(at % STROKE_CLOCK_WRAP_MS).unwrap_or(0);
            let points = u64::try_from(s.points.len()).unwrap_or(u64::MAX);
            at = at
                .saturating_add(points.saturating_mul(interval))
                .saturating_add(STROKE_GAP_MS);
            Stroke {
                time: StrokeTime::SincePowerUp { millis: stamp_ms },
                pen: None,
                points: s.points.clone(),
                lost_points: s.lost_points,
            }
        })
        .collect()
}

/// Encodes a reply; the replies built here always fit a frame.
fn encode(reply: &Reply) -> Vec<u8> {
    reply.encode().unwrap_or_else(|_| vec![0xb3, 0x01, 0x01])
}

fn command(reply: &Reply) -> (Channel, Vec<u8>) {
    (Channel::Command, encode(reply))
}

fn status(code: StatusCode) -> Vec<(Channel, Vec<u8>)> {
    vec![command(&Reply::Status(code))]
}
