use ardoise_ink::{Point, Rgba, Tool};

use super::*;
use crate::note::{Background, Paper, Template};
use crate::testutil::{Lcg, blank_note};

const NOW: i64 = 1_800_000_000_000;

fn new_note(title: &str) -> NewNote {
    NewNote {
        id: "new-id".to_owned(),
        title: title.to_owned(),
        now: NOW,
    }
}

fn point(x: f32, y: f32, t: u32) -> Point {
    Point {
        x,
        y,
        pressure: 0.5,
        t,
    }
}

fn stroke(layer: u32, points: &[(f32, f32, u32)]) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer,
        points: points.iter().map(|&(x, y, t)| point(x, y, t)).collect(),
    }
}

fn note_with(title: &str, strokes: Vec<Stroke>) -> Note {
    let mut note = blank_note(&format!("id-{title}"), title);
    note.strokes = strokes;
    note
}

fn layer(name: &str, visible: bool) -> Layer {
    Layer {
        name: name.to_owned(),
        visible,
    }
}

fn times(note: &Note) -> Vec<Vec<u32>> {
    note.strokes
        .iter()
        .map(|s| s.points.iter().map(|p| p.t).collect())
        .collect()
}

fn point_count(note: &Note) -> usize {
    note.strokes.iter().map(|s| s.points.len()).sum()
}

// --- time_range ---------------------------------------------------------------------------

#[test]
fn time_range_spans_every_stroke() {
    let note = note_with(
        "A",
        vec![
            stroke(0, &[(0.0, 0.0, 500), (1.0, 1.0, 900)]),
            stroke(0, &[]),
            stroke(0, &[(2.0, 2.0, 120), (3.0, 3.0, 300)]),
        ],
    );
    assert_eq!(time_range(&note), Some((120, 900)));
    assert_eq!(time_range(&note_with("B", vec![stroke(0, &[])])), None);
    assert_eq!(time_range(&note_with("C", vec![])), None);
}

// --- split at an instant ------------------------------------------------------------------

fn timed_note() -> Note {
    let mut note = note_with(
        "Cours",
        vec![
            stroke(0, &[(10.0, 10.0, 0), (20.0, 10.0, 100), (30.0, 10.0, 200)]),
            // Spans the instant 1000.
            stroke(
                0,
                &[
                    (10.0, 50.0, 900),
                    (20.0, 50.0, 950),
                    (30.0, 50.0, 1000),
                    (40.0, 50.0, 1100),
                ],
            ),
            stroke(0, &[(10.0, 90.0, 1500), (20.0, 90.0, 1600)]),
        ],
    );
    note.tags = vec!["maths".into()];
    note.favorite = true;
    note.origin = Origin::Tablet;
    note.timing = Timing::Synthesized;
    note.created = 1_000_000;
    note.modified = 2_000_000;
    note
}

#[test]
fn split_at_time_cuts_the_stroke_that_spans_the_instant() {
    let source = timed_note();
    let split = split_at_time(&source, 1000, new_note("Suite")).unwrap();

    assert_eq!(times(&split.kept), [vec![0, 100, 200], vec![900, 950]]);
    // Times of the new note start at 0: 1000 -> 0, 1100 -> 100, 1500 -> 500, 1600 -> 600.
    assert_eq!(times(&split.extracted), [vec![0, 100], vec![500, 600]]);
    // The cut stroke keeps its look in both halves.
    assert_eq!(split.kept.strokes[1].width, source.strokes[1].width);
    assert_eq!(split.extracted.strokes[0].color, source.strokes[1].color);
    assert_eq!(split.extracted.strokes[0].points[1].x, 40.0);
    assert_eq!(point_count(&split.kept) + point_count(&split.extracted), 9);
}

#[test]
fn split_at_time_new_note_metadata() {
    let source = timed_note();
    let split = split_at_time(&source, 1000, new_note("Suite")).unwrap();
    let extracted = &split.extracted;

    assert_eq!(extracted.id, "new-id");
    assert_eq!(extracted.title, "Suite");
    // `created` moves by the first moved sample time (1000 ms).
    assert_eq!(extracted.created, source.created + 1000);
    assert_eq!(extracted.modified, NOW);
    assert_eq!(extracted.origin, Origin::Ardoise);
    assert!(!extracted.favorite);
    assert_eq!(extracted.page, source.page);
    assert_eq!(extracted.background, source.background);
    assert_eq!(extracted.layers, source.layers);
    assert_eq!(extracted.tags, source.tags);
    assert_eq!(extracted.timing, Timing::Synthesized);

    // The kept note is the source with fewer strokes.
    let kept = &split.kept;
    assert_eq!(kept.id, source.id);
    assert_eq!(kept.title, source.title);
    assert_eq!(kept.created, source.created);
    assert_eq!(kept.origin, Origin::Tablet);
    assert!(kept.favorite);
    assert_eq!(kept.modified, NOW);
}

