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
  per notification) and `split_chunks` (≤ 244 bytes) for the simulator.
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
  `oldest_page`, `download(info)`, `delete(time)`, `sync(sink)`, `start_live`, `stop_live`, `next_live_event`,
  `idle`, `take_page_stored`, `take_oddities`. Replies are sorted from stream bytes as they arrive; stream bytes go to
  the page being downloaded, else to live mode, else are counted as stray. Unknown or malformed frames, stray bytes and
  notifications of other channels are counted (`Oddities`), never an error.
- `manager.rs`: `find_known(slate, board, timeout, quiet)` (each known tablet at its saved address, advertising as its
  kind), `find_blackboards(timeout)` (every blackboard heard), `connect_blackboard(address)` (link + clock). The log
  transport describes frames in the protocol of the tablet it connects to (`LoggingTransport::for_tablet`).
- `pages.rs`: `DownloadedPage::kind`; `note_from_page` reads a blackboard page with `blackboard_note` (its own page,
  never turned, `Timing::Synthesized`); `page_kind(bytes)` recognises kept bytes (`original.bin`) so that "back to the
  original" works for blackboard notes; `BLACKBOARD_PAGE`.
- `service/worker/board.rs`: the blackboard's half of the worker (search to add, connection, sync, live mode, the
  Slate's priority); `service.rs`: the status fields and requests of §5.

### ardoise-ble

- `gatt.rs`: `BOARD_DATA` (`6e400004`), `XIAOMI_SERVICE` (`fe95`), `BOARD_SUBSCRIPTIONS` (replies, then records),
  `subscriptions(kind)`, `is_forbidden_service` (DFU and `fe95`: nothing in them is ever used, even a characteristic
  with a Nordic UUID).
