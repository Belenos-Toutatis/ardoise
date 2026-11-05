# Tablet in the interface, live mode and classroom (milestone M4)

Developer document (English). Requirements: SPEC §4.1, §4.2, §5, §6.2 ("Rust → interface", "Fenêtres"), §8;
ROADMAP M4. Builds on [device-stack.md](device-stack.md) (Session, Manager, simulator) and
[interface.md](interface.md) (screens, browser stand-in, translations).

```
                 ardoise-device::service::TabletService  (one worker thread, owns Manager + Session)
 BLE connector ──┐   status ─────────────► app tablet.rs ── Channel<TabletEvent> ──► main window (sidebar, panel, wizard)
 sim (real time)─┘   pages ──► store_page (app: LibrarySink + thumbnail + automatic export)
                     live  ──► LiveSink ──► app live.rs LiveHub (pages, layers, style, view) ── Channel<LiveBatch> ──► every window
                                                                                                        (control = main window, presentation)
```

## Rules

- Everything in [device-stack.md](device-stack.md) still holds (no panic on tablet bytes, `Clock` for protocol time,
  blocking + timeouts, atomic writes, CRLF files keep CRLF).
- **Latency stamps are the one exception to "time comes from a `Clock`"**: they measure real elapsed time, so they
  use the system clock (`SystemTime` for the wall-clock stamp sent to the interface, see "Latency").
- One owner per file (see "Work packages"); a shared file is changed only by its owner, others ask in their report.

## Settings (`ardoise.toml`, contract in `ardoise-core::settings`, done)

`[tablet]` section, shared by every PC (like `auto_export`): `auto_sync = true`, `delete_after_import = true`,
`orientation = "landscape" | "landscapeFlipped" | "portraitLeft" | "portraitRight"` (M8, replaces `hand`),
`button = "newPage" | "newLayer"`, unknown keys kept. Unknown enum values load as the
default. Per machine: `[machines."PC"] projector = "<monitor key>"` (the last screen chosen for the presentation).

Left hand (`hand = "left"`, before M8): the tablet is used upside down, so every point (synced pages and live mode) is
turned half a turn: `x' = width - x`, `y' = height - y` (`ardoise_device::pages::orient`, applied by the service and by
the page store). Settings are written through `commands::update` (`pub(crate)`). See "M8: orientation" below for
what replaced it.

## ardoise-device::service (work package A)

