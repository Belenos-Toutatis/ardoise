//! Handwriting-like single-stroke glyphs: synthetic notes whose text is known, publishable
//! (drawn by this module, never taken from anyone's notes). They serve the reference test of
//! the recognition and the demo library.
//!
//! The glyphs are print lowercase letters, mostly one stroke each as a hand writes them (the
//! dots of `i` and `j` and the bar of `t` and `f` are strokes of their own), drawn here by hand
//! from lines and elliptic arcs (no font crate was suitable: the Hershey fonts found on
//! crates.io have no accents and no clear licence for the code). French accents (é è ê ë à â
//! î ï ô ù û ç) are added as small strokes over (or under) the letter. Every glyph gets a
//! little random variation (size, baseline, wobble, slant) from a seed, so two notes written
//! with different seeds differ like two samples of the same hand. Points are spaced as the
//! tablet samples them (200 Hz at a writing speed), with times and a pressure curve.

use ardoise_ink::{Point, Rgba, Stroke, Tool};
use unicode_normalization::UnicodeNormalization;

/// One part of a stroke, in glyph units: baseline at y = 0, x-height 1, y up.
#[derive(Debug, Clone, Copy)]
enum Seg {
    /// Start of a stroke.
    M(f32, f32),
    /// Straight to a point.
    L(f32, f32),
    /// Elliptic arc (centre x, centre y, radius x, radius y, from and to angles in degrees,
    /// counter-clockwise), joined to the previous point.
    A(f32, f32, f32, f32, f32, f32),
}

use Seg::{A, L, M};

/// A letter: its width and its strokes, plus where its dot goes (`i`, `j`).
struct Glyph {
    advance: f32,
    strokes: &'static [&'static [Seg]],
    dot: Option<(f32, f32)>,
}

const fn glyph(advance: f32, strokes: &'static [&'static [Seg]]) -> Glyph {
    Glyph {
        advance,
        strokes,
        dot: None,
    }
}

/// The bowl of `a`, `d`, `g`, `q`: from the upper right, counter-clockwise, round.
const BOWL: Seg = A(0.3, 0.5, 0.27, 0.5, 35.0, 385.0);

