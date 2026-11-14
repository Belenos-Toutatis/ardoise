//! Outlines with curves: what a glyph is made of, kept exact for every output (a quarter turn,
//! a move or a change of scale of a frame maps the control points, which keeps the curves).

use std::fmt::Write as _;

/// One step of a path. Points are `[x, y]`, y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    /// Quadratic Bézier curve: control point, end point (TrueType outlines).
    QuadTo([f32; 2], [f32; 2]),
    /// Cubic Bézier curve: two control points, end point (CFF outlines).
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    /// Closes the current contour.
    Close,
}

/// Closed contours to fill with the non-zero rule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    pub segments: Vec<Segment>,
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The path with every point (end and control points) through `f`. Exact for an affine `f`.
    pub fn map(&self, f: impl Fn([f32; 2]) -> [f32; 2]) -> Path {
        Path {
            segments: self
                .segments
                .iter()
                .map(|segment| match *segment {
                    Segment::MoveTo(p) => Segment::MoveTo(f(p)),
                    Segment::LineTo(p) => Segment::LineTo(f(p)),
                    Segment::QuadTo(c, p) => Segment::QuadTo(f(c), f(p)),
                    Segment::CubicTo(c1, c2, p) => Segment::CubicTo(f(c1), f(c2), f(p)),
                    Segment::Close => Segment::Close,
                })
                .collect(),
        }
    }

    /// Appends `other`'s contours.
    pub fn extend(&mut self, other: Path) {
        self.segments.extend(other.segments);
    }

    /// The same path with each quadratic curve written as the cubic curve it is (PDF has only
    /// cubic curves).
    pub fn to_cubic(&self) -> Path {
        let mut current = [0.0_f32; 2];
        let mut start = current;
        let mut segments = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            match *segment {
                Segment::MoveTo(p) => {
                    current = p;
                    start = p;
                    segments.push(*segment);
                }
                Segment::LineTo(p) => {
                    current = p;
                    segments.push(*segment);
                }
                Segment::QuadTo(c, p) => {
                    let toward = |from: [f32; 2]| {
                        [
                            from[0] + 2.0 / 3.0 * (c[0] - from[0]),
                            from[1] + 2.0 / 3.0 * (c[1] - from[1]),
                        ]
                    };
                    segments.push(Segment::CubicTo(toward(current), toward(p), p));
                    current = p;
                }
                Segment::CubicTo(_, _, p) => {
                    current = p;
                    segments.push(*segment);
                }
                Segment::Close => {
                    current = start;
                    segments.push(*segment);
                }
            }
        }
        Path { segments }
    }

    /// The box `[min x, min y, max x, max y]` of every point, control points included (it holds
    /// the outline, a little larger at most); `None` for an empty path.
    pub fn bounds(&self) -> Option<[f32; 4]> {
        let mut points = self.segments.iter().flat_map(|segment| {
            let (list, count): ([[f32; 2]; 3], usize) = match *segment {
                Segment::MoveTo(p) | Segment::LineTo(p) => ([p, p, p], 1),
                Segment::QuadTo(c, p) => ([c, p, p], 2),
                Segment::CubicTo(c1, c2, p) => ([c1, c2, p], 3),
                Segment::Close => ([[0.0; 2]; 3], 0),
            };
            list.into_iter().take(count)
        });
        let first = points.next()?;
        Some(
            points.fold([first[0], first[1], first[0], first[1]], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            }),
        )
    }

    /// SVG path data (`M`, `L`, `Q`, `C`, `Z`, absolute), numbers with at most two decimals.
    pub fn to_svg(&self) -> String {
        let mut out = String::with_capacity(self.segments.len() * 24);
        let point = |out: &mut String, p: [f32; 2]| {
            push_number(out, p[0]);
            out.push(' ');
            push_number(out, p[1]);
        };
        for segment in &self.segments {
            match *segment {
                Segment::MoveTo(p) => {
                    out.push('M');
                    point(&mut out, p);
                }
                Segment::LineTo(p) => {
                    out.push('L');
                    point(&mut out, p);
                }
                Segment::QuadTo(c, p) => {
                    out.push('Q');
                    point(&mut out, c);
                    out.push(' ');
                    point(&mut out, p);
                }
                Segment::CubicTo(c1, c2, p) => {
                    out.push('C');
                    point(&mut out, c1);
                    out.push(' ');
                    point(&mut out, c2);
                    out.push(' ');
                    point(&mut out, p);
                }
                Segment::Close => out.push('Z'),
            }
        }
        out
    }
}

