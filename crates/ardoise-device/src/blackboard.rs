//! One conversation with the Xiaomi blackboard (docs/design/blackboard.md): the flows of
//! docs/research/xiaomi-blackboard.md §3 over a [`Transport`].
//!
//! There is no authentication and no registration: the tablet answers any client, and the
//! Xiaomi service (`fe95`) is never written to. A [`BoardSession`] only ever sends the
//! requests of [`BoardRequest`], the frames seen in Mi Home's captures.
//!
//! Like the Slate's [`Session`](crate::session::Session), it sorts what arrives as it comes:
//! replies wait in a queue for the request that expects them, the bytes of the stream
//! characteristic go to the page being downloaded or, in live mode, to the caller; anything
//! else (stream bytes nobody asked for, unknown or malformed frames) is dropped and counted
//! ([`BoardSession::take_oddities`]), never an error. The diagnostic log has every frame
//! anyway (the [`LoggingTransport`](crate::logging::LoggingTransport)).
//!
//! # A page has nothing but its count
//!
//! A page arrives on the stream characteristic with no header, no end mark and no checksum
//! (research note §3): only the size announced by `12 01 05` says where it ends. So bytes of an
//! earlier transfer that is still running would join the next page unnoticed. Three layers keep
//! a wrong page out of the library (and so off the deletion that follows):
//!
//! 1. **Dirty stream.** A download that does not end cleanly (no byte for
//!    [`DATA_IDLE_TIMEOUT`], more bytes than announced, cancelled, a page that fails the record
//!    check, any error but a lost link), or stream bytes arriving after a download ended, mark
//!    the stream as dirty ([`BoardSession::is_dirty`]): this link then refuses downloads and live
//!    mode ([`DeviceError::StreamDirty`]), and its owner must close it and open a fresh one.
//!    Only a fresh link is known to carry nothing of the old transfer (**supposed**: that the
//!    tablet ends a transfer with its link is to check at the hardware checkpoint).
//! 2. **Quiet window.** A download is asked only once the stream has been quiet for
//!    [`QUIET_WINDOW`] (at once on a stream already quiet, the usual case; a stream that does
//!    not go quiet within [`DATA_IDLE_TIMEOUT`] makes the session dirty: a tablet still
//!    streaming would mix its bytes into the page). Once the count reaches the announced size,
//!    the stream must stay quiet for [`QUIET_WINDOW`] again before the page is returned; any
//!    byte more makes it incomplete.
//! 3. **Record check.** The page must be whole 7-byte records of a known status
//!    ([`ardoise_proto::blackboard::check_page`]), else [`DeviceError::PageUnreadable`].
//!
//! Every unclean transfer of a page uses up its one retry ([`BoardSession::sync_page`]): a
//! tablet that always misbehaves ends the sync with [`DeviceError::TransferIncomplete`], never in
//! a loop of fresh links.
//!
//! # Sync
//!
//! The tablet only describes its oldest page, so reading the next one needs the deletion of the
//! first. [`BoardSession::sync_page`] is the one step of a sync (describe, download, keep, and
//! only once kept delete), used by the tablet service between its other work; the service
//! closes and reopens the link when the step asks ([`PageStep::Reopen`]).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use ardoise_proto::blackboard::{BoardReply, BoardRequest, PageInfo, check_page};

use crate::session::{DeviceError, DownloadedPage, LiveNotification, unix_seconds};
use crate::transport::{
    Advertisement, Channel, Clock, TabletKind, Transport, TransportError, TransportEvent,
};

/// Wait for the reply to a command. The tablet answered in under 50 ms in every capture.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// A download is abandoned when no byte of the page arrived for this long. A page of 52 185
/// bytes took 2.1 s in Mi Home's capture (research note §3).
pub const DATA_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
/// After the last byte of a page, how long the stream must stay quiet before the page is
/// accepted: one byte more means the count was reached with bytes of another transfer.
/// **Supposed**: the page notifications came about every 10 ms in Mi Home's capture (52 185
/// bytes, 214 notifications of at most 244 bytes, in 2.1 s: research note §3).
pub const QUIET_WINDOW: Duration = Duration::from_millis(300);
/// When the tablet still describes the page it was just asked to delete, it is described once
/// more after this long before the sync gives up ([`DeviceError::PageNotDeleted`]): the delete
/// has no reply, and how long the tablet takes to erase is not measured (**supposed**).
pub const DELETE_SETTLE: Duration = Duration::from_millis(300);
/// Largest page accepted while the tablet's memory is not known yet (`11 01 02` not asked):
/// its measured memory (126 909 952 bytes, research note §3), rounded up (**supposed** bound).
pub const MAX_PAGE_BYTES: u32 = 128 * 1024 * 1024;
/// Most pages one sync reads, whatever the tablet says: about 400 fit in its memory (research
/// note §1). Guards against a tablet that never runs out of pages.
pub const MAX_SYNC_PAGES: u32 = 1000;
/// Granularity of the waits, so that cancellation is noticed quickly.
const WAIT_SLICE: Duration = Duration::from_millis(250);
/// Live notifications kept while nobody reads them (oldest dropped first).
const MAX_LIVE_QUEUE: usize = 4096;

/// What the tablet says about itself (`11 01 01`, `11 01 02`, `12 01 04`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardInfo {
    /// Per cent (inferred from the values 100 and 90 seen).
    pub battery_percent: u8,
    /// Memory in bytes.
    pub memory_total: u32,
    pub memory_free: u32,
    pub stored_pages: u16,
    /// Bytes of the stored pages.
    pub stored_bytes: u32,
}

/// What a session dropped since it was last asked ([`BoardSession::take_oddities`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Oddities {
    /// Frames of the reply characteristic that are unknown or malformed, as hex.
    pub frames: Vec<String>,
    /// Bytes of the stream characteristic received outside a download and outside live mode.
    pub stray_bytes: usize,
    /// Notifications of a channel the blackboard does not have.
    pub other_channels: usize,
}

impl Oddities {
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty() && self.stray_bytes == 0 && self.other_channels == 0
    }
}

/// Where a sync of the blackboard stands between two of its steps
/// ([`BoardSession::sync_page`]); it outlives a link reopened in the middle of the sync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoardSync {
    /// Pages kept and deleted so far.
    pages: u32,
    /// Time of the page deleted last: the tablet must not describe it again.
    last_deleted: Option<u32>,
    /// Time of the page whose transfer already failed once: its one retry is used.
    retried: Option<u32>,
}

