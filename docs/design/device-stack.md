# Device stack design (milestone M1)

Developer document (English). Protocol facts: [protocol.md](../research/protocol.md). Requirements: SPEC §3.2, §4.1, §4.2, §4.8, §6.

```
ardoise-proto   pure: frames, requests/replies, BCD time, stroke files, live packets, CRC, describe()
      ^
ardoise-device  transport traits (transport.rs, fixed), Clock, diagnostic log (.jsonl), device.json,
      ^         Session (protocol flows), Manager (connector + identity + reconnect), diagnostic runner
      |
  +---+-----------------+
ardoise-ble            ardoise-sim
(btleplug, Windows)    (simulated tablet, virtual clock, replay of .jsonl, simulated user)
      ^                       ^
      +------ app/src-tauri --+   (--diagnostic, --diagnostic --simulated, UI page)
```

## Rules shared by all crates

- Clean room: implement from `protocol.md` and our own captures. Tuhi test files may be read **only** to extract
  byte test vectors (data); never read or translate Tuhi's implementation (`protocol.py`, `wacom.py`).
- No `unwrap`/`expect`/panic on bytes coming from the tablet or from files: typed errors (`thiserror`).
- All multi-byte integers are little-endian unless protocol.md says otherwise.
- Everything is blocking + timeouts (see `transport.rs`); no async runtime leaks out of `ardoise-ble`.
- Time always comes from a `Clock` (never `SystemTime::now()` / `Instant::now()` directly in device/sim code),
  so simulated sessions are instant and deterministic.

## ardoise-proto (no I/O)

Public surface (names may be refined, the shape must stay):

- `Frame { opcode: u8, args: Vec<u8> }` with `encode()` / `decode()` (`opcode, len, args`; len must match).
- `Request` (host to tablet), each with `to_frame()` and `Request::from_frame()` (the simulator decodes requests):
  `Connect{uuid:[u8;6]}` e6, `RegisterPressButton{uuid}` e7, `GetName` / `SetName(String)` bb, `GetTime` /
  `SetTime(BcdTime)` b6, `GetFirmware{part}` b7, `GetBattery` b9, `GetDimension(Dimension)` ea,
  `SetFileTransferReportingType` ec, `SetMode(Mode)` b1, `AvailableFilesCount` c1, `GetFileInfo` cc,
  `DownloadOldestFile` c3, `DeleteOldestFile` ca, `Unknown(Frame)`.
- `Reply` (tablet to host), `to_frame()` (simulator) and `Reply::from_frame()` (host):
  `Status(StatusCode)` b3, `RegisterButtonPressed` e4, `NameChunk(Vec<u8>)` bc, `Time(BcdTime)` bd,
  `Firmware{part, text}` b8, `Battery{percent, charging}` ba, `Dimension{which, value}` eb, `FilesCount(u16)` c2,
  `FileInfo{size, time}` cf, `DownloadStart{extra}` c8 be, `DownloadEnd{crc}` c8 ed, `Unknown(Frame)`.
  Unknown opcodes decode to `Unknown`, malformed known frames are errors.
- `StatusCode` (Success, GeneralError, InvalidState, ReadOnlyParam, CommandNotSupported, AuthorizationError, Other(u8)),
  `Mode` (Live 0, Paper 1, Idle 2), `Dimension` (Width 03, Height 04).
- `BcdTime` (UTC `yymmddhhmmss` BCD) with unix-seconds conversions.
- `stroke`: `decode(&[u8]) -> StrokeFile` (Slate `62 38 62 74` and Intuos Pro headers, all packet kinds) and
  `encode_slate(&[Stroke]) -> Vec<u8>` (used by the simulator; must decode back to the same points).
  Points are raw device units: `x, y` (10 µm), `pressure` (0..=2047 on the Slate).
- `live`: `decode(&[u8]) -> LivePacket` (`a1` samples incl. proximity-out record, `a2` proximity-in, `10` status)
  and encoders that split samples into notifications of at most 20 bytes.
- `crc32` (IEEE, as zlib) and `describe_*` helpers giving a one-line English interpretation of any frame or
  packet for the diagnostic log (never fails: unknown bytes are described as unknown).

## ardoise-device

- `transport.rs` (fixed contract): `Connector`, `Transport`, `Channel`, `TransportEvent`, `Advertisement`,
  `TransportError`, `Clock`, `SystemClock`.
- Diagnostic log (SPEC §4.8): `<library>/_ardoise/diagnostics/YYYY-MM-DD_HHMMSS.jsonl` (local time).
  One JSON object per line. First line `{"type":"header","format":1,"app":"0.1.0","transport":"ble|sim|replay",
  "created":"<RFC 3339>"}`; then `{"t":12.345,"dir":"tx|rx|event|note","ch":"command|page_data|live|sys_event",
  "hex":"e6 06 …","info":"CONNECT uuid=…"}` (`t` = seconds on the session clock; `ch`/`hex` absent for
  event/note). A `LoggingTransport` decorator records every write and every received event, so the log is the
  same whatever the transport. Writes to the library must be atomic (SPEC §3.4): keep the lines in memory and
  rewrite the whole file with `ardoise_core::fsutil::write_atomic` at most every 2 s while dirty and at the end.