`TabletService` runs one worker thread that owns the connector, the `Manager` and the current `Session`. Public API
(signatures fixed in `service.rs`; bodies are package A's):

- `TabletService::start(config: ServiceConfig, connector: Box<dyn ConnectorFactory>, hooks: Arc<dyn ServiceHooks>,
  on_status: StatusCallback, store_page: PageStore) -> TabletService`.
- `status()`, `register()`, `sync_now()`, `cancel()`, `rename(name)`, `set_time()`, `set_options(options)`,
  `forget()`, `set_library(library, identity_root)`, `start_live(sink)`, `stop_live()`, `shutdown()`.
- Blocking requests (`rename`, `set_time`, `forget`) go to the worker and wait for its answer (timeout 30 s).

Behaviour:

- **Auto-connect** (SPEC §4.1): while a tablet is registered and nothing else runs, the worker scans for it in slices
  of at most 5 s (`find_registered`), connects (`Manager::connect`: auth, set time), reads `info()` and the stored page
  count, then, if `auto_sync` and pages are waiting, syncs. It stays connected (`connected`) until the tablet drops the
  link, then goes back to `searching`. A connection error is shown (`error`) and retried after 5 s.
- **Register** (wizard): `register()` → `registering` with `register_step` `holdButton` (hooks.registration_started),
  `searching` (find in register mode, up to 60 s), `connecting`, `pressButton` (hooks.waiting_for_button; button
  timeout 30 s), `done`. The identity is saved in `<identity_root>/_ardoise/device.json` by `Manager::register`.
  `cancel()` stops it (`register_step` back to `null`, connection `notRegistered`).
- **Sync** (`sync_now()` or automatic): `syncing` with `sync = { current, total, saved }`. Every page goes through
  `store_page` (which writes the note and reads it back); only when it returned `Ok` is the page deleted from the
  tablet, and only when `delete_after_import`. Without deletion the tablet only lets us read its oldest page: a page
  already imported (same deterministic note id, `pages::page_note_id`) is not stored twice. `cancel()` stops between
  pages (pages already saved stay saved and deleted). At the end `last_sync = { at, saved: [paths], deleted, error }`
  (`error` when the sync stopped early or could not start, e.g. `syncRefused`; it stays until the next sync, across
  reconnections, unlike `status.error`). After a sync that deleted pages the count is read again at once: a page
  finished meanwhile is synced on the same connection (H1, see device-stack.md).
  When the tablet is not connected, `sync_now()` waits for it (connection `searching`, `pending = "sync"`).
- **Live mode**: `start_live(sink)` enters live mode when connected (or as soon as the tablet connects: `pending =
  "live"`), calls `hooks.live_started()`, then forwards every notification to `sink` as `LiveInput` (decoded records
  with the receipt stamp, proximity, button, state changes). **Records are delivered already oriented** (left hand:
  `pages::orient` applied by the service), so the hub uses them as page coordinates; `Started.orientation` tells the
  hub the page size (M8). Lost links are reopened (`Manager` logic) and reported
  as `LiveInput::State(Reconnecting)` then `Live`. `stop_live()` leaves live mode (`b1 02`) and stays connected.
- **Button in live mode**: the Slate sends the unasked command frame `cb 01 00` (`ardoise_proto::live::BUTTON_COMMAND`,
  found in our journals, confirmed on the real tablet on 2026-11-05); a match becomes `LiveInput::Button`. Until then the
  code looked for a sysevent invented for the simulator (`f2 01 01`), so the real button did nothing. **Every other notification received during live mode that is
  not pen data is written to the diagnostic log with the note `unclassified during live mode`**, so that the H2 log
  shows the real signature, except the frames already known (battery level, and the frames of the real Slate whose
  meaning is unknown: protocol.md §12), which are only in their `rx` lines.
- **Battery**: the battery level announced by the tablet (sysevent `f1`, protocol.md §12) replaces
  `device.batteryPercent` at once, in any state (idle, sync, live).
- **Log**: the service keeps an in-memory diagnostic log; when an error happens it is written to
  `<identity_root>/_ardoise/diagnostics/` (atomic, one file per failing session) so that problems come back by
  Nextcloud. Live mode sessions are not written unless they fail. A written log always ends with the link's
  `disconnect` (or `disconnected`) event and a last note `end of this log: <why>` (closed after a failure, the tablet
  disconnected, the tablet service stopped...), also when the application exits (it stops the service, waiting at
  most 3 s).
- `set_library(None, ..)`: no library, connection `unavailable` with error code `noLibrary`.
- BLE adapter missing / off: connection `unavailable`, error `bluetoothOff` / `bluetoothMissing`, checked again every 10 s.

`TabletStatus` (serde camelCase, mirrored by `app/ui/src/lib/tablet.ts`):

| field | meaning |
|---|---|
| `connection` | `unavailable`, `notRegistered`, `registering`, `searching`, `connecting`, `connected`, `syncing`, `live` |
| `registered` | `TabletInfo` of `device.json` (name, address, registered, width, height) or null |
| `device` | last information read: `name, firmware, batteryPercent, charging, width, height, storedPages, model, readAt` (unix ms) or null |
| `registerStep` | `holdButton`, `searching`, `connecting`, `pressButton`, `done`, or null |
| `sync` | `{ current, total, saved }` while syncing, else null |
| `lastSync` | `{ at, saved: string[], deleted, error }` (paths of the notes written; `error`: `{ code, message }` when the sync failed, else null) or null |
| `pending` | `sync`, `live` or null: what waits for the tablet to connect |
| `error` | `{ code, message }` of the last failure (translated by `errorCodes.*`), cleared by the next success |
| `simulated` | started with `--simulated-tablet` |
| `options` | the `TabletOptions` in use (`autoSync`, `deleteAfterImport`, `hand`, `button`) |

`model` is derived from the dimensions: `Bamboo Slate A4` (29700 × 21600), `Bamboo Slate A5` (21600 × 14800), else
`Wacom (W × H)`.

## Simulated tablet in the app (`--simulated-tablet`, work package A)

`ardoise.exe --simulated-tablet` replaces Bluetooth by `ardoise_sim::realtime::RealTimeConnector`: the virtual
clock follows the wall clock (recv waits for real), so live mode streams at 200 samples/s for real.

- The simulated tablet starts with the 3 reference pages stored (`drawings::reference_pages`), battery 87 %.
- `SimHooks` play the user: `registration_started` holds the button after 1 s, `waiting_for_button` presses it after
  1 s, `live_started` queues 10 minutes of pen movement (`drawings::live_script_seeded`, hover gaps included).
- **Never touches the real registration**: with `--simulated-tablet` the identity root is a throwaway folder
  (`%TEMP%\ardoise-simulation-*`, like `--diagnostic --simulated`); synced notes still go to the configured library
  (the e2e journeys use a demo library).
- Test command, only with `--simulated-tablet` (`simulationOnly` error otherwise): `sim_press_button` (presses the
  simulated tablet's button: in live mode it sends the button signature).

## App commands (Tauri)

Errors are `{ code, message }`. New codes: `tabletNotConnected`, `tabletBusy`, `tabletNotRegistered`,
`bluetoothOff`, `bluetoothMissing`, `tabletTimeout`, `tabletRefused`, `liveNotRunning`, `noMonitor`,
`simulationOnly`, `invalidName`, `tablet` (any other device error, `message` = detail), `liveEditStale`,
`liveEditInvalid`, `narrationTooLong`, `narrationUnreadable`, `narrationNotKept` (M12).

Tablet (`tablet.rs`, package A):

| Command | Arguments | Returns |
|---|---|---|
| `tablet_info` / `forget_tablet` | – | unchanged (M3); `forget_tablet` also stops the service's connection |
| `tablet_status` | – | `TabletStatus` |
| `tablet_subscribe` | `onEvent: Channel<TabletEvent>` | current `TabletStatus`; then `{ type: 'status', status }` at every change |
| `tablet_register` / `tablet_cancel` | – | – (progress in the status) |
| `tablet_sync` | – | – (progress in the status) |
| `tablet_rename` | `name` (1–15 printable ASCII characters after trimming, else `invalidName`) | `TabletStatus` |
| `tablet_set_time` | – | drift in seconds after setting it |
| `tablet_set_options` | `options: TabletOptions` | `TabletStatus` (saved in `ardoise.toml`) |
| `sim_press_button` | – | – |

Live and classroom (`live.rs`, `presentation.rs`, package B):

| Command | Arguments | Returns |
|---|---|---|
| `live_subscribe` | `onEvent: Channel<LiveBatch>` | `LiveSnapshot`; then batches until the window goes away (a failed send drops the subscriber) |
| `live_start` | – | `LiveSnapshot` (`state: 'waiting'` until the tablet streams) |
| `live_resume` | `path` (a note of the library) | `LiveSnapshot`, like `live_start`, on that note (M12) |
| `live_page_background` | `pageIndex`, `page`, `background`, `widthPx` | PNG data URL of the page's paper, like `render_background`, with the background image of a resumed sheet (M12) |
| `live_stop` | – | `LiveStopped`: `{ saved: string[]; forks: SheetFork[]; narration: { code, message } \| null }` (paths of the notes saved; the sheets that went on in a new note, the last write included; a recording under way that could not join its pages) |
| `live_set_style` | `style: LiveStyle` | – (broadcast as a snapshot) |
| `live_command` | `command: LiveCommand` | – |
| `live_restore_cleared` | – | whether the ink of the page's last clearing came back (through its history, M12) |
| `live_edit_strokes` | `pageIndex`, `revision`, `edit: { kind: 'delete', indices } \| { kind: 'recolour', indices, color }` | the page's revision after the edit; `liveEditStale`, `liveEditInvalid` (M12) |
| `live_undo_edit` / `live_redo_edit` | `pageIndex` | whether there was a step; `liveEditStale` when that page is not on screen (M12) |
| `live_set_view` | `view: LiveViewState` | – (broadcast as `view`) |
| `list_monitors` | – | `MonitorInfo[]` (the saved choice, if still present, has `chosen: true`) |
| `open_presentation` | `monitor: string \| null` (null = saved or default choice) | `MonitorInfo` used |
| `close_presentation` | – | – |

### Live session (`LiveHub`, `live.rs`)

- A session holds **pages**; each page is one note (`origin: live`, page size = tablet size, background white, or
  `slate` paper while the blackboard is on), created in the library root when its first stroke ends, then rewritten
  atomically (`Library::save` with the stamp it got) at most every 5 s while dirty (`save_wait`: ten times the last write's
  duration when that is longer, at most 6 intervals, 30 s), and when the page changes and at `live_stop`. The title is empty
  (the library grid shows the date), like synced pages.
- Strokes: pressure > 0 starts or continues a stroke (tool, colour, width from the current `LiveStyle`, current layer);
  pressure 0 or "pen left" ends it. Pressure is normalised (0..2047 → 0..1); point time = ms since the note's
  `created`, from the record stamps (5 ms per record).
- Hover (pen near, pressure 0): `hover` events at most once per batch, `hoverEnd` when the pen leaves. With
  `view.laser` on, the interface draws a red dot there and **nothing is written** while hovering.
- Commands (`LiveCommand`): `nextPage` (to the next page, a new one at the end), `previousPage`, `newPage`,
  `newLayer` (adds a layer to the current page, new strokes go there), `clearPage` (removes what shows of the current page's strokes and the
  stroke in progress; a step of the page's history since M12), `button` (what the tablet's button does: `newPage` or `newLayer` from the options).
  `LiveInput::Button` runs `button`.
- View (`LiveViewState`): `zoom` (1 = whole page, up to 8), `x`, `y` (centre of the projected area in page units),
  `board` (blackboard: slate paper, default style chalk white), `laser` (hover shows a dot). Shared by every window.
- Batching (SPEC §6.2): input is applied at once; events are sent to every subscriber as soon as they exist, but not
  more often than every 4 ms (pending events are flushed by a timer thread), each send carrying everything pending.

TypeScript mirror (`app/ui/src/lib/live.ts`):

```ts
LiveStyle = { tool: Tool; color: Rgba; width: number }
LiveViewState = { zoom: number; x: number; y: number; board: boolean; laser: boolean }
LiveCommand = { kind: 'nextPage' | 'previousPage' | 'newPage' | 'newLayer' | 'clearPage' | 'button' }
LiveSnapshot = { state: 'idle' | 'waiting' | 'live' | 'reconnecting'; page: Note; pageIndex: number; pageCount: number;
                 canTurnPage?: boolean; // M12: false on a resumed sheet; absent on the share's viewers
                 layer: number; style: LiveStyle; view: LiveViewState; hover: { x: number; y: number } | null;
                 presentation: MonitorInfo | null; saved: string[];
                 revision?: number; canUndo?: boolean; canRedo?: boolean } // M12; absent on the share's viewers
LivePoint = { x: number; y: number; p: number; t: number; recv: number }   // recv: wall-clock ms (fractional) at receipt
LiveEvent =
  | { type: 'snapshot'; snapshot: LiveSnapshot }               // page change, clear, layer, style, state, presentation
  | { type: 'strokeStart'; pageIndex: number; stroke: Stroke } // stroke with its first point(s), as `LivePoint`s
  | { type: 'points'; pageIndex: number; points: LivePoint[] } // appended to the stroke in progress
  | { type: 'strokeEnd'; pageIndex: number }
  | { type: 'hover'; x: number; y: number } | { type: 'hoverEnd' }
  | { type: 'view'; view: LiveViewState }
  | { type: 'saved'; paths: string[] }
  | { type: 'sheetForked'; pageIndex: number; reason: 'changed' | 'deleted'; original: string; path: string } // M12
  | { type: 'strokesEdited'; pageIndex: number; revision?: number; removed: number[];                       // M12
      inserted: { index: number; stroke: Stroke }[]; recoloured: { color: Rgba; indices: number[] }[];
      canUndo?: boolean; canRedo?: boolean }
LiveBatch = { events: LiveEvent[]; sentAt: number }            // sentAt: wall-clock ms (fractional) when sent
MonitorInfo = { key: string; name: string; x: number; y: number; width: number; height: number; scale: number;
                primary: boolean; chosen: boolean }
```

### Presentation window (`presentation.rs`)

- `list_monitors` uses `available_monitors()` (physical pixels). `key` = `name@x,y` (stable across unplugging).
- Choice (pure function, unit-tested): the requested key if present; else the saved key (`[machines."PC"]
  projector`) if present; else the first non-primary monitor; else the primary one; none → `noMonitor`.
- `open_presentation` creates (or moves) the window labelled `presentation`, URL `index.html?view=presentation`,
  `decorations(false)`, `skip_taskbar(true)`, `shadow(false)`, positioned at the monitor's physical position with its
  physical size, then `set_fullscreen(true)`; created from an async command (SPEC §6.2: avoids a Tauri deadlock on
  Windows). The choice is saved. Closing the window (Esc in it, or `close_presentation`) broadcasts a snapshot with
  `presentation: null`.

## Latency (SPEC §4.2: < 30 ms from the point's receipt to its display)

- `LivePoint.recv` = wall-clock time (ms since the Unix epoch, fractional, `SystemTime`) when the notification holding
  the point came out of the transport (`Session` stamps it). `LiveBatch.sentAt` = wall-clock time when the batch was
  handed to the channel. **Rust part** = `sentAt - recv`.
- **Interface part**: from the batch's arrival (`performance.timeOrigin + performance.now()`) to the end of the
  drawing of its points (after the canvas calls of the `requestAnimationFrame` callback that painted them). **End to
  end** (Windows, real window) = the start of the *next* animation frame (a point painted in frame k counts as
  displayed when frame k+1 begins, the closest the page gets to the screen) − `recv`, both on the wall clock of the
  same PC; the display's own scan-out is not included.
- The interface keeps the last 2 000 end-to-end samples (`lib/latency.ts`), shows the median in the control window
  (`[data-latency-median]`, ms with one decimal) and exposes `window.__ardoiseLatency = { samples(), median() }` for
  the e2e journey.
- Linux proof (no window): the Rust part through simulator → service → hub (test prints the median), and the
  interface part in vitest (batch → drawing on a mock canvas, prints the median).

## Interface

- `lib/tablet.ts` (types), `lib/live.ts` (types), `lib/api.ts` wrappers (section "Tablet (M4)" and "Live mode and
  classroom (M4)"), browser stand-ins `lib/mock-tablet.ts` and `lib/mock-live.ts` (routed by `lib/mock.ts`).
- `main.ts` mounts `components/live/Presentation.svelte` when the URL has `?view=presentation`, `App.svelte` otherwise.
- `Home.svelte` shows `components/live/LiveView.svelte` (props `{ app, onclose }`) full space when live mode is open
  (`openLive()` from the tablet panel), like the editor.
- Sidebar (`library/TabletStatus.svelte`): name, connection state (dot + text), battery (% and charging), a progress
  bar while syncing; click opens the tablet panel (`components/tablet/TabletPanel.svelte`).
- Tablet panel: information (SPEC §4.1: name, battery and charge, firmware, pages in memory, model and size), "Sync
  now" with progress `current / total` and **Cancel**, "Live mode", automatic sync toggle, "delete from the tablet
  after import" toggle (on by default, with the sentence saying the page is deleted only once its file is written and
  checked), button action (new page / new layer), settings: rename, set the clock, the tablet's position (M8: four), forget (with
  confirmation). No tablet registered: the registration wizard.
- Registration wizard (`components/tablet/RegisterWizard.svelte`): 1. warning first (registering makes the tablet
  unusable with the phone app and with Inkspace until it is registered there again), 2. animation: a tablet drawing
  whose button is held 6 s while a ring fills, then the LED blinks blue (CSS, `prefers-reduced-motion` respected),
  3. searching, 4. "press the button briefly" animation, 5. done (information), with Cancel at every step and an
  error step that says what to do.
- Live view (control window): toolbar (tools, colours, width — the editor's palette), page navigation (previous,
  `page n / N`, next, new page), new layer, clear page, blackboard, laser pointer, zoom (+ / − / 1:1) and pan (drag in
  the preview), monitor chooser + "Project" / "Stop projecting", the preview (`LiveCanvas` of the whole page with the
  projected rectangle drawn on it), the measured latency, "Stop".
- Presentation window: only `LiveCanvas` with the view (zoom/pan), the blackboard and the laser dot; no element of
  interface, cursor hidden after 2 s still.
- Keyboard (both windows): → / PageDown / Space = next page, ← / PageUp = previous page, Ctrl+Delete (or Ctrl+Backspace)
  = clear the page, B = blackboard, L = laser, + / − = zoom, 0 = whole page, arrows with Shift = pan, Esc = leave
  (presentation window: close it). The pure mapping is `lib/live-keys.ts` (unit-tested).
- `LiveCanvas.svelte`: a `<canvas class="live-canvas">` drawing the page background (`render_background`, or slate
  paper), finished strokes cached on an offscreen canvas, the stroke in progress redrawn each frame from
  `ink.ts` outlines (same geometry as the exports), the laser dot. Attributes for tests: `data-strokes` (number of
  strokes of the page, in progress included), `data-page` (`n/N`).
- Selectors used by the e2e journeys: `.tablet-status[data-connection]`, `.tablet-panel`, `button[data-action="sync"]`,
  `button[data-action="register"]`, `.register-wizard [data-step]`, `button[data-action="live"]`, `.live-view[data-state]`,
  `canvas.live-canvas[data-strokes][data-page]`, `[data-latency-median]`, `button[data-action="stop-live"]`.

## Work packages

| Package | Profile | Owns |
|---|---|---|
| A — service, simulator, tablet commands | specialiste | `crates/ardoise-device/src/{service,session,manager,pages}.rs` (+ tests), `crates/ardoise-proto/src/live.rs`, `crates/ardoise-sim/src/*` (+ `realtime.rs`, tests), `app/src-tauri/src/tablet.rs` |
| B — live session, classroom windows | specialiste | `app/src-tauri/src/live.rs`, `app/src-tauri/src/presentation.rs` |
| C — tablet interface, e2e, H2 | developpeur | `components/library/TabletStatus.svelte`, `components/tablet/*`, `components/Settings.svelte`, `lib/tablet-state.svelte.ts`, `lib/mock-tablet.ts`, locales section `tablet`, `app/e2e/*`, `docs/hardware-tests.md` |
| D — live view, presentation, latency | specialiste | `components/live/*`, `lib/live-*.ts`, `lib/latency.ts`, `lib/mock-live.ts`, locales sections `live` and `presentation` |

Contracts (this document, `service.rs` types, `settings.rs`, `lib.rs` command list and CLI, `api.ts`, `tablet.ts`,
`live.ts`, `mock.ts` routing, `main.ts`, `Home.svelte`, `errorCodes.*`) are set in the contracts commit; a package that
needs to change one says so in its report.

## Decisions made while building M4

Tablet service (package A):

- **One short step at a time.** The worker does one scan slice, one connection, one page or one notification, then
  reads its requests. An urgent request (cancel, forget, another library, shutdown, or any request while scanning)
  raises the `Manager`'s cancel flag; `Manager::scan` now notices it at the next advertisement instead of at the end
  of the 5 s slice, so cancelling the wizard takes less than 2 s.
- **Deletion is never interrupted halfway.** A page is deleted only after `store_page` returned `Ok`, and a cancel
  arriving meanwhile does not stop that deletion (it would leave a saved page on the tablet, synced again later).
  A panic in `store_page` is caught: the page stays on the tablet.
- **No duplicates without deletion.** `pages::page_note_id` is a deterministic UUID (version 8) from the page's
  time and bytes; `ImportedPages` recognises a page already imported by that id and identical `original.bin` bytes,
  including a note now in the trash (it is not imported again).
- New `DeviceError` cases `NotConnected`, `Busy`, `NoTabletRegistered`, `NoLibrary`, `InvalidName`, mapped to the
  error codes by `service::error_code` / `From<&DeviceError> for StatusError`. `note_from_page` takes the orientation (M8; the
  hand before); `LibrarySink::with_orientation`.
- Synced notes go to the library root. `sync.total` is the number of pages read (1 without deletion). When the
  tablet connects, a pending live mode goes before the automatic sync. Renaming or setting the clock during a sync or
  live mode answers `tabletBusy`. A foreign tablet that refuses our identity at another address is ignored silently.
- **Logs.** One in-memory log per connection attempt, written atomically to `<identity_root>/_ardoise/diagnostics/`
  when it ends in an error (once for the same error repeated until the next success), and for a live session that
  received an `unclassified during live mode` notification (at the end of live mode, at a lost link, or past 20 000
  lines). The `10 …` status report is logged as unclassified.
- `tablet_rename`, `tablet_set_time` and `forget_tablet` run in `spawn_blocking` (they wait for the worker).
- Simulator: `realtime::RealTimeConnector` (the virtual clock follows the wall clock), `SimFactory`, `SimHooks`
  (1 s delays in the app, none in tests; 10 min of handwriting when live mode starts), `app_tablet()` (3 reference
  pages, battery 87 %), `SimStats::live_buttons`; the button pressed while live mode streams sends `cb 01 00` on
  the command channel, as the real tablet does (see "Button in live mode").

Live session and presentation (package B):

- The first batch of a subscription starts with the snapshot the command returns (the batch can arrive before the
  reply), so a window applies batches in order. One subscription per window: subscribing again replaces it.
- `pageIndex` is 0-based; `saved` carries every path, in page order; `hover` is sent even without the laser and a
  `hoverEnd` is sent when the pen touches the paper. A snapshot sent in the middle of a stroke is followed by a
  `strokeStart` that gives the stroke in progress again.
- Point times are 5 ms apart and follow the wall clock after a pause, never going back; a page's `created` is its
  first point.
- After `live_stop` the session stays on screen (pages and view can still change); `liveNotRunning` only before the
  first start; `live_stop` can be repeated. Pages are also saved when the main window closes. A note moved meanwhile
  is followed; a note changed elsewhere is left alone and the page continues in a new note.
- The blackboard changes the paper of the current and new pages only; leaving it restores the previous style.
- Sends and file writes happen outside the hub's lock (a window or the disk never delays the tablet's input); within
  a batch, points are grouped and only the last `hover` is kept.
