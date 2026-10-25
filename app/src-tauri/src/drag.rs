//! Dragging notes out of the library into other programs (SPEC §4.6): dropped on the Explorer,
//! the desktop, Word, OneNote or a mail, a note becomes a PNG named after it. Contract:
//! `docs/design/extras.md`, "Drag a note out".
//!
//! The drag is made of two commands, because Windows only gives a file to another program
//! through an OLE drag (`DoDragDrop`) that must start while the mouse button is held down:
//!
//! 1. [`prepare_note_drag`] renders the notes, with the code of the image export
//!    ([`image_bytes`], so whatever the export draws comes along), into a fresh folder of
//!    `%TEMP%\Ardoise\drag\` and returns the files. The interface calls it when the button goes
//!    down on the card's handle, so the rendering overlaps the user's first movement.
//! 2. [`start_note_drag`] runs the OLE drag over those files, on the main thread
//!    (`drag_windows.rs`), and answers when it ends. Outside Windows it fails with
//!    `dragUnavailable`.
//!
//! The drop target may read a file long after the drop, so nothing is deleted during or just
//! after the drag: folders older than [`MAX_AGE`] are removed at the start of the app and
//! every [`CLEAN_EVERY`] by a thread of its own. The files are never inside the library.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ardoise_core::library::Library;
use ardoise_render::{Image, decode_png};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::export::{
    ExportFormat, ExportOptions, ExportSource, PdfPageSize, check_dpi, export_error, image_bytes,
    library_for, load_sources, write_new_file,
};
use crate::library::path_text;

/// Error code when notes cannot be dragged out (not Windows, or the drag could not start).
pub const DRAG_UNAVAILABLE: &str = "dragUnavailable";
/// Resolutions tried in turn: the export default, then lower ones for a note so large that it
/// cannot be rendered at 150 dpi (`imageTooLarge`).
const DPI_STEPS: [u32; 3] = [150, 96, 72];
/// Drag folders older than this are removed (the drop target has long finished with them).
const MAX_AGE: Duration = Duration::from_secs(3600);
/// Interval of the clean-up thread.
const CLEAN_EVERY: Duration = Duration::from_secs(600);
/// Longest side of the picture that follows the pointer, in pixels.
#[cfg_attr(not(windows), allow(dead_code))]
pub const PREVIEW_SIDE: u32 = 128;

/// The folder holding the drag folders: `%TEMP%\Ardoise\drag`.
pub fn drag_root() -> PathBuf {
    std::env::temp_dir().join("Ardoise").join("drag")
}

/// What the drag ended with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DragOutcome {
    /// Dropped on a program that accepted the files.
    Dropped,
    /// Cancelled (Escape, released over nothing that accepts files, or never started).
    Cancelled,
}

/// PNG of `note` for the drag: the export's defaults (150 dpi, with the paper), or a lower
/// resolution when the note is too large for that.
fn png_for_drag(note: &ardoise_core::note::Note) -> CommandResult<Vec<u8>> {
    let mut last = None;
    for dpi in DPI_STEPS {
        let options = ExportOptions {
            dpi,
            background: true,
            quality: 90,
            pdf_page: PdfPageSize::Actual,
        };
        check_dpi(dpi)?;
        match image_bytes(note, ExportFormat::Png, &options) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if error.is_code(crate::export::IMAGE_TOO_LARGE) => last = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last.unwrap_or_else(|| export_error("cannot render the note")))
}

/// Renders `sources` as PNG files named after the notes into a new folder of `root` and
/// returns the files in the order of the sources. Two notes of the same name get ` (2)`. The
/// folder is removed if anything fails.
pub fn prepare_files(
    library: Option<&Library>,
    sources: Vec<ExportSource>,
    root: &Path,
) -> CommandResult<Vec<PathBuf>> {
    if sources.is_empty() {
        return Err(export_error("nothing to drag"));
    }
    let loaded = load_sources(library, sources)?;
    let dir = root.join(uuid::Uuid::new_v4().simple().to_string());
    let write_error =
        |error: std::io::Error| export_error(format!("cannot write in {}: {error}", dir.display()));
    fs::create_dir_all(&dir).map_err(write_error)?;
    let written = loaded
        .iter()
        .map(|source| {
            let bytes = png_for_drag(&source.note)?;
            write_new_file(&dir, &source.stem, "png", &bytes).map_err(write_error)
        })
        .collect::<CommandResult<Vec<PathBuf>>>();
    if written.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    written
}

