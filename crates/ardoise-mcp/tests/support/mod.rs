//! What the write-tool and prompt tests share: a temporary library of synthetic notes, a fake
//! handwriting service that also keeps the corrections, fake write services made of
//! `ardoise-core`'s operations, and a client session on rmcp's in-memory transport.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ardoise_core::format::NoteFile;
use ardoise_core::library::{self, FileStamp, Library, SavedNote};
use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_core::ops::{self, NewNote};
use ardoise_hwr::structure::{DrawingArea, Item, Structure, from_recognition, ink_boxes};
use ardoise_hwr::{Bounds, HwrError, Language, Line, Recognition, Word};
use ardoise_ink::{Point, Rgba, ShapeTuning, Stroke, Tool};
use ardoise_mcp::write::{ExportOutcome, ExportRequest, ExportTarget, MergeLayout, WriteServices};
use ardoise_mcp::{
    Correction, Environment, Handwriting, Host, ServerOptions, TextHit, TextSearch, TextStatus,
    ToolError, serve,
};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use serde_json::{Value, json};

/// 2026-10-24 at noon UTC.
pub const DAY0: i64 = 1_792_843_200_000;
pub const DAY_MS: i64 = 86_400_000;

pub const OPTIQUE: &str = "11111111-1111-4111-8111-111111111111";
pub const CHIMIE: &str = "22222222-2222-4222-8222-222222222222";
pub const DEVOIR: &str = "33333333-3333-4333-8333-333333333333";
pub const VIDE: &str = "44444444-4444-4444-8444-444444444444";

// ---------------------------------------------------------------------------------------------
// Fakes

pub struct Env {
    pub root: PathBuf,
    pub hidden: Option<String>,
    pub language: Language,
    /// The user's "Redresser les formes" setting.
    pub tuning: ShapeTuning,
}

impl Environment for Env {
    fn library(&self) -> Result<Library, ToolError> {
        Ok(Library::open(&self.root)?)
    }

    fn hidden_binder(&self) -> Option<String> {
        self.hidden.clone()
    }

    fn language(&self) -> Language {
        self.language
    }

    fn shape_tuning(&self) -> ShapeTuning {
        self.tuning
    }
}

/// The corrections made with `set_note_text`, by note id.
pub type Corrections = Arc<Mutex<HashMap<String, (String, Language)>>>;

#[derive(Default)]
pub struct Fake {
    /// Lines of text by note id (what the "recognizer" reads).
    pub by_id: Mutex<HashMap<String, Vec<String>>>,
    pub corrections: Corrections,
    /// The library the search looks into (notes are found by id, wherever they were moved).
    pub root: Mutex<PathBuf>,
    /// The drawings that "Windows' ink analysis" finds, by note id (stroke indices); a note
    /// without an entry gets no analysis (the layout heuristics, `analyzed: false`).
    pub drawings: Mutex<HashMap<String, Vec<usize>>>,
}

pub fn fold(text: &str) -> String {
    ardoise_hwr::text::fold(text)
}

impl Handwriting for Fake {
    fn recognition_of(
        &self,
        note: &Note,
        language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        let lines = self
            .by_id
            .lock()
            .unwrap()
            .get(&note.id)
            .cloned()
            .unwrap_or_default();
        let lines = lines
            .iter()
            .enumerate()
            .map(|(row, text)| {
                Line::new(
                    text.split(' ')
                        .enumerate()
                        .map(|(column, word)| Word {
                            text: word.to_owned(),
                            alternatives: Vec::new(),
                            bounds: Bounds {
                                x: 1_000.0 + column as f32 * 1_500.0,
                                y: 1_000.0 + row as f32 * 2_000.0,
                                width: 1_400.0,
                                height: 600.0,
                            },
                        })
                        .collect(),
                )
            })
            .collect();
        Ok(Arc::new(Recognition {
            language: language.unwrap_or(Language::Fr),
            recognizer: "fake".into(),
            lines,
        }))
    }