/// Why path data cannot be read ([`Path::from_svg`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathDataError {
    /// More bytes than the limit.
    TooLong,
    /// Not the grammar (a command or a number expected at this byte offset).
    Syntax(usize),
    /// A coordinate beyond the limit.
    OutOfRange,
}

impl std::fmt::Display for PathDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathDataError::TooLong => write!(f, "path data too long"),
            PathDataError::Syntax(at) => write!(f, "path data not understood at byte {at}"),
            PathDataError::OutOfRange => write!(f, "a coordinate of the path is out of range"),
        }
    }
}

impl std::error::Error for PathDataError {}

/// A reader of path data, one byte at a time (the grammar is ASCII).
struct PathReader<'a> {
    bytes: &'a [u8],
    at: usize,
    max_abs: f32,
}

impl PathReader<'_> {
    /// Skips white space, and commas when `commas`.
    fn skip(&mut self, commas: bool) {
        while let Some(&b) = self.bytes.get(self.at) {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || (commas && b == b',') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    /// Whether a number starts here.
    fn number_ahead(&self) -> bool {
        matches!(
            self.bytes.get(self.at),
            Some(b'-' | b'+' | b'.' | b'0'..=b'9')
        )
    }

    /// `[-+]? (digits ('.' digits?)? | '.' digits)`: no exponent.
    fn number(&mut self) -> Result<f32, PathDataError> {
        self.skip(true);
        let start = self.at;
        if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        let digits = |reader: &mut Self| {
            let from = reader.at;
            while reader.bytes.get(reader.at).is_some_and(u8::is_ascii_digit) {
                reader.at += 1;
            }
            reader.at - from
        };
        let whole = digits(self);
        let mut fraction = 0;
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            fraction = digits(self);
        }
        if whole == 0 && fraction == 0 {
            return Err(PathDataError::Syntax(start));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| PathDataError::Syntax(start))?;
        let value: f32 = text.parse().map_err(|_| PathDataError::Syntax(start))?;
        if !value.is_finite() || value.abs() > self.max_abs {
            return Err(PathDataError::OutOfRange);
        }
        Ok(value)
    }

    fn point(&mut self) -> Result<[f32; 2], PathDataError> {
        Ok([self.number()?, self.number()?])
    }
}

