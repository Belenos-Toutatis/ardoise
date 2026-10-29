//! A large demo library for the performance measure (`cargo xtask perf`,
//! `ardoise.exe --create-demo-library DIR --demo-notes 1000`): after the 8 demo notes, copies of
//! them with their own ids, titles, dates, tags and binders, so that the library looks like one
//! a teacher filled over a few school years. Deterministic, like the demo notes.

use super::{DAY_MS, DemoNote, FIRST_DAY_MS, demo_id, demo_notes};

/// Binders the generated notes are spread over (about 40 notes each for 1 000 notes).
const BINDERS: [&str; 24] = [
    "Classe/6e A",
    "Classe/6e B",
    "Classe/5e A",
    "Classe/5e B",
    "Classe/4e A",
    "Classe/4e B",
    "Classe/3e A",
    "Classe/3e B",
    "Classe/3e C",
    "Classe/3e D",
    "Cours/Électricité",
    "Cours/Optique",
    "Cours/Mécanique",
    "Cours/Chimie",
    "Cours/Ondes",
    "Réunions",
    "Réunions/Conseils de classe",
    "Brouillons",
    "Perso",
    "Perso/Idées",
    "Projets/Club sciences",
    "Projets/Salon",
    "Préparation/Évaluations",
    "",
];

/// Ids of generated notes start here (the demo notes use 1 to 8).
const FIRST_INDEX: u64 = 1000;

/// The notes to add after the 8 demo notes so that the library holds `total` of them (none when
/// `total` is 8 or less).
pub(super) fn extra_notes(total: usize) -> Vec<DemoNote> {
    let bases = demo_notes();
    let extra = total.saturating_sub(bases.len());
    (0..extra)
        .map(|i| {
            let base = &bases[i % bases.len()];
            let mut note = base.note.clone();
            note.id = demo_id(FIRST_INDEX + i as u64);
            let name = if base.note.title.is_empty() {
                "Séance"
            } else {
                base.note.title.as_str()
            };
            note.title = format!("{name} {}", i + 1);
            // Spread over a school year and a half, in an order unrelated to the base note.
            let day = (i as i64 * 7) % 540;
            note.created = FIRST_DAY_MS + day * DAY_MS + (i as i64 % 600) * 60 * 1000;
            note.modified = note.created + (1 + i as i64 % 5) * 60 * 60 * 1000;
            note.favorite = i % 17 == 0;
            note.tags.push(format!("séance {}", i % 30));
            DemoNote {
                binder: BINDERS[(i * 7) % BINDERS.len()],
                note,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_library_reaches_the_requested_size() {
        assert!(extra_notes(0).is_empty());
        assert!(extra_notes(8).is_empty());
        assert_eq!(extra_notes(9).len(), 1);
        assert_eq!(extra_notes(1000).len(), 992);
    }

    #[test]
    fn generated_notes_are_valid_and_unique() {
        let demo = demo_notes();
        let extra = extra_notes(1000);
        let ids: HashSet<&str> = demo
            .iter()
            .chain(&extra)
            .map(|d| d.note.id.as_str())
            .collect();
        assert_eq!(ids.len(), 1000);
        let titles: HashSet<&str> = extra.iter().map(|d| d.note.title.as_str()).collect();
        assert_eq!(titles.len(), extra.len());
        let binders: HashSet<&str> = extra.iter().map(|d| d.binder).collect();
        assert_eq!(binders.len(), BINDERS.len());
        for d in &extra {
            assert!(!d.note.strokes.is_empty());
            assert!(d.note.modified > d.note.created);
            assert!(
                ardoise_core::format::to_bytes(&ardoise_core::format::NoteFile::new(
                    d.note.clone()
                ))
                .is_ok()
            );
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a: Vec<_> = extra_notes(50)
            .into_iter()
            .map(|d| (d.binder, d.note))
            .collect();
        let b: Vec<_> = extra_notes(50)
            .into_iter()
            .map(|d| (d.binder, d.note))
            .collect();
        assert_eq!(a, b);
    }
}