#[test]
fn split_at_time_boundaries() {
    let source = timed_note();
    // A sample exactly at the instant goes to the new note.
    let split = split_at_time(&source, 200, new_note("X")).unwrap();
    assert_eq!(times(&split.kept), [vec![0, 100]]);
    assert_eq!(split.extracted.created, source.created + 200);
    assert_eq!(split.extracted.strokes[0].points[0].t, 0);

    // From the very first sample: nothing stays.
    assert_eq!(
        split_at_time(&source, 0, new_note("X")).unwrap_err(),
        OpsError::NothingToSplit
    );
    // After the last sample: nothing moves.
    assert_eq!(
        split_at_time(&source, 1601, new_note("X")).unwrap_err(),
        OpsError::NothingToSplit
    );
    assert!(split_at_time(&source, 1600, new_note("X")).is_ok());
    assert!(split_at_time(&source, 1, new_note("X")).is_ok());
}

#[test]
fn split_at_time_needs_samples_on_both_sides() {
    let empty = note_with("Vide", vec![stroke(0, &[])]);
    assert_eq!(
        split_at_time(&empty, 10, new_note("X")).unwrap_err(),
        OpsError::NothingToSplit
    );
    assert_eq!(
        split_at_time(&note_with("Rien", vec![]), 0, new_note("X")).unwrap_err(),
        OpsError::NothingToSplit
    );
}

#[test]
fn split_at_time_keeps_strokes_without_samples_and_whole_strokes() {
    let source = note_with(
        "A",
        vec![
            stroke(0, &[]),
            stroke(0, &[(1.0, 1.0, 10)]),
            stroke(0, &[(2.0, 2.0, 500)]),
        ],
    );
    let split = split_at_time(&source, 100, new_note("B")).unwrap();
    assert_eq!(times(&split.kept), [vec![], vec![10]]);
    assert_eq!(times(&split.extracted), [vec![0]]);
    assert_eq!(split.extracted.created, source.created + 500);
}

#[test]
fn split_at_time_handles_unordered_times() {
    // Times that are not increasing inside a stroke: every sample still goes to one side.
    let source = note_with(
        "A",
        vec![stroke(
            0,
            &[(0.0, 0.0, 300), (1.0, 0.0, 100), (2.0, 0.0, 400)],
        )],
    );
    let split = split_at_time(&source, 200, new_note("B")).unwrap();
    assert_eq!(times(&split.kept), [vec![100]]);
    assert_eq!(times(&split.extracted), [vec![0, 100]]);
}

#[test]
fn split_at_time_never_modifies_the_source() {
    let source = timed_note();
    let before = source.clone();
    split_at_time(&source, 1000, new_note("B")).unwrap();
    assert_eq!(source, before);
}

// --- split by zone ------------------------------------------------------------------------

/// A square from (0, 0) to (100, 100).
const SQUARE: [[f32; 2]; 4] = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];

fn zone_note() -> Note {
    let mut note = note_with(
        "Zone",
        vec![
            // Fully inside.
            stroke(0, &[(10.0, 10.0, 0), (20.0, 20.0, 10), (30.0, 30.0, 20)]),
            // Fully outside.
            stroke(0, &[(500.0, 500.0, 30), (510.0, 510.0, 40)]),
            // Exactly half inside (2 of 4): moves.
            stroke(
                0,
                &[
                    (50.0, 50.0, 50),
                    (60.0, 60.0, 60),
                    (300.0, 300.0, 70),
                    (310.0, 310.0, 80),
                ],
            ),
            // One of three inside: stays.
            stroke(
                0,
                &[(50.0, 50.0, 90), (300.0, 300.0, 100), (310.0, 310.0, 110)],
            ),
        ],
    );
    note.created = 5_000;
    note.timing = Timing::Synthesized;
    note
}

#[test]
fn split_by_zone_moves_strokes_with_at_least_half_their_samples_inside() {
    let source = zone_note();
    let split = split_by_zone(&source, &SQUARE, new_note("Zone")).unwrap();

    assert_eq!(
        split.extracted.strokes,
        [source.strokes[0].clone(), source.strokes[2].clone()]
    );
    assert_eq!(
        split.kept.strokes,
        [source.strokes[1].clone(), source.strokes[3].clone()]
    );
    // Positions, times and `created` are untouched.
    assert_eq!(split.extracted.created, source.created);
    assert_eq!(split.extracted.strokes[1].points[0].t, 50);
    assert_eq!(split.extracted.origin, Origin::Ardoise);
    assert!(!split.extracted.favorite);
    assert_eq!(split.extracted.timing, Timing::Synthesized);
    assert_eq!(split.extracted.id, "new-id");
    assert_eq!(split.kept.id, source.id);
    assert_eq!(split.kept.modified, NOW);
}

#[test]
fn split_by_zone_share_is_exactly_one_half() {
    let inside_of = |inside: usize, total: usize| {
        let points: Vec<(f32, f32, u32)> = (0..total)
            .map(|i| {
                if i < inside {
                    (50.0, 50.0, i as u32)
                } else {
                    (500.0, 500.0, i as u32)
                }
            })
            .collect();
        let source = note_with(
            "A",
            vec![stroke(0, &points), stroke(0, &[(900.0, 900.0, 0)])],
        );
        split_by_zone(&source, &SQUARE, new_note("B")).is_ok()
    };
    assert!(inside_of(1, 2));
    assert!(inside_of(2, 4));
    assert!(inside_of(2, 3));
    assert!(!inside_of(1, 3));
    assert!(!inside_of(0, 4));
    assert!(inside_of(4, 4));
}