impl BoardSync {
    /// Pages kept and deleted so far.
    pub fn pages(&self) -> u32 {
        self.pages
    }
}

/// What one step of a sync did ([`BoardSession::sync_page`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStep<T> {
    /// The page `info` describes was kept (`kept`: what the keeper returned), then deleted from
    /// the tablet.
    Kept { info: PageInfo, kept: T },
    /// Nothing more to read: the tablet holds no page, or [`MAX_SYNC_PAGES`] were read.
    Done,
    /// The stream is dirty (a transfer of this page did not end cleanly, the first time, or
    /// bytes came after the page deleted last): close this link, open a fresh one and step again
    /// there.
    Reopen,
}

/// A page being downloaded.
struct Download {
    expected: usize,
    bytes: Vec<u8>,
    /// When the last byte came (session clock), or when the request was sent.
    last: Duration,
}

/// An open link to the blackboard.
pub struct BoardSession {
    transport: Box<dyn Transport>,
    clock: Arc<dyn Clock>,
    cancel: Option<Arc<AtomicBool>>,
    commands: VecDeque<Vec<u8>>,
    download: Option<Download>,
    live: VecDeque<LiveNotification>,
    live_mode: bool,
    /// The tablet said a page was stored with its button (`12 01 09`) and nobody asked yet.
    page_stored: bool,
    oddities: Oddities,
    connected: bool,
    /// The tablet's memory as it said (`11 01 02`): no page can be larger.
    memory_total: Option<u32>,
    /// A transfer did not end cleanly: the stream may still carry its bytes.
    dirty: bool,
    /// A download ended and nothing took the stream since: stream bytes now are more of that
    /// page than announced.
    after_download: bool,
    /// Stream bytes that came after the last download ended (more than it announced).
    extra: usize,
    /// When the last stream byte came, whatever it was, or when the link opened while none came
    /// yet (session clock): a tablet streaming from the start of a link may not have sent its
    /// first bytes when the first download is asked.
    last_stream: Duration,
    reply_timeout: Duration,
    data_timeout: Duration,
}

impl BoardSession {
    /// Starts a conversation on an open link. Nothing is sent: the tablet needs no login.
    pub fn open(transport: Box<dyn Transport>, clock: Arc<dyn Clock>) -> Self {
        let opened = clock.monotonic();
        Self {
            transport,
            clock,
            cancel: None,
            commands: VecDeque::new(),
            download: None,
            live: VecDeque::new(),
            live_mode: false,
            page_stored: false,
            oddities: Oddities::default(),
            connected: true,
            memory_total: None,
            dirty: false,
            after_download: false,
            extra: 0,
            last_stream: opened,
            reply_timeout: REPLY_TIMEOUT,
            data_timeout: DATA_IDLE_TIMEOUT,
        }
    }

    /// Lets the caller interrupt long waits (download, live mode).
    pub fn set_cancel_flag(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = Some(cancel);
    }

    /// Shorter waits (tests of a stalled transfer).
    pub fn with_timeouts(mut self, reply: Duration, data: Duration) -> Self {
        self.reply_timeout = reply;
        self.data_timeout = data;
        self
    }

