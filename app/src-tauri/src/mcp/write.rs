//! The app's side of the MCP write tools (`ardoise_mcp::WriteServices`): every method is a call
//! of the code the interface's commands use, so that a tool gives exactly what the same action
//! gives in the app (thumbnails, the original of a tablet's note, background images, narration,
//! exports, the handwriting cache). The tools themselves, their arguments and the choice of
//! free file names are in the crate; the translations of the types are all that is here.
//!
//! When the app has the note open while a tool writes it, nothing special is done: the editor
//! sees the file change like a Nextcloud change (its banner offers to reload the note), and a
//! save from the editor is refused with `noteConflict` instead of overwriting the tool's write
//! (`Library::save` checks the file's stamp).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ardoise_core::format::NoteFile;
use ardoise_core::fsutil::write_atomic_new;
use ardoise_core::library::{FileStamp, Library, SavedNote};
use ardoise_core::note::Note;
use ardoise_hwr::Language;
use ardoise_hwr::document::{Document, Labels};
use ardoise_hwr::docx::to_docx;
use ardoise_mcp::ToolError;
use ardoise_mcp::write::{
    ExportFormat, ExportKind, ExportOutcome, ExportRequest, ExportTarget, MergeLayout, PdfPage,
    SplitRequest, VideoResolution, VideoTiming, WriteServices,
};
use ardoise_media::OpenH264Codec;
use ardoise_render::{NoteView, PdfOptions, notes_to_pdf};
use serde_json::json;

use super::AppHandwriting;
use crate::AppContext;
use crate::background;
use crate::codec::{codec_dir, openh264_allowed};
use crate::commands::CommandError;
use crate::export::{self, ExportSource, LoadedSource, load_sources, write_new_file};
use crate::handwriting::{CacheDir, NoteText, handwriting_dir, hwr_error, text_document};
use crate::library::path_text;
use crate::notes::save_with_key;
use crate::operations::{self, SplitSpec, merge_in, split_in};
use crate::textdoc::{document_of, write_markdown};
use crate::thumbnails::thumbnail_png;
use crate::video::{self, VideoExportOptions, export_video_in};

/// An export is not cancellable from the assistant's side: the flag is never set.
static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// The services of a server that has the app's code at hand.
pub(super) struct AppWrite {
    context: Arc<AppContext>,
    handwriting: Arc<AppHandwriting>,
    /// The handwriting cache of this PC, where the corrected transcriptions go.
    cache: CacheDir,
}

impl AppWrite {
    pub(super) fn new(context: Arc<AppContext>, handwriting: Arc<AppHandwriting>) -> AppWrite {
        AppWrite::with_cache(context, handwriting, CacheDir::new(handwriting_dir()))
    }

    fn with_cache(
        context: Arc<AppContext>,
        handwriting: Arc<AppHandwriting>,
        cache: CacheDir,
    ) -> AppWrite {
        AppWrite {
            context,
            handwriting,
            cache,
        }
    }
}

/// The error of the interface's commands as a tool error (same code, same detail).
fn tool_error(error: CommandError) -> ToolError {
    let (code, message) = error.parts();
    ToolError::new(code, message)
}

fn export_failed(message: impl ToString) -> ToolError {
    ToolError::new(export::EXPORT_FAILED, message)
}

/// The sources of an export: the notes at `paths`, read from the library.
fn sources_of(library: &Library, paths: &[PathBuf]) -> Result<Vec<LoadedSource>, ToolError> {
    let sources = paths
        .iter()
        .map(|path| ExportSource {
            path: path_text(path),
            note: None,
            image_key: None,
        })
        .collect();
    load_sources(Some(library), sources).map_err(tool_error)
}

/// The words of the md and docx files, in `language` (`handwriting.structured.labels`).
fn labels(language: Language) -> Labels {
    match language {
        Language::Fr => Labels {
            date: "Créée le :".to_owned(),
            tags: "Étiquettes :".to_owned(),
            drawing: "Dessin".to_owned(),
        },
        Language::En => Labels {
            date: "Created:".to_owned(),
            tags: "Tags:".to_owned(),
            drawing: "Drawing".to_owned(),
        },
    }
}

