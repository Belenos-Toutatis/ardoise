//! Just enough HTTP/1.1 for the share (`docs/design/lan-share.md`, "Server"): request heads are
//! parsed by `httparse` from a bounded buffer, and every answer is written by hand with the same
//! security headers. One request per connection (`Connection: close`); nothing pipelined is read.

use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Longest request head accepted (request line and headers).
pub(crate) const MAX_HEAD: usize = 16 * 1024;
/// Longest entry of `X-Forwarded-For` or value of `for=` read as the address of a PC: an IPv6
/// address written in full with an IPv4 end (45 characters) in brackets with a port is 53.
/// Anything longer is no address, and is not even parsed.
pub(crate) const MAX_FORWARDED_ADDRESS: usize = 64;
/// Most headers accepted in a request.
pub(crate) const MAX_HEADERS: usize = 32;
/// Longest body accepted (`POST /join` only).
pub(crate) const MAX_BODY: usize = 1024;
/// Name of the session cookie.
pub(crate) const COOKIE: &str = "ardoise_share";

/// `Content-Security-Policy` of the viewer page (`GET /`).
pub(crate) const PAGE_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
/// `Cache-Control` of everything but the assets.
pub(crate) const NO_STORE: &str = "no-store";
/// `Cache-Control` of the assets (their names carry a hash of their content).
pub(crate) const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// What the share needs from a request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Head {
    pub method: String,
    /// The request target, verbatim (query string included).
    pub target: String,
    /// Bytes taken by the head: the body, if any, starts there.
    pub len: usize,
    pub content_length: ContentLength,
    /// A `Transfer-Encoding` header was given (chunked bodies are not accepted).
    pub transfer_encoding: bool,
    /// Values of the session cookie, in the order given.
    pub session_cookies: Vec<String>,
    /// Values of the `Content-Type`, `Origin` and `Host` headers, in the order given.
    pub content_types: Vec<String>,
    pub origins: Vec<String>,
    pub hosts: Vec<String>,
    /// A proxy forwarded it (`Via`, `Forwarded` or `X-Forwarded-For` given): for the journal.
    pub proxied: bool,
    /// The PC that the proxy says it forwards the request for.
    pub forwarded_for: ForwardedFor,
}

