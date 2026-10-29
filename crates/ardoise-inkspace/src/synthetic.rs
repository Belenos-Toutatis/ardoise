//! Made-up Inkspace data in the layouts of research §3–§5: a `LocalState` folder whose
//! `WacomCloudDb\_` note store holds invented notes, and a `db\` settings store whose values
//! carry a marker (to prove the importer never reads them). Everything is generated here, so it
//! can be committed and published.
//!
//! Only with the `synthetic` feature (tests, and `ardoise.exe --create-demo-inkspace`). This is
//! the one part of the crate that writes files, and only into the folder it is given: the
//! importer itself never writes anything.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rusty_leveldb::env::Env;
use rusty_leveldb::{DB, MemEnv, Options, Status};
use uuid::Uuid;

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Minimal protobuf writer.
#[derive(Default)]
pub struct Pb(pub Vec<u8>);

impl Pb {
    fn raw_varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(byte);
                return;
            }
            self.0.push(byte | 0x80);
        }
    }
    pub fn varint(&mut self, field: u32, v: u64) -> &mut Self {
        self.raw_varint(u64::from(field) << 3);
        self.raw_varint(v);
        self
    }
    pub fn bytes(&mut self, field: u32, b: &[u8]) -> &mut Self {
        self.raw_varint((u64::from(field) << 3) | 2);
        self.raw_varint(b.len() as u64);
        self.0.extend_from_slice(b);
        self
    }
    pub fn fixed32(&mut self, field: u32, v: u32) -> &mut Self {
        self.raw_varint((u64::from(field) << 3) | 5);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn packed_sint(&mut self, field: u32, values: &[i64]) -> &mut Self {
        let mut inner = Pb::default();
        for &v in values {
            inner.raw_varint(zigzag(v));
        }
        self.bytes(field, &inner.0)
    }
}

pub fn node_ref_uuid(id: Uuid) -> Vec<u8> {
    let mut pb = Pb::default();
    pb.varint(1, 1).bytes(2, id.as_bytes());
    pb.0
}

pub fn node_ref_name(name: &str) -> Vec<u8> {
    let mut pb = Pb::default();
    pb.varint(1, 1).bytes(2, name.as_bytes());
    pb.0
}

fn node_ref_inline(payload: &[u8]) -> Vec<u8> {
    let mut pb = Pb::default();
    pb.varint(1, 2).bytes(2, payload);
    pb.0
}

fn child_entry(node_ref: &[u8], props: &[(&str, String)]) -> Vec<u8> {
    let mut pb = Pb::default();
    pb.bytes(1, node_ref);
    for (name, value) in props {
        let mut prop = Pb::default();
        prop.bytes(1, name.as_bytes()).bytes(2, value.as_bytes());
        pb.bytes(3, &prop.0);
    }
    pb.0
}

fn doc_value(children: &[Vec<u8>]) -> Vec<u8> {
    let mut pb = Pb::default();
    for child in children {
        pb.bytes(1, child);
    }
    // The constant DiffSync counters seen on every doc (research §4).
    let mut seven = Pb::default();
    seven.varint(1, 0);
    pb.varint(3, 0)
        .varint(4, 0)
        .varint(6, 0)
        .bytes(7, &seven.0)
        .varint(8, 0)
        .varint(9, 1);
    pb.0
}

