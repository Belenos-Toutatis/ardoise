//! `ardoise.exe --mcp [--read-only]`: the MCP server (SPEC §4.9, `docs/design/mcp.md`) on standard
//! input/output, without a window. This is the app's side of `ardoise-mcp`: it finds the library
//! and the language like the app does, and gives the server the handwriting cache and indexer
//! of M7 through the crate's traits.
//!
//! Nothing but protocol is written to stdout. Messages for the user go to stderr, which a GUI
//! program may not have: they are written best effort.

use std::io::Write as _;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use ardoise_core::library::Library;
use ardoise_core::note::Note;
use ardoise_core::settings::SettingsStore;
use ardoise_hwr::structure::{Structure, structure_of};
use ardoise_hwr::{HwrError, Language, Recognition, SystemRecognizer};
use ardoise_mcp::{
    Environment, Handwriting, Host, ServerOptions, TextHit, TextSearch, TextStatus, ToolError,
};

use crate::AppContext;
use crate::commands::current_state;
use crate::handwriting::{
    CacheDir, IndexState, IndexStatus, Indexer, PAUSE_POLL, Services, handwriting_dir, index_config,
};

mod write;

/// How long a search waits for the indexer to read the notes whose text is not in the cache yet
/// (the cached ones load in a fraction of a second; recognising a note takes about a second).
const INDEX_WAIT: Duration = Duration::from_secs(20);

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
}

/// Windows' handwriting recognition with the per-PC cache, through the app's indexer. The
/// indexer starts at the first use (a session that only lists notes never wakes the engine);
/// its background pass then reads the rest of the library at low priority while the server runs,
/// so that the search sees more notes over time.
struct AppHandwriting {
    context: Arc<AppContext>,
    indexer: OnceLock<Indexer>,
}

impl AppHandwriting {
    fn indexer(&self) -> &Indexer {
        self.indexer.get_or_init(|| {
            let context = Arc::clone(&self.context);
            Indexer::start(Services {
                background: Arc::new(SystemRecognizer::background()),
                on_demand: Arc::new(SystemRecognizer::new()),
                config: Box::new(move || index_config(&context)),
                // Live mode runs in the app's process, which this one cannot ask: the low
                // priority of the background engine is what keeps the lesson smooth.
                busy: Box::new(|| false),
                notify: Box::new(|_| {}),
                cache_dir: handwriting_dir(),
                pause_poll: PAUSE_POLL,
            })
        })
    }

    /// The status once the first pass is over (every note read or failed), or after
    /// [`INDEX_WAIT`]: a small library is searched whole, a large one answers with what is read
    /// so far and the number of notes still to read.
    fn settled_status(&self) -> IndexStatus {
        let indexer = self.indexer();
        let started = Instant::now();
        loop {
            let status = indexer.status();
            // The default status (before the pass has listed the library) is `Idle` with no
            // language and no reason: it is not a finished pass.
            let started_pass = status.language.is_some() || status.reason.is_some();
            let over = started_pass && status.state != IndexState::Indexing;
            if over || started.elapsed() >= INDEX_WAIT {
                return status;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Handwriting for AppHandwriting {
    fn recognition_of(
        &self,
        note: &Note,
        language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        self.indexer().recognition_in(note, language)
    }

    fn structure_of(&self, note: &Note, recognition: &Recognition) -> Structure {
        structure_of(&*self.indexer().on_demand(), recognition, note)
    }

    fn correction_of(&self, note: &Note) -> Option<ardoise_mcp::Correction> {
        // Read from the cache folder: no need to wake the engine.
        let correction = CacheDir::new(handwriting_dir()).correction_for(note)?;
        Some(ardoise_mcp::Correction {
            text: correction.text,
            language: Some(correction.language),
        })
    }

    fn search(&self, query: &str) -> TextSearch {
        let status = self.settled_status();
        let available = status.state != IndexState::Unavailable;
        TextSearch {
            hits: self
                .indexer()
                .search(query)
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
                unread: if available {
                    status.total.saturating_sub(status.done)
                } else {
                    0
                },
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
    let handwriting = Arc::new(AppHandwriting {
        context: Arc::clone(&context),
        indexer: OnceLock::new(),
    });
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
    use super::*;
    use crate::Cli;

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
