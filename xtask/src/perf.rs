//! `cargo xtask perf [--write] [--only start,library,live,touchup,export,size]`: measures the targets of
//! SPEC §8 on the production executable (`target/release/ardoise.exe`, built by `cargo xtask
//! check`), prints a table (target, measured value, verdict) and, with `--write`, puts it in
//! `docs/performance.md`.
//!
//! - start, library of 1,000 notes, live mode, live mode touched up (M12): `app/e2e/perf.mjs` drives a portable copy of the
//!   executable through WebView2's DevTools protocol and returns raw measures (times, frame
//!   intervals); the statistics and the verdicts are computed here, where they are unit-tested.
//! - MP4 1080p of a 2-minute replay: the release benchmark `export_speed_mp4_2min` of
//!   `ardoise-media` (the code the app's export runs), through Media Foundation.
//! - executable size: the file.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

/// SPEC §8: start in under 2 s.
const START_MS: f64 = 2000.0;
/// A library of 1,000 notes is listed as fast as the app starts.
const LIBRARY_LISTED_MS: f64 = 2000.0;
/// The thumbnails on screen are drawn within this time of the spawn.
const THUMBNAILS_MS: f64 = 3000.0;
/// Fluid scrolling: 95 % of the frames at 30 fps or better…
const SCROLL_P95_MS: f64 = 1000.0 / 30.0;
/// …and at most this share of frames over 50 ms.
const SCROLL_OVER_50_MAX_PERCENT: f64 = 1.0;
/// SPEC §8: 60 images per second or more; the measure is the browser's frame callback, which
/// jitters by a few percent around a 60 Hz display.
const LIVE_MIN_FPS: f64 = 57.0;
/// A frame later than this missed a 60 Hz refresh (1.5 periods).
const DROPPED_MS: f64 = 25.0;
/// At most this share of dropped frames in live mode.
const LIVE_DROPPED_MAX_PERCENT: f64 = 1.0;
/// SPEC §8: MP4 1080p of a 2-minute replay in under 2 minutes.
const EXPORT_SECONDS: f64 = 120.0;
/// SPEC §8: executable under 30 MB (the same bound as `cargo xtask selftest`).
const MAX_EXE_BYTES: u64 = 30 * 1024 * 1024;

const PARTS: [&str; 6] = ["start", "library", "live", "touchup", "export", "size"];
const START_MARK: &str = "<!-- perf:start -->";
const END_MARK: &str = "<!-- perf:end -->";

// --- Statistics ---------------------------------------------------------------------------------

/// What the frame intervals of a scroll or of live mode say.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameStats {
    pub frames: usize,
    pub mean_fps: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    /// Frames later than 50 ms.
    pub over_50: usize,
    /// Frames later than [`DROPPED_MS`].
    pub dropped: usize,
}

impl FrameStats {
    /// `None` without any interval.
    pub fn of(intervals: &[f64]) -> Option<FrameStats> {
        let intervals: Vec<f64> = intervals
            .iter()
            .copied()
            .filter(|ms| ms.is_finite() && *ms > 0.0)
            .collect();
        if intervals.is_empty() {
            return None;
        }
        let total: f64 = intervals.iter().sum();
        let mut sorted = intervals.clone();
        sorted.sort_by(f64::total_cmp);
        Some(FrameStats {
            frames: intervals.len(),
            mean_fps: 1000.0 * intervals.len() as f64 / total,
            p95_ms: percentile(&sorted, 0.95),
            max_ms: sorted[sorted.len() - 1],
            over_50: intervals.iter().filter(|ms| **ms > 50.0).count(),
            dropped: intervals.iter().filter(|ms| **ms > DROPPED_MS).count(),
        })
    }

    fn percent(&self, count: usize) -> f64 {
        100.0 * count as f64 / self.frames as f64
    }
}

/// The value at rank `p` (0 to 1) of an ascending list, interpolating between the closest ranks.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    match sorted {
        [] => f64::NAN,
        [only] => *only,
        _ => {
            let rank = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
            let low = rank.floor() as usize;
            let high = (low + 1).min(sorted.len() - 1);
            sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
        }
    }
}

pub fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    percentile(&sorted, 0.5)
}

// --- Measures -----------------------------------------------------------------------------------

