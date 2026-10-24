//! `strokes.bin`: the compact binary encoding of a note's strokes.
//!
//! Layout (all multi-byte integers are LEB128 varints, signed values are zigzag-encoded;
//! the byte-by-byte description is in `docs/file-format.md`):
//!
//! ```text
//! magic "ASTK" | version u8 = 1 | stroke count
//! per stroke:  tool u8 | colour r g b a | width f32 LE | layer | point count
//! per point:   Δx | Δy | Δpressure | Δt      (zigzag varints)
//! ```
//!
//! Each point is stored as the difference from the previous point of the stream (the first
//! one from zero), in fixed point: coordinates in 1/100 device unit, pressure in 1/65535,
//! time in exact milliseconds. Decoding then re-encoding a decoded stream gives back the same
//! values exactly, so saving a note again and again never makes its points drift.

use ardoise_ink::{Point, Rgba, Stroke, Tool};

/// First bytes of every `strokes.bin`.
pub const MAGIC: [u8; 4] = *b"ASTK";
/// Version of the `strokes.bin` layout written by this code.
pub const VERSION: u8 = 1;
/// Coordinates are stored as integers in 1/`COORD_SCALE` device unit.
pub const COORD_SCALE: f64 = 100.0;
/// Pressures (0..=1) are stored as integers in 1/`PRESSURE_SCALE`.
pub const PRESSURE_SCALE: f64 = 65535.0;
/// Largest absolute coordinate that can be stored, in device units (10 km).
pub const MAX_COORD: f32 = 1.0e9;

/// `MAX_COORD` in stored units.
const MAX_COORD_Q: i64 = 100_000_000_000;
const MAX_PRESSURE_Q: i64 = 65_535;
/// Smallest possible stroke record: tool, colour, width, a one-byte layer and point count.
const MIN_STROKE_BYTES: usize = 1 + 4 + 4 + 1 + 1;
/// Smallest possible point record: four one-byte varints.
const MIN_POINT_BYTES: usize = 4;
/// A u64 varint never needs more than 10 bytes.
const MAX_VARINT_BYTES: usize = 10;

/// A `strokes.bin` stream that cannot be decoded, or strokes that cannot be encoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StrokesError {
    /// The stream does not start with [`MAGIC`].
    #[error("not a strokes stream (wrong magic bytes)")]
    BadMagic,
    /// The layout version is unknown to this version of Ardoise.
    #[error("strokes layout version {0} is not supported by this version of Ardoise")]
    UnsupportedVersion(u8),
    /// The stream ends in the middle of a record.
    #[error("the strokes data is truncated")]
    Truncated,
    /// A varint is longer than 10 bytes or overflows 64 bits.
    #[error("malformed number at byte {offset}")]
    BadVarint {
        /// Offset of the first byte of the number.
        offset: usize,
    },
    /// A tool code this version does not know.
    #[error("stroke {stroke}: unknown tool code {code}")]
    UnknownTool {
        /// Index of the stroke.
        stroke: usize,
        /// The code found.
        code: u8,
    },
    /// A count larger than the remaining data could possibly hold (corrupt or hostile file).
    #[error("{what} count {count} at byte {offset} exceeds the data available")]
    CountTooLarge {
        /// What is being counted ("stroke" or "point").
        what: &'static str,
        /// The count found.
        count: u64,
        /// Offset of the count.
        offset: usize,
    },
    /// A stroke-level value is invalid.
    #[error("stroke {stroke}: {reason}")]
    InvalidStroke {
        /// Index of the stroke.
        stroke: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// A point value is invalid.
    #[error("stroke {stroke}, point {point}: {reason}")]
    InvalidPoint {
        /// Index of the stroke.
        stroke: usize,
        /// Index of the point in the stroke.
        point: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// Bytes are left after the last stroke.
    #[error("{0} unexpected bytes after the last stroke")]
    TrailingBytes(usize),
}

/// Code of each tool in the stream. Codes are never reused.
pub const fn tool_code(tool: Tool) -> u8 {
    match tool {
        Tool::Ballpoint => 0,
        Tool::FountainPen => 1,
        Tool::FeltTip => 2,
        Tool::Pencil => 3,
        Tool::Brush => 4,
        Tool::Highlighter => 5,
        Tool::Chalk => 6,
    }
}

/// Tool of a code, if known.
pub const fn tool_from_code(code: u8) -> Option<Tool> {
    Some(match code {
        0 => Tool::Ballpoint,
        1 => Tool::FountainPen,
        2 => Tool::FeltTip,
        3 => Tool::Pencil,
        4 => Tool::Brush,
        5 => Tool::Highlighter,
        6 => Tool::Chalk,
        _ => return None,
    })
}

/// A point in stored units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Quantized {
    x: i64,
    y: i64,
    pressure: i64,
    t: i64,
}

