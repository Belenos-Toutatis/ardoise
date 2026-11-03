//! The listener, its thread, and the connections (`docs/design/lan-share.md`, "Server").
//!
//! One current-thread tokio runtime on the thread `ardoise-share`; each connection is a task
//! that reads one request head (bounded in size and time) and answers it, or holds the event
//! stream open. A watch channel tells every task that the share stops.
//!
//! One machine of the network must not be able to take the share from the others (a hostile
//! classmate's PC must never keep the projector's PC out): connections and event streams are
//! limited per address, a request must start within 3 s and be answered within 5 s, part of
//! the connection slots is kept for addresses that joined with the code, and when there is no
//! room a connection is closed for the newcomer by fair share: one still waiting for its request
//! head, else a request of the address that holds the most slots if it holds at least two more
//! (never an event stream). An address may open an event stream only so often (each one copies
//! the whole page).

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{Notify, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior, timeout, timeout_at};

use crate::access::{self, Access, StreamRate, Verdict};
use crate::content::{Content, Frame, JoinRefused, Viewer, end_frame};
use crate::http::{self, ContentLength, Head, HeadError, ResponseHead};
use crate::{Assets, Limits, LiveFeed, OnChange, ShareConfig, ShareError, ShareStatus};

/// Time to write an answer: the largest, a script of about 200 KB, takes milliseconds on a
/// LAN; a client that does not read must not hold its slot long.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Connections waiting to be accepted (std's value).
const LISTEN_BACKLOG: u32 = 128;
/// Time to write one batch of messages to an event stream before the viewer counts as gone.
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(15);
/// After an answer, how long (and how many bytes) the client's leftovers are read before the
/// socket closes, so that unread bytes do not turn the close into a reset that could destroy
/// the answer in flight.
const LINGER_TIME: Duration = Duration::from_secs(1);
const LINGER_BYTES: usize = 64 * 1024;
/// At stop: time for a stream to finish the message it is writing, then time to write `end`;
/// then time for every connection to finish (more than both).
const END_TIMEOUT: Duration = Duration::from_millis(400);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
/// Messages written to an event stream are gathered up to this size per write.
const COALESCE_BYTES: usize = 64 * 1024;
/// First bytes of an event stream: the browser waits 2 s before reconnecting.
const STREAM_PREAMBLE: &str = "retry: 2000\n\n";
const PING: &[u8] = b": ping\n\n";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// State shared by the share's handle and its thread.
pub(crate) struct Server {
    pub port: u16,
    /// The port that was asked for (`ShareConfig::port`), which `port` follows unless it was taken.
    pub requested_port: u16,
    pub code: String,
    pub content: Content,
    limits: Limits,
    assets: Assets,
    /// This computer's name (Windows), accepted in the `Host` header besides IPv4 addresses
    /// and `localhost`.
    host_name: Option<String>,
    access: Mutex<Access>,
    stream_rate: Mutex<StreamRate>,
    connections: Mutex<Connections>,
    shutdown: watch::Sender<bool>,
    on_change: OnChange,
}

/// A started server: its state, its thread, and a signal sent when the thread ends.
pub(crate) struct Running {
    pub server: Arc<Server>,
    pub thread: JoinHandle<()>,
    pub done: mpsc::Receiver<()>,
}