- Full screen uses `set_fullscreen_on_monitor` at the monitor's physical position (safer with mixed DPI); the
  presentation window does not take the focus. `capabilities/presentation.json` grants `core:default` only (Esc goes
  through `close_presentation`).
- The 4 ms batching delay uses `thread::sleep` (to measure on Windows at H2: a timed wait on a condition variable
  would follow the 15.6 ms system tick).

Interface (packages C and D):

- One `tabletSubscribe` for the whole app (`lib/tablet-state.svelte.ts`); options apply at once and roll back if the
  service refuses. The panel has a `sim-press` button with the simulated tablet only.
- `lib/live-state.svelte.ts` keeps batches received before the subscription's snapshot; view and style changes show
  at once (an older echo from Rust does not undo them), views are sent at most every 32 ms.
- `window.__ardoiseLatency = { samples, median, p95, uiSamples, uiMedian }`; `median()` is `null` without samples.
- The e2e journey checks that the tablet is empty from the sync result ("3 pages deleted"), and clicks "Sync now" if
  the automatic sync did not bring 10 notes within 20 s.

Review fixes (after the independent review of M4):

- **A copy that failed its check never counts as imported.** `LibrarySink::discard` deletes for good a file holding
  exactly the bytes written (nothing is lost: the page stays on the tablet), moves anything else to the trash, and
  leaves an unreadable file in place; `in_trash` only counts a trashed note that reads back with the page's id and
  its exact `original.bin`.