#[test]
fn split_by_zone_ignores_hidden_layers_and_follows_missing_ones() {
    let mut source = zone_note();
    source.layers = vec![layer("Visible", true), layer("Cachée", false)];
    // Stroke 0 (inside) moves to the hidden layer: it cannot be selected.
    source.strokes[0].layer = 1;
    let split = split_by_zone(&source, &SQUARE, new_note("B")).unwrap();
    assert_eq!(split.extracted.strokes.len(), 1);
    assert_eq!(split.extracted.strokes[0].points[0].x, 50.0);
    assert_eq!(split.kept.strokes[0].layer, 1);
    // Layers are copied as they are, hidden ones included.
    assert_eq!(split.extracted.layers, source.layers);

    // Only a hidden stroke inside the zone: nothing to move.
    let hidden_only = {
        let mut note = note_with(
            "H",
            vec![
                stroke(1, &[(10.0, 10.0, 0)]),
                stroke(0, &[(500.0, 500.0, 5)]),
            ],
        );
        note.layers = source.layers.clone();
        note
    };
    assert_eq!(
        split_by_zone(&hidden_only, &SQUARE, new_note("B")).unwrap_err(),
        OpsError::NothingToSplit
    );

    // A stroke whose layer has no entry counts as visible (as in the editor).
    let orphan = note_with(
        "O",
        vec![
            stroke(5, &[(10.0, 10.0, 0)]),
            stroke(0, &[(500.0, 500.0, 5)]),
        ],
    );
    let split = split_by_zone(&orphan, &SQUARE, new_note("B")).unwrap();
    assert_eq!(split.extracted.strokes[0].layer, 5);
}

#[test]
fn split_by_zone_refuses_an_empty_side() {
    let source = zone_note();
    // Nothing inside.
    let far = [[1000.0, 1000.0], [1100.0, 1000.0], [1100.0, 1100.0]];
    assert_eq!(
        split_by_zone(&source, &far, new_note("B")).unwrap_err(),
        OpsError::NothingToSplit
    );
    // Everything inside: nothing stays.
    let all = [
        [-1.0e6, -1.0e6],
        [1.0e6, -1.0e6],
        [1.0e6, 1.0e6],
        [-1.0e6, 1.0e6],
    ];
    assert_eq!(
        split_by_zone(&source, &all, new_note("B")).unwrap_err(),
        OpsError::NothingToSplit
    );
    // Not a polygon.
    for polygon in [&SQUARE[..0], &SQUARE[..2]] {
        assert_eq!(
            split_by_zone(&source, polygon, new_note("B")).unwrap_err(),
            OpsError::NothingToSplit
        );
    }
}

#[test]
fn split_by_zone_uses_the_even_odd_rule_like_the_editor() {
    // A bow tie: the two triangles are inside, the crossing region is not "inside twice".
    let bow = [[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]];
    let source = note_with(
        "A",
        vec![
            stroke(0, &[(10.0, 50.0, 0)]), // left triangle: inside
            stroke(0, &[(90.0, 50.0, 1)]), // right triangle: inside
            stroke(0, &[(50.0, 10.0, 2)]), // top: outside
            stroke(0, &[(50.0, 90.0, 3)]), // bottom: outside
        ],
    );
    let split = split_by_zone(&source, &bow, new_note("B")).unwrap();
    assert_eq!(times(&split.extracted), [vec![0], vec![1]]);
    assert_eq!(times(&split.kept), [vec![2], vec![3]]);
}

