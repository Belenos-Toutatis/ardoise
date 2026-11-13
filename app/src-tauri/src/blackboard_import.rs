//! `ardoise.exe --import-blackboard-pages DIR [--library LIB]`: a one-time import of raw pages of
//! the Xiaomi blackboard kept outside the tablet (the pages Mi Home's capture received during the
//! study, docs/research/xiaomi-blackboard.md §3), without a window. The report stays in French,
//! like `--import-inkspace`'s.
//!
//! - Each `page-<unix time>.bin` of DIR is a page as the tablet sends it (7-byte records); the
//!   time in its name is when it was stored. `page-<unix time>.json` beside it, when there is
//!   one, gives the earlier store of the same screen that the page replaces (`{"previous": T}`,
//!   as the blackboard's rescue copies have it: docs/design/blackboard.md §9.2). Other files are
//!   left alone; a `.bin` that is not whole records of a known status (the sync's own check) is
//!   reported and not imported. The rescue folder (`_ardoise/rescued-pages`) can be given.
//! - Each page goes through the sync's own path ([`store_stored_page_in`]): the note is written
//!   atomically, read back and checked, with its thumbnail, dated by the page's time, then the
//!   automatic export runs as the settings say. A new note never overwrites anything
//!   (`Library::create` gives it a free name); a page that replaces an earlier store of its
//!   screen updates that store's note as the sync does (SPEC §4.1), or adds a note.
//! - **Running it twice imports nothing the second time**: the note of a page has an id derived
//!   from the page's time and bytes (`page_note_id`, the sync's), and a page whose note is in the
//!   library, or complete in its trash (the user deleted it), is skipped ([`ImportedPages`]), as
//!   the sync skips a page it already imported.
//! - The library is LIB, else this PC's library (the settings); its `device.json` and
//!   `blackboard.json` are not touched.

use std::path::{Path, PathBuf};

use ardoise_core::library::Library;
use ardoise_core::settings::{Orientation, SettingsStore};
use ardoise_device::pages::{BLACKBOARD_PAGE, ImportedPages, page_kind};
use ardoise_device::rescue::read_page_file;
use ardoise_device::service::{PageContext, Stored, StoredPage};
use ardoise_device::session::DownloadedPage;
use ardoise_device::transport::TabletKind;
use ardoise_proto::blackboard::check_page;

use crate::AppContext;
use crate::commands::current_state;
use crate::tablet::store_stored_page_in;

/// Options of the blackboard import mode.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BlackboardImportArgs {
    /// The folder of the `page-<unix time>.bin` files.
    pub pages: PathBuf,
    /// The library to import into (`--library`); this PC's library when absent.
    pub library: Option<PathBuf>,
}

/// What an import did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    /// The notes written, in the order of the pages' times.
    pub imported: Vec<PathBuf>,
    /// Of those, the notes of an earlier store of the same screen that a page updated.
    pub updated: Vec<PathBuf>,
    /// Pages whose note was already in the library or in its trash.
    pub skipped: usize,
    /// Files that could not become a note, and why.
    pub failed: Vec<(PathBuf, String)>,
}

/// The pages of `dir`: (time from the name, path), by time.
fn page_files(dir: &Path) -> Result<Vec<(u32, PathBuf)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut pages: Vec<(u32, PathBuf)> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
        })
        .filter_map(|path| {
            let time = path
                .file_stem()?
                .to_str()?
                .strip_prefix("page-")?
                .parse()
                .ok()?;
            Some((time, path))
        })
        .collect();
    pages.sort();
    Ok(pages)
}

