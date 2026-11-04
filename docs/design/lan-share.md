# Sharing the sheet on the local network (milestone M11)

Developer document (English). Requirements: SPEC §4.10, §6.2 ("Réseau"), §8; ROADMAP M11 and H4;
`docs/questions.md` Q4 (the user's decision). Builds on [tablet-live-classroom.md](tablet-live-classroom.md)
(`LiveHub`, batches, presentation) and [interface.md](interface.md) (editor, translations, browser stand-in).

```
 tablet ─► LiveHub ── LiveBatch (4 ms) ──► main window, presentation window      (unchanged)
              │
              └── one subscription per viewer ("share:<id>") ── HubFeed (app share/feed.rs: allow list, JSON)
                                                                     │
 editor ── share_show { note } (debounced) ── ShareRuntime ──────────┤
 Home / LiveView ── share_show { live | waiting }                     ▼
                                                     ardoise-share::Share (own thread, tokio current-thread)
                                                       ├─ GET  /              viewer.html  ┐ bytes from the exe's
                                                       ├─ GET  /assets/<file> viewer only ┘ embedded build
                                                       ├─ POST /join          code → HttpOnly cookie
                                                       └─ GET  /events        Server-Sent Events (read only)
                                                                     │  LAN, IPv4, port 8765
                                                                     ▼
                                       other PC: any browser ─ viewer.html (no Tauri): code form,
                                       LiveState + LiveRenderer + ardoise-ink WebAssembly, full screen
```

## Rules

- Everything in [tablet-live-classroom.md](tablet-live-classroom.md) still holds. The tablet's input is never held up
  by the share: sends to viewers never block (bounded queues, a slow viewer is disconnected and reconnects).
- **Only the Ardoise PC sends.** The browser receives; the only thing it sends is the code. No request path ever
  reaches the file system, the library or Tauri's asset resolver: the server looks paths up, verbatim, in a map of
  bytes built when the share starts.
- **Nothing personal leaves the PC**: no library path, note title, tag, file name, monitor name or error message
  is ever in a message (see "What a viewer receives").
- Started by hand, stopped by hand or when Ardoise exits. Never runs `netsh` nor changes any system setting.
- No panic on network input; no `unwrap`/`expect` outside tests; typed errors (`thiserror`) in the crate.
- CRLF files keep CRLF.

## Server: crate `ardoise-share`

Independent of Tauri, tested on Linux with real TCP clients (`crates/ardoise-share/tests/`).

### Choice of the HTTP server

A **minimal HTTP/1.1 server written for this crate on `tokio` (`net`, `io-util`, `time`, `sync`, `rt`, `macros`) and
`httparse`**, instead of a framework:

| Option | In the Windows build today | Effect on the exe | Notes |
|---|---|---|---|
| hyper 1 + hyper-util | no (only in the Linux build, through Tauri's `reqwest`) | +~400 KB | general purpose: keep-alive, chunked bodies, HTTP/2 we do not need |
| axum | no | more | brings hyper, tower, routing |
| tiny_http | no | +~150 KB, 4 crates | one thread per connection; stopping it cleanly is awkward |
| **tokio + httparse** | **yes, both** (`tokio` with these features through btleplug, rmcp and Tauri; `httparse` through `ureq`) | **our code only** | three routes, bounded requests, close after each response |

The server has three routes and answers one request per connection (`Connection: close`), except the event stream,
which stays open. That is a few hundred lines with a very small attack surface: request heads are read with a size
limit and a timeout and parsed by `httparse` (the parser hyper uses), bodies only on `POST /join` with
`Content-Length` ≤ 1 KiB, no chunked request bodies, nothing pipelined is read. Licences: tokio MIT, httparse
MIT OR Apache-2.0, both already listed in `THIRD-PARTY-NOTICES`.

New crates: **`qrcodegen` 1.8** (MIT, Project Nayuki, no dependency, a few tens of KB) for the QR code. `getrandom`
0.4 (MIT OR Apache-2.0) is already in the tree (through `uuid`); the crate depends on it directly for the code and
the tokens. `serde` / `serde_json` as everywhere. On Windows only, `windows-sys` 0.61 (MIT OR Apache-2.0) for one
`setsockopt`, with the feature mio already enables: no new code in the exe. In the tests only, `socket2` 0.6 (MIT
OR Apache-2.0, already in the tree through tokio) for clients bound to other loopback addresses.

### Threads and lifetime

- `Share::start` binds the listener **synchronously** (so a port that cannot be bound is an error at once, after the
  following ports were tried: see "Port choice and fallback"), builds a
  current-thread tokio runtime and runs it on a thread of its own (`ardoise-share`). Each connection is a task.
- `Share::stop` (also on `Drop`): every stream gets the `end` message, every connection is closed, the listener is
  dropped, and the thread is joined (at most 2 s). When `stop` returns, the port is closed: a new connection is
  refused. The app calls it from `share_stop` and from `RunEvent::Exit`.
- Listening: `0.0.0.0:<port>` (all IPv4 interfaces), `port` from `ardoise.toml` (`[share] port = 8765`): the address
  stays the same from one share to the next, and an IT department can open that port. Port 0 (tests) takes a free
  port; `Share::port()` says which. **The rule Windows creates from its firewall prompt is for the program's path**
  (any port), not for the port: moving or renaming the portable `ardoise.exe` brings the prompt back; changing the
  port does not.
- The listening socket is made with tokio's `TcpSocket`. On Windows it gets **`SO_EXCLUSIVEADDRUSE`** before `bind`
  (through `windows-sys` 0.61, whose `Win32_Networking_WinSock` feature mio already compiles; no administrator
  rights needed): while the share listens, no other socket can bind its port, on any address. Microsoft documents
  that without it a socket of the same user that sets `SO_REUSEADDR` may bind the port (for example a single address
  next to our wildcard, and take the connections to it). On Windows 11 our test sees a takeover of the share's own
  address refused even without the option (`WSAEACCES`), and a test cannot bind the wildcard without the firewall
  prompt; a unit test reads the option back from the socket. A share stopped and started again at once still gets
  its port back (tested on Windows: the connections it closed first stay in `TIME_WAIT` without blocking the new
  bind). Elsewhere `SO_REUSEADDR`, as std and mio set it on Unix (a listening port is never shared there).
- Port choice and fallback (Q7, user's decision of 2026-11-02): a program already on the port (the Python helper of
  an MCP server held 127.0.0.1:8765 on the user's PC, which conflicts with our exclusive `0.0.0.0` bind) must not
  stop the share. `ShareConfig::fallback_ports` (the app passes `FALLBACK_PORTS` = 20) is how many ports after
  `port` are tried in order, never past 65535; port 0 is bound once. Each port is **really bound** (no "is it free?"
  check followed by a bind, so nothing can take it in between); only `AddrInUse` and `PermissionDenied` (what
  Windows answers for the ports of Hyper-V / WSL excluded ranges) move on to the next port, any other error ends
  the search. `Share::port()` is the port bound, `Share::requested_port()` the one asked for. Errors:
  `ShareError::PortInUse { port }` (code `sharePortInUse`) when only one port was asked for, and
  `ShareError::NoFreePort { first, last }` (code `shareNoFreePort`) when a range was tried and nothing bound. Neither
  message points to `ardoise.toml`: they point to the setting "Port du partage".
- Limits (a hostile classmate's PC must never keep the projector's PC out; `Limits` in `lib.rs`):
  - **64 connections** at once, **8 per address** (more are closed at once). **16 of the 64 are reserved for the
    addresses that hold a session token** (they joined with the code): the PCs without the code share the other
    48, so a screen that joined always gets back in (reconnection, page reload), whatever they do.
  - When there is no room, one connection is closed for the newcomer, never an event stream: the oldest one that
    has not sent its request head yet, if it is of the newcomer's own address or of an address that holds more
    slots; else the oldest request of the address that holds the most slots, if it holds **at least two more** than
    the newcomer's (fair share); a newcomer from an address with a token first takes the slot of the oldest
    connection without one. Otherwise the newcomer is closed at once. So PCs that hold their slots with requests
    they never finish (an asset they never read, a `/join` body that never comes) and reconnect at once lose a slot
    to every newcomer that holds fewer, and never take it back (addresses that hold about as many never swap
    slots): the projector loads the page while it holds fewer than they do, and its connections are never theirs
    to take. The age of a connection plays no part: a PC that recycles its connections every second gains nothing.
  - Known limit: PCs without the code that use **48 addresses or more**, one slot each, leave no address holding
    two more than a newcomer: a browser that has not joined yet is then closed at once (one that has joined uses the
    reserved slots). A laptop takes that many addresses only with administrator rights and a network that lets it.
  - **32 event streams** and **4 per address** (more get `503` + `Retry-After: 5`). An address opens at most **8
    streams in a row, then one every 2 s** (`StreamRate`, a token bucket; more get `503` with `Retry-After`, the
    seconds until the next): each new stream makes the app copy and serialise the whole page under the live hub's
    lock. The viewer opens one stream per page load; after a cut it retries after 0.5 s, 1, 2, 4, then every 5 s
    (its count starting again after each stream that opens) and treats `503` as "retry in 5 s": a viewer whose
    stream is cut over and over gets 8 quick reconnections, then about one every 2 s, with a 5 s wait after a
    refusal. A holder of the code looping on reconnections makes the app copy the page at most every 2 s per
    address.
  - The first byte of a request within 20 s, its head (16 KiB) within 30 s of the connection (and the body of
    `/join`, 1 KiB), 32 headers. 20 s is longer than browsers keep a connection opened ahead of a request (Chrome:
    10 s): with 3 s, Chrome on Android sent its request on such connections just as the server closed them, retried
    twice and gave up (`ERR_TOO_MANY_RETRIES`, 2026-11-03). A waiting connection takes no room from others: it is
    the first closed to make room. A first byte that is not a letter is no HTTP request — above all a browser
    trying `https://` (a TLS handshake starts with 0x16) — and is closed at once, so that the browser falls back
    to `http://` without waiting; **an answer written within 5 s** (the largest, a script of about 200 KB, takes milliseconds on a LAN),
    then the browser closes first (the server waits up to 2 s, see "Through a school's filtering proxy"). A head is parsed again only when a blank line may have arrived.
    `TCP_NODELAY` on every socket (no Nagle delay on the stream).

### Routes

The only `(method, path)` pairs that exist; the query string is ignored, nothing is percent-decoded or normalised.
**Any other request gets `404`** (also another method on a known path).

**`Host` first.** A request whose `Host` header does not name the share gets **`421 Misdirected Request`** before
any route, and is not counted as an attempt: the share's names are an IPv4 address (any: the PC may have several),
`localhost`, and on Windows this computer's name (`COMPUTERNAME`, used only if it is a single label of letters,
digits and `-`), each with the share's port or without a port. A page of another site whose DNS name was pointed at
this PC (DNS rebinding), opened in the projector's own browser, names its own host: it can neither spend the
attempts of that address nor take its slots with requests that count as same-origin. No `Host` header (not a
browser: browsers always send one) is accepted; two are refused. Other names of the PC (a DNS name with a suffix,
`<name>.local`) are refused too: the panel shows addresses, the guide says to type one.

| Request | Answer |
|---|---|
| `GET /` | `viewer.html` |
| `GET /assets/<file>` | that viewer asset, if `/assets/<file>` is a key of the asset map; else 404 |
| `POST /join` | body `{"code":"K7M Q2X"}` (JSON). Right code: `200` `{"joined":true}` (a body with its length: a `204`, which has none, held the browser 2 minutes behind a school's filtering proxy) + `Set-Cookie: ardoise_share=<token>; Path=/; HttpOnly; SameSite=Strict`. Wrong: `403` `{"error":"wrongCode","remaining":n}`. Too many attempts: `429` + `Retry-After: s`, `{"error":"tooManyAttempts","retryAfter":s}`. Bad request (no JSON, too long, chunked): `400` / `413`. `Content-Type` other than `application/json`: `415`; an `Origin` header other than `http://<Host>` (the `Host` already checked, see above): `403` `{"error":"badOrigin"}` (a page of another site cannot spend the attempts of the projector's browser); neither counts as an attempt |
| `GET /events` | valid cookie: `200`, `text/event-stream`, the stream (below). Missing or unknown token: `401`. Too many streams, or this address opened streams too often: `503` + `Retry-After` |

Headers of every answer: `Connection: close`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`,
`Cache-Control: no-store` (assets: `public, max-age=31536000, immutable`, their names carry a hash). `viewer.html`
also gets `Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'
'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; form-action 'none';
frame-ancestors 'none'` and `X-Frame-Options: DENY`. Content types come from the asset map.

### Assets

`Assets` is a map from the request path (`/` is stored as `/`, the others as `/assets/<file>`) to
`Asset { content_type, bytes: Arc<[u8]> }`, built by the app when the share starts (below) or by the tests from
bytes. The server only ever does `map.get(path)`: `/assets/../x`, `/assets/%2e%2e/x`, `//assets/x`, `/assets\x`,
`/ASSETS/x`, `/assets/x?y` (query ignored → `/assets/x` if it exists), `/index.html`, `/viewer.html`,
`/_ardoise/…` are not keys, so 404. The tests check these and more.

### Access: code, token, attempts

- **Code**: 6 characters drawn uniformly (rejection sampling on `getrandom` bytes) from the 31 unambiguous symbols
  `ABCDEFGHJKMNPQRSTUVWXYZ23456789` (no `0 O 1 I L`): 31⁶ ≈ 8.9 × 10⁸ codes. New for each share. Shown as two
  groups of three (`K7M Q2X`); the page accepts it in any case, with or without spaces or a dash.
- **Token**: 32 random bytes in hex, one per successful join, valid until the share stops; at most 8 per address
  (an address's new join forgets its own oldest, never another address's) and 256 in all (then the oldest token of
  the address that holds the most is forgotten: an address with a single token, the projector, keeps it while
  others hold several). Compared in constant time. An address that holds a token may use the reserved connection
  slots (above).
- **Attempts**, per IP address: at most **5 wrong codes in 5 minutes**; the 6th attempt (right or wrong) within the
  window gets `429` with the seconds until the oldest wrong code is 5 minutes old. A right code clears the
  address's record. At most 4 096 addresses remembered: when all have running records, nothing is evicted and a
  new address gets `429` until the first record expires (cycling through addresses gains nothing). **Nearby
  addresses have room of their own**: an address on the same /24 network as one of this PC's addresses (found at
  share start by `addresses::local_ipv4()`), or one of the first 64 addresses that joined, keeps its record in a
  second table that is never full by construction (at most 254 addresses per address of this PC, plus 64). PCs with
  thousands of addresses elsewhere on a large network therefore cannot keep the projector of the classroom's
  network from typing the code; nearby addresses still have their 5 wrong codes in 5 minutes. Per address only: a
  classmate's PC can lock itself out, never the projector's PC.
- **Brute force**: the code changes at each share; it is one of 31⁶ = 887 503 681. With N addresses, at most 5·N
  tries per 5 minutes (60·N an hour): 100 PCs trying all the time make 6 000 tries an hour, about 1 chance in
  148 000. However many addresses an attacker has, the two tables hold at most 4 096 + 254 per address of this PC
  + 64 running records: with one address, at most 4 414 × 60 ≈ 265 000 tries an hour, about 1 chance in 3 350 per
  hour of share (an attacker with thousands of addresses on the network).
- The page and its assets are public (they are the GPL viewer, no data); only `/events` needs the token.

### The event stream

`text/event-stream`, one SSE message per server message: `data: <JSON on one line>\n\n`; a comment `: ping\n\n`
every 15 s (a dead connection is found and dropped); `retry: 2000` at the start. No event ids: a viewer that
reconnects gets a fresh snapshot. Messages (`type` field; TypeScript in `app/ui/src/lib/share.ts`):

| Message | Meaning |
|---|---|
| `{"type":"waiting"}` | the Ardoise PC shows nothing to share: waiting screen |
| `{"type":"live","reset":bool,"batch":LiveBatch}` | a batch of live mode, as the presentation window gets it (stripped, see below). `reset: true` on the first batch of a subscription: it starts with a snapshot, the viewer starts again from it |
| `{"type":"livePaper","key":string,"picture":string}` | the paper of a live page that is not plain (a resumed sheet: template, texture, background image, page of a PDF), a PNG data URL, drawn under the strokes while the page's snapshot has `paperKey` = `key`. Never inside a batch, at most once per picture to each viewer, right after the snapshot to a viewer that joins. Ordinary live pages never get one (see "The paper of a live page") |
| `{"type":"note","page":SharedPage,"background":string\|null}` | the whole page of the note open in the editor; `background`: the paper (template, texture, background image) as a PNG data URL, `null` when it is plain paper (the paper colour is drawn) |
| `{"type":"noteStrokes","strokes":Stroke[]}` | strokes added at the end of the page last sent (the common case while writing in the editor) |
| `{"type":"pause","paused":bool}` | the share is paused (the image is frozen; the viewer shows a discreet badge) or resumed |
| `{"type":"end"}` | the share stops: the viewer closes the stream and shows "sharing ended" |

`SharedPage = { id, page: PageSize, background: Background, layers: Layer[], strokes: Stroke[] }` (the `Note`
shapes of `note.ts`; layer names are emptied, the viewer only needs `visible`; strokes of a hidden layer are left out). A `note` message **without**
`background` (sent to the viewers already showing a note, when only the page changed) keeps the paper shown: the
picture, which may weigh megabytes (texture, background image), goes only when the paper changes, to a viewer that
joins, and when the viewers come back from another content.

### Content, always chosen by the Ardoise PC

The crate keeps what is shown (`Shown::Waiting | Live | Note`), the last note message, and whether it is paused:

- `show_waiting()`: every viewer gets `waiting`.
- `show_note(NoteUpdate { full, append })`: `full` is the complete `note` message, kept for viewers that join later;
  `append` (optional) a `noteStrokes` message relative to the previous `show_note`. Viewers already showing that
  note get `append` when given, else `full`; switching from another content always sends `full`.
- `show_live()`: every viewer is handed to the `LiveFeed` (`start`), which sends it a first batch starting with a
  snapshot (`reset: true`), then every batch. Leaving live mode calls `LiveFeed::stop` for each viewer.
- `show_live_paper(message)`: the `livePaper` message of the live page, `None` on plain paper (see "The paper of a live
  page"). Recorded; sent at once only to the viewers following live mode.
- **A viewer that connects** gets what is shown: `waiting`, the last `full` note message, or a live subscription
  (snapshot, then the paper if any, then increments). **Late joiners therefore get a snapshot, then increments**, in
  every case.
- **Pause** (`set_paused(true)`): live viewers are stopped (`LiveFeed::stop`); the frozen image is what is shown at
  that moment: for live mode `LiveFeed::freeze()` (the session as one batch starting with a snapshot, sent with
  `reset: true` to every viewer so that all show the same frozen frame), for a note its last `full`, else
  `waiting`; then `pause` true. Viewers joining during the pause get the frozen image (with the paper it had, between
  the image and the badge) and `pause` true. Content changes during the pause are recorded and sent nowhere, a new
  paper included. `set_paused(false)`: `pause` false, then the current content as for a new viewer (live: a new
  subscription with a fresh snapshot, then the paper if the viewer does not have that one).
- **Queues**: each viewer has a bounded queue (1 024 messages). A send never blocks; a full queue disconnects that
  viewer (`LiveViewer::send` returns `Err`, the hub drops the subscription), and the browser reconnects and gets a
  fresh snapshot. This keeps the hub's flusher thread free whatever a viewer's network does.

### Public API (fixed in the contracts commit; changes only with a reason in the report)

```rust
pub struct ShareConfig { pub bind: Ipv4Addr /* UNSPECIFIED */, pub port: u16, pub fallback_ports: u16 /* 20 */, pub code: Option<String> /* tests */ }
pub struct Asset { pub content_type: String, pub bytes: Arc<[u8]> }
pub type Assets = HashMap<String, Asset>;           // "/" and "/assets/<file>"
pub enum Shown { Waiting, Live, Note }              // serde camelCase
pub struct ShareStatus { pub port: u16, pub code: String, pub viewers: usize, pub paused: bool, pub shown: Shown }
pub struct NoteUpdate { pub full: String, pub append: Option<String> }   // complete JSON messages
pub trait LiveFeed: Send + Sync + 'static {
    fn start(&self, viewer: LiveViewer);            // first batch: a snapshot; then every batch
    fn stop(&self, viewer: u64);
    fn freeze(&self) -> Option<String>;             // LiveBatch JSON starting with a snapshot
}
#[derive(Clone)] pub struct LiveViewer { .. }       // id(), send(&self, batch_json: &str) -> Result<(), ViewerGone>
pub type OnChange = Box<dyn Fn(ShareStatus) + Send + Sync>;
impl Share {
    pub fn start(config, assets, feed: Arc<dyn LiveFeed>, on_change: OnChange) -> Result<Share, ShareError>;
    pub fn port(&self) -> u16; pub fn requested_port(&self) -> u16; pub fn code(&self) -> String; pub fn status(&self) -> ShareStatus;
    pub fn show_waiting(&self); pub fn show_live(&self); pub fn show_note(&self, update: NoteUpdate);
    pub fn show_live_paper(&self, message: Option<&str>);   // not from a LiveFeed method, nor the hub's flusher
    pub fn set_paused(&self, paused: bool); pub fn stop(&self);
}
pub fn format_code(code: &str) -> String;           // "K7MQ2X" → "K7M Q2X"
pub mod addresses { pub fn local_ipv4() -> Vec<Ipv4Addr>; }
pub mod qr { pub struct QrCode { pub size: u32, pub path: String } pub fn qr_code(text: &str) -> Option<QrCode>; }
```

`LiveFeed` methods may be called with the crate's lock held: they must not call back into `Share`.
`LiveViewer::send` never takes that lock (it only pushes to the viewer's queue). `on_change` is called (outside the
lock) when the number of viewers, the pause or what is shown changes.

### Addresses and QR code

- `addresses::local_ipv4()`: the IPv4 address of the interface with the default route, found by "connecting" a UDP
  socket to a public address (a routing lookup: no packet is sent), then the same for the private ranges
  (`10.x`, `172.16–31.x`, `192.168.x`) in case the LAN has no gateway; loopback, `0.0.0.0` and link-local
  `169.254.x` are dropped, duplicates removed, **private addresses first** (RFC 1918), then the others. No
  dependency, no `unsafe`, the same code on Windows and Linux. Virtual adapters (WSL, Hyper-V) are not listed
  unless the route goes through them, which avoids proposing an address the other PC cannot reach.
- `qr::qr_code(text)`: `qrcodegen`, error correction Medium, no border (the interface adds the quiet zone);
  `path` is SVG path data, one `M x y h1v1h-1z` per dark module, drawn in a `viewBox="0 0 size size"`.
- The QR code holds the first address only (`http://192.168.1.23:8765/`), never the code: the page asks for it
  (SPEC §4.10).

## App: `app/src-tauri/src/share.rs`

### Settings

`ardoise_core::settings::ShareSettings` (`[share]`, shared by every PC, unknown keys kept): `port: u16` (default
8765; 0 or a missing value loads as 8765).

**The port of this PC.** `ardoise.toml` sits next to the portable `ardoise.exe`, in a folder (Nextcloud) that several
PCs share, and a conflict on a port belongs to one PC. So the port found by the fallback is kept in this machine's
table, `[machines."<NAME>"] share_port = 8766` (`MachineSettings::share_port`), and `[share] port` stays the shared
default. The effective port (`Settings::share_port_for(machine)`) is this machine's `share_port` if set, else
`[share] port`, else 8765. A zero, out of range or mistyped value of either loads as unset (the rest of the file is
still read); writes go through the usual atomic `SettingsStore::update`, which re-reads the file first, so the
sections of the other PCs and the unknown keys are kept. `share_start` asks for the effective port; when the share
bound another one it writes that one as this machine's `share_port` (a settings file that cannot be written is only
logged: the share runs). The settings screen sets the same key (`set_share_port`, 1024 to 65535; the next share uses
it; a running share keeps its port).

Test-only environment variable **`ARDOISE_SHARE_LOOPBACK=1`**: the
server listens on `127.0.0.1` instead of `0.0.0.0` (the e2e journeys: no firewall prompt, no firewall rule left
for each temporary copy of the exe).

### Viewer assets from the executable

`lib.rs` takes the embedded assets out of `tauri::generate_context!()` (`Context::set_assets`) and puts them back
wrapped in an `Arc` (`ShareAssets`, a delegating `tauri::Assets`), so the share reads them **by exact key, without
Tauri's resolver** (which percent-decodes paths and falls back to `index.html`). At share start
`viewer_assets(get)` reads `/.vite/manifest.json` (Vite's build manifest, `build.manifest: true`), takes the
entry `viewer.html` and, recursively, its `file`, `css`, `assets` and `imports` (the shared chunks, the stylesheet,
`ardoise_ink-*.wasm`), and copies those files into the `Assets` map (`viewer.html` under `/`). A missing manifest,
a missing WebAssembly module in that closure (the viewer would draw no stroke), or a missing file (a development build that loads the interface from the dev server) is `shareUnavailable`. Content types by
extension (`html`, `js`, `css`, `wasm`, `svg`, `png`, `woff2`, else `application/octet-stream`). Done once per
share start, a few hundred KB.

### What a viewer receives (`HubFeed`, stripping)

`HubFeed { hub: Arc<LiveHub> }` implements `LiveFeed`:

- `start(viewer)`: `hub.subscribe("share:<id>", ShareSink)`: the hub's own per-window queue, so a viewer is a
  subscriber like a window and the pipeline is the presentation window's (no second one). Its first batch starts
  with the snapshot and the stroke in progress, exactly as for a window.
- `stop(id)`: `hub.unsubscribe("share:<id>")`. `freeze()`: `hub.current_events()` (new: the snapshot and the stroke
  in progress, taken under the hub's lock) as one batch.
- `ShareSink::send(batch)` writes the batch from an **allow list** (`batch_json`: `SharedEvent`, `SharedSnapshot`,
  `SharedLivePage` in `share/feed.rs`, borrowed views of the hub's types) and calls `viewer.send`; an `Err` drops
  the subscription (the hub's existing rule). A field added later to `LiveSnapshot` or `Note` therefore never
  reaches a browser until it is added there on purpose. `saved`, `saveFailed` and `sheetForked` events are left out
  (library paths, system messages); in a snapshot `saved` = `[]`, `presentation` = `null`, `origin` =
  `{"kind":"ardoise"}`, and the page keeps only what is drawn: `id` (a digest salted at each run, `viewer_page_id`, not the note's), `page`, `background`, `layers` (names emptied),
  `strokes`; `created` and `modified` are 0 (a resumed note keeps its date on the PC). Strokes of a hidden layer are left out here as in `SharedPage` (a teacher hides an "answers" layer to
  keep it to themselves); the layer list stays whole so that stroke indices stay valid. The background image's
  description (file name, PDF page, place) is not sent either: only `paperKey`, next to the page, when the paper is not
  plain (below). `strokesEdited` (M12) carries only `pageIndex`, `removed`, `inserted`, `recoloured`, numbered as in the
  viewer's list: `ViewerIndices`, worked out with the edit on a page with a hidden layer (no revision, no history: nothing
  there is drawn).
- Serialising runs on the hub's flusher thread, as for the windows' channels, and never waits. A `Memo` shared by
  the viewers' sinks keeps the last batch and its text (`Arc<str>`): viewers that were up to date get equal batches
  one after the other, and the text (the whole page, after a page change) is written once for all of them.

### The paper of a live page (M12)

A session seeded from a note (`live_resume`) has a page that may have a template, a texture and a background image or
page of a PDF. A browser cannot draw them, and the hub's events carry no pixels. Every other live page is plain paper:
white, or slate with the blackboard, which the viewer fills with the paper colour; **nothing below happens for them**: no
key, no message, no thread (a test checks it).

- **The key** (`paper_key`, `share/live_paper.rs`). `SharedSnapshot` has `paperKey`: 16 hex digits, a digest of the note's
  id, the page size, the paper as drawn (slate while the blackboard is on, like `effectiveBackground`), the template, the
  texture and the image's description. Absent on plain paper. It is opaque and tells nothing of the note; it costs a
  hash per snapshot of such a page.
- **The picture.** The viewer keeps the last `livePaper` apart from the sheet (`ViewerState.livePaper`, which a `live` with
  `reset` does not touch) and draws it (`ViewerState.paper`) only while `live.paperKey` equals its `key`. So a page change
  shows or hides it with the snapshot itself, at once, in whatever order the two arrive (the paper may overtake the
  batch: the two go by different roads), and going to a plain page clears it **without any message**. Back on the sheet
  the viewer still has the picture: nothing is sent, nothing flashes.
- **Rendering.** What `page.rs` does for the editor's note: `background_png` at `PAPER_PX` (2 560 px) on the longer side, as
  a PNG data URL; the pixels are the session's (`ResumeBase::picture`, decoded once), not the editor's staging area. The
  two last pictures are kept (`LivePapers::pictures`): the sheet and the sheet on the blackboard.
- **Threads.** `ShareSink::send`, on the hub's flusher, calls `LivePapers::saw` for each snapshot in a batch: a comparison
  under a small lock of its own. Only when the paper changed does it start the `share-paper` thread, which ends when
  there is nothing more to do. That thread takes the hub's lock once, to find the file (`LiveHub::page_image`, no pixels
  copied), renders with no lock, and then calls `Share::show_live_paper` (the content's lock, which the flusher never
  takes). A paper overtaken while it was rendering is not sent. A new subscription (`LiveFeed::start`) asks for a pass
  even for the same key: viewers come back from the editor, where the note's file may have got other pixels under the same
  name and place; the pixels' hash finds out (same pixels: the same message, no render, nothing sent).
- **Once per viewer** (`content.rs`). Each viewer's queue remembers the frame it last got (`Queue::paper`, compared by
  identity); `show_live_paper` compares the bytes once with the last frame there was (`State::last_paper`) and keeps
  that frame when they are the same. So a viewer gets a picture once whatever goes back and forth: strokes, plain page and
  back, pause and resume, the editor's note or the waiting screen and back. A viewer that reconnects is a new viewer: it
  gets it again. A new share too (the viewer forgets the picture on `end` and on `401`).
- **Pause.** The frozen image keeps the paper of that moment, also for a viewer joining during the pause. A paper that
  comes during the pause goes to nobody: it may be a sheet the teacher does not show yet. Known limit: a pause pressed in
  the second it takes to render a new paper freezes that sheet on its paper colour until the resume.
- **What leaves the PC.** `key` and `picture`, written from an allow list (`Message` in `live_paper.rs`). The PNG is made
  from a blank note with the paper and the image only: no stroke, hidden or not, can be in it. The real-TCP test looks
  for the title, tags, layer names, file names, the library and the hidden stroke in every message.
- **Cost for a viewer.** A photo may weigh several MB: the strokes that follow wait behind it on that viewer's stream
  only, once, as for a note's paper. The windows and the other viewers are not held up: sends never block. A fork (the
  sheet changed elsewhere) gives the page a new id, hence a new key: the picture goes once more.

### Through a school's filtering proxy (2026-11-03, measured at school)

At school, the viewer's browser reached the share through the school's filtering proxy (e2guardian on the router
`192.168.228.254`, port 3128: the browsers there use it even for local addresses). Joining took 2 minutes, then
everything was instant. The share's journal showed it three times: `POST /join` answered in a few milliseconds, then
`GET /events` arriving 119–120 s later. The cause was the join's answer: a `204` has no length, and some
intermediaries wait for the connection to time out (2 minutes) before passing such an answer on (the same symptom is
described on squid-users, November 2010). Since then the join answers `200 {"joined":true}` with its
`Content-Length`, and `GET /events` follows 70 ms after it through the same proxy. The event stream itself is not
held back by e2guardian: live strokes arrive at once.

The next day the page stayed on its code form with "1 screen connected" on the PC (2026-11-04). e2guardian keeps any
answer to itself until it has read enough of it to scan, whatever its type (`text/event-stream`, `text/plain`,
`application/octet-stream`, `application/json`…): measured through it, a stream was held up to 450 KB and passed from
512 KB, and each message then arrived at once. A note with a large paper had got through the day before; a small page or
the waiting screen never did. So a stream whose request says it came through a proxy (`Via`, `Forwarded`,
`X-Forwarded-For`) starts with 2 MiB of blank SSE comments (a few hundred ms on a LAN, each time it connects). A filter
that adds none of these headers gets no padding: the page then says, after 12 s of "connecting", that a web filter may be
holding the connection. It leaves the code form as soon as the code is accepted: another click would only open one more
stream, counted as one more screen on the PC.

A first guess was wrong and is noted here so that it is not made again: that this PC, closing each connection first,
kept its address pairs reserved for 2 minutes (TIME_WAIT) and that the router's reused ports fell on them. Making the
browser close first changed nothing at school. It stays, as most servers do: after an answer the server waits up to
2 s for the browser to close (browsers close a `Connection: close` connection as soon as they have read the answer),
then closes itself.

### Journal

While a share runs, `<library>/_ardoise/diagnostics/share_<local time>.log` gets one line per connection event:
connected (address and port), what was asked (a route of the share, else "(other)"; "through a proxy" when a
`Via`, `Forwarded` or `X-Forwarded-For` header says so) and after how long, the status
answered and in how long, streams opened and closed, connections refused or closed to make room. Never the code, a
cookie, a token, a query string or an unknown path. It comes back to the developer with the library, like the tablet's
diagnostic logs.

### Marks written over the presentation (Q6, decided 2026-11-02)

The viewers show the temporary marks written over the presentation window (M8, `lib/live-marks.ts`), so that the
other screen shows exactly what the projector shows. The presentation window keeps drawing its own marks locally
(no echo, no added latency on the projector) and also reports them:

- `MarksSender` (`lib/live-marks-sender.ts`) calls `live_set_marks(pageIndex, marks)` with the whole list for the
  page, the mark in progress included, reduced to tool, colour, width and rounded points `{x, y, p}`; at most every
  50 ms, one call at a time (order kept); an empty list goes at once; closing the window sends `[]`.
- `LiveSession` keeps the list with the page it belongs to (index, orientation, page size). `set_marks` ignores a
  list for another page (one sent late for the page just left). `sanitize_marks` bounds it (200 marks, 4 000 points
  per mark, 20 000 in all, the first ones kept), drops non-finite points and clamps coordinates, pressure and width.
  A snapshot of another page, orientation or page size clears the marks, as the window does; so do closing the
  presentation window and a new session. Clearing the page, a style or a view change keep them, as in the window.
- `LiveEvent::Marks { marks }` is coalesced like `View` (the latest list wins); a late subscriber gets it after its
  snapshot, and `current_events` includes it, so the frozen image of a pause shows the marks of that moment.
- `feed.rs` writes it from the allow list (`SharedMark`, `SharedMarkPoint`); the viewer's `LiveState.marks` feeds
  `buildScene`, which draws them as the presentation does. The control window stores the event and never draws it.
- Nothing about the marks reaches the note or the library.

### Note of the editor

`share_show({ kind: 'note', note, imageKey })`: the runtime builds `SharedPage` from the editor's note (unsaved state
included) and keeps the previous one: same `id`, page, background, background image and layers, and the old strokes
a prefix of the new ones → `append` = the new strokes; otherwise only `full`. The background is rendered by
`notes::background_png` (with the staged picture under `imageKey`, like `render_background`) at 2 560 px on the
longer side, **only when it changed** (cached by page size, background and image placement), in `spawn_blocking`;
plain paper (template `blank`, no texture, no image) gives `null`. Ordinary live pages are plain paper too (white, or
slate with the blackboard): the viewer draws their paper colour, as the app does before its PNG arrives. A resumed sheet
is not: "The paper of a live page" above.

### Commands (`share.rs`)

| Command | Arguments | Returns |
|---|---|---|
| `share_status` | – | `ShareStatus` |
| `share_subscribe` | `onEvent: Channel<ShareStatus>` | current `ShareStatus`; then the status at every change (one subscription per window) |
| `share_start` | – | `ShareStatus` (starts, or returns the running share). Errors `shareNoFreePort`, `sharePortInUse`, `shareUnavailable`, `shareFailed` |
| `share_port` | – | `SharePortSetting` `{ port, sharedPort, own }`: the port this PC asks for, `[share] port`, whether this PC has a port of its own |
| `set_share_port` | `port: number` | `SharePortSetting`; `sharePortInvalid` unless 1024 to 65535 |
| `share_stop` | – | `ShareStatus` (inactive); can be repeated |
| `share_set_paused` | `paused: boolean` | `ShareStatus` |
| `share_show` | `content: ShareContent` | – (ignored while no share runs) |

```ts
type ShareContent = { kind: 'waiting' } | { kind: 'live' } | { kind: 'note'; note: Note; imageKey: string | null };
interface ShareStatus {
  active: boolean;
  port: number;                       // the port listened on; the one this PC asks for when inactive
  requestedPort: number;              // the port asked for at the start; differs from `port` when it was held
  code: string | null;                // "K7M Q2X" while active
  addresses: string[];                // "http://192.168.1.23:8765/", private LAN first; [] when inactive
  qr: { size: number; path: string } | null;   // of addresses[0]
  viewers: number;
  paused: boolean;
  shown: 'waiting' | 'live' | 'note';
}
```

New error codes (`errorCodes.*`): `sharePortInUse`, `shareNoFreePort`, `sharePortInvalid`, `shareUnavailable`,
`shareFailed`. `share_start` while the UI
already said what it shows: the runtime remembers the last `share_show` content even while inactive, and shows it
when the share starts (the UI sends it again after a start anyway).

`RunEvent::Exit` (`lib.rs`) stops the share before the tablet: the port closes and every browser is told `end`.

## Interface

### In the app (main window)

- `lib/share.ts`: the types above and the messages. `lib/api.ts` section "LAN share (M11)": `shareStatus`,
  `shareSubscribe`, `shareStart`, `shareStop`, `shareSetPaused`, `shareShow`; browser stand-in `lib/mock-share.ts`
  (routed by `lib/mock.ts`).
- `lib/share-state.svelte.ts`: one subscription for the app, the status, the panel open or not, and **what is shown**:
  `showLive()`, `showNote(note, imageKey)` (debounced: sent 100 ms after the last change, at most every 300 ms while
  changes keep coming), `showWaiting()`. It always remembers the last content and sends it after `start`.
- Who says what is shown: `LiveView.svelte` (mounted → `showLive()`), `Editor.svelte` (open and every change of the
  note, from the same `doc` the drafts use → `showNote(...)`), `Home.svelte` (library, settings: `showWaiting()` when
  neither is open). The rule is "what the main window shows"; nothing else is ever shared.
- **"Partager sur le réseau"** button (`button[data-action="share"]`) in the live control window's toolbar and in the
  editor's toolbar; it opens the share panel (`.share-panel`), which starts the share.
- **Share panel** (`components/share/SharePanel.svelte`): the address(es) (`[data-share-address]`, the first one
  large, the others below), the QR code (`svg.share-qr`), the code (`[data-share-code]`, large, monospace), the
  number of connected screens (`[data-share-viewers]`, the number as the attribute value), what is shown, **Pause /
  Resume** (`button[data-action="share-pause"]`), **Stop sharing** (`button[data-action="share-stop"]`), a short help
  line (type the address in the other PC's browser, then the code; the firewall prompt: "Réseau privé"), a notice
  when the port asked for was held (`[data-share-port-moved]`, `status.port !== status.requestedPort`: "Port 8765 is
  used by another program: the share uses port 8766, kept for this PC."), errors in words (port in use, no free port:
  the range tried, and the setting to change). A failure is shown as the explanation alone (`sentence()` in `api.ts`:
  a capital and a full stop), without the "Something went wrong." that `genericError` puts before it elsewhere: the
  phrase of the error code already tells what to do.
- **Settings, "Partage sur le réseau"** (`components/SharePortSection.svelte`): "Port du partage", a number field
  (1024 to 65535, validated before saving, with a message in words), saved for this PC through `set_share_port`; says
  that the other computer types the new address, that the firewall permission follows the program (no new prompt),
  and that a running share keeps its port.
- While a share runs, an indicator in the top bar of the library and in the two toolbars
  (`.share-indicator`, with the number of screens) reopens the panel: the teacher always sees that the port is open.
- Texts: `share.*` (panel, buttons) and `errorCodes.share*` in both locales.

### The viewer (other PC, any browser)

- Second Vite entry `app/viewer.html` → `ui/src/viewer/main.ts` → `Viewer.svelte`. **No import of `@tauri-apps/*` nor
  of `lib/api.ts`** (a vitest test checks the module graph). `vite.config.ts`: `build.rolldownOptions.input` (Vite 8's name
  for `rollupOptions`) with both pages, `build.manifest: true`.
- Reuses `lib/live-state.svelte.ts` (`LiveState`: snapshots and batches, exactly as the presentation window),
  `lib/live-render.ts` (`LiveRenderer`, `buildScene`, mode `presentation`: zoom, pan, blackboard, laser dot),
  `lib/live-view.ts` and `lib/ink.ts` (the `ardoise-ink` WebAssembly outlines), so it looks exactly like the app.
  `ViewerCanvas.svelte` is a small canvas component (size, pixel ratio, animation frame, background image from the
  message instead of `render_background`); no marks, no pen: its pointer handling only moves the viewer's own view
  (below).
- A `note` message becomes a `LiveSnapshot` for the same `LiveState` (state `idle`, one page, whole-page view, no
  laser); `noteStrokes` appends; every `note` replaces the strokes (new generation).
- `lib/viewer-stream.ts`: reads `/events` with `fetch` (so that it sees the status: `401` → code form, `503` → "too
  many screens", retried) and a pure SSE parser; reconnects by itself after a cut (0.5 s, 1, 2, 4, then every 5 s),
  shows "reconnecting" meanwhile; after `end` it shows "sharing ended" and keeps trying every 5 s (a new share
  answers `401`: the code form).
- `lib/viewer-state.svelte.ts`: the screen (`code`, `connecting`, `waiting`, `live`, `note`, `ended`), the pause badge,
  the background image (`paper`: the note's, or the live page's when its key matches); pure and tested.
- Screens: code form (6 characters, autofocus, errors: wrong code with the attempts left, too many attempts with the
  wait), waiting screen, the page full window (black around, like the projector), a full-screen button (Fullscreen
  API; also a double click) that hides with the cursor after 2 s still, "reconnecting" overlay, "sharing ended".
- **The viewer's own view** (Q8, decided 2026-11-03; purely local: nothing is sent, no route, the server stays
  read-only). `lib/viewer-view.ts` (pure, on the geometry of `live-view.ts`) and `lib/viewer-view.svelte.ts`
  (`ViewerView`, owned by `ViewerState` as `view`). The viewer *follows* the PC: it draws `live.view`, the view
  events of the PC. A fit button, the wheel, a drag, a pinch or a key *takes an own view* (`own`): a fit mode
  (`page`: zoom 1; `width`: `fitWidthView`, the top of the page) or a `free` zoom and centre (`LiveViewState`
  vocabulary, zoom 1 to 8, centre clamped by `clampView` so that the page cannot be lost). The view drawn is then
  `resolveView`: the own zoom and centre on the *PC's* blackboard and laser, so that they are always the PC's, and
  `LiveRenderer` draws the page, the marks and the dot in page coordinates at any zoom. `page` and `width` are
  worked out again from the window's size at each frame (a resize or full screen keeps them). While an own view
  is in force, `view` events of the PC change nothing that is drawn; « Follow the PC » (button, or Esc) drops it.
  An action that changes nothing (a drag when the whole page is shown, a zoom out at the limit) does not leave the
  PC's view. A `free` zoom is bound to a page key (`pageKey`: live page index and size, or the note's id) and
  becomes the whole page when the PC shows another page or another note; a fit mode survives page changes, new notes,
  reconnections, pauses and the switch between live and note. The waiting, ended and code screens drop the own
  view. For a note there is no PC view: the default is the whole page and « Follow the PC » is not shown (Esc goes
  back to the whole page).
- Controls: a bar (`role="toolbar"`, `data-action` `fit-page`, `fit-width`, `zoom-out`, `zoom-in`, `follow`,
  and the full-screen button) hidden with the cursor after 2 s like the full-screen button alone was. Mouse: wheel
  zooms around the pointer (like the editor and the live preview), drag moves. Touch: `TouchGestures` of
  `lib/gestures.ts` (one finger moves, two pinch; a double tap is left to full screen). Keys (`liveKeyAction`,
  those of the live windows, minus what changes the PC's page): `0`/`Home`, `W`, `+`/`-`, Shift + arrows, Esc,
  `F`; combinations with Ctrl or Alt are the browser's. The canvas exposes `data-fit` (`follow`, `page`,
  `width`, `free`), `data-zoom` and `data-scale` (CSS pixels per page unit) for the tests.
- Language: French if `navigator.language` starts with `fr`, else English (`resolveLocale('auto', …)`), texts in
  `viewer.*` of the same locale files (parity test).

## Latency (SPEC §4.10: < 150 ms from the stroke drawn to the other screen, on a LAN)

Path: tablet → service → hub (batched every 4 ms) → `ShareSink` (JSON) → viewer queue → socket (`TCP_NODELAY`,
written and flushed per message) → LAN → `fetch` reader → `LiveState` → next animation frame. Measured:

- Crate test: `LiveViewer::send` → bytes read by a TCP client on localhost (median printed, must stay < 20 ms).
- App test: hub input (`LivePoint.recv`) → SSE message read by a TCP client (median printed).
- e2e (Windows, same PC): `recv` of the points → arrival in Node (same wall clock), median < 50 ms asserted; the
  network and the other PC's drawing are what H4 judges by eye (less than 150 ms in total).

## Tests

- `ardoise-share` (Linux, real TCP): wrong / right code, cookie, `401` without it, attempts limit (`429`, then
  allowed again after the window, with an injected clock in the unit tests), 404 for every other path including
  traversal and encoded variants and other methods, snapshot then increments for a late joiner, live batches,
  note full then append, pause (frozen, late joiner during pause, resume), waiting, stop (streams get `end`, port
  refused), slow viewer disconnected without blocking the sender, limits (header size), latency median. Other PCs
  are clients bound to other loopback addresses (`127.0.0.2`…: Windows and Linux route all of `127.0.0.0/8` to
  loopback): hostile PCs holding every shared slot and reconnecting at once while the projector loads the page,
  joins and opens its stream; PCs without the code never taking the reserved slots; the stream rate (`503`, another
  address unaffected, one more after the interval); `421` for foreign `Host` values (no attempt counted); a socket
  with `SO_REUSEADDR` that tries to take the port; a share started again at once on the same port; the port fallback
  (a plain listener on the port asked for, then on the following ones, a range with nothing free, a share started
  on a free port, 65535), the paper of a live page (after the snapshot, once per viewer, pause, other contents,
  reconnection). Unit tests for
  the slot choice, the nearby attempts table, the token limit and the stream rate (injected clock).
- App (Linux): settings `[share] port` and this machine's `share_port` (order, bad values, atomic write), the fallback
  with a real listener on the port asked for (both ports reported, the port kept for this PC and asked for at the
  next share, nothing written when the port asked for is free), manifest walk (`viewer_assets` with a fake manifest), stripping of live
  batches, note diff (append / full), a share started with a real `LiveHub` and a TCP client receiving live points
  without any path. A resumed sheet with a template, an image and a hidden layer over real TCP (the picture and its
  pixels, once per viewer, late joiner, plain page and back, pause, reconnection, blackboard, nothing else of the note in
  any message); ordinary pages (no key, no message); `LivePapers` (no work on plain paper, rendered once); ink on the
  paper against the editor's scene, pixel by pixel, with the image's `rect` and `quarter_turns`.
- vitest: SSE parser, stream client (fake `fetch`: 401, 503, reconnect, `end`), viewer state (note → snapshot,
  append, pause, reset, the paper of a live page), the viewer page drawing that paper, no Tauri import in the viewer, share state (debounce, content after start), panel
  component (address, code, viewers, pause, stop, errors), locales parity.
- e2e (`app/e2e/journeys-m11-share.mjs`, wired in `run.mjs`, `ARDOISE_SHARE_LOOPBACK=1` and a free port in
  `[share]`): live mode with the simulated tablet, share, Node joins with the code and checks the events
  (snapshot, points, latency), 404s, the viewer page and its assets; then a note open in the editor (note message,
  then a change), pause, stop (`end`, port closed). A second journey holds the configured port with a Node `net`
  server on 127.0.0.1 before the share starts: another port with the notice, `GET /` there, `share_port` in this
  PC's table of `ardoise.toml`, the same port without notice at the next share.
- Hardware: H4 in `docs/hardware-tests.md`.

## Decisions made while building M11

Server (package A):

- `on_change` runs on the share's own thread, never on the caller's: calls stay in order, several changes in a
  row are merged into one call with the latest status, and it is never called by nor after `stop()` (the app
  tells its windows itself). `show_waiting` / `show_live` do nothing when already shown; `show_note` nothing for
  a byte-identical `full`.
- Pause on a note or the waiting screen: the viewers get only `pause: true` (they already show that image);
  resume sends the content again only when it changed meanwhile. `freeze()` giving `None` freezes on `waiting`.
- **Stale live subscriptions**: every `LiveFeed::start` gets a subscription of its own; once the share stopped it
  (pause, another content, the viewer left, stop), `LiveViewer::send` on it returns `Err`, so a batch the hub's
  flusher had already taken never reaches a viewer after what replaced live mode (a frozen image that moves, a
  `note` overtaken by an old snapshot).
- Viewer queues: 1 024 messages **and 64 MiB** (one larger message still goes through an empty queue). A full
  queue closes that stream without `end`; the browser reconnects and starts again from a snapshot.
- `PermissionDenied` on bind is also `PortInUse` (Windows answers so for ports reserved by Hyper-V).
- After each non-stream answer, the server does not close first: it reads the client's leftovers (up to 64 KiB)
  until the client closes, for 2 s at most, then closes itself (see "Through a school's filtering proxy" below); a refusal
  such as `431` is thus never destroyed by a reset. A stream also ends when the client sends anything.
- `stop()`: each stream drops its backlog and writes `end` (500 ms at most), connections get 1 s to finish; it
  normally takes well under a millisecond, about 1 s with a viewer that does not read.
- Address probes: 8.8.8.8, then 10.0.0.1, 172.16.0.1, 192.168.0.1, 192.168.1.1 (routing lookups only). Known
  limit: with a VPN that takes the default route, its address may come first (the panel lists every address).
- After the independent review: the per-address limits, no eviction from the attempts table, tokens per address,
  `Content-Type` and `Origin` on `/join`, heads parsed only when complete, the bytes of messages being written
  counted against the queue limit, and at stop a message being written gets 400 ms to finish before `end`.
  `on_change` must not call `stop()`.
- After the second review (findings A–E): the 16 slots reserved for addresses holding a token and the fair-share
  rule above (a few PCs holding every slot with requests they never finish kept the projector out); answers within
  5 s instead of 30 s; the `Host` check (`421`; it replaces "no `Host` check" above); the nearby table of attempts
  and the corrected brute-force figures; the token limit forgetting from the address that holds the most;
  `SO_EXCLUSIVEADDRUSE` on Windows; the stream rate per address.
  - A first version protected requests younger than 2 s from being closed to make room. Dropped: PCs that recycle
    their connections every second keep them all young, and a newcomer then finds nothing to close. Fair share
    needs no age.
  - The real-TCP test with hostile PCs on other loopback addresses found that "the oldest connection that has not
    sent its head" must also follow fair share: a hostile PC reconnecting at once took the slot of the projector's
    connection in the instant between its accept and its request head. It now takes only one of its own address or
    of an address that holds more.
  - The firewall rule follows the program's path: said in the guide and in H4.

App (package B) and review fixes:

- `share_start` never holds the runtime's `running` lock while the server starts (a separate `starting` lock keeps
  two starts from making two servers), so the status callback can always read it.