/// The rule written the way `selectInLasso` does (share as a float, even-odd crossing test).
fn reference_selection(note: &Note, polygon: &[[f32; 2]]) -> Vec<usize> {
    let contains = |x: f64, y: f64| {
        let mut inside = false;
        let mut j = polygon.len() - 1;
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[j]);
            let (ax, ay, bx, by) = (a[0] as f64, a[1] as f64, b[0] as f64, b[1] as f64);
            if (ay > y) != (by > y) && x < ((bx - ax) * (y - ay)) / (by - ay) + ax {
                inside = !inside;
            }
            j = i;
        }
        inside
    };
    note.strokes
        .iter()
        .enumerate()
        .filter(|(_, s)| note.layers.get(s.layer as usize).is_none_or(|l| l.visible))
        .filter(|(_, s)| {
            let inside = s
                .points
                .iter()
                .filter(|p| contains(p.x as f64, p.y as f64))
                .count();
            inside > 0 && inside as f64 >= 0.5 * s.points.len() as f64
        })
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn split_by_zone_matches_the_lasso_rule_on_random_notes() {
    let mut rng = Lcg::new(7);
    let mut splits = 0;
    for _ in 0..600 {
        let mut note = rng.note();
        let count = 4 + rng.below(8) as usize;
        note.strokes = rng.strokes(count, 12);
        // Keep the strokes on a small page so that polygons catch some of them.
        for stroke in &mut note.strokes {
            stroke.layer %= note.layers.len() as u32;
            for p in &mut stroke.points {
                p.x = p.x.rem_euclid(30_000.0);
                p.y = p.y.rem_euclid(30_000.0);
            }
        }
        // A rough rectangle with jittered corners, sometimes with extra vertices.
        let (x0, y0) = (rng.unit() * 15_000.0, rng.unit() * 15_000.0);
        let (x1, y1) = (
            x0 + 5_000.0 + rng.unit() * 25_000.0,
            y0 + 5_000.0 + rng.unit() * 25_000.0,
        );
        let mut polygon = vec![
            [x0, y0],
            [x1, y0 + rng.unit() * 3_000.0],
            [x1, y1],
            [x0, y1],
        ];
        for _ in 0..rng.below(3) {
            polygon.push([rng.unit() * 30_000.0, rng.unit() * 30_000.0]);
        }
        let expected = reference_selection(&note, &polygon);
        match split_by_zone(&note, &polygon, new_note("Z")) {
            Ok(split) => {
                splits += 1;
                let moved: Vec<Stroke> =
                    expected.iter().map(|&i| note.strokes[i].clone()).collect();
                assert_eq!(split.extracted.strokes, moved);
                assert_eq!(split.kept.strokes.len() + moved.len(), note.strokes.len());
            }
            Err(error) => {
                assert_eq!(error, OpsError::NothingToSplit);
                let moved_points: usize =
                    expected.iter().map(|&i| note.strokes[i].points.len()).sum();
                let kept_points: usize = note
                    .strokes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !expected.contains(i))
                    .map(|(_, s)| s.points.len())
                    .sum();
                assert!(
                    moved_points == 0 || kept_points == 0,
                    "refused a valid split"
                );
            }
        }
    }
    assert!(
        splits > 20,
        "the random polygons never split anything ({splits})"
    );
}

#[test]
fn split_by_zone_never_modifies_the_source() {
    let source = zone_note();
    let before = source.clone();
    split_by_zone(&source, &SQUARE, new_note("B")).unwrap();
    assert_eq!(source, before);
}

#[test]
fn splits_survive_random_notes_without_losing_a_sample() {
    let mut rng = Lcg::new(11);
    for _ in 0..300 {
        let note = rng.note();
        let before = note.clone();
        let at = rng.next_u32() % 3000;
        if let Ok(split) = split_at_time(&note, at, new_note("T")) {
            assert_eq!(
                point_count(&split.kept) + point_count(&split.extracted),
                point_count(&note)
            );
            assert!(
                split
                    .kept
                    .strokes
                    .iter()
                    .flat_map(|s| &s.points)
                    .all(|p| p.t < at)
            );
            assert_eq!(time_range(&split.extracted).map(|r| r.0), Some(0));
            assert!(
                split
                    .extracted
                    .strokes
                    .iter()
                    .all(|s| (s.layer as usize) < note.layers.len())
            );
        }
        assert_eq!(note, before);
    }
}

// --- merge --------------------------------------------------------------------------------

fn page(width: u32, height: u32) -> PageSize {
    PageSize { width, height }
}

fn two_notes() -> [Note; 2] {
    let mut a = note_with(
        "Premier",
        vec![stroke(0, &[(100.0, 100.0, 50), (200.0, 100.0, 1000)])],
    );
    a.page = page(20_000, 10_000);
    a.created = 1_000;
    a.tags = vec!["Maths".into(), "cours".into()];
    a.background = Background {
        paper: Paper::Cream,
        template: Template::Seyes,
        texture: true,
    };
    let mut b = note_with(
        "Second",
        vec![stroke(0, &[(300.0, 400.0, 20), (350.0, 450.0, 220)])],
    );
    b.page = page(15_000, 25_000);
    b.created = 9_000;
    b.tags = vec!["maths".into(), "devoir".into()];
    [a, b]
}

#[test]
fn merge_needs_at_least_two_notes() {
    let [a, _] = two_notes();
    assert_eq!(
        merge(&[], MergeLayout::Overlay, new_note("M")).unwrap_err(),
        OpsError::NotEnough
    );
    assert_eq!(
        merge(&[a], MergeLayout::EndToEnd, new_note("M")).unwrap_err(),
        OpsError::NotEnough
    );
}

#[test]
fn merge_overlay_makes_one_page_as_large_as_the_largest() {
    let notes = two_notes();
    let merged = merge(&notes, MergeLayout::Overlay, new_note("Les deux")).unwrap();

    assert_eq!(merged.page, page(20_000, 25_000));
    assert_eq!(merged.id, "new-id");
    assert_eq!(merged.title, "Les deux");
    assert_eq!(merged.created, 1_000);
    assert_eq!(merged.modified, NOW);
    assert_eq!(merged.origin, Origin::Ardoise);
    assert!(!merged.favorite);
    assert_eq!(merged.background, notes[0].background);
    // Each single-layer note adds one layer named after its title.
    assert_eq!(
        merged.layers,
        [layer("Premier", true), layer("Second", true)]
    );
    assert_eq!(merged.strokes.len(), 2);
    assert_eq!(merged.strokes[0].layer, 0);
    assert_eq!(merged.strokes[1].layer, 1);
    // Positions unchanged.
    assert_eq!(merged.strokes[1].points[1].x, 350.0);
    assert_eq!(merged.strokes[1].points[1].y, 450.0);
}

