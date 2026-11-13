//! The 7-byte records of the blackboard's stream characteristic (`6e400004-…`): live pen
//! samples, and the content of a stored page (research note §3, "Live samples" and "Import of
//! the stored pages": a page is, byte for byte, the records the live stream carried).
//!
//! A record is `status, x (u16), y (u16), pressure (u16)`, little-endian. `a1`: the pen
//! touches (pressure 1 to 2047); `a0`: it hovers (pressure 0); `c0`: the last record before it
//! leaves the tablet's range. A stroke is a run of `a1` records. Records carry no time: a page
//! is timed by [`RECORDS_PER_SECOND`] and [`OUT_OF_RANGE_PAUSE_MS`], both approximations.

use crate::live::LiveRecord;
use crate::stroke::Point;

use super::{BOARD_MAX_PRESSURE, MAX_CHUNK, MAX_LIVE_RECORDS};

/// Size of one record.
pub const RECORD_LEN: usize = 7;
/// Status of a record while the pen touches the tablet.
pub const TOUCH: u8 = 0xa1;
/// Status of a record while the pen hovers.
pub const HOVER: u8 = 0xa0;
/// Status of the last record before the pen leaves the tablet's range.
pub const OUT: u8 = 0xc0;

/// Records the tablet sends per second while the pen is in range. **Measured, rounded**: dense
/// runs of live notifications (less than 80 ms apart) carry about 112 records per second in the
/// PC's capture and 125 in Mi Home's (research note §3 says "about 120"). Used to time the
/// records of a stored page, which carry no time.
pub const RECORDS_PER_SECOND: u32 = 120;
/// Time added after a `c0` record in a stored page, when the pen left the tablet's range: the
/// page does not say how long it stayed away. **Supposed** (half a second).
pub const OUT_OF_RANGE_PAUSE_MS: u32 = 500;

/// What a record says about the pen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordStatus {
    /// `a1`: the pen touches.
    Touch,
    /// `a0`: the pen hovers.
    Hover,
    /// `c0`: the last record before the pen leaves the range.
    Out,
    /// Any other status byte: kept, never drawn.
    Unknown(u8),
}

impl RecordStatus {
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            TOUCH => Self::Touch,
            HOVER => Self::Hover,
            OUT => Self::Out,
            other => Self::Unknown(other),
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Self::Touch => TOUCH,
            Self::Hover => HOVER,
            Self::Out => OUT,
            Self::Unknown(byte) => byte,
        }
    }
}

/// One pen record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Record {
    pub status: RecordStatus,
    pub x: u16,
    pub y: u16,
    pub pressure: u16,
}

impl Record {
    pub fn from_bytes(bytes: &[u8; RECORD_LEN]) -> Self {
        let [status, x0, x1, y0, y1, p0, p1] = *bytes;
        Self {
            status: RecordStatus::from_byte(status),
            x: u16::from_le_bytes([x0, x1]),
            y: u16::from_le_bytes([y0, y1]),
            pressure: u16::from_le_bytes([p0, p1]),
        }
    }

    pub fn to_bytes(&self) -> [u8; RECORD_LEN] {
        let [x0, x1] = self.x.to_le_bytes();
        let [y0, y1] = self.y.to_le_bytes();
        let [p0, p1] = self.pressure.to_le_bytes();
        [self.status.byte(), x0, x1, y0, y1, p0, p1]
    }

    /// The record as the live hub takes it (the Slate's [`LiveRecord`]): a touching record with
    /// its pressure (at least 1, at most 2047), a hovering one with pressure 0 (the laser
    /// pointer), "pen out of range" for `c0`. An unknown status gives nothing.
    pub fn to_live(&self) -> Option<LiveRecord> {
        let point = |pressure| {
            LiveRecord::Pen(Point {
                x: self.x,
                y: self.y,
                pressure,
            })
        };
        match self.status {
            RecordStatus::Touch => Some(point(self.pressure.clamp(1, BOARD_MAX_PRESSURE))),
            RecordStatus::Hover => Some(point(0)),
            RecordStatus::Out => Some(LiveRecord::ProximityOut),
            RecordStatus::Unknown(_) => None,
        }
    }
}

/// Records read from bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedRecords {
    pub records: Vec<Record>,
    /// Bytes after the last whole record (ignored).
    pub trailing: usize,
}

/// Reads whole records; the bytes of a partial last record are counted, not read.
pub fn decode_records(bytes: &[u8]) -> DecodedRecords {
    let (records, rest) = bytes.as_chunks::<RECORD_LEN>();
    DecodedRecords {
        records: records.iter().map(Record::from_bytes).collect(),
        trailing: rest.len(),
    }
}

/// What a received page is made of, in counts only ([`check_page`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageShape {
    /// Whole records.
    pub records: usize,
    /// Whole records whose status is not `a1`, `a0` or `c0`.
    pub unknown_status: usize,
    /// Bytes after the last whole record.
    pub trailing: usize,
}