    pub fn peer(&self) -> &Advertisement {
        self.transport.peer()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// The stream may still carry bytes of a transfer that did not end cleanly: this link
    /// refuses downloads and live mode; close it and open a fresh one.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Sets the tablet's clock to the computer's (Unix seconds, UTC): `11 05 07`, no reply.
    pub fn set_time(&mut self) -> Result<(), DeviceError> {
        let now = unix_seconds(&*self.clock);
        let unix = u32::try_from(now)
            .map_err(|_| DeviceError::Unexpected(format!("cannot send the time {now}")))?;
        self.send(BoardRequest::SetTime { unix })
    }

    /// Battery level in per cent (`11 01 01`).
    pub fn battery(&mut self) -> Result<u8, DeviceError> {
        match self.exchange(BoardRequest::Battery, "battery", |r| {
            matches!(r, BoardReply::Battery { .. })
        })? {
            BoardReply::Battery { percent } => Ok(percent),
            other => Err(unexpected(&other)),
        }
    }

    /// Total and free memory in bytes (`11 01 02`). The total bounds the size of a page.
    pub fn memory(&mut self) -> Result<(u32, u32), DeviceError> {
        match self.exchange(BoardRequest::Memory, "memory", |r| {
            matches!(r, BoardReply::Memory { .. })
        })? {
            BoardReply::Memory { total, free } => {
                self.memory_total = Some(total);
                Ok((total, free))
            }
            other => Err(unexpected(&other)),
        }
    }

    /// Pages stored and their total size in bytes (`12 01 04`).
    pub fn page_count(&mut self) -> Result<(u16, u32), DeviceError> {
        match self.exchange(BoardRequest::PageCount, "page count", |r| {
            matches!(r, BoardReply::PageCount { .. })
        })? {
            BoardReply::PageCount { count, bytes } => Ok((count, bytes)),
            other => Err(unexpected(&other)),
        }
    }

    /// Battery, memory and stored pages, in Mi Home's order.
    pub fn info(&mut self) -> Result<BoardInfo, DeviceError> {
        let battery_percent = self.battery()?;
        let (memory_total, memory_free) = self.memory()?;
        let (stored_pages, stored_bytes) = self.page_count()?;
        Ok(BoardInfo {
            battery_percent,
            memory_total,
            memory_free,
            stored_pages,
            stored_bytes,
        })
    }

    /// The oldest stored page (`12 01 05`), `None` when the tablet holds none: it only ever
    /// describes its oldest page.
    pub fn oldest_page(&mut self) -> Result<Option<PageInfo>, DeviceError> {
        match self.exchange(BoardRequest::OldestPage, "oldest page", |r| {
            matches!(r, BoardReply::OldestPage(_))
        })? {
            BoardReply::OldestPage(info) => Ok(info),
            other => Err(unexpected(&other)),
        }
    }

    /// Downloads the page `info` describes (`12 06 03 01 <time>`): its bytes come on the stream
    /// characteristic, with no header, no end mark and no checksum, so the request is written
    /// only once the stream has been quiet for [`QUIET_WINDOW`] (at once when it already is),
    /// the bytes are counted against the size announced, then the stream must stay quiet for
    /// [`QUIET_WINDOW`], then the page must be whole records of a known status. Nothing is
    /// deleted here.
    ///
    /// - No byte for [`DATA_IDLE_TIMEOUT`], more bytes than announced, or a stream that did not
    ///   go quiet within [`DATA_IDLE_TIMEOUT`] before the request (then nothing is sent, 0 bytes
    ///   received): [`DeviceError::TransferIncomplete`].
    /// - Not whole records of a known status, or announced larger than the tablet's memory
    ///   (then nothing is sent): [`DeviceError::PageUnreadable`].
    /// - On a dirty stream: [`DeviceError::StreamDirty`], nothing sent.
    ///
    /// Any failure but a lost link, the oversized announcement excepted, leaves the stream
    /// dirty (a cancellation while waiting for the quiet stream too: nothing was asked, but the
    /// stream was busy).
    pub fn download(&mut self, info: PageInfo) -> Result<DownloadedPage, DeviceError> {
        // Stream bytes still in flight (the end of a live stream) must not join the page, nor
        // an earlier reply pass for anything; late bytes of the last page make the stream dirty.
        self.drain()?;
        self.commands.clear();
        if self.dirty {
            return Err(DeviceError::StreamDirty);
        }
        let limit = self.memory_total.unwrap_or(MAX_PAGE_BYTES);
        if info.size > limit {
            return Err(DeviceError::PageUnreadable {
                time: info.time,
                reason: format!(
                    "{} bytes announced, more than the {limit} bytes of the tablet's memory",
                    info.size
                ),
            });
        }
        // A tablet still streaming (live mode kept across a reconnection, a transfer nobody
        // asked for) would mix its bytes into the page: ask only once the stream is quiet.
        match self.wait_for_quiet() {
            Ok(true) => {}
            Ok(false) => {
                self.dirty = true;
                return Err(DeviceError::TransferIncomplete {
                    expected: info.size,
                    received: 0,
                });
            }
            Err(DeviceError::Disconnected) => return Err(DeviceError::Disconnected),
            Err(error) => {
                self.dirty = true;
                return Err(error);
            }
        }
        // More of the page downloaded before came meanwhile.
        if self.dirty {
            return Err(DeviceError::StreamDirty);
        }
        self.after_download = false;
        self.extra = 0;
        let result = self.receive_page(info);
        match &result {
            Ok(_) => self.after_download = true,
            Err(DeviceError::Disconnected) => {}
            Err(_) => self.dirty = true,
        }
        result
    }

    fn receive_page(&mut self, info: PageInfo) -> Result<DownloadedPage, DeviceError> {
        // At most the tablet's memory (checked by the caller).
        let expected = usize::try_from(info.size).unwrap_or(usize::MAX);
        self.download = Some(Download {
            expected,
            bytes: Vec::with_capacity(expected.min(1 << 20)),
            last: self.clock.monotonic(),
        });
        let counted = self.count_page(BoardRequest::Download { time: info.time }, info.size);
        let download = self.download.take();
        counted?;
        let bytes = download.map(|d| d.bytes).unwrap_or_default();
        if bytes.len() != expected {
            return Err(DeviceError::TransferIncomplete {
                expected: info.size,
                received: bytes.len(),
            });
        }
        let shape = check_page(&bytes);
        if !shape.is_whole() {
            return Err(DeviceError::PageUnreadable {
                time: info.time,
                reason: shape.to_string(),
            });
        }
        Ok(DownloadedPage {
            bytes,
            timestamp: Some(i64::from(info.time)),
            announced_size: info.size,
            kind: TabletKind::Blackboard,
        })
    }

    /// Waits until no stream byte came for [`QUIET_WINDOW`] (since the link opened, when none
    /// came yet), for at most the data timeout ([`DATA_IDLE_TIMEOUT`]): `Ok(false)` when the
    /// stream never went quiet. No wait at all on a stream already quiet: a page ends with a
    /// quiet window; on a fresh link, at most what is left of the window after the information
    /// was read (once per link).
    fn wait_for_quiet(&mut self) -> Result<bool, DeviceError> {
        let start = self.clock.monotonic();
        loop {
            let now = self.clock.monotonic();
            let quiet = now.saturating_sub(self.last_stream);
            if quiet >= QUIET_WINDOW {
                return Ok(true);
            }
            let waited = now.saturating_sub(start);
            if waited >= self.data_timeout {
                return Ok(false);
            }
            self.check_cancel()?;
            let wait = (QUIET_WINDOW - quiet)
                .min(self.data_timeout - waited)
                .min(WAIT_SLICE);
            self.pump(wait)?;
        }
    }

    /// Writes `request`, then counts the bytes until the announced size and a quiet window.
    fn count_page(&mut self, request: BoardRequest, size: u32) -> Result<(), DeviceError> {
        let incomplete = |received| DeviceError::TransferIncomplete {
            expected: size,
            received,
        };
        self.write(request)?;
        loop {
            let (received, expected, last) = match &self.download {
                Some(d) => (d.bytes.len(), d.expected, d.last),
                None => return Ok(()),
            };
            if received > expected {
                return Err(incomplete(received));
            }
            self.check_cancel()?;
            let idle = self.clock.monotonic().saturating_sub(last);
            let wait = if received == expected {
                // Complete: accepted once the stream stayed quiet long enough.
                if idle >= QUIET_WINDOW {
                    return Ok(());
                }
                QUIET_WINDOW - idle
            } else {
                if idle >= self.data_timeout {
                    return Err(incomplete(received));
                }
                self.data_timeout - idle
            };
            self.pump(wait.min(WAIT_SLICE))?;
        }
    }

    /// Deletes the page stored at `time` (`12 05 06 <time>`): no reply. Only ever called once
    /// the page is safe in the library (see [`BoardSession::sync_page`]).
    pub fn delete(&mut self, time: u32) -> Result<(), DeviceError> {
        self.send(BoardRequest::Delete { time })
    }

    /// One step of a sync: describes the oldest page, downloads it, hands it to `keep`, and only
    /// once `keep` returned `Ok` (the note written and read back) deletes it from the tablet;
    /// the cancellation flag is ignored for that one write. The tablet only describes its oldest
    /// page, so reading the next one needs this deletion: a sync always deletes.
    ///
    /// - [`PageStep::Done`] when the tablet holds no page, or after [`MAX_SYNC_PAGES`].
    /// - [`PageStep::Reopen`] on a dirty stream, or when the page's transfer did not end cleanly
    ///   for the first time: stopped, too long, a stream that never went quiet before the
    ///   request, or more stream bytes while `keep` ran (the page, kept, is then not deleted).
    ///   The caller opens a fresh link and steps again with the same `sync`, so that the page's
    ///   one retry happens on the new link, never on this one.
    /// - [`DeviceError::TransferIncomplete`] when the retry is unclean too, whichever way; the
    ///   page stays (its note too, when it was kept: a second note may come if the page is read
    ///   differently next time).
    /// - [`DeviceError::PageNotDeleted`] when the tablet still describes the page just deleted
    ///   after [`DELETE_SETTLE`] (rather than import it again and again).
    /// - Any error of `keep` is returned as it is: the page stays on the tablet.
    pub fn sync_page<T>(
        &mut self,
        sync: &mut BoardSync,
        keep: impl FnOnce(DownloadedPage) -> Result<T, DeviceError>,
    ) -> Result<PageStep<T>, DeviceError> {
        if self.dirty {
            return Ok(PageStep::Reopen);
        }
        if sync.pages >= MAX_SYNC_PAGES {
            return Ok(PageStep::Done);
        }
        let Some(mut info) = self.oldest_page()? else {
            return Ok(PageStep::Done);
        };
        if sync.last_deleted == Some(info.time) {
            // The tablet may take a moment to erase: once more, a little later.
            self.idle(DELETE_SETTLE)?;
            match self.oldest_page()? {
                None => return Ok(PageStep::Done),
                Some(again) if again.time == info.time => {
                    return Err(DeviceError::PageNotDeleted(info.time));
                }
                Some(again) => info = again,
            }
        }
        let page = match self.download(info) {
            Ok(page) => page,
            Err(DeviceError::StreamDirty) => return Ok(PageStep::Reopen),
            // Notifications can be lost on the way (BLE stack buffers), or the tablet can stall:
            // once more, on a fresh link.
            Err(DeviceError::TransferIncomplete { .. }) if sync.retried != Some(info.time) => {
                sync.retried = Some(info.time);
                return Ok(PageStep::Reopen);
            }
            Err(error) => return Err(error),
        };
        let size = page.bytes.len();
        let kept = keep(page)?;
        // Bytes that came while the page was kept are more of a transfer that was thought
        // complete: the page may be wrong, so it stays on the tablet (its note too, in the
        // library) and is read again on a fresh link, with the page's one retry.
        self.drain()?;
        if self.dirty {
            if sync.retried == Some(info.time) {
                return Err(DeviceError::TransferIncomplete {
                    expected: info.size,
                    received: size.saturating_add(self.extra),
                });
            }
            sync.retried = Some(info.time);
            return Ok(PageStep::Reopen);
        }
        // Only now, the page safe in the library, may it leave the tablet; a cancellation
        // arriving meanwhile takes effect after it.
        let cancel = self.cancel.take();
        let deleted = self.delete(info.time);
        self.cancel = cancel;
        deleted?;
        sync.pages = sync.pages.saturating_add(1);
        sync.last_deleted = Some(info.time);
        sync.retried = None;
        Ok(PageStep::Kept { info, kept })
    }

    /// Starts the live stream (`12 02 08 01`, no reply): records follow on the stream
    /// characteristic, see [`BoardSession::next_live_event`]. Refused on a dirty stream
    /// ([`DeviceError::StreamDirty`]): the rest of a page would pass for pen records.
    pub fn start_live(&mut self) -> Result<(), DeviceError> {
        self.drain()?;
        if self.dirty {
            return Err(DeviceError::StreamDirty);
        }
        self.live.clear();
        self.live_mode = false;
        self.after_download = false;
        self.send(BoardRequest::StartLive)?;
        self.live_mode = true;
        Ok(())
    }

    /// Stops the live stream (`12 02 08 02`, no reply).
    pub fn stop_live(&mut self) -> Result<(), DeviceError> {
        let result = self.send(BoardRequest::StopLive);
        self.live_mode = false;
        self.live.clear();
        result
    }

    pub fn in_live_mode(&self) -> bool {
        self.live_mode
    }

    /// Next notification of live mode, waiting up to `timeout`: records
    /// ([`Channel::BoardData`]) and the frames of the reply characteristic
    /// ([`Channel::Command`]: the "page stored" event, unknown frames), in arrival order, each
    /// stamped when it came out of the transport. With a zero `timeout` it takes only what
    /// already arrived.
    pub fn next_live_event(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<LiveNotification>, DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            if let Some(notification) = self.live.pop_front() {
                return Ok(Some(notification));
            }
            self.check_cancel()?;
            let now = self.clock.monotonic();
            let received = self.pump(deadline.saturating_sub(now).min(WAIT_SLICE))?;
            if !received && now >= deadline {
                return Ok(None);
            }
        }
    }