/// Removes the drag folders of `root` last modified more than `max_age` before `now`; returns
/// how many. Files lying loose in `root` are left alone; a folder that cannot be removed (a
/// program still holds a file) stays for the next pass.
pub fn prune_old(root: &Path, now: SystemTime, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let Ok(meta) = entry.metadata() else {
                return false;
            };
            let age = meta
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok());
            meta.is_dir() && age.is_some_and(|age| age > max_age)
        })
        .filter(|entry| fs::remove_dir_all(entry.path()).is_ok())
        .count()
}

/// Cleans the drag folders now, then every [`CLEAN_EVERY`], on a thread of its own.
pub fn start_cleaning() {
    let spawned = std::thread::Builder::new()
        .name("drag-cleanup".into())
        .spawn(|| {
            loop {
                prune_old(&drag_root(), SystemTime::now(), MAX_AGE);
                std::thread::sleep(CLEAN_EVERY);
            }
        });
    // Without the thread the folders stay until the next start: not worth failing for.
    drop(spawned);
}

/// Whether `file` is a file of a drag folder of `root` (the interface may only start a drag over
/// files this module prepared, never over any file of the disk).
pub fn is_prepared_file(root: &Path, file: &Path) -> bool {
    let Ok(relative) = file.strip_prefix(root) else {
        return false;
    };
    let parts: Vec<_> = relative.components().collect();
    parts.len() == 2
        && parts
            .iter()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        && file.is_file()
}

/// The picture that follows the pointer: `image` reduced so that its long side is at most
/// `side` pixels, as premultiplied BGRA rows from top to bottom (what Windows' drag image wants),
/// with its size. `None` for an empty image.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn preview_bgra(image: &Image, side: u32) -> Option<(u32, u32, Vec<u8>)> {
    if image.width == 0 || image.height == 0 || side == 0 {
        return None;
    }
    let long = image.width.max(image.height);
    let (width, height) = if long <= side {
        (image.width, image.height)
    } else {
        let scale = f64::from(side) / f64::from(long);
        (
            ((f64::from(image.width) * scale).round() as u32).max(1),
            ((f64::from(image.height) * scale).round() as u32).max(1),
        )
    };
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        // Each output pixel averages the block of source pixels it covers.
        let y0 = (u64::from(y) * u64::from(image.height) / u64::from(height)) as u32;
        let y1 = ((u64::from(y + 1) * u64::from(image.height) / u64::from(height)) as u32)
            .clamp(y0 + 1, image.height);
        for x in 0..width {
            let x0 = (u64::from(x) * u64::from(image.width) / u64::from(width)) as u32;
            let x1 = ((u64::from(x + 1) * u64::from(image.width) / u64::from(width)) as u32)
                .clamp(x0 + 1, image.width);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    if let Some([pr, pg, pb, pa]) = image.pixel(sx, sy) {
                        // Averaged premultiplied, so that transparent pixels weigh nothing.
                        r += u64::from(pr) * u64::from(pa);
                        g += u64::from(pg) * u64::from(pa);
                        b += u64::from(pb) * u64::from(pa);
                        a += u64::from(pa);
                        n += 1;
                    }
                }
            }
            if n == 0 || a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                // r, g, b are sums of colour × alpha: dividing by 255 × n gives the premultiplied
                // average.
                out.push((b / (255 * n)) as u8);
                out.push((g / (255 * n)) as u8);
                out.push((r / (255 * n)) as u8);
                out.push((a / n) as u8);
            }
        }
    }
    Some((width, height, out))
}

