//! Split and merge commands (SPEC §4.4), over `ardoise_core::ops`.
//! Contract: `docs/design/exports-and-operations.md`.
//!
//! Both operations only ever add a note before they change anything: a split creates the new
//! note first and moves it to the trash again if the kept part cannot be saved, a merge leaves
//! its sources alone. Nothing is lost whichever step fails.

use std::path::{Path, PathBuf};

use ardoise_core::format::{BackgroundFile, NoteFile};
use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_core::note::{ImageRect, Note, PageSize};
use ardoise_core::ops::{self, NewNote, OpsError};
use ardoise_render::{NoteView, picture};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::background::{self, Chosen};
use crate::commands::{CommandError, CommandResult};
use crate::library::{NoteSummary, library_error, library_of, now_ms, summary_of};
use crate::notes::{blocking, save_in};
use crate::thumbnails::{file_thumbnail, thumbnail_view};

/// `SplitSpec` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SplitSpec {
    /// Everything written from `at` (ms since `created`) on goes to the new note.
    Time { at: u32 },
    /// Strokes with at least half of their samples inside the polygon (page coordinates).
    Zone { polygon: Vec<[f32; 2]> },
}

/// `MergeLayout` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeLayout {
    Overlay,
    EndToEnd,
}

impl From<MergeLayout> for ops::MergeLayout {
    fn from(layout: MergeLayout) -> Self {
        match layout {
            MergeLayout::Overlay => ops::MergeLayout::Overlay,
            MergeLayout::EndToEnd => ops::MergeLayout::EndToEnd,
        }
    }
}

/// `SplitResult` in `note.ts`: the kept part as saved over the note, and the new note.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitResult {
    pub note: Note,
    pub stamp: FileStamp,
    pub created: NoteSummary,
}

/// The error the interface translates (`errorCodes.*`).
fn ops_error(error: OpsError) -> CommandError {
    let code = match error {
        OpsError::NothingToSplit => "nothingToSplit",
        OpsError::NotEnough => "nothingToMerge",
        OpsError::PageTooLarge => "pageTooLarge",
    };
    CommandError::new(code, error)
}

/// The identity of a note made by an operation: a fresh id, the title as typed (trimmed), now.
fn new_note(title: &str) -> NewNote {
    NewNote {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.trim().to_owned(),
        now: now_ms(),
    }
}

/// The binder (relative to the library root) of the note file at `path`, which is checked to
/// be a note of the library.
fn binder_of(library: &Library, path: &Path) -> Result<PathBuf, LibraryError> {
    let absolute = library.note_path(path)?;
    Ok(absolute
        .parent()
        .and_then(|parent| parent.strip_prefix(library.root()).ok())
        .map(Path::to_path_buf)
        .unwrap_or_default())
}

/// Splits `note` (the editor's current content of the note at `path`, `stamp` being the version
/// of the file it was opened from). The new note goes into the same binder, titled `title`; the
/// kept part is then saved over `path` with the stamp check. If that save fails, the new note is
/// moved to the trash and the error is returned. Both notes keep the background image, with
/// the pixels `image` (staged by the editor), else those of the file (which must then show the
/// image the note describes: `background::check_keyless`).
pub fn split_in(
    library: &Library,
    path: &Path,
    note: Note,
    stamp: &FileStamp,
    split: &SplitSpec,
    title: &str,
    image: Option<Chosen>,
) -> CommandResult<SplitResult> {
    let binder = binder_of(library, path).map_err(library_error)?;
    // Refused early, before anything is created, if the file has changed since it was opened.
    let loaded = library.read(path).map_err(library_error)?;
    if loaded.stamp != *stamp {
        return Err(library_error(LibraryError::Conflict { path: loaded.path }));
    }
    if image.is_none() {
        background::check_keyless(&loaded.file.note, &note)?;
    }

    let new = new_note(title);
    let parts = match split {
        SplitSpec::Time { at } => ops::split_at_time(&note, *at, new),
        SplitSpec::Zone { polygon } => ops::split_by_zone(&note, polygon, new),
    }
    .map_err(ops_error)?;

    let mut extracted = NoteFile::new(parts.extracted);
    // A page background image or PDF belongs to the page, so it goes with the new note too;
    // a new note without an image (the editor removed it) gets no member.
    extracted.background = match (&extracted.note.background_image, &image) {
        (None, _) => None,
        (Some(_), Some(chosen)) => Some(chosen.staged.file.clone()),
        (Some(_), None) => loaded.file.background.clone(),
    };
    extracted.thumbnail = match &image {
        // The staged pixels are decoded already.
        Some(chosen) if extracted.note.background_image.is_some() => {
            thumbnail_view(NoteView::new(&extracted.note, chosen.picture.as_deref()))
        }
        _ => file_thumbnail(&extracted),
    };
    let created = library.create(&binder, &extracted).map_err(library_error)?;

    let kept = background::with_staged(image, || save_in(library, path, parts.kept, stamp));
    if let Err(error) = kept {
        // The new note is not wanted without the other half: take it back (best effort, it is
        // a complete note either way).
        let _ = library.trash(&created.path);
        return Err(library_error(error));
    }
    let saved = library.read(path).map_err(library_error)?;
    Ok(SplitResult {
        note: saved.file.note,
        stamp: saved.stamp,
        created: summary_of(library, &created.path, &extracted.note, created.stamp),
    })
}