impl Path {
    /// Reads SVG path data of absolute `M`, `L`, `Q`, `C` and `Z` commands (what [`Path::to_svg`]
    /// writes; as in SVG, a command may be followed by several groups of numbers, and numbers
    /// are separated by white space or commas). Strict, for data read from a file: at most
    /// `max_len` bytes, the first command an `M`, nothing but an `M` after a `Z`, no exponent,
    /// every coordinate within `±max_abs`. Linear in the length; never panics.
    pub fn from_svg(data: &str, max_len: usize, max_abs: f32) -> Result<Path, PathDataError> {
        if data.len() > max_len {
            return Err(PathDataError::TooLong);
        }
        let mut reader = PathReader {
            bytes: data.as_bytes(),
            at: 0,
            max_abs,
        };
        let mut segments = Vec::new();
        // The command whose numbers are being read, and whether a `Z` was the last one.
        let mut command: Option<u8> = None;
        loop {
            reader.skip(false);
            let Some(&b) = reader.bytes.get(reader.at) else {
                break;
            };
            let letter = if b.is_ascii_alphabetic() {
                reader.at += 1;
                Some(b)
            } else {
                None
            };
            let current = match (letter, command) {
                (Some(l), None) if l != b'M' => return Err(PathDataError::Syntax(reader.at - 1)),
                (Some(l), Some(b'Z')) if l != b'M' => {
                    return Err(PathDataError::Syntax(reader.at - 1));
                }
                (Some(l), _) => l,
                // More numbers: the same command again (`L` after an `M`); none after a `Z`.
                (None, Some(b'M')) if reader.number_ahead() => b'L',
                (None, Some(c)) if c != b'Z' && reader.number_ahead() => c,
                _ => return Err(PathDataError::Syntax(reader.at)),
            };
            match current {
                b'M' => segments.push(Segment::MoveTo(reader.point()?)),
                b'L' => segments.push(Segment::LineTo(reader.point()?)),
                b'Q' => {
                    let c = reader.point()?;
                    segments.push(Segment::QuadTo(c, reader.point()?));
                }
                b'C' => {
                    let c1 = reader.point()?;
                    let c2 = reader.point()?;
                    segments.push(Segment::CubicTo(c1, c2, reader.point()?));
                }
                b'Z' => segments.push(Segment::Close),
                _ => return Err(PathDataError::Syntax(reader.at.saturating_sub(1))),
            }
            command = Some(current);
            reader.skip(true);
        }
        Ok(Path { segments })
    }
}