/// The preview of the first PNG file of a drag, `None` when it cannot be read (the drag then
/// has no picture, which changes nothing else).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn preview_of_file(file: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let image = decode_png(&fs::read(file).ok()?).ok()?;
    preview_bgra(&image, PREVIEW_SIDE)
}

/// Renders the notes at `paths` as PNG files for a drag out of the app; returns the files, one
/// per note, named after the notes (see the module documentation).
#[tauri::command]
pub async fn prepare_note_drag(
    context: State<'_, AppContext>,
    paths: Vec<String>,
) -> CommandResult<Vec<String>> {
    let sources: Vec<ExportSource> = paths
        .into_iter()
        .map(|path| ExportSource {
            path,
            note: None,
            image_key: None,
        })
        .collect();
    let library = library_for(&context, &sources)?;
    // Several notes at 150 dpi take a while: off the async runtime's threads.
    let files = tauri::async_runtime::spawn_blocking(move || {
        prepare_files(library.as_ref(), sources, &drag_root())
    })
    .await
    .map_err(export_error)??;
    Ok(files.iter().map(|file| path_text(file)).collect())
}

/// Whether notes can be dragged out of the app (a native drag source exists: Windows).
#[tauri::command]
pub fn note_drag_supported() -> bool {
    cfg!(windows)
}

/// Starts the drag of `files` (from [`prepare_note_drag`]) with the pointer, which must still be
/// down, and answers when the drag ends.
#[tauri::command]
pub async fn start_note_drag(app: AppHandle, files: Vec<String>) -> CommandResult<DragOutcome> {
    let root = drag_root();
    let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    if files.is_empty() || !files.iter().all(|file| is_prepared_file(&root, file)) {
        return Err(CommandError::new(
            DRAG_UNAVAILABLE,
            "the files to drag were not prepared by the app",
        ));
    }
    run_native_drag(app, files).await
}

#[cfg(windows)]
async fn run_native_drag(app: AppHandle, files: Vec<PathBuf>) -> CommandResult<DragOutcome> {
    let (sender, receiver) = std::sync::mpsc::channel();
    // OLE drag and drop belongs to the thread that owns the window: the main one.
    app.run_on_main_thread(move || {
        let _ = sender.send(crate::drag_windows::drag_files(&files));
    })
    .map_err(|error| CommandError::new(DRAG_UNAVAILABLE, error))?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv())
        .await
        .map_err(|error| CommandError::new(DRAG_UNAVAILABLE, error))?
        .map_err(|error| CommandError::new(DRAG_UNAVAILABLE, error))?
        .map_err(|error| CommandError::new(DRAG_UNAVAILABLE, error))
}