/// Binds the listener (synchronously), then runs the server on its own thread.
pub(crate) fn start(
    config: ShareConfig,
    limits: Limits,
    assets: Assets,
    feed: Arc<dyn LiveFeed>,
    on_change: OnChange,
) -> Result<Running, ShareError> {
    let code = match config.code.as_deref().map(access::normalize_code) {
        Some(code) if !code.is_empty() => code,
        _ => access::random_code().map_err(|error| ShareError::Random(error.to_string()))?,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let listener = {
        let _context = runtime.enter();
        listen_from(config.bind, config.port, config.fallback_ports)?
    };
    let port = listener.local_addr()?.port();
    let (shutdown, _) = watch::channel(false);
    let server = Arc::new(Server {
        port,
        requested_port: config.port,
        code: code.clone(),
        content: Content::new(feed, limits.streams, limits.streams_per_address),
        limits,
        assets,
        host_name: http::computer_name(),
        access: Mutex::new(Access::new(code, crate::addresses::local_ipv4())),
        stream_rate: Mutex::new(StreamRate::new(
            limits.stream_opens,
            limits.stream_open_every,
        )),
        connections: Mutex::new(Connections::default()),
        shutdown,
        on_change,
    });
    let (done_sender, done) = mpsc::channel();
    let thread_server = Arc::clone(&server);
    let thread = std::thread::Builder::new()
        .name("ardoise-share".into())
        .spawn(move || {
            runtime.block_on(thread_server.run(listener));
            // Drops the connections still open, if any.
            drop(runtime);
            let _ = done_sender.send(());
        })?;
    Ok(Running {
        server,
        thread,
        done,
    })
}

/// The listener on `port`, or else on the first of the `fallback` ports after it that binds,
/// within the runtime's context. Each port is really tried (no check, then bind: another
/// program could take it in between); only a port in use or reserved moves on to the next, any
/// other failure ends the search. Port 0 (any port) is tried once.
fn listen_from(bind: Ipv4Addr, port: u16, fallback: u16) -> Result<TcpListener, ShareError> {
    let last = if port == 0 {
        0
    } else {
        port.saturating_add(fallback)
    };
    let mut candidate = port;
    loop {
        match listen(SocketAddrV4::new(bind, candidate)) {
            Err(ShareError::PortInUse { .. }) if candidate < last => candidate += 1,
            Err(ShareError::PortInUse { .. }) if last > port => {
                return Err(ShareError::NoFreePort { first: port, last });
            }
            result => return result,
        }
    }
}

/// The listener on `address`, within the runtime's context. On Windows it takes the port for
/// itself (`SO_EXCLUSIVEADDRUSE`): another program, even one that sets `SO_REUSEADDR`, cannot
/// bind it too and take the connections. Elsewhere `SO_REUSEADDR`, as std and mio set it: a
/// share started again at once binds the port despite its last connections in `TIME_WAIT`.
fn listen(address: SocketAddrV4) -> Result<TcpListener, ShareError> {
    let socket = TcpSocket::new_v4()?;
    #[cfg(windows)]
    exclusive_address_use(&socket)?;
    #[cfg(not(windows))]
    socket.set_reuseaddr(true)?;
    socket
        .bind(address.into())
        .map_err(|error| match error.kind() {
            // Also a port that Windows reserves (Hyper-V's excluded ranges answer "access
            // denied"): the remedy is the same, another port.
            std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied => {
                ShareError::PortInUse {
                    port: address.port(),
                }
            }
            _ => ShareError::Io(error),
        })?;
    Ok(socket.listen(LISTEN_BACKLOG)?)
}

/// Sets `SO_EXCLUSIVEADDRUSE` on a socket not yet bound (no administrator rights needed).
#[cfg(windows)]
fn exclusive_address_use(socket: &TcpSocket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        SO_EXCLUSIVEADDRUSE, SOCKET, SOCKET_ERROR, SOL_SOCKET, setsockopt,
    };
    let enable: i32 = 1;
    let raw = socket.as_raw_socket() as SOCKET;
    // SAFETY: `raw` is the open socket that `socket` owns for the whole call; the option value
    // points to a live `i32` and its length is that of an `i32`.
    let result = unsafe {
        setsockopt(
            raw,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&raw const enable).cast(),
            size_of::<i32>() as i32,
        )
    };
    if result == SOCKET_ERROR {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The connections open, oldest first, how many come from each address, and how many from
/// addresses without a session token.
#[derive(Default)]
struct Connections {
    open: BTreeMap<u64, Connection>,
    per_address: HashMap<IpAddr, usize>,
    untrusted: usize,
    next: u64,
}

/// What a connection is doing, for the choice of one to close when there is no room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Waiting for its request head: the first to make room (for its own address, or for one
    /// that holds fewer slots).
    Head,
    /// Its request is read or answered (or the answer lingers): it makes room only for a
    /// connection with a token when it has none, or by fair share.
    Request,
    /// An event stream: never closed to make room (the stream limits bound them).
    Stream,
}

struct Connection {
    address: IpAddr,
    /// Its address held a session token when it connected: it may take the reserved slots,
    /// and a connection from an address without one never takes its slot.
    trusted: bool,
    phase: Phase,
    /// Closes it to make room for a newcomer.
    evict: Arc<Notify>,
}

impl Connections {
    /// Slots held by `address`.
    fn held(&self, address: IpAddr) -> usize {
        self.per_address.get(&address).copied().unwrap_or(0)
    }

    /// Takes a slot for a connection from `address` (`trusted`: the address holds a session
    /// token): its id and what closes it to make room. `None`: closed at once (too many from
    /// this address, or no room and no connection to close for it). Without a token, an
    /// address only takes the slots that are not reserved.
    fn admit(
        &mut self,
        address: IpAddr,
        trusted: bool,
        limits: &Limits,
    ) -> Option<(u64, Arc<Notify>)> {
        let held = self.held(address);
        if held >= limits.connections_per_address {
            return None;
        }
        let shared = limits.connections.saturating_sub(limits.reserved);
        if self.open.len() >= limits.connections || (!trusted && self.untrusted >= shared) {
            let victim = if trusted {
                // A screen that joined comes first: the oldest connection without a token
                // that is not an event stream, else fair share among those with one.
                self.oldest_untrusted()
                    .or_else(|| self.victim(address, held, true))
            } else {
                self.victim(address, held, false)
            }?;
            if let Some(evicted) = self.remove(victim) {
                evicted.evict.notify_one();
            }
        }
        let id = self.next;
        self.next += 1;
        let evict = Arc::new(Notify::new());
        self.open.insert(
            id,
            Connection {
                address,
                trusted,
                phase: Phase::Head,
                evict: Arc::clone(&evict),
            },
        );
        *self.per_address.entry(address).or_default() += 1;
        if !trusted {
            self.untrusted += 1;
        }
        Some((id, evict))
    }