- Leaving a note (waiting screen, live mode) forgets the note sent: the same note opened again goes whole (it was
  never sent again before, the viewers stayed on the waiting screen).
- `share_stop` forgets the content shown; the window says it again after the next start.
- A port in use is reported with its number (`{detail}`), no free port with the range (`8765-8785`); the port is read
  again at each start, so changing it needs a new share, not a new launch.
- `--self-test` checks that the viewer is embedded (`share:` line; a build without it fails the self-test).
- The editor sends its note without the title (Rust keeps only what is drawn): typing a title sends nothing.

Interfaces (packages C, D):

- The viewer replaces its `LiveState` for each `note` and each `live` batch with `reset: true` (a full redraw even
  for a same-id note); `noteStrokes` go through `receive` as `strokeStart` + `strokeEnd`. After `end` the page stays
  "ended" while the port is closed, and a `401` (a new share) shows the code form. A 45 s silence (the server pings
  every 15 s) aborts and reconnects a half-open stream. The SSE parser searches each character once (a message of
  megabytes in small chunks). An opening stream clears the pause badge.
- The share panel is a modal of `Home.svelte`, over the library, the editor and live mode; while it is open the
  editor's and the live window's shortcuts (and cut / paste) are off, Esc closes the panel only; closing it never
  stops the share. Notes are sent 100 ms after the last change, at most every 300 ms.