fn base_glyph(c: char) -> Option<Glyph> {
    Some(match c {
        'a' => glyph(0.62, &[&[BOWL, L(0.58, 1.0), L(0.6, 0.0)]]),
        'b' => glyph(
            0.6,
            &[&[
                M(0.06, 1.75),
                L(0.06, 0.0),
                L(0.06, 0.6),
                A(0.32, 0.5, 0.26, 0.5, 160.0, -175.0),
            ]],
        ),
        'c' => glyph(0.52, &[&[A(0.29, 0.5, 0.26, 0.5, 40.0, 320.0)]]),
        'd' => glyph(0.62, &[&[BOWL, L(0.58, 1.75), L(0.6, 0.0)]]),
        'e' => glyph(
            0.56,
            &[&[
                M(0.04, 0.5),
                L(0.54, 0.52),
                A(0.29, 0.5, 0.26, 0.5, 5.0, 320.0),
            ]],
        ),
        'f' => glyph(
            0.45,
            &[
                &[A(0.38, 1.5, 0.17, 0.22, 20.0, 180.0), L(0.21, 0.0)],
                &[M(0.02, 1.0), L(0.44, 1.0)],
            ],
        ),
        'g' => glyph(
            0.62,
            &[&[
                BOWL,
                L(0.58, 1.0),
                L(0.58, -0.45),
                A(0.33, -0.45, 0.25, 0.3, 0.0, -170.0),
            ]],
        ),
        'h' => glyph(
            0.6,
            &[&[
                M(0.06, 1.75),
                L(0.06, 0.0),
                L(0.06, 0.55),
                A(0.31, 0.55, 0.25, 0.42, 170.0, 0.0),
                L(0.56, 0.0),
            ]],
        ),
        'i' => Glyph {
            advance: 0.22,
            strokes: &[&[M(0.11, 1.0), L(0.11, 0.0)]],
            dot: Some((0.11, 1.4)),
        },
        'j' => Glyph {
            advance: 0.36,
            strokes: &[&[
                M(0.26, 1.0),
                L(0.26, -0.45),
                A(0.08, -0.45, 0.18, 0.3, 0.0, -160.0),
            ]],
            dot: Some((0.26, 1.4)),
        },
        'k' => glyph(
            0.55,
            &[
                &[M(0.06, 1.75), L(0.06, 0.0)],
                &[M(0.5, 1.0), L(0.08, 0.42), L(0.54, 0.0)],
            ],
        ),
        'l' => glyph(0.24, &[&[M(0.1, 1.75), L(0.1, 0.06), L(0.2, 0.0)]]),
        'm' => glyph(
            0.9,
            &[&[
                M(0.06, 1.0),
                L(0.06, 0.0),
                L(0.06, 0.6),
                A(0.25, 0.6, 0.19, 0.36, 170.0, 0.0),
                L(0.44, 0.0),
                L(0.44, 0.6),
                A(0.64, 0.6, 0.2, 0.36, 170.0, 0.0),
                L(0.84, 0.0),
            ]],
        ),
        'n' => glyph(
            0.6,
            &[&[
                M(0.06, 1.0),
                L(0.06, 0.0),
                L(0.06, 0.55),
                A(0.31, 0.55, 0.25, 0.42, 170.0, 0.0),
                L(0.56, 0.0),
            ]],
        ),
        'o' => glyph(0.6, &[&[A(0.3, 0.5, 0.28, 0.5, 100.0, 470.0)]]),
        'p' => glyph(
            0.6,
            &[&[
                M(0.06, 1.0),
                L(0.06, -0.75),
                L(0.06, 0.6),
                A(0.32, 0.5, 0.26, 0.5, 160.0, -175.0),
            ]],
        ),
        'q' => glyph(
            0.64,
            &[&[BOWL, L(0.58, 1.0), L(0.58, -0.75), L(0.68, -0.62)]],
        ),
        'r' => glyph(
            0.46,
            &[&[
                M(0.06, 1.0),
                L(0.06, 0.0),
                L(0.06, 0.55),
                A(0.28, 0.55, 0.22, 0.4, 170.0, 40.0),
            ]],
        ),
        's' => glyph(
            0.5,
            &[&[
                A(0.26, 0.76, 0.22, 0.24, 20.0, 270.0),
                A(0.26, 0.26, 0.22, 0.26, 90.0, -160.0),
            ]],
        ),
        't' => glyph(
            0.45,
            &[
                &[
                    M(0.2, 1.45),
                    L(0.2, 0.12),
                    A(0.32, 0.12, 0.12, 0.12, 180.0, 300.0),
                ],
                &[M(0.02, 1.0), L(0.42, 1.0)],
            ],
        ),
        'u' => glyph(
            0.6,
            &[&[
                M(0.06, 1.0),
                L(0.06, 0.42),
                A(0.3, 0.42, 0.24, 0.42, 180.0, 360.0),
                L(0.54, 1.0),
                L(0.56, 0.0),
            ]],
        ),
        'v' => glyph(0.6, &[&[M(0.0, 1.0), L(0.3, 0.0), L(0.6, 1.0)]]),
        'w' => glyph(
            0.86,
            &[&[
                M(0.0, 1.0),
                L(0.2, 0.0),
                L(0.42, 0.85),
                L(0.63, 0.0),
                L(0.85, 1.0),
            ]],
        ),
        'x' => glyph(
            0.56,
            &[&[M(0.02, 1.0), L(0.55, 0.0)], &[M(0.55, 1.0), L(0.02, 0.0)]],
        ),
        'y' => glyph(
            0.6,
            &[
                &[M(0.02, 1.0), L(0.3, 0.05)],
                &[M(0.6, 1.0), L(0.12, -0.75)],
            ],
        ),
        'z' => glyph(
            0.56,
            &[&[M(0.04, 1.0), L(0.54, 1.0), L(0.04, 0.0), L(0.56, 0.0)]],
        ),
        _ => return None,
    })
}

