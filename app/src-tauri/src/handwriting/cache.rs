//! The per-PC cache of the recognised text: `%LOCALAPPDATA%\Ardoise\handwriting\`.
//!
//! - One entry per note, strokes and language: `<note id>-<strokes hash>-<language>.json`. The
//!   hash covers the strokes that are recognised (visible layers, see
//!   `ardoise_hwr::ink::recognizable_strokes`) and [`PIPELINE`], so a note edited here or on
//!   another PC gets a new entry, a title change does not, and changing how notes are
//!   recognised makes every entry again. Two files with the same id (a Nextcloud conflict copy)
//!   have their own entries.
//! - `files.json`: for each note file seen, its stamp (size and modification time), id and
//!   strokes hash, so that an unchanged file is not read again to know whether its entry is
//!   still the right one.
//!
//! The notes themselves are never written for the recognition (the `text.json` member of the
//! format stays unused): each PC keeps its own results, so two PCs never compete to rewrite the
//! same note through Nextcloud. Everything here is a convenience: an entry that cannot be read
//! or written is simply made again.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ardoise_core::fsutil::write_atomic;
use ardoise_core::library::FileStamp;
use ardoise_core::note::Note;
use ardoise_hwr::ink::recognizable_strokes;
use ardoise_hwr::{Bounds, Language, Line, Recognition, Word};
use ardoise_ink::Stroke;
use serde::{Deserialize, Serialize};

/// Version of the way notes are recognised (choice of strokes, scale handed to the engine,
/// lines). Changing it makes every entry again.
pub const PIPELINE: u32 = 1;

/// Version of the files of the cache.
const FORMAT: u32 = 1;

/// Name of the record of the files seen.
pub const FILES_JSON: &str = "files.json";

/// Entries of notes no longer in the library are removed once this old.
const MAX_AGE: Duration = Duration::from_secs(90 * 24 * 3600);

/// Hash of the strokes that are recognised (FNV-1a, 64 bits, as 16 hex digits).
pub fn strokes_hash(strokes: &[Stroke]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    feed(&PIPELINE.to_le_bytes());
    feed(&(strokes.len() as u64).to_le_bytes());
    for stroke in strokes {
        feed(&(stroke.points.len() as u64).to_le_bytes());
        for point in &stroke.points {
            feed(&point.x.to_bits().to_le_bytes());
            feed(&point.y.to_bits().to_le_bytes());
            feed(&point.pressure.to_bits().to_le_bytes());
        }
    }
    format!("{hash:016x}")
}

/// What an entry is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntryKey {
    pub id: String,
    pub strokes: String,
    pub language: Language,
}

impl EntryKey {
    /// `<id>-<strokes>-<language>.json`, the id reduced to letters, digits and `-` (note ids are
    /// UUIDs; anything else must not reach outside the folder).
    pub fn file_name(&self) -> String {
        format!(
            "{}-{}-{}.json",
            safe_id(&self.id),
            self.strokes,
            self.language.code()
        )
    }
}

/// A note id as it appears in file names.
fn safe_id(id: &str) -> String {
    id.chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A transcription corrected by the assistant (`set_note_text`, MCP): the recogniser's text of a
/// note is replaced by it in the search until the strokes change. It lives next to the entries,
/// as `<note id>-<strokes hash>-fix.json`: the same pruning drops it with the older versions of
/// the note, and a note edited since never finds it (its strokes hash is another).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correction {
    format: u32,
    pub id: String,
    pub strokes: String,
    /// The language the text was given in.
    pub language: Language,
    pub text: String,
}

impl Correction {
    pub fn new(id: &str, strokes: &str, language: Language, text: &str) -> Correction {
        Correction {
            format: FORMAT,
            id: id.to_owned(),
            strokes: strokes.to_owned(),
            language,
            text: text.to_owned(),
        }
    }

