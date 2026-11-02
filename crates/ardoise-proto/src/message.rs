//! Commands (host to tablet) and replies (tablet to host) on the Nordic UART characteristics,
//! as used by the Bamboo Slate (protocol.md §2.4, §3, §4, §5, §7.1).
//!
//! Both enums are encoded and decoded in both directions: the host encodes [`Request`]s and
//! decodes [`Reply`]s, the simulator does the opposite. Unknown opcodes decode to `Unknown`,
//! known opcodes with unexpected arguments are a [`MessageError`].
//!
//! The generic status reply `b3 01 <code>` ([`Reply::Status`]) is a legal answer to any
//! request: a tablet in the wrong state answers `b3 01 02` instead of the expected reply.

use std::fmt;

use crate::frame::{Frame, FrameError};
use crate::hex;
use crate::time::{BcdTime, TimeError};

/// Opcodes of the Slate messages.
pub mod opcode {
    /// Request: authenticate with the host id (`e6 06 <id>`).
    pub const CONNECT: u8 = 0xe6;
    /// Request: start registration with a new host id (`e7 06 <id>`), no immediate reply.
    pub const REGISTER: u8 = 0xe7;
    /// Request: get (`bb 01 00`) or set (`bb n <name> 0a`) the tablet name.
    pub const NAME: u8 = 0xbb;
    /// Request: get (`b6 01 00`) or set (`b6 06 <bcd>`) the clock.
    pub const TIME: u8 = 0xb6;
    /// Request: firmware version part (`b7 01 <part>`).
    pub const FIRMWARE: u8 = 0xb7;
    /// Request: battery state (`b9 01 00`).
    pub const BATTERY: u8 = 0xb9;
    /// Request: sensor dimension (`ea 02 <which> 00`).
    pub const DIMENSION: u8 = 0xea;
    /// Request: send page data on the offline characteristic (`ec 06 06 00 00 00 00 00`).
    pub const FILE_TRANSFER_REPORTING_TYPE: u8 = 0xec;
    /// Request: switch mode (`b1 01 <mode>`).
    pub const SET_MODE: u8 = 0xb1;
    /// Request: number of stored pages (`c1 01 00`).
    pub const FILES_COUNT: u8 = 0xc1;
    /// Request: size and time of the oldest page (`cc 01 00`).
    pub const FILE_INFO: u8 = 0xcc;
    /// Request: download the oldest page (`c3 01 00`).
    pub const DOWNLOAD_OLDEST: u8 = 0xc3;
    /// Request: delete the oldest page (`ca 01 00`).
    pub const DELETE_OLDEST: u8 = 0xca;

    /// Reply: generic status (`b3 01 <code>`).
    pub const STATUS: u8 = 0xb3;
    /// Reply: the button was pressed during registration (`e4 01 00`).
    pub const REGISTER_BUTTON_PRESSED: u8 = 0xe4;
    /// Reply: a chunk of the tablet name (`bc n <bytes>`).
    pub const NAME_CHUNK: u8 = 0xbc;
    /// Reply: clock value (`bd 06 <bcd>`).
    pub const TIME_VALUE: u8 = 0xbd;
    /// Reply: firmware version part (`b8 n <part> <digits>`).
    pub const FIRMWARE_VALUE: u8 = 0xb8;
    /// Reply: battery state (`ba 02 <percent> <charging>`).
    pub const BATTERY_VALUE: u8 = 0xba;
    /// Reply: sensor dimension (`eb 06 <which> 00 <u32>`).
    pub const DIMENSION_VALUE: u8 = 0xeb;
    /// Reply: number of stored pages (`c2 02 <u16>`).
    pub const FILES_COUNT_VALUE: u8 = 0xc2;
    /// Reply: size and time of the oldest page (`cf 0a <u32> <bcd>`).
    pub const FILE_INFO_VALUE: u8 = 0xcf;
    /// Reply: download start (`c8 n be ...`) or end (`c8 05 ed <crc32>`).
    pub const DOWNLOAD: u8 = 0xc8;
    /// Unasked frame seen once from a real Slate A4 during live mode (`cb 01 00`, protocol.md
    /// §12). Meaning unknown: it decodes as [`Reply::Unknown`](super::Reply::Unknown), and
    /// [`Reply::is_unexplained`](super::Reply::is_unexplained) tells it apart.
    pub const UNEXPLAINED_CB: u8 = 0xcb;
}

/// First argument of a `c8` reply that starts a download.
const DOWNLOAD_START_MARKER: u8 = 0xbe;
/// First argument of a `c8` reply that ends a download.
const DOWNLOAD_END_MARKER: u8 = 0xed;
/// Arguments of SET_FILE_TRANSFER_REPORTING_TYPE, identical in every known trace.
const FILE_TRANSFER_REPORTING_ARGS: [u8; 6] = [0x06, 0x00, 0x00, 0x00, 0x00, 0x00];
/// Line feed that ends a name.
const NAME_END: u8 = 0x0a;
/// Most name bytes per `bc` frame so that the frame fits a 20-byte notification.
pub const NAME_CHUNK_MAX: usize = crate::MAX_NOTIFICATION_PAYLOAD - 2;