/// `value` with at most two decimals, without trailing zeros (`12`, `-3.5`, `0.25`).
fn push_number(out: &mut String, value: f32) {
    let value = if value.is_finite() { value } else { 0.0 };
    let mut text = String::new();
    let _ = write!(text, "{value:.2}");
    let trimmed = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    out.push_str(if trimmed == "-0" { "0" } else { trimmed });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_with_curve() -> Path {
        Path {
            segments: vec![
                Segment::MoveTo([0.0, 0.0]),
                Segment::LineTo([10.0, 0.0]),
                Segment::QuadTo([10.0, 10.0], [0.0, 10.0]),
                Segment::Close,
            ],
        }
    }

    #[test]
    fn svg_data_is_compact() {
        let path = Path {
            segments: vec![
                Segment::MoveTo([1.0, -0.001]),
                Segment::LineTo([2.5, 3.256]),
                Segment::CubicTo([1.0, 2.0], [3.0, 4.0], [5.0, 6.0]),
                Segment::Close,
            ],
        };
        assert_eq!(path.to_svg(), "M1 0L2.5 3.26C1 2 3 4 5 6Z");
    }

    #[test]
    fn a_quadratic_curve_becomes_the_same_cubic_one() {
        let cubic = square_with_curve().to_cubic();
        let Segment::CubicTo(c1, c2, p) = cubic.segments[2] else {
            panic!("{:?}", cubic.segments[2]);
        };
        let near =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        assert!(
            near(c1, [10.0, 20.0 / 3.0]) && near(c2, [20.0 / 3.0, 10.0]),
            "{c1:?} {c2:?}"
        );
        assert_eq!(p, [0.0, 10.0]);
        assert_eq!(cubic.segments.len(), 4);
    }

    const LIMIT: usize = 1 << 16;

    #[test]
    fn svg_data_reads_back() {
        let path = Path {
            segments: vec![
                Segment::MoveTo([1.0, -2.5]),
                Segment::LineTo([2.5, 3.25]),
                Segment::QuadTo([4.0, 5.0], [6.0, 7.0]),
                Segment::CubicTo([1.0, 2.0], [3.0, 4.0], [5.0, 6.0]),
                Segment::Close,
                Segment::MoveTo([10.0, 10.0]),
                Segment::LineTo([11.0, 10.0]),
                Segment::Close,
            ],
        };
        assert_eq!(Path::from_svg(&path.to_svg(), LIMIT, 1e6), Ok(path));
        // SVG's own spellings: repeated groups, commas, white space, signs and bare fractions.
        let loose = Path::from_svg(
            " M 1,2 3 4 L5-6 .5+1 Q1 2 3 4 5 6 7 8\nC1 2 3 4 5 6 Z ",
            LIMIT,
            1e6,
        )
        .unwrap();
        assert_eq!(
            loose.segments,
            [
                Segment::MoveTo([1.0, 2.0]),
                Segment::LineTo([3.0, 4.0]),
                Segment::LineTo([5.0, -6.0]),
                Segment::LineTo([0.5, 1.0]),
                Segment::QuadTo([1.0, 2.0], [3.0, 4.0]),
                Segment::QuadTo([5.0, 6.0], [7.0, 8.0]),
                Segment::CubicTo([1.0, 2.0], [3.0, 4.0], [5.0, 6.0]),
                Segment::Close,
            ]
        );
        assert_eq!(Path::from_svg("", LIMIT, 1e6), Ok(Path::default()));
    }

    #[test]
    fn svg_data_outside_the_grammar_is_refused() {
        for bad in [
            "L0 0",
            "M0",
            "M0 0L1",
            "M0 0 Z L1 1",
            "M0 0ZZ",
            "m0 0",
            "M0 0h5",
            "M0 0A1 1 0 0 1 2 2",
            "M1e3 0",
            "M0 0Q1 1 2",
            "M- 1",
            "M. 1",
            "M0 0 #",
            "M0 0C1 2 3 4 5",
            "<script>",
        ] {
            assert!(
                matches!(
                    Path::from_svg(bad, LIMIT, 1e6),
                    Err(PathDataError::Syntax(_))
                ),
                "{bad}"
            );
        }
        assert_eq!(
            Path::from_svg("M0 2000000", LIMIT, 1e6),
            Err(PathDataError::OutOfRange)
        );
        let huge = format!("M{} 0", "9".repeat(60));
        assert_eq!(
            Path::from_svg(&huge, LIMIT, 1e6),
            Err(PathDataError::OutOfRange)
        );
        assert_eq!(
            Path::from_svg("M0 0L1 1", 7, 1e6),
            Err(PathDataError::TooLong)
        );
    }

    #[test]
    fn hostile_svg_data_never_panics() {
        // Every truncation of a valid path, and pseudo-random bytes from its alphabet.
        let valid = "M1.5 2L3 -4Q5 6 7 8C9 10 11 12 13 14ZM0 0L1 1Z";
        for end in 0..=valid.len() {
            let _ = Path::from_svg(&valid[..end], LIMIT, 1e6);
        }
        let alphabet = b"MLQCZmlqczHVA0123456789.-+, \n";
        let mut seed: u32 = 0x1234_5678;
        for _ in 0..2000 {
            let mut text = String::new();
            for _ in 0..(seed % 80) {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                text.push(char::from(alphabet[(seed >> 24) as usize % alphabet.len()]));
            }
            if let Ok(path) = Path::from_svg(&text, LIMIT, 1e6) {
                assert!(path.segments.len() <= text.len());
            }
        }
        // A long run of digits is read in one pass and refused, not parsed for ever.
        let long = format!("M0 0L{} 0", "1".repeat(LIMIT - 10));
        assert!(Path::from_svg(&long, LIMIT, 1e6).is_err());
    }

    #[test]
    fn bounds_and_map() {
        let path = square_with_curve();
        assert_eq!(path.bounds(), Some([0.0, 0.0, 10.0, 10.0]));
        let moved = path.map(|[x, y]| [x * 2.0 + 1.0, y + 5.0]);
        assert_eq!(moved.bounds(), Some([1.0, 5.0, 21.0, 15.0]));
        assert_eq!(Path::default().bounds(), None);
    }
}
