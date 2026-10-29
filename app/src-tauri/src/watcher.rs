//! Watches this computer's library while the app runs and tells the window (event
//! `library-changed`) when notes change on disk: here, in the Explorer, or on another computer
//! through Nextcloud. The watcher follows the library chosen in the settings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use ardoise_core::library::{Library, LibraryChange, LibraryError, LibraryWatcher};
use tauri::{AppHandle, Emitter};

/// Event sent to the main window; its payload is the list of changes.
pub const LIBRARY_CHANGED: &str = "library-changed";

/// Quiet period before a batch of changes is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

struct Running {
    root: PathBuf,
    _watcher: LibraryWatcher,
}

/// The running watcher, if any (managed state).
#[derive(Default)]
pub struct LibraryWatch {
    current: Mutex<Option<Running>>,
}

impl LibraryWatch {
    /// Watches `library` (none: stops watching), sending changes to the main window. A failure
    /// is logged and leaves the app without live refresh, never stops it.
    pub fn follow(&self, app: &AppHandle, library: Option<&str>) {
        let app = app.clone();
        // The handwriting index follows the library, and every change in it.
        crate::handwriting::refresh(&app);
        let result = self.follow_with(library.map(Path::new), move |changes| {
            let _ = app.emit_to("main", LIBRARY_CHANGED, changes);
            crate::handwriting::refresh(&app);
        });
        if let Err(error) = result {
            eprintln!("Ardoise: {error}");
        }
    }

    /// [`follow`](Self::follow) with any receiver of the changes. Nothing happens when the
    /// library is the one already watched.
    pub fn follow_with(
        &self,
        library: Option<&Path>,
        on_change: impl FnMut(Vec<LibraryChange>) + Send + 'static,
    ) -> Result<(), LibraryError> {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current.as_ref().map(|r| r.root.as_path()) == library {
            return Ok(());
        }
        // The old watcher stops before the new one starts.
        *current = None;
        let Some(root) = library else {
            return Ok(());
        };
        let watcher = Library::open(root)?.watch(DEBOUNCE, on_change)?;
        *current = Some(Running {
            root: root.to_path_buf(),
            _watcher: watcher,
        });
        Ok(())
    }

    /// Waits for the watcher's first scan (it runs on the watcher's thread): a note written
    /// before it ends is part of the starting state, not a change.
    #[cfg(test)]
    fn wait_until_scanned(&self) {
        if let Some(running) = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            running._watcher.wait_until_scanned();
        }
    }

    /// The library being watched.
    #[cfg(test)]
    fn watched(&self) -> Option<PathBuf> {
        self.current
            .lock()
            .ok()
            .and_then(|c| c.as_ref().map(|r| r.root.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::library::tests::{library, write_sample};

    #[test]
    fn reports_changes_and_follows_the_chosen_library() {
        let (dir, library) = library();
        let watch = LibraryWatch::default();
        let (sender, receiver) = mpsc::channel();
        watch
            .follow_with(Some(dir.path()), move |changes| {
                let _ = sender.send(changes);
            })
            .unwrap();
        assert_eq!(watch.watched().as_deref(), Some(dir.path()));
        watch.wait_until_scanned();

        let saved = write_sample(&library, "", "Essai");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while !seen && std::time::Instant::now() < deadline {
            if let Ok(changes) = receiver.recv_timeout(Duration::from_millis(200)) {
                seen = changes.iter().any(|c| c.path() == saved.path);
            }
        }
        assert!(seen, "the new note was reported");

        // Another library: the first one is no longer watched.
        let (other, _) = crate::library::tests::library();
        watch.follow_with(Some(other.path()), |_| {}).unwrap();
        assert_eq!(watch.watched().as_deref(), Some(other.path()));
        // Not a library: logged by `follow`, never a panic; nothing is watched.
        let plain = tempfile::tempdir().unwrap();
        let error = watch.follow_with(Some(plain.path()), |_| {}).unwrap_err();
        assert_eq!(error.code(), "notALibrary");
        assert_eq!(watch.watched(), None);
        watch.follow_with(None, |_| {}).unwrap();
        assert_eq!(watch.watched(), None);
    }
}
