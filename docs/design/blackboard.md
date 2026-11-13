# The Xiaomi blackboard (milestone M18)

Developer document (English). Protocol facts: [xiaomi-blackboard.md](../research/xiaomi-blackboard.md) (the only source
of what may be sent to the tablet). Requirements: SPEC §3.2 (`device.json` line), §4.1 (« Tablette Xiaomi »), §4.2
(« Avec la tablette Xiaomi »), §4.8; ROADMAP M18; `docs/questions.md` Q14. The Slate's stack:
[device-stack.md](device-stack.md), [tablet-live-classroom.md](tablet-live-classroom.md).

Section 5 is the contract for the interface: commands, payloads, status in each state, error codes, what to show
and hide.

## 1. What exists, layer by layer

```
ardoise-proto::blackboard   frames, requests (the nine observed, nothing else), replies, events, 7-byte records,
                            page decoding and timing, MiBeacon, describe_*            (pure, no I/O)
ardoise-device              Channel::BoardData, Advertisement::xiaomi_service_data, TabletKind,
                            identity::BlackboardRecord (blackboard.json), blackboard::BoardSession (flows),
                            Manager::{find_known, find_blackboards, connect_blackboard}, pages (blackboard notes),
                            service: the two tablets, the search to add one, sync, live mode
ardoise-ble                 recognition (fe95 service data), GATT layout per kind, fe95 and DFU never touched
ardoise-sim                 SimBoard (pages, screen, store/erase buttons, live writing, faults), SimDevice,
                            connectors over several devices, SimHooks for the blackboard, app_board()
app/src-tauri               tablet_find_blackboards, tablet_add_blackboard, tablet_forget_blackboard,
                            sim_blackboard_store_page, --simulated-blackboard, --import-blackboard-pages
```

### ardoise-proto

- `BoardFrame { group, code, payload }`: `group, length, code, payload`, `length` counting code and payload; integers
  big-endian. `BoardFrame::decode` rejects a wrong length, a length of 0, fewer than 3 bytes.
