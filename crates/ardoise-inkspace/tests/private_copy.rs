//! Import of the user's private Inkspace copy (`testdata/private/`, git-ignored). Every test
//! here skips cleanly when the copy is absent. Nothing derived from it is written anywhere:
//! the tables go to the test output only.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use ardoise_core::note::Timing;
use ardoise_inkspace::compare;
use ardoise_inkspace::{ImportReport, Scores, THRESHOLDS, TrackedReader, import_local_state};

/// Research §9: 22 notes, 3874 strokes, 238 062 control points.
const NOTES: usize = 22;
const STROKES: usize = 3874;
const CONTROL_POINTS: usize = 238_062;

fn import(local_state: &std::path::Path) -> ImportReport {
    let reader = TrackedReader::new();
    let report = import_local_state(local_state, &reader).expect("import");
    // Privacy, on the real copy too: nothing under db\ but the notes' preview folders.
    let db = local_state.join("db");
    for access in reader.accesses() {
        if let Ok(rest) = access.path.strip_prefix(&db) {
            let parts: Vec<_> = rest.iter().map(|p| p.to_string_lossy()).collect();
            assert!(
                parts.len() >= 2 && parts[0] == "notes",
                "unexpected access {access:?}"
            );
            if parts.len() == 3 {
                assert!(
                    parts[2].starts_with("preview_"),
                    "unexpected access {access:?}"
                );
            }
        }
    }
    report
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn print_table(report: &ImportReport) {
    println!(
        "{:<8} {:>13} {:<9} {:>2} {:>5} {:>6} {:>6} {:>4} {:<11}",
        "note", "created", "page", "L", "str", "ctrl", "pts", "clip", "timing"
    );
    for n in &report.notes {
        let s = &n.stats;
        println!(
            "{:<8} {:>13} {:<9} {:>2} {:>5} {:>6} {:>6} {:>4} {:<11}",
            short(&n.inkspace_id),
            n.note.created,
            if n.note.page.width > n.note.page.height {
                "landscape"
            } else {
                "portrait"
            },
            s.layers,
            s.strokes,
            s.control_points,
            s.points,
            s.clipped_widths,
            format!("{:?}", s.timing)
        );
    }
}

#[test]
fn imports_all_22_notes() {
    let Some(local_state) = common::private_copy_or_skip("imports_all_22_notes") else {
        return;
    };
    let report = import(&local_state);
    print_table(&report);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.notes.len(), NOTES);
    let strokes: usize = report.notes.iter().map(|n| n.stats.strokes).sum();
    let control: usize = report.notes.iter().map(|n| n.stats.control_points).sum();
    let points: usize = report.notes.iter().map(|n| n.stats.points).sum();
    println!("total: {strokes} strokes, {control} control points, {points} points");
    assert_eq!(strokes, STROKES);
    assert_eq!(control, CONTROL_POINTS);
    // Every stroke of 3+ control points loses its two phantom ends.
    assert!(points <= CONTROL_POINTS - 2 * STROKES + 2 * 2);
    for n in &report.notes {
        assert!(
            n.preview.is_some(),
            "{} has no preview",
            short(&n.inkspace_id)
        );
        assert_eq!(n.note.strokes.len(), n.stats.strokes);
        assert!(n.note.created <= n.note.modified);
        let landscape = n.note.page == ardoise_core::note::A4_LANDSCAPE;
        let portrait = n.note.page == ardoise_core::note::A4_PORTRAIT;
        assert!(landscape || portrait);
        for stroke in &n.note.strokes {
            for p in &stroke.points {
                assert!((0.0..=1.0).contains(&p.pressure));
                assert!(p.x.is_finite() && p.y.is_finite());
            }
        }
    }
    let portraits = report
        .notes
        .iter()
        .filter(|n| n.note.page.height > n.note.page.width)
        .count();
    assert_eq!(portraits, 4);
    let synthesized = report
        .notes
        .iter()
        .filter(|n| n.stats.timing == Timing::Synthesized)
        .count();
    println!("timing synthesised on {synthesized} notes");
    // The same input always gives the same notes (ids included).
    assert_eq!(import(&local_state), report);
}

