//! Notes for the interface: the editor's commands (open, save, page background) and stroke
//! outlines. The outlines the screen draws come from the same geometry
//! (`ardoise_ink::outline`) as the PNG/PDF/video exports, so both always show the same strokes
//! (SPEC §6.2).

use std::path::Path;

use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_core::note::{Background, BackgroundImage, Note, PageSize};
use ardoise_ink::{Tool, outline};
use ardoise_render::{NoteView, Picture, RenderError, RenderOptions, encode_png, render_note};
use serde::Serialize;
use tauri::State;

use crate::AppContext;
use crate::background::{self, PlacedImage};
use crate::commands::{CommandError, CommandResult};
use crate::library::{blank_note, library_error, library_of, now_ms};
use crate::revision::{self, RevisionView};
use crate::thumbnails::png_data_url;

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

/// Outlines of the note stored at `path`, which must be a note of the library (the interface
/// cannot read other files through this command).
pub fn outlines_in(library: &Library, path: &Path) -> Result<NoteOutlines, LibraryError> {
    Ok(outlines_of(&library.read(path)?.file.note))
}

#[tauri::command]
pub async fn note_outlines(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<NoteOutlines> {
    let library = library_of(&context)?;
    blocking(move || outlines_in(&library, Path::new(&path)).map_err(library_error)).await
}

/// A note opened for editing, with the stamp to pass back when saving it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenedNote {
    pub note: Note,
    pub stamp: FileStamp,
    /// Present when the note is a revision, a retouched copy of another note: its account, and
    /// where its source is now.
    pub revision: Option<RevisionView>,
}

pub fn open_in(library: &Library, path: &Path) -> Result<OpenedNote, LibraryError> {
    let loaded = library.read(path)?;
    Ok(OpenedNote {
        revision: loaded
            .file
            .revision
            .as_ref()
            .map(|info| revision::view_of(library, info)),
        note: loaded.file.note,
        stamp: loaded.stamp,
    })
}

/// Saves an edited note over its file: only the note changes (the tablet's `original.bin`,
/// audio, background image, recognised text and anything unknown are kept), `modified` becomes
/// now and the thumbnail is made again. The first save that changes the ink of a note without
/// an original keeps the version on disk as `original.ardoise` (`original::keep_original`). Refused with `noteConflict` if the file is no longer
/// the version of `stamp`. The note keeps its id and its file name.
pub fn save_in(
    library: &Library,
    path: &Path,
    mut note: Note,
    stamp: &FileStamp,
) -> Result<FileStamp, LibraryError> {
    let loaded = library.read(path)?;
    if loaded.stamp != *stamp {
        return Err(LibraryError::Conflict { path: loaded.path });
    }
    let mut file = loaded.file;
    note.id = file.note.id.clone();
    note.modified = now_ms().max(file.note.modified);
    // "Back to the original": the version on disk is kept once, before its first edit.
    crate::original::keep_original(&mut file, &note).map_err(|source| LibraryError::Format {
        path: loaded.path.clone(),
        source,
    })?;
    file.thumbnail = background::settle(&mut file, &note);
    file.note = note;
    library.save(&loaded.path, &file, stamp)
}

/// Largest side of a rendered background, in pixels.
pub const MAX_BACKGROUND_PX: u32 = 4096;

/// PNG of a page background (paper, template, texture, and the background image when its
/// pixels are given) `width_px` wide, both sides capped at [`MAX_BACKGROUND_PX`].
pub fn background_png(
    page: PageSize,
    background: Background,
    width_px: u32,
    image: Option<(BackgroundImage, &Picture)>,
) -> Result<Vec<u8>, CommandError> {
    if page.width == 0 || page.height == 0 {
        return Err(CommandError::new(
            "noteInvalid",
            format!("empty page ({} x {})", page.width, page.height),
        ));
    }
    let width = width_px.clamp(1, MAX_BACKGROUND_PX) as f32;
    let scale = (width / page.width as f32).min(MAX_BACKGROUND_PX as f32 / page.height as f32);
    let mut note = blank_note("", "", 0);
    note.page = page;
    note.background = background;
    let picture = image.map(|(image, picture)| {
        note.background_image = Some(image);
        picture
    });
    let render_error = |error: RenderError| match error {
        RenderError::Size { .. } => CommandError::new("noteInvalid", error),
        _ => CommandError::new("render", error),
    };
    let image = render_note(
        NoteView::new(&note, picture),
        &RenderOptions {
            scale,
            background: true,
        },
    )
    .map_err(render_error)?;
    encode_png(&image).map_err(render_error)
}