/// The 6-byte host identifier ("UUID") chosen at registration (protocol.md §3.1).
pub type HostId = [u8; 6];

/// Failure while decoding a message from a frame.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageError {
    /// The bytes are not a valid frame.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// A known opcode with the wrong number of argument bytes.
    #[error("opcode {opcode:02x}: {actual} argument bytes, expected {expected}")]
    Length {
        /// Opcode of the frame.
        opcode: u8,
        /// Number of argument bytes received.
        actual: usize,
        /// Accepted lengths, in words.
        expected: &'static str,
    },
    /// A known opcode with argument values outside what the protocol defines.
    #[error("opcode {opcode:02x}: {reason}")]
    Argument {
        /// Opcode of the frame.
        opcode: u8,
        /// What is wrong.
        reason: String,
    },
    /// A known opcode carrying an invalid BCD time.
    #[error("opcode {opcode:02x}: {source}")]
    Time {
        /// Opcode of the frame.
        opcode: u8,
        /// What is wrong with the time.
        source: TimeError,
    },
}

/// Code of a generic status reply `b3 01 <code>` (protocol.md §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusCode {
    /// `00`: the request succeeded.
    Success,
    /// `01`: general error (also seen when the tablet is in the wrong state).
    GeneralError,
    /// `02`: the tablet is not in a state where the request makes sense.
    InvalidState,
    /// `03`: the parameter cannot be written.
    ReadOnlyParam,
    /// `04`: the request is not supported.
    CommandNotSupported,
    /// `07`: wrong host id; the tablet is registered with another app.
    AuthorizationError,
    /// Any other code.
    Other(u8),
}

impl StatusCode {
    /// Interprets a status byte.
    pub fn from_byte(code: u8) -> Self {
        match code {
            0x00 => Self::Success,
            0x01 => Self::GeneralError,
            0x02 => Self::InvalidState,
            0x03 => Self::ReadOnlyParam,
            0x04 => Self::CommandNotSupported,
            0x07 => Self::AuthorizationError,
            other => Self::Other(other),
        }
    }

    /// The status byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Success => 0x00,
            Self::GeneralError => 0x01,
            Self::InvalidState => 0x02,
            Self::ReadOnlyParam => 0x03,
            Self::CommandNotSupported => 0x04,
            Self::AuthorizationError => 0x07,
            Self::Other(code) => code,
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => f.write_str("success"),
            Self::GeneralError => f.write_str("general error"),
            Self::InvalidState => f.write_str("invalid state"),
            Self::ReadOnlyParam => f.write_str("read-only parameter"),
            Self::CommandNotSupported => f.write_str("command not supported"),
            Self::AuthorizationError => f.write_str("authorization error"),
            Self::Other(code) => write!(f, "unknown code {code:02x}"),
        }
    }
}

/// Tablet mode set by SET_MODE (protocol.md §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// `00`: pen samples are streamed on the live characteristic.
    Live,
    /// `01`: normal mode, pages are stored and can be downloaded.
    Paper,
    /// `02`: live mode without reporting; used to leave live mode.
    Idle,
}

impl Mode {
    /// Interprets a mode byte.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Live),
            0x01 => Some(Self::Paper),
            0x02 => Some(Self::Idle),
            _ => None,
        }
    }

    /// The mode byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Live => 0x00,
            Self::Paper => 0x01,
            Self::Idle => 0x02,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Live => "live",
            Self::Paper => "paper",
            Self::Idle => "idle",
        })
    }
}

/// Sensor dimension asked with GET_DIMENSION (protocol.md §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    /// `03`: width (x range), in units of 10 µm.
    Width,
    /// `04`: height (y range), in units of 10 µm.
    Height,
}

impl Dimension {
    /// Interprets a dimension selector byte.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x03 => Some(Self::Width),
            0x04 => Some(Self::Height),
            _ => None,
        }
    }

    /// The selector byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Width => 0x03,
            Self::Height => 0x04,
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Width => "width",
            Self::Height => "height",
        })
    }
}

/// One of the two halves of the firmware version (protocol.md §4.2). The full version is
/// `high-low`, e.g. `B160805A-C170704A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirmwarePart {
    /// `00`: first half.
    High,
    /// `01`: second half.
    Low,
}

impl FirmwarePart {
    /// Interprets a part byte.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::High),
            0x01 => Some(Self::Low),
            _ => None,
        }
    }

    /// The part byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::High => 0x00,
            Self::Low => 0x01,
        }
    }
}

impl fmt::Display for FirmwarePart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::High => "high",
            Self::Low => "low",
        })
    }
}

