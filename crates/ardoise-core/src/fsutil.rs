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
    write_via_temp(path, bytes, Replace::Yes).map(drop)
}

/// Like [`write_atomic`], and returns the metadata of the new file (its modification time and
/// size, as read just before the final rename, which keeps them). Comparing it later with
/// the file's current metadata tells whether someone else has written the file since.
pub fn write_atomic_metadata(path: &Path, bytes: &[u8]) -> io::Result<fs::Metadata> {
    write_via_temp(path, bytes, Replace::Yes)
}

/// Like [`write_atomic_metadata`], but never replaces an existing file: if `path` exists
/// (or appears meanwhile), fails with [`io::ErrorKind::AlreadyExists`] and leaves it alone.
pub fn write_atomic_new(path: &Path, bytes: &[u8]) -> io::Result<fs::Metadata> {
    write_via_temp(path, bytes, Replace::No)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Replace {
    Yes,
    No,
}

fn write_via_temp(path: &Path, bytes: &[u8], replace: Replace) -> io::Result<fs::Metadata> {
    let temp = temp_path_for(path);
    let result = write_and_rename(&temp, path, bytes, replace);
    if result.is_err() {
        // Best effort: the original error is the one worth reporting.
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_and_rename(
    temp: &Path,
    target: &Path,
    bytes: &[u8],
    replace: Replace,
) -> io::Result<fs::Metadata> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    // Read once the handle is closed, so that no pending timestamp update is missed.
    let metadata = fs::metadata(temp)?;
    match replace {
        Replace::Yes => rename_with_retry(temp, target)?,
        Replace::No => rename_no_replace(temp, target)?,
    }
    Ok(metadata)
}

fn with_retry(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match operation() {
            Err(error) if attempt < RENAME_ATTEMPTS && is_transient_lock(&error) => {
                attempt += 1;
                std::thread::sleep(RENAME_RETRY_DELAY);
            }
            result => return result,
        }
    }
}

fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    with_retry(|| fs::rename(from, to))
}

/// Renames (moves) a file or folder within one volume, failing with
/// [`io::ErrorKind::AlreadyExists`] instead of replacing an existing target. On Windows the
/// check and the move are one atomic system call, so no file is ever overwritten, even one
/// that a sync client creates at the same moment.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    with_retry(|| move_no_replace(from, to))
}

/// Removes a file, retrying while a sync client or antivirus briefly holds it open.
pub fn remove_file_with_retry(path: &Path) -> io::Result<()> {
    with_retry(|| fs::remove_file(path))
}

#[cfg(windows)]
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    let (from, to) = (wide(from), wide(to));
    // Without MOVEFILE_REPLACE_EXISTING the call fails if the target exists, and without
    // MOVEFILE_COPY_ALLOWED it never falls back to a non-atomic copy.
    // SAFETY: both pointers are NUL-terminated UTF-16 strings that outlive the call.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    // Not atomic, but this application only runs on Windows; this keeps tests portable.
    if to.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        ));
    }
    fs::rename(from, to)
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
    fn returned_metadata_matches_the_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        let first = write_atomic_metadata(&target, b"hello").unwrap();
        let on_disk = fs::metadata(&target).unwrap();
        assert_eq!(first.modified().unwrap(), on_disk.modified().unwrap());
        assert_eq!(first.len(), 5);
        assert_eq!(on_disk.len(), 5);
        assert_eq!(entries(dir.path()), ["a.txt"]);
    }

    #[test]
    fn write_new_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        let metadata = write_atomic_new(&target, b"first").unwrap();
        assert_eq!(metadata.len(), 5);
        let err = write_atomic_new(&target, b"second").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"first");
        assert_eq!(entries(dir.path()), ["a.txt"]);
    }

    #[test]
    fn rename_no_replace_moves_or_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let sub = dir.path().join("sub");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        fs::create_dir(&sub).unwrap();

        let err = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&b).unwrap(), b"b");
        assert_eq!(fs::read(&a).unwrap(), b"a");

        rename_no_replace(&a, &sub.join("a.txt")).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read(sub.join("a.txt")).unwrap(), b"a");

        // Folders move too.
        rename_no_replace(&sub, &dir.path().join("moved")).unwrap();
        assert!(dir.path().join("moved").join("a.txt").is_file());

        let missing = rename_no_replace(&dir.path().join("missing"), &a).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn remove_with_retry_removes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        fs::write(&a, b"a").unwrap();
        remove_file_with_retry(&a).unwrap();
        assert!(!a.exists());
        assert_eq!(
            remove_file_with_retry(&a).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
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
