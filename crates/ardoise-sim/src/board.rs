//! The simulated Xiaomi blackboard (docs/design/blackboard.md): what it stores, what it
//! answers, its "store" and "erase" buttons and its pen.
//!
//! [`SimBoard`] is a cheap handle (clone it freely) to one simulated blackboard. Connectors
//! reach it like the simulated Slate ([`SimConnector::over`](crate::SimConnector::over)); the
//! test or [`SimHooks`](crate::SimHooks) performs the physical actions (write, store, erase)
//! and injects faults.
//!
//! # Behaviour
//!
//! Facts from docs/research/xiaomi-blackboard.md §3 and §6, plus the assumptions marked below:
//!
//! - No login: every known request is answered at once, on [`Channel::Command`]; the frames
//!   are those of [`ardoise_proto::blackboard`]. `11 05 07` sets the clock, `12 05 06`
//!   deletes, `12 02 08` starts or stops the stream: no reply. `12 01 05` describes the oldest
//!   page, or answers the bare `12 01 05` when none is stored (measured on the real tablet),
//!   and also when it is asked before any `12 01 04` on the link although a page is stored
//!   (measured once, research note §6).
//! - `12 06 03 01 <time>`: the page's bytes on [`Channel::BoardData`] in chunks of at most 244
//!   bytes, nothing else. **Assumption:** a time that names no page gets no answer. Deleting a
//!   page that is not there does nothing (measured: no reply, the count unchanged).
//! - **Assumption:** unknown or malformed requests get no answer (counted in
//!   [`BoardStats::unknown_requests`]).
//! - The pen writes on the screen whether a host listens or not. While a host streams
//!   (`12 02 08 01`), the records follow on [`Channel::BoardData`], up to six per notification,
//!   [`RECORDS_PER_SECOND`] of them per second: `a0` hovering, `a1` touching, `c0` when the pen
//!   leaves. Movement is timed from when it is given ([`SimBoard::draw_live`]), as a hand
//!   writes: what happens while nobody streams is only on the screen. A new link does not
//!   stream until asked (measured: live mode is not kept across connections).
//! - The store button ([`SimBoard::press_store`]) stores the screen as a page dated by the
//!   tablet's clock and sends `12 01 09` to a connected host, streaming or not. Two pages never
//!   share a time (**assumption**: one second apart at least). **Assumption:** an empty screen
//!   stores nothing and sends nothing.
//!
//! ## The open page (measured, research note §6)
//!
//! - The page of a screen not erased since it was stored is the *open* page
//!   ([`SimBoard::open_page`]), whether a host was connected or not, streaming or not. It is
//!   described in the long form `12 0d 05 <time> <size> <previous>`: `previous` is the time of
//!   the store before it of the same screen, 0 for the first. Every other page is described in
//!   the short form.
//! - Storing again without erasing replaces the open page by one holding the whole screen, with
//!   a new time; when the host already took it, the new page is added, its `previous` still the
//!   time of the store taken.
//! - **The open page is removed by the tablet itself once all its bytes have been sent for a
//!   download** (three times out of three), counted in [`BoardStats::removed_after_download`]. A
//!   page in the short form stays until it is deleted. **Assumption** (not measured): a download
//!   cut short leaves the open page on the tablet: a stalled download
//!   ([`SimBoard::stall_next_download`]) or a link lost before the last chunk; a delayed one
//!   ([`SimBoard::delay_next_download`]) removes it once its rest is sent.
//! - **Assumption:** switching the tablet off changes nothing to the open page.
//!
//! ## The erase button ([`SimBoard::erase`])
//!
//! It clears the screen and closes the open page (short form from then on, kept). It sends
//! `11 01 0c` while a host streams, nothing otherwise. What it does to the open page reproduces
//! the observed cases, not a rule (the tablet's rule is not established):
//!
//! | Erased | The open page | Observed | Here |
//! |---|---|---|---|
//! | outside live mode | described since it was stored | the whole page pushed on the stream | pushed in chunks of at most 244 bytes, kept closed |
//! | outside live mode | not described | closed, kept | the same |
//! | in live mode | described during this live run | `11 01 0c`, then the whole page pushed | the same |
//! | in live mode | stored during this run, not described | `11 01 0c`, nothing pushed | kept closed |
//! | in live mode | stored and described before this run | lost (twice) | removed ([`BoardStats::lost_pages`]) |
//! | in live mode | stored before this run, not described | not measured | **assumption**: removed, the side of loss |
//!
//! Starting live mode alone removes nothing (measured). **Assumption:** a page pushed stays,
//! closed (the general rule of the button; not checked after a push). A live run starts with
//! each `12 02 08 01` and ends with `12 02 08 02` or its link.
//!
//! ## Links
//!
//! - One connection at a time; no advertisement while connected (the PC did not hear the
//!   tablet while the phone held it).
//! - **Assumption:** a transfer ends with its link: what a download had still to send is lost
//!   when the link closes (to check at the hardware checkpoint; the host reopens the link to be
//!   rid of a transfer that did not end cleanly).
//!
//! # Faults
//!
//! [`SimBoard::power_off`], [`SimBoard::disconnect_after`],
//! [`SimBoard::disconnect_at_next_request`]; downloads that stop
//! ([`SimBoard::stall_next_download`]) or whose rest comes late, just after the host's next
//! download request on the same link ([`SimBoard::delay_next_download`]); downloads whose chunks
//! come over time ([`SimBoard::pace_downloads`], about 10 ms apart in Mi Home's capture; at once
//! otherwise); stream bytes sent now, nobody asking ([`SimBoard::send_stream`]); a pen streamed to
//! every new link without `12 02 08 01` ([`SimBoard::stream_without_asking`], a tablet that kept
//! live mode across a reconnection: not measured); a delete ignored
//! ([`SimBoard::ignore_next_delete`]) or slow ([`SimBoard::delay_next_delete`]); a page that is
//! not records ([`SimBoard::store_raw_page`]); a store of the screen whose page is not the screen
//! ([`SimBoard::store_raw_screen`]); a page taken by another application
//! ([`SimBoard::page_taken_elsewhere`]); an action run when the next download starts
//! ([`SimBoard::on_next_download`], to land a request in the middle of it) or right after the
//! next delete ([`SimBoard::on_next_delete`]).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ardoise_device::transport::{Advertisement, Channel, TransportError, TransportEvent};
use ardoise_proto::blackboard::beacon::blackboard_service_data;
use ardoise_proto::blackboard::record::{RECORDS_PER_SECOND, encode_live, split_chunks};
use ardoise_proto::blackboard::{
    BOARD_HEIGHT, BOARD_WIDTH, BoardReply, BoardRequest, MAX_LIVE_RECORDS, PageInfo, Record,
    RecordStatus, record::encode_records,
};
use ardoise_proto::stroke::Stroke;

use crate::clock::{VirtualClock, unix_seconds};
use crate::pen::{PenScript, PenSegment};

/// Time between two records of the pen.
pub const BOARD_RECORD_INTERVAL: Duration =
    Duration::from_nanos(1_000_000_000 / RECORDS_PER_SECOND as u64);

/// What the simulated blackboard is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimBoardConfig {
    /// Advertised name (the real tablet's is generic).
    pub name: String,
    /// Bluetooth address, in the range reserved for documentation (RFC 7042).
    pub address: String,
    pub battery_percent: u8,
    /// Memory in bytes (the real tablet's: about 121 MB).
    pub memory_total: u32,
    /// Signal strength while scanning, in dBm.
    pub rssi: i16,
    /// Tablet clock minus the virtual wall clock before the host sets it, in seconds.
    pub clock_offset: i64,
    /// The tablet drops a link on which the host wrote nothing for this long, if set
    /// (**assumption**: the real one kept a silent link 80 s; unknown beyond).
    pub idle_timeout: Option<Duration>,
}

