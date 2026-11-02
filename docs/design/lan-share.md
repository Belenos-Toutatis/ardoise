# Sharing the sheet on the local network (milestone M11)

Developer document (English). Requirements: SPEC §4.10, §6.2 ("Réseau"), §8; ROADMAP M11 and H4;
`docs/questions.md` Q4 (the user's decision). Builds on [tablet-live-classroom.md](tablet-live-classroom.md)
(`LiveHub`, batches, presentation) and [interface.md](interface.md) (editor, translations, browser stand-in).

```
 tablet ─► LiveHub ── LiveBatch (4 ms) ──► main window, presentation window      (unchanged)
              │
              └── one subscription per viewer ("share:<id>") ── HubFeed (app share.rs: strip, JSON)
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

- `Share::start` binds the listener **synchronously** (so a port already in use is an error at once), builds a
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
- Port already used by another program: `ShareError::PortInUse { port }` (code `sharePortInUse`; the message tells to
  close the other program or change `[share] port` in `ardoise.toml`).
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
  - The first byte of a request within 3 s, its head (16 KiB) within 10 s (and the body of `/join`, 1 KiB), 32
    headers; **an answer written within 5 s** (the largest, a script of about 200 KB, takes milliseconds on a LAN),
    then the lingering close (1 s at most). A head is parsed again only when a blank line may have arrived.
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
| `POST /join` | body `{"code":"K7M Q2X"}` (JSON). Right code: `204` + `Set-Cookie: ardoise_share=<token>; Path=/; HttpOnly; SameSite=Strict`. Wrong: `403` `{"error":"wrongCode","remaining":n}`. Too many attempts: `429` + `Retry-After: s`, `{"error":"tooManyAttempts","retryAfter":s}`. Bad request (no JSON, too long, chunked): `400` / `413`. `Content-Type` other than `application/json`: `415`; an `Origin` header other than `http://<Host>` (the `Host` already checked, see above): `403` `{"error":"badOrigin"}` (a page of another site cannot spend the attempts of the projector's browser); neither counts as an attempt |
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
| `{"type":"note","page":SharedPage,"background":string\|null}` | the whole page of the note open in the editor; `background`: the paper (template, texture, background image) as a PNG data URL, `null` when it is plain paper (the paper colour is drawn) |
| `{"type":"noteStrokes","strokes":Stroke[]}` | strokes added at the end of the page last sent (the common case while writing in the editor) |
| `{"type":"pause","paused":bool}` | the share is paused (the image is frozen; the viewer shows a discreet badge) or resumed |
| `{"type":"end"}` | the share stops: the viewer closes the stream and shows "sharing ended" |

`SharedPage = { id, page: PageSize, background: Background, layers: Layer[], strokes: Stroke[] }` (the `Note`
shapes of `note.ts`; layer names are emptied, the viewer only needs `visible`). A `note` message **without**
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
- **A viewer that connects** gets what is shown: `waiting`, the last `full` note message, or a live subscription
  (snapshot, then increments). **Late joiners therefore get a snapshot, then increments**, in every case.
- **Pause** (`set_paused(true)`): live viewers are stopped (`LiveFeed::stop`); the frozen image is what is shown at
  that moment: for live mode `LiveFeed::freeze()` (the session as one batch starting with a snapshot, sent with
  `reset: true` to every viewer so that all show the same frozen frame), for a note its last `full`, else
  `waiting`; then `pause` true. Viewers joining during the pause get the frozen image and `pause` true. Content
  changes during the pause are recorded and sent nowhere. `set_paused(false)`: `pause` false, then the current
  content as for a new viewer (live: a new subscription with a fresh snapshot).
- **Queues**: each viewer has a bounded queue (1 024 messages). A send never blocks; a full queue disconnects that
  viewer (`LiveViewer::send` returns `Err`, the hub drops the subscription), and the browser reconnects and gets a
  fresh snapshot. This keeps the hub's flusher thread free whatever a viewer's network does.

### Public API (fixed in the contracts commit; changes only with a reason in the report)

```rust
pub struct ShareConfig { pub bind: Ipv4Addr /* UNSPECIFIED */, pub port: u16, pub code: Option<String> /* tests */ }
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
    pub fn port(&self) -> u16; pub fn code(&self) -> String; pub fn status(&self) -> ShareStatus;
    pub fn show_waiting(&self); pub fn show_live(&self); pub fn show_note(&self, update: NoteUpdate);
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
8765; 0 or a missing value loads as 8765). Test-only environment variable **`ARDOISE_SHARE_LOOPBACK=1`**: the
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
- `ShareSink::send(batch)` strips the batch, serialises it, and calls `viewer.send`; an `Err` drops the
  subscription (the hub's existing rule). **Stripping**: `saved` and `saveFailed` events are dropped (library paths,
  system messages); in a snapshot `saved` = `[]`, `presentation` = `null`, and the page keeps only what is drawn:
  `id`, `page`, `background`, `layers`, `strokes`, `created`, `modified` (title, tags, favourite, origin… emptied).
- Serialising runs on the hub's flusher thread, as for the windows' channels, and never waits.

### Note of the editor

`share_show({ kind: 'note', note, imageKey })`: the runtime builds `SharedPage` from the editor's note (unsaved state
included) and keeps the previous one: same `id`, page, background, background image and layers, and the old strokes
a prefix of the new ones → `append` = the new strokes; otherwise only `full`. The background is rendered by
`notes::background_png` (with the staged picture under `imageKey`, like `render_background`) at 2 560 px on the
longer side, **only when it changed** (cached by page size, background and image placement), in `spawn_blocking`;
plain paper (template `blank`, no texture, no image) gives `null`. The live pages are always plain paper (white, or
slate with the blackboard): the viewer draws their paper colour, as the app does before its PNG arrives.

### Commands (`share.rs`)

| Command | Arguments | Returns |
|---|---|---|
| `share_status` | – | `ShareStatus` |
| `share_subscribe` | `onEvent: Channel<ShareStatus>` | current `ShareStatus`; then the status at every change (one subscription per window) |
| `share_start` | – | `ShareStatus` (starts, or returns the running share). Errors `sharePortInUse`, `shareUnavailable`, `shareFailed` |
| `share_stop` | – | `ShareStatus` (inactive); can be repeated |
| `share_set_paused` | `paused: boolean` | `ShareStatus` |
| `share_show` | `content: ShareContent` | – (ignored while no share runs) |

```ts
type ShareContent = { kind: 'waiting' } | { kind: 'live' } | { kind: 'note'; note: Note; imageKey: string | null };
interface ShareStatus {
  active: boolean;
  port: number;                       // the configured one when inactive
  code: string | null;                // "K7M Q2X" while active
  addresses: string[];                // "http://192.168.1.23:8765/", private LAN first; [] when inactive
  qr: { size: number; path: string } | null;   // of addresses[0]
  viewers: number;
  paused: boolean;
  shown: 'waiting' | 'live' | 'note';
}
```

New error codes (`errorCodes.*`): `sharePortInUse`, `shareUnavailable`, `shareFailed`. `share_start` while the UI
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
  line (type the address in the other PC's browser, then the code; the firewall prompt: "Réseau privé"), errors in
  words (port in use: which port and how to change it).
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
  message instead of `render_background`); no pointer handling, no marks.
- A `note` message becomes a `LiveSnapshot` for the same `LiveState` (state `idle`, one page, whole-page view, no
  laser); `noteStrokes` appends; every `note` replaces the strokes (new generation).
- `lib/viewer-stream.ts`: reads `/events` with `fetch` (so that it sees the status: `401` → code form, `503` → "too
  many screens", retried) and a pure SSE parser; reconnects by itself after a cut (0.5 s, 1, 2, 4, then every 5 s),
  shows "reconnecting" meanwhile; after `end` it shows "sharing ended" and keeps trying every 5 s (a new share
  answers `401`: the code form).
- `lib/viewer-state.svelte.ts`: the screen (`code`, `connecting`, `waiting`, `live`, `note`, `ended`), the pause badge,
  the background image; pure and tested.
- Screens: code form (6 characters, autofocus, errors: wrong code with the attempts left, too many attempts with the
  wait), waiting screen, the page full window (black around, like the projector), a full-screen button (Fullscreen
  API; also a double click) that hides with the cursor after 2 s still, "reconnecting" overlay, "sharing ended".
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
  with `SO_REUSEADDR` that tries to take the port; a share started again at once on the same port. Unit tests for
  the slot choice, the nearby attempts table, the token limit and the stream rate (injected clock).
- App (Linux): settings `[share] port`, manifest walk (`viewer_assets` with a fake manifest), stripping of live
  batches, note diff (append / full), a share started with a real `LiveHub` and a TCP client receiving live points
  without any path.
- vitest: SSE parser, stream client (fake `fetch`: 401, 503, reconnect, `end`), viewer state (note → snapshot,
  append, pause, reset), no Tauri import in the viewer, share state (debounce, content after start), panel
  component (address, code, viewers, pause, stop, errors), locales parity.
- e2e (`app/e2e/journeys-m11-share.mjs`, wired in `run.mjs`, `ARDOISE_SHARE_LOOPBACK=1` and a free port in
  `[share]`): live mode with the simulated tablet, share, Node joins with the code and checks the events
  (snapshot, points, latency), 404s, the viewer page and its assets; then a note open in the editor (note message,
  then a change), pause, stop (`end`, port closed).
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
- After each non-stream answer, a "lingering close" (write side shut, client's leftover bytes read for up to 1 s)
  so that a refusal such as `431` is not destroyed by a reset. A stream also ends when the client sends anything.
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
- A port in use is reported with its number (`{detail}`); the port is read again at each start, so changing it
  needs a new share, not a new launch.
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
