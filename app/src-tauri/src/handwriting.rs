//! Handwriting recognition in the app (SPEC §4.3, §4.7): the library search also finds notes by
//! their handwriting, and the recognised text is exported as `.txt` or copied. Engine:
//! `ardoise_hwr` (Windows' own recognizers, French, and English when Windows' English
//! handwriting pack is installed). Contract: `docs/design/extras.md`, "Handwriting recognition".
//!
//! - [`cache`]: the results, a cache on this PC (`%LOCALAPPDATA%\Ardoise\handwriting\`) keyed by
//!   note id, strokes and language. **The notes are never written for the recognition**: the
//!   format's `text.json` member stays unused, so that two PCs never compete to rewrite the
//!   same note through Nextcloud.
//! - [`indexer`]: recognises in the background the notes whose entry is missing or stale,
//!   pausing while live mode or the presentation runs; its progress goes to the main window
//!   (event [`HANDWRITING_STATUS`]).
//! - [`search`]: which words of a query the handwriting of each note has, with an excerpt.
//!
//! The recognition language is `[handwriting] language` of `ardoise.toml` (auto, fr or en,
//! shared by every PC); each PC resolves it with its own recognizers.

mod cache;
mod indexer;
mod search;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ardoise_core::fsutil::write_atomic;
use ardoise_core::settings::HandwritingLanguage;
use ardoise_hwr::{HwrError, Language, RecognizerInfo, SystemRecognizer};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, current_state, store, update};
use crate::export::{ExportSource, export_error, library_for, load_sources};
use crate::live::LiveRuntime;

pub use indexer::{IndexConfig, IndexStatus, Indexer, Services};
pub use search::SearchHit;

/// Event sent to the main window at every change of the indexing (payload: [`IndexStatus`]).
pub const HANDWRITING_STATUS: &str = "handwriting-status";

/// How often a paused indexing checks whether live mode or the presentation ended.
const PAUSE_POLL: Duration = Duration::from_millis(500);

/// Where the cache lives: `%LOCALAPPDATA%\Ardoise\handwriting`, local to this PC (never in the
/// library nor next to the portable exe); under the temporary folder when `LOCALAPPDATA` is
/// not set (Linux).
pub fn handwriting_dir() -> PathBuf {
    handwriting_dir_in(std::env::var_os("LOCALAPPDATA"))
}

fn handwriting_dir_in(local_app_data: Option<OsString>) -> PathBuf {
    local_app_data
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Ardoise")
        .join("handwriting")
}

fn hwr_error(error: HwrError) -> CommandError {
    CommandError::new(error.code(), error)
}

/// The indexer of the running app, started with the main window.
#[derive(Default)]
pub struct HandwritingRuntime {
    indexer: OnceLock<Indexer>,
}

impl HandwritingRuntime {
    /// Starts the background indexing (once).
    pub fn start(&self, app: &AppHandle) {
        let (for_config, for_busy, for_notify) = (app.clone(), app.clone(), app.clone());
        let _ = self.indexer.set(Indexer::start(Services {
            background: Arc::new(SystemRecognizer::background()),
            on_demand: Arc::new(SystemRecognizer::new()),
            config: Box::new(move || index_config(&for_config.state::<AppContext>())),
            busy: Box::new(move || busy(&for_busy)),
            notify: Box::new(move |status| {
                let _ = for_notify.emit_to("main", HANDWRITING_STATUS, status);
            }),
            cache_dir: handwriting_dir(),
            pause_poll: PAUSE_POLL,
        }));
    }

    fn indexer(&self) -> CommandResult<&Indexer> {
        self.indexer.get().ok_or_else(|| {
            CommandError::new("handwritingFailed", "the handwriting index is not started")
        })
    }
}

/// The library, the settings or the interface language may have changed: the indexing looks
/// again (nothing before the app started it).
pub fn refresh(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<HandwritingRuntime>()
        && let Some(indexer) = runtime.indexer.get()
    {
        indexer.refresh();
    }
}

/// True while live mode runs or the presentation window is open: their latency budget is
/// 30 ms, the indexing waits.
fn busy(app: &AppHandle) -> bool {
    let live = app
        .try_state::<LiveRuntime>()
        .is_some_and(|runtime| runtime.hub().page_position().is_some_and(|p| p.running));
    live || app
        .get_webview_window(crate::presentation::PRESENTATION)
        .is_some()
}