#[test]
fn merge_overlay_layer_names() {
    let mut a = note_with(
        "Cours",
        vec![
            stroke(0, &[(1.0, 1.0, 0)]),
            stroke(1, &[(2.0, 2.0, 5)]),
            stroke(2, &[(3.0, 3.0, 9)]),
        ],
    );
    a.layers = vec![
        layer("Texte", true),
        layer("Figures", false),
        layer("Notes", true),
    ];
    let mut b = note_with("", vec![stroke(0, &[(1.0, 1.0, 0)])]);
    b.layers = vec![layer("Calque 1", true)];
    let mut c = note_with("Brouillon", vec![stroke(1, &[(1.0, 1.0, 0)])]);
    c.layers = vec![layer("Un", true), layer("Deux", true)];

    let merged = merge(&[a, b, c], MergeLayout::Overlay, new_note("M")).unwrap();
    let names: Vec<&str> = merged.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Cours - Texte",
            "Cours - Figures",
            "Cours - Notes",
            "Calque 1",
            "Brouillon - Un",
            "Brouillon - Deux"
        ]
    );
    // Visibility is kept, strokes follow their layers.
    assert!(!merged.layers[1].visible);
    let layers: Vec<u32> = merged.strokes.iter().map(|s| s.layer).collect();
    assert_eq!(layers, [0, 1, 2, 3, 5]);
}

#[test]
fn merge_end_to_end_stacks_the_pages() {
    let notes = two_notes();
    let merged = merge(&notes, MergeLayout::EndToEnd, new_note("Long")).unwrap();

    assert_eq!(merged.page, page(20_000, 35_000));
    assert_eq!(merged.strokes[0].points[0].y, 100.0);
    // The second note starts below the first page (10 000 high).
    assert_eq!(merged.strokes[1].points[0].y, 10_400.0);
    assert_eq!(merged.strokes[1].points[1].y, 10_450.0);
    assert_eq!(merged.strokes[1].points[1].x, 350.0);
}

#[test]
fn merge_end_to_end_merges_layers_by_index() {
    let mut a = note_with(
        "A",
        vec![stroke(0, &[(1.0, 1.0, 0)]), stroke(1, &[(2.0, 2.0, 1)])],
    );
    a.layers = vec![layer("Texte", true), layer("Figures", false)];
    let mut b = note_with(
        "B",
        vec![stroke(0, &[(1.0, 1.0, 0)]), stroke(2, &[(2.0, 2.0, 1)])],
    );
    b.layers = vec![
        layer("Autre", true),
        layer("Autre 2", true),
        layer("Troisième", true),
    ];

    let merged = merge(&[a, b], MergeLayout::EndToEnd, new_note("M")).unwrap();
    assert_eq!(
        merged.layers,
        [
            layer("Texte", true),
            // Hidden in A, visible in B: visible.
            layer("Figures", true),
            layer("Troisième", true)
        ]
    );
    let layers: Vec<u32> = merged.strokes.iter().map(|s| s.layer).collect();
    assert_eq!(layers, [0, 1, 0, 2]);
}

#[test]
fn merge_end_to_end_hides_a_layer_only_when_every_note_hides_it() {
    let mut a = note_with("A", vec![stroke(1, &[(1.0, 1.0, 0)])]);
    a.layers = vec![layer("Texte", true), layer("Brouillon", false)];
    let mut b = note_with("B", vec![stroke(1, &[(2.0, 2.0, 0)])]);
    b.layers = vec![layer("Texte", false), layer("Autre", true)];
    let mut c = note_with("C", vec![stroke(0, &[(3.0, 3.0, 0)])]);
    c.layers = vec![layer("Texte", false), layer("Brouillon", false)];

    let merged = merge(&[a, b, c], MergeLayout::EndToEnd, new_note("M")).unwrap();
    // Names from the first note; layer 0 visible in A, layer 1 visible in B.
    assert_eq!(
        merged.layers,
        [layer("Texte", true), layer("Brouillon", true)]
    );

    // Hidden everywhere: hidden.
    let mut d = note_with("D", vec![]);
    d.layers = vec![layer("Texte", false)];
    let mut e = note_with("E", vec![]);
    e.layers = vec![layer("Texte", false)];
    let merged = merge(&[d, e], MergeLayout::EndToEnd, new_note("M")).unwrap();
    assert_eq!(merged.layers, [layer("Texte", false)]);
}

#[test]
fn merge_makes_times_sequential() {
    let [a, b] = two_notes();
    let mut c = note_with(
        "Troisième",
        vec![stroke(0, &[(0.0, 0.0, 700), (1.0, 1.0, 800)])],
    );
    c.page = a.page;
    for layout in [MergeLayout::Overlay, MergeLayout::EndToEnd] {
        let merged = merge(&[a.clone(), b.clone(), c.clone()], layout, new_note("M")).unwrap();
        // First note keeps its times (50..1000); the second starts 1 s after 1000 (2000..2200);
        // the third 1 s after 2200 (3200..3300).
        assert_eq!(
            times(&merged),
            [vec![50, 1000], vec![2000, 2200], vec![3200, 3300]]
        );
    }
}