#[cfg(not(windows))]
#[allow(clippy::unused_async)]
async fn run_native_drag(_app: AppHandle, _files: Vec<PathBuf>) -> CommandResult<DragOutcome> {
    Err(CommandError::new(
        DRAG_UNAVAILABLE,
        "dragging a note out of the app needs Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::{library, sample_note, write_sample};

    fn source(path: &Path) -> ExportSource {
        ExportSource {
            path: path_text(path),
            note: None,
            image_key: None,
        }
    }

    fn names(files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_note_becomes_a_png_named_after_its_file() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Cours: les <fractions>");
        let root = tempfile::tempdir().unwrap();
        let files = prepare_files(Some(&library), vec![source(&saved.path)], root.path()).unwrap();
        assert_eq!(files.len(), 1);
        let stem = saved
            .path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(names(&files), vec![format!("{stem}.png")]);
        assert!(!stem.contains(':') && !stem.contains('<'));
        // Inside one new folder of the root, and a real PNG.
        assert_eq!(files[0].parent().unwrap().parent().unwrap(), root.path());
        let image = decode_png(&fs::read(&files[0]).unwrap()).unwrap();
        assert!(image.width > 0 && image.height > 0);
        assert!(is_prepared_file(root.path(), &files[0]));
    }

    #[test]
    fn the_png_is_the_one_of_the_image_export() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let root = tempfile::tempdir().unwrap();
        let files = prepare_files(Some(&library), vec![source(&saved.path)], root.path()).unwrap();
        let note = library.read(&saved.path).unwrap().file.note;
        let options = ExportOptions {
            dpi: 150,
            background: true,
            quality: 90,
            pdf_page: PdfPageSize::Actual,
        };
        let expected = image_bytes(&note, ExportFormat::Png, &options).unwrap();
        assert_eq!(fs::read(&files[0]).unwrap(), expected);
    }

    #[test]
    fn several_notes_give_several_files_in_one_folder_in_order() {
        let (_dir, library) = library();
        let a = write_sample(&library, "", "Alpha");
        let b = write_sample(&library, "Classe", "Beta");
        let c = write_sample(&library, "", "Gamma");
        let root = tempfile::tempdir().unwrap();
        let files = prepare_files(
            Some(&library),
            vec![source(&c.path), source(&a.path), source(&b.path)],
            root.path(),
        )
        .unwrap();
        assert_eq!(files.len(), 3);
        assert!(names(&files)[0].ends_with("Gamma.png"));
        assert!(names(&files)[1].ends_with("Alpha.png"));
        assert!(names(&files)[2].ends_with("Beta.png"));
        assert!(files.iter().all(|f| f.parent() == files[0].parent()));
        assert!(files.iter().all(|f| f.is_file()));
    }

    #[test]
    fn notes_of_the_same_name_are_told_apart() {
        let (_dir, library) = library();
        let first = write_sample(&library, "Classe", "Pareil");
        let second = write_sample(&library, "Autre", "Pareil");
        // The same note twice and two binders' notes with one file name: no file is overwritten.
        let root = tempfile::tempdir().unwrap();
        let files = prepare_files(
            Some(&library),
            vec![
                source(&first.path),
                source(&second.path),
                source(&first.path),
            ],
            root.path(),
        )
        .unwrap();
        let names = names(&files);
        let unique: std::collections::BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), 3, "{names:?}");
        assert!(names.iter().any(|n| n.ends_with("Pareil (2).png")));
        assert!(files.iter().all(|f| f.is_file()));
    }

    #[test]
    fn a_note_given_with_its_content_is_not_read_from_the_library() {
        let root = tempfile::tempdir().unwrap();
        let files = prepare_files(
            None,
            vec![ExportSource {
                path: "C:\\Ardoise\\2026-10-24 Brouillon.ardoise".into(),
                note: Some(sample_note("Brouillon")),
                image_key: None,
            }],
            root.path(),
        )
        .unwrap();
        assert_eq!(names(&files), vec!["2026-10-24 Brouillon.png"]);
    }

    #[test]
    fn nothing_to_drag_and_unreadable_notes_fail_without_leaving_a_folder() {
        let (_dir, library) = library();
        let root = tempfile::tempdir().unwrap();
        let none = prepare_files(Some(&library), vec![], root.path()).unwrap_err();
        assert!(none.is_code(crate::export::EXPORT_FAILED));
        let missing = prepare_files(
            Some(&library),
            vec![source(&library.root().join("absent.ardoise"))],
            root.path(),
        );
        assert!(missing.is_err());
        let no_library =
            prepare_files(None, vec![source(Path::new("x.ardoise"))], root.path()).unwrap_err();
        assert!(no_library.is_code("noLibrary"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_failure_in_the_middle_removes_the_folder_and_its_files() {
        let (_dir, library) = library();
        let good = write_sample(&library, "", "Bonne");
        let root = tempfile::tempdir().unwrap();
        let result = prepare_files(
            Some(&library),
            vec![
                source(&good.path),
                source(&library.root().join("absent.ardoise")),
            ],
            root.path(),
        );
        // `load_sources` reads every note before the folder exists, so nothing is left either way.
        assert!(result.is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn old_folders_are_removed_and_recent_ones_and_loose_files_stay() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("a.png"), b"x").unwrap();
        let loose = root.path().join("loose.txt");
        fs::write(&loose, b"x").unwrap();
        let now = SystemTime::now();
        // Nothing is older than an hour yet.
        assert_eq!(prune_old(root.path(), now, MAX_AGE), 0);
        assert!(old.exists());
        // Two hours later the folder is old; the file lying in the root is not ours to remove.
        let later = now + Duration::from_secs(2 * 3600);
        assert_eq!(prune_old(root.path(), later, MAX_AGE), 1);
        assert!(!old.exists());
        assert!(loose.exists());
        // A missing root is fine.
        assert_eq!(prune_old(&root.path().join("none"), later, MAX_AGE), 0);
    }

    #[test]
    fn only_files_prepared_here_can_be_dragged() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("abc");
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("note.png");
        fs::write(&file, b"png").unwrap();
        assert!(is_prepared_file(root.path(), &file));
        // Missing, deeper, directly in the root, outside the root, or with `..`.
        assert!(!is_prepared_file(root.path(), &folder.join("absent.png")));
        fs::create_dir_all(folder.join("deeper")).unwrap();
        fs::write(folder.join("deeper").join("x.png"), b"x").unwrap();
        assert!(!is_prepared_file(
            root.path(),
            &folder.join("deeper").join("x.png")
        ));
        fs::write(root.path().join("loose.png"), b"x").unwrap();
        assert!(!is_prepared_file(
            root.path(),
            &root.path().join("loose.png")
        ));
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("a.png"), b"x").unwrap();
        assert!(!is_prepared_file(
            root.path(),
            &outside.path().join("a.png")
        ));
        assert!(!is_prepared_file(
            root.path(),
            &folder.join("..").join("loose.png")
        ));
    }

    fn solid(width: u32, height: u32, pixel: [u8; 4]) -> Image {
        Image {
            width,
            height,
            pixels: pixel.repeat((width * height) as usize),
        }
    }

    #[test]
    fn the_preview_is_reduced_and_premultiplied_bgra() {
        let (w, h, bytes) = preview_bgra(&solid(400, 200, [200, 100, 50, 255]), 128).unwrap();
        assert_eq!((w, h), (128, 64));
        assert_eq!(bytes.len(), 128 * 64 * 4);
        assert_eq!(&bytes[..4], &[50, 100, 200, 255]);
        // Half-transparent: colour multiplied by alpha.
        let (_, _, half) = preview_bgra(&solid(10, 10, [200, 100, 50, 128]), 128).unwrap();
        assert_eq!(&half[..4], &[25, 50, 100, 128]);
        // Fully transparent stays empty; small images are not enlarged; empty ones have none.
        let (w, h, clear) = preview_bgra(&solid(4, 2, [9, 9, 9, 0]), 128).unwrap();
        assert_eq!((w, h), (4, 2));
        assert!(clear.iter().all(|&b| b == 0));
        assert!(preview_bgra(&solid(0, 0, [0; 4]), 128).is_none());
    }

    #[test]
    fn the_preview_of_a_rendered_file_comes_out_with_the_page_proportions() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Aperçu");
        let root = tempfile::tempdir().unwrap();
        let files = prepare_files(Some(&library), vec![source(&saved.path)], root.path()).unwrap();
        let (w, h, bytes) = preview_of_file(&files[0]).unwrap();
        assert_eq!(w.max(h), PREVIEW_SIDE);
        assert_eq!(bytes.len(), (w * h * 4) as usize);
        assert!(preview_of_file(&root.path().join("absent.png")).is_none());
    }

    #[test]
    fn the_outcome_has_the_interface_spelling() {
        assert_eq!(
            serde_json::to_value(DragOutcome::Dropped).unwrap(),
            serde_json::json!("dropped")
        );
        assert_eq!(
            serde_json::to_value(DragOutcome::Cancelled).unwrap(),
            serde_json::json!("cancelled")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn dragging_is_not_supported_outside_windows() {
        assert!(!note_drag_supported());
    }
}
