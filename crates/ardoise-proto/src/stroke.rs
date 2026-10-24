//! Stored page files, as downloaded from the offline characteristic (protocol.md §6).
//!
//! A file is a header followed by a packed stream of packets:
//!
//! ```text
//! FILE_HEADER                       Slate: 62 38 62 74; Intuos Pro: 67 82 69 65 + 12 bytes
//! repeat:
//!   STROKE_HEADER                   Slate: H ff ee ee ee tt tt ..; Intuos Pro: H fa .. [+ pen id]
//!   POINT                           first sample, absolute
//!   DELTA / POINT / LOST_POINT ...  one sample per POINT or DELTA packet, 5 ms apart
//!   STROKE_END                      fc ff ff ff ff ff ff
//! [EOF]                             ff ff ff ff ff ff ff ff ff
//! ```
//!
//! # Packet identification
//!
//! Every packet starts with a header byte `H`. Identification follows protocol.md §6.2, made
//! explicit where the research leaves room for doubt:
//!
//! 1. `H` with its two low bits at `00` is a DELTA: three 2-bit masks (pressure in bits 7..6,
//!    y in 5..4, x in 3..2) give the length. The only exception is the exact sequence
//!    `fc ff ff ff ff ff ff`, which is STROKE_END: read as a DELTA it would be three absolute
//!    values of `0xffff`, impossible on a real tablet. Any other DELTA whose bytes happen to be
//!    `ff` is a DELTA, e.g. `a8 ff ff ff` is three deltas of -1.
//! 2. Any other `H` is a "special" packet of `1 + popcount(H)` bytes. If its payload is all
//!    `ff` it is EOF when the payload has 8 bytes (`ff` x 9), STROKE_END otherwise.
//! 3. Payload `fa ..` is an Intuos Pro stroke header (followed by an `ff` + 8-byte pen id
//!    packet when its flags ask for it), `ff ee ee ..` a Slate stroke header, `ff ff ..` a
//!    POINT (the rest decoded as a DELTA body with the masks of `H`), `dd dd ..` a LOST_POINT.
//! 4. Anything else, including an empty payload, is unknown: skipped by popcount and counted.
//!
//! # Reconstruction
//!
//! Points are second-order delta coded (§6.5). Per axis the decoder keeps a value and a
//! velocity; a delta is added to the velocity, an absolute value replaces the value and
//! zeroes the velocity, an absent axis keeps both; each sample is then `value + velocity`. The
//! values persist across strokes; only the velocity is reset, at every stroke header.

/// Magic bytes of a Spark or Slate file (`b8bt`); the file header is these 4 bytes.
pub const SLATE_MAGIC: [u8; 4] = [0x62, 0x38, 0x62, 0x74];
/// Magic bytes of an Intuos Pro file, whose header is [`INTUOS_PRO_HEADER_LEN`] bytes.
pub const INTUOS_PRO_MAGIC: [u8; 4] = [0x67, 0x82, 0x69, 0x65];
/// Size of the Intuos Pro file header.
pub const INTUOS_PRO_HEADER_LEN: usize = 16;
/// The STROKE_END packet written by the encoder (the Slate's own form).
pub const STROKE_END_PACKET: [u8; 7] = [0xfc, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
/// Resolution of the Slate stroke-header time offset.
const TICK_MS: u32 = 5;
/// Size of the pen-id packet that may follow an Intuos Pro stroke header.
const PEN_ID_PACKET_LEN: usize = 9;
/// Intuos Pro stroke-header flags.
const FLAG_PEN_ID: u8 = 0x80;
const FLAG_NEW_LAYER: u8 = 0x40;
const PEN_TYPE_MASK: u8 = 0x3f;
const AXIS_NAMES: [&str; 3] = ["x", "y", "pressure"];

/// One pen sample in raw device units: `x`, `y` in 10 µm on the Slate, `pressure` from 0 to
/// 2047 on the Slate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    /// Horizontal position.
    pub x: u16,
    /// Vertical position.
    pub y: u16,
    /// Pen pressure.
    pub pressure: u16,
}

/// How one axis of a POINT or DELTA packet is coded (protocol.md §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Mask `00`: keep moving at the current velocity.
    Absent,
    /// Mask `10`: signed change of velocity (never 0).
    Delta(i8),
    /// Mask `11`: absolute value, velocity reset to 0.
    Absolute(u16),
}

/// One packet of a page file, as identified by [`parse_packet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// Slate stroke header `H ff ee ee ee tt tt ..`.
    SlateStrokeHeader {
        /// `tt tt`: time since power-up in 5 ms ticks; `None` if the packet is too short to
        /// carry it.
        time_ticks: Option<u16>,
    },
    /// Intuos Pro stroke header `H fa FL t0 t1 t2 t3 m0 m1`, with its pen-id packet if any.
    IntuosStrokeHeader {
        /// `FL`: pen-id flag (0x80), new-layer flag (0x40), pen type (low 6 bits).
        flags: u8,
        /// Unix time of the stroke.
        seconds: u32,
        /// Sub-second part (unit unclear, probably milliseconds).
        subsecond: u16,
        /// Serial of the pen, from the following pen-id packet.
        pen_id: Option<u64>,
    },
    /// POINT `H ff ff <body>`: a sample (absolute in practice).
    Point {
        /// Horizontal axis.
        x: Axis,
        /// Vertical axis.
        y: Axis,
        /// Pressure axis.
        pressure: Axis,
    },
    /// DELTA: a sample relative to the previous ones.
    Delta {
        /// Horizontal axis.
        x: Axis,
        /// Vertical axis.
        y: Axis,
        /// Pressure axis.
        pressure: Axis,
    },
    /// LOST_POINT `H dd dd LL LL ..`: samples the tablet could not record.
    LostPoint {
        /// Number of lost samples.
        count: u16,
    },
    /// End of the current stroke.
    StrokeEnd,
    /// End of the file.
    Eof,
    /// Unrecognized packet of `1 + popcount(header)` bytes.
    Unknown {
        /// Its header byte.
        header: u8,
    },
}