/// One opening of the library of many notes.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    pub listed_ms: f64,
    pub thumbnails_ms: f64,
    pub visible_cards: u64,
    pub scroll: Option<FrameStats>,
    /// After the scroll: time for the thumbnails of the place it stopped at.
    pub settle_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Start {
    pub cold_ms: f64,
    pub warm_ms: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    pub notes: u64,
    pub first: Opening,
    pub second: Opening,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Live {
    pub stats: Option<FrameStats>,
    /// Strokes on the live canvas before and after the measure: it must have grown.
    pub strokes: (i64, i64),
}

/// Live mode on a loaded page while strokes are deleted, recoloured and brought back (M12).
#[derive(Debug, Clone, PartialEq)]
pub struct TouchUp {
    pub stats: Option<FrameStats>,
    /// The same page for a few seconds before the first touch-up: what the page costs by itself.
    pub idle: Option<FrameStats>,
    /// Strokes on the live canvas before and after the measure: the tablet must have written.
    pub strokes: (i64, i64),
    /// Strokes of the page when the measure began (the sheet that was resumed).
    pub loaded: u64,
    /// Edits made through the interface during the measure (colours, deletions, undos, redos).
    pub edits: u64,
    /// Strokes a deletion took away from the canvas, on average: the edits must have shown.
    pub removed_per_delete: f64,
    /// What one edit costs on this page (absent from an older harness).
    pub edit_cost: Option<EditCost>,
}

/// The cost of one edit, against a full redraw of the cache on the same page. Not judged: the frames of the measure are.
#[derive(Debug, Clone, PartialEq)]
pub struct EditCost {
    /// The longest frame after an edit, ms: median and worst.
    pub frame_ms: (f64, f64),
    /// The longest frame after a full redraw of the cache, ms: median and worst.
    pub full_frame_ms: Option<(f64, f64)>,
    /// The drawing calls of a repaint and of a full redraw (median, ms), without what the graphics process does with them.
    pub call_ms: Option<f64>,
    pub full_call_ms: Option<f64>,
    /// Edits the cache took by repainting an area, and by a full redraw.
    pub partial_redraws: u64,
    pub full_redraws: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub frames: u64,
    pub seconds: f64,
    pub replay_seconds: f64,
    pub encoder: String,
}

/// Everything measured in a run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measures {
    pub start: Option<Start>,
    pub library: Option<Library>,
    pub live: Option<Live>,
    pub touch_up: Option<TouchUp>,
    pub export: Option<Export>,
    pub exe_bytes: Option<u64>,
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

/// The median and the largest of `values`.
fn median_and_worst(values: &[f64]) -> Option<(f64, f64)> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some((*sorted.get(sorted.len() / 2)?, *sorted.last()?))
}

fn edit_cost(value: &Value) -> Option<EditCost> {
    Some(EditCost {
        frame_ms: median_and_worst(&numbers(&value["editFrames"]))?,
        full_frame_ms: median_and_worst(&numbers(&value["fullRedrawFrames"])),
        call_ms: median_and_worst(&numbers(&value["editCalls"])).map(|(median, _)| median),
        full_call_ms: median_and_worst(&numbers(&value["fullRedrawCalls"]))
            .map(|(median, _)| median),
        partial_redraws: value["partialRedraws"].as_u64().unwrap_or(0),
        full_redraws: value["fullRedraws"].as_u64().unwrap_or(0),
    })
}

fn opening(value: &Value) -> Option<Opening> {
    Some(Opening {
        listed_ms: value["listedMs"].as_f64()?,
        thumbnails_ms: value["thumbnailsMs"].as_f64()?,
        visible_cards: value["visibleCards"].as_u64().unwrap_or(0),
        scroll: FrameStats::of(&numbers(&value["scroll"]["intervals"])),
        settle_ms: value["settleMs"].as_f64(),
    })
}

/// The raw measures `perf.mjs` prints after `PERF-JSON `.
pub fn parse_harness(json: &str, into: &mut Measures) -> Result<(), String> {
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if let Some(start) = value.get("start") {
        into.start = Some(Start {
            cold_ms: start["coldMs"].as_f64().ok_or("start.coldMs missing")?,
            warm_ms: numbers(&start["warmMs"]),
        });
    }
    if let Some(library) = value.get("library") {
        into.library = Some(Library {
            notes: library["notes"].as_u64().unwrap_or(0),
            first: opening(&library["first"]).ok_or("library.first incomplete")?,
            second: opening(&library["second"]).ok_or("library.second incomplete")?,
        });
    }
    if let Some(live) = value.get("live") {
        into.live = Some(Live {
            stats: FrameStats::of(&numbers(&live["intervals"])),
            strokes: (
                live["strokesBefore"].as_i64().unwrap_or(-1),
                live["strokesAfter"].as_i64().unwrap_or(-1),
            ),
        });
    }
    if let Some(touch_up) = value.get("liveTouchUp") {
        into.touch_up = Some(TouchUp {
            stats: FrameStats::of(&numbers(&touch_up["intervals"])),
            idle: FrameStats::of(&numbers(&touch_up["idleIntervals"])),
            strokes: (
                touch_up["strokesBefore"].as_i64().unwrap_or(-1),
                touch_up["strokesAfter"].as_i64().unwrap_or(-1),
            ),
            loaded: touch_up["loaded"].as_u64().unwrap_or(0),
            edits: touch_up["edits"].as_u64().unwrap_or(0),
            removed_per_delete: touch_up["removedPerDelete"].as_f64().unwrap_or(0.0),
            edit_cost: edit_cost(touch_up),
        });
    }
    Ok(())
}

/// The `PERF-EXPORT` line printed by the benchmark `export_speed_mp4_2min`.
pub fn parse_export(output: &str) -> Option<Export> {
    let line = output.lines().find(|l| l.starts_with("PERF-EXPORT "))?;
    let field = |name: &str| -> Option<&str> {
        let start = line.find(&format!(" {name}="))? + name.len() + 2;
        let rest = &line[start..];
        // `encoder=` is the last field and may hold spaces.
        Some(if name == "encoder" {
            rest.trim()
        } else {
            rest.split(' ').next()?
        })
    };
    Some(Export {
        frames: field("frames")?.parse().ok()?,
        seconds: field("seconds")?.parse().ok()?,
        replay_seconds: field("replay_s")?.parse().ok()?,
        encoder: field("encoder")?.to_owned(),
    })
}

// --- Verdicts and table -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Missed,
    /// Shown, not judged.
    Info,
}

impl Verdict {
    fn word(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Verdict::Ok, _) => "OK",
            (Verdict::Missed, Lang::En) => "MISSED",
            (Verdict::Missed, Lang::Fr) => "NON ATTEINT",
            (Verdict::Info, Lang::En) => "info",
            (Verdict::Info, Lang::Fr) => "à titre indicatif",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

/// A row of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub target: String,
    pub measured: String,
    pub verdict: Verdict,
}