/// A stroke payload in the WILL 2.0 layout of research §5 (precision 2).
pub fn stroke_payload(points: &[(f64, f64)], widths: &[f64], rgba: u32) -> Vec<u8> {
    let to_int = |v: f64| (v * 100.0).round() as i64;
    let mut xy = Vec::new();
    let (mut px, mut py) = (0, 0);
    for &(x, y) in points {
        let (ix, iy) = (to_int(x), to_int(y));
        xy.push(ix - px);
        xy.push(iy - py);
        (px, py) = (ix, iy);
    }
    let mut ws = Vec::new();
    let mut pw = 0;
    for &w in widths {
        let iw = to_int(w);
        ws.push(iw - pw);
        pw = iw;
    }
    let mut msg = Pb::default();
    msg.fixed32(1, 0.0f32.to_bits())
        .fixed32(2, 1.0f32.to_bits())
        .varint(3, 2)
        .packed_sint(4, &xy)
        .packed_sint(5, &ws)
        .packed_sint(6, &[i64::from(rgba as i32)])
        .varint(9, 2);
    let mut framed = Pb::default();
    framed.raw_varint(msg.0.len() as u64);
    framed.0.extend_from_slice(&msg.0);
    framed.0
}

#[derive(Debug, Clone)]
pub struct SynthStroke {
    /// Control points in stroke space (portrait Slate space), first and last duplicated.
    pub points: Vec<(f64, f64)>,
    pub widths: Vec<f64>,
    pub rgba: u32,
    pub points_per_second: Option<String>,
    pub creation_date: Option<i64>,
    /// Raw payload instead of `points`/`widths` (to inject corrupt data).
    pub raw_payload: Option<Vec<u8>>,
}