/// When a stroke started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeTime {
    /// No stroke header (points before the first header, as in Spark files).
    #[default]
    Unknown,
    /// Slate: time since the tablet powered up, 5 ms resolution, wraps every 327.68 s.
    SincePowerUp {
        /// Milliseconds (a multiple of 5 when decoded).
        millis: u32,
    },
    /// Intuos Pro: absolute time.
    Unix {
        /// Unix seconds.
        seconds: u32,
        /// Sub-second part as sent (unit unclear, probably milliseconds).
        subsecond: u16,
    },
}

/// Pen details of an Intuos Pro stroke (the Slate has none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenInfo {
    /// Pen type, low 6 bits of the flags.
    pub pen_type: u8,
    /// The stroke starts a new layer.
    pub new_layer: bool,
    /// Pen serial, when the tablet sent it.
    pub pen_id: Option<u64>,
}

/// One stroke: the samples between a stroke header and the next STROKE_END or stroke header.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stroke {
    /// Start time from the stroke header.
    pub time: StrokeTime,
    /// Intuos Pro pen details; `None` on the Slate.
    pub pen: Option<PenInfo>,
    /// Samples in order, 5 ms apart.
    pub points: Vec<Point>,
    /// Samples reported lost by LOST_POINT packets (not interpolated).
    pub lost_points: u32,
}

/// Format announced by the file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    /// Spark or Slate (`62 38 62 74`): no timestamp in the file (it comes from the `cf` reply).
    Slate,
    /// Intuos Pro (`67 82 69 65`).
    IntuosPro {
        /// Unix time of the end of the drawing.
        seconds: u32,
        /// Milliseconds (always 0 in practice).
        millis: u16,
        /// Announced number of strokes (only roughly reliable).
        stroke_count: u32,
    },
}

/// A decoded page file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrokeFile {
    /// Format from the file header.
    pub format: FileFormat,
    /// Strokes in order, including strokes without points.
    pub strokes: Vec<Stroke>,
    /// Unrecognized packets that were skipped.
    pub unknown_packets: usize,
    /// The data ended with an EOF packet (otherwise it simply ran out).
    pub eof: bool,
    /// Bytes after the EOF packet, ignored.
    pub trailing_bytes: usize,
}

impl StrokeFile {
    /// Total number of samples in all strokes.
    pub fn point_count(&self) -> usize {
        self.strokes.iter().map(|s| s.points.len()).sum()
    }
}

/// Failure while decoding a page file. Offsets count from the start of the data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StrokeError {
    /// The data is shorter than its file header.
    #[error("{len} bytes are too short for a page file header")]
    TooShort {
        /// Data length.
        len: usize,
    },
    /// Neither the Slate nor the Intuos Pro magic.
    #[error("unknown page file format (magic {magic:02x?})")]
    UnknownFormat {
        /// First four bytes.
        magic: [u8; 4],
    },
    /// The last packet is cut short by the end of the data.
    #[error("packet at offset {offset} is cut short by the end of the data")]
    Truncated {
        /// Offset of the packet.
        offset: usize,
    },
    /// An axis mask of `01`, which no device is known to send.
    #[error("packet at offset {offset} uses the reserved axis mask 01")]
    ReservedMask {
        /// Offset of the packet.
        offset: usize,
    },
    /// A delta of 0, which the protocol forbids (the axis would be absent instead).
    #[error("packet at offset {offset} has a zero delta")]
    ZeroDelta {
        /// Offset of the packet.
        offset: usize,
    },
    /// The packet after an Intuos Pro stroke header that announces a pen id does not start
    /// with `ff`.
    #[error("pen id packet at offset {offset} starts with {header:02x} instead of ff")]
    PenIdHeader {
        /// Offset of the pen-id packet.
        offset: usize,
        /// Its header byte.
        header: u8,
    },
    /// A recognized packet whose length does not fit its kind.
    #[error("malformed {kind} packet at offset {offset}")]
    Malformed {
        /// Offset of the packet.
        offset: usize,
        /// Kind of packet.
        kind: &'static str,
    },
    /// The reconstructed value of an axis leaves the 16-bit range.
    #[error("{axis} leaves the 0..=65535 range ({value}) at offset {offset}")]
    OutOfRange {
        /// Offset of the packet.
        offset: usize,
        /// Axis name.
        axis: &'static str,
        /// Value reached.
        value: i64,
    },
}