/// Whether every character of `text` can be written (letters a–z with French accents,
/// spaces and line breaks).
pub fn can_write(text: &str) -> bool {
    text.chars()
        .all(|c| c == ' ' || c == '\n' || letter(c).is_some())
}

/// A letter split into its base glyph and its accents (combining marks).
fn letter(c: char) -> Option<(Glyph, Vec<char>)> {
    let mut parts = std::iter::once(c).nfd();
    let base = parts.next()?;
    let marks: Vec<char> = parts.collect();
    let known = |m: &char| {
        matches!(
            *m,
            '\u{300}' | '\u{301}' | '\u{302}' | '\u{308}' | '\u{327}'
        )
    };
    if !marks.iter().all(known) {
        return None;
    }
    Some((base_glyph(base)?, marks))
}

/// The strokes of an accent over (or under) a letter whose middle is at `x`.
fn accent(mark: char, x: f32) -> Vec<Vec<Seg>> {
    match mark {
        '\u{301}' => vec![vec![M(x + 0.14, 1.5), L(x - 0.04, 1.22)]],
        '\u{300}' => vec![vec![M(x - 0.12, 1.5), L(x + 0.05, 1.22)]],
        '\u{302}' => vec![vec![M(x - 0.16, 1.2), L(x, 1.46), L(x + 0.16, 1.2)]],
        '\u{308}' => vec![dot(x - 0.12, 1.34), dot(x + 0.12, 1.34)],
        '\u{327}' => vec![vec![M(x, 0.02), L(x + 0.08, -0.1), L(x - 0.07, -0.27)]],
        _ => Vec::new(),
    }
}

/// A dot: a tiny closed curve.
fn dot(x: f32, y: f32) -> Vec<Seg> {
    vec![A(x, y, 0.035, 0.035, 0.0, 330.0)]
}

/// Space between letters and between words, in glyph units.
const LETTER_GAP: f32 = 0.16;
const WORD_GAP: f32 = 0.75;

/// Small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// Uniform in `[0, 1)`.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (value >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `[-amplitude, amplitude)`.
    fn spread(&mut self, amplitude: f32) -> f32 {
        amplitude * (2.0 * self.next() - 1.0)
    }
}

/// How the text is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hand {
    /// Left end of the lines, page units.
    pub left: f32,
    /// Baseline of the first line, page units.
    pub baseline: f32,
    /// Lines wrap before this width, page units.
    pub width: f32,
    /// Height of a lowercase `x`, page units (300 = 3 mm).
    pub x_height: f32,
    /// Distance between two baselines, page units.
    pub line_gap: f32,
    /// Lean of the letters (x moves by `slant` × height).
    pub slant: f32,
    /// Amount of random variation, 0 (none) to about 1 (a hasty hand).
    pub jitter: f32,
    pub seed: u64,
    pub tool: Tool,
    pub color: Rgba,
    pub width_units: f32,
    pub layer: u32,
    /// Time of the first point, milliseconds since the note's creation.
    pub start_ms: u32,
}

impl Default for Hand {
    fn default() -> Self {
        Hand {
            left: 2000.0,
            baseline: 3000.0,
            width: 25_000.0,
            x_height: 320.0,
            line_gap: 1100.0,
            slant: 0.12,
            jitter: 0.6,
            seed: 1,
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width_units: 40.0,
            layer: 0,
            start_ms: 500,
        }
    }
}

/// Distance between two samples, in x-heights (about 200 samples a second at a writing speed of
/// a few centimetres a second).
const SAMPLE_STEP: f32 = 0.07;

