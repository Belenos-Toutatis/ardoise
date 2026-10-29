//! Filesystem helpers shared by everything that writes to the user's data.

use std::convert::Infallible;
use std::fmt;
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

/// Longest file name NTFS accepts, in UTF-16 code units.
const MAX_FILE_NAME_UNITS: usize = 255;

/// Builds a unique temporary path next to `path` (same directory, so the final rename stays
/// on one volume and is atomic): `.{name}.{pid}.{n}.part`. The `.part` suffix is in the
/// default ignore list of Nextcloud and ownCloud, so the temporary file is never uploaded,
/// and the leading dot hides it from the library. A long `name` is shortened so that the
/// temporary name still fits in one file name; the process id and the counter keep it unique.
fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tail = format!(".{}.{unique}.part", std::process::id());
    let room = MAX_FILE_NAME_UNITS
        .saturating_sub(1)
        .saturating_sub(tail.encode_utf16().count());
    let mut used = 0;
    let short: String = name
        .chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= room
        })
        .collect();
    path.with_file_name(format!(".{short}{tail}"))
}

/// Why [`write_atomic_checked`] did not write. In both cases the target was left untouched
/// and the temporary file was removed.
#[derive(Debug)]
pub enum CheckedWriteError<E> {
    /// The check refused the write.
    Refused(E),
    /// The file could not be written or renamed.
    Io(io::Error),
}

impl<E> From<io::Error> for CheckedWriteError<E> {
    fn from(error: io::Error) -> Self {
        CheckedWriteError::Io(error)
    }
}

impl<E: fmt::Display> fmt::Display for CheckedWriteError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckedWriteError::Refused(error) => write!(f, "write refused: {error}"),
            CheckedWriteError::Io(error) => error.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CheckedWriteError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckedWriteError::Refused(error) => Some(error),
            CheckedWriteError::Io(error) => Some(error),
        }
    }
}

fn no_check() -> Result<(), Infallible> {
    Ok(())
}

fn unchecked<T>(result: Result<T, CheckedWriteError<Infallible>>) -> io::Result<T> {
    result.map_err(|error| match error {
        CheckedWriteError::Io(error) => error,
        CheckedWriteError::Refused(never) => match never {},
    })
}

/// Writes `bytes` to `path` atomically.
///
/// The data goes to a temporary file in the same directory, is flushed and synced to disk, and
/// then renamed over the target (`std::fs::rename` replaces an existing file on Windows). A
/// reader, or a sync client such as Nextcloud, never sees a half-written target. On error the
/// temporary file is removed and the target is left untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    unchecked(write_via_temp(path, bytes, Replace::Yes, &mut no_check)).map(drop)
}

/// Like [`write_atomic`], and returns the metadata of the new file (its modification time and
/// size, as read just before the final rename, which keeps them). Comparing it later with
/// the file's current metadata tells whether someone else has written the file since.
pub fn write_atomic_metadata(path: &Path, bytes: &[u8]) -> io::Result<fs::Metadata> {
    unchecked(write_via_temp(path, bytes, Replace::Yes, &mut no_check))
}

