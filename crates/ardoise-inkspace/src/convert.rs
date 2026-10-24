//! Converting a decoded Inkspace page into an Ardoise [`Note`].
//!
//! Rules (see the crate documentation for the reasons):
//! - points: Catmull-Rom control points minus the duplicated first and last ones, page
//!   `transform` applied, page `boundingBox` origin removed, ×25 (page units → device units);
//! - widths: stored diameter × 25 × [`width::width_factor`], stored as pressure through the
//!   ballpoint's inverse response with a nominal width per stroke;
//! - timing: `creationDate` + i·1000/`pointsPerSecond` when every stroke has both, otherwise
//!   synthesised for the whole note (200 Hz, fixed gap) and marked `Timing::Synthesized`.

use ardoise_core::format::strokes::MAX_COORD;
use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use uuid::Uuid;

use crate::decode::{DocumentData, InkStroke, PageData};
use crate::store::Props;
use crate::{DecodeError, NoteStats, width};

/// Inkspace page units per device unit: 1 page unit = 25 device units (0.25 mm).
pub const UNITS_PER_PAGE_UNIT: f64 = 25.0;

/// Namespace of the note ids derived from Inkspace ids (UUID v5). Fixed forever: the same
/// Inkspace note always gets the same Ardoise id, on every PC and every import.
pub const NOTE_ID_NAMESPACE: Uuid = Uuid::from_u128(0x6c1e_93a4_5b0d_4f7e_9a1c_2d8b_e4f0_7a35);

/// Sampling rate assumed when synthesising times (the Slate samples at 200 Hz).
const SYNTH_INTERVAL_MS: u32 = 5;
/// Pause inserted between strokes when synthesising times.
const SYNTH_GAP_MS: u32 = 250;

/// Largest page side accepted, in page units (2.5 m).
const MAX_PAGE_SIDE: f64 = 10_000.0;

/// The Slate's portrait page (page units), used when a page has no `boundingBox`.
const DEFAULT_BOUNDING_BOX: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 864.0,
    height: 1188.0,
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// SVG `matrix(a b c d e f)`: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }

    /// How lengths scale (square root of the absolute determinant).
    fn length_scale(&self) -> f64 {
        let [a, b, c, d, _, _] = self.0;
        (a * d - b * c).abs().sqrt()
    }
}

/// The Ardoise id of an Inkspace note (page 0) or of its extra pages.
pub fn note_id(inkspace_id: &str, page_index: usize) -> String {
    let name = if page_index == 0 {
        inkspace_id.to_owned()
    } else {
        format!("{inkspace_id}/page/{page_index}")
    };
    Uuid::new_v5(&NOTE_ID_NAMESPACE, name.as_bytes()).to_string()
}

/// Numbers inside `name( … )`, separated by spaces and/or commas.
fn parse_function(value: &str, name: &str, count: usize) -> Result<Vec<f64>, DecodeError> {
    let invalid = || DecodeError::Invalid {
        what: if name == "rect" {
            "boundingBox"
        } else {
            "transform"
        },
        value: value.chars().take(80).collect(),
    };
    let inner = value
        .trim()
        .strip_prefix(name)
        .and_then(|rest| rest.trim_start().strip_prefix('('))
        .and_then(|rest| rest.trim_end().strip_suffix(')'))
        .ok_or_else(invalid)?;
    let numbers = inner
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect::<Option<Vec<f64>>>()
        .ok_or_else(invalid)?;
    if numbers.len() == count {
        Ok(numbers)
    } else {
        Err(invalid())
    }
}

fn bounding_box(props: &Props) -> Result<Rect, DecodeError> {
    let Some(value) = props.get("boundingBox") else {
        return Ok(DEFAULT_BOUNDING_BOX);
    };
    let n = parse_function(value, "rect", 4)?;
    let rect = Rect {
        x: n[0],
        y: n[1],
        width: n[2],
        height: n[3],
    };
    if !(rect.width > 0.0
        && rect.height > 0.0
        && rect.width <= MAX_PAGE_SIDE
        && rect.height <= MAX_PAGE_SIDE)
    {
        return Err(DecodeError::Invalid {
            what: "boundingBox",
            value: value.chars().take(80).collect(),
        });
    }
    Ok(rect)
}