    fn structure_of(&self, note: &Note, recognition: &Recognition) -> Structure {
        match self.drawings.lock().unwrap().get(&note.id) {
            Some(strokes) => Structure {
                items: vec![Item::Drawing(DrawingArea {
                    bounds: Bounds::default(),
                    strokes: strokes.clone(),
                })],
                analyzed: true,
            },
            None => Structure {
                items: from_recognition(recognition, &ink_boxes(note)),
                analyzed: false,
            },
        }
    }

    fn search(&self, query: &str) -> TextSearch {
        // Like the app: a correction stands in for the recogniser's text.
        let words: Vec<String> = fold(query).split_whitespace().map(str::to_owned).collect();
        let mut hits = Vec::new();
        let by_id = self.by_id.lock().unwrap();
        let corrections = self.corrections.lock().unwrap();
        let root = self.root.lock().unwrap().clone();
        let listing = Library::open(root).unwrap().list();
        for (path, id) in listing.notes.iter().map(|n| (n.path.clone(), n.id.clone())) {
            let lines: Vec<String> = match corrections.get(&id) {
                Some((text, _)) => text.lines().map(str::to_owned).collect(),
                None => by_id.get(&id).cloned().unwrap_or_default(),
            };
            let mut found = Vec::new();
            let mut excerpt = None;
            for word in &words {
                for line in &lines {
                    let tokens: Vec<&str> = line.split(' ').collect();
                    if let Some(at) = tokens
                        .iter()
                        .position(|t| fold(t).starts_with(word.as_str()))
                    {
                        found.push(word.clone());
                        excerpt.get_or_insert((
                            tokens[..at].join(" "),
                            tokens[at].to_owned(),
                            tokens[at + 1..].join(" "),
                        ));
                        break;
                    }
                }
            }
            if let Some((before, word, after)) = excerpt {
                hits.push(TextHit {
                    path,
                    words: found,
                    before,
                    word,
                    after,
                });
            }
        }
        TextSearch {
            hits,
            status: TextStatus {
                available: true,
                reason: None,
                unread: 0,
            },
        }
    }

    fn correction_of(&self, note: &Note) -> Option<Correction> {
        self.corrections
            .lock()
            .unwrap()
            .get(&note.id)
            .map(|(text, language)| Correction {
                text: text.clone(),
                language: Some(*language),
            })
    }
}

/// What runs just before a note is saved.
pub type BeforeSave = Box<dyn Fn(&Path) + Send>;

/// The write services of the tests: the library operations of `ardoise-core` where the app's
/// code is bigger (no thumbnails, no background), and a record of what was asked.
#[derive(Default)]
pub struct FakeWrite {
    pub corrections: Corrections,
    /// Files asked for by `create_from_file`: (file, page).
    pub imports: Mutex<Vec<(PathBuf, u32)>>,
    pub exports: Mutex<Vec<ExportRequest>>,
    /// Run just before a note is saved: lets a test change the file under the tool's feet.
    pub before_save: Mutex<Option<BeforeSave>>,
    pub saves: Mutex<usize>,
    /// The binders `create_revision` was asked to write in.
    pub creations: Mutex<Vec<PathBuf>>,
    /// How many more notes `create_revision` writes before it fails; `None`: no limit.
    pub creations_left: Mutex<Option<usize>>,
}

fn new_note(title: &str) -> NewNote {
    NewNote {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.to_owned(),
        now: DAY0 + 10 * DAY_MS,
    }
}

impl WriteServices for FakeWrite {
    fn create_blank(
        &self,
        library: &Library,
        binder: &Path,
        note: Note,
    ) -> Result<SavedNote, ToolError> {
        Ok(library.create(binder, &NoteFile::new(note))?)
    }