/// A command sent by the host on the Nordic UART TX characteristic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `e6 06 <id>`: authenticate. Reply `b3 01 00`, or `b3 01 07` for an unknown id.
    Connect {
        /// Host id given at registration.
        uuid: HostId,
    },
    /// `e7 06 <id>`: registration with a new id. No reply until the user presses the button
    /// (`e4 01 00`).
    RegisterPressButton {
        /// New host id.
        uuid: HostId,
    },
    /// `bb 01 00`: read the name. Reply: one or more `bc` chunks ending with `0a`.
    GetName,
    /// `bb n <name> 0a`: rename the tablet. Reply `b3 01 00`.
    SetName(String),
    /// `b6 01 00`: read the clock. Reply `bd 06 <bcd>`.
    GetTime,
    /// `b6 06 <bcd>`: set the clock (UTC). Reply `b3 01 00`.
    SetTime(BcdTime),
    /// `b7 01 <part>`: read half of the firmware version. Reply `b8 n <part> <digits>`.
    GetFirmware {
        /// Which half.
        part: FirmwarePart,
    },
    /// `b9 01 00`: read the battery. Reply `ba 02 <percent> <charging>`.
    GetBattery,
    /// `ea 02 <which> 00`: read a sensor dimension. Reply `eb 06 <which> 00 <u32>`.
    GetDimension(Dimension),
    /// `ec 06 06 00 00 00 00 00`: page data goes to the offline characteristic. Reply
    /// `b3 01 00`.
    SetFileTransferReportingType,
    /// `b1 01 <mode>`: switch mode. Reply `b3 01 00`.
    SetMode(Mode),
    /// `c1 01 00`: number of stored pages. Reply `c2 02 <u16>`.
    AvailableFilesCount,
    /// `cc 01 00`: size and time of the oldest page. Reply `cf 0a <u32> <bcd>`.
    GetFileInfo,
    /// `c3 01 00`: download the oldest page. Reply `c8 n be ...`, then the data on the offline
    /// characteristic, then `c8 05 ed <crc32>`.
    DownloadOldestFile,
    /// `ca 01 00`: delete the oldest page. Reply `b3 01 00`.
    DeleteOldestFile,
    /// Any other opcode, kept verbatim.
    Unknown(Frame),
}

impl Request {
    /// The frame to send.
    pub fn to_frame(&self) -> Frame {
        match self {
            Self::Connect { uuid } => Frame::new(opcode::CONNECT, uuid.as_slice()),
            Self::RegisterPressButton { uuid } => Frame::new(opcode::REGISTER, uuid.as_slice()),
            Self::GetName => empty_request(opcode::NAME),
            Self::SetName(name) => {
                let mut args = name.as_bytes().to_vec();
                args.push(NAME_END);
                Frame::new(opcode::NAME, args)
            }
            Self::GetTime => empty_request(opcode::TIME),
            Self::SetTime(time) => Frame::new(opcode::TIME, time.to_bcd()),
            Self::GetFirmware { part } => Frame::new(opcode::FIRMWARE, [part.to_byte()]),
            Self::GetBattery => empty_request(opcode::BATTERY),
            Self::GetDimension(which) => Frame::new(opcode::DIMENSION, [which.to_byte(), 0x00]),
            Self::SetFileTransferReportingType => Frame::new(
                opcode::FILE_TRANSFER_REPORTING_TYPE,
                FILE_TRANSFER_REPORTING_ARGS,
            ),
            Self::SetMode(mode) => Frame::new(opcode::SET_MODE, [mode.to_byte()]),
            Self::AvailableFilesCount => empty_request(opcode::FILES_COUNT),
            Self::GetFileInfo => empty_request(opcode::FILE_INFO),
            Self::DownloadOldestFile => empty_request(opcode::DOWNLOAD_OLDEST),
            Self::DeleteOldestFile => empty_request(opcode::DELETE_OLDEST),
            Self::Unknown(frame) => frame.clone(),
        }
    }