/// Identifies the packet at the start of `bytes` and returns it with its size in bytes.
///
/// An Intuos Pro stroke header includes its pen-id packet in the size. Offsets in errors
/// count from the start of `bytes`.
pub fn parse_packet(bytes: &[u8]) -> Result<(Packet, usize), StrokeError> {
    packet_at(bytes, 0)
}

/// Decodes a whole page file (Slate or Intuos Pro).
///
/// Points that come before any stroke header, or after a STROKE_END without a new header,
/// open a stroke of unknown time.
pub fn decode(data: &[u8]) -> Result<StrokeFile, StrokeError> {
    let (format, mut offset) = file_header(data)?;
    let mut builder = Builder::default();
    let mut unknown_packets = 0;
    let mut eof = false;
    while offset < data.len() && !eof {
        let (packet, size) = packet_at(data, offset)?;
        match packet {
            Packet::SlateStrokeHeader { time_ticks } => {
                let time =
                    time_ticks.map_or(StrokeTime::Unknown, |ticks| StrokeTime::SincePowerUp {
                        millis: u32::from(ticks) * TICK_MS,
                    });
                builder.start(time, None);
            }
            Packet::IntuosStrokeHeader {
                flags,
                seconds,
                subsecond,
                pen_id,
            } => {
                let pen = PenInfo {
                    pen_type: flags & PEN_TYPE_MASK,
                    new_layer: flags & FLAG_NEW_LAYER != 0,
                    pen_id,
                };
                builder.start(StrokeTime::Unix { seconds, subsecond }, Some(pen));
            }
            Packet::Point { x, y, pressure } | Packet::Delta { x, y, pressure } => {
                builder.sample([x, y, pressure], offset)?;
            }
            Packet::LostPoint { count } => builder.lost(count),
            Packet::StrokeEnd => builder.close(),
            Packet::Eof => eof = true,
            Packet::Unknown { .. } => unknown_packets += 1,
        }
        offset += size;
    }
    builder.close();
    Ok(StrokeFile {
        format,
        strokes: builder.strokes,
        unknown_packets,
        eof,
        trailing_bytes: data.len().saturating_sub(offset),
    })
}

/// Encodes strokes as a Slate page file, as the tablet would store them (used by the
/// simulator).
///
/// Layout: the magic, then per stroke a stroke header carrying the time offset, the first
/// sample as an absolute POINT, the other samples as DELTA packets (per axis: absent when the
/// velocity is unchanged, an 8-bit delta when it fits, the absolute value otherwise), one
/// LOST_POINT packet per 65 535 lost samples, and STROKE_END. No EOF packet is written.
///
/// [`decode`] gives back exactly the same strokes, with two documented limits:
/// - the time is stored as `SincePowerUp` in 5 ms ticks modulo 65 536 (other times become 0);
/// - a sample with `x`, `y` and `pressure` all equal to `0xffff` would read as an end marker,
///   so its pressure is stored as `0xfffe` (real Slates never exceed a pressure of 2047).
///
/// Pen details are not part of the Slate format and are dropped.
pub fn encode_slate(strokes: &[Stroke]) -> Vec<u8> {
    let mut out = SLATE_MAGIC.to_vec();
    let mut encoder = SampleEncoder::default();
    for stroke in strokes {
        let ticks = match stroke.time {
            // Truncation is the device's own wrap-around of the 16-bit tick counter.
            StrokeTime::SincePowerUp { millis } => (millis / TICK_MS) as u16,
            StrokeTime::Unknown | StrokeTime::Unix { .. } => 0,
        };
        let [low, high] = ticks.to_le_bytes();
        out.extend_from_slice(&[0xff, 0xff, 0xee, 0xee, 0xee, low, high, 0x00, 0x00]);
        encoder.velocity = [0; 3];
        for (index, point) in stroke.points.iter().enumerate() {
            let point = representable(*point);
            if index == 0 {
                out.extend_from_slice(&[0xff, 0xff, 0xff]);
                for value in [point.x, point.y, point.pressure] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                encoder.value = [point.x, point.y, point.pressure].map(i64::from);
            } else {
                push_delta(&mut out, encoder.next(point));
            }
        }
        let mut lost = stroke.lost_points;
        while lost > 0 {
            let count = u16::try_from(lost).unwrap_or(u16::MAX);
            let [low, high] = count.to_le_bytes();
            out.extend_from_slice(&[0xff, 0xdd, 0xdd, low, high, 0x00, 0x00, 0x00, 0x00]);
            lost -= u32::from(count);
        }
        out.extend_from_slice(&STROKE_END_PACKET);
    }
    out
}

fn file_header(data: &[u8]) -> Result<(FileFormat, usize), StrokeError> {
    let too_short = StrokeError::TooShort { len: data.len() };
    if data.starts_with(&SLATE_MAGIC) {
        return Ok((FileFormat::Slate, SLATE_MAGIC.len()));
    }
    if data.starts_with(&INTUOS_PRO_MAGIC) {
        let [_, _, _, _, s0, s1, s2, s3, m0, m1, c0, c1, c2, c3, _, _, ..] = data else {
            return Err(too_short);
        };
        let format = FileFormat::IntuosPro {
            seconds: u32::from_le_bytes([*s0, *s1, *s2, *s3]),
            millis: u16::from_le_bytes([*m0, *m1]),
            stroke_count: u32::from_le_bytes([*c0, *c1, *c2, *c3]),
        };
        return Ok((format, INTUOS_PRO_HEADER_LEN));
    }
    match data {
        [a, b, c, d, ..] => Err(StrokeError::UnknownFormat {
            magic: [*a, *b, *c, *d],
        }),
        _ => Err(too_short),
    }
}

