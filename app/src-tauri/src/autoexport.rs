//! Automatic export of each synced note (SPEC §3.2): the after-save hook of
//! `ardoise_device::pages::LibrarySink`, writing `<library>/<folder>/<binder>/<stem>.pdf|.png`
//! so that the notes can be read from a phone through the Nextcloud app.
//!
//! One file per note, named like the note file and rewritten atomically when the note is
//! synced again: PDF at the note's actual size, or PNG at 150 dpi, with the paper. A failed
//! export never fails the sync (the note is safe in the library): the hook reports it to its
//! caller, which logs it.

// The automatic sync (milestone M4) goes through `LibrarySink` with this hook; until it is
// wired, only the tests below call it.
#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};

use ardoise_core::fsutil;
use ardoise_core::library::{Library, SavedNote};
use ardoise_core::note::Note;
use ardoise_core::settings::{AutoExport, AutoExportFormat};
use ardoise_render::{
    PdfOptions, PdfPage, RenderError, RenderOptions, encode_png_at, notes_to_pdf, render_note,
};

/// Resolution of the automatic PNG exports: sharp on a phone, about 1 MB per page.
pub const AUTO_EXPORT_DPI: u32 = 150;

/// Why a note was not exported.
#[derive(Debug, thiserror::Error)]
pub enum AutoExportError {
    #[error("the automatic export folder {0:?} is not a folder inside the library")]
    InvalidFolder(String),
    #[error("{} is not a note of the library", .0.display())]
    NotInLibrary(PathBuf),
    #[error("cannot render the note: {0}")]
    Render(#[from] RenderError),
    #[error("cannot write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
}

/// Where the export of the note stored at `note_path` goes, or `None` when the automatic
/// export is off.
pub fn export_path(
    settings: &AutoExport,
    library: &Library,
    note_path: &Path,
) -> Result<Option<PathBuf>, AutoExportError> {
    let extension = match settings.format {
        AutoExportFormat::Off => return Ok(None),
        AutoExportFormat::Pdf => "pdf",
        AutoExportFormat::Png => "png",
    };
    let folder = settings
        .relative_folder()
        .ok_or_else(|| AutoExportError::InvalidFolder(settings.folder.clone()))?;
    let not_in_library = || AutoExportError::NotInLibrary(note_path.to_path_buf());
    let binder = note_path
        .parent()
        .and_then(|dir| dir.strip_prefix(library.root()).ok())
        .ok_or_else(not_in_library)?;
    let stem = note_path.file_stem().ok_or_else(not_in_library)?;
    let mut name = stem.to_os_string();
    name.push(".");
    name.push(extension);
    Ok(Some(library.root().join(folder).join(binder).join(name)))
}

/// The file of `note` in the automatic export format, `None` when it is off.
pub fn export_bytes(format: AutoExportFormat, note: &Note) -> Result<Option<Vec<u8>>, RenderError> {
    match format {
        AutoExportFormat::Off => Ok(None),
        AutoExportFormat::Pdf => {
            let options = PdfOptions {
                page: PdfPage::Actual,
                background: true,
            };
            notes_to_pdf(&[note], &options).map(Some)
        }
        AutoExportFormat::Png => {
            let image = render_note(note, &RenderOptions::at_dpi(AUTO_EXPORT_DPI, true))?;
            encode_png_at(&image, Some(AUTO_EXPORT_DPI)).map(Some)
        }
    }
}

/// Exports `note`, just saved at `saved.path`, as the settings say. Returns the file written,
/// `None` when the automatic export is off.
pub fn auto_export(
    settings: &AutoExport,
    library: &Library,
    saved: &SavedNote,
    note: &Note,
) -> Result<Option<PathBuf>, AutoExportError> {
    let Some(path) = export_path(settings, library, &saved.path)? else {
        return Ok(None);
    };
    let Some(bytes) = export_bytes(settings.format, note)? else {
        return Ok(None);
    };
    let write_error = |source| AutoExportError::Write {
        path: path.clone(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(write_error)?;
    }
    fsutil::write_atomic(&path, &bytes).map_err(write_error)?;
    Ok(Some(path))
}

/// What the automatic export did during one sync.
#[derive(Debug, Default)]
pub struct AutoExportReport {
    /// Files written, in order.
    pub written: Vec<PathBuf>,
    /// Notes not exported, with the reason.
    pub failures: Vec<(PathBuf, String)>,
}

impl AutoExportReport {
    pub fn record(
        &mut self,
        saved: &SavedNote,
        outcome: &Result<Option<PathBuf>, AutoExportError>,
    ) {
        match outcome {
            Ok(Some(path)) => self.written.push(path.clone()),
            Ok(None) => {}
            Err(error) => self.failures.push((saved.path.clone(), error.to_string())),
        }
    }
}

/// The after-save hook for `LibrarySink::with_after_save`: exports every note the sink
/// stores, and hands each outcome to `on_outcome` (to record or log it). It never fails the
/// sync.
pub fn after_save_hook<'a>(
    settings: &'a AutoExport,
    library: &'a Library,
    mut on_outcome: impl FnMut(&SavedNote, &Result<Option<PathBuf>, AutoExportError>) + 'a,
) -> impl FnMut(&SavedNote, &Note) + 'a {
    move |saved, note| {
        let outcome = auto_export(settings, library, saved, note);
        on_outcome(saved, &outcome);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use ardoise_core::note::{A4_LANDSCAPE, PageSize};
    use ardoise_device::log::DiagnosticLog;
    use ardoise_device::manager::Manager;
    use ardoise_device::pages::LibrarySink;
    use ardoise_sim::{SimTablet, drawings};

    use super::*;
    use crate::library::tests::{library, sample_note, write_sample};

    const WAIT: Duration = Duration::from_secs(10);

    fn settings(format: AutoExportFormat, folder: &str) -> AutoExport {
        AutoExport {
            format,
            folder: folder.into(),
            ..AutoExport::default()
        }
    }

    fn pdf_media_boxes(bytes: &[u8]) -> Vec<Vec<f32>> {
        let document = lopdf::Document::load_mem(bytes).expect("the PDF parses");
        document
            .get_pages()
            .values()
            .map(|&id| {
                document
                    .get_dictionary(id)
                    .unwrap()
                    .get(b"MediaBox")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_float().unwrap())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn exports_go_to_the_folder_and_binder_of_the_note() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "Classe/3e B", "Électricité");
        let note = library.read(&saved.path).unwrap().file.note;
        let stem = saved
            .path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let pdf = auto_export(
            &settings(AutoExportFormat::Pdf, "Exports"),
            &library,
            &saved,
            &note,
        )
        .unwrap()
        .unwrap();
        let expected = library
            .root()
            .join("Exports")
            .join("Classe")
            .join("3e B")
            .join(format!("{stem}.pdf"));
        assert_eq!(pdf, expected);
        let boxes = pdf_media_boxes(&fs::read(&pdf).unwrap());
        assert_eq!(boxes.len(), 1);
        assert!((boxes[0][2] - 841.89).abs() < 0.01 && (boxes[0][3] - 612.28).abs() < 0.01);

        // A sub-folder written with backslashes, PNG at 150 dpi.
        let png = auto_export(
            &settings(AutoExportFormat::Png, "\\Partage\\Téléphone\\"),
            &library,
            &saved,
            &note,
        )
        .unwrap()
        .unwrap();
        assert!(png.starts_with(library.root().join("Partage").join("Téléphone")));
        let image = ardoise_render::decode_png(&fs::read(&png).unwrap()).unwrap();
        assert_eq!((image.width, image.height), (1754, 1276));

        // Off: nothing.
        let off = auto_export(
            &settings(AutoExportFormat::Off, "Exports"),
            &library,
            &saved,
            &note,
        );
        assert!(off.unwrap().is_none());

        // Exports are not notes of the library.
        assert_eq!(library.list().notes.len(), 1);
    }

    #[test]
    fn a_note_synced_again_is_exported_again_over_the_same_file() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let mut note = library.read(&saved.path).unwrap().file.note;
        let settings = settings(AutoExportFormat::Pdf, "Exports");
        let first = auto_export(&settings, &library, &saved, &note)
            .unwrap()
            .unwrap();
        let before = fs::read(&first).unwrap();
        note.page = PageSize {
            width: 14_800,
            height: 21_000,
        };
        let second = auto_export(&settings, &library, &saved, &note)
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        let after = fs::read(&second).unwrap();
        assert_ne!(before, after);
        assert!((pdf_media_boxes(&after)[0][2] - 419.53).abs() < 0.01);
        // No temporary file left next to it.
        let dir = second.parent().unwrap();
        assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
    }