/// Like [`write_atomic_metadata`], but calls `check` once the temporary file is completely
/// written and synced, right before every attempt to rename it over `path` (the rename is
/// retried while a sync client or an antivirus holds the target open). If `check` fails, the
/// temporary file is removed, `path` is left untouched and the check's error is returned in
/// [`CheckedWriteError::Refused`].
///
/// This is how a save makes sure that the file it replaces is still the one it loaded: only
/// the time between the last check and the rename system call itself stays uncovered.
pub fn write_atomic_checked<E>(
    path: &Path,
    bytes: &[u8],
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<fs::Metadata, CheckedWriteError<E>> {
    write_via_temp(path, bytes, Replace::Yes, &mut check)
}

/// Like [`write_atomic_metadata`], but never replaces an existing file: if `path` exists
/// (or appears meanwhile), fails with [`io::ErrorKind::AlreadyExists`] and leaves it alone.
pub fn write_atomic_new(path: &Path, bytes: &[u8]) -> io::Result<fs::Metadata> {
    unchecked(write_via_temp(path, bytes, Replace::No, &mut no_check))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Replace {
    Yes,
    No,
}

fn write_via_temp<E>(
    path: &Path,
    bytes: &[u8],
    replace: Replace,
    check: &mut dyn FnMut() -> Result<(), E>,
) -> Result<fs::Metadata, CheckedWriteError<E>> {
    let temp = temp_path_for(path);
    let result = write_and_rename(&temp, path, bytes, replace, check);
    if result.is_err() {
        // Best effort: the original error is the one worth reporting.
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_and_rename<E>(
    temp: &Path,
    target: &Path,
    bytes: &[u8],
    replace: Replace,
    check: &mut dyn FnMut() -> Result<(), E>,
) -> Result<fs::Metadata, CheckedWriteError<E>> {
    let mut file = create_temp_file(temp)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    show_temp_file(temp);
    // Read once the handle is closed, so that no pending timestamp update is missed.
    let metadata = fs::metadata(temp)?;
    let mut attempt = 1;
    loop {
        // Before every attempt: what `check` verifies may have changed while the data was
        // written, or while a sync client kept the target busy.
        check().map_err(CheckedWriteError::Refused)?;
        let renamed = match replace {
            Replace::Yes => fs::rename(temp, target),
            Replace::No => move_no_replace(temp, target),
        };
        match renamed {
            Err(error) if attempt < RENAME_ATTEMPTS && is_transient_lock(&error) => {
                attempt += 1;
                std::thread::sleep(RENAME_RETRY_DELAY);
            }
            Err(error) => return Err(CheckedWriteError::Io(error)),
            Ok(()) => return Ok(metadata),
        }
    }
}

/// Creates the temporary file. On Windows it is hidden while it is being written (a leading
/// dot hides nothing there): a crash leaves a file Explorer does not show, and
/// [`sweep_stale_temp_files`] removes it later.
fn create_temp_file(temp: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.attributes(windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN);
    }
    options.open(temp)
}

/// Makes the completed temporary file an ordinary one again: the rename keeps its attributes,
/// and the note must not end up hidden. Best effort (a file left hidden is only less visible).
fn show_temp_file(temp: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_ARCHIVE, SetFileAttributesW};
        let wide: Vec<u16> = temp.as_os_str().encode_wide().chain([0]).collect();
        let mut attempt = 1;
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
        while unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_ARCHIVE) } == 0
            && attempt < RENAME_ATTEMPTS
        {
            attempt += 1;
            std::thread::sleep(RENAME_RETRY_DELAY);
        }
    }
    #[cfg(not(windows))]
    let _ = temp;
}

/// Whether `name` is a temporary file of [`temp_path_for`]: `.{name}.{pid}.{n}.part`.
fn is_temp_name(name: &str) -> bool {
    let Some(inner) = name
        .strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".part"))
    else {
        return false;
    };
    let mut parts = inner.rsplitn(3, '.');
    let number = |part: Option<&str>| {
        part.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    };
    number(parts.next()) && number(parts.next()) && parts.next().is_some_and(|n| !n.is_empty())
}

/// Deepest folder [`sweep_stale_temp_files`] explores.
const SWEEP_DEPTH: usize = 64;