impl PageShape {
    /// Whole records of a known status only, as every page of the captures is (research note
    /// §3: every size a multiple of 7; statuses `a1`, `a0`, `c0`). Since x, y and pressure never
    /// reach `a0` in their high byte (largest values seen: `4f`, `69`, `07`), a stream cut or
    /// joined at another offset than a multiple of 7 fails this check with near certainty.
    pub fn is_whole(&self) -> bool {
        self.unknown_status == 0 && self.trailing == 0
    }
}

impl std::fmt::Display for PageShape {
    /// Counts only, never the bytes (a page is the user's handwriting).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} records, {} of an unknown status, {} bytes of a partial record",
            self.records, self.unknown_status, self.trailing
        )
    }
}

/// The shape of a page's bytes: is it whole records of a known status? A page is accepted
/// (kept, then deleted from the tablet) only when [`PageShape::is_whole`].
pub fn check_page(bytes: &[u8]) -> PageShape {
    let (records, rest) = bytes.as_chunks::<RECORD_LEN>();
    PageShape {
        records: records.len(),
        unknown_status: records
            .iter()
            .filter(|record| ![TOUCH, HOVER, OUT].contains(&record[0]))
            .count(),
        trailing: rest.len(),
    }
}

/// The bytes of `records`, as a page stores them.
pub fn encode_records(records: &[Record]) -> Vec<u8> {
    records.iter().flat_map(Record::to_bytes).collect()
}

/// `records` as live notifications of at most [`MAX_LIVE_RECORDS`] records (the simulator).
pub fn encode_live(records: &[Record]) -> Vec<Vec<u8>> {
    records
        .chunks(MAX_LIVE_RECORDS)
        .map(encode_records)
        .collect()
}

/// A page's bytes cut into notifications of at most [`MAX_CHUNK`] bytes, as the tablet sends
/// them during a download (the simulator). Chunks cut records: only the whole page decodes.
pub fn split_chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes.chunks(MAX_CHUNK).map(<[u8]>::to_vec).collect()
}

/// One point of a stroke of a stored page, timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedPoint {
    pub x: u16,
    pub y: u16,
    /// 1..=2047 as sent (0 never occurs in a touching record of the captures).
    pub pressure: u16,
    /// Milliseconds since the first record of the page ([`record_time_ms`]).
    pub t: u32,
}

/// A run of touching records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageStroke {
    pub points: Vec<TimedPoint>,
}

/// A stored page read as strokes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedPage {
    pub strokes: Vec<PageStroke>,
    /// Whole records in the page, every status included.
    pub records: usize,
    /// Records with a status other than `a1`, `a0` and `c0` (skipped).
    pub unknown_records: usize,
    /// Bytes after the last whole record (ignored; never seen: every page size is a multiple
    /// of 7).
    pub trailing_bytes: usize,
    /// Time of the last record, in milliseconds.
    pub duration_ms: u32,
}

impl DecodedPage {
    pub fn point_count(&self) -> usize {
        self.strokes.iter().map(|s| s.points.len()).sum()
    }
}

/// Time of a record in a stored page: one period of [`RECORDS_PER_SECOND`] per record before
/// it (hovering and unknown ones included), plus [`OUT_OF_RANGE_PAUSE_MS`] per `c0` record
/// before it.
pub fn record_time_ms(records_before: u64, outs_before: u64) -> u32 {
    let ms = records_before * 1000 / u64::from(RECORDS_PER_SECOND)
        + outs_before * u64::from(OUT_OF_RANGE_PAUSE_MS);
    u32::try_from(ms).unwrap_or(u32::MAX)
}

