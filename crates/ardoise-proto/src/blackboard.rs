//! The vendor protocol of the Xiaomi Mijia LCD blackboard, storage edition (米家液晶小黑板 存储版,
//! Mi Home model `jiqid.sxb.v1`), as measured in docs/research/xiaomi-blackboard.md §3.
//!
//! Everything here comes from captures of Mi Home and from sessions driven from the PC: the
//! frames below are the only ones the host may ever send ([`BoardRequest`] has no other
//! variant and no "unknown" one, so the host cannot encode anything else). The Xiaomi service
//! (`fe95`) and its login are not part of this protocol: Ardoise never touches them.
//!
//! - Commands are written to the Nordic UART TX characteristic (`6e400002-…`); replies and
//!   events come on Nordic UART RX (`6e400003-…`), the same characteristics as the Slate's
//!   command channel. A frame is `group, length, code, payload`: `length` counts the code and
//!   the payload. Integers in frames are **big-endian** ([`BoardFrame`]).
//! - Live pen samples and the bytes of a stored page arrive on a third characteristic,
//!   `6e400004-…`, as 7-byte [`Record`]s (integers **little-endian** there): a stored page is
//!   the same records as the live stream, in the order they were written ([`record`]).
//! - [`beacon`]: how the tablet is recognised in its advertisement (the MiBeacon of `fe95`).
//!
//! Unknown frames decode to [`BoardReply::Unknown`]; known codes with an unexpected payload are
//! a [`BoardMessageError`]. Nothing here panics on bytes from the tablet.

pub mod beacon;
pub mod record;

use std::fmt;

pub use record::{
    DecodedPage, PageStroke, Record, RecordStatus, TimedPoint, decode_page, decode_records,
};

/// Group of the device commands (battery, memory, clock).
pub const GROUP_DEVICE: u8 = 0x11;
/// Group of the page commands (count, describe, download, delete, live mode, events).
pub const GROUP_PAGES: u8 = 0x12;

/// Codes of the frames seen in the captures (research note §3, "Frames of the vendor channel").
pub mod code {
    /// `11 01 01` → `11 02 01 <percent>`.
    pub const BATTERY: u8 = 0x01;
    /// `11 01 02` → `11 09 02 <total u32> <free u32>`.
    pub const MEMORY: u8 = 0x02;
    /// `11 05 07 <unix u32>`, no reply.
    pub const SET_TIME: u8 = 0x07;
    /// `12 01 04` → `12 07 04 <count u16> <bytes u32>`.
    pub const PAGE_COUNT: u8 = 0x04;
    /// `12 01 05` → `12 09 05 <time u32> <size u32>`, or the bare `12 01 05` on an empty tablet.
    pub const OLDEST_PAGE: u8 = 0x05;
    /// `12 06 03 01 <time u32>`: the page's bytes follow on the stream characteristic.
    pub const DOWNLOAD: u8 = 0x03;
    /// `12 05 06 <time u32>`, no reply.
    pub const DELETE: u8 = 0x06;
    /// `12 02 08 01` starts the live stream, `12 02 08 02` stops it; no reply.
    pub const LIVE: u8 = 0x08;
    /// Event `12 01 09`: a page was stored with the tablet's button.
    pub const PAGE_STORED: u8 = 0x09;
}

/// First payload byte of a download request. Always `01` in Mi Home's captures; its meaning
/// is not known (research note §3), so it is a constant, never a parameter.
pub const DOWNLOAD_FLAG: u8 = 0x01;
/// Payload of `12 02 08` that starts the live stream.
pub const LIVE_START: u8 = 0x01;
/// Payload of `12 02 08` that stops the live stream.
pub const LIVE_STOP: u8 = 0x02;