impl Hand {
    /// The strokes of `text`, written line after line (`\n` starts a new line, long lines wrap
    /// at word boundaries). Characters that cannot be written ([`can_write`]) are skipped.
    pub fn write(&self, text: &str) -> Vec<Stroke> {
        let mut rng = Rng::new(self.seed);
        let mut strokes = Vec::new();
        let mut t = self.start_ms;
        let mut baseline = self.baseline;
        for paragraph in text.split('\n') {
            let mut x = 0.0_f32;
            for word in paragraph.split(' ').filter(|w| !w.is_empty()) {
                let letters: Vec<(Glyph, Vec<char>)> = word.chars().filter_map(letter).collect();
                if letters.is_empty() {
                    continue;
                }
                let width: f32 = letters.iter().map(|(g, _)| g.advance + LETTER_GAP).sum();
                if x > 0.0 && (x + width) * self.x_height > self.width {
                    x = 0.0;
                    baseline += self.line_gap;
                }
                // The word floats a little around the baseline.
                let word_shift = rng.spread(0.06 * self.jitter);
                for (glyph, marks) in &letters {
                    let mut parts: Vec<Vec<Seg>> =
                        glyph.strokes.iter().map(|s| s.to_vec()).collect();
                    let middle = glyph.advance / 2.0;
                    match glyph.dot {
                        // An accent on `i` or `j` replaces its dot.
                        Some(_) if !marks.is_empty() => {}
                        Some((dx, dy)) => parts.push(dot(dx, dy)),
                        None => {}
                    }
                    for mark in marks {
                        parts.extend(accent(*mark, middle));
                    }
                    let scale = 1.0 + rng.spread(0.07 * self.jitter);
                    let shift = word_shift + rng.spread(0.04 * self.jitter);
                    for part in &parts {
                        let points = self.trace(part, &mut rng, x, baseline, scale, shift);
                        strokes.push(self.stroke(points, &mut rng, &mut t));
                        // Lifting the pen.
                        t += 60 + (rng.next() * 90.0) as u32;
                    }
                    x += glyph.advance + LETTER_GAP;
                }
                x += WORD_GAP - LETTER_GAP;
                t += 150 + (rng.next() * 200.0) as u32;
            }
            baseline += self.line_gap;
        }
        strokes
    }

    /// The page points of one stroke: densified, varied, slanted, resampled at the tablet's
    /// spacing.
    fn trace(
        &self,
        part: &[Seg],
        rng: &mut Rng,
        x0: f32,
        baseline: f32,
        scale: f32,
        shift: f32,
    ) -> Vec<(f32, f32)> {
        let glyph = densify(part);
        // A slow wobble along the stroke, different for each stroke.
        let (phase_x, phase_y) = (rng.next() * 6.3, rng.next() * 6.3);
        let amplitude = 0.025 * self.jitter;
        let lean = self.slant + rng.spread(0.04 * self.jitter);
        let page: Vec<(f32, f32)> = glyph
            .iter()
            .enumerate()
            .map(|(i, &(gx, gy))| {
                let s = i as f32 * 0.35;
                let gx = gx * scale + amplitude * (s + phase_x).sin();
                let gy = gy * scale + amplitude * (s * 0.8 + phase_y).sin() + shift;
                (
                    self.left + (x0 + gx + lean * gy) * self.x_height,
                    baseline - gy * self.x_height,
                )
            })
            .collect();
        resample(&page, SAMPLE_STEP * self.x_height)
    }

    fn stroke(&self, points: Vec<(f32, f32)>, rng: &mut Rng, t: &mut u32) -> Stroke {
        let n = points.len().max(2) as f32;
        let mut out = Vec::with_capacity(points.len());
        for (i, (x, y)) in points.into_iter().enumerate() {
            let s = i as f32 / (n - 1.0);
            let pressure = (0.35 + 0.45 * (s * std::f32::consts::PI).sin() + rng.spread(0.03))
                .clamp(0.05, 1.0);
            out.push(Point {
                x,
                y,
                pressure,
                t: *t,
            });
            *t += 5;
        }
        Stroke {
            tool: self.tool,
            color: self.color,
            width: self.width_units,
            layer: self.layer,
            points: out,
        }
    }
}

