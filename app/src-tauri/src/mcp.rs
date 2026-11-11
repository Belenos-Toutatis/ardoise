//! `ardoise.exe --mcp [--read-only]`: the MCP server (SPEC §4.9, `docs/design/mcp.md`) on standard
//! input/output, without a window. This is the app's side of `ardoise-mcp`: it finds the library
//! and the language like the app does, and gives the server the handwriting cache and indexer
//! of M7 through the crate's traits.
//!
//! Nothing but protocol is written to stdout. Messages for the user go to stderr, which a GUI
//! program may not have: they are written best effort.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use ardoise_core::library::{FileStamp, Library, NoteEntry};
use ardoise_core::note::Note;
use ardoise_core::settings::SettingsStore;
use ardoise_hwr::structure::{Structure, structure_of};
use ardoise_hwr::{HwrError, Language, Recognition, SystemRecognizer};
use ardoise_ink::ShapeTuning;
use ardoise_mcp::{
    Environment, Handwriting, Host, ServerOptions, TextHit, TextSearch, TextStatus, ToolError,
};

use crate::AppContext;
use crate::commands::current_state;
use crate::handwriting::{
    CacheDir, IndexState, Indexer, Listed, PAUSE_POLL, Services, handwriting_dir, index_config,
};

mod write;

/// How long a search waits for the indexer to list the library and take what its cache knows
/// (a fraction of a second, unless the disk is very slow).
const READY_WAIT: Duration = Duration::from_secs(10);
/// When at most this many notes are left to recognise (a note written or edited since the last
/// search), the search waits for them...
const FEW_LEFT: usize = 10;
/// ... at most this long (recognising a note takes about a second);
const FEW_LEFT_WAIT: Duration = Duration::from_secs(15);
/// with more (a first pass over a large library), this long only: it answers with what is read
/// and the number of notes left, which the pass goes on reading while the server runs.
const MANY_LEFT_WAIT: Duration = Duration::from_secs(1);
/// How often a waiting search looks at the indexer.
const WAIT_POLL: Duration = Duration::from_millis(20);

/// Command-line options of the mode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct McpArgs {
    /// `--read-only`: only the read tools are served.
    pub read_only: bool,
}

/// This PC's library and settings, read afresh at each call.
struct AppEnvironment {
    context: Arc<AppContext>,
}

impl Environment for AppEnvironment {
    fn library(&self) -> Result<Library, ToolError> {
        let state = current_state(&self.context);
        let Some(root) = state.library() else {
            let detail = state
                .settings_error()
                .map(|error| format!(" ({error})"))
                .unwrap_or_default();
            return Err(ToolError::new(
                "noLibrary",
                format!(
                    "no library is chosen on this computer ({}){detail}: open Ardoise once and choose one",
                    state.machine()
                ),
            ));
        };
        Ok(Library::open(root)?)
    }

    fn hidden_binder(&self) -> Option<String> {
        let state = current_state(&self.context);
        state
            .auto_export()
            .is_active()
            .then(|| state.auto_export().valid_folder())
            .flatten()
    }

    fn language(&self) -> Language {
        if crate::diagnostic::report_language(&self.context) == "fr" {
            Language::Fr
        } else {
            Language::En
        }
    }

    /// `[shapes]` of `ardoise.toml`, the setting next to the editor's button (read afresh: the
    /// user may change it while the assistant is connected).
    fn shape_tuning(&self) -> ShapeTuning {
        ardoise_core::settings::ShapeSettings::from(current_state(&self.context).shapes()).tuning()
    }
}

/// Windows' handwriting recognition with the per-PC cache, through the app's indexer. The
/// indexer starts at the first use (a session that only lists notes never wakes the engine);
/// its background pass then reads the rest of the library at low priority while the server runs.
/// No folder watcher here: each search compares the library's listing with the one the indexer
/// last looked at and asks it to look again when the library changed (a note written by the app
/// or a tool, synced, moved, split, straightened...).
struct AppHandwriting {
    context: Arc<AppContext>,
    /// What the indexer is started with.
    services: Box<dyn Fn() -> Services + Send + Sync>,
    /// The cache folder, where the corrected transcriptions are.
    cache: CacheDir,
    indexer: OnceLock<Indexer>,
    /// The fingerprint of the listing the indexer was last asked to look at.
    seen: Mutex<Option<u64>>,
}

