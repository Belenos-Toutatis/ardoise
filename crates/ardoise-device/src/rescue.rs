//! Rescue copies of the Xiaomi blackboard's pages (docs/design/blackboard.md §9.2).
//!
//! The tablet removes an *open* page (the page of a screen not erased since it was stored,
//! described in the long form) by itself once it has been downloaded (research note §6): from
//! then on, the bytes the host received are the only copy. They are written here before
//! anything can fail, and removed only once their note is written and read back.
//!
//! - `<library>/_ardoise/rescued-pages/page-<time>.bin`: the raw bytes as the tablet sent them
//!   (the files `--import-blackboard-pages` reads), with `page-<time>.json` beside them when the
//!   page replaces an earlier store of its screen (`{"previous": <time>}`). When that folder
//!   cannot be written, the same under the fallback folder (the app's local data,
//!   `%LOCALAPPDATA%\Ardoise\blackboard-rescue`).
//! - *Clean* copies are imported at the next sync. *Doubtful* copies (a transfer that did not end
//!   cleanly) go to the `doubtful` subfolder and are never made into notes on their own.
//! - Every write is atomic (SPEC §3.4).

use std::fs;
use std::path::{Path, PathBuf};

use ardoise_core::fsutil::{remove_file_with_retry, write_atomic};

use crate::session::DownloadedPage;
use crate::transport::TabletKind;

/// The folder of the rescue copies under `_ardoise/` (or the fallback folder).
pub const RESCUE_DIR: &str = "rescued-pages";
/// The subfolder of the doubtful copies.
pub const DOUBTFUL_DIR: &str = "doubtful";

/// Whether a copy can become a note as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RescueKind {
    /// Received cleanly (count, quiet window, record check): imported at the next sync.
    Clean,
    /// From a transfer that did not end cleanly: kept apart, never imported on its own.
    Doubtful,
}

impl RescueKind {
    fn name(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Doubtful => "doubtful",
        }
    }
}

/// A rescue copy on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescuedFile {
    /// When the page was stored on the tablet (Unix seconds).
    pub time: u32,
    pub kind: RescueKind,
    /// The `.bin` file.
    pub path: PathBuf,
}

/// The rescue folders of one library: its own `_ardoise/rescued-pages`, then the fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RescueStore {
    folders: Vec<PathBuf>,
}

impl RescueStore {
    /// The rescue folders of the library at `library` (none without one), then `fallback`.
    pub fn new(library: Option<&Path>, fallback: Option<&Path>) -> Self {
        let mut folders = Vec::new();
        if let Some(library) = library {
            folders.push(
                library
                    .join(ardoise_core::library::META_DIR)
                    .join(RESCUE_DIR),
            );
        }
        if let Some(fallback) = fallback {
            folders.push(fallback.to_path_buf());
        }
        Self { folders }
    }

    fn folder(base: &Path, kind: RescueKind) -> PathBuf {
        match kind {
            RescueKind::Clean => base.to_path_buf(),
            RescueKind::Doubtful => base.join(DOUBTFUL_DIR),
        }
    }

    /// Writes `page` as a copy of `kind`, atomically, in the first folder that can be written;
    /// returns its path. A doubtful copy with more bytes of the same page is kept rather than
    /// replaced by fewer. `Err` (every folder failed) says why.
    pub fn save(&self, page: &DownloadedPage, kind: RescueKind) -> Result<PathBuf, String> {
        let time = page_time(page);
        if kind == RescueKind::Doubtful
            && let Some(existing) = self.find(time, kind)
            && fs::metadata(&existing.path).is_ok_and(|m| m.len() >= page.bytes.len() as u64)
        {
            return Ok(existing.path);
        }
        let mut failures = Vec::new();
        for base in &self.folders {
            let dir = Self::folder(base, kind);
            match write_copy(&dir, time, page) {
                Ok(path) => return Ok(path),
                Err(error) => failures.push(format!("{}: {error}", dir.display())),
            }
        }
        if failures.is_empty() {
            failures.push("no rescue folder (no library)".into());
        }
        Err(failures.join("; "))
    }