/// Quantises one coordinate: finite, within ±[`MAX_COORD`], rounded to 1/100 unit.
fn quantize_coord(value: f32) -> Result<i64, &'static str> {
    if !value.is_finite() {
        return Err("coordinate is not a finite number");
    }
    if value.abs() > MAX_COORD {
        return Err("coordinate is out of range");
    }
    // |value * 100| <= 1e11: exact enough in f64 and far from the i64 limits.
    Ok((f64::from(value) * COORD_SCALE).round() as i64)
}

fn dequantize_coord(value: i64) -> f32 {
    (value as f64 / COORD_SCALE) as f32
}

/// Quantises a pressure: finite, clamped to 0..=1, rounded to 1/65535.
fn quantize_pressure(value: f32) -> Result<i64, &'static str> {
    if !value.is_finite() {
        return Err("pressure is not a finite number");
    }
    Ok((f64::from(value.clamp(0.0, 1.0)) * PRESSURE_SCALE).round() as i64)
}

fn dequantize_pressure(value: i64) -> f32 {
    (value as f64 / PRESSURE_SCALE) as f32
}

fn quantize(point: &Point) -> Result<Quantized, &'static str> {
    Ok(Quantized {
        x: quantize_coord(point.x)?,
        y: quantize_coord(point.y)?,
        pressure: quantize_pressure(point.pressure)?,
        t: i64::from(point.t),
    })
}

fn check_width(width: f32) -> Result<(), &'static str> {
    if width.is_finite() && width >= 0.0 {
        Ok(())
    } else {
        Err("width must be a finite number >= 0")
    }
}