- `device.json` in `<library>/_ardoise/` (shared by every PC): `format`, `uuid` (12 hex chars), `address`, `name`,
  `protocol` ("slate"), `width`, `height`, `registered` (RFC 3339). Atomic write. A fresh uuid is 6 random bytes.
- `Session` over a `Box<dyn Transport>` implements the flows of protocol.md §3–§7: register (e7, wait e4 up to
  a configurable time, then set time, read back, ec, name, dimensions, firmware, battery), connect (e6; `b3 01 07`
  = not registered with us), info, sync (paper mode, count, loop cc/c3/data/c8 ed + CRC; hand each page to a sink
  that must persist it, and only then send `ca` if deletion is enabled), live mode (b1 00 / b1 02, samples
  delivered to the caller). It keeps per-channel queues so that notifications arriving while waiting for a reply
  are never lost, tolerates unsolicited frames (stray `bc`, sysevent), and reassembles names until `0a`.
- `Manager` ties a `Connector`, the library's `device.json` and a log together: find a tablet (register mode or
  normal), register and save the identity, connect with the saved identity, reconnect transparently in live mode
  after `TransportEvent::Disconnected` (re-auth, re-enter live mode).
- Diagnostic runner (SPEC §4.8): ordered steps (adapter, find, register-or-connect, info + time, reference pages,
  sync, live 30 s, disconnect + reconnect check), driven by an `Operator` trait (instructions, ready/skip/cancel,
  progress, outcome), producing a structured report (pass / fail / skipped per step, details, file paths).

## ardoise-sim

- Simulated Slate (default: Bamboo Slate Large 29700 × 21600, pressure 2047, name `Bamboo Slate`, firmware
  `B160805A-C170704A`): registration with one stored uuid, auth (`b3 01 07` for a wrong uuid), time, name,
  battery, dimensions, file count/info/download (20-byte chunks on `PageData`, then `c8 ed` + CRC on `Command`),
  delete oldest, modes, live streaming (`a2` then `a1` packets, 5 ms per sample), physical button (hold = register
  mode, press = confirm registration / store page), optional faults (idle disconnect, bad CRC once) for tests.
- `SimConnector` implements `Connector` with a virtual `Clock`: `recv(timeout)` with nothing queued advances the
  virtual time by `timeout` and returns at once; every emitted live sample advances it by 5 ms.
- Replay: a `Connector`/`Transport` built from a `.jsonl` log. Host writes must match the recorded `tx` frames
  (byte for byte, except the payload of SET_TIME when replaying a real capture); recorded `rx` events are
  delivered in order. A log produced against the simulator must replay identically (same tx/rx sequence).
- A simulated user for the diagnostic runner (presses the button at the right moment, draws the reference pages
  and 30 s of live strokes), used by `ardoise.exe --diagnostic --simulated` and by the tests.

## Decisions made while building M1

- **Virtual time (simulator and replay).** Every event a simulated transport will deliver carries a virtual
  timestamp. `recv(timeout)`: if the next event is due at or before `now + timeout`, set `now = max(now, due)` and
  deliver it; otherwise advance `now` by `timeout` and return `None`. Command replies are due at the time of the
  request; live records are due 5 ms apart from the start of live mode (3 records per `a1` notification). The
  virtual clock's wall-clock origin is a fixed constant, so simulated sessions (and their logs) are reproducible.
- **Replay rules.** Entries are consumed in log order, ignoring `note` lines. On `write`, the next pending entry must
  be a `tx` with the same bytes (SET_TIME payload exempt only in lenient mode, for real captures); anything else is
  `TransportError::Replay`. On `recv`, a pending `rx` entry is delivered when the replay clock reaches its recorded
  `t` (same rule as virtual time; `t` is relative to the log creation, and the replay clock starts at the same
  origin), an `event: disconnected` becomes `TransportEvent::Disconnected`, and a pending `tx` means nothing to
  deliver (the clock advances by `timeout`). `connected` / `disconnect` events delimit successive connections. A
  recorded registration is replayed by giving the recorded identity to `Manager::with_host_id` (or
  `DiagnosticConfig::host_id`).
- **Real captures (H1).** `crates/ardoise-sim/tests/real_captures.rs` runs on the journals of the real Slate A4 in
  `testdata/private/captures/` (git-ignored; the tests skip themselves without it, and nothing taken from a journal is
  written in the test). It checks that every frame of every journal decodes with `ardoise-proto` (the unexplained
  frames of protocol.md §12 count as named), that the pages downloaded in a former-order session decode, and that the
  guided test recorded with the current command order replays through `diagnostic::run` with the same report, the
  same page files and the same exchanges (SET_TIME aside). The replay rules above are unchanged; the test only adds
  what a journal shows in its notes and times: the person's waiting time before a prompt, the duration of a scan or of
  a connection attempt, and that an attempt failed ("direct reconnection failed", `TransportError::NotFound`).
  Journals of the former order (`ec` after paper mode) cannot replay against the current host: decoding only.