fn transform(props: &Props) -> Result<Matrix, DecodeError> {
    match props.get("transform") {
        None => Ok(Matrix::IDENTITY),
        Some(value) => {
            let n = parse_function(value, "matrix", 6)?;
            Ok(Matrix([n[0], n[1], n[2], n[3], n[4], n[5]]))
        }
    }
}

fn millis(props: &Props, name: &str) -> Option<i64> {
    props.get(name).and_then(|v| v.trim().parse::<i64>().ok())
}

/// Samples of a stroke: the Catmull-Rom control points without the phantom first and last
/// ones (with 3 or more points); 1 or 2 points are kept as they are (a dot or a dash), minus
/// an exact duplicate.
fn sample_range(stroke: &InkStroke) -> std::ops::Range<usize> {
    let n = stroke.points.len();
    match n {
        0 | 1 => 0..n,
        2 if stroke.points[0] == stroke.points[1] => 0..1,
        2 => 0..2,
        _ => 1..n - 1,
    }
}

/// Clamps a millisecond offset into `Point::t`.
fn clamp_t(ms: f64) -> u32 {
    if ms.is_nan() || ms <= 0.0 {
        0
    } else if ms >= u32::MAX as f64 {
        u32::MAX
    } else {
        ms.round() as u32
    }
}