    /// Interprets a frame received by the tablet (used by the simulator).
    pub fn from_frame(frame: Frame) -> Result<Self, MessageError> {
        let op = frame.opcode;
        let args = frame.args.as_slice();
        let request = match op {
            opcode::CONNECT => Self::Connect {
                uuid: host_id(op, args)?,
            },
            opcode::REGISTER => Self::RegisterPressButton {
                uuid: host_id(op, args)?,
            },
            opcode::NAME => match args {
                [0x00] => Self::GetName,
                [name @ .., NAME_END] => Self::SetName(String::from_utf8_lossy(name).into_owned()),
                [] => return Err(length(op, 0, "at least 1")),
                _ => return Err(argument(op, "a new name must end with 0a")),
            },
            opcode::TIME => match args {
                [0x00] => Self::GetTime,
                [_] => return Err(argument(op, "GET_TIME takes the argument 00")),
                bcd => Self::SetTime(time_arg(op, bcd, "1 or 6")?),
            },
            opcode::FIRMWARE => {
                let byte = single_arg(op, args)?;
                let part = FirmwarePart::from_byte(byte)
                    .ok_or_else(|| argument(op, format!("unknown firmware part {byte:02x}")))?;
                Self::GetFirmware { part }
            }
            opcode::BATTERY => {
                no_arg(op, args)?;
                Self::GetBattery
            }
            opcode::DIMENSION => match args {
                [byte, 0x00] => Self::GetDimension(dimension(op, *byte)?),
                [_, _] => return Err(argument(op, "second argument must be 00")),
                _ => return Err(length(op, args.len(), "2")),
            },
            opcode::FILE_TRANSFER_REPORTING_TYPE => {
                if args.len() != FILE_TRANSFER_REPORTING_ARGS.len() {
                    return Err(length(op, args.len(), "6"));
                }
                if args != FILE_TRANSFER_REPORTING_ARGS {
                    return Err(argument(op, "arguments must be 06 00 00 00 00 00"));
                }
                Self::SetFileTransferReportingType
            }
            opcode::SET_MODE => {
                let byte = single_arg(op, args)?;
                let mode = Mode::from_byte(byte)
                    .ok_or_else(|| argument(op, format!("unknown mode {byte:02x}")))?;
                Self::SetMode(mode)
            }
            opcode::FILES_COUNT => {
                no_arg(op, args)?;
                Self::AvailableFilesCount
            }
            opcode::FILE_INFO => {
                no_arg(op, args)?;
                Self::GetFileInfo
            }
            opcode::DOWNLOAD_OLDEST => {
                no_arg(op, args)?;
                Self::DownloadOldestFile
            }
            opcode::DELETE_OLDEST => {
                no_arg(op, args)?;
                Self::DeleteOldestFile
            }
            _ => Self::Unknown(frame),
        };
        Ok(request)
    }

    /// Encodes the request as the bytes to write.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        self.to_frame().encode()
    }

    /// Decodes the bytes of one write (used by the simulator).
    pub fn decode(bytes: &[u8]) -> Result<Self, MessageError> {
        Self::from_frame(Frame::decode(bytes)?)
    }

    /// Whether the tablet answers this request right away. Registration (`e7`) is only
    /// answered once the user presses the button (protocol.md §2.3).
    pub fn expects_reply(&self) -> bool {
        !matches!(self, Self::RegisterPressButton { .. })
    }
}

/// `CONNECT uuid=89ea7888ffcb`, `SET_MODE paper`, ...
impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect { uuid } => write!(f, "CONNECT uuid={}", host_id_text(uuid)),
            Self::RegisterPressButton { uuid } => {
                write!(f, "REGISTER uuid={}", host_id_text(uuid))
            }
            Self::GetName => f.write_str("GET_NAME"),
            Self::SetName(name) => write!(f, "SET_NAME {name:?}"),
            Self::GetTime => f.write_str("GET_TIME"),
            Self::SetTime(time) => write!(f, "SET_TIME {time}"),
            Self::GetFirmware { part } => write!(f, "GET_FIRMWARE {part}"),
            Self::GetBattery => f.write_str("GET_BATTERY"),
            Self::GetDimension(which) => write!(f, "GET_DIMENSION {which}"),
            Self::SetFileTransferReportingType => f.write_str("SET_FILE_TRANSFER_REPORTING_TYPE"),
            Self::SetMode(mode) => write!(f, "SET_MODE {mode}"),
            Self::AvailableFilesCount => f.write_str("AVAILABLE_FILES_COUNT"),
            Self::GetFileInfo => f.write_str("GET_FILE_INFO"),
            Self::DownloadOldestFile => f.write_str("DOWNLOAD_OLDEST_FILE"),
            Self::DeleteOldestFile => f.write_str("DELETE_OLDEST_FILE"),
            Self::Unknown(frame) => write!(f, "unknown request {frame}"),
        }
    }
}

