//! `tidy_line`: the same handwriting, better laid out. Letter shapes do not change, except for the
//! smoothing asked for. Each word is sheared to remove the slope (x does not change, so that its
//! letters keep their places), put on the baseline and placed in turn.

use crate::geom::{
    Area, Span, UNITS, Word, area_of, cut_words, default_word_gap, fit_line, median, outline,
    samples_of, smooth_stroke,
};
use crate::orders::{TidyGaps, TidyMode};
use crate::report::{Ctx, LineReport, ReviseError};
use crate::work::Work;

/// A word's own median bottom can sit this far from the line's, in body heights.
const WORD_DRIFT_BODIES: f32 = 0.4;
/// A word of fewer bins only follows the line: its median bottom says nothing.
const WORD_MIN_BINS: usize = 4;
/// A word lower than this many body heights (punctuation) only follows the line.
const WORD_MIN_BODIES: f32 = 0.45;
/// `gaps: even`: a blank goes this far towards the line's usual blank...
const EVEN_OWN_SHARE: f32 = 0.3;
const EVEN_USUAL_SHARE: f32 = 0.7;
/// ...and stays within these shares of it.
const EVEN_MIN: f32 = 0.7;
const EVEN_MAX: f32 = 1.4;

/// The fields of the order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TidySpec {
    pub baseline: Option<f32>,
    pub left: Option<f32>,
    pub mode: TidyMode,
    pub scale: f32,
    pub slope: Option<f32>,
    pub gaps: TidyGaps,
    pub word_gap: Option<f32>,
    pub smooth: Option<f32>,
}

impl Work<'_> {
    pub(crate) fn tidy(
        &mut self,
        ctx: Ctx,
        ids: &[usize],
        spec: &TidySpec,
    ) -> Result<Option<LineReport>, ReviseError> {
        if spec.smooth.is_some() {
            self.charge_smoothing(ctx, ids)?;
        }
        let strokes = ids.iter().filter_map(|&number| self.stroke(number));
        let Some(fit) = fit_line(strokes, spec.slope) else {
            return Ok(None);
        };
        let threshold = spec.word_gap.unwrap_or(default_word_gap(fit.body));
        let spans: Vec<Span> = ids
            .iter()
            .filter_map(|&number| {
                let area = area_of(&self.stroke(number)?.points)?;
                Some(Span {
                    number,
                    x0: area.x0,
                    x1: area.x1,
                })
            })
            .collect();
        let rigid = spec.mode == TidyMode::Rigid;
        let (words, gaps) = if rigid {
            let word = Word {
                strokes: spans.iter().map(|span| span.number).collect(),
                x0: fit.x0,
                x1: fit.x1,
            };
            (vec![word], Vec::new())
        } else {
            cut_words(spans, threshold)
        };
        let mut sorted_gaps = gaps.clone();
        let usual = median(&mut sorted_gaps).unwrap_or(0.0);
        let baseline = spec.baseline.unwrap_or(fit.at(fit.x0));
        let left = spec.left.unwrap_or(fit.x0);
        let scale = spec.scale;

        let mut cursor = left;
        for (index, word) in words.iter().enumerate() {
            let Some(area) = word
                .strokes
                .iter()
                .filter_map(|&number| area_of(&self.stroke(number)?.points))
                .reduce(Area::union)
            else {
                continue;
            };
            let pivot = if rigid { fit.x0 } else { area.centre()[0] };
            let line_here = fit.at(pivot);
            // Where this word really sits, once its slope is gone.
            let bottoms = outline(
                word.strokes
                    .iter()
                    .filter_map(|&number| self.stroke(number))
                    .flat_map(|stroke| samples_of(&stroke.points))
                    .map(|[x, y]| [x, y - fit.slope * (x - pivot)]),
                true,
            );
            let drift = if rigid
                || bottoms.len() < WORD_MIN_BINS
                || area.height() < WORD_MIN_BODIES * fit.body
            {
                0.0
            } else {
                let mut rest: Vec<f32> = bottoms.iter().map(|p| p[1] - line_here).collect();
                let reach = WORD_DRIFT_BODIES * fit.body;
                median(&mut rest).unwrap_or(0.0).clamp(-reach, reach)
            };
            let base = line_here + drift;
            for &number in &word.strokes {
                let Some(stroke) = self.stroke_mut(number) else {
                    continue;
                };
                for p in &mut stroke.points {
                    let (x, y) = (p.x / UNITS, p.y / UNITS);
                    let level = y - fit.slope * (x - pivot);
                    p.y = (baseline + (level - base) * scale) * UNITS;
                    p.x = (cursor + (x - area.x0) * scale) * UNITS;
                }
                if let Some(sigma) = spec.smooth {
                    smooth_stroke(stroke, sigma);
                }
            }
            cursor += area.width() * scale;
            if let Some(&gap) = gaps.get(index) {
                let wanted = match spec.gaps {
                    TidyGaps::Keep => gap,
                    TidyGaps::Even => (EVEN_OWN_SHARE * gap + EVEN_USUAL_SHARE * usual)
                        .clamp(EVEN_MIN * usual, EVEN_MAX * usual),
                };
                cursor += wanted * scale;
            }
        }
        self.check_reach(ctx, ids)?;
        Ok(Some(
            LineReport {
                strokes: ids.len(),
                words: words.len(),
                slope: fit.slope_degrees(),
                slope_from: fit.slope_from,
                body: fit.body,
                baseline,
                left,
                word_gap: threshold,
            }
            .rounded(),
        ))
    }
}