/// Reads a stored page: its strokes (runs of `a1` records), timed by [`record_time_ms`].
/// Never fails: unknown records are counted and skipped, a partial last record is ignored.
pub fn decode_page(bytes: &[u8]) -> DecodedPage {
    let decoded = decode_records(bytes);
    let mut page = DecodedPage {
        records: decoded.records.len(),
        trailing_bytes: decoded.trailing,
        ..DecodedPage::default()
    };
    let mut current: Option<PageStroke> = None;
    let mut outs = 0u64;
    for (index, record) in decoded.records.iter().enumerate() {
        let t = record_time_ms(index as u64, outs);
        page.duration_ms = t;
        match record.status {
            RecordStatus::Touch => {
                current
                    .get_or_insert_with(PageStroke::default)
                    .points
                    .push(TimedPoint {
                        x: record.x,
                        y: record.y,
                        pressure: record.pressure,
                        t,
                    })
            }
            RecordStatus::Hover | RecordStatus::Out => {
                if let Some(stroke) = current.take() {
                    page.strokes.push(stroke);
                }
                if record.status == RecordStatus::Out {
                    outs += 1;
                }
            }
            RecordStatus::Unknown(_) => page.unknown_records += 1,
        }
    }
    if let Some(stroke) = current {
        page.strokes.push(stroke);
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(status: RecordStatus, x: u16, y: u16, pressure: u16) -> Record {
        Record {
            status,
            x,
            y,
            pressure,
        }
    }

    #[test]
    fn a_page_is_whole_records_of_a_known_status_or_refused() {
        let page = encode_records(&[
            rec(RecordStatus::Hover, 0x4f00, 0x6900, 0),
            rec(RecordStatus::Touch, 20_401, 27_067, 2047),
            rec(RecordStatus::Out, 1, 2, 0),
        ]);
        let shape = check_page(&page);
        assert!(shape.is_whole());
        assert_eq!(shape.records, 3);
        assert!(
            check_page(&[]).is_whole(),
            "an empty page is whole (0 records)"
        );
        // A stream joined at another offset than a multiple of 7: misaligned statuses.
        for cut in 1..RECORD_LEN {
            let mut rotated = page[cut..].to_vec();
            rotated.extend_from_slice(&page[..cut]);
            assert!(!check_page(&rotated).is_whole(), "rotation by {cut}");
        }
        let short = check_page(&page[..20]);
        assert_eq!((shape.records, short.records, short.trailing), (3, 2, 6));
        assert!(!short.is_whole());
        let mut unknown = page.clone();
        unknown[7] = 0xb7;
        assert_eq!(check_page(&unknown).unknown_status, 1);
        // Counts only in the text.
        assert_eq!(
            check_page(&unknown).to_string(),
            "3 records, 1 of an unknown status, 0 bytes of a partial record"
        );
    }

    #[test]
    fn records_are_little_endian_and_round_trip() {
        let bytes = [0xa1, 0x10, 0x27, 0x20, 0x4e, 0x00, 0x04];
        let record = Record::from_bytes(&bytes);
        assert_eq!(record, rec(RecordStatus::Touch, 10_000, 20_000, 1024));
        assert_eq!(record.to_bytes(), bytes);
        let unknown = Record::from_bytes(&[0xb7, 1, 0, 2, 0, 3, 0]);
        assert_eq!(unknown.status, RecordStatus::Unknown(0xb7));
        assert_eq!(unknown.to_bytes(), [0xb7, 1, 0, 2, 0, 3, 0]);
    }

    #[test]
    fn records_map_to_the_live_hub() {
        let touch = rec(RecordStatus::Touch, 5, 6, 3000);
        assert_eq!(
            touch.to_live(),
            Some(LiveRecord::Pen(Point {
                x: 5,
                y: 6,
                pressure: 2047
            }))
        );
        let light = rec(RecordStatus::Touch, 5, 6, 0).to_live();
        assert!(matches!(
            light,
            Some(LiveRecord::Pen(Point { pressure: 1, .. }))
        ));
        let hover = rec(RecordStatus::Hover, 5, 6, 9).to_live();
        assert!(matches!(
            hover,
            Some(LiveRecord::Pen(Point { pressure: 0, .. }))
        ));
        assert_eq!(
            rec(RecordStatus::Out, 5, 6, 0).to_live(),
            Some(LiveRecord::ProximityOut)
        );
        assert_eq!(rec(RecordStatus::Unknown(0x55), 5, 6, 0).to_live(), None);
    }

    #[test]
    fn a_page_becomes_timed_strokes() {
        let records = [
            rec(RecordStatus::Hover, 1, 1, 0),
            rec(RecordStatus::Touch, 2, 2, 100),
            rec(RecordStatus::Touch, 3, 3, 200),
            rec(RecordStatus::Unknown(0x99), 0, 0, 0),
            rec(RecordStatus::Touch, 4, 4, 300),
            rec(RecordStatus::Out, 4, 4, 0),
            rec(RecordStatus::Hover, 9, 9, 0),
            rec(RecordStatus::Touch, 10, 10, 400),
        ];
        let mut bytes = encode_records(&records);
        bytes.extend_from_slice(&[0xa1, 0x00]);
        let page = decode_page(&bytes);
        assert_eq!(page.records, 8);
        assert_eq!(page.unknown_records, 1);
        assert_eq!(page.trailing_bytes, 2);
        assert_eq!(page.strokes.len(), 2);
        assert_eq!(page.point_count(), 4);
        let times: Vec<u32> = page.strokes[0].points.iter().map(|p| p.t).collect();
        // Records 1, 2 and 4 at 120 per second; the unknown record keeps its slot.
        assert_eq!(times, [8, 16, 33]);
        // After the `c0` (record 5), half a second more: record 7 at 58 + 500 ms.
        assert_eq!(page.strokes[1].points[0].t, 58 + 500);
        assert_eq!(page.duration_ms, 558);
        assert_eq!(decode_page(&[]), DecodedPage::default());
    }

    #[test]
    fn notifications_and_chunks_keep_the_bytes() {
        let records: Vec<Record> = (0..14)
            .map(|i| rec(RecordStatus::Touch, i, i, 10))
            .collect();
        let live = encode_live(&records);
        assert_eq!(live.iter().map(Vec::len).collect::<Vec<_>>(), [42, 42, 14]);
        let bytes = encode_records(&records);
        assert_eq!(live.concat(), bytes);
        let page: Vec<u8> = (0..600u32).map(|i| i as u8).collect();
        let chunks = split_chunks(&page);
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            [244, 244, 112]
        );
        assert_eq!(chunks.concat(), page);
        assert_eq!(decode_records(&bytes).records, records);
    }
}
