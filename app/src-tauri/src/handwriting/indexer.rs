//! The background indexer: recognises the notes of the library whose cache entry is missing or
//! stale, most recently changed first, on a thread of its own.
//!
//! A pass starts at launch, after every change in the library (the folder watcher), when the
//! library or the language changes, and when the user resumes it. A newer request interrupts
//! the pass under way; so does stopping it. While live mode or the presentation window runs
//! (their latency budget is 30 ms) it waits between two notes. Its progress goes to the
//! interface after every note (at most a few times a second).
//!
//! Threads and locks (`docs/design/extras.md`, "Handwriting recognition"): the indexer's thread
//! runs at normal priority, because it takes locks that the interface's commands also take (its
//! own, and the app's through [`Services`]); only the engine's worker thread, which shares no
//! lock, runs at background priority. No lock of the indexer is ever held across a disk access,
//! a recognition or a call into the app ([`unlocked`] checks it in debug builds), so a search,
//! the status or a stop never waits for the disk or the engine.
//!
//! Everything the indexer needs from the app comes through [`Services`], so that it is tested
//! with a fake recognizer and plain folders.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ardoise_core::format::read_note;
use ardoise_core::library::{FileStamp, Library, NoteEntry};
use ardoise_core::note::Note;
use ardoise_core::settings::HandwritingLanguage;
use ardoise_hwr::ink::recognizable_strokes;
use ardoise_hwr::{HwrError, Language, Recognition, Recognizer, RecognizerInfo, recognizer_for};
use serde::Serialize;

use super::cache::{CacheDir, Correction, EntryKey, FileRecord, HandwritingCache, strokes_hash};
use super::search::{SearchHit, query_words, search_recognition};
use crate::library::path_text;

/// Least time between two progress reports to the interface (state changes always go).
const NOTIFY_INTERVAL: Duration = Duration::from_millis(200);

/// `files.json` is written after this many notes, and at the end of a pass.
const FLUSH_EVERY: usize = 10;

/// What the indexer reads from the settings at the start of each pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexConfig {
    pub library: Option<PathBuf>,
    pub setting: HandwritingLanguage,
    /// The interface language (`auto` follows it).
    pub interface: Language,
}

/// The language recognised on this PC: the one chosen if its recognizer is installed; for
/// `auto`, the interface language if its recognizer is installed, else the first installed
/// recognizer of a language the app knows; `None` when there is none.
pub fn resolve_language(
    setting: &HandwritingLanguage,
    interface: Language,
    installed: &[RecognizerInfo],
) -> Option<Language> {
    let has = |language| recognizer_for(installed, language).is_some();
    match setting.known() {
        HandwritingLanguage::Fr => has(Language::Fr).then_some(Language::Fr),
        HandwritingLanguage::En => has(Language::En).then_some(Language::En),
        _ if has(interface) => Some(interface),
        _ => installed.iter().find_map(|r| r.language),
    }
}

/// `IndexState` in `handwriting.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IndexState {
    /// Every note is indexed (or nothing has started yet).
    Idle,
    Indexing,
    /// Waiting for live mode or the presentation to end.
    Paused,
    /// Stopped by the user.
    Stopped,
    /// No recognition here: see `reason`.
    Unavailable,
}

/// Why recognition is unavailable (`IndexStatus.reason`).
pub const UNSUPPORTED: &str = "unsupported";
pub const NO_RECOGNIZER: &str = "noRecognizer";
pub const NO_LIBRARY: &str = "noLibrary";

/// `IndexStatus` in `handwriting.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub state: IndexState,
    /// [`UNSUPPORTED`], [`NO_RECOGNIZER`] or [`NO_LIBRARY`] when unavailable.
    pub reason: Option<&'static str>,
    /// The language recognised.
    pub language: Option<Language>,
    /// Notes whose text is known, of `total` in the library.
    pub done: usize,
    pub total: usize,
    /// Notes that could not be read or recognised in this pass.
    pub failed: usize,
}

impl Default for IndexStatus {
    fn default() -> Self {
        IndexStatus {
            state: IndexState::Idle,
            reason: None,
            language: None,
            done: 0,
            total: 0,
            failed: 0,
        }
    }
}

/// What the indexer uses from the app.
pub struct Services {
    /// The engine for the background work (low priority).
    pub background: Arc<dyn Recognizer>,
    /// The engine for a recognition someone waits for (a text export).
    pub on_demand: Arc<dyn Recognizer>,
    pub config: Box<dyn Fn() -> IndexConfig + Send + Sync>,
    /// True while live mode or the presentation runs. Asked before every note and every
    /// `pause_poll` while paused: it must be cheap and take no lock that live mode or the
    /// interface takes (the app reads atomics, `live::Activity`).
    pub busy: Box<dyn Fn() -> bool + Send + Sync>,
    /// Receives the status at every change.
    pub notify: Box<dyn Fn(&IndexStatus) + Send + Sync>,
    pub cache_dir: PathBuf,
    /// How often a paused pass checks whether it may go on.
    pub pause_poll: Duration,
}

/// A note of the library as a listing gives it: enough to find its records and entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub path: PathBuf,
    pub stamp: FileStamp,
    pub id: String,
}

impl From<&NoteEntry> for Listed {
    fn from(entry: &NoteEntry) -> Listed {
        Listed {
            path: entry.path.clone(),
            stamp: entry.stamp,
            id: entry.id.clone(),
        }
    }
}

/// What a search in a listing found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListedSearch {
    pub hits: Vec<SearchHit>,
    /// Notes of the listing whose recognised text is not known yet (always 0 when there is no
    /// recognition here).
    pub unread: usize,
}

struct Inner {
    /// Bumped by every request; a pass goes on only while it is the one it started with.
    generation: u64,
    /// The latest generation whose pass has listed the library and taken everything it knew
    /// without recognising (or found that there is no recognition here).
    ready: u64,
    stopped: bool,
    quit: bool,
    status: IndexStatus,
    last_notified: Option<Instant>,
    /// The notes of the library whose entry is known, by path.
    current: HashMap<PathBuf, EntryKey>,
    /// The library as the latest pass listed it (the search of the interface looks in it).
    listed: Arc<Vec<Listed>>,
}

struct Shared {
    services: Services,
    inner: Mutex<Inner>,
    wake: Condvar,
    /// The cache in memory (never locked across a disk access: `disk` does those).
    cache: Mutex<HandwritingCache>,
    disk: CacheDir,
}

thread_local! {
    /// Locks of the indexer that this thread holds (see [`unlocked`]).
    static HELD: Cell<usize> = const { Cell::new(0) };
}

/// A lock of the indexer, counted in [`HELD`] while it is held.
struct Held<'a, T>(MutexGuard<'a, T>);

