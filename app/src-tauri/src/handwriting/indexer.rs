//! The background indexer: recognises the notes of the library whose cache entry is missing or
//! stale, most recently changed first, on a thread of its own at background priority.
//!
//! A pass starts at launch, after every change in the library (the folder watcher), when the
//! library or the language changes, and when the user resumes it. A newer request interrupts
//! the pass under way; so does stopping it. While live mode or the presentation window runs
//! (their latency budget is 30 ms) it waits between two notes. Its progress goes to the
//! interface after every note (at most a few times a second).
//!
//! Everything the indexer needs from the app comes through [`Services`], so that it is tested
//! with a fake recognizer and plain folders.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ardoise_core::library::Library;
use ardoise_core::note::Note;
use ardoise_core::settings::HandwritingLanguage;
use ardoise_hwr::ink::recognizable_strokes;
use ardoise_hwr::{
    HwrError, Language, Recognition, Recognizer, RecognizerInfo, lower_thread_priority,
    recognizer_for,
};
use serde::Serialize;

use super::cache::{EntryKey, FileRecord, HandwritingCache, strokes_hash};
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
    /// True while live mode or the presentation runs.
    pub busy: Box<dyn Fn() -> bool + Send + Sync>,
    /// Receives the status at every change.
    pub notify: Box<dyn Fn(&IndexStatus) + Send + Sync>,
    pub cache_dir: PathBuf,
    /// How often a paused pass checks whether it may go on.
    pub pause_poll: Duration,
}

struct Inner {
    /// Bumped by every request; a pass goes on only while it is the one it started with.
    generation: u64,
    stopped: bool,
    quit: bool,
    status: IndexStatus,
    last_notified: Option<Instant>,
    /// The notes of the library whose entry is known, by path.
    current: HashMap<PathBuf, EntryKey>,
}

struct Shared {
    services: Services,
    inner: Mutex<Inner>,
    wake: Condvar,
    cache: Mutex<HandwritingCache>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let cache = HandwritingCache::open(services.cache_dir.clone());
        let shared = Arc::new(Shared {
            services,
            inner: Mutex::new(Inner {
                generation: 1,
                stopped: false,
                quit: false,
                status: IndexStatus::default(),
                last_notified: None,
                current: HashMap::new(),
            }),
            wake: Condvar::new(),
            cache: Mutex::new(cache),
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

    fn bump(&self, change: impl FnOnce(&mut Inner)) {
        let mut inner = lock(&self.shared.inner);
        inner.generation += 1;
        change(&mut inner);
        drop(inner);
        self.shared.wake.notify_all();
    }

    /// Indexes again (the library or the settings changed). Does not undo a stop.
    pub fn refresh(&self) {
        self.bump(|_| {});
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

    /// The notes of the library whose handwriting has some of the words of `query`.
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let words = query_words(query);
        if words.is_empty() {
            return Vec::new();
        }
        let current: Vec<(PathBuf, EntryKey)> = lock(&self.shared.inner)
            .current
            .iter()
            .map(|(path, key)| (path.clone(), key.clone()))
            .collect();
        let cache = lock(&self.shared.cache);
        let mut hits: Vec<SearchHit> = current
            .iter()
            .filter_map(|(path, key)| {
                let recognition = cache.loaded(key)?;
                let (words, excerpt) = search_recognition(&recognition, &words)?;
                Some(SearchHit {
                    path: path_text(path),
                    words,
                    excerpt,
                })
            })
            .collect();
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        hits
    }

    /// The installed recognizers, the settings and the language they give.
    pub fn languages(
        &self,
    ) -> (
        Result<Vec<RecognizerInfo>, HwrError>,
        IndexConfig,
        Option<Language>,
    ) {
        let config = (self.shared.services.config)();
        let installed = self.shared.services.on_demand.recognizers();
        let language = installed
            .as_ref()
            .ok()
            .and_then(|list| resolve_language(&config.setting, config.interface, list));
        (installed, config, language)
    }

    /// The recognised text of `note` (as it is in memory: an editor's unsaved changes count),
    /// from the cache or recognised now (and kept).
    pub fn recognition_of(&self, note: &Note) -> Result<Arc<Recognition>, HwrError> {
        let (installed, _, language) = self.languages();
        let installed = installed?;
        let language = language.ok_or(HwrError::NoRecognizer {
            language: installed
                .iter()
                .find_map(|r| r.language)
                .unwrap_or(Language::Fr),
        })?;
        let strokes = recognizable_strokes(note);
        let key = EntryKey {
            id: note.id.clone(),
            strokes: strokes_hash(&strokes),
            language,
        };
        if let Some(found) = lock(&self.shared.cache).get(&key) {
            return Ok(found);
        }
        let recognition = if strokes.is_empty() {
            Recognition::empty(language)
        } else {
            self.shared
                .services
                .on_demand
                .recognize(&strokes, language)?
        };
        Ok(lock(&self.shared.cache).put(key, recognition))
    }

    /// Stops the thread and waits for it (the tests; the app simply ends).
    #[cfg(test)]
    pub fn shutdown(&self) {
        self.bump(|inner| inner.quit = true);
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
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
            let inner = lock(&self.inner);
            if inner.generation == generation && !inner.quit {
                let _ = self.wake.wait_timeout(inner, self.services.pause_poll);
            }
        }
    }

    fn unavailable(&self, reason: &'static str, language: Option<Language>) {
        lock(&self.inner).current.clear();
        self.publish(true, |s| {
            *s = IndexStatus {
                state: IndexState::Unavailable,
                reason: Some(reason),
                language,
                ..IndexStatus::default()
            }
        });
    }
}

fn run(shared: &Shared) {
    lower_thread_priority();
    let mut seen = 0;
    loop {
        let generation = {
            let mut inner = lock(&shared.inner);
            while !inner.quit && inner.generation == seen {
                inner = shared
                    .wake
                    .wait(inner)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        lock(&shared.cache).flush();
    }
}

/// One pass over the library (see the module documentation).
fn pass(shared: &Shared, generation: u64) {
    // Not even the listing of the library while live mode runs.
    if !shared.wait_while_busy(generation) {
        return;
    }
    let config = (shared.services.config)();
    let language = match shared.services.background.recognizers() {
        Err(HwrError::Unsupported) => return shared.unavailable(UNSUPPORTED, None),
        Err(_) => return shared.unavailable(NO_RECOGNIZER, None),
        Ok(installed) => resolve_language(&config.setting, config.interface, &installed),
    };
    let Some(language) = language else {
        return shared.unavailable(NO_RECOGNIZER, None);
    };
    let Some(library) = config.library.and_then(|root| Library::open(root).ok()) else {
        return shared.unavailable(NO_LIBRARY, Some(language));
    };
    let mut notes = library.list().notes;
    notes.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.path.cmp(&b.path))
    });

