//! `ardoise.exe --import-inkspace [LocalState] [--library DIR] [--compare]`: imports the
//! notes of Wacom Inkspace without a window and prints a table.

use std::path::PathBuf;

use ardoise_core::note::Timing;
use ardoise_inkspace::{
    ImportReport, ImportedNote, Scores, THRESHOLDS, TrackedReader, compare_with_preview,
    import_local_state, locate_local_state,
};

/// Options of the Inkspace import mode.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportArgs {
    /// Inkspace's `LocalState` folder; located automatically when absent.
    pub local_state: Option<PathBuf>,
    /// Library to import into (`--library DIR`).
    pub library: Option<PathBuf>,
    /// `--compare`: render each note and score it against Inkspace's preview.
    pub compare: bool,
}

/// Runs the import and returns the exit code: 0 only when every note imported (and, with
/// `--compare`, every note is above the thresholds).
pub fn run_cli(args: &ImportArgs) -> i32 {
    let reader = TrackedReader::new();
    let local_state = match &args.local_state {
        Some(dir) => dir.clone(),
        None => match locate_local_state(&reader) {
            Some(dir) => dir,
            None => {
                println!(
                    "Inkspace introuvable (%LOCALAPPDATA%\\Packages\\{}*\\LocalState) : \
                     indiquez le dossier LocalState.",
                    ardoise_inkspace::PACKAGE_PREFIX
                );
                println!("0/0 notes importées");
                return 1;
            }
        },
    };
    println!("Source : {}", local_state.display());

    let report = match import_local_state(&local_state, &reader) {
        Ok(report) => report,
        Err(error) => {
            println!("Échec de l'import : {error}");
            println!("0/0 notes importées");
            return 1;
        }
    };

    let all_pass = print_table(&report, &reader, args.compare);

    // TODO(integration): save every `imported.note` into the library (`args.library`, else the
    // configured one) in the binder `Inkspace` through ardoise-core's library API (atomic
    // writes, original dates in the file name), skipping notes whose
    // `Origin::Inkspace { id }` is already in the library, once the `.ardoise` format exists.
    if let Some(library) = &args.library {
        println!(
            "Remarque : l'enregistrement dans la bibliothèque ({}) n'est pas encore disponible ; \
             aucune note n'a été écrite.",
            library.display()
        );
    }

    println!("{}/{} notes importées", report.notes.len(), report.total());
    if report.failures.is_empty() && all_pass {
        0
    } else {
        1
    }
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn local_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "?".into())
}

fn orientation(note: &ImportedNote) -> &'static str {
    if note.note.page.width > note.note.page.height {
        "paysage"
    } else {
        "portrait"
    }
}

fn timing(note: &ImportedNote) -> &'static str {
    match note.stats.timing {
        Timing::Recorded => "enregistré",
        Timing::Synthesized => "synthétisé",
    }
}

/// Prints one row per note (and per failure); returns whether every comparison passed.
fn print_table(report: &ImportReport, reader: &TrackedReader, compare: bool) -> bool {
    let mut header = format!(
        "{:<8}  {:<16}  {:<8}  {:>7}  {:>6}  {:>6}  {:<10}",
        "Note", "Date", "Page", "Calques", "Traits", "Points", "Minutage"
    );
    if compare {
        header.push_str(&format!(
            "  {:>5}  {:>10}  {:>11}  Résultat",
            "IoU", "Rendu≤1px", "Aperçu≤1px"
        ));
    }
    println!("{header}");

    let mut all_pass = true;
    for imported in &report.notes {
        let mut row = format!(
            "{:<8}  {:<16}  {:<8}  {:>7}  {:>6}  {:>6}  {:<10}",
            short(&imported.inkspace_id),
            local_date(imported.note.created),
            orientation(imported),
            imported.stats.layers,
            imported.stats.strokes,
            imported.stats.points,
            timing(imported),
        );
        if compare {
            match compare_with_preview(reader, imported) {
                Ok(scores) => {
                    let pass = scores.passes(&THRESHOLDS);
                    all_pass &= pass;
                    row.push_str(&score_columns(&scores, pass));
                }
                Err(error) => {
                    all_pass = false;
                    row.push_str(&format!("  ÉCHEC : {error}"));
                }
            }
        }
        println!("{row}");
    }
    for failure in &report.failures {
        println!(
            "{:<8}  non importée : {}",
            short(&failure.inkspace_id),
            failure.error
        );
    }
    all_pass
}

fn score_columns(scores: &Scores, pass: bool) -> String {
    format!(
        "  {:>5.3}  {:>10.4}  {:>11.4}  {}",
        scores.iou,
        scores.render_within_1px,
        scores.preview_within_1px,
        if pass { "OK" } else { "FAIL" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_folder_fails_with_exit_code_1() {
        let dir = tempfile::tempdir().unwrap();
        let args = ImportArgs {
            local_state: Some(dir.path().join("missing")),
            library: None,
            compare: true,
        };
        assert_eq!(run_cli(&args), 1);
    }

    #[test]
    fn dates_and_ids() {
        assert_eq!(short("01234567-89ab-4cde-8f01-23456789abcd"), "01234567");
        assert_eq!(short("abc"), "abc");
        assert_eq!(local_date(0).len(), 16);
        assert_eq!(local_date(i64::MAX), "?");
    }
}
