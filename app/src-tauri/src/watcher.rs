//! Watches this computer's library while the app runs and tells the window (event
//! `library-changed`) when notes change on disk: here, in the Explorer, or on another computer
//! through Nextcloud. The watcher follows the library chosen in the settings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ardoise_core::library::{Library, LibraryChange, LibraryError, LibraryWatcher};
use tauri::{AppHandle, Emitter, Manager};

/// Event sent to the main window; its payload is the list of changes.
pub const LIBRARY_CHANGED: &str = "library-changed";

/// Quiet period before a batch of changes is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// How often a library that could not be opened (a network drive or a disk mounted after the
/// start) is tried again.
const RETRY_EVERY: Duration = Duration::from_secs(5);

struct Running {
    root: PathBuf,
    _watcher: LibraryWatcher,
}

/// Outcome of one try of [`LibraryWatch::retry_once`].
#[derive(Debug, PartialEq, Eq)]
enum Retry {
    /// The library still cannot be watched.
    Again,
    /// Watching started.
    Started,
    /// Another library was chosen meanwhile.
    Superseded,
}

/// The running watcher, if any (managed state).
#[derive(Default)]
pub struct LibraryWatch {
    current: Mutex<Option<Running>>,
    /// Counts the calls of [`follow`](Self::follow): a retry that is no longer the latest
    /// choice gives up.
    choice: AtomicU64,
}

impl LibraryWatch {
    /// Watches `library` (none: stops watching), sending changes to the main window. A failure
    /// is logged and leaves the app without live refresh, never stops it; the library is tried
    /// again every few seconds, so that the refresh starts when its folder comes back.
    pub fn follow(&self, app: &AppHandle, library: Option<&str>) {
        let choice = self.choice.fetch_add(1, Ordering::SeqCst) + 1;
        // The handwriting index follows the library, and every change in it.
        crate::handwriting::refresh(app);
        let result = self.follow_with(library.map(Path::new), changes_to_window(app));
        if let Err(error) = result {
            eprintln!("Ardoise: {error}");
            if let Some(root) = library {
                retry_later(app.clone(), PathBuf::from(root), choice);
            }
        }
    }

    /// One try of the retry that [`follow`](Self::follow) schedules: starts watching `root`
    /// unless another library was chosen since (`choice` is no longer the latest).
    fn retry_once(
        &self,
        root: &Path,
        choice: u64,
        on_change: impl FnMut(Vec<LibraryChange>) + Send + 'static,
    ) -> Retry {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Checked under the lock: a newer `follow` that waits for it replaces what is set here.
        if self.choice.load(Ordering::SeqCst) != choice {
            return Retry::Superseded;
        }
        let Ok(watcher) =
            Library::open(root).and_then(|library| library.watch(DEBOUNCE, on_change))
        else {
            return Retry::Again;
        };
        *current = Some(Running {
            root: root.to_path_buf(),
            _watcher: watcher,
        });
        Retry::Started
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

/// What the watcher does with a batch of changes: tells the main window and the handwriting
/// index.
fn changes_to_window(app: &AppHandle) -> impl FnMut(Vec<LibraryChange>) + Send + 'static + use<> {
    let app = app.clone();
    move |changes| {
        let _ = app.emit_to("main", LIBRARY_CHANGED, changes);
        crate::handwriting::refresh(&app);
    }
}

/// Tries to watch `root` every [`RETRY_EVERY`] until it works or another library is chosen.
/// When it starts, the window is told to list the library again: it may have changed (or been
/// unreadable) while nothing was watching.
fn retry_later(app: AppHandle, root: PathBuf, choice: u64) {
    let _ = std::thread::Builder::new()
        .name("ardoise-library-watch-retry".into())
        .spawn(move || {
            loop {
                std::thread::sleep(RETRY_EVERY);
                let watch = app.state::<LibraryWatch>();
                match watch.retry_once(&root, choice, changes_to_window(&app)) {
                    Retry::Again => {}
                    Retry::Superseded => return,
                    Retry::Started => {
                        let _ = app.emit_to("main", LIBRARY_CHANGED, Vec::<LibraryChange>::new());
                        crate::handwriting::refresh(&app);
                        return;
                    }
                }
            }
        });
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

    #[test]
    fn a_library_that_comes_back_is_watched_by_the_retry() {
        // A drive mounted after the start: the folder is not a library yet.
        let drive = tempfile::tempdir().unwrap();
        let watch = LibraryWatch::default();
        let first = watch.choice.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(
            watch
                .follow_with(Some(drive.path()), |_| {})
                .is_err_and(|error| error.code() == "notALibrary")
        );
        assert_eq!(watch.retry_once(drive.path(), first, |_| {}), Retry::Again);
        assert_eq!(watch.watched(), None);

        // The library appears: the next try starts the watch.
        std::fs::create_dir(drive.path().join("_ardoise")).unwrap();
        assert_eq!(
            watch.retry_once(drive.path(), first, |_| {}),
            Retry::Started
        );
        assert_eq!(watch.watched().as_deref(), Some(drive.path()));

        // A retry of an older choice does not replace the newer one.
        let (other, _) = crate::library::tests::library();
        let second = watch.choice.fetch_add(1, Ordering::SeqCst) + 1;
        watch.follow_with(Some(other.path()), |_| {}).unwrap();
        assert_eq!(
            watch.retry_once(drive.path(), first, |_| {}),
            Retry::Superseded
        );
        assert_eq!(watch.watched().as_deref(), Some(other.path()));
        assert_ne!(first, second);
    }
}
