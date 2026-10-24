//! Finding Inkspace's `LocalState` folder.

use std::path::{Path, PathBuf};

use crate::reader::TrackedReader;

/// Package folder name prefix of Wacom Inkspace (Microsoft Store app).
pub const PACKAGE_PREFIX: &str = "D91E29CF.InkspaceApp_";

/// `%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_*\LocalState`, if Inkspace's note store is
/// there. Only lists `Packages` and checks that the folders exist; nothing is read.
pub fn locate_local_state(reader: &TrackedReader) -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
    locate_in_packages(reader, &PathBuf::from(local_app_data).join("Packages"))
}

/// Same as [`locate_local_state`] with an explicit `Packages` folder. With several matching
/// packages, the first one (by name) that holds a note store wins.
pub fn locate_in_packages(reader: &TrackedReader, packages: &Path) -> Option<PathBuf> {
    let entries = reader.list(packages).ok()?;
    entries
        .into_iter()
        .filter(|entry| entry.is_dir && is_inkspace_package(&entry.name))
        .map(|entry| entry.path.join("LocalState"))
        .find(|local_state| reader.is_dir(&crate::store::db_dir(local_state)))
}

fn is_inkspace_package(name: &str) -> bool {
    name.get(..PACKAGE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PACKAGE_PREFIX))
        && name.len() > PACKAGE_PREFIX.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names() {
        assert!(is_inkspace_package("D91E29CF.InkspaceApp_38kynpdw5g1aw"));
        assert!(is_inkspace_package("d91e29cf.inkspaceapp_x"));
        assert!(!is_inkspace_package("D91E29CF.InkspaceApp_"));
        assert!(!is_inkspace_package("D91E29CF.Other_x"));
        assert!(!is_inkspace_package("é"));
    }

    #[test]
    fn finds_the_package_with_a_note_store() {
        let packages = tempfile::tempdir().unwrap();
        let p = packages.path();
        std::fs::create_dir_all(p.join("D91E29CF.InkspaceApp_aaa").join("LocalState")).unwrap();
        let good = p.join("D91E29CF.InkspaceApp_bbb").join("LocalState");
        std::fs::create_dir_all(good.join("WacomCloudDb").join("_")).unwrap();
        std::fs::create_dir_all(p.join("Other.App_ccc")).unwrap();
        let reader = TrackedReader::new();
        assert_eq!(locate_in_packages(&reader, p), Some(good));
        assert_eq!(locate_in_packages(&reader, &p.join("missing")), None);
        // Only listings and existence checks.
        assert!(
            reader
                .accesses()
                .iter()
                .all(|a| a.kind != crate::reader::AccessKind::Read)
        );
    }
}