/// A number with `decimals` decimals, a comma in French.
fn num(value: f64, decimals: usize, lang: Lang) -> String {
    let text = format!("{value:.decimals$}");
    match lang {
        Lang::En => text,
        Lang::Fr => text.replace('.', ","),
    }
}

fn seconds(ms: f64, lang: Lang) -> String {
    format!("{} s", num(ms / 1000.0, 2, lang))
}

fn judge(ok: bool) -> Verdict {
    if ok { Verdict::Ok } else { Verdict::Missed }
}

fn edit_cost_text(cost: &EditCost, lang: Lang) -> String {
    let (full_redraw, calls, against, repaints, redraws) = match lang {
        Lang::En => (
            "full redraw",
            "calls",
            "against",
            "repaints",
            "full redraws",
        ),
        Lang::Fr => (
            "dessin complet",
            "appels",
            "contre",
            "dessins partiels",
            "dessins complets",
        ),
    };
    let pair = |(median, worst): (f64, f64)| {
        format!("{} / {} ms", num(median, 1, lang), num(worst, 1, lang))
    };
    let mut text = pair(cost.frame_ms);
    if let Some(full) = cost.full_frame_ms {
        let colon = if lang == Lang::En { ":" } else { " :" };
        text.push_str(&format!("; {full_redraw}{colon} {}", pair(full)));
    }
    if let (Some(call), Some(full)) = (cost.call_ms, cost.full_call_ms) {
        text.push_str(&format!(
            "; {calls} {} ms {against} {} ms",
            num(call, 1, lang),
            num(full, 1, lang)
        ));
    }
    text.push_str(&format!(
        "; {} {repaints}, {} {redraws}",
        cost.partial_redraws, cost.full_redraws
    ));
    text
}

fn frames_text(stats: &FrameStats, lang: Lang) -> String {
    let (late, over) = match lang {
        Lang::En => ("late (> 25 ms)", "over 50 ms"),
        Lang::Fr => ("en retard (> 25 ms)", "au-delà de 50 ms"),
    };
    format!(
        "{} fps, p95 {} ms, {} {late}, {} {over} ({} {})",
        num(stats.mean_fps, 1, lang),
        num(stats.p95_ms, 1, lang),
        stats.dropped,
        stats.over_50,
        stats.frames,
        if lang == Lang::En { "frames" } else { "images" }
    )
}

/// The table of a run, in English (terminal) or French (`docs/performance.md`).
pub fn rows(m: &Measures, lang: Lang) -> Vec<Row> {
    let mut rows = build_rows(m, lang);
    if lang == Lang::Fr {
        // French typography: a space before the semicolon.
        for row in &mut rows {
            row.measured = row.measured.replace("; ", " ; ");
        }
    }
    rows
}