## Work packages

| Package | Profile | Owns |
|---|---|---|
| A — server crate | specialiste | `crates/ardoise-share/**` |
| B — app integration | specialiste | `crates/ardoise-core/src/settings.rs` (`ShareSettings`), `app/src-tauri/src/share.rs` (+ `share/`), `app/src-tauri/src/lib.rs`, `app/src-tauri/src/live.rs`, `app/src-tauri/src/live/hub.rs` (`current_events`, `Arc`), `app/src-tauri/Cargo.toml` |
| C — viewer | developpeur | `app/viewer.html`, `app/vite.config.ts`, `app/ui/src/viewer/**`, `lib/viewer-*.ts` (+ tests), locales section `viewer` |
| D — share in the app's interface | developpeur | `lib/api.ts` (share section), `lib/mock-share.ts`, `lib/mock.ts`, `lib/share-state.svelte.ts`, `components/share/**`, hooks in `LiveView.svelte`, `Editor.svelte`, `Home.svelte`, locales section `share` and `errorCodes.share*` |
| E — e2e, guide, H4 | developpeur | `app/e2e/journeys-m11-share.mjs`, `app/e2e/run.mjs`, `docs/guide/guide.fr.md`, `docs/guide/guide.en.md`, `docs/hardware-tests.md` |

Contracts (this document, `crates/ardoise-share/src/lib.rs` signatures, `app/ui/src/lib/share.ts`) are set in the
contracts commit; a package that needs to change one says so in its report.