    /// Keeps the link while nothing is asked, for up to `timeout`; a lost link is
    /// [`DeviceError::Disconnected`]. With a zero `timeout` it only takes in what arrived.
    pub fn idle(&mut self, timeout: Duration) -> Result<(), DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            self.drain()?;
            self.commands.clear();
            self.live.clear();
            let now = self.clock.monotonic();
            if now >= deadline {
                return Ok(());
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
        }
    }

    /// Whether the tablet said a page was stored with its button since the last call.
    pub fn take_page_stored(&mut self) -> bool {
        std::mem::take(&mut self.page_stored)
    }

    /// What was dropped since the last call (for the diagnostic log).
    pub fn take_oddities(&mut self) -> Oddities {
        std::mem::take(&mut self.oddities)
    }

    /// Closes the link.
    pub fn disconnect(mut self) -> Result<(), DeviceError> {
        self.connected = false;
        self.transport.disconnect().map_err(DeviceError::from)
    }

    // --- plumbing -------------------------------------------------------------------------

    /// Takes in everything already arrived.
    fn drain(&mut self) -> Result<(), DeviceError> {
        while self.pump(Duration::ZERO)? {}
        Ok(())
    }

    /// Writes `request` after taking in what already arrived (a lost link is noticed before
    /// writing) and forgetting earlier replies (a late answer must not pass for this one's).
    fn send(&mut self, request: BoardRequest) -> Result<(), DeviceError> {
        self.drain()?;
        self.commands.clear();
        self.write(request)
    }

    fn write(&mut self, request: BoardRequest) -> Result<(), DeviceError> {
        if !self.connected {
            return Err(DeviceError::Disconnected);
        }
        let written = self.transport.write(&request.encode());
        self.link_result(written)
    }

    fn link_result<T>(&mut self, result: Result<T, TransportError>) -> Result<T, DeviceError> {
        match result {
            Ok(value) => Ok(value),
            Err(TransportError::NotConnected) => {
                self.connected = false;
                Err(DeviceError::Disconnected)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn exchange(
        &mut self,
        request: BoardRequest,
        what: &'static str,
        accept: impl Fn(&BoardReply) -> bool,
    ) -> Result<BoardReply, DeviceError> {
        self.send(request)?;
        let deadline = self.clock.monotonic() + self.reply_timeout;
        loop {
            while let Some(bytes) = self.commands.pop_front() {
                if let Ok(reply) = BoardReply::decode(&bytes)
                    && accept(&reply)
                {
                    return Ok(reply);
                }
            }
            self.check_cancel()?;
            let now = self.clock.monotonic();
            if now >= deadline {
                return Err(DeviceError::Timeout(what));
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
        }
    }

    /// Receives at most one event and files it.
    fn pump(&mut self, timeout: Duration) -> Result<bool, DeviceError> {
        if !self.connected {
            return Err(DeviceError::Disconnected);
        }
        let received = self.transport.recv(timeout);
        match self.link_result(received)? {
            None => Ok(false),
            Some(TransportEvent::Disconnected) => {
                self.connected = false;
                Err(DeviceError::Disconnected)
            }
            Some(TransportEvent::Notification { channel, data }) => {
                // The latency stamp (system clock, see `LiveNotification`).
                let received = SystemTime::now();
                self.file(channel, data, received);
                Ok(true)
            }
        }
    }

    fn file(&mut self, channel: Channel, data: Vec<u8>, received: SystemTime) {
        match channel {
            Channel::Command => {
                match BoardReply::decode(&data) {
                    Ok(BoardReply::PageStored) => self.page_stored = true,
                    Ok(BoardReply::Unknown(_)) | Err(_) => {
                        self.oddities.frames.push(ardoise_proto::hex::encode(&data));
                    }
                    Ok(_) => {}
                }
                if self.live_mode {
                    self.push_live(channel, data.clone(), received);
                }
                self.commands.push_back(data);
            }
            Channel::BoardData => {
                let now = self.clock.monotonic();
                self.last_stream = now;
                if let Some(download) = self.download.as_mut() {
                    download.bytes.extend_from_slice(&data);
                    download.last = now;
                } else if self.live_mode {
                    self.push_live(channel, data, received);
                } else {
                    self.oddities.stray_bytes += data.len();
                    // More of the page just downloaded than it announced: its transfer did not
                    // end cleanly after all.
                    if self.after_download {
                        self.dirty = true;
                        self.extra = self.extra.saturating_add(data.len());
                    }
                }
            }
            Channel::PageData | Channel::Live | Channel::SysEvent => {
                self.oddities.other_channels += 1;
            }
        }
    }

    fn push_live(&mut self, channel: Channel, data: Vec<u8>, received: SystemTime) {
        if self.live.len() >= MAX_LIVE_QUEUE {
            self.live.pop_front();
        }
        self.live.push_back(LiveNotification {
            channel,
            data,
            received,
        });
    }

    fn check_cancel(&self) -> Result<(), DeviceError> {
        match &self.cancel {
            Some(flag) if flag.load(Ordering::SeqCst) => Err(DeviceError::Cancelled),
            _ => Ok(()),
        }
    }
}

fn unexpected(reply: &BoardReply) -> DeviceError {
    DeviceError::Unexpected(format!("{reply:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TestClock, hex, on};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    /// Plays the blackboard: each write must be the next expected frame; its answers follow.
    struct Board {
        peer: Advertisement,
        clock: Arc<TestClock>,
        steps: VecDeque<(Vec<u8>, Vec<TransportEvent>)>,
        pending: VecDeque<TransportEvent>,
        /// Events that come once the test clock reaches their time (ms), whatever is written.
        late: VecDeque<(u64, TransportEvent)>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Transport for Board {
        fn peer(&self) -> &Advertisement {
            &self.peer
        }
        fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            self.writes.lock().unwrap().push(frame.to_vec());
            let (expected, answers) = self
                .steps
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected write {frame:02x?}"));
            assert_eq!(frame, expected.as_slice(), "write");
            self.pending.extend(answers);
            Ok(())
        }
        fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            let now = self.clock.0.load(Ordering::SeqCst);
            if self.late.front().is_some_and(|(due, _)| *due <= now) {
                return Ok(self.late.pop_front().map(|(_, event)| event));
            }
            let ms = u64::try_from(timeout.as_millis()).unwrap();
            self.clock
                .0
                .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
            Ok(None)
        }
        fn disconnect(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn reply(text: &str) -> TransportEvent {
        on(Channel::Command, hex(text))
    }

    fn data(bytes: &[u8]) -> TransportEvent {
        on(Channel::BoardData, bytes.to_vec())
    }

    /// Every frame the session wrote.
    type Writes = Arc<Mutex<Vec<Vec<u8>>>>;

    fn session(steps: Vec<(&str, Vec<TransportEvent>)>) -> (BoardSession, Writes) {
        let (board, writes, _) = session_with(steps, Vec::new());
        (board, writes)
    }

    /// A scripted session with events coming later (by the test clock, in ms), and the clock.
    fn session_with(
        steps: Vec<(&str, Vec<TransportEvent>)>,
        late: Vec<(u64, TransportEvent)>,
    ) -> (BoardSession, Writes, Arc<TestClock>) {
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let board = Board {
            peer: Advertisement {
                address: "00:00:5E:00:53:B1".into(),
                name: Some("xiaomi.dev.ble".into()),
                rssi: None,
                company_id: None,
                manufacturer_data: Vec::new(),
                xiaomi_service_data: Vec::new(),
            },
            clock: clock.clone(),
            steps: steps.into_iter().map(|(w, a)| (hex(w), a)).collect(),
            pending: VecDeque::new(),
            late: late.into(),
            writes: writes.clone(),
        };
        (
            BoardSession::open(Box::new(board), clock.clone()),
            writes,
            clock,
        )
    }

    /// A page of `n` records (7 bytes each).
    fn page_bytes(n: usize) -> Vec<u8> {
        (0..n)
            .flat_map(|i| [0xa1, i as u8, 0, 0, 1, 0, 4])
            .collect()
    }

    #[test]
    fn information_and_clock() {
        let (mut board, writes) = session(vec![
            // 2026-10-24 12:00:00 UTC, the test clock's start.
            ("11 05 07 6a dc 9d c0", vec![]),
            ("11 01 01", vec![reply("11 02 01 5a")]),
            ("11 01 02", vec![reply("11 09 02 07 90 7e 00 07 8e ba 00")]),
            ("12 01 04", vec![reply("12 07 04 00 02 00 00 00 1c")]),
        ]);
        board.set_time().unwrap();
        let info = board.info().unwrap();
        assert_eq!(
            info,
            BoardInfo {
                battery_percent: 90,
                memory_total: 126_909_952,
                memory_free: 126_794_240,
                stored_pages: 2,
                stored_bytes: 28,
            }
        );
        assert_eq!(writes.lock().unwrap().len(), 4);
    }

    /// The pages a sync keeps, step after step as the tablet service runs them, until the
    /// tablet holds no page (`Ok`) or a step asks for a fresh link (`Err(StreamDirty)`).
    fn sync_all(
        board: &mut BoardSession,
        sync: &mut BoardSync,
        keep: &mut dyn FnMut(DownloadedPage) -> Result<(), DeviceError>,
    ) -> Result<Vec<u32>, DeviceError> {
        let mut kept = Vec::new();
        loop {
            match board.sync_page(sync, &mut *keep)? {
                PageStep::Kept { info, kept: () } => kept.push(info.time),
                PageStep::Done => return Ok(kept),
                PageStep::Reopen => return Err(DeviceError::StreamDirty),
            }
        }
    }

    fn deletes(writes: &Mutex<Vec<Vec<u8>>>) -> usize {
        writes
            .lock()
            .unwrap()
            .iter()
            .filter(|w| w[..3] == [0x12, 0x05, 0x06])
            .count()
    }

    #[test]
    fn a_sync_deletes_each_page_after_the_sink_kept_it() {
        let first = page_bytes(3);
        let second = page_bytes(40);
        let (mut board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&first)]),
            ("12 05 06 68 00 00 01", vec![]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 02 00 00 01 18")]),
            // 280 bytes in two chunks, as the tablet cuts them (244 at most).
            (
                "12 06 03 01 68 00 00 02",
                vec![data(&second[..244]), data(&second[244..])],
            ),
            ("12 05 06 68 00 00 02", vec![]),
            ("12 01 05", vec![reply("12 01 05")]),
        ]);
        let mut kept = Vec::new();
        let mut sync = BoardSync::default();
        let times = sync_all(&mut board, &mut sync, &mut |page| {
            assert_eq!(page.kind, TabletKind::Blackboard);
            kept.push((page.timestamp, page.bytes.len()));
            Ok(())
        })
        .unwrap();
        assert_eq!(times, [0x6800_0001, 0x6800_0002]);
        assert_eq!(kept, [(Some(0x6800_0001), 21), (Some(0x6800_0002), 280)]);
        assert_eq!(sync.pages(), 2);
        assert_eq!(writes.lock().unwrap().len(), 7);
        assert!(!board.is_dirty());
    }

    #[test]
    fn a_page_is_returned_only_after_a_quiet_window() {
        let page = page_bytes(3);
        let (mut board, _) = session(vec![("12 06 03 01 68 00 00 01", vec![data(&page)])]);
        let info = PageInfo {
            time: 0x6800_0001,
            size: 21,
        };
        let start = board.clock().monotonic();
        assert_eq!(board.download(info).unwrap().bytes, page);
        assert!(board.clock().monotonic() - start >= QUIET_WINDOW);

        // A byte more within the window: the count was reached with bytes of another transfer.
        let (mut board, _) = session(vec![(
            "12 06 03 01 68 00 00 01",
            vec![data(&page), data(&page[..7])],
        )]);
        assert_eq!(
            board.download(info),
            Err(DeviceError::TransferIncomplete {
                expected: 21,
                received: 28
            })
        );
        assert!(board.is_dirty());
    }

    #[test]
    fn a_download_is_asked_only_once_the_stream_is_quiet() {
        let page = page_bytes(3);
        let info = PageInfo {
            time: 0x6800_0001,
            size: 21,
        };
        // A fresh link counts as quiet from when it opened: the first download waits for what
        // is left of the window (here all of it, nothing else was asked), the next one, after
        // the quiet window that ends a page, is asked at once.
        let (mut board, _) = session(vec![
            ("12 06 03 01 68 00 00 01", vec![data(&page)]),
            ("12 06 03 01 68 00 00 01", vec![data(&page)]),
        ]);
        let start = board.clock().monotonic();
        assert_eq!(board.download(info).unwrap().bytes, page);
        assert_eq!(board.clock().monotonic() - start, 2 * QUIET_WINDOW);
        let start = board.clock().monotonic();
        assert_eq!(board.download(info).unwrap().bytes, page);
        assert_eq!(board.clock().monotonic() - start, QUIET_WINDOW);

        // Stream bytes nobody asked for just before (the end of a live stream): asked once
        // they stopped for the quiet window, the stream not dirty.
        let (mut board, _, _) = session_with(
            vec![("12 06 03 01 68 00 00 01", vec![data(&page)])],
            vec![(0, data(&page[..14])), (100, data(&page[..7]))],
        );
        let start = board.clock().monotonic();
        assert_eq!(board.download(info).unwrap().bytes, page);
        let took = board.clock().monotonic() - start;
        assert!(
            took >= Duration::from_millis(100) + 2 * QUIET_WINDOW,
            "{took:?}"
        );
        assert_eq!(board.take_oddities().stray_bytes, 21);
        assert!(!board.is_dirty());

        // A stream that never goes quiet (a tablet still in live mode after a reconnection):
        // nothing is asked, the stream is dirty, the transfer incomplete with 0 bytes.
        let records: Vec<(u64, TransportEvent)> =
            (0..40).map(|i| (i * 50, data(&page[..7]))).collect();
        let (board, writes, _) = session_with(vec![], records);
        let mut board = board.with_timeouts(REPLY_TIMEOUT, Duration::from_secs(1));
        assert_eq!(
            board.download(info),
            Err(DeviceError::TransferIncomplete {
                expected: 21,
                received: 0
            })
        );
        assert!(writes.lock().unwrap().is_empty(), "no request written");
        assert!(board.is_dirty());
    }

    #[test]
    fn a_page_that_cannot_be_kept_is_not_deleted() {
        let (mut board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page_bytes(3))]),
        ]);
        let result = sync_all(&mut board, &mut BoardSync::default(), &mut |_| {
            Err(DeviceError::Storage("disk full".into()))
        });
        assert_eq!(result, Err(DeviceError::Storage("disk full".into())));
        assert_eq!(deletes(&writes), 0);
    }

    #[test]
    fn a_stalled_transfer_is_retried_once_on_a_fresh_link_only() {
        let page = page_bytes(10);
        // The first link: the transfer stalls, the step asks for a fresh link.
        let (board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 46")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page[..30])]),
        ]);
        let mut board = board.with_timeouts(REPLY_TIMEOUT, Duration::from_secs(2));
        let mut sync = BoardSync::default();
        let step = board.sync_page(&mut sync, |_| -> Result<(), DeviceError> {
            panic!("nothing to keep")
        });
        assert_eq!(step, Ok(PageStep::Reopen));
        assert!(board.is_dirty());
        // This link refuses to download or to stream again, without writing anything.
        let info = PageInfo {
            time: 0x6800_0001,
            size: 70,
        };
        assert_eq!(board.download(info), Err(DeviceError::StreamDirty));
        assert_eq!(board.start_live(), Err(DeviceError::StreamDirty));
        assert_eq!(
            board.sync_page(&mut sync, |_| -> Result<(), DeviceError> { panic!() }),
            Ok(PageStep::Reopen)
        );
        assert_eq!(writes.lock().unwrap().len(), 2);

        // The fresh link: the retry stalls again, a typed error, nothing deleted.
        let (board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 46")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page[..44])]),
        ]);
        let mut board = board.with_timeouts(REPLY_TIMEOUT, Duration::from_secs(2));
        let result = sync_all(&mut board, &mut sync, &mut |_| panic!("nothing to keep"));
        assert_eq!(
            result,
            Err(DeviceError::TransferIncomplete {
                expected: 70,
                received: 44
            })
        );
        assert_eq!(deletes(&writes), 0);
        assert!(board.is_dirty());

        // Once more on a third link, the page completes: the retry was this page's only.
        let (mut board, _) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 46")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page)]),
            ("12 05 06 68 00 00 01", vec![]),
            ("12 01 05", vec![reply("12 01 05")]),
        ]);
        let times = sync_all(&mut board, &mut sync, &mut |_| Ok(())).unwrap();
        assert_eq!(times, [0x6800_0001]);
    }

    #[test]
    fn bytes_after_a_page_make_the_stream_dirty_and_keep_the_page_on_the_tablet() {
        let page = page_bytes(3);
        // More of the page comes after the quiet window, while its note is written: the page
        // may be wrong, it stays on the tablet.
        let (mut board, writes, clock) = session_with(
            vec![
                ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
                ("12 06 03 01 68 00 00 01", vec![data(&page)]),
            ],
            vec![(1_000, data(&page[..14]))],
        );
        let mut sync = BoardSync::default();
        let step = board.sync_page(&mut sync, |_| {
            clock.0.fetch_add(2_000, Ordering::SeqCst);
            Ok(())
        });
        assert_eq!(step, Ok(PageStep::Reopen));
        assert_eq!(deletes(&writes), 0);
        assert!(board.is_dirty());

        // That used up the page's retry: the same again on the fresh link ends the sync, rather
        // than reopen the link again and again (the page and its note stay).
        let (mut board, writes, clock) = session_with(
            vec![
                ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
                ("12 06 03 01 68 00 00 01", vec![data(&page)]),
            ],
            vec![(1_000, data(&page[..14]))],
        );
        let step = board.sync_page(&mut sync, |_| {
            clock.0.fetch_add(2_000, Ordering::SeqCst);
            Ok(())
        });
        assert_eq!(
            step,
            Err(DeviceError::TransferIncomplete {
                expected: 21,
                received: 35
            })
        );
        assert_eq!(deletes(&writes), 0);
        // A stall after bytes during the keep: the retry is used up the same way.
        let (board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page[..7])]),
        ]);
        let mut board = board.with_timeouts(REPLY_TIMEOUT, Duration::from_secs(1));
        let mut sync = BoardSync {
            retried: Some(0x6800_0001),
            ..BoardSync::default()
        };
        let step = board.sync_page(&mut sync, |_| -> Result<(), DeviceError> { panic!() });
        assert!(
            matches!(step, Err(DeviceError::TransferIncomplete { .. })),
            "{step:?}"
        );
        assert_eq!(deletes(&writes), 0);

        // Bytes after the delete of page 1: the next page is not downloaded on this link.
        let (mut board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page)]),
            ("12 05 06 68 00 00 01", vec![data(&page[..14])]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 02 00 00 00 15")]),
        ]);
        let result = sync_all(&mut board, &mut BoardSync::default(), &mut |_| Ok(()));
        assert_eq!(result, Err(DeviceError::StreamDirty));
        assert!(board.is_dirty());
        assert_eq!(board.take_oddities().stray_bytes, 14);
        assert_eq!(writes.lock().unwrap().len(), 4, "no second download");

        // Stray bytes outside any download (the end of a live stream) are only counted.
        let (mut board, _) = session(vec![("11 01 01", vec![data(&page), reply("11 02 01 64")])]);
        assert_eq!(board.battery(), Ok(100));
        assert!(!board.is_dirty());
    }

    #[test]
    fn an_unreadable_page_is_refused_and_stays_on_the_tablet() {
        // Seven bytes of a record cut at another offset: not a known status.
        let mut page = page_bytes(3);
        page.rotate_left(3);
        let (mut board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page)]),
        ]);
        let result = sync_all(&mut board, &mut BoardSync::default(), &mut |_| {
            panic!("an unreadable page is never kept")
        });
        assert_eq!(
            result,
            Err(DeviceError::PageUnreadable {
                time: 0x6800_0001,
                reason: "3 records, 3 of an unknown status, 0 bytes of a partial record".into()
            })
        );
        assert_eq!(deletes(&writes), 0);
        assert!(board.is_dirty());

        // A size larger than the tablet's memory: refused before anything is sent.
        let (mut board, writes) = session(vec![
            ("11 01 02", vec![reply("11 09 02 07 90 7e 00 07 8e ba 00")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 10 00 00 00")]),
        ]);
        assert_eq!(board.memory(), Ok((126_909_952, 126_794_240)));
        let result = sync_all(&mut board, &mut BoardSync::default(), &mut |_| panic!());
        assert!(
            matches!(result, Err(DeviceError::PageUnreadable { .. })),
            "{result:?}"
        );
        assert_eq!(writes.lock().unwrap().len(), 2, "no download asked");
        assert!(!board.is_dirty());
    }

    #[test]
    fn a_page_still_described_after_its_deletion_is_asked_once_more_then_stops_the_sync() {
        // The tablet took a moment to erase: described again a little later, the next page.
        let (mut board, writes) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page_bytes(3))]),
            ("12 05 06 68 00 00 01", vec![]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 02 00 00 00 15")]),
            ("12 06 03 01 68 00 00 02", vec![data(&page_bytes(3))]),
            ("12 05 06 68 00 00 02", vec![]),
            ("12 01 05", vec![reply("12 01 05")]),
        ]);
        let before = board.clock().monotonic();
        let times = sync_all(&mut board, &mut BoardSync::default(), &mut |_| Ok(())).unwrap();
        assert_eq!(times, [0x6800_0001, 0x6800_0002]);
        assert_eq!(deletes(&writes), 2);
        assert!(board.clock().monotonic() - before >= DELETE_SETTLE);

        // Still there the second time: the sync stops rather than import it again.
        let (mut board, _) = session(vec![
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page_bytes(3))]),
            ("12 05 06 68 00 00 01", vec![]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
        ]);
        let mut kept = 0;
        let result = sync_all(&mut board, &mut BoardSync::default(), &mut |_| {
            kept += 1;
            Ok(())
        });
        assert_eq!(result, Err(DeviceError::PageNotDeleted(0x6800_0001)));
        assert_eq!(kept, 1);
    }

    #[test]
    fn an_empty_tablet_or_a_full_sync_reads_nothing_more() {
        let (mut board, _) = session(vec![("12 01 05", vec![reply("12 01 05")])]);
        assert_eq!(
            sync_all(&mut board, &mut BoardSync::default(), &mut |_| panic!(
                "no page"
            )),
            Ok(Vec::new())
        );
        // After MAX_SYNC_PAGES the step writes nothing (the script has no step).
        let (mut board, writes) = session(vec![]);
        let mut sync = BoardSync {
            pages: MAX_SYNC_PAGES,
            ..BoardSync::default()
        };
        assert_eq!(
            board.sync_page(&mut sync, |_| -> Result<(), DeviceError> { panic!() }),
            Ok(PageStep::Done)
        );
        assert!(writes.lock().unwrap().is_empty());
    }

    #[test]
    fn a_cancelled_download_leaves_the_stream_dirty() {
        let (mut board, _) = session(vec![(
            "12 06 03 01 68 00 00 01",
            vec![data(&page_bytes(1))],
        )]);
        let cancel = Arc::new(AtomicBool::new(true));
        board.set_cancel_flag(cancel.clone());
        let info = PageInfo {
            time: 0x6800_0001,
            size: 70,
        };
        assert_eq!(board.download(info), Err(DeviceError::Cancelled));
        assert!(board.is_dirty());
        cancel.store(false, Ordering::SeqCst);
        // The rest of the page must not pass for pen records.
        assert_eq!(board.start_live(), Err(DeviceError::StreamDirty));
        assert!(!board.in_live_mode());
    }

    #[test]
    fn unknown_input_is_ignored_and_counted() {
        let (mut board, _) = session(vec![(
            "11 01 01",
            vec![
                reply("12 01 0b"),
                reply("13"),
                data(&[0xa1; 7]),
                on(Channel::SysEvent, vec![1]),
                reply("12 01 09"),
                reply("11 02 01 64"),
            ],
        )]);
        assert_eq!(board.battery().unwrap(), 100);
        let oddities = board.take_oddities();
        assert_eq!(oddities.frames, ["12 01 0b", "13"]);
        assert_eq!(oddities.stray_bytes, 7);
        assert_eq!(oddities.other_channels, 1);
        assert!(board.take_page_stored());
        assert!(!board.take_page_stored());
        assert!(board.take_oddities().is_empty());
    }

    #[test]
    fn live_mode_hands_over_records_and_events_in_order() {
        let records = page_bytes(6);
        let (mut board, _) = session(vec![
            (
                "12 02 08 01",
                vec![data(&records), reply("12 01 09"), data(&records[..7])],
            ),
            ("12 02 08 02", vec![]),
        ]);
        board.start_live().unwrap();
        assert!(board.in_live_mode());
        let mut seen = Vec::new();
        while let Some(n) = board.next_live_event(Duration::from_millis(100)).unwrap() {
            seen.push((n.channel, n.data.len()));
        }
        assert_eq!(
            seen,
            [
                (Channel::BoardData, 42),
                (Channel::Command, 3),
                (Channel::BoardData, 7)
            ]
        );
        assert!(
            board.take_page_stored(),
            "live mode goes on, the event is noted"
        );
        board.stop_live().unwrap();
        assert!(!board.in_live_mode());
    }

    #[test]
    fn a_lost_link_is_disconnected() {
        let (mut board, _) = session(vec![("12 01 04", vec![TransportEvent::Disconnected])]);
        assert_eq!(board.page_count(), Err(DeviceError::Disconnected));
        assert!(!board.is_connected());
        assert_eq!(board.battery(), Err(DeviceError::Disconnected));
    }
}