fn build_rows(m: &Measures, lang: Lang) -> Vec<Row> {
    let fr = lang == Lang::Fr;
    let t = |en: &str, fr_text: &str| (if fr { fr_text } else { en }).to_owned();
    let mut rows = Vec::new();
    if let Some(start) = &m.start {
        let warm = median(&start.warm_ms);
        rows.push(Row {
            target: t("Start, warm (< 2 s)", "Démarrage à chaud (< 2 s)"),
            measured: format!(
                "{} ({} {}; {} {})",
                seconds(warm, lang),
                t("median of", "médiane de"),
                start.warm_ms.len(),
                t("cold:", "à froid :"),
                seconds(start.cold_ms, lang)
            ),
            verdict: judge(warm < START_MS),
        });
    }
    if let Some(lib) = &m.library {
        let n = lib.notes;
        let pair = |first: f64, second: f64| {
            format!(
                "{} {}; {} {}",
                t("cached:", "en cache :"),
                seconds(second, lang),
                t("not cached:", "hors cache :"),
                seconds(first, lang)
            )
        };
        rows.push(Row {
            target: t(
                &format!("{n} notes listed (< 2 s)"),
                &format!("{n} notes affichées (< 2 s)"),
            ),
            measured: pair(lib.first.listed_ms, lib.second.listed_ms),
            verdict: judge(lib.second.listed_ms < LIBRARY_LISTED_MS),
        });
        rows.push(Row {
            target: t(
                &format!("{n} notes, thumbnails on screen drawn (< 3 s)"),
                &format!("{n} notes, miniatures visibles dessinées (< 3 s)"),
            ),
            measured: format!(
                "{} ({} {})",
                pair(lib.first.thumbnails_ms, lib.second.thumbnails_ms),
                lib.second.visible_cards,
                t("cards", "cartes")
            ),
            verdict: judge(lib.second.thumbnails_ms < THUMBNAILS_MS),
        });
        for (opening, label_en, label_fr, judged) in [
            (
                &lib.second,
                "thumbnails cached",
                "miniatures en cache",
                true,
            ),
            (
                &lib.first,
                "thumbnails not cached",
                "miniatures hors cache",
                false,
            ),
        ] {
            let (measured, verdict) = match &opening.scroll {
                Some(stats) => (
                    frames_text(stats, lang),
                    if !judged {
                        Verdict::Info
                    } else {
                        judge(
                            stats.p95_ms <= SCROLL_P95_MS
                                && stats.percent(stats.over_50) <= SCROLL_OVER_50_MAX_PERCENT,
                        )
                    },
                ),
                None => (
                    t("no frame measured", "aucune image mesurée"),
                    Verdict::Missed,
                ),
            };
            rows.push(Row {
                target: t(
                    &format!("{n} notes, scrolling, {label_en} (p95 <= 33 ms, <= 1 % over 50 ms)"),
                    &format!(
                        "{n} notes, défilement, {label_fr} (p95 <= 33 ms, <= 1 % au-delà de 50 ms)"
                    ),
                ),
                measured,
                verdict,
            });
        }
    }
    if let Some(live) = &m.live {
        let (measured, verdict) = match &live.stats {
            Some(stats) if live.strokes.1 > live.strokes.0 => (
                frames_text(stats, lang),
                judge(
                    stats.mean_fps >= LIVE_MIN_FPS
                        && stats.percent(stats.dropped) <= LIVE_DROPPED_MAX_PERCENT,
                ),
            ),
            Some(_) => (
                t(
                    "invalid: no stroke arrived during the measure",
                    "invalide : aucun trait reçu pendant la mesure",
                ),
                Verdict::Missed,
            ),
            None => (
                t("no frame measured", "aucune image mesurée"),
                Verdict::Missed,
            ),
        };
        rows.push(Row {
            target: t(
                "Live mode (>= 60 fps, <= 1 % late frames)",
                "Mode direct (>= 60 images/s, <= 1 % d'images en retard)",
            ),
            measured,
            verdict,
        });
    }
    if let Some(touch) = &m.touch_up {
        let (measured, verdict) = match &touch.stats {
            Some(stats)
                if touch.strokes.1 > touch.strokes.0
                    && touch.edits > 0
                    && touch.removed_per_delete >= 1.0 =>
            {
                (
                    format!(
                        "{}; {} {}",
                        frames_text(stats, lang),
                        touch.edits,
                        t("edits", "retouches")
                    ),
                    judge(
                        stats.mean_fps >= LIVE_MIN_FPS
                            && stats.percent(stats.dropped) <= LIVE_DROPPED_MAX_PERCENT,
                    ),
                )
            }
            Some(_) => (
                t(
                    "invalid: no stroke arrived or no edit showed during the measure",
                    "invalide : aucun trait reçu ou aucune retouche visible pendant la mesure",
                ),
                Verdict::Missed,
            ),
            None => (
                t("no frame measured", "aucune image mesurée"),
                Verdict::Missed,
            ),
        };
        rows.push(Row {
            target: t(
                &format!(
                    "Live mode, page of {} strokes touched up (>= 60 fps, <= 1 % late frames)",
                    touch.loaded
                ),
                &format!(
                    "Mode direct, page de {} traits retouchée (>= 60 images/s, <= 1 % d'images en retard)",
                    touch.loaded
                ),
            ),
            measured,
            verdict,
        });
        if let Some(idle) = &touch.idle {
            rows.push(Row {
                target: t(
                    &format!(
                        "Same page, tablet writing, nothing touched up ({} strokes)",
                        touch.loaded
                    ),
                    &format!(
                        "Même page, la tablette écrit, sans retouche ({} traits)",
                        touch.loaded
                    ),
                ),
                measured: frames_text(idle, lang),
                verdict: Verdict::Info,
            });
        }
        if let Some(cost) = &touch.edit_cost {
            rows.push(Row {
                target: t(
                    "One edit on this page, longest frame after it (median / worst)",
                    "Une retouche sur cette page, plus longue image après elle (médiane / pire)",
                ),
                measured: edit_cost_text(cost, lang),
                verdict: Verdict::Info,
            });
        }
    }
    if let Some(export) = &m.export {
        rows.push(Row {
            target: t(
                "MP4 1080p of a 2-min replay (< 2 min)",
                "MP4 1080p d'une relecture de 2 min (< 2 min)",
            ),
            measured: format!(
                "{} s ({} {}, {} {}; {})",
                num(export.seconds, 1, lang),
                export.frames,
                t("frames", "images"),
                num(export.replay_seconds, 0, lang),
                t("s of replay", "s de relecture"),
                export.encoder
            ),
            verdict: judge(export.seconds < EXPORT_SECONDS),
        });
    }
    if let Some(bytes) = m.exe_bytes {
        rows.push(Row {
            target: t(
                "Executable size (< 30 MB)",
                "Taille de l'exécutable (< 30 Mo)",
            ),
            measured: format!(
                "{} {}",
                num(bytes as f64 / (1024.0 * 1024.0), 1, lang),
                t("MB", "Mo")
            ),
            verdict: judge(bytes < MAX_EXE_BYTES),
        });
    }
    rows
}

/// A plain-text table with aligned columns.
pub fn text_table(rows: &[Row], lang: Lang) -> String {
    let header = match lang {
        Lang::En => ("Target", "Measured", "Result"),
        Lang::Fr => ("Objectif", "Mesuré", "Résultat"),
    };
    let width = |f: fn(&Row) -> &str, head: &str| {
        rows.iter()
            .map(|r| f(r).chars().count())
            .chain([head.chars().count()])
            .max()
            .unwrap_or(0)
    };
    let (w1, w2) = (
        width(|r| &r.target, header.0),
        width(|r| &r.measured, header.1),
    );
    let mut out = format!("{:<w1$}  {:<w2$}  {}\n", header.0, header.1, header.2);
    for row in rows {
        out.push_str(&format!(
            "{:<w1$}  {:<w2$}  {}\n",
            row.target,
            row.measured,
            row.verdict.word(lang)
        ));
    }
    out
}

/// A Markdown table.
pub fn markdown_table(rows: &[Row], lang: Lang) -> String {
    let header = match lang {
        Lang::En => "| Target | Measured | Result |",
        Lang::Fr => "| Objectif | Mesuré | Résultat |",
    };
    let mut out = format!("{header}\n|---|---|---|\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            row.target.replace('|', "/"),
            row.measured.replace('|', "/"),
            row.verdict.word(lang)
        ));
    }
    out
}