/// The points of a stroke in glyph units, close together (arcs every 6°, lines every 0.04).
fn densify(part: &[Seg]) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    let line_to = |out: &mut Vec<(f32, f32)>, to: (f32, f32)| {
        if let Some(&from) = out.last() {
            let length = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
            let steps = (length / 0.04).ceil().max(1.0) as usize;
            for k in 1..=steps {
                let s = k as f32 / steps as f32;
                out.push((from.0 + (to.0 - from.0) * s, from.1 + (to.1 - from.1) * s));
            }
        } else {
            out.push(to);
        }
    };
    for seg in part {
        match *seg {
            M(x, y) => out.push((x, y)),
            L(x, y) => line_to(&mut out, (x, y)),
            A(cx, cy, rx, ry, from, to) => {
                let steps = ((to - from).abs() / 6.0).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let angle = (from + (to - from) * k as f32 / steps as f32).to_radians();
                    let point = (cx + rx * angle.cos(), cy + ry * angle.sin());
                    if k == 0 {
                        line_to(&mut out, point);
                    } else {
                        out.push(point);
                    }
                }
            }
        }
    }
    out
}

/// Points every `step` along the polyline (its first and last points kept).
fn resample(points: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    let mut carried = 0.0_f32;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if length <= 0.0 {
            continue;
        }
        let mut along = step - carried;
        while along <= length {
            let s = along / length;
            out.push((a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s));
            along += step;
        }
        carried = length - (along - step);
    }
    if let Some(&last) = points.last()
        && out.last() != Some(&last)
    {
        out.push(last);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_letter_and_french_accent_can_be_written() {
        assert!(can_write("abcdefghijklmnopqrstuvwxyz"));
        assert!(can_write("éèêëàâîïôùûç\nle chat"));
        assert!(!can_write("Le chat"), "no capitals");
        assert!(!can_write("chat."), "no punctuation");
    }

    #[test]
    fn a_text_becomes_strokes_on_its_lines() {
        let hand = Hand::default();
        let strokes = hand.write("le chat dort\nsur le canapé");
        // l e c h a t d o r t: one stroke each, t has two; s u r l e c a n a p e + accent.
        assert_eq!(strokes.len(), 10 + 2 + 11 + 1);
        let mut previous = 0;
        for stroke in &strokes {
            assert!(stroke.points.len() >= 2);
            for p in &stroke.points {
                assert!(p.t >= previous, "times increase");
                previous = p.t;
                assert!((0.0..=1.0).contains(&p.pressure));
            }
        }
        // The second line is below the first.
        let first_line = strokes[0].points[0].y;
        let last = strokes.last().unwrap().points[0].y;
        assert!(
            last > first_line + hand.line_gap * 0.5,
            "{first_line} {last}"
        );
        // Samples about 0.07 x-height apart, like the tablet's.
        let p = &strokes[0].points;
        let gap = ((p[1].x - p[0].x).powi(2) + (p[1].y - p[0].y).powi(2)).sqrt();
        assert!((gap - SAMPLE_STEP * hand.x_height).abs() < 2.0, "{gap}");
    }

    #[test]
    fn long_lines_wrap_and_seeds_vary_the_hand() {
        let narrow = Hand {
            width: 3000.0,
            ..Hand::default()
        };
        let strokes = narrow.write("un deux trois quatre");
        let lowest = strokes
            .iter()
            .flat_map(|s| s.points.iter())
            .map(|p| p.y)
            .fold(f32::MIN, f32::max);
        assert!(
            lowest > narrow.baseline + narrow.line_gap,
            "wrapped: {lowest}"
        );

        let a = Hand::default().write("forêt");
        let b = Hand {
            seed: 2,
            ..Hand::default()
        }
        .write("forêt");
        assert_eq!(a.len(), b.len());
        assert_ne!(a, b);
        assert_eq!(a, Hand::default().write("forêt"), "deterministic");
    }

    #[test]
    fn an_accent_replaces_the_dot_of_an_i() {
        assert_eq!(Hand::default().write("i").len(), 2);
        assert_eq!(Hand::default().write("î").len(), 2);
        assert_eq!(Hand::default().write("ï").len(), 3);
    }
}
