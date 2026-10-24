//! Notes for the interface. The outlines the screen draws come from the same geometry
//! (`ardoise_ink::outline`) as the PNG/PDF/video exports, so both always show the same strokes
//! (SPEC §6.2).

use std::path::Path;

use ardoise_core::format::read_note;
use ardoise_core::note::Note;
use ardoise_ink::{Tool, outline};
use serde::Serialize;

use crate::commands::CommandError;

/// One stroke ready to draw: an SVG path to fill with the non-zero rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeOutline {
    /// `#rrggbbaa`.
    pub color: String,
    pub tool: Tool,
    pub layer: u32,
    /// SVG path data in page coordinates (device units of 10 µm).
    pub path: String,
}

/// The visible strokes of a note, in drawing order, with the page size.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteOutlines {
    pub width: u32,
    pub height: u32,
    pub strokes: Vec<StrokeOutline>,
}

/// Outlines of the strokes on visible layers (a stroke whose layer is missing is drawn, as
/// the renderer does).
pub fn outlines_of(note: &Note) -> NoteOutlines {
    let visible = |layer: u32| {
        note.layers
            .get(layer as usize)
            .is_none_or(|layer| layer.visible)
    };
    let strokes = note
        .strokes
        .iter()
        .filter(|stroke| visible(stroke.layer))
        .map(|stroke| StrokeOutline {
            color: format!("#{:08x}", stroke.color.to_rgba_u32()),
            tool: stroke.tool,
            layer: stroke.layer,
            path: outline(stroke).to_svg_path(),
        })
        .collect();
    NoteOutlines {
        width: note.page.width,
        height: note.page.height,
        strokes,
    }
}

/// Outlines of the note stored at `path`.
#[tauri::command]
pub fn note_outlines(path: String) -> Result<NoteOutlines, CommandError> {
    let file = read_note(Path::new(&path)).map_err(|e| CommandError::new(e.code(), e))?;
    Ok(outlines_of(&file.note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::format::{NoteFile, write_note};
    use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Timing};
    use ardoise_ink::{Point, Rgba, Stroke};

    fn stroke(layer: u32, color: Rgba) -> Stroke {
        Stroke {
            tool: Tool::FountainPen,
            color,
            width: 90.0,
            layer,
            points: (0..20)
                .map(|i| Point {
                    x: 1000.0 + 150.0 * i as f32,
                    y: 2000.0 + 40.0 * (i as f32).sin(),
                    pressure: 0.3 + 0.03 * i as f32,
                    t: 5 * i,
                })
                .collect(),
        }
    }

    fn note() -> Note {
        Note {
            id: "6f1c9a52-6a3b-4f39-9a0e-3c2b8f8e1d7a".into(),
            title: "Essai".into(),
            created: 1_792_843_200_000,
            modified: 1_792_843_200_000,
            page: A4_LANDSCAPE,
            background: Background::default(),
            tags: vec![],
            layers: vec![
                Layer {
                    name: "Calque 1".into(),
                    visible: true,
                },
                Layer {
                    name: "Calque 2".into(),
                    visible: false,
                },
            ],
            strokes: vec![
                stroke(0, Rgba::rgb(0x51, 0xa7, 0xf9)),
                stroke(1, Rgba::BLACK),
                stroke(0, Rgba::rgb(0xbb, 0x3e, 0x9a)),
            ],
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
    }

    #[test]
    fn the_screen_gets_exactly_the_geometry_of_the_exports() {
        let note = note();
        let outlines = outlines_of(&note);
        assert_eq!((outlines.width, outlines.height), (29_700, 21_600));
        // The stroke on the hidden layer is left out; the others keep their order.
        assert_eq!(outlines.strokes.len(), 2);
        assert_eq!(outlines.strokes[0].color, "#51a7f9ff");
        assert_eq!(outlines.strokes[1].color, "#bb3e9aff");
        for (drawn, stroke) in outlines.strokes.iter().zip([&note.strokes[0], &note.strokes[2]]) {
            assert_eq!(drawn.path, outline(stroke).to_svg_path());
            assert!(drawn.path.starts_with('M'), "{}", drawn.path);
        }
    }

    #[test]
    fn reads_the_outlines_of_a_saved_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2026-10-24 Essai.ardoise");
        write_note(&path, &NoteFile::new(note())).unwrap();
        let outlines = note_outlines(path.display().to_string()).unwrap();
        assert_eq!(outlines.strokes.len(), 2);
        let missing = note_outlines(dir.path().join("absent.ardoise").display().to_string());
        assert!(missing.is_err());
    }
}