// --- The PC and the date -------------------------------------------------------------------------

/// The PC the numbers come from: nothing personal (no user name, no computer name, no path).
#[derive(Debug, Clone, PartialEq)]
pub struct System {
    pub cpu: String,
    pub cores: u64,
    pub threads: u64,
    pub ram_bytes: u64,
    pub gpu: String,
    pub windows: String,
}

pub fn parse_system(json: &str) -> Option<System> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    Some(System {
        cpu: v["cpu"].as_str()?.trim().to_owned(),
        cores: v["cores"].as_u64()?,
        threads: v["threads"].as_u64()?,
        ram_bytes: v["ram"].as_u64()?,
        gpu: v["gpu"].as_str().unwrap_or("").trim().to_owned(),
        windows: v["os"].as_str().unwrap_or("").trim().to_owned(),
    })
}

impl System {
    pub fn describe(&self, lang: Lang) -> String {
        let ram = self.ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let (cores, threads, ram_word) = match lang {
            Lang::En => ("cores", "threads", "GB of RAM"),
            Lang::Fr => ("cœurs", "threads", "Go de RAM"),
        };
        let mut text = format!(
            "{}, {} {cores} / {} {threads}, {} {ram_word}",
            self.cpu,
            self.cores,
            self.threads,
            num(ram, 0, lang)
        );
        if !self.gpu.is_empty() {
            text.push_str(&format!(", GPU : {}", self.gpu));
        }
        if !self.windows.is_empty() {
            text.push_str(&format!(", Windows {}", self.windows));
        }
        text
    }
}

fn read_system() -> Option<System> {
    const SCRIPT: &str = "$c = Get-CimInstance Win32_Processor | Select-Object -First 1; \
        $m = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory; \
        $g = (Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join ' + '; \
        [pscustomobject]@{ cpu = $c.Name.Trim(); cores = $c.NumberOfCores; threads = $c.NumberOfLogicalProcessors; \
        ram = $m; gpu = $g; os = [Environment]::OSVersion.Version.ToString() } | ConvertTo-Json -Compress";
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_system(&String::from_utf8_lossy(&output.stdout))
}