/// The PC that a proxy says it forwards a request for (`docs/design/lan-share.md`, "Behind a
/// proxy"). Read from `X-Forwarded-For` when the request has one (squid adds it by default,
/// e2guardian with `addforwardedfor`), else from `Forwarded: for=` (RFC 7239). In both, the nearest
/// proxy, the one this share is connected to, writes its entry last: the entries before it come
/// from the browser or from proxies further away, and only the last is read (MDN,
/// "X-Forwarded-For"; RFC 7239 §4), save a loopback one (see [`nearest_pc`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForwardedFor {
    /// Neither header.
    Absent,
    /// The header read (its name) gives no address: empty, too long, not an address.
    Unreadable(&'static str),
    /// The address of the PC, and the header it was read from.
    Address(IpAddr, &'static str),
}

impl ForwardedFor {
    /// The address of the PC, if one was read.
    pub fn address(self) -> Option<IpAddr> {
        match self {
            ForwardedFor::Address(address, _) => Some(address),
            ForwardedFor::Absent | ForwardedFor::Unreadable(_) => None,
        }
    }
}

/// The PC that the nearest proxy names, from the addresses of a header's entries, last first:
/// the last entry, or while it is a loopback address, the one before it. A loopback entry is
/// written by a proxy whose client runs on its own machine: the classic filter is e2guardian
/// handing each request to a squid on the same router (e2guardian adds its `X-Forwarded-For`
/// only when it goes through that upstream proxy, and squid appends its own client's address,
/// e2guardian's: `<PC>, 127.0.0.1`). A browser elsewhere never gets a loopback entry written for
/// it, so it cannot use this to choose its name. That the school's filter is built that way is
/// (supposé): without this step every PC behind such a pair is one client, `127.0.0.1`.
fn nearest_pc(mut entries: impl Iterator<Item = Option<IpAddr>>) -> Option<IpAddr> {
    let mut found = entries.next()??;
    while found.is_loopback() {
        match entries.next() {
            Some(earlier) => found = earlier?,
            None => break,
        }
    }
    Some(found)
}

/// The entries of the values of a header given several times, last first: one list (RFC 9110
/// §5.3; e2guardian adds a line of its own after the browser's rather than appending to it).
fn entries_last_first<'a>(values: &'a [&'a [u8]]) -> impl Iterator<Item = &'a [u8]> + 'a {
    values
        .iter()
        .rev()
        .flat_map(|value| value.rsplit(|&b| b == b','))
}

/// The address of the PC in the `X-Forwarded-For` headers (see [`nearest_pc`]).
fn forwarded_for_header(values: &[&[u8]]) -> ForwardedFor {
    let entries = entries_last_first(values).map(forwarded_address);
    match nearest_pc(entries) {
        Some(address) => ForwardedFor::Address(address, "X-Forwarded-For"),
        None => ForwardedFor::Unreadable("X-Forwarded-For"),
    }
}

/// The address of the PC in the `Forwarded` headers: the `for=` of their last element
/// (`for=192.0.2.60;proto=http, for="[2001:db8::17]:4711"`; see [`nearest_pc`]). A quoted value
/// holding a comma is cut there, and then gives no address.
fn forwarded_header(values: &[&[u8]]) -> ForwardedFor {
    let entries = entries_last_first(values).map(|element| {
        element
            .split(|&b| b == b';')
            .find_map(|pair| {
                let mut parts = pair.splitn(2, |&b| b == b'=');
                let name = parts.next()?;
                let value = parts.next()?;
                name.trim_ascii()
                    .eq_ignore_ascii_case(b"for")
                    .then_some(value)
            })
            .and_then(forwarded_address)
    });
    match nearest_pc(entries) {
        Some(address) => ForwardedFor::Address(address, "Forwarded"),
        None => ForwardedFor::Unreadable("Forwarded"),
    }
}

/// One address as proxies write it: `192.0.2.60`, `192.0.2.60:4711`, `2001:db8::17`,
/// `[2001:db8::17]`, `[2001:db8::17]:4711`, perhaps in quotes; the port is dropped and an IPv4
/// address written as IPv6 (`::ffff:192.0.2.60`) is the IPv4 address. `None` for anything else
/// (`unknown`, `_hidden`, a name, an empty or overlong value).
fn forwarded_address(text: &[u8]) -> Option<IpAddr> {
    let text = text.trim_ascii();
    let text = text
        .strip_prefix(b"\"")
        .and_then(|inner| inner.strip_suffix(b"\""))
        .unwrap_or(text);
    if text.is_empty() || text.len() > MAX_FORWARDED_ADDRESS {
        return None;
    }
    let text = std::str::from_utf8(text).ok()?;
    let address = text
        .parse::<IpAddr>()
        .ok()
        .or_else(|| text.parse::<SocketAddr>().ok().map(|socket| socket.ip()))
        .or_else(|| {
            let inner = text.strip_prefix('[')?.strip_suffix(']')?;
            inner.parse::<Ipv6Addr>().ok().map(IpAddr::V6)
        })?;
    Some(address.to_canonical())
}

impl Head {
    /// The target without its query string: what the routes and the asset map are matched
    /// against. Nothing is decoded or normalised.
    pub fn path(&self) -> &str {
        match self.target.split_once('?') {
            Some((path, _)) => path,
            None => &self.target,
        }
    }

    /// One `Content-Type` header whose media type is `application/json` (parameters such as
    /// `charset` allowed). A browser sends this type across origins only after a preflight
    /// `OPTIONS`, which gets 404.
    pub fn is_json(&self) -> bool {
        match self.content_types.as_slice() {
            [content_type] => content_type
                .split(';')
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json")),
            _ => false,
        }
    }

    /// No `Host` header (not a browser: a browser always sends one), or one that names this
    /// share: an IPv4 address, `localhost` or `name` (this computer's name), with `port` or
    /// without a port. A browser sends the host of the page's address: a page of another site
    /// whose DNS name was pointed at this PC (DNS rebinding) names its own, and is refused.
    pub fn host_allowed(&self, port: u16, name: Option<&str>) -> bool {
        match self.hosts.as_slice() {
            [] => true,
            [host] => allowed_host(host, port, name),
            _ => false,
        }
    }

    /// No `Origin` header (not a browser, or a request that is not cross-site), or one that is
    /// `http://` followed by the `Host` header: the page served by this share (whose `Host`
    /// [`Head::host_allowed`] checked first).
    pub fn same_origin(&self) -> bool {
        match (self.origins.as_slice(), self.hosts.as_slice()) {
            ([], _) => true,
            ([origin], [host]) => origin
                .strip_prefix("http://")
                .is_some_and(|origin_host| origin_host.eq_ignore_ascii_case(host)),
            _ => false,
        }
    }
}

/// A `Host` header value that names this share (see [`Head::host_allowed`]).
pub(crate) fn allowed_host(value: &str, port: u16, name: Option<&str>) -> bool {
    let (host, given_port) = match value.split_once(':') {
        Some((host, digits)) => (host, Some(digits)),
        None => (value, None),
    };
    if let Some(digits) = given_port {
        let ours = !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
            && digits.parse::<u16>().ok() == Some(port);
        if !ours {
            return false;
        }
    }
    host.parse::<Ipv4Addr>().is_ok()
        || host.eq_ignore_ascii_case("localhost")
        || name.is_some_and(|name| host.eq_ignore_ascii_case(name))
}

/// This computer's name, as another PC of the network may type it (`http://PC-SALLE12:8765/`):
/// Windows' `COMPUTERNAME`, if it is a plain name (letters, digits, `-`); `None` elsewhere.
pub(crate) fn computer_name() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let name = std::env::var("COMPUTERNAME").ok()?;
    plain_name(&name).then_some(name)
}