/// What a stored point becomes once decoded: the value a save/load cycle yields.
///
/// Useful to compare a note with its reloaded copy.
pub fn round_trip_point(point: &Point) -> Option<Point> {
    let q = quantize(point).ok()?;
    Some(Point {
        x: dequantize_coord(q.x),
        y: dequantize_coord(q.y),
        pressure: dequantize_pressure(q.pressure),
        t: point.t,
    })
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

const fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

const fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

/// Encodes strokes. Fails (without writing anything) if a point is not finite, a coordinate
/// is beyond ±[`MAX_COORD`], or a width is negative or not finite. Pressures are clamped to
/// 0..=1.
pub fn encode(strokes: &[Stroke]) -> Result<Vec<u8>, StrokesError> {
    let points: usize = strokes.iter().map(|s| s.points.len()).sum();
    let mut out = Vec::with_capacity(16 + strokes.len() * 16 + points * 6);
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    put_varint(&mut out, strokes.len() as u64);
    let mut previous = Quantized::default();
    for (stroke_index, stroke) in strokes.iter().enumerate() {
        check_width(stroke.width).map_err(|reason| StrokesError::InvalidStroke {
            stroke: stroke_index,
            reason,
        })?;
        out.push(tool_code(stroke.tool));
        out.extend_from_slice(&[
            stroke.color.r,
            stroke.color.g,
            stroke.color.b,
            stroke.color.a,
        ]);
        out.extend_from_slice(&stroke.width.to_le_bytes());
        put_varint(&mut out, u64::from(stroke.layer));
        put_varint(&mut out, stroke.points.len() as u64);
        for (point_index, point) in stroke.points.iter().enumerate() {
            let q = quantize(point).map_err(|reason| StrokesError::InvalidPoint {
                stroke: stroke_index,
                point: point_index,
                reason,
            })?;
            // All quantised values are within ±1e11 or 0..=u32::MAX: no overflow.
            put_varint(&mut out, zigzag(q.x - previous.x));
            put_varint(&mut out, zigzag(q.y - previous.y));
            put_varint(&mut out, zigzag(q.pressure - previous.pressure));
            put_varint(&mut out, zigzag(q.t - previous.t));
            previous = q;
        }
    }
    Ok(out)
}

/// Bounds-checked reader over the stream.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], StrokesError> {
        let end = self.pos.checked_add(n).ok_or(StrokesError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(StrokesError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, StrokesError> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> Result<u64, StrokesError> {
        let offset = self.pos;
        let mut value = 0u64;
        for index in 0..MAX_VARINT_BYTES {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            // The tenth byte may only carry the 64th bit.
            if index == MAX_VARINT_BYTES - 1 && bits > 1 {
                return Err(StrokesError::BadVarint { offset });
            }
            value |= bits << (7 * index);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(StrokesError::BadVarint { offset })
    }

    fn zigzag(&mut self) -> Result<i64, StrokesError> {
        self.varint().map(unzigzag)
    }

    /// Reads a count and checks that the remaining data can hold that many records.
    fn count(&mut self, what: &'static str, min_record: usize) -> Result<usize, StrokesError> {
        let offset = self.pos;
        let count = self.varint()?;
        let fits = usize::try_from(count)
            .ok()
            .filter(|&n| n <= self.remaining() / min_record);
        fits.ok_or(StrokesError::CountTooLarge {
            what,
            count,
            offset,
        })
    }
}

/// Adds a decoded delta to a running value and checks the result stays within `min..=max`.
fn apply_delta(value: i64, delta: i64, min: i64, max: i64) -> Option<i64> {
    value.checked_add(delta).filter(|v| (min..=max).contains(v))
}

/// Decodes a `strokes.bin` stream. Never panics; memory use is bounded by the input size.
pub fn decode(bytes: &[u8]) -> Result<Vec<Stroke>, StrokesError> {
    let mut reader = Reader { bytes, pos: 0 };
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(StrokesError::BadMagic);
    }
    let version = reader.byte()?;
    if version != VERSION {
        return Err(StrokesError::UnsupportedVersion(version));
    }
    let stroke_count = reader.count("stroke", MIN_STROKE_BYTES)?;
    let mut strokes = Vec::with_capacity(stroke_count);
    let mut previous = Quantized::default();
    for stroke_index in 0..stroke_count {
        let code = reader.byte()?;
        let tool = tool_from_code(code).ok_or(StrokesError::UnknownTool {
            stroke: stroke_index,
            code,
        })?;
        let rgba = reader.take(4)?;
        let color = Rgba {
            r: rgba[0],
            g: rgba[1],
            b: rgba[2],
            a: rgba[3],
        };
        let width_bytes = reader.take(4)?;
        let width = f32::from_le_bytes([
            width_bytes[0],
            width_bytes[1],
            width_bytes[2],
            width_bytes[3],
        ]);
        let invalid_stroke = |reason| StrokesError::InvalidStroke {
            stroke: stroke_index,
            reason,
        };
        check_width(width).map_err(invalid_stroke)?;
        let layer = u32::try_from(reader.varint()?)
            .map_err(|_| invalid_stroke("layer index is out of range"))?;
        let point_count = reader.count("point", MIN_POINT_BYTES)?;
        let mut points = Vec::with_capacity(point_count);
        for point_index in 0..point_count {
            let invalid_point = |reason| StrokesError::InvalidPoint {
                stroke: stroke_index,
                point: point_index,
                reason,
            };
            let (dx, dy, dp, dt) = (
                reader.zigzag()?,
                reader.zigzag()?,
                reader.zigzag()?,
                reader.zigzag()?,
            );
            let q = Quantized {
                x: apply_delta(previous.x, dx, -MAX_COORD_Q, MAX_COORD_Q)
                    .ok_or_else(|| invalid_point("coordinate is out of range"))?,
                y: apply_delta(previous.y, dy, -MAX_COORD_Q, MAX_COORD_Q)
                    .ok_or_else(|| invalid_point("coordinate is out of range"))?,
                pressure: apply_delta(previous.pressure, dp, 0, MAX_PRESSURE_Q)
                    .ok_or_else(|| invalid_point("pressure is out of range"))?,
                t: apply_delta(previous.t, dt, 0, i64::from(u32::MAX))
                    .ok_or_else(|| invalid_point("time is out of range"))?,
            };
            points.push(Point {
                x: dequantize_coord(q.x),
                y: dequantize_coord(q.y),
                pressure: dequantize_pressure(q.pressure),
                // Checked against the u32 range just above.
                t: u32::try_from(q.t).map_err(|_| invalid_point("time is out of range"))?,
            });
            previous = q;
        }
        strokes.push(Stroke {
            tool,
            color,
            width,
            layer,
            points,
        });
    }
    match reader.remaining() {
        0 => Ok(strokes),
        n => Err(StrokesError::TrailingBytes(n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Lcg;

    fn stroke(points: Vec<Point>) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points,
        }
    }

    fn point(x: f32, y: f32, pressure: f32, t: u32) -> Point {
        Point { x, y, pressure, t }
    }

    /// Largest difference allowed between a coordinate and its decoded value: half the
    /// fixed-point step plus half an f32 step at that magnitude.
    fn coord_tolerance(value: f32) -> f64 {
        0.5 / COORD_SCALE + f64::from(value.abs()) * f64::from(f32::EPSILON)
    }

    fn assert_close(original: &[Stroke], decoded: &[Stroke]) {
        assert_eq!(original.len(), decoded.len());
        for (a, b) in original.iter().zip(decoded) {
            assert_eq!(a.tool, b.tool);
            assert_eq!(a.color, b.color);
            assert_eq!(a.width.to_bits(), b.width.to_bits());
            assert_eq!(a.layer, b.layer);
            assert_eq!(a.points.len(), b.points.len());
            for (p, q) in a.points.iter().zip(&b.points) {
                let dx = (f64::from(p.x) - f64::from(q.x)).abs();
                let dy = (f64::from(p.y) - f64::from(q.y)).abs();
                assert!(dx <= coord_tolerance(p.x), "x {} -> {}", p.x, q.x);
                assert!(dy <= coord_tolerance(p.y), "y {} -> {}", p.y, q.y);
                let expected_pressure = f64::from(p.pressure.clamp(0.0, 1.0));
                let dp = (expected_pressure - f64::from(q.pressure)).abs();
                assert!(
                    dp <= 0.5 / PRESSURE_SCALE + 1e-7,
                    "p {} -> {}",
                    p.pressure,
                    q.pressure
                );
                assert_eq!(p.t, q.t);
            }
        }
    }

    #[test]
    fn varint_and_zigzag_round_trip() {
        for value in [0u64, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            let mut out = Vec::new();
            put_varint(&mut out, value);
            let mut reader = Reader {
                bytes: &out,
                pos: 0,
            };
            assert_eq!(reader.varint().unwrap(), value);
            assert_eq!(reader.remaining(), 0);
        }
        for value in [0i64, -1, 1, -64, 64, i64::MIN, i64::MAX] {
            assert_eq!(unzigzag(zigzag(value)), value);
        }
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
    }

    #[test]
    fn empty_stream_is_header_only() {
        let bytes = encode(&[]).unwrap();
        assert_eq!(bytes, [b'A', b'S', b'T', b'K', 1, 0]);
        assert!(decode(&bytes).unwrap().is_empty());
    }

    #[test]
    fn documented_example_bytes() {
        // The example of docs/file-format.md, byte for byte.
        let strokes = [Stroke {
            tool: Tool::Pencil,
            color: Rgba::rgb(0x12, 0x34, 0x56),
            width: 40.0,
            layer: 1,
            points: vec![point(100.0, 200.5, 0.5, 0), point(101.25, 200.0, 1.0, 5)],
        }];
        let bytes = encode(&strokes).unwrap();
        let expected: &[u8] = &[
            b'A', b'S', b'T', b'K', 0x01, // magic, version
            0x01, // 1 stroke
            0x03, // pencil
            0x12, 0x34, 0x56, 0xFF, // colour
            0x00, 0x00, 0x20, 0x42, // width 40.0 (f32 LE)
            0x01, // layer 1
            0x02, // 2 points
            0xA0, 0x9C, 0x01, // dx = +10000 (100.00)
            0xA4, 0xB9, 0x02, // dy = +20050 (200.50)
            0x80, 0x80, 0x04, // dp = +32768 (0.5)
            0x00, // dt = 0
            0xFA, 0x01, // dx = +125
            0x63, // dy = -50
            0xFE, 0xFF, 0x03, // dp = +32767
            0x0A, // dt = +5
        ];
        assert_eq!(bytes, expected);
        assert_close(&strokes, &decode(&bytes).unwrap());
    }

    #[test]
    fn random_strokes_round_trip_within_tolerance_and_are_stable() {
        let mut rng = Lcg::new(0x5eed_0001);
        for case in 0..200 {
            let strokes = rng.strokes(case % 7, 60);
            let bytes = encode(&strokes).unwrap();
            let decoded = decode(&bytes).unwrap();
            assert_close(&strokes, &decoded);
            // A second cycle is exact: saving again never moves points.
            let again = decode(&encode(&decoded).unwrap()).unwrap();
            assert_eq!(again, decoded);
            for (s, d) in strokes.iter().zip(&decoded) {
                for (p, q) in s.points.iter().zip(&d.points) {
                    assert_eq!(round_trip_point(p), Some(*q));
                }
            }
        }
    }

    #[test]
    fn extreme_values_round_trip() {
        let values = [
            0.0f32,
            -0.0,
            0.004,
            0.005,
            -0.005,
            0.015,
            29_700.0,
            21_600.0,
            -12_345.678,
            65_535.99,
            131_071.99,
            131_072.0,
            1_000_000.07,
            16_777_217.0,
            -999_999_999.0,
            MAX_COORD,
            -MAX_COORD,
            f32::MIN_POSITIVE,
        ];
        let mut points = Vec::new();
        for (i, &x) in values.iter().enumerate() {
            let y = values[values.len() - 1 - i];
            let t = [0, 1, u32::MAX, u32::MAX / 2, 7][i % 5];
            let pressure = [0.0, 1.0, 0.5, 1.0 / 2047.0, 2046.0 / 2047.0][i % 5];
            points.push(point(x, y, pressure, t));
        }
        let strokes = [stroke(points)];
        let decoded = decode(&encode(&strokes).unwrap()).unwrap();
        assert_close(&strokes, &decoded);
        assert_eq!(decode(&encode(&decoded).unwrap()).unwrap(), decoded);
        // Values with an exact 1/100 representation in f32 come back unchanged.
        for (p, q) in strokes[0].points.iter().zip(&decoded[0].points) {
            if [0.0, 29_700.0, 21_600.0, 131_072.0, MAX_COORD, -MAX_COORD].contains(&p.x) {
                assert_eq!(p.x, q.x);
            }
        }
    }

    #[test]
    fn tablet_pressure_levels_survive_exactly() {
        // The Slate reports 0..=2047; normalised and stored, the level is recoverable.
        let points: Vec<Point> = (0..=2047u32)
            .map(|level| point(0.0, 0.0, level as f32 / 2047.0, level))
            .collect();
        let decoded = decode(&encode(&[stroke(points)]).unwrap()).unwrap();
        for (level, p) in decoded[0].points.iter().enumerate() {
            assert_eq!((p.pressure * 2047.0).round() as usize, level);
        }
    }

    #[test]
    fn out_of_range_pressure_is_clamped() {
        let strokes = [stroke(vec![
            point(1.0, 1.0, 1.5, 0),
            point(1.0, 1.0, -0.2, 1),
        ])];
        let decoded = decode(&encode(&strokes).unwrap()).unwrap();
        assert_eq!(decoded[0].points[0].pressure, 1.0);
        assert_eq!(decoded[0].points[1].pressure, 0.0);
    }

    #[test]
    fn times_may_go_backwards() {
        let strokes = [
            stroke(vec![point(0.0, 0.0, 0.5, 1000), point(0.0, 0.0, 0.5, 10)]),
            stroke(vec![point(0.0, 0.0, 0.5, 0)]),
        ];
        let decoded = decode(&encode(&strokes).unwrap()).unwrap();
        assert_eq!(decoded[0].points[1].t, 10);
        assert_eq!(decoded[1].points[0].t, 0);
    }

    #[test]
    fn invalid_values_are_refused_on_encode() {
        for (x, reason) in [
            (f32::NAN, "coordinate is not a finite number"),
            (f32::INFINITY, "coordinate is not a finite number"),
            (MAX_COORD * 1.01, "coordinate is out of range"),
        ] {
            let strokes = [stroke(vec![point(0.0, 0.0, 0.5, 0), point(x, 0.0, 0.5, 1)])];
            assert_eq!(
                encode(&strokes),
                Err(StrokesError::InvalidPoint {
                    stroke: 0,
                    point: 1,
                    reason
                })
            );
        }
        let strokes = [stroke(vec![point(0.0, 0.0, f32::NAN, 0)])];
        assert!(matches!(
            encode(&strokes),
            Err(StrokesError::InvalidPoint { .. })
        ));
        let mut bad_width = stroke(vec![]);
        bad_width.width = -1.0;
        assert!(matches!(
            encode(&[bad_width]),
            Err(StrokesError::InvalidStroke { stroke: 0, .. })
        ));
    }

    #[test]
    fn every_tool_has_a_stable_code() {
        let codes: Vec<u8> = Tool::ALL.iter().map(|&t| tool_code(t)).collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6]);
        for tool in Tool::ALL {
            assert_eq!(tool_from_code(tool_code(tool)), Some(tool));
        }
        assert_eq!(tool_from_code(7), None);
    }

    #[test]
    fn wrong_magic_and_version_are_refused() {
        assert_eq!(decode(b"PK\x03\x04\x01\x00"), Err(StrokesError::BadMagic));
        assert_eq!(
            decode(b"ASTK\x02\x00"),
            Err(StrokesError::UnsupportedVersion(2))
        );
        assert_eq!(decode(b"AST"), Err(StrokesError::Truncated));
        assert_eq!(decode(b""), Err(StrokesError::Truncated));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let mut rng = Lcg::new(42);
        let bytes = encode(&rng.strokes(5, 20)).unwrap();
        for len in 0..bytes.len() {
            assert!(decode(&bytes[..len]).is_err(), "prefix of {len} bytes");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(decode(&longer), Err(StrokesError::TrailingBytes(1)));
    }

    #[test]
    fn huge_counts_are_refused_without_allocating() {
        let mut bytes = b"ASTK\x01".to_vec();
        put_varint(&mut bytes, u64::MAX);
        assert!(matches!(
            decode(&bytes),
            Err(StrokesError::CountTooLarge { what: "stroke", .. })
        ));

        let mut bytes = b"ASTK\x01\x01\x00\x00\x00\x00\xff\x00\x00\x20\x42\x00".to_vec();
        put_varint(&mut bytes, 1 << 40);
        assert!(matches!(
            decode(&bytes),
            Err(StrokesError::CountTooLarge { what: "point", .. })
        ));
    }

    #[test]
    fn malformed_values_are_refused() {
        // 11-byte varint.
        let mut bytes = b"ASTK\x01".to_vec();
        bytes.extend_from_slice(&[0xff; 10]);
        bytes.push(0x01);
        assert_eq!(decode(&bytes), Err(StrokesError::BadVarint { offset: 5 }));

        // Unknown tool.
        let bytes = b"ASTK\x01\x01\x09\x00\x00\x00\xff\x00\x00\x20\x42\x00\x00";
        assert_eq!(
            decode(bytes),
            Err(StrokesError::UnknownTool { stroke: 0, code: 9 })
        );

        // NaN width.
        let bytes = b"ASTK\x01\x01\x00\x00\x00\x00\xff\x00\x00\xc0\x7f\x00\x00";
        assert!(matches!(
            decode(bytes),
            Err(StrokesError::InvalidStroke { stroke: 0, .. })
        ));

        // Layer beyond u32.
        let mut bytes = b"ASTK\x01\x01\x00\x00\x00\x00\xff\x00\x00\x20\x42".to_vec();
        put_varint(&mut bytes, 1 << 33);
        bytes.push(0);
        assert!(matches!(
            decode(&bytes),
            Err(StrokesError::InvalidStroke { stroke: 0, .. })
        ));

        // Pressure delta leaving 0..=65535, coordinate beyond the limit, negative time.
        for (delta_index, value, reason) in [
            (2, 65_536i64, "pressure is out of range"),
            (0, MAX_COORD_Q + 1, "coordinate is out of range"),
            (3, -1, "time is out of range"),
            (1, i64::MIN, "coordinate is out of range"),
        ] {
            let mut bytes = b"ASTK\x01\x01\x00\x00\x00\x00\xff\x00\x00\x20\x42\x00\x01".to_vec();
            for i in 0..4 {
                put_varint(&mut bytes, zigzag(if i == delta_index { value } else { 0 }));
            }
            assert_eq!(
                decode(&bytes),
                Err(StrokesError::InvalidPoint {
                    stroke: 0,
                    point: 0,
                    reason
                })
            );
        }
    }

    #[test]
    fn random_garbage_never_panics() {
        let mut rng = Lcg::new(7);
        let valid = encode(&rng.strokes(4, 30)).unwrap();
        for _ in 0..3000 {
            let mut bytes = valid.clone();
            let flips = 1 + rng.below(4) as usize;
            for _ in 0..flips {
                let at = rng.below(bytes.len() as u64) as usize;
                bytes[at] = rng.next_u32() as u8;
            }
            let _ = decode(&bytes);
        }
        for len in 0..200 {
            let bytes: Vec<u8> = (0..len).map(|_| rng.next_u32() as u8).collect();
            let _ = decode(&bytes);
            let mut with_header = b"ASTK\x01".to_vec();
            with_header.extend_from_slice(&bytes);
            let _ = decode(&with_header);
        }
    }
}
