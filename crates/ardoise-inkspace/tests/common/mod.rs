//! Shared test helpers: the private Inkspace copy, a synthetic `LocalState` builder and a
//! reference renderer for geometry checks.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use ardoise_core::note::Note;
use ardoise_inkspace::width;
use ardoise_render::Image;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

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
// Synthetic Inkspace data (publishable: made up in `ardoise_inkspace::synthetic`).

// Each test file uses its own share of these.
#[allow(unused_imports)]
pub use ardoise_inkspace::synthetic::{SynthNote, SynthStroke, node_ref_uuid, utf16le};

/// [`ardoise_inkspace::synthetic::write_leveldb`], failing the test on error.
pub fn write_leveldb(dir: &Path, entries: &[(Vec<u8>, Vec<u8>)], flush_between: bool) {
    ardoise_inkspace::synthetic::write_leveldb(dir, entries, flush_between).expect("write leveldb");
}

/// [`ardoise_inkspace::synthetic::write_local_state`], failing the test on error.
pub fn write_local_state(root: &Path, notes: &[SynthNote]) -> PathBuf {
    ardoise_inkspace::synthetic::write_local_state(root, notes).expect("write LocalState")
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