/// Imports every page of `dir` into the library at `root` with `store` (the app's
/// [`store_stored_page_in`]), skipping the pages already imported.
pub fn import_pages(
    dir: &Path,
    root: &Path,
    store: &mut dyn FnMut(&Path, DownloadedPage, &PageContext) -> Result<StoredPage, String>,
) -> Result<ImportSummary, String> {
    let library = Library::open(root).map_err(|e| e.to_string())?;
    let mut imported = ImportedPages::scan(&library);
    let context = PageContext {
        size: BLACKBOARD_PAGE,
        orientation: Orientation::Landscape,
    };
    let mut summary = ImportSummary::default();
    for (time, path) in page_files(dir)? {
        let page = match read_page_file(&path, time) {
            Ok(page) => page,
            Err(error) => {
                summary.failed.push((path, error));
                continue;
            }
        };
        let bytes = &page.bytes;
        // The sync's own check: whole records of a known status (counts only in the report).
        let shape = check_page(bytes);
        if page_kind(bytes) != TabletKind::Blackboard || !shape.is_whole() {
            summary.failed.push((
                path,
                format!("not a page of the blackboard (whole 7-byte records): {shape}"),
            ));
            continue;
        }
        if imported.find(&library, &page).is_some() || imported.in_trash(&page) {
            summary.skipped += 1;
            continue;
        }
        let recorded = page.clone();
        match store(root, page, &context) {
            Ok(note) => {
                imported.record(&recorded, &note.path, note.created);
                if matches!(note.how, Stored::Updated { .. }) {
                    summary.updated.push(note.path.clone());
                }
                if !summary.imported.contains(&note.path) {
                    summary.imported.push(note.path);
                }
            }
            Err(error) => summary.failed.push((path, error)),
        }
    }
    Ok(summary)
}

