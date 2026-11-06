//! Where strokes go in time. `t_after` is the end of the stroke they come after; `next` is the
//! first start among the other strokes after it. The strokes come one after the other, with their
//! own duration and short pauses. When they fit before `next`, nothing else moves. When they do not:
//! without a narration, what starts at or after `next` moves on by what is missing; with one,
//! nothing may move (ink and voice would part), so they are drawn faster. Times are `f64`: `f32`
//! cannot hold a clock of hours to the millisecond.

use std::collections::HashSet;

use crate::report::{Ctx, ReviseError, TimeReport};
use crate::work::{Work, span_of, start_of};

/// The pause before each stroke placed, and after the last one, milliseconds.
pub(crate) const PAUSE_MS: u64 = 40;
/// With a narration, the strokes end this far before what follows at the least, milliseconds.
const SQUEEZE_MARGIN_MS: u64 = 2;

impl Work<'_> {
    /// Places the strokes `numbers`, in this order: right after stroke `after`, or at the end of
    /// the note. `None` when there is nothing to place. `after` was checked by the caller.
    pub(crate) fn place(
        &mut self,
        ctx: Ctx,
        numbers: &[usize],
        after: Option<usize>,
    ) -> Result<Option<TimeReport>, ReviseError> {
        let durations: Vec<(usize, u64)> = numbers
            .iter()
            .filter_map(|&number| {
                let (start, end) = span_of(self.stroke(number)?)?;
                Some((number, u64::from(end - start)))
            })
            .collect();
        if durations.is_empty() {
            return Ok(None);
        }
        let placed: HashSet<usize> = durations.iter().map(|d| d.0).collect();
        for &number in &placed {
            if let Some(slot) = self.slots.get_mut(number) {
                slot.placed_by = Some(ctx.index);
            }
        }

        let mut pace = 1.0f64;
        let mut pushed_ms = 0u64;
        let mut pushed_strokes = 0usize;
        let anchor = match after {
            None => self
                .slots
                .iter()
                .enumerate()
                .filter(|(number, _)| !placed.contains(number))
                .filter_map(|(_, slot)| span_of(slot.stroke.as_ref()?))
                .map(|span| u64::from(span.1))
                .max()
                .unwrap_or(0),
            Some(after) => {
                let Some((_, t_after)) = self.stroke(after).and_then(span_of) else {
                    return Err(ctx.err(format!("stroke {after} has no points")));
                };
                let t_after = u64::from(t_after);
                let needed: u64 = durations.iter().map(|d| d.1 + PAUSE_MS).sum::<u64>() + PAUSE_MS;
                let next = self
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(number, _)| *number != after && !placed.contains(number))
                    .filter_map(|(_, slot)| start_of(slot.stroke.as_ref()?))
                    .map(u64::from)
                    .filter(|&start| start >= t_after)
                    .min();
                if let Some(next) = next {
                    let gap = next - t_after;
                    if needed > gap {
                        if self.input.narration {
                            pace = gap.saturating_sub(SQUEEZE_MARGIN_MS) as f64 / needed as f64;
                            self.faster.extend(placed.iter().copied());
                        } else {
                            pushed_ms = needed - gap;
                            pushed_strokes = self.push_from(next, pushed_ms, after, &placed);
                        }
                    }
                }
                t_after
            }
        };

        let mut cursor = anchor as f64 + PAUSE_MS as f64 * pace;
        let (mut first_ms, mut last_ms) = (u32::MAX, 0u32);
        for &(number, duration) in &durations {
            let Some(stroke) = self.stroke_mut(number) else {
                continue;
            };
            let start = stroke.points.first().map_or(0, |p| p.t);
            let mut latest = start;
            for point in &mut stroke.points {
                latest = latest.max(point.t);
                let at = (cursor + f64::from(latest - start) * pace).floor();
                point.t = at.min(f64::from(u32::MAX)) as u32;
                first_ms = first_ms.min(point.t);
                last_ms = last_ms.max(point.t);
            }
            cursor += (duration + PAUSE_MS) as f64 * pace;
        }
        Ok(Some(TimeReport {
            after,
            start_ms: first_ms,
            end_ms: last_ms,
            pushed_ms: u32::try_from(pushed_ms).unwrap_or(u32::MAX),
            pushed_strokes,
            pace: pace as f32,
        }))
    }

    /// Moves on by `shift` every stroke starting at or after `next`, but the stroke `after` and
    /// those being placed. Gives how many moved.
    fn push_from(&mut self, next: u64, shift: u64, after: usize, placed: &HashSet<usize>) -> usize {
        let step = u32::try_from(shift).unwrap_or(u32::MAX);
        let mut moved = 0;
        for (number, slot) in self.slots.iter_mut().enumerate() {
            if number == after || placed.contains(&number) {
                continue;
            }
            let Some(stroke) = slot.stroke.as_mut() else {
                continue;
            };
            if start_of(stroke).is_none_or(|start| u64::from(start) < next) {
                continue;
            }
            for point in &mut stroke.points {
                point.t = point.t.saturating_add(step);
            }
            moved += 1;
            self.pushed.insert(number);
        }
        self.pushed_ms = self.pushed_ms.saturating_add(shift);
        moved
    }
}