/// Runs `work` on a blocking thread: reading, decoding and writing a note file take time and
/// must not hold one of the async runtime's few threads.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::new("unexpected", error))?
}

#[tauri::command]
pub async fn open_note(context: State<'_, AppContext>, path: String) -> CommandResult<OpenedNote> {
    let library = library_of(&context)?;
    blocking(move || open_in(&library, Path::new(&path)).map_err(library_error)).await
}

/// Saves the editor's note. `image_key` names the staged pixels of its background image
/// (`background::Staging`); without it the file's own image member is kept, which must show
/// the image the note describes (`background::check_keyless`).
#[tauri::command]
pub async fn save_note(
    context: State<'_, AppContext>,
    path: String,
    note: Note,
    stamp: FileStamp,
    image_key: Option<String>,
) -> CommandResult<FileStamp> {
    let library = library_of(&context)?;
    blocking(move || {
        save_with_key(
            &library,
            Path::new(&path),
            note,
            &stamp,
            image_key.as_deref(),
        )
    })
    .await
}

/// [`save_in`] with the background pixels staged under `key` (`save_note`).
pub fn save_with_key(
    library: &Library,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    key: Option<&str>,
) -> CommandResult<FileStamp> {
    let chosen = background::chosen_for_save(background::staging(), &note, key)?;
    if key.is_none() && note.background_image.is_some() {
        let on_disk = library.read(path).map_err(library_error)?;
        if on_disk.stamp == *stamp {
            background::check_keyless(&on_disk.file.note, &note)?;
        }
    }
    background::with_staged(chosen, || save_in(library, path, note, stamp)).map_err(library_error)
}