    /// The corrected text as a recognition without positions, which the search reads like the
    /// recogniser's.
    pub fn recognition(&self) -> Recognition {
        let lines = self
            .text
            .lines()
            .map(|line| {
                Line::new(
                    line.split_whitespace()
                        .map(|word| Word {
                            text: word.to_owned(),
                            alternatives: Vec::new(),
                            bounds: Bounds::default(),
                        })
                        .collect(),
                )
            })
            .filter(|line: &Line| !line.words.is_empty())
            .collect();
        Recognition {
            language: self.language,
            recognizer: CORRECTION_RECOGNIZER.to_owned(),
            lines,
        }
    }
}

/// The `recognizer` of a recognition made from a correction.
pub const CORRECTION_RECOGNIZER: &str = "correction";

/// Suffix of the file name of a correction (the entries end with their language).
const CORRECTION_SUFFIX: &str = "-fix.json";

/// What `files.json` remembers of a note file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub stamp: FileStamp,
    pub id: String,
    pub strokes: String,
}

#[derive(Serialize, Deserialize)]
struct FilesJson {
    format: u32,
    files: HashMap<PathBuf, FileRecord>,
}

#[derive(Deserialize)]
struct EntryJson {
    format: u32,
    id: String,
    strokes: String,
    language: Language,
    recognition: Recognition,
}

/// [`EntryJson`] as it is written (borrowed: the recognition is not copied).
#[derive(Serialize)]
struct EntryOut<'a> {
    format: u32,
    id: &'a str,
    strokes: &'a str,
    language: Language,
    recognition: &'a Recognition,
}

/// What the cache of this PC holds in memory. It never touches the disk itself: its owner keeps
/// it behind a lock that is never held across a disk access ([`CacheDir`] does those).
#[derive(Debug, Default)]
pub struct HandwritingCache {
    files: HashMap<PathBuf, FileRecord>,
    files_changed: bool,
    entries: HashMap<EntryKey, Arc<Recognition>>,
}

impl HandwritingCache {
    /// Takes the records read from `files.json` ([`CacheDir::read_files`]); those already set
    /// in this session win.
    pub fn load_files(&mut self, files: HashMap<PathBuf, FileRecord>) {
        for (path, record) in files {
            self.files.entry(path).or_insert(record);
        }
    }

    /// What is known of the file at `path` when its stamp is still `stamp`.
    pub fn record(&self, path: &Path, stamp: &FileStamp) -> Option<&FileRecord> {
        self.files.get(path).filter(|r| r.stamp == *stamp)
    }

    pub fn set_record(&mut self, path: PathBuf, record: FileRecord) {
        if self.files.get(&path) != Some(&record) {
            self.files.insert(path, record);
            self.files_changed = true;
        }
    }

    /// The recognition of `key` if it is in memory.
    pub fn loaded(&self, key: &EntryKey) -> Option<Arc<Recognition>> {
        self.entries.get(key).cloned()
    }

    /// Keeps `recognition` in memory as the entry of `key`.
    pub fn insert(&mut self, key: EntryKey, recognition: Arc<Recognition>) -> Arc<Recognition> {
        self.entries.insert(key, recognition.clone());
        recognition
    }

    /// `files.json` to write when it changed since it was last taken (it then counts as
    /// written: see [`files_unwritten`](Self::files_unwritten)).
    pub fn files_to_write(&mut self) -> Option<Vec<u8>> {
        if !self.files_changed {
            return None;
        }
        let bytes = serde_json::to_vec(&FilesJson {
            format: FORMAT,
            files: self.files.clone(),
        })
        .ok()?;
        self.files_changed = false;
        Some(bytes)
    }

    /// The `files.json` taken could not be written: it will be written again.
    pub fn files_unwritten(&mut self) {
        self.files_changed = true;
    }

    /// Forgets the files below `root` that are not in `present` (deleted, moved, renamed).
    pub fn retain_files(&mut self, root: &Path, present: &HashSet<PathBuf>) {
        let before = self.files.len();
        self.files
            .retain(|path, _| !path.starts_with(root) || present.contains(path));
        if self.files.len() != before {
            self.files_changed = true;
        }
    }

