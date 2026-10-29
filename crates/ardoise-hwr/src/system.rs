//! Windows' own handwriting recognition (`Windows.UI.Input.Inking`), on a worker thread of its
//! own; "unsupported" elsewhere.
//!
//! `InkRecognizerContainer` is not agile: it is created and used on one thread. That thread
//! joins the multithreaded apartment, so that blocking on the asynchronous recognition (WinRT's
//! `.get()`, `join()` in the `windows` crate) cannot deadlock. Recognition is never on the
//! interface's thread.
//!
//! [`SystemRecognizer::background`] runs its worker at background priority. That thread shares
//! no lock with the rest of the app, so lowering it cannot make anyone wait (Windows' locks
//! have no priority inheritance). It only covers part of the work, though: measured on
//! Windows 11 (`docs/design/extras.md`, "Handwriting recognition"), `RecognizeAsync` does most
//! of its CPU work on a thread of the WinRT thread pool and on the engine's own inference
//! threads, at normal priority. What keeps recognition away from live mode is that the indexer
//! starts none while live mode or the presentation runs. Cancelling a recognition under way
//! (`IAsyncInfo::Cancel`) is not used: it answers within 100 to 250 ms, but releasing the
//! container afterwards crashed the process (access violation) in the same measurement.

use ardoise_ink::Stroke;

use crate::ink::Scale;
use crate::{HwrError, Language, Recognition, Recognizer, RecognizerInfo};

/// Lowers the priority of the calling thread (CPU and disk) for background work: only for a
/// thread that takes no lock shared with other threads (a starved thread holding one would
/// hold up everyone waiting for it). Does nothing outside Windows.
#[cfg_attr(not(windows), allow(dead_code))]
fn lower_thread_priority() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: the pseudo-handle of the current thread is always valid for this call. A
        // failure only leaves the normal priority.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
    }
}

#[cfg(windows)]
pub use windows_impl::SystemRecognizer;

#[cfg(not(windows))]
pub use unsupported::SystemRecognizer;

#[cfg(not(windows))]
mod unsupported {
    use super::*;

    /// Outside Windows there is no engine: every call answers [`HwrError::Unsupported`].
    #[derive(Debug, Default)]
    pub struct SystemRecognizer;

    impl SystemRecognizer {
        pub fn new() -> SystemRecognizer {
            SystemRecognizer
        }

        pub fn background() -> SystemRecognizer {
            SystemRecognizer
        }

        pub fn with_scale(_scale: Scale) -> SystemRecognizer {
            SystemRecognizer
        }
    }

    impl Recognizer for SystemRecognizer {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            Err(HwrError::Unsupported)
        }

        fn recognize(&self, _: &[Stroke], _: Language) -> Result<Recognition, HwrError> {
            Err(HwrError::Unsupported)
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Receiver, Sender};

    use windows::Foundation::Point;
    use windows::UI::Input::Inking::Analysis::{
        IInkAnalysisNode, InkAnalysisLine, InkAnalysisListItem, InkAnalysisNodeKind,
        InkAnalysisParagraph, InkAnalyzer,
    };
    use windows::UI::Input::Inking::{
        InkPoint, InkRecognitionTarget, InkRecognizer, InkRecognizerContainer, InkStrokeBuilder,
        InkStrokeContainer,
    };
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::Interface;
    use windows_collections::IIterable;
    use windows_numerics::Matrix3x2;

    use super::*;
    use crate::lines::group_lines;
    use crate::model::Word;
    use crate::structure::{Region, RegionKind};

    enum Request {
        List(Sender<Result<Vec<RecognizerInfo>, HwrError>>),
        Recognize {
            strokes: Vec<Stroke>,
            language: Language,
            reply: Sender<Result<Recognition, HwrError>>,
        },
        Analyze {
            strokes: Vec<Stroke>,
            reply: Sender<Result<Vec<Region>, HwrError>>,
        },
    }

    /// Windows' recognizers, driven by a worker thread started with the recognizer. Dropping
    /// it lets the worker end after the request under way (it is not waited for: the app may be
    /// closing). Calls block until the worker answers.
    pub struct SystemRecognizer {
        requests: Mutex<Option<Sender<Request>>>,
    }

    impl Default for SystemRecognizer {
        fn default() -> Self {
            SystemRecognizer::new()
        }
    }