    /// The oldest connection without a token that is not an event stream (one still waiting
    /// for its head first).
    fn oldest_untrusted(&self) -> Option<u64> {
        let candidates = || {
            self.open
                .iter()
                .filter(|(_, c)| !c.trusted && c.phase != Phase::Stream)
        };
        candidates()
            .find(|(_, c)| c.phase == Phase::Head)
            .or_else(|| candidates().next())
            .map(|(id, _)| *id)
    }

    /// The connection to close for a newcomer from `address`, which holds `held` slots, among
    /// those whose address held a token (`trusted`) or not: the oldest one still waiting for
    /// its request head, of the newcomer's own address or of one that holds more; else the
    /// oldest request of the address that holds the most slots, if it holds at least two more
    /// than the newcomer's. Fair share: an address that holds few always gets in, one that
    /// holds as many as others never takes their slots (not even a connection that has just
    /// opened and not yet sent its request), and two addresses that hold about as many never
    /// take slots from each other back and forth.
    fn victim(&self, address: IpAddr, held: usize, trusted: bool) -> Option<u64> {
        let candidates = || {
            self.open
                .iter()
                .filter(move |(_, connection)| connection.trusted == trusted)
        };
        let idle = candidates().find(|(_, c)| {
            c.phase == Phase::Head && (c.address == address || self.held(c.address) > held)
        });
        if let Some((id, _)) = idle {
            return Some(*id);
        }
        let mut best: Option<(usize, u64)> = None;
        for (id, connection) in candidates() {
            if connection.phase != Phase::Request {
                continue;
            }
            let its = self.held(connection.address);
            // Oldest first: a later connection replaces it only if its address holds more.
            if best.is_none_or(|(most, _)| its > most) {
                best = Some((its, *id));
            }
        }
        best.filter(|(most, _)| *most >= held + 2).map(|(_, id)| id)
    }

    /// The connection `id` moves to `phase`; false if it was closed to make room meanwhile.
    fn advance(&mut self, id: u64, phase: Phase) -> bool {
        match self.open.get_mut(&id) {
            Some(connection) => {
                connection.phase = phase;
                true
            }
            None => false,
        }
    }

    /// Frees the slot of `id` (if it was not already given to a newcomer).
    fn remove(&mut self, id: u64) -> Option<Connection> {
        let connection = self.open.remove(&id)?;
        if let Entry::Occupied(mut count) = self.per_address.entry(connection.address) {
            *count.get_mut() -= 1;
            if *count.get() == 0 {
                count.remove();
            }
        }
        if !connection.trusted {
            self.untrusted = self.untrusted.saturating_sub(1);
        }
        Some(connection)
    }
}

/// A connection's slot, freed when its task ends (or is dropped).
struct Slot {
    server: Arc<Server>,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        lock(&self.server.connections).remove(self.id);
    }
}

#[derive(Deserialize)]
struct JoinBody {
    code: String,
}

/// Why a request could not be read.
enum ReadError {
    /// Closed, reset, or timed out: nothing to answer.
    Gone,
    Head(HeadError),
}

impl Server {
    pub fn status(&self) -> ShareStatus {
        let (viewers, paused, shown) = self.content.summary();
        ShareStatus {
            port: self.port,
            code: self.code.clone(),
            viewers,
            paused,
            shown,
        }
    }