- `BoardRequest`: `Battery` `11 01 01`, `Memory` `11 01 02`, `SetTime{unix}` `11 05 07 u32`, `PageCount` `12 01 04`,
  `OldestPage` `12 01 05`, `Download{time}` `12 06 03 01 u32`, `Delete{time}` `12 05 06 u32`, `StartLive`
  `12 02 08 01`, `StopLive` `12 02 08 02`. **There is no "unknown" variant**: the host cannot encode any other frame;
  `from_frame` (the simulator's side) refuses the rest (`UnknownRequest`). The `01` of the download is a constant
  (`DOWNLOAD_FLAG`), never a parameter.
- `BoardReply`: `Battery{percent}`, `Memory{total, free}`, `PageCount{count, bytes}`, `OldestPage(Option<PageInfo>)`
  (the bare `12 01 05` of an empty tablet is `None`, measured in `send-04`), `PageStored` (`12 01 09`), `Unknown(frame)`.
  Known codes with a wrong payload are errors, unknown codes are `Unknown`; nothing panics.
- `record`: `Record { status, x, y, pressure }` (little-endian), `RecordStatus::{Touch a1, Hover a0, Out c0,
  Unknown(u8)}`, `Record::to_live()` (the Slate's `LiveRecord`: touch with pressure clamped to 1..=2047, hover with 0,
  `c0` = proximity out, unknown = nothing), `decode_page()` (strokes = runs of `a1`, timed), `encode_live` (≤ 6 records
  per notification) and `split_chunks` (≤ 244 bytes) for the simulator; `check_page` (`PageShape`: whole
  records of a known status, in counts only), the check every page passes before it is kept.
- Timing (records carry no time): `RECORDS_PER_SECOND = 120` and `OUT_OF_RANGE_PAUSE_MS = 500`, see §3.
- Page size: `BOARD_WIDTH = 20 500`, `BOARD_HEIGHT = 27 200` (measured extents rounded, to be calibrated), portrait,
  origin top left, units taken 1:1 as the notes' 10 µm; `BOARD_MAX_PRESSURE = 2047`.
- `beacon::MiBeacon::parse` (frame control, product, counter), `BLACKBOARD_PRODUCT_ID = 0x0fd8`,
  `blackboard_service_data()` (the simulator's advertisement).
- `describe_request`, `describe_reply`, `describe_stream`, `describe_page`: one line each for the log (`BOARD …`,
  `BOARD_DATA …`), never failing.

### ardoise-device

- `transport.rs`: `Channel::BoardData` (`"board_data"` in the log) for `6e400004`; `Advertisement::xiaomi_service_data`
  (`#[serde(default)]`, skipped when empty: older logs and JSON load unchanged); `TabletKind::{Slate, Blackboard}`
  (`"slate"`, `"blackboard"`); `Advertisement::is_blackboard()` (MiBeacon product 4056 only, never the name) and
  `kind()` (Wacom company id first).
- `identity.rs`: `device.json` unchanged (the Slate). `blackboard.json` next to it: `format`, `address`, `name`,
  `protocol` (`"xiaomi-blackboard"`), `model` (`"jiqid.sxb.v1"`), `added` (RFC 3339), unknown fields kept. Atomic
  writes (`write_atomic`). An older Ardoise never reads it and keeps working with the Slate.
- `blackboard.rs`: `BoardSession` over a `Transport`: `set_time`, `battery`, `memory`, `page_count`, `info`,
  `oldest_page`, `download(info)`, `delete(time)`, `sync_page(state, keep)` (the one per-page step of a sync, §2.5),
  `start_live`, `stop_live`, `next_live_event`, `idle`, `is_dirty`, `take_page_stored`, `take_oddities`. Replies are
  sorted from stream bytes as they arrive; stream bytes go to the page being downloaded, else to live mode, else are
  counted as stray. Unknown or malformed frames, stray bytes and notifications of other channels are counted
  (`Oddities`), never an error. A dirty stream (§2.15) refuses downloads and live mode (`StreamDirty`).
- `manager.rs`: `find_known(slate, board, timeout, quiet)` (each known tablet at its saved address, advertising as its
  kind), `find_blackboards(timeout)` (every blackboard heard), `connect_blackboard(address)` (link + clock). The log
  transport describes frames in the protocol of the tablet it connects to (`LoggingTransport::for_tablet`).
- `pages.rs`: `DownloadedPage::kind`; `note_from_page` reads a blackboard page with `blackboard_note` (its own page,
  turned for the position, « Positions, the Rust side », `Timing::Synthesized`); `page_kind(bytes)` recognises kept bytes (`original.bin`) so that "back to the
  original" works for blackboard notes; `BLACKBOARD_PAGE` (the size it writes now), `BLACKBOARD_PAGES` and
  `is_blackboard_page(size)` (every size it ever wrote with, §3).
- `service/worker/board.rs`: the blackboard's half of the worker (search to add, connection, sync, live mode, the
  Slate's priority, the fresh link of a dirty stream); `service.rs`: the status fields and requests of §5;
  `LiveInput::Started` names the tablet (`kind`), and a `LiveSink` that answers `active() == false` right after it
  ends live mode (the link stays).

### ardoise-ble

- `gatt.rs`: `BOARD_DATA` (`6e400004`), `XIAOMI_SERVICE` (`fe95`), `BOARD_SUBSCRIPTIONS` (replies, then records),
  `subscriptions(kind)`, `is_forbidden_service` (DFU and `fe95`: nothing in them is ever used, even a characteristic
  with a Nordic UUID).
- `convert.rs`: the advertisement keeps the service data of `fe95`; `layout_kind(peer, characteristics)` chooses the
  layout from the advertisement's kind, else (an advertisement without service data at connection) from the
  characteristics (Slate's `00001524`, else `6e400004`); `locate_characteristics(characteristics, kind)`.
- `connector.rs`: a scan made while a link is open (looking for the Slate while the blackboard holds the link) is
  *gentle*: Windows' peripheral list is not emptied and not swept, only fresh advertisements count (§8).
- `link.rs`: the layout of the kind; writes only to Nordic UART TX outside the forbidden services (checked again
  before each write). The tablet's ATT MTU request (247) is Windows' business; the download counts bytes whatever
  the chunking.

### ardoise-sim

- `SimBoard` (`board.rs`), its behaviour and the assumptions are listed in its module documentation. Physical actions:
  `draw_live(script)` (the pen writes from now on, on the screen and to a streaming host, at 120 records per second),
  `draw_page(strokes)` (write, store, erase), `press_store()`, `erase()`. Faults: `power_off/on`,
  `disconnect_after`, `disconnect_at_next_request`, `stall_next_download(bytes)` (queued: the rest never comes),
  `delay_next_download(bytes)` (queued: the rest comes just after the host's next download request on the same link,
  lost with the link), `pace_downloads(interval)` (the chunks of a page come that far apart on the virtual clock, about
  10 ms in Mi Home's capture; at once by default), `send_stream(bytes)` (stream bytes now, nobody asking: more of a
  transfer thought finished), `stream_without_asking(on)` (every new link streams the pen without `12 02 08 01`: a
  tablet that kept live mode across a reconnection, not measured), `ignore_next_delete`, `delay_next_delete(after)`,
  `store_raw_page(bytes)` (a page that is not records), `on_next_download(action)` (to land a request in the middle of
  a download), `on_next_delete(action)` (right after the next delete). Notifications leave in the order they are due
  (a reply overtakes the chunks of a paced download still to come). **Supposed**: a transfer ends with its link.
  Inspection: `stored_pages`, `screen`, `stats` (connections, requests, unknown requests, downloads, deletes, live
  records, page-stored events).
- `SimDevice::{Slate, Board}`; `SimConnector::over(devices)`, `RealTimeConnector::over(devices)`,
  `SimFactory::of(devices, real_time)` / `.with_board(board)`; devices must share one virtual clock
  (`SimBoard::with_clock(config, tablet.clock())`).
- `SimHooks::for_board(board)` / `.with_board(board)`: writes 600 s of handwriting when the blackboard's live mode
  starts (`ServiceHooks::blackboard_live_started`). `app_board(clock)`: the app's blackboard (three reference pages).

### app/src-tauri

- `tablet.rs`: `Simulation { tablet: Option<SimTablet>, board: Option<SimBoard>, … }`, the four commands of §5.1.
- `--simulated-blackboard` (alone, or with `--simulated-tablet` for both), §6.
- `blackboard_import.rs`: `--import-blackboard-pages DIR [--library LIB]`, §7.
- `original.rs`: a blackboard note's original comes back on the blackboard's page.
- `live/resume.rs`: `tablet_fits(kind, sheet)` (which tablet may write on a resumed sheet, §2.20), `held_for` (the
  blackboard is never turned to follow a sheet); `live/hub.rs` ends a resumed run the tablet does not fit
  (`refused` event); `live.rs`: `live_resume` refuses at once with a tablet connected that does not fit;
  `live/model.rs`: `LiveSnapshot.tablet`, the tablet the session writes with (§5.5).

## 2. Decisions

Given by the lead session (kept as given): one `Channel` for `6e400004`; recognition by the MiBeacon of `fe95`
(product `0x0fd8`) with serde defaults; GATT layout chosen by the kind; nothing subscribed or written on `fe95`; own
identity file, no registration; both tablets known, the one switched on connected; units 1:1, pressure 0..=2047, page
20 500 × 27 200 portrait; one record rate and a fixed pause after `c0`; `a1`/`a0`/`c0` in live mode, `12 01 09`
not interrupting it; sync loop describe → download (counted, timeout, typed error) → persist → delete → describe;
automatic sync only with deletion on; a one-time import option.

Taken while building:

1. **The host cannot encode an unobserved frame**: `BoardRequest` has only the nine observed variants (no `Unknown`),
   and the session writes nothing else. Unknown requests exist only on the simulator's side, as decode errors.
2. **Record rate 120 per second** (`RECORDS_PER_SECOND`): dense runs of live notifications (gaps under 80 ms) carry
   112 records per second in the PC's capture and 125 in Mi Home's (HCI timestamps, more precise); notifications come
   every 40–45 ms with 5–6 records. Rounded to the research note's "about 120". The pause after `c0` is 500 ms
   (supposed). Both are named constants in `ardoise-proto::blackboard::record`.
3. **A stored page's note**: one record = one period, hovering and unknown records included; `c0` adds the pause;
   strokes are runs of `a1`; unknown statuses are skipped without ending a stroke (never seen). The note is dated by
   the page's time (the tablet's clock, set by Ardoise at each connection), `Timing::Synthesized`, black ballpoint
   0.6 mm like the Slate's pages, one layer named in the interface language.
4. **No duplicate**: a page's note id is the sync's (`page_note_id`: time + bytes), so a page synced again after a
   failed deletion, or imported twice by `--import-blackboard-pages`, is recognised (`ImportedPages`) and not written
   again; its deletion then proceeds.
5. **Deletion guarantee**, one per-page step (`BoardSession::sync_page`, the app's and the tests' only one):
   describe, download, keep, delete. `12 05 06` is written only after the page store returned `Ok` (note written and
   read back, `LibrarySink::store`), with cancellation disabled for that one write (as for the Slate), and only if no
   stream byte came while the page was kept. A page is accepted only after its count, a quiet window and the record
   check (§2.15). A download that stops short (no byte for 5 s), brings more bytes than announced, whose stream did
   not go quiet before it could be asked (§2.15), or after which more bytes come while the page is kept is
   `transferIncomplete`: retried once **on a fresh link**, then the sync stops and the page stays. Every unclean
   transfer of a page uses up that one retry, whichever way it failed, so a tablet that always misbehaves ends the
   sync with `transferIncomplete`, never in a loop of fresh links (second review of M18, R2). A page that is not
   whole records of a known status, or announced larger than the tablet's memory (`11 01 02`; 128 MiB while unknown),
   is `pageUnreadable`: nothing kept, nothing deleted, the sync stops, one log line with counts only. If the tablet
   describes again the page it was just asked to delete, it is described once more after 300 ms (`DELETE_SETTLE`,
   supposed), then the sync stops (`pageNotDeleted`, the page not counted as deleted) rather than importing it again
   and again. At most 1 000 pages per sync (`MAX_SYNC_PAGES`, in the shared step). A page of 0 bytes is kept as an
   empty note and deleted, as before (to look at in H8: whether the tablet ever announces one).
6. **Who may delete the blackboard's pages** (a sync must, to reach the next page, SPEC §4.1). Each sync carries a
   permission: "delete after import" on (checked again whenever the sync starts or resumes), or a request confirmed
   while the blackboard was the connected tablet (`tablet_sync({ confirmBlackboardDelete: true })` with
   `active === 'blackboard'`). Without one, a sync does nothing for the blackboard, without an error: a request made
   while nothing or the Slate is connected, a sync queued behind live mode or the Slate's automatic sync never deletes
   the blackboard's pages when the option is off. A request without the flag while the blackboard is connected and the
   option off is refused (`blackboardConfirmDelete`, nothing done). A sync resumed after a lost link, a fresh link, a
   cancelled step or another library keeps exactly its permission. With the option on, nothing changes: the blackboard
   syncs automatically and on request.
7. **One link at a time.** When both tablets are known, the search accepts whichever known tablet it hears first. While
   the blackboard holds the link and nothing happens (no sync, no live mode), the worker looks for the Slate for 1 s
   every 10 s (`SLATE_CHECK_SCAN`, `SLATE_CHECK_INTERVAL`); a Slate advertising (its button pressed) gets the link:
   the Slate only advertises when it wants to connect, the blackboard is always on. That scan leaves Windows'
   peripheral list alone (`ScanKind::Gentle`, §8). Supposed: Windows scans while a link is open (to check at H8).
8. **Adding**: `tablet_find_blackboards` closes any open link and listens 60 s (`BOARD_SEARCH`) in 1 s slices, publishing
   what it hears (refused with `tabletBusy` during a sync and while live mode runs or waits for a tablet: §2.10);
   `tablet_add_blackboard(address)` accepts only an address heard by the search under way, saves
   `blackboard.json` (replacing a blackboard known before: one at a time) and ends the search; the worker then connects
   to it. The name saved is `Mijia LCD blackboard` (the tablet advertises a generic `xiaomi.dev.ble`).
9. **"Page stored" event**: noted in the log; the page count is read again as soon as nothing else runs (never during
   live mode or a sync), so a page stored during live mode is imported afterwards (a safe duplicate of the live note).
10. **Live mode** on the blackboard: `Started { width: 20 500, height: 27 200, orientation: landscape }` (landscape
    means "not turned" in the hub); orientation changes are ignored while the blackboard holds the link or its live
    mode waits for it; records are never turned (since « Positions, the Rust side »: turned for the run's position). **A live run belongs to the tablet it started on** and never changes
    tablets (its page has that tablet's size and way of holding; second review of M18, R1):
    - after a lost link only that tablet is looked for and accepted, the other one is ignored until live mode ends;
    - the Slate's registration is refused (`tabletBusy`) while live mode runs (as before) or waits for the
      blackboard it was written with; the search for a blackboard to add is refused while any live run exists. A run
      waiting for a Slate, or for any tablet, lets the wizard run, as before the blackboard existed;
    - a run whose tablet is no longer known (its file gone or unreadable: forgotten from another computer through the
      shared library) ends as soon as the other tablet is known, before the other one is searched for: the sink is told
      live mode stopped, nothing shown, as `forget` does here. With no tablet known at all it waits, as before;
    - last guard, not expected to be reached: when the other tablet holds the link, the run ends before anything is
      asked of that tablet (`live_step`, `board_live_step`), nothing of it is written, the sink is told live mode
      stopped and the status shows `sheetOtherTablet` (`DeviceError::LiveOnOtherTablet`); the link stays.

    Live mode asked with nothing connected takes the first tablet that connects. The hub times records 5 ms apart
    inside a notification and follows the wall clock across notifications: with up to 6 records per notification at
    120 per second, the times inside one notification are compressed by at most about 20 ms, which the next
    notification corrects. Not worth a new field in `LiveInput`.
11. **The erase button** (`11 01 0c`, `BoardReply::ScreenErased`; research note §6): `live_input()` (in
    `service/worker/board.rs`) is the one place where a frame of the reply characteristic becomes a live input, and
    it maps this one to `LiveInput::ScreenErased`. The hub goes to a new page (`LiveSession::screen_erased`),
    whatever the setting of the tablet's button (a new layer would put the writing over ink the tablet no longer
    shows), unless the page on screen has no ink yet. Outside live mode the frame is ignored.
12. **Rename and clock**: renaming with the blackboard connected is `notSupported`; `tablet_set_time` sets its clock and
    returns a drift of 0 (the tablet cannot be asked its time).
13. **Battery** is shown capped at 100 (inferred per cent); `charging` is always false, `firmware` empty (the tablet
    has neither).
14. **Logs**: the blackboard's connections are in the same service journal as the Slate's (`_ardoise/diagnostics/`,
    written when something went wrong or something unclassified arrived); every frame is described in the
    blackboard's protocol.
15. **A page has nothing but its count** (no header, end mark or checksum): bytes of an earlier transfer still running
    would join the next page unnoticed, and the wrong page would be kept and deleted (review of M18, C1). Three
    layers:
    - *Dirty stream, fresh link.* A download that does not end cleanly (idle timeout, more bytes than announced,
      cancelled, a page failing the record check, any error but a lost link), or stream bytes arriving after a
      download ended, mark the session dirty. A dirty session refuses downloads and live mode (`StreamDirty`), and the
      worker closes the link and opens a fresh one (`Worker::board_reopen`) before the stream is next needed (a sync
      step, live mode, a sync waiting: at once during a sync; not while nothing waits, §9.4); the sync or live mode
      under way carries on there, with its permission. Live mode that finds late page bytes as it starts
      (`StreamDirty` from `start_live`'s drain, after a sync thought clean) waits the same way: the next step reopens
      the link and live mode starts there; `StreamDirty` never reaches the screen (R3). The reopening is not cut short by
      requests (a request that cannot wait is taken first). Only a fresh link is known to carry nothing of the old
      transfer (supposed: to check at H8, whether the tablet aborts a transfer on disconnection).
    - *Quiet window.* A download is asked only once the stream has been quiet for 300 ms (`QUIET_WINDOW`, supposed;
      page notifications came about every 10 ms in Mi Home's capture: 52 185 bytes in 2.1 s), a fresh link counting
      from its opening; the session waits for that at most 5 s (`DATA_IDLE_TIMEOUT`), and a stream that does not go
      quiet is an unclean transfer (dirty, the page's retry used up, nothing asked: R4). No wait in a normal sync: a
      page ends with a quiet window; on a fresh link, at most what is left of the window after the information was read
      (once per link); after live mode, at most 0.3 s. Once the count reaches the announced size, the stream must stay
      quiet for 300 ms again before the page is returned; any byte more makes it incomplete. About 0.3 s per page.
    - *Record check.* Size a multiple of 7, every byte at an offset multiple of 7 `a0`, `a1` or `c0` (x, y and
      pressure never reach `a0` in their high byte: 244-byte chunks, 244 mod 7 = 6, so most splices are misaligned).
      A rotation of the same page by a multiple of 7 would pass, with every record kept in the wrong order; the fresh
      link is what prevents it.
16. **Cancelled in the middle of a blackboard step** (cancel, another library, forgetting a tablet: the requests that
    interrupt): the sync waits, its stream dirty, and the request decides: cancelling ends it, another library takes
    the pages left, forgetting the Slate leaves it running. Forgetting the Slate while the blackboard holds the link
    (and the blackboard while the Slate does) does not interrupt the worker at all.
17. **Forgetting a tablet** stops a live mode waiting for a tablet, the sync waiting and the pending state, as before
    the blackboard existed, unless the other tablet holds the link: then its sync, live mode and what waits for it stay.
    Forgotten from another computer (its file gone from the shared library): a live run waiting for it ends once the
    other tablet is known (§2.10).
18. **`blackboard.json` unreadable**: no blackboard known (a Slate goes on working), one line in the diagnostic log
    (written once), no error shown, the file left as it is until a blackboard is added (which replaces it).
19. **A page dated before 2020-01-01 UTC** (a clock never set) gives a note dated by the import; its id still comes from
    its time and bytes, so it is recognised all the same (`EARLIEST_BOARD_PAGE`).
20. **Resuming a sheet in live mode** (M12) **with the other tablet.** What the hub did before: it fits the tablet's
    whole area in the sheet at one uniform scale, turned to how the sheet is held ([`hold_for`]). A Slate sheet in
    landscape with the blackboard: shrunk to about 79 % (21 600 / 27 200) in a central band, 67 mm of margin on each
    side; turned A4 with the blackboard: its records turned a quarter (the blackboard cannot be held otherwise in
    Ardoise) and shrunk; a blackboard page with the Slate: the Slate held upright, shrunk to about 92 %. Nothing lost
    or out of the page, but never at the same physical scale, and turned ink for the blackboard. And its own page
    with the blackboard was turned and shrunk too (`hold_for` took its "landscape", which means "not turned", for a
    wide tablet): fixed, the blackboard is held as it is (`held_for`), its records land as they come. **Rule**
    (`tablet_fits`): the blackboard writes only on a page of a size it wrote with (`is_blackboard_page`: today exactly
    `BLACKBOARD_PAGE`, §3), the Slate on every other sheet (as before the blackboard: A5 on A4 and so on, unchanged).
    Any other pair: `live_resume` returns
    `sheetOtherTablet` at once when that tablet is connected; otherwise the run ends when it connects (`refused` event,
    state `idle`), the tablet leaves live mode, nothing is written to the note. A blackboard note turned in the
    editor is a sheet like any other (the Slate's). (Changed by « Positions, the Rust side »: a landscape page of the blackboard is its own.)
21. **Deletion is by time** (`12 05 06 <time>`): two pages stored within the same second are assumed impossible (the
    tablet's clock has seconds; the simulator keeps one second between pages). If the real tablet allowed it, one
    delete could remove both, the second never downloaded: to look at in H8.

## 3. Units and time

- x and y as sent, 1:1 in units of 10 µm (inferred: 13.5" 3:4 panel, about 206 × 274 mm); the page 20 500 × 27 200
  (largest coordinates seen: 20 401 and 27 067). To calibrate with a ruler at H8: the two constants change
  (`BOARD_WIDTH`, `BOARD_HEIGHT`, so `BLACKBOARD_PAGE`), and the new size is **added** to `BLACKBOARD_PAGES`, the old
  ones kept: every check of "is this a blackboard page" (`is_blackboard_page`: who may resume it, §2.20) goes through
  that list, so the notes written before stay blackboard pages. New notes take the new size.
- Pressure 1..=2047 when touching, normalised by 2047 like the Slate's.
- Live mode: each notification stamped on arrival (latency stamp), as for the Slate.

## 4. Tests

- `ardoise-proto` (`blackboard.rs`, `blackboard/record.rs`, `blackboard/beacon.rs`): every request and reply of the
  research note encodes and decodes identically; unknown replies kept, unknown requests refused; malformed frames and
  every prefix never panic; records round-trip; a page becomes timed strokes; beacon recognition.
- `ardoise-device`: `blackboard.rs` (scripted tablet, the per-page step: info and clock, each page deleted after the
  keeper, a page refused is not deleted, the quiet window, a download asked only on a quiet stream (a fresh link
  counting from its opening, at once after a page, nothing asked on a stream that never stops), a stall retried once
  on a fresh link only, bytes after a page make the stream dirty and keep the page on the tablet and use up its retry
  (the second time `transferIncomplete`), an unreadable page, a size beyond the memory, a delete
  described again once, an empty tablet and the 1 000-page cap, a cancelled download, unknown input counted, live mode
  order, a lost link); `identity.rs`, `pages.rs` (a clock never set), `logging.rs`, `transport.rs`,
  `service/tests.rs` (status shapes, error codes).
- `ardoise-ble/src/convert.rs`, `gatt.rs`: recognition, the blackboard's layout never touching `fe95`, the layout choice.
- `ardoise-sim/src/board.rs`: the simulated tablet itself (paced downloads, stream bytes nobody asked for, a pen
  streamed to a new link unasked, the action after a delete).
- `ardoise-sim/tests/blackboard.rs` (service against the simulated blackboard, virtual time): adding it then three dated
  notes; a page deleted only after its note is written and checked (the store refuses the second page: it stays on the
  tablet, the next sync brings it without the first one twice); deletion off: on request only, still deleted; live mode
  gives exactly the touching records written, hovering ones too, and goes on when a page is stored (imported
  afterwards); a link lost during live mode is recovered; both tablets known, the one switched on connects, the Slate
  goes first, forgetting one keeps the other; a search hearing nothing ends with `blackboardNotFound`, a search can be
  cancelled. Through `TabletService`, the app's own sync step: a stall recovered on a fresh link, a second stall
  (`transferIncomplete`, the page stays); the rest of an earlier transfer, late, never joins another page (the review's
  two cases: a rotation by 20 records, a page entirely late); a download cancelled for live mode leaves no phantom
  record (the page paced over time, so that without the fresh link its rest would reach live mode: checked by
  removing the dirty flag, the phantom check then fails); late page bytes found as live mode starts reopen the link
  and live mode starts, no error shown; bytes while a page is kept, after its retry: `transferIncomplete`, its note
  kept, one more fresh link and no loop; a tablet streaming unasked on every link: no download ever asked, no page
  kept, `transferIncomplete` after one fresh link; while a live run waits for its tablet, registering a Slate (run of
  the blackboard) and searching for a blackboard (run of the Slate) are refused; a run whose tablet is forgotten
  elsewhere ends and the other tablet writes nothing, both ways (each of the two guards alone passes it, checked by
  disabling the other); a slow delete waited for, an ignored one (`pageNotDeleted`); an unreadable page (`pageUnreadable`, counts in
  the log); the status after an automatic, explicit, empty and cancelled sync (`connected`, `active: 'blackboard'`,
  `storedPages` going down); a page stored during a failed sync counted at once; live mode keeps its tablet (both
  ways, and the first to connect); who may delete (a request for the Slate, an automatic sync resumed after the option
  was turned off, a confirmed sync whose link drops before its first page, a confirmed sync into another library);
  forgetting a tablet (Slate only as before, blackboard only, the Slate while the blackboard syncs); an unreadable
  `blackboard.json`; a sink refusing the tablet ends live mode, the link kept.
- `app/src-tauri/src/live/resume/tests.rs`: who may write where; a blackboard page resumed with the blackboard takes
  its records exactly as they come whatever the Slate's setting; a Slate sheet (landscape and turned) with the
  blackboard and a blackboard page with the Slate end at once (`refused`, `sheetOtherTablet`, `idle`), the note's file
  byte for byte the same, every size of `BLACKBOARD_PAGES` is the blackboard's; `live/tests.rs`: `live_resume` refused
  at once, `LiveSnapshot.tablet` (`null`, then the tablet, kept while reconnecting); `contract_tests.rs`: the error
  codes of the tablet paths read from the code (`error_code`, `sync_error`, `StatusError::new`, `CommandError::new`,
  the hub's refusal), each with its text in both languages; `LiveSnapshot.tablet` as words.
- `ardoise-sim/tests/blackboard_captures.rs` (private captures, skipped without them): every vendor frame of every
  capture decodes and encodes back (60 requests, 35 replies); every live notification is whole records of a known
  status (904 notifications, 4 062 records); the nine downloads of Mi Home's import are, byte for byte, the kept
  pages, of the announced sizes; every kept page passes the sync's record check and decodes completely, into as many
  strokes and points as the study's renderings, and becomes a note; the page stored during the study holds the records
  received live, byte for byte. Failure messages name captures and pages by rank and print counts only.
- `app/src-tauri`: `tablet.rs` (the commands with the simulated blackboard of `--simulated-blackboard`), 
  `blackboard_import.rs` (dated notes once whatever the number of runs, files failing the record check refused,
  a later store with its `page-<time>.json` updating the earlier note), `lib.rs` (options), `live/tests.rs` (the
  `PagesFirst` refusal becomes the `refused` event `blackboardPagesFirst`, the session idle, nothing written).
- After H8's first measurements (§9): `ardoise-sim/src/board.rs` (the open page described in full and removed once
  downloaded, the count before the description, the observed cases of the erase button, a download cut short leaving
  the open page); `ardoise-device/src/blackboard.rs` (the open page copied before its note and released after, a
  keeper failing: `pageRescued`, nowhere to copy: `pageNotSaved`, a short-form page not copied, more bytes: a
  doubtful copy and the sync going on, a page no longer described on the fresh link: what came copied apart, pushes
  set aside in live mode); `ardoise-device/src/rescue.rs` (copies written, listed, doubted, released, the fallback
  folder); `ardoise-device/src/pages.rs` (`update_tests`: the earlier note updated in place, both pages imported,
  a third store; edited ink, a renamed layer, the trash, a page not holding the earlier one: a new note);
  `ardoise-sim/tests/blackboard.rs` through the service (an open page's copy gone once its note is read back; a
  note that cannot be written: rescued, imported at the next sync, nothing twice; the local fallback and memory; more
  bytes: doubtful, never a note; a stall: whole on the fresh link, its doubtful copy gone; bytes while the note is
  written: the note kept, its copy apart; the count before the description on a link opened by `Manager`; pushes of
  518 and 1 743 bytes during live mode, no phantom; a push after a sync, no reconnection while idle; live mode asked
  with pages, both orders, the pages imported before anything is asked of the stream; without the permission, refused
  at once or at the connection, then started after a confirmed sync; a screen stored again updating its note, and
  the three cases adding one).

Not done: a replay of a blackboard journal (`ardoise-sim::replay` knows the Slate's journals only; no journal of the
real blackboard exists yet), e2e journeys (the interface work).

## 5. The contract for the interface

### 5.1 Commands

Unchanged: `tablet_status`, `tablet_subscribe`, `tablet_set_options`, `tablet_info` (the Slate's `device.json`),
`forget_tablet` (the Slate only), `sim_press_button` (the simulated Slate), the live commands (`live_start`,
`live_stop`, …: live mode starts on whichever tablet is connected or connects first). `tablet_register` (the Slate's
wizard): as before, plus `tabletBusy` (in `status.error`, nothing started) while live mode waits for the blackboard it
was written with (a live session never changes tablets, §2.10); a live session waiting for a Slate, or for any tablet,
still lets it run.

Changed meaning, same signature:

| Command | With the blackboard |
|---|---|
| `tablet_sync({ confirmBlackboardDelete? })` | New optional argument, `boolean`, absent = `false`. Syncs the connected (or next connected) tablet. A sync of the blackboard **always deletes** each page once its note is written. With `options.deleteAfterImport` on: as for the Slate, the flag changes nothing. With it off: while `active === 'blackboard'`, a call without the flag returns the error `blackboardConfirmDelete` and does nothing; ask the user, then call again with `confirmBlackboardDelete: true`. A call while nothing or the Slate is connected never deletes the blackboard's pages (if the blackboard is the tablet that connects, that sync does nothing for it, no error). Rare race: the blackboard connects between the call and the service: the same `blackboardConfirmDelete` comes in `status.error`. |
| `tablet_cancel()` | Also ends the search for a blackboard (`connection` back to `searching` or `notRegistered`, no error). |
| `tablet_rename({ name })` | Error `notSupported` while the blackboard is connected. |
| `tablet_set_time()` | Sets the blackboard's clock; returns `0` (it cannot be read back). |
| `tablet_set_orientation({ orientation })` | Saved; not used while the blackboard is live. |

New:

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `tablet_find_blackboards` | none | `null` | Starts the search: `connection: 'adding'`, `boardSearch` filled as blackboards are heard, for 60 s. Closes any open link. `tabletBusy` (in `status.error`, nothing started) during a sync, and while a live session runs or waits for its tablet (`pending: 'live'`). |
| `tablet_add_blackboard` | `{ address: string }` (one of `boardSearch.found[].address`, any case) | `TabletStatus` | Saves `blackboard.json`, ends the search, then the service connects. Errors: `blackboardNotFound` (not heard by the search under way, or no search), `libraryAccess`, `noLibrary`. |
| `tablet_forget_blackboard` | none | `null` | Disconnects the blackboard if connected and deletes `blackboard.json`; a connected Slate stays connected. With the simulated tablets, only their throwaway folder. |
| `sim_blackboard_store_page` | none | `null` | Presses the simulated blackboard's store button (`--simulated-blackboard` only, else `simulationOnly`). |

Errors are `{ code, message }` as everywhere; `code` selects `errorCodes.*`.

### 5.2 Status (TypeScript, `lib/tablet.ts`)

```ts
export type TabletKind = 'slate' | 'blackboard';

export type Connection = /* as today */ | 'adding'; // looking for a blackboard to add

export interface KnownTablet {
  kind: TabletKind;
  name: string;          // the Slate's name; 'Mijia LCD blackboard' for the blackboard
  address: string;
  added: string;         // RFC 3339: registered (Slate) or added (blackboard)
  width: number;         // device units (10 µm); 20500 × 27200 for the blackboard
  height: number;
  model: string;         // 'Bamboo Slate A4' …, 'Xiaomi Mijia LCD blackboard'
}

export interface FoundTablet { address: string; name: string | null; rssi: number | null }
export interface BoardSearch { found: FoundTablet[]; until: number /* unix ms when it gives up */ }
export interface DeviceMemory { total: number; free: number } // bytes

export interface DeviceDetails {
  kind: TabletKind;                 // new: which tablet these details are about
  /* name, firmware ('' for the blackboard), batteryPercent, charging (false for the blackboard),
     width, height, storedPages, model, readAt: as today */
  memory: DeviceMemory | null;      // new: the blackboard only
}

export interface TabletStatus {
  /* every field of today, unchanged */
  active: TabletKind | null;        // the tablet of a connection: set only while connection is
                                    // 'connecting', 'connected', 'syncing' or 'live'
  known: KnownTablet[];             // every known tablet, the Slate first ([] when none)
  boardSearch: BoardSearch | null;  // while connection is 'adding'
}
```

`registered` keeps its meaning (the Slate of `device.json`, `null` when only the blackboard is known). `device` is what
the last connected tablet said (check `device.kind`); `lastSync` is the last sync, whichever tablet.

### 5.3 The status in each state

| State | `connection` | `active` | `known` | `device` | others |
|---|---|---|---|---|---|
| Nothing known | `notRegistered` | `null` | `[]` | `null` | |
| Searching for blackboards to add | `adding` | `null` | as before | as before | `boardSearch: { found, until }` grows; ends in `notRegistered`/`searching` with `boardSearch: null` (and `error.code: 'blackboardNotFound'` when nothing was heard) |
| Just added | `searching` | `null` | contains `{ kind: 'blackboard' }` | | `boardSearch: null` |
| Waiting for a known tablet | `searching` | `null` | 1 or 2 entries | last one | |
| Connecting to the blackboard | `connecting` | `'blackboard'` | | | |
| Connected | `connected` | `'blackboard'` | | `{ kind: 'blackboard', batteryPercent, storedPages, memory: { total, free }, firmware: '', charging: false, model: 'Xiaomi Mijia LCD blackboard', width: 20500, height: 27200 }` | `pending: 'sync'` when a sync waits |
| Syncing | `syncing` | `'blackboard'` | | `storedPages` = pages left, going down page by page | `sync: { current, total, saved }`, then `lastSync: { saved, deleted, error }` and back to `connected` with `active: 'blackboard'` (also after a sync of 0 pages, a failed or a cancelled one) |
| Live | `live` | `'blackboard'` | | | live window as for the Slate |
| Link lost | `searching` | `null` | | kept | live mode shows "reconnecting" |

### 5.4 Error codes (new ones need `errorCodes.*` in `fr.json` and `en.json`)

- `blackboardNotFound` (new): the search heard no blackboard in 60 s (Mi Home may hold it, or it is off), or the
  address given was not heard.
- `transferIncomplete` (new): a page download stopped before its announced size (twice); the page stays on the tablet.
- `pageNotDeleted` (new): the tablet still holds the page Ardoise deleted; the sync stopped (the page's note is in the
  library).
- `notSupported` (new): the connected tablet cannot do it (renaming the blackboard).
- `pageUnreadable` (new): a page the blackboard sent is not whole pen records (or was announced larger than its
  memory); nothing was kept or deleted, the page stays on the tablet, the sync stopped. Usually a transfer problem:
  syncing again on a fresh connection may work.
- `blackboardConfirmDelete` (new): returned by `tablet_sync` without `confirmBlackboardDelete` while the blackboard is
  connected and "delete after import" is off; nothing was done. Not a failure to show as such: the cue to ask.
- `sheetOtherTablet` (M18, already translated): `live_resume` refused, the `refused` live event (§5.5), and, in
  `status.error`, a live session that met the other tablet (§5.5, not expected).
- `blackboardPagesFirst` (H8, §9.5): `live_start` and `live_resume` refused at once, or the `refused` live event, while
  the blackboard holds stored pages and the automatic sync or "delete after import" is off. The window answers it with
  the "pages first" dialog, never with this text in a banner. The text has `{count}` (and a singular,
  `blackboardPagesFirstOne`): `pagesFirstText(count)` builds it, and `errorMessage` takes the count from the service's
  message (`the Xiaomi tablet holds N stored pages…`) or shows that message as it is.
- `pageRescued` (H8, §9.2): the note of a page the tablet no longer holds could not be written; its rescue copy waits
  for the next sync (`lastSync.error`, `status.error`).
- `pageDoubtful` (H8, §9.2): a page the tablet may no longer hold was not received cleanly; what came is kept apart,
  never made into a note. Can end a sync that read every page.
- `pageNotSaved` (H8, §9.2): such a page could be written nowhere; Ardoise holds it in memory until the next sync.
- `sheetBoardPosition` (H8, « Positions, the Rust side »): `live_resume` refused at once, or the `refused` live event, for a page of the
  blackboard held the other way round than the blackboard's position (a landscape page, the blackboard set upright, or
  the reverse); nothing written on the note.
- Already translated, also used: `tabletTimeout`, `tabletNotConnected`, `tabletBusy`, `tabletNotRegistered` (no tablet
  known at all), `noLibrary`, `libraryAccess`, `bluetoothOff`, `bluetoothMissing`, `tablet` (an unexpected failure;
  the internal "dirty stream" state never reaches it), `simulationOnly`.

### 5.5 Live mode

Same commands and events as for the Slate. The live page is portrait (20 500 × 27 200; landscape in a landscape position, « Positions, the Rust side »). Hovering records drive the
laser pointer. Live mode resumes only on the tablet it started on (the other one is not connected while it waits).

`LiveSnapshot` (`lib/live.ts`) has one more field, `tablet: 'slate' | 'blackboard' | null`: the tablet this live session
writes with, set when its tablet first connects, kept while it reconnects (`state: 'reconnecting'`, `active` then
`null` in the tablet status), `null` before any tablet connected (and in a session refused at once, below). Use it,
not `tablet.active`, to show or hide what is the Slate's only (the position switch, the hint about its button). It is
not sent to the share's viewers.

A live session never changes tablets. What the interface receives when one cannot go on:

- Its tablet is forgotten from another computer (its file gone from the shared library) while it waits for it and the
  other tablet is known: the session ends as after `live_stop` (the `idle` snapshot, its notes saved), with no
  error, as when the tablet is forgotten here; `status.known` loses that tablet.
- The other tablet holds the link (not expected: the registration and the search wait, see §5.1): the same end, and
  `status.error.code === 'sheetOtherTablet'`; the other tablet stays connected, nothing of it was written (what the
  first tablet wrote is saved). Not the `refused` event below: that one is for a resumed sheet that never started,
  and opens the start banner (try again, back to the library).

Resuming a note (`live_resume`, M12): the blackboard writes only on a blackboard page (20 500 × 27 200: its synced
pages and its live notes), the Slate on every other note, never on a blackboard page (§2.20).

- The tablet that does not fit is connected when `live_resume({ path })` is called: the command fails with
  `{ code: 'sheetOtherTablet', message }`, nothing started, the session shown before left as it is.
- It connects later (nothing was connected, or both tablets are known and the other one came first): the session
  ends at once. The live channel gets `{ type: 'refused', code: 'sheetOtherTablet', message }` and a snapshot with
  `state: 'idle'`; the note is not written; the tablet leaves live mode and stays connected (`connection` back to
  `connected`). Say that this note can only be resumed with the other tablet (the blackboard's notes with the
  blackboard, the others with the Slate), then close the live view as after `live_stop` (calling `live_stop` is
  harmless). Storing a page with the tablet's button does not interrupt live mode. The tablet's erase button makes a new
page (a snapshot follows, as after `live_command('newPage')`), unless the page has no ink yet; "next page" on the
computer stays. The orientation switch has no effect on the blackboard.

### 5.6 What to show and hide for the blackboard (`device.kind === 'blackboard'`, `known[].kind`)

- "Add a tablet" offers the blackboard: `tablet_find_blackboards`, list `boardSearch.found` (one is the usual case;
  `rssi` helps choose), `tablet_add_blackboard`. **No** "hold the button" animation, no registration warning: it stays
  bound to Mi Home. Say that Mi Home must be closed (or the phone's Bluetooth off) while adding and using it.
- Show: battery, free memory (`memory.free` of `memory.total`), pages stored, model. The clock is set at each connection:
  no button needed.
- Hide: rename, the Slate's orientation and button, firmware, "charging". Show its own four positions
  (`boardPosition`, §10).
- Sync: with `options.deleteAfterImport === false` and `active === 'blackboard'`, `tablet_sync()` answers
  `blackboardConfirmDelete`: say that the pages will be deleted from the blackboard (it only gives its oldest page)
  and, if the user agrees, call `tablet_sync({ confirmBlackboardDelete: true })`. The automatic sync does nothing for
  it then, nor does a sync asked while it is not connected.
- Forget: one "forget" per known tablet (`forget_tablet` for the Slate, `tablet_forget_blackboard` for the blackboard).
- Resume in live mode: a blackboard note with the blackboard only, any other note with the Slate only (§5.5).
- Limits for the guide (SPEC §4.1): one application at a time; pages go to whichever application imports them first;
  replay and video of a synced page run at a regular pace (no time per point); the page is about 20 × 27 cm, portrait.

## 6. The simulated blackboard in the app and the journeys

- `ardoise.exe --simulated-blackboard`: the simulated blackboard (real time, battery 90 %, three reference pages, not
  known yet) replaces Bluetooth; with `--simulated-tablet` too, both tablets are in range (one clock). Its file is
  written in the simulation's throwaway folder (`%TEMP%\ardoise-simulation-*`), never in the library.
- A journey: launch with `args: ['--simulated-blackboard']`; `tablet_find_blackboards`; wait for
  `status.boardSearch.found.length > 0` (about a second); `tablet_add_blackboard({ address: found[0].address })`; wait
  for `status.lastSync` (three notes, `storedPages` 0); live mode as with the Slate (the simulated hand writes 600 s
  when live mode starts); `sim_blackboard_store_page` stores the screen (a page appears, live mode goes on).

## 7. `--import-blackboard-pages DIR [--library LIB]`

Turns every `page-<unix time>.bin` of DIR (the raw pages as the tablet sends them, e.g. the nine of
`testdata/private/xiaomi/pages/`) into a dated note of LIB, else of this PC's library, through the sync's own store
(`tablet::store_page_in`: atomic write, read back, thumbnail, automatic export); never overwrites. Running it twice
imports nothing the second time (note id from time and bytes, `ImportedPages`; a note in the trash counts). Other files
are ignored, a `.bin` that is not whole records of a known status (the sync's own check) is reported. Prints the notes written and a summary in French; exit 0
when nothing failed.

## 8. Not done, not measured, risks

- First sessions with the real tablet on 2026-11-13 (research note §6): the download and the deletion work from the
  PC; the erase button sends `11 01 0c` in live mode; a page stored while the application is connected outside live
  mode is described in a longer form (`12 0d 05`, `PageInfo::extra`), which the sync took for a malformed frame.
  The long form is now read. Measured since with the user at the tablet (research note §6): that form is the page
  of a screen not erased yet; **the tablet removes such a page as soon as it is downloaded**, so the sync never
  drops its bytes (rescue copies, §9.2); the erase button can push the page on the stream during live mode (set
  aside, §9.4) and can lose a page not fetched before live mode (fetched first or live mode refused, §9.5); a screen
  stored again updates its note (§9.6). `docs/questions.md` Q16. Since the store button can
  replace a page without changing the count, the "page stored" event alone starts the automatic sync when a page
  is there. Still to measure at H8: the unit (ruler), whether Windows delivers 244-byte notifications whole (the
  count does not depend on it), whether the blackboard drops a silent link (the link was lost twice during live
  mode, cause unknown).
- To check at H8 for the page transfers (§2.15): whether the tablet ends a transfer when the link closes (the fresh
  link relies on it), whether it ever pauses more than 300 ms inside a page (the quiet window would then refuse good
  pages after their count, as `transferIncomplete` twice: nothing lost), how long a delete takes (`DELETE_SETTLE`,
  300 ms), whether it ever announces a page of 0 bytes (kept as an empty note and deleted today), whether two pages
  can share a second (deletion is by time), whether it keeps live mode across a reconnection (§2.15: then no
  download is asked while it streams). Not covered by the quiet window: a tablet that ignores a download request while
  it streams, and whose pen paused more than 300 ms just before the request; its pen records could then be counted as
  the page. A page that reaches exactly its announced size that way and passes the record check would be kept and
  the real page deleted unread. Only measuring whether the tablet keeps streaming after a reconnection can close it.
- The Slate's priority scans for 1 s every 10 s while the blackboard's link is open. Emptying Windows' peripheral list
  then (what every other scan does, btleplug #405) might drop the open link's peripheral or its disconnection events;
  so that scan is gentle: the list is neither emptied nor swept (a swept list still holds a Slate that stopped
  advertising, which would pass for one asking to connect), only advertisements received during the scan count. The
  open link's own task polls its peripheral's state, independent of that list. Not provable without the hardware: at
  H8, check that the blackboard's link survives the checks (live mode and syncs over several minutes with the Slate
  known), that its loss is still noticed at once, and that a Slate pressing its button is still found.
- btleplug's WinRT backend may drop notifications when our task falls behind (device-stack.md): a page download then
  ends short and fails as `transferIncomplete` (nothing deleted), never silently.
- A firmware update may change the protocol: unknown frames are logged and ignored, never answered.

## 9. Open pages, rescue copies, live mode (after H8's first measurements, 2026-11-13)

Source: research note §6 (measured), `docs/questions.md` Q16, SPEC §4.1 and §4.2 as amended on 2026-11-13. What
is not measured is written **supposed**, with the side chosen so that no page can be lost.

### 9.1 The measured model, and the simulator

An *open page* is the page of a screen not erased since it was stored. The tablet describes it in the long form,
`12 0d 05 <time> <size> <previous>` (`PageInfo::extra`, `PageInfo::is_open`), `previous` being the time of the store
before it of the same screen (0 for the first; `PageInfo::previous` gives `None` then). **The tablet removes an open
page by itself once all its bytes have been sent for a download**; a page in the short form stays until deleted.
Storing again replaces the open page (or, when the host already took it, adds one) by one holding the whole screen.
`12 01 05` asked before any `12 01 04` on a link answers the bare frame. The erase button closes the open page
(short form, kept); outside live mode it sends no frame, in live mode `11 01 0c`.

`SimBoard` (`ardoise-sim/src/board.rs`) does exactly that, and reproduces the observed cases of the erase button as
cases, not as a rule:

| Screen erased | The open page was | Observed | The simulator |
|---|---|---|---|
| outside live mode | described since it was stored | pushed, 917 bytes | pushes it (chunks of 244 bytes at most), keeps it closed |
| outside live mode | not described | closed, kept | the same |
| in live mode | described during this live run | `11 01 0c`, then pushed (1 694 bytes) | the same |
| in live mode | stored during this run, not described | `11 01 0c`, nothing pushed | keeps it closed |
| in live mode | stored and described before this run | lost, twice | removes it |
| in live mode | stored before this run, not described | **not measured** | **removes it** (supposed: the losing side, the one the host must guard against) |

Also **supposed** in the simulator: a download cut short (`stall_next_download`, a link lost before the last chunk)
leaves an open page on the tablet (whether the real one removes it is not measured; the host does not rely on it,
§9.2); a page pushed stays, closed; switching the tablet off changes nothing to an open page.

### 9.2 A page the tablet no longer holds is never dropped

The host cannot rely on a page staying on the tablet when it was described in the long form. `BoardSession::sync_page`
(still the one place where a page is described, downloaded, kept and deleted) talks to a `PageKeeper` (`keep`,
`rescue`, `doubt`, `release`), which the worker implements over a `RescueStore` (`ardoise-device/src/rescue.rs`).

- **Where.** `<library>/_ardoise/rescued-pages/page-<time>.bin` (the raw bytes as the tablet sent them, the files
  `--import-blackboard-pages` reads), with `page-<time>.json` beside it when the page had a `previous`
  (`{"previous": T}`). When that folder cannot be written: `%LOCALAPPDATA%\Ardoise\blackboard-rescue\library-<hash>\`
  (`ServiceConfig::rescue_fallback`), a subfolder per library (a 64-bit FNV-1a hash of its root, lower case, `/`
  separators: never its path in clear), so that a copy is only ever imported into the library it was taken for. Every
  write is atomic. Doubtful copies go to a `doubtful\` subfolder of either.
- **Clean copy, written before anything can fail.** A clean transfer of an open page (count reached, quiet window,
  record check) is copied before its note is written. Once the note is written and read back (and no stream byte
  came meanwhile) the copy is removed. If the note cannot be written, the copy stays and the sync stops with
  `pageRescued` (the page is safe and comes at the next sync).
- **Doubtful copy, kept apart.** The bytes of an open page whose transfer did not end cleanly (stopped short, more
  bytes than announced, not whole records, cut by a cancel or a lost link) are copied to `doubtful\` at once. The same
  for any page (short form too) when the fresh link of its retry no longer describes it: what was received is all
  there is. An open page whose note was written but after which more stream bytes came keeps its note, and its clean
  copy moves to `doubtful\`. Doubtful copies are **never** made into notes on their own; the sync tells it with
  `pageDoubtful` (at its end, when nothing else stopped it). A doubtful copy of page T is removed as soon as a clean
  note of the same page T is written (its retry came whole). Of two doubtful copies of one page, the one with more
  bytes is kept.
- **Nowhere to write.** If neither folder can be written, the bytes stay in the worker's memory, are written out at
  the next sync, and the sync stops with `pageNotSaved` (Ardoise must stay open until then).
- **Imported at the next sync, first.** A blackboard sync (with its usual permission) first writes out what is held,
  then imports every clean copy, oldest first, through the same path as a page from the tablet (`keep_page`, the
  app's page store: atomic note, read back, `ImportedPages` against duplicates), removing each copy once its note is
  read back, or when its note is in the trash (the user deleted it); then the tablet's pages. A copy, or a page held,
  that cannot be written stays as it is and stops nothing: the other copies and the tablet's pages come, and the
  first such failure is said at the end of the sync when nothing else is (`pageRescued`, `pageNotSaved`;
  `SyncRun::deferred`). A clean copy that is not whole records moves to `doubtful\`. The automatic sync at
  connection also starts when copies wait.
- **Short-form pages** are not copied before their note: they stay until deleted (measured). The explicit delete
  `12 05 06` after a page is kept stays for every page (harmless when the tablet already removed it).

### 9.3 `12 01 05` after `12 01 04`

`BoardSession::oldest_page` asks the count first when it was not asked yet on this link (research note §6).

### 9.4 A page pushed during live mode is not ink; outside live mode it costs nothing

- In live mode, a stream notification of more than 6 records (42 bytes, `MAX_LIVE_RECORDS`) is a page the erase button
  made the tablet send: it starts a *push*, never given to the live sink. The push goes on while notifications are
  full chunks (244 bytes), while the bytes pushed so far are not whole records (a page always is), or while they come
  within `PUSH_GAP` of the push's previous one; its first notification shorter than a full chunk is its last. The
  log says how many bytes were set aside. Limit: a page of 6 records or fewer pushed alone passes for pen records.
- Time does not tell a page from the pen: on the PC's own sessions (research note §6) the chunks of a page and the
  pen's notifications both came a few tens of milliseconds apart, with bursts of about a millisecond (the earlier
  figures of §2.2, from Mi Home's capture, do not hold on the PC). So the size rule does the work, and time only
  closes a push that ends on a full chunk: `PUSH_GAP` is 100 ms (**supposed**: above the gaps seen inside a page).
  The trade-off: a push that ends on a full chunk of whole records (no short last chunk) swallows the pen's next
  notification if it comes within 100 ms (6 records at most, about 50 ms of ink); the opposite choice (25 ms at
  first) let up to 6 records of the pushed page become ink when its short last chunk came late, which is worse.
- Outside live mode, the quiet window already keeps a push out of a later download, and a push before any download on
  the link is only counted. What was wrong: after a sync, the session takes any stream byte for "more of the last
  page" (dirty: rightly, the next page or live mode must not take them), and the worker reopened the link at once,
  even with nothing waiting: a reconnection for nothing. Now a dirty stream is opened again only before it is needed
  (a sync step, live mode, a sync waiting: `Worker::board_step`); meanwhile the link idles, counts its stray bytes
  and still answers the count and the Slate's checks. A sync, a retry or live mode never runs on a dirty stream, as
  before. (Clearing the "after a download" state at the end of a sync was tried and dropped: late bytes of the last
  page arriving between the sync and live mode would then not reopen the link, and live mode would take them.)

### 9.5 Pages fetched before live mode (SPEC §4.2)

When live mode is asked (a new run, not one reconnecting) and the blackboard holds stored pages (counted again then):

- automatic sync **and** delete after import on: the sync runs first (the status shows `syncing`, the sink
  `waiting`). Once no sync runs or waits, the pages are counted again: none left, live mode starts; pages left (the
  sync failed, could not run, or was cancelled) end the run as below (`PagesFirst`), the sync's error staying in
  `lastSync`. A pre-live sync cut by a lost link resumes at the reconnection, still before live mode (`PreLive`:
  the run counts as checked only once the tablet holds no page);
- otherwise live mode does not start. `TabletService::live_pages_first()` answers `BlackboardPagesFirst { pages }`
  (`blackboardPagesFirst`) at once while the blackboard is connected, so `live_start` and `live_resume` fail before
  any session starts; a run that was waiting when the blackboard connects ends with `LiveInput::PagesFirst { pages }`,
  which the hub turns into the live event `{ type: 'refused', code: 'blackboardPagesFirst', message }` and the `idle`
  snapshot. Nothing is asked of the tablet's stream. A confirmed sync then imports the pages, and live mode can be
  asked again. With no stored page, or with the Slate, nothing changes. A run that reconnects after a lost link is not
  "asked": it starts again at once (pages stored during it are a copy of its own ink).

For the interface (the "pages first" dialog, H8; `lib/live-pages-first.svelte.ts` and `components/live/PagesFirstDialog.svelte`).
The live view is the one place that starts live mode (the tablet panel's « Live mode », a card's « Resume in live
mode » and the editor's all open it, `live_start` or `live_resume` is called there), so it is the one place that
meets the refusal:

1. `live_start()` (or `live_resume({ path })`) rejects with `{ code: 'blackboardPagesFirst', message }`, or the live
   channel brings `{ type: 'refused', code: 'blackboardPagesFirst', message }` (then the `idle` snapshot): the view
   shows a modal dialog (no start banner) with the sentence for the count, one form for 1 page and one for several
   (`pagesFirstText`: `errorCodes.blackboardPagesFirstOne`, or `errorCodes.blackboardPagesFirst` with `{count}`), and two
   buttons, **Import and delete** (first, focused) and **Cancel**. The count is the one in the service's message
   (`the Xiaomi tablet holds N stored pages…`, counted when it refused), else `tabletStatus.device.storedPages`.
   `errorMessage()` gives the same sentence for this code (and the service's message when it holds no count), so
   `{count}` is never shown anywhere. Escape closes the dialog as Cancel; the focus goes back to where it was.
2. **Import**: `tablet_sync({ confirmBlackboardDelete: true })`, once (the button is disabled meanwhile), then the
   dialog follows the tablet status: `connection: 'syncing'` with its progress, then a new `lastSync` (another `at`).
   If `lastSync.error` is set, it shows it in the dialog and stops there (import again, or cancel). If the link is lost
   (the status is no longer `connected` or `connecting` before a new `lastSync`) or the service fails meanwhile
   (`status.error`, a different one from before), it says so the same way. If `device.storedPages` is 0, it closes
   and the view calls `live_start()` (or `live_resume` with the same path) again; should the service refuse again,
   the dialog shows with the new count. If pages are left, it shows again with the new count.
3. **Cancel**: before the import, the dialog closes and the view closes as after `live_stop` (a `refused` event already
   left the session `idle`; `live_start` rejected started nothing). The library gives the tablet panel back, the focus
   on its « Live mode » button, when live mode was asked from there. During the import, Cancel stops the sync between
   two pages (`tablet_cancel`), then does the same.

The mock tablet (`mock-tablet.ts`) refuses live mode in the same conditions (the blackboard is the connected tablet,
it holds stored pages, and the automatic sync or "delete after import" is off): `live_start` and `live_resume` reject
(`live_resume` after the sheet check, as in Rust), a run that waits when the blackboard connects ends with the
`refused` event, and with both options on the pages are synced first. `mockTablet.boardPages` sets how many pages it
holds when first looked at (3 by default).

### 9.6 A screen stored again updates its note (SPEC §4.1)

A page arriving with `previous` (long form, `previous != 0`) replaces the page of time `previous`. The page store
(`LibrarySink::store_page`: the sync, the rescue import and `--import-blackboard-pages` all go through it) updates
the note made from that page instead of adding one when all of this holds; otherwise it adds a note, and the log line
of the page says why:

- **Exactly one note of the library is made from the page of time `previous`**: among the notes created at or before
  `previous` and modified since `previous` minus a day (`CANDIDATE_MARGIN`, the listing only), those whose id is
  `page_note_id(previous, original.bin)`. None (deleted, in the trash, never imported, a clock never set) or two (a
  sync client's conflict copy): a new note.
- **Not edited since its import**: the note is made again from its own `original.bin` (`blackboard_note`, its layer
  named as the store names it now) and must be identical, after the format's own rounding, in its ink, layers, paper,
  background and image, timing and origin, with no narration, background file, text member, revision or
  `original_note`. Title, tags, favourite mark and binder (and the thumbnail, `modified`) do not count. A note
  imported in the other interface language counts as edited (its layer has the other name): a new note, the safe
  side. A transcription corrected by the assistant (`set_note_text`) lives in the handwriting cache, not in the note:
  it is not seen, and stops applying once the ink changes (it was for the old ink).
- **The new page holds it whole**: the kept bytes of the earlier page are a prefix of the new page's.

The update is one checked atomic write of the same file (`Library::save` with the stamp read: a file changed meanwhile
is never overwritten; a new note is added instead), read back like a new note (a failed read-back is an error, never a
deletion: the rescue copy of an open page stays). Kept: path, title, tags, favourite, binder, creation date, unknown
fields and members. Replaced: ink, layers, page, timing, `original.bin` (the new page), thumbnail; `modified` is now.
**No copy of the version before**: the new page holds it whole (checked), so the ink and "back to the original" give
it back.

**The note's id becomes the new page's** (`page_note_id`, as for any synced note): the id of a synced note stays the
id of the page it holds, which lets `ImportedPages` recognise the new page exactly (a sync or a rescue import run
again imports nothing) and lets a third store of the same screen find this note. What else refers to a note by id:
the handwriting cache is keyed by id and ink, and the ink changed anyway; a draft of the editor holds the path and the
stamp it started from, and a changed file is restored as a copy anyway; the assistant names notes by id or path and
lists again. The earlier page still counts as imported: `ImportedPages::find` also accepts, for a blackboard page, a
note created at that page's time whose `original.bin` starts with the page's bytes (a rescue copy of it found later
does not come back), and `ImportedPages::in_trash` the same in the trash (once the user trashed the updated note,
neither page comes back). A revision an assistant made of the note before the update keeps `revision.of` = the old
id: it still names the note it was made from, which now has another id. Not covered: a page of a clock never set (its note is dated by the import), and the middle page
of three stores found again later (a duplicate note, nothing lost).
## 10. The four positions of the blackboard: the interface (Q17; the Rust side is §11)

The user may hold the blackboard in any of four positions, and Ardoise turns what it writes so that the page is upright
(SPEC §4.1; `docs/questions.md` Q17). The interface (`TabletOptions.svelte`, `BoardDrawing.svelte`, `lib/board-position.ts`,
the mock in `lib/mock-tablet.ts`) is done; this section is the contract for the Rust side.

| | |
|---|---|
| Name | `boardPosition` in the options the interface sends and reads (`tablet_set_options({ options })`, `TabletStatus.options`); `TabletSettings::board_position` on the Rust side, stored next to `orientation` in `[tablet]` of `ardoise.toml` |
| Values (strings, as `Orientation`'s) | `"buttonsBottom"` (default: held upright, the erase and store buttons on the bottom band), `"buttonsTop"` (half a turn), `"buttonsRight"` (the bottom edge is on the right), `"buttonsLeft"` (the bottom edge is on the left) |
| Default | `buttonsBottom`; any other value loads as the default; an `ardoise.toml` without the key loads as the default (older files, older Ardoise) |
| Independent of | `orientation`, which stays the Slate's: a Slate and a blackboard known together each keep their own position. `autoSync` and `deleteAfterImport` stay shared |

- **`TabletOptions`** (`ardoise-device::service`, `tablet.ts`): add `board_position: BoardPosition` (`boardPosition` on the
  wire, `rename_all = "camelCase"`), kept by `tablet_set_options_in` with the other fields, and in `From<&TabletSettings>`.
  The interface already sends it; until the Rust side has the field serde drops it and the status answers without it
  (the interface then reads `buttonsBottom`, `boardPositionOf` in `lib/board-position.ts`), so a position chosen today
  does not stay.
- **Where it is read**: the pages of the blackboard, imported by the sync (`pages::` for blackboard notes) and the live
  page of the blackboard, so that both come out upright. The page of a blackboard note is portrait, 20 500 × 27 200 in
  the tablet's own frame; held with the buttons at the right or at the left, the pen writes in that frame turned a
  quarter turn, and the note is turned back (the page of a note held that way is landscape: `pageSizeFor`-like swap,
  as for the Slate's `portraitLeft` / `portraitRight`). Which way each quarter turn goes has to be measured with the
  real blackboard (H8): in the pictures, `buttonsLeft` is the upright picture turned a quarter turn clockwise,
  `buttonsRight` a quarter turn anticlockwise.
- **Live window**: it keeps today's behaviour for the blackboard (no position switch, portrait page, the Xiaomi
  note `live.xiaomi.label`) until the Rust side applies the position to the live page; then
  `LiveView.svelte` shows the same switch as for the Slate (`data-slot="orientation"`, four buttons) with the
  blackboard's own four values, through a command like `tablet_set_orientation` (`tablet_set_board_position`, to be
  added with the field). Not done here.
- **Resuming** a note in live mode (§5.5) must compare the note's page with the position the way it does for the
  Slate (the `live.resume.hold` notice), once the position is applied.
- **Tests to add on the Rust side**: the default and an unknown value in `settings.rs`; `tablet_set_options` keeping the
  field; a blackboard page imported in each position (golden strokes turned back); the file without the key.

## 11. Positions, the Rust side (Q17)

Source: SPEC §4.1 as amended on 2026-11-13 (« Comme la Bamboo, elle se tient dans quatre positions… »),
`docs/questions.md` Q17, the interface's contract (`app/ui/src/lib/board-position.ts`, `tablet.ts`, `mock-tablet.ts` and
its section "The four positions of the blackboard", from the interface's branch). Named, not numbered: the
sections of the interface's branch and of this one are numbered at their merge.

### Positions: the setting

| | |
|---|---|
| Rust | `ardoise_core::settings::BoardPosition` (`ButtonsBottom`, the default, `ButtonsTop`, `ButtonsRight`, `ButtonsLeft`), `TabletSettings::board_position`, `TabletOptions::board_position`, `PageContext::board_position` |
| `ardoise.toml` | `[tablet] board_position = "buttonsLeft"`: the key in snake_case like every key of the file (`auto_sync`, `delete_after_import`), the value one of the four words of the wire. Absent (older files) or unknown (a newer version): `buttonsBottom`. An older Ardoise keeps the key verbatim (its unknown keys), so a file shared by two PCs keeps it |
| Wire | `boardPosition` in `TabletOptions` (`tablet_status`, the status events, `tablet_set_options`), next to the Slate's `orientation`, independent of it. Strict: an unknown word, or options without the field, are refused when the command's arguments are read (`errorCodes.unexpected`), as the interface's stand-in refuses them (`invalid options`). `tablet_set_options` saves it with the other fields |
| Not added | a quick switch for the live window (`tablet_set_board_position`): a blackboard run keeps the position it started with (below, live mode) |

### Positions: the convention, one function

The blackboard's own frame: portrait, 20 500 × 27 200, origin at the top left of the screen of the tablet held upright,
x to the right, y downwards (measured: research note §3, the diagonal drawn from the top left corner), its two buttons
on the band under the screen (the interface's pictures: erase at its left end, store at its right end).

The positions are named after where the buttons end up for the user, so each one is a rotation of the tablet from
upright: `buttonsLeft` is the tablet turned a quarter turn **clockwise** (its bottom edge moves to the left),
`buttonsRight` a quarter turn anticlockwise, `buttonsTop` half a turn. What the pen writes in the tablet's frame is
then seen by the user turned the same way, so its points are turned the same way to be upright on the page:

| Position | Clockwise quarter turns (`BoardPosition::quarter_turns`, = `drawingTurns`) | Point `(x, y)` → page | Page |
|---|---|---|---|
| `buttonsBottom` | 0 | `(x, y)` | 20 500 × 27 200 |
| `buttonsLeft` | 1 | `(H − y, x)` | 27 200 × 20 500 |
| `buttonsTop` | 2 | `(W − x, H − y)` | 20 500 × 27 200 |
| `buttonsRight` | 3 | `(y, W − x)` | 27 200 × 20 500 |

(`W` = 20 500, `H` = 27 200.) Check by a corner: held with the buttons on the left, the tablet's bottom left corner
(the erase button's end of the band) is at the top left for the user, and `(0, H)` → `(0, 0)`. The numbers are those
of the Slate's positions for the same turn (`Orientation::quarter_turns`, `pages::orient`: `portraitLeft` is also its
button on the left after a quarter turn clockwise), and those of the interface's pictures (`drawingTurns`: the upright
picture turned that many quarter turns clockwise). One function turns every point of the blackboard:
`pages::orient_board(x, y, position)` (with `orient_board_record` for a live record and `board_page(position)` for the
page), on the same arithmetic as the Slate's `orient` (`turn_area_point`). A coordinate beyond the area gives 0, never a
negative one.

Measured: the frame (origin, axes). **Supposed**, to check at H8 (« C quinquies » of `docs/hardware-tests.md`, an
arrow pointing up written in each position, stored and live): that "upright" in the research note is the tablet with
its buttons at the bottom, as the interface draws it. If the real tablet were the other way, only the frame changes:
`orient_board` and its tests, nothing else.

### Positions: stored pages

Where the turn happens: **when the note is made** (`pages::blackboard_note(page, id, layer, now, position)`, through
`note_from_page`, `LibrarySink::with_board_position`), the only place where a note is made from the tablet's bytes, for
the sync, the rescue copies and `--import-blackboard-pages` alike. Not in the session, the worker or the rescue store:
`original.bin`, the rescue copies and `page_note_id` stay the tablet's raw bytes, which the "screen stored again" rule
compares as prefixes (§9.6) and from which the note's id comes. The note's page is `board_page(position)` (landscape for
`buttonsLeft` and `buttonsRight`).

- **The position in force**: the sync takes `options.board_position` when it starts (`SyncRun::context`, like the
  Slate's orientation), for all its pages, the rescue copies imported first included; `--import-blackboard-pages` takes
  the settings' (and prints it).
- **Recorded in the note**: `note.json` gets the top-level key `boardPosition` (`pages::BOARD_POSITION_FIELD`), the
  word of the position its `original.bin` was turned for. The Slate's notes record nothing (their "back to the
  original" uses today's orientation, `original.rs`). A top-level key the format does not know (`docs/file-format.md`):
  every reader keeps it as it is, an older Ardoise included, and it stays true whatever is edited, since `original.bin`
  never changes (only an update of §9.6 replaces both together). Not inside `origin` (a new key inside a known object is
  a major version of the format). `pages::board_position_of(file)`: absent = `buttonsBottom` (the notes made before,
  2026-11), an unknown word = `Err`.
- **"Back to the original"** of a blackboard note decodes `original.bin` in the note's own position (an unknown word:
  upright), whatever is set now.
- **A screen stored again (§9.6)**: "unedited" rebuilds the note from its `original.bin` **in the note's own position**
  (a note made held on a side is not taken for edited, an upright one is not rebuilt turned), and the update makes the
  new page **in that same position** and records it, whatever is set now: it is the same screen, written on in the same
  frame, and the note keeps the way up the user saw it with. A note made before the positions is updated upright and
  gets the key. A note whose key holds an unknown word is not updated: a new note (in the position set now), the reason
  in the log. A page that adds a note instead (edited, no earlier note…) is made in the position set now, like any page.
- **Recognition** (`ImportedPages`): by id and bytes, as before, whatever the position; the second rule (a note created
  at the page's time whose `original.bin` starts with the page) accepts a page of the blackboard either way round
  (`is_blackboard_page`), so a landscape note holds its earlier pages too.
- `is_blackboard_page(size)` is true for every size of `BLACKBOARD_PAGES`, upright or swapped.

### Positions: live mode

- The worker turns each record as it arrives (`Worker::board_forward`, `orient_board_record`), as it turns the Slate's
  (`forward`, `orient_record`): the hub receives the points upright.
- **The position in force when the run starts holds for the run** (`LiveRun::board_position`, taken when its `Started`
  is sent): `Started { width: 20 500, height: 27 200, orientation, kind: Blackboard }` gives the position's quarter turns
  as the Slate's position of the same turn (`Orientation::from_quarter_turns`: the hub's "orientation" counts quarter
  turns from the tablet's own frame), so the live page has the turned size (`oriented_size`). After a lost link the run
  keeps it (`Started` is sent once per run). What the Slate does when its orientation changes during live mode: the
  service sends `Reoriented` and the hub turns the page with its ink. The blackboard does not: a change of
  `boardPosition` during its run is only written in the log ("this live run keeps …") and applies to the next run and
  the next sync. Simplest and safe: no ink turned under the user's hand, no page that changes size in the middle of a
  lesson; the interface has no position switch in the live window for the blackboard anyway. A run waiting for the
  blackboard (not started yet) takes the position set when it starts.
- `LiveSnapshot.orientation` of a blackboard session is therefore the Slate's word for its quarter turns
  (`portraitLeft` for `buttonsLeft`…): the interface must not read it as the Slate's (it does not: no switch and no
  "hold" notice for the blackboard, `LiveView.svelte`).

### Positions: resuming a sheet

- A page of the blackboard is one of its sizes **upright or swapped** (`is_blackboard_page`): a landscape page (made
  held on a side, or turned in the editor) is the blackboard's, never the Slate's (`sheetOtherTablet` with the Slate, as
  before). SPEC §4.2 still says « en hauteur » for this rule: `docs/questions.md`, entry of 2026-11-13 below Q17.
- **The transform of a resumed sheet is the run's position**, as for a new page, never the position the sheet was made
  in: two positions of the same way round are half a turn apart, and the position set now is how the user holds the
  tablet; the sheet's own would write upside down whenever the user holds it the other way from when the sheet was made.
  Written as they come (the worker turned them), the points then land 1:1 on the sheet, upright for the user. The
  sheet's recorded position plays no part in live mode (a live note records none: it has no `original.bin`).
- **A sheet the other way round** (a landscape page with the blackboard held upright, a portrait page with it held on a
  side) is **refused**, never turned or shrunk: turning the tablet's points a quarter turn would write sideways for a
  user who holds it as set (the blackboard has no "hold it so" notice, unlike the Slate's `hold_for`), and fitting the
  area in the sheet would shrink the ink (§2.20). `resume::sheet_refusal(kind, sheet, held)`, `SheetRefusal`:
  `sheetOtherTablet` (the other tablet's sheet), `sheetBoardPosition` (new code, texts at the end of `errorCodes` in
  `fr.json` and `en.json`): at once from `live_resume` when the blackboard is connected (with the position set now), or
  as the `refused` live event when its run starts (`LiveSession::refusal`, with the run's position: also a position
  changed in between), the session `idle`, the note untouched, the tablet leaving live mode, as for
  `sheetOtherTablet` (§5.5). The interface shows `errorCodes.sheetBoardPosition` (the user chooses a position of the
  sheet's way round and resumes again).

### Positions: tests

- `ardoise-core` `settings.rs`: the file without the key, an unknown word, a number, every word; written next to the
  Slate's `orientation`, which it leaves alone, unknown keys kept; read by an older Ardoise as an unknown key; quarter
  turns (the table of `drawingTurns`), names, the wire refusing unknown words.
- `ardoise-device` `pages.rs`: the four corners of each position, the bottom band on the edge the position is named
  after, a round trip, the formulas, a live record; an asymmetric stroke (from the erase button along the band, then up
  the screen) where the user sees it in each position, the page swapped, `original.bin` raw, the key written and read
  back; the sink in each position, recognised by `ImportedPages`; `update_tests`: stored in one position, stored again
  in another (four pairs, then a third store), the note's own position kept; a note made before the positions updated
  upright and given the key; an unknown word: a new note.
- `ardoise-device` `service/tests.rs`: the options on the wire with `boardPosition`.
- `ardoise-sim/tests/blackboard.rs` (through the service, the simulated blackboard): the user's "up" and "right" in the
  tablet's frame for each position, from the buttons' place (`user_axes`, not from the code under test), a "Γ" written
  so: stored, it goes up then right on the note in every position, the page swapped for a landscape one; live, the same,
  `Started` with the position's quarter turns, a change of position during the run ignored, the next run held as set
  then; a screen stored held on the left, the position set to the right, stored again: one note, still turned for the
  left, and a new screen then made for the right.
- `app/src-tauri`: `contract_tests.rs` (`TabletOptions` with `boardPosition`, an unknown word and a missing field
  refused; the quarter turns read from `drawingTurns` of `board-position.ts` when the interface's file is there; the new
  code has its texts); `tablet.rs` (the option saved by `tablet_set_options`, kept by the Slate's quick switch; the
  simulated blackboard of the app synced held on the left: landscape notes that say so); `blackboard_import.rs` (each
  position, recognised again in any; a later store keeps the earlier note's position); `original/tests.rs` (back to the
  original in the note's position, whatever is set now; a note without the key: upright); `live/tests.rs`
  (`refused_now`: the eight cases of position and way round); `live/resume/tests.rs` (a landscape page with the
  blackboard held on either side: its records 1:1; the eight pairs of the sheet's position and the run's through the
  hub: accepted, ink as it came, or `sheetBoardPosition`, the note byte for byte the same).
- Mutations tried on 2026-11-13, each run against the tests named and reverted:
  - `orient_board` returning the point as it came, or `(y, x)` for `buttonsLeft` (a mirror): the two `pages.rs`
    transform tests and the three `ardoise-sim` position tests fail;
  - the quarter turns of `buttonsLeft` and `buttonsRight` swapped (the turn the other way): the `settings.rs` table,
    the two `pages.rs` tests, the three `ardoise-sim` tests and the contract test reading `drawingTurns` fail (the
    import test, which compares with `orient_board` itself, passes, as expected);
  - the update made in the position set now: both `update_tests` of positions and the `ardoise-sim` screen stored again
    fail; the "unedited" check rebuilt upright as before: the four-pairs update test and the `ardoise-sim` one fail;
  - the live records not turned, or turned for the setting instead of the run's position: the `ardoise-sim` live test
    fails;
  - `sheet_refusal` ignoring the way round: the hub's eight-pairs resume test and `refused_now`'s test fail.