/// Deletes the temporary files of [`write_atomic`] that a crash (or a killed process) left under
/// `root`, when they were last written more than `older_than` ago, so that a write in progress
/// is never touched. Only files named exactly as ours are removed; links are not followed and
/// hidden folders (`.git`, a sync client's) are skipped. Returns how many were deleted.
pub fn sweep_stale_temp_files(root: &Path, older_than: Duration) -> usize {
    let Some(limit) = std::time::SystemTime::now().checked_sub(older_than) else {
        return 0;
    };
    let mut removed = 0;
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if depth < SWEEP_DEPTH && !name.starts_with('.') {
                    pending.push((entry.path(), depth + 1));
                }
            } else if kind.is_file()
                && is_temp_name(name)
                && entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|modified| modified < limit)
                && fs::remove_file(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
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

/// `path` made absolute and verbatim (`\\?\C:\…`, `\\?\UNC\server\share\…`): the form in
/// which Win32 file functions accept paths longer than 260 characters. The standard library
/// does this by itself; it is needed for the system calls made here directly. `/`, `.` and
/// `..` are resolved first, since a verbatim path is taken literally.
#[cfg(windows)]
fn verbatim(path: &Path) -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Prefix};

    let absolute = std::path::absolute(path)?;
    let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let units: Vec<u16> = match absolute.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => r"\\?\".encode_utf16().chain(wide).collect(),
            // `\\server\share\…`: the two leading backslashes become `\\?\UNC\`.
            Prefix::UNC(..) => r"\\?\UNC\"
                .encode_utf16()
                .chain(wide.into_iter().skip(2))
                .collect(),
            // Already verbatim (`\\?\…`), or a device path (`\\.\…`): kept as is.
            _ => wide,
        },
        _ => wide,
    };
    Ok(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(windows)]
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        Ok(verbatim(path)?
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect())
    }
    let (from, to) = (wide(from)?, wide(to)?);
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
    fn stale_temp_files_are_swept_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Classe/3e B")).unwrap();
        fs::create_dir_all(root.join("_ardoise")).unwrap();
        fs::create_dir_all(root.join(".sync")).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(3 * 24 * 3600);
        let make = |path: &str, stamp: std::time::SystemTime| {
            let path = root.join(path);
            fs::write(&path, b"x").unwrap();
            OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(stamp)
                .unwrap();
        };
        let now = std::time::SystemTime::now();
        // Ours and stale: gone, at the root, in a binder and in the library folder.
        make(".2026-10-24 Cours.ardoise.4242.0.part", old);
        make("Classe/3e B/.a.ardoise.7.12.part", old);
        make("_ardoise/.device.json.9.3.part", old);
        // Ours but recent (a write in progress), someone else's, or not ours.
        make(".b.ardoise.4242.1.part", now);
        make(".download.part", old);
        make("c.ardoise.part", old);
        make(".sync/.x.1.2.part", old);
        make("note.ardoise", old);
        assert_eq!(sweep_stale_temp_files(root, Duration::from_secs(24 * 3600)), 3);
        assert_eq!(
            entries(root),
            [
                ".b.ardoise.4242.1.part",
                ".download.part",
                ".sync",
                "Classe",
                "_ardoise",
                "c.ardoise.part",
                "note.ardoise"
            ]
        );
        assert!(is_temp_name(".a.b.12.0.part"));
        assert!(!is_temp_name(".a.12.part"));
        assert!(!is_temp_name("a.1.2.part"));
    }

    #[cfg(windows)]
    #[test]
    fn the_temp_file_is_hidden_while_written_and_the_result_is_not() {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join(".t.1.0.part");
        drop(create_temp_file(&temp).unwrap());
        assert_ne!(fs::metadata(&temp).unwrap().file_attributes() & HIDDEN, 0);
        show_temp_file(&temp);
        assert_eq!(fs::metadata(&temp).unwrap().file_attributes() & HIDDEN, 0);
        let target = dir.path().join("a.txt");
        write_atomic(&target, b"x").unwrap();
        assert_eq!(fs::metadata(&target).unwrap().file_attributes() & HIDDEN, 0);
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
    fn checked_write_runs_the_check_on_the_complete_file_and_obeys_it() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"old").unwrap();

        // The check sees the new data completely written next to the target.
        let mut temp_content = Vec::new();
        let metadata = write_atomic_checked(&target, b"new", || {
            let temp = entries(dir.path())
                .into_iter()
                .find(|n| n.ends_with(".part"))
                .unwrap();
            temp_content = fs::read(dir.path().join(temp)).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"old");
            Ok::<(), Infallible>(())
        })
        .unwrap();
        assert_eq!(temp_content, b"new");
        assert_eq!(metadata.len(), 3);
        assert_eq!(fs::read(&target).unwrap(), b"new");

        // Another program replaces the target between the write and the rename: the check
        // notices, and nothing is overwritten or left behind.
        let error = write_atomic_checked(&target, b"mine", || {
            fs::write(&target, b"theirs").unwrap();
            if fs::read(&target).unwrap() == b"new" {
                Ok(())
            } else {
                Err("changed meanwhile")
            }
        })
        .unwrap_err();
        assert!(matches!(
            error,
            CheckedWriteError::Refused("changed meanwhile")
        ));
        assert_eq!(error.to_string(), "write refused: changed meanwhile");
        assert_eq!(fs::read(&target).unwrap(), b"theirs");
        assert_eq!(entries(dir.path()), ["a.txt"]);
    }

    #[cfg(windows)]
    #[test]
    fn checked_write_checks_again_before_every_rename_attempt() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"old").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(lock);
        });
        let mut checks = 0;
        write_atomic_checked(&target, b"new", || {
            checks += 1;
            Ok::<(), Infallible>(())
        })
        .unwrap();
        releaser.join().unwrap();
        assert!(checks >= 2, "{checks} check(s)");
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_forms() {
        let cases = [
            (r"C:\a\b.txt", r"\\?\C:\a\b.txt"),
            ("C:/a/./x/../b.txt", r"\\?\C:\a\b.txt"),
            (r"\\server\share\a\b.txt", r"\\?\UNC\server\share\a\b.txt"),
            (r"\\?\C:\a\b.txt", r"\\?\C:\a\b.txt"),
        ];
        for (path, expected) in cases {
            assert_eq!(
                verbatim(Path::new(path)).unwrap().as_os_str(),
                expected,
                "{path}"
            );
        }
        let relative = verbatim(Path::new("x.txt")).unwrap();
        let expected = std::env::current_dir().unwrap().join("x.txt");
        assert_eq!(
            relative.as_os_str(),
            format!(r"\\?\{}", expected.display()).as_str()
        );
    }

    /// A folder whose path is over 300 characters long, inside `root`.
    fn deep_folder(root: &Path) -> PathBuf {
        let mut dir = root.to_path_buf();
        while dir.as_os_str().len() < 300 {
            dir.push("d".repeat(60));
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn long_paths_are_renamed_and_created() {
        let root = tempfile::tempdir().unwrap();
        let dir = deep_folder(root.path());
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        assert!(b.as_os_str().len() > 300);

        let metadata = write_atomic_new(&a, b"a").unwrap();
        assert_eq!(metadata.len(), 1);
        assert_eq!(
            write_atomic_new(&a, b"again").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        rename_no_replace(&a, &b).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"a");
        write_atomic(&a, b"x").unwrap();
        assert_eq!(
            rename_no_replace(&a, &b).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        // From a long path to a short one and back, and a folder.
        let short = root.path().join("b.txt");
        rename_no_replace(&b, &short).unwrap();
        rename_no_replace(&short, &b).unwrap();
        let moved = root.path().join("moved");
        rename_no_replace(&dir, &moved).unwrap();
        assert_eq!(entries(&moved), ["a.txt", "b.txt"]);
    }

    #[test]
    fn long_file_names_can_be_written() {
        let dir = tempfile::tempdir().unwrap();
        // 250 characters: Windows counts UTF-16 units (an accented name fits), Linux counts
        // bytes (at most 255), so the accented variant is Windows only.
        let letter = if cfg!(windows) { "é" } else { "e" };
        let name = format!("{}.txt", letter.repeat(246));
        assert_eq!(name.chars().count(), 250);
        let target = dir.path().join(&name);
        write_atomic(&target, b"first").unwrap();
        write_atomic(&target, b"second").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");
        assert_eq!(entries(dir.path()), std::slice::from_ref(&name));

        // Temporary names stay unique, hidden, ignored by sync clients and short enough.
        let first = temp_path_for(&target);
        let second = temp_path_for(&target);
        assert_ne!(first, second);
        for temp in [first, second] {
            let temp_name = temp.file_name().unwrap().to_str().unwrap();
            assert!(
                temp_name.starts_with(&format!(".{letter}{letter}")),
                "{temp_name}"
            );
            assert!(temp_name.ends_with(".part"), "{temp_name}");
            assert!(temp_name.encode_utf16().count() <= 255, "{temp_name}");
            assert_eq!(temp.parent(), target.parent());
        }
        // Short names are kept whole.
        let short = temp_path_for(&dir.path().join("a.ardoise"));
        let short_name = short.file_name().unwrap().to_str().unwrap();
        assert!(short_name.starts_with(".a.ardoise."), "{short_name}");
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