    fn create_from_file(
        &self,
        library: &Library,
        binder: &Path,
        file: &Path,
        pdf_page: u32,
        layer_name: &str,
    ) -> Result<SavedNote, ToolError> {
        self.imports
            .lock()
            .unwrap()
            .push((file.to_path_buf(), pdf_page));
        let mut note = make_note(
            &uuid::Uuid::new_v4().to_string(),
            &file.file_stem().unwrap().to_string_lossy(),
            10,
        );
        note.strokes.clear();
        note.layers = vec![Layer {
            name: layer_name.to_owned(),
            visible: true,
        }];
        Ok(library.create(binder, &NoteFile::new(note))?)
    }

    fn save_note(
        &self,
        library: &Library,
        path: &Path,
        note: Note,
        stamp: &FileStamp,
        account: Option<&str>,
    ) -> Result<FileStamp, ToolError> {
        if let Some(hook) = self.before_save.lock().unwrap().as_ref() {
            hook(path);
        }
        *self.saves.lock().unwrap() += 1;
        let loaded = library.read(path)?;
        let mut file = loaded.file;
        file.note = note;
        if let Some(account) = account {
            // The real clock, as the app does and as the copy itself was stamped: a fixed date
            // would fall before the copy once that date is past.
            file.add_revision_entry(chrono::Utc::now().timestamp_millis(), account);
        }
        Ok(library.save(path, &file, stamp)?)
    }

    fn create_revision(
        &self,
        library: &Library,
        binder: &Path,
        mut file: NoteFile,
        revised: Note,
    ) -> Result<SavedNote, ToolError> {
        if let Some(left) = self.creations_left.lock().unwrap().as_mut() {
            if *left == 0 {
                return Err(ToolError::new("libraryAccess", "the disk is full"));
            }
            *left -= 1;
        }
        self.creations.lock().unwrap().push(binder.to_path_buf());
        // The editor's rule, in short: the ink the copy starts from is its first version, unless
        // the file already keeps an original; the thumbnail shows the new ink.
        if file.original.is_none()
            && file.original_note.is_none()
            && !file.note.strokes.is_empty()
            && file.note.strokes != revised.strokes
        {
            file.original_note =
                Some(ardoise_core::format::to_bytes(&NoteFile::new(file.note.clone())).unwrap());
        }
        file.thumbnail =
            Some(format!("thumbnail of {} strokes", revised.strokes.len()).into_bytes());
        file.note = revised;
        Ok(library.create(binder, &file)?)
    }

    fn merge_notes(
        &self,
        library: &Library,
        paths: &[PathBuf],
        layout: MergeLayout,
        title: &str,
    ) -> Result<PathBuf, ToolError> {
        let notes: Vec<Note> = paths
            .iter()
            .map(|path| library.read(path).map(|loaded| loaded.file.note))
            .collect::<Result<_, _>>()?;
        let layout = match layout {
            MergeLayout::Overlay => ops::MergeLayout::Overlay,
            MergeLayout::EndToEnd => ops::MergeLayout::EndToEnd,
        };
        let merged = ops::merge(&notes, layout, new_note(title))
            .map_err(|error| ToolError::new("nothingToMerge", error))?;
        let binder = paths[0]
            .parent()
            .unwrap()
            .strip_prefix(library.root())
            .unwrap()
            .to_path_buf();
        Ok(library.create(&binder, &NoteFile::new(merged))?.path)
    }