    impl SystemRecognizer {
        /// The engine at the scale chosen for notes ([`Scale::default`]), at normal priority
        /// (someone waits for the result).
        pub fn new() -> SystemRecognizer {
            SystemRecognizer::start(Scale::default(), false)
        }

        /// The engine for background work: its worker thread runs at background priority (CPU
        /// and disk); the engine's own threads do not (see the module documentation).
        pub fn background() -> SystemRecognizer {
            SystemRecognizer::start(Scale::default(), true)
        }

        /// The engine with the strokes scaled by `scale` (the reference test compares scales).
        pub fn with_scale(scale: Scale) -> SystemRecognizer {
            SystemRecognizer::start(scale, false)
        }

        fn start(scale: Scale, background: bool) -> SystemRecognizer {
            let (sender, receiver) = mpsc::channel();
            let worker = std::thread::Builder::new()
                .name("ardoise-handwriting".into())
                .spawn(move || run(receiver, scale, background))
                .ok();
            SystemRecognizer {
                // Without a worker every call answers `Stopped`.
                requests: Mutex::new(worker.map(|_| sender)),
            }
        }

        fn ask<T>(
            &self,
            request: impl FnOnce(Sender<Result<T, HwrError>>) -> Request,
        ) -> Result<T, HwrError> {
            let (reply, answer) = mpsc::channel();
            {
                let requests = self.requests.lock().map_err(|_| HwrError::Stopped)?;
                let sender = requests.as_ref().ok_or(HwrError::Stopped)?;
                sender.send(request(reply)).map_err(|_| HwrError::Stopped)?;
            }
            answer.recv().map_err(|_| HwrError::Stopped)?
        }
    }

    impl Recognizer for SystemRecognizer {
        fn recognizers(&self) -> Result<Vec<RecognizerInfo>, HwrError> {
            self.ask(Request::List)
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
        ) -> Result<Recognition, HwrError> {
            if strokes.is_empty() {
                return Ok(Recognition::empty(language));
            }
            let strokes = strokes.to_vec();
            self.ask(|reply| Request::Recognize {
                strokes,
                language,
                reply,
            })
        }

        fn analyze(&self, strokes: &[Stroke]) -> Result<Vec<Region>, HwrError> {
            if strokes.is_empty() {
                return Ok(Vec::new());
            }
            let strokes = strokes.to_vec();
            self.ask(|reply| Request::Analyze { strokes, reply })
        }
    }

    fn engine_error(step: &'static str) -> impl Fn(windows::core::Error) -> HwrError {
        move |error| HwrError::Engine {
            step,
            message: error.to_string(),
        }
    }

    /// The engine's objects, made once by the worker and used only there.
    struct Engine {
        container: InkRecognizerContainer,
        builder: InkStrokeBuilder,
        recognizers: Vec<(RecognizerInfo, InkRecognizer)>,
    }

    impl Engine {
        fn new() -> Result<Engine, HwrError> {
            // Fails when Windows has no recognition engine at all.
            let container =
                InkRecognizerContainer::new().map_err(engine_error("InkRecognizerContainer"))?;
            let builder = InkStrokeBuilder::new().map_err(engine_error("InkStrokeBuilder"))?;
            let mut recognizers = Vec::new();
            for recognizer in &container
                .GetRecognizers()
                .map_err(engine_error("GetRecognizers"))?
            {
                let name = recognizer
                    .Name()
                    .map_err(engine_error("InkRecognizer.Name"))?
                    .to_string();
                let info = RecognizerInfo {
                    language: crate::language_of(&name),
                    name,
                };
                recognizers.push((info, recognizer));
            }
            Ok(Engine {
                container,
                builder,
                recognizers,
            })
        }

        fn list(&self) -> Vec<RecognizerInfo> {
            self.recognizers
                .iter()
                .map(|(info, _)| info.clone())
                .collect()
        }

