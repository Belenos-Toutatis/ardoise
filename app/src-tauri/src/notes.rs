//! Notes for the interface: the editor's commands (open, save, page background) and stroke
//! outlines. The outlines the screen draws come from the same geometry
//! (`ardoise_ink::outline`) as the PNG/PDF/video exports, so both always show the same strokes
//! (SPEC §6.2).

use std::path::Path;

use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_core::note::{Background, BackgroundImage, Note, PageSize};
use ardoise_ink::{Tool, outline};
use ardoise_media::narration_header;
use ardoise_render::{NoteView, Picture, RenderError, RenderOptions, encode_png, render_note};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::AppContext;
use crate::background::{self, PlacedImage};
use crate::commands::{CommandError, CommandResult};
use crate::library::{blank_note, library_error, library_of, now_ms};
use crate::narration::{NarrationInfo, NarrationRuntime};
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
    /// The narration the note holds, read from the header of its audio member (M17); `None` without
    /// one, or when it is damaged (the replay says why: `narration_info`).
    pub narration: Option<NarrationInfo>,
}

pub fn open_in(library: &Library, path: &Path) -> Result<OpenedNote, LibraryError> {
    let loaded = library.read(path)?;
    // The member is already read: only its headers are parsed, nothing is decoded.
    let narration = loaded
        .file
        .audio
        .as_deref()
        .and_then(|bytes| narration_header(bytes).ok())
        .map(|header| NarrationInfo::of(&header));
    Ok(OpenedNote {
        revision: loaded
            .file
            .revision
            .as_ref()
            .map(|info| revision::view_of(library, info)),
        note: loaded.file.note,
        stamp: loaded.stamp,
        narration,
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
    note: Note,
    stamp: &FileStamp,
) -> Result<FileStamp, LibraryError> {
    save_in_with_account(library, path, note, stamp, None)
}

/// [`save_in`] that also adds `account` (a line, dated now) to the account of the file, a
/// revision, in the same write (`edit_revision`). A file that is not a revision takes no line.
pub fn save_in_with_account(
    library: &Library,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    account: Option<&str>,
) -> Result<FileStamp, LibraryError> {
    save_file(library, path, note, stamp, account, None)
}

/// [`save_in_with_account`] that, with `audio` (an `audio.opus` member), also makes it the note's
/// narration, replacing any, in the same write.
fn save_file(
    library: &Library,
    path: &Path,
    mut note: Note,
    stamp: &FileStamp,
    account: Option<&str>,
    audio: Option<&[u8]>,
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
    if let Some(account) = account {
        file.add_revision_entry(note.modified, account);
    }
    if let Some(audio) = audio {
        file.audio = Some(audio.to_vec());
    }
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
/// the image the note describes (`background::check_keyless`). `narration_key` names the sound of
/// a recording made while writing, kept in memory until now (`NarrationSaved::staged`): it is
/// written in the same save as the note (M17, [`save_note_in`]).
#[tauri::command]
pub async fn save_note(
    app: AppHandle,
    context: State<'_, AppContext>,
    path: String,
    note: Note,
    stamp: FileStamp,
    image_key: Option<String>,
    narration_key: Option<String>,
) -> CommandResult<FileStamp> {
    let library = library_of(&context)?;
    blocking(move || {
        save_note_in(
            &library,
            &app.state::<NarrationRuntime>(),
            Path::new(&path),
            note,
            &stamp,
            image_key.as_deref(),
            narration_key.as_deref(),
        )
    })
    .await
}

/// What `save_note` does: the note saved with the background pixels staged under `image_key`
/// ([`save_with_key_and_account`]) and, with `narration_key`, the sound staged under it
/// in `narration` written as the note's narration (replacing any) in the same atomic write, so the
/// strokes timed on it are never in the file without it. A key that is not staged is
/// `noStagedNarration` and nothing is written. Once saved, the staged sound and its backup copy
/// go; a failed save (`noteConflict`, a disk error) keeps both for the next save (a copy saved
/// elsewhere, or a retry).
pub fn save_note_in(
    library: &Library,
    narration: &NarrationRuntime,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    image_key: Option<&str>,
    narration_key: Option<&str>,
) -> CommandResult<FileStamp> {
    let audio = narration_key
        .map(|key| narration.staged_audio(key))
        .transpose()?;
    let saved = save_with_keys(
        library,
        path,
        note,
        stamp,
        image_key,
        None,
        audio.as_deref(),
    )?;
    if let Some(key) = narration_key {
        narration.drop_staged(key);
    }
    Ok(saved)
}

/// [`save_in`] with the background pixels staged under `key`: [`save_note_in`] without a
/// narration, for the tests of the background images.
#[cfg(test)]
pub fn save_with_key(
    library: &Library,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    key: Option<&str>,
) -> CommandResult<FileStamp> {
    save_with_key_and_account(library, path, note, stamp, key, None)
}

/// [`save_with_key`] that also adds `account` to the account of a revision
/// ([`save_in_with_account`]).
pub fn save_with_key_and_account(
    library: &Library,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    key: Option<&str>,
    account: Option<&str>,
) -> CommandResult<FileStamp> {
    save_with_keys(library, path, note, stamp, key, account, None)
}

/// [`save_with_key_and_account`] that, with `audio` (an `audio.opus` member), also makes it the
/// note's narration in the same write.
fn save_with_keys(
    library: &Library,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    key: Option<&str>,
    account: Option<&str>,
    audio: Option<&[u8]>,
) -> CommandResult<FileStamp> {
    let chosen = background::chosen_for_save(background::staging(), &note, key)?;
    if key.is_none() && note.background_image.is_some() {
        let on_disk = library.read(path).map_err(library_error)?;
        if on_disk.stamp == *stamp {
            background::check_keyless(&on_disk.file.note, &note)?;
        }
    }
    background::with_staged(chosen, || {
        save_file(library, path, note, stamp, account, audio)
    })
    .map_err(library_error)
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
    use std::time::Duration;

    use super::*;
    use crate::library::tests::{library, write_sample};
    use crate::narration::{BaseName, Source};
    use crate::thumbnails::tests::png_of;
    use ardoise_core::format::{BackgroundFile, Member, NoteFile, write_note};
    use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Timing};
    use ardoise_core::note::{A4_PORTRAIT, Paper, Template};
    use ardoise_ink::{Point, Rgba, Stroke};
    use ardoise_media::microphone::simulated_narration;
    use ardoise_media::replay::ReplayOptions;
    use ardoise_media::{Narration, NarrationBase, encode_narration};

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

    // --- The sound of a recording made while writing (M17) ---

    /// An `audio.opus` of `ms` of the simulated voice, in `base` time from `start_ms`.
    fn narration_audio(ms: u64, base: NarrationBase, start_ms: i64) -> Vec<u8> {
        encode_narration(&Narration {
            clip: simulated_narration(Duration::from_millis(ms), 1),
            base,
            options: ReplayOptions::default(),
            start_ms,
        })
        .unwrap()
    }

    /// A runtime that never records (the sounds are staged by hand), its copies in `backup`.
    fn narration_runtime(backup: &Path) -> NarrationRuntime {
        NarrationRuntime::new(
            Box::new(
                |_: Option<&str>, _: bool| -> CommandResult<Box<dyn Source>> {
                    Err(CommandError::new("microphoneFailed", "not in these tests"))
                },
            ),
            backup.to_path_buf(),
        )
    }

    /// How many files `dir` holds.
    fn files_in(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, |entries| entries.count())
    }

    #[test]
    fn a_staged_narration_is_written_with_the_note_in_one_save() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let backup = tempfile::tempdir().unwrap();
        let runtime = narration_runtime(backup.path());
        let opened = open_in(&library, &saved.path).unwrap();
        assert_eq!(opened.narration, None);
        assert_eq!(
            serde_json::to_value(&opened).unwrap()["narration"],
            serde_json::Value::Null
        );

        // Stopped: the sound waits, with its copy.
        let audio = narration_audio(500, NarrationBase::Note, 1_232);
        let key = runtime.stage(&saved.path, audio.clone());
        assert_eq!(files_in(backup.path()), 1);
        // The strokes written meanwhile and the sound go in the same save.
        let mut edited = opened.note.clone();
        edited.strokes.push(stroke(0, Rgba::BLACK));
        let stamp = save_note_in(
            &library,
            &runtime,
            &saved.path,
            edited.clone(),
            &opened.stamp,
            None,
            Some(&key),
        )
        .unwrap();
        let after = library.read(&saved.path).unwrap();
        assert_eq!(after.stamp, stamp);
        // (The file rounds the samples: compare the strokes, not their exact values.)
        assert_eq!(after.file.note.strokes.len(), 2);
        assert_eq!(after.file.note.strokes[1].tool, Tool::FountainPen);
        assert_eq!(after.file.audio.as_deref(), Some(&audio[..]));
        // Saved: the sound and its copy go.
        assert!(!runtime.has_staged());
        assert_eq!(files_in(backup.path()), 0);

        // Opening the note says what narration it holds, from the member's header.
        let reopened = open_in(&library, &saved.path).unwrap();
        let info = reopened.narration.clone().unwrap();
        assert_eq!(
            (info.base, info.speed, info.max_pause_ms, info.start_ms),
            (BaseName::Note, 1.0, None, 1_232)
        );
        assert!((490..=510).contains(&info.duration_ms), "{info:?}");
        let json = serde_json::to_value(&reopened).unwrap();
        assert_eq!(json["narration"]["base"], "note");
        assert_eq!(json["narration"]["startMs"], 1_232);

        // The key is used up: a save with it is refused and writes nothing.
        let before = std::fs::read(&saved.path).unwrap();
        let error = save_note_in(
            &library,
            &runtime,
            &saved.path,
            edited.clone(),
            &stamp,
            None,
            Some(&key),
        )
        .unwrap_err();
        assert_eq!(error.code(), "noStagedNarration");
        assert_eq!(std::fs::read(&saved.path).unwrap(), before);
        // Without a key, the note's narration is kept as it is.
        let mut retitled = edited;
        retitled.title = "Renommée".into();
        save_note_in(
            &library,
            &runtime,
            &saved.path,
            retitled,
            &stamp,
            None,
            None,
        )
        .unwrap();
        let after = library.read(&saved.path).unwrap().file;
        assert_eq!(after.note.title, "Renommée");
        assert_eq!(after.audio.as_deref(), Some(&audio[..]));
    }

    #[test]
    fn a_staged_narration_survives_a_failed_save_and_replaces_the_old_one() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        // The note already has a narration, recorded during a replay.
        let mut file = library.read(&saved.path).unwrap().file;
        file.audio = Some(narration_audio(300, NarrationBase::Replay, 0));
        let stamp = library.save(&saved.path, &file, &saved.stamp).unwrap();
        let backup = tempfile::tempdir().unwrap();
        let runtime = narration_runtime(backup.path());
        let audio = narration_audio(400, NarrationBase::Note, 1_232);
        let key = runtime.stage(&saved.path, audio.clone());

        // The note changed on disk meanwhile: a conflict, nothing written, the sound kept.
        let mut elsewhere = file.clone();
        elsewhere.note.title = "Changée ailleurs".into();
        let newer = library.save(&saved.path, &elsewhere, &stamp).unwrap();
        let before = std::fs::read(&saved.path).unwrap();
        let error = save_note_in(
            &library,
            &runtime,
            &saved.path,
            file.note.clone(),
            &stamp,
            None,
            Some(&key),
        )
        .unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        assert_eq!(std::fs::read(&saved.path).unwrap(), before);
        assert!(runtime.has_staged());
        assert_eq!(files_in(backup.path()), 1);

        // « Enregistrer une copie »: the copy gets the strokes and the sound.
        let copy = write_sample(&library, "", "Copie");
        save_note_in(
            &library,
            &runtime,
            &copy.path,
            file.note.clone(),
            &copy.stamp,
            None,
            Some(&key),
        )
        .unwrap();
        let copied = library.read(&copy.path).unwrap().file;
        assert_eq!(copied.audio.as_deref(), Some(&audio[..]));
        assert!(!runtime.has_staged());
        assert_eq!(files_in(backup.path()), 0);
        // The note itself kept its own narration.
        let kept = library.read(&saved.path).unwrap();
        assert_eq!(kept.stamp, newer);
        assert_eq!(kept.file.audio, elsewhere.audio);

        // A save at the right stamp replaces the old narration by the staged one.
        let again = narration_audio(200, NarrationBase::Note, 1_500);
        let key = runtime.stage(&saved.path, again.clone());
        save_note_in(
            &library,
            &runtime,
            &saved.path,
            kept.file.note.clone(),
            &newer,
            None,
            Some(&key),
        )
        .unwrap();
        let after = library.read(&saved.path).unwrap().file;
        assert_eq!(after.audio.as_deref(), Some(&again[..]));
        assert_eq!(after.note.title, "Changée ailleurs");
        let info = open_in(&library, &saved.path).unwrap().narration.unwrap();
        assert_eq!((info.base, info.start_ms), (BaseName::Note, 1_500));
    }

    #[test]
    fn a_damaged_narration_is_not_said_when_opening() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let mut file = library.read(&saved.path).unwrap().file;
        file.audio = Some(b"OggS but not really".to_vec());
        library.save(&saved.path, &file, &saved.stamp).unwrap();
        let opened = open_in(&library, &saved.path).unwrap();
        assert_eq!(opened.narration, None);
        assert_eq!(opened.note, file.note);
    }
}