/// `YYYY-MM-DD` (UTC) of `secs` seconds after 1970-01-01.
pub fn date_of(secs: u64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

// --- docs/performance.md --------------------------------------------------------------------------

/// `document` with what lies between the two markers replaced by `content`; `None` when a marker
/// is missing.
pub fn replace_between_markers(document: &str, content: &str) -> Option<String> {
    let start = document.find(START_MARK)? + START_MARK.len();
    let end = start + document[start..].find(END_MARK)?;
    Some(format!(
        "{}\n{}\n{}",
        &document[..start],
        content.trim_end(),
        &document[end..]
    ))
}

fn doc_block(rows: &[Row], system: Option<&System>, date: &str) -> String {
    let pc = system.map_or_else(|| "PC non identifié".to_owned(), |s| s.describe(Lang::Fr));
    format!(
        "Dernière mesure : {date}, sur {pc}.\n\n{}",
        markdown_table(rows, Lang::Fr)
    )
}

// --- Running --------------------------------------------------------------------------------------

fn wanted(only: &[String], part: &str) -> bool {
    only.is_empty() || only.iter().any(|p| p == part)
}

/// `cargo xtask perf [--write] [--only a,b]`.
pub fn command(args: &[&str], root: &Path) -> ExitCode {
    let mut write = false;
    let mut only: Vec<String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "--write" => write = true,
            "--only" => {
                only = iter
                    .next()
                    .map(|list| list.split(',').map(str::to_owned).collect())
                    .unwrap_or_default();
            }
            other => {
                eprintln!("perf: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    if let Some(unknown) = only.iter().find(|p| !PARTS.contains(&p.as_str())) {
        eprintln!("perf: unknown part {unknown} (one of {})", PARTS.join(", "));
        return ExitCode::from(2);
    }
    if write && !only.is_empty() {
        eprintln!("perf: --write needs the complete measure (without --only)");
        return ExitCode::from(2);
    }
    if !cfg!(windows) {
        eprintln!("perf: the measures need Windows (WebView2, Media Foundation)");
        return ExitCode::from(2);
    }
    let exe = root.join("target").join("release").join("ardoise.exe");
    if !exe.exists() {
        eprintln!(
            "{} not found: run `cargo xtask check` first.",
            exe.display()
        );
        println!("PERF FAILED: ardoise.exe missing");
        return ExitCode::FAILURE;
    }

    let mut measures = Measures::default();
    if wanted(&only, "size") {
        measures.exe_bytes = fs::metadata(&exe).ok().map(|m| m.len());
    }

    let harness: Vec<&str> = ["start", "library", "live", "touchup"]
        .into_iter()
        .filter(|p| wanted(&only, p))
        .collect();
    if !harness.is_empty() {
        let script = root.join("app").join("e2e").join("perf.mjs");
        println!("==> start, library and live mode ({})", harness.join(", "));
        let output = Command::new("node")
            .arg(&script)
            .arg(&exe)
            .arg("--only")
            .arg(harness.join(","))
            .current_dir(root)
            .stderr(Stdio::inherit())
            .output();
        let output = match output {
            Ok(output) => output,
            Err(e) => {
                println!("PERF FAILED: cannot run node: {e}");
                return ExitCode::FAILURE;
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed = stdout
            .lines()
            .find_map(|l| l.strip_prefix("PERF-JSON "))
            .ok_or_else(|| "the harness printed no measure".to_owned())
            .and_then(|json| parse_harness(json, &mut measures));
        if let Err(e) = parsed {
            println!("PERF FAILED: {e}");
            return ExitCode::FAILURE;
        }
        if !output.status.success() {
            println!("PERF FAILED: the harness stopped before the end (see above)");
            return ExitCode::FAILURE;
        }
    }

    if wanted(&only, "export") {
        println!("==> MP4 1080p of a 2-minute replay (release benchmark of ardoise-media)");
        let mut command = Command::new("cargo");
        command
            .args([
                "test",
                "--release",
                "-p",
                "ardoise-media",
                "export_speed_mp4_2min",
                "--",
                "--ignored",
                "--nocapture",
            ])
            .current_dir(root)
            .stderr(Stdio::inherit());
        if let Some(nasm) = crate::local_nasm() {
            command.env("NASM", nasm);
        }
        match command.output() {
            Ok(output) if output.status.success() => {
                match parse_export(&String::from_utf8_lossy(&output.stdout)) {
                    Some(export) => measures.export = Some(export),
                    None => {
                        println!("PERF FAILED: the export benchmark printed no PERF-EXPORT line");
                        return ExitCode::FAILURE;
                    }
                }
            }
            Ok(output) => {
                println!("{}", String::from_utf8_lossy(&output.stdout));
                println!("PERF FAILED: the export benchmark failed");
                return ExitCode::FAILURE;
            }
            Err(e) => {
                println!("PERF FAILED: cannot run cargo: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let system = read_system();
    let table = rows(&measures, Lang::En);
    println!();
    match &system {
        Some(system) => println!("PC: {}", system.describe(Lang::En)),
        None => println!("PC: unknown (PowerShell could not tell)"),
    }
    print!("\n{}", text_table(&table, Lang::En));

    if write {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let block = doc_block(&rows(&measures, Lang::Fr), system.as_ref(), &date_of(secs));
        let path = root.join("docs").join("performance.md");
        let written = fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|doc| {
                // Keep the file's line endings.
                let crlf = doc.contains("\r\n");
                let doc = doc.replace("\r\n", "\n");
                replace_between_markers(&doc, &block)
                    .ok_or_else(|| format!("markers {START_MARK} / {END_MARK} missing"))
                    .map(|new| if crlf { new.replace('\n', "\r\n") } else { new })
            })
            .and_then(|new| fs::write(&path, new).map_err(|e| e.to_string()));
        match written {
            Ok(()) => println!("\n{} updated", path.display()),
            Err(e) => {
                println!("PERF FAILED: cannot update docs/performance.md: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let missed = table
        .iter()
        .filter(|r| r.verdict == Verdict::Missed)
        .count();
    if missed == 0 {
        println!("PERF OK");
        ExitCode::SUCCESS
    } else {
        println!("PERF MISSED: {missed} target(s) not reached");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn percentiles_interpolate_between_ranks() {
        assert!(percentile(&[], 0.5).is_nan());
        assert!(close(percentile(&[7.0], 0.95), 7.0));
        assert!(close(median(&[3.0, 1.0, 2.0]), 2.0));
        assert!(close(median(&[4.0, 1.0, 3.0, 2.0]), 2.5));
        let hundred: Vec<f64> = (0..=100).map(f64::from).collect();
        assert!(close(percentile(&hundred, 0.95), 95.0));
        assert!(close(percentile(&hundred, 2.0), 100.0));
    }

    #[test]
    fn frame_statistics() {
        assert_eq!(FrameStats::of(&[]), None);
        assert_eq!(FrameStats::of(&[0.0, f64::NAN]), None);
        // 98 frames at 16 ms, one at 30 ms (late), one at 60 ms (late, over 50).
        let mut intervals = vec![16.0; 98];
        intervals.extend([30.0, 60.0]);
        let stats = FrameStats::of(&intervals).unwrap();
        assert_eq!(stats.frames, 100);
        assert_eq!((stats.over_50, stats.dropped), (1, 2));
        assert!(close(stats.max_ms, 60.0));
        assert!(close(stats.p95_ms, 16.0));
        assert!(close(stats.mean_fps, 1000.0 * 100.0 / (98.0 * 16.0 + 90.0)));
        assert!(close(stats.percent(stats.dropped), 2.0));
    }

    fn steady(ms: f64, count: usize) -> FrameStats {
        FrameStats::of(&vec![ms; count]).unwrap()
    }

    fn opening(listed_ms: f64, scroll: FrameStats) -> Opening {
        Opening {
            listed_ms,
            thumbnails_ms: listed_ms + 300.0,
            visible_cards: 12,
            scroll: Some(scroll),
            settle_ms: Some(80.0),
        }
    }

    fn measures() -> Measures {
        Measures {
            start: Some(Start {
                cold_ms: 2600.0,
                warm_ms: vec![1500.0, 1300.0, 1400.0],
            }),
            library: Some(Library {
                notes: 1000,
                first: opening(2500.0, steady(40.0, 300)),
                second: opening(1200.0, steady(16.7, 360)),
            }),
            live: Some(Live {
                stats: Some(steady(16.7, 480)),
                strokes: (100, 1500),
            }),
            touch_up: Some(TouchUp {
                stats: Some(steady(16.7, 600)),
                idle: Some(steady(16.7, 240)),
                strokes: (4000, 4010),
                loaded: 4000,
                edits: 60,
                removed_per_delete: 100.0,
                edit_cost: None,
            }),
            export: Some(Export {
                frames: 3600,
                seconds: 47.3,
                replay_seconds: 120.0,
                encoder: "Media Foundation: test (hardware)".into(),
            }),
            exe_bytes: Some(14 * 1024 * 1024),
        }
    }

    fn verdicts(m: &Measures) -> Vec<Verdict> {
        rows(m, Lang::En).iter().map(|r| r.verdict).collect()
    }

    #[test]
    fn the_cost_of_one_edit_is_told_and_not_judged() {
        let value: Value = serde_json::from_str(
            r#"{"editFrames":[20,17,50],"fullRedrawFrames":[110,120,100],"editCalls":[1.5,0.5,2.5],
                "fullRedrawCalls":[30,40,20],"partialRedraws":24,"fullRedraws":1}"#,
        )
        .unwrap();
        let cost = edit_cost(&value).unwrap();
        assert_eq!(cost.frame_ms, (20.0, 50.0));
        assert_eq!(cost.full_frame_ms, Some((110.0, 120.0)));
        assert_eq!(cost.call_ms, Some(1.5));
        assert_eq!(cost.full_call_ms, Some(30.0));
        assert_eq!((cost.partial_redraws, cost.full_redraws), (24, 1));
        // An older harness tells nothing: no row.
        assert!(edit_cost(&serde_json::json!({})).is_none());

        let mut told = measures();
        told.touch_up.as_mut().unwrap().edit_cost = Some(cost);
        let table = rows(&told, Lang::En);
        let row = table
            .iter()
            .find(|r| r.target.starts_with("One edit"))
            .unwrap();
        assert_eq!(row.verdict, Verdict::Info);
        assert!(
            row.measured
                .starts_with("20.0 / 50.0 ms; full redraw: 110.0 / 120.0 ms")
        );
        assert!(row.measured.ends_with("24 repaints, 1 full redraws"));
        let french = rows(&told, Lang::Fr);
        assert!(
            french
                .iter()
                .any(|r| r.measured.contains("dessin complet : 110,0 / 120,0 ms"))
        );
    }

    #[test]
    fn the_verdicts_follow_spec_section_8() {
        use Verdict::*;
        // The start is judged warm; the unfilled cache only informs.
        assert_eq!(
            verdicts(&measures()),
            [Ok, Ok, Ok, Ok, Info, Ok, Ok, Info, Ok, Ok]
        );

        let mut slow = measures();
        slow.start.as_mut().unwrap().warm_ms = vec![2100.0, 2200.0, 2300.0];
        assert_eq!(verdicts(&slow)[0], Missed);

        let mut jerky = measures();
        jerky.library.as_mut().unwrap().second.scroll = FrameStats::of(&[16.7, 120.0, 16.7, 16.7]);
        assert_eq!(verdicts(&jerky)[3], Missed);

        let mut stutter = measures();
        // 5 % late frames in live mode.
        let mut intervals = vec![16.7; 95];
        intervals.extend([40.0; 5]);
        stutter.live.as_mut().unwrap().stats = FrameStats::of(&intervals);
        assert_eq!(verdicts(&stutter)[5], Missed);

        let mut idle = measures();
        idle.live.as_mut().unwrap().strokes = (200, 200);
        let rows = rows(&idle, Lang::En);
        assert_eq!(rows[5].verdict, Missed);
        assert!(rows[5].measured.starts_with("invalid"));

        let mut late = measures();
        late.export.as_mut().unwrap().seconds = 121.0;
        late.exe_bytes = Some(MAX_EXE_BYTES);
        let v = verdicts(&late);
        assert_eq!((v[8], v[9]), (Missed, Missed));
    }

    #[test]
    fn live_mode_touched_up_is_judged_like_live_mode_and_must_have_been_touched() {
        use Verdict::*;
        let touch_up = |m: &Measures| rows(m, Lang::En)[6].clone();
        assert!(
            touch_up(&measures())
                .target
                .contains("4000 strokes touched up")
        );

        // 5 % late frames while the page is touched up.
        let mut stutter = measures();
        let mut intervals = vec![16.7; 95];
        intervals.extend([40.0; 5]);
        stutter.touch_up.as_mut().unwrap().stats = FrameStats::of(&intervals);
        assert_eq!(touch_up(&stutter).verdict, Missed);

        // Nothing was written, or no edit reached the canvas: the measure says nothing.
        let mut quiet = measures();
        quiet.touch_up.as_mut().unwrap().strokes = (4000, 4000);
        assert!(touch_up(&quiet).measured.starts_with("invalid"));
        let mut untouched = measures();
        untouched.touch_up.as_mut().unwrap().removed_per_delete = 0.0;
        assert_eq!(touch_up(&untouched).verdict, Missed);
        let mut none = measures();
        none.touch_up.as_mut().unwrap().edits = 0;
        assert_eq!(touch_up(&none).verdict, Missed);
        assert_eq!(touch_up(&measures()).verdict, Ok);
    }

    #[test]
    fn rows_exist_only_for_what_was_measured() {
        assert!(rows(&Measures::default(), Lang::En).is_empty());
        let only_size = Measures {
            exe_bytes: Some(1024 * 1024),
            ..Measures::default()
        };
        assert_eq!(rows(&only_size, Lang::En).len(), 1);
    }

    #[test]
    fn tables_are_aligned_and_translated() {
        let table = text_table(&rows(&measures(), Lang::En), Lang::En);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 11);
        assert!(lines[0].starts_with("Target"));
        // The third column starts at the same place on every line.
        let column = lines[0].find("Result").unwrap();
        for line in &lines[1..] {
            assert!(line[column..].starts_with("OK") || line[column..].starts_with("info"));
        }
        let french = markdown_table(&rows(&measures(), Lang::Fr), Lang::Fr);
        assert!(french.starts_with("| Objectif | Mesuré | Résultat |\n|---|---|---|\n"));
        assert!(french.contains("Démarrage à chaud"));
        assert!(french.contains("1,40 s"));
        assert!(french.contains("14,0 Mo"));
        assert!(french.contains("médiane de 3 ; à froid"));
        assert!(french.contains("à titre indicatif"));
        assert_eq!(french.lines().count(), 12);
    }

    #[test]
    fn the_harness_output_is_read() {
        let json = r#"{"start":{"coldMs":2500.5,"warmMs":[1400,1300,1500]},
            "library":{"notes":1000,
              "first":{"listedMs":2000,"thumbnailsMs":2300,"visibleCards":12,"settleMs":90,
                       "scroll":{"intervals":[16,17,16,60],"scrolledPx":18000,"cards":1000}},
              "second":{"listedMs":1100,"thumbnailsMs":1300,"visibleCards":12,"settleMs":null,
                       "scroll":{"intervals":[16,16],"scrolledPx":18000,"cards":1000}}},
            "live":{"intervals":[16,17],"strokesBefore":10,"strokesAfter":400,"measuredMs":8000},
            "liveTouchUp":{"intervals":[16,17,18],"strokesBefore":4000,"strokesAfter":4012,"measuredMs":10000,
                           "loaded":4000,"iterations":10,"edits":60,"removedPerDelete":98.5}}"#;
        let mut m = Measures::default();
        parse_harness(json, &mut m).unwrap();
        assert!(close(m.start.as_ref().unwrap().cold_ms, 2500.5));
        let library = m.library.unwrap();
        assert_eq!(library.notes, 1000);
        assert_eq!(library.first.scroll.as_ref().unwrap().over_50, 1);
        assert_eq!(library.second.settle_ms, None);
        assert_eq!(m.live.unwrap().strokes, (10, 400));
        let touch_up = m.touch_up.unwrap();
        assert_eq!(touch_up.strokes, (4000, 4012));
        assert_eq!((touch_up.loaded, touch_up.edits), (4000, 60));
        assert!(close(touch_up.removed_per_delete, 98.5));
        assert_eq!(touch_up.stats.unwrap().frames, 3);

        let mut m = Measures::default();
        assert!(parse_harness("not json", &mut m).is_err());
        assert!(parse_harness(r#"{"library":{"first":{},"second":{}}}"#, &mut m).is_err());
    }

    #[test]
    fn the_export_line_is_read() {
        let output = "running 1 test\nPERF-EXPORT mp4 1920x1080 30fps replay_s=120.0 frames=3600 \
                      seconds=47.31 bytes=123456 encoder=Media Foundation: NVIDIA H.264 Encoder MFT (hardware)\n";
        let export = parse_export(output).unwrap();
        assert_eq!(export.frames, 3600);
        assert!(close(export.seconds, 47.31));
        assert!(close(export.replay_seconds, 120.0));
        assert_eq!(
            export.encoder,
            "Media Foundation: NVIDIA H.264 Encoder MFT (hardware)"
        );
        assert_eq!(parse_export("nothing"), None);
        assert_eq!(parse_export("PERF-EXPORT mp4 frames=x"), None);
    }

    #[test]
    fn the_pc_is_described_without_anything_personal() {
        let system = parse_system(
            r#"{"cpu":"Intel(R) Core(TM) i7 ","cores":8,"threads":16,"ram":17048256512,
                "gpu":"NVIDIA GeForce RTX + Intel UHD","os":"10.0.26300.0"}"#,
        )
        .unwrap();
        assert_eq!(
            system.describe(Lang::En),
            "Intel(R) Core(TM) i7, 8 cores / 16 threads, 16 GB of RAM, GPU : NVIDIA GeForce RTX + Intel UHD, Windows 10.0.26300.0"
        );
        assert!(system.describe(Lang::Fr).contains("16 Go de RAM"));
        assert_eq!(parse_system("{}"), None);
        assert_eq!(parse_system("garbage"), None);
    }

    #[test]
    fn dates_are_civil() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(951_782_400), "2000-02-29");
        assert_eq!(date_of(1_793_231_999), "2026-10-28");
        assert_eq!(date_of(1_793_232_000), "2026-10-29");
        assert_eq!(date_of(1_793_318_399), "2026-10-29");
    }

    #[test]
    fn the_table_of_the_document_is_replaced_between_its_markers() {
        let document = format!("Avant\n{START_MARK}\nancien\n{END_MARK}\nAprès\n");
        let new = replace_between_markers(&document, "nouveau\n").unwrap();
        assert_eq!(
            new,
            format!("Avant\n{START_MARK}\nnouveau\n{END_MARK}\nAprès\n")
        );
        // Again: the same.
        assert_eq!(replace_between_markers(&new, "nouveau").unwrap(), new);
        assert_eq!(replace_between_markers("sans marques", "x"), None);
        assert_eq!(
            replace_between_markers(&format!("{END_MARK} {START_MARK}"), "x"),
            None
        );
    }

    #[test]
    fn the_document_block_names_the_date_and_the_pc() {
        let rows = rows(&measures(), Lang::Fr);
        let block = doc_block(&rows, None, "2026-10-29");
        assert!(block.starts_with("Dernière mesure : 2026-10-29, sur PC non identifié."));
        assert!(block.contains("| Objectif | Mesuré | Résultat |"));
    }
}
