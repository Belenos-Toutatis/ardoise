//! Shared test helpers: the private Inkspace copy, a synthetic `LocalState` builder and a
//! reference renderer for geometry checks.
#![allow(dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ardoise_core::note::Note;
use ardoise_inkspace::width;
use ardoise_render::Image;
use rusty_leveldb::env::Env;
use rusty_leveldb::{DB, MemEnv, Options};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};
use uuid::Uuid;

// ---------------------------------------------------------------------------------------
// The private copy (git-ignored, never committed).

/// `testdata/private/inkspace/LocalState` of the main repository (found by walking up from
/// this crate, so it also works from a git worktree), or `ARDOISE_INKSPACE_COPY`.
pub fn private_local_state() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ARDOISE_INKSPACE_COPY").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(dir);
        return dir.is_dir().then_some(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|a| {
            a.join("testdata")
                .join("private")
                .join("inkspace")
                .join("LocalState")
        })
        .find(|candidate| candidate.is_dir())
}

/// The private copy, or `None` after printing why the test is skipped.
pub fn private_copy_or_skip(test: &str) -> Option<PathBuf> {
    let found = private_local_state();
    if found.is_none() {
        println!(
            "{test}: SKIPPED - no private Inkspace copy (testdata/private/inkspace/LocalState \
             or ARDOISE_INKSPACE_COPY)"
        );
    }
    found
}

// ---------------------------------------------------------------------------------------
// Reference renderer: what the ballpoint draws, without the real renderer.

/// Draws every stroke as round-capped straight segments between consecutive samples, each
/// with the mean of its two end diameters (`width::width_at(stroke.width, pressure)`), and a
/// lone sample as a dot. Visible layers in order, white page, anti-aliased.
pub fn reference_render(note: &Note, scale: f32) -> Image {
    let width = (note.page.width as f32 * scale).round() as u32;
    let height = (note.page.height as f32 * scale).round() as u32;
    let mut pixmap = Pixmap::new(width, height).expect("pixmap size");
    pixmap.fill(tiny_skia::Color::WHITE);
    let transform = Transform::from_scale(scale, scale);
    for (layer_index, layer) in note.layers.iter().enumerate() {
        if !layer.visible {
            continue;
        }
        for stroke in note
            .strokes
            .iter()
            .filter(|s| s.layer as usize == layer_index)
        {
            let mut paint = Paint::default();
            let c = stroke.color;
            paint.set_color_rgba8(c.r, c.g, c.b, c.a);
            paint.anti_alias = true;
            let d = |p: &ardoise_ink::Point| width::width_at(stroke.width, p.pressure);
            if let [only] = stroke.points.as_slice() {
                if let Some(circle) =
                    PathBuilder::from_circle(only.x, only.y, (d(only) / 2.0).max(0.01))
                {
                    pixmap.fill_path(&circle, &paint, FillRule::Winding, transform, None);
                }
                continue;
            }
            for pair in stroke.points.windows(2) {
                let (a, b) = (&pair[0], &pair[1]);
                let mut pb = PathBuilder::new();
                pb.move_to(a.x, a.y);
                pb.line_to(b.x, b.y);
                let Some(path) = pb.finish() else { continue };
                let line = Stroke {
                    width: ((d(a) + d(b)) / 2.0).max(0.01),
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Default::default()
                };
                pixmap.stroke_path(&path, &paint, &line, transform, None);
            }
        }
    }
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Image {
        width,
        height,
        pixels,
    }
}

/// PNG bytes of an RGBA image.
pub fn encode_png(image: &Image) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(&image.pixels).expect("png data");
    }
    bytes
}

// ---------------------------------------------------------------------------------------
// Synthetic Inkspace data (publishable: made up here).

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
    /// A stroke through `samples` (duplicated ends added), constant-ish widths.
    pub fn through(
        samples: &[(f64, f64)],
        rgba: u32,
        pps: Option<&str>,
        date: Option<i64>,
    ) -> Self {
        let mut points = vec![samples[0]];
        points.extend_from_slice(samples);
        points.push(samples[samples.len() - 1]);
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

/// Writes a LevelDB holding `entries` into `dir` (created): built in memory, then every file
/// is copied out, including `LOCK` and `LOG` like a real database folder. With
/// `flush_between`, the first half of the entries goes to a table file and the rest stays in
/// the log.
pub fn write_leveldb(dir: &Path, entries: &[(Vec<u8>, Vec<u8>)], flush_between: bool) {
    let env: Rc<Box<dyn Env>> = Rc::new(Box::new(MemEnv::new()));
    let name = Path::new("db");
    {
        let options = Options {
            env: env.clone(),
            ..Options::default()
        };
        let mut db = DB::open(name, options).expect("open in-memory db");
        let half = entries.len() / 2;
        for (i, (k, v)) in entries.iter().enumerate() {
            db.put(k, v).expect("put");
            if flush_between && i + 1 == half {
                // Writes the memtable out to a table file.
                db.compact_range(b"", &[0xff; 64]).expect("compact");
            }
        }
        db.close().expect("close");
    }
    std::fs::create_dir_all(dir).expect("create db dir");
    for child in env.children(name).expect("children") {
        let mut file = env
            .open_sequential_file(&name.join(&child))
            .expect("open mem file");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("read mem file");
        std::fs::write(dir.join(&child), bytes).expect("write db file");
    }
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

/// Builds `<root>/LocalState` with `WacomCloudDb\_` holding `notes`; returns its path.
pub fn write_local_state(root: &Path, notes: &[SynthNote]) -> PathBuf {
    let local_state = root.join("LocalState");
    write_leveldb(
        &local_state.join("WacomCloudDb").join("_"),
        &cloud_db_entries(notes),
        true,
    );
    local_state
}

/// Every file under `dir` with its bytes, sorted (to prove a folder was not modified).
pub fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("read");
                out.push((path, bytes));
            }
        }
    }
    out.sort();
    out
}