/// A frame sent by the tablet on the Nordic UART RX characteristic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// `b3 01 <code>`: generic status, a legal answer to any request.
    Status(StatusCode),
    /// `e4 01 00`: the user pressed the button during registration (any arguments accepted).
    RegisterButtonPressed,
    /// `bc n <bytes>`: part of the name. Chunks are appended until one ends with `0a`
    /// (see [`NameAssembler`]).
    NameChunk(Vec<u8>),
    /// `bd 06 <bcd>`: clock value.
    Time(BcdTime),
    /// `b8 n <part> <digits>`: half of the firmware version.
    Firmware {
        /// Which half.
        part: FirmwarePart,
        /// Version text, e.g. `B160805A`: the digits a Slate sends one per byte, or the ASCII
        /// text of other devices (trailing NUL bytes and whitespace removed).
        text: String,
    },
    /// `ba 02 <percent> <charging>`: battery state.
    Battery {
        /// Charge, 0 to 100 (not checked).
        percent: u8,
        /// The charging byte is `01`.
        charging: bool,
    },
    /// `eb 06 <which> 00 <u32>`: sensor dimension in units of 10 µm.
    Dimension {
        /// Which dimension.
        which: Dimension,
        /// Value, e.g. 29 700 for the width of a Slate Large.
        value: u32,
    },
    /// `c2 02 <u16>`: number of stored pages.
    FilesCount(u16),
    /// `cf 0a <u32> <bcd>`: the oldest page.
    FileInfo {
        /// Announced size in bytes (informative only; not used to check the download).
        size: u32,
        /// When the page was stored (UTC), the only timestamp of a Slate page.
        time: BcdTime,
    },
    /// `c8 n be ...`: the download starts.
    DownloadStart {
        /// Bytes after `be`, empty in the known Slate exchange (the Intuos Pro sends size and
        /// time there).
        extra: Vec<u8>,
    },
    /// `c8 05 ed <crc32 LE>`: the download is complete.
    DownloadEnd {
        /// IEEE CRC-32 of every byte received on the offline characteristic.
        crc: u32,
    },
    /// Any other opcode, kept verbatim.
    Unknown(Frame),
}

impl Reply {
    /// Whether this is a frame seen from a real Slate whose meaning is unknown (the unasked
    /// `cb 01 00`, [`opcode::UNEXPLAINED_CB`]): named in the log and ignored.
    pub fn is_unexplained(&self) -> bool {
        matches!(self, Self::Unknown(frame) if frame.opcode == opcode::UNEXPLAINED_CB)
    }

    /// The frame to send (used by the simulator).
    pub fn to_frame(&self) -> Frame {
        match self {
            Self::Status(code) => Frame::new(opcode::STATUS, [code.to_byte()]),
            Self::RegisterButtonPressed => Frame::new(opcode::REGISTER_BUTTON_PRESSED, [0x00]),
            Self::NameChunk(bytes) => Frame::new(opcode::NAME_CHUNK, bytes.as_slice()),
            Self::Time(time) => Frame::new(opcode::TIME_VALUE, time.to_bcd()),
            Self::Firmware { part, text } => {
                let mut args = vec![part.to_byte()];
                args.extend_from_slice(text.as_bytes());
                Frame::new(opcode::FIRMWARE_VALUE, args)
            }
            Self::Battery { percent, charging } => {
                Frame::new(opcode::BATTERY_VALUE, [*percent, u8::from(*charging)])
            }
            Self::Dimension { which, value } => {
                let mut args = vec![which.to_byte(), 0x00];
                args.extend_from_slice(&value.to_le_bytes());
                Frame::new(opcode::DIMENSION_VALUE, args)
            }
            Self::FilesCount(count) => Frame::new(opcode::FILES_COUNT_VALUE, count.to_le_bytes()),
            Self::FileInfo { size, time } => {
                let mut args = size.to_le_bytes().to_vec();
                args.extend_from_slice(&time.to_bcd());
                Frame::new(opcode::FILE_INFO_VALUE, args)
            }
            Self::DownloadStart { extra } => {
                let mut args = vec![DOWNLOAD_START_MARKER];
                args.extend_from_slice(extra);
                Frame::new(opcode::DOWNLOAD, args)
            }
            Self::DownloadEnd { crc } => {
                let mut args = vec![DOWNLOAD_END_MARKER];
                args.extend_from_slice(&crc.to_le_bytes());
                Frame::new(opcode::DOWNLOAD, args)
            }
            Self::Unknown(frame) => frame.clone(),
        }
    }