fn print_scores(label: &str, rows: &[(String, Scores)]) {
    println!("{label}");
    println!(
        "{:<8} {:>6} {:>10} {:>11} {:>7} {:>7}  result",
        "note", "IoU", "render<=1", "preview<=1", "inkR", "inkP"
    );
    for (id, s) in rows {
        println!(
            "{:<8} {:>6.3} {:>10.4} {:>11.4} {:>7} {:>7}  {}",
            short(id),
            s.iou,
            s.render_within_1px,
            s.preview_within_1px,
            s.render_ink,
            s.preview_ink,
            if s.passes(&THRESHOLDS) { "OK" } else { "FAIL" }
        );
    }
    let n = rows.len().max(1) as f64;
    let mean = |f: fn(&Scores) -> f64| rows.iter().map(|(_, s)| f(s)).sum::<f64>() / n;
    let min = |f: fn(&Scores) -> f64| rows.iter().map(|(_, s)| f(s)).fold(1.0, f64::min);
    println!(
        "mean IoU {:.3} (min {:.3}), render<=1 min {:.4}, preview<=1 min {:.4}",
        mean(|s| s.iou),
        min(|s| s.iou),
        min(|s| s.render_within_1px),
        min(|s| s.preview_within_1px)
    );
}

/// The import test of ROADMAP M2: every note imported from the private copy, rendered by
/// `ardoise_render::render_note`, must match Inkspace's own preview (geometry within 1 px both
/// ways, ink masks overlapping with IoU above the threshold).
#[test]
fn renderer_matches_inkspace_previews() {
    let Some(local_state) = common::private_copy_or_skip("renderer_matches_inkspace_previews")
    else {
        return;
    };
    let report = import(&local_state);
    let reader = TrackedReader::new();
    let mut rows = Vec::new();
    for n in &report.notes {
        let scores = ardoise_inkspace::compare_with_preview(&reader, n).expect("comparison");
        rows.push((n.inkspace_id.clone(), scores));
    }
    print_scores("ardoise_render::render_note vs Inkspace previews", &rows);
    let failing: Vec<_> = rows
        .iter()
        .filter(|(_, s)| !s.passes(&THRESHOLDS))
        .map(|(id, s)| format!("{} {s:?}", short(id)))
        .collect();
    assert!(
        failing.is_empty(),
        "{} of {} notes below the thresholds {:?}: {failing:#?}",
        failing.len(),
        rows.len(),
        THRESHOLDS
    );
}

/// Stroke class for the tuning: sampling rate read back from the point times (5 ms = 200 pps,
/// ~17 ms = 60 pps), or "synth" on notes with synthesised timing.
fn stroke_class(note: &ardoise_core::note::Note, stroke: &ardoise_ink::Stroke) -> &'static str {
    if note.timing == Timing::Synthesized {
        return "synth";
    }
    match stroke.points.as_slice() {
        [a, b, ..] if b.t.saturating_sub(a.t) <= 8 => "200",
        [_, _, ..] => "60",
        _ => "short",
    }
}

/// Rescales the drawn diameters of the strokes of `class`: d' = scale * d + offset (device
/// units), re-deriving the nominal width and the pressures like the importer does.
fn rescale(note: &mut ardoise_core::note::Note, class: &str, scale: f32, offset: f32) {
    use ardoise_inkspace::width;
    let original = note.clone();
    for (stroke, before) in note.strokes.iter_mut().zip(&original.strokes) {
        if stroke_class(&original, before) != class {
            continue;
        }
        let diameters: Vec<f32> = before
            .points
            .iter()
            .map(|p| (scale * width::width_at(before.width, p.pressure) + offset).max(1.0))
            .collect();
        stroke.width = width::stroke_width_for(diameters.iter().copied());
        for (p, d) in stroke.points.iter_mut().zip(diameters) {
            p.pressure = width::pressure_for_width(stroke.width, d);
        }
    }
}

