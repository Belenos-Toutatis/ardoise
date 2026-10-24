//! Folder watching: notes created, changed, removed or renamed by anyone (this app, the
//! Explorer, Nextcloud bringing changes made on another computer).
//!
//! System notifications (`notify`, ReadDirectoryChangesW on Windows) only tell *that*
//! something happened. After a quiet period (the debounce), the watcher rescans the tree —
//! names, modification times and sizes, no file content — and compares it with the previous
//! scan. The result is deduplicated (one change per file per batch) and immune to lost or
//! reordered notifications; temporary `.part` files, hidden files and `_ardoise/` never show
//! up, and events that only touch them do not even trigger a rescan.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;

use super::walk::{self, Found, is_skipped_name};
use super::{FileStamp, Library, LibraryError};

/// A change to a note file, reported by [`LibraryWatcher`].
///
/// `stamp` is the file's stamp as the rescan found it. Compared with the stamp returned by
/// [`Library::save`] or [`Library::create`] (or held for an open note), it tells the
/// application's own writes from changes made elsewhere (another program, or Nextcloud
/// bringing another computer's version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LibraryChange {
    Created {
        path: PathBuf,
        stamp: FileStamp,
    },
    Changed {
        path: PathBuf,
        stamp: FileStamp,
    },
    Removed {
        path: PathBuf,
    },
    /// Renamed or moved (also when its binder was renamed); `stamp` is the file's at `to`.
    /// A `Changed` for `to` follows in the same batch if the content changed too.
    Renamed {
        from: PathBuf,
        to: PathBuf,
        stamp: FileStamp,
    },
}

impl LibraryChange {
    /// The path the note has now (or had, for `Removed`).
    pub fn path(&self) -> &Path {
        match self {
            LibraryChange::Created { path, .. }
            | LibraryChange::Changed { path, .. }
            | LibraryChange::Removed { path } => path,
            LibraryChange::Renamed { to, .. } => to,
        }
    }

    /// The file's stamp after the change (`None` for `Removed`).
    pub fn stamp(&self) -> Option<FileStamp> {
        match self {
            LibraryChange::Created { stamp, .. }
            | LibraryChange::Changed { stamp, .. }
            | LibraryChange::Renamed { stamp, .. } => Some(*stamp),
            LibraryChange::Removed { .. } => None,
        }
    }
}

enum Message {
    Fs(notify::Result<notify::Event>),
    Stop,
}

