//! `LocalState\WacomCloudDb\_`: the LevelDB note store (research §3–§4).
//!
//! Its files are read through the [`TrackedReader`] into memory and opened with
//! `rusty-leveldb` on its in-memory environment: recovery, compaction, the lock and the info
//! log all happen in memory, so nothing is ever written next to Inkspace's data.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rusty_leveldb::env::Env;
use rusty_leveldb::{DB, LdbIterator, MemEnv, Options};
use uuid::Uuid;

use crate::protobuf::{self, Value};
use crate::reader::TrackedReader;
use crate::{DecodeError, ImportError};

/// Largest single database file read into memory.
const MAX_FILE_BYTES: u64 = 512 << 20;
/// Largest total read into memory.
const MAX_TOTAL_BYTES: u64 = 1 << 30;
/// Name of the database inside the in-memory environment.
const MEM_DB_NAME: &str = "WacomCloudDb";

const DOC_PREFIX: &[u8] = b"__doc__";

/// The note store folder inside `LocalState`.
pub(crate) fn db_dir(local_state: &Path) -> PathBuf {
    local_state.join("WacomCloudDb").join("_")
}

/// Identifier of a node: a UUID, or an ASCII name such as the root `__mate__`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NodeId {
    Uuid(Uuid),
    Name(String),
}

impl NodeId {
    pub(crate) const ROOT: &'static str = "__mate__";
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeId::Uuid(uuid) => write!(f, "{uuid}"),
            NodeId::Name(name) => write!(f, "{name}"),
        }
    }
}

/// A NodeRef: `{1: kind, 2: bytes}`; kind 1 names a node, kind 2 carries an inline value
/// (a stroke payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeRef<'a> {
    Node(NodeId),
    Inline(&'a [u8]),
}

pub(crate) fn parse_node_ref(bytes: &[u8]) -> Result<NodeRef<'_>, DecodeError> {
    let fields = protobuf::parse_message(bytes)?;
    let kind = protobuf::varint_field(&fields, 1).ok_or(DecodeError::Missing("NodeRef kind"))?;
    let raw = protobuf::bytes_field(&fields, 2).unwrap_or(&[]);
    match kind {
        1 if raw.len() == 16 => Ok(NodeRef::Node(NodeId::Uuid(
            Uuid::from_slice(raw).map_err(|_| DecodeError::Missing("node id"))?,
        ))),
        1 => match std::str::from_utf8(raw) {
            Ok(name) if !name.is_empty() => Ok(NodeRef::Node(NodeId::Name(name.to_owned()))),
            _ => Err(DecodeError::Invalid {
                what: "node id",
                value: format!("{} bytes", raw.len()),
            }),
        },
        2 => Ok(NodeRef::Inline(raw)),
        other => Err(DecodeError::Invalid {
            what: "NodeRef kind",
            value: other.to_string(),
        }),
    }
}

/// String properties of a node (stored in its parent's child entry), in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Props(Vec<(String, String)>);

impl Props {
    /// Value of `name` (the last one if repeated).
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    #[cfg(test)]
    pub(crate) fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Props(
            pairs
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
        )
    }
}

/// One entry of a node's child list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChildEntry<'a> {
    pub node: NodeRef<'a>,
    pub props: Props,
}

