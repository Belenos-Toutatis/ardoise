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

    #[test]
    fn bounds_and_map() {
        let path = square_with_curve();
        assert_eq!(path.bounds(), Some([0.0, 0.0, 10.0, 10.0]));
        let moved = path.map(|[x, y]| [x * 2.0 + 1.0, y + 5.0]);
        assert_eq!(moved.bounds(), Some([1.0, 5.0, 21.0, 15.0]));
        assert_eq!(Path::default().bounds(), None);
    }
}