impl<T> Deref for Held<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Held<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Drop for Held<'_, T> {
    fn drop(&mut self) {
        HELD.with(|held| held.set(held.get().saturating_sub(1)));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Held<'_, T> {
    let guard = mutex.lock().unwrap_or_else(PoisonError::into_inner);
    HELD.with(|held| held.set(held.get() + 1));
    Held(guard)
}

/// The lock of `inner` for a wait on [`Shared::wake`] (nothing slow happens while it is held).
fn lock_for_wait(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many locks of the indexer the calling thread holds.
fn held_locks() -> usize {
    HELD.with(Cell::get)
}

/// Said before every disk access, recognition and call into the app (settings, `busy`, the
/// interface): the calling thread must hold no lock of the indexer, so that nobody waiting for
/// one waits for the disk, the engine or the app. Checked in debug builds (the tests).
fn unlocked(what: &str) {
    debug_assert_eq!(
        held_locks(),
        0,
        "{what} while holding a lock of the handwriting indexer"
    );
}

/// The running indexer (managed by the app).
pub struct Indexer {
    shared: Arc<Shared>,
    /// Waited for by the tests only (the app simply ends).
    #[cfg_attr(not(test), allow(dead_code))]
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Indexer {
    /// Starts the indexer: its first pass begins at once.
    pub fn start(services: Services) -> Indexer {
        let disk = CacheDir::new(services.cache_dir.clone());
        let shared = Arc::new(Shared {
            disk,
            services,
            inner: Mutex::new(Inner {
                generation: 1,
                ready: 0,
                stopped: false,
                quit: false,
                status: IndexStatus::default(),
                last_notified: None,
                current: HashMap::new(),
                listed: Arc::new(Vec::new()),
            }),
            wake: Condvar::new(),
            cache: Mutex::new(HandwritingCache::default()),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("ardoise-handwriting-index".into())
            .spawn(move || run(&worker))
            .ok();
        Indexer {
            shared,
            thread: Mutex::new(thread),
        }
    }

    fn bump(&self, change: impl FnOnce(&mut Inner)) -> u64 {
        let mut inner = lock(&self.shared.inner);
        inner.generation += 1;
        let generation = inner.generation;
        change(&mut inner);
        drop(inner);
        self.shared.wake.notify_all();
        generation
    }

    /// Indexes again (the library or the settings changed). Does not undo a stop. Returns the
    /// generation of the pass it asks for (see [`ready`](Self::ready)).
    pub fn refresh(&self) -> u64 {
        self.bump(|_| {})
    }

    /// The generation of the latest pass asked for (1 for the pass the start begins).
    pub fn generation(&self) -> u64 {
        lock(&self.shared.inner).generation
    }

    /// Whether the pass of `generation` (or a later one) has listed the library and taken what
    /// the cache knows of it, so that a search sees every note whose text is known; the notes
    /// it still has to recognise come after.
    pub fn ready(&self, generation: u64) -> bool {
        lock(&self.shared.inner).ready >= generation
    }

    /// Stops indexing until [`resume`](Self::resume).
    pub fn stop(&self) {
        self.bump(|inner| inner.stopped = true);
        self.shared
            .publish(true, |status| status.state = IndexState::Stopped);
    }

    pub fn resume(&self) {
        self.bump(|inner| inner.stopped = false);
    }

    pub fn status(&self) -> IndexStatus {
        lock(&self.shared.inner).status.clone()
    }

    /// The notes of the library whose handwriting has some of the words of `query`: the
    /// library as the latest pass listed it (the interface's search, which the folder watcher
    /// keeps up to date by refreshing the indexer).
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let listed = Arc::clone(&lock(&self.shared.inner).listed);
        self.search_in(query, &listed).hits
    }

    /// The notes of `notes` (a listing of the library made now) whose handwriting has some of
    /// the words of `query`, and how many of them have not been read yet. A note counts with
    /// the text of the version listed, never an older one: its record (path and stamp) must give
    /// the strokes of an entry in memory in the language recognised. A corrected transcription
    /// stands in for the recogniser's text, also for a note that was never recognised (no
    /// recognizer on this PC, or not read yet): its strokes are then read once to check that
    /// the correction is for the ink the note has now.
    pub fn search_in(&self, query: &str, notes: &[Listed]) -> ListedSearch {
        let words = query_words(query);
        let language = lock(&self.shared.inner).status.language;
        // Read without a lock: the MCP server may be another process than the app, and there
        // are few of them.
        unlocked("reading the corrections");
        let corrections = self.shared.disk.corrections();
        let corrected: HashSet<&str> = corrections.keys().map(|(id, _)| id.as_str()).collect();

        // Corrected notes whose strokes are not known for the version listed: read once (the
        // record then remembers them).
        let unknown: Vec<&Listed> = {
            let cache = lock(&self.shared.cache);
            notes
                .iter()
                .filter(|note| {
                    corrected.contains(note.id.as_str())
                        && cache.record(&note.path, &note.stamp).is_none()
                })
                .collect()
        };
        for note in unknown {
            self.shared.record_strokes(note);
        }

        let cache = lock(&self.shared.cache);
        let mut unread = 0;
        let mut hits = Vec::new();
        for note in notes {
            let record = cache.record(&note.path, &note.stamp);
            let correction = record
                .and_then(|r| corrections.get(&(r.id.clone(), r.strokes.clone())))
                .filter(|c| c.id == note.id);
            let recognition = match correction {
                Some(correction) => Some(Arc::new(correction.recognition())),
                None => {
                    let known = record.zip(language).and_then(|(record, language)| {
                        cache.loaded(&EntryKey {
                            id: record.id.clone(),
                            strokes: record.strokes.clone(),
                            language,
                        })
                    });
                    if known.is_none() && language.is_some() {
                        unread += 1;
                    }
                    known
                }
            };
            let Some(recognition) = recognition.filter(|_| !words.is_empty()) else {
                continue;
            };
            if let Some((words, excerpt)) = search_recognition(&recognition, &words) {
                hits.push(SearchHit {
                    path: path_text(&note.path),
                    words,
                    excerpt,
                });
            }
        }
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        ListedSearch { hits, unread }
    }

    /// The installed recognizers, the settings and the language they give.
    pub fn languages(
        &self,
    ) -> (
        Result<Vec<RecognizerInfo>, HwrError>,
        IndexConfig,
        Option<Language>,
    ) {
        unlocked("reading the settings");
        let config = (self.shared.services.config)();
        unlocked("listing the recognizers");
        let installed = self.shared.services.on_demand.recognizers();
        let language = installed
            .as_ref()
            .ok()
            .and_then(|list| resolve_language(&config.setting, config.interface, list));
        (installed, config, language)
    }

    /// The engine for a recognition someone waits for (the layout analysis of a text export).
    pub fn on_demand(&self) -> Arc<dyn Recognizer> {
        Arc::clone(&self.shared.services.on_demand)
    }

    /// The transcription corrected for `note` (`set_note_text`), if its ink is still the one it
    /// was written for: the text exports give it instead of the recogniser's text.
    pub fn correction_of(&self, note: &Note) -> Option<Correction> {
        unlocked("reading a correction");
        self.shared.disk.correction_for(note)
    }

    /// The recognised text of `note` (as it is in memory: an editor's unsaved changes count),
    /// from the cache or recognised now (and kept).
    pub fn recognition_of(&self, note: &Note) -> Result<Arc<Recognition>, HwrError> {
        self.recognition_in(note, None)
    }

    /// Like [`recognition_of`](Self::recognition_of), in `forced` language when given (the MCP
    /// server's `language` argument; `NoRecognizer` when none is installed for it).
    pub fn recognition_in(
        &self,
        note: &Note,
        forced: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        self.recognize(note, forced)
            .map(|(_, recognition)| recognition)
    }

    /// [`recognition_in`](Self::recognition_in) for the note of the library at `path`, read in
    /// the version `stamp`: its record is kept too, so that the search finds its text at once
    /// (the MCP server's `get_note_text`) instead of waiting for a pass to read the note again.
    pub fn recognition_at(
        &self,
        path: &Path,
        stamp: &FileStamp,
        note: &Note,
        forced: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        let (key, recognition) = self.recognize(note, forced)?;
        lock(&self.shared.cache).set_record(
            path.to_path_buf(),
            FileRecord {
                stamp: *stamp,
                id: key.id,
                strokes: key.strokes,
            },
        );
        Ok(recognition)
    }

    /// The entry of `note` in `forced` language or the one recognised here, and its text.
    fn recognize(
        &self,
        note: &Note,
        forced: Option<Language>,
    ) -> Result<(EntryKey, Arc<Recognition>), HwrError> {
        let (installed, _, language) = self.languages();
        let installed = installed?;
        let language = match forced {
            Some(forced) if recognizer_for(&installed, forced).is_some() => forced,
            Some(forced) => return Err(HwrError::NoRecognizer { language: forced }),
            None => language.ok_or(HwrError::NoRecognizer {
                language: installed
                    .iter()
                    .find_map(|r| r.language)
                    .unwrap_or(Language::Fr),
            })?,
        };
        let strokes = recognizable_strokes(note);
        let key = EntryKey {
            id: note.id.clone(),
            strokes: strokes_hash(&strokes),
            language,
        };
        if let Some(found) = self.shared.cached(&key) {
            return Ok((key, found));
        }
        let recognition = if strokes.is_empty() {
            Recognition::empty(language)
        } else {
            unlocked("recognising a note");
            self.shared
                .services
                .on_demand
                .recognize(&strokes, language)?
        };
        Ok((key.clone(), self.shared.keep(key, recognition)))
    }

    /// Whether the thread ended (the tests: it only ends on `shutdown`, or when it panicked).
    #[cfg(test)]
    fn thread_ended(&self) -> bool {
        lock(&self.thread)
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
    }

    /// Stops the thread and waits for it (the tests; the app simply ends). Fails when the
    /// thread panicked (a debug check such as [`unlocked`] failed there).
    #[cfg(test)]
    pub fn shutdown(&self) {
        self.bump(|inner| inner.quit = true);
        let thread = lock(&self.thread).take();
        if let Some(thread) = thread {
            assert!(thread.join().is_ok(), "the indexer's thread panicked");
        }
    }
}

impl Drop for Indexer {
    fn drop(&mut self) {
        // The thread ends at its next check; it is not waited for.
        self.bump(|inner| inner.quit = true);
    }
}

impl Shared {
    /// Applies `change` to the status and tells the interface (`force`, or not too often).
    fn publish(&self, force: bool, change: impl FnOnce(&mut IndexStatus)) {
        let mut inner = lock(&self.inner);
        let before = inner.status.clone();
        change(&mut inner.status);
        let due = inner
            .last_notified
            .is_none_or(|at| at.elapsed() >= NOTIFY_INTERVAL);
        let changed_state = before.state != inner.status.state;
        if inner.status != before && (force || changed_state || due) {
            inner.last_notified = Some(Instant::now());
            let status = inner.status.clone();
            drop(inner);
            unlocked("telling the interface");
            (self.services.notify)(&status);
        }
    }

    /// Whether the pass of `generation` may go on.
    fn still(&self, generation: u64) -> bool {
        let inner = lock(&self.inner);
        inner.generation == generation && !inner.quit && !inner.stopped
    }

    /// Waits while live mode or the presentation runs; `false` when the pass must end.
    fn wait_while_busy(&self, generation: u64) -> bool {
        let mut paused = false;
        loop {
            if !self.still(generation) {
                return false;
            }
            unlocked("asking whether live mode runs");
            if !(self.services.busy)() {
                if paused {
                    self.publish(true, |s| s.state = IndexState::Indexing);
                }
                return true;
            }
            if !paused {
                paused = true;
                self.publish(true, |s| s.state = IndexState::Paused);
            }
            let inner = lock_for_wait(&self.inner);
            if inner.generation == generation && !inner.quit {
                let _ = self.wake.wait_timeout(inner, self.services.pause_poll);
            }
        }
    }

    /// The pass of `generation` has listed the library and taken what the cache knows.
    fn set_ready(&self, generation: u64) {
        let mut inner = lock(&self.inner);
        inner.ready = inner.ready.max(generation);
    }

    /// Reads the strokes of the listed `note` and keeps its record (a search needs them to
    /// check a correction). Nothing is kept when the file changed meanwhile or cannot be read.
    fn record_strokes(&self, note: &Listed) {
        unlocked("reading a note");
        let before = FileStamp::of(&note.path).ok();
        let Ok(file) = read_note(&note.path) else {
            return;
        };
        let after = FileStamp::of(&note.path).ok();
        if before != Some(note.stamp) || after != before || file.note.id != note.id {
            return;
        }
        let strokes = strokes_hash(&recognizable_strokes(&file.note));
        lock(&self.cache).set_record(
            note.path.clone(),
            FileRecord {
                stamp: note.stamp,
                id: note.id.clone(),
                strokes,
            },
        );
    }

    fn unavailable(&self, reason: &'static str, language: Option<Language>, generation: u64) {
        lock(&self.inner).current.clear();
        self.set_ready(generation);
        self.publish(true, |s| {
            *s = IndexStatus {
                state: IndexState::Unavailable,
                reason: Some(reason),
                language,
                ..IndexStatus::default()
            }
        });
    }

    /// The recognition of `key`: in memory, or read from its file (without the lock).
    fn cached(&self, key: &EntryKey) -> Option<Arc<Recognition>> {
        let found = lock(&self.cache).loaded(key);
        if found.is_some() {
            return found;
        }
        unlocked("reading the cache");
        let recognition = self.disk.read_entry(key)?;
        Some(lock(&self.cache).insert(key.clone(), Arc::new(recognition)))
    }

    /// Keeps `recognition` as the entry of `key`: its file is written (without the lock), then
    /// it is kept in memory.
    fn keep(&self, key: EntryKey, recognition: Recognition) -> Arc<Recognition> {
        unlocked("writing the cache");
        self.disk.write_entry(&key, &recognition);
        lock(&self.cache).insert(key, Arc::new(recognition))
    }

    /// Writes `files.json` if it changed (without the lock; tried again later if it failed).
    fn flush(&self) {
        let Some(bytes) = lock(&self.cache).files_to_write() else {
            return;
        };
        unlocked("writing files.json");
        if !self.disk.write_files(&bytes) {
            lock(&self.cache).files_unwritten();
        }
    }
}

fn run(shared: &Shared) {
    // Read here rather than on the thread that starts the indexer (the app's main thread).
    unlocked("reading files.json");
    let files = shared.disk.read_files();
    lock(&shared.cache).load_files(files);
    let mut seen = 0;
    loop {
        let generation = {
            let mut inner = lock_for_wait(&shared.inner);
            while !inner.quit && inner.generation == seen {
                inner = shared
                    .wake
                    .wait(inner)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if inner.quit {
                return;
            }
            seen = inner.generation;
            if inner.stopped {
                continue;
            }
            seen
        };
        pass(shared, generation);
        shared.flush();
    }
}

/// One pass over the library (see the module documentation).
fn pass(shared: &Shared, generation: u64) {
    // Not even the listing of the library while live mode runs.
    if !shared.wait_while_busy(generation) {
        return;
    }
    unlocked("reading the settings");
    let config = (shared.services.config)();
    // Listed even when nothing can be recognised here: the search still finds the corrected
    // transcriptions (`set_note_text`) of the notes listed.
    unlocked("listing the library");
    let library = config.library.and_then(|root| Library::open(root).ok());
    let mut notes = library
        .as_ref()
        .map(|library| library.list().notes)
        .unwrap_or_default();
    notes.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    if !shared.still(generation) {
        return;
    }
    lock(&shared.inner).listed = Arc::new(notes.iter().map(Listed::from).collect());
    unlocked("listing the recognizers");
    let language = match shared.services.background.recognizers() {
        Err(HwrError::Unsupported) => return shared.unavailable(UNSUPPORTED, None, generation),
        Err(_) => return shared.unavailable(NO_RECOGNIZER, None, generation),
        Ok(installed) => resolve_language(&config.setting, config.interface, &installed),
    };
    let Some(language) = language else {
        return shared.unavailable(NO_RECOGNIZER, None, generation);
    };
    let Some(library) = library else {
        return shared.unavailable(NO_LIBRARY, Some(language), generation);
    };

    // The notes whose entry is known without reading them.
    let keys: Vec<Option<EntryKey>> = {
        let cache = lock(&shared.cache);
        notes
            .iter()
            .map(|note| {
                cache
                    .record(&note.path, &note.stamp)
                    .map(|record| EntryKey {
                        id: record.id.clone(),
                        strokes: record.strokes.clone(),
                        language,
                    })
            })
            .collect()
    };
    let mut current = HashMap::new();
    let mut pending = Vec::new();
    for (note, key) in notes.iter().zip(keys) {
        // Reading the entries from the disk (after a restart) waits for live mode too.
        if !shared.wait_while_busy(generation) {
            return;
        }
        match key {
            Some(key) if shared.cached(&key).is_some() => {
                current.insert(note.path.clone(), key);
            }
            _ => pending.push(note),
        }
    }
    let total = notes.len();
    let mut done = current.len();
    let mut failed = 0;
    if !shared.still(generation) {
        return;
    }
    lock(&shared.inner).current = current;
    shared.publish(true, |s| {
        *s = IndexStatus {
            state: IndexState::Indexing,
            reason: None,
            language: Some(language),
            done,
            total,
            failed,
        }
    });
    shared.set_ready(generation);

    for (count, entry) in pending.into_iter().enumerate() {
        if !shared.wait_while_busy(generation) {
            return;
        }
        if count % FLUSH_EVERY == FLUSH_EVERY - 1 {
            shared.flush();
        }
        unlocked("reading a note");
        let loaded = match library.read(&entry.path) {
            Ok(loaded) => loaded,
            Err(_) => {
                failed += 1;
                shared.publish(false, |s| s.failed = failed);
                continue;
            }
        };
        let strokes = recognizable_strokes(&loaded.file.note);
        let key = EntryKey {
            id: loaded.file.note.id.clone(),
            strokes: strokes_hash(&strokes),
            language,
        };
        lock(&shared.cache).set_record(
            loaded.path.clone(),
            FileRecord {
                stamp: loaded.stamp,
                id: key.id.clone(),
                strokes: key.strokes.clone(),
            },
        );
        let known = shared.cached(&key).is_some();
        if !known && strokes.is_empty() {
            // Nothing to read: no need to wake the engine.
            shared.keep(key.clone(), Recognition::empty(language));
        } else if !known {
            unlocked("recognising a note");
            match shared.services.background.recognize(&strokes, language) {
                Ok(recognition) => {
                    shared.keep(key.clone(), recognition);
                }
                Err(HwrError::Unsupported) => {
                    return shared.unavailable(UNSUPPORTED, None, generation);
                }
                Err(HwrError::NoRecognizer { .. }) => {
                    return shared.unavailable(NO_RECOGNIZER, None, generation);
                }
                Err(_) => {
                    failed += 1;
                    shared.publish(false, |s| s.failed = failed);
                    continue;
                }
            }
        }
        if !shared.still(generation) {
            return;
        }
        lock(&shared.inner).current.insert(entry.path.clone(), key);
        done += 1;
        shared.publish(false, |s| s.done = done);
    }

    finish(shared, library.root(), &notes);
    shared.publish(true, |s| s.state = IndexState::Idle);
}

/// End of a complete pass: forget the files no longer there and the entries made useless.
fn finish(shared: &Shared, root: &Path, notes: &[ardoise_core::library::NoteEntry]) {
    let present: HashSet<PathBuf> = notes.iter().map(|n| n.path.clone()).collect();
    let mut in_use: HashMap<String, HashSet<String>> = HashMap::new();
    for key in lock(&shared.inner).current.values() {
        in_use
            .entry(key.id.clone())
            .or_default()
            .insert(key.strokes.clone());
    }
    {
        let mut cache = lock(&shared.cache);
        cache.retain_files(root, &present);
        cache.retain_entries(&in_use);
    }
    // A correction of another version of a note is removed only once the note, read again
    // now, has other strokes: it may be for an edit made after this pass listed the library
    // (another process, the MCP server, writes them at any time).
    let paths: HashMap<&str, &Path> = notes
        .iter()
        .map(|note| (note.id.as_str(), note.path.as_path()))
        .collect();
    let is_current = |correction: &Correction| {
        let Some(path) = paths.get(correction.id.as_str()) else {
            return false;
        };
        unlocked("reading a note");
        // A note that cannot be read now is not judged: its correction stays.
        read_note(path).map_or(true, |file| {
            file.note.id != correction.id
                || strokes_hash(&recognizable_strokes(&file.note)) == correction.strokes
        })
    };
    unlocked("pruning the cache");
    shared.disk.prune(&in_use, &is_current);
    shared.flush();
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use ardoise_core::format::NoteFile;
    use ardoise_hwr::{Bounds, Line, Word};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    use super::*;
    use crate::library::tests::{library, sample_note};

    /// Words the fake engine "reads": each stroke's first point gives one, by its x / 1000.
    const VOCABULARY: [&str; 6] = ["le", "chat", "dort", "lumière", "forêt", "énergie"];

    /// Reads the words of [`VOCABULARY`] out of the strokes; counts its calls, notes when they
    /// start, and how many locks of the indexer the calling thread held.
    struct Fake {
        installed: Vec<RecognizerInfo>,
        calls: AtomicUsize,
        fail: AtomicBool,
        delay: Duration,
        started: Mutex<Vec<Instant>>,
        held: AtomicUsize,
    }

    impl Fake {
        fn new(languages: &[Language]) -> Arc<Fake> {
            Arc::new(Fake {
                installed: languages
                    .iter()
                    .map(|l| RecognizerInfo {
                        name: format!("Fake {}", l.code()),
                        language: Some(*l),
                    })
                    .collect(),
                calls: AtomicUsize::new(0),
                fail: AtomicBool::new(false),
                delay: Duration::ZERO,
                started: Mutex::new(Vec::new()),
                held: AtomicUsize::new(0),
            })
        }

        fn slow(languages: &[Language], delay: Duration) -> Arc<Fake> {
            Arc::new(Fake {
                delay,
                ..Arc::into_inner(Fake::new(languages)).unwrap()
            })
        }

        /// The most locks of the indexer held by a thread that called it.
        fn held(&self) -> usize {
            self.held.load(Ordering::SeqCst)
        }
    }

    impl Recognizer for Fake {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            self.held.fetch_max(held_locks(), Ordering::SeqCst);
            Ok(self.installed.clone())
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
        ) -> Result<Recognition, HwrError> {
            self.held.fetch_max(held_locks(), Ordering::SeqCst);
            self.started.lock().unwrap().push(Instant::now());
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if self.fail.load(Ordering::SeqCst) {
                return Err(HwrError::Engine {
                    step: "fake",
                    message: "failed".into(),
                });
            }
            if recognizer_for(&self.installed, language).is_none() {
                return Err(HwrError::NoRecognizer { language });
            }
            let words = strokes
                .iter()
                .filter_map(|s| s.points.first())
                .map(|p| Word {
                    text: VOCABULARY[(p.x / 1000.0) as usize % VOCABULARY.len()].into(),
                    alternatives: Vec::new(),
                    bounds: Bounds::default(),
                })
                .collect();
            Ok(Recognition {
                language,
                recognizer: "Fake".into(),
                lines: vec![Line::new(words)],
            })
        }
    }

    /// Strokes the fake reads as `words` (indices into [`VOCABULARY`]).
    fn written(words: &[usize]) -> Vec<Stroke> {
        words
            .iter()
            .map(|&w| Stroke {
                tool: Tool::Ballpoint,
                color: Rgba::BLACK,
                width: 40.0,
                layer: 0,
                points: vec![
                    Point {
                        x: w as f32 * 1000.0 + 10.0,
                        y: 500.0,
                        pressure: 0.5,
                        t: 0,
                    },
                    Point {
                        x: w as f32 * 1000.0 + 200.0,
                        y: 520.0,
                        pressure: 0.5,
                        t: 5,
                    },
                ],
            })
            .collect()
    }

    fn create(library: &Library, title: &str, words: &[usize]) -> PathBuf {
        let mut note = sample_note(title);
        note.id = format!("id-{title}");
        note.strokes = written(words);
        library
            .create(Path::new(""), &NoteFile::new(note))
            .unwrap()
            .path
    }

    struct Harness {
        indexer: Indexer,
        statuses: Arc<Mutex<Vec<IndexStatus>>>,
        busy: Arc<AtomicBool>,
        config: Arc<Mutex<IndexConfig>>,
        _cache: tempfile::TempDir,
    }

    fn harness(root: &Path, fake: Arc<Fake>) -> Harness {
        let cache = tempfile::tempdir().unwrap();
        harness_in(root, fake, cache)
    }

    fn harness_in(root: &Path, fake: Arc<Fake>, cache: tempfile::TempDir) -> Harness {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let busy = Arc::new(AtomicBool::new(false));
        let config = Arc::new(Mutex::new(IndexConfig {
            library: Some(root.to_path_buf()),
            setting: HandwritingLanguage::Auto,
            interface: Language::Fr,
        }));
        let (s, b, c) = (statuses.clone(), busy.clone(), config.clone());
        let indexer = Indexer::start(Services {
            background: fake.clone(),
            on_demand: fake,
            config: Box::new(move || lock(&c).clone()),
            busy: Box::new(move || b.load(Ordering::SeqCst)),
            notify: Box::new(move |status| lock(&s).push(status.clone())),
            cache_dir: cache.path().to_path_buf(),
            pause_poll: Duration::from_millis(5),
        });
        Harness {
            indexer,
            statuses,
            busy,
            config,
            _cache: cache,
        }
    }

    impl Harness {
        /// Waits until the status satisfies `done`.
        fn wait(&self, what: &str, done: impl Fn(&IndexStatus) -> bool) -> IndexStatus {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = self.indexer.status();
                if done(&status) {
                    return status;
                }
                assert!(
                    !self.indexer.thread_ended(),
                    "the indexer's thread panicked while waiting for {what}: {status:?}"
                );
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {what}: {status:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn idle(&self, total: usize) -> IndexStatus {
            self.wait("the end of the pass", |s| {
                s.state == IndexState::Idle && s.total == total && s.done + s.failed == total
            })
        }
    }

    #[test]
    fn languages_follow_the_setting_and_the_installed_recognizers() {
        let fr = RecognizerInfo {
            name: "Français".into(),
            language: Some(Language::Fr),
        };
        let en = RecognizerInfo {
            name: "English".into(),
            language: Some(Language::En),
        };
        let other = RecognizerInfo {
            name: "Español".into(),
            language: None,
        };
        let both = [other.clone(), en.clone(), fr.clone()];
        let french = [other.clone(), fr.clone()];
        let auto = HandwritingLanguage::Auto;
        assert_eq!(
            resolve_language(&auto, Language::En, &both),
            Some(Language::En)
        );
        assert_eq!(
            resolve_language(&auto, Language::Fr, &both),
            Some(Language::Fr)
        );
        // English interface, English handwriting pack missing: French.
        assert_eq!(
            resolve_language(&auto, Language::En, &french),
            Some(Language::Fr)
        );
        assert_eq!(resolve_language(&auto, Language::En, &[other]), None);
        // An explicit choice is not replaced by another language.
        assert_eq!(
            resolve_language(&HandwritingLanguage::En, Language::Fr, &french),
            None
        );
        assert_eq!(
            resolve_language(&HandwritingLanguage::En, Language::Fr, &both),
            Some(Language::En)
        );
        // A value of a newer version works as auto.
        let unknown = HandwritingLanguage::Unknown("de".into());
        assert_eq!(
            resolve_language(&unknown, Language::Fr, &both),
            Some(Language::Fr)
        );
    }

    #[test]
    fn the_library_is_indexed_once_and_searched() {
        let (dir, library) = library();
        let cat = create(&library, "Chat", &[0, 1, 2]);
        let light = create(&library, "Lumière", &[3]);
        create(&library, "Vide", &[]);
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        let status = h.idle(3);
        assert_eq!((status.done, status.failed), (3, 0));
        assert_eq!(status.language, Some(Language::Fr));
        // A note without strokes needs no recognition.
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);

        let hits = h.indexer.search("CHAT dort");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, path_text(&cat));
        assert_eq!(hits[0].words, ["chat", "dort"]);
        assert_eq!(hits[0].excerpt.word, "chat");
        let hits = h.indexer.search("lumiere");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, path_text(&light));
        assert!(h.indexer.search("foret").is_empty());
        assert!(h.indexer.search("   ").is_empty());
        // The progress reached the interface, ending idle.
        let statuses = lock(&h.statuses).clone();
        assert!(statuses.iter().any(|s| s.state == IndexState::Indexing));
        assert_eq!(statuses.last().map(|s| s.state), Some(IndexState::Idle));
        h.indexer.shutdown();

        // Another run of the app: nothing is recognised again, the search works at once.
        let fake = Fake::new(&[Language::Fr]);
        let cache = h._cache;
        let h = harness_in(dir.path(), fake.clone(), cache);
        h.idle(3);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(h.indexer.search("chat").len(), 1);
        h.indexer.shutdown();
    }

    #[test]
    fn edited_renamed_and_removed_notes_are_followed() {
        let (dir, library) = library();
        let cat = create(&library, "Chat", &[1]);
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        h.idle(1);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);

        // A title change rewrites the file, the strokes are the same: no new recognition.
        let loaded = library.read(&cat).unwrap();
        let renamed = library
            .retitle(&cat, "Le chat", Some(&loaded.stamp))
            .unwrap()
            .path;
        h.indexer.refresh();
        h.wait("the renamed note", |_| {
            !h.indexer.search("chat").is_empty()
                && h.indexer.search("chat")[0].path == path_text(&renamed)
        });
        h.idle(1);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);

        // New strokes: recognised again.
        let mut loaded = library.read(&renamed).unwrap();
        loaded.file.note.strokes = written(&[4]);
        library.save(&renamed, &loaded.file, &loaded.stamp).unwrap();
        h.indexer.refresh();
        h.wait("the edited note", |_| !h.indexer.search("foret").is_empty());
        assert!(h.indexer.search("chat").is_empty());
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);

        // Removed: no longer found.
        library.trash(&renamed).unwrap();
        h.indexer.refresh();
        h.idle(0);
        assert!(h.indexer.search("foret").is_empty());
        h.indexer.shutdown();
    }

    #[test]
    fn a_corrected_transcription_stands_in_for_the_recognisers_text() {
        let (dir, library) = library();
        let cat = create(&library, "Chat", &[0, 1, 2]);
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        h.idle(1);
        assert_eq!(h.indexer.search("chat").len(), 1);
        assert!(h.indexer.search("renard").is_empty());

        // The assistant's text replaces the recogniser's words in the search, whichever process
        // wrote it (the file is read at each search).
        let note = library.read(&cat).unwrap().file.note;
        let disk = CacheDir::new(h._cache.path());
        disk.set_correction_for(&note, Language::Fr, "Le renard\r\ndort dans la forêt")
            .unwrap();
        let hits = h.indexer.search("renard foret");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, path_text(&cat));
        assert_eq!(hits[0].words, ["renard", "foret"]);
        assert_eq!(hits[0].excerpt.word, "renard");
        assert!(h.indexer.search("chat").is_empty());
        assert_eq!(disk.correction_for(&note).unwrap().language, Language::Fr);
        // The recognition itself is untouched.
        assert_eq!(
            h.indexer.recognition_of(&note).unwrap().text(),
            "le chat dort"
        );

        // An empty text removes it.
        disk.set_correction_for(&note, Language::Fr, "  ").unwrap();
        assert!(disk.correction_for(&note).is_none());
        assert!(h.indexer.search("renard").is_empty());
        assert_eq!(h.indexer.search("chat").len(), 1);

        // The correction is bound to the ink: written again, then the note is edited, it is
        // not found any more (and the pruning at the end of the next pass removes the file).
        disk.set_correction_for(&note, Language::Fr, "Le renard")
            .unwrap();
        assert_eq!(h.indexer.search("renard").len(), 1);
        let mut loaded = library.read(&cat).unwrap();
        loaded.file.note.strokes = written(&[4]);
        library.save(&cat, &loaded.file, &loaded.stamp).unwrap();
        h.indexer.refresh();
        h.wait("the edited note", |_| !h.indexer.search("foret").is_empty());
        assert!(h.indexer.search("renard").is_empty());
        assert!(disk.correction_for(&loaded.file.note).is_none());
        h.idle(1);
        let leftover = std::fs::read_dir(h._cache.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with("-fix.json"))
            .count();
        assert_eq!(leftover, 0, "the older version's correction is pruned");
        h.indexer.shutdown();
    }

    fn listed(library: &Library) -> Vec<Listed> {
        library.list().notes.iter().map(Listed::from).collect()
    }

    #[test]
    fn a_listing_of_now_is_searched_with_the_text_of_each_version() {
        let (dir, library) = library();
        let cat = create(&library, "Chat", &[1]);
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        h.idle(1);
        let found = h.indexer.search_in("chat", &listed(&library));
        assert_eq!((found.hits.len(), found.unread), (1, 0));

        // Edited since the pass (nobody refreshed it): the old text is not searched any more,
        // the note counts as not read yet.
        let mut loaded = library.read(&cat).unwrap();
        loaded.file.note.strokes = written(&[4]);
        library.save(&cat, &loaded.file, &loaded.stamp).unwrap();
        let found = h.indexer.search_in("chat", &listed(&library));
        assert_eq!((found.hits.len(), found.unread), (0, 1));
        // A new note too.
        let fox = create(&library, "Renard", &[5]);
        assert_eq!(h.indexer.search_in("x", &listed(&library)).unread, 2);

        // Read on demand with its place in the library (`get_note_text`): found at once.
        let new = library.read(&fox).unwrap();
        h.indexer
            .recognition_at(&fox, &new.stamp, &new.file.note, None)
            .unwrap();
        let found = h.indexer.search_in("energie", &listed(&library));
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].path, path_text(&fox));
        assert_eq!(found.unread, 1);

        // Once a pass has looked again, everything is read.
        let generation = h.indexer.refresh();
        h.wait("the new pass", |_| h.indexer.ready(generation));
        h.idle(2);
        let found = h.indexer.search_in("foret", &listed(&library));
        assert_eq!((found.hits.len(), found.unread), (1, 0));
        assert!(
            h.indexer
                .search_in("chat", &listed(&library))
                .hits
                .is_empty()
        );
        h.indexer.shutdown();
    }

    #[test]
    fn a_correction_is_found_without_any_recognition() {
        let (dir, library) = library();
        let cat = create(&library, "Chat", &[1]);
        // No recognizer on this PC: nothing is read, nothing is counted as unread.
        let h = harness(dir.path(), Fake::new(&[]));
        let status = h.wait("unavailable", |s| s.state == IndexState::Unavailable);
        assert_eq!(status.reason, Some(NO_RECOGNIZER));
        assert!(h.indexer.ready(h.indexer.generation()));
        let found = h.indexer.search_in("renard", &listed(&library));
        assert_eq!(found, ListedSearch::default());

        // The assistant's transcription is found all the same, by the listing given and by the
        // interface's search (the library as the pass listed it).
        let loaded = library.read(&cat).unwrap();
        let disk = CacheDir::new(h._cache.path());
        disk.set_correction_for(&loaded.file.note, Language::Fr, "Le renard")
            .unwrap();
        let found = h.indexer.search_in("renard", &listed(&library));
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].path, path_text(&cat));
        assert_eq!(h.indexer.search("renard").len(), 1);

        // Not once the ink has changed.
        let mut edited = loaded;
        edited.file.note.strokes = written(&[4]);
        library.save(&cat, &edited.file, &edited.stamp).unwrap();
        assert!(
            h.indexer
                .search_in("renard", &listed(&library))
                .hits
                .is_empty()
        );
        h.indexer.shutdown();
    }

    #[test]
    fn a_correction_written_during_a_pass_is_not_pruned_at_its_end() {
        let (dir, library) = library();
        for i in 0..6 {
            create(&library, &format!("Note {i}"), &[2]);
        }
        // The most recently changed note is read first.
        std::thread::sleep(Duration::from_millis(20));
        let cat = create(&library, "Chat", &[1]);
        let fake = Fake::slow(&[Language::Fr], Duration::from_millis(60));
        let h = harness(dir.path(), fake.clone());
        h.wait("the first note", |s| s.done >= 1);
        assert_eq!(h.indexer.search("chat").len(), 1);

        // While the pass reads the others, the note is edited and the new ink corrected
        // (the MCP server, another process).
        let mut loaded = library.read(&cat).unwrap();
        loaded.file.note.strokes = written(&[4]);
        library.save(&cat, &loaded.file, &loaded.stamp).unwrap();
        // As `set_note_text` does: the note as it is in its file.
        let edited = library.read(&cat).unwrap().file.note;
        let disk = CacheDir::new(h._cache.path());
        disk.set_correction_for(&edited, Language::Fr, "Le renard")
            .unwrap();
        assert!(
            h.indexer.status().state == IndexState::Indexing,
            "the pass is still under way"
        );
        h.idle(7);
        // The pass ended with the old ink in use: the correction of the new one stays.
        assert_eq!(disk.correction_for(&edited).unwrap().text, "Le renard");
        assert_eq!(
            h.indexer.search_in("renard", &listed(&library)).hits.len(),
            1
        );
        h.indexer.shutdown();
    }

    #[test]
    fn indexing_waits_for_live_mode_and_can_be_stopped() {
        let (dir, library) = library();
        for i in 0..3 {
            create(&library, &format!("Note {i}"), &[1]);
        }
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        h.busy.store(true, Ordering::SeqCst);
        h.indexer.refresh();
        h.wait("the pause", |s| s.state == IndexState::Paused);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            0,
            "nothing while live mode runs"
        );
        h.busy.store(false, Ordering::SeqCst);
        h.idle(3);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 3);

        // Stopped: a new note is not indexed until resumed.
        h.indexer.stop();
        assert_eq!(h.indexer.status().state, IndexState::Stopped);
        create(&library, "Nouvelle", &[5]);
        h.indexer.refresh();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(h.indexer.status().state, IndexState::Stopped);
        assert!(h.indexer.search("energie").is_empty());
        h.indexer.resume();
        h.idle(4);
        assert_eq!(h.indexer.search("energie").len(), 1);
        h.indexer.shutdown();
    }

    #[test]
    fn a_newer_request_interrupts_the_pass() {
        let (dir, library) = library();
        for i in 0..20 {
            create(&library, &format!("Note {i}"), &[2]);
        }
        let fake = Fake::slow(&[Language::Fr], Duration::from_millis(20));
        let h = harness(dir.path(), fake.clone());
        h.wait("some progress", |s| {
            s.state == IndexState::Indexing && s.done >= 2
        });
        // The library changes: the pass starts over and still ends with every note.
        lock(&h.config).library = None;
        h.indexer.refresh();
        let status = h.wait("no library", |s| s.state == IndexState::Unavailable);
        assert_eq!(status.reason, Some(NO_LIBRARY));
        let calls = fake.calls.load(Ordering::SeqCst);
        assert!(calls < 20, "interrupted after {calls} notes");
        lock(&h.config).library = Some(dir.path().to_path_buf());
        h.indexer.refresh();
        h.idle(20);
        // Notes recognised before the interruption were kept.
        assert_eq!(fake.calls.load(Ordering::SeqCst), 20);
        h.indexer.shutdown();
    }

    #[test]
    fn missing_recognizers_and_failures_are_reported() {
        let (dir, library) = library();
        create(&library, "Chat", &[1]);
        // English chosen, only French installed.
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        lock(&h.config).setting = HandwritingLanguage::En;
        h.indexer.refresh();
        let status = h.wait("unavailable", |s| s.state == IndexState::Unavailable);
        assert_eq!(status.reason, Some(NO_RECOGNIZER));
        assert!(h.indexer.search("chat").is_empty());
        // Back to auto: French.
        lock(&h.config).setting = HandwritingLanguage::Auto;
        h.indexer.refresh();
        h.idle(1);
        assert_eq!(h.indexer.search("chat").len(), 1);
        h.indexer.shutdown();

        // The engine fails on a note: counted, the others go on.
        let fake = Fake::new(&[Language::Fr]);
        fake.fail.store(true, Ordering::SeqCst);
        let h = harness(dir.path(), fake.clone());
        let status = h.idle(1);
        assert_eq!((status.done, status.failed), (0, 1));
        h.indexer.shutdown();

        // No engine at all.
        let none = Fake::new(&[]);
        let h = harness(dir.path(), none);
        let status = h.wait("unavailable", |s| s.state == IndexState::Unavailable);
        assert_eq!(status.reason, Some(NO_RECOGNIZER));
        h.indexer.shutdown();
    }

    #[test]
    fn a_note_in_memory_is_recognised_on_demand_and_kept() {
        let (dir, _library) = library();
        let fake = Fake::new(&[Language::Fr]);
        let h = harness(dir.path(), fake.clone());
        h.idle(0);
        let mut note = sample_note("Brouillon");
        note.strokes = written(&[3, 4]);
        let text = h.indexer.recognition_of(&note).unwrap();
        assert_eq!(text.text(), "lumière forêt");
        h.indexer.recognition_of(&note).unwrap();
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            1,
            "the second time from the cache"
        );
        // A hidden layer is not read.
        note.layers[0].visible = false;
        assert_eq!(h.indexer.recognition_of(&note).unwrap().text(), "");
        h.indexer.shutdown();

        let h = harness(dir.path(), Fake::new(&[]));
        assert!(matches!(
            h.indexer.recognition_of(&note),
            Err(HwrError::NoRecognizer { .. })
        ));
        h.indexer.shutdown();
    }

    #[test]
    fn a_pass_pauses_between_two_notes_while_live_mode_runs() {
        let (dir, library) = library();
        for i in 0..10 {
            create(&library, &format!("Note {i}"), &[i % 6]);
        }
        let fake = Fake::slow(&[Language::Fr], Duration::from_millis(40));
        let h = harness(dir.path(), fake.clone());
        h.wait("some progress", |s| {
            s.state == IndexState::Indexing && s.done >= 2
        });
        // Live mode starts in the middle of the pass.
        h.busy.store(true, Ordering::SeqCst);
        let raised = Instant::now();
        h.wait("the pause", |s| s.state == IndexState::Paused);
        let paused = Instant::now();
        std::thread::sleep(Duration::from_millis(150));
        let started = lock(&fake.started).clone();
        assert!(
            started.iter().filter(|&&at| at > raised).count() <= 1,
            "at most the note under way when live mode started"
        );
        assert!(
            started.iter().all(|&at| at < paused),
            "no recognition once paused"
        );
        let done = h.indexer.status().done;
        assert!(done < 10, "{done}");

        // Stopped while paused, then resumed while live mode still runs: paused again.
        h.indexer.stop();
        assert_eq!(h.indexer.status().state, IndexState::Stopped);
        h.indexer.resume();
        h.wait("the pause again", |s| s.state == IndexState::Paused);
        assert_eq!(fake.calls.load(Ordering::SeqCst), started.len());

        // Live mode ends: the pass goes on where it was and ends.
        h.busy.store(false, Ordering::SeqCst);
        let status = h.idle(10);
        assert_eq!((status.done, status.failed), (10, 0));
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            10,
            "nothing recognised twice"
        );
        let states: Vec<IndexState> = lock(&h.statuses).iter().map(|s| s.state).collect();
        let resumed = states
            .windows(2)
            .any(|w| w == [IndexState::Paused, IndexState::Indexing]);
        assert!(resumed, "{states:?}");
        h.indexer.shutdown();
    }

    /// The indexer's locks are never held while the engine recognises, nor across a disk
    /// access or a call into the app (`unlocked` checks the latter in debug builds: a failure
    /// there panics the indexer's thread, which `shutdown` reports).
    #[test]
    fn no_lock_is_held_while_recognising_or_reading_files() {
        let (dir, library) = library();
        for i in 0..4 {
            create(&library, &format!("Note {i}"), &[i]);
        }
        create(&library, "Vide", &[]);
        let fake = Fake::slow(&[Language::Fr], Duration::from_millis(300));
        let h = harness(dir.path(), fake.clone());
        h.wait("a recognition under way", |_| {
            fake.calls.load(Ordering::SeqCst) >= 1
        });
        // While the engine works, the interface's calls answer at once.
        let asked = Instant::now();
        let _ = h.indexer.status();
        let _ = h.indexer.search("chat");
        h.indexer.stop();
        h.indexer.resume();
        assert!(
            asked.elapsed() < Duration::from_millis(150),
            "the interface waited {:?} for the engine",
            asked.elapsed()
        );
        h.idle(5);
        // On demand, from another thread (a command).
        let mut note = sample_note("Brouillon");
        note.strokes = written(&[2]);
        assert_eq!(h.indexer.recognition_of(&note).unwrap().text(), "dort");
        assert_eq!(fake.held(), 0);
        h.indexer.shutdown();

        // Another run reads its cache from the disk, without a lock either.
        let fake = Fake::new(&[Language::Fr]);
        let cache = h._cache;
        let h = harness_in(dir.path(), fake.clone(), cache);
        h.idle(5);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(h.indexer.recognition_of(&note).unwrap().text(), "dort");
        assert_eq!(fake.held(), 0);
        h.indexer.shutdown();
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "while holding a lock of the handwriting indexer")]
    fn holding_a_lock_across_slow_work_is_caught() {
        let mutex = Mutex::new(());
        let _held = lock(&mutex);
        unlocked("reading a note");
    }

    #[test]
    fn locks_are_counted_while_held() {
        let mutex = Mutex::new(1);
        assert_eq!(held_locks(), 0);
        {
            let mut held = lock(&mutex);
            *held += 1;
            assert_eq!(held_locks(), 1);
        }
        assert_eq!(held_locks(), 0);
        assert_eq!(*lock(&mutex), 2);
        assert_eq!(held_locks(), 0);
    }
}