    // The notes whose entry is known without reading them.
    let mut current = HashMap::new();
    let mut pending = Vec::new();
    {
        let mut cache = lock(&shared.cache);
        for note in &notes {
            let key = cache
                .record(&note.path, &note.stamp)
                .map(|record| EntryKey {
                    id: record.id.clone(),
                    strokes: record.strokes.clone(),
                    language,
                });
            match key {
                Some(key) if cache.get(&key).is_some() => {
                    current.insert(note.path.clone(), key);
                }
                _ => pending.push(note),
            }
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

    for (count, entry) in pending.into_iter().enumerate() {
        if !shared.wait_while_busy(generation) {
            return;
        }
        if count % FLUSH_EVERY == FLUSH_EVERY - 1 {
            lock(&shared.cache).flush();
        }
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
        let known = {
            let mut cache = lock(&shared.cache);
            cache.set_record(
                loaded.path.clone(),
                FileRecord {
                    stamp: loaded.stamp,
                    id: key.id.clone(),
                    strokes: key.strokes.clone(),
                },
            );
            cache.get(&key).is_some()
        };
        if !known && strokes.is_empty() {
            // Nothing to read: no need to wake the engine.
            lock(&shared.cache).put(key.clone(), Recognition::empty(language));
        } else if !known {
            match shared.services.background.recognize(&strokes, language) {
                Ok(recognition) => {
                    lock(&shared.cache).put(key.clone(), recognition);
                }
                Err(HwrError::Unsupported) => return shared.unavailable(UNSUPPORTED, None),
                Err(HwrError::NoRecognizer { .. }) => {
                    return shared.unavailable(NO_RECOGNIZER, None);
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
    let mut cache = lock(&shared.cache);
    cache.retain_files(root, &present);
    cache.prune(&in_use);
    cache.flush();
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

    /// Reads the words of [`VOCABULARY`] out of the strokes; counts its calls.
    struct Fake {
        installed: Vec<RecognizerInfo>,
        calls: AtomicUsize,
        fail: AtomicBool,
        delay: Duration,
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
            })
        }
    }

    impl Recognizer for Fake {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            Ok(self.installed.clone())
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
        ) -> Result<Recognition, HwrError> {
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
        let fake = Arc::new(Fake {
            delay: Duration::from_millis(20),
            ..Arc::into_inner(Fake::new(&[Language::Fr])).unwrap()
        });
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
}