/// Parses a `__doc__` value: the node's ordered child list (research §4).
pub(crate) fn parse_child_list(doc: &[u8]) -> Result<Vec<ChildEntry<'_>>, DecodeError> {
    let mut children = Vec::new();
    for field in protobuf::parse_message(doc)? {
        let (1, Value::Bytes(entry)) = (field.number, field.value) else {
            continue;
        };
        let entry_fields = protobuf::parse_message(entry)?;
        let node_ref =
            protobuf::bytes_field(&entry_fields, 1).ok_or(DecodeError::Missing("child NodeRef"))?;
        let node = parse_node_ref(node_ref)?;
        let mut props = Vec::new();
        for f in &entry_fields {
            let (3, Value::Bytes(prop)) = (f.number, f.value) else {
                continue;
            };
            // A property is `{1: name, 2: value}`; other field-3 payloads (a bare 16-byte
            // value was seen during research) are not properties and are skipped.
            let Ok(prop_fields) = protobuf::parse_message(prop) else {
                continue;
            };
            let Some(name) = protobuf::bytes_field(&prop_fields, 1) else {
                continue;
            };
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            let value = protobuf::bytes_field(&prop_fields, 2).unwrap_or(&[]);
            props.push((name.to_owned(), String::from_utf8_lossy(value).into_owned()));
        }
        children.push(ChildEntry {
            node,
            props: Props(props),
        });
    }
    Ok(children)
}

/// The `__doc__` values of the store, by node id.
#[derive(Debug, Default)]
pub(crate) struct CloudDb {
    pub docs: HashMap<NodeId, Vec<u8>>,
}

impl CloudDb {
    pub(crate) fn doc(&self, id: &NodeId) -> Option<&[u8]> {
        self.docs.get(id).map(Vec::as_slice)
    }
}

/// Files LevelDB needs to open the database. `LOCK` (held by a running Inkspace) and the
/// text logs `LOG`/`LOG.old` are never read.
fn is_database_file(name: &str) -> bool {
    name == "CURRENT"
        || name.starts_with("MANIFEST-")
        || name.ends_with(".ldb")
        || name.ends_with(".sst")
        || name.ends_with(".log")
}

/// Reads the note store into memory and collects every `__doc__` value.
pub(crate) fn load(reader: &TrackedReader, local_state: &Path) -> Result<CloudDb, ImportError> {
    let dir = db_dir(local_state);
    let entries = reader.list(&dir).map_err(|source| ImportError::Io {
        path: dir.clone(),
        source,
    })?;
    let mut files = Vec::new();
    let mut total = 0u64;
    for entry in entries
        .iter()
        .filter(|e| e.is_file && is_database_file(&e.name))
    {
        let bytes = reader
            .read(&entry.path, MAX_FILE_BYTES)
            .map_err(|source| ImportError::Io {
                path: entry.path.clone(),
                source,
            })?;
        total += bytes.len() as u64;
        if total > MAX_TOTAL_BYTES {
            return Err(ImportError::TooLarge { bytes: total });
        }
        files.push((entry.name.clone(), bytes));
    }
    if !files.iter().any(|(name, _)| name == "CURRENT") {
        return Err(ImportError::Database(format!(
            "no CURRENT file in {}",
            dir.display()
        )));
    }
    // rusty-leveldb may panic on some corrupt inputs: contain it and report an error.
    catch_unwind(AssertUnwindSafe(|| open_in_memory(files)))
        .unwrap_or_else(|_| Err(ImportError::Database("corrupt database".into())))
}