    /// Interprets a frame received by the host.
    pub fn from_frame(frame: Frame) -> Result<Self, MessageError> {
        let op = frame.opcode;
        let args = frame.args.as_slice();
        let reply = match op {
            opcode::STATUS => Self::Status(StatusCode::from_byte(single_arg(op, args)?)),
            opcode::REGISTER_BUTTON_PRESSED => Self::RegisterButtonPressed,
            opcode::NAME_CHUNK => Self::NameChunk(args.to_vec()),
            opcode::TIME_VALUE => Self::Time(time_arg(op, args, "6")?),
            opcode::FIRMWARE_VALUE => {
                let Some((&byte, text)) = args.split_first() else {
                    return Err(length(op, 0, "at least 1"));
                };
                let part = FirmwarePart::from_byte(byte)
                    .ok_or_else(|| argument(op, format!("unknown firmware part {byte:02x}")))?;
                Self::Firmware {
                    part,
                    text: firmware_text(text),
                }
            }
            opcode::BATTERY_VALUE => match args {
                [percent, charging] => Self::Battery {
                    percent: *percent,
                    charging: *charging == 0x01,
                },
                _ => return Err(length(op, args.len(), "2")),
            },
            opcode::DIMENSION_VALUE => match args {
                [byte, 0x00, a, b, c, d] => Self::Dimension {
                    which: dimension(op, *byte)?,
                    value: u32::from_le_bytes([*a, *b, *c, *d]),
                },
                [_, _, _, _, _, _] => return Err(argument(op, "second argument must be 00")),
                _ => return Err(length(op, args.len(), "6")),
            },
            opcode::FILES_COUNT_VALUE => match args {
                [low, high] => Self::FilesCount(u16::from_le_bytes([*low, *high])),
                _ => return Err(length(op, args.len(), "2")),
            },
            opcode::FILE_INFO_VALUE => match args {
                [a, b, c, d, bcd @ ..] if bcd.len() == 6 => Self::FileInfo {
                    size: u32::from_le_bytes([*a, *b, *c, *d]),
                    time: time_arg(op, bcd, "10")?,
                },
                _ => return Err(length(op, args.len(), "10")),
            },
            opcode::DOWNLOAD => match args {
                [DOWNLOAD_START_MARKER, extra @ ..] => Self::DownloadStart {
                    extra: extra.to_vec(),
                },
                [DOWNLOAD_END_MARKER, a, b, c, d] => Self::DownloadEnd {
                    crc: u32::from_le_bytes([*a, *b, *c, *d]),
                },
                [DOWNLOAD_END_MARKER, ..] => {
                    return Err(length(op, args.len(), "5 for a download end"));
                }
                [marker, ..] => {
                    return Err(argument(
                        op,
                        format!("first argument {marker:02x} is neither be (start) nor ed (end)"),
                    ));
                }
                [] => return Err(length(op, 0, "at least 1")),
            },
            _ => Self::Unknown(frame),
        };
        Ok(reply)
    }

    /// Encodes the reply as the bytes of one notification (used by the simulator).
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        self.to_frame().encode()
    }

    /// Decodes the bytes of one notification.
    pub fn decode(bytes: &[u8]) -> Result<Self, MessageError> {
        Self::from_frame(Frame::decode(bytes)?)
    }

    /// The `bc` replies the tablet sends for `name`: the name followed by `0a`, cut into
    /// chunks of at most [`NAME_CHUNK_MAX`] bytes so that each frame fits one notification.
    pub fn name_chunks(name: &str) -> Vec<Self> {
        let mut bytes = name.as_bytes().to_vec();
        bytes.push(NAME_END);
        bytes
            .chunks(NAME_CHUNK_MAX)
            .map(|chunk| Self::NameChunk(chunk.to_vec()))
            .collect()
    }
}

/// `STATUS success`, `BATTERY 62% discharging`, ...
impl fmt::Display for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(code) => write!(f, "STATUS {code}"),
            Self::RegisterButtonPressed => f.write_str("REGISTER_BUTTON_PRESSED"),
            Self::NameChunk(bytes) => write!(f, "NAME_CHUNK {:?}", String::from_utf8_lossy(bytes)),
            Self::Time(time) => write!(f, "TIME {time}"),
            Self::Firmware { part, text } => write!(f, "FIRMWARE {part} {text:?}"),
            Self::Battery { percent, charging } => write!(
                f,
                "BATTERY {percent}% {}",
                if *charging { "charging" } else { "discharging" }
            ),
            Self::Dimension { which, value } => write!(f, "DIMENSION {which}={value}"),
            Self::FilesCount(count) => write!(f, "FILES_COUNT {count}"),
            Self::FileInfo { size, time } => write!(f, "FILE_INFO size={size} time={time}"),
            Self::DownloadStart { extra } if extra.is_empty() => f.write_str("DOWNLOAD_START"),
            Self::DownloadStart { extra } => {
                write!(f, "DOWNLOAD_START extra={}", hex::encode(extra))
            }
            Self::DownloadEnd { crc } => write!(f, "DOWNLOAD_END crc={crc:08x}"),
            Self::Unknown(frame) if frame.opcode == opcode::UNEXPLAINED_CB => write!(
                f,
                "{frame}, seen once during live mode, meaning unknown (ignored)"
            ),
            Self::Unknown(frame) => write!(f, "unknown reply {frame}"),
        }
    }
}

/// Reassembles the tablet name from its `bc` chunks (protocol.md §4.1).
///
/// The tablet may split the name over several frames and the last one ends with `0a`. Some
/// units were seen sending a long name without the final `0a`: the caller then waits a short
/// time and calls [`NameAssembler::finish`].
#[derive(Debug, Clone, Default)]
pub struct NameAssembler {
    bytes: Vec<u8>,
}

impl NameAssembler {
    /// An empty assembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one chunk. Returns the complete name when the chunk ends with `0a`, and is
    /// then ready for a new name.
    pub fn push(&mut self, chunk: &[u8]) -> Option<String> {
        self.bytes.extend_from_slice(chunk);
        if chunk.last() == Some(&NAME_END) {
            Some(decode_name(&std::mem::take(&mut self.bytes)))
        } else {
            None
        }
    }

