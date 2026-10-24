//! `ardoise.exe --import-inkspace [LocalState] [--library DIR] [--compare]`: imports the
//! notes of Wacom Inkspace without a window and prints a table.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ardoise_core::format::NoteFile;
use ardoise_core::library::Library;
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

    let saved = match &args.library {
        Some(library) => match save_into_library(library, &report) {
            Ok(saved) => {
                println!(
                    "Bibliothèque {} : {} note(s) enregistrée(s) dans le classeur « {BINDER} », \
                     {} déjà présente(s)",
                    library.display(),
                    saved.written,
                    saved.skipped
                );
                true
            }
            Err(error) => {
                println!("Échec de l'enregistrement dans la bibliothèque : {error}");
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

/// Binder receiving imported notes.
const BINDER: &str = "Inkspace";

struct Saved {
    written: usize,
    skipped: usize,
}

/// Writes every imported note into `library` (binder [`BINDER`], atomic writes, original dates
/// in the file names). Notes already imported (same id, derived from Inkspace's) are skipped,
/// so importing twice adds nothing.
fn save_into_library(library: &Path, report: &ImportReport) -> Result<Saved, String> {
    ardoise_core::library::init(library).map_err(|e| e.to_string())?;
    let library = Library::open(library).map_err(|e| e.to_string())?;
    let existing: HashSet<String> = library
        .list()
        .notes
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    let mut saved = Saved {
        written: 0,
        skipped: 0,
    };
    for imported in &report.notes {
        if existing.contains(&imported.note.id) {
            saved.skipped += 1;
            continue;
        }
        let mut file = NoteFile::new(imported.note.clone());
        file.thumbnail = thumbnail(&imported.note);
        library
            .create(Path::new(BINDER), &file)
            .map_err(|e| format!("{}: {e}", short(&imported.inkspace_id)))?;
        saved.written += 1;
    }
    Ok(saved)
}

/// Thumbnail width in pixels.
const THUMBNAIL_WIDTH: u32 = 320;

/// A PNG thumbnail of `note`, or `None` if it cannot be made (the note is saved anyway).
fn thumbnail(note: &ardoise_core::note::Note) -> Option<Vec<u8>> {
    let options = ardoise_render::RenderOptions::fit_width(note.page, THUMBNAIL_WIDTH);
    let image = ardoise_render::render_note(note, &options).ok()?;
    ardoise_render::encode_png(&image).ok()
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