- `convert.rs`: the advertisement keeps the service data of `fe95`; `layout_kind(peer, characteristics)` chooses the
  layout from the advertisement's kind, else (an advertisement without service data at connection) from the
  characteristics (Slate's `00001524`, else `6e400004`); `locate_characteristics(characteristics, kind)`.
- `link.rs`: the layout of the kind; writes only to Nordic UART TX outside the forbidden services (checked again
  before each write). The tablet's ATT MTU request (247) is Windows' business; the download counts bytes whatever
  the chunking.

### ardoise-sim

- `SimBoard` (`board.rs`), its behaviour and the assumptions are listed in its module documentation. Physical actions:
  `draw_live(script)` (the pen writes from now on, on the screen and to a streaming host, at 120 records per second),
  `draw_page(strokes)` (write, store, erase), `press_store()`, `erase()`. Faults: `power_off/on`,
  `disconnect_after`, `stall_next_download(bytes)` (queued), `ignore_next_delete`. Inspection: `stored_pages`,
  `screen`, `stats` (requests, unknown requests, downloads, deletes, live records, page-stored events).
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
5. **Deletion guarantee**: `12 05 06` is written only after the page store returned `Ok` (note written and read back,
   `LibrarySink::store`), with cancellation disabled for that one write (as for the Slate). A download that stops
   short (no byte for 5 s) or brings more bytes than announced is `transferIncomplete`, retried once, then the sync
   stops and the page stays. If the tablet describes again the page it was just asked to delete, the sync stops
   (`pageNotDeleted`) rather than importing it again and again. At most 1 000 pages per sync.
6. **The explicit sync of the blackboard always deletes** (the only way to the next page, SPEC §4.1); the automatic one
   runs only with "delete after import" on; a sync of the blackboard cut by a lost link resumes at the next connection
   as if asked. The interface must warn before an explicit sync when the option is off (§5.6).
7. **One link at a time.** When both tablets are known, the search accepts whichever known tablet it hears first. While
   the blackboard holds the link and nothing happens (no sync, no live mode), the worker looks for the Slate for 1 s
   every 10 s (`SLATE_CHECK_SCAN`, `SLATE_CHECK_INTERVAL`); a Slate advertising (its button pressed) gets the link:
   the Slate only advertises when it wants to connect, the blackboard is always on. Supposed: Windows scans while a
   link is open (to check at H8).
8. **Adding**: `tablet_find_blackboards` closes any open link and listens 60 s (`BOARD_SEARCH`) in 1 s slices, publishing
   what it hears; `tablet_add_blackboard(address)` accepts only an address heard by the search under way, saves
   `blackboard.json` (replacing a blackboard known before: one at a time) and ends the search; the worker then connects
   to it. The name saved is `Mijia LCD blackboard` (the tablet advertises a generic `xiaomi.dev.ble`).
9. **"Page stored" event**: noted in the log; the page count is read again as soon as nothing else runs (never during
   live mode or a sync), so a page stored during live mode is imported afterwards (a safe duplicate of the live note).
10. **Live mode** on the blackboard: `Started { width: 20 500, height: 27 200, orientation: landscape }` (landscape
    means "not turned" in the hub); orientation changes are ignored while the blackboard holds the link; records are
    never turned. The hub times records 5 ms apart inside a notification and follows the wall clock across
    notifications: with up to 6 records per notification at 120 per second, the times inside one notification are
    compressed by at most about 20 ms, which the next notification corrects. Not worth a new field in `LiveInput`.
11. **Future "screen erased" event**: `live_input()` (in `service/worker/board.rs`) is the one place where a frame of
    the reply characteristic becomes a live input; today it maps nothing. When the erase button is measured, its frame
    becomes a `BoardReply` variant mapped there to a new page.
12. **Rename and clock**: renaming with the blackboard connected is `notSupported`; `tablet_set_time` sets its clock and
    returns a drift of 0 (the tablet cannot be asked its time).
13. **Battery** is shown capped at 100 (inferred per cent); `charging` is always false, `firmware` empty (the tablet
    has neither).
14. **Logs**: the blackboard's connections are in the same service journal as the Slate's (`_ardoise/diagnostics/`,
    written when something went wrong or something unclassified arrived); every frame is described in the
    blackboard's protocol.

## 3. Units and time

- x and y as sent, 1:1 in units of 10 µm (inferred: 13.5" 3:4 panel, about 206 × 274 mm); the page 20 500 × 27 200
  (largest coordinates seen: 20 401 and 27 067). To calibrate with a ruler at H8: only the two constants change.
- Pressure 1..=2047 when touching, normalised by 2047 like the Slate's.
- Live mode: each notification stamped on arrival (latency stamp), as for the Slate.

## 4. Tests

- `ardoise-proto` (`blackboard.rs`, `blackboard/record.rs`, `blackboard/beacon.rs`): every request and reply of the
  research note encodes and decodes identically; unknown replies kept, unknown requests refused; malformed frames and
  every prefix never panic; records round-trip; a page becomes timed strokes; beacon recognition.
- `ardoise-device`: `blackboard.rs` (scripted tablet: info and clock, a sync deleting each page after the sink, a page
  the sink refuses is not deleted, short and stalled transfers, a page still described after its deletion, an empty
  tablet, unknown input counted, live mode order, a lost link); `identity.rs`, `pages.rs`, `logging.rs`,
  `transport.rs`, `service/tests.rs` (status shapes, error codes).
- `ardoise-ble/src/convert.rs`, `gatt.rs`: recognition, the blackboard's layout never touching `fe95`, the layout choice.
- `ardoise-sim/src/board.rs`: the simulated tablet itself.
- `ardoise-sim/tests/blackboard.rs` (service against the simulated blackboard, virtual time): adding it then three dated
  notes; a page deleted only after its note is written and checked (the store refuses the second page: it stays on the
  tablet, the next sync brings it without the first one twice); deletion off: on request only, still deleted; live mode
  gives exactly the touching records written, hovering ones too, and goes on when a page is stored (imported
  afterwards); a link lost during live mode is recovered; both tablets known, the one switched on connects, the Slate
  goes first, forgetting one keeps the other; a search hearing nothing ends with `blackboardNotFound`, a search can be
  cancelled; transfer faults through the manager (stall once retried, stall twice a typed error, an ignored delete).
- `ardoise-sim/tests/blackboard_captures.rs` (private captures, skipped without them): every vendor frame of every
  capture decodes and encodes back (60 requests, 35 replies); every live notification is whole records of a known
  status (904 notifications, 4 062 records); the nine downloads of Mi Home's import are, byte for byte, the kept
  pages, of the announced sizes; every kept page decodes completely, into as many strokes and points as the study's
  renderings, and becomes a note; the page stored during the study holds the records received live, byte for byte.
- `app/src-tauri`: `tablet.rs` (the commands with the simulated blackboard of `--simulated-blackboard`), 
  `blackboard_import.rs` (dated notes once whatever the number of runs), `lib.rs` (options).

Not done: a replay of a blackboard journal (`ardoise-sim::replay` knows the Slate's journals only; no journal of the
real blackboard exists yet), e2e journeys (the interface work).

## 5. The contract for the interface

### 5.1 Commands

Unchanged: `tablet_status`, `tablet_subscribe`, `tablet_register` (the Slate's wizard), `tablet_set_options`,
`tablet_info` (the Slate's `device.json`), `forget_tablet` (the Slate only), `sim_press_button` (the simulated Slate),
the live commands (`live_start`, `live_stop`, …: live mode starts on whichever tablet is connected or connects first).

Changed meaning, same signature:

| Command | With the blackboard |
|---|---|
| `tablet_sync()` | Syncs the connected (or next connected) tablet. On the blackboard it **always deletes** each page once its note is written: warn first when `options.deleteAfterImport` is false (§5.6). |
| `tablet_cancel()` | Also ends the search for a blackboard (`connection` back to `searching` or `notRegistered`, no error). |
| `tablet_rename({ name })` | Error `notSupported` while the blackboard is connected. |
| `tablet_set_time()` | Sets the blackboard's clock; returns `0` (it cannot be read back). |
| `tablet_set_orientation({ orientation })` | Saved; not used while the blackboard is live. |

New:

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `tablet_find_blackboards` | none | `null` | Starts the search: `connection: 'adding'`, `boardSearch` filled as blackboards are heard, for 60 s. Closes any open link. `tabletBusy` (in `status.error`) during a sync or live mode. |
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
| Syncing | `syncing` | `'blackboard'` | | `storedPages` = pages left | `sync: { current, total, saved }`, then `lastSync: { saved, deleted, error }` |
| Live | `live` | `'blackboard'` | | | live window as for the Slate |
| Link lost | `searching` | `null` | | kept | live mode shows "reconnecting" |

### 5.4 Error codes (new ones need `errorCodes.*` in `fr.json` and `en.json`)

- `blackboardNotFound` (new): the search heard no blackboard in 60 s (Mi Home may hold it, or it is off), or the
  address given was not heard.
- `transferIncomplete` (new): a page download stopped before its announced size (twice); the page stays on the tablet.
- `pageNotDeleted` (new): the tablet still holds the page Ardoise deleted; the sync stopped (the page's note is in the
  library).
- `notSupported` (new): the connected tablet cannot do it (renaming the blackboard).
- Already translated, also used: `tabletTimeout`, `tabletNotConnected`, `tabletBusy`, `tabletNotRegistered` (no tablet
  known at all), `noLibrary`, `libraryAccess`, `bluetoothOff`, `bluetoothMissing`, `tablet`, `simulationOnly`.

### 5.5 Live mode

Same commands and events as for the Slate. The live page is portrait (20 500 × 27 200). Hovering records drive the
laser pointer. Storing a page with the tablet's button does not interrupt live mode. The tablet's erase button sends
nothing known yet: "next page" stays on the computer. The orientation switch has no effect on the blackboard.

### 5.6 What to show and hide for the blackboard (`device.kind === 'blackboard'`, `known[].kind`)

- "Add a tablet" offers the blackboard: `tablet_find_blackboards`, list `boardSearch.found` (one is the usual case;
  `rssi` helps choose), `tablet_add_blackboard`. **No** "hold the button" animation, no registration warning: it stays
  bound to Mi Home. Say that Mi Home must be closed (or the phone's Bluetooth off) while adding and using it.
- Show: battery, free memory (`memory.free` of `memory.total`), pages stored, model. The clock is set at each connection:
  no button needed.
- Hide: rename, orientation, firmware, "charging".
- Sync: before `tablet_sync` with `options.deleteAfterImport === false`, say that the pages will be deleted from the
  blackboard (it only gives its oldest page). The automatic sync does nothing for it then.
- Forget: one "forget" per known tablet (`forget_tablet` for the Slate, `tablet_forget_blackboard` for the blackboard).
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
are ignored, a `.bin` that is not whole records is reported. Prints the notes written and a summary in French; exit 0
when nothing failed.

## 8. Not done, not measured, risks

- Nothing here was tried on the real tablet. To measure at H8: the download and deletion of a test page from the PC
  (only seen in Mi Home's capture), what the erase button sends, the unit (ruler), whether Windows delivers 244-byte
  notifications whole (the count does not depend on it), whether a scan works while the blackboard's link is open (the
  Slate's priority), whether the blackboard drops a silent link.
- btleplug's WinRT backend may drop notifications when our task falls behind (device-stack.md): a page download then
  ends short and fails as `transferIncomplete` (nothing deleted), never silently.
- A firmware update may change the protocol: unknown frames are logged and ignored, never answered.