/// A host name of letters, digits and `-` (a single DNS label, as a NetBIOS name is).
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// True if `bytes` hold the blank line that ends a request head (`\n\n` or `\n\r\n`): only
/// then is the head worth parsing again (parsing at every byte would be quadratic).
pub(crate) fn has_blank_line(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|w| w == b"\n\n") || bytes.windows(3).any(|w| w == b"\n\r\n")
}

/// The `Content-Length` of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentLength {
    Absent,
    Given(usize),
    /// Not a number, or several different values.
    Invalid,
}

/// Why a request head is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeadError {
    /// More than [`MAX_HEAD`] bytes without the end of the head.
    TooLarge,
    /// More than [`MAX_HEADERS`] headers.
    TooManyHeaders,
    /// Not an HTTP/1.x request.
    Malformed,
}

impl HeadError {
    /// The status answered.
    pub fn status(self) -> u16 {
        match self {
            HeadError::TooLarge | HeadError::TooManyHeaders => 431,
            HeadError::Malformed => 400,
        }
    }
}

/// Parses the request head at the start of `buf`: `Ok(None)` while it is incomplete.
pub(crate) fn parse_head(buf: &[u8]) -> Result<Option<Head>, HeadError> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut headers);
    let len = match request.parse(buf) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) if buf.len() >= MAX_HEAD => return Err(HeadError::TooLarge),
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(httparse::Error::TooManyHeaders) => return Err(HeadError::TooManyHeaders),
        Err(_) => return Err(HeadError::Malformed),
    };
    if len > MAX_HEAD {
        return Err(HeadError::TooLarge);
    }
    let (Some(method), Some(target)) = (request.method, request.path) else {
        return Err(HeadError::Malformed);
    };
    let mut content_length = ContentLength::Absent;
    let mut transfer_encoding = false;
    let mut session_cookies = Vec::new();
    let mut content_types = Vec::new();
    let mut origins = Vec::new();
    let mut hosts = Vec::new();
    let mut proxied = false;
    // Their values, in order (at most `MAX_HEADERS`).
    let mut x_forwarded_for: Vec<&[u8]> = Vec::new();
    let mut forwarded: Vec<&[u8]> = Vec::new();
    // A value that is not UTF-8 is kept lossily: it then matches nothing expected.
    let text = |value: &[u8]| String::from_utf8_lossy(value).trim().to_owned();
    for header in request.headers.iter() {
        let name = header.name;
        if name.eq_ignore_ascii_case("content-length") {
            content_length = merge_length(content_length, header.value);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            transfer_encoding = true;
        } else if name.eq_ignore_ascii_case("cookie")
            && let Ok(value) = std::str::from_utf8(header.value)
        {
            session_cookies.extend(cookie_values(value, COOKIE).map(str::to_owned));
        } else if name.eq_ignore_ascii_case("content-type") {
            content_types.push(text(header.value));
        } else if name.eq_ignore_ascii_case("origin") {
            origins.push(text(header.value));
        } else if name.eq_ignore_ascii_case("host") {
            hosts.push(text(header.value));
        } else if name.eq_ignore_ascii_case("x-forwarded-for") {
            proxied = true;
            x_forwarded_for.push(header.value);
        } else if name.eq_ignore_ascii_case("forwarded") {
            proxied = true;
            forwarded.push(header.value);
        } else if name.eq_ignore_ascii_case("via") {
            proxied = true;
        }
    }
    // `X-Forwarded-For` first: it is the header the usual proxies add, so a `Forwarded` header
    // that a browser makes up beside it is never read.
    let forwarded_for = if !x_forwarded_for.is_empty() {
        forwarded_for_header(&x_forwarded_for)
    } else if !forwarded.is_empty() {
        forwarded_header(&forwarded)
    } else {
        ForwardedFor::Absent
    };
    Ok(Some(Head {
        method: method.to_owned(),
        target: target.to_owned(),
        len,
        content_length,
        transfer_encoding,
        session_cookies,
        content_types,
        origins,
        hosts,
        proxied,
        forwarded_for,
    }))
}