    /// Forgets the entries of older versions of the notes in `in_use` (id → strokes hashes of
    /// the files that have it); [`CacheDir::prune`] removes their files.
    pub fn retain_entries(&mut self, in_use: &HashMap<String, HashSet<String>>) {
        self.entries.retain(|key, _| {
            in_use
                .get(&key.id)
                .is_none_or(|hashes| hashes.contains(&key.strokes))
        });
    }
}

/// The folder of the cache: every disk access of the cache, made without holding any lock.
/// Everything is best effort (an entry that cannot be read or written is made again).
#[derive(Debug, Clone)]
pub struct CacheDir {
    dir: PathBuf,
}

impl CacheDir {
    /// The cache in `dir` (created when first written).
    pub fn new(dir: impl Into<PathBuf>) -> CacheDir {
        CacheDir { dir: dir.into() }
    }

    /// The records of `files.json`; none when it is missing, damaged or of another format.
    pub fn read_files(&self) -> HashMap<PathBuf, FileRecord> {
        fs::read(self.dir.join(FILES_JSON))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FilesJson>(&bytes).ok())
            .filter(|json| json.format == FORMAT)
            .map(|json| json.files)
            .unwrap_or_default()
    }

    /// Writes `files.json` ([`HandwritingCache::files_to_write`]); false when it failed.
    pub fn write_files(&self, bytes: &[u8]) -> bool {
        fs::create_dir_all(&self.dir).is_ok()
            && write_atomic(&self.dir.join(FILES_JSON), bytes).is_ok()
    }

    /// The recognition of `key` read from its file, when it is there and is the right one.
    pub fn read_entry(&self, key: &EntryKey) -> Option<Recognition> {
        let bytes = fs::read(self.dir.join(key.file_name())).ok()?;
        let json: EntryJson = serde_json::from_slice(&bytes).ok()?;
        let matches = json.format == FORMAT
            && json.id == key.id
            && json.strokes == key.strokes
            && json.language == key.language
            && json.recognition.language == key.language;
        matches.then_some(json.recognition)
    }

    /// Writes the entry of `key`.
    pub fn write_entry(&self, key: &EntryKey, recognition: &Recognition) {
        let json = EntryOut {
            format: FORMAT,
            id: &key.id,
            strokes: &key.strokes,
            language: key.language,
            recognition,
        };
        if let Ok(bytes) = serde_json::to_vec(&json)
            && fs::create_dir_all(&self.dir).is_ok()
        {
            let _ = write_atomic(&self.dir.join(key.file_name()), &bytes);
        }
    }

    /// The corrected transcription of `note` (`set_note_text`), if its ink is still the one it
    /// was written for: the key is the hash of the strokes that are recognised.
    pub fn correction_for(&self, note: &Note) -> Option<Correction> {
        self.read_correction(&note.id, &strokes_hash(&recognizable_strokes(note)))
    }

    /// Stores `text` as the transcription of `note`, bound to its current ink, in `language`;
    /// an empty `text` removes it.
    pub fn set_correction_for(
        &self,
        note: &Note,
        language: Language,
        text: &str,
    ) -> std::io::Result<()> {
        let strokes = strokes_hash(&recognizable_strokes(note));
        if text.trim().is_empty() {
            return self.remove_correction(&note.id, &strokes);
        }
        self.write_correction(&Correction::new(&note.id, &strokes, language, text))
    }

    /// Name of the file holding the correction of the note `id` with these `strokes`.
    fn correction_file(&self, id: &str, strokes: &str) -> PathBuf {
        self.dir
            .join(format!("{}-{strokes}{CORRECTION_SUFFIX}", safe_id(id)))
    }

    /// The correction of the note `id` with these `strokes`, when there is one.
    pub fn read_correction(&self, id: &str, strokes: &str) -> Option<Correction> {
        let bytes = fs::read(self.correction_file(id, strokes)).ok()?;
        let json: Correction = serde_json::from_slice(&bytes).ok()?;
        (json.format == FORMAT && json.id == id && json.strokes == strokes).then_some(json)
    }

    /// Stores `correction`, replacing the one of the same note and strokes.
    pub fn write_correction(&self, correction: &Correction) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(correction).map_err(std::io::Error::other)?;
        fs::create_dir_all(&self.dir)?;
        write_atomic(
            &self.correction_file(&correction.id, &correction.strokes),
            &bytes,
        )
        .map(|_| ())
    }

    /// Forgets the correction of the note `id` with these `strokes` (none is not an error).
    pub fn remove_correction(&self, id: &str, strokes: &str) -> std::io::Result<()> {
        match fs::remove_file(self.correction_file(id, strokes)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Every correction in the folder, by note id and strokes hash. Read at each search: the
    /// MCP server may be another process than the app, and there are few of them.
    pub fn corrections(&self) -> HashMap<(String, String), Correction> {
        let Ok(listing) = fs::read_dir(&self.dir) else {
            return HashMap::new();
        };
        listing
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(CORRECTION_SUFFIX))
            })
            .filter_map(|entry| {
                let json: Correction =
                    serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
                (json.format == FORMAT).then(|| ((json.id.clone(), json.strokes.clone()), json))
            })
            .collect()
    }

    /// Removes the files of the entries made useless: for a note id in `in_use` (id → strokes
    /// hashes of the files that have it), the entries of other strokes (older versions of the
    /// note); for any other id, the entries older than [`MAX_AGE`] (notes deleted, or of
    /// another library). A correction of other strokes than `in_use` gives is asked about
    /// first, right before it would go: it stays when `is_current` says it is for the ink the
    /// note has now (an edit made since `in_use` was known, corrected meanwhile).
    pub fn prune(
        &self,
        in_use: &HashMap<String, HashSet<String>>,
        is_current: &dyn Fn(&Correction) -> bool,
    ) {
        self.prune_at(in_use, is_current, SystemTime::now());
    }

    fn prune_at(
        &self,
        in_use: &HashMap<String, HashSet<String>>,
        is_current: &dyn Fn(&Correction) -> bool,
        now: SystemTime,
    ) {
        let Ok(listing) = fs::read_dir(&self.dir) else {
            return;
        };
        // File names of the entries in use: `<id>-<strokes>-`.
        let mut keep_prefixes = HashSet::new();
        let mut used_ids = HashSet::new();
        for (id, hashes) in in_use {
            let id = safe_id(id);
            used_ids.insert(format!("{id}-"));
            for hash in hashes {
                keep_prefixes.insert(format!("{id}-{hash}-"));
            }
        }
        for entry in listing.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name == FILES_JSON || !name.ends_with(".json") {
                continue;
            }
            let Some(id_prefix) = entry_id_prefix(name) else {
                continue;
            };
            let remove = if used_ids.contains(id_prefix) {
                !keep_prefixes
                    .iter()
                    .any(|keep| name.starts_with(keep.as_str()))
                    && !(name.ends_with(CORRECTION_SUFFIX)
                        && fs::read(entry.path())
                            .ok()
                            .and_then(|bytes| serde_json::from_slice::<Correction>(&bytes).ok())
                            .is_some_and(|correction| is_current(&correction)))
            } else {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > MAX_AGE)
            };
            if remove {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// `<id>-` of an entry's file name `<id>-<16 hex digits>-<language>.json`.
fn entry_id_prefix(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(".json")?;
    let (rest, _language) = stem.rsplit_once('-')?;
    let (_, hash) = rest.rsplit_once('-')?;
    if hash.len() != 16 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(&name[..rest.len() - hash.len()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_hwr::{Bounds, Line, Word};
    use ardoise_ink::{Point, Rgba, Tool};

    pub(crate) fn recognition(text: &str, language: Language) -> Recognition {
        Recognition {
            language,
            recognizer: "test".into(),
            lines: vec![Line::new(
                text.split(' ')
                    .map(|w| Word {
                        text: w.into(),
                        alternatives: Vec::new(),
                        bounds: Bounds::default(),
                    })
                    .collect(),
            )],
        }
    }

    fn stroke(x: f32) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points: vec![
                Point {
                    x,
                    y: 0.0,
                    pressure: 0.5,
                    t: 0,
                },
                Point {
                    x: x + 10.0,
                    y: 5.0,
                    pressure: 0.6,
                    t: 5,
                },
            ],
        }
    }

    fn key(id: &str, strokes: &str, language: Language) -> EntryKey {
        EntryKey {
            id: id.into(),
            strokes: strokes.into(),
            language,
        }
    }

    fn stamp(len: u64) -> FileStamp {
        FileStamp {
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
            len,
        }
    }

    #[test]
    fn the_hash_follows_the_strokes_only() {
        let a = vec![stroke(0.0), stroke(100.0)];
        let hash = strokes_hash(&a);
        assert_eq!(hash.len(), 16);
        assert_eq!(hash, strokes_hash(&a.clone()), "stable");
        // Times, colours and tools do not change what is read.
        let mut retimed = a.clone();
        retimed[0].points[1].t = 999;
        retimed[1].color = Rgba::rgb(200, 0, 0);
        assert_eq!(strokes_hash(&retimed), hash);
        // A moved point, a stroke more or less, a stroke split in two: another hash.
        let mut moved = a.clone();
        moved[1].points[0].x += 1.0;
        assert_ne!(strokes_hash(&moved), hash);
        assert_ne!(strokes_hash(&a[..1]), hash);
        assert_ne!(strokes_hash(&[]), hash);
    }

    #[test]
    fn entry_names_are_safe_and_tell_their_language() {
        let name = key("5e1f0a3c-d3e0", "0123456789abcdef", Language::En).file_name();
        assert_eq!(name, "5e1f0a3c-d3e0-0123456789abcdef-en.json");
        let odd = key("..\\x/y", "0123456789abcdef", Language::Fr).file_name();
        assert_eq!(odd, "___x_y-0123456789abcdef-fr.json");
        assert!(!odd.contains('/') && !odd.contains('\\'));
        assert_eq!(entry_id_prefix(&name), Some("5e1f0a3c-d3e0-"));
        assert_eq!(entry_id_prefix("files.json"), None);
        assert_eq!(entry_id_prefix("a-nothex-fr.json"), None);
    }

    #[test]
    fn entries_survive_a_restart_and_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let disk = CacheDir::new(dir.path());
        let k = key("note-1", "0123456789abcdef", Language::Fr);
        assert!(disk.read_entry(&k).is_none());
        disk.write_entry(&k, &recognition("le chat", Language::Fr));
        assert_eq!(disk.read_entry(&k).unwrap().text(), "le chat");
        // Another language or other strokes are other entries.
        assert!(
            disk.read_entry(&key("note-1", "0123456789abcdef", Language::En))
                .is_none()
        );
        assert!(
            disk.read_entry(&key("note-1", "fedcba9876543210", Language::Fr))
                .is_none()
        );
        // A damaged entry is a missing one.
        fs::write(dir.path().join(k.file_name()), b"{ not json").unwrap();
        assert!(disk.read_entry(&k).is_none());
    }

    #[test]
    fn entries_in_memory_are_kept_and_forgotten() {
        let mut cache = HandwritingCache::default();
        let current = key("note-1", "1111111111111111", Language::Fr);
        let old = key("note-1", "2222222222222222", Language::Fr);
        let other = key("note-9", "4444444444444444", Language::Fr);
        assert!(cache.loaded(&current).is_none());
        for k in [&current, &old, &other] {
            cache.insert(k.clone(), Arc::new(recognition("x", k.language)));
        }
        assert_eq!(cache.loaded(&current).unwrap().text(), "x");
        cache.retain_entries(&HashMap::from([(
            "note-1".to_owned(),
            HashSet::from(["1111111111111111".to_owned()]),
        )]));
        assert!(cache.loaded(&current).is_some());
        assert!(cache.loaded(&old).is_none(), "an older version goes");
        assert!(cache.loaded(&other).is_some(), "other notes stay");
    }

    #[test]
    fn files_are_remembered_with_their_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Bibliothèque");
        let path = root.join("2026-10-25 Cours.ardoise");
        let gone = root.join("Perso").join("Ancienne.ardoise");
        let elsewhere = dir.path().join("Autre").join("Note.ardoise");
        let record = FileRecord {
            stamp: stamp(10),
            id: "n".into(),
            strokes: "0123456789abcdef".into(),
        };
        let disk = CacheDir::new(dir.path().join("cache"));
        assert!(disk.read_files().is_empty());
        let mut cache = HandwritingCache::default();
        assert!(cache.files_to_write().is_none(), "nothing changed");
        cache.set_record(path.clone(), record.clone());
        cache.set_record(gone.clone(), record.clone());
        cache.set_record(elsewhere.clone(), record.clone());
        let bytes = cache.files_to_write().unwrap();
        assert!(cache.files_to_write().is_none(), "taken once");
        assert!(disk.write_files(&bytes));

        let mut reopened = HandwritingCache::default();
        reopened.load_files(disk.read_files());
        assert_eq!(reopened.record(&path, &stamp(10)), Some(&record));
        // The file changed since: unknown.
        assert_eq!(reopened.record(&path, &stamp(11)), None);
        // Files of the library no longer there are forgotten, those of another folder stay.
        reopened.retain_files(&root, &HashSet::from([path.clone()]));
        assert!(disk.write_files(&reopened.files_to_write().unwrap()));
        let mut last = HandwritingCache::default();
        last.load_files(disk.read_files());
        assert!(last.record(&path, &stamp(10)).is_some());
        assert!(last.record(&gone, &stamp(10)).is_none());
        assert!(last.record(&elsewhere, &stamp(10)).is_some());

        // A record set before `files.json` is read wins over the older one.
        let newer = FileRecord {
            stamp: stamp(12),
            ..record.clone()
        };
        let mut early = HandwritingCache::default();
        early.set_record(path.clone(), newer.clone());
        early.load_files(disk.read_files());
        assert_eq!(early.record(&path, &stamp(12)), Some(&newer));
        // A write that failed is tried again.
        early.files_to_write().unwrap();
        early.files_unwritten();
        assert!(early.files_to_write().is_some());
        // Not a folder: nothing written.
        let file = dir.path().join("a-file");
        fs::write(&file, b"x").unwrap();
        assert!(!CacheDir::new(&file).write_files(b"{}"));
    }

    #[test]
    fn old_versions_of_a_note_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let disk = CacheDir::new(dir.path());
        let current_fr = key("note-1", "1111111111111111", Language::Fr);
        let current_en = key("note-1", "1111111111111111", Language::En);
        let old = key("note-1", "2222222222222222", Language::Fr);
        let conflict = key("note-1", "3333333333333333", Language::Fr);
        let other_library = key("note-9", "4444444444444444", Language::Fr);
        for k in [&current_fr, &current_en, &old, &conflict, &other_library] {
            disk.write_entry(k, &recognition("x", k.language));
        }
        let in_use = HashMap::from([(
            "note-1".to_owned(),
            HashSet::from(["1111111111111111".to_owned(), "3333333333333333".to_owned()]),
        )]);
        disk.prune(&in_use, &|_| false);
        let exists = |k: &EntryKey| dir.path().join(k.file_name()).exists();
        assert!(
            exists(&current_fr) && exists(&current_en),
            "both languages kept"
        );
        assert!(exists(&conflict), "a conflict copy keeps its entry");
        assert!(!exists(&old), "the older version goes");
        assert!(exists(&other_library), "recent entries of other notes stay");
        // Months later, entries of notes no longer seen go.
        disk.prune_at(
            &in_use,
            &|_| false,
            SystemTime::now() + MAX_AGE + Duration::from_secs(60),
        );
        assert!(!exists(&other_library));
        assert!(exists(&current_fr));
    }

    #[test]
    fn corrections_are_bound_to_the_ink_and_old_caches_stay_readable() {
        let dir = tempfile::tempdir().unwrap();
        let disk = CacheDir::new(dir.path());
        let note = |x: f32| {
            let mut note = crate::library::blank_note("Note", "Calque 1", 1_000);
            note.strokes = vec![stroke(x)];
            note
        };
        let (written, edited) = (note(0.0), note(50.0));
        assert!(disk.correction_for(&written).is_none());

        // A cache from before the corrections: entries only, read as always.
        let entry = key(&written.id, &strokes_hash(&written.strokes), Language::Fr);
        disk.write_entry(&entry, &recognition("le chat", Language::Fr));
        assert_eq!(disk.read_entry(&entry).unwrap().text(), "le chat");
        assert!(disk.corrections().is_empty());

        disk.set_correction_for(&written, Language::En, "the cat\nsleeps")
            .unwrap();
        let found = disk.correction_for(&written).unwrap();
        assert_eq!(
            (found.text.as_str(), found.language),
            ("the cat\nsleeps", Language::En)
        );
        assert_eq!(disk.corrections().len(), 1);
        // The recognition is a text without positions that the search reads like the engine's.
        let read = found.recognition();
        assert_eq!(read.text(), "the cat\nsleeps");
        assert_eq!(read.recognizer, CORRECTION_RECOGNIZER);
        // The entry of the recogniser is still there, untouched.
        assert_eq!(disk.read_entry(&entry).unwrap().text(), "le chat");
        // Other ink, no correction.
        assert!(disk.correction_for(&edited).is_none());

        // Pruned with the older versions of the note; kept while its ink is the one in use.
        let current = strokes_hash(&written.strokes);
        let in_use =
            |hash: &str| HashMap::from([(written.id.clone(), HashSet::from([hash.to_owned()]))]);
        disk.prune(&in_use(&current), &|_| false);
        assert!(disk.correction_for(&written).is_some());
        // A correction of other strokes than those in use is for an edit made since they were
        // known when the note, read again, has them: it stays (the entry of the old ink goes).
        let asked = std::sync::Mutex::new(Vec::new());
        disk.prune(&in_use(&strokes_hash(&edited.strokes)), &|correction| {
            asked.lock().unwrap().push(correction.strokes.clone());
            true
        });
        assert_eq!(*asked.lock().unwrap(), std::slice::from_ref(&current));
        assert!(disk.correction_for(&written).is_some());
        assert!(!dir.path().join(entry.file_name()).exists());
        // Otherwise it goes with the older versions.
        disk.prune(&in_use(&strokes_hash(&edited.strokes)), &|_| false);
        assert!(disk.correction_for(&written).is_none());

        // A damaged file is no correction, and an empty text removes one.
        disk.set_correction_for(&written, Language::Fr, "x")
            .unwrap();
        fs::write(
            dir.path()
                .join(format!("{}-{current}-fix.json", written.id)),
            b"{ not json",
        )
        .unwrap();
        assert!(disk.correction_for(&written).is_none());
        assert!(disk.corrections().is_empty());
        disk.set_correction_for(&written, Language::Fr, "x")
            .unwrap();
        disk.set_correction_for(&written, Language::Fr, "").unwrap();
        assert!(disk.correction_for(&written).is_none());
        // Removing what is not there is not an error.
        disk.set_correction_for(&written, Language::Fr, "").unwrap();
    }
}
