//! The listener, its thread, and the connections (`docs/design/lan-share.md`, "Server").
//!
//! One current-thread tokio runtime on the thread `ardoise-share`; each connection is a task
//! that reads one request head (bounded in size and time) and answers it, or holds the event
//! stream open. A watch channel tells every task that the share stops.

use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio::time::{Instant, MissedTickBehavior, timeout, timeout_at};

use crate::access::{self, Access, Verdict};
use crate::content::{Content, JoinRefused, Viewer, end_frame};
use crate::http::{self, ContentLength, Head, HeadError, ResponseHead};
use crate::{Assets, LiveFeed, OnChange, ShareConfig, ShareError, ShareStatus};

/// Connections open at once; more are closed at once.
pub(crate) const MAX_CONNECTIONS: usize = 64;
/// Time a client has to send its request (head, and body for `POST /join`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Time to write an answer (the largest is a WebAssembly file of a few hundred KB).
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Time to write one batch of messages to an event stream before the viewer counts as gone.
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(15);
/// A comment line on idle event streams, so that dead connections are found.
const PING_INTERVAL: Duration = Duration::from_secs(15);
/// After an answer, how long (and how many bytes) the client's leftovers are read before the
/// socket closes, so that unread bytes do not turn the close into a reset that could destroy
/// the answer in flight.
const LINGER_TIME: Duration = Duration::from_secs(1);
const LINGER_BYTES: usize = 64 * 1024;
/// At stop, time to write `end`, then time for every connection to finish.
const END_TIMEOUT: Duration = Duration::from_millis(500);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
/// Messages written to an event stream are gathered up to this size per write.
const COALESCE_BYTES: usize = 64 * 1024;
/// First bytes of an event stream: the browser waits 2 s before reconnecting.
const STREAM_PREAMBLE: &str = "retry: 2000\n\n";
const PING: &[u8] = b": ping\n\n";

