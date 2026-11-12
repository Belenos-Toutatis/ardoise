//! The Xiaomi blackboard's half of the worker (docs/design/blackboard.md): adding it (a search
//! that lists the blackboards nearby, then the user's choice), connecting to it when it is the
//! known tablet in range, its information, its sync and its live mode.
//!
//! - **No registration**: adding saves `blackboard.json` and nothing else; nothing is written
//!   to the tablet, and nothing ever to its Xiaomi service.
//! - **Sync**: the tablet only describes its oldest page, so reading the next one needs the
//!   deletion of the first. Each page is deleted (`12 05 06`) only once its note is written and
//!   read back ([`keep_page`], the store of the app). The automatic sync at connection runs only
//!   when "delete after import" is on; otherwise the pages are imported on request only (SPEC
//!   §4.1), and still deleted.
//! - **Live mode**: `a1` records are pen-down samples, `a0` hovering (the laser pointer), `c0`
//!   the pen leaving the range. The "page stored" event does not interrupt it. A lost link is
//!   reconnected by the normal search, as for the Slate.
//! - **The Slate goes first**: while the blackboard holds the link and nothing happens, a known
//!   Slate advertising (its button pressed) gets the link.
//! - Unknown or malformed frames, stray stream bytes: noted, the log kept, never an error.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ardoise_core::library::Library;
use ardoise_core::settings::Orientation;
use ardoise_proto::blackboard::beacon::BLACKBOARD_MODEL;
use ardoise_proto::blackboard::{BOARD_HEIGHT, BOARD_WIDTH, BoardReply, Record, decode_records};
use ardoise_proto::live::LiveRecord;

use super::{
    Connection, IDLE_POLL, ImportedPages, LIVE_POLL, LiveInput, PageContext, SEARCH_PAUSE,
    SEARCH_SLICE, StatusError, SyncProgress, SyncRun, Worker, keep_page, pages_text, saved_count,
};
use crate::blackboard::{BoardInfo, Oddities};
use crate::identity::{
    self, BLACKBOARD_FORMAT, BLACKBOARD_PROTOCOL, BlackboardRecord, DeviceRecord,
};
use crate::pages::BLACKBOARD_PAGE;
use crate::service::{
    BLACKBOARD_MODEL_NAME, BLACKBOARD_NAME, BOARD_SEARCH, BOARD_SEARCH_SLICE, BoardSearch,
    DeviceDetails, DeviceMemory, FoundTablet, SLATE_CHECK_INTERVAL, SLATE_CHECK_SCAN,
};
use crate::session::{DeviceError, LiveNotification};
use crate::transport::{Advertisement, Channel, TabletKind};

/// The search for a blackboard to add.
pub(in crate::service) struct AddSearch {
    found: Vec<FoundTablet>,
    /// When it gives up, on the connector's clock.
    until: Duration,
    /// The same, unix milliseconds (shown).
    until_ms: i64,
}

impl Worker {
    // --- adding ---------------------------------------------------------------------------

    /// Starts the search for blackboards to add (closes a link open meanwhile: one at a time).
    pub(super) fn find_blackboards(&mut self) {
        if self.sync.is_some() || self.live.as_ref().is_some_and(|run| run.active) {
            self.show_error(&DeviceError::Busy);
            return;
        }
        if self.config.library.is_none() || self.config.identity_root.is_none() {
            self.show_error(&DeviceError::NoLibrary);
            return;
        }
        if !self.ensure_manager() {
            return;
        }
        self.close_session("looking for a blackboard to add");
        if let Some(journal) = self.journal.as_mut() {
            journal.begin();
            journal.note("looking for a blackboard to add");
        }
        let Some(until) = self
            .manager
            .as_ref()
            .map(|manager| manager.clock().monotonic() + BOARD_SEARCH)
        else {
            return;
        };
        let until_ms = self
            .now_ms()
            .saturating_add(i64::try_from(BOARD_SEARCH.as_millis()).unwrap_or(i64::MAX));
        self.adding = Some(AddSearch {
            found: Vec::new(),
            until,
            until_ms,
        });
        self.publisher.update(|status| status.error = None);
        self.publish_search();
    }

