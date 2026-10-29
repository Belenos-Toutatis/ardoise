//! `ardoise.exe --import-inkspace [LocalState] [--library DIR] [--compare]`: imports the
//! notes of Wacom Inkspace without a window and prints a table. Same detection, reading and
//! saving as the interface's import ([`super`]); the report stays in French.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ardoise_core::library::Library;
use ardoise_core::note::Timing;
use ardoise_inkspace::{
    ImportReport, ImportedNote, Scores, THRESHOLDS, TrackedReader, compare_with_preview,
    local_state_in, locate_local_state,
};

use super::{DEFAULT_BINDER, LibraryImport, NOTE_DAMAGED, read_notes, save_into_library};

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
        // The package folder, `WacomCloudDb`… lead to their LocalState; anything else is read
        // as given, and fails with the reason.
        Some(dir) => local_state_in(&reader, dir).unwrap_or_else(|| dir.clone()),
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

    let report = match read_notes(&local_state, &reader) {
        Ok(report) => report,
        Err(error) => {
            println!("Échec de l'import : {}", error.parts().1);
            println!("0/0 notes importées");
            return 1;
        }
    };

    let all_pass = print_table(&report, &reader, args.compare);

    let saved = match &args.library {
        Some(library) => match save_into(library, &report) {
            Ok(saved) => {
                println!(
                    "Bibliothèque {} : {} note(s) enregistrée(s) dans le classeur \
                     « {DEFAULT_BINDER} », {} déjà présente(s)",
                    library.display(),
                    saved.imported,
                    saved.skipped
                );
                // Notes that cannot be decoded are already in the table.
                let unwritten: Vec<_> = saved
                    .failed
                    .iter()
                    .filter(|failure| failure.code != NOTE_DAMAGED)
                    .collect();
                for failure in &unwritten {
                    println!(
                        "Non enregistrée : {}: {}",
                        short(&failure.inkspace_id),
                        failure.message
                    );
                }
                unwritten.is_empty()
            }
            Err(error) => {
                println!("Échec de l'enregistrement dans la bibliothèque : {}", error);
                false
            }
        },
        None => {
            println!("Aucune bibliothèque indiquée (--library) : rien n'a été enregistré.");
            true
        }
    };

    println!("{}/{} notes importées", report.notes.len(), report.total());
    if report.failures.is_empty() && all_pass && saved {
        0
    } else {
        1
    }
}

/// Saves into `library` (made a library if needed), binder [`DEFAULT_BINDER`], skipping the
/// notes already imported.
fn save_into(library: &Path, report: &ImportReport) -> Result<LibraryImport, String> {
    ardoise_core::library::init(library).map_err(|e| e.to_string())?;
    let library = Library::open(library).map_err(|e| e.to_string())?;
    save_into_library(
        &library,
        Path::new(DEFAULT_BINDER),
        report,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .map_err(|error| error.parts().1.to_owned())
}

/// `ardoise.exe --create-demo-inkspace DIR`: writes made-up Inkspace data
/// (`ardoise_inkspace::synthetic::demo_notes`) into DIR, which then plays the part of
/// Inkspace's `LocalState` folder: for the end-to-end journeys, and to try the import on a PC
/// without Inkspace. DIR must be new or empty, so real Inkspace data can never be touched.
pub fn run_demo_cli(dir: &Path) -> i32 {
    if dir.as_os_str().is_empty() {
        println!("Usage : ardoise.exe --create-demo-inkspace DOSSIER");
        return 2;
    }
    let occupied = std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some());
    if occupied || dir.is_file() {
        println!(
            "{} n'est pas un dossier vide : rien n'a été écrit.",
            dir.display()
        );
        return 1;
    }
    let notes = ardoise_inkspace::synthetic::demo_notes();
    match ardoise_inkspace::synthetic::write_note_store(dir, &notes) {
        Ok(()) => {
            println!(
                "Données Inkspace de démonstration : {} note(s) dans {}",
                notes.len(),
                dir.display()
            );
            0
        }
        Err(error) => {
            println!("Échec de l'écriture des données de démonstration : {error}");
            1
        }
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
    if compare {
        println!(
            "Seuils : IoU ≥ {:.2} ; encre à moins d'1 px (dans les deux sens) ≥ {:.2}",
            THRESHOLDS.iou, THRESHOLDS.within_1px
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