        /// The strokes as an ink container (engine coordinates), and the engine's id of each.
        fn container_of(
            &self,
            strokes: &[Stroke],
            scale: Scale,
        ) -> Result<(InkStrokeContainer, Vec<u32>), HwrError> {
            let ink = InkStrokeContainer::new().map_err(engine_error("InkStrokeContainer"))?;
            let mut ids = Vec::with_capacity(strokes.len());
            for stroke in strokes {
                let mut points = Vec::with_capacity(stroke.points.len().max(2));
                for p in &stroke.points {
                    let (x, y) = scale.to_engine(p.x, p.y);
                    let point = InkPoint::CreateInkPoint(Point { X: x, Y: y }, p.pressure)
                        .map_err(engine_error("CreateInkPoint"))?;
                    points.push(Some(point));
                }
                // A dot (one sample) becomes a tiny segment: a stroke needs two points.
                if let [Some(only)] = points.as_slice() {
                    let position = only.Position().map_err(engine_error("InkPoint"))?;
                    let pressure = only.Pressure().map_err(engine_error("InkPoint"))?;
                    let next = Point {
                        X: position.X + 0.5,
                        Y: position.Y + 0.5,
                    };
                    points.push(Some(
                        InkPoint::CreateInkPoint(next, pressure)
                            .map_err(engine_error("CreateInkPoint"))?,
                    ));
                }
                let points = IIterable::<InkPoint>::from(points);
                let built = self
                    .builder
                    .CreateStrokeFromInkPoints(&points, Matrix3x2::identity())
                    .map_err(engine_error("CreateStrokeFromInkPoints"))?;
                ink.AddStroke(&built).map_err(engine_error("AddStroke"))?;
                ids.push(built.Id().map_err(engine_error("InkStroke.Id"))?);
            }
            Ok((ink, ids))
        }

        /// The paragraphs, list items and drawings `InkAnalyzer` finds in `strokes`, in page
        /// units.
        fn analyze(&self, strokes: &[Stroke], scale: Scale) -> Result<Vec<Region>, HwrError> {
            let (ink, ids) = self.container_of(strokes, scale)?;
            let analyzer = InkAnalyzer::new().map_err(engine_error("InkAnalyzer"))?;
            analyzer
                .AddDataForStrokes(&ink.GetStrokes().map_err(engine_error("GetStrokes"))?)
                .map_err(engine_error("AddDataForStrokes"))?;
            analyzer
                .AnalyzeAsync()
                .map_err(engine_error("AnalyzeAsync"))?
                .join()
                .map_err(engine_error("AnalyzeAsync"))?;
            let root: IInkAnalysisNode = analyzer
                .AnalysisRoot()
                .and_then(|root| root.cast())
                .map_err(engine_error("AnalysisRoot"))?;
            let mut regions = Vec::new();
            collect_regions(&root, &ids, scale, &mut regions)?;
            Ok(regions)
        }

        fn recognize(
            &self,
            strokes: &[Stroke],
            language: Language,
            scale: Scale,
        ) -> Result<Recognition, HwrError> {
            let (info, recognizer) = self
                .recognizers
                .iter()
                .find(|(info, _)| info.language == Some(language))
                .ok_or(HwrError::NoRecognizer { language })?;
            self.container
                .SetDefaultRecognizer(recognizer)
                .map_err(engine_error("SetDefaultRecognizer"))?;
            let (ink, _) = self.container_of(strokes, scale)?;
            let results = self
                .container
                .RecognizeAsync(&ink, InkRecognitionTarget::All)
                .map_err(engine_error("RecognizeAsync"))?
                .join()
                .map_err(engine_error("RecognizeAsync"))?;
            let mut words = Vec::new();
            for result in &results {
                let mut candidates = Vec::new();
                for candidate in &result
                    .GetTextCandidates()
                    .map_err(engine_error("GetTextCandidates"))?
                {
                    let candidate = candidate.to_string();
                    if !candidate.trim().is_empty() && !candidates.contains(&candidate) {
                        candidates.push(candidate);
                    }
                }
                if candidates.is_empty() {
                    continue;
                }
                let rect = result
                    .BoundingRect()
                    .map_err(engine_error("BoundingRect"))?;
                let text = candidates.remove(0);
                words.push(Word {
                    text,
                    alternatives: candidates,
                    bounds: scale.to_page(rect.X, rect.Y, rect.Width, rect.Height),
                });
            }
            Ok(Recognition {
                language,
                recognizer: info.name.clone(),
                lines: group_lines(words),
            })
        }
    }