/// Width of the blackboard's drawing area in its units, taken as the notes' units of 10 µm
/// (research note §3, "Live samples": one unit is close to 0.01 mm, inferred). **Measured
/// extents rounded, to be calibrated**: the largest x seen is 20 401.
pub const BOARD_WIDTH: u32 = 20_500;
/// Height of the drawing area (portrait, origin top left, y downwards). **Measured extents
/// rounded, to be calibrated**: the largest y seen is 27 067.
pub const BOARD_HEIGHT: u32 = 27_200;
/// Highest pressure of a touching record (2048 levels, like the Slate).
pub const BOARD_MAX_PRESSURE: u16 = 2047;
/// The ATT MTU the tablet asks for (research note §3).
pub const BOARD_ATT_MTU: usize = 247;
/// Largest payload of one notification with that MTU: page bytes arrive in chunks of at most
/// this many bytes (research note §3, "Import of the stored pages").
pub const MAX_CHUNK: usize = BOARD_ATT_MTU - 3;
/// Most records in one live notification (research note §3: one to six).
pub const MAX_LIVE_RECORDS: usize = 6;

/// One frame of the vendor channel: `group, length, code, payload`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoardFrame {
    pub group: u8,
    pub code: u8,
    pub payload: Vec<u8>,
}

/// Failure while encoding or decoding a [`BoardFrame`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoardFrameError {
    /// Fewer than the three bytes of group, length and code.
    #[error("frame of {len} bytes is too short (group, length and code need 3)")]
    TooShort { len: usize },
    /// The length byte does not match the bytes present.
    #[error("frame length byte says {declared} bytes after it but {actual} are present")]
    LengthMismatch { declared: usize, actual: usize },
    /// A length byte of 0 leaves no room for the code.
    #[error("frame length 0: no code")]
    NoCode,
    /// More payload than a length byte can describe.
    #[error("{len} payload bytes do not fit in a frame (at most 254)")]
    TooLong { len: usize },
}

impl BoardFrame {
    pub fn new(group: u8, code: u8, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            group,
            code,
            payload: payload.into(),
        }
    }

    /// `group, 1 + payload length, code, payload`.
    pub fn encode(&self) -> Result<Vec<u8>, BoardFrameError> {
        let length =
            u8::try_from(self.payload.len() + 1).map_err(|_| BoardFrameError::TooLong {
                len: self.payload.len(),
            })?;
        let mut bytes = Vec::with_capacity(3 + self.payload.len());
        bytes.extend_from_slice(&[self.group, length, self.code]);
        bytes.extend_from_slice(&self.payload);
        Ok(bytes)
    }

    /// Parses one whole notification; the length byte must match exactly.
    pub fn decode(bytes: &[u8]) -> Result<Self, BoardFrameError> {
        let [group, length, rest @ ..] = bytes else {
            return Err(BoardFrameError::TooShort { len: bytes.len() });
        };
        let declared = usize::from(*length);
        if declared == 0 {
            return Err(BoardFrameError::NoCode);
        }
        if rest.len() != declared {
            return Err(BoardFrameError::LengthMismatch {
                declared,
                actual: rest.len(),
            });
        }
        let [code, payload @ ..] = rest else {
            return Err(BoardFrameError::TooShort { len: bytes.len() });
        };
        Ok(Self::new(*group, *code, payload))
    }
}

/// Hex dump of the encoded frame, e.g. `12 01 09`.
impl fmt::Display for BoardFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.encode() {
            Ok(bytes) => f.write_str(&crate::hex::encode(&bytes)),
            Err(_) => write!(
                f,
                "{:02x} ?? {:02x} {}",
                self.group,
                self.code,
                crate::hex::encode(&self.payload)
            ),
        }
    }
}

/// Failure while decoding a request or a reply.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoardMessageError {
    #[error(transparent)]
    Frame(#[from] BoardFrameError),
    /// A request the host never sends (the simulator answers nothing to it).
    #[error("unknown request {group:02x} {code:02x}")]
    UnknownRequest { group: u8, code: u8 },
    /// A known group and code with a payload of the wrong size or value.
    #[error("frame {group:02x} {code:02x}: {reason}")]
    Payload { group: u8, code: u8, reason: String },
}

