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
use ardoise_hwr::{Language, Recognition};
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

#[derive(Serialize, Deserialize)]
struct EntryJson {
    format: u32,
    id: String,
    strokes: String,
    language: Language,
    recognition: Recognition,
}

/// The cache of this PC (not shared between threads by itself: its owner locks it).
#[derive(Debug)]
pub struct HandwritingCache {
    dir: PathBuf,
    files: HashMap<PathBuf, FileRecord>,
    files_changed: bool,
    entries: HashMap<EntryKey, Arc<Recognition>>,
}

impl HandwritingCache {
    /// The cache in `dir` (created when first written); `files.json` is read if it is there.
    pub fn open(dir: impl Into<PathBuf>) -> HandwritingCache {
        let dir = dir.into();
        let files = fs::read(dir.join(FILES_JSON))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FilesJson>(&bytes).ok())
            .filter(|json| json.format == FORMAT)
            .map(|json| json.files)
            .unwrap_or_default();
        HandwritingCache {
            dir,
            files,
            files_changed: false,
            entries: HashMap::new(),
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

    /// The recognition of `key`, from memory or from its file.
    pub fn get(&mut self, key: &EntryKey) -> Option<Arc<Recognition>> {
        if let Some(found) = self.entries.get(key) {
            return Some(found.clone());
        }
        let bytes = fs::read(self.dir.join(key.file_name())).ok()?;
        let json: EntryJson = serde_json::from_slice(&bytes).ok()?;
        let matches = json.format == FORMAT
            && json.id == key.id
            && json.strokes == key.strokes
            && json.language == key.language
            && json.recognition.language == key.language;
        if !matches {
            return None;
        }
        let recognition = Arc::new(json.recognition);
        self.entries.insert(key.clone(), recognition.clone());
        Some(recognition)
    }

    /// The recognition of `key` if it is already in memory (no disk access).
    pub fn loaded(&self, key: &EntryKey) -> Option<Arc<Recognition>> {
        self.entries.get(key).cloned()
    }

    /// Keeps `recognition` as the entry of `key` (in memory, and on disk when possible).
    pub fn put(&mut self, key: EntryKey, recognition: Recognition) -> Arc<Recognition> {
        let json = EntryJson {
            format: FORMAT,
            id: key.id.clone(),
            strokes: key.strokes.clone(),
            language: key.language,
            recognition,
        };
        if let Ok(bytes) = serde_json::to_vec(&json)
            && fs::create_dir_all(&self.dir).is_ok()
        {
            let _ = write_atomic(&self.dir.join(key.file_name()), &bytes);
        }
        let recognition = Arc::new(json.recognition);
        self.entries.insert(key, recognition.clone());
        recognition
    }

    /// Writes `files.json` if it changed (best effort).
    pub fn flush(&mut self) {
        if !self.files_changed {
            return;
        }
        let json = FilesJson {
            format: FORMAT,
            files: self.files.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec(&json)
            && fs::create_dir_all(&self.dir).is_ok()
            && write_atomic(&self.dir.join(FILES_JSON), &bytes).is_ok()
        {
            self.files_changed = false;
        }
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

    /// Removes the entries made useless: for a note id in `in_use` (id → strokes hashes of the
    /// files that have it), the entries of other strokes (older versions of the note); for any
    /// other id, the entries older than [`MAX_AGE`] (notes deleted, or of another library).
    pub fn prune(&mut self, in_use: &HashMap<String, HashSet<String>>) {
        self.prune_at(in_use, SystemTime::now());
    }

    fn prune_at(&mut self, in_use: &HashMap<String, HashSet<String>>, now: SystemTime) {
        self.entries.retain(|key, _| {
            in_use
                .get(&key.id)
                .is_none_or(|hashes| hashes.contains(&key.strokes))
        });
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
        let k = key("note-1", "0123456789abcdef", Language::Fr);
        let mut cache = HandwritingCache::open(dir.path());
        assert!(cache.get(&k).is_none());
        cache.put(k.clone(), recognition("le chat", Language::Fr));
        assert_eq!(cache.get(&k).unwrap().text(), "le chat");

        let mut reopened = HandwritingCache::open(dir.path());
        assert!(reopened.loaded(&k).is_none(), "nothing in memory yet");
        assert_eq!(reopened.get(&k).unwrap().text(), "le chat");
        // Another language or other strokes are other entries.
        assert!(
            reopened
                .get(&key("note-1", "0123456789abcdef", Language::En))
                .is_none()
        );
        assert!(
            reopened
                .get(&key("note-1", "fedcba9876543210", Language::Fr))
                .is_none()
        );
        // A damaged entry is a missing one.
        fs::write(dir.path().join(k.file_name()), b"{ not json").unwrap();
        assert!(HandwritingCache::open(dir.path()).get(&k).is_none());
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
        let mut cache = HandwritingCache::open(dir.path().join("cache"));
        cache.set_record(path.clone(), record.clone());
        cache.set_record(gone.clone(), record.clone());
        cache.set_record(elsewhere.clone(), record.clone());
        cache.flush();

        let mut reopened = HandwritingCache::open(dir.path().join("cache"));
        assert_eq!(reopened.record(&path, &stamp(10)), Some(&record));
        // The file changed since: unknown.
        assert_eq!(reopened.record(&path, &stamp(11)), None);
        // Files of the library no longer there are forgotten, those of another folder stay.
        reopened.retain_files(&root, &HashSet::from([path.clone()]));
        reopened.flush();
        let last = HandwritingCache::open(dir.path().join("cache"));
        assert!(last.record(&path, &stamp(10)).is_some());
        assert!(last.record(&gone, &stamp(10)).is_none());
        assert!(last.record(&elsewhere, &stamp(10)).is_some());
    }

    #[test]
    fn old_versions_of_a_note_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = HandwritingCache::open(dir.path());
        let current_fr = key("note-1", "1111111111111111", Language::Fr);
        let current_en = key("note-1", "1111111111111111", Language::En);
        let old = key("note-1", "2222222222222222", Language::Fr);
        let conflict = key("note-1", "3333333333333333", Language::Fr);
        let other_library = key("note-9", "4444444444444444", Language::Fr);
        for k in [&current_fr, &current_en, &old, &conflict, &other_library] {
            cache.put(k.clone(), recognition("x", k.language));
        }
        let in_use = HashMap::from([(
            "note-1".to_owned(),
            HashSet::from(["1111111111111111".to_owned(), "3333333333333333".to_owned()]),
        )]);
        cache.prune(&in_use);
        let exists = |k: &EntryKey| dir.path().join(k.file_name()).exists();
        assert!(
            exists(&current_fr) && exists(&current_en),
            "both languages kept"
        );
        assert!(exists(&conflict), "a conflict copy keeps its entry");
        assert!(!exists(&old), "the older version goes");
        assert!(cache.loaded(&old).is_none());
        assert!(exists(&other_library), "recent entries of other notes stay");
        // Months later, entries of notes no longer seen go.
        cache.prune_at(
            &in_use,
            SystemTime::now() + MAX_AGE + Duration::from_secs(60),
        );
        assert!(!exists(&other_library));
        assert!(exists(&current_fr));
    }
}
