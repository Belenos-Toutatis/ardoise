//! Replay time ↔ note time, and the note as drawn at a replay instant (see the module
//! documentation of `replay`).

use std::time::Duration;

use ardoise_core::note::Note;
use ardoise_ink::Stroke;

use crate::MediaError;

/// Slowest and fastest replay (SPEC §4.5: 0.25× to 16×).
pub const MIN_SPEED: f64 = 0.25;
pub const MAX_SPEED: f64 = 16.0;

/// Slack, in milliseconds of kept note time, when a replay instant falls on the boundary of
/// two kept stretches (`Duration` keeps nanoseconds: a round trip may land a few of them
/// short of the boundary, and the later stretch must win there).
const BOUNDARY_SLACK_MS: f64 = 1e-5;

/// How a note is replayed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayOptions {
    /// 1 = as written, 2 = twice as fast; [`MIN_SPEED`] to [`MAX_SPEED`].
    pub speed: f64,
    /// Longest pause kept, in note time (before the speed applies): `Some` trims the dead
    /// time ("supprimer les temps morts"), `None` keeps every pause.
    pub max_pause: Option<Duration>,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            speed: 1.0,
            max_pause: None,
        }
    }
}

impl ReplayOptions {
    /// The options that make the replay of `note` last `target` with pauses trimmed to
    /// `max_pause` (an export's "durée cible"). The speed is kept within [`MIN_SPEED`] and
    /// [`MAX_SPEED`], so the replay can end up longer or shorter than asked; a note without
    /// any sample gets speed 1.
    pub fn for_duration(note: &Note, max_pause: Option<Duration>, target: Duration) -> Self {
        let kept = Clock::new(note, max_pause).kept;
        let target_ms = duration_ms(target);
        let speed = if kept <= 0.0 {
            1.0
        } else if target_ms <= 0.0 {
            MAX_SPEED
        } else {
            (kept / target_ms).clamp(MIN_SPEED, MAX_SPEED)
        };
        Self { speed, max_pause }
    }
}

/// A stretch of note time that is kept, mapped linearly to replay time; the note time
/// between two consecutive stretches is cut.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    /// Note time, in milliseconds.
    note_start: f64,
    note_end: f64,
    /// Note time kept before this stretch, in milliseconds.
    kept_before: f64,
}

/// When the samples of each stroke appear, and what is kept of the note time (independent of
/// the speed).
#[derive(Debug, Clone)]
struct Clock {
    /// Per stroke of the note: when each of its samples appears, in note milliseconds (the
    /// running maximum of the sample times, so that no sample comes before the one it
    /// follows); empty for a stroke that is not drawn (hidden layer, no sample).
    times: Vec<Vec<u32>>,
    /// The strokes drawn, in drawing order (indices into `note.strokes`), all with samples.
    order: Vec<usize>,
    segments: Vec<Segment>,
    /// Note time of the last sample drawn, in milliseconds (0 without any).
    end: f64,
    /// Total note time kept, in milliseconds.
    kept: f64,
}

impl Clock {
    fn new(note: &Note, max_pause: Option<Duration>) -> Self {
        let drawn = ardoise_render::drawing_order(note);
        let mut times = vec![Vec::new(); note.strokes.len()];
        let mut order = Vec::with_capacity(drawn.len());
        for i in drawn {
            let (Some(stroke), Some(slot)) = (note.strokes.get(i), times.get_mut(i)) else {
                continue;
            };
            *slot = appearance_times(stroke);
            if !slot.is_empty() {
                order.push(i);
            }
        }

        // When some stroke is being drawn: the union of the strokes' spans.
        let mut spans: Vec<(u32, u32)> = order
            .iter()
            .filter_map(|&i| {
                let t = times.get(i)?;
                Some((*t.first()?, *t.last()?))
            })
            .collect();
        spans.sort_unstable();
        let mut busy: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
        for (start, end) in spans {
            match busy.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => busy.push((start, end)),
            }
        }
        let end = busy.last().map_or(0.0, |&(_, end)| f64::from(end));

        // The pauses (before the first stroke, between busy spans) cut after `max_pause`.
        let mut cuts: Vec<(f64, f64)> = Vec::new();
        if let Some(max_pause) = max_pause {
            let cap = duration_ms(max_pause);
            let mut idle_from = 0.0;
            for &(start, end) in &busy {
                let start = f64::from(start);
                if start - idle_from > cap {
                    cuts.push((idle_from + cap, start));
                }
                idle_from = f64::from(end);
            }
        }
        let mut segments = Vec::with_capacity(cuts.len() + 1);
        let (mut note_start, mut kept) = (0.0, 0.0);
        for (cut_start, cut_end) in cuts {
            segments.push(Segment {
                note_start,
                note_end: cut_start,
                kept_before: kept,
            });
            kept += cut_start - note_start;
            note_start = cut_end;
        }
        segments.push(Segment {
            note_start,
            note_end: end,
            kept_before: kept,
        });
        kept += end - note_start;
        Self {
            times,
            order,
            segments,
            end,
            kept,
        }
    }
}

/// When each sample of `stroke` appears: its time, or the time of an earlier sample if that
/// is later (times out of order are not trusted to go backwards).
fn appearance_times(stroke: &Stroke) -> Vec<u32> {
    let mut latest = 0;
    stroke
        .points
        .iter()
        .map(|p| {
            latest = latest.max(p.t);
            latest
        })
        .collect()
}

/// `duration` in milliseconds.
fn duration_ms(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1e6
}

/// Milliseconds as a `Duration` (negative or NaN counts as zero; saturates).
fn ms_duration(ms: f64) -> Duration {
    Duration::try_from_secs_f64(ms.max(0.0) / 1000.0).unwrap_or(Duration::MAX)
}