/// Watches a library until dropped.
pub struct LibraryWatcher {
    watcher: Option<RecommendedWatcher>,
    messages: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for LibraryWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryWatcher").finish_non_exhaustive()
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        // Stop the system watch first, then the debouncing thread.
        drop(self.watcher.take());
        let _ = self.messages.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type Snapshot = BTreeMap<PathBuf, FileStamp>;

/// Names, times and sizes of every note file.
fn scan(root: &Path) -> Snapshot {
    let mut snapshot = Snapshot::new();
    walk::walk(root, |found| {
        if let Found::Note { path, .. } = found
            && let Ok(stamp) = FileStamp::of(&path)
        {
            snapshot.insert(path, stamp);
        }
    });
    snapshot
}

/// Whether an event may concern a note: not only hidden files, `.part` files or `_ardoise/`.
fn is_relevant(root: &Path, event: &notify::Event) -> bool {
    if event.need_rescan() || event.paths.is_empty() {
        return true;
    }
    event
        .paths
        .iter()
        .any(|path| match path.strip_prefix(root) {
            Ok(relative) => !relative
                .components()
                .any(|c| c.as_os_str().to_str().is_none_or(is_skipped_name)),
            Err(_) => true,
        })
}

/// Pairs the system's rename notifications (Windows sends the old and new names as two
/// consecutive events).
#[derive(Default)]
struct RenameHints {
    pending_from: Option<PathBuf>,
    pairs: Vec<(PathBuf, PathBuf)>,
}

impl RenameHints {
    fn observe(&mut self, event: &notify::Event) {
        match event.kind {
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                self.pending_from = event.paths.first().cloned();
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                if let (Some(from), Some(to)) = (self.pending_from.take(), event.paths.first()) {
                    self.pairs.push((from, to.clone()));
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                if let [from, to, ..] = event.paths.as_slice() {
                    self.pairs.push((from.clone(), to.clone()));
                }
            }
            _ => {}
        }
    }
}

/// Compares two scans. Removed + created files are paired as renames using the system's
/// rename hints (file or folder renames), then identical time and size (a move keeps both).
fn diff(old: &Snapshot, new: &Snapshot, hints: &[(PathBuf, PathBuf)]) -> Vec<LibraryChange> {
    let mut removed: BTreeSet<&PathBuf> = old.keys().filter(|p| !new.contains_key(*p)).collect();
    let mut created: BTreeSet<&PathBuf> = new.keys().filter(|p| !old.contains_key(*p)).collect();
    let mut renames: Vec<(PathBuf, PathBuf)> = Vec::new();

    let mut pair = |from: &PathBuf,
                    to: &PathBuf,
                    removed: &mut BTreeSet<&PathBuf>,
                    created: &mut BTreeSet<&PathBuf>| {
        if removed.contains(from) && created.contains(to) {
            removed.remove(from);
            created.remove(to);
            renames.push((from.clone(), to.clone()));
        }
    };
    for (from, to) in hints {
        if removed.contains(from) {
            pair(from, to, &mut removed, &mut created);
            continue;
        }
        // A renamed folder: every note under it moved along.
        let moved: Vec<(PathBuf, PathBuf)> = removed
            .iter()
            .filter_map(|r| Some(((*r).clone(), to.join(r.strip_prefix(from).ok()?))))
            .collect();
        for (from, to) in moved {
            pair(&from, &to, &mut removed, &mut created);
        }
    }
    let mut by_stamp: HashMap<FileStamp, (Vec<&PathBuf>, Vec<&PathBuf>)> = HashMap::new();
    for path in &removed {
        by_stamp.entry(old[*path]).or_default().0.push(path);
    }
    for path in &created {
        if let Some(entry) = by_stamp.get_mut(&new[*path]) {
            entry.1.push(path);
        }
    }
    let mut unique: Vec<(PathBuf, PathBuf)> = by_stamp
        .values()
        .filter(|(r, c)| r.len() == 1 && c.len() == 1)
        .map(|(r, c)| (r[0].clone(), c[0].clone()))
        .collect();
    unique.sort();
    for (from, to) in unique {
        pair(&from, &to, &mut removed, &mut created);
    }

    let mut changes = Vec::new();
    renames.sort();
    for (from, to) in renames {
        let stamp = new[&to];
        let content_changed = old[&from] != stamp;
        changes.push(LibraryChange::Renamed {
            from,
            to: to.clone(),
            stamp,
        });
        if content_changed {
            changes.push(LibraryChange::Changed { path: to, stamp });
        }
    }
    changes.extend(
        removed
            .into_iter()
            .map(|p| LibraryChange::Removed { path: p.clone() }),
    );
    changes.extend(created.into_iter().map(|p| LibraryChange::Created {
        path: p.clone(),
        stamp: new[p],
    }));
    changes.extend(
        new.iter()
            .filter(|(path, stamp)| old.get(*path).is_some_and(|before| before != *stamp))
            .map(|(path, stamp)| LibraryChange::Changed {
                path: path.clone(),
                stamp: *stamp,
            }),
    );
    changes
}

fn run(
    root: PathBuf,
    debounce: Duration,
    messages: Receiver<Message>,
    mut snapshot: Snapshot,
    mut on_change: impl FnMut(Vec<LibraryChange>),
) {
    // Under continuous activity, still report at least this often.
    let max_delay = (debounce * 4).max(Duration::from_secs(1));
    let mut first_event: Option<Instant> = None;
    let mut last_event = Instant::now();
    let mut hints = RenameHints::default();
    loop {
        let timeout = match first_event {
            Some(first) => {
                let quiet = debounce.saturating_sub(last_event.elapsed());
                let cap = max_delay.saturating_sub(first.elapsed());
                quiet.min(cap)
            }
            None => Duration::from_secs(3600),
        };
        match messages.recv_timeout(timeout) {
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Message::Fs(Ok(event))) => {
                hints.observe(&event);
                if is_relevant(&root, &event) {
                    last_event = Instant::now();
                    first_event.get_or_insert(last_event);
                }
            }
            // Lost notifications (buffer overflow…): the rescan catches up.
            Ok(Message::Fs(Err(_))) => {
                last_event = Instant::now();
                first_event.get_or_insert(last_event);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let Some(first) = first_event else {
            hints.pairs.clear();
            continue;
        };
        if last_event.elapsed() >= debounce || first.elapsed() >= max_delay {
            let current = scan(&root);
            let changes = diff(&snapshot, &current, &hints.pairs);
            snapshot = current;
            hints.pairs.clear();
            first_event = None;
            if !changes.is_empty() {
                on_change(changes);
            }
        }
    }
}

impl Library {
    /// Watches the library: `on_change` receives batches of changes to note files, at most
    /// once per `debounce` of quiet (and at least every few debounces under continuous
    /// activity), on a background thread. Watching stops when the returned value is dropped.
    pub fn watch<F>(&self, debounce: Duration, on_change: F) -> Result<LibraryWatcher, LibraryError>
    where
        F: FnMut(Vec<LibraryChange>) + Send + 'static,
    {
        let watch_error = |message: String| LibraryError::Watch {
            path: self.root.clone(),
            message,
        };
        let (sender, receiver) = mpsc::channel();
        let events = sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let _ = events.send(Message::Fs(event));
            })
            .map_err(|e| watch_error(e.to_string()))?;
        watcher
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| watch_error(e.to_string()))?;
        // Scanned once the system watch is active: nothing can fall in between.
        let snapshot = scan(&self.root);
        let root = self.root.clone();
        let thread = std::thread::Builder::new()
            .name("ardoise-library-watch".to_owned())
            .spawn(move || run(root, debounce, receiver, snapshot, on_change))
            .map_err(|e| watch_error(e.to_string()))?;
        Ok(LibraryWatcher {
            watcher: Some(watcher),
            messages: sender,
            thread: Some(thread),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn stamp(secs: u64, len: u64) -> FileStamp {
        FileStamp {
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            len,
        }
    }

    fn snapshot(entries: &[(&str, FileStamp)]) -> Snapshot {
        entries
            .iter()
            .map(|(p, s)| (PathBuf::from(p), *s))
            .collect()
    }

    fn p(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn diff_reports_each_kind_once() {
        let old = snapshot(&[
            ("r/a", stamp(1, 1)),
            ("r/b", stamp(2, 2)),
            ("r/c", stamp(3, 3)),
        ]);
        let new = snapshot(&[
            ("r/a", stamp(1, 1)),
            ("r/b", stamp(9, 2)),
            ("r/d", stamp(4, 4)),
        ]);
        let changes = diff(&old, &new, &[]);
        assert_eq!(
            changes,
            [
                LibraryChange::Removed { path: p("r/c") },
                LibraryChange::Created {
                    path: p("r/d"),
                    stamp: stamp(4, 4)
                },
                LibraryChange::Changed {
                    path: p("r/b"),
                    stamp: stamp(9, 2)
                },
            ]
        );
        let stamps: Vec<Option<FileStamp>> = changes.iter().map(LibraryChange::stamp).collect();
        assert_eq!(stamps, [None, Some(stamp(4, 4)), Some(stamp(9, 2))]);
        assert!(diff(&new, &new, &[]).is_empty());
    }

    #[test]
    fn diff_pairs_renames_by_hint_and_by_stamp() {
        let old = snapshot(&[("r/a", stamp(1, 1)), ("r/b", stamp(2, 2))]);
        // a renamed (hinted) and rewritten; b moved without a hint (same stamp).
        let new = snapshot(&[("r/a2", stamp(5, 1)), ("r/sub/b", stamp(2, 2))]);
        assert_eq!(
            diff(&old, &new, &[(p("r/a"), p("r/a2"))]),
            [
                LibraryChange::Renamed {
                    from: p("r/a"),
                    to: p("r/a2"),
                    stamp: stamp(5, 1)
                },
                LibraryChange::Changed {
                    path: p("r/a2"),
                    stamp: stamp(5, 1)
                },
                LibraryChange::Renamed {
                    from: p("r/b"),
                    to: p("r/sub/b"),
                    stamp: stamp(2, 2)
                },
            ]
        );
    }

    #[test]
    fn diff_follows_renamed_folders() {
        let same = stamp(1, 1);
        let old = snapshot(&[("r/old/a", same), ("r/old/b", same)]);
        let new = snapshot(&[("r/new/a", same), ("r/new/b", same)]);
        let changes = diff(&old, &new, &[(p("r/old"), p("r/new"))]);
        assert_eq!(
            changes,
            [
                LibraryChange::Renamed {
                    from: p("r/old/a"),
                    to: p("r/new/a"),
                    stamp: same
                },
                LibraryChange::Renamed {
                    from: p("r/old/b"),
                    to: p("r/new/b"),
                    stamp: same
                },
            ]
        );
        // Without the hint, identical stamps are ambiguous: reported as removed + created.
        let changes = diff(&old, &new, &[]);
        assert_eq!(changes.len(), 4);
    }

    #[test]
    fn relevance_ignores_hidden_and_temporary_files() {
        let root = Path::new("C:/lib");
        let event = |path: &str| {
            notify::Event::new(EventKind::Create(notify::event::CreateKind::Any))
                .add_path(root.join(path))
        };
        assert!(is_relevant(root, &event("2026-10-24 Note.ardoise")));
        assert!(is_relevant(root, &event("Classe")));
        assert!(!is_relevant(root, &event("_ardoise/diagnostics/log.txt")));
        assert!(!is_relevant(root, &event(".ardoise/diagnostics/log.txt")));
        assert!(!is_relevant(
            root,
            &event(".2026-10-24 Note.ardoise.42.0.part")
        ));
        assert!(!is_relevant(root, &event("Classe/x.ardoise.part")));
        assert!(!is_relevant(root, &event(".sync_123.db")));
        assert!(is_relevant(root, &notify::Event::new(EventKind::Any)));
    }
}
