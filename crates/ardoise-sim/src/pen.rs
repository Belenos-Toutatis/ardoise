//! Pen movement over the simulated tablet in live mode.
//!
//! A [`PenScript`] says what the pen does, in physical time: it comes near the tablet and moves
//! through samples 5 ms apart (pressure 0 while hovering, above 0 while touching), leaves, stays
//! away for a while, comes back... The tablet anchors a script when live mode starts and
//! streams it as protocol.md §7.2 describes: `a2` when the pen comes near, then `a1`
//! notifications of up to three records, the last one being the all-`ff` record when the pen
//! leaves.

use std::time::Duration;

use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point;

/// Time between two pen samples (protocol.md §6.5, §7.2).
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(5);

/// One phase of a [`PenScript`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PenSegment {
    /// The pen comes near the tablet, goes through these samples (one every 5 ms; pressure 0
    /// means hovering), then leaves. Lasts `(samples + 1) × 5 ms`: the last slot is the
    /// "pen left" record. An empty list does nothing.
    Near(Vec<Point>),
    /// The pen stays away from the tablet.
    Away(Duration),
}

/// What the pen does during live mode, from the moment the tablet starts streaming it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PenScript {
    segments: Vec<PenSegment>,
}

impl PenScript {
    /// An empty script.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a phase where the pen is near the tablet (builder style).
    pub fn near(mut self, points: Vec<Point>) -> Self {
        self.push(PenSegment::Near(points));
        self
    }

    /// Adds a phase where the pen is away (builder style).
    pub fn away(mut self, duration: Duration) -> Self {
        self.push(PenSegment::Away(duration));
        self
    }

    /// Adds a phase.
    pub fn push(&mut self, segment: PenSegment) {
        self.segments.push(segment);
    }

    pub fn segments(&self) -> &[PenSegment] {
        &self.segments
    }

    /// Number of pen samples (records with a position), i.e. what a host receives when it
    /// streams the whole script.
    pub fn sample_count(&self) -> usize {
        self.points().count()
    }

    /// Number of times the pen comes near the tablet (`a2` notifications).
    pub fn approach_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, PenSegment::Near(points) if !points.is_empty()))
            .count()
    }

    /// Every sample, in order.
    pub fn points(&self) -> impl Iterator<Item = &Point> {
        self.segments.iter().flat_map(|segment| match segment {
            PenSegment::Near(points) => points.as_slice(),
            PenSegment::Away(_) => &[],
        })
    }

    /// Total physical duration.
    pub fn duration(&self) -> Duration {
        self.segments.iter().map(segment_duration).sum()
    }
}

fn segment_duration(segment: &PenSegment) -> Duration {
    match segment {
        PenSegment::Near(points) if points.is_empty() => Duration::ZERO,
        PenSegment::Near(points) => {
            SAMPLE_INTERVAL * u32::try_from(points.len() + 1).unwrap_or(u32::MAX)
        }
        PenSegment::Away(duration) => *duration,
    }
}

/// One thing the pen does, at an absolute time on the virtual clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PenEvent {
    /// The pen comes near (the tablet sends `a2`).
    Enter,
    /// A record of an `a1` notification.
    Record(LiveRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimedPen {
    pub due: Duration,
    pub event: PenEvent,
}

/// The events of `script` when it starts at `start`: for each approach, `Enter` and the first
/// sample at the start of the phase, then one record every 5 ms, the "pen left" record last.
pub(crate) fn timeline(script: &PenScript, start: Duration) -> Vec<TimedPen> {
    let mut events = Vec::with_capacity(script.sample_count() + 2 * script.approach_count());
    let mut at = start;
    for segment in &script.segments {
        if let PenSegment::Near(points) = segment
            && !points.is_empty()
        {
            events.push(TimedPen {
                due: at,
                event: PenEvent::Enter,
            });
            let mut due = at;
            for point in points {
                events.push(TimedPen {
                    due,
                    event: PenEvent::Record(LiveRecord::Pen(*point)),
                });
                due += SAMPLE_INTERVAL;
            }
            events.push(TimedPen {
                due,
                event: PenEvent::Record(LiveRecord::ProximityOut),
            });
        }
        at += segment_duration(segment);
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: u16) -> Point {
        Point {
            x,
            y: 100,
            pressure: 500,
        }
    }

    #[test]
    fn counts_and_durations() {
        let script = PenScript::new()
            .near(vec![p(1), p(2), p(3)])
            .away(Duration::from_millis(100))
            .near(vec![])
            .near(vec![p(4)]);
        assert_eq!(script.sample_count(), 4);
        assert_eq!(script.approach_count(), 2);
        // 4 slots + 100 ms + 0 + 2 slots.
        assert_eq!(script.duration(), Duration::from_millis(20 + 100 + 10));
    }

    #[test]
    fn timeline_places_records_5_ms_apart() {
        let script = PenScript::new()
            .near(vec![p(1), p(2)])
            .away(Duration::from_millis(50))
            .near(vec![p(3)]);
        let start = Duration::from_secs(1);
        let ms = |n: u64| start + Duration::from_millis(n);
        let events = timeline(&script, start);
        let expected = [
            (ms(0), PenEvent::Enter),
            (ms(0), PenEvent::Record(LiveRecord::Pen(p(1)))),
            (ms(5), PenEvent::Record(LiveRecord::Pen(p(2)))),
            (ms(10), PenEvent::Record(LiveRecord::ProximityOut)),
            (ms(65), PenEvent::Enter),
            (ms(65), PenEvent::Record(LiveRecord::Pen(p(3)))),
            (ms(70), PenEvent::Record(LiveRecord::ProximityOut)),
        ];
        let actual: Vec<_> = events.iter().map(|e| (e.due, e.event)).collect();
        assert_eq!(actual, expected);
    }
}