/// The timeline of the replay of one note: its duration, replay time ↔ note time, and the
/// note as drawn at any replay instant.
#[derive(Debug, Clone)]
pub struct Timeline<'n> {
    note: &'n Note,
    options: ReplayOptions,
    clock: Clock,
    duration: Duration,
}

impl<'n> Timeline<'n> {
    /// The timeline of `note` replayed with `options`. Fails with
    /// [`MediaError::InvalidSettings`] when the speed is not within [`MIN_SPEED`] and
    /// [`MAX_SPEED`].
    pub fn new(note: &'n Note, options: &ReplayOptions) -> Result<Self, MediaError> {
        let speed = options.speed;
        if !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
            return Err(MediaError::InvalidSettings(format!(
                "replay speed {speed} is not between {MIN_SPEED} and {MAX_SPEED}"
            )));
        }
        let clock = Clock::new(note, options.max_pause);
        let duration = ms_duration(clock.kept / speed);
        Ok(Self {
            note,
            options: *options,
            clock,
            duration,
        })
    }

    /// The note replayed.
    pub fn note(&self) -> &'n Note {
        self.note
    }

    pub fn options(&self) -> ReplayOptions {
        self.options
    }

    /// Length of the replay: from zero to the moment the last sample appears (zero for a note
    /// without any sample drawn).
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Note time (milliseconds since `Note::created`) of the last sample drawn.
    pub fn note_end(&self) -> f64 {
        self.clock.end
    }

    /// The replay instant at which note time `note_ms` is shown (clamped to the note's span;
    /// a time inside a cut pause maps to the instant of the cut).
    pub fn to_replay(&self, note_ms: f64) -> Duration {
        let clock = &self.clock;
        let tau = if note_ms.is_nan() {
            0.0
        } else {
            note_ms.clamp(0.0, clock.end)
        };
        let index = clock
            .segments
            .partition_point(|s| s.note_start <= tau)
            .saturating_sub(1);
        let Some(segment) = clock.segments.get(index) else {
            return Duration::ZERO;
        };
        let kept = segment.kept_before + (tau.min(segment.note_end) - segment.note_start).max(0.0);
        ms_duration(kept / self.options.speed)
    }

    /// The latest note time (milliseconds since `Note::created`) shown at replay instant
    /// `replay`: everything written up to it is on screen. From [`Timeline::duration`] on,
    /// the time of the last sample.
    pub fn to_note(&self, replay: Duration) -> f64 {
        let clock = &self.clock;
        if replay >= self.duration {
            return clock.end;
        }
        let kept = duration_ms(replay) * self.options.speed;
        let index = clock
            .segments
            .partition_point(|s| s.kept_before <= kept + BOUNDARY_SLACK_MS)
            .saturating_sub(1);
        let Some(segment) = clock.segments.get(index) else {
            return 0.0;
        };
        (segment.note_start + (kept - segment.kept_before).max(0.0)).min(segment.note_end)
    }

    /// The strokes drawn by the replay (visible layers, with samples), in drawing order:
    /// indices into `note.strokes`.
    pub fn drawing_order(&self) -> &[usize] {
        &self.clock.order
    }

    /// How many samples of stroke `index` are shown at note time `note_ms` (0 for a stroke
    /// that is not drawn or not begun).
    pub fn samples_shown(&self, index: usize, note_ms: f64) -> usize {
        self.clock.times.get(index).map_or(0, |times| {
            times.partition_point(|&t| f64::from(t) <= note_ms)
        })
    }

    /// True when stroke `index` is drawn and all of it is shown at note time `note_ms`.
    pub fn is_complete(&self, index: usize, note_ms: f64) -> bool {
        self.clock
            .times
            .get(index)
            .and_then(|times| times.last())
            .is_some_and(|&last| f64::from(last) <= note_ms)
    }

    /// The note as drawn at replay instant `replay`: the strokes of visible layers that have
    /// begun, in note order, each cut after its last sample shown. Everything else (page,
    /// background, layers…) is the note's.
    pub fn note_at(&self, replay: Duration) -> Note {
        let note = self.note;
        let tau = self.to_note(replay);
        let strokes = note
            .strokes
            .iter()
            .enumerate()
            .filter_map(|(i, stroke)| {
                let shown = self.samples_shown(i, tau);
                let points = stroke.points.get(..shown)?;
                (shown > 0).then(|| Stroke {
                    tool: stroke.tool,
                    color: stroke.color,
                    width: stroke.width,
                    layer: stroke.layer,
                    points: points.to_vec(),
                })
            })
            .collect();
        Note {
            id: note.id.clone(),
            title: note.title.clone(),
            created: note.created,
            modified: note.modified,
            page: note.page,
            background: note.background,
            tags: note.tags.clone(),
            favorite: note.favorite,
            layers: note.layers.clone(),
            strokes,
            timing: note.timing,
            origin: note.origin.clone(),
        }
    }

    /// Number of frames of the replay at `frame_rate` frames per second: frame `k` shows
    /// replay instant `k / frame_rate` ([`frame_time`]), and the last one is the first at or
    /// after [`Timeline::duration`], so it shows the whole note. At least one frame.
    pub fn frame_count(&self, frame_rate: u32) -> u64 {
        if frame_rate == 0 {
            return 1;
        }
        let units = self.duration.as_nanos() * u128::from(frame_rate);
        let last = units.div_ceil(1_000_000_000);
        u64::try_from(last).map_or(u64::MAX, |last| last.saturating_add(1))
    }
}

/// Replay instant of frame `index` at `frame_rate` frames per second (rounded down to the
/// nanosecond, so timestamps never drift and strictly increase).
pub fn frame_time(index: u64, frame_rate: u32) -> Duration {
    if frame_rate == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(index) * 1_000_000_000 / u128::from(frame_rate);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}