impl SimBoardConfig {
    /// The blackboard of the research note, battery 90 %.
    pub fn blackboard() -> Self {
        Self {
            name: "xiaomi.dev.ble".into(),
            address: "00:00:5E:00:53:B1".into(),
            battery_percent: 90,
            memory_total: 126_909_952,
            rssi: -55,
            clock_offset: 0,
            idle_timeout: None,
        }
    }
}

/// A page stored on the simulated blackboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardStoredPage {
    /// When it was stored, on the tablet's clock (Unix seconds).
    pub time: u32,
    /// Its records, as the tablet sends them.
    pub bytes: Vec<u8>,
}

/// Counters of what happened on the simulated blackboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BoardStats {
    pub connections: u32,
    pub links_lost: u32,
    /// Frames written by hosts.
    pub requests: u32,
    /// Frames that are not a known request (no answer).
    pub unknown_requests: u32,
    /// Downloads of a stored page started.
    pub downloads: u32,
    /// Pages deleted by a host.
    pub deletes: u32,
    /// Records streamed in live mode.
    pub live_records: u64,
    /// `12 01 09` events sent.
    pub page_stored_events: u32,
    /// `11 01 0c` events sent.
    pub screen_erased_events: u32,
    /// Pages described in the long form (`12 0d 05`).
    pub long_descriptions: u32,
    /// `12 01 05` answered bare because no `12 01 04` was asked yet on the link.
    pub uncounted_descriptions: u32,
    /// Open pages the tablet removed by itself once downloaded.
    pub removed_after_download: u32,
    /// Pages pushed on the stream by the erase button.
    pub pushed_pages: u32,
    /// Open pages lost by the erase button during live mode.
    pub lost_pages: u32,
}

/// Handle to one simulated blackboard. Clones share the same tablet.
#[derive(Debug, Clone)]
pub struct SimBoard {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    clock: Arc<VirtualClock>,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    config: SimBoardConfig,
    powered: bool,
    clock_offset: i64,
    pages: VecDeque<BoardStoredPage>,
    /// What the screen shows: every record written since the last erase.
    screen: Vec<Record>,
    /// The time of the last store of the screen since it was erased (the `previous` of the next
    /// store), whether the tablet still holds that page or not.
    last_store: Option<u32>,
    /// The open page: the page holding the screen as stored last, while the tablet holds it and
    /// the screen is not erased.
    open: Option<OpenPage>,
    /// Live runs started so far (each `12 02 08 01`, and each link streaming unasked).
    live_runs: u64,
    /// Pen movement, anchored on the virtual clock.
    pen: Vec<Timed>,
    /// End of the anchored movement.
    pen_end: Duration,
    /// Records of `pen` already on the screen.
    on_screen: usize,
    link: Option<Link>,
    next_link_id: u64,
    drop_at: Option<Duration>,
    /// The next downloads send only these many bytes each, then stall or send the rest late.
    stalls: VecDeque<Stall>,
    /// The next delete does nothing.
    ignore_next_delete: bool,
    /// The next delete takes effect only this long after it is asked.
    delete_delay: Option<Duration>,
    /// A delete asked, not done yet: (time of the page, when it is done).
    pending_delete: Option<(u32, Duration)>,
    /// The link drops when the host writes its next request.
    drop_at_next_request: bool,
    /// Run when the next download starts (after its first bytes are queued).
    on_download: Option<Action>,
    /// Run right after the next delete request is handled.
    on_delete: Option<Action>,
    /// Time between two chunks of a download (zero: all at once).
    pace: Duration,
    /// Every new link streams the pen at once, without `12 02 08 01`.
    stream_unasked: bool,
    /// Set by a download of the open page answered in full: the page to remove once its last
    /// chunk is sent.
    removes_after: Option<u32>,
    stats: BoardStats,
}

/// The page of the screen not erased since it was stored.
#[derive(Debug, Clone, Copy)]
struct OpenPage {
    time: u32,
    /// The store before it of the same screen, 0 for the first.
    previous: u32,
    /// The live run streaming when it was stored, if any.
    stored_during: Option<u64>,
    /// It was described to a host since it was stored: `Some(run)` with the live run streaming
    /// then (`Some(None)`: none).
    described: Option<Option<u64>>,
}

/// A download that does not go as it should.
#[derive(Debug, Clone, Copy)]
struct Stall {
    /// Bytes sent at once.
    bytes: usize,
    /// The rest comes when the host writes its next request on the same link (else never).
    late: bool,
}

/// An action of the test, run on the host's thread.
struct Action(Box<dyn FnOnce() + Send>);

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Action")
    }
}

#[derive(Debug, Clone, Copy)]
struct Timed {
    due: Duration,
    record: Record,
}

#[derive(Debug)]
struct Link {
    id: u64,
    queue: VecDeque<Outgoing>,
    /// Next record of `pen` to stream, while the host streams.
    stream: Option<usize>,
    /// The live run of this link while it streams.
    run: u64,
    /// `12 01 04` was asked on this link.
    counted: bool,
    last_write: Duration,
    /// The rest of a download held back ([`SimBoard::delay_next_download`]): sent just after
    /// the host's next download request on this link, before that download's own bytes; lost
    /// with the link.
    late_tail: Vec<Vec<u8>>,
    /// The open page whose last bytes are in `late_tail` (removed once they are sent).
    late_removes: Option<u32>,
}

#[derive(Debug)]
struct Outgoing {
    due: Duration,
    channel: Channel,
    data: Vec<u8>,
    /// The last chunk of a download of the open page of this time: once it is sent, the tablet
    /// removes that page.
    removes: Option<u32>,
}

impl Outgoing {
    fn now(due: Duration, channel: Channel, data: Vec<u8>) -> Self {
        Self {
            due,
            channel,
            data,
            removes: None,
        }
    }
}

/// The next notification of a stream.
struct LiveItem {
    due: Duration,
    data: Vec<u8>,
    consumed: usize,
}

impl SimBoard {
    /// A blackboard with its own virtual clock, switched on, empty.
    pub fn new(config: SimBoardConfig) -> Self {
        Self::with_clock(config, Arc::new(VirtualClock::new()))
    }

    /// A blackboard on an existing clock (the simulated Slate's, so one connector reaches both).
    pub fn with_clock(config: SimBoardConfig, clock: Arc<VirtualClock>) -> Self {
        let clock_offset = config.clock_offset;
        Self {
            shared: Arc::new(Shared {
                clock,
                state: Mutex::new(State {
                    config,
                    powered: true,
                    clock_offset,
                    pages: VecDeque::new(),
                    screen: Vec::new(),
                    last_store: None,
                    open: None,
                    live_runs: 0,
                    pen: Vec::new(),
                    pen_end: Duration::ZERO,
                    on_screen: 0,
                    link: None,
                    next_link_id: 1,
                    drop_at: None,
                    stalls: VecDeque::new(),
                    ignore_next_delete: false,
                    delete_delay: None,
                    pending_delete: None,
                    drop_at_next_request: false,
                    on_download: None,
                    on_delete: None,
                    pace: Duration::ZERO,
                    stream_unasked: false,
                    removes_after: None,
                    stats: BoardStats::default(),
                }),
            }),
        }
    }

    /// A blackboard whose clock follows the computer's, for the real-time connector.
    pub fn real_time(config: SimBoardConfig) -> Self {
        Self::with_clock(config, Arc::new(VirtualClock::following_real_time()))
    }

    /// The default simulated blackboard ([`SimBoardConfig::blackboard`]).
    pub fn blackboard() -> Self {
        Self::new(SimBoardConfig::blackboard())
    }

    /// A [`Connector`](ardoise_device::transport::Connector) over this blackboard alone.
    pub fn connector(&self) -> crate::SimConnector {
        crate::SimConnector::over(vec![crate::SimDevice::Board(self.clone())])
    }

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