/// Why the axis values of a POINT or DELTA could not be read.
enum AxesError {
    Reserved,
    ZeroDelta,
    Short,
}

/// Reads the axis values announced by the masks in bits 7..2 of `header` from `body`, in the
/// order x, y, pressure. Returns them with the number of bytes used.
fn parse_axes(header: u8, body: &[u8]) -> Result<([Axis; 3], usize), AxesError> {
    let mut axes = [Axis::Absent; 3];
    let mut used = 0;
    for (index, axis) in axes.iter_mut().enumerate() {
        let mask = (header >> (2 + 2 * index)) & 0b11;
        *axis = match mask {
            0b00 => Axis::Absent,
            0b10 => {
                let byte = *body.get(used).ok_or(AxesError::Short)?;
                used += 1;
                match i8::from_le_bytes([byte]) {
                    0 => return Err(AxesError::ZeroDelta),
                    delta => Axis::Delta(delta),
                }
            }
            0b11 => {
                let pair = body
                    .get(used..used + 2)
                    .and_then(|pair| <[u8; 2]>::try_from(pair).ok())
                    .ok_or(AxesError::Short)?;
                used += 2;
                Axis::Absolute(u16::from_le_bytes(pair))
            }
            _ => return Err(AxesError::Reserved),
        };
    }
    Ok((axes, used))
}

fn packet_at(data: &[u8], offset: usize) -> Result<(Packet, usize), StrokeError> {
    let bytes = data.get(offset..).unwrap_or_default();
    let Some((&header, after)) = bytes.split_first() else {
        return Err(StrokeError::Truncated { offset });
    };

    if header & 0b11 == 0 {
        if bytes.starts_with(&STROKE_END_PACKET) {
            return Ok((Packet::StrokeEnd, STROKE_END_PACKET.len()));
        }
        return match parse_axes(header, after) {
            Ok(([x, y, pressure], used)) => Ok((Packet::Delta { x, y, pressure }, 1 + used)),
            Err(AxesError::Short) => Err(StrokeError::Truncated { offset }),
            Err(AxesError::Reserved) => Err(StrokeError::ReservedMask { offset }),
            Err(AxesError::ZeroDelta) => Err(StrokeError::ZeroDelta { offset }),
        };
    }

    let payload_len = header.count_ones() as usize;
    let size = 1 + payload_len;
    let payload = after
        .get(..payload_len)
        .ok_or(StrokeError::Truncated { offset })?;
    if !payload.is_empty() && payload.iter().all(|&b| b == 0xff) {
        let packet = if payload_len == 8 {
            Packet::Eof
        } else {
            Packet::StrokeEnd
        };
        return Ok((packet, size));
    }
    match payload {
        [0xfa, rest @ ..] => intuos_stroke_header(data, offset, rest, size),
        [0xff, 0xee, 0xee, rest @ ..] => {
            // rest[0] is the fourth byte (ee in every trace), rest[1..3] the time.
            let time_ticks = match rest {
                [_, low, high, ..] => Some(u16::from_le_bytes([*low, *high])),
                _ => None,
            };
            Ok((Packet::SlateStrokeHeader { time_ticks }, size))
        }
        [0xff, 0xff, body @ ..] => match parse_axes(header, body) {
            Ok(([x, y, pressure], used)) if used == body.len() => {
                Ok((Packet::Point { x, y, pressure }, size))
            }
            // The masks need more or fewer bytes than the popcount gives: only possible when
            // the low bits of the header are not 11.
            Ok(_) | Err(AxesError::Short) => Err(StrokeError::Malformed {
                offset,
                kind: "point",
            }),
            Err(AxesError::Reserved) => Err(StrokeError::ReservedMask { offset }),
            Err(AxesError::ZeroDelta) => Err(StrokeError::ZeroDelta { offset }),
        },
        [0xdd, 0xdd, low, high, ..] => Ok((
            Packet::LostPoint {
                count: u16::from_le_bytes([*low, *high]),
            },
            size,
        )),
        [0xdd, 0xdd, ..] => Err(StrokeError::Malformed {
            offset,
            kind: "lost point",
        }),
        _ => Ok((Packet::Unknown { header }, size)),
    }
}

/// Parses the rest of an Intuos Pro stroke header (`rest` = the payload after `fa`) and its
/// pen-id packet.
fn intuos_stroke_header(
    data: &[u8],
    offset: usize,
    rest: &[u8],
    size: usize,
) -> Result<(Packet, usize), StrokeError> {
    let [flags, s0, s1, s2, s3, subsecond @ ..] = rest else {
        return Err(StrokeError::Malformed {
            offset,
            kind: "Intuos Pro stroke header",
        });
    };
    // Headers other than ff carry fewer sub-second bytes.
    let subsecond = match subsecond {
        [] => 0,
        [low] => u16::from(*low),
        [low, high, ..] => u16::from_le_bytes([*low, *high]),
    };
    let mut total = size;
    let pen_id = if flags & FLAG_PEN_ID != 0 {
        let pen_offset = offset + size;
        match data.get(pen_offset..).unwrap_or_default() {
            [0xff, a, b, c, d, e, f, g, h, ..] => {
                total += PEN_ID_PACKET_LEN;
                Some(u64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h]))
            }
            [] | [0xff, ..] => return Err(StrokeError::Truncated { offset: pen_offset }),
            [header, ..] => {
                return Err(StrokeError::PenIdHeader {
                    offset: pen_offset,
                    header: *header,
                });
            }
        }
    } else {
        None
    };
    let packet = Packet::IntuosStrokeHeader {
        flags: *flags,
        seconds: u32::from_le_bytes([*s0, *s1, *s2, *s3]),
        subsecond,
        pen_id,
    };
    Ok((packet, total))
}

