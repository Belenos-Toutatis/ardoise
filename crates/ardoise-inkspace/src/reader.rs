//! Tracked file access: every file read, directory listing and existence check of the
//! importer goes through [`TrackedReader`], which records the path *before* touching it.
//! A test can therefore prove what was (and what was not) opened, in particular that nothing
//! under `LocalState\db\` is read except the preview images.
//!
//! The reader only reads: it has no method that writes, renames or deletes.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What was done with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    /// The file's content was read (or an attempt was made).
    Read,
    /// The directory's entries were listed.
    List,
    /// Only its existence or type was checked (no content opened).
    Stat,
}

/// One recorded access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub kind: AccessKind,
    pub path: PathBuf,
}

/// An entry of a listed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// File name (lossy UTF-8).
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_file: bool,
    /// Size in bytes (0 for directories).
    pub len: u64,
}

/// Read-only file access that records every path it touches.
#[derive(Debug, Default)]
pub struct TrackedReader {
    log: Mutex<Vec<Access>>,
}

impl TrackedReader {
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&self, kind: AccessKind, path: &Path) {
        let mut log = self
            .log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        log.push(Access {
            kind,
            path: path.to_path_buf(),
        });
    }

    /// Every access so far, in order.
    pub fn accesses(&self) -> Vec<Access> {
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Reads a whole file of at most `max_len` bytes (larger files fail with
    /// `InvalidData` without being read entirely).
    pub fn read(&self, path: &Path, max_len: u64) -> io::Result<Vec<u8>> {
        self.record(AccessKind::Read, path);
        let file = fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len > max_len {
            return Err(too_large(len, max_len));
        }
        let mut bytes = Vec::with_capacity(len as usize);
        file.take(max_len.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_len {
            return Err(too_large(bytes.len() as u64, max_len));
        }
        Ok(bytes)
    }

    /// Lists a directory, sorted by name.
    pub fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        self.record(AccessKind::List, dir);
        let mut entries = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let len = if file_type.is_file() {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                is_dir: file_type.is_dir(),
                is_file: file_type.is_file(),
                len,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Whether `path` is an existing file.
    pub fn is_file(&self, path: &Path) -> bool {
        self.record(AccessKind::Stat, path);
        path.is_file()
    }

    /// Whether `path` is an existing directory.
    pub fn is_dir(&self, path: &Path) -> bool {
        self.record(AccessKind::Stat, path);
        path.is_dir()
    }
}

fn too_large(len: u64, max_len: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("file too large ({len} bytes, limit {max_len})"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_every_access_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.bin");
        fs::write(&file, b"hello").unwrap();
        let reader = TrackedReader::new();
        assert!(reader.is_dir(dir.path()));
        let entries = reader.list(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.bin");
        assert!(entries[0].is_file && entries[0].len == 5);
        assert_eq!(reader.read(&file, 5).unwrap(), b"hello");
        assert!(reader.read(&file, 4).is_err());
        assert!(reader.read(&dir.path().join("missing"), 10).is_err());
        let kinds: Vec<_> = reader.accesses().iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            [
                AccessKind::Stat,
                AccessKind::List,
                AccessKind::Read,
                AccessKind::Read,
                AccessKind::Read
            ]
        );
        // Failed attempts are recorded too.
        assert_eq!(reader.accesses()[4].path, dir.path().join("missing"));
    }
}