    /// One slice of the search: the blackboards heard are published.
    pub(super) fn add_step(&mut self) {
        let Some(manager) = self.manager.as_mut() else {
            self.adding = None;
            return;
        };
        let now = manager.clock().monotonic();
        let Some(until) = self.adding.as_ref().map(|search| search.until) else {
            return;
        };
        if now >= until {
            let nothing = self
                .adding
                .as_ref()
                .is_some_and(|search| search.found.is_empty());
            self.note("the search for a blackboard ended");
            let error = nothing.then(|| {
                DeviceError::TabletNotFound(format!(
                    "nothing heard in {} s",
                    BOARD_SEARCH.as_secs()
                ))
            });
            return self.end_search(error);
        }
        let heard = manager.find_blackboards((until - now).min(BOARD_SEARCH_SLICE));
        match heard {
            Ok(heard) => {
                let mut new = Vec::new();
                if let Some(search) = self.adding.as_mut() {
                    for ad in heard {
                        let found = FoundTablet {
                            address: ad.address.clone(),
                            name: ad.name.clone(),
                            rssi: ad.rssi,
                        };
                        match search
                            .found
                            .iter_mut()
                            .find(|f| f.address.eq_ignore_ascii_case(&ad.address))
                        {
                            Some(known) => *known = found,
                            None => {
                                new.push(ad.address.clone());
                                search.found.push(found);
                            }
                        }
                    }
                }
                for address in new {
                    self.note(format!("heard a blackboard at {address}"));
                }
                self.publish_search();
            }
            // A request waits: handled before the next step.
            Err(DeviceError::Cancelled) => {}
            Err(error) => self.end_search(Some(error)),
        }
    }

