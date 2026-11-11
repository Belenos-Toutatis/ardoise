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

## M14: correct during live on one's own layer

Decisions: `docs/questions.md` Q11 (2026-11-11), SPEC §4.2 « Corriger pendant le direct », §4.4 « Calques », §4.6
« Calques à l'export »; ROADMAP M14 and H6. The layers zone follows the mockup the user validated
(`target/maquettes/maquette-calques.html`, not in git: its script is the reference for the zone's behaviour). Exports:
[exports-and-operations.md](exports-and-operations.md), "Layers at export (M14)". Written on 2026-11-11 from reading the
code, nothing built or run: every "today" cites where it was read; "(supposé)" marks what was not checked.

**Today.**
- The tablet's layer is not stored: `LiveSession::current_layer` (session.rs) is the current page's last visible layer,
  read at each pen down (`pen_down`) and by every snapshot (`LiveSnapshot.layer`). `LiveCommandKind::NewLayer` (the
  toolbar's `data-action="new-layer"` in `LiveView.svelte`, and the tablet's button set to « nouveau calque ») ends the
  tablet's stroke and adds a layer on top, which thereby receives the tablet's ink.
- The control window writes nothing: its preview drags the projected area, the Select tool (M12) picks strokes of every
  visible layer (`LiveCanvas.endLasso` passes `{ strokes: live.strokes, layers: live.layers }` to `pickStrokes`), and
  the eraser end is ignored there (`LiveCanvas.onpointerdown`: `kind === 'eraser'` returns).
- One stroke in progress at a time everywhere: `LiveSession.drawing: Option<Drawing>`; `strokeStart`, `points` and
  `strokeEnd` name no pen; `hub::enqueue` merges `points` into the queue's last `strokeStart`/`points` of the same page;
  `LiveState.apply` finishes the stroke in progress when a `strokeStart` arrives. Two pens at once would mix their
  points in all three places.
- The page's history (`edits::History`, M12) holds edits only (delete, recolour, clear); appended ink is never a step.
- A layer's visibility never changes during a session, and the share relies on it (M12 "Viewers": `ViewerIndices`).
- Editor: `LayersPanel.svelte` (a radio for the active layer, a checkbox to show, a name field, ↑ ↓ ⤓ on every row);
  the lasso, the click, Ctrl+A and the eraser take every visible layer (`lib/editor/hit.ts` `selectInLasso`,
  `hitStroke`, `strokesTouched`; `state.ts` `selectAll`, `erase`); hiding the active layer moves it to the nearest
  visible one (`state.ts` `setLayerVisible` → `nearestVisibleLayer`).

**Words.** The **tablet layer** of a page is where the tablet's ink goes. The **computer layer** ("le calque choisi" in
the interface) is where the computer's pen, its eraser end and the selection act. Both are per page, in Rust, in memory
only: a note has no active layer in its file. A **computer stroke** is written in the control window with the computer's
pen or mouse (Pen tool); in the note it is an ordinary `Stroke`. No format change: which pen wrote a stroke is not stored.

### Data model (package B)

`Page` (session.rs) gets `tablet_layer: u32` and `computer_layer: u32`, always indices of `note.layers` (a page always
has a layer: `Page::resumed`, `format.rs`). `current_layer` goes.