    #[test]
    fn bad_folders_and_failures_are_reported_not_raised() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let note = library.read(&saved.path).unwrap().file.note;
        for folder in ["", "..", "../dehors", "_ardoise/exports", "C:\\Exports"] {
            let error = auto_export(
                &settings(AutoExportFormat::Pdf, folder),
                &library,
                &saved,
                &note,
            )
            .unwrap_err();
            assert!(
                matches!(error, AutoExportError::InvalidFolder(_)),
                "{folder}: {error}"
            );
        }
        // A file where the folder should be.
        fs::write(library.root().join("Bloqué"), "x").unwrap();
        let mut report = AutoExportReport::default();
        {
            let blocked = settings(AutoExportFormat::Png, "Bloqué");
            let mut hook = after_save_hook(&blocked, &library, |saved, outcome| {
                report.record(saved, outcome)
            });
            hook(&saved, &note);
            // An unrenderable note.
            let mut empty = sample_note("Vide");
            empty.page = PageSize {
                width: 0,
                height: 0,
            };
            hook(&saved, &empty);
        }
        assert!(report.written.is_empty());
        assert_eq!(report.failures.len(), 2);
        assert!(
            report.failures[0].1.contains("cannot write"),
            "{:?}",
            report.failures
        );
        assert!(
            report.failures[1].1.contains("render"),
            "{:?}",
            report.failures
        );
        // A path outside the library.
        let outside = SavedNote {
            path: PathBuf::from("/elsewhere/2026-10-24 X.ardoise"),
            ..saved.clone()
        };
        let error = auto_export(
            &settings(AutoExportFormat::Pdf, "Exports"),
            &library,
            &outside,
            &note,
        )
        .unwrap_err();
        assert!(matches!(error, AutoExportError::NotInLibrary(_)));
    }

    /// The proof of the ROADMAP item "Export automatique PDF/PNG après synchro": a simulated
    /// Slate with three drawn pages is registered and synced through `LibrarySink`; every page
    /// becomes a note of the library and, with the export on, a PDF or PNG next to it.
    #[test]
    fn synced_pages_are_exported_automatically() {
        for format in [
            AutoExportFormat::Pdf,
            AutoExportFormat::Png,
            AutoExportFormat::Off,
        ] {
            let (_dir, library) = library();
            let tablet = SimTablet::slate_large();
            let config = tablet.config();
            for page in drawings::reference_pages(config.width, config.height) {
                tablet.draw_page(&page);
            }
            assert_eq!(tablet.stored_page_count(), 3);
            let log = DiagnosticLog::in_memory("sim", tablet.clock()).shared();
            let mut manager = Manager::new(
                Box::new(tablet.connector()),
                library.root().to_path_buf(),
                log,
            );
            tablet.hold_button();
            let found = manager.find(true, WAIT).unwrap().expect("tablet found");
            let (mut session, registration) = manager
                .register(&found, WAIT, &mut || tablet.press_button())
                .unwrap();
            let page = PageSize {
                width: registration.info.width,
                height: registration.info.height,
            };
            assert_eq!(page, A4_LANDSCAPE);

            let settings = settings(format, "Exports");
            let mut report = AutoExportReport::default();
            let (summary, saved) = {
                let mut sink =
                    LibrarySink::new(&library, "Tablette", page, "Calque 1", || 1_792_843_200_000)
                        .with_after_save(after_save_hook(&settings, &library, |saved, outcome| {
                            report.record(saved, outcome)
                        }));
                let summary = session
                    .sync(true, &mut |page, _, _| sink.store(page).map(|_| ()))
                    .unwrap();
                (summary, sink.saved.clone())
            };
            assert_eq!((summary.downloaded, summary.deleted), (3, 3), "{format:?}");
            assert_eq!(tablet.stored_page_count(), 0);

            // Three notes in the library, in the binder of the tablet.
            let listing = library.list();
            assert_eq!(listing.notes.len(), 3, "{format:?}");
            assert!(listing.problems.is_empty());
            assert!(
                listing
                    .notes
                    .iter()
                    .all(|n| n.binder == Path::new("Tablette"))
            );
            assert!(report.failures.is_empty(), "{:?}", report.failures);
            let exports = library.root().join("Exports").join("Tablette");
            if format == AutoExportFormat::Off {
                assert!(report.written.is_empty());
                assert!(!library.root().join("Exports").exists());
                continue;
            }

            // One file per note, named like it.
            assert_eq!(report.written.len(), 3, "{format:?}");
            assert_eq!(fs::read_dir(&exports).unwrap().count(), 3);
            for (note, written) in saved.iter().zip(&report.written) {
                let stem = note
                    .path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let extension = if format == AutoExportFormat::Pdf {
                    "pdf"
                } else {
                    "png"
                };
                assert_eq!(*written, exports.join(format!("{stem}.{extension}")));
                let bytes = fs::read(written).unwrap();
                if format == AutoExportFormat::Pdf {
                    let boxes = pdf_media_boxes(&bytes);
                    assert_eq!(boxes.len(), 1, "one page per note");
                    assert!((boxes[0][2] - 841.89).abs() < 0.01);
                    assert!((boxes[0][3] - 612.28).abs() < 0.01);
                } else {
                    let image = ardoise_render::decode_png(&bytes).unwrap();
                    assert_eq!((image.width, image.height), (1754, 1276));
                    // Ink was drawn: some dark pixels.
                    let dark = image
                        .pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter(|p| p[0] < 100)
                        .count();
                    assert!(dark > 100, "{dark}");
                }
            }
        }
    }
}