    fn publish_search(&self) {
        let board_search = self.adding.as_ref().map(|search| BoardSearch {
            found: search.found.clone(),
            until: search.until_ms,
        });
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Adding;
            status.board_search = board_search;
            status.pending = pending;
        });
    }

    /// Ends the search (cancelled, timed out, failed); `error` is shown.
    pub(super) fn end_search(&mut self, error: Option<DeviceError>) {
        self.adding = None;
        if let Some(error) = &error {
            self.keep(error.to_string());
            self.end_journal("the search for a blackboard failed");
        }
        let connection = self.waiting_connection();
        let error = error.as_ref().map(StatusError::from);
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = connection;
            status.board_search = None;
            status.pending = pending;
            if error.is_some() {
                status.error = error;
            }
        });
    }

    /// Adds the blackboard heard at `address` by the search under way: `blackboard.json`
    /// (replacing another blackboard known before), then the search ends and the worker
    /// connects to it as to any known tablet.
    pub(super) fn add_blackboard(&mut self, address: &str) -> Result<(), DeviceError> {
        let root = self
            .config
            .identity_root
            .clone()
            .ok_or(DeviceError::NoLibrary)?;
        let wanted = address.trim();
        let found = self.adding.as_ref().and_then(|search| {
            search
                .found
                .iter()
                .find(|f| f.address.eq_ignore_ascii_case(wanted))
                .cloned()
        });
        let Some(found) = found else {
            return Err(DeviceError::TabletNotFound(format!(
                "{wanted} was not heard by the search"
            )));
        };
        let now = self
            .manager
            .as_ref()
            .map_or_else(std::time::SystemTime::now, |manager| manager.clock().now());
        let record = BlackboardRecord {
            format: BLACKBOARD_FORMAT,
            address: found.address,
            name: BLACKBOARD_NAME.to_owned(),
            protocol: BLACKBOARD_PROTOCOL.to_owned(),
            model: BLACKBOARD_MODEL.to_owned(),
            added: chrono::DateTime::<chrono::Local>::from(now)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            extra: Default::default(),
        };
        identity::save_blackboard(&root, &record)
            .map_err(|e| DeviceError::Storage(e.to_string()))?;
        self.note(format!("blackboard {} added", record.address));
        self.board_record = Some(record);
        self.adding = None;
        let known = self.known_tablets();
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Searching;
            status.board_search = None;
            status.known = known;
            status.pending = pending;
            status.error = None;
        });
        Ok(())
    }

    /// Disconnects the blackboard if it holds the link, and forgets it (`blackboard.json`).
    pub(super) fn forget_blackboard(&mut self) -> Result<(), DeviceError> {
        let root = self
            .config
            .identity_root
            .clone()
            .ok_or(DeviceError::NoLibrary)?;
        if self.board.is_some() {
            if let Some(run) = self.sync.take() {
                self.finish_sync(run, None);
            }
            self.stop_live();
            self.close_session("the blackboard is forgotten");
            self.want_sync = false;
            self.asked_sync = false;
        }
        identity::forget_blackboard(&root).map_err(|e| DeviceError::Storage(e.to_string()))?;
        self.board_record = None;
        self.note("blackboard forgotten");
        let slate_connected = self.session.is_some();
        let connection = self.waiting_connection();
        let known = self.known_tablets();
        let pending = self.pending();
        self.publisher.update(|status| {
            if !slate_connected {
                status.connection = connection;
                status.sync = None;
                status.error = None;
            }
            if status
                .device
                .as_ref()
                .is_some_and(|d| d.kind == TabletKind::Blackboard)
            {
                status.device = None;
            }
            status.known = known;
            status.pending = pending;
        });
        Ok(())
    }

    // --- connection -------------------------------------------------------------------------

    /// Looks for whichever known tablet is in range during one slice, and connects to it.
    pub(super) fn search_known(&mut self, slate: Option<&DeviceRecord>, board: &BlackboardRecord) {
        self.publish(Connection::Searching);
        if let Some(journal) = self.journal.as_mut() {
            journal.begin();
        }
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        match manager.find_known(slate, Some(board), SEARCH_SLICE, false) {
            Ok(Some((TabletKind::Slate, tablet))) => {
                if let Some(record) = slate {
                    self.connect(record, &tablet);
                }
            }
            Ok(Some((TabletKind::Blackboard, tablet))) => self.board_connect(&tablet),
            Ok(None) => self.wait(Some(SEARCH_PAUSE)),
            Err(DeviceError::Cancelled) => {}
            Err(error) => self.connection_failed(&error),
        }
    }

    /// Connects (clock set), then reads the battery, the memory and the page count.
    fn board_connect(&mut self, tablet: &Advertisement) {
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Connecting;
            status.active = Some(TabletKind::Blackboard);
            status.pending = pending;
        });
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        let result = manager
            .connect_blackboard(&tablet.address)
            .and_then(|mut board| {
                let info = board.info()?;
                Ok((board, info))
            });
        match result {
            Ok((board, info)) => {
                self.board = Some(board);
                self.board_connected(&info);
            }
            Err(DeviceError::Cancelled) => {}
            Err(error) => self.connection_failed(&error),
        }
    }

    fn board_connected(&mut self, info: &BoardInfo) {
        if let Some(journal) = self.journal.as_mut() {
            journal.success();
            journal.note(format!(
                "connected to the blackboard: battery {}%, {} stored ({} bytes), {} of {} bytes free",
                info.battery_percent,
                pages_text(info.stored_pages),
                info.stored_bytes,
                info.memory_free,
                info.memory_total
            ));
        }
        self.counted_at = self.session_time();
        self.slate_checked_at = self.session_time();
        let options = self.config.options;
        // Its pages are imported automatically only when they may be deleted; on request always.
        let wanted = self.asked_sync
            || (options.auto_sync && options.delete_after_import && info.stored_pages > 0);
        if self.want_sync && !wanted {
            self.note(
                "a sync was waiting, not asked for the blackboard: its pages are imported on \
                 request only while delete after import is off",
            );
        }
        self.want_sync = wanted;
        let details = self.board_details(info);
        let known = self.known_tablets();
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Connected;
            status.active = Some(TabletKind::Blackboard);
            status.device = Some(details);
            status.known = known;
            status.pending = pending;
            status.error = None;
        });
    }

    fn board_details(&self, info: &BoardInfo) -> DeviceDetails {
        DeviceDetails {
            kind: TabletKind::Blackboard,
            name: self
                .board_record
                .as_ref()
                .map_or_else(|| BLACKBOARD_NAME.to_owned(), |record| record.name.clone()),
            firmware: String::new(),
            battery_percent: info.battery_percent.min(100),
            charging: false,
            width: BOARD_WIDTH,
            height: BOARD_HEIGHT,
            stored_pages: info.stored_pages,
            model: BLACKBOARD_MODEL_NAME.to_owned(),
            read_at: self.now_ms(),
            memory: Some(DeviceMemory {
                total: info.memory_total,
                free: info.memory_free,
            }),
        }
    }

    /// One bounded step while the blackboard holds the link.
    pub(super) fn board_step(&mut self) {
        self.board_events();
        if self.sync.is_some() {
            self.board_sync_step();
        } else if self.live.is_some() {
            self.board_live_step();
        } else if self.want_sync {
            self.board_start_sync();
        } else if self.slate_check_due() {
            self.check_for_slate();
        } else if self.page_count_due() {
            self.board_count_pages();
        } else {
            let result = match self.board.as_mut() {
                Some(board) => board.idle(Duration::ZERO),
                None => return,
            };
            match result {
                Ok(()) => {
                    self.publish(Connection::Connected);
                    self.wait(Some(IDLE_POLL));
                }
                Err(error) => self.session_failed(error),
            }
        }
    }

    /// What the session dropped (noted, the log kept) and the "page stored" event (the page
    /// count is read again as soon as nothing else runs).
    fn board_events(&mut self) {
        let Some(board) = self.board.as_mut() else {
            return;
        };
        let oddities = board.take_oddities();
        let stored = board.take_page_stored();
        if !oddities.is_empty() {
            self.note(describe_oddities(&oddities));
            self.keep("input of the blackboard ignored");
        }
        if stored {
            self.note("a page was stored with the tablet's button");
            self.count_now = true;
        }
    }

    fn slate_check_due(&self) -> bool {
        if self.record.is_none() || self.board.is_none() {
            return false;
        }
        match (self.slate_checked_at, self.session_time()) {
            (Some(at), Some(now)) => now.saturating_sub(at) >= SLATE_CHECK_INTERVAL,
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// A short look for the known Slate: when it advertises, it gets the link.
    fn check_for_slate(&mut self) {
        self.slate_checked_at = self.session_time();
        let Some(record) = self.record.clone() else {
            return;
        };
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        match manager.find_known(Some(&record), None, SLATE_CHECK_SCAN, true) {
            Ok(Some((_, tablet))) => {
                self.note("the Bamboo Slate asks to connect: it goes first");
                self.close_session("the Bamboo Slate asks to connect");
                self.connect(&record, &tablet);
            }
            Ok(None) | Err(DeviceError::Cancelled) => {}
            Err(error) => self.note(format!("could not look for the Bamboo Slate: {error}")),
        }
    }

    fn board_count_pages(&mut self) {
        self.counted_at = self.session_time();
        self.count_now = false;
        let counted = match self.board.as_mut() {
            Some(board) => board.page_count(),
            None => return,
        };
        let (pages, bytes) = match counted {
            Ok(counted) => counted,
            Err(error) => return self.session_failed(error),
        };
        let before = self.stored_pages();
        if pages != before {
            self.note(format!(
                "page count: {} on the blackboard now ({bytes} bytes), {before} after the last \
                 count or sync",
                pages_text(pages)
            ));
        }
        let options = self.config.options;
        if options.auto_sync && options.delete_after_import && pages > before {
            self.want_sync = true;
        }
        let pending = self.pending();
        self.publisher.update(|status| {
            if let Some(device) = status.device.as_mut() {
                device.stored_pages = pages;
            }
            status.pending = pending;
        });
    }

    /// Sets the blackboard's clock again; it cannot be asked its time, so the drift is 0.
    pub(super) fn board_set_time(&mut self) -> Result<i64, DeviceError> {
        if self.sync.is_some() || self.live.as_ref().is_some_and(|run| run.active) {
            return Err(DeviceError::Busy);
        }
        let result = match self.board.as_mut() {
            Some(board) => board.set_time(),
            None => return Err(DeviceError::NotConnected),
        };
        if result == Err(DeviceError::Disconnected) {
            self.link_lost();
        }
        result?;
        self.note("clock set (the blackboard cannot be asked its time)");
        Ok(0)
    }

    // --- sync -------------------------------------------------------------------------------

    fn board_start_sync(&mut self) {
        let asked = self.asked_sync;
        self.want_sync = false;
        self.asked_sync = false;
        if !asked && !self.config.options.delete_after_import {
            self.note("the blackboard's pages are imported on request only");
            let pending = self.pending();
            self.publisher.update(|status| status.pending = pending);
            return;
        }
        let Some(root) = self.config.library.clone() else {
            self.show_error(&DeviceError::NoLibrary);
            return;
        };
        let library = match Library::open(&root) {
            Ok(library) => library,
            Err(error) => {
                self.show_error(&DeviceError::Storage(error.to_string()));
                return;
            }
        };
        let counted = match self.board.as_mut() {
            Some(board) => board.page_count(),
            None => return,
        };
        let (total, bytes) = match counted {
            Ok(counted) => counted,
            Err(error) => return self.sync_not_started(error),
        };
        let imported = if total > 0 {
            ImportedPages::scan(&library)
        } else {
            ImportedPages::default()
        };
        self.note(format!(
            "sync: {} on the blackboard ({bytes} bytes); each is deleted from it once its note \
             is written and checked",
            pages_text(total)
        ));
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Syncing;
            status.pending = pending;
            status.sync = Some(SyncProgress {
                current: 0,
                total,
                saved: 0,
            });
            if let Some(device) = status.device.as_mut() {
                device.stored_pages = total;
            }
        });
        let run = SyncRun {
            library,
            root,
            imported,
            total,
            to_read: total,
            done: 0,
            // The only way to the next page.
            delete: true,
            saved: Vec::new(),
            deleted: 0,
            context: PageContext {
                size: BLACKBOARD_PAGE,
                orientation: Orientation::Landscape,
            },
            last_deleted: None,
        };
        if total == 0 {
            self.finish_sync(run, None);
        } else {
            self.sync = Some(run);
        }
    }

    /// Describes, downloads, stores and only then deletes one page.
    fn board_sync_step(&mut self) {
        let (Some(run), Some(board)) = (self.sync.as_mut(), self.board.as_mut()) else {
            return;
        };
        let index = run.done + 1;
        let (to_read, saved) = (run.to_read, saved_count(run));
        self.publisher.update(|status| {
            status.sync = Some(SyncProgress {
                current: index,
                total: to_read,
                saved,
            });
        });
        let info = match board.oldest_page() {
            Ok(Some(info)) => info,
            // Fewer pages than counted (one deleted elsewhere): done.
            Ok(None) => {
                if let Some(run) = self.sync.take() {
                    self.finish_sync(run, None);
                }
                return;
            }
            Err(error) => return self.sync_failed(error),
        };
        if run.last_deleted == Some(info.time) {
            return self.sync_failed(DeviceError::PageNotDeleted(info.time));
        }
        let page = match board.download(info) {
            // Notifications can be lost on the way (BLE stack buffers): try once more.
            Err(DeviceError::TransferIncomplete { .. }) => board.download(info),
            other => other,
        };
        let page = match page {
            Ok(page) => page,
            Err(error) => return self.sync_failed(error),
        };
        let message = match keep_page(&mut self.store_page, run, page, index) {
            Ok(message) => message,
            // The page stays on the tablet.
            Err(error) => return self.sync_failed(error),
        };
        // Only now, the page safe in the library, may it leave the tablet; a cancellation
        // arriving meanwhile takes effect after it.
        board.set_cancel_flag(Arc::new(AtomicBool::new(false)));
        let deleted = board.delete(info.time);
        board.set_cancel_flag(self.interrupt.clone());
        if let Err(error) = deleted {
            return self.sync_failed(error);
        }
        run.deleted += 1;
        run.last_deleted = Some(info.time);
        run.done = index;
        let finished = run.done >= run.to_read;
        let saved = saved_count(run);
        self.note(message);
        self.publisher.update(|status| {
            status.sync = Some(SyncProgress {
                current: index,
                total: to_read,
                saved,
            });
        });
        if finished && let Some(run) = self.sync.take() {
            self.finish_sync(run, None);
        }
    }

    // --- live mode --------------------------------------------------------------------------

    /// Starts the stream if needed, then forwards one notification.
    fn board_live_step(&mut self) {
        let Some(board) = self.board.as_mut() else {
            return;
        };
        if !board.in_live_mode() {
            if let Err(error) = board.start_live() {
                return self.live_failed(error);
            }
            let Some(mut run) = self.live.take() else {
                return;
            };
            self.started(&mut run);
            run.active = true;
            self.live = Some(run);
            self.note("live mode started on the blackboard");
            self.publish(Connection::Live);
            self.hooks.blackboard_live_started();
            return;
        }
        match board.next_live_event(LIVE_POLL) {
            Ok(Some(notification)) => self.board_forward(notification),
            Ok(None) | Err(DeviceError::Cancelled) => {}
            Err(error) => self.live_failed(error),
        }
    }

    /// Hands the records of a notification to the sink (the blackboard's page is never turned).
    /// Records of an unknown status, or bytes of a partial record, are noted and keep the log.
    pub(super) fn board_forward(&mut self, notification: LiveNotification) {
        match notification.channel {
            Channel::BoardData => {
                let decoded = decode_records(&notification.data);
                let records: Vec<LiveRecord> =
                    decoded.records.iter().filter_map(Record::to_live).collect();
                let unknown = decoded.records.len() - records.len();
                if unknown > 0 || decoded.trailing > 0 {
                    self.note(format!(
                        "live: {unknown} records of unknown status and {} bytes of a partial \
                         record ignored: {}",
                        decoded.trailing,
                        ardoise_proto::hex::encode(&notification.data)
                    ));
                    self.keep("unclassified records during live mode");
                }
                if !records.is_empty()
                    && let Some(run) = self.live.as_mut()
                {
                    run.sink.input(LiveInput::Records {
                        records,
                        received: notification.received,
                    });
                }
            }
            Channel::Command => {
                if let Ok(reply) = BoardReply::decode(&notification.data)
                    && let Some(input) = live_input(&reply)
                    && let Some(run) = self.live.as_mut()
                {
                    run.sink.input(input);
                }
            }
            Channel::PageData | Channel::Live | Channel::SysEvent => {}
        }
        self.count_live_notification();
    }
}