    fn export(
        &self,
        _library: &Library,
        request: &ExportRequest,
    ) -> Result<ExportOutcome, ToolError> {
        self.exports.lock().unwrap().push(request.clone());
        let extension = request.format.extension();
        let files = match &request.target {
            ExportTarget::File(path) => {
                // The tool promised a free name: a file is never replaced.
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| ToolError::new("exportFailed", e))?;
                std::fs::write(path, format!("fake {extension}")).unwrap();
                vec![path.clone()]
            }
            ExportTarget::Folder(folder) => request
                .notes
                .iter()
                .map(|note| {
                    let stem = note.file_stem().unwrap().to_string_lossy().into_owned();
                    let path =
                        ardoise_mcp::write::free_file(&folder.join(format!("{stem}.{extension}")))
                            .unwrap();
                    std::fs::write(&path, format!("fake {extension}")).unwrap();
                    path
                })
                .collect(),
        };
        Ok(ExportOutcome {
            files,
            details: json!({ "fake": true }),
        })
    }

    fn set_note_text(&self, note: &Note, text: &str, language: Language) -> Result<(), ToolError> {
        let mut corrections = self.corrections.lock().unwrap();
        if text.is_empty() {
            corrections.remove(&note.id);
        } else {
            corrections.insert(note.id.clone(), (text.to_owned(), language));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Synthetic notes

pub fn wave(layer: u32, tool: Tool, y0: f32) -> Stroke {
    Stroke {
        tool,
        color: Rgba::rgb(0, 0, 255),
        width: 60.0,
        layer,
        points: (0..100)
            .map(|i| Point {
                x: 1_000.0 + i as f32 * 100.0,
                y: y0 + (i as f32 * 0.2).sin() * 400.0,
                pressure: 0.8,
                t: i as u32 * 8,
            })
            .collect(),
    }
}

/// A stroke drawing a circle, 40 mm wide, written between `t0` and `t0 + 600` ms.
pub fn circle(cx: f32, cy: f32, t0: u32) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer: 0,
        points: (0..=120)
            .map(|i| {
                let a = std::f32::consts::TAU * 1.04 * i as f32 / 120.0;
                Point {
                    x: cx + 2_000.0 * a.cos(),
                    y: cy + 2_000.0 * a.sin(),
                    pressure: 0.7,
                    t: t0 + i as u32 * 5,
                }
            })
            .collect(),
    }
}

pub fn make_note(id: &str, title: &str, day: i64) -> Note {
    Note {
        id: id.to_owned(),
        title: title.to_owned(),
        created: DAY0 + day * DAY_MS,
        modified: DAY0 + day * DAY_MS + 60_000,
        page: A4_LANDSCAPE,
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
        favorite: false,
        layers: vec![
            Layer {
                name: "Calque 1".into(),
                visible: true,
            },
            Layer {
                name: "Corrections".into(),
                visible: false,
            },
        ],
        strokes: vec![
            wave(0, Tool::Ballpoint, 5_000.0),
            wave(0, Tool::Pencil, 9_000.0),
            wave(1, Tool::Ballpoint, 13_000.0),
        ],
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

pub struct Harness {
    pub _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub fake: Arc<Fake>,
    pub write: Arc<FakeWrite>,
    pub language: Language,
    /// What the server reads as the user's setting of `straighten_shapes`.
    pub tuning: ShapeTuning,
}

impl Harness {
    /// A library with four notes in two binders (one without a title, one with a circle), and
    /// the fakes.
    pub fn new(language: Language) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Bibliothèque");
        std::fs::create_dir(&root).unwrap();
        library::init(&root).unwrap();
        let library = Library::open(&root).unwrap();
        let corrections = Corrections::default();
        let fake = Arc::new(Fake {
            corrections: Arc::clone(&corrections),
            root: Mutex::new(root.clone()),
            ..Fake::default()
        });
        let write = Arc::new(FakeWrite {
            corrections,
            ..FakeWrite::default()
        });

        let mut optique = make_note(OPTIQUE, "Cours d'optique", 0);
        optique.tags = vec!["Physique".into(), "Quatrième".into()];
        optique.favorite = true;
        let mut chimie = make_note(CHIMIE, "Chimie des métaux", 2);
        chimie.tags = vec!["chimie".into()];
        // Handwriting-like waves, then a circle drawn 2 s later.
        chimie.strokes.push(circle(15_000.0, 12_000.0, 2_000));
        let mut devoir = make_note(DEVOIR, "Devoir surveillé", 5);
        devoir.tags = vec!["physique".into()];
        devoir.page = PageSize {
            width: 21_600,
            height: 29_700,
        };
        let mut vide = make_note(VIDE, "", 6);
        vide.strokes.clear();

        let put = |binder: &str, note: Note, lines: &[&str]| {
            let id = note.id.clone();
            let saved = library
                .create(Path::new(binder), &NoteFile::new(note))
                .unwrap();
            fake.by_id
                .lock()
                .unwrap()
                .insert(id.clone(), lines.iter().map(|l| (*l).to_owned()).collect());
            let _ = saved;
        };
        put(
            "Physique/4e",
            optique,
            &["Cours d'optique", "La lumière se propage en ligne droite"],
        );
        put("Physique", chimie, &["Les métaux conduisent le courant"]);
        put("", devoir, &["Exercice 1"]);
        put("", vide, &[]);
        library.create_binder(Path::new("Exports/PDF")).unwrap();

        Harness {
            _dir: dir,
            root,
            fake,
            write,
            language,
            tuning: ShapeTuning::default(),
        }
    }

    pub fn host(&self) -> Host {
        Host::new(
            Arc::new(Env {
                root: self.root.clone(),
                hidden: Some("Exports".into()),
                language: self.language,
                tuning: self.tuning,
            }),
            self.fake.clone(),
        )
        .with_write(self.write.clone())
    }

    pub fn library(&self) -> Library {
        Library::open(&self.root).unwrap()
    }

    /// The path of a note of the library by its id.
    pub fn path_of(&self, id: &str) -> PathBuf {
        self.library()
            .list()
            .notes
            .into_iter()
            .find(|n| n.id == id)
            .unwrap()
            .path
    }
}

pub struct Session {
    pub client: RunningService<RoleClient, ()>,
    pub harness: Harness,
}

pub async fn session_with(language: Language, options: ServerOptions) -> Session {
    let harness = Harness::new(language);
    let host = harness.host();
    session_on(harness, host, options).await
}

/// A session on `harness` served with `host` (a host with other services than the harness's).
pub async fn session_on(harness: Harness, host: Host, options: ServerOptions) -> Session {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let _ = serve(host, options, server_io).await;
    });
    let client = ().serve(client_io).await.unwrap();
    Session { client, harness }
}