/// Runs the import and returns the exit code: 0 when every page is in the library.
pub fn run_cli(args: &BlackboardImportArgs) -> i32 {
    let store = SettingsStore::open_default().ok();
    let locale = sys_locale::get_locale();
    let root = match &args.library {
        Some(library) => library.clone(),
        None => {
            let context = AppContext {
                store: store.clone().ok_or_else(|| "no settings".to_owned()),
                system_locale: locale.clone(),
                self_test: None,
                open_diagnostic: false,
            };
            match current_state(&context).library() {
                Some(library) => PathBuf::from(library),
                None => {
                    println!(
                        "Aucune bibliothèque : indiquez-la avec --library ou choisissez-la dans Ardoise."
                    );
                    return 1;
                }
            }
        }
    };
    println!("Pages : {}", args.pages.display());
    println!("Bibliothèque : {}", root.display());
    let mut save = |root: &Path, page: DownloadedPage, context: &PageContext| {
        store_stored_page_in(store.as_ref(), locale.as_deref(), root, page, context)
    };
    match import_pages(&args.pages, &root, &mut save) {
        Ok(summary) => {
            for note in &summary.imported {
                if summary.updated.contains(note) {
                    println!("Note mise à jour : {}", note.display());
                } else {
                    println!("Note enregistrée : {}", note.display());
                }
            }
            for (path, error) in &summary.failed {
                println!("Non importée : {} : {error}", path.display());
            }
            println!(
                "{} page(s) importée(s), {} déjà présente(s), {} en échec",
                summary.imported.len(),
                summary.skipped,
                summary.failed.len()
            );
            i32::from(!summary.failed.is_empty())
        }
        Err(error) => {
            println!("Échec de l'import : {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::Timing;
    use ardoise_sim::{SimBoard, reference_board_pages};

    #[test]
    fn pages_become_dated_notes_once_whatever_the_number_of_runs() {
        let pages = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        ardoise_core::library::init(library.path()).unwrap();
        let board = SimBoard::blackboard();
        for page in reference_board_pages().iter().take(2) {
            board.draw_page(page);
        }
        for page in board.stored_pages() {
            std::fs::write(
                pages.path().join(format!("page-{}.bin", page.time)),
                &page.bytes,
            )
            .unwrap();
        }
        // Left alone: another kind of file, a name without a time; refused: not records.
        std::fs::write(pages.path().join("page-1.svg"), "<svg/>").unwrap();
        std::fs::write(pages.path().join("notes.bin"), [0xa1; 7]).unwrap();
        std::fs::write(pages.path().join("page-7.bin"), [1, 2, 3]).unwrap();
        // Its first record is fine, the second is not of a known status.
        let mut misaligned = vec![0xa1, 0, 0, 0, 0, 1, 0];
        misaligned.extend([0x07, 0, 0, 0, 0, 0, 0]);
        std::fs::write(pages.path().join("page-8.bin"), misaligned).unwrap();
        let mut store = |root: &Path, page: DownloadedPage, context: &PageContext| {
            store_stored_page_in(None, Some("fr-FR"), root, page, context)
        };

        let first = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!(first.imported.len(), 2);
        assert_eq!(first.skipped, 0);
        assert_eq!(first.failed.len(), 2);
        assert!(
            first.failed[1]
                .1
                .ends_with("2 records, 1 of an unknown status, 0 bytes of a partial record")
        );
        let opened = Library::open(library.path()).unwrap();
        let mut created = Vec::new();
        for path in &first.imported {
            let file = opened.read(path).unwrap().file;
            assert_eq!(file.note.page, BLACKBOARD_PAGE);
            assert_eq!(file.note.timing, Timing::Synthesized);
            assert_eq!(file.note.layers[0].name, "Calque 1");
            assert!(file.thumbnail.is_some());
            created.push(file.note.created);
        }
        let times: Vec<i64> = board
            .stored_pages()
            .iter()
            .map(|p| i64::from(p.time) * 1000)
            .collect();
        assert_eq!(created, times);

        // Again: nothing new. A note the user deleted does not come back either.
        let second = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!((second.imported.len(), second.skipped), (0, 2));
        opened.trash(&first.imported[0]).unwrap();
        let third = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!((third.imported.len(), third.skipped), (0, 2));
        assert_eq!(Library::open(library.path()).unwrap().list().notes.len(), 1);
        // The library's tablets are not touched.
        assert!(
            ardoise_device::identity::load(library.path())
                .unwrap()
                .is_none()
        );
        assert!(
            ardoise_device::identity::load_blackboard(library.path())
                .unwrap()
                .is_none()
        );
        assert!(import_pages(&pages.path().join("missing"), library.path(), &mut store).is_err());
    }

    /// The rescue copies of the sync (docs/design/blackboard.md §9.2) are such files, a later
    /// store of a screen with its `previous` beside it: imported through the same store, it
    /// updates the note of the earlier store (SPEC §4.1), and nothing comes twice.
    #[test]
    fn a_later_store_of_a_screen_updates_the_note_of_the_earlier_one() {
        use ardoise_device::rescue::{RescueKind, RescueStore};
        let pages = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        ardoise_core::library::init(library.path()).unwrap();
        let board = SimBoard::blackboard();
        board.write(&reference_board_pages()[0]);
        board.press_store();
        let first = board.stored_pages()[0].clone();
        let mut store = |root: &Path, page: DownloadedPage, context: &PageContext| {
            store_stored_page_in(None, Some("fr-FR"), root, page, context)
        };
        let copies = RescueStore::new(None, Some(pages.path()));
        let as_page = |time: u32, bytes: &[u8], previous| DownloadedPage {
            announced_size: 0,
            bytes: bytes.to_vec(),
            timestamp: Some(i64::from(time)),
            kind: TabletKind::Blackboard,
            previous,
        };
        copies
            .save(&as_page(first.time, &first.bytes, None), RescueKind::Clean)
            .unwrap();
        let summary = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!((summary.imported.len(), summary.updated.len()), (1, 0));
        let note = summary.imported[0].clone();

        board.clock().advance(std::time::Duration::from_secs(60));
        board.write(&reference_board_pages()[1]);
        board.press_store();
        let second = board.stored_pages()[0].clone();
        copies
            .save(
                &as_page(second.time, &second.bytes, Some(first.time)),
                RescueKind::Clean,
            )
            .unwrap();
        let summary = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!(summary.skipped, 1, "the earlier page counts as imported");
        assert_eq!(summary.updated, std::slice::from_ref(&note));
        let opened = Library::open(library.path()).unwrap();
        assert_eq!(opened.list().notes.len(), 1);
        let file = opened.read(&note).unwrap().file;
        assert_eq!(file.original.as_deref(), Some(second.bytes.as_slice()));
        assert_eq!(file.note.created, i64::from(first.time) * 1000);
        // Again: both pages are recognised.
        let again = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!((again.imported.len(), again.skipped), (0, 2));
        // The user deletes the note: neither page comes back (the earlier one is recognised in
        // the trash by its time and bytes, the note carrying the later page's id).
        opened.trash(&note).unwrap();
        let trashed = import_pages(pages.path(), library.path(), &mut store).unwrap();
        assert_eq!((trashed.imported.len(), trashed.skipped), (0, 2));
        assert!(opened.list().notes.is_empty());
    }
}