/// Stroke list and reconstruction state of the decoder.
#[derive(Default)]
struct Builder {
    strokes: Vec<Stroke>,
    current: Option<Stroke>,
    /// Last value per axis (x, y, pressure); kept across strokes.
    value: [i64; 3],
    /// Velocity per axis; reset at every stroke header.
    velocity: [i64; 3],
}

impl Builder {
    fn start(&mut self, time: StrokeTime, pen: Option<PenInfo>) {
        self.close();
        self.velocity = [0; 3];
        self.current = Some(Stroke {
            time,
            pen,
            ..Stroke::default()
        });
    }

    fn close(&mut self) {
        if let Some(stroke) = self.current.take() {
            self.strokes.push(stroke);
        }
    }

    fn current(&mut self) -> &mut Stroke {
        self.current.get_or_insert_with(Stroke::default)
    }

    fn lost(&mut self, count: u16) {
        let stroke = self.current();
        stroke.lost_points = stroke.lost_points.saturating_add(u32::from(count));
    }

    fn sample(&mut self, axes: [Axis; 3], offset: usize) -> Result<(), StrokeError> {
        let mut point = [0u16; 3];
        for (index, axis) in axes.into_iter().enumerate() {
            match axis {
                Axis::Absent => {}
                Axis::Delta(delta) => self.velocity[index] += i64::from(delta),
                Axis::Absolute(value) => {
                    self.value[index] = i64::from(value);
                    self.velocity[index] = 0;
                }
            }
            let next = self.value[index] + self.velocity[index];
            point[index] = u16::try_from(next).map_err(|_| StrokeError::OutOfRange {
                offset,
                axis: AXIS_NAMES[index],
                value: next,
            })?;
            self.value[index] = next;
        }
        let [x, y, pressure] = point;
        self.current().points.push(Point { x, y, pressure });
        Ok(())
    }
}

/// Mirror of the decoder state, used to choose the cheapest coding of each sample.
#[derive(Default)]
struct SampleEncoder {
    value: [i64; 3],
    velocity: [i64; 3],
}

impl SampleEncoder {
    fn next(&mut self, point: Point) -> [Axis; 3] {
        let targets = [point.x, point.y, point.pressure];
        let mut axes = [Axis::Absent; 3];
        for (index, axis) in axes.iter_mut().enumerate() {
            let target = i64::from(targets[index]);
            let predicted = self.value[index] + self.velocity[index];
            *axis = if predicted == target {
                Axis::Absent
            } else if let Ok(delta) = i8::try_from(target - predicted) {
                self.velocity[index] += i64::from(delta);
                Axis::Delta(delta)
            } else {
                self.velocity[index] = 0;
                Axis::Absolute(targets[index])
            };
            self.value[index] = target;
        }
        axes
    }
}

fn push_delta(out: &mut Vec<u8>, axes: [Axis; 3]) {
    let mut header = 0u8;
    let mut body = Vec::with_capacity(6);
    for (index, axis) in axes.into_iter().enumerate() {
        let mask = match axis {
            Axis::Absent => 0b00,
            Axis::Delta(delta) => {
                body.extend_from_slice(&delta.to_le_bytes());
                0b10
            }
            Axis::Absolute(value) => {
                body.extend_from_slice(&value.to_le_bytes());
                0b11
            }
        };
        header |= mask << (2 + 2 * index);
    }
    out.push(header);
    out.extend_from_slice(&body);
}