#[test]
fn merge_times_skip_notes_without_samples() {
    let [a, b] = two_notes();
    let empty = note_with("Vide", vec![stroke(0, &[])]);
    let merged = merge(
        &[empty.clone(), a, empty, b],
        MergeLayout::Overlay,
        new_note("M"),
    )
    .unwrap();
    assert_eq!(
        times(&merged),
        [vec![], vec![50, 1000], vec![], vec![2000, 2200]]
    );
}

#[test]
fn merge_tags_timing_and_background() {
    let [a, mut b] = two_notes();
    let merged = merge(&[a.clone(), b.clone()], MergeLayout::Overlay, new_note("M")).unwrap();
    // Union, first spelling kept, no duplicates.
    assert_eq!(merged.tags, ["Maths", "cours", "devoir"]);
    assert_eq!(merged.timing, Timing::Recorded);

    b.timing = Timing::Synthesized;
    for layout in [MergeLayout::Overlay, MergeLayout::EndToEnd] {
        let merged = merge(&[a.clone(), b.clone()], layout, new_note("M")).unwrap();
        assert_eq!(merged.timing, Timing::Synthesized);
    }
    let merged = merge(&[b, a.clone()], MergeLayout::Overlay, new_note("M")).unwrap();
    assert_eq!(merged.timing, Timing::Synthesized);
    assert_eq!(merged.created, 9_000);
}

#[test]
fn merge_refuses_pages_larger_than_100_metres() {
    let [mut a, mut b] = two_notes();
    a.page = page(MAX_PAGE_SIDE, 1000);
    b.page = page(1000, MAX_PAGE_SIDE);
    // Exactly 100 m is allowed for the overlay.
    let merged = merge(&[a.clone(), b.clone()], MergeLayout::Overlay, new_note("M")).unwrap();
    assert_eq!(merged.page, page(MAX_PAGE_SIDE, MAX_PAGE_SIDE));
    // Stacked, the height goes over.
    assert_eq!(
        merge(
            &[a.clone(), b.clone()],
            MergeLayout::EndToEnd,
            new_note("M")
        )
        .unwrap_err(),
        OpsError::PageTooLarge
    );
    // A single side over the limit.
    a.page = page(MAX_PAGE_SIDE + 1, 1000);
    assert_eq!(
        merge(&[a, b], MergeLayout::Overlay, new_note("M")).unwrap_err(),
        OpsError::PageTooLarge
    );
    // Sums that overflow u32.
    let mut c = note_with("C", vec![]);
    c.page = page(1000, u32::MAX);
    assert_eq!(
        merge(&[c.clone(), c], MergeLayout::EndToEnd, new_note("M")).unwrap_err(),
        OpsError::PageTooLarge
    );
}

#[test]
fn merge_copes_with_odd_notes() {
    // Strokes without samples, a stroke on a layer its note does not have, a note without layer.
    let mut a = note_with("A", vec![stroke(0, &[]), stroke(7, &[(1.0, 1.0, 5)])]);
    a.layers = vec![layer("Seule", true)];
    let mut b = note_with("B", vec![stroke(0, &[(2.0, 2.0, 1)])]);
    b.layers = vec![];
    for layout in [MergeLayout::Overlay, MergeLayout::EndToEnd] {
        let merged = merge(&[a.clone(), b.clone()], layout, new_note("M")).unwrap();
        assert!(!merged.layers.is_empty());
        assert!(
            merged
                .strokes
                .iter()
                .all(|s| (s.layer as usize) < merged.layers.len())
        );
        assert_eq!(merged.strokes.len(), 3);
        assert_eq!(merged.strokes[1].layer, 0);
    }
    // Only notes without layer: a layer is made up so the note stays valid.
    let merged = merge(&[b.clone(), b], MergeLayout::Overlay, new_note("Titre")).unwrap();
    assert_eq!(merged.layers, [layer("Titre", true)]);
    assert!(merged.strokes.iter().all(|s| s.layer == 0));
}

#[test]
fn merge_never_modifies_its_sources_and_is_finite() {
    let mut rng = Lcg::new(23);
    for _ in 0..200 {
        let notes: Vec<Note> = (0..2 + rng.below(3)).map(|_| rng.note()).collect();
        let before = notes.clone();
        for layout in [MergeLayout::Overlay, MergeLayout::EndToEnd] {
            if let Ok(merged) = merge(&notes, layout, new_note("M")) {
                assert_eq!(
                    merged.strokes.len(),
                    notes.iter().map(|n| n.strokes.len()).sum::<usize>()
                );
                assert!(
                    merged
                        .strokes
                        .iter()
                        .all(|s| (s.layer as usize) < merged.layers.len())
                );
                assert!(
                    merged
                        .strokes
                        .iter()
                        .flat_map(|s| &s.points)
                        .all(|p| p.x.is_finite() && p.y.is_finite())
                );
                assert!(merged.page.width > 0 && merged.page.height > 0);
            }
        }
        assert_eq!(notes, before);
    }
}