    /// The name from the chunks received so far, for when the final `0a` never comes.
    pub fn finish(self) -> String {
        decode_name(&self.bytes)
    }

    /// Whether no chunk has been received since the last complete name.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Text of a name: the bytes before the first `0a` or NUL, read as UTF-8 (invalid sequences
/// replaced), with trailing whitespace removed.
///
/// Cutting at the first NUL copes with the stray chunk `00 "Bamboo " 0a` seen after a
/// complete name in one trace (issue #117).
pub fn decode_name(bytes: &[u8]) -> String {
    let name = bytes
        .split(|&b| b == NAME_END || b == 0x00)
        .next()
        .unwrap_or_default();
    String::from_utf8_lossy(name).trim_end().to_owned()
}

/// The firmware version text of a `b8` reply. A Bamboo Slate sends one byte per hexadecimal
/// digit: `0b 01 06 00 09 02 09 0a` is `B160929A` (real Slate A4, 2026-11-02). The Intuos Pro
/// sends the digits as ASCII (issue #256): `42 31 38 30 31 33 31 61` is `B180131a`; that form is
/// read leniently as UTF-8, trailing NUL bytes and whitespace removed.
fn firmware_text(bytes: &[u8]) -> String {
    if !bytes.is_empty() && bytes.iter().all(|b| *b < 0x10) {
        return bytes
            .iter()
            .map(|b| char::from_digit(u32::from(*b), 16).map_or('?', |c| c.to_ascii_uppercase()))
            .collect();
    }
    String::from_utf8_lossy(bytes)
        .trim_end_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_owned()
}

/// The `b8` reply of a Bamboo Slate for a firmware half: one byte per hexadecimal digit of
/// `text` (see [`Reply::Firmware`]). `None` when `text` holds anything but hexadecimal digits.
/// [`Reply::encode`] writes the ASCII form of other devices instead.
pub fn firmware_digits_frame(part: FirmwarePart, text: &str) -> Option<Frame> {
    let mut args = vec![part.to_byte()];
    for c in text.chars() {
        args.push(u8::try_from(c.to_digit(16)?).ok()?);
    }
    Some(Frame::new(opcode::FIRMWARE_VALUE, args))
}

fn empty_request(op: u8) -> Frame {
    Frame::new(op, [0x00])
}

fn host_id_text(uuid: &HostId) -> String {
    uuid.iter().map(|b| format!("{b:02x}")).collect()
}

fn length(opcode: u8, actual: usize, expected: &'static str) -> MessageError {
    MessageError::Length {
        opcode,
        actual,
        expected,
    }
}

fn argument(opcode: u8, reason: impl Into<String>) -> MessageError {
    MessageError::Argument {
        opcode,
        reason: reason.into(),
    }
}

fn host_id(op: u8, args: &[u8]) -> Result<HostId, MessageError> {
    HostId::try_from(args).map_err(|_| length(op, args.len(), "6"))
}

fn single_arg(op: u8, args: &[u8]) -> Result<u8, MessageError> {
    match args {
        [byte] => Ok(*byte),
        _ => Err(length(op, args.len(), "1")),
    }
}

/// Checks the single `00` argument of an "empty" request (protocol.md §2.1).
fn no_arg(op: u8, args: &[u8]) -> Result<(), MessageError> {
    match single_arg(op, args)? {
        0x00 => Ok(()),
        byte => Err(argument(op, format!("argument must be 00, got {byte:02x}"))),
    }
}

fn time_arg(op: u8, bcd: &[u8], expected: &'static str) -> Result<BcdTime, MessageError> {
    if bcd.len() != 6 {
        return Err(length(op, bcd.len(), expected));
    }
    BcdTime::from_bcd_slice(bcd).map_err(|source| MessageError::Time { opcode: op, source })
}

fn dimension(op: u8, byte: u8) -> Result<Dimension, MessageError> {
    Dimension::from_byte(byte)
        .ok_or_else(|| argument(op, format!("unknown dimension selector {byte:02x}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).unwrap()
    }

    #[test]
    fn status_codes_round_trip() {
        for code in 0..=255u8 {
            assert_eq!(StatusCode::from_byte(code).to_byte(), code);
        }
        assert_eq!(StatusCode::from_byte(7), StatusCode::AuthorizationError);
        assert_eq!(StatusCode::from_byte(9), StatusCode::Other(9));
    }

    #[test]
    fn enums_round_trip_and_reject_unknown_bytes() {
        for mode in [Mode::Live, Mode::Paper, Mode::Idle] {
            assert_eq!(Mode::from_byte(mode.to_byte()), Some(mode));
        }
        assert_eq!(Mode::from_byte(3), None);
        for which in [Dimension::Width, Dimension::Height] {
            assert_eq!(Dimension::from_byte(which.to_byte()), Some(which));
        }
        assert_eq!(Dimension::from_byte(0x14), None);
        for part in [FirmwarePart::High, FirmwarePart::Low] {
            assert_eq!(FirmwarePart::from_byte(part.to_byte()), Some(part));
        }
        assert_eq!(FirmwarePart::from_byte(2), None);
    }

    #[test]
    fn set_name_distinguishes_from_get_name() {
        assert_eq!(
            Request::decode(&bytes("bb 01 00")).unwrap(),
            Request::GetName
        );
        assert_eq!(
            Request::decode(&bytes("bb 01 0a")).unwrap(),
            Request::SetName(String::new())
        );
        assert_eq!(
            Request::SetName(String::new()).encode().unwrap(),
            [0xbb, 0x01, 0x0a]
        );
        assert!(matches!(
            Request::decode(&bytes("bb 02 41 42")),
            Err(MessageError::Argument { opcode: 0xbb, .. })
        ));
        assert!(matches!(
            Request::decode(&bytes("bb 00")),
            Err(MessageError::Length { opcode: 0xbb, .. })
        ));
    }

    #[test]
    fn name_assembly() {
        let mut assembler = NameAssembler::new();
        assert!(assembler.is_empty());
        assert_eq!(assembler.push(b"Bamboo "), None);
        assert!(!assembler.is_empty());
        assert_eq!(assembler.push(b"Slate\n"), Some("Bamboo Slate".to_owned()));
        assert!(assembler.is_empty());

        // Issue #117: an 18-byte name with no final 0a; the caller times out.
        let mut assembler = NameAssembler::new();
        assert_eq!(assembler.push(b"Bamboo Slate Small"), None);
        assert_eq!(assembler.finish(), "Bamboo Slate Small");

        // Issue #117: the stray chunk arriving before the timeout does not pollute the name.
        let mut assembler = NameAssembler::new();
        assert_eq!(assembler.push(b"Bamboo Slate Small"), None);
        assert_eq!(
            assembler.push(b"\0Bamboo \n"),
            Some("Bamboo Slate Small".to_owned())
        );
    }

    #[test]
    fn name_chunks_fit_notifications_and_reassemble() {
        for name in [
            "",
            "Bamboo Slate",
            "Bamboo Slate Small",
            "A rather long tablet name",
        ] {
            let chunks = Reply::name_chunks(name);
            let mut assembler = NameAssembler::new();
            let mut result = None;
            for chunk in &chunks {
                let encoded = chunk.encode().unwrap();
                assert!(encoded.len() <= crate::MAX_NOTIFICATION_PAYLOAD);
                let Reply::NameChunk(data) = Reply::decode(&encoded).unwrap() else {
                    panic!("not a name chunk");
                };
                assert!(result.is_none());
                result = assembler.push(&data);
            }
            assert_eq!(result.as_deref(), Some(name));
        }
        assert_eq!(Reply::name_chunks("Bamboo Slate Small").len(), 2);
    }

    #[test]
    fn slate_firmware_is_one_byte_per_digit() {
        // Real Bamboo Slate A4 (registration of 2026-11-02).
        let high = Reply::decode(&bytes("b8 09 00 0b 01 06 00 09 02 09 0a")).unwrap();
        let low = Reply::decode(&bytes("b8 09 01 0c 01 07 00 07 00 04 0a")).unwrap();
        assert_eq!(
            high,
            Reply::Firmware {
                part: FirmwarePart::High,
                text: "B160929A".to_owned()
            }
        );
        assert_eq!(
            low,
            Reply::Firmware {
                part: FirmwarePart::Low,
                text: "C170704A".to_owned()
            }
        );
        assert_eq!(
            firmware_digits_frame(FirmwarePart::High, "B160929A")
                .unwrap()
                .encode()
                .unwrap(),
            bytes("b8 09 00 0b 01 06 00 09 02 09 0a")
        );
        assert_eq!(firmware_digits_frame(FirmwarePart::Low, "B18013-1"), None);
    }

    #[test]
    fn firmware_text_is_trimmed() {
        let reply = Reply::decode(&bytes("b8 0a 01 43 31 37 30 37 30 34 41 00")).unwrap();
        assert_eq!(
            reply,
            Reply::Firmware {
                part: FirmwarePart::Low,
                text: "C170704A".to_owned()
            }
        );
    }

    #[test]
    fn expects_reply() {
        assert!(!Request::RegisterPressButton { uuid: [0; 6] }.expects_reply());
        assert!(Request::Connect { uuid: [0; 6] }.expects_reply());
        assert!(Request::DeleteOldestFile.expects_reply());
    }

    #[test]
    fn register_button_accepts_any_arguments() {
        assert_eq!(
            Reply::decode(&[0xe4, 0x01, 0x00]).unwrap(),
            Reply::RegisterButtonPressed
        );
        // Tuhi's test suite uses an empty frame.
        assert_eq!(
            Reply::decode(&[0xe4, 0x00]).unwrap(),
            Reply::RegisterButtonPressed
        );
    }
}