/// Replaces the one sample that cannot be coded (all axes `0xffff`, the end-marker pattern).
fn representable(point: Point) -> Point {
    if point.x == u16::MAX && point.y == u16::MAX && point.pressure == u16::MAX {
        Point {
            pressure: u16::MAX - 1,
            ..point
        }
    } else {
        point
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).unwrap()
    }

    fn packet(text: &str) -> (Packet, usize) {
        parse_packet(&bytes(text)).unwrap()
    }

    fn point(x: u16, y: u16, pressure: u16) -> Point {
        Point { x, y, pressure }
    }

    fn slate_stroke(millis: u32, points: Vec<Point>) -> Stroke {
        Stroke {
            time: StrokeTime::SincePowerUp { millis },
            points,
            ..Stroke::default()
        }
    }

    fn round_trip(strokes: &[Stroke]) -> StrokeFile {
        let encoded = encode_slate(strokes);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.format, FileFormat::Slate);
        assert_eq!(decoded.strokes, strokes);
        assert_eq!(decoded.unknown_packets, 0);
        assert!(!decoded.eof);
        assert_eq!(decoded.trailing_bytes, 0);
        decoded
    }

    // Per-packet vectors below are byte data copied from Tuhi's test suite
    // (test/test_strokes.py :: TestStrokeParsers, Copyright (c) 2019 Red Hat, Inc.,
    // GPL-2.0-or-later), and the deltas of the worked example in protocol.md §6.4.

    #[test]
    fn delta_examples_from_protocol_md() {
        assert_eq!(
            packet("c8 01 cb 04"),
            (
                Packet::Delta {
                    x: Axis::Delta(1),
                    y: Axis::Absent,
                    pressure: Axis::Absolute(0x04cb)
                },
                4
            )
        );
        assert_eq!(
            packet("e8 ff 01 0f 06"),
            (
                Packet::Delta {
                    x: Axis::Delta(-1),
                    y: Axis::Delta(1),
                    pressure: Axis::Absolute(0x060f)
                },
                5
            )
        );
        assert_eq!(
            packet("a8 03 ff 75"),
            (
                Packet::Delta {
                    x: Axis::Delta(3),
                    y: Axis::Delta(-1),
                    pressure: Axis::Delta(0x75)
                },
                4
            )
        );
    }

    #[test]
    fn delta_mask_combinations() {
        let cases: [(&str, [Axis; 3]); 8] = [
            ("08 01", [Axis::Delta(1), Axis::Absent, Axis::Absent]),
            ("20 02", [Axis::Absent, Axis::Delta(2), Axis::Absent]),
            ("80 03", [Axis::Absent, Axis::Absent, Axis::Delta(3)]),
            ("88 03 05", [Axis::Delta(3), Axis::Absent, Axis::Delta(5)]),
            (
                "a8 03 05 07",
                [Axis::Delta(3), Axis::Delta(5), Axis::Delta(7)],
            ),
            (
                "fc 01 02 03 04 05 06",
                [
                    Axis::Absolute(0x0201),
                    Axis::Absolute(0x0403),
                    Axis::Absolute(0x0605),
                ],
            ),
            (
                "30 02 03",
                [Axis::Absent, Axis::Absolute(0x0302), Axis::Absent],
            ),
            (
                "ac 02 03 04 05",
                [Axis::Absolute(0x0302), Axis::Delta(4), Axis::Delta(5)],
            ),
        ];
        for (text, [x, y, pressure]) in cases {
            let data = bytes(text);
            assert_eq!(
                parse_packet(&data).unwrap(),
                (Packet::Delta { x, y, pressure }, data.len()),
                "{text}"
            );
        }
        // No mask at all: a one-byte sample that keeps the current velocity.
        assert_eq!(
            packet("00"),
            (
                Packet::Delta {
                    x: Axis::Absent,
                    y: Axis::Absent,
                    pressure: Axis::Absent
                },
                1
            )
        );
    }

    #[test]
    fn deltas_made_of_ff_bytes_are_deltas() {
        assert_eq!(
            packet("a8 ff ff ff"),
            (
                Packet::Delta {
                    x: Axis::Delta(-1),
                    y: Axis::Delta(-1),
                    pressure: Axis::Delta(-1)
                },
                4
            )
        );
        // fc with fewer than six ff is an absolute DELTA, not a STROKE_END.
        assert_eq!(
            packet("fc ff ff ff ff ff 07"),
            (
                Packet::Delta {
                    x: Axis::Absolute(0xffff),
                    y: Axis::Absolute(0xffff),
                    pressure: Axis::Absolute(0x07ff)
                },
                7
            )
        );
    }

    #[test]
    fn delta_errors() {
        assert_eq!(
            parse_packet(&bytes("04 01")),
            Err(StrokeError::ReservedMask { offset: 0 })
        );
        assert_eq!(
            parse_packet(&bytes("08 00")),
            Err(StrokeError::ZeroDelta { offset: 0 })
        );
        assert_eq!(
            parse_packet(&bytes("fc 01 02 03")),
            Err(StrokeError::Truncated { offset: 0 })
        );
        assert_eq!(parse_packet(&[]), Err(StrokeError::Truncated { offset: 0 }));
    }

    #[test]
    fn point_packets() {
        assert_eq!(
            packet("ff ff ff 01 02 03 04 05 06"),
            (
                Packet::Point {
                    x: Axis::Absolute(0x0201),
                    y: Axis::Absolute(0x0403),
                    pressure: Axis::Absolute(0x0605)
                },
                9
            )
        );
        // bf: 8-bit pressure delta, so the packet is 8 bytes and the 06 belongs to the next.
        assert_eq!(
            packet("bf ff ff 01 02 03 04 05 06"),
            (
                Packet::Point {
                    x: Axis::Absolute(0x0201),
                    y: Axis::Absolute(0x0403),
                    pressure: Axis::Delta(5)
                },
                8
            )
        );
        // Low bits 01: the masks need one byte more than the popcount allows.
        assert_eq!(
            parse_packet(&bytes("fd ff ff 01 02 03 04 05 06")),
            Err(StrokeError::Malformed {
                offset: 0,
                kind: "point"
            })
        );
    }

    #[test]
    fn lost_point_eof_and_stroke_end() {
        assert_eq!(
            packet("ff dd dd 01 02 03 04 05 06"),
            (Packet::LostPoint { count: 0x0201 }, 9)
        );
        assert_eq!(packet("ff ff ff ff ff ff ff ff ff"), (Packet::Eof, 9));
        assert_eq!(packet("fc ff ff ff ff ff ff"), (Packet::StrokeEnd, 7));
        // Any special header with an all-ff payload shorter than 8 bytes ends the stroke.
        assert_eq!(packet("3f ff ff ff ff ff ff"), (Packet::StrokeEnd, 7));
        assert_eq!(
            parse_packet(&bytes("07 dd dd 01")),
            Err(StrokeError::Malformed {
                offset: 0,
                kind: "lost point"
            })
        );
    }

    #[test]
    fn stroke_headers() {
        assert_eq!(
            packet("ff ff ee ee ee 34 12 00 00"),
            (
                Packet::SlateStrokeHeader {
                    time_ticks: Some(0x1234)
                },
                9
            )
        );
        // A header too short to hold the time is still a stroke header.
        assert_eq!(
            packet("1f ff ee ee ee 34"),
            (Packet::SlateStrokeHeader { time_ticks: None }, 6)
        );
        assert_eq!(
            packet("ff fa 43 1f 73 53 5d 2e 01"),
            (
                Packet::IntuosStrokeHeader {
                    flags: 0x43,
                    seconds: 1_565_750_047,
                    subsecond: 302,
                    pen_id: None
                },
                9
            )
        );
        assert_eq!(
            packet("ff fa c1 1f 73 53 5d 2e 01 ff 0a 87 75 80 28 42 00 10"),
            (
                Packet::IntuosStrokeHeader {
                    flags: 0xc1,
                    seconds: 1_565_750_047,
                    subsecond: 302,
                    pen_id: Some(0x1000_4228_8075_870a)
                },
                18
            )
        );
        assert_eq!(
            parse_packet(&bytes(
                "ff fa c1 1f 73 53 5d 2e 01 bf 0a 87 75 80 28 42 00 10"
            )),
            Err(StrokeError::PenIdHeader {
                offset: 9,
                header: 0xbf
            })
        );
        assert_eq!(
            parse_packet(&bytes("ff fa c1 1f 73 53 5d 2e 01 ff 0a 87")),
            Err(StrokeError::Truncated { offset: 9 })
        );
    }

    #[test]
    fn unknown_packets_are_skipped_by_popcount() {
        assert_eq!(packet("03 12 34"), (Packet::Unknown { header: 0x03 }, 3));
        assert_eq!(packet("01 77"), (Packet::Unknown { header: 0x01 }, 2));
        let data = [
            SLATE_MAGIC.as_slice(),
            &bytes("07 12 34 56 ff ff ff 01 00 02 00 03 00"),
        ]
        .concat();
        let file = decode(&data).unwrap();
        assert_eq!(file.unknown_packets, 1);
        assert_eq!(file.strokes.len(), 1);
        assert_eq!(file.strokes[0].points, [point(1, 2, 3)]);
    }

    #[test]
    fn file_headers() {
        assert_eq!(decode(&SLATE_MAGIC).unwrap().format, FileFormat::Slate);
        assert_eq!(decode(&SLATE_MAGIC).unwrap().strokes, []);
        let intuos = bytes("67 82 69 65 22 73 53 5d 00 00 02 00 00 00 00 00");
        assert_eq!(
            decode(&intuos).unwrap().format,
            FileFormat::IntuosPro {
                seconds: 1_565_750_050,
                millis: 0,
                stroke_count: 2
            }
        );
        assert_eq!(
            decode(&intuos[..10]),
            Err(StrokeError::TooShort { len: 10 })
        );
        assert_eq!(
            decode(&bytes("62 38 62")),
            Err(StrokeError::TooShort { len: 3 })
        );
        assert_eq!(
            decode(&bytes("62 38 62 73 00")),
            Err(StrokeError::UnknownFormat {
                magic: [0x62, 0x38, 0x62, 0x73]
            })
        );
        assert_eq!(
            decode(&bytes("ff 62 38 62 74")),
            Err(StrokeError::UnknownFormat {
                magic: [0xff, 0x62, 0x38, 0x62]
            })
        );
    }

    #[test]
    fn spark_style_file_without_stroke_header() {
        // Start of a Spark file quoted in issue #111: magic, then a POINT with an 8-bit
        // pressure delta (from the initial pressure 0).
        let file = decode(&bytes("62 38 62 74 bf ff ff 4a 14 29 31 6a")).unwrap();
        assert_eq!(file.strokes.len(), 1);
        assert_eq!(file.strokes[0].time, StrokeTime::Unknown);
        assert_eq!(file.strokes[0].points, [point(0x144a, 0x3129, 0x6a)]);
    }

    #[test]
    fn eof_stops_decoding() {
        let data = [
            SLATE_MAGIC.as_slice(),
            &bytes("ff ff ee ee ee 01 00 00 00 ff ff ff 01 00 02 00 03 00"),
            &[0xff; 9],
            &[0x12, 0x34],
        ]
        .concat();
        let file = decode(&data).unwrap();
        assert!(file.eof);
        assert_eq!(file.trailing_bytes, 2);
        assert_eq!(file.strokes.len(), 1);
        assert_eq!(file.strokes[0].time, StrokeTime::SincePowerUp { millis: 5 });
        assert_eq!(file.strokes[0].points, [point(1, 2, 3)]);
    }

    #[test]
    fn values_out_of_range_are_errors() {
        // The first sample is a delta from the initial (0, 0, 0): x = -1.
        assert_eq!(
            decode(&bytes("62 38 62 74 08 ff")),
            Err(StrokeError::OutOfRange {
                offset: 4,
                axis: "x",
                value: -1
            })
        );
        assert_eq!(
            decode(&bytes("62 38 62 74 ff ff ff ff ff 00 00 00 00 08 01")),
            Err(StrokeError::OutOfRange {
                offset: 13,
                axis: "x",
                value: 65_536
            })
        );
    }

    #[test]
    fn velocity_resets_at_stroke_headers_but_values_persist() {
        let data = [
            SLATE_MAGIC.as_slice(),
            &bytes("ff ff ee ee ee 00 00 00 00"),
            &bytes("ff ff ff 64 00 64 00 0a 00"), // (100, 100, 10)
            &bytes("08 05"),                      // vx 5 -> (105, 100, 10)
            &bytes("00"),                         // (110, 100, 10)
            &STROKE_END_PACKET,
            &bytes("ff ff ee ee ee 00 00 00 00"),
            &bytes("00"),    // velocity reset: (110, 100, 10)
            &bytes("08 01"), // (111, 100, 10)
        ]
        .concat();
        let file = decode(&data).unwrap();
        assert_eq!(
            file.strokes[0].points,
            [
                point(100, 100, 10),
                point(105, 100, 10),
                point(110, 100, 10)
            ]
        );
        assert_eq!(
            file.strokes[1].points,
            [point(110, 100, 10), point(111, 100, 10)]
        );
    }

    #[test]
    fn encoder_uses_absent_delta_and_absolute_axes() {
        let stroke = slate_stroke(
            1000,
            vec![
                point(1000, 2000, 500),
                point(1010, 2000, 500),  // dx +10
                point(1020, 2000, 500),  // constant velocity: nothing to send
                point(1030, 2000, 500),  // idem
                point(9000, 2000, 2047), // jump: absolute x and pressure
            ],
        );
        let encoded = encode_slate(std::slice::from_ref(&stroke));
        let expected = [
            SLATE_MAGIC.as_slice(),
            &bytes("ff ff ee ee ee c8 00 00 00"), // 1000 ms = 200 ticks
            &bytes("ff ff ff e8 03 d0 07 f4 01"),
            &bytes("08 0a"),
            &bytes("00"),
            &bytes("00"),
            &bytes("cc 28 23 ff 07"),
            &STROKE_END_PACKET,
        ]
        .concat();
        assert_eq!(encoded, expected);
        round_trip(&[stroke]);
    }

    #[test]
    fn round_trip_of_varied_strokes() {
        let wave: Vec<Point> = (0..400u16)
            .map(|i| {
                let angle = f64::from(i) / 15.0;
                point(
                    15_000 + (angle.cos() * 3000.0) as u16 + i * 3,
                    10_000 + (angle.sin() * 2500.0 + 2500.0) as u16,
                    (i * 37) % 2048,
                )
            })
            .collect();
        let strokes = vec![
            slate_stroke(0, vec![point(0, 0, 0)]),
            slate_stroke(5, wave),
            slate_stroke(
                10_000,
                vec![
                    point(0, 0, 0),
                    point(29_700, 21_600, 2047),
                    point(0, 21_600, 0),
                    point(29_700, 0, 1),
                    point(29_699, 1, 2),
                ],
            ),
            slate_stroke(20_000, Vec::new()),
            Stroke {
                lost_points: 70_000,
                ..slate_stroke(25_000, vec![point(5, 5, 5), point(6, 7, 8)])
            },
            slate_stroke(
                327_675,
                (0..=2047).map(|p| point(12_000, 8_000, p)).collect(),
            ),
            slate_stroke(
                60,
                (0..=2047)
                    .rev()
                    .map(|p| point(12_000 + p, 8_000, p))
                    .collect(),
            ),
        ];
        round_trip(&strokes);
    }

    #[test]
    fn round_trip_of_many_strokes() {
        let strokes: Vec<Stroke> = (0..1000u32)
            .map(|i| {
                let base = u16::try_from(i % 200).unwrap() * 100;
                slate_stroke(
                    i * 5,
                    (0..u16::try_from(i % 7 + 1).unwrap())
                        .map(|j| point(base + j * 3, base / 2 + j, 300 + j * 50))
                        .collect(),
                )
            })
            .collect();
        let decoded = round_trip(&strokes);
        assert_eq!(decoded.strokes.len(), 1000);
    }

    #[test]
    fn time_offsets_wrap_and_unrepresentable_sample() {
        // 327 680 ms is 65 536 ticks: the counter wraps to 0.
        let encoded = encode_slate(&[slate_stroke(327_680, vec![point(1, 1, 1)])]);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(
            decoded.strokes[0].time,
            StrokeTime::SincePowerUp { millis: 0 }
        );

        let max = point(u16::MAX, u16::MAX, u16::MAX);
        let encoded = encode_slate(&[slate_stroke(0, vec![max, point(1, 1, 1), max])]);
        let decoded = decode(&encoded).unwrap();
        let stored = point(u16::MAX, u16::MAX, u16::MAX - 1);
        assert_eq!(decoded.strokes[0].points, [stored, point(1, 1, 1), stored]);
    }
}