/// What the host may send: the frames of Mi Home's captures, and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoardRequest {
    /// `11 01 01`.
    Battery,
    /// `11 01 02`.
    Memory,
    /// `11 05 07 <unix u32>`: sets the tablet's clock (Unix seconds, UTC).
    SetTime { unix: u32 },
    /// `12 01 04`.
    PageCount,
    /// `12 01 05`: describe the oldest stored page.
    OldestPage,
    /// `12 06 03 01 <time u32>`: download the page stored at `time`.
    Download { time: u32 },
    /// `12 05 06 <time u32>`: delete the page stored at `time`.
    Delete { time: u32 },
    /// `12 02 08 01`.
    StartLive,
    /// `12 02 08 02`.
    StopLive,
}

impl BoardRequest {
    /// The frame of this request.
    pub fn to_frame(&self) -> BoardFrame {
        match *self {
            Self::Battery => BoardFrame::new(GROUP_DEVICE, code::BATTERY, []),
            Self::Memory => BoardFrame::new(GROUP_DEVICE, code::MEMORY, []),
            Self::SetTime { unix } => {
                BoardFrame::new(GROUP_DEVICE, code::SET_TIME, unix.to_be_bytes())
            }
            Self::PageCount => BoardFrame::new(GROUP_PAGES, code::PAGE_COUNT, []),
            Self::OldestPage => BoardFrame::new(GROUP_PAGES, code::OLDEST_PAGE, []),
            Self::Download { time } => {
                let mut payload = vec![DOWNLOAD_FLAG];
                payload.extend_from_slice(&time.to_be_bytes());
                BoardFrame::new(GROUP_PAGES, code::DOWNLOAD, payload)
            }
            Self::Delete { time } => BoardFrame::new(GROUP_PAGES, code::DELETE, time.to_be_bytes()),
            Self::StartLive => BoardFrame::new(GROUP_PAGES, code::LIVE, [LIVE_START]),
            Self::StopLive => BoardFrame::new(GROUP_PAGES, code::LIVE, [LIVE_STOP]),
        }
    }

    /// The bytes written to the tablet. Infallible: every payload is at most 5 bytes.
    pub fn encode(&self) -> Vec<u8> {
        let frame = self.to_frame();
        let mut bytes = Vec::with_capacity(3 + frame.payload.len());
        // At most 6, always fits the length byte.
        let length = u8::try_from(frame.payload.len() + 1).unwrap_or(u8::MAX);
        bytes.extend_from_slice(&[frame.group, length, frame.code]);
        bytes.extend_from_slice(&frame.payload);
        bytes
    }

    /// Reads a request (the simulator's side).
    pub fn from_frame(frame: &BoardFrame) -> Result<Self, BoardMessageError> {
        let payload = frame.payload.as_slice();
        let wrong = |reason: &str| BoardMessageError::Payload {
            group: frame.group,
            code: frame.code,
            reason: reason.to_owned(),
        };
        let empty = |request: Self| {
            if payload.is_empty() {
                Ok(request)
            } else {
                Err(wrong("expected no payload"))
            }
        };
        match (frame.group, frame.code) {
            (GROUP_DEVICE, code::BATTERY) => empty(Self::Battery),
            (GROUP_DEVICE, code::MEMORY) => empty(Self::Memory),
            (GROUP_DEVICE, code::SET_TIME) => be_u32(payload)
                .map(|unix| Self::SetTime { unix })
                .ok_or_else(|| wrong("expected a 4-byte time")),
            (GROUP_PAGES, code::PAGE_COUNT) => empty(Self::PageCount),
            (GROUP_PAGES, code::OLDEST_PAGE) => empty(Self::OldestPage),
            (GROUP_PAGES, code::DOWNLOAD) => match payload {
                [DOWNLOAD_FLAG, time @ ..] => be_u32(time)
                    .map(|time| Self::Download { time })
                    .ok_or_else(|| wrong("expected 01 and a 4-byte time")),
                _ => Err(wrong("expected 01 and a 4-byte time")),
            },
            (GROUP_PAGES, code::DELETE) => be_u32(payload)
                .map(|time| Self::Delete { time })
                .ok_or_else(|| wrong("expected a 4-byte time")),
            (GROUP_PAGES, code::LIVE) => match payload {
                [LIVE_START] => Ok(Self::StartLive),
                [LIVE_STOP] => Ok(Self::StopLive),
                _ => Err(wrong("expected 01 or 02")),
            },
            (group, code) => Err(BoardMessageError::UnknownRequest { group, code }),
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BoardMessageError> {
        Self::from_frame(&BoardFrame::decode(bytes)?)
    }
}

impl fmt::Display for BoardRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Battery => f.write_str("BOARD GET_BATTERY"),
            Self::Memory => f.write_str("BOARD GET_MEMORY"),
            Self::SetTime { unix } => write!(f, "BOARD SET_TIME {}", utc(*unix)),
            Self::PageCount => f.write_str("BOARD PAGE_COUNT"),
            Self::OldestPage => f.write_str("BOARD DESCRIBE_OLDEST_PAGE"),
            Self::Download { time } => write!(f, "BOARD DOWNLOAD page of {}", utc(*time)),
            Self::Delete { time } => write!(f, "BOARD DELETE page of {}", utc(*time)),
            Self::StartLive => f.write_str("BOARD START_LIVE"),
            Self::StopLive => f.write_str("BOARD STOP_LIVE"),
        }
    }
}