- **Simulated user.** `SimulatedUser` performs the physical actions on the simulated tablet: draws the reference pages
  on `DrawReferencePages`, holds the button on `HoldButtonToRegister`, presses it on `PressButtonToConfirm` and
  `PressButton`, queues pen movement on `DrawLive`. Wrapping another operator (the interface), it first forwards the
  prompt and acts only if the answer is `Continue`; alone, it always continues.
- **Simulated runs never touch the user's library**: `--diagnostic --simulated` and the interface's simulation option
  use a fresh library in `%TEMP%`, so a simulated registration cannot replace the real tablet's `device.json`.
- **Log size (to revisit in M4).** The whole log is rewritten at most every 2 s. Fine for diagnostic sessions; for
  hours of live mode in class, rotate files (e.g. every 1 MiB) so Nextcloud does not re-upload a growing file.
- **Draining before each write.** `Session` calls `recv(Duration::ZERO)` until nothing is left before every
  write: a lost link is noticed before writing, and command frames already received (late answers to an earlier,
  timed-out request) are discarded. Virtual-time transports deliver on `recv(0)` only events already due and never
  advance the clock.
- **Connection boundaries in the log.** `LoggingTransport` logs `event: disconnect` on an explicit `disconnect()`
  and also when it is dropped without one (never twice); `disconnected` is logged when the link is lost. Replay
  uses these events as connection boundaries.
- **BLE notification loss (to check in H1).** btleplug's WinRT backend buffers 16 notifications per peripheral and
  drops the rest silently if our task falls behind. A page download then fails its CRC (retried once, nothing is
  deleted); live mode would show gaps. If H1 shows it, move the notification handler to direct WinRT
  `ValueChanged` callbacks feeding an unbounded channel (the plan B of SPEC §6.2).

## H1: what the first real-tablet session changed (2026-11-02)

Evidence: the diagnostic journals of the user's Slate A4 (protocol.md §5.3 and §12; the journals are private and
stay out of git).

- **`ec` before paper mode.** The Slate refuses `ec` in paper mode (`b3 01 02`), and the connection read the page
  count in paper mode before the sync sent `ec`: nothing synced in 19 of 23 sessions. `Session` now sends `ec` once
  per link before the first paper mode (Tuhi's order), again after live mode (from idle mode: the only sync that
  worked did that), never in paper or live mode; a refusal is retried once after `b1 01 02`. `set_mode(Paper)` tries
  it (a refusal does not stop the page count, so the tablet still connects); `sync` and the service's sync require
  it. The simulator refuses `ec` in paper mode (live mode too, supposed), refuses `c3` on a link without `ec`
  (supposed) and starts a connection in idle mode (supposed): the order mistake fails the guided test, the service
  and replay tests, and `ardoise-sim/tests/sync_order.rs`.
- **Failed syncs are failures.** A sync that cannot start (refusal) or stops early sets `LastSync.error` (kept
  across reconnections, shown in the panel) and `status.error` (`syncRefused` for a refusal), is noted in the log and
  writes it. The link is closed after a sync that could not start (the next connection tries again).
- **Recount after a sync.** After a sync that deleted pages the count is read at once (not 30 s later), so a page
  finished during the sync is synced on the same connection.
- **No "any tablet" fallback.** The registered tablet came late in the scan slice (its advertisements arrive at any
  moment) and was accepted by the "any tablet after two thirds" fallback; its address never changed. The scan now
  waits for the registered address only and notes other Wacom tablets in range; another tablet is never connected
  to without the user.
- **Battery event, known frames.** `ardoise_proto::sysevent` decodes `f1 02 <percent> <flag>` (battery level, shown
  at once) and names `ef 01 00`; `live` names `a3 …`. They are not "unclassified" any more. `cb 01 00`, first named the
  same way, turned out to be the live-mode button (`live::BUTTON_COMMAND`, confirmed on the real tablet on 2026-11-05); the
  simulator sends it too.
- **Logs end.** Each written service log ends with the disconnection and `end of this log: <why>`; the app stops the
  tablet service (and cancels a guided test) at exit, waiting at most 3 s.

## M10: the guided test keeps the tablet's pages as notes

`DiagnosticConfig::page_import` (a `PageImport`, shareable) turns each downloaded page into a note of the library
before the session deletes it from the tablet, like the normal synchronisation: the app gives `tablet::store_page_in`
(thumbnail, automatic export, the tablet settings' orientation); without one the runner uses a plain `LibrarySink`.
`ImportedPages` recognises a page already in the library (an earlier test, the normal sync): not stored twice. A page
that cannot be stored stays on the tablet (the sink's `Err`). The raw `page-N.bin` and the `.svg` preview are still
written in the diagnostics folder. The step's report has a `notes` detail (notes added).
