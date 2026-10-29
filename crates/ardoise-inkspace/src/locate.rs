//! Finding Inkspace's `LocalState` folder: on this computer (research §1), or from a folder the
//! user chose by hand. Only directory listings and existence checks: nothing is read.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::reader::TrackedReader;
use crate::store::db_dir;

/// Package folder name prefix of Wacom Inkspace (Microsoft Store app).
pub const PACKAGE_PREFIX: &str = "D91E29CF.InkspaceApp_";

/// `%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_*\LocalState`, if Inkspace's note store is
/// there ([`locate_with_env`] with this process's environment). Inkspace only exists on
/// Windows: elsewhere there is nothing to find.
#[cfg(windows)]
pub fn locate_local_state(reader: &TrackedReader) -> Option<PathBuf> {
    locate_with_env(reader, |name| std::env::var_os(name))
}

/// `%LOCALAPPDATA%\Packages\D91E29CF.InkspaceApp_*\LocalState`, if Inkspace's note store is
/// there ([`locate_with_env`] with this process's environment). Inkspace only exists on
/// Windows: elsewhere there is nothing to find.
#[cfg(not(windows))]
pub fn locate_local_state(_reader: &TrackedReader) -> Option<PathBuf> {
    None
}

/// The `Packages` folders where Windows keeps the data of Store apps, from the environment
/// `env` (a variable's value by name): `%LOCALAPPDATA%\Packages`, then
/// `%USERPROFILE%\AppData\Local\Packages` when that is another folder (`LOCALAPPDATA` unset or
/// pointing elsewhere). Research §1 knows of no other place where Inkspace 3 keeps its notes.
pub fn packages_dirs(env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let var = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(local) = var("LOCALAPPDATA") {
        dirs.push(local.join("Packages"));
    }
    if let Some(profile) = var("USERPROFILE") {
        let dir = profile.join("AppData").join("Local").join("Packages");
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The first Inkspace `LocalState` found in the [`packages_dirs`] of `env`.
pub fn locate_with_env(
    reader: &TrackedReader,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    packages_dirs(env)
        .iter()
        .find_map(|packages| locate_in_packages(reader, packages))
}

/// Same as [`locate_local_state`] with an explicit `Packages` folder. With several matching
/// packages, the first one (by name) that holds a note store wins.
pub fn locate_in_packages(reader: &TrackedReader, packages: &Path) -> Option<PathBuf> {
    let entries = reader.list(packages).ok()?;
    entries
        .into_iter()
        .filter(|entry| entry.is_dir && is_inkspace_package(&entry.name))
        .map(|entry| entry.path.join("LocalState"))
        .find(|local_state| has_note_store(reader, local_state))
}

/// The `LocalState` folder meant by `folder`, a folder chosen by hand: a `LocalState` folder
/// (Inkspace's, or a copy of it under any name), the package folder above it, the
/// `WacomCloudDb` or `WacomCloudDb\_` folder inside it, or a `Packages` folder holding the
/// Inkspace package. `None` when it is none of these. Only existence checks, and the listing of
/// a folder named `Packages`: a folder chosen by mistake (Inkspace's `db\`, the user's
/// documents) is never listed or read.
pub fn local_state_in(reader: &TrackedReader, folder: &Path) -> Option<PathBuf> {
    if has_note_store(reader, folder) {
        return Some(folder.to_path_buf());
    }
    let named = |path: &Path, name: &str| {
        path.file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
    };
    // WacomCloudDb\_, then WacomCloudDb: the LocalState above.
    if named(folder, "_")
        && let Some(cloud) = folder.parent().filter(|p| named(p, "WacomCloudDb"))
        && let Some(local_state) = cloud.parent()
        && has_note_store(reader, local_state)
    {
        return Some(local_state.to_path_buf());
    }
    if named(folder, "WacomCloudDb")
        && let Some(local_state) = folder.parent()
        && has_note_store(reader, local_state)
    {
        return Some(local_state.to_path_buf());
    }
    // The package folder.
    let below = folder.join("LocalState");
    if has_note_store(reader, &below) {
        return Some(below);
    }
    if named(folder, "Packages") {
        return locate_in_packages(reader, folder);
    }
    None
}

fn has_note_store(reader: &TrackedReader, local_state: &Path) -> bool {
    reader.is_dir(&db_dir(local_state))
}

/// Whether `name` is the name of Inkspace's package folder (`D91E29CF.InkspaceApp_<id>`, any
/// case).
pub fn is_inkspace_package(name: &str) -> bool {
    name.get(..PACKAGE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PACKAGE_PREFIX))
        && name.len() > PACKAGE_PREFIX.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::AccessKind;

    #[test]
    fn package_names() {
        assert!(is_inkspace_package("D91E29CF.InkspaceApp_38kynpdw5g1aw"));
        assert!(is_inkspace_package("d91e29cf.inkspaceapp_x"));
        assert!(!is_inkspace_package("D91E29CF.InkspaceApp_"));
        assert!(!is_inkspace_package("D91E29CF.Other_x"));
        assert!(!is_inkspace_package("é"));
    }

    /// `<root>/<packages>/D91E29CF.InkspaceApp_bbb/LocalState` with an (empty) note store, next
    /// to an Inkspace package without one and another app's package; returns the LocalState.
    fn fake_packages(packages: &Path) -> PathBuf {
        std::fs::create_dir_all(packages.join("D91E29CF.InkspaceApp_aaa").join("LocalState"))
            .unwrap();
        let good = packages.join("D91E29CF.InkspaceApp_bbb").join("LocalState");
        std::fs::create_dir_all(good.join("WacomCloudDb").join("_")).unwrap();
        std::fs::create_dir_all(packages.join("Other.App_ccc")).unwrap();
        good
    }

    fn no_reads(reader: &TrackedReader) -> bool {
        reader.accesses().iter().all(|a| a.kind != AccessKind::Read)
    }

    #[test]
    fn finds_the_package_with_a_note_store() {
        let packages = tempfile::tempdir().unwrap();
        let p = packages.path();
        let good = fake_packages(p);
        let reader = TrackedReader::new();
        assert_eq!(locate_in_packages(&reader, p), Some(good));
        assert_eq!(locate_in_packages(&reader, &p.join("missing")), None);
        // Only listings and existence checks.
        assert!(no_reads(&reader));
    }

    fn env_of(vars: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let vars: Vec<(String, OsString)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.as_os_str().to_owned()))
            .collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn packages_folders_come_from_the_environment() {
        let local = Path::new("C:/Users/prof/AppData/Local");
        let profile = Path::new("C:/Users/prof");
        assert_eq!(
            packages_dirs(env_of(&[("LOCALAPPDATA", local), ("USERPROFILE", profile)])),
            vec![local.join("Packages")]
        );
        let redirected = Path::new("D:/Local");
        assert_eq!(
            packages_dirs(env_of(&[
                ("LOCALAPPDATA", redirected),
                ("USERPROFILE", profile)
            ])),
            vec![redirected.join("Packages"), local.join("Packages")]
        );
        assert_eq!(
            packages_dirs(env_of(&[("USERPROFILE", profile)])),
            vec![local.join("Packages")]
        );
        assert_eq!(
            packages_dirs(env_of(&[("LOCALAPPDATA", Path::new(""))])),
            Vec::<PathBuf>::new()
        );
        assert!(packages_dirs(|_| None).is_empty());
    }

    #[test]
    fn detection_searches_local_app_data_then_the_profile() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("Local");
        let profile = root.path().join("Profile");
        let in_profile = fake_packages(&profile.join("AppData").join("Local").join("Packages"));
        std::fs::create_dir_all(local.join("Packages")).unwrap();
        let reader = TrackedReader::new();
        // Nothing under LOCALAPPDATA: the profile's folder is searched next.
        let env = env_of(&[("LOCALAPPDATA", &local), ("USERPROFILE", &profile)]);
        assert_eq!(locate_with_env(&reader, &env), Some(in_profile.clone()));
        // LOCALAPPDATA wins when it has Inkspace.
        let in_local = fake_packages(&local.join("Packages"));
        assert_eq!(locate_with_env(&reader, &env), Some(in_local));
        // No environment, or folders without Inkspace: nothing.
        assert_eq!(locate_with_env(&reader, |_| None), None);
        let elsewhere = env_of(&[("LOCALAPPDATA", root.path())]);
        assert_eq!(locate_with_env(&reader, elsewhere), None);
        assert!(no_reads(&reader));
    }

    #[cfg(not(windows))]
    #[test]
    fn detection_finds_nothing_outside_windows() {
        let reader = TrackedReader::new();
        assert_eq!(locate_local_state(&reader), None);
        assert!(reader.accesses().is_empty());
    }

    #[test]
    fn a_folder_chosen_by_hand_leads_to_its_local_state() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("Packages");
        let local_state = fake_packages(&packages);
        let package = local_state.parent().unwrap().to_path_buf();
        let cloud = local_state.join("WacomCloudDb");
        let reader = TrackedReader::new();
        for chosen in [&local_state, &package, &cloud, &cloud.join("_"), &packages] {
            assert_eq!(
                local_state_in(&reader, chosen),
                Some(local_state.clone()),
                "{chosen:?}"
            );
        }
        // A copy under another name (testdata/private/inkspace/LocalState, a USB stick…).
        let copy = root.path().join("Copie Inkspace");
        std::fs::create_dir_all(copy.join("WacomCloudDb").join("_")).unwrap();
        assert_eq!(local_state_in(&reader, &copy), Some(copy.clone()));
        // Anything else: nothing, and only existence checks (no listing of the user's folders).
        let db = local_state.join("db");
        std::fs::create_dir_all(db.join("notes")).unwrap();
        let documents = root.path().join("Documents");
        std::fs::create_dir_all(&documents).unwrap();
        let before = reader.accesses().len();
        for chosen in [
            &db,
            &db.join("notes"),
            &documents,
            &root.path().join("missing"),
        ] {
            assert_eq!(local_state_in(&reader, chosen), None, "{chosen:?}");
        }
        assert!(
            reader.accesses()[before..]
                .iter()
                .all(|a| a.kind == AccessKind::Stat),
            "{:?}",
            &reader.accesses()[before..]
        );
        assert!(no_reads(&reader));
    }
}
