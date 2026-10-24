//! Filesystem helpers shared by everything that writes to the user's data.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// How many times a rename refused because another process holds the target open is tried.
/// Sync clients (Nextcloud, OneDrive) and antivirus scanners open files briefly without
/// sharing them.
const RENAME_ATTEMPTS: u32 = 10;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(50);

/// Builds a unique temporary path next to `path` (same directory, so the final rename stays
/// on one volume and is atomic). The `.part` suffix is in the default ignore list of
/// Nextcloud and ownCloud, so the temporary file is never uploaded.
fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(".{name}.{}.{unique}.part", std::process::id());
    path.with_file_name(temp_name)
}

/// Writes `bytes` to `path` atomically.
///
/// The data goes to a temporary file in the same directory, is flushed and synced to disk, and
/// then renamed over the target (`std::fs::rename` replaces an existing file on Windows). A
/// reader, or a sync client such as Nextcloud, never sees a half-written target. On error the
/// temporary file is removed and the target is left untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = temp_path_for(path);
    let result = write_and_rename(&temp, path, bytes);
    if result.is_err() {
        // Best effort: the original error is the one worth reporting.
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_and_rename(temp: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    rename_with_retry(temp, target)
}

fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Err(error) if attempt < RENAME_ATTEMPTS && is_transient_lock(&error) => {
                attempt += 1;
                std::thread::sleep(RENAME_RETRY_DELAY);
            }
            result => return result,
        }
    }
}

/// Windows reports a file held open by another process as "access denied" (5) or
/// "sharing violation" (32).
fn is_transient_lock(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        write_atomic(&target, b"hello").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
        assert_eq!(entries(dir.path()), ["a.txt"]);
    }

    #[test]
    fn replaces_existing_content_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"old content that is longer").unwrap();
        write_atomic(&target, b"new").unwrap();
        write_atomic(&target, b"newer").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"newer");
        assert_eq!(entries(dir.path()), ["a.txt"]);
    }

    #[cfg(windows)]
    #[test]
    fn waits_for_a_reader_that_briefly_locks_the_target() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"old").unwrap();
        // Exclusive handle, as a sync client scanning the file might hold.
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(lock);
        });
        write_atomic(&target, b"new").unwrap();
        releaser.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(entries(dir.path()), ["a.txt"]);
    }

    #[test]
    fn missing_parent_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("missing").join("a.txt");
        assert!(write_atomic(&target, b"x").is_err());
        assert!(entries(dir.path()).is_empty());
    }

    #[test]
    fn failed_rename_cleans_up_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        // Renaming a file over a non-empty directory fails on every platform.
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("child"), b"x").unwrap();
        assert!(write_atomic(&target, b"x").is_err());
        assert_eq!(entries(dir.path()), ["target"]);
        assert!(target.join("child").is_file());
    }
}