    /// The pen moves as `script` says, from now (after the movement given before): on the
    /// screen, and to a host that streams. Points 5 ms apart in the script are played at the
    /// blackboard's rate ([`BOARD_RECORD_INTERVAL`]).
    pub fn draw_live(&self, script: PenScript) {
        let now = self.now();
        let mut state = self.state();
        let start = state.pen_end.max(now);
        let (records, end) = timeline(&script, start);
        state.pen.extend(records);
        state.pen_end = end;
    }

    /// Writes `strokes` on the screen at once (hovering to each stroke, touching along it,
    /// leaving after it), nothing else: the screen keeps them until it is erased.
    pub fn write(&self, strokes: &[Stroke]) {
        let now = self.now();
        let mut state = self.state();
        state.catch_up(now);
        state.screen.extend(records_of(strokes));
    }

    /// Writes `strokes` on the screen at once (as [`write`](Self::write)), stores the screen as
    /// a page, and erases it: one call, one page.
    pub fn draw_page(&self, strokes: &[Stroke]) {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut state = self.state();
        state.catch_up(now);
        state.screen.extend(records_of(strokes));
        state.store(now, wall);
        state.erase(now);
    }

    /// Presses the store button: the screen becomes a page (see the module documentation).
    pub fn press_store(&self) {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut state = self.state();
        state.catch_up(now);
        state.store(now, wall);
    }

    /// Presses the erase button: the screen is cleared, and a host that streams is told
    /// (`11 01 0c`).
    pub fn erase(&self) {
        let now = self.now();
        let mut state = self.state();
        state.catch_up(now);
        state.erase(now);
    }

    pub fn set_battery(&self, percent: u8) {
        self.state().config.battery_percent = percent;
    }

    pub fn power_off(&self) {
        let now = self.now();
        let mut state = self.state();
        state.powered = false;
        if state.link.is_some() {
            state.lose_link(now);
        }
    }

    pub fn power_on(&self) {
        self.state().powered = true;
    }

    /// The link connected at that moment is lost `after` from now (fault injection).
    pub fn disconnect_after(&self, after: Duration) {
        let at = self.now() + after;
        self.state().drop_at = Some(at);
    }

    /// The next download (after those already stalled) sends only `bytes` bytes of the page,
    /// then nothing.
    pub fn stall_next_download(&self, bytes: usize) {
        self.state().stalls.push_back(Stall { bytes, late: false });
    }

    /// The next download (after those already stalled) sends `bytes` bytes of the page at once
    /// and the rest only once the host asks for a page again on the same link, before the bytes
    /// of that new download: a stalled transfer that resumes just as the host tries again (the
    /// worst moment). The rest is lost if the link closes first.
    pub fn delay_next_download(&self, bytes: usize) {
        self.state().stalls.push_back(Stall { bytes, late: true });
    }

    /// The next delete request does nothing.
    pub fn ignore_next_delete(&self) {
        self.state().ignore_next_delete = true;
    }

    /// The next delete request takes effect `after` this long: until then the page is still
    /// described and counted (a tablet slow to erase).
    pub fn delay_next_delete(&self, after: Duration) {
        self.state().delete_delay = Some(after);
    }

    /// The link drops when the host writes its next request (the write fails).
    pub fn disconnect_at_next_request(&self) {
        self.state().drop_at_next_request = true;
    }

    /// Stores `bytes` as a page, as they are (a tablet storing something that is not records),
    /// dated by the tablet's clock; nothing is sent to a host. A page in the short form.
    pub fn store_raw_page(&self, bytes: Vec<u8>) {
        let wall = self.wall_seconds();
        self.state().push_page(bytes, wall);
    }

    /// The page of `time` leaves the tablet without this host deleting it, as when another
    /// application imported it (Mi Home deletes each page it imports): a fault, for a page that
    /// is no longer described when the host looks again.
    pub fn page_taken_elsewhere(&self, time: u32) {
        let mut state = self.state();
        state.pages.retain(|page| page.time != time);
        if state.open.is_some_and(|open| open.time == time) {
            state.open = None;
        }
    }

    /// Presses the store button, but the page stored holds `bytes` instead of the screen's
    /// records: an open page whose `previous` is the screen's last store, which it does not hold
    /// whole (not measured: a fault, for the host's checks).
    pub fn store_raw_screen(&self, bytes: Vec<u8>) {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut state = self.state();
        state.catch_up(now);
        state.store_bytes(bytes, now, wall);
    }

    /// Runs `action` on the host's thread when the next download starts, once its first bytes
    /// are queued (to land a request in the middle of a download).
    pub fn on_next_download(&self, action: impl FnOnce() + Send + 'static) {
        self.state().on_download = Some(Action(Box::new(action)));
    }

    /// Runs `action` on the host's thread right after the next delete request is handled
    /// (whatever the delete did).
    pub fn on_next_delete(&self, action: impl FnOnce() + Send + 'static) {
        self.state().on_delete = Some(Action(Box::new(action)));
    }

    /// From the next download on, the chunks of a page come `interval` apart on the virtual
    /// clock (the first at once), as a real transfer takes time; `Duration::ZERO` (the default)
    /// sends them all at once. What a download had still to send is lost with its link.
    pub fn pace_downloads(&self, interval: Duration) {
        self.state().pace = interval;
    }

    /// The tablet sends `bytes` on the stream now, to the connected host, in chunks of at most
    /// 244 bytes, nobody asking (more of a transfer that was thought finished). Nothing without a
    /// link.
    pub fn send_stream(&self, bytes: &[u8]) {
        let now = self.now();
        let mut state = self.state();
        if !state.alive(now) {
            return;
        }
        if let Some(link) = state.link.as_mut() {
            for data in split_chunks(bytes) {
                enqueue(
                    &mut link.queue,
                    Outgoing::now(now, Channel::BoardData, data),
                );
            }
        }
    }

    /// While `on`, every new link streams the pen from the moment it opens, without
    /// `12 02 08 01` (a tablet that kept live mode across a reconnection: not measured).
    pub fn stream_without_asking(&self, on: bool) {
        self.state().stream_unasked = on;
    }

    // --- inspection -------------------------------------------------------------------------

    pub fn stored_pages(&self) -> Vec<BoardStoredPage> {
        self.state().pages.iter().cloned().collect()
    }

    pub fn stored_page_count(&self) -> usize {
        self.state().pages.len()
    }

    /// The time of the open page (the page of the screen not erased since it was stored), while
    /// the tablet holds it.
    pub fn open_page(&self) -> Option<u32> {
        self.state().open.map(|open| open.time)
    }

    /// What the screen shows now.
    pub fn screen(&self) -> Vec<Record> {
        let now = self.now();
        let mut state = self.state();
        state.catch_up(now);
        state.screen.clone()
    }

    pub fn is_streaming(&self) -> bool {
        let now = self.now();
        let state = self.state();
        state.alive(now) && state.link.as_ref().is_some_and(|l| l.stream.is_some())
    }

    pub fn is_connected(&self) -> bool {
        let now = self.now();
        self.state().alive(now)
    }

    /// The tablet's clock, Unix seconds.
    pub fn tablet_time(&self) -> i64 {
        self.wall_seconds() + self.state().clock_offset
    }

    pub fn stats(&self) -> BoardStats {
        self.state().stats
    }

    pub fn config(&self) -> SimBoardConfig {
        self.state().config.clone()
    }

    /// What a scan sees now: `None` while switched off or connected.
    pub fn advertisement(&self) -> Option<Advertisement> {
        let now = self.now();
        let state = self.state();
        (state.powered && !state.alive(now)).then(|| state.advertisement())
    }

    // --- link (used by the connectors and their transports) ---------------------------------