// --- background images --------------------------------------------------------------------

fn with_image(mut note: Note, name: &str) -> Note {
    note.background_image = Some(BackgroundImage {
        name: name.to_owned(),
        pdf_page: Some(2),
        rect: ImageRect {
            x: -500.0,
            y: 300.0,
            width: 21_000.0,
            height: 9_000.0,
        },
        quarter_turns: 1,
        opacity: 0.6,
    });
    note
}

#[test]
fn both_halves_of_a_split_keep_the_background_image() {
    let source = with_image(timed_note(), "fiche.pdf");
    let split = split_at_time(&source, 1000, new_note("Suite")).unwrap();
    assert_eq!(split.kept.background_image, source.background_image);
    assert_eq!(split.extracted.background_image, source.background_image);
    let source = with_image(zone_note(), "fiche.pdf");
    let split = split_by_zone(&source, &SQUARE, new_note("Zone")).unwrap();
    assert_eq!(split.kept.background_image, source.background_image);
    assert_eq!(split.extracted.background_image, source.background_image);
}

#[test]
fn merge_keeps_only_the_first_background_image_where_it_was() {
    let [a, b] = two_notes();
    let (a, b) = (with_image(a, "premier.png"), with_image(b, "second.png"));
    assert_eq!(dropped_images(&[a.clone(), b.clone()]), 1);
    assert_eq!(dropped_images(&[a.clone(), two_notes()[1].clone()]), 0);
    assert_eq!(dropped_images(&[two_notes()[0].clone(), b.clone()]), 1);
    for layout in [MergeLayout::Overlay, MergeLayout::EndToEnd] {
        let merged = merge(&[a.clone(), b.clone()], layout, new_note("M")).unwrap();
        // The first note stays at the top left, like its strokes: its image does not move.
        assert_eq!(merged.background_image, a.background_image, "{layout:?}");
        assert_eq!(merged.strokes[0].points, a.strokes[0].points);
        let merged = merge(&[two_notes()[0].clone(), b.clone()], layout, new_note("M")).unwrap();
        assert_eq!(merged.background_image, None, "{layout:?}");
    }
}

// --- Page objects ---------------------------------------------------------------------------

fn frame(id: &str, layer: u32, t: u32, x: f32, y: f32) -> PageObject {
    PageObject::text(
        id,
        layer,
        t,
        x,
        y,
        crate::note::TextBody {
            text: "Texte".to_owned(),
            font: ardoise_text::FontFamily::LatinModern,
            bold: false,
            size: 400.0,
            color: Rgba::BLACK,
        },
    )
}

fn ids(note: &Note) -> Vec<&str> {
    note.objects.iter().map(|o| o.id.as_str()).collect()
}

#[test]
fn time_range_counts_the_objects() {
    let mut note = note_with("A", vec![stroke(0, &[(0.0, 0.0, 100), (1.0, 1.0, 200)])]);
    note.objects.push(frame("o", 0, 5000, 0.0, 0.0));
    assert_eq!(time_range(&note), Some((100, 5000)));
    let mut only = note_with("B", vec![]);
    only.objects.push(frame("o", 0, 70, 0.0, 0.0));
    assert_eq!(time_range(&only), Some((70, 70)));
}

#[test]
fn split_at_time_moves_the_objects_that_appear_after_the_instant() {
    let mut note = note_with("A", vec![stroke(0, &[(0.0, 0.0, 100), (1.0, 1.0, 200)])]);
    note.objects = vec![
        frame("before", 0, 150, 0.0, 0.0),
        frame("after", 0, 900, 0.0, 0.0),
    ];
    let split = split_at_time(&note, 500, new_note("B")).unwrap();
    assert_eq!(ids(&split.kept), ["before"]);
    assert_eq!(ids(&split.extracted), ["after"]);
    // The new note starts with the object: its time is 0 and `created` moved by 900 ms.
    assert_eq!(split.extracted.objects[0].t, 0);
    assert_eq!(split.extracted.created, note.created + 900);
    assert!(split.extracted.strokes.is_empty());

    // Objects alone are enough on either side; nothing on one side is refused.
    let mut objects_only = note_with("C", vec![]);
    objects_only.objects = vec![frame("a", 0, 10, 0.0, 0.0), frame("b", 0, 20, 0.0, 0.0)];
    let split = split_at_time(&objects_only, 15, new_note("D")).unwrap();
    assert_eq!(
        (ids(&split.kept), ids(&split.extracted)),
        (vec!["a"], vec!["b"])
    );
    assert_eq!(
        split_at_time(&objects_only, 5, new_note("E")),
        Err(OpsError::NothingToSplit)
    );
    assert_eq!(
        split_at_time(&objects_only, 50, new_note("E")),
        Err(OpsError::NothingToSplit)
    );
}