| When | Tablet layer | Computer layer |
|---|---|---|
| Start (`LiveSession::new`: one blank page, « Calque 1 ») | 0 | 0: on the tablet's layer (Q11) |
| New page (`NextPage` past the last, `NewPage`, the tablet's button set to « nouvelle page ») | 0 | 0 |
| Resumed sheet (`Page::resumed`) | the last visible layer (M12; every layer hidden: the visible layer M12 adds) | the tablet's |
| Tablet's button set to « nouveau calque », or `live_command` `newLayer` | the new top layer | unchanged |
| `live_layers` `add` (« Ajouter un calque ») | unchanged | the new top layer |
| `live_layers` `tablet` / `computer` | the layer named | the layer named |
| `live_layers` `move`, `merge` | follows | follows |
| `live_layers` `rename`, `visible` | unchanged | unchanged |

- Following, as the mockup's `follow` and the editor's `ops.moveIndex`: moving `from` to `to`, an index equal to `from`
  becomes `to`; with `from < to`, indices in `from + 1 ..= to` go down by one; with `from > to`, indices in `to .. from`
  go up by one. Merging layer `i` into `i - 1`: indices `>= i` go down by one (`i` becomes `i - 1`).
- A page keeps its two layers when it is left; coming back finds them as they were.
- `LiveSnapshot.layer` keeps its name and becomes "the tablet's layer" (what it meant for every reader so far, since only
  the tablet wrote). New `LiveSnapshot.computer_layer` (`computerLayer`). The share's `SharedSnapshot` (feed.rs) lists
  its fields one by one: `layer` stays, `computerLayer` is not added.
- **The tablet on a hidden layer** is allowed (Q11 default: a warning, no refusal). Its ink is recorded there as usual;
  the presentation does not draw it (`LiveRenderer` draws visible layers only, `visible`), the share does not send it
  (below, `hidden`). Hiding the tablet's layer does not move the tablet: `current_layer`'s "never a hidden one" goes.
- **The computer on a hidden layer** is allowed as a choice (warning); its pen is refused (`liveLayerHidden`), its eraser
  and the selection find nothing there (hidden strokes are never hit).
- **A resumed sheet** (`hub::written_sheet`): today a sheet whose strokes did not change is written with the layers it
  was read with, so that a layer the session added by itself (all hidden at the seed, the tablet's button) writes
  nothing. M14 adds `Resumed.layers_edited`, set by every `live_layers` op that changes the note (`add`, `rename`,
  `visible`, `move`, `merge`; not `tablet` nor `computer`): from then on the sheet's layers are written as they are,
  new ink or not. A deliberate act on the layers is a change of the sheet, as in the editor.

### Layer operations during live (package B)

Command `live_layers(pageIndex, op) -> ()`. `LayerOp` (new `live/layers.rs`), mirrored by `LiveLayerOp` (live.ts):
`add`, `rename { layer, name }`, `visible { layer, visible }`, `move { from, to }`, `merge { layer }` (into
`layer - 1`), `computer { layer }`, `tablet { layer }`. Indices arrive as plain numbers (`f64`) and are checked in
`live/layers.rs`, as M12's `StrokesEdit` does, so that a bad one gets a coded error rather than Tauri's rejection.

- Refused as a whole, nothing changed: `liveEditStale` when `pageIndex` is not the page on screen; `liveLayerInvalid`
  for a layer that does not exist or a number that is no index, `move` outside the list or onto itself, `merge` of the
  bottom layer, a name empty once trimmed or longer than `MAX_LAYER_NAME` (200 characters), `add` beyond `MAX_LAYERS`
  (100; our bound: the tablet's button does nothing beyond it either); `liveNotRunning` once the session is stopped,
  for every op but `tablet` and `computer`, which change no note (as `newLayer` and `clearPage`: `LiveHub::command`,
  its `rewrites`).
- Done under the hub's lock, then one snapshot, as `NewLayer` today. A layer op is a click; the snapshot's clone of the
  page is the cost M12 measured (58 ms for 50 000 strokes of 100 points in the test build, a few ms for a lesson).
  - `add`: a layer `layer_name(french, len + 1)` on top, visible; the computer goes there; `page.touch()`.
  - `rename`: the trimmed name (the same: no change, no event). `visible`: as asked (the same: nothing).
  - `move`: the list reordered; every stroke's `layer` remapped (places unchanged: the revision and a selection stay);
    the strokes kept in the page's history remapped too (`History::remap_layers`, like `History::turn` for a quarter
    turn); both strokes in progress (`Drawing.layer`) remapped; both indices followed.
  - `merge` of `i` into `i - 1`, as the editor's `ops.mergeDown` (the same result in both places): the lower layer stays,
    with its name, visible if either was (as in the editor: merging into a hidden layer shows its ink); the strokes of
    `i` take `i - 1` and move after the others in the list, so that they stay drawn above the lower layer's ink (drawing
    order is layer, then list); the layers above shift down. Places move: the revision goes up (a selection is dropped)
    and **the page's undo and redo steps are dropped**: their places would name other strokes, and a redo could delete
    the pupil's ink. The strokes in progress are not in the list: only their layer follows.
  - `tablet`, `computer`: the index; no `touch()` (the saver wakes on the snapshot and finds nothing to write).
- No op ends a stroke in progress (unlike `NewLayer`, which still ends the tablet's).
- **Not undoable with Ctrl+Z.** Ctrl+Z in front of the class undoes ink actions (Q11 lists strokes, eraser, touch-ups,
  clearing). Undoing « Ajouter un calque » after the tablet wrote on it would have to move or delete the pupil's ink;
  show, hide, rename and move are undone by the same click in the zone; merge loses no ink. The editor keeps undoing its
  own layer ops (they are part of its doc).

### The computer's strokes

**From the window to the session** (B and C). One command, always called through the window's queue (below):
`live_computer_pen(input) -> bool`. `ComputerPenInput` (new `live/pen.rs`), mirrored by `LiveComputerPenInput`:

```ts
interface ComputerSample { x: number; y: number; p: number; ms: number }
type LiveComputerPenInput =
  | { kind: 'begin'; stroke: number; pageIndex: number; style: LiveStyle; samples: ComputerSample[]; now: number }
  | { kind: 'points'; stroke: number; samples: ComputerSample[]; now: number }
  | { kind: 'end'; stroke: number; samples: ComputerSample[]; now: number }
  | { kind: 'cancel'; stroke: number };
```

- `stroke`: the window's number for the gesture (a counter from 1). Rust ignores a call for another number than the
  computer stroke in progress; a `begin` while one is in progress ends that one first (kept).
- `x`, `y`: page units of the page shown, from the preview's transform (`LiveCanvas.pagePoint`); on a resumed sheet its
  own units (`PenMapping` concerns the tablet only). `p`: `inkPressure(kind, e.pressure)` (gestures.ts): the pen's
  pressure, 0.5 for a pressed mouse, as in the editor. `ms`: the event's `timeStamp`, coalesced events included; `now`:
  `performance.now()` when the call is made (the same clock). The window drops a sample closer than 0.5 screen pixel to
  the previous one (as `Canvas.svelte` and the marks).
- `style`: the computer's ink (`lib/live-computer-style.ts`, below), `LiveStyle::sanitized` by Rust. The layer is Rust's
  `computer_layer` of the page: the window's queue makes a choice of layer arrive before the pen that follows it.
- Answer. `begin`: `true`, or `liveNotRunning` (never started, or stopped: `LiveHub::change` with `rewrites` true),
  `liveEditStale` (`pageIndex` is not the page on screen), `liveLayerHidden` (the computer layer is hidden),
  `liveEditInvalid` (no sample with finite coordinates, more than `MAX_PEN_SAMPLES` = 4096 samples in one call).
  `points`, `end`: `true` while the stroke goes on; `false` once the session ended or dropped it (page change, quarter
  turn, « Clear the page », stop, a newer stroke, `MAX_COMPUTER_POINTS` = 100 000 points reached): the window stops
  sending for that gesture. `cancel`: whether there was a stroke to drop.
- Checked before the lock (`LiveHub::computer_pen`, as `StrokesEdit::checked`): samples without finite `x`/`y` dropped,
  `p` clamped to 0..=1 (non-finite: 1); under it, the coordinates clamped within one page size around the page (the page
  is the session's), as `sanitize_marks` does for the marks.

**Time.** Each sample becomes a wall-clock time (`SystemTime`, the clock of the tablet's stamps: `wall_ms(received)`
in `Shared::input`): `wall = wall_now − max(0, now − ms)`, then at least the previous sample's of the stroke, at most
`wall_now`, at least `wall_now − 10 000`. Anchored at each call, so the window's clock and Rust's never need to agree
(only a call's IPC delay, about a millisecond, is not counted). Then `t` as `point_time` computes it, from the same
origin: `note.created` (a fresh page takes the first sample of either pen as its `created` and stops being fresh) or, on
a resumed sheet, `Resumed.origin` (M12 "Time"); floored, clamped to `u32`, never below the stroke's previous `t`. Never
through the tablet's `Page.last_t`, whose 5 ms spacing is the tablet's rate: neither pen pushes the other's times. Equal
times within a stroke are allowed (`format/strokes.rs` `apply_delta` accepts a zero delta; `replay.rs`: a sample never
shows before the one it follows). The replay needs nothing new: `Timeline` already plays overlapping strokes point by
point in note time (`crates/ardoise-media/src/replay.rs`, "when several strokes overlap in time"), so the computer's
strokes appear at their time among the pupil's (Q11 default). `LivePoint.recv` of a computer point is that sample's
wall time: the windows' latency figures then count from the pen's movement.

**In the session** (B). `LiveSession.computer: Option<ComputerDrawing>` beside the tablet's `drawing`: what `Drawing`
holds plus the window's number and the last wall time. Each call appends and emits at once (`strokeStart` with the first
samples, then `points`). When it ends, the stroke goes at the tail of `note.strokes` (as `end_stroke` does), with
`page.touch()`, `urgent` when the page has no file yet (a page's first stroke creates its note, whichever pen),
`Resumed.ink_on_board` set as for the tablet (any ink written under the blackboard), an `Added` undo step (below), then
`strokeEnd` and a `history` event.

- Ended (kept) by: `end`; a page change (`NextPage`, `PreviousPage`, `NewPage`, also from the tablet's button); a
  quarter turn (`hold`, before the page turns, so that it turns with the ink); `set_state(Idle)` (stop, the service
  stopped, the end of the app: `close_at_session_end` then writes it); a `begin` with another number. Dropped
  (`strokeCancel`) by: `cancel`; « Clear the page » (which drops the tablet's too, as today).
- A lost link (`Waiting`, `Reconnecting`) ends the tablet's stroke, not the computer's. The computer writes whenever the
  session runs (`is_running`: started and not stopped), the tablet connected or not.
- `in_progress()` returns 0, 1 or 2 events (the tablet's first): `push_snapshot`, `subscribe` and `current_events` follow
  the snapshot with both. Each function that ended "the stroke in progress" says which one it ends.

**Events** (model.rs, live.ts). `strokeStart`, `points` and `strokeEnd` get `pen` (`LivePen`: `tablet` | `computer`,
written only for `computer`: the tablet's events stay byte for byte as they are, for every test and every viewer) and
`id` (the window's number, computer only). New: `strokeCancel { pageIndex, pen, id }` and
`history { pageIndex, canUndo, canRedo }`. The four stroke events also carry `hidden`, never serialised
(`#[serde(skip)]`): the stroke's layer was hidden when the event was made.

- `hub::enqueue` merges `points` only into a `strokeStart`/`points` of the same page **and the same pen**;
  `strokeCancel` and `history` are never merged (the default arm). `may_need_save` needs nothing new.
- Every window gets every event. The share (`share/feed.rs`, allow list): `strokeStart`, `points`, `strokeEnd` with
  `pen` (not `id`), `strokeCancel` with `pageIndex` and `pen`; all four left out when `hidden`; `history` left out (like
  `revision`). A stroke on a hidden layer, the tablet's or the computer's, never reaches a viewer, whose list keeps
  having no hidden stroke.
- `hidden` is computed when the event is made. A change of visibility sends a snapshot followed by the strokes in
  progress, `hidden` computed again: a viewer drops its strokes in progress at a snapshot (`LiveState.applySnapshot`)
  and gets one back only if its layer shows.

**Saving**: as for the tablet's ink (hub.rs, unchanged): dirty when a stroke ends, written at most every 5 s
(`save_wait`), at once when the page has no file yet, when it is left, at `live_stop`.

**Windows** (C).
- `LiveState`: `computerCurrent: Stroke | null` beside `current`; events with `pen: 'computer'` go there, the others
  where they go today; `strokeCancel` drops it; a snapshot drops both (the `strokeStart`s that follow bring them back);
  `strokeCount` counts both; `computerLayer = snapshot.computerLayer ?? snapshot.layer`; `history` sets `canUndo` and
  `canRedo` for the page shown; `lastComputerDone`: the `id` of the last computer `strokeEnd` or `strokeCancel`.
  Latency: `pending` keeps the tablet's points only (the control window's `[data-latency-median]` stays the tablet's);
  computer points go to their own list, `takeUnpaintedComputer()`. `live-state.svelte.ts` keeps no runtime import of
  `api.ts` (the viewer runs it: `viewer/imports.test.ts`).
- `live-render.ts`: `LiveSource.computerCurrent?`, `Scene.computerCurrent`, drawn after the cache like `current` and only
  when its layer shows; the two in their layers' order (the lower first; on one layer, the tablet's first);
  `PaintReport.computerPoints`. `SceneOptions.localComputer?: Stroke | null`, when given, is drawn instead of
  `live.computerCurrent`.
- Control window: the stroke is drawn from its own pointer events (`localComputer`, as the editor draws its stroke in
  progress: no round trip), and Rust's copy is not drawn meanwhile. The local stroke goes when `lastComputerDone`
  reaches its number (the finished stroke is then in `strokes`, on the cache in the same frame), when the page or the
  note id changes, or 2 s after the window sent `end` or `cancel` (a lost answer).
- Presentation window: draws `computerCurrent` from the events (no local ink there; its marks are unchanged) and records
  the latency of computer points in a second `LatencyRecorder`: `window.__ardoiseLatency.computer` (`samples()`,
  `median()`, `p95()`; `lib/latency.ts` `exposeLatency`).
- Viewer: `ViewerCanvas.svelte` builds its `LiveSource` field by field: it adds `computerCurrent`. Nothing else.

**Latency targets.** Projector: as the tablet's ink, a median under 30 ms from the pen's sample to the presentation's
frame (SPEC §4.2's figure, measured by `__ardoiseLatency.computer`, `cargo xtask perf --only pen`). Share: under 150 ms
to the other screen, stroke in progress included (SPEC §4.10); the e2e journey asserts a median under 50 ms from `recv`
to the SSE message read by Node, as M11 does.

**The tablet is never slowed** (hub.rs, module documentation: nothing slow under the lock). Checks outside the lock;
under it, at most 4096 samples appended and their events pushed (the points are copied into that one event, as
`flush_points` does); a layer op is one snapshot, like a page change; `merge` moves strokes, no point is copied; nothing
waits for a window or the disk. The window keeps one call in flight. Measured by a hub test: the tablet's input on one
thread while another sends computer strokes (one call every 4 ms, 8 samples each); it prints the median and p99 of the
tablet's Rust part (`sentAt − recv`) and asserts the p99 under 20 ms in the test build (supposé: a loose bound, to set
from the first run). As built (`live/tests.rs` `the_tablet_is_never_slowed_by_the_computer_s_pen`, 2026-11-11, test
build, other builds running): p99 4.5 to 4.9 ms, max 5.3 ms, median 2.0 to 2.2 ms (about 580 calls, 4 600 computer
points, 500 tablet points), against a median of 0.06 ms for the tablet alone (`hub_latency`). The difference is the
batching (at most one batch every 4 ms, which the computer's calls keep busy), not the lock: no point waits more than
about one flush interval. Bound set to 10 ms (`NEVER_SLOWED_P99_MS`). Share (`share/tests.rs`, computer points, hub
call → the viewer's socket): median 2.5 to 2.9 ms.

### The window's queue, pen and eraser (C)

`lib/live-computer.ts`:
- `ComputerQueue`: the control window's computer commands one at a time, in order: `live_computer_pen`, `live_layers`,
  `live_edit_strokes` (the eraser's and the selection's: `lib/live-touch-up.ts` takes the queue), `live_undo_edit`,
  `live_redo_edit`. Tauri runs `async` commands on a pool of threads: two calls in flight could reach the hub in either
  order, and a Ctrl+Z pressed just after the pen is lifted must reach it after the stroke's `end`. `live_command` (pages,
  clear) stays outside: a clear or a page change that overtakes the end of a stroke drops or ends it, as the session
  says.
- `ComputerPen`: `down` (Pen tool; the pen's tip or the mouse's left button; the session running; the computer layer
  visible, else `onblocked`), `move` (coalesced events), `up`, `cancel`. One call of the gesture in flight; samples that
  come meanwhile go in the next call; `begin` at once, `end` with the last samples. A `false` answer, a page or note
  change stops the gesture and drops the local stroke. `liveEditStale` is silent (the page changed under the pen);
  other refusals go to the window's notice (`errorMessage`). Cancelled by `pointercancel`, a lost capture
  (`release(e, true)`), a second finger landing (as a mark is), Esc, a page change.
- `ComputerEraser`, for **the eraser end** (`pointerKind` = `eraser`): whole strokes of the computer layer, whatever the
  tool (none, Select, Pen), as the editor's eraser end (`Canvas.svelte`: "The back of the pen erases ... whatever the
  tool"). Radius: 18 screen pixels (`MARK_ERASER_PX`, the presentation marks' eraser) over the preview's scale: the
  preview shows the whole page small (an A4 page some 800 px wide, about 37 page units a pixel), where the editor's
  300 page units would be 8 px. Hit test: `strokesTouched(onLayer(live, live.computerLayer), [last, p], radius)`
  (`lib/editor/hit.ts`, `onLayer` from package A). A gesture has a `group` number; each move that touches new strokes
  sends `live_edit_strokes(pageIndex, live.revision, { kind: 'delete', indices, group })` through the queue, with the
  indices those stroke objects have in `live.strokes` and the revision read when the call leaves the queue, not when it
  is queued (objects are kept between moves, since a delete moves indices). On `liveEditStale` (the window's list is behind Rust's revision) the same objects are tried again
  after the next change of the `LiveState`, at most three times; objects gone meanwhile are skipped. The projector and
  the share see each delete as it happens. A ring of the eraser's size follows the eraser end in the control window
  (`Scene` overlay, never projected).
- **Mouse**: with Pen, the left button writes and the wheel still zooms; with no tool it drags the projected area, with
  Select it draws a lasso (today). **Finger**: navigation in the control window whatever the tool (pan, pinch, double
  tap: M8), never ink (Q11). **Palm rejection**: `PenPresence` as today (a finger is ignored while a pen touches or
  hovered less than 1.2 s ago). The barrel button means nothing in live: a tip with it held writes.

### Undo and redo (B; TS in C)

The page's history gets the computer's ink. The tablet's writing is never a step; a new computer stroke or edit empties
the redo steps (`History::record`), the tablet's writing does not (M12).

| Computer action | Undo step | Undo | Redo |
|---|---|---|---|
| a stroke ended | `Added { indices: [place] }` | takes it off (`take_strokes`), pushes `RedoStep::Add { strokes }` | gives it back at its place (`give_back`), pushes `Added` |
| an eraser gesture | one `Removed` per delete, tagged with the gesture's group | every step of the group, top down, in one command | the whole group again |
| delete or recolour of a selection, « Clear the page » | unchanged (M12) | | |

- What the tablet wrote is taken or given back only by the computer's eraser, delete or clear (on whichever layer the
  computer chose), and an undo gives back exactly what they took. M12's argument still holds once "the tablet only
  appends" reads "the pens only append" (`edits.rs` module documentation): a computer stroke joins the tail too, and
  steps come off in reverse order.
- Groups: `StrokesEdit::Delete` gets `group?: number` (`#[serde(default)]`; `CheckedEdit.group: Option<u32>`); a step
  keeps its group; `undo_edit`/`redo_edit` take the top step and every next step of the same group, one `strokesEdited`
  per step, in one publish. `MAX_UNDO_STEPS` and `MAX_UNDO_BYTES` count groups and drop whole groups, oldest first.
  `restore_cleared_with` (« Undo the clearing ») goes through the same `undo_step` (as built: no longer, see "As built
  (B)", « Annuler l'effacement »).
- `canUndo`/`canRedo` reach the windows through `strokesEdited` and the snapshot (as today) and through `history` after a
  computer stroke is recorded.
- Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z (M12) are ignored while a computer gesture (pen or eraser) is under way, as the editor's
  are (`Editor.svelte`: "A gesture under way commits on top of the doc it started from").
- Viewers: an undo may now give back a stroke on a layer hidden since (delete, hide the layer, Ctrl+Z) and a redo take
  one off. `ViewerIndices` becomes per stroke: `removed` and `recoloured` list visible strokes only (viewer numbering),
  `inserted: Vec<Option<u32>>` follows the event's `inserted`, `None` for a hidden stroke, which `feed.rs` leaves out.
  M12's "whether a stroke is hidden never changes during a session" goes; what keeps the viewers right is that every
  change of visibility is a snapshot, which rebuilds their list from what shows.

### Selection on the chosen layer (A and C)

- Live: `LiveCanvas.endLasso` gives `pickStrokes` `onLayer({ strokes: live.strokes, layers: live.layers },
  live.computerLayer)`. Choosing another layer drops the selection (`LiveView`, an effect on `live.computerLayer`), as a
  page or revision change does (M12 `follow`). `live.select.help` says only the chosen layer's strokes are picked.
- Rust does not check the layer of edited strokes: an edit is what the window chose, with M12's checks (index, hidden,
  duplicate). The one window that edits drops its selection when the layer changes, and an undo must still reach
  strokes of other layers.
- The computer's stroke in progress cannot be chosen (not in `live.strokes`, M12); when it ends it joins the tail, no
  index moves, a selection stays.
- Tools: the computer's group has Select and Pen, exclusive, or neither (navigation: today's behaviour, also at the
  start, so that a click on the preview still moves the area). Pen on turns Select off, which drops the selection (M12).

### The computer's ink (C)

`lib/live-computer-style.ts`: key `ardoise.live.computerStyle` in the browser's storage, JSON
`{ tool, color: '#rrggbb', width }`; `loadComputerStyle(storage?)` gives a `LiveStyle` (a tool of `TOOLS`, an opaque
colour, a width within the live slider's 10..1000, else the default); `saveComputerStyle(style, storage?)` at each
change, ignoring a blocked storage (as `rememberCustomColor`). Default: felt tip, Inkspace red `#d80000`
(`INKSPACE_COLORS[4]`), width 120 (`DEFAULT_WIDTHS.feltTip` of `LiveView`): it reads well on the projector next to a
ballpoint (our choice; Q11 says red only).
- In the browser's storage, not `ardoise.toml`: the pen is this PC's while the settings file is shared by the PCs
  through Nextcloud; the export choices already live there (`export-options.ts`), remembered from one session to the
  next by the WebView2 profile; and Rust needs the style only per stroke, which `begin` carries.
- Apart from the tablet's style (`live_set_style`, the snapshot's `style`, `carried_ink`): the blackboard turns the
  tablet's to chalk, not the computer's.

### The layers zone (A; used by C)

`components/layers/LayersZone.svelte`: the same in the editor's side panel (in place of `LayersPanel.svelte`, removed)
and as the first block of the live window's side panel (in place of the toolbar's `new-layer` button). Pure helpers
in `lib/layers-zone.ts`: `rowsTopFirst`, `layerWarnings(layers, chosen, tablet?)` (`tabletHidden` first, then
`chosenHidden`), `followMove(index, from, to)`, `followMerge(index, merged)` (the mock uses the last two).

```ts
// Props. The component keeps only the rename field's state; the rest comes from the editor's doc or the live snapshot.
{
  where: 'editor' | 'live';
  layers: readonly Layer[];        // bottom first, as in the note
  chosen: number;                  // editor: doc.activeLayer; live: live.computerLayer
  tablet?: number;                 // live: live.layer (the tablet's); absent in the editor
  disabled?: boolean;              // editor: the split panel is open; live: the session is not running
  onchoose: (index: number) => void;
  onvisible: (index: number, visible: boolean) => void;
  onrename: (index: number, name: string) => void;   // trimmed, non-empty, different from the old name
  onmove: (from: number, to: number) => void;        // to = from ± 1
  onmerge: (index: number) => void;                  // into index - 1
  onadd: () => void;
  ontablet?: (index: number) => void;                // live
}
```

DOM, relied upon by the tests and journeys of every package:
- `section.layers-zone[data-where]`, its `h3` (`layers.title`), `ul.layers` (`aria-label` `layers.list`), the rows top
  first: `li.layer[data-layer={index}]`, with `data-chosen="true"` and class `chosen` on the chosen row (the frame),
  `data-hidden="true"` on a hidden layer (its name in muted italics), `data-tablet="true"` on the tablet's row (live).
- In a row, in this order: the eye `button[data-action="layer-visible"][data-visible="true"|"false"]` (eye, or eye
  barred when hidden; `aria-label` `layers.hide` / `layers.show`); the name `button.layer-name[data-action="choose-layer"]`
  (`aria-pressed` = chosen; `aria-label` and `title`: `layers.chosen.<where>` on the chosen row, `layers.choose` on the
  others), replaced while renaming by `input.layer-rename` (`layers.nameField`); the pencil
  `button[data-action="rename-layer"]` (`layers.rename`); live only, the tablet `button[data-action="tablet-layer"]`:
  on the tablet's row `data-here="true"`, accent colour (warning colour when that layer is hidden), `layers.tabletHere`;
  on the others invisible until the row is hovered or the button focused, then pale, `layers.tabletSend`, and always
  pale under `@media (hover: none)` (a finger has no hover); on the chosen row only,
  `button[data-action="layer-up"|"layer-down"|"layer-merge"]` (↑ ↓ ⤓; up disabled on the top row, down and merge on the
  bottom one; `layers.up`, `layers.down`, `layers.merge`, plus `layers.mergeNoUndo` in the live title).
- Under the list: `button[data-action="add-layer"]` (« + » `layers.add`); the warnings,
  `p.notice[data-slot="layer-warning"][data-kind="tabletHidden"|"chosenHidden"]`, `role="status"` (live: the tablet's
  layer hidden; both places: the chosen layer hidden); the help `p.help[data-slot="layers-help"]` (`layers.help.<where>`).
- Mouse: a click anywhere on the row outside its buttons chooses it (the mockup); the buttons stop the click. The
  keyboard's control is the name button (Tab, then Enter or Space). The row's click handler carries
  `<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->` with that reason
  (`npm run check` fails on warnings; precedent: `components/library/Menu.svelte`).
- Rename: the pencil shows the field, focused, its text selected; Enter keeps (trimmed; empty or unchanged: nothing),
  Esc gives the old name back, leaving the field keeps (the mockup); then the focus goes to the row's name button. One
  field at a time; `disabled` closes it.
- Icons (eye, eye barred, pencil, tablet): the mockup's inline SVG, `aria-hidden`, in
  `components/layers/LayerIcon.svelte`; colours from the app's tokens.

### Live window (C)

- Toolbar: the style group becomes the tablet's, with the visible label `live.tabletStyle`; its controls are unchanged
  (with a selection its swatches still recolour it, M12). The `touch-up` group (`data-slot` kept for the M12 tests)
  becomes the computer's, labelled `live.computer.label`: `[data-action="select"]` (V), `[data-action="pen"]`
  (`live.pen.toggle`, P, `aria-pressed`, `title` `live.pen.hint`, disabled at idle like Select),
  `[data-action="computer-style"]` (a swatch and the tool's name, `aria-label` `live.pen.styleHint`), which opens
  `[data-slot="computer-style"]` (`live.pen.style`: the tool `select`, the colour swatches, which here only set the
  computer's colour, and the width slider; Esc or a click outside closes it), then Undo and Redo (unchanged; their
  hint is `live.undoHint`). The `new-layer` button goes.
- Side panel: first block `[data-section="layers"]`, `LayersZone where="live"`: `onchoose` → op `computer`, `ontablet` →
  op `tablet`, the others their op, all through the queue; a refusal in the window's notice. With Pen on,
  `[data-slot="pen-hint"]` (`live.pen.help`) under the preview's hint, as `select-hint` is.
- `LiveCanvas`: `tool: 'none' | 'select' | 'pen'` replaces `selecting` (`data-tool` on the canvas); `computer` (the
  window's queue, pen and eraser; absent in the presentation); `onblocked`. A pen down while the computer layer is
  hidden calls `onblocked`, and the window shows `layers.warn.chosenHidden` as a notice (the zone shows it too).
- Keys (`liveEditKeyAction`, while a session runs): P toggles Pen (the editor's key), V Select; `KEYS` adds `pen`; Esc
  drops a pen gesture under way before anything else.

**As built (C, interface).** Where the code says more than the sections above, or departs from them:
- `SceneOptions.localComputer` is a list, `readonly Stroke[]`, not one stroke: a second gesture may begin before the
  session ended the first, and both are drawn until `lastComputerDone` reaches them. Given, it replaces Rust's copy;
  an empty list hides it (a cancelled gesture, until its `strokeCancel` comes). `Scene.computer` holds what is drawn;
  `SceneOptions.eraser` / `Scene.eraser` is the ring (`ERASER_RING_PX`, 18, the marks' eraser), preview only.
- `ComputerPen` keeps one call of the gesture in flight and at most one waiting in the queue; the waiting call takes the
  samples (at most `MAX_PEN_SAMPLES`) and `now` when it leaves. So `end` is queued as soon as the pen is lifted, before
  a Ctrl+Z pressed after it. A gesture cancelled before its `begin` left sends nothing. A `false` answer, a refusal or a
  page or note change drops the local stroke at once (Rust's copy, if any, then shows until its `strokeEnd`).
- `ComputerEraser` skips, when its call leaves, the strokes gone meanwhile and those of a layer hidden since (Rust would
  refuse the whole delete); a stale call is tried again after the next notification of the `LiveState`, at most
  `STALE_RETRIES` (3) times, silently. Groups come from `nextGroup()`, from a random value in 1..2^31 drawn at load.
- `LiveComputer.busy` (a pen or eraser gesture under way) makes Ctrl+Z, Ctrl+Y and the Undo / Redo buttons wait. The
  layer ops read `pageIndex` at the click and go through the queue; every refusal shows in the window's notice.
- `loadComputerStyle` falls back field by field (a bad tool keeps a good colour). The panel's tool change takes that
  tool's last width in the session, else `DEFAULT_WIDTHS`, as the tablet's does.
- `LiveState`: computer `points` without a `strokeStart` (supposé: subscribed mid-stroke) start a stroke in the
  tablet's style on `computerLayer`; a tablet `strokeCancel` (B never sends one) drops the tablet's stroke.
- The toolbar groups are named by their visible labels (`aria-labelledby`): `live.tabletStyle` and
  `live.computer.label`. `live.style` and `live.select.label` are then unused, with `live.layer.{new, current}`.
- `lib/mock-live.ts`: after `live_stop` its session is gone, so `live_layers` `tablet` and `computer` answer
  `liveNotRunning` there too (B accepts them); the zone is disabled at idle anyway.

### Editor (A)

- A click on a row (or its name button) chooses the layer. `setActiveLayer` now also clears the selection: the selection
  belongs to the chosen layer.
- Selection: the lasso, the click and the barrel button's lasso (`Canvas.svelte` `finish`: `selectInLasso`, `hitStroke`)
  take `onLayer(doc.note, doc.activeLayer)`; Ctrl+A (`selectAll`) the chosen layer's strokes, when it shows. Moving,
  scaling, recolouring, deleting, copying act on the selection: nothing to change.
- Eraser, the tool or the eraser end, whole or partial: `state.erase` passes `doc.activeLayer`;
  `ops.eraseWhole(note, path, radius, layer?)` and `ops.erasePartial(note, path, radius, layer?)` touch only that layer.
- Pen, paste, duplicate: already on the active layer, refused on a hidden one with `editor.layers.hiddenActive`.
- `lib/editor/hit.ts`: `Hittable` gets `only?: number`; `onLayer(note, layer)` returns `{ strokes, layers, only: layer }`;
  with `only`, `layerVisible` is true for that layer alone and only while it shows (so a stroke whose layer has no entry
  is never hit then); `drawingOrder`, `selectInLasso`, `hitStroke`, `strokesTouched` all go through `layerVisible`.
  Splitting by a zone (`split.ts`) keeps every visible layer: it is not a selection.
- **The chosen layer hidden.** Today the active layer moves to the nearest visible one. M14: it stays chosen and the zone
  shows `layers.warn.chosenHidden` (the mockup). Reasons: one rule in both places (Q11: « pour que la règle soit la
  même partout »); in live, a computer moved by itself would put the teacher's next stroke on the pupil's layer; hiding
  then showing a layer gives the same state back; the frame always shows which layer is chosen. The pen then gives its
  notice, the eraser and the selection find nothing. Opening a note still chooses the visible layer nearest the top
  (`docFromNote`, unchanged).

**As built (A, layers zone and editor).** Where the code says more than the sections above, or departs from them:
- `lib/layers-zone.ts`: `rowsTopFirst(layers)` returns `{ index, layer }[]` (top first); `layerWarnings` returns
  `{ kind, index, name }[]`; `followMove` is `ops.moveIndex` (a test compares them on every case); `followMerge(index,
  merged)` takes `merged` itself down onto `merged - 1`.
- `LayersZone.svelte` renders `data-section="layers"` on its own `section` (package C need not wrap it to get the
  `[data-section="layers"]` block of "Live window (C)"). The section carries its own padding and bottom border (the
  `side-section` / `side-block` look), so the owner puts nothing around it.
- A click on the chosen row, and a click on the tablet icon of the tablet's own row, call nothing (the editor's
  `setActiveLayer` and the live op would change nothing). The focus goes back to the name button after Enter and Esc in
  the rename field, not when the field is left by clicking elsewhere (it would take the focus from what was clicked).
- The add button shows « + » and `layers.add`; its `aria-label` is `layers.add` alone. `layers.mergeNoUndo` is in the
  merge button's `title` in live, not in its `aria-label`.
- The warning colours are component tokens (`--zone-warn`, `--zone-warn-bg`: the mockup's, with the dark theme's), as
  `app.css` has no warning token. The global `.layers` / `.layer` rules of `app.css` are gone (the zone's rules are
  scoped).
- Editor: `setActiveLayer` and `addLayer` both clear the selection (the selection belongs to the chosen layer);
  `setLayerVisible` still drops the hidden layer's strokes from the selection. `hit.ts`: `onLayer(note, layer)`;
  `layerVisible` with `only` is true for that layer alone and only while it shows; `drawingOrder` with `only` lists
  that layer's strokes in file order. `ops.eraseWhole` and `ops.erasePartial` take `layer?`; `state.erase` always passes
  the active layer.
- Test helper `components/layers/LayersZoneHarness.svelte` (props changed from outside, callbacks recorded).

### Translations

All added by package 0, in both catalogs (the French in the user's tone: short, plain; quotes « » and plain spaces, as
the files have them). New:

| Key | Français | English |
|---|---|---|
| `layers.title` | Calques | Layers |
| `layers.list` | Calques de la page | Layers of the page |
| `layers.add` | Ajouter un calque | Add a layer |
| `layers.choose` | Agir sur « {name} » | Act on “{name}” |
| `layers.chosen.editor` | Le stylet, la gomme et la sélection agissent sur « {name} » | The pen, the eraser and the selection act on “{name}” |
| `layers.chosen.live` | L'ordinateur agit sur « {name} » | The computer acts on “{name}” |
| `layers.show` | Afficher « {name} » | Show “{name}” |
| `layers.hide` | Masquer « {name} » | Hide “{name}” |
| `layers.rename` | Renommer « {name} » | Rename “{name}” |
| `layers.nameField` | Nom du calque {n} | Name of layer {n} |
| `layers.up` | Monter « {name} » | Move “{name}” up |
| `layers.down` | Descendre « {name} » | Move “{name}” down |
| `layers.merge` | Fusionner « {name} » avec le calque du dessous | Merge “{name}” into the layer below |
| `layers.mergeNoUndo` | Ctrl+Z ne l'annule pas. | Ctrl+Z does not undo it. |
| `layers.tabletHere` | La tablette écrit sur « {name} » | The tablet writes on “{name}” |
| `layers.tabletSend` | Faire écrire la tablette sur « {name} » | Make the tablet write on “{name}” |
| `layers.warn.tabletHidden` | La tablette écrit sur « {name} », qui est masqué : ce que l'élève écrit n'est ni projeté ni partagé. | The tablet writes on “{name}”, which is hidden: what the pupil writes is neither projected nor shared. |
| `layers.warn.chosenHidden` | Le calque choisi, « {name} », est masqué : affichez-le pour y écrire. | The chosen layer, “{name}”, is hidden: show it to write on it. |
| `layers.help.editor` | Le stylet, la gomme et la sélection agissent sur le calque choisi (encadré). | The pen, the eraser and the selection act on the chosen layer (framed). |
| `layers.help.live` | Le stylet de l'ordinateur, sa gomme et la sélection agissent sur le calque choisi (encadré). La tablette écrit sur celui marqué de l'icône de tablette. | The computer's pen, its eraser and the selection act on the chosen layer (framed). The tablet writes on the one marked with the tablet icon. |
| `live.tabletStyle` | Tablette | Tablet |
| `live.computer.label` | Ordinateur | Computer |
| `live.pen.toggle` | Stylo | Pen |
| `live.pen.hint` | Écrire avec le stylet ou la souris de l'ordinateur sur le calque choisi (P) | Write with the computer's pen or mouse on the chosen layer (P) |
| `live.pen.help` | Stylet ou souris : écrire · bout gomme du stylet : effacer des traits entiers du calque choisi · doigt : déplacer et zoomer la zone projetée. | Pen or mouse: write · eraser end of the pen: erase whole strokes of the chosen layer · finger: move and zoom the projected area. |
| `live.pen.style` | Encre de l'ordinateur | Computer's ink |
| `live.pen.styleHint` | Choisir l'outil, la couleur et l'épaisseur de l'ordinateur | Choose the computer's tool, colour and width |
| `live.keys.list.pen.keys` | P | P |
| `live.keys.list.pen.action` | Outil Stylo : écrire avec le stylet ou la souris de l'ordinateur | Pen tool: write with the computer's pen or mouse |
| `errorCodes.liveLayerHidden` | Le calque choisi est masqué : affichez-le pour y écrire. | The chosen layer is hidden: show it to write on it. |
| `errorCodes.liveLayerInvalid` | Cette opération sur les calques n'est pas possible. | This layer operation is not possible. |
| `export.layers.*` | see [exports-and-operations.md](exports-and-operations.md), "Layers at export (M14)" | |

Changed:

| Key | Français | English |
|---|---|---|
| `live.select.help` | Lasso : sélectionner · clic : un trait · Maj : ajouter · Ctrl : retirer · Échap : désélectionner. Les pastilles de couleur changent la couleur de la sélection. Seuls les traits du calque choisi sont pris. | Lasso: select · click: one stroke · Shift: add · Ctrl: remove · Esc: deselect. The colour swatches change the colour of the selection. Only strokes of the chosen layer are picked. |
| `live.undoHint` | Annuler la dernière action de l'ordinateur : un trait, un coup de gomme, une retouche ou l'effacement de la page (Ctrl+Z) ; ce que la tablette a écrit n'est pas annulé | Undo the computer's last action: a stroke, an erasing, a touch-up or the clearing of the page (Ctrl+Z); what the tablet wrote is not undone |
| `live.keys.list.undo.action` | Annuler, rétablir un trait ou un coup de gomme de l'ordinateur, une suppression, un changement de couleur ou l'effacement de la page | Undo, redo a stroke or an erasing of the computer, a delete, a change of colour or the clearing of the page |
| `editor.layers.hiddenActive` | Le calque choisi est masqué : affichez-le ou choisissez-en un autre pour dessiner. | The chosen layer is hidden: show it or choose another one to draw. |

Unused once A and C are in, removed by package E: `editor.layers.{label, add, active, visible, name, up, down, merge}`,
`live.layer.{new, current}` (`editor.layers.defaultName` stays: `docFromNote` and the editor's « add »).

**As built (package E).** Removed from both catalogs, after a search of `app/ui/src`, `app/e2e`, `app/scripts` and `app/src-tauri/src`
found no use: `editor.layers.{label, add, active, visible, name, up, down, merge}`, `live.layer.{new, current}`,
`live.style` and `live.select.label`. Kept, still used: `editor.layers.defaultName` (`docFromNote`, the editor's « add ») and
`editor.layers.hiddenActive` (the editor's notice when the pen meets a hidden layer).

### Contracts between packages

Rust (package B; `model.rs` unless said):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LivePen { #[default] Tablet, Computer }     // LivePen::is_tablet for skip_serializing_if

// LiveSnapshot: `layer` (the tablet's, unchanged name) + `computer_layer: u32`.
// LiveEvent:
StrokeStart  { page_index, stroke: LiveStroke, pen: LivePen, id: Option<u32>, hidden: bool }
Points       { page_index, points: Vec<LivePoint>, pen: LivePen, id: Option<u32>, hidden: bool }
StrokeEnd    { page_index, pen: LivePen, id: Option<u32>, hidden: bool }
StrokeCancel { page_index, pen: LivePen, id: Option<u32>, hidden: bool }     // new
History      { page_index, can_undo: bool, can_redo: bool }                  // new
// `pen`: skip_serializing_if = "LivePen::is_tablet"; `id`: skip_serializing_if = "Option::is_none"; `hidden`: skip.
// ViewerIndices.inserted: Vec<Option<u32>>; removed and recoloured hold visible strokes only.

// live/pen.rs
#[derive(Deserialize)] pub struct ComputerSample { pub x: f64, pub y: f64, pub p: f64, pub ms: f64 }
#[derive(Deserialize)] #[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ComputerPenInput {
    Begin { stroke: u32, page_index: usize, style: LiveStyle, samples: Vec<ComputerSample>, now: f64 },
    Points { stroke: u32, samples: Vec<ComputerSample>, now: f64 },
    End { stroke: u32, samples: Vec<ComputerSample>, now: f64 },
    Cancel { stroke: u32 },
}
pub const MAX_PEN_SAMPLES: usize = 4096;  pub const MAX_COMPUTER_POINTS: usize = 100_000;

// live/layers.rs (same serde attributes)
pub enum LayerOp { Add, Rename { layer: f64, name: String }, Visible { layer: f64, visible: bool },
                   Move { from: f64, to: f64 }, Merge { layer: f64 }, Computer { layer: f64 }, Tablet { layer: f64 } }
pub const MAX_LAYERS: usize = 100;  pub const MAX_LAYER_NAME: usize = 200;

// edits.rs: StrokesEdit::Delete { indices: Vec<f64>, #[serde(default)] group: Option<f64> };
// LiveEditError gains LayerHidden (code liveLayerHidden) and BadLayer, BadLayerName, TooManyLayers (liveLayerInvalid).

// live.rs, registered in lib.rs:
#[tauri::command] pub async fn live_computer_pen(runtime: State<'_, LiveRuntime>, input: ComputerPenInput) -> CommandResult<bool>
#[tauri::command] pub async fn live_layers(runtime: State<'_, LiveRuntime>, page_index: usize, op: LayerOp) -> CommandResult<()>
```

**As built (B, 2026-11-11).** The shapes above are unchanged. Where the code goes further or differs:
- `LiveEditError` also has `BadGroup` (a `group` that is no whole `u32`), `NoSample` (a `begin` without a finite sample)
  and `TooManySamples { count }` (more than `MAX_PEN_SAMPLES` in any call): all `liveEditInvalid`, no new code.
- `MAX_COMPUTER_POINTS` reached: the stroke **ends** there, kept (`strokeEnd`, `history`), and that call answers
  `false`; the window's local stroke goes on `lastComputerDone` as for any end.
- `points`, `end`, `cancel` for another number than the stroke in progress (or none), or on a stopped session: `false`,
  never an error; `begin` on a stopped session: `liveNotRunning`; before the first start every call is `liveNotRunning`.
  A refused `begin` (stale, hidden layer) leaves the stroke in progress as it is.
- `strokeCancel` is sent for the computer only. « Clear the page »: `strokeCancel` of the computer's gesture, then the
  snapshot; the tablet's stroke is still dropped by the snapshot alone, as before.
- `hub::enqueue` merges points only into the last event of the same page, pen, `id` and `hidden`.
- A `strokesEdited` that names nothing a viewer has (only strokes of a layer hidden since went or came back) is not sent
  to the viewers at all (`share/feed.rs` `shared_edit`).
- A merge always raises the revision and drops the page's history, even when the merged layer is empty.
- A redo that gives back a computer stroke copies it before the lock (`LiveSession::next_redone`, `Prepared::of_all`),
  as an undo does; a group's copies are made together.
- Times: `t` never goes back within a stroke; `recv` follows the design's bounds in order (at least the previous
  sample's, at most the call's wall time), so it may when the system clock itself goes back.
- `newLayer` (the tablet's button) on a page of `MAX_LAYERS` layers does nothing at all.
- Groups: steps next to each other naming the same group are one, whatever came before them. A control window that
  restarts its counter (reloaded mid-session) could join a gesture to the previous window's last one if nothing came
  between. For package C: number the gestures' groups from a value drawn at each load of the window, not from 1
  (supposé enough).
- **« Annuler l'effacement » (`live_restore_cleared`), fixed after the review.** Going through `undo_step` down to the
  clearing, as written above and in M12, undid the computer's strokes written since (a correction made within the
  8 s offer vanished, went to the redo stack under the clearing, and was lost from the file at the next write). Now
  `History::unclear` undoes the clearing **out of order**: its strokes come back under what was written since,
  whichever pen wrote it, at the places they had among the strokes it left (those of hidden layers, kept first in the
  list); everything done since stays done. **The history after a restore**: the clearing is no longer in it (Ctrl+Y
  does not clear again); every step done since stays where it was, each place it names moved for the strokes back
  (a computer stroke or a delete of ink written since: by the number of strokes back; a stroke the clearing left:
  by those that stood before it), so Ctrl+Z undoes the teacher's last stroke, then the other steps since, then those
  from before the clearing, as if the clearing had never been done; the redo steps are dropped, as for any new action.
  The tablet's ink is never a step and stays. The places are followed step by step, so they stay right when ink the
  clearing left (a layer hidden at the clearing, shown since) was taken out in between (`Unclearing`, `edits.rs`).
  Each `Removed` step keeps `left`, the strokes left in the list just after it. One `strokesEdited` (viewer
  numbering per stroke, as any). After a merge (history dropped) there is no clearing to undo: `false`, as when the
  page has none, and the window says `live.clearUndoFailed`. The copy of the cleared strokes is made before the lock
  (`LiveSession::cleared_strokes`). The browser stand-in (`mock-live.ts` `restoreCleared`) does the same.

TypeScript (`lib/live.ts`, `lib/api.ts`; package 0 writes them):

```ts
export type LivePen = 'tablet' | 'computer';
// LiveSnapshot: `layer` = the tablet's layer; new `computerLayer?: number` (absent on the share's viewers).
// LiveEvent: strokeStart / points / strokeEnd gain `pen?: 'computer'; id?: number`, and
  | { type: 'strokeCancel'; pageIndex: number; pen?: 'computer'; id?: number }
  | { type: 'history'; pageIndex: number; canUndo: boolean; canRedo: boolean }
export interface ComputerSample { x: number; y: number; p: number; ms: number }
export type LiveComputerPenInput = /* as above */;
export type LiveLayerOp =
  | { kind: 'add' } | { kind: 'rename'; layer: number; name: string } | { kind: 'visible'; layer: number; visible: boolean }
  | { kind: 'move'; from: number; to: number } | { kind: 'merge'; layer: number }
  | { kind: 'computer'; layer: number } | { kind: 'tablet'; layer: number };
// LiveStrokesEdit: { kind: 'delete'; indices: number[]; group?: number } | (recolour unchanged)
api.liveComputerPen: (input: LiveComputerPenInput) => call<boolean>('live_computer_pen', { input })
api.liveLayers: (pageIndex: number, op: LiveLayerOp) => call<void>('live_layers', { pageIndex, op })
```

Interface between A and C: `LayersZone.svelte` (props and DOM above), `lib/layers-zone.ts`, `onLayer` and the optional
`only` of `Hittable` in `lib/editor/hit.ts`.

### Work packages

One owner per file, as in M4 ("Rules"): a package that needs to change a file it does not own says so in its report.
Each package works in its own worktree and branch, merged into `m14` by the session.

| # | Package (profile) | Owns | Starts |
|---|---|---|---|
| 0 | Contracts (the session itself, one commit) | `locales/fr.json`, `locales/en.json` (every key of "Translations" and of the export section); `lib/live.ts`, `lib/api.ts` (the types and wrappers of "Contracts"); `lib/note.ts` (`ExportOptions.layers?`); `app/e2e/journeys-m14-{editor,live,export}.mjs` created as `export const … = []`, registered in `app/e2e/run.mjs` | first |
| A | Layers zone and editor (developpeur) | new `components/layers/{LayersZone,LayerIcon}.svelte`, `LayersZone.test.ts`, `lib/layers-zone.ts` (+ test); `components/editor/LayersPanel.svelte` (removed), `Editor.svelte`, `Canvas.svelte`, their tests and test harnesses; `lib/editor/{state,ops,hit}.ts` and their tests; `app/e2e/journeys-m14-editor.mjs` | after 0 |
| B | Live, Rust side (specialiste) | `app/src-tauri/src/live/{model,session,edits,hub,resume}.rs`, new `live/{pen,layers}.rs`, the tests `live/tests.rs`, `live/edits/tests.rs`, `live/resume/tests.rs`, new `live/pen/tests.rs`, `live/layers/tests.rs`; `app/src-tauri/src/live.rs`; `app/src-tauri/src/lib.rs` (two commands); `app/src-tauri/src/share/{feed,tests}.rs` | after 0, with A and D |
| C | Live, interface (specialiste) | `components/live/*` (`LiveView`, `LiveCanvas`, `Presentation`, new `ComputerStyle.svelte`, their tests); `lib/live-state.svelte.ts`, `lib/live-render.ts`, `lib/live-select.svelte.ts`, `lib/live-keys.ts`, `lib/live-touch-up.ts`, `lib/live-actions.ts` and `lib/live-edits.ts` (if they need it), `lib/latency.ts`, new `lib/live-computer.ts`, `lib/live-computer-style.ts`, their tests; `lib/mock-live.ts` (+ tests: the rules of B for the new commands); `lib/live.ts` and `lib/api.ts` after 0 (comments only: the shapes are fixed); `viewer/ViewerCanvas.svelte`; `app/e2e/journeys-m14-live.mjs`, `app/e2e/journeys-m12-touchup.mjs` (only if a selector moved), `app/e2e/perf.mjs`, `xtask/src/perf.rs` | after A is merged (it imports the zone and `onLayer`); its e2e and perf once B is merged |
| D | Layers at export (developpeur) | `crates/ardoise-render/src/{lib,svg,pdf,vector}.rs` and `crates/ardoise-render/tests/export_formats.rs`; `app/src-tauri/src/{export,autoexport,drag}.rs`, `app/src-tauri/src/mcp/write.rs` and `background.rs` (struct literals only); `lib/note.ts` after 0, `lib/export-options.ts`, `components/ExportDialog.svelte`, `lib/mock-export.ts` and their tests; `app/e2e/journeys-m14-export.mjs` | after 0, with A and B |
| E | Guides and closing (developpeur) | `docs/guide/*.md` (FR and EN), `CHANGELOG.md`, `docs/hardware-tests.md` (H6), the locale files after 0 (removes the unused keys), the "as built" notes of this section and of the export one, `docs/performance.md` (by `cargo xtask perf --write` on the PC) | after A to D |
| R | Independent review (relecteur), remarks fixed by their package's owner | – | last |

Order: 0, then A, B and D in parallel, then C, then E, then R. `ROADMAP.md` is ticked only after `cargo xtask check` and
`cargo xtask e2e` on the PC (CLAUDE.md).

**Tests each package writes.**
- A: `layers-zone.test.ts` (rows top first, warnings, following a move and a merge, the mockup's cases);
  `LayersZone.test.ts` (a row click chooses, the name button with Enter and Space, `aria-pressed`, the eye's two states,
  rename with Enter, Esc and blur, an empty name refused, up/down/merge on the chosen row only and their limits, add,
  the tablet icon in live only, `data-here`, the hover class and `hover: none`, the two warnings, `disabled`); `hit`,
  `ops`, `state` (`onLayer` and `only`, an orphan stroke never hit with `only`, erasers on one layer, `selectAll`,
  `setActiveLayer` clears the selection, hiding the chosen layer keeps it); `Canvas` (lasso, click, barrel lasso and the
  eraser end take the chosen layer only); `Editor` (the zone replaces the panel, the hidden-layer notice).
- B: Rust unit tests per section above: the table of "Data model" (start, page, resume, button, every op and the
  following of both indices, merge's order, revision and dropped history, move's remapped history and strokes in
  progress, refusals and their codes, after stop); the computer stroke (begin, points, end, cancel; times on a fresh
  page, on a resumed sheet, never pushed by the tablet nor pushing it; two strokes in progress, interleaved events with
  `pen` and `id`, `enqueue` never merging across pens, a snapshot mid-strokes followed by both; ended by a page change,
  a quarter turn, a stop; dropped by a clear; `false` afterwards; stale page, hidden layer, bad samples, the limits);
  undo and redo (a computer stroke undone and redone at its place while the tablet appended after it, the tablet's
  strokes never undone, an eraser group undone in one command, limits in groups, « Undo the clearing » with groups);
  the share (hidden strokes in progress never sent, the tablet on a hidden layer, an undo giving back a stroke of a
  layer hidden since, the allow list's `pen` and no `id`, `history` left out); the hub test of "The tablet is never
  slowed"; a latency test hub → SSE for computer points, as M11's.
- C: `live-state` (two strokes in progress, `pen`, `strokeCancel`, `history`, `computerLayer`, `lastComputerDone`, the
  tablet-only latency list); `live-render` (both strokes drawn in their layers' order, a hidden one not, `localComputer`
  instead of Rust's copy; the partial repaint of M12 unchanged); `live-computer` (the queue keeps order, one call in
  flight, batching, end and cancel, a `false` answer, the eraser's groups and its stale retry); `live-computer-style`
  (default, garbage, blocked storage); `LiveCanvas` (Pen: pen and mouse write, finger pans and pinches, palm rejection,
  the eraser end erases the chosen layer only in every tool, selection on the chosen layer only, the local stroke
  handed over on `lastComputerDone`); `LiveView` (the zone wired to `live_layers`, the tablet icon, warnings, P, the
  style panel remembered, the selection dropped when the layer changes, the `new-layer` button gone); `mock-live` (the
  new commands follow B's rules).
- D: see the export section.

### End-to-end journeys

Driven through the real window as the others (`app/e2e/run.mjs`: WebView2 over CDP), in French and in English. The pen
is `Input.dispatchMouseEvent` with `pointerType: 'pen'` and `force` (`journeys-m8-input.mjs` `pen`); CDP drops the
eraser button (32) there (same file, "has no eraser button"): the journeys first try it, and when the page did not see
`buttons & 32`, dispatch `PointerEvent`s built in the page (`pointerType: 'pen'`, `buttons: 32`) on the canvas instead,
saying so in the log.

- **`journeys-m14-live.mjs`** (C), « corriger pendant le direct : l'ordinateur écrit sur « Correction » pendant que la
  tablette écrit » (`--simulated-tablet`, the share on loopback and the browser flags of `journeys-m12-touchup.mjs`):
  open live and wait for the tablet's ink; project; join the share's stream (`joinStream`, `startShare`); the zone has
  one row, chosen, with the tablet; « Ajouter un calque » → a second row, chosen, the tablet still on the first; rename
  it « Correction » (pencil, type, Enter); Pen (P); draw a stroke while the tablet writes → the preview counts one more
  stroke, the presentation draws it (`data-strokes`, `__ardoiseLatency.computer` has samples), and the stream got a
  `strokeStart` with `pen: "computer"` and `points` **before** the pen was lifted, median `recv` → arrival under 50 ms;
  Ctrl+Z → the stroke leaves the preview, the presentation and the stream (`strokesEdited`), and the tablet's strokes
  are all still there; Ctrl+Y → back; the eraser end across a tablet stroke and the computer's stroke → only the
  computer's goes; choose « Calque 1 » (a click on its row), erase a tablet stroke → it goes, Ctrl+Z → it comes back;
  send the tablet to « Correction » (the pale icon on hover) → its next strokes come with `layer: 1`; hide the tablet's
  layer → the warning, and for 2 s the stream gets no tablet stroke while the preview's count grows; show it again;
  Finish. The note's file (read as `journeys-m12-touchup.mjs` decodes strokes): layers `[Calque 1, Correction]`, the
  tablet's strokes on the layers it was on, the computer's on layer 1 in its style (red felt tip 120), its first `t`
  within 150 ms of the wall time of the pen down on the page's clock, between the tablet's strokes written just before
  and just after it.
- **`journeys-m14-editor.mjs`** (A), « éditeur : la sélection, Ctrl+A et la gomme ne prennent que le calque choisi »:
  open a note, draw strokes, « Ajouter un calque » (chosen), draw strokes crossing the first ones; a lasso around all →
  only the second layer's strokes selected; Ctrl+A → the same; the Eraser tool (E) across both → only the second
  layer's go; click the first row → the selection is empty, a lasso takes the first layer's strokes; hide the chosen
  layer → it stays chosen, the warning shows, a pen stroke gives the notice and adds nothing; rename with Enter, then Esc
  cancels; up, down and merge only on the chosen row; merge → one layer, every stroke kept.
- **`journeys-m14-export.mjs`** (D): see the export section.

### Performance (C)

`cargo xtask perf --only pen` (new part `pen`, `perf.mjs` `measureLivePen`, `xtask/src/perf.rs`): as `live` (the
simulated tablet streams), with the presentation open (the browser flags of `journeys-m12-touchup.mjs`, so that the
covered control window keeps painting), a layer « Correction » added and Pen on; while the frame intervals are recorded
in the control window and in the presentation window, the script draws with the pen through CDP (a move every 8 ms, a
stroke of 1.5 s, 0.3 s off, at the foot of the page, below the simulated pen's lines, where the M12 pixel sheet puts its
strokes). Rows: « Mode direct, la tablette et le stylet de l'ordinateur ensemble (≥ 60 i/s, ≤ 1 % d'images en
retard) », one per window, judged with `LIVE_MIN_FPS` and `LIVE_DROPPED_MAX_PERCENT`; « Latence du stylet de
l'ordinateur au vidéoprojecteur (médiane < 30 ms) », from `__ardoiseLatency.computer`. The strokes of both pens must
have grown, or the measure says nothing.

### Not done on purpose

An eraser button in live (Q11: the pen's eraser end), a mouse eraser, the barrel button's lasso in live, undoing layer
ops, deleting a layer (the editor has none either), storing which pen wrote a stroke, the computer's ink in the
presentation window (its temporary marks are unchanged, Q11), the mockup's « nouveau » badge in the export dialog.

### To check on Windows (H6 and the PC's runs)

The pen's events in WebView2 on the Surface (rate, coalesced events, `pressure`, the pen's hover giving `:hover` for the
tablet icon), Windows Ink's own feedback over the canvas (`touch-action: none` is set), the IPC rate of one call every
few milliseconds, the eraser end (CDP cannot send it), the projector's latency figure, Acrobat Reader's and Firefox's
layer panels (`/PageMode /UseOC` in Firefox is supposé), Inkscape's labels.