    pub(crate) fn open_link(
        &self,
        address: &str,
        timeout: Duration,
    ) -> Result<(u64, Advertisement), TransportError> {
        let clock = &self.shared.clock;
        let now = clock.monotonic();
        let mut state = self.state();
        if !address.eq_ignore_ascii_case(&state.config.address) {
            return Err(TransportError::NotFound(address.to_owned()));
        }
        if !state.powered || state.alive(now) {
            clock.advance(timeout);
            return Err(TransportError::Timeout);
        }
        if state.link.is_some() {
            state.lose_link(now);
        }
        if state.drop_at.is_some_and(|at| at <= now) {
            state.drop_at = None;
        }
        let id = state.next_link_id;
        state.next_link_id += 1;
        let stream = state
            .stream_unasked
            .then(|| state.pen.partition_point(|timed| timed.due < now));
        if stream.is_some() {
            state.live_runs += 1;
        }
        let run = state.live_runs;
        state.link = Some(Link {
            id,
            queue: VecDeque::new(),
            stream,
            run,
            counted: false,
            last_write: now,
            late_tail: Vec::new(),
            late_removes: None,
        });
        state.stats.connections += 1;
        Ok((id, state.advertisement()))
    }

    pub(crate) fn link_write(&self, id: u64, bytes: &[u8]) -> Result<(), TransportError> {
        let now = self.now();
        let wall = self.wall_seconds();
        let mut state = self.state();
        if !state.link.as_ref().is_some_and(|l| l.id == id) || !state.alive(now) {
            return Err(TransportError::NotConnected);
        }
        if std::mem::take(&mut state.drop_at_next_request) {
            state.lose_link(now);
            return Err(TransportError::NotConnected);
        }
        state.stats.requests += 1;
        if let Some(link) = state.link.as_mut() {
            link.last_write = now;
        }
        // Records already due travel before the reply; so does the rest of a download held
        // back, when the host asks for a page again.
        state.materialize_live(now);
        let request = BoardRequest::decode(bytes);
        let download = matches!(request, Ok(BoardRequest::Download { .. }));
        let delete = matches!(request, Ok(BoardRequest::Delete { .. }));
        if download && let Some(link) = state.link.as_mut() {
            let tail = std::mem::take(&mut link.late_tail);
            let removes = link.late_removes.take();
            let last = tail.len();
            for (n, data) in tail.into_iter().enumerate() {
                let mut chunk = Outgoing::now(now, Channel::BoardData, data);
                if n + 1 == last {
                    chunk.removes = removes;
                }
                enqueue(&mut link.queue, chunk);
            }
        }
        let downloads = state.stats.downloads;
        let replies = state.handle(bytes, now, wall);
        // The open page leaves the tablet once the last chunk of its download is sent.
        let removes = state.removes_after.take();
        // A download's or a push's chunks one `pace` apart (all at once by default).
        let pace = state.pace;
        if let Some(link) = state.link.as_mut() {
            let mut chunks = 0u32;
            let last = replies
                .iter()
                .rposition(|(channel, _)| *channel == Channel::BoardData);
            for (n, (channel, data)) in replies.into_iter().enumerate() {
                let due = if channel == Channel::BoardData {
                    chunks += 1;
                    now + pace * (chunks - 1)
                } else {
                    now
                };
                let mut item = Outgoing::now(due, channel, data);
                if Some(n) == last {
                    item.removes = removes;
                }
                enqueue(&mut link.queue, item);
            }
        }
        let action = if state.stats.downloads > downloads {
            state.on_download.take()
        } else if delete {
            state.on_delete.take()
        } else {
            None
        };
        // The test's action runs without the tablet's lock.
        drop(state);
        if let Some(Action(action)) = action {
            action();
        }
        Ok(())
    }

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
            return Ok(Some(TransportEvent::Disconnected));
        };
        let end = state.link_end(link);
        let queued = link.queue.front().map(|o| o.due);
        let live = link.stream.and_then(|cursor| peek_live(&state.pen, cursor));
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
                    if let Some(time) = outgoing.as_ref().and_then(|o| o.removes) {
                        state.downloaded(time);
                    }
                    Ok(outgoing.map(|o| TransportEvent::Notification {
                        channel: o.channel,
                        data: o.data,
                    }))
                } else {
                    Ok(live.map(|item| {
                        state.consume_live(&item);
                        TransportEvent::Notification {
                            channel: Channel::BoardData,
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

    pub(crate) fn link_next_due(&self, id: u64) -> Option<Duration> {
        let state = self.state();
        let Some(link) = state.link.as_ref().filter(|l| l.id == id) else {
            return Some(Duration::ZERO);
        };
        let queued = link.queue.front().map(|o| o.due);
        let live = link
            .stream
            .and_then(|cursor| peek_live(&state.pen, cursor))
            .map(|item| item.due);
        [queued, live, state.link_end(link)]
            .into_iter()
            .flatten()
            .min()
    }

    pub(crate) fn close_link(&self, id: u64) {
        let mut state = self.state();
        if state.link.as_ref().is_some_and(|l| l.id == id) {
            state.link = None;
        }
    }
}

impl State {
    fn link_end(&self, link: &Link) -> Option<Duration> {
        let idle = self.config.idle_timeout.map(|t| link.last_write + t);
        match (self.drop_at, idle) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn alive(&self, now: Duration) -> bool {
        self.link
            .as_ref()
            .is_some_and(|link| self.link_end(link).is_none_or(|end| end > now))
    }

    fn lose_link(&mut self, at: Duration) {
        self.link = None;
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
            company_id: None,
            manufacturer_data: Vec::new(),
            xiaomi_service_data: blackboard_service_data(address_bytes(&self.config.address), 0),
        }
    }

    /// Puts the pen records due by `now` on the screen.
    fn catch_up(&mut self, now: Duration) {
        while let Some(timed) = self.pen.get(self.on_screen)
            && timed.due <= now
        {
            self.screen.push(timed.record);
            self.on_screen += 1;
        }
    }

    fn tablet_seconds(&self, wall: i64) -> u32 {
        u32::try_from((wall + self.clock_offset).max(0)).unwrap_or(u32::MAX)
    }

    /// Stores `bytes` as the newest page, dated by the tablet's clock (one second after the
    /// newest page at least); returns its time.
    fn push_page(&mut self, bytes: Vec<u8>, wall: i64) -> u32 {
        let mut time = self.tablet_seconds(wall);
        if let Some(last) = self.pages.iter().map(|page| page.time).max() {
            time = time.max(last.saturating_add(1));
        }
        self.pages.push_back(BoardStoredPage { time, bytes });
        time
    }

    /// The live run of the connected host while it streams.
    fn streaming_run(&self, now: Duration) -> Option<u64> {
        self.link
            .as_ref()
            .filter(|link| link.stream.is_some() && self.alive(now))
            .map(|link| link.run)
    }

    /// The store button.
    fn store(&mut self, now: Duration, wall: i64) {
        if self.screen.is_empty() {
            return;
        }
        let bytes = encode_records(&self.screen);
        self.store_bytes(bytes, now, wall);
    }

    /// Stores `bytes` as the page of the screen: it replaces the open page (or follows the
    /// store the host took) and is the open page from now on.
    fn store_bytes(&mut self, bytes: Vec<u8>, now: Duration, wall: i64) {
        if let Some(open) = self.open.take() {
            self.pages.retain(|page| page.time != open.time);
        }
        let previous = self.last_store.unwrap_or(0);
        let time = self.push_page(bytes, wall);
        self.last_store = Some(time);
        self.open = Some(OpenPage {
            time,
            previous,
            stored_during: self.streaming_run(now),
            described: None,
        });
        if self.alive(now)
            && let Some(link) = self.link.as_mut()
        {
            enqueue(
                &mut link.queue,
                Outgoing::now(now, Channel::Command, encode(&BoardReply::PageStored)),
            );
            self.stats.page_stored_events += 1;
        }
    }

    /// The erase button: the cases observed (module documentation).
    fn erase(&mut self, now: Duration) {
        self.screen.clear();
        self.last_store = None;
        let run = self.streaming_run(now);
        if run.is_some()
            && let Some(link) = self.link.as_mut()
        {
            enqueue(
                &mut link.queue,
                Outgoing::now(now, Channel::Command, encode(&BoardReply::ScreenErased)),
            );
            self.stats.screen_erased_events += 1;
        }
        let Some(open) = self.open.take() else {
            return;
        };
        let push = match run {
            // Described during this live run: pushed after `11 01 0c`.
            Some(run) if open.described == Some(Some(run)) => true,
            // Stored before this live run: lost (described then: measured twice; not described:
            // supposed).
            Some(run) if open.stored_during != Some(run) => {
                self.pages.retain(|page| page.time != open.time);
                self.stats.lost_pages += 1;
                return;
            }
            // Stored during this run, not described: kept, closed.
            Some(_) => false,
            // Outside live mode: pushed when described since it was stored.
            None => open.described.is_some(),
        };
        let bytes = self
            .pages
            .iter()
            .find(|page| page.time == open.time)
            .map(|page| page.bytes.clone());
        if push
            && self.alive(now)
            && let (Some(bytes), Some(link)) = (bytes, self.link.as_mut())
        {
            for (n, data) in split_chunks(&bytes).into_iter().enumerate() {
                let due = now + self.pace * u32::try_from(n).unwrap_or(u32::MAX);
                enqueue(
                    &mut link.queue,
                    Outgoing::now(due, Channel::BoardData, data),
                );
            }
            self.stats.pushed_pages += 1;
        }
    }

    /// The last chunk of a download of the open page `time` was sent: the tablet removes it.
    fn downloaded(&mut self, time: u32) {
        if self.open.is_some_and(|open| open.time == time) {
            self.open = None;
            self.pages.retain(|page| page.time != time);
            self.stats.removed_after_download += 1;
        }
    }

    fn materialize_live(&mut self, now: Duration) {
        loop {
            let Some(cursor) = self.link.as_ref().and_then(|l| l.stream) else {
                return;
            };
            let Some(item) = peek_live(&self.pen, cursor) else {
                return;
            };
            if item.due > now {
                return;
            }
            self.consume_live(&item);
            if let Some(link) = self.link.as_mut() {
                enqueue(
                    &mut link.queue,
                    Outgoing::now(item.due, Channel::BoardData, item.data),
                );
            }
        }
    }

    fn consume_live(&mut self, item: &LiveItem) {
        if let Some(cursor) = self.link.as_mut().and_then(|l| l.stream.as_mut()) {
            *cursor += item.consumed;
        }
        self.stats.live_records += item.consumed as u64;
    }

    /// Removes the page of `time` (a delete); counted when there was one. A page that is not
    /// there: nothing (measured).
    fn remove_page(&mut self, time: u32) {
        let before = self.pages.len();
        self.pages.retain(|page| page.time != time);
        if self.pages.len() < before {
            self.stats.deletes += 1;
            if self.open.is_some_and(|open| open.time == time) {
                self.open = None;
            }
        }
    }

    fn handle(&mut self, bytes: &[u8], now: Duration, wall: i64) -> Vec<(Channel, Vec<u8>)> {
        if let Some((time, due)) = self.pending_delete
            && due <= now
        {
            self.pending_delete = None;
            self.remove_page(time);
        }
        let Ok(request) = BoardRequest::decode(bytes) else {
            self.stats.unknown_requests += 1;
            return Vec::new();
        };
        let reply = |reply: BoardReply| vec![(Channel::Command, encode(&reply))];
        match request {
            BoardRequest::Battery => reply(BoardReply::Battery {
                percent: self.config.battery_percent,
            }),
            BoardRequest::Memory => {
                let used: usize = self.pages.iter().map(|page| page.bytes.len()).sum();
                let used = u32::try_from(used).unwrap_or(u32::MAX);
                reply(BoardReply::Memory {
                    total: self.config.memory_total,
                    free: self.config.memory_total.saturating_sub(used),
                })
            }
            BoardRequest::SetTime { unix } => {
                self.clock_offset = i64::from(unix) - wall;
                Vec::new()
            }
            BoardRequest::PageCount => {
                if let Some(link) = self.link.as_mut() {
                    link.counted = true;
                }
                let bytes: usize = self.pages.iter().map(|page| page.bytes.len()).sum();
                reply(BoardReply::PageCount {
                    count: u16::try_from(self.pages.len()).unwrap_or(u16::MAX),
                    bytes: u32::try_from(bytes).unwrap_or(u32::MAX),
                })
            }
            BoardRequest::OldestPage => {
                // Asked before any `12 01 04` on the link: the bare frame (measured once).
                if !self.link.as_ref().is_some_and(|link| link.counted) {
                    self.stats.uncounted_descriptions += 1;
                    return reply(BoardReply::OldestPage(None));
                }
                let run = self.streaming_run(now);
                let open = self.open;
                let oldest = self.pages.front().map(|page| PageInfo {
                    time: page.time,
                    size: u32::try_from(page.bytes.len()).unwrap_or(u32::MAX),
                    extra: open
                        .filter(|open| open.time == page.time)
                        .map(|open| open.previous),
                });
                if oldest.is_some_and(|info| info.extra.is_some()) {
                    self.stats.long_descriptions += 1;
                    if let Some(open) = self.open.as_mut() {
                        open.described = Some(run);
                    }
                }
                reply(BoardReply::OldestPage(oldest))
            }
            BoardRequest::Download { time } => {
                let Some(page) = self.pages.iter().find(|page| page.time == time) else {
                    return Vec::new();
                };
                self.stats.downloads += 1;
                let mut bytes = page.bytes.clone();
                let open = self.open.is_some_and(|open| open.time == time);
                // The open page leaves once all its bytes are sent (measured); a download cut
                // short leaves it (supposed).
                let mut whole = true;
                if let Some(stall) = self.stalls.pop_front() {
                    let tail = bytes.split_off(stall.bytes.min(bytes.len()));
                    whole = tail.is_empty();
                    if stall.late
                        && let Some(link) = self.link.as_mut()
                    {
                        link.late_tail = split_chunks(&tail);
                        link.late_removes = (open && !tail.is_empty()).then_some(time);
                    }
                }
                if open && whole {
                    if bytes.is_empty() {
                        self.downloaded(time);
                    } else {
                        self.removes_after = Some(time);
                    }
                }
                split_chunks(&bytes)
                    .into_iter()
                    .map(|chunk| (Channel::BoardData, chunk))
                    .collect()
            }
            BoardRequest::Delete { time } => {
                if std::mem::take(&mut self.ignore_next_delete) {
                    return Vec::new();
                }
                if let Some(delay) = self.delete_delay.take() {
                    self.pending_delete = Some((time, now + delay));
                    return Vec::new();
                }
                self.remove_page(time);
                Vec::new()
            }
            BoardRequest::StartLive => {
                let cursor = self.pen.partition_point(|timed| timed.due < now);
                self.live_runs += 1;
                let run = self.live_runs;
                if let Some(link) = self.link.as_mut() {
                    link.stream = Some(cursor);
                    link.run = run;
                }
                Vec::new()
            }
            BoardRequest::StopLive => {
                if let Some(link) = self.link.as_mut() {
                    link.stream = None;
                }
                Vec::new()
            }
        }
    }
}

/// Queues `item` after everything due no later than it: notifications leave in the order they
/// are due (a reply overtakes the chunks of a paced download still to come).
fn enqueue(queue: &mut VecDeque<Outgoing>, item: Outgoing) {
    let at = queue
        .iter()
        .position(|queued| queued.due > item.due)
        .unwrap_or(queue.len());
    queue.insert(at, item);
}

/// Up to [`MAX_LIVE_RECORDS`] records from `cursor`, the group ending at a `c0`; due with its
/// last record.
fn peek_live(pen: &[Timed], cursor: usize) -> Option<LiveItem> {
    let rest = pen.get(cursor..)?;
    rest.first()?;
    let mut records = Vec::with_capacity(MAX_LIVE_RECORDS);
    let mut due = Duration::ZERO;
    for timed in rest.iter().take(MAX_LIVE_RECORDS) {
        records.push(timed.record);
        due = timed.due;
        if timed.record.status == RecordStatus::Out {
            break;
        }
    }
    Some(LiveItem {
        due,
        data: encode_live(&records).into_iter().next()?,
        consumed: records.len(),
    })
}

/// The records of `script` from `start`, one every [`BOARD_RECORD_INTERVAL`] while the pen is
/// near (hovering with pressure 0, touching otherwise), a `c0` after each approach; and when
/// the movement ends.
fn timeline(script: &PenScript, start: Duration) -> (Vec<Timed>, Duration) {
    let mut records = Vec::with_capacity(script.sample_count() + script.approach_count());
    let mut at = start;
    for segment in script.segments() {
        match segment {
            PenSegment::Near(points) if !points.is_empty() => {
                for point in points {
                    let status = if point.pressure > 0 {
                        RecordStatus::Touch
                    } else {
                        RecordStatus::Hover
                    };
                    records.push(Timed {
                        due: at,
                        record: Record {
                            status,
                            x: point.x,
                            y: point.y,
                            pressure: point.pressure,
                        },
                    });
                    at += BOARD_RECORD_INTERVAL;
                }
                if let Some(last) = points.last() {
                    records.push(Timed {
                        due: at,
                        record: Record {
                            status: RecordStatus::Out,
                            x: last.x,
                            y: last.y,
                            pressure: 0,
                        },
                    });
                    at += BOARD_RECORD_INTERVAL;
                }
            }
            PenSegment::Near(_) => {}
            PenSegment::Away(duration) => at += *duration,
        }
    }
    (records, at)
}

/// The records a hand writing `strokes` leaves: hovering to each stroke's first point, its
/// points touching, leaving after its last.
fn records_of(strokes: &[Stroke]) -> Vec<Record> {
    let mut records = Vec::new();
    for stroke in strokes {
        let (Some(first), Some(last)) = (stroke.points.first(), stroke.points.last()) else {
            continue;
        };
        records.push(Record {
            status: RecordStatus::Hover,
            x: first.x,
            y: first.y,
            pressure: 0,
        });
        records.extend(stroke.points.iter().map(|point| Record {
            status: RecordStatus::Touch,
            x: point.x,
            y: point.y,
            pressure: point.pressure.max(1),
        }));
        records.push(Record {
            status: RecordStatus::Out,
            x: last.x,
            y: last.y,
            pressure: 0,
        });
    }
    records
}

/// `00:00:5E:00:53:B1` as bytes (zeros for what does not parse).
fn address_bytes(address: &str) -> [u8; 6] {
    let mut bytes = [0u8; 6];
    for (byte, part) in bytes.iter_mut().zip(address.split(':')) {
        *byte = u8::from_str_radix(part, 16).unwrap_or(0);
    }
    bytes
}

/// Encodes a reply; the replies built here always fit a frame.
fn encode(reply: &BoardReply) -> Vec<u8> {
    reply.encode().unwrap_or_default()
}

/// The pages of `ardoise.exe --simulated-blackboard`: the three reference drawings, sized for
/// the blackboard's page.
pub fn reference_board_pages() -> Vec<Vec<Stroke>> {
    crate::drawings::reference_pages(BOARD_WIDTH, BOARD_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_proto::blackboard::decode_page;
    use ardoise_proto::stroke::Point;

    fn write(board: &SimBoard, link: u64, request: BoardRequest) {
        board.link_write(link, &request.encode()).unwrap();
    }

    fn next(board: &SimBoard, link: u64) -> Option<(Channel, Vec<u8>)> {
        match board.link_recv(link, Duration::from_millis(100)).unwrap() {
            Some(TransportEvent::Notification { channel, data }) => Some((channel, data)),
            _ => None,
        }
    }

    #[test]
    fn it_answers_the_observed_requests() {
        let board = SimBoard::blackboard();
        let ad = board.advertisement().unwrap();
        assert!(ad.is_blackboard());
        let (link, _) = board
            .open_link("00:00:5e:00:53:b1", Duration::ZERO)
            .unwrap();
        assert!(
            board.advertisement().is_none(),
            "not advertised while connected"
        );
        write(&board, link, BoardRequest::Battery);
        assert_eq!(next(&board, link).unwrap().1, [0x11, 0x02, 0x01, 90]);
        write(&board, link, BoardRequest::PageCount);
        assert_eq!(
            next(&board, link).unwrap().1,
            [0x12, 0x07, 0x04, 0, 0, 0, 0, 0, 0]
        );
        write(&board, link, BoardRequest::OldestPage);
        assert_eq!(next(&board, link).unwrap().1, [0x12, 0x01, 0x05]);
        // Unknown requests get no answer.
        board.link_write(link, &[0x12, 0x01, 0x0b]).unwrap();
        assert_eq!(next(&board, link), None);
        assert_eq!(board.stats().unknown_requests, 1);
        // The clock follows SET_TIME.
        let now = board.tablet_time();
        let unix = u32::try_from(now + 3600).unwrap();
        write(&board, link, BoardRequest::SetTime { unix });
        assert_eq!(board.tablet_time(), now + 3600);
    }

    #[test]
    fn pages_are_stored_downloaded_and_deleted_by_time() {
        let board = SimBoard::blackboard();
        let stroke = |x: u16| Stroke {
            points: (0..50u16)
                .map(|i| Point {
                    x: x + i,
                    y: 100,
                    pressure: 900,
                })
                .collect(),
            ..Stroke::default()
        };
        board.draw_page(&[stroke(10)]);
        board.draw_page(&[stroke(20), stroke(30)]);
        let pages = board.stored_pages();
        assert_eq!(pages.len(), 2);
        assert!(pages[1].time > pages[0].time, "one second apart at least");
        assert_eq!(decode_page(&pages[1].bytes).strokes.len(), 2);
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        write(
            &board,
            link,
            BoardRequest::Download {
                time: pages[1].time,
            },
        );
        let mut bytes = Vec::new();
        while let Some((channel, chunk)) = next(&board, link) {
            assert_eq!(channel, Channel::BoardData);
            assert!(chunk.len() <= 244);
            bytes.extend(chunk);
        }
        assert_eq!(bytes, pages[1].bytes);
        write(
            &board,
            link,
            BoardRequest::Delete {
                time: pages[0].time,
            },
        );
        assert_eq!(board.stored_page_count(), 1);
        board.ignore_next_delete();
        write(
            &board,
            link,
            BoardRequest::Delete {
                time: pages[1].time,
            },
        );
        assert_eq!(board.stored_page_count(), 1);
    }

    #[test]
    fn faults_of_downloads_and_deletes() {
        let board = SimBoard::blackboard();
        let stroke = Stroke {
            points: (0..80u16)
                .map(|i| Point {
                    x: i,
                    y: 100,
                    pressure: 900,
                })
                .collect(),
            ..Stroke::default()
        };
        board.draw_page(std::slice::from_ref(&stroke));
        board.store_raw_page(vec![1, 2, 3]);
        let pages = board.stored_pages();
        assert_eq!(pages[1].bytes, [1, 2, 3]);
        assert!(pages[1].time > pages[0].time);
        let page = pages[0].clone();
        let download = BoardRequest::Download { time: page.time };
        let all = |board: &SimBoard, link| {
            let mut bytes = Vec::new();
            while let Some((_, chunk)) = next(board, link) {
                bytes.extend(chunk);
            }
            bytes
        };

        // The rest of a delayed download comes before the answer to the next request.
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        board.delay_next_download(100);
        let started = Arc::new(Mutex::new(false));
        let flag = started.clone();
        board.on_next_download(move || *flag.lock().unwrap() = true);
        write(&board, link, download);
        assert!(
            *started.lock().unwrap(),
            "the action ran when the download started"
        );
        assert_eq!(all(&board, link), page.bytes[..100]);
        write(&board, link, BoardRequest::Battery);
        assert_eq!(all(&board, link), [0x11, 0x02, 0x01, 90]);
        write(&board, link, download);
        let mut late = all(&board, link);
        let again = late.split_off(late.len() - page.bytes.len());
        assert_eq!(late, page.bytes[100..]);
        assert_eq!(again, page.bytes);
        // Lost with its link.
        board.delay_next_download(0);
        write(&board, link, download);
        board.close_link(link);
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        write(&board, link, download);
        assert_eq!(all(&board, link), page.bytes);

        // A slow delete: still described a moment, then gone (the link counts first).
        write(&board, link, BoardRequest::PageCount);
        assert_eq!(all(&board, link)[..3], [0x12, 0x07, 0x04]);
        board.delay_next_delete(Duration::from_millis(200));
        write(&board, link, BoardRequest::Delete { time: page.time });
        write(&board, link, BoardRequest::OldestPage);
        assert_eq!(all(&board, link)[3..7], page.time.to_be_bytes());
        board.clock().advance(Duration::from_millis(200));
        write(&board, link, BoardRequest::OldestPage);
        assert_eq!(all(&board, link)[3..7], pages[1].time.to_be_bytes());
        assert_eq!(board.stats().deletes, 1);

        // The link drops at the next request.
        board.disconnect_at_next_request();
        assert_eq!(
            board.link_write(link, &BoardRequest::PageCount.encode()),
            Err(TransportError::NotConnected)
        );
        assert!(!board.is_connected());
    }

    #[test]
    fn paced_downloads_and_unasked_streams() {
        let board = SimBoard::blackboard();
        let stroke = Stroke {
            points: (0..200u16)
                .map(|i| Point {
                    x: i,
                    y: 100,
                    pressure: 900,
                })
                .collect(),
            ..Stroke::default()
        };
        board.draw_page(std::slice::from_ref(&stroke));
        let page = board.stored_pages()[0].clone();
        let chunks = split_chunks(&page.bytes).len();
        assert!(chunks > 3);
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();

        // Chunks 10 ms apart; a reply asked meanwhile overtakes the chunks still to come.
        board.pace_downloads(Duration::from_millis(10));
        let start = board.clock().monotonic();
        write(&board, link, BoardRequest::Download { time: page.time });
        assert_eq!(next(&board, link).unwrap().0, Channel::BoardData);
        write(&board, link, BoardRequest::Battery);
        assert_eq!(
            next(&board, link).unwrap(),
            (Channel::Command, vec![0x11, 0x02, 0x01, 90])
        );
        let mut bytes = split_chunks(&page.bytes)[0].clone();
        while let Some((channel, chunk)) = next(&board, link) {
            assert_eq!(channel, Channel::BoardData);
            bytes.extend(chunk);
        }
        assert_eq!(bytes, page.bytes);
        let took = board.clock().monotonic() - start;
        assert!(
            took >= Duration::from_millis(10) * u32::try_from(chunks - 1).unwrap(),
            "{took:?}"
        );
        // Lost with the link.
        write(&board, link, BoardRequest::Download { time: page.time });
        board.close_link(link);
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        assert_eq!(next(&board, link), None);

        // Bytes sent nobody asking, and an action right after a delete.
        board.send_stream(&page.bytes[..14]);
        assert_eq!(
            next(&board, link).unwrap(),
            (Channel::BoardData, page.bytes[..14].to_vec())
        );
        let after = board.clone();
        board.on_next_delete(move || after.send_stream(&[0xa1; 7]));
        write(&board, link, BoardRequest::Delete { time: page.time });
        assert_eq!(board.stored_page_count(), 0);
        assert_eq!(
            next(&board, link).unwrap(),
            (Channel::BoardData, vec![0xa1; 7])
        );

        // A pen streamed to a new link without being asked.
        board.close_link(link);
        board.stream_without_asking(true);
        board.draw_live(PenScript::new().near(vec![
            Point {
                x: 5,
                y: 6,
                pressure: 0
            };
            12
        ]));
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        assert!(board.is_streaming());
        assert_eq!(next(&board, link).unwrap().0, Channel::BoardData);
        assert_eq!(board.stats().unknown_requests, 0);
    }

    #[test]
    fn storing_again_without_erasing_replaces_the_page() {
        let board = SimBoard::blackboard();
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        board.press_store();
        assert_eq!(
            board.stored_page_count(),
            0,
            "an empty screen stores nothing"
        );
        board.draw_live(PenScript::new().near(vec![
            Point {
                x: 1,
                y: 2,
                pressure: 5
            };
            10
        ]));
        board.clock().advance(Duration::from_secs(1));
        board.press_store();
        assert_eq!(next(&board, link).unwrap().1, [0x12, 0x01, 0x09]);
        let first = board.stored_pages()[0].clone();
        assert_eq!(first.bytes.len(), 11 * 7);
        board.draw_live(PenScript::new().near(vec![
            Point {
                x: 3,
                y: 4,
                pressure: 5
            };
            4
        ]));
        board.clock().advance(Duration::from_secs(1));
        board.press_store();
        let pages = board.stored_pages();
        assert_eq!(pages.len(), 1, "replaced");
        assert_eq!(pages[0].bytes.len(), 16 * 7);
        assert!(pages[0].time > first.time);
        board.erase();
        assert!(board.screen().is_empty());
    }

    /// What came on the link until it went quiet: the frames of the reply characteristic and
    /// the stream's bytes, apart.
    fn received(board: &SimBoard, link: u64) -> (Vec<Vec<u8>>, Vec<u8>) {
        let (mut frames, mut stream) = (Vec::new(), Vec::new());
        while let Some((channel, data)) = next(board, link) {
            match channel {
                Channel::BoardData => stream.extend(data),
                _ => frames.push(data),
            }
        }
        (frames, stream)
    }

    fn describe(board: &SimBoard, link: u64) -> Option<PageInfo> {
        write(board, link, BoardRequest::OldestPage);
        let (frames, _) = received(board, link);
        match BoardReply::decode(frames.last().unwrap()).unwrap() {
            BoardReply::OldestPage(info) => info,
            other => panic!("{other:?}"),
        }
    }

    fn count(board: &SimBoard, link: u64) -> u16 {
        write(board, link, BoardRequest::PageCount);
        let (frames, _) = received(board, link);
        match BoardReply::decode(frames.last().unwrap()).unwrap() {
            BoardReply::PageCount { count, .. } => count,
            other => panic!("{other:?}"),
        }
    }

    fn pen(x: u16, n: usize) -> PenScript {
        PenScript::new().near(vec![
            Point {
                x,
                y: 2,
                pressure: 300
            };
            n
        ])
    }

    /// Research note §6: the open page has the long description, `previous` names the store it
    /// replaces, and the tablet removes it once it has been downloaded; a page in the short form
    /// stays; deleting a page that is not there does nothing.
    #[test]
    fn an_open_page_is_described_in_full_and_leaves_once_downloaded() {
        let board = SimBoard::blackboard();
        board.draw_live(pen(1, 10));
        board.clock().advance(Duration::from_secs(1));
        board.press_store();
        let first = board.stored_pages()[0].clone();
        assert_eq!(board.open_page(), Some(first.time));
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        // Asked before the count: the bare frame (measured once); after it, the page.
        assert_eq!(describe(&board, link), None);
        assert_eq!(board.stats().uncounted_descriptions, 1);
        assert_eq!(count(&board, link), 1);
        let info = describe(&board, link).unwrap();
        assert_eq!(
            (info.time, info.size, info.extra),
            (first.time, 77, Some(0)),
            "the long form, a first store"
        );
        write(&board, link, BoardRequest::Download { time: first.time });
        let (_, bytes) = received(&board, link);
        assert_eq!(bytes, first.bytes);
        assert_eq!(count(&board, link), 0, "removed by the tablet itself");
        assert_eq!(board.stats().removed_after_download, 1);
        assert_eq!(board.open_page(), None);
        write(&board, link, BoardRequest::Delete { time: first.time });
        assert_eq!(board.stats().deletes, 0, "nothing to delete");

        // The screen, not erased, written on and stored again: a page holding the whole
        // screen, replacing the store the host took.
        board.draw_live(pen(5, 4));
        board.clock().advance(Duration::from_secs(1));
        board.press_store();
        let second = board.stored_pages()[0].clone();
        assert!(second.bytes.starts_with(&first.bytes));
        assert_eq!(second.bytes.len(), 77 + 35);
        assert_eq!(describe(&board, link).unwrap().extra, Some(first.time));
        // Erased: closed, the short form, and it stays when downloaded.
        board.erase();
        assert_eq!(board.open_page(), None);
        let info = describe(&board, link).unwrap();
        assert_eq!((info.time, info.extra), (second.time, None));
        assert_eq!(
            board.stats().pushed_pages,
            1,
            "described, then erased: pushed"
        );
        let (_, pushed) = received(&board, link);
        assert!(
            pushed.is_empty(),
            "already taken in by the description above"
        );
        write(&board, link, BoardRequest::Download { time: second.time });
        let (_, bytes) = received(&board, link);
        assert_eq!(bytes, second.bytes);
        assert_eq!(count(&board, link), 1, "a short-form page stays");
        // A page stored with nothing connected and not erased is open at the next connection.
        board.close_link(link);
        board.draw_live(pen(9, 3));
        board.clock().advance(Duration::from_secs(1));
        board.press_store();
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        assert_eq!(count(&board, link), 2);
        write(&board, link, BoardRequest::Delete { time: second.time });
        let info = describe(&board, link).unwrap();
        assert_eq!(info.extra, Some(0), "a first store of the new screen");
    }

    /// The six observed cases of the erase button (research note §6), and the one supposed.
    #[test]
    fn the_erase_button_pushes_or_loses_the_open_page_as_observed() {
        let stored = |board: &SimBoard| {
            board.draw_live(pen(3, 8));
            board.clock().advance(Duration::from_secs(1));
            board.press_store();
            board.stored_pages().last().unwrap().clone()
        };
        let fresh = || {
            let board = SimBoard::blackboard();
            let (link, _) = board
                .open_link(&board.config().address, Duration::ZERO)
                .unwrap();
            count(&board, link);
            (board, link)
        };

        // Outside live mode, described since it was stored: pushed whole, kept closed.
        let (board, link) = fresh();
        let page = stored(&board);
        received(&board, link);
        describe(&board, link);
        board.erase();
        let (frames, pushed) = received(&board, link);
        assert!(frames.is_empty(), "no frame outside live mode");
        assert_eq!(pushed, page.bytes);
        assert_eq!(describe(&board, link).unwrap().extra, None);
        // Outside live mode, not described: closed, kept, nothing pushed.
        let (board, link) = fresh();
        stored(&board);
        board.erase();
        assert_eq!(
            received(&board, link),
            (vec![vec![0x12, 0x01, 0x09]], vec![])
        );
        assert_eq!((count(&board, link), board.stats().pushed_pages), (1, 0));

        // In live mode, described during this live run: `11 01 0c`, then the page pushed in
        // chunks like a download's.
        let (board, link) = fresh();
        write(&board, link, BoardRequest::StartLive);
        let page = stored(&board);
        received(&board, link);
        describe(&board, link);
        board.erase();
        let mut chunks = Vec::new();
        while let Some(item) = next(&board, link) {
            chunks.push(item);
        }
        assert_eq!(chunks[0], (Channel::Command, vec![0x11, 0x01, 0x0c]));
        let pushed: Vec<u8> = chunks[1..].iter().flat_map(|(_, d)| d.clone()).collect();
        assert_eq!(pushed, page.bytes);
        assert_eq!(count(&board, link), 1, "kept");
        // In live mode, stored during this run, not described: `11 01 0c`, kept, not pushed.
        let (board, link) = fresh();
        write(&board, link, BoardRequest::StartLive);
        stored(&board);
        board.erase();
        let (frames, _) = received(&board, link);
        assert!(frames.contains(&vec![0x11, 0x01, 0x0c]));
        assert_eq!((count(&board, link), board.stats().pushed_pages), (1, 0));
        // In live mode, stored and described before this run: lost, nothing pushed (twice).
        let (board, link) = fresh();
        stored(&board);
        describe(&board, link);
        write(&board, link, BoardRequest::StartLive);
        received(&board, link);
        assert_eq!(count(&board, link), 1, "starting live mode removes nothing");
        board.erase();
        assert_eq!(count(&board, link), 0);
        assert_eq!(
            (board.stats().lost_pages, board.stats().pushed_pages),
            (1, 0)
        );
        // Supposed: stored before this run and not described: lost all the same.
        let (board, link) = fresh();
        stored(&board);
        write(&board, link, BoardRequest::StartLive);
        board.erase();
        assert_eq!(count(&board, link), 0);
        // A new link does not stream until asked (live mode is not kept across connections).
        board.close_link(link);
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        assert!(!board.is_streaming());
        board.close_link(link);
    }

    /// Supposed (not measured): a download cut short leaves the open page; one whose rest comes
    /// late removes it once the rest is sent.
    #[test]
    fn a_download_cut_short_leaves_the_open_page() {
        let board = SimBoard::blackboard();
        board.draw_live(pen(1, 80));
        board.clock().advance(Duration::from_secs(2));
        board.press_store();
        let page = board.stored_pages()[0].clone();
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        count(&board, link);
        board.stall_next_download(100);
        write(&board, link, BoardRequest::Download { time: page.time });
        assert_eq!(received(&board, link).1.len(), 100);
        assert_eq!(board.open_page(), Some(page.time), "stalled: stays");
        // Paced, and the link lost before the last chunk: stays.
        board.pace_downloads(Duration::from_millis(10));
        write(&board, link, BoardRequest::Download { time: page.time });
        next(&board, link);
        board.close_link(link);
        assert_eq!(
            board.open_page(),
            Some(page.time),
            "lost with the link: stays"
        );
        board.pace_downloads(Duration::ZERO);
        // Its rest late: removed once the rest is sent.
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        count(&board, link);
        board.delay_next_download(140);
        write(&board, link, BoardRequest::Download { time: page.time });
        received(&board, link);
        assert_eq!(board.open_page(), Some(page.time));
        write(&board, link, BoardRequest::Download { time: page.time });
        let (_, bytes) = received(&board, link);
        assert_eq!(bytes[..page.bytes.len() - 140], page.bytes[140..]);
        assert_eq!(board.open_page(), None);
        assert_eq!(board.stats().removed_after_download, 1);
    }

    #[test]
    fn live_records_stream_at_the_boards_rate() {
        let board = SimBoard::blackboard();
        let (link, _) = board
            .open_link(&board.config().address, Duration::ZERO)
            .unwrap();
        write(&board, link, BoardRequest::StartLive);
        let points: Vec<Point> = (0..20)
            .map(|i| Point {
                x: i,
                y: i,
                pressure: if i < 5 { 0 } else { 700 },
            })
            .collect();
        board.draw_live(PenScript::new().near(points));
        let start = board.clock().monotonic();
        let mut records = Vec::new();
        while let Some((channel, data)) = next(&board, link) {
            assert_eq!(channel, Channel::BoardData);
            assert!(data.len() <= 42);
            records.extend(ardoise_proto::blackboard::decode_records(&data).records);
        }
        assert_eq!(records.len(), 21);
        assert_eq!(records[0].status, RecordStatus::Hover);
        assert_eq!(records[5].status, RecordStatus::Touch);
        assert_eq!(records[20].status, RecordStatus::Out);
        let elapsed = board.clock().monotonic() - start;
        assert!(elapsed >= BOARD_RECORD_INTERVAL * 20, "{elapsed:?}");
        write(&board, link, BoardRequest::StopLive);
        assert!(!board.is_streaming());
        assert_eq!(board.screen().len(), 21, "written on the screen too");
    }
}