    /// The analysis tree flattened to the regions the export needs: a paragraph (or each list
    /// item of one that holds list items), a drawing, ink the analysis could not classify.
    fn collect_regions(
        node: &IInkAnalysisNode,
        ids: &[u32],
        scale: Scale,
        out: &mut Vec<Region>,
    ) -> Result<(), HwrError> {
        let step = engine_error("InkAnalysisNode");
        let kind = node.Kind().map_err(&step)?;
        let children = |node: &IInkAnalysisNode| -> Result<Vec<IInkAnalysisNode>, HwrError> {
            Ok(node.Children().map_err(&step)?.into_iter().collect())
        };
        let region = |node: &IInkAnalysisNode, kind: RegionKind, text: String| {
            let rect = node.BoundingRect().map_err(&step)?;
            let mut strokes = Vec::new();
            for id in node.GetStrokeIds().map_err(&step)? {
                if let Some(position) = ids.iter().position(|known| *known == id) {
                    strokes.push(position);
                }
            }
            Ok::<Region, HwrError>(Region {
                kind,
                bounds: scale.to_page(rect.X, rect.Y, rect.Width, rect.Height),
                text,
                strokes,
            })
        };
        if kind == InkAnalysisNodeKind::Paragraph {
            let has_items = children(node)?
                .iter()
                .any(|c| c.Kind().is_ok_and(|k| k == InkAnalysisNodeKind::ListItem));
            if has_items {
                // A paragraph of list items (and maybe a line before or after them).
                for child in children(node)? {
                    collect_regions(&child, ids, scale, out)?;
                }
            } else {
                let text = node
                    .cast::<InkAnalysisParagraph>()
                    .and_then(|p| p.RecognizedText())
                    .map(|t| t.to_string())
                    .unwrap_or_default();
                out.push(region(node, RegionKind::Paragraph, text)?);
            }
        } else if kind == InkAnalysisNodeKind::ListItem {
            let text = node
                .cast::<InkAnalysisListItem>()
                .and_then(|p| p.RecognizedText())
                .map(|t| t.to_string())
                .unwrap_or_default();
            out.push(region(node, RegionKind::ListItem, text)?);
        } else if kind == InkAnalysisNodeKind::InkDrawing
            || kind == InkAnalysisNodeKind::UnclassifiedInk
        {
            // The kind of drawing (circle, rectangle...) is not needed: the ink is cropped.
            out.push(region(node, RegionKind::Drawing, String::new())?);
        } else if kind == InkAnalysisNodeKind::Line {
            // A line outside any paragraph.
            let text = node
                .cast::<InkAnalysisLine>()
                .and_then(|l| l.RecognizedText())
                .map(|t| t.to_string())
                .unwrap_or_default();
            out.push(region(node, RegionKind::Paragraph, text)?);
        } else if kind == InkAnalysisNodeKind::Root || kind == InkAnalysisNodeKind::WritingRegion {
            for child in children(node)? {
                collect_regions(&child, ids, scale, out)?;
            }
        }
        Ok(())
    }

    fn run(requests: Receiver<Request>, scale: Scale, background: bool) {
        // SAFETY: plain FFI call with a valid constant. Already initialised (S_FALSE) or in
        // another apartment: WinRT calls still work, so the result is ignored.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        if background {
            lower_thread_priority();
        }
        // Made at the first request, then kept; a failure is retried at the next one.
        let mut engine: Option<Engine> = None;
        let ready = |engine: &mut Option<Engine>| -> Result<(), HwrError> {
            if engine.is_none() {
                *engine = Some(Engine::new()?);
            }
            Ok(())
        };
        for request in requests {
            match request {
                Request::List(reply) => {
                    let result = ready(&mut engine)
                        .map(|()| engine.as_ref().map(Engine::list).unwrap_or_default());
                    let _ = reply.send(result);
                }
                Request::Recognize {
                    strokes,
                    language,
                    reply,
                } => {
                    let result = ready(&mut engine).and_then(|()| match engine.as_ref() {
                        Some(engine) => engine.recognize(&strokes, language, scale),
                        None => Err(HwrError::Stopped),
                    });
                    let _ = reply.send(result);
                }
                Request::Analyze { strokes, reply } => {
                    let result = ready(&mut engine).and_then(|()| match engine.as_ref() {
                        Some(engine) => engine.analyze(&strokes, scale),
                        None => Err(HwrError::Stopped),
                    });
                    let _ = reply.send(result);
                }
            }
        }
    }
}