- **The guided test and the service share the tablet.** A real (not simulated) guided test pauses the service
  (`TabletRuntime::pause`: status `unavailable` with `tabletBusy`); a guard restarts a fresh service when the test
  ends, even on cancel, error or panic, so `device.json` is read again.
- **Live pages that cannot be written.** `LiveEvent::SaveFailed { pageIndex, message }` is sent once per page while
  the error repeats; a later successful write sends `saved`, after which the pages still failing are announced again.
  The control window shows a persistent banner. When the main window closes, a page that cannot be written to the
  library is backed up (complete `.ardoise`, atomic, new id) in `%LOCALAPPDATA%\Ardoise\live-backup\` (the temporary
  folder without `LOCALAPPDATA`), and the path is logged in `live-backup.log` there.
- After `live_stop`, `clearPage`, `newLayer` (also through the button) and the blackboard are refused
  (`liveNotRunning`): they would rewrite notes already saved. Paging, view and style stay allowed.
- While the tablet stays connected and idle, its page count is read again every 30 s (status up to date, automatic
  sync when it grew; assumed harmless for the tablet, to check at H2).
- `TabletRuntime` keeps the service in an `Arc` and never holds its lock while a request waits; tablet subscribers are
  keyed by window label; the interrupt flag stays up while an interrupting request waits (`Interrupts` counter);
  a long live log is written by a detached thread.
- Without "delete after import" only the oldest page can be read: the option's hint says so, and the panel says how
  many pages wait after such a sync.
- With `--simulated-tablet`, forgetting the tablet never falls back on the library's `device.json`.
- Known limit: the receipt stamp is taken when the session takes the notification out of the transport, so a wait
  between the Bluetooth thread and the session is not in the measured latency (with the simulator there is none).

## M8: pen and touch, writing over the presentation

*Since M11 (decision Q6, 2026-11-02) the marks are also reported to Rust for the LAN share's viewers: see
docs/design/lan-share.md, "Marks written over the presentation". The window still draws its own marks locally.*

**Writing over the presentation.** Until M8 the presentation window was only a mirror: no pointer handling at all
(`touch-action: none` and a cursor that hides after 2 s). Now, in the **presentation window only**, the pen, a finger
or the mouse write **temporary marks** over the projected page:

- Marks are `Stroke`s in page units kept by `LiveCanvas` (`mode="presentation"`), never sent to Rust, never in the
  note, so the format and the tablet pipeline are untouched. They are drawn after the cache and the stroke in progress
  (`Scene.marks`, `Scene.markCurrent` in `live-render.ts`, same outline geometry as the page: `ink.contours`), follow
  the zoom and pan of the projected area (page coordinates), and disappear when the page changes.
- Look (`lib/live-marks.ts`): felt tip, 6 screen pixels wide at the moment of writing (so the line is as thick at any
  zoom), red on a light page, yellow on a dark one (blackboard, slate paper). No palette: the window has no controls.
- Erase: the **eraser end of the pen** removes whole marks within 18 px; **Delete / Backspace** clears all marks
  (`clearMarks` key action, a no-op in the control window); a **"Clear the marks" button** shows in a corner while
  there are marks, together with the cursor (it hides after 2 s like the cursor, since the window otherwise has no
  element of interface). Ctrl+Delete still clears the page itself, in the control window and the presentation.
- Palm rejection as in the editor (`PenPresence`). A finger writes here (the "Draw with a finger" preference concerns
  the editor); **two fingers pinch the projected area** (`pinchPresentationView`, sent like any view change through
  `live_set_view`, so the control window follows). A second finger dropping while one writes drops that mark.
- The latency measurements do not include marks (they are not tablet points).
- Not done on purpose: marks in the control window's preview, saving marks, a palette or an undo.

**Control window preview**: one finger moves the projected area (as the mouse), two fingers pinch it
(`pinchPreviewView`: zoom around the fingers and follow them), a double tap shows the whole page. **Page width**:
button next to "Whole page" and key **W** (`fitWidthView(view, page, aspect)`: the area as wide as the page, showing
its top; never less than the whole page, zoom at most 8).

## M8: orientation of the tablet (four positions)

Replaces right / left hand with the four positions of Wacom Inkspace (ROADMAP M8). `ardoise_core::settings::Orientation`
(`[tablet] orientation`, wire value of the same name in the interface):

| Value | Held | Button of the tablet | Quarter turns (clockwise) | Page |
|---|---|---|---|---|
| `landscape` | as drawn (long side across) | at the bottom | 0 | width x height |
| `landscapeFlipped` | half a turn (the old left hand) | at the top | 2 | width x height |
| `portraitLeft` | a quarter turn clockwise | on the left | 1 | height x width |
| `portraitRight` | a quarter turn anticlockwise | on the right | 3 | height x width |

"Button at the bottom" is how the tablet is drawn in the registration wizard (`TabletDrawing.svelte`: light at the
bottom left, button at the bottom right); to be confirmed with the real tablet (`docs/hardware-tests.md`, C3).

- **Transform** (`ardoise_device::pages`): `orient(x, y, width, height, orientation)` turns a point of the tablet's own
  frame (`width` x `height` = drawing area) by the quarter turns, clockwise: 1 -> `(height - y, x)`, 2 -> `(width - x,
  height - y)`, 3 -> `(y, width - x)`; the page is `orientation.page_size(width, height)`. Corners map to corners;
  points beyond the area are clamped. `turn_point` is the same turn on a point of a page (f32), used on ink already
  written; `oriented_size` gives the page size. Tablet bytes (`original.bin`) are never touched, so "back to the
  original" decodes again with today's orientation (as it already did with the hand).
- **Migration**: a settings file with `hand = "right"` reads as `landscape`, `hand = "left"` as `landscapeFlipped`
  (`RawTabletSettings` reads both keys; the legacy values live in `From<String>`). Both keys together are accepted
  (`orientation` wins; no serde alias, which would make the pair a duplicate-field error and send the whole file
  back to the defaults). The file is written back with `orientation` **and** `hand` (`landscapeFlipped` ->
  `"left"`, everything else -> `"right"`), so that an older Ardoise sharing the file through Nextcloud keeps the half
  turn: it keeps `orientation` in its unknown keys and rewrites `hand` next to it.
- **Sync**: `note_from_page` gives the note the oriented page size and turned points. The note id is derived from the
  page bytes only, so a page already imported is not imported again after the position changed.
- **Live**: the service orients the records (`orient_record`) and sends `LiveInput::Started { width, height,
  orientation }` (the tablet's area; the session computes the page size). `LiveSnapshot.orientation` tells every window.
- **Quick switch** (live window, four icon buttons, `data-slot="orientation"`): `tablet_set_orientation` saves the
  setting and calls `TabletService::set_options`; the worker, on the same thread that forwards the notifications (so
  the records before the switch are oriented the old way, the ones after the new way), sends `LiveInput::Reoriented`.
  `LiveSession::reorient` then: ends the stroke in progress and the hover; **turns the current page with its ink**
  (page size swapped for a quarter turn, times and pressures untouched), so what is on screen stays readable; the
  shared view keeps its zoom, its centre is turned (a whole-page view stays whole); blank pages not written yet just
  take the new size; **other pages keep their ink and orientation until they are shown again** (`follow`, on page
  next / previous), when they are turned like the current one. So the pen's points, always in the current orientation,
  land on a page that has the same one. A page with ink only looked at is thus rewritten turned: accepted (it is
  rare and the ink is not lost). The turned page is saved at once (`urgent`). The switch also counts while live mode
  waits for the tablet (not connected, reconnecting): the worker forwards `Reoriented` to any live sink (the
  tablet's area when none is known yet), the session takes the position and its snapshot shows it; the `Started`
  that follows repeats the same position and turns nothing again. In the presentation, the marks written over it
  are removed when the page index, the page size or the orientation changes (a switch keeps the index).
- **Relation to `device.json`'s `orientation`**: `identity.rs` only has a test keeping an `"orientation": "left-handed"`
  field "from a newer version" in the record's `extra`; it is not a field of the record and nothing reads it. The
  setting lives in `ardoise.toml` ([tablet]), shared by the PCs, not with the tablet's registration.
- **Interface**: `TabletOptions` (Settings and tablet panel) lists the four positions as radio buttons with a picture
  (`OrientationIcon.svelte`, the tablet turned by quarter turns, button and light drawn) and a name that says where
  the button ends up. The stand-ins (`mock-tablet.ts`, `mock-live.ts`, `lib/orientation.ts`) follow the same rules.

## M10: review fixes (live mode)

- **Clear the page can be undone.** `LiveSession` keeps the strokes of the last cleared page (`Cleared`: page, orientation, strokes); `restore_cleared` puts them back under what was written since, only while that page is still on screen and held as it was (a quarter turn drops the offer). Hub: `restore_cleared`, command `live_restore_cleared` (returns whether the ink came back). The control window offers it for 8 s and withdraws it on a page change. M12: `Cleared` is gone, the clearing is a step of the page's history (below), and a quarter turn no longer drops it: the ink kept turns with the page.
- **A note changed from outside no longer duplicates the pages.** Each written page remembers a fingerprint of its ink and layout (`SavedFile::content`: hash of the `strokes.bin` encoding and of the layout JSON, not of title, tags, favourite or modification time). When the write meets a `Conflict`, or a `NotFound` whose note exists elsewhere under the same id (the file was renamed), `merge_outside` reads the file: if its fingerprint is the one last written, only metadata changed; the page is written into it with the outside title, tags and favourite mark kept, and the page adopts them (`Written::outside`) so that its next writes keep them. Any other change (ink edited elsewhere, another note under that id) still gives a new note next to it, as before.
- `saveFailed` events carry the library error's `code` next to the system's `message`.

## M12: resume a sheet, touch up during live

Decisions: `docs/questions.md` Q9, SPEC §4.2. "Live" paths are under `app/src-tauri/src/live/`. "Resume" below is built
and tested (packages 2 and 3, Rust side only), and so are "Paper" (package 5) and "Touch up, as built" (package 6: Rust, the
event, the share, `LiveState`). "Touch up, the selection" was written from reading the code on 2026-11-04, nothing run: check each
point against the code.

**Today.** Live starts only from the tablet panel: `live_start` → `LiveHub::start`, one blank page. Several pages per
session, one note per page, created at the first stroke's end, in the library root, then rewritten at most every 5 s and
when the page is left or live stops. The only resume is the same session after a lost link. After a crash the last 5 s
and the stroke in progress are lost.

**Resume, as built** (`resume.rs`, `session.rs`, `hub.rs`; tests in `resume/tests.rs`).
- **Start.** `live_resume(path)` → `LiveHub::resume`, then the tablet service as `live_start`. A note that cannot be read is
  refused first (`noteNotFound`, `notInLibrary`, `noteDamaged`...) and what runs goes on. Otherwise what runs ends as
  `live_stop` ends it (narration, tablet, `hub.stop`, `remember_live_session`), then the session shown is written out, and if
  that fails nothing starts. The note is read after that: it may be a page of that session. One sheet: page 0 is the note, the
  next page is a new note in the library root.
- **Data loss.** `write_page` built `NoteFile::new(job.note)`, which drops `original`, `original_note`, `background`, `text`
  and the unknown fields and members; `back_up_unsaved` did the same. A resumed page keeps the file as read
  (`ResumeBase`, in memory: a note deleted meanwhile must still give its image to the new note); `page_file` builds both from it,
  replacing only the note, the narration and the thumbnail (drawn with the image, decoded once, at the first write).
- **Unchanged.** A seeded page is not dirty, and `write_page` skips the disk when the fingerprint is the one on disk (clearing an
  empty sheet). What a resumed sheet writes is `written_sheet`: while its strokes are the ones it was read with, its layers and
  paper are too, whatever the session did around it (the blackboard pressed, the layer added when every layer is hidden, a new
  layer from the tablet's button, an edit undone), so a note that received no ink is not written at all. `Page::saved_file` hides
  the file until the session wrote it: `saved`, `live_stop` and the bridge's `last_live_note` (`LiveOverview.notes`) do not count a
  note left as it was.
- **Original.** `keep_resumed_original` runs at every write from the version read at the resume, not just at the first: the first
  write may change only the narration, and the original is taken at the first change of the ink. Unlike the editor's `keep_original`
  it keeps a blank sheet too: a page of a PDF or an image taken up to write on is the common case for a teacher, and its blank state
  is what Q9 says stays recoverable. A new note in the editor keeps the editor's rule. It never replaces an existing original.
  What the user gets from « Revenir à l'original », and the page size, layers, strokes and image placement come back: a note
  without original: the sheet before the resume, blank or not; a tablet note: the page as synced (`original.bin`); a note edited
  before: its first version (`original.ardoise`), older than the resume. An original of no stroke is an ordinary one for the editor
  (`revertToOriginal`; a note always has a layer, `format.rs`), the panel says « feuille vierge » (`helpBlank`). The paper stays what
  it is now, except slate over a first version on another paper: chalk needed it, and the chalk is going
  (`NoteOriginal::FirstVersion.paper`, `revertToOriginal`; the confirmation says so). The rule cannot tell that slate from one
  chosen in the editor: a deliberate slate goes too, which Ctrl+Z undoes.
- **Stamp.** `SavedFile` is seeded with the path, stamp and fingerprint read. A change elsewhere forks: a new note in the note's
  binder (`write_beside`; created again if it went with the note), with every member, the original, and a new id, and a
  `sheetForked` event (`reason: 'changed' | 'deleted'`, `original`, `path`), once, also to a window that subscribes later, never
  to viewers. The next writes go to the new note. Not a fork: only the title, tags or favourite mark changed (`merge_outside`,
  M10), or the file moved (followed). The fingerprint (`file_fingerprint`, all pages) now covers the narration and the members
  too: a narration replaced elsewhere was overwritten silently before. An ordinary page still forks to the root, silently.
  The last write happens as the view closes: `live_stop` also answers with the session's forks (`LiveStopped.forks`), the interface
  telling the user. A window that subscribes later is told only while the session runs: the view of the next lesson subscribes
  to the stopped session first, and must not open on a notice about the last one.
- **Id.** A fork gives the note a new id; the windows never see it (`Page::shown_id`, the id the page started with, in every
  snapshot). A window takes another id for another page: it would drop its marks and outlines, and a viewer would download the
  paper's picture again, with nothing changed on screen. `Page.note.id` is the real one: writes, `LiveOverview.notes`.
- **Coordinates.** Pen points are raw tablet units, already oriented (worker.rs, session.rs). `PenMapping` (computed at each
  notification from the session's state): a quarter turn when the sheet is held otherwise than the service orients the points,
  then a uniform scale and an offset, the tablet's whole area fitted in the sheet, centred (Q9 says "couvrir"; a fit reaches
  everywhere from the tablet except the margins, a cover would lose the tablet's edges). Same size, same way: exact, no rounding.
  Hover too. The line width is not scaled: the page's units are physical.
- **Orientation.** `hold` would turn a filed page, `follow` too when it is shown again, and `Page::turn` ignores
  `background_image`. On a resumed page `turn` only updates how it is to be held (`hold_for`): the setting when the sheet is the
  same way round (a square one always is), else a quarter turn on the same side (`landscape` ↔ `portraitLeft`, `landscapeFlipped` ↔
  `portraitRight`: the button stays away from the writing hand; our choice, one table to change). The service keeps orienting
  for the setting: the session corrects, so nothing changes on the tablet's side and the next page takes the setting at once.
  `snapshot.orientation` is the way to hold the current page, `snapshot.canTurnPage` is false on a resumed one. A quick switch
  that still arrives (settings) is safe: the ink stays, the pen follows. The view follows a page of another size (`fit_view`).
- **Time.** `t` is ms since `created` in a clamped u32, and `created` is the note's date (and file name): it stays. New samples
  count from a shifted origin (`Page::time_origin`, which `page_origins` gives to the narration): the resume's wall clock less
  `time_base`, which is after the last stroke (hidden ones too) **and after the end of the narration**, plus 1 s
  (`RESUME_GAP_MS`). The origin is fixed at the resume, not at the first stroke: a narration started before the first stroke must not
  land on the older sound. The wait until the first stroke is a pause in the replay, which its dead-time removal handles.
- **Narration.** `Page::audio` is seeded; `attach_narration` merges, but `Narration::merge` needed care: it assumes the same time
  base, gives the later clip alone when they are more than 4 hours apart, and the later one wins an overlap. So a narration
  recorded in a replay at another speed or with pauses trimmed is in that replay's time: `placed_for_merge` converts the start (and
  never over the older sound; aligned at its start only, a known limit), and `merged_keeping_both` moves a clip too far away right
  after the older one. This also holds on ordinary pages.
- **A sound that cannot join.** Two cases: together longer than a track (4 hours), or the page's own `audio.opus` cannot be decoded.
  The page's narration stays as it is and the new sound is never dropped (`LiveHub::keep_apart`): a new note next to the page's,
  holding the page as it is now with that sound and a new id. A note rather than a backup file: it is in the library, where the
  teacher looks, in time with its ink, and synced. If the library cannot be written, an `.opus` copy in `narration-backup`
  (`narration.rs`). Typed errors (`LiveError::NarrationKeptApart`, `NarrationLost`) and sentences saying where the sound is:
  `narrationTooLong`, `narrationUnreadable` (`message` = the path), `narrationNotKept`. They come back through `narration_stop`, which
  `LiveView` shows, and `LiveStopped.narration` for a recording stopped by `live_stop`. Other pages of the recording still get
  their pieces (`give_to_hub`), and pieces of one page too far apart for a track stay apart (`merge_by_page`). `live_start` and
  `live_resume` have no way to say it (a recording still running there is a race): `live-backup.log` only.
- **Layers.** New ink goes on the last visible layer. Every layer hidden: the page gets a new visible layer (named like any
  new one) when it is seeded, which is no change until something is written on it; none is shown (one may hold the answers).
- **Blackboard.** Each page keeps the paper to give back (`paper_before_board`). A slate sheet has slate as its own paper: turning
  the blackboard off keeps it. A resumed sheet starts with the blackboard on only if it is slate (with chalk), whatever the
  last session did; the style from before the blackboard carries over. Such a blackboard stays with the sheet: a plain
  `live_start` after it opens on paper with that earlier style (`carried_ink`, `board_imposed`); one that was asked for carries over,
  and gives back the ink it replaced. On a resumed sheet the page in memory shows slate while the file keeps its paper: slate is
  written only once ink was added under the blackboard (`Resumed.ink_on_board`, also on a sheet it was not turned on over), for
  white chalk, and until that ink is gone. A blackboard left on at « Finish » or turned off on another page changes nothing in the
  file; shown again with it off, the sheet has its paper back in memory too (`settle_paper`). Ordinary pages are only ever white or
  slate: same behaviour as before.
- **Editor.** Never mounted with live (Home.svelte): the editor saves and closes first (package 4). A draft left for the note (a
  crash) keeps the older stamp: once live mode wrote the note, `recoveryOf` (`lib/editor/drafts.ts`) offers it only as a new
  note. Nothing to change; tested on the Rust side.
- **Viewers.** `sheetForked` and `canTurnPage` are not in the allow list (feed.rs). `created` and `modified` are now always 0 there:
  a resumed note would give its real date; nothing in the viewer reads them. The page's `id` there is a digest, salted at each run
  (`viewer_page_id`): one per page, all the viewer's page-change detection needs, without the identity of a note of the library.

**Session end.** Windows ending the session while live runs (shutdown, an update's restart, logoff, Restart Manager;
`session_end.rs`, `live.rs`, `narration.rs`). Before: nothing was written, the editor alone reported unsaved work, and tao's hidden
window calls `exit(0)` on `WM_ENDSESSION`, so the unattached narration and up to 5 s of ink were lost.
- **Reported.** While live runs (`Activity::live`, read in the window procedure without a lock) `WM_QUERYENDSESSION` answers FALSE
  unless critical, and the main window has a block reason, in the interface's language, from Rust (`live_reason`, like the layer
  names: live may run with no view); the hub tells of the transitions (`LiveHub::watch_running`). The editor's reason goes first. The
  query also writes the pages now, without stopping anything (`write_now`): the user may refuse.
- **Closed.** The first `WM_ENDSESSION` (`wParam` TRUE) to arrive runs `session_ending` on a thread of its own and waits at most
  `SESSION_END_BUDGET` (4 s; Windows allows about 5, assumed, not measured). Tao's window is subclassed too, by class name
  (`Tao Thread Event Target`, tao 0.37.2): nothing says the main window hears first, and the subclass runs before tao's procedure.
- **Order** (`close_at_session_end`), what loses least if the process ends halfway: the pages (stroke in progress included, a backup
  copy for what the library refuses), then a WAV copy of the recording under way (a plain write, 345 MB per hour: encoding hours of
  sound takes far longer than the budget), then its pages as usual, written again, and the copy removed. Last the tablet.
- **Tested** without a shutdown: the order, the copy, the budget (`run_within`), and the window procedure on real hidden windows with
  messages sent as Windows sends them (answers, critical, the end called off, once for two windows). **Not tested**: a real shutdown,
  how long Windows really waits, in which order it tells the windows, and that the class name still matches after an upgrade of tao
  (the main window's route remains, should tao's window be told second). A WAV copy left by an end that came too soon is named in
  `live-backup.log`; nothing offers it at the next start.

**Paper, as built.** Ordinary pages are plain paper and pay nothing: the same `render_background` as before, no key, no
message, no thread.
- **Windows.** `LiveState.pageImage` is the snapshot's `backgroundImage`. With one, `LiveCanvas` asks `live_page_background`
  instead of `render_background`, with the same arguments (the paper as drawn: slate on the blackboard, as the editor draws
  slate paper; the width of the zoom's bucket) plus the page's index. Not `stage_note_background`: the staging area is the
  editor's, which empties it when it closes, whichever window draws. Rust takes the hub's lock once (`LiveHub::page_image`:
  the image's place and an `Arc` of the file, no pixels), then decodes (`ResumeBase::picture`, once per session, shared with
  the thumbnails) and renders on a blocking thread. The window decodes the picture (`HTMLImageElement.decode`) before
  drawing it, then only the cache is redrawn with it, as for any paper: a frame of the pen copies the cache, which holds it.
- **Alignment.** The picture is the whole page, `drawImage` under the page's transform, the one the strokes use, at any zoom
  and in both windows; Rust places the image in it (`rect`, `quarter_turns`, `opacity`) as for the editor and the exports. A
  page's picture is stretched to the page, as the editor's `<image preserveAspectRatio="none">`. Tested: `LiveCanvas` gives
  the renderer that scene in both modes (vitest), and `background_png` under ink gives, pixel by pixel, `render_note`'s
  picture of the same note (Rust, a turned image reaching outside the page included).
- **Viewers.** [lan-share.md](lan-share.md), "The paper of a live page": the snapshot names the paper (`paperKey`, absent on
  plain paper), a `livePaper` message outside the batches brings the picture, once per viewer. The hub's flusher only
  compares; rendering is on a thread of its own. Hidden layers: `SharedSnapshot` filters them on a resumed page too, and the
  picture is made without any stroke.

**Touch up, as built** (`edits.rs`, `session.rs`, `hub.rs`, `share/feed.rs`, `lib/live-edits.ts`, `lib/live-state.svelte.ts`;
tests in `edits/tests.rs`, `resume/tests.rs`, `share/tests.rs`, `live-state.edits.test.ts`, `mock-live.edits.test.ts`).
- **Identity.** Strokes have no ids, only their place in the page's list of finished strokes, stable under appends, shifted by
  a delete. No format change: a revision says what the places mean.
- **Revision.** Per page, in the snapshot (`revision`) and in every `strokesEdited`. It changes when strokes leave the list or
  come back: a delete, a clearing that removed something, the undo or redo of either. Not when the tablet appends (a selection
  stays valid while it writes), not for a change of colour or its undo (a second click on a swatch is not stale), not for a
  page turn or a new layer (no place moves). Two sessions never share one (`session id << 32`, plus the changes of the page):
  a command meant for a session that ended is stale, though the page number is the same.
- **Commands.** `live_edit_strokes(pageIndex, revision, edit)`, `live_undo_edit(pageIndex)`, `live_redo_edit(pageIndex)`; a stopped
  session refuses them (`liveNotRunning`, as `clearPage`). `liveEditStale`: another page on screen, or another revision; the
  interface drops its selection. `liveEditInvalid`, refused as a whole: an empty list, more than `MAX_EDIT_STROKES` (2^20), a
  duplicate, a number that is no index, an index out of range, a stroke of a hidden layer (none is shown, so it was not chosen
  on purpose), a colour out of 0..=255 or fully transparent. Indices and channels arrive as plain numbers so that a bad one gets
  this code rather than Tauri's rejection of the arguments. A colour the strokes already have is no step and no event.
- **Event.** `strokesEdited { pageIndex, revision, removed, inserted, recoloured, canUndo, canRedo }`, applied in that order:
  `removed` in the list before it, `inserted` (`{ index, stroke }`) and `recoloured` (`{ color, indices }`, one per colour) in the
  list after it. Never coalesced (the default arm of `enqueue`), in `may_need_save` so that the saver wakes. Not a snapshot: a
  snapshot clones the page once under the hub's lock (`LiveEvent::Snapshot` shares it between the queues, `Arc`, same JSON), and
  a changed snapshot makes `applySnapshot` recompute every outline. `clearPage` stays a snapshot: the page is empty by then, and it also drops the stroke in progress. `canUndo` and
  `canRedo` are also in the snapshot, so the interface always knows them.
- **Under the lock.** Sorting and the checks that need no page are done before it (`StrokesEdit::checked`). A delete moves
  the structs after the first place (`extract_if`), no point is copied, and the strokes go to the history. An undo gives them back
  through a copy made before the lock is taken (`Prepared`: the hub peeks at the top step, copies, then applies; another command
  in between just means a copy under the lock), and each window gets the stored strokes behind one `Arc` instead of its own copy.
- **In progress.** The tablet's stroke in progress is untouched by an edit, undo or redo: it joins at the tail at `end_stroke`. It
  cannot be named: it is not in the list.
- **History** (`edits::History`, per page). `MAX_UNDO_STEPS` (100) and `MAX_UNDO_BYTES` (64 MB of kept strokes per page); the oldest
  goes first, except the last step left, which holds no more than the page held (« Clear » of a very long lesson can be undone).
  A place in a step is the place the stroke had, never translated: steps come off in reverse order, so what stands before it is what
  stood there, and the tablet only adds at the tail, which stays after the places given back. The tablet's writing is not a step and
  does not empty the redo steps; a new edit does.
- **Clear.** `clearPage` removes what shows and pushes a step; hidden layers keep their ink (nobody can tell it is going; before
  M12 it went too). `live_restore_cleared` undoes down to and including the page's last clearing: the button keeps working, and
  edits made after the clearing can still be redone. On another page: `false`, and the clearing stays on its page.
- **Page turn.** The strokes kept turn with the page (`History::turn`, called from `Page::turn`): they come back where they would
  be had they stayed. Places and colours do not change, so nothing else does. A page emptied by a delete and never written is not a
  "blank" page for `hold` while its history is not empty. A resumed sheet is never turned, nor what can come back to it.
- **Page change.** A page keeps its history when it is left: Ctrl+Z on it again undoes what was done on it. `pageIndex` says which
  page the key was pressed on. A stop drops every history (`set_state(Idle)`).
- **Resumed page.** Edits may touch the note's old strokes. Written like any change: `page_file`, so members, `keep_original` and
  stamps as before; the fingerprint (`strokes.bin`, which holds the colour) sees a delete and a recolour. A sheet whose only change
  was undone before the saver wrote it is not written (same fingerprint: same bytes, same date, not among the session's notes). The
  first change of a resumed sheet is written at once (`page_due`: never tried), so more often it was written: it is then rewritten
  once more, as it was read, with no `original.ardoise` since nothing differs, and it counts among the session's notes.
- **Viewers.** The event goes through the allow list with `pageIndex`, `removed`, `inserted`, `recoloured` only. Their list has no
  hidden strokes, so their places differ on a page with a hidden layer: indices are **translated, not replaced by a snapshot** (a
  snapshot per viewer is what the event avoids, and it would have to be placed in the queue under the lock). The session works out
  `ViewerIndices` with the edit, a pass over the list only on such a page (`place - hidden strokes before it`); `feed.rs` picks them.
  An edit never touches a hidden stroke, so none can leave in `inserted`, and whether a stroke is hidden never changes during a
  session. A paused share has no subscription: its frozen image, then a fresh snapshot on resume.
- **`LiveState`.** `revision`, `canUndo`, `canRedo` (reactive), `strokes` changed in place (`lib/live-edits.ts`), `generation` + 1.
  Strokes not named keep their object, hence their outline in the renderer's `WeakMap`; a recoloured one too (a new `color` on the
  same object: `Ink.contours` depends on the points, tool and width only). On a viewer `revision` stays 0.
- **Renderer.** A new `generation` first looked for in `LiveState.edits` (what each of the last 32 edits touched, removed strokes
  included) repaints the area of those strokes on the cache; a change of `generation` not found there (a snapshot, a window that
  did not paint for 32 edits) redraws everything, as does a change of the view, the size, the background or the layers. Same order
  (`drawingOrder`, which skips hidden layers), same looks (highlighter multiplied or screened, felt tip 0.9, chalk 0.95), each
  stroke's colour read at that moment; the grains of pencil and chalk are not drawn on screen.
- **Repainting an area** (`LiveRenderer.planRepaint`, `repaintArea`). The area is the box of the *outlines* of the strokes touched
  (their geometry, so width and tool are in it; no texture on screen), in device pixels, plus 2 pixels for antialiasing, rounded
  outward to whole pixels and cut to the canvas. Under a clip on that box, what a full redraw does (`drawBackground`: clear,
  surround, paper, picture) then every stroke whose outline's box comes within 2 pixels of it, in drawing order. A whole-pixel clip
  replaces each pixel entirely, and no other stroke touches it, so each pixel goes through the same operations as in a full redraw:
  same value. Outlines stay cached per stroke object, with their box; a recolour computes none. A stroke that came back by an undo
  is a new object and gets its outline again.
  - **Strokes finished meanwhile** (not on the cache, `Outline.onCache`, and not touched) stay out of the area and are appended
    after it, whole. That is right only if they are last in the drawing order: if a stroke on the cache comes after one of them (a
    higher layer brought back by an undo), everything is redrawn. A stroke finished and touched in the same frame is simply in the area.
  - **Full redraw instead**: an area over 60 % of the canvas, or over 60 % of the strokes crossing it (more than 16). The
    union is one box: two strokes in opposite corners are a full redraw. No spatial index: the pass over the strokes is one box test each,
    well under a millisecond for 4,000 strokes.
  - **Tested** in pixels on a software canvas (`live-raster-canvas.ts`, `live-render.partial.test.ts`: the listed cases and 600 random
    frames against a full redraw of the same state; a clip off the pixel grid throws) and on real canvases, in a real WebView2 and a
    real Edge (`Places.samePixels` in `journeys-m12-touchup.mjs`, with `canvas.repaintNow()` and `canvas.repaintFromScratch()`). On real
    canvases a few edge pixels of a stroke cut by the clip come out a little differently from the graphics stack (seen: 9 pixels of 5.5
    million, by up to 18/255), hence the journeys' tolerance: `EDGE_PIXELS` pixels by `EDGE_DELTA` at most. They stay until the next full redraw.
- **Cost.** Page of 50 000 strokes of 100 points, measured in the unoptimised test build (`cargo test -p ardoise large_page --
  --nocapture`; the app is much faster): under the lock, delete 1 000 strokes 2.4 ms, undo them 2.8 ms, recolour all 22 ms, clear 13 ms,
  against 58 ms for the clone of a single snapshot, once per snapshot whatever the number of windows since `LiveEvent::Snapshot` is an
  `Arc`. In the window (`live-state.edits.test.ts`), 1 to 3 ms per event.
  A full redraw of the cache stays costly (one fill per stroke of the page, on the window's thread, as after any change of view,
  never under the hub's lock); an edit no longer asks for one (`cargo xtask perf --only touchup` tells both). Also under the lock, as before: the clone of
  the page by a save pass (`SaveJob`) every 5 s, less often when writes are slow (`save_wait`), which an edit now triggers too.

**Touch up, the selection, as built** (package 7; `lib/live-select.svelte.ts`, `lib/live-touch-up.ts`, `lib/live-keys.ts`
`liveEditKeyAction`, `LiveCanvas`, `live-render`, `LiveView`).
- **Reused.** `selectInLasso` and `hitStroke` of `lib/editor/hit.ts` (typed `Hittable = Pick<Note, 'strokes' | 'layers'>`), through
  `pickStrokes` (a gesture under 4 screen pixels is a click: ink within 6 px, as in the editor; else the lasso). Shift adds, Ctrl
  removes, a gesture on nothing clears, as in the editor. Hidden layers are never picked. The stroke in progress is not in
  `LiveState.strokes`, so it cannot be.
- **Selection.** `LiveSelection` (control window only): `active` (the Select toggle), `indices` (a new array at each change) and the
  `revision`, `noteId`, `pageIndex` they were chosen at. `follow(live)` (an effect of `LiveView`) drops them when the page or the
  revision changes (a delete, an undo, « Clear the page »); the tablet's strokes and a recolour keep them. Turning the tool off clears.
  Nothing of it is sent to Rust, the projector or the share.
- **Drawing.** `Scene.selection` and `Scene.lasso` (always empty for `presentation`). The renderer draws the highlight on a surface
  of its own, redrawn only when the selection, the strokes' generation, the layers, the view or the size change; a frame copies it.
  The page cache is never redrawn for a selection (`PaintReport.selectionDrawn`). The highlight: a faint fill of the box, a blue ring
  (the stroke's outline made wider by the ink module, 3.5 px on each side where the pen pressed hardest, opaque), a band of paper colour
  inside it (1.5 px) which keeps blue ink apart from the ring, then all of that taken out (`destination-out`) where the ink is, 0.5 px
  past its edge, and the dashed box. The page below shows through the hole, so that the strokes keep their real colour (a recolour
  shows at once, a highlighter its blend); the ink is never drawn twice. Fills, one path per stroke, their outlines kept per stroke and
  scale: on 100 strokes in a real WebView2, one path holding the whole selection takes 70 to 90 ms of calls and two frames are late, and
  `stroke()` makes one frame very late (some 90 ms in the graphics process) on strokes it has not seen yet. The lasso being drawn is a polyline per frame. Colours: `SELECTION_COLORS` (a lighter blue on dark paper: slate
  or blackboard).
- **Pointers.** `LiveCanvas` `selecting`: mouse and pen draw the lasso instead of dragging the area; finger (pan, pinch, double
  tap) and wheel stay navigation. The check runs once, on release: 24 000 strokes of 30 samples take about 0.4 s the first time (bounds
  are cached per stroke object) and 0.15 s after; a click takes about 10 ms.
- **Actions.** `lib/live-touch-up.ts`: delete, recolour (keeps the selection), undo, redo, with the selection's revision; a
  `liveEditStale` drops the selection and every refusal shows its `errorCodes.*` sentence. A swatch with a selection recolours it and
  leaves the pen's colour alone (unlike the editor: a correction made in front of the class must not change the ink under the
  pen); without one it only sets the pen.
- **Keys** (control window only, `liveEditKeyAction`, tried before `liveKeyAction`; the presentation window's mapping is unchanged):
  V toggles Select (the editor's key); Ctrl+Z undoes, Ctrl+Y and Ctrl+Shift+Z redo (only when the session says there is a step);
  Delete and Backspace delete the selection, else clear the marks; Esc deselects, else closes the projection or asks to leave.

**Work packages**, in order. Packages 2, 3 and 6 share `session.rs` and `hub.rs`: one after the other.

| # | Content | Main files | Difficulty |
|---|---|---|---|
| 1 | The stroke counted twice (test first) | `live-state` | standard |
| 2 | Seed a session from a note; keep every member; original; stamp; fork in the binder with a notice | session, hub, live.rs, live/tests.rs | hard |
| 3 | Scale, orientation, time, narration, visible layer, blackboard's paper | session, narration.rs | hard |
| 4 | Entry points: note menu, editor toolbar | Home, NoteCard, Editor, api, mocks | standard |
| 5 | Paper in LiveCanvas and for viewers | LiveCanvas, share.rs, feed.rs, viewer-state | hard |
| 6 | Edits and undo in Rust, event, feed, TS apply | edits, session, hub, model, feed.rs, live.ts, live-edits, live-state | hard |
| 7 | Selection UI in the control window, overlay, keys | new `lib/live-selection`, LiveCanvas, live-render, LiveView, live-keys | standard |
| 8 | e2e journeys, guide, H5, perf run | app/e2e, docs | standard |
