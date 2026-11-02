/**
 * Marks written over the presentation (docs/design/tablet-live-classroom.md, "M8: writing over
 * the presentation"): temporary strokes in page units, held by the presentation window (which
 * draws them itself) and shown on the share's viewers, never saved into the note and gone when
 * the page changes. Pure helpers; the window holds the list and `LiveRenderer` draws it.
 */
import { polylinesDistanceSq, type Vec } from './editor/geometry';
import type { InkPoint, Tool } from './ink';
import type { LiveMark } from './live';
import { INKSPACE_COLORS, type Rgba, type Stroke } from './note';

/** Line diameter of a mark at full pressure, in screen pixels at the moment it is written. */
export const MARK_WIDTH_PX = 6;
/** Radius of the eraser on the marks, screen pixels. */
export const MARK_ERASER_PX = 18;
/** The ink of the marks: red on a light page, yellow on a dark one (blackboard, slate paper). */
const MARK_ON_LIGHT: Rgba = INKSPACE_COLORS[4];
const MARK_ON_DARK: Rgba = { r: 255, g: 214, b: 0, a: 255 };

export interface MarkStyle {
  tool: Tool;
  color: Rgba;
}

export function markStyle(darkPage: boolean): MarkStyle {
  return { tool: 'feltTip', color: darkPage ? MARK_ON_DARK : MARK_ON_LIGHT };
}

/** A new mark of the given style starting at `first`; `scale` is the screen pixels per page unit. */
export function newMark(style: MarkStyle, first: InkPoint, scale: number): Stroke {
  return {
    tool: style.tool,
    color: { ...style.color },
    width: MARK_WIDTH_PX / Math.max(1e-6, scale),
    layer: 0,
    points: [first],
  };
}

/**
 * The marks a path of the eraser leaves: those within `radius` (page units) of the path are
 * removed whole. The same array comes back when nothing was touched.
 */
export function eraseMarks(marks: readonly Stroke[], path: readonly Vec[], radius: number): readonly Stroke[] {
  const kept = marks.filter((mark) => {
    // The line's own half width counts too: a thick mark is touched by the edge of the circle.
    const reach = (radius + mark.width / 2) ** 2;
    return polylinesDistanceSq(path, mark.points) > reach;
  });
  return kept.length === marks.length ? marks : kept;
}

// --- Marks for the viewers of the share ------------------------------------------------------

/** What Rust keeps of a page's marks (`live/model.rs`): the rest is not sent, so as to bound a message. */
export const MAX_SHARED_MARKS = 200;
export const MAX_SHARED_MARK_POINTS = 4000;
export const MAX_SHARED_POINTS = 20_000;

const round = (value: number, decimals: number) => {
  const k = 10 ** decimals;
  return Math.round(value * k) / k;
};

/**
 * The list as `live_set_marks` takes it: only the ink and the points (a tenth of a page unit,
 * pressure to the hundredth), within the limits Rust keeps, the first marks first.
 */
export function marksToWire(marks: readonly Stroke[]): LiveMark[] {
  let budget = MAX_SHARED_POINTS;
  const out: LiveMark[] = [];
  for (const mark of marks.slice(0, MAX_SHARED_MARKS)) {
    if (budget <= 0) break;
    const points = mark.points.slice(0, Math.min(MAX_SHARED_MARK_POINTS, budget));
    if (points.length === 0) continue;
    budget -= points.length;
    out.push({
      tool: mark.tool,
      color: { ...mark.color },
      width: round(mark.width, 1),
      points: points.map((p) => ({ x: round(p.x, 1), y: round(p.y, 1), p: round(p.pressure, 2) })),
    });
  }
  return out;
}

/** The marks of a `marks` event as strokes `LiveRenderer` draws (time is of no use to the outline). */
export function marksFromWire(marks: readonly LiveMark[]): Stroke[] {
  return marks.map((mark) => ({
    tool: mark.tool,
    color: { ...mark.color },
    width: mark.width,
    layer: 0,
    points: mark.points.map((p) => ({ x: p.x, y: p.y, pressure: p.p, t: 0 })),
  }));
}

/** Whether two marks draw the same (style, number of points, first and last point). */
function sameMark(a: Stroke, b: Stroke): boolean {
  if (a.tool !== b.tool || a.width !== b.width || a.points.length !== b.points.length) return false;
  if (a.color.r !== b.color.r || a.color.g !== b.color.g || a.color.b !== b.color.b || a.color.a !== b.color.a) return false;
  const n = a.points.length;
  if (n === 0) return true;
  const [p0, q0, p1, q1] = [a.points[0], b.points[0], a.points[n - 1], b.points[n - 1]];
  return p0.x === q0.x && p0.y === q0.y && p1.x === q1.x && p1.y === q1.y && p1.pressure === q1.pressure;
}

/**
 * `next` with the strokes of `previous` that are unchanged put back in place, so that the
 * renderer, which keeps the outline of each stroke object, computes only the mark that grew.
 */
export function reuseMarks(previous: readonly Stroke[], next: Stroke[]): Stroke[] {
  return next.map((mark, i) => (i < previous.length && sameMark(previous[i], mark) ? previous[i] : mark));
}