/// State shared by the share's handle and its thread.
pub(crate) struct Server {
    pub port: u16,
    pub code: String,
    pub content: Content,
    assets: Assets,
    access: Mutex<Access>,
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
    assets: Assets,
    feed: Arc<dyn LiveFeed>,
    on_change: OnChange,
) -> Result<Running, ShareError> {
    let code = match config.code.as_deref().map(access::normalize_code) {
        Some(code) if !code.is_empty() => code,
        _ => access::random_code().map_err(|error| ShareError::Random(error.to_string()))?,
    };
    let listener = std::net::TcpListener::bind(SocketAddrV4::new(config.bind, config.port))
        .map_err(|error| match error.kind() {
            // Also a port that Windows reserves (Hyper-V's excluded ranges answer "access
            // denied"): the remedy is the same, another port.
            std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied => {
                ShareError::PortInUse { port: config.port }
            }
            _ => ShareError::Io(error),
        })?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let listener = {
        let _context = runtime.enter();
        TcpListener::from_std(listener)?
    };
    let (shutdown, _) = watch::channel(false);
    let server = Arc::new(Server {
        port,
        code: code.clone(),
        content: Content::new(feed),
        assets,
        access: Mutex::new(Access::new(code)),
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
        let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let notifier = tokio::spawn(Arc::clone(&self).notify_changes());
        loop {
            tokio::select! {
                _ = stopping(&mut shutdown) => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                            // Too many connections: this one is closed at once.
                            continue;
                        };
                        let server = Arc::clone(&self);
                        tokio::spawn(async move {
                            server.connection(stream, peer).await;
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
        let all = u32::try_from(MAX_CONNECTIONS).unwrap_or(u32::MAX);
        let _ = timeout(SHUTDOWN_GRACE, slots.acquire_many(all)).await;
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

    async fn connection(self: Arc<Self>, mut stream: TcpStream, peer: SocketAddr) {
        let _ = stream.set_nodelay(true);
        let mut shutdown = self.shutdown.subscribe();
        if *shutdown.borrow() {
            return;
        }
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let read = tokio::select! {
            read = timeout_at(deadline, read_head(&mut stream)) => read,
            _ = stopping(&mut shutdown) => return,
        };
        let (head, rest) = match read {
            Ok(Ok(request)) => request,
            Ok(Err(ReadError::Head(error))) => {
                let answer = http::text(error.status(), &[]);
                reply(stream, &answer, &[], &mut shutdown).await;
                return;
            }
            Ok(Err(ReadError::Gone)) | Err(_) => return,
        };
        match (head.method.as_str(), head.path()) {
            ("GET", "/events") => self.events(stream, &head, &mut shutdown).await,
            ("POST", "/join") => {
                let ip = peer.ip();
                let answer = tokio::select! {
                    answer = timeout_at(deadline, self.join(&mut stream, &head, rest, ip)) => answer,
                    _ = stopping(&mut shutdown) => return,
                };
                if let Ok(Some(answer)) = answer {
                    reply(stream, &answer, &[], &mut shutdown).await;
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
                        reply(stream, &head.finish(), &bytes, &mut shutdown).await;
                    }
                    None => reply(stream, &http::text(404, &[]), &[], &mut shutdown).await,
                }
            }
            _ => reply(stream, &http::text(404, &[]), &[], &mut shutdown).await,
        }
    }

    /// `POST /join`: the answer, or `None` when the client went away.
    async fn join(
        &self,
        stream: &mut TcpStream,
        head: &Head,
        mut body: Vec<u8>,
        ip: IpAddr,
    ) -> Option<Vec<u8>> {
        let bad_request = || http::json(400, r#"{"error":"badRequest"}"#, &[]);
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
        let mut access = self.access.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = match access.try_code(ip, &code, std::time::Instant::now()) {
            Verdict::Right => match access::random_token() {
                Ok(token) => {
                    let cookie = format!(
                        "{}={token}; Path=/; HttpOnly; SameSite=Strict",
                        http::COOKIE
                    );
                    access.add_token(token);
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

    /// `GET /events`: checks the cookie, then holds the stream open until the viewer leaves,
    /// is too slow, or the share stops.
    async fn events(&self, stream: TcpStream, head: &Head, shutdown: &mut watch::Receiver<bool>) {
        let authorised = {
            let access = self.access.lock().unwrap_or_else(PoisonError::into_inner);
            head.session_cookies
                .iter()
                .fold(false, |found, token| found | access.token_valid(token))
        };
        if !authorised {
            reply(stream, &http::text(401, &[]), &[], shutdown).await;
            return;
        }
        let viewer = match self.content.join() {
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
        stream_events(stream, &viewer, shutdown).await;
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

/// Reads a request head: the head and the bytes read after it.
async fn read_head(stream: &mut TcpStream) -> Result<(Head, Vec<u8>), ReadError> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    loop {
        let room = http::MAX_HEAD.saturating_sub(buf.len()).min(chunk.len());
        if room == 0 {
            return Err(ReadError::Head(HeadError::TooLarge));
        }
        let n = match stream.read(&mut chunk[..room]).await {
            Ok(0) | Err(_) => return Err(ReadError::Gone),
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
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
async fn stream_events(stream: TcpStream, viewer: &Viewer, shutdown: &mut watch::Receiver<bool>) {
    let (mut reader, mut writer) = stream.into_split();
    let mut head = ResponseHead::new(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", http::NO_STORE)
        .finish();
    head.extend_from_slice(STREAM_PREAMBLE.as_bytes());
    if !write_stream(&mut writer, &head, viewer).await {
        return;
    }
    let mut ping = tokio::time::interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut scratch = [0u8; 64];
    loop {
        if *shutdown.borrow() {
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
            if !write_frames(&mut writer, &pending.frames, viewer).await {
                return;
            }
            continue;
        }
        tokio::select! {
            biased;
            _ = stopping(shutdown) => {}
            _ = viewer.kicked() => {}
            _ = viewer.queued() => {}
            _ = ping.tick() => {
                if !write_stream(&mut writer, PING, viewer).await {
                    return;
                }
            }
            // The viewer only receives: the end of its side, an error, or any byte it sends
            // ends the stream.
            _ = reader.read(&mut scratch) => return,
        }
    }
}

/// Writes queued messages, gathered into writes of up to [`COALESCE_BYTES`].
async fn write_frames(
    writer: &mut OwnedWriteHalf,
    frames: &[crate::content::Frame],
    viewer: &Viewer,
) -> bool {
    let mut gathered: Vec<u8> = Vec::new();
    for frame in frames {
        if frame.len() >= COALESCE_BYTES {
            if !gathered.is_empty() && !write_stream(writer, &gathered, viewer).await {
                return false;
            }
            gathered.clear();
            if !write_stream(writer, frame, viewer).await {
                return false;
            }
        } else {
            gathered.extend_from_slice(frame);
            if gathered.len() >= COALESCE_BYTES {
                if !write_stream(writer, &gathered, viewer).await {
                    return false;
                }
                gathered.clear();
            }
        }
    }
    gathered.is_empty() || write_stream(writer, &gathered, viewer).await
}

/// Writes and flushes `bytes`; false if the viewer is gone, too slow, or was kicked meanwhile.
async fn write_stream(writer: &mut OwnedWriteHalf, bytes: &[u8], viewer: &Viewer) -> bool {
    tokio::select! {
        written = timeout(STREAM_WRITE_TIMEOUT, async {
            writer.write_all(bytes).await?;
            writer.flush().await
        }) => matches!(written, Ok(Ok(()))),
        _ = viewer.kicked() => false,
    }
}