/// Adds one `Content-Length` value to what the previous ones said.
fn merge_length(previous: ContentLength, value: &[u8]) -> ContentLength {
    let value = value.trim_ascii();
    let parsed = if !value.is_empty() && value.iter().all(u8::is_ascii_digit) {
        std::str::from_utf8(value)
            .ok()
            .and_then(|digits| digits.parse::<usize>().ok())
    } else {
        None
    };
    match (previous, parsed) {
        (_, None) | (ContentLength::Invalid, _) => ContentLength::Invalid,
        (ContentLength::Absent, Some(length)) => ContentLength::Given(length),
        (ContentLength::Given(before), Some(length)) if before == length => {
            ContentLength::Given(length)
        }
        (ContentLength::Given(_), Some(_)) => ContentLength::Invalid,
    }
}

/// Values of the cookie `name` in a `Cookie` header value (`a=1; name=2; b=3`).
fn cookie_values<'a>(header: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    header.split(';').filter_map(move |pair| {
        let (key, value) = pair.split_once('=')?;
        (key.trim() == name).then(|| value.trim().trim_matches('"'))
    })
}

/// Reason phrase of the statuses the share answers.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// A response head: the status line and the headers every answer carries (`Connection: close`,
/// `X-Content-Type-Options`, `Referrer-Policy`), then those added.
pub(crate) struct ResponseHead {
    text: String,
}

impl ResponseHead {
    pub fn new(status: u16) -> Self {
        let mut text = String::with_capacity(512);
        let _ = write!(
            text,
            "HTTP/1.1 {status} {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\
             Referrer-Policy: no-referrer\r\n",
            reason(status)
        );
        Self { text }
    }

    /// Adds a header. A value with a line break or another control character (never one of
    /// ours) is dropped rather than written.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        if value.bytes().all(|b| b == b'\t' || !b.is_ascii_control()) {
            let _ = write!(self.text, "{name}: {value}\r\n");
        }
        self
    }

    /// The head, ending with the blank line.
    pub fn finish(mut self) -> Vec<u8> {
        self.text.push_str("\r\n");
        self.text.into_bytes()
    }
}