/// What a frame of the reply characteristic does to live mode. Nothing today: the "page stored"
/// event (`12 01 09`) does not interrupt it (SPEC §4.2), and what the erase button sends is not
/// measured yet (research note §5). Once it is, its frame (a new [`BoardReply`] variant) maps
/// here to a new page, and nowhere else; unknown frames are already noted by
/// [`Worker::board_step`] (the session's oddities).
fn live_input(reply: &BoardReply) -> Option<LiveInput> {
    match reply {
        BoardReply::PageStored
        | BoardReply::Battery { .. }
        | BoardReply::Memory { .. }
        | BoardReply::PageCount { .. }
        | BoardReply::OldestPage(_)
        | BoardReply::Unknown(_) => None,
    }
}

/// One line for the log about what the session dropped.
fn describe_oddities(oddities: &Oddities) -> String {
    let mut parts = Vec::new();
    if !oddities.frames.is_empty() {
        parts.push(format!(
            "{} unknown or malformed frames ({})",
            oddities.frames.len(),
            oddities.frames.join(", ")
        ));
    }
    if oddities.stray_bytes > 0 {
        parts.push(format!(
            "{} stream bytes outside a download or live mode",
            oddities.stray_bytes
        ));
    }
    if oddities.other_channels > 0 {
        parts.push(format!(
            "{} notifications of a channel the blackboard does not have",
            oddities.other_channels
        ));
    }
    format!("ignored from the blackboard: {}", parts.join("; "))
}