    /// Every copy of `kind`, oldest page first (a page found in two folders is listed twice).
    pub fn pages(&self, kind: RescueKind) -> Vec<RescuedFile> {
        let mut found = Vec::new();
        for base in &self.folders {
            let dir = Self::folder(base, kind);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(time) = bin_time(&path)
                    && path.is_file()
                {
                    found.push(RescuedFile { time, kind, path });
                }
            }
        }
        found.sort_by(|a, b| (a.time, &a.path).cmp(&(b.time, &b.path)));
        found
    }

    /// Whether a copy of `kind` waits.
    pub fn has(&self, kind: RescueKind) -> bool {
        !self.pages(kind).is_empty()
    }

    fn find(&self, time: u32, kind: RescueKind) -> Option<RescuedFile> {
        self.pages(kind).into_iter().find(|file| file.time == time)
    }

    /// The page of a copy, as the tablet sent it ([`read_page_file`]).
    pub fn read(&self, file: &RescuedFile) -> Result<DownloadedPage, String> {
        read_page_file(&file.path, file.time)
    }

    /// The copy of page `time` is doubtful after all: written again as doubtful, then the clean
    /// one is removed. Best effort; `Err` leaves the clean copy (it is then imported at the next
    /// sync, never lost).
    pub fn doubt(&self, time: u32) -> Result<(), String> {
        for file in self.pages(RescueKind::Clean) {
            if file.time != time {
                continue;
            }
            let page = self.read(&file)?;
            self.save(&page, RescueKind::Doubtful)?;
            remove_copy(&file.path);
        }
        Ok(())
    }

    /// The note of page `time` is written and read back: every copy of it goes, clean and
    /// doubtful. Best effort (a copy left is recognised as imported at the next sync).
    pub fn release(&self, time: u32) {
        for kind in [RescueKind::Clean, RescueKind::Doubtful] {
            for file in self.pages(kind) {
                if file.time == time {
                    remove_copy(&file.path);
                }
            }
        }
    }

    /// Removes one copy (a clean copy whose note is in the library).
    pub fn remove(&self, file: &RescuedFile) {
        remove_copy(&file.path);
    }
}

/// The page's time as the copy's name has it (0 for a page without a time, never expected).
fn page_time(page: &DownloadedPage) -> u32 {
    page.timestamp
        .and_then(|t| u32::try_from(t).ok())
        .unwrap_or(0)
}

/// `page-<time>.bin`, and its `.json` when the page has a `previous` (written first: a `.bin`
/// never goes without it).
fn write_copy(dir: &Path, time: u32, page: &DownloadedPage) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let side = dir.join(format!("page-{time}.json"));
    match page.previous {
        Some(previous) => {
            let text = serde_json::json!({ "previous": previous }).to_string();
            write_atomic(&side, text.as_bytes())?;
        }
        None if side.exists() => remove_file_with_retry(&side)?,
        None => {}
    }
    let path = dir.join(format!("page-{time}.bin"));
    write_atomic(&path, &page.bytes)?;
    Ok(path)
}

fn remove_copy(bin: &Path) {
    let _ = remove_file_with_retry(bin);
    let side = bin.with_extension("json");
    if side.exists() {
        let _ = remove_file_with_retry(&side);
    }
}

/// The time of a `page-<time>.bin` file, `None` for any other name.
pub fn bin_time(path: &Path) -> Option<u32> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
    {
        return None;
    }
    path.file_stem()?
        .to_str()?
        .strip_prefix("page-")?
        .parse()
        .ok()
}