/// Width tuning (slow; run in release):
/// `cargo test -p ardoise-inkspace --release --test private_copy -- --ignored --nocapture`.
/// For each stroke class, rescales that class's diameters (on top of the import) and prints
/// the mean IoU of the notes containing it, against Inkspace's previews.
#[test]
#[ignore = "tuning tool, slow in debug builds"]
fn tune_width_factor() {
    let Some(local_state) = common::private_copy_or_skip("tune_width_factor") else {
        return;
    };
    let report = import(&local_state);
    let reader = TrackedReader::new();
    let previews: Vec<_> = report
        .notes
        .iter()
        .map(|n| {
            compare::load_preview(&reader, n.preview.as_deref().expect("preview"))
                .expect("preview png")
        })
        .collect();
    let scales = [
        0.80f32, 0.85, 0.90, 0.95, 1.00, 1.05, 1.10, 1.15, 1.20, 1.25, 1.30,
    ];
    let offsets = [-10.0f32, -5.0, 0.0, 5.0, 10.0];
    for class in ["200", "60", "synth"] {
        let members: Vec<usize> = (0..report.notes.len())
            .filter(|&i| {
                let note = &report.notes[i].note;
                note.strokes.iter().any(|s| stroke_class(note, s) == class)
            })
            .collect();
        let count: usize = members
            .iter()
            .map(|&i| {
                let note = &report.notes[i].note;
                note.strokes
                    .iter()
                    .filter(|s| stroke_class(note, s) == class)
                    .count()
            })
            .sum();
        println!(
            "class {class}: {count} strokes on {} notes; rows = offset (device units), \
             columns = scale on top of the import",
            members.len()
        );
        print!("{:>7}", "");
        for s in scales {
            print!(" {s:>6.2}");
        }
        println!();
        for offset in offsets {
            print!("{offset:>7.1}");
            for scale in scales {
                let mut sum = 0.0;
                for &i in &members {
                    let mut note = report.notes[i].note.clone();
                    rescale(&mut note, class, scale, offset);
                    let rendered =
                        ardoise_render::render_note(&note, &compare::preview_render_options())
                            .expect("render");
                    sum += compare::compare_images(&rendered, &previews[i])
                        .expect("same size")
                        .iou;
                }
                print!(" {:>6.4}", sum / members.len().max(1) as f64);
            }
            println!();
        }
    }
}

/// Rescales every drawn diameter of `note` by `scale`, re-deriving the nominal widths and the
/// pressures like the importer does.
fn rescale_all(note: &mut ardoise_core::note::Note, scale: f32) {
    use ardoise_inkspace::width;
    for stroke in &mut note.strokes {
        let diameters: Vec<f32> = stroke
            .points
            .iter()
            .map(|p| (scale * width::width_at(stroke.width, p.pressure)).max(1.0))
            .collect();
        stroke.width = width::stroke_width_for(diameters.iter().copied());
        for (p, d) in stroke.points.iter_mut().zip(diameters) {
            p.pressure = width::pressure_for_width(stroke.width, d);
        }
    }
}