pub async fn session() -> Session {
    session_with(Language::En, ServerOptions::default()).await
}

impl Session {
    /// The `version` `get_note` gives for this note, `"unread"` when it fails.
    pub async fn version(&self, note: &Value) -> String {
        let arguments = json!({ "note": note }).as_object().cloned().unwrap();
        let result = self
            .client
            .call_tool(CallToolRequestParams::new("get_note").with_arguments(arguments))
            .await;
        result
            .ok()
            .and_then(|result| result.structured_content)
            .and_then(|value| value["version"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "unread".to_owned())
    }

    /// Calls a tool. Most tests are not about versions: `create_revision` and `edit_revision`,
    /// when not given `expected_version`, are called like an assistant that has just looked at
    /// the note.
    pub async fn call(&self, tool: &'static str, arguments: Value) -> CallToolResult {
        let mut arguments = arguments.as_object().cloned().unwrap_or_default();
        if matches!(tool, "create_revision" | "edit_revision")
            && !arguments.contains_key("expected_version")
            && let Some(note) = arguments.get("note").cloned()
        {
            let version = self.version(&note).await;
            arguments.insert("expected_version".to_owned(), version.into());
        }
        self.client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
            .unwrap()
    }

    /// The structured content of a successful call.
    pub async fn ok(&self, tool: &'static str, arguments: Value) -> Value {
        let result = self.call(tool, arguments).await;
        assert_ne!(result.is_error, Some(true), "{tool} failed: {result:?}");
        result.structured_content.expect("structured content")
    }

    /// The `{ code, message }` of a failed call.
    pub async fn failure(&self, tool: &'static str, arguments: Value) -> Value {
        let result = self.call(tool, arguments).await;
        assert_eq!(
            result.is_error,
            Some(true),
            "{tool} should fail: {result:?}"
        );
        result.structured_content.unwrap_or_else(|| {
            // Arguments that do not fit the schema are reported by rmcp as plain text.
            json!({ "code": "invalidArguments", "message": result.content[0].as_text().unwrap().text })
        })
    }
}