/// The library, the recognition language and the interface language, read fresh.
fn index_config(context: &AppContext) -> IndexConfig {
    let setting = store(context)
        .ok()
        .and_then(|store| store.load().ok())
        .map(|settings| settings.handwriting.language)
        .unwrap_or_default();
    IndexConfig {
        library: current_state(context).library().map(PathBuf::from),
        setting,
        interface: if crate::diagnostic::report_language(context) == "fr" {
            Language::Fr
        } else {
            Language::En
        },
    }
}

/// `HandwritingInfo` in `handwriting.ts`: the recognizers of this PC, the setting and the
/// language it gives, the indexing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandwritingInfo {
    /// False outside Windows (no engine).
    pub supported: bool,
    pub recognizers: Vec<RecognizerInfo>,
    /// Only auto / fr / en: a value written by a newer version is reported as auto.
    pub setting: HandwritingLanguage,
    /// The language recognised on this PC; `None` when there is no recognizer for it.
    pub language: Option<Language>,
    pub status: IndexStatus,
}

fn info_of(indexer: &Indexer) -> HandwritingInfo {
    let (installed, config, language) = indexer.languages();
    HandwritingInfo {
        supported: !matches!(installed, Err(HwrError::Unsupported)),
        recognizers: installed.unwrap_or_default(),
        setting: config.setting.known(),
        language,
        status: indexer.status(),
    }
}

/// A blocking call on the indexer, off the async runtime's threads.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&Indexer, &AppContext) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = app.state::<HandwritingRuntime>();
        work(runtime.indexer()?, &app.state::<AppContext>())
    })
    .await
    .map_err(|error| CommandError::new("handwritingFailed", error))?
}

#[tauri::command]
pub async fn handwriting_info(app: AppHandle) -> CommandResult<HandwritingInfo> {
    blocking(app, |indexer, _| Ok(info_of(indexer))).await
}

/// Sets the recognition language (shared by every PC) and indexes again. Saving `auto` keeps a
/// value written by a newer version (the same thing for this one).
#[tauri::command]
pub async fn set_handwriting_language(
    app: AppHandle,
    language: HandwritingLanguage,
) -> CommandResult<HandwritingInfo> {
    blocking(app, move |indexer, context| {
        let language = language.known();
        update(context, |settings| {
            let stored = &mut settings.handwriting.language;
            if language != HandwritingLanguage::Auto
                || !matches!(stored, HandwritingLanguage::Unknown(_))
            {
                *stored = language;
            }
        })?;
        indexer.refresh();
        Ok(info_of(indexer))
    })
    .await
}

#[tauri::command]
pub fn handwriting_status(runtime: State<'_, HandwritingRuntime>) -> CommandResult<IndexStatus> {
    Ok(runtime.indexer()?.status())
}

/// The notes whose handwriting has words of `query`, with an excerpt each.
#[tauri::command]
pub async fn handwriting_search(app: AppHandle, query: String) -> CommandResult<Vec<SearchHit>> {
    blocking(app, move |indexer, _| Ok(indexer.search(&query))).await
}

/// Stops the indexing until [`handwriting_resume`].
#[tauri::command]
pub fn handwriting_stop(runtime: State<'_, HandwritingRuntime>) -> CommandResult<IndexStatus> {
    let indexer = runtime.indexer()?;
    indexer.stop();
    Ok(indexer.status())
}

#[tauri::command]
pub fn handwriting_resume(runtime: State<'_, HandwritingRuntime>) -> CommandResult<()> {
    runtime.indexer()?.resume();
    Ok(())
}

/// The recognised text of one note (`NoteText` in `handwriting.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteText {
    pub path: String,
    /// The note's title, or its file name (its date) when it has none.
    pub title: String,
    /// Line after line.
    pub text: String,
}

/// The recognised text of `sources` (the editor's note as it is in memory, or notes of the
/// library), recognised now when the cache does not have it.
fn texts_of(
    indexer: &Indexer,
    context: &AppContext,
    sources: Vec<ExportSource>,
) -> CommandResult<Vec<NoteText>> {
    let library = library_for(context, &sources)?;
    let paths: Vec<String> = sources.iter().map(|s| s.path.clone()).collect();
    let loaded = load_sources(library.as_ref(), sources)?;
    loaded
        .into_iter()
        .zip(paths)
        .map(|(source, path)| {
            let recognition = indexer.recognition_of(&source.note).map_err(hwr_error)?;
            let title = source.note.title.trim();
            Ok(NoteText {
                path,
                title: if title.is_empty() {
                    source.stem
                } else {
                    title.to_owned()
                },
                text: recognition.text(),
            })
        })
        .collect()
}

