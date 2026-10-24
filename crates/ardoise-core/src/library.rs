//! The user's library folder: an ordinary folder of notes plus a hidden `.ardoise/` folder.

use std::fs;
use std::io;
use std::path::Path;

/// Hidden folder that marks a library and holds its shared data (tablet registration,
/// diagnostics, trash).
pub const META_DIR: &str = ".ardoise";

/// Whether `folder` already holds an Ardoise library.
pub fn is_library(folder: &Path) -> bool {
    folder.join(META_DIR).is_dir()
}

/// Makes `folder` a library, keeping anything already there. The folder itself must exist,
/// so that a typo in a path never silently creates a new tree.
pub fn init(folder: &Path) -> io::Result<()> {
    if !folder.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not an existing folder", folder.display()),
        ));
    }
    fs::create_dir_all(folder.join(META_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_marker_and_keeps_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note.txt"), "keep me").unwrap();
        assert!(!is_library(dir.path()));

        init(dir.path()).unwrap();
        assert!(is_library(dir.path()));
        assert_eq!(
            fs::read_to_string(dir.path().join("note.txt")).unwrap(),
            "keep me"
        );

        // Initialising twice is harmless.
        init(dir.path()).unwrap();
        assert!(is_library(dir.path()));
    }

    #[test]
    fn init_refuses_missing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let err = init(&missing).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(!missing.exists());
    }
}
