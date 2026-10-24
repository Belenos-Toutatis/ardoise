//! Helpers shared by the unit tests: a seeded pseudo-random generator and sample notes.

use ardoise_ink::{Point, Rgba, Stroke, Tool};

use crate::note::{
    A4_LANDSCAPE, Background, Layer, Note, Origin, PageSize, Paper, Template, Timing,
};

/// Deterministic linear congruential generator (Knuth's MMIX constants), so that every
/// "random" test is reproducible.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }

    /// Uniform in `0.0..1.0`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A coordinate: mostly on the page, sometimes far away or with odd fractions.
    pub fn coord(&mut self) -> f32 {
        match self.below(10) {
            0 => (self.unit() - 0.5) * 2.0e9,
            1 => (self.unit() - 0.5) * 2.0e6,
            2 => self.below(29_701) as f32,
            _ => self.unit() * 29_700.0,
        }
    }

    pub fn strokes(&mut self, count: usize, max_points: u64) -> Vec<Stroke> {
        let mut t = self.below(1000) as u32;
        (0..count)
            .map(|_| {
                let n = self.below(max_points + 1);
                let points = (0..n)
                    .map(|_| {
                        t = t.wrapping_add(self.below(40) as u32);
                        Point {
                            x: self.coord(),
                            y: self.coord(),
                            pressure: self.unit(),
                            t: if self.below(50) == 0 {
                                self.next_u32()
                            } else {
                                t
                            },
                        }
                    })
                    .collect();
                Stroke {
                    tool: Tool::ALL[self.below(Tool::ALL.len() as u64) as usize],
                    color: Rgba::from_rgba_u32(self.next_u32()),
                    width: self.unit() * 500.0,
                    layer: self.below(8) as u32,
                    points,
                }
            })
            .collect()
    }

    pub fn note(&mut self) -> Note {
        let layers = 1 + self.below(8) as usize;
        let count = self.below(12) as usize;
        let mut strokes = self.strokes(count, 80);
        for stroke in &mut strokes {
            stroke.layer %= layers as u32;
        }
        let paper = match self.below(5) {
            0 => Paper::White,
            1 => Paper::Cream,
            2 => Paper::Kraft,
            3 => Paper::Slate,
            _ => Paper::Custom(Rgba::from_rgba_u32(self.next_u32())),
        };
        let created = 1_700_000_000_000 + self.below(100_000_000_000) as i64;
        Note {
            id: format!("{:016x}-{:08x}", self.next_u64(), self.next_u32()),
            title: [
                "",
                "Cours",
                "Réunion « budget » 2026",
                "a/b:c*d?",
                "  spaced  ",
            ][self.below(5) as usize]
                .to_owned(),
            created,
            modified: created + self.below(1_000_000) as i64,
            page: PageSize {
                width: 1 + self.below(100_000) as u32,
                height: 1 + self.below(100_000) as u32,
            },
            background: Background {
                paper,
                template: [
                    Template::Blank,
                    Template::Lined,
                    Template::Seyes,
                    Template::Grid5mm,
                    Template::Millimetric,
                    Template::Dotted,
                    Template::Music,
                ][self.below(7) as usize],
                texture: self.below(2) == 0,
            },
            tags: (0..self.below(4))
                .map(|i| format!("tag{i}-{}", self.below(100)))
                .collect(),
            favorite: self.below(2) == 0,
            layers: (0..layers)
                .map(|i| Layer {
                    name: format!("Calque {}", i + 1),
                    visible: self.below(3) != 0,
                })
                .collect(),
            strokes,
            timing: if self.below(2) == 0 {
                Timing::Recorded
            } else {
                Timing::Synthesized
            },
            origin: match self.below(4) {
                0 => Origin::Tablet,
                1 => Origin::Live,
                2 => Origin::Inkspace {
                    id: format!("{:032x}", self.next_u64()),
                },
                _ => Origin::Ardoise,
            },
        }
    }
}

/// A small blank note with one layer, created on 2026-10-24 at noon UTC.
pub fn blank_note(id: &str, title: &str) -> Note {
    Note {
        id: id.to_owned(),
        title: title.to_owned(),
        created: 1_792_843_200_000,
        modified: 1_792_843_200_000,
        page: A4_LANDSCAPE,
        background: Background::default(),
        tags: Vec::new(),
        favorite: false,
        layers: vec![Layer {
            name: "Calque 1".to_owned(),
            visible: true,
        }],
        strokes: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}