/// Converts page `page_index` of `doc`.
pub(crate) fn convert_page(
    doc: &DocumentData,
    page_index: usize,
    page: &PageData,
) -> Result<(Note, NoteStats), DecodeError> {
    let inkspace_id = doc.id.to_string();
    let bbox = bounding_box(&page.props)?;
    let matrix = transform(&page.props)?;
    let width_scale = matrix.length_scale();
    let page_size = PageSize {
        width: (bbox.width * UNITS_PER_PAGE_UNIT).round() as u32,
        height: (bbox.height * UNITS_PER_PAGE_UNIT).round() as u32,
    };

    let all_strokes = || page.layers.iter().flat_map(|l| l.strokes.iter());
    let recorded = all_strokes()
        .all(|s| s.points_per_second.is_some_and(|pps| pps > 0.0) && s.creation_date.is_some());
    let earliest_stroke = all_strokes().filter_map(|s| s.creation_date).min();

    let mut created = millis(&doc.props, "creationDate")
        .or_else(|| millis(&page.props, "creationDate"))
        .or(earliest_stroke)
        .unwrap_or(0);
    let modified = millis(&doc.props, "lastModifiedDate")
        .or_else(|| millis(&page.props, "lastModifiedDate"))
        .unwrap_or(created)
        .max(created);
    if recorded && let Some(earliest) = earliest_stroke {
        // Point times count from `created`: never let a stroke start before it.
        created = created.min(earliest);
    }

    // Inkspace layers have no names: neutral numbered names, all visible.
    let layers: Vec<Layer> = (0..page.layers.len().max(1))
        .map(|i| Layer {
            name: format!("Calque {}", i + 1),
            visible: true,
        })
        .collect();

    let mut stats = NoteStats {
        layers: layers.len(),
        strokes: 0,
        control_points: 0,
        points: 0,
        clipped_widths: 0,
        timing: if recorded {
            Timing::Recorded
        } else {
            Timing::Synthesized
        },
    };

    let mut strokes = Vec::new();
    let mut synth_t: u32 = 0;
    for (layer_index, layer) in page.layers.iter().enumerate() {
        for ink in &layer.strokes {
            stats.strokes += 1;
            stats.control_points += ink.points.len();
            let range = sample_range(ink);
            let factor = f64::from(width::width_factor(ink.points_per_second));
            let diameters: Vec<f32> = ink.widths[range.clone()]
                .iter()
                .map(|w| (w.max(0.0) * width_scale * UNITS_PER_PAGE_UNIT * factor) as f32)
                .collect();
            let stroke_width = width::stroke_width_for(diameters.iter().copied());

            let mut points = Vec::with_capacity(range.len());
            for (j, (control, &diameter)) in ink.points[range].iter().zip(&diameters).enumerate() {
                let [px, py] = matrix.apply(*control);
                let t = if recorded {
                    let start = ink.creation_date.unwrap_or(created).saturating_sub(created);
                    let pps = ink.points_per_second.unwrap_or(200.0);
                    clamp_t(start as f64 + j as f64 * 1000.0 / pps)
                } else {
                    synth_t.saturating_add((j as u32).saturating_mul(SYNTH_INTERVAL_MS))
                };
                if !width::is_representable(stroke_width, diameter) {
                    stats.clipped_widths += 1;
                }
                let x = ((px - bbox.x) * UNITS_PER_PAGE_UNIT) as f32;
                let y = ((py - bbox.y) * UNITS_PER_PAGE_UNIT) as f32;
                // The note file refuses such points: report the note as not importable rather
                // than failing later when it is saved.
                if !(x.is_finite() && y.is_finite() && x.abs() <= MAX_COORD && y.abs() <= MAX_COORD)
                {
                    return Err(DecodeError::Invalid {
                        what: "point coordinates",
                        value: format!("{x}, {y}"),
                    });
                }
                points.push(Point {
                    x,
                    y,
                    pressure: width::pressure_for_width(stroke_width, diameter),
                    t,
                });
            }
            if let Some(last) = points.last() {
                synth_t = last
                    .t
                    .saturating_add(SYNTH_INTERVAL_MS)
                    .saturating_add(SYNTH_GAP_MS);
            }
            stats.points += points.len();
            strokes.push(Stroke {
                tool: Tool::Ballpoint,
                color: Rgba::from_rgba_u32(ink.rgba),
                width: stroke_width,
                layer: layer_index as u32,
                points,
            });
        }
    }

    let note = Note {
        id: note_id(&inkspace_id, page_index),
        // Inkspace titles are always empty (it shows dates instead).
        title: doc.props.get("title").unwrap_or_default().trim().to_owned(),
        created,
        modified,
        page: page_size,
        background: Background::default(),
        tags: Vec::new(),
        favorite: false,
        layers,
        strokes,
        timing: stats.timing,
        origin: Origin::Inkspace { id: inkspace_id },
    };
    Ok((note, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::LayerData;
    use crate::store::NodeId;

    const LANDSCAPE: &[(&str, &str)] = &[
        ("nodeType", "page"),
        ("boundingBox", "rect(0.0 0.0 1188.0 864.0)"),
        ("transform", "matrix(0.0 -1.0 1.0 0.0 0.0 864.0)"),
    ];

    fn stroke(
        points: &[[f64; 2]],
        widths: &[f64],
        pps: Option<f64>,
        date: Option<i64>,
    ) -> InkStroke {
        InkStroke {
            points: points.to_vec(),
            widths: widths.to_vec(),
            rgba: 0x51A7_F9FF,
            points_per_second: pps,
            creation_date: date,
        }
    }

    fn doc(page_props: &[(&str, &str)], layers: Vec<Vec<InkStroke>>) -> DocumentData {
        DocumentData {
            id: NodeId::Uuid(Uuid::from_u128(0x0123_4567_89ab_4cde_8f01_2345_6789_abcd)),
            props: Props::from_pairs(&[
                ("nodeType", "document"),
                ("title", ""),
                ("creationDate", "1700000000000"),
                ("lastModifiedDate", "1700000250555"),
            ]),
            pages: vec![PageData {
                props: Props::from_pairs(page_props),
                layers: layers
                    .into_iter()
                    .map(|strokes| LayerData { strokes })
                    .collect(),
            }],
        }
    }

    fn convert(doc: &DocumentData) -> (Note, NoteStats) {
        convert_page(doc, 0, &doc.pages[0]).unwrap()
    }

    #[test]
    fn landscape_page_rotates_into_place() {
        let s = stroke(
            &[[10.0, 20.0], [10.0, 20.0], [12.0, 24.0], [12.0, 24.0]],
            &[1.0, 1.0, 2.0, 2.0],
            Some(200.0),
            Some(1_700_000_017_515),
        );
        let d = doc(LANDSCAPE, vec![vec![s]]);
        let (note, stats) = convert(&d);
        assert_eq!(note.page, ardoise_core::note::A4_LANDSCAPE);
        let p = &note.strokes[0].points;
        assert_eq!(p.len(), 2);
        // page_x = y, page_y = 864 − x, then ×25.
        assert_eq!((p[0].x, p[0].y), (500.0, 21_350.0));
        assert_eq!((p[1].x, p[1].y), (600.0, 21_300.0));
        assert_eq!(stats.control_points, 4);
        assert_eq!(stats.points, 2);
        assert_eq!(note.strokes[0].color, Rgba::rgb(0x51, 0xA7, 0xF9));
        assert_eq!(note.strokes[0].tool, Tool::Ballpoint);
        assert_eq!(
            note.origin,
            Origin::Inkspace {
                id: d.id.to_string()
            }
        );
        assert_eq!(note.title, "");
        assert_eq!(note.created, 1_700_000_000_000);
        assert_eq!(note.modified, 1_700_000_250_555);
    }

    #[test]
    fn portrait_page_is_identity() {
        let props = [("boundingBox", "rect(0 0 864 1188)")];
        let s = stroke(
            &[[1.0, 2.0], [1.0, 2.0], [3.0, 4.0], [3.0, 4.0]],
            &[1.0; 4],
            None,
            None,
        );
        let (note, _) = convert(&doc(&props, vec![vec![s]]));
        assert_eq!(note.page, ardoise_core::note::A4_PORTRAIT);
        let p = &note.strokes[0].points;
        assert_eq!((p[0].x, p[0].y, p[1].x, p[1].y), (25.0, 50.0, 75.0, 100.0));
    }

    #[test]
    fn widths_round_trip_through_pressure() {
        let widths = [1.06, 1.06, 1.5, 2.12, 1.8, 1.8];
        let pts: Vec<[f64; 2]> = (0..6).map(|i| [i as f64, 0.0]).collect();
        let s = stroke(&pts, &widths, Some(200.0), Some(1_700_000_017_515));
        let (note, stats) = convert(&doc(LANDSCAPE, vec![vec![s]]));
        let st = &note.strokes[0];
        let factor = f64::from(width::width_factor(Some(200.0)));
        assert!((f64::from(st.width) - 2.12 * 25.0 * factor).abs() < 1e-3);
        for (p, w) in st.points.iter().zip(&widths[1..5]) {
            assert!((0.0..=1.0).contains(&p.pressure));
            let drawn = f64::from(width::width_at(st.width, p.pressure));
            assert!((drawn - w * 25.0 * factor).abs() < 1e-3, "{drawn} vs {w}");
        }
        assert_eq!(stats.clipped_widths, 0);
    }

    #[test]
    fn recorded_timing() {
        let a = stroke(
            &[[0.0, 0.0]; 5],
            &[1.0; 5],
            Some(200.0),
            Some(1_700_000_001_000),
        );
        let b = stroke(
            &[[0.0, 0.0]; 4],
            &[1.0; 4],
            Some(60.0),
            Some(1_700_000_002_000),
        );
        let (note, stats) = convert(&doc(LANDSCAPE, vec![vec![a], vec![b]]));
        assert_eq!(note.timing, Timing::Recorded);
        assert_eq!(stats.timing, Timing::Recorded);
        let t: Vec<u32> = note.strokes[0].points.iter().map(|p| p.t).collect();
        assert_eq!(t, [1000, 1005, 1010]);
        let t: Vec<u32> = note.strokes[1].points.iter().map(|p| p.t).collect();
        assert_eq!(t, [2000, 2017]);
        assert_eq!(note.strokes[1].layer, 1);
        assert_eq!(note.layers.len(), 2);
        assert_eq!(note.layers[1].name, "Calque 2");
        assert!(note.layers.iter().all(|l| l.visible));
    }

    #[test]
    fn a_stroke_before_the_document_date_moves_the_time_origin() {
        let a = stroke(
            &[[0.0, 0.0]; 3],
            &[1.0; 3],
            Some(200.0),
            Some(1_699_999_999_000),
        );
        let (note, _) = convert(&doc(LANDSCAPE, vec![vec![a]]));
        assert_eq!(note.created, 1_699_999_999_000);
        assert_eq!(note.strokes[0].points[0].t, 0);
    }

    #[test]
    fn synthesized_timing_when_any_stroke_lacks_it() {
        let a = stroke(
            &[[0.0, 0.0]; 5],
            &[1.0; 5],
            Some(200.0),
            Some(1_700_000_001_000),
        );
        let b = stroke(&[[0.0, 0.0]; 4], &[1.0; 4], None, None);
        let c = stroke(
            &[[0.0, 0.0]; 4],
            &[1.0; 4],
            Some(0.0),
            Some(1_700_000_001_000),
        );
        let (note, _) = convert(&doc(LANDSCAPE, vec![vec![a, b, c]]));
        assert_eq!(note.timing, Timing::Synthesized);
        let times: Vec<Vec<u32>> = note
            .strokes
            .iter()
            .map(|s| s.points.iter().map(|p| p.t).collect())
            .collect();
        assert_eq!(times, [vec![0, 5, 10], vec![265, 270], vec![525, 530]]);
    }

    #[test]
    fn short_strokes() {
        let one = stroke(&[[1.0, 1.0]], &[1.0], None, None);
        let two_same = stroke(&[[1.0, 1.0], [1.0, 1.0]], &[1.0, 1.0], None, None);
        let two = stroke(&[[1.0, 1.0], [2.0, 1.0]], &[1.0, 1.0], None, None);
        let three = stroke(&[[1.0, 1.0], [1.0, 1.0], [1.0, 1.0]], &[1.0; 3], None, None);
        let empty = stroke(&[], &[], None, None);
        let (note, stats) = convert(&doc(
            LANDSCAPE,
            vec![vec![one, two_same, two, three, empty]],
        ));
        let counts: Vec<usize> = note.strokes.iter().map(|s| s.points.len()).collect();
        assert_eq!(counts, [1, 1, 2, 1, 0]);
        assert_eq!(stats.strokes, 5);
    }

    #[test]
    fn a_page_without_layers_gets_one() {
        let (note, stats) = convert(&doc(LANDSCAPE, vec![]));
        assert_eq!(note.layers.len(), 1);
        assert_eq!(stats.layers, 1);
        assert!(note.strokes.is_empty());
    }

    #[test]
    fn invalid_page_properties() {
        for (name, value) in [
            ("boundingBox", "rect(0 0 0 864)"),
            ("boundingBox", "rect(0 0 1e9 864)"),
            ("boundingBox", "rect(0 0 864)"),
            ("boundingBox", "square(0 0 1 1)"),
            ("transform", "matrix(1 0 0 1 0 NaN)"),
            ("transform", "rotate(90)"),
        ] {
            let d = doc(&[(name, value)], vec![]);
            assert!(convert_page(&d, 0, &d.pages[0]).is_err(), "{value}");
        }
        let d = doc(&[("transform", "matrix(1,0, 0,1 ,0,0)")], vec![]);
        assert!(convert_page(&d, 0, &d.pages[0]).is_ok());
    }

    #[test]
    fn note_ids_are_deterministic() {
        let id = "01234567-89ab-4cde-8f01-23456789abcd";
        assert_eq!(note_id(id, 0), note_id(id, 0));
        assert_ne!(note_id(id, 0), note_id(id, 1));
        assert_ne!(note_id(id, 0), id);
        assert!(Uuid::parse_str(&note_id(id, 0)).is_ok());
    }
}