impl SynthStroke {
    /// A stroke through `samples` (at least one; duplicated ends added), constant-ish widths.
    pub fn through(
        samples: &[(f64, f64)],
        rgba: u32,
        pps: Option<&str>,
        date: Option<i64>,
    ) -> Self {
        let mut points = Vec::with_capacity(samples.len() + 2);
        points.extend(samples.first().copied());
        points.extend_from_slice(samples);
        points.extend(samples.last().copied());
        let widths = (0..points.len())
            .map(|i| 1.6 + 0.4 * ((i % 3) as f64) / 2.0)
            .collect();
        SynthStroke {
            points,
            widths,
            rgba,
            points_per_second: pps.map(str::to_owned),
            creation_date: date,
            raw_payload: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SynthNote {
    pub id: Uuid,
    pub created: i64,
    pub modified: i64,
    pub landscape: bool,
    pub layers: Vec<Vec<SynthStroke>>,
}

fn leveldb_error(status: Status) -> io::Error {
    io::Error::other(status.to_string())
}

/// Writes a LevelDB holding `entries` into `dir` (created): built in memory, then every file
/// is copied out, including `LOCK` and `LOG` like a real database folder. With
/// `flush_between`, the first half of the entries goes to a table file and the rest stays in
/// the log.
pub fn write_leveldb(
    dir: &Path,
    entries: &[(Vec<u8>, Vec<u8>)],
    flush_between: bool,
) -> io::Result<()> {
    let env: Rc<Box<dyn Env>> = Rc::new(Box::new(MemEnv::new()));
    let name = Path::new("db");
    {
        let options = Options {
            env: env.clone(),
            ..Options::default()
        };
        let mut db = DB::open(name, options).map_err(leveldb_error)?;
        let half = entries.len() / 2;
        for (i, (k, v)) in entries.iter().enumerate() {
            db.put(k, v).map_err(leveldb_error)?;
            if flush_between && i + 1 == half {
                // Writes the memtable out to a table file.
                db.compact_range(b"", &[0xff; 64]).map_err(leveldb_error)?;
            }
        }
        db.close().map_err(leveldb_error)?;
    }
    std::fs::create_dir_all(dir)?;
    for child in env.children(name).map_err(leveldb_error)? {
        let mut file = env
            .open_sequential_file(&name.join(&child))
            .map_err(leveldb_error)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        std::fs::write(dir.join(&child), bytes)?;
    }
    Ok(())
}

/// Key/value entries of a `WacomCloudDb\_` holding `notes`.
pub fn cloud_db_entries(notes: &[SynthNote]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let doc_key = |node_ref: &[u8]| [b"__doc__".as_slice(), node_ref].concat();
    let mut entries = vec![
        (b"__db_ver__".to_vec(), b"1.1".to_vec()),
        (b"__db_uid__".to_vec(), vec![7u8; 16]),
    ];
    let mut root_children = Vec::new();
    for (n, note) in notes.iter().enumerate() {
        let page_id = Uuid::from_u128(0xaaaa_0000_0000_4000_8000_0000_0000_0000 + n as u128);
        let (bbox, transform) = if note.landscape {
            (
                "rect(0.0 0.0 1188.0 864.0)",
                "matrix(0.0 -1.0 1.0 0.0 0.0 864.0)",
            )
        } else {
            (
                "rect(0.0 0.0 864.0 1188.0)",
                "matrix(1.0 0.0 0.0 1.0 0.0 0.0)",
            )
        };
        root_children.push(child_entry(
            &node_ref_uuid(note.id),
            &[
                ("nodeType", "document".into()),
                ("stream", "mate".into()),
                ("title", String::new()),
                ("creationDate", note.created.to_string()),
                ("lastModifiedDate", note.modified.to_string()),
            ],
        ));
        let page_entry = child_entry(
            &node_ref_uuid(page_id),
            &[
                ("nodeType", "page".into()),
                ("stream", "mate".into()),
                ("locale", "fr_FR".into()),
                ("boundingBox", bbox.into()),
                ("transform", transform.into()),
                ("creationDate", note.created.to_string()),
                ("lastModifiedDate", note.modified.to_string()),
            ],
        );
        entries.push((doc_key(&node_ref_uuid(note.id)), doc_value(&[page_entry])));
        let mut layer_entries = Vec::new();
        for (l, strokes) in note.layers.iter().enumerate() {
            let layer_id = Uuid::from_u128(
                0xbbbb_0000_0000_4000_8000_0000_0000_0000 + (n as u128) * 100 + l as u128,
            );
            layer_entries.push(child_entry(
                &node_ref_uuid(layer_id),
                &[
                    ("nodeType", "layer".into()),
                    ("stream", "mate".into()),
                    ("layerType", "vector".into()),
                    ("lastModifiedDate", note.modified.to_string()),
                ],
            ));
            let stroke_entries: Vec<Vec<u8>> = strokes
                .iter()
                .map(|s| {
                    let payload = s
                        .raw_payload
                        .clone()
                        .unwrap_or_else(|| stroke_payload(&s.points, &s.widths, s.rgba));
                    let mut props = Vec::new();
                    if let Some(pps) = &s.points_per_second {
                        props.push(("pointsPerSecond", pps.clone()));
                    }
                    if let Some(date) = s.creation_date {
                        props.push(("creationDate", date.to_string()));
                    }
                    child_entry(&node_ref_inline(&payload), &props)
                })
                .collect();
            entries.push((
                doc_key(&node_ref_uuid(layer_id)),
                doc_value(&stroke_entries),
            ));
        }
        entries.push((doc_key(&node_ref_uuid(page_id)), doc_value(&layer_entries)));
    }
    entries.push((
        doc_key(&node_ref_name("__mate__")),
        doc_value(&root_children),
    ));
    entries
}

/// Writes `notes` as the note store of the `LocalState` folder `local_state`
/// (`WacomCloudDb\_`, created).
pub fn write_note_store(local_state: &Path, notes: &[SynthNote]) -> io::Result<()> {
    write_leveldb(
        &local_state.join("WacomCloudDb").join("_"),
        &cloud_db_entries(notes),
        true,
    )
}

/// Builds `<root>/LocalState` with `WacomCloudDb\_` holding `notes`; returns its path.
pub fn write_local_state(root: &Path, notes: &[SynthNote]) -> io::Result<PathBuf> {
    let local_state = root.join("LocalState");
    write_note_store(&local_state, notes)?;
    Ok(local_state)
}

/// `s` as UTF-16LE bytes, the encoding of Inkspace's settings store (research §2).
pub fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// `db\`: an Inkspace-like settings LevelDB (UTF-16LE keys and JSON values, plus ASCII copies)
/// whose `device`, `profile`, `queue` and `settings` values, and the text logs `LOG` and
/// `LOG.old`, all carry `marker`. Nothing may ever read them.
pub fn write_settings_db(db: &Path, marker: &str) -> io::Result<()> {
    let value =
        |name: &str| format!("{{\"{name}\":\"{marker}\",\"email\":\"{marker}@example.invalid\"}}");
    let mut entries = Vec::new();
    for key in ["device", "profile", "queue", "settings"] {
        entries.push((utf16le(key), utf16le(&value(key))));
        entries.push((format!("{key}-ascii").into_bytes(), value(key).into_bytes()));
    }
    write_leveldb(db, &entries, true)?;
    std::fs::write(db.join("LOG"), format!("profile {marker}\n"))?;
    std::fs::write(db.join("LOG.old"), format!("profile {marker}\n"))
}

/// One stroke of the demo notes: a wavy line of `n` samples along row `row` of the page.
/// Stroke space is the Slate's portrait space (research §4.1): on a landscape page, page x is
/// stroke y and page y is 864 − stroke x, so a row there runs along stroke y.
fn wave(row: usize, landscape: bool, n: usize, rgba: u32, date: Option<i64>) -> SynthStroke {
    let across = 150.0 + 60.0 * row as f64;
    let samples: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let along = 120.0 + 0.8 * i as f64;
            let wobble = 6.0 * (i as f64 / 6.0).sin();
            if landscape {
                (864.0 - across + wobble, along)
            } else {
                (along, across + wobble)
            }
        })
        .collect();
    let pps = date.map(|_| "200");
    SynthStroke::through(&samples, rgba, pps, date)
}

/// A few invented notes in the shapes Inkspace stores (research §9): landscape and portrait
/// pages, one and two layers, timed strokes (200 Hz) and untimed ones, Inkspace's colours.
pub fn demo_notes() -> Vec<SynthNote> {
    // 2025-09-08 08:00 UTC, then one note every few days.
    const FIRST_MS: i64 = 1_757_318_400_000;
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    const BLUE: u32 = 0x51A7_F9FF;
    const BLACK: u32 = 0x0000_00FF;
    const MAGENTA: u32 = 0xBB3E_9AFF;
    const GREEN: u32 = 0x84BD_00FF;
    const RED: u32 = 0xD800_00FF;
    let at = |day: i64| FIRST_MS + day * DAY_MS;
    // Rows `rows` of handwriting-like lines; timed strokes start 2 s apart from `start`.
    let lines = |rows: std::ops::Range<usize>, landscape: bool, rgba: u32, start: Option<i64>| {
        rows.map(|row| {
            let date = start.map(|s| s + 2_000 * row as i64);
            wave(row, landscape, 160, rgba, date)
        })
        .collect::<Vec<_>>()
    };
    let note = |n: u128, day: i64, landscape: bool, layers: Vec<Vec<SynthStroke>>| SynthNote {
        // The number leads the id, so that the short ids (8 characters) of a table differ.
        id: Uuid::from_u128(0x5e1f_0000_0000_4000_8000_0000_0000_0000 + (n << 96)),
        created: FIRST_MS + day * DAY_MS,
        modified: FIRST_MS + day * DAY_MS + 600_000,
        landscape,
        layers,
    };
    vec![
        note(1, 0, true, vec![lines(0..6, true, BLUE, Some(at(0)))]),
        note(2, 3, true, vec![lines(0..4, true, BLACK, None)]),
        note(
            3,
            7,
            false,
            vec![
                lines(0..5, false, MAGENTA, Some(at(7))),
                lines(6..8, false, RED, Some(at(7) + 60_000)),
            ],
        ),
        note(4, 10, true, vec![lines(0..3, true, GREEN, Some(at(10)))]),
    ]
}