    /// Tells the thread to stop (the content must already be stopped).
    pub fn signal_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    async fn run(self: Arc<Self>, listener: TcpListener) {
        let mut shutdown = self.shutdown.subscribe();
        let mut tasks = JoinSet::new();
        let notifier = tokio::spawn(Arc::clone(&self).notify_changes());
        loop {
            tokio::select! {
                _ = stopping(&mut shutdown) => break,
                Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let trusted = lock(&self.access).holds_token(peer.ip());
                        let admitted =
                            lock(&self.connections).admit(peer.ip(), trusted, &self.limits);
                        // Not admitted: dropped, so closed at once.
                        let Some((id, evict)) = admitted else { continue };
                        let slot = Slot { server: Arc::clone(&self), id };
                        let server = Arc::clone(&self);
                        tasks.spawn(async move {
                            server.connection(stream, peer, id, evict).await;
                            drop(slot);
                        });
                    }
                    // Out of sockets or the like: wait a little rather than spin.
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                },
            }
        }
        // The port closes now; the connections write `end` and finish.
        drop(listener);
        let _ = timeout(SHUTDOWN_GRACE, async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        tasks.abort_all();
        notifier.abort();
    }

    /// Calls `on_change` with the status after each change (several changes in a row may give
    /// one call, always with the latest status).
    async fn notify_changes(self: Arc<Self>) {
        loop {
            self.content.changed().await;
            if self.content.is_stopped() {
                return;
            }
            (self.on_change)(self.status());
        }
    }

    async fn connection(
        self: Arc<Self>,
        stream: TcpStream,
        peer: SocketAddr,
        id: u64,
        evict: Arc<Notify>,
    ) {
        let _ = stream.set_nodelay(true);
        let mut shutdown = self.shutdown.subscribe();
        if *shutdown.borrow() {
            return;
        }
        // Closed at once when it makes room for a newcomer (never once it is an event stream:
        // then it is no longer chosen).
        tokio::select! {
            _ = self.serve(stream, peer, id, &mut shutdown) => {}
            _ = evict.notified() => {}
        }
    }

    /// Reads the request of the connection `id` and answers it (or holds its event stream).
    async fn serve(
        &self,
        mut stream: TcpStream,
        peer: SocketAddr,
        id: u64,
        shutdown: &mut watch::Receiver<bool>,
    ) {
        let now = Instant::now();
        let deadline = now + self.limits.request;
        let first_byte = now + self.limits.first_byte;
        let read = tokio::select! {
            read = timeout_at(deadline, read_head(&mut stream, first_byte)) => read,
            _ = stopping(shutdown) => return,
        };
        let (head, rest) = match read {
            Ok(Ok(request)) => request,
            Ok(Err(ReadError::Head(error))) => {
                let answer = http::text(error.status(), &[]);
                reply(stream, &answer, &[], shutdown).await;
                return;
            }
            Ok(Err(ReadError::Gone)) | Err(_) => return,
        };
        if !lock(&self.connections).advance(id, Phase::Request) {
            return;
        }
        // A page of another site whose name was pointed at this PC (DNS rebinding) names its
        // own host: it gets nothing, and spends no attempt.
        if !head.host_allowed(self.port, self.host_name.as_deref()) {
            reply(stream, &http::text(421, &[]), &[], shutdown).await;
            return;
        }
        let address = peer.ip();
        match (head.method.as_str(), head.path()) {
            ("GET", "/events") => self.events(stream, &head, address, id, shutdown).await,
            ("POST", "/join") => {
                let answer = tokio::select! {
                    answer = timeout_at(deadline, self.join(&mut stream, &head, rest, address)) => answer,
                    _ = stopping(shutdown) => return,
                };
                if let Ok(Some(answer)) = answer {
                    reply(stream, &answer, &[], shutdown).await;
                }
            }
            ("GET", path) => {
                let asset = (path == "/" || path.starts_with("/assets/"))
                    .then(|| self.assets.get(path))
                    .flatten();
                match asset {
                    Some(asset) => {
                        let mut head = ResponseHead::new(200)
                            .header("Content-Type", &asset.content_type)
                            .header("Content-Length", &asset.bytes.len().to_string());
                        head = if path == "/" {
                            head.header("Cache-Control", http::NO_STORE)
                                .header("Content-Security-Policy", http::PAGE_CSP)
                                .header("X-Frame-Options", "DENY")
                        } else {
                            head.header("Cache-Control", http::IMMUTABLE)
                        };
                        let bytes = Arc::clone(&asset.bytes);
                        reply(stream, &head.finish(), &bytes, shutdown).await;
                    }
                    None => reply(stream, &http::text(404, &[]), &[], shutdown).await,
                }
            }
            _ => reply(stream, &http::text(404, &[]), &[], shutdown).await,
        }
    }

    /// `POST /join`: the answer, or `None` when the client went away. Only a request whose JSON
    /// body was read counts as an attempt.
    async fn join(
        &self,
        stream: &mut TcpStream,
        head: &Head,
        mut body: Vec<u8>,
        address: IpAddr,
    ) -> Option<Vec<u8>> {
        let bad_request = || http::json(400, r#"{"error":"badRequest"}"#, &[]);
        // Another site's page: refused (a browser always says where a POST comes from).
        if !head.same_origin() {
            return Some(http::json(403, r#"{"error":"badOrigin"}"#, &[]));
        }
        // A form of another site could post `text/plain` without asking first; JSON needs the
        // browser to ask (OPTIONS), which gets 404.
        if !head.is_json() {
            return Some(http::json(415, r#"{"error":"badRequest"}"#, &[]));
        }
        if head.transfer_encoding {
            return Some(bad_request());
        }
        let length = match head.content_length {
            ContentLength::Given(length) => length,
            ContentLength::Absent | ContentLength::Invalid => return Some(bad_request()),
        };
        if length > http::MAX_BODY {
            return Some(http::json(413, r#"{"error":"tooLarge"}"#, &[]));
        }
        let mut chunk = [0u8; http::MAX_BODY];
        while body.len() < length {
            let want = length - body.len();
            match stream.read(&mut chunk[..want]).await {
                Ok(0) | Err(_) => return None,
                Ok(n) => body.extend_from_slice(&chunk[..n]),
            }
        }
        body.truncate(length);
        let Ok(JoinBody { code }) = serde_json::from_slice::<JoinBody>(&body) else {
            return Some(bad_request());
        };
        let mut access = lock(&self.access);
        let answer = match access.try_code(address, &code, std::time::Instant::now()) {
            Verdict::Right => match access::random_token() {
                Ok(token) => {
                    let cookie = format!(
                        "{}={token}; Path=/; HttpOnly; SameSite=Strict",
                        http::COOKIE
                    );
                    access.add_token(address, token);
                    ResponseHead::new(204)
                        .header("Cache-Control", http::NO_STORE)
                        .header("Set-Cookie", &cookie)
                        .finish()
                }
                Err(_) => http::text(500, &[]),
            },
            Verdict::Wrong { remaining } => http::json(
                403,
                &format!(r#"{{"error":"wrongCode","remaining":{remaining}}}"#),
                &[],
            ),
            Verdict::TooMany { retry_after } => http::json(
                429,
                &format!(r#"{{"error":"tooManyAttempts","retryAfter":{retry_after}}}"#),
                &[("Retry-After", &retry_after.to_string())],
            ),
        };
        Some(answer)
    }

    /// `GET /events` on the connection `id`: checks the cookie and how often this address
    /// opens streams, then holds the stream open until the viewer leaves, is too slow, or the
    /// share stops.
    async fn events(
        &self,
        stream: TcpStream,
        head: &Head,
        address: IpAddr,
        id: u64,
        shutdown: &mut watch::Receiver<bool>,
    ) {
        let authorised = {
            let access = lock(&self.access);
            head.session_cookies
                .iter()
                .fold(false, |found, token| found | access.token_valid(token))
        };
        if !authorised {
            reply(stream, &http::text(401, &[]), &[], shutdown).await;
            return;
        }
        // Each new stream makes the app copy the whole page: a holder of the code looping on
        // reconnections waits like a browser told "too many screens" (503, retried).
        let now = std::time::Instant::now();
        let allowed = lock(&self.stream_rate).check(address, now);
        if let Err(wait) = allowed {
            let seconds = access::whole_seconds(wait).to_string();
            let answer = http::text(503, &[("Retry-After", &seconds)]);
            reply(stream, &answer, &[], shutdown).await;
            return;
        }
        let viewer = match self.content.join(address) {
            Ok(viewer) => viewer,
            Err(JoinRefused::TooMany) => {
                let answer = http::text(503, &[("Retry-After", "5")]);
                reply(stream, &answer, &[], shutdown).await;
                return;
            }
            Err(JoinRefused::Stopped) => return,
        };
        // Removes the viewer however the stream ends, even if the task is dropped.
        let _leave = Leave {
            content: &self.content,
            id: viewer.id(),
        };
        lock(&self.stream_rate).spend(address, now);
        if !lock(&self.connections).advance(id, Phase::Stream) {
            return;
        }
        stream_events(stream, &viewer, self.limits.ping, shutdown).await;
    }
}

/// Removes a viewer from the content when its stream ends.
struct Leave<'a> {
    content: &'a Content,
    id: u64,
}

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        self.content.leave(self.id);
    }
}

/// Wakes when the share stops.
async fn stopping(shutdown: &mut watch::Receiver<bool>) {
    // The guard `wait_for` returns is dropped here: it must not live across an await.
    let _ = shutdown.wait_for(|stop| *stop).await;
}

/// Reads a request head: the head and the bytes read after it. The first byte must arrive by
/// `first_byte`.
async fn read_head(
    stream: &mut TcpStream,
    first_byte: Instant,
) -> Result<(Head, Vec<u8>), ReadError> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    loop {
        let room = http::MAX_HEAD.saturating_sub(buf.len()).min(chunk.len());
        if room == 0 {
            return Err(ReadError::Head(HeadError::TooLarge));
        }
        let read = stream.read(&mut chunk[..room]);
        let read = if buf.is_empty() {
            timeout_at(first_byte, read)
                .await
                .map_err(|_| ReadError::Gone)?
        } else {
            read.await
        };
        let n = match read {
            Ok(0) | Err(_) => return Err(ReadError::Gone),
            Ok(n) => n,
        };
        // A request starts with its method, a word. Anything else is no HTTP request: above all
        // a browser trying `https://` (a TLS handshake starts with 0x16), which must fail at once
        // so that it falls back to `http://`, instead of waiting for a head that never comes.
        if buf.is_empty() && !chunk[0].is_ascii_alphabetic() {
            return Err(ReadError::Gone);
        }
        let start = buf.len();
        buf.extend_from_slice(&chunk[..n]);
        // Parsed again only when the blank line that ends a head may have arrived (with the 3
        // bytes before, for one split between two reads): never once per byte.
        if !http::has_blank_line(&buf[start.saturating_sub(3)..]) {
            continue;
        }
        match http::parse_head(&buf) {
            Ok(Some(head)) => {
                let rest = buf.split_off(head.len.min(buf.len()));
                return Ok((head, rest));
            }
            Ok(None) => {}
            Err(error) => return Err(ReadError::Head(error)),
        }
    }
}

/// Writes an answer, then closes the connection (after reading the client's leftovers for a
/// moment).
async fn reply(
    mut stream: TcpStream,
    head: &[u8],
    body: &[u8],
    shutdown: &mut watch::Receiver<bool>,
) {
    let written = tokio::select! {
        written = timeout(RESPONSE_TIMEOUT, async {
            stream.write_all(head).await?;
            stream.write_all(body).await?;
            stream.flush().await?;
            stream.shutdown().await
        }) => matches!(written, Ok(Ok(()))),
        _ = stopping(shutdown) => false,
    };
    if !written {
        return;
    }
    let drain = async {
        let mut scratch = [0u8; 4096];
        let mut total = 0;
        loop {
            match stream.read(&mut scratch).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n;
                    if total >= LINGER_BYTES {
                        break;
                    }
                }
            }
        }
    };
    tokio::select! {
        _ = timeout(LINGER_TIME, drain) => {}
        _ = stopping(shutdown) => {}
    }
}

/// The event stream of one viewer.
async fn stream_events(
    stream: TcpStream,
    viewer: &Viewer,
    ping_every: Duration,
    shutdown: &mut watch::Receiver<bool>,
) {
    let (mut reader, mut writer) = stream.into_split();
    let mut head = ResponseHead::new(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", http::NO_STORE)
        .finish();
    head.extend_from_slice(STREAM_PREAMBLE.as_bytes());
    if !write_stream(&mut writer, &head, viewer, shutdown).await {
        return;
    }
    // A zero period would make the timer panic.
    let ping_every = ping_every.max(Duration::from_millis(1));
    let mut ping = tokio::time::interval_at(Instant::now() + ping_every, ping_every);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut scratch = [0u8; 64];
    loop {
        if *shutdown.borrow() {
            // What was queued is dropped: `end` comes at once.
            let _ = timeout(END_TIMEOUT, async {
                writer.write_all(&end_frame()).await?;
                writer.flush().await?;
                writer.shutdown().await
            })
            .await;
            return;
        }
        let pending = viewer.take();
        if pending.kicked {
            return;
        }
        if !pending.frames.is_empty() {
            if !write_frames(&mut writer, &pending.frames, viewer, shutdown).await {
                return;
            }
            viewer.written();
            continue;
        }
        tokio::select! {
            biased;
            _ = stopping(shutdown) => {}
            _ = viewer.kicked() => {}
            _ = viewer.queued() => {}
            _ = ping.tick() => {
                if !write_stream(&mut writer, PING, viewer, shutdown).await {
                    return;
                }
            }
            // The viewer only receives: the end of its side, an error, or any byte it sends
            // ends the stream.
            _ = reader.read(&mut scratch) => return,
        }
    }
}

/// Writes queued messages, gathered into writes of up to [`COALESCE_BYTES`]. Once the share
/// stops, it stops after the write under way (the rest is dropped for `end`).
async fn write_frames(
    writer: &mut OwnedWriteHalf,
    frames: &[Frame],
    viewer: &Viewer,
    shutdown: &mut watch::Receiver<bool>,
) -> bool {
    let mut gathered: Vec<u8> = Vec::new();
    for frame in frames {
        if *shutdown.borrow() {
            return true;
        }
        if frame.len() >= COALESCE_BYTES {
            if !gathered.is_empty() && !write_stream(writer, &gathered, viewer, shutdown).await {
                return false;
            }
            gathered.clear();
            if !write_stream(writer, frame, viewer, shutdown).await {
                return false;
            }
        } else {
            gathered.extend_from_slice(frame);
            if gathered.len() >= COALESCE_BYTES {
                if !write_stream(writer, &gathered, viewer, shutdown).await {
                    return false;
                }
                gathered.clear();
            }
        }
    }
    gathered.is_empty() || write_stream(writer, &gathered, viewer, shutdown).await
}

/// Writes and flushes `bytes`; false if the viewer is gone, too slow, or was kicked meanwhile.
/// If the share stops meanwhile, the write gets [`END_TIMEOUT`] more to finish, so that `end`
/// follows a whole message.
async fn write_stream(
    writer: &mut OwnedWriteHalf,
    bytes: &[u8],
    viewer: &Viewer,
    shutdown: &mut watch::Receiver<bool>,
) -> bool {
    let write = timeout(STREAM_WRITE_TIMEOUT, async {
        writer.write_all(bytes).await?;
        writer.flush().await
    });
    tokio::pin!(write);
    tokio::select! {
        biased;
        written = &mut write => return matches!(written, Ok(Ok(()))),
        _ = viewer.kicked() => return false,
        _ = stopping(shutdown) => {}
    }
    matches!(timeout(END_TIMEOUT, &mut write).await, Ok(Ok(Ok(()))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
    }

    fn limits(connections: usize, per_address: usize) -> Limits {
        Limits {
            connections,
            connections_per_address: per_address,
            reserved: 0,
            ..Limits::default()
        }
    }

    /// Whether `evict` was told to close its connection.
    fn evicted(evict: &Notify) -> bool {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            timeout(Duration::from_millis(10), evict.notified())
                .await
                .is_ok()
        })
    }

    /// Admits a connection from `address` that sent its request head.
    fn request(
        connections: &mut Connections,
        address: IpAddr,
        trusted: bool,
        limits: &Limits,
    ) -> Option<(u64, Arc<Notify>)> {
        let (id, evict) = connections.admit(address, trusted, limits)?;
        assert!(connections.advance(id, Phase::Request));
        Some((id, evict))
    }

    #[test]
    fn connections_per_address() {
        let limits = limits(64, 2);
        let mut connections = Connections::default();
        let (first, _) = connections.admit(ip(1), false, &limits).unwrap();
        connections.admit(ip(1), false, &limits).unwrap();
        assert!(connections.admit(ip(1), false, &limits).is_none());
        assert!(connections.admit(ip(1), true, &limits).is_none());
        assert!(connections.admit(ip(2), false, &limits).is_some());
        connections.remove(first);
        assert!(connections.admit(ip(1), false, &limits).is_some());
        assert_eq!(connections.per_address[&ip(1)], 2);
        assert_eq!(connections.untrusted, 3);
    }

    #[test]
    fn the_oldest_connection_without_a_head_makes_room() {
        let limits = limits(3, 8);
        let mut connections = Connections::default();
        let (a, evict_a) = connections.admit(ip(1), false, &limits).unwrap();
        let (b, evict_b) = connections.admit(ip(2), false, &limits).unwrap();
        let (c, _) = connections.admit(ip(3), false, &limits).unwrap();
        assert!(connections.advance(a, Phase::Request), "a sent its head");
        // Full: b, the oldest still waiting for its head, makes room.
        let (d, _) = connections.admit(ip(4), false, &limits).unwrap();
        assert!(!connections.open.contains_key(&b));
        assert!(!connections.per_address.contains_key(&ip(2)));
        assert!(
            !connections.advance(b, Phase::Request),
            "b was closed meanwhile"
        );
        connections.remove(b);
        assert_eq!(connections.open.len(), 3);
        // Then c.
        connections.admit(ip(5), false, &limits).unwrap();
        assert!(!connections.open.contains_key(&c));
        // When every connection sent its head and no address holds two more than the
        // newcomer's, the newcomer is refused.
        assert!(connections.advance(d, Phase::Request));
        let last = *connections.open.keys().last().unwrap();
        assert!(connections.advance(last, Phase::Request));
        assert!(connections.admit(ip(6), false, &limits).is_none());
        assert_eq!(connections.open.len(), 3);
        assert_eq!(connections.untrusted, 3);
        // The evicted connections were told; the others were not.
        assert!(evicted(&evict_b));
        assert!(!evicted(&evict_a));
    }

    #[test]
    fn a_connection_waiting_for_its_head_makes_room_only_for_fewer_or_its_own() {
        let limits = limits(3, 8);
        let mut connections = Connections::default();
        // A hostile address holds 2 requests; the projector has just connected and not yet
        // sent its request (found in a real run: the hostile PC reconnecting took it).
        for _ in 0..2 {
            request(&mut connections, ip(66), false, &limits).unwrap();
        }
        let (_, fresh) = connections.admit(ip(1), false, &limits).unwrap();
        // The hostile address holds more: it may not take it.
        assert!(connections.admit(ip(66), false, &limits).is_none());
        assert!(!evicted(&fresh));
        // The projector's own next connection may (a browser's spare connection).
        connections.admit(ip(1), false, &limits).unwrap();
        assert!(evicted(&fresh));
        assert_eq!(connections.held(ip(1)), 1);
    }

    #[test]
    fn an_address_that_holds_few_takes_the_room_of_one_that_holds_many() {
        let limits = limits(6, 4);
        let mut connections = Connections::default();
        // A hostile address holds 4 slots with requests it never finishes; another holds a
        // request and an event stream.
        let hostile: Vec<_> = (0..4)
            .map(|_| request(&mut connections, ip(66), false, &limits).unwrap())
            .collect();
        let (_, other_evict) = request(&mut connections, ip(7), false, &limits).unwrap();
        let (stream, stream_evict) = connections.admit(ip(7), false, &limits).unwrap();
        connections.advance(stream, Phase::Stream);

        // A newcomer takes the oldest slot of the address that holds the most (4 ≥ 0 + 2).
        request(&mut connections, ip(1), false, &limits).unwrap();
        assert!(evicted(&hostile[0].1));
        assert_eq!(connections.held(ip(66)), 3);
        // Again (3 ≥ 1 + 2).
        request(&mut connections, ip(1), false, &limits).unwrap();
        assert!(evicted(&hostile[1].1));
        assert_eq!(connections.held(ip(66)), 2);
        assert_eq!(connections.held(ip(1)), 2);
        // Every address holds 2: nobody holds two more than anyone, nothing moves.
        assert!(request(&mut connections, ip(1), false, &limits).is_none());
        // The hostile address cannot take its slots back, even by reconnecting at once.
        assert!(request(&mut connections, ip(66), false, &limits).is_none());
        assert!(request(&mut connections, ip(66), false, &limits).is_none());
        // A new address can (2 ≥ 0 + 2): the oldest request among the largest holders, which
        // is the hostile address's (older than ip(1)'s).
        request(&mut connections, ip(9), false, &limits).unwrap();
        assert!(evicted(&hostile[2].1));
        assert_eq!(connections.held(ip(1)), 2);
        // The other address's request and its event stream were never touched.
        assert!(!evicted(&other_evict));
        assert!(!evicted(&stream_evict));
        assert!(connections.open.contains_key(&stream));
    }

    #[test]
    fn an_event_stream_never_makes_room() {
        let limits = limits(2, 8);
        let mut connections = Connections::default();
        for _ in 0..2 {
            let (id, _) = connections.admit(ip(66), false, &limits).unwrap();
            connections.advance(id, Phase::Stream);
        }
        assert!(connections.admit(ip(1), false, &limits).is_none());
        assert!(connections.admit(ip(1), true, &limits).is_none());
    }

    #[test]
    fn addresses_without_a_token_never_take_the_reserved_slots() {
        let limits = Limits {
            reserved: 2,
            ..limits(6, 8)
        };
        let mut connections = Connections::default();
        // Without a token: 4 slots, the others are reserved.
        let hostile: Vec<_> = (0..4)
            .map(|_| request(&mut connections, ip(66), false, &limits).unwrap())
            .collect();
        assert_eq!(connections.untrusted, 4);
        assert!(connections.admit(ip(66), false, &limits).is_none());
        // Another address without a token takes one of these 4 slots (fair share), not a
        // reserved one.
        request(&mut connections, ip(5), false, &limits).unwrap();
        assert!(evicted(&hostile[0].1));
        assert_eq!(connections.untrusted, 4);
        assert_eq!(connections.open.len(), 4);
        // An address that joined takes the reserved slots without closing anything.
        let (stream, _) = connections.admit(ip(1), true, &limits).unwrap();
        connections.advance(stream, Phase::Stream);
        let (_, page_evict) = request(&mut connections, ip(1), true, &limits).unwrap();
        assert_eq!(connections.open.len(), 6);
        assert!(!evicted(&hostile[1].1));
        // Full: an address without a token takes the room of one without (the hostile address
        // holds 3 ≥ 0 + 2), never of one with a token.
        request(&mut connections, ip(9), false, &limits).unwrap();
        assert!(evicted(&hostile[1].1));
        assert!(!evicted(&page_evict));
        assert_eq!(connections.untrusted, 4);
        // With a token, it takes the room of the oldest without a token, whatever it holds.
        request(&mut connections, ip(1), true, &limits).unwrap();
        assert!(evicted(&hostile[2].1));
        assert_eq!(connections.held(ip(1)), 3);
        assert_eq!(connections.untrusted, 3);
        assert!(!evicted(&page_evict));
    }

    #[test]
    fn an_address_without_a_token_never_closes_one_with_a_token() {
        let limits = Limits {
            reserved: 2,
            ..limits(4, 8)
        };
        let mut connections = Connections::default();
        // With a token: a request and a connection that sent nothing yet.
        let (_, request_evict) = request(&mut connections, ip(1), true, &limits).unwrap();
        let (_, idle_evict) = connections.admit(ip(1), true, &limits).unwrap();
        // Without a token: one still waiting for its head, one request.
        let (_, waiting) = connections.admit(ip(2), false, &limits).unwrap();
        let (_, other) = request(&mut connections, ip(3), false, &limits).unwrap();
        // Full: a newcomer without a token takes the room of the one without a head of its
        // kind…
        request(&mut connections, ip(4), false, &limits).unwrap();
        assert!(evicted(&waiting));
        // …then finds nothing it may close: the connections with a token are never its to
        // take, even the one waiting for its head, though ip(1) holds two more than ip(5).
        assert!(connections.admit(ip(5), false, &limits).is_none());
        assert!(!evicted(&request_evict));
        assert!(!evicted(&idle_evict));
        assert!(!evicted(&other));
        assert_eq!(connections.untrusted, 2);
        assert_eq!(connections.open.len(), 4);
    }

    /// The listener holds `SO_EXCLUSIVEADDRUSE` (read back from the socket).
    #[cfg(windows)]
    #[test]
    fn the_listener_takes_its_port_for_itself() {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            SO_EXCLUSIVEADDRUSE, SOCKET, SOL_SOCKET, getsockopt,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let _context = runtime.enter();
        let listener = listen(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut value: i32 = 0;
        let mut length = size_of::<i32>() as i32;
        // SAFETY: the socket is open for the call; `value` and `length` are live and sized.
        let result = unsafe {
            getsockopt(
                listener.as_raw_socket() as SOCKET,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                (&raw mut value).cast(),
                &raw mut length,
            )
        };
        assert_eq!(result, 0);
        assert_eq!(value, 1);
    }

    #[test]
    fn among_addresses_with_a_token_fair_share_too() {
        let limits = Limits {
            reserved: 1,
            ..limits(3, 8)
        };
        let mut connections = Connections::default();
        let first: Vec<_> = (0..3)
            .map(|_| request(&mut connections, ip(1), true, &limits).unwrap())
            .collect();
        // No connection without a token to close: fair share among those with one.
        request(&mut connections, ip(2), true, &limits).unwrap();
        assert!(evicted(&first[0].1));
        assert!(request(&mut connections, ip(2), true, &limits).is_none());
    }
}