#[test]
fn split_by_zone_moves_the_objects_whose_centre_is_inside_on_visible_layers() {
    let mut note = note_with(
        "A",
        vec![stroke(0, &[(9000.0, 9000.0, 0), (9100.0, 9000.0, 5)])],
    );
    note.layers.push(layer("Caché", false));
    let inside = frame("inside", 0, 0, 1000.0, 1000.0);
    let hidden = frame("hidden", 1, 0, 1000.0, 1000.0);
    let outside = frame("outside", 0, 0, 20_000.0, 1000.0);
    note.objects = vec![inside, hidden, outside];
    let zone = [[0.0, 0.0], [8000.0, 0.0], [8000.0, 8000.0], [0.0, 8000.0]];
    let split = split_by_zone(&note, &zone, new_note("B")).unwrap();
    assert_eq!(ids(&split.extracted), ["inside"]);
    assert_eq!(ids(&split.kept), ["hidden", "outside"]);
    assert_eq!(split.extracted.strokes.len(), 0);

    // A zone around an object alone is a split; a zone around nothing is not.
    let mut only = note_with("C", vec![]);
    only.objects = vec![
        frame("a", 0, 0, 1000.0, 1000.0),
        frame("b", 0, 0, 20_000.0, 1000.0),
    ];
    assert!(split_by_zone(&only, &zone, new_note("D")).is_ok());
    let far = [[50_000.0, 0.0], [60_000.0, 0.0], [60_000.0, 1.0]];
    assert_eq!(
        split_by_zone(&only, &far, new_note("D")),
        Err(OpsError::NothingToSplit)
    );
}

#[test]
fn merge_carries_the_objects_with_their_note() {
    let mut first = note_with("A", vec![stroke(0, &[(0.0, 0.0, 0), (1.0, 1.0, 1000)])]);
    first.page = page(1000, 2000);
    first.objects = vec![frame("same", 0, 500, 10.0, 20.0)];
    let mut second = note_with("B", vec![stroke(0, &[(0.0, 0.0, 0), (1.0, 1.0, 100)])]);
    second.page = page(1000, 3000);
    second.layers.push(layer("Deux", true));
    second.objects = vec![
        frame("same", 1, 50, 10.0, 20.0),
        frame("other", 7, 0, 0.0, 0.0),
    ];

    let overlay = merge(
        &[first.clone(), second.clone()],
        MergeLayout::Overlay,
        new_note("M"),
    )
    .unwrap();
    assert_eq!(overlay.objects.len(), 3);
    // Layers follow the note's (appended after the first note's), a missing one is its last.
    assert_eq!(
        overlay.objects.iter().map(|o| o.layer).collect::<Vec<_>>(),
        [0, 2, 2]
    );
    // Times follow the sequential times: the second note starts 1 s after the first ends.
    assert_eq!(overlay.objects[0].t, 500);
    assert_eq!(overlay.objects[1].t, 1000 + MERGE_GAP_MS + 50);
    // Ids stay unique.
    let mut seen = ids(&overlay);
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 3);
    assert_eq!(overlay.objects[0].id, "same");

    let stacked = merge(&[first, second], MergeLayout::EndToEnd, new_note("M")).unwrap();
    assert_eq!((stacked.objects[0].x, stacked.objects[0].y), (10.0, 20.0));
    assert_eq!((stacked.objects[1].x, stacked.objects[1].y), (10.0, 2020.0));
    assert_eq!(stacked.objects[1].layer, 1);
}

/// An equation (format 1.5) is moved by the operations like a text frame, through its placement:
/// its source and drawing come out untouched.
#[test]
fn equations_go_through_splits_and_merges_whole() {
    let equation = |id: &str, t: u32, x: f32| {
        PageObject::equation(
            id,
            0,
            t,
            [x, 1000.0],
            0.6,
            crate::note::EquationBody {
                latex: r"\ce{H2O}".to_owned(),
                path: "M0 0L500 0L500 300Z".to_owned(),
                color: Rgba::rgb(0, 0, 200),
            },
        )
    };
    let mut note = note_with("A", vec![stroke(0, &[(0.0, 0.0, 100), (1.0, 1.0, 200)])]);
    note.objects = vec![
        equation("early", 150, 1000.0),
        equation("late", 900, 20_000.0),
    ];
    let at = split_at_time(&note, 500, new_note("B")).unwrap();
    assert_eq!(
        (ids(&at.kept), ids(&at.extracted)),
        (vec!["early"], vec!["late"])
    );
    assert_eq!(
        at.extracted.objects[0].as_equation(),
        note.objects[1].as_equation()
    );
    let zone = [[0.0, 0.0], [8000.0, 0.0], [8000.0, 8000.0], [0.0, 8000.0]];
    let by_zone = split_by_zone(&note, &zone, new_note("C")).unwrap();
    assert_eq!(ids(&by_zone.extracted), ["early"]);
    let mut second = note_with("D", vec![stroke(0, &[(0.0, 0.0, 0), (1.0, 1.0, 100)])]);
    second.page = page(29_700, 21_600);
    second.objects = vec![equation("early", 0, 500.0)];
    let merged = merge(
        &[note.clone(), second],
        MergeLayout::EndToEnd,
        new_note("M"),
    )
    .unwrap();
    assert_eq!(merged.objects.len(), 3);
    assert!(
        merged
            .objects
            .iter()
            .all(|o| o.as_equation() == note.objects[0].as_equation())
    );
    assert_eq!(ids(&merged)[..2], ["early", "late"]);
}