impl AppHandwriting {
    /// This PC's recognition and cache.
    fn new(context: Arc<AppContext>) -> AppHandwriting {
        let for_config = Arc::clone(&context);
        AppHandwriting::with_services(
            context,
            CacheDir::new(handwriting_dir()),
            Box::new(move || {
                let context = Arc::clone(&for_config);
                Services {
                    background: Arc::new(SystemRecognizer::background()),
                    on_demand: Arc::new(SystemRecognizer::new()),
                    config: Box::new(move || index_config(&context)),
                    // Live mode runs in the app's process, which this one cannot ask: the low
                    // priority of the background engine is what keeps the lesson smooth.
                    busy: Box::new(|| false),
                    notify: Box::new(|_| {}),
                    cache_dir: handwriting_dir(),
                    pause_poll: PAUSE_POLL,
                }
            }),
        )
    }

    fn with_services(
        context: Arc<AppContext>,
        cache: CacheDir,
        services: Box<dyn Fn() -> Services + Send + Sync>,
    ) -> AppHandwriting {
        AppHandwriting {
            context,
            services,
            cache,
            indexer: OnceLock::new(),
            seen: Mutex::new(None),
        }
    }

    fn indexer(&self) -> &Indexer {
        self.indexer
            .get_or_init(|| Indexer::start((self.services)()))
    }

    /// Asks the indexer to look again when `notes` is not the library it last looked at, then
    /// waits for what is reasonable: the listing and the cached texts, then the notes left to
    /// recognise if they are few (a note just written or edited), else a moment only.
    fn catch_up(&self, notes: &[NoteEntry]) -> &Indexer {
        let fresh = self.indexer.get().is_none();
        let indexer = self.indexer();
        let fingerprint = fingerprint(notes);
        let changed = {
            let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
            let changed = *seen != Some(fingerprint);
            *seen = Some(fingerprint);
            changed
        };
        // A fresh indexer's first pass lists the library of now.
        let target = if changed && !fresh {
            indexer.refresh()
        } else {
            indexer.generation()
        };
        let started = Instant::now();
        while !indexer.ready(target) {
            if started.elapsed() >= READY_WAIT {
                return indexer;
            }
            std::thread::sleep(WAIT_POLL);
        }
        let status = indexer.status();
        let left = status.total.saturating_sub(status.done + status.failed);
        let budget = if left <= FEW_LEFT {
            FEW_LEFT_WAIT
        } else {
            MANY_LEFT_WAIT
        };
        let waiting = Instant::now();
        while indexer.status().state == IndexState::Indexing && waiting.elapsed() < budget {
            std::thread::sleep(WAIT_POLL);
        }
        indexer
    }
}

/// What tells two listings apart: every note's path, version and id.
fn fingerprint(notes: &[NoteEntry]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for note in notes {
        note.path.hash(&mut hasher);
        note.stamp.hash(&mut hasher);
        note.id.hash(&mut hasher);
    }
    notes.len().hash(&mut hasher);
    hasher.finish()
}

impl Handwriting for AppHandwriting {
    fn recognition_of(
        &self,
        note: &Note,
        language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        self.indexer().recognition_in(note, language)
    }

    fn recognition_at(
        &self,
        path: &Path,
        stamp: &FileStamp,
        note: &Note,
        language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        self.indexer().recognition_at(path, stamp, note, language)
    }

    fn structure_of(&self, note: &Note, recognition: &Recognition) -> Structure {
        structure_of(&*self.indexer().on_demand(), recognition, None, note)
    }

    fn correction_of(&self, note: &Note) -> Option<ardoise_mcp::Correction> {
        // Read from the cache folder: no need to wake the engine.
        let correction = self.cache.correction_for(note)?;
        Some(ardoise_mcp::Correction {
            text: correction.text,
            language: Some(correction.language),
        })
    }

    fn search(&self, query: &str) -> TextSearch {
        let notes = current_state(&self.context)
            .library()
            .and_then(|root| Library::open(root).ok())
            .map(|library| library.list().notes)
            .unwrap_or_default();
        self.search_in(query, &notes)
    }

    fn search_in(&self, query: &str, notes: &[NoteEntry]) -> TextSearch {
        let indexer = self.catch_up(notes);
        let listed: Vec<Listed> = notes.iter().map(Listed::from).collect();
        let found = indexer.search_in(query, &listed);
        let status = indexer.status();
        let available = status.state != IndexState::Unavailable;
        TextSearch {
            hits: found
                .hits
                .into_iter()
                .map(|hit| TextHit {
                    path: hit.path.into(),
                    words: hit.words,
                    before: hit.excerpt.before,
                    word: hit.excerpt.word,
                    after: hit.excerpt.after,
                })
                .collect(),
            status: TextStatus {
                available,
                reason: status.reason.filter(|_| !available).map(str::to_owned),
                unread: if available { found.unread } else { 0 },
            },
        }
    }
}