/// The text export: one note after the other, its title underlined, then its text; Windows
/// line ends.
pub fn text_document(notes: &[NoteText]) -> String {
    let mut out = String::new();
    for (index, note) in notes.iter().enumerate() {
        if index > 0 {
            out.push_str("\r\n\r\n");
        }
        out.push_str(&note.title);
        out.push_str("\r\n");
        out.push_str(&"=".repeat(note.title.chars().count().max(1)));
        out.push_str("\r\n\r\n");
        for line in note.text.lines() {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out
}

/// The recognised text of each source (a preview, or one note's text).
#[tauri::command]
pub async fn handwriting_text(
    app: AppHandle,
    sources: Vec<ExportSource>,
) -> CommandResult<Vec<NoteText>> {
    blocking(app, move |indexer, context| {
        texts_of(indexer, context, sources)
    })
    .await
}

/// Writes the recognised text of `sources` to `destination` (UTF-8, atomically); returns it.
#[tauri::command]
pub async fn export_handwriting_text(
    app: AppHandle,
    sources: Vec<ExportSource>,
    destination: String,
) -> CommandResult<String> {
    blocking(app, move |indexer, context| {
        let document = text_document(&texts_of(indexer, context, sources)?);
        write_text(Path::new(&destination), &document)?;
        Ok(destination)
    })
    .await
}

fn write_text(destination: &Path, document: &str) -> CommandResult<()> {
    write_atomic(destination, document.as_bytes())
        .map_err(|error| export_error(format!("{}: {error}", destination.display())))
}

/// Copies the recognised text of `sources` to the clipboard (`clipboardUnavailable` outside
/// Windows).
#[tauri::command]
pub async fn copy_handwriting_text(
    app: AppHandle,
    window: tauri::WebviewWindow,
    sources: Vec<ExportSource>,
) -> CommandResult<()> {
    #[cfg(windows)]
    let owner = window.hwnd().map_or(0, |hwnd| hwnd.0 as usize);
    #[cfg(not(windows))]
    let owner = {
        let _ = window;
        0
    };
    blocking(app, move |indexer, context| {
        let document = text_document(&texts_of(indexer, context, sources)?);
        crate::clipboard::copy_text(&document, owner)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::{context, library, sample_note};

    #[test]
    fn the_cache_is_local_to_the_pc() {
        let dir = handwriting_dir_in(Some(OsString::from(r"C:\Users\x\AppData\Local")));
        assert!(dir.ends_with(Path::new("Ardoise").join("handwriting")));
        assert!(dir.starts_with(r"C:\Users\x\AppData\Local"));
        let fallback = handwriting_dir_in(None);
        assert!(fallback.starts_with(std::env::temp_dir()));
        assert_eq!(handwriting_dir_in(Some(OsString::new())), fallback);
    }

    #[test]
    fn the_text_document_has_each_note_with_its_title() {
        let notes = [
            NoteText {
                path: "a".into(),
                title: "Électricité".into(),
                text: "la loi d'Ohm\nU = R × I".into(),
            },
            NoteText {
                path: "b".into(),
                title: "2026-10-24".into(),
                text: String::new(),
            },
        ];
        assert_eq!(
            text_document(&notes),
            "Électricité\r\n===========\r\n\r\nla loi d'Ohm\r\nU = R × I\r\n\
             \r\n\r\n2026-10-24\r\n==========\r\n\r\n"
        );
        assert_eq!(text_document(&[]), "");
    }

    #[test]
    fn the_text_is_written_as_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("texte.txt");
        write_text(&file, "forêt\r\n").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), "forêt\r\n".as_bytes());
        let error = write_text(&dir.path().join("absent").join("x.txt"), "x").unwrap_err();
        assert_eq!(error.code(), "exportFailed");
    }

    #[test]
    fn the_index_follows_the_settings() {
        let settings = tempfile::tempdir().unwrap();
        let (dir, _library) = library();
        let context = context(settings.path());
        let config = index_config(&context);
        assert_eq!(config.library, None);
        assert_eq!(config.setting, HandwritingLanguage::Auto);
        // The test context's system is French.
        assert_eq!(config.interface, Language::Fr);
        crate::commands::choose_library(&context, &dir.path().display().to_string()).unwrap();
        update(&context, |s| {
            s.language = ardoise_core::settings::Language::En;
            s.handwriting.language = HandwritingLanguage::Fr;
        })
        .unwrap();
        let config = index_config(&context);
        assert_eq!(config.library.as_deref(), Some(dir.path()));
        assert_eq!(config.setting, HandwritingLanguage::Fr);
        assert_eq!(config.interface, Language::En);
    }

    #[test]
    fn texts_come_from_the_note_in_memory_or_the_library() {
        let settings = tempfile::tempdir().unwrap();
        let (dir, library) = library();
        let context = context(settings.path());
        crate::commands::choose_library(&context, &dir.path().display().to_string()).unwrap();
        let cache = tempfile::tempdir().unwrap();
        let indexer = Indexer::start(Services {
            background: Arc::new(Unsupported),
            on_demand: Arc::new(Unsupported),
            config: Box::new(|| IndexConfig {
                library: None,
                setting: HandwritingLanguage::Auto,
                interface: Language::Fr,
            }),
            busy: Box::new(|| false),
            notify: Box::new(|_| {}),
            cache_dir: cache.path().to_path_buf(),
            pause_poll: Duration::from_millis(5),
        });
        let saved = crate::library::tests::write_sample(&library, "", "Cours");
        let source = ExportSource {
            path: saved.path.display().to_string(),
            note: None,
            image_key: None,
        };
        // Without an engine: the code says why.
        let error = texts_of(&indexer, &context, vec![source.clone()]).unwrap_err();
        assert_eq!(error.code(), "handwritingUnsupported");
        let missing = ExportSource {
            path: dir.path().join("absent.ardoise").display().to_string(),
            note: None,
            image_key: None,
        };
        assert_eq!(
            texts_of(&indexer, &context, vec![missing])
                .unwrap_err()
                .code(),
            "noteNotFound"
        );
        let in_memory = ExportSource {
            path: "x".into(),
            note: Some(sample_note("Brouillon")),
            image_key: None,
        };
        assert_eq!(
            texts_of(&indexer, &context, vec![in_memory])
                .unwrap_err()
                .code(),
            "handwritingUnsupported"
        );
        indexer.shutdown();
    }

    /// Windows' engine, the cache and the indexer together: the demo note written by hand is found
    /// by a word of its handwriting, and the words the end-to-end journeys look for in titles are
    /// found nowhere in the demo handwriting. Skipped without the French recognizer.
    #[cfg(windows)]
    #[test]
    fn the_demo_note_is_found_by_its_handwriting() {
        use std::time::Instant;
        let recognizer = Arc::new(SystemRecognizer::new());
        let installed = ardoise_hwr::Recognizer::recognizers(&*recognizer).unwrap_or_default();
        if ardoise_hwr::recognizer_for(&installed, Language::Fr).is_none() {
            println!("skipped: no French handwriting recognizer on this PC");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Démo");
        crate::demo::create_demo_library(&root).unwrap();
        let cache = tempfile::tempdir().unwrap();
        let library = root.clone();
        let indexer = Indexer::start(Services {
            background: recognizer.clone(),
            on_demand: recognizer,
            config: Box::new(move || IndexConfig {
                library: Some(library.clone()),
                setting: HandwritingLanguage::Auto,
                interface: Language::Fr,
            }),
            busy: Box::new(|| false),
            notify: Box::new(|_| {}),
            cache_dir: cache.path().to_path_buf(),
            pause_poll: Duration::from_millis(5),
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let status = indexer.status();
            if status.state == indexer::IndexState::Idle && status.total == 8 {
                assert_eq!((status.done, status.failed), (8, 0));
                break;
            }
            assert!(Instant::now() < deadline, "indexing too slow: {status:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
        let hits = indexer.search("lumiere");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].path.contains("Cours d'optique"), "{hits:?}");
        assert_eq!(hits[0].excerpt.word, "lumière");
        assert_eq!(indexer.search("LIGNE droite").len(), 1);
        for word in ["energie", "bout", "journey", "electron", "lumi\u{e8}res"] {
            assert!(
                indexer.search(word).is_empty(),
                "{word}: {:?}",
                indexer.search(word)
            );
        }
        indexer.shutdown();
    }

    /// No engine, as outside Windows.
    struct Unsupported;

    impl ardoise_hwr::Recognizer for Unsupported {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            Err(HwrError::Unsupported)
        }

        fn recognize(
            &self,
            _: &[ardoise_ink::Stroke],
            _: Language,
        ) -> Result<ardoise_hwr::Recognition, HwrError> {
            Err(HwrError::Unsupported)
        }
    }
}