/// The page background as a PNG data URL; with `image`, the background image staged under its
/// key is drawn over the paper (an image whose pixels are not staged is left out).
#[tauri::command]
pub async fn render_background(
    page: PageSize,
    background: Background,
    width_px: u32,
    image: Option<PlacedImage>,
) -> CommandResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let picture = image
            .as_ref()
            .and_then(|placed| background::staging().picture(&placed.key));
        let image = image
            .zip(picture.as_deref())
            .map(|(placed, picture)| (placed.image, picture));
        background_png(page, background, width_px, image).map(|png| png_data_url(&png))
    })
    .await
    .map_err(|error| CommandError::new("render", error))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::{library, write_sample};
    use crate::thumbnails::tests::png_of;
    use ardoise_core::format::{BackgroundFile, Member, NoteFile, write_note};
    use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Timing};
    use ardoise_core::note::{A4_PORTRAIT, Paper, Template};
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
            background_image: None,
            tags: vec![],
            favorite: false,
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
        for (drawn, stroke) in outlines
            .strokes
            .iter()
            .zip([&note.strokes[0], &note.strokes[2]])
        {
            assert_eq!(drawn.path, outline(stroke).to_svg_path());
            assert!(drawn.path.starts_with('M'), "{}", drawn.path);
        }
    }

    #[test]
    fn reads_the_outlines_of_a_note_of_the_library_only() {
        let (dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let strokes = library.read(&saved.path).unwrap().file.note.strokes.len();
        assert_eq!(
            outlines_in(&library, &saved.path).unwrap().strokes.len(),
            strokes
        );
        let missing = outlines_in(&library, &dir.path().join("absent.ardoise"));
        assert!(
            matches!(missing, Err(LibraryError::NotFound { .. })),
            "{missing:?}"
        );

        // A note file outside the library is refused, even though it could be read.
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("2026-10-24 Ailleurs.ardoise");
        write_note(&outside, &NoteFile::new(note())).unwrap();
        let refused = outlines_in(&library, &outside);
        assert!(
            matches!(refused, Err(LibraryError::NotInLibrary { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn open_and_save_keep_every_other_member() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        // Give the file members the editor knows nothing about.
        let mut file = library.read(&saved.path).unwrap().file;
        file.original = Some(vec![1, 2, 3, 4]);
        file.audio = Some(vec![9; 16]);
        file.text = Some(br#"{"lines":[]}"#.to_vec());
        file.mark_as_revision_of(&blank_note("Brouillon", "Calque 1", 1), 5, "Rangé");
        file.background = Some(BackgroundFile {
            extension: "png".into(),
            bytes: vec![7; 8],
        });
        file.unknown_fields
            .insert("future".into(), serde_json::json!({"x": 1}));
        file.unknown_members.push(Member {
            name: "future/data.bin".into(),
            bytes: vec![5; 10],
        });
        let old_thumbnail = file.thumbnail.clone();
        let stamp = library.save(&saved.path, &file, &saved.stamp).unwrap();

        let opened = open_in(&library, &saved.path).unwrap();
        assert_eq!(opened.stamp, stamp);
        assert_eq!(opened.note, file.note);
        let json = serde_json::to_value(&opened).unwrap();
        assert!(json["note"]["strokes"][0]["points"][0]["pressure"].is_number());
        assert!(json["stamp"]["modified"]["nanos_since_epoch"].is_u64());

        // The editor removes the stroke and changes the paper.
        let mut edited = opened.note.clone();
        edited.strokes.clear();
        edited.background.paper = Paper::Slate;
        edited.id = "something else".into();
        let new_stamp = save_in(&library, &saved.path, edited, &opened.stamp).unwrap();
        assert_ne!(new_stamp, stamp);
        assert_eq!(FileStamp::of(&saved.path).unwrap(), new_stamp);

        let after = library.read(&saved.path).unwrap().file;
        assert!(after.note.strokes.is_empty());
        assert_eq!(after.note.background.paper, Paper::Slate);
        assert_eq!(after.note.id, file.note.id, "the id never changes");
        assert!(after.note.modified >= file.note.modified);
        assert_eq!(after.original, file.original);
        assert_eq!(after.audio, file.audio);
        assert_eq!(after.text, file.text);
        assert_eq!(after.revision, file.revision);
        assert!(after.revision.is_some());
        assert_eq!(after.background, file.background);
        assert_eq!(after.unknown_fields, file.unknown_fields);
        assert_eq!(after.unknown_members, file.unknown_members);
        assert!(after.thumbnail.is_some());
        assert_ne!(after.thumbnail, old_thumbnail, "thumbnail made again");

        // A second save with the old stamp is a conflict and changes nothing.
        let error = save_in(&library, &saved.path, opened.note.clone(), &opened.stamp).unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        assert_eq!(FileStamp::of(&saved.path).unwrap(), new_stamp);
    }

    #[test]
    fn invalid_notes_are_not_saved() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let opened = open_in(&library, &saved.path).unwrap();
        let mut broken = opened.note.clone();
        broken.layers.clear();
        let error = save_in(&library, &saved.path, broken, &opened.stamp).unwrap_err();
        assert_eq!(error.code(), "noteInvalid");
        assert_eq!(FileStamp::of(&saved.path).unwrap(), opened.stamp);

        let outside = tempfile::tempdir().unwrap();
        let elsewhere = outside.path().join("2026-10-24 X.ardoise");
        assert_eq!(
            open_in(&library, &elsewhere).unwrap_err().code(),
            "notInLibrary"
        );
    }

    #[test]
    fn renders_backgrounds_at_the_requested_width() {
        let background = Background {
            paper: Paper::Cream,
            template: Template::Seyes,
            texture: true,
        };
        let url = render_background_url(A4_LANDSCAPE, background, 1200);
        let image = ardoise_render::decode_png(&png_of(&url)).unwrap();
        assert_eq!(image.width, 1200);
        assert_eq!(
            image.height,
            (1200.0f32 * 21_600.0 / 29_700.0).round() as u32
        );
        // Cream, not white.
        let [r, g, b, _] = image.pixel(5, 5).unwrap();
        assert!(r > b && g > b, "{r} {g} {b}");

        // Capped on both sides.
        let huge = background_png(A4_PORTRAIT, background, 100_000, None).unwrap();
        let image = ardoise_render::decode_png(&huge).unwrap();
        assert!(image.width <= MAX_BACKGROUND_PX && image.height <= MAX_BACKGROUND_PX);
        assert_eq!(image.height, MAX_BACKGROUND_PX);

        let error = background_png(
            PageSize {
                width: 0,
                height: 10,
            },
            background,
            100,
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "noteInvalid");
    }

    fn render_background_url(page: PageSize, background: Background, width: u32) -> String {
        tauri::async_runtime::block_on(render_background(page, background, width, None)).unwrap()
    }
}