/// The server's host: this PC's library and handwriting.
fn host() -> Host {
    let context = Arc::new(AppContext {
        store: SettingsStore::open_default().map_err(|e| e.to_string()),
        system_locale: sys_locale::get_locale(),
        self_test: None,
        open_diagnostic: false,
    });
    let handwriting = Arc::new(AppHandwriting::new(Arc::clone(&context)));
    let app = crate::app_bridge::client(&context);
    Host::new(
        Arc::new(AppEnvironment {
            context: Arc::clone(&context),
        }),
        handwriting.clone(),
    )
    .with_write(Arc::new(write::AppWrite::new(context, handwriting)))
    .with_app(app)
}

/// Best-effort message on stderr (a GUI program may have none).
fn say(message: &str) {
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// Runs the mode and returns the exit code: serves until stdin closes.
pub fn run_cli(args: McpArgs) -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            say(&format!("ardoise --mcp: cannot start the runtime: {error}"));
            return 1;
        }
    };
    let options = ServerOptions {
        read_only: args.read_only,
    };
    say(&format!(
        "ardoise --mcp: serving on standard input/output{}",
        if args.read_only { " (read only)" } else { "" }
    ));
    match runtime.block_on(ardoise_mcp::serve_stdio(host(), options)) {
        Ok(()) => 0,
        Err(error) => {
            say(&format!("ardoise --mcp: {error}"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ardoise_core::format::NoteFile;
    use ardoise_core::settings::HandwritingLanguage;
    use ardoise_hwr::{Bounds, Line, Recognizer, RecognizerInfo, Word};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};
    use ardoise_mcp::write::WriteServices;

    use super::*;
    use crate::Cli;
    use crate::handwriting::IndexConfig;
    use crate::library::tests::{context, library, sample_note};

    const WORDS: [&str; 3] = ["chat", "forêt", "renard"];

    /// Reads one word per stroke, `WORDS[x / 1000]` of its first point, after `delay`.
    struct Reader {
        delay: Duration,
    }

    impl Recognizer for Reader {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            Ok(vec![RecognizerInfo {
                name: "Fake".into(),
                language: Some(Language::Fr),
            }])
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
        ) -> Result<Recognition, HwrError> {
            std::thread::sleep(self.delay);
            let words = strokes
                .iter()
                .filter_map(|s| s.points.first())
                .map(|p| Word {
                    text: WORDS[(p.x / 1000.0) as usize % WORDS.len()].into(),
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

    /// A stroke the reader reads as `WORDS[word]`.
    fn written(word: usize) -> Vec<Stroke> {
        vec![Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points: [10.0, 200.0]
                .iter()
                .map(|dx| Point {
                    x: word as f32 * 1000.0 + dx,
                    y: 500.0,
                    pressure: 0.5,
                    t: 0,
                })
                .collect(),
        }]
    }

    /// The handwriting of the server on `root`, with the reader and a cache in `dir`.
    fn handwriting_on(root: &Path, dir: &Path, delay: Duration) -> Arc<AppHandwriting> {
        let (root, cache) = (root.to_path_buf(), dir.join("handwriting"));
        let context = Arc::new(context(dir));
        Arc::new(AppHandwriting::with_services(
            context,
            CacheDir::new(&cache),
            Box::new(move || {
                let root: PathBuf = root.clone();
                Services {
                    background: Arc::new(Reader { delay }),
                    on_demand: Arc::new(Reader { delay }),
                    config: Box::new(move || IndexConfig {
                        library: Some(root.clone()),
                        setting: HandwritingLanguage::Auto,
                        interface: Language::Fr,
                    }),
                    busy: Box::new(|| false),
                    notify: Box::new(|_| {}),
                    cache_dir: cache.clone(),
                    pause_poll: Duration::from_millis(5),
                }
            }),
        ))
    }

    fn create(library: &Library, title: &str, word: usize) -> PathBuf {
        let mut note = sample_note(title);
        note.id = format!("id-{title}");
        note.strokes = written(word);
        library
            .create(Path::new(""), &NoteFile::new(note))
            .unwrap()
            .path
    }

    /// The assistant's environment on the settings in `dir`.
    fn environment(dir: &Path) -> AppEnvironment {
        AppEnvironment {
            context: Arc::new(context(dir)),
        }
    }

    #[test]
    fn the_shape_tuning_is_read_from_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(SettingsStore::FILE_NAME);
        let environment = environment(dir.path());
        let defaults = ShapeTuning::default();
        // No file: the app's original values.
        assert_eq!(environment.shape_tuning(), defaults);
        // A file without a `[shapes]` section: the same.
        std::fs::write(&file, "language = \"fr\"\n").unwrap();
        assert_eq!(environment.shape_tuning(), defaults);

        std::fs::write(
            &file,
            r"[shapes]
tolerance = 4
min_size_mm = 7.5
snap_angle = false
snap_angle_deg = 6.0
snap_round = true
snap_round_percent = 8.0
",
        )
        .unwrap();
        // An unticked box gives `None`, whatever its value; the others are as written.
        assert_eq!(
            environment.shape_tuning(),
            ShapeTuning {
                tolerance: 4,
                min_size_mm: 7.5,
                snap_angle_deg: None,
                snap_round_percent: Some(8.0),
            }
        );

        // Read afresh at each call: the user may change the setting while the assistant is connected.
        std::fs::write(
            &file,
            r"[shapes]
snap_angle = true
snap_angle_deg = 2.5
snap_round = false
",
        )
        .unwrap();
        assert_eq!(
            environment.shape_tuning(),
            ShapeTuning {
                snap_angle_deg: Some(2.5),
                snap_round_percent: None,
                ..defaults
            }
        );

        // A value out of range is brought within the limits.
        std::fs::write(
            &file,
            r"[shapes]
tolerance = 9
min_size_mm = 2.0
",
        )
        .unwrap();
        let tuning = environment.shape_tuning();
        assert_eq!((tuning.tolerance, tuning.min_size_mm), (5, 5.0));
    }

    #[test]
    fn the_search_follows_the_library_after_a_write_tool() {
        let (_library_dir, library) = library();
        let dir = tempfile::tempdir().unwrap();
        let cat = create(&library, "Chat", 0);
        let handwriting = handwriting_on(library.root(), dir.path(), Duration::ZERO);
        let search = |query: &str| handwriting.search_in(query, &library.list().notes);
        let found = search("chat");
        assert_eq!(found.hits.len(), 1);
        assert_eq!((found.status.available, found.status.unread), (true, 0));

        // `straighten_shapes` saves new ink with the editor's save: the next search has the
        // text of the new ink, not the old one, without anyone refreshing the indexer.
        let write = write::AppWrite::new(Arc::clone(&handwriting.context), handwriting.clone());
        let opened = library.read(&cat).unwrap();
        let mut note = opened.file.note;
        note.strokes = written(1);
        write
            .save_note(&library, &cat, note, &opened.stamp, None)
            .unwrap();
        let found = search("foret");
        assert_eq!(found.hits.len(), 1, "{found:?}");
        assert_eq!(found.hits[0].path, cat);
        assert!(search("chat").hits.is_empty());

        // A note added since (the app, a sync, `create_note`, a split) is searched too.
        let fox = create(&library, "Renard", 2);
        let found = search("renard");
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].path, fox);
        assert_eq!(found.status.unread, 0);
    }

    #[test]
    fn a_long_first_pass_costs_a_search_a_moment_only() {
        let (_library_dir, library) = library();
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(FEW_LEFT + 4) {
            create(&library, &format!("Note {i}"), 0);
        }
        let handwriting = handwriting_on(library.root(), dir.path(), Duration::from_millis(400));
        let asked = Instant::now();
        let found = handwriting.search_in("chat", &library.list().notes);
        assert!(
            asked.elapsed() < MANY_LEFT_WAIT + Duration::from_secs(3),
            "{:?}",
            asked.elapsed()
        );
        assert!(found.status.available);
        assert!(found.status.unread > 0, "{found:?}");
        assert_eq!(found.hits.len() + found.status.unread, FEW_LEFT + 4);

        // A note read on demand with its place in the library is found by the next search.
        let notes = library.list().notes;
        let last = notes.iter().find(|n| n.title == "Note 0").unwrap();
        let loaded = library.read(&last.path).unwrap();
        handwriting
            .recognition_at(&loaded.path, &loaded.stamp, &loaded.file.note, None)
            .unwrap();
        let found = handwriting.search_in("chat", &library.list().notes);
        assert!(found.hits.iter().any(|hit| hit.path == last.path));
    }

    #[test]
    fn the_mode_and_its_option_are_parsed() {
        let cli = Cli::parse(["--mcp".to_owned()]);
        assert_eq!(cli.mcp, Some(McpArgs { read_only: false }));
        let cli = Cli::parse(["--mcp".to_owned(), "--read-only".to_owned()]);
        assert_eq!(cli.mcp, Some(McpArgs { read_only: true }));
        // `--read-only` alone, or another mode, does not start the server.
        assert_eq!(Cli::parse(["--read-only".to_owned()]).mcp, None);
        assert_eq!(Cli::parse(["--self-test".to_owned()]).mcp, None);
        assert_eq!(Cli::parse(Vec::<String>::new()).mcp, None);
    }
}
