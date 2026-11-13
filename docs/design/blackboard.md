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
  never turned, `Timing::Synthesized`); `page_kind(bytes)` recognises kept bytes (`original.bin`) so that "back to the
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
    mode waits for it; records are never turned. **A live run belongs to the tablet it started on** and never changes
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
      worker closes the link and opens a fresh one (`Worker::board_reopen`) before anything else; the sync or live mode
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
    editor is a sheet like any other (the Slate's).
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
  `blackboard_import.rs` (dated notes once whatever the number of runs, files failing the record check refused),
  `lib.rs` (options).

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
- Already translated, also used: `tabletTimeout`, `tabletNotConnected`, `tabletBusy`, `tabletNotRegistered` (no tablet
  known at all), `noLibrary`, `libraryAccess`, `bluetoothOff`, `bluetoothMissing`, `tablet` (an unexpected failure;
  the internal "dirty stream" state never reaches it), `simulationOnly`.

### 5.5 Live mode

Same commands and events as for the Slate. The live page is portrait (20 500 × 27 200). Hovering records drive the
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
- Hide: rename, orientation, firmware, "charging".
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
  of a screen not erased yet; **the tablet removes such a page as soon as it is downloaded**, so the sync must never
  drop its bytes (to do: a rescue copy when the note cannot be written or the transfer is doubtful; today those
  paths assume the page stayed on the tablet); the erase button can push the page on the stream during live mode
  (to do: such bytes are not pen records) and can lose a page not fetched before live mode (to do: sync before
  live mode when allowed). `docs/questions.md` Q16 lists what is to do and to decide. Since the store button can
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