/// The oldest stored page as the tablet describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageInfo {
    /// When the page was stored, Unix seconds on the tablet's clock. It identifies the page in
    /// the download and delete requests.
    pub time: u32,
    /// Size of the page in bytes (a multiple of 7 in every capture).
    pub size: u32,
}

/// What the tablet sends on the reply characteristic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardReply {
    /// `11 02 01 <percent>` (per cent, inferred: 100 and 90 were seen).
    Battery { percent: u8 },
    /// `11 09 02 <total u32> <free u32>`, bytes.
    Memory { total: u32, free: u32 },
    /// `12 07 04 <count u16> <bytes u32>`: pages stored and their total size.
    PageCount { count: u16, bytes: u32 },
    /// `12 09 05 <time u32> <size u32>`, or the bare `12 01 05` of an empty tablet (`None`).
    OldestPage(Option<PageInfo>),
    /// Event `12 01 09`: a page was stored with the tablet's button.
    PageStored,
    /// Any other group or code, kept verbatim (logged and ignored by the host).
    Unknown(BoardFrame),
}

impl BoardReply {
    pub fn to_frame(&self) -> BoardFrame {
        match self {
            Self::Battery { percent } => BoardFrame::new(GROUP_DEVICE, code::BATTERY, [*percent]),
            Self::Memory { total, free } => {
                let mut payload = total.to_be_bytes().to_vec();
                payload.extend_from_slice(&free.to_be_bytes());
                BoardFrame::new(GROUP_DEVICE, code::MEMORY, payload)
            }
            Self::PageCount { count, bytes } => {
                let mut payload = count.to_be_bytes().to_vec();
                payload.extend_from_slice(&bytes.to_be_bytes());
                BoardFrame::new(GROUP_PAGES, code::PAGE_COUNT, payload)
            }
            Self::OldestPage(None) => BoardFrame::new(GROUP_PAGES, code::OLDEST_PAGE, []),
            Self::OldestPage(Some(info)) => {
                let mut payload = info.time.to_be_bytes().to_vec();
                payload.extend_from_slice(&info.size.to_be_bytes());
                BoardFrame::new(GROUP_PAGES, code::OLDEST_PAGE, payload)
            }
            Self::PageStored => BoardFrame::new(GROUP_PAGES, code::PAGE_STORED, []),
            Self::Unknown(frame) => frame.clone(),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, BoardFrameError> {
        self.to_frame().encode()
    }

    /// Reads a reply (the host's side). Unknown codes are [`BoardReply::Unknown`].
    pub fn from_frame(frame: BoardFrame) -> Result<Self, BoardMessageError> {
        let payload = frame.payload.as_slice();
        let wrong = |reason: &str| BoardMessageError::Payload {
            group: frame.group,
            code: frame.code,
            reason: reason.to_owned(),
        };
        match (frame.group, frame.code) {
            (GROUP_DEVICE, code::BATTERY) => match payload {
                [percent] => Ok(Self::Battery { percent: *percent }),
                _ => Err(wrong("expected 1 byte")),
            },
            (GROUP_DEVICE, code::MEMORY) => match payload {
                [t0, t1, t2, t3, f0, f1, f2, f3] => Ok(Self::Memory {
                    total: u32::from_be_bytes([*t0, *t1, *t2, *t3]),
                    free: u32::from_be_bytes([*f0, *f1, *f2, *f3]),
                }),
                _ => Err(wrong("expected 8 bytes")),
            },
            (GROUP_PAGES, code::PAGE_COUNT) => match payload {
                [c0, c1, b0, b1, b2, b3] => Ok(Self::PageCount {
                    count: u16::from_be_bytes([*c0, *c1]),
                    bytes: u32::from_be_bytes([*b0, *b1, *b2, *b3]),
                }),
                _ => Err(wrong("expected 6 bytes")),
            },
            (GROUP_PAGES, code::OLDEST_PAGE) => match payload {
                [] => Ok(Self::OldestPage(None)),
                [t0, t1, t2, t3, s0, s1, s2, s3] => Ok(Self::OldestPage(Some(PageInfo {
                    time: u32::from_be_bytes([*t0, *t1, *t2, *t3]),
                    size: u32::from_be_bytes([*s0, *s1, *s2, *s3]),
                }))),
                _ => Err(wrong("expected nothing or 8 bytes")),
            },
            (GROUP_PAGES, code::PAGE_STORED) if payload.is_empty() => Ok(Self::PageStored),
            (GROUP_PAGES, code::PAGE_STORED) => Err(wrong("expected no payload")),
            _ => Ok(Self::Unknown(frame)),
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BoardMessageError> {
        Self::from_frame(BoardFrame::decode(bytes)?)
    }
}

impl fmt::Display for BoardReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Battery { percent } => write!(f, "BOARD BATTERY {percent}%"),
            Self::Memory { total, free } => {
                write!(f, "BOARD MEMORY {free} of {total} bytes free")
            }
            Self::PageCount { count, bytes } => {
                write!(f, "BOARD PAGE_COUNT {count} pages, {bytes} bytes")
            }
            Self::OldestPage(None) => f.write_str("BOARD OLDEST_PAGE none (no page stored)"),
            Self::OldestPage(Some(info)) => write!(
                f,
                "BOARD OLDEST_PAGE stored {}, {} bytes",
                utc(info.time),
                info.size
            ),
            Self::PageStored => f.write_str("BOARD PAGE_STORED (the tablet's button)"),
            Self::Unknown(frame) => write!(f, "BOARD unknown frame {frame}"),
        }
    }
}

/// Describes a frame written by the host, for the diagnostic log. Never fails.
pub fn describe_request(bytes: &[u8]) -> String {
    match BoardRequest::decode(bytes) {
        Ok(request) => request.to_string(),
        Err(error) => format!("BOARD {error}"),
    }
}

/// Describes a frame of the reply characteristic, for the diagnostic log. Never fails.
pub fn describe_reply(bytes: &[u8]) -> String {
    match BoardReply::decode(bytes) {
        Ok(reply) => reply.to_string(),
        Err(error) => format!("BOARD malformed reply: {error}"),
    }
}

/// Describes a notification of the stream characteristic (`6e400004`): live records, or a
/// chunk of a page during a download (chunks cut records, so a chunk that is not whole
/// records is only counted). Never fails.
pub fn describe_stream(bytes: &[u8]) -> String {
    let plural = if bytes.len() == 1 { "" } else { "s" };
    if bytes.is_empty() || !bytes.len().is_multiple_of(record::RECORD_LEN) {
        return format!("BOARD_DATA {} byte{plural}", bytes.len());
    }
    let decoded = decode_records(bytes);
    let count = |status: RecordStatus| {
        decoded
            .records
            .iter()
            .filter(|r| r.status == status)
            .count()
    };
    let mut text = format!(
        "BOARD_DATA {} byte{plural}: {} records",
        bytes.len(),
        decoded.records.len()
    );
    for (name, n) in [
        ("touch", count(RecordStatus::Touch)),
        ("hover", count(RecordStatus::Hover)),
        ("out of range", count(RecordStatus::Out)),
    ] {
        if n > 0 {
            text.push_str(&format!(", {n} {name}"));
        }
    }
    let unknown = decoded
        .records
        .iter()
        .filter(|r| matches!(r.status, RecordStatus::Unknown(_)))
        .count();
    if unknown > 0 {
        text.push_str(&format!(", {unknown} unknown"));
    }
    text
}

/// Describes a whole downloaded page, e.g. `BOARD PAGE 3 strokes, 412 points, 500 records`.
pub fn describe_page(bytes: &[u8]) -> String {
    let page = decode_page(bytes);
    let mut text = format!(
        "BOARD PAGE {} strokes, {} points, {} records",
        page.strokes.len(),
        page.point_count(),
        page.records
    );
    if page.unknown_records > 0 {
        text.push_str(&format!(", {} unknown records", page.unknown_records));
    }
    if page.trailing_bytes > 0 {
        text.push_str(&format!(
            ", {} bytes after the last record",
            page.trailing_bytes
        ));
    }
    text
}

/// `4-byte` big-endian integer, `None` for any other length.
fn be_u32(bytes: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// `2026-05-10 03:48:16 UTC` for Unix seconds (civil-from-days; no time zone database).
pub fn utc(unix: u32) -> String {
    let seconds = u64::from(unix);
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// Year, month and day of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).unwrap()
    }

    /// Every request of the research note's table, with synthetic values.
    const REQUESTS: [(&str, BoardRequest); 9] = [
        ("11 01 01", BoardRequest::Battery),
        ("11 01 02", BoardRequest::Memory),
        (
            "11 05 07 6a 00 00 00",
            BoardRequest::SetTime { unix: 0x6a00_0000 },
        ),
        ("12 01 04", BoardRequest::PageCount),
        ("12 01 05", BoardRequest::OldestPage),
        (
            "12 06 03 01 68 00 00 01",
            BoardRequest::Download { time: 0x6800_0001 },
        ),
        (
            "12 05 06 68 00 00 01",
            BoardRequest::Delete { time: 0x6800_0001 },
        ),
        ("12 02 08 01", BoardRequest::StartLive),
        ("12 02 08 02", BoardRequest::StopLive),
    ];

    #[test]
    fn every_request_encodes_and_decodes_identically() {
        for (text, request) in REQUESTS {
            assert_eq!(request.encode(), bytes(text), "{request:?}");
            assert_eq!(request.to_frame().encode().unwrap(), bytes(text));
            assert_eq!(BoardRequest::decode(&bytes(text)), Ok(request), "{text}");
        }
    }

    #[test]
    fn replies_encode_and_decode_identically() {
        let replies = [
            ("11 02 01 64", BoardReply::Battery { percent: 100 }),
            (
                "11 09 02 07 90 7e 00 07 8e ba 00",
                BoardReply::Memory {
                    total: 126_909_952,
                    free: 126_794_240,
                },
            ),
            (
                "12 07 04 00 09 00 01 cd 5f",
                BoardReply::PageCount {
                    count: 9,
                    bytes: 118_111,
                },
            ),
            (
                "12 07 04 00 00 00 00 00 00",
                BoardReply::PageCount { count: 0, bytes: 0 },
            ),
            (
                "12 09 05 68 00 00 01 00 00 53 20",
                BoardReply::OldestPage(Some(PageInfo {
                    time: 0x6800_0001,
                    size: 21_280,
                })),
            ),
            ("12 01 05", BoardReply::OldestPage(None)),
            ("12 01 09", BoardReply::PageStored),
        ];
        for (text, reply) in replies {
            assert_eq!(
                BoardReply::decode(&bytes(text)),
                Ok(reply.clone()),
                "{text}"
            );
            assert_eq!(reply.encode().unwrap(), bytes(text), "{reply:?}");
        }
    }

    #[test]
    fn unknown_replies_are_kept_and_requests_refused() {
        let unknown = bytes("12 03 0a 01 02");
        let reply = BoardReply::decode(&unknown).unwrap();
        assert_eq!(
            reply,
            BoardReply::Unknown(BoardFrame::new(0x12, 0x0a, [1, 2]))
        );
        assert_eq!(reply.encode().unwrap(), unknown);
        assert_eq!(
            BoardRequest::decode(&bytes("12 01 0a")),
            Err(BoardMessageError::UnknownRequest {
                group: 0x12,
                code: 0x0a
            })
        );
        assert_eq!(
            BoardRequest::decode(&bytes("13 01 01")),
            Err(BoardMessageError::UnknownRequest {
                group: 0x13,
                code: 0x01
            })
        );
    }

    #[test]
    fn malformed_frames_are_errors_never_panics() {
        for text in [
            "",
            "11",
            "11 01",
            "11 00",
            "11 02 01",
            "11 01 01 00",
            "11 03 01 64",
        ] {
            let data = bytes(text);
            assert!(BoardReply::decode(&data).is_err(), "{text}");
            assert!(BoardRequest::decode(&data).is_err(), "{text}");
        }
        // A known code with a payload of the wrong size.
        assert!(matches!(
            BoardReply::decode(&bytes("11 03 02 00 01")),
            Err(BoardMessageError::Payload { .. })
        ));
        assert!(matches!(
            BoardReply::decode(&bytes("12 02 09 00")),
            Err(BoardMessageError::Payload { .. })
        ));
        assert!(matches!(
            BoardRequest::decode(&bytes("12 06 03 02 00 00 00 01")),
            Err(BoardMessageError::Payload { .. })
        ));
        assert!(matches!(
            BoardRequest::decode(&bytes("12 02 08 03")),
            Err(BoardMessageError::Payload { .. })
        ));
        // Every prefix of every frame decodes or fails, never panics.
        for (text, _) in REQUESTS {
            let data = bytes(text);
            for end in 0..=data.len() {
                let _ = BoardRequest::decode(&data[..end]);
                let _ = BoardReply::decode(&data[..end]);
            }
        }
        assert_eq!(
            BoardFrame::new(0x12, 0x01, vec![0; 255]).encode(),
            Err(BoardFrameError::TooLong { len: 255 })
        );
    }

    #[test]
    fn descriptions_name_every_frame() {
        assert_eq!(
            describe_request(&bytes("11 05 07 6a 00 00 00")),
            "BOARD SET_TIME 2026-05-10 03:48:16 UTC"
        );
        assert_eq!(
            describe_request(&bytes("12 06 03 01 68 00 00 01")),
            "BOARD DOWNLOAD page of 2025-04-16 19:07:45 UTC"
        );
        assert_eq!(
            describe_reply(&bytes("12 01 05")),
            "BOARD OLDEST_PAGE none (no page stored)"
        );
        assert_eq!(describe_reply(&bytes("11 02 01 5a")), "BOARD BATTERY 90%");
        assert!(describe_reply(&bytes("12 01 0b")).starts_with("BOARD unknown frame 12 01 0b"));
        assert!(describe_reply(&bytes("12 05")).starts_with("BOARD malformed reply"));
        assert!(describe_request(&bytes("12 01 0b")).starts_with("BOARD unknown request"));
        assert_eq!(
            describe_stream(&bytes("a1 10 27 20 4e 00 04 a0 11 27 21 4e 00 00")),
            "BOARD_DATA 14 bytes: 2 records, 1 touch, 1 hover"
        );
        assert_eq!(describe_stream(&[0xa1; 10]), "BOARD_DATA 10 bytes");
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(951_825_600), "2000-02-29 12:00:00 UTC");
    }
}