/// A raw page of the blackboard kept in a file: `page-<time>.bin` (`time`, from its name),
/// with the `previous` of `page-<time>.json` beside it when there is one (a `.json` that cannot
/// be read gives no `previous`: the page then never updates an earlier note, the safe side).
pub fn read_page_file(path: &Path, time: u32) -> Result<DownloadedPage, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let previous = fs::read(path.with_extension("json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("previous")?.as_u64())
        .and_then(|previous| u32::try_from(previous).ok())
        .filter(|previous| *previous != 0);
    Ok(DownloadedPage {
        announced_size: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
        bytes,
        timestamp: Some(i64::from(time)),
        kind: TabletKind::Blackboard,
        previous,
    })
}

/// One line for the log: `clean copy`, `doubtful copy`.
pub fn describe(kind: RescueKind) -> String {
    format!("{} copy", kind.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(time: u32, bytes: &[u8], previous: Option<u32>) -> DownloadedPage {
        DownloadedPage {
            bytes: bytes.to_vec(),
            timestamp: Some(i64::from(time)),
            announced_size: bytes.len() as u32,
            kind: TabletKind::Blackboard,
            previous,
        }
    }

    #[test]
    fn copies_are_written_listed_doubted_and_released() {
        let library = tempfile::tempdir().unwrap();
        ardoise_core::library::init(library.path()).unwrap();
        let store = RescueStore::new(Some(library.path()), None);
        assert!(!store.has(RescueKind::Clean));
        let path = store
            .save(
                &page(1_800_000_002, &[0xa1; 14], Some(1_800_000_001)),
                RescueKind::Clean,
            )
            .unwrap();
        assert_eq!(
            path,
            library
                .path()
                .join("_ardoise")
                .join(RESCUE_DIR)
                .join("page-1800000002.bin")
        );
        store
            .save(&page(1_800_000_001, &[0xa0; 7], None), RescueKind::Clean)
            .unwrap();
        let clean = store.pages(RescueKind::Clean);
        assert_eq!(
            clean.iter().map(|f| f.time).collect::<Vec<_>>(),
            [1_800_000_001, 1_800_000_002]
        );
        let read = store.read(&clean[1]).unwrap();
        assert_eq!(read, page(1_800_000_002, &[0xa1; 14], Some(1_800_000_001)));
        assert_eq!(store.read(&clean[0]).unwrap().previous, None);

        // Doubtful after all: moved apart, with its previous.
        store.doubt(1_800_000_002).unwrap();
        assert_eq!(store.pages(RescueKind::Clean).len(), 1);
        let doubtful = store.pages(RescueKind::Doubtful);
        assert_eq!(doubtful.len(), 1);
        assert!(doubtful[0].path.parent().unwrap().ends_with(DOUBTFUL_DIR));
        assert_eq!(
            store.read(&doubtful[0]).unwrap().previous,
            Some(1_800_000_001)
        );
        // Fewer bytes never replace more.
        store
            .save(&page(1_800_000_002, &[0xa1; 7], None), RescueKind::Doubtful)
            .unwrap();
        assert_eq!(store.read(&doubtful[0]).unwrap().bytes.len(), 14);
        store
            .save(
                &page(1_800_000_002, &[0xa1; 21], None),
                RescueKind::Doubtful,
            )
            .unwrap();
        assert_eq!(store.read(&doubtful[0]).unwrap().bytes.len(), 21);
        assert_eq!(store.read(&doubtful[0]).unwrap().previous, None, "replaced");

        // The note written: every copy of the page goes, its .json too.
        store.release(1_800_000_002);
        assert!(store.pages(RescueKind::Doubtful).is_empty());
        let dir = library.path().join("_ardoise").join(RESCUE_DIR);
        assert!(!dir.join(DOUBTFUL_DIR).join("page-1800000002.json").exists());
        store.release(1_800_000_001);
        assert!(!store.has(RescueKind::Clean));
    }

    #[test]
    fn the_fallback_folder_takes_a_copy_the_library_cannot() {
        let library = tempfile::tempdir().unwrap();
        ardoise_core::library::init(library.path()).unwrap();
        // A file where the folder should be: the library's rescue folder cannot be made.
        std::fs::write(library.path().join("_ardoise").join(RESCUE_DIR), b"x").unwrap();
        let local = tempfile::tempdir().unwrap();
        let store = RescueStore::new(Some(library.path()), Some(local.path()));
        let path = store
            .save(&page(1_800_000_003, &[0xa1; 7], None), RescueKind::Clean)
            .unwrap();
        assert_eq!(path, local.path().join("page-1800000003.bin"));
        assert_eq!(store.pages(RescueKind::Clean).len(), 1);
        // Nowhere at all: an error that names the folders tried.
        let nowhere = RescueStore::new(Some(library.path()), None);
        let error = nowhere
            .save(&page(1_800_000_004, &[0xa1; 7], None), RescueKind::Clean)
            .unwrap_err();
        assert!(error.contains(RESCUE_DIR), "{error}");
        assert!(
            RescueStore::default()
                .save(&page(1, &[0xa1; 7], None), RescueKind::Clean)
                .is_err()
        );
    }

    #[test]
    fn only_page_files_are_copies() {
        assert_eq!(bin_time(Path::new("page-17.bin")), Some(17));
        assert_eq!(bin_time(Path::new("page-17.BIN")), Some(17));
        assert_eq!(bin_time(Path::new("page-17.json")), None);
        assert_eq!(bin_time(Path::new("page-x.bin")), None);
        assert_eq!(bin_time(Path::new("notes.bin")), None);
    }
}
