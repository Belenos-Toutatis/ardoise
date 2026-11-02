//! Just enough HTTP/1.1 for the share (`docs/design/lan-share.md`, "Server"): request heads are
//! parsed by `httparse` from a bounded buffer, and every answer is written by hand with the same
//! security headers. One request per connection (`Connection: close`); nothing pipelined is read.

use std::fmt::Write as _;

/// Longest request head accepted (request line and headers).
pub(crate) const MAX_HEAD: usize = 16 * 1024;
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
    for header in request.headers.iter() {
        if header.name.eq_ignore_ascii_case("content-length") {
            content_length = merge_length(content_length, header.value);
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            transfer_encoding = true;
        } else if header.name.eq_ignore_ascii_case("cookie")
            && let Ok(value) = std::str::from_utf8(header.value)
        {
            session_cookies.extend(cookie_values(value, COOKIE).map(str::to_owned));
        }
    }
    Ok(Some(Head {
        method: method.to_owned(),
        target: target.to_owned(),
        len,
        content_length,
        transfer_encoding,
        session_cookies,
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