/// A complete small answer with a body (`Cache-Control: no-store`).
pub(crate) fn simple(
    status: u16,
    content_type: &str,
    body: &str,
    extra: &[(&str, &str)],
) -> Vec<u8> {
    let mut head = ResponseHead::new(status)
        .header("Content-Type", content_type)
        .header("Content-Length", &body.len().to_string())
        .header("Cache-Control", NO_STORE);
    for (name, value) in extra {
        head = head.header(name, value);
    }
    let mut bytes = head.finish();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// A plain-text answer (errors other than `POST /join`'s).
pub(crate) fn text(status: u16, extra: &[(&str, &str)]) -> Vec<u8> {
    let body = format!("{}\n", reason(status));
    simple(status, "text/plain; charset=utf-8", &body, extra)
}

/// A JSON answer.
pub(crate) fn json(status: u16, body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    simple(status, "application/json", body, extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Option<Head>, HeadError> {
        parse_head(text.as_bytes())
    }

    #[test]
    fn complete_head() {
        let raw = "GET /assets/app.js?v=2 HTTP/1.1\r\nHost: pc:8765\r\nCookie: a=1; ardoise_share=abc ; b=2\r\n\r\nrest";
        let head = parse(raw).unwrap().unwrap();
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "/assets/app.js?v=2");
        assert_eq!(head.path(), "/assets/app.js");
        assert_eq!(head.len, raw.len() - "rest".len());
        assert_eq!(head.content_length, ContentLength::Absent);
        assert!(!head.transfer_encoding);
        assert_eq!(head.session_cookies, vec!["abc".to_owned()]);
    }

    #[test]
    fn partial_head_waits_for_more() {
        assert_eq!(parse("GET / HTTP/1.1\r\nHost: x\r\n"), Ok(None));
        assert_eq!(parse("GE"), Ok(None));
        assert_eq!(parse(""), Ok(None));
    }

    #[test]
    fn head_too_large() {
        let mut raw = String::from("GET / HTTP/1.1\r\nX-Big: ");
        raw.push_str(&"a".repeat(MAX_HEAD));
        assert_eq!(parse(&raw), Err(HeadError::TooLarge));
        assert_eq!(HeadError::TooLarge.status(), 431);
    }

    #[test]
    fn too_many_headers() {
        let mut raw = String::from("GET / HTTP/1.1\r\n");
        for i in 0..=MAX_HEADERS {
            raw.push_str(&format!("X-{i}: 1\r\n"));
        }
        raw.push_str("\r\n");
        assert_eq!(parse(&raw), Err(HeadError::TooManyHeaders));
        // Exactly the limit is fine.
        let mut raw = String::from("GET / HTTP/1.1\r\n");
        for i in 0..MAX_HEADERS {
            raw.push_str(&format!("X-{i}: 1\r\n"));
        }
        raw.push_str("\r\n");
        assert!(parse(&raw).unwrap().is_some());
    }

    #[test]
    fn malformed_heads() {
        for raw in [
            "GET\r\n\r\n",
            "GET / HTTP/2.0\r\n\r\n",
            "GET  / HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nBad Header: x\r\n\r\n",
            "\x00\x01\x02 / HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(parse(raw), Err(HeadError::Malformed), "{raw:?}");
        }
        // A target that is not UTF-8.
        let raw = b"GET /\xff HTTP/1.1\r\n\r\n";
        assert_eq!(parse_head(raw), Err(HeadError::Malformed));
    }

    #[test]
    fn targets_are_kept_verbatim() {
        for target in [
            "/assets/../x",
            "/assets/%2e%2e/x",
            "//assets/x",
            "/assets\\x",
            "/ASSETS/x",
            "http://host/",
            "*",
        ] {
            let raw = format!("GET {target} HTTP/1.1\r\n\r\n");
            let head = parse(&raw).unwrap().unwrap();
            assert_eq!(head.target, target);
        }
    }

    #[test]
    fn content_length_rules() {
        let length = |headers: &str| {
            parse(&format!("POST /join HTTP/1.1\r\n{headers}\r\n"))
                .unwrap()
                .unwrap()
                .content_length
        };
        assert_eq!(length("Content-Length: 12\r\n"), ContentLength::Given(12));
        assert_eq!(length("content-length:  7 \r\n"), ContentLength::Given(7));
        assert_eq!(
            length("Content-Length: 3\r\nContent-Length: 3\r\n"),
            ContentLength::Given(3)
        );
        assert_eq!(
            length("Content-Length: 3\r\nContent-Length: 4\r\n"),
            ContentLength::Invalid
        );
        assert_eq!(length("Content-Length: -1\r\n"), ContentLength::Invalid);
        assert_eq!(length("Content-Length: +1\r\n"), ContentLength::Invalid);
        assert_eq!(length("Content-Length: \r\n"), ContentLength::Invalid);
        assert_eq!(
            length("Content-Length: 99999999999999999999999999\r\n"),
            ContentLength::Invalid
        );
        let head = parse("POST /join HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
            .unwrap()
            .unwrap();
        assert!(head.transfer_encoding);
    }

    #[test]
    fn session_cookies() {
        let head = parse(
            "GET /events HTTP/1.1\r\nCookie: ardoise_share=one\r\nCookie: x=y;ardoise_share=\"two\"; ardoise_sharex=no\r\n\r\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            head.session_cookies,
            vec!["one".to_owned(), "two".to_owned()]
        );
    }

    #[test]
    fn the_pc_a_proxy_forwards_for() {
        let read = |headers: &str| {
            let head = parse(&format!("POST /join HTTP/1.1\r\n{headers}\r\n"))
                .unwrap()
                .unwrap();
            (head.proxied, head.forwarded_for)
        };
        let v4 = |text: &str| IpAddr::V4(text.parse().unwrap());
        let v6 = |text: &str| IpAddr::V6(text.parse().unwrap());
        let xff = |address| (true, ForwardedFor::Address(address, "X-Forwarded-For"));
        let standard = |address| (true, ForwardedFor::Address(address, "Forwarded"));
        assert_eq!(read(""), (false, ForwardedFor::Absent));
        assert_eq!(
            read("Via: 1.1 e2guardian\r\n"),
            (true, ForwardedFor::Absent)
        );
        // The last entry is the one the nearest proxy wrote; those before come from the browser.
        for (headers, address) in [
            ("X-Forwarded-For: 192.168.228.37\r\n", v4("192.168.228.37")),
            (
                "X-Forwarded-For: 10.9.9.9, 192.168.228.37\r\n",
                v4("192.168.228.37"),
            ),
            (
                "X-Forwarded-For: 10.9.9.9\r\nx-forwarded-for:  192.168.228.37 \r\n",
                v4("192.168.228.37"),
            ),
            (
                "X-Forwarded-For: 192.168.228.37:61050\r\n",
                v4("192.168.228.37"),
            ),
            (
                "X-Forwarded-For: \"192.168.228.37\"\r\n",
                v4("192.168.228.37"),
            ),
            ("X-Forwarded-For: 2001:db8::17\r\n", v6("2001:db8::17")),
            ("X-Forwarded-For: [2001:DB8::17]\r\n", v6("2001:db8::17")),
            (
                "X-Forwarded-For: [2001:db8::17]:4711\r\n",
                v6("2001:db8::17"),
            ),
            // An IPv4 address written as IPv6 is that IPv4 address: one record, not two.
            (
                "X-Forwarded-For: ::ffff:192.168.228.37\r\n",
                v4("192.168.228.37"),
            ),
            // Two proxies on one machine (e2guardian, then squid): the loopback entry is the
            // first one's, the one before it is the PC.
            (
                "X-Forwarded-For: 192.168.228.37, 127.0.0.1\r\n",
                v4("192.168.228.37"),
            ),
            (
                "X-Forwarded-For: 10.9.9.9, 192.168.228.37, ::1, 127.0.0.1\r\n",
                v4("192.168.228.37"),
            ),
            (
                "X-Forwarded-For: 192.168.228.37\r\nX-Forwarded-For: 127.0.0.1\r\n",
                v4("192.168.228.37"),
            ),
            // A browser on the proxy's own machine.
            ("X-Forwarded-For: 127.0.0.1\r\n", v4("127.0.0.1")),
            ("X-Forwarded-For: ::1, 127.0.0.1\r\n", v6("::1")),
        ] {
            assert_eq!(read(headers), xff(address), "{headers:?}");
        }
        // A loopback entry after one that is no address: no PC, never the loopback.
        assert_eq!(
            read("X-Forwarded-For: unknown, 127.0.0.1\r\n"),
            (true, ForwardedFor::Unreadable("X-Forwarded-For"))
        );
        // A loopback address before the proxy's own entry is the browser's word: never read.
        assert_eq!(
            read("X-Forwarded-For: 127.0.0.1, 192.168.228.37\r\n"),
            xff(v4("192.168.228.37"))
        );
        for (headers, address) in [
            ("Forwarded: for=192.0.2.60\r\n", v4("192.0.2.60")),
            (
                "Forwarded: proto=http;For=192.0.2.60;by=203.0.113.43\r\n",
                v4("192.0.2.60"),
            ),
            (
                "Forwarded: for=10.9.9.9, for=\"[2001:db8:cafe::17]:4711\"\r\n",
                v6("2001:db8:cafe::17"),
            ),
            (
                "Forwarded: for=10.9.9.9\r\nForwarded: for = 192.0.2.60\r\n",
                v4("192.0.2.60"),
            ),
            (
                "Forwarded: for=192.0.2.60;proto=http, for=127.0.0.1\r\n",
                v4("192.0.2.60"),
            ),
        ] {
            assert_eq!(read(headers), standard(address), "{headers:?}");
        }
        // `X-Forwarded-For` first, whatever the order: a browser cannot add `Forwarded` to
        // choose its own address beside the proxy's `X-Forwarded-For`.
        assert_eq!(
            read("Forwarded: for=10.9.9.9\r\nX-Forwarded-For: 192.168.228.37\r\n"),
            xff(v4("192.168.228.37"))
        );
        // Nothing that is not an address is ever used.
        let huge = "9".repeat(8 * 1024);
        let many = format!("{}192.168.228.37", "10.0.0.1, ".repeat(1000));
        assert_eq!(
            read(&format!("X-Forwarded-For: {many}\r\n")),
            xff(v4("192.168.228.37"))
        );
        let near_limit = format!("{}1", "0".repeat(MAX_FORWARDED_ADDRESS + 1));
        for value in [
            "",
            " ",
            ",",
            "192.168.228.37,",
            "192.168.228.37, ",
            "unknown",
            "pc-salle12",
            "192.168.228.300",
            "192.168.228",
            "fe80::1%eth0",
            "[2001:db8::17",
            "\"\"",
            "1.2.3.4 5.6.7.8",
            &huge,
            &near_limit,
        ] {
            assert_eq!(
                read(&format!("X-Forwarded-For: {value}\r\n")),
                (true, ForwardedFor::Unreadable("X-Forwarded-For")),
                "{:.60}",
                value
            );
        }
        for value in [
            "",
            "for=",
            "for=unknown",
            "for=_hidden",
            "for=\"_gazonk\"",
            "proto=http;by=203.0.113.43",
            "for=192.0.2.60, proto=http",
            "for=\"[2001:db8::17],x\"",
            "for",
        ] {
            assert_eq!(
                read(&format!("Forwarded: {value}\r\n")),
                (true, ForwardedFor::Unreadable("Forwarded")),
                "{value:?}"
            );
        }
        // Bytes that are not UTF-8 are no address either (earlier entries do not matter).
        let mut raw =
            b"GET / HTTP/1.1\r\nX-Forwarded-For: \xff\xfe, 192.168.228.37\r\n\r\n".to_vec();
        let head = parse_head(&raw).unwrap().unwrap();
        assert_eq!(head.forwarded_for, xff(v4("192.168.228.37")).1);
        raw = b"GET / HTTP/1.1\r\nX-Forwarded-For: 192.168.228.37, \xff\r\n\r\n".to_vec();
        let head = parse_head(&raw).unwrap().unwrap();
        assert_eq!(
            head.forwarded_for,
            ForwardedFor::Unreadable("X-Forwarded-For")
        );
        assert_eq!(ForwardedFor::Unreadable("Forwarded").address(), None);
        assert_eq!(ForwardedFor::Absent.address(), None);
        assert_eq!(
            xff(v4("192.168.228.37")).1.address(),
            Some(v4("192.168.228.37"))
        );
    }

    #[test]
    fn json_content_type() {
        let head = |headers: &str| {
            parse(&format!("POST /join HTTP/1.1\r\n{headers}\r\n"))
                .unwrap()
                .unwrap()
        };
        for json in [
            "Content-Type: application/json\r\n",
            "content-type: Application/JSON\r\n",
            "Content-Type: application/json; charset=utf-8\r\n",
            "Content-Type:  application/json ;charset=UTF-8\r\n",
        ] {
            assert!(head(json).is_json(), "{json}");
        }
        for other in [
            "",
            "Content-Type: text/plain\r\n",
            "Content-Type: application/x-www-form-urlencoded\r\n",
            "Content-Type: multipart/form-data; boundary=x\r\n",
            "Content-Type: application/jsonx\r\n",
            "Content-Type: text/plain; a=application/json\r\n",
            "Content-Type: application/json\r\nContent-Type: application/json\r\n",
        ] {
            assert!(!head(other).is_json(), "{other}");
        }
    }

    #[test]
    fn origins() {
        let head = |headers: &str| {
            parse(&format!("POST /join HTTP/1.1\r\n{headers}\r\n"))
                .unwrap()
                .unwrap()
        };
        for same in [
            "Host: 192.168.1.23:8765\r\n",
            "",
            "Host: 192.168.1.23:8765\r\nOrigin: http://192.168.1.23:8765\r\n",
            "Origin: http://PC.local:8765\r\nHost: pc.local:8765\r\n",
        ] {
            assert!(head(same).same_origin(), "{same}");
        }
        for other in [
            "Host: 192.168.1.23:8765\r\nOrigin: http://evil.example\r\n",
            "Host: 192.168.1.23:8765\r\nOrigin: null\r\n",
            "Host: 192.168.1.23:8765\r\nOrigin: https://192.168.1.23:8765\r\n",
            "Host: 192.168.1.23:8765\r\nOrigin: http://192.168.1.23\r\n",
            "Host: 192.168.1.23:8765\r\nOrigin: http://192.168.1.23:8765/\r\n",
            "Origin: http://192.168.1.23:8765\r\n",
            "Host: a\r\nHost: a\r\nOrigin: http://a\r\n",
            "Host: a\r\nOrigin: http://a\r\nOrigin: http://a\r\n",
        ] {
            assert!(!head(other).same_origin(), "{other}");
        }
    }

    #[test]
    fn hosts() {
        let head = |headers: &str| {
            parse(&format!("GET / HTTP/1.1\r\n{headers}\r\n"))
                .unwrap()
                .unwrap()
        };
        let allowed = |headers: &str| head(headers).host_allowed(8765, Some("PC-SALLE12"));
        for ours in [
            "",
            "Host: 192.168.1.23:8765\r\n",
            "Host: 192.168.1.23\r\n",
            "Host: 10.0.0.7:8765\r\n",
            "Host: 127.0.0.1:8765\r\n",
            "Host: localhost:8765\r\n",
            "Host: LocalHost\r\n",
            "Host: pc-salle12:8765\r\n",
            "Host: PC-SALLE12\r\n",
        ] {
            assert!(allowed(ours), "{ours:?}");
        }
        for other in [
            "Host: evil.example\r\n",
            "Host: evil.example:8765\r\n",
            "Host: 192.168.1.23.evil.example:8765\r\n",
            "Host: pc-salle12.evil.example:8765\r\n",
            "Host: localhost.evil.example\r\n",
            "Host: 192.168.1.23:8766\r\n",
            "Host: 192.168.1.23:\r\n",
            "Host: 192.168.1.23:+8765\r\n",
            "Host: 192.168.1.23:8765:8765\r\n",
            "Host: 192.168.1.256:8765\r\n",
            "Host: [::1]:8765\r\n",
            "Host: \r\n",
            "Host: 192.168.1.23:8765\r\nHost: 192.168.1.23:8765\r\n",
        ] {
            assert!(!allowed(other), "{other:?}");
        }
        // Without a name of this computer, only addresses and `localhost`.
        assert!(!head("Host: pc-salle12\r\n").host_allowed(8765, None));
        assert!(head("Host: localhost\r\n").host_allowed(8765, None));
        // Plain names only.
        assert!(plain_name("PC-SALLE12"));
        assert!(!plain_name(""));
        assert!(!plain_name("pc.example"));
        assert!(!plain_name("pc salle"));
        assert!(!plain_name(&"a".repeat(64)));
        if let Some(name) = computer_name() {
            assert!(plain_name(&name), "{name}");
        }
    }

    #[test]
    fn blank_lines() {
        assert!(has_blank_line(b"GET / HTTP/1.1\r\n\r\n"));
        assert!(has_blank_line(b"\n\r\n"));
        assert!(has_blank_line(b"x\n\n"));
        assert!(!has_blank_line(b"GET / HTTP/1.1\r\nHost: x\r\n"));
        assert!(!has_blank_line(b"\r\n\r"));
        assert!(!has_blank_line(b""));
    }

    #[test]
    fn response_heads() {
        let bytes = ResponseHead::new(204)
            .header("Set-Cookie", "a=b")
            .header("X-Bad", "a\r\nInjected: yes")
            .finish();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 204 No Content\r\n"));
        assert!(text.contains("\r\nConnection: close\r\n"));
        assert!(text.contains("\r\nX-Content-Type-Options: nosniff\r\n"));
        assert!(text.contains("\r\nReferrer-Policy: no-referrer\r\n"));
        assert!(text.contains("\r\nSet-Cookie: a=b\r\n"));
        assert!(!text.contains("Injected"));
        assert!(text.ends_with("\r\n\r\n"));

        let text = String::from_utf8(json(403, "{}", &[])).unwrap();
        assert!(text.contains("Content-Type: application/json\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.ends_with("\r\n\r\n{}"));
    }
}