/// The directory and the name (without extension) of a file target.
fn split_target(path: &Path) -> (&Path, String) {
    (
        path.parent().unwrap_or(Path::new("")),
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

/// Writes `bytes` to `target`, or to the next free name if it was taken meanwhile; never
/// replaces a file.
fn write_target(target: &Path, bytes: &[u8]) -> Result<PathBuf, ToolError> {
    match write_atomic_new(target, bytes) {
        Ok(_) => Ok(target.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let (dir, stem) = split_target(target);
            let extension = target
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            write_new_file(dir, &stem, &extension, bytes)
                .map_err(|error| export_failed(format!("{}: {error}", target.display())))
        }
        Err(error) => Err(export_failed(format!("{}: {error}", target.display()))),
    }
}

fn image_options(request: &ExportRequest) -> export::ExportOptions {
    let options = &request.options;
    export::ExportOptions {
        dpi: options.dpi,
        background: options.background,
        quality: options.quality,
        pdf_page: match options.pdf_page {
            PdfPage::Actual => export::PdfPageSize::Actual,
            PdfPage::A4 => export::PdfPageSize::A4,
        },
    }
}

fn image_format(format: ExportFormat) -> export::ExportFormat {
    match format {
        ExportFormat::Jpg => export::ExportFormat::Jpg,
        ExportFormat::Webp => export::ExportFormat::Webp,
        ExportFormat::Svg => export::ExportFormat::Svg,
        ExportFormat::Pdf => export::ExportFormat::Pdf,
        _ => export::ExportFormat::Png,
    }
}

/// The video options of `request`, in the interface's terms (the app validates them).
fn video_options(request: &ExportRequest) -> VideoExportOptions {
    let options = &request.options;
    let video = &options.video;
    VideoExportOptions {
        format: match request.format {
            ExportFormat::Gif => video::VideoFormat::Gif,
            ExportFormat::Webm => video::VideoFormat::Webm,
            _ => video::VideoFormat::Mp4,
        },
        resolution: match video.resolution {
            VideoResolution::Hd => video::VideoResolution::Hd,
            VideoResolution::FullHd => video::VideoResolution::FullHd,
            VideoResolution::Uhd => video::VideoResolution::Uhd,
            VideoResolution::Page => video::VideoResolution::Page,
        },
        frame_rate: f64::from(video.frame_rate),
        timing: match video.timing {
            VideoTiming::Speed(speed) => video::VideoTiming::Speed { speed },
            VideoTiming::Duration(seconds) => video::VideoTiming::Duration { seconds },
        },
        trim_pauses: video.trim_pauses,
        background: options.background,
        hold_end_seconds: video.hold_end_seconds,
        narration: video.narration,
    }
}

impl AppWrite {
    /// One image per note (a file for one, a folder for several), or one PDF of all the notes.
    fn export_images(
        &self,
        library: &Library,
        request: &ExportRequest,
    ) -> Result<ExportOutcome, ToolError> {
        let format = image_format(request.format);
        let options = image_options(request);
        let sources = sources_of(library, &request.notes)?;
        let files = match &request.target {
            ExportTarget::Folder(folder) => {
                export::export(&sources, format, &options, folder, &NOT_CANCELLED)
                    .map_err(tool_error)?
            }
            ExportTarget::File(target) => {
                let bytes = if format == export::ExportFormat::Pdf {
                    let views: Vec<NoteView<'_>> = sources.iter().map(LoadedSource::view).collect();
                    notes_to_pdf(
                        &views,
                        &PdfOptions {
                            page: options.pdf_page.into(),
                            background: options.background,
                        },
                    )
                    .map_err(|error| tool_error(export::render_error(error)))?
                } else {
                    export::image_bytes(sources[0].view(), format, &options).map_err(tool_error)?
                };
                vec![write_target(target, &bytes)?]
            }
        };
        Ok(ExportOutcome {
            files,
            details: json!({}),
        })
    }

    /// The recognised text, with or without its structure, of the notes in one file.
    fn export_text(
        &self,
        library: &Library,
        request: &ExportRequest,
    ) -> Result<ExportOutcome, ToolError> {
        let ExportTarget::File(target) = &request.target else {
            return Err(export_failed("a text export is one file"));
        };
        let sources = sources_of(library, &request.notes)?;
        let indexer = self.handwriting.indexer();
        let mut recognitions = Vec::new();
        for source in &sources {
            recognitions.push(indexer.recognition_of(&source.note).map_err(|error| {
                let error = hwr_error(error);
                tool_error(error)
            })?);
        }
        if request.format == ExportFormat::Txt {
            let notes: Vec<NoteText> = sources
                .iter()
                .zip(&recognitions)
                .map(|(source, recognition)| NoteText {
                    path: String::new(),
                    title: match source.note.title.trim() {
                        "" => source.stem.clone(),
                        title => title.to_owned(),
                    },
                    text: recognition.text(),
                })
                .collect();
            let file = write_target(target, text_document(&notes).as_bytes())?;
            return Ok(ExportOutcome {
                files: vec![file],
                details: json!({}),
            });
        }
        let engine = indexer.on_demand();
        let mut documents: Vec<Document> = Vec::new();
        let mut analyzed = true;
        for (source, recognition) in sources.iter().zip(&recognitions) {
            let (document, by_analysis) =
                document_of(source, recognition, &*engine).map_err(tool_error)?;
            analyzed &= by_analysis;
            documents.push(document);
        }
        let labels = labels(request.options.language);
        let files = if request.format == ExportFormat::Md {
            write_markdown(target, &documents, &labels).map_err(tool_error)?
        } else {
            let bytes = to_docx(&documents, &labels).map_err(export_failed)?;
            vec![write_target(target, &bytes)?]
        };
        Ok(ExportOutcome {
            files,
            details: json!({ "layout_analysed": analyzed }),
        })
    }

    /// The replay of one note (gif, webm, mp4).
    fn export_video(
        &self,
        library: &Library,
        request: &ExportRequest,
    ) -> Result<ExportOutcome, ToolError> {
        let ExportTarget::File(target) = &request.target else {
            return Err(export_failed("a replay is one file"));
        };
        let options = video_options(request);
        let source = ExportSource {
            path: path_text(&request.notes[0]),
            note: None,
            image_key: None,
        };
        let openh264 = (options.format == video::VideoFormat::Mp4
            && openh264_allowed(&self.context))
        .then(|| OpenH264Codec::new(codec_dir()));
        let result = export_video_in(
            Some(library),
            source,
            &options,
            target,
            openh264.as_ref(),
            // No progress notification: the call answers when the file is complete.
            &mut |_| {},
            &NOT_CANCELLED,
        )
        .map_err(tool_error)?;
        Ok(ExportOutcome {
            files: vec![PathBuf::from(result.path)],
            details: json!({
                "frames": result.frames,
                "duration_ms": result.duration_ms,
                "audio": result.audio,
                "encoder": result.encoder,
            }),
        })
    }
}

impl WriteServices for AppWrite {
    fn create_blank(
        &self,
        library: &Library,
        binder: &Path,
        note: Note,
    ) -> Result<SavedNote, ToolError> {
        let mut file = NoteFile::new(note);
        file.thumbnail = thumbnail_png(&file.note);
        Ok(library.create(binder, &file)?)
    }

    fn create_from_file(
        &self,
        library: &Library,
        binder: &Path,
        file: &Path,
        pdf_page: u32,
        layer_name: &str,
    ) -> Result<SavedNote, ToolError> {
        let binder = path_text(binder);
        let made = background::notes_from_file_with_progress(
            library,
            &binder,
            file,
            Some(&[pdf_page]),
            layer_name,
            &mut |_| {},
        )
        .map_err(tool_error)?;
        made.into_iter()
            .next()
            .map(|summary| SavedNote {
                path: PathBuf::from(summary.path),
                stamp: summary.stamp,
            })
            .ok_or_else(|| ToolError::new("backgroundDamaged", "the file gave no note"))
    }

    fn save_note(
        &self,
        library: &Library,
        path: &Path,
        note: Note,
        stamp: &FileStamp,
    ) -> Result<FileStamp, ToolError> {
        // The editor's own save, with the file's background image kept as it is.
        save_with_key(library, path, note, stamp, None).map_err(tool_error)
    }

    fn split_note(
        &self,
        library: &Library,
        path: &Path,
        split: &SplitRequest,
        title: &str,
    ) -> Result<PathBuf, ToolError> {
        let loaded = library.read(path)?;
        let spec = match split {
            SplitRequest::AtMs(at) => SplitSpec::Time { at: *at },
            SplitRequest::Polygon(polygon) => SplitSpec::Zone {
                polygon: polygon.clone(),
            },
        };
        let result = split_in(
            library,
            path,
            loaded.file.note.clone(),
            &loaded.stamp,
            &spec,
            title,
            None,
        )
        .map_err(tool_error)?;
        Ok(PathBuf::from(result.created.path))
    }

    fn merge_notes(
        &self,
        library: &Library,
        paths: &[PathBuf],
        layout: MergeLayout,
        title: &str,
    ) -> Result<PathBuf, ToolError> {
        let paths: Vec<String> = paths.iter().map(|path| path_text(path)).collect();
        let layout = match layout {
            MergeLayout::Overlay => operations::MergeLayout::Overlay,
            MergeLayout::EndToEnd => operations::MergeLayout::EndToEnd,
        };
        let merged = merge_in(library, &paths, layout, title).map_err(tool_error)?;
        Ok(PathBuf::from(merged.path))
    }

    fn export(
        &self,
        library: &Library,
        request: &ExportRequest,
    ) -> Result<ExportOutcome, ToolError> {
        match request.format.kind() {
            ExportKind::Image | ExportKind::Pdf => self.export_images(library, request),
            ExportKind::Text => self.export_text(library, request),
            ExportKind::Video => self.export_video(library, request),
        }
    }

    fn set_note_text(&self, note: &Note, text: &str, language: Language) -> Result<(), ToolError> {
        self.cache
            .set_correction_for(note, language, text)
            .map_err(|error| {
                ToolError::new(
                    "handwritingFailed",
                    format!("cannot store the text: {error}"),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::OnceLock;

    use ardoise_core::format::read_note;
    use ardoise_mcp::write::{ExportOptions, VideoOptions};
    use ardoise_render::{Image, encode_png};

    use super::*;
    use crate::library::tests::{context, library, sample_note, write_sample};

    /// The services on a temporary settings folder and handwriting cache.
    fn services() -> (AppWrite, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let context = Arc::new(context(dir.path()));
        let handwriting = Arc::new(AppHandwriting {
            context: Arc::clone(&context),
            indexer: OnceLock::new(),
        });
        let cache = CacheDir::new(dir.path().join("handwriting"));
        (AppWrite::with_cache(context, handwriting, cache), dir)
    }

    fn options() -> ExportOptions {
        ExportOptions {
            dpi: 72,
            background: true,
            quality: 80,
            pdf_page: PdfPage::Actual,
            language: Language::En,
            video: VideoOptions {
                resolution: VideoResolution::Hd,
                frame_rate: 24,
                timing: VideoTiming::Duration(5.0),
                trim_pauses: true,
                hold_end_seconds: 2.0,
                narration: false,
            },
        }
    }

    fn request(notes: Vec<PathBuf>, format: ExportFormat, target: ExportTarget) -> ExportRequest {
        ExportRequest {
            notes,
            format,
            target,
            options: options(),
        }
    }

    #[test]
    fn a_blank_note_gets_its_thumbnail_and_never_replaces_a_note() {
        let (services, _dir) = services();
        let (_library_dir, library) = library();
        let note = sample_note("Séance");
        let first = services
            .create_blank(&library, Path::new("Classe"), note.clone())
            .unwrap();
        let second = services
            .create_blank(&library, Path::new("Classe"), note)
            .unwrap();
        assert_ne!(first.path, second.path);
        assert!(second.path.to_string_lossy().ends_with("(2).ardoise"));
        let file = read_note(&first.path).unwrap();
        assert!(file.thumbnail.is_some());
    }

    #[test]
    fn a_note_is_saved_like_the_editor_saves_it_with_the_stamp_check() {
        let (services, _dir) = services();
        let (_library_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Cours");
        let opened = library.read(&saved.path).unwrap();
        let mut edited = opened.file.note.clone();
        edited.strokes.clear();

        let stamp = services
            .save_note(&library, &saved.path, edited.clone(), &opened.stamp)
            .unwrap();
        let after = library.read(&saved.path).unwrap();
        assert_eq!(after.stamp, stamp);
        assert!(after.file.note.strokes.is_empty());
        assert!(
            after.file.thumbnail.is_some(),
            "the thumbnail is made again"
        );
        // The first change of the ink keeps the version on disk, as the editor does.
        assert!(after.file.original_note.is_some());

        // The stamp of the version that was read before is refused, the file is left alone.
        let bytes = fs::read(&saved.path).unwrap();
        let error = services
            .save_note(&library, &saved.path, edited, &opened.stamp)
            .unwrap_err();
        assert_eq!(error.code, "noteConflict");
        assert_eq!(fs::read(&saved.path).unwrap(), bytes);
    }

    #[test]
    fn splitting_and_merging_are_the_apps_operations() {
        let (services, _dir) = services();
        let (_library_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Cours");
        // Samples are at 0, 8, 16... 232 ms.
        let created = services
            .split_note(&library, &saved.path, &SplitRequest::AtMs(120), "Suite")
            .unwrap();
        assert_eq!(created.parent(), saved.path.parent());
        let new = read_note(&created).unwrap();
        assert_eq!(new.note.title, "Suite");
        assert!(new.thumbnail.is_some());
        assert_eq!(
            read_note(&saved.path).unwrap().note.strokes[0].points.len(),
            15
        );
        let nothing = services
            .split_note(&library, &saved.path, &SplitRequest::AtMs(5_000_000), "X")
            .unwrap_err();
        assert_eq!(nothing.code, "nothingToSplit");

        let merged = services
            .merge_notes(
                &library,
                &[saved.path.clone(), created.clone()],
                MergeLayout::EndToEnd,
                "Tout",
            )
            .unwrap();
        assert_eq!(merged.parent(), saved.path.parent());
        let file = read_note(&merged).unwrap();
        assert_eq!(file.note.title, "Tout");
        assert_eq!(file.note.strokes.len(), 2);
        let one = services
            .merge_notes(&library, &[saved.path], MergeLayout::Overlay, "X")
            .unwrap_err();
        assert_eq!(one.code, "nothingToMerge");
    }

    #[test]
    fn a_note_is_made_on_an_image_like_the_apps_import() {
        let (services, dir) = services();
        let (_library_dir, library) = library();
        let image = Image {
            width: 40,
            height: 20,
            pixels: vec![200; 40 * 20 * 4],
        };
        let png = dir.path().join("schema.png");
        fs::write(&png, encode_png(&image).unwrap()).unwrap();
        let saved = services
            .create_from_file(&library, Path::new("Classe"), &png, 1, "Layer 1")
            .unwrap();
        let file = read_note(&saved.path).unwrap();
        assert_eq!(file.note.title, "schema");
        assert_eq!(file.note.layers[0].name, "Layer 1");
        let image = file
            .note
            .background_image
            .expect("the image is the background");
        assert_eq!(image.name, "schema.png");
        assert!(file.background.is_some());
        // A file that is neither an image nor a PDF: a coded error, no note.
        let text = dir.path().join("a.txt");
        fs::write(&text, b"hello").unwrap();
        let count = library.list().notes.len();
        assert!(
            services
                .create_from_file(&library, Path::new("Classe"), &text, 1, "Layer 1")
                .is_err()
        );
        assert_eq!(library.list().notes.len(), count);
    }

    #[test]
    fn images_and_pdfs_are_written_without_replacing_a_file() {
        let (services, dir) = services();
        let (_library_dir, library) = library();
        let first = write_sample(&library, "Classe", "Optique").path;
        let second = write_sample(&library, "Classe", "Chimie").path;
        let out = dir.path().join("out");
        fs::create_dir(&out).unwrap();

        // A file target that was taken meanwhile: the next free name, the other file kept.
        let target = out.join("optique.png");
        fs::write(&target, b"precious").unwrap();
        let outcome = services
            .export(
                &library,
                &request(
                    vec![first.clone()],
                    ExportFormat::Png,
                    ExportTarget::File(target.clone()),
                ),
            )
            .unwrap();
        assert_eq!(outcome.files, [out.join("optique (2).png")]);
        assert_eq!(fs::read(&target).unwrap(), b"precious");
        assert!(fs::read(&outcome.files[0]).unwrap().starts_with(b"\x89PNG"));

        // Several images in a folder, twice: the notes' file names, then " (2)".
        let folder = out.join("jour");
        for round in 0..2 {
            let outcome = services
                .export(
                    &library,
                    &request(
                        vec![first.clone(), second.clone()],
                        ExportFormat::Svg,
                        ExportTarget::Folder(folder.clone()),
                    ),
                )
                .unwrap();
            assert_eq!(outcome.files.len(), 2);
            let suffix = if round == 0 { "" } else { " (2)" };
            assert!(
                outcome.files[0]
                    .to_string_lossy()
                    .ends_with(&format!("Optique{suffix}.svg")),
                "{:?}",
                outcome.files
            );
        }
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 4);

        // One PDF for all the notes.
        let pdf = out.join("classe.pdf");
        let outcome = services
            .export(
                &library,
                &request(
                    vec![first, second],
                    ExportFormat::Pdf,
                    ExportTarget::File(pdf.clone()),
                ),
            )
            .unwrap();
        assert_eq!(outcome.files, [pdf.clone()]);
        assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF"));

        // A note that is not in the library: a coded error, nothing written.
        let missing = request(
            vec![out.join("none.ardoise")],
            ExportFormat::Png,
            ExportTarget::File(out.join("z.png")),
        );
        assert!(services.export(&library, &missing).is_err());
        assert!(!out.join("z.png").exists());
    }

    #[test]
    fn the_replay_options_become_the_apps_video_options() {
        let note = PathBuf::from("x.ardoise");
        let target = ExportTarget::File(PathBuf::from("x.webm"));
        let video = video_options(&request(
            vec![note.clone()],
            ExportFormat::Webm,
            target.clone(),
        ));
        assert_eq!(video.format, video::VideoFormat::Webm);
        assert_eq!(video.resolution, video::VideoResolution::Hd);
        assert_eq!(video.frame_rate, 24.0);
        assert_eq!(video.timing, video::VideoTiming::Duration { seconds: 5.0 });
        assert!(video.trim_pauses && video.background && !video.narration);
        assert_eq!(video.hold_end_seconds, 2.0);
        // What the tool sends is what the app's own validation accepts.
        assert!(video::check_options(&video).is_ok());
        for (format, expected) in [
            (ExportFormat::Gif, video::VideoFormat::Gif),
            (ExportFormat::Mp4, video::VideoFormat::Mp4),
        ] {
            let options = video_options(&request(vec![note.clone()], format, target.clone()));
            assert_eq!(options.format, expected);
        }
        let image = image_options(&request(vec![note], ExportFormat::Jpg, target));
        assert_eq!((image.dpi, image.quality, image.background), (72, 80, true));
    }

    #[test]
    fn the_words_of_the_text_files_follow_the_language() {
        assert_eq!(labels(Language::Fr).drawing, "Dessin");
        assert_eq!(labels(Language::En).drawing, "Drawing");
        assert_eq!(labels(Language::Fr).date, "Créée le :");
    }

    #[test]
    fn a_corrected_text_goes_to_the_handwriting_cache_only() {
        let (services, dir) = services();
        let (_library_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Cours");
        let before = fs::read(&saved.path).unwrap();
        let note = library.read(&saved.path).unwrap().file.note;

        services
            .set_note_text(&note, "la réfraction", Language::Fr)
            .unwrap();
        let cache = CacheDir::new(dir.path().join("handwriting"));
        assert_eq!(cache.correction_for(&note).unwrap().text, "la réfraction");
        assert_eq!(
            fs::read(&saved.path).unwrap(),
            before,
            "the note is untouched"
        );
        services.set_note_text(&note, "", Language::Fr).unwrap();
        assert!(cache.correction_for(&note).is_none());
    }

    #[test]
    fn command_errors_keep_their_code_and_detail() {
        let error = tool_error(CommandError::new("noteConflict", "changed elsewhere"));
        assert_eq!(
            (error.code, error.message.as_str()),
            ("noteConflict", "changed elsewhere")
        );
    }
}