/// Merges the notes at `paths`, in this order, into a new note titled `title` in the binder of
/// the first one. The sources are only read.
pub fn merge_in(
    library: &Library,
    paths: &[String],
    layout: MergeLayout,
    title: &str,
) -> CommandResult<NoteSummary> {
    let Some(first) = paths.first() else {
        return Err(ops_error(OpsError::NotEnough));
    };
    let binder = binder_of(library, Path::new(first)).map_err(library_error)?;
    let mut loaded = paths
        .iter()
        .map(|path| library.read(Path::new(path)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(library_error)?;
    let notes: Vec<Note> = loaded.iter().map(|l| l.file.note.clone()).collect();
    let merged = ops::merge(&notes, layout.into(), new_note(title)).map_err(ops_error)?;

    let mut file = NoteFile::new(merged);
    // The merged note takes the first note's background, image or PDF included (the others'
    // images are dropped: the dialog says so).
    let first = loaded.swap_remove(0).file;
    file.background = first.background;
    keep_on_first_page(&mut file, first.note.page);
    file.thumbnail = file_thumbnail(&file);
    let saved = library.create(&binder, &file).map_err(library_error)?;
    Ok(summary_of(library, &saved.path, &file.note, saved.stamp))
}

/// Cuts the merged note's background image to the first note's page (top left of the merged
/// page) when it reaches beyond it on a page that grew: it would otherwise cover another note.
/// An image that cannot be decoded or cut is left as it is.
fn keep_on_first_page(file: &mut NoteFile, first: PageSize) {
    let Some(image) = file.note.background_image.clone() else {
        return;
    };
    let area = ImageRect {
        x: 0.0,
        y: 0.0,
        width: first.width as f32,
        height: first.height as f32,
    };
    if file.note.page == first || picture::fits_in(&image, area) {
        return;
    }
    let Some(pixels) = background::picture_of(&file.note, file.background.as_ref()) else {
        return;
    };
    match picture::crop_to(&image, &pixels, area) {
        Ok(Some((cropped, stored))) => {
            file.note.background_image = Some(cropped);
            file.background = Some(BackgroundFile {
                extension: stored.extension().to_owned(),
                bytes: stored.bytes,
            });
        }
        Ok(None) => {
            file.note.background_image = None;
            file.background = None;
        }
        Err(_) => {}
    }
}

// --- Commands ---------------------------------------------------------------------------

#[tauri::command]
pub async fn split_note(
    context: State<'_, AppContext>,
    path: String,
    note: Note,
    stamp: FileStamp,
    split: SplitSpec,
    title: String,
    image_key: Option<String>,
) -> CommandResult<SplitResult> {
    let library = library_of(&context)?;
    // Reading, rendering the thumbnails and writing two notes: off the async runtime's threads.
    blocking(move || {
        let image =
            background::chosen_for_save(background::staging(), &note, image_key.as_deref())?;
        split_in(
            &library,
            Path::new(&path),
            note,
            &stamp,
            &split,
            &title,
            image,
        )
    })
    .await
}

#[tauri::command]
pub async fn merge_notes(
    context: State<'_, AppContext>,
    paths: Vec<String>,
    layout: MergeLayout,
    title: String,
) -> CommandResult<NoteSummary> {
    let library = library_of(&context)?;
    blocking(move || merge_in(&library, &paths, layout, &title)).await
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ardoise_core::format::{BackgroundFile, read_note};
    use ardoise_core::library::SavedNote;
    use ardoise_core::note::{A4_LANDSCAPE, Layer, Origin, PageSize};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    use super::*;
    use crate::library::tests::{library, sample_note, write_sample};
    use crate::library::{binder_path, list};

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn times(note: &Note) -> Vec<Vec<u32>> {
        note.strokes
            .iter()
            .map(|s| s.points.iter().map(|p| p.t).collect())
            .collect()
    }

    fn stroke_at(layer: u32, x: f32, t: u32) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 60.0,
            layer,
            points: vec![Point {
                x,
                y: 100.0,
                pressure: 0.5,
                t,
            }],
        }
    }

    /// Writes `note` (no thumbnail, which keeps huge pages cheap) into `binder`.
    fn write_plain(library: &Library, binder: &str, note: Note) -> SavedNote {
        library
            .create(&binder_path(binder), &NoteFile::new(note))
            .unwrap()
    }

    #[test]
    fn splits_at_an_instant_into_a_new_note_of_the_same_binder() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Cours");
        let opened = library.read(&saved.path).unwrap();
        // Samples are at 0, 8, 16... 232 ms.
        let result = split_in(
            &library,
            &saved.path,
            opened.file.note.clone(),
            &opened.stamp,
            &SplitSpec::Time { at: 120 },
            " Suite ",
            None,
        )
        .unwrap();

        // The kept part was saved over the note, which keeps its id and its file.
        assert_eq!(result.note.id, opened.file.note.id);
        assert_eq!(result.note.strokes[0].points.len(), 15);
        assert!(result.note.strokes[0].points.iter().all(|p| p.t < 120));
        assert_eq!(result.stamp, FileStamp::of(&saved.path).unwrap());
        assert_ne!(result.stamp, opened.stamp);
        let on_disk = read_note(&saved.path).unwrap();
        assert_eq!(on_disk.note, result.note);
        assert!(on_disk.thumbnail.is_some());

        // The new note: same binder, trimmed title, times from 0, `created` moved.
        let created = &result.created;
        assert_eq!(created.binder, "Classe");
        assert_eq!(created.title, "Suite");
        assert_ne!(created.path, text(&saved.path));
        assert!(created.file_name.ends_with(" Suite.ardoise"));
        assert_eq!(
            FileStamp::of(Path::new(&created.path)).unwrap(),
            created.stamp
        );
        let file = read_note(Path::new(&created.path)).unwrap();
        assert_eq!(file.note.id, created.id);
        assert_ne!(file.note.id, opened.file.note.id);
        assert_eq!(file.note.origin, Origin::Ardoise);
        assert_eq!(file.note.created, opened.file.note.created + 120);
        assert_eq!(file.note.strokes[0].points.len(), 15);
        assert_eq!(file.note.strokes[0].points[0].t, 0);
        assert_eq!(file.note.page, opened.file.note.page);
        assert!(file.thumbnail.is_some());
        assert_eq!(created.created, file.note.created);

        assert_eq!(list(&library).notes.len(), 2);
    }

    #[test]
    fn splits_the_editors_unsaved_content() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Cours");
        let opened = library.read(&saved.path).unwrap();
        let mut note = opened.file.note.clone();
        note.strokes.push(stroke_at(0, 50.0, 1000));
        note.strokes.push(stroke_at(0, 60.0, 2000));

        let result = split_in(
            &library,
            &saved.path,
            note,
            &opened.stamp,
            &SplitSpec::Time { at: 1500 },
            "Fin",
            None,
        )
        .unwrap();
        // The unsaved stroke at 1000 ms is in the kept part, the one at 2000 ms moved.
        assert_eq!(result.note.strokes.len(), 2);
        assert_eq!(result.note.strokes[1].points[0].t, 1000);
        let created = read_note(Path::new(&result.created.path)).unwrap();
        assert_eq!(times(&created.note), [vec![0]]);
        assert_eq!(created.note.created, opened.file.note.created + 2000);
    }

    #[test]
    fn splits_by_zone() {
        let (_dir, library) = library();
        let mut note = sample_note("Zones");
        note.strokes.push(stroke_at(0, 9000.0, 500));
        let saved = write_plain(&library, "Perso", note);
        let opened = library.read(&saved.path).unwrap();

        let polygon = vec![
            [8000.0, 0.0],
            [10_000.0, 0.0],
            [10_000.0, 500.0],
            [8000.0, 500.0],
        ];
        let result = split_in(
            &library,
            &saved.path,
            opened.file.note.clone(),
            &opened.stamp,
            &SplitSpec::Zone { polygon },
            "Zone",
            None,
        )
        .unwrap();
        assert_eq!(result.note.strokes.len(), 1);
        assert_eq!(result.note.strokes[0].points.len(), 30);
        let created = read_note(Path::new(&result.created.path)).unwrap();
        assert_eq!(created.note.strokes.len(), 1);
        // Positions and times untouched, `created` too.
        assert_eq!(created.note.strokes[0].points[0].t, 500);
        assert_eq!(created.note.created, opened.file.note.created);
        assert_eq!(result.created.binder, "Perso");
    }

    #[test]
    fn a_split_with_nothing_to_move_changes_nothing() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Cours");
        let opened = library.read(&saved.path).unwrap();
        let before = fs::read(&saved.path).unwrap();
        for split in [
            SplitSpec::Time { at: 0 },
            SplitSpec::Time { at: 10_000 },
            SplitSpec::Zone {
                polygon: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            },
            SplitSpec::Zone { polygon: vec![] },
        ] {
            let error = split_in(
                &library,
                &saved.path,
                opened.file.note.clone(),
                &opened.stamp,
                &split,
                "X",
                None,
            )
            .unwrap_err();
            assert_eq!(error.code(), "nothingToSplit", "{split:?}");
        }
        assert_eq!(fs::read(&saved.path).unwrap(), before);
        assert_eq!(list(&library).notes.len(), 1);
        assert!(library.trash_items().unwrap().is_empty());
    }

    #[test]
    fn a_stale_stamp_is_refused_before_anything_is_created() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Cours");
        let opened = library.read(&saved.path).unwrap();
        // Another computer saved the note meanwhile.
        library
            .set_tags(&saved.path, &["maths".into()], Some(&opened.stamp))
            .unwrap();
        let error = split_in(
            &library,
            &saved.path,
            opened.file.note.clone(),
            &opened.stamp,
            &SplitSpec::Time { at: 120 },
            "Suite",
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        assert_eq!(list(&library).notes.len(), 1);
        assert!(library.trash_items().unwrap().is_empty());
    }

    #[test]
    fn the_new_note_goes_to_the_trash_when_the_kept_part_cannot_be_saved() {
        let (_dir, library) = library();
        // The first half (kept) has a stroke on a layer the note does not have: it cannot be
        // written; the second half (the new note) is fine.
        let mut note = sample_note("Cassée");
        note.strokes = vec![stroke_at(9, 10.0, 10), stroke_at(0, 20.0, 500)];
        let saved = write_plain(&library, "Classe", sample_note("Cassée"));
        let before = fs::read(&saved.path).unwrap();
        let stamp = FileStamp::of(&saved.path).unwrap();

        let error = split_in(
            &library,
            &saved.path,
            note,
            &stamp,
            &SplitSpec::Time { at: 100 },
            "Suite",
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "noteInvalid");
        // The original is untouched, the new note was taken back.
        assert_eq!(fs::read(&saved.path).unwrap(), before);
        let listing = list(&library);
        assert_eq!(listing.notes.len(), 1);
        assert_eq!(listing.notes[0].path, text(&saved.path));
        let trashed = library.trash_items().unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].title.as_deref(), Some("Suite"));
    }

    #[test]
    fn only_notes_of_the_library_can_be_split() {
        let (_dir, library) = library();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("2026-10-24 X.ardoise");
        let stamp = FileStamp {
            modified: std::time::SystemTime::UNIX_EPOCH,
            len: 0,
        };
        let error = split_in(
            &library,
            &path,
            sample_note("X"),
            &stamp,
            &SplitSpec::Time { at: 10 },
            "Y",
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "notInLibrary");
        let missing = library.root().join("2026-10-24 Absente.ardoise");
        let error = split_in(
            &library,
            &missing,
            sample_note("X"),
            &stamp,
            &SplitSpec::Time { at: 10 },
            "Y",
            None,
        )
        .unwrap_err();
        assert_eq!(error.code(), "noteNotFound");
    }

    #[test]
    fn a_split_carries_the_background_file_to_the_new_note() {
        for described in [true, false] {
            let (_dir, library) = library();
            let mut note = sample_note("Fond");
            if described {
                note.background_image = Some(ardoise_core::note::BackgroundImage {
                    name: "fond.png".into(),
                    pdf_page: None,
                    rect: ImageRect {
                        x: 0.0,
                        y: 0.0,
                        width: note.page.width as f32,
                        height: note.page.height as f32,
                    },
                    quarter_turns: 0,
                    opacity: 1.0,
                });
            }
            let mut file = NoteFile::new(note);
            file.background = Some(BackgroundFile {
                extension: "png".to_owned(),
                bytes: b"not really a png".to_vec(),
            });
            let saved = library.create(Path::new(""), &file).unwrap();
            let opened = library.read(&saved.path).unwrap();
            let result = split_in(
                &library,
                &saved.path,
                opened.file.note.clone(),
                &opened.stamp,
                &SplitSpec::Time { at: 120 },
                "Suite",
                None,
            )
            .unwrap();
            let created = read_note(Path::new(&result.created.path)).unwrap();
            // The new note gets the member with the image; a member no image describes
            // (written by another version) is not copied into a note that has no image.
            let expected = if described {
                file.background.clone()
            } else {
                None
            };
            assert_eq!(created.background, expected);
            // The kept note still has its own.
            assert_eq!(read_note(&saved.path).unwrap().background, file.background);
        }
    }

    fn three_notes(library: &Library) -> Vec<String> {
        let mut a = sample_note("Un");
        a.tags = vec!["maths".into()];
        let mut b = sample_note("Deux");
        b.page = PageSize {
            width: 40_000,
            height: 10_000,
        };
        b.layers.push(Layer {
            name: "Figures".into(),
            visible: true,
        });
        let c = sample_note("Trois");
        [("Classe/3e", a), ("Autre", b), ("", c)]
            .into_iter()
            .map(|(binder, note)| text(&write_plain(library, binder, note).path))
            .collect()
    }

    #[test]
    fn merges_into_the_binder_of_the_first_note_and_leaves_the_sources_alone() {
        let (_dir, library) = library();
        let paths = three_notes(&library);
        let before: Vec<Vec<u8>> = paths.iter().map(|p| fs::read(p).unwrap()).collect();

        let summary = merge_in(&library, &paths, MergeLayout::Overlay, " Les trois ").unwrap();
        assert_eq!(summary.binder, "Classe/3e");
        assert_eq!(summary.title, "Les trois");
        assert!(summary.file_name.ends_with(" Les trois.ardoise"));
        assert_eq!(
            FileStamp::of(Path::new(&summary.path)).unwrap(),
            summary.stamp
        );

        let merged = read_note(Path::new(&summary.path)).unwrap();
        assert_eq!(merged.note.id, summary.id);
        assert_eq!(merged.note.origin, Origin::Ardoise);
        assert_eq!(merged.note.page.width, 40_000);
        assert_eq!(merged.note.layers.len(), 4);
        assert_eq!(merged.note.strokes.len(), 3);
        assert_eq!(merged.note.tags, ["maths"]);
        assert!(merged.thumbnail.is_some());
        // Sequential times: each note starts one second after the previous one ends.
        let starts: Vec<u32> = times(&merged.note).iter().map(|t| t[0]).collect();
        assert_eq!(starts, [0, 232 + 1000, 2 * (232 + 1000)]);

        for (path, bytes) in paths.iter().zip(&before) {
            assert_eq!(&fs::read(path).unwrap(), bytes);
        }
        assert_eq!(list(&library).notes.len(), 4);
    }

    #[test]
    fn merges_end_to_end() {
        let (_dir, library) = library();
        let paths = three_notes(&library);
        let summary = merge_in(&library, &paths, MergeLayout::EndToEnd, "Long").unwrap();
        let merged = read_note(Path::new(&summary.path)).unwrap().note;
        assert_eq!(merged.page.width, 40_000);
        assert_eq!(
            merged.page.height,
            A4_LANDSCAPE.height + 10_000 + A4_LANDSCAPE.height
        );
        assert_eq!(merged.layers.len(), 2);
    }

    #[test]
    fn merge_keeps_the_background_file_of_the_first_note() {
        let (_dir, library) = library();
        let mut file = NoteFile::new(sample_note("Fond"));
        file.background = Some(BackgroundFile {
            extension: "pdf".to_owned(),
            bytes: b"%PDF-1.7".to_vec(),
        });
        let first = library.create(Path::new(""), &file).unwrap();
        let second = write_sample(&library, "", "Autre");
        let summary = merge_in(
            &library,
            &[text(&first.path), text(&second.path)],
            MergeLayout::Overlay,
            "M",
        )
        .unwrap();
        let merged = read_note(Path::new(&summary.path)).unwrap();
        assert_eq!(merged.background, file.background);
    }

    #[test]
    fn merge_errors() {
        let (_dir, library) = library();
        let paths = three_notes(&library);

        for few in [&paths[..0], &paths[..1]] {
            let error = merge_in(&library, few, MergeLayout::Overlay, "M").unwrap_err();
            assert_eq!(error.code(), "nothingToMerge");
        }
        let mut with_missing = paths.clone();
        with_missing.push(text(&library.root().join("2026-10-24 Absente.ardoise")));
        let error = merge_in(&library, &with_missing, MergeLayout::Overlay, "M").unwrap_err();
        assert_eq!(error.code(), "noteNotFound");
        let mut outside = paths.clone();
        outside.push("/elsewhere/2026-10-24 X.ardoise".to_owned());
        let error = merge_in(&library, &outside, MergeLayout::Overlay, "M").unwrap_err();
        assert_eq!(error.code(), "notInLibrary");

        // Nothing was created by the failures.
        assert_eq!(list(&library).notes.len(), 3);

        let mut tall = sample_note("Haute");
        tall.page = PageSize {
            width: 1000,
            height: 6_000_000,
        };
        let a = write_plain(&library, "", tall.clone());
        let b = write_plain(&library, "", tall);
        let both = [text(&a.path), text(&b.path)];
        let error = merge_in(&library, &both, MergeLayout::EndToEnd, "M").unwrap_err();
        assert_eq!(error.code(), "pageTooLarge");
        assert!(merge_in(&library, &both, MergeLayout::Overlay, "M").is_ok());
    }

    #[test]
    fn arguments_come_in_the_shapes_of_the_interface() {
        let split: SplitSpec = serde_json::from_str(r#"{"kind":"time","at":1500}"#).unwrap();
        assert_eq!(split, SplitSpec::Time { at: 1500 });
        let split: SplitSpec =
            serde_json::from_str(r#"{"kind":"zone","polygon":[[0,0],[10.5,0],[10,20]]}"#).unwrap();
        assert_eq!(
            split,
            SplitSpec::Zone {
                polygon: vec![[0.0, 0.0], [10.5, 0.0], [10.0, 20.0]]
            }
        );
        let layouts: Vec<MergeLayout> = serde_json::from_str(r#"["overlay","endToEnd"]"#).unwrap();
        assert_eq!(layouts, [MergeLayout::Overlay, MergeLayout::EndToEnd]);
    }

    #[test]
    fn the_split_result_has_the_shape_of_the_interface() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Cours");
        let opened = library.read(&saved.path).unwrap();
        let result = split_in(
            &library,
            &saved.path,
            opened.file.note,
            &opened.stamp,
            &SplitSpec::Time { at: 120 },
            "Suite",
            None,
        )
        .unwrap();
        let json = serde_json::to_value(&result).unwrap();
        assert!(json["note"]["strokes"].is_array());
        assert!(json["stamp"]["len"].is_u64());
        for key in ["path", "binder", "fileName", "id", "title", "stamp"] {
            assert!(json["created"].get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn operation_errors_have_their_own_codes() {
        assert_eq!(ops_error(OpsError::NothingToSplit).code(), "nothingToSplit");
        assert_eq!(ops_error(OpsError::NotEnough).code(), "nothingToMerge");
        assert_eq!(ops_error(OpsError::PageTooLarge).code(), "pageTooLarge");
    }
}