/// Held-out check of the width calibration. The factors (1.0 timed, 1.1 untimed) were chosen
/// by looking at these 22 notes, so scoring the same notes is not an independent proof. Here
/// the notes are split in two halves (alternating by creation date); for each half, the
/// factors are tuned from scratch (neutral widths = Inkspace's stored widths, best mean IoU
/// per timing class over candidate factors) on the OTHER half only, then the held-out half is
/// scored with them. Every held-out note must pass `THRESHOLDS`.
#[test]
fn width_calibration_holds_on_notes_it_was_not_tuned_on() {
    use ardoise_inkspace::width::{TIMED_WIDTH_FACTOR, UNTIMED_WIDTH_FACTOR};
    let Some(local_state) =
        common::private_copy_or_skip("width_calibration_holds_on_notes_it_was_not_tuned_on")
    else {
        return;
    };
    let mut notes = import(&local_state).notes;
    notes.sort_by_key(|n| n.note.created);
    let reader = TrackedReader::new();
    let candidates = [0.80f32, 0.85, 0.90, 0.95, 1.00, 1.05, 1.10, 1.15, 1.20];
    let timed = |n: &ardoise_inkspace::ImportedNote| n.note.timing == Timing::Recorded;

    // Scores of every note at every candidate factor, from neutral widths.
    let scores: Vec<Vec<Scores>> = notes
        .iter()
        .map(|n| {
            let preview = compare::load_preview(&reader, n.preview.as_deref().expect("preview"))
                .expect("preview png");
            let current = if timed(n) {
                TIMED_WIDTH_FACTOR
            } else {
                UNTIMED_WIDTH_FACTOR
            };
            candidates
                .iter()
                .map(|&factor| {
                    let mut note = n.note.clone();
                    rescale_all(&mut note, factor / current);
                    let rendered =
                        ardoise_render::render_note(&note, &compare::preview_render_options())
                            .expect("render");
                    compare::compare_images(&rendered, &preview).expect("same size")
                })
                .collect()
        })
        .collect();

    println!("two-fold cross-validation of the width factors (thresholds {THRESHOLDS:?})");
    let mut failing = Vec::new();
    for held_out in 0..2 {
        let training: Vec<usize> = (0..notes.len()).filter(|i| i % 2 != held_out).collect();
        // Best candidate per timing class, on the training half only.
        let best = |want_timed: bool| -> usize {
            (0..candidates.len())
                .max_by(|&a, &b| {
                    let mean = |c: usize| {
                        let members: Vec<f64> = training
                            .iter()
                            .filter(|&&i| timed(&notes[i]) == want_timed)
                            .map(|&i| scores[i][c].iou)
                            .collect();
                        members.iter().sum::<f64>() / members.len().max(1) as f64
                    };
                    mean(a).total_cmp(&mean(b))
                })
                .expect("candidates")
        };
        let (best_timed, best_untimed) = (best(true), best(false));
        println!(
            "fold {}: tuned on {} notes -> timed x{:.2}, untimed x{:.2}; held-out notes:",
            held_out + 1,
            training.len(),
            candidates[best_timed],
            candidates[best_untimed]
        );
        for i in (0..notes.len()).filter(|i| i % 2 == held_out) {
            let s = &scores[i][if timed(&notes[i]) {
                best_timed
            } else {
                best_untimed
            }];
            let pass = s.passes(&THRESHOLDS);
            println!(
                "  {}  {}  IoU {:.3}  render<=1 {:.4}  preview<=1 {:.4}  {}",
                short(&notes[i].inkspace_id),
                if timed(&notes[i]) {
                    "timed  "
                } else {
                    "untimed"
                },
                s.iou,
                s.render_within_1px,
                s.preview_within_1px,
                if pass { "OK" } else { "FAIL" }
            );
            if !pass {
                failing.push(short(&notes[i].inkspace_id).to_owned());
            }
        }
    }
    // Without any tuning at all: Inkspace's stored widths drawn as they are (factor 1.0).
    let neutral = candidates
        .iter()
        .position(|&c| c == 1.0)
        .expect("factor 1.0");
    let neutral_failing: Vec<String> = (0..notes.len())
        .filter(|&i| !scores[i][neutral].passes(&THRESHOLDS))
        .map(|i| short(&notes[i].inkspace_id).to_owned())
        .collect();
    let min_iou = (0..notes.len())
        .map(|i| scores[i][neutral].iou)
        .fold(f64::INFINITY, f64::min);
    println!(
        "no tuning (factor 1.0 for every note): min IoU {min_iou:.3}, {} of {} notes pass",
        notes.len() - neutral_failing.len(),
        notes.len()
    );
    assert!(
        failing.is_empty(),
        "held-out notes below the thresholds: {failing:?}"
    );
    assert!(
        neutral_failing.is_empty(),
        "notes below the thresholds without tuning: {neutral_failing:?}"
    );
}