fn open_in_memory(files: Vec<(String, Vec<u8>)>) -> Result<CloudDb, ImportError> {
    let env = MemEnv::new();
    let base = Path::new(MEM_DB_NAME);
    for (name, bytes) in files {
        let mut file = env
            .open_writable_file(&base.join(&name))
            .map_err(|e| ImportError::Database(e.to_string()))?;
        file.write_all(&bytes)
            .map_err(|e| ImportError::Database(e.to_string()))?;
    }
    let options = Options {
        env: Rc::new(Box::new(env)),
        create_if_missing: false,
        ..Options::default()
    };
    let mut db = DB::open(base, options).map_err(|e| ImportError::Database(e.to_string()))?;
    let mut iter = db
        .new_iter()
        .map_err(|e| ImportError::Database(e.to_string()))?;
    let mut docs = HashMap::new();
    while let Some((key, value)) = LdbIterator::next(&mut iter) {
        let Some(node_ref) = key.strip_prefix(DOC_PREFIX) else {
            continue;
        };
        match parse_node_ref(node_ref) {
            Ok(NodeRef::Node(id)) => {
                docs.insert(id, value);
            }
            // A key that is not a node reference is not part of the note tree.
            Ok(NodeRef::Inline(_)) | Err(_) => {}
        }
    }
    Ok(CloudDb { docs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_refs() {
        // A key in the layout of research §3, after the `__doc__` prefix (made-up id): the 16
        // bytes are the UUID in textual order.
        let key = [
            0x08, 0x01, 0x12, 0x10, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0x4c, 0xde, 0x8f, 0x01,
            0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        ];
        let NodeRef::Node(NodeId::Uuid(uuid)) = parse_node_ref(&key).unwrap() else {
            panic!("expected a uuid node");
        };
        assert_eq!(uuid.to_string(), "01234567-89ab-4cde-8f01-23456789abcd");
        let mut root = vec![0x08, 0x01, 0x12, 0x08];
        root.extend_from_slice(b"__mate__");
        assert_eq!(
            parse_node_ref(&root).unwrap(),
            NodeRef::Node(NodeId::Name(NodeId::ROOT.into()))
        );
        assert_eq!(
            parse_node_ref(&[0x08, 0x02, 0x12, 0x01, 0xaa]).unwrap(),
            NodeRef::Inline(&[0xaa])
        );
        assert!(parse_node_ref(&[0x08, 0x07]).is_err());
        assert!(parse_node_ref(&[0x12, 0x00]).is_err());
        assert!(parse_node_ref(&[0x08, 0x01, 0x12, 0x01, 0xff]).is_err());
    }

    #[test]
    fn child_list_with_properties() {
        // ChildEntry { 1: NodeRef(kind 2, [0xaa]), 3: {1:"k", 2:"v"}, 3: 16 bare bytes }
        let mut entry = vec![0x0a, 0x05, 0x08, 0x02, 0x12, 0x01, 0xaa];
        entry.extend_from_slice(&[0x1a, 0x06, 0x0a, 0x01, b'k', 0x12, 0x01, b'v']);
        entry.extend_from_slice(&[0x1a, 0x03, 0xff, 0xff, 0xff]);
        let mut doc = vec![0x0a, entry.len() as u8];
        doc.extend_from_slice(&entry);
        doc.extend_from_slice(&[0x18, 0x00, 0x20, 0x00, 0x48, 0x01]);
        let children = parse_child_list(&doc).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].node, NodeRef::Inline(&[0xaa]));
        assert_eq!(children[0].props.get("k"), Some("v"));
        assert_eq!(children[0].props.get("missing"), None);
        assert!(parse_child_list(&[0x0a, 0x02, 0x08]).is_err());
        assert_eq!(parse_child_list(&[]).unwrap(), vec![]);
    }

    #[test]
    fn database_files() {
        for name in [
            "CURRENT",
            "MANIFEST-000041",
            "000031.ldb",
            "000042.log",
            "1.sst",
        ] {
            assert!(is_database_file(name), "{name}");
        }
        for name in ["LOCK", "LOG", "LOG.old", "notes.png"] {
            assert!(!is_database_file(name), "{name}");
        }
    }

    #[test]
    fn missing_or_corrupt_store_fails_cleanly() {
        let local_state = tempfile::tempdir().unwrap();
        let reader = TrackedReader::new();
        assert!(matches!(
            load(&reader, local_state.path()),
            Err(ImportError::Io { .. })
        ));
        let dir = db_dir(local_state.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("000001.log"), b"garbage").unwrap();
        assert!(matches!(
            load(&reader, local_state.path()),
            Err(ImportError::Database(_))
        ));
        std::fs::write(dir.join("CURRENT"), b"MANIFEST-000002\n").unwrap();
        std::fs::write(dir.join("MANIFEST-000002"), b"not a manifest at all").unwrap();
        assert!(matches!(
            load(&reader, local_state.path()),
            Err(ImportError::Database(_))
        ));
    }
}
