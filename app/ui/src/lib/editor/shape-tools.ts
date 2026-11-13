/**
 * The shape tools (SPEC §4.4 « Outils de tracé », docs/questions.md Q20, docs/design/shape-tools.md): a line, a
 * rectangle or an ellipse dragged between two points, which becomes an ordinary stroke. Pure geometry, in page units
 * (device units, 10 µm, y down), free of the DOM so that the rules are tested alone; `Canvas.svelte` feeds it the
 * pointer and draws what it gives back.
 *
 * - `resolveShape`: the two points, the three options (regular, from the centre, snapped to the page's template) and the
 *   minimal size give a `ShapeSpec`, or null for a click. Nothing is straightened unless an option asks for it.
 * - `shapePath` / `shapePoints`: the spec as the points of one stroke, as fine as the ones « Redresser les formes »
 *   makes (`ardoise_ink::shapes`), so that the outline smooths the curves and keeps the corners (the outline keeps a
 *   turn of more than 60° sharp: a rectangle's corners are samples of the stroke).
 * - `buildShapeStroke`: the whole stroke, in the current ink, colour and width.
 */
import type { InkPoint, Tool } from '../ink';
import type { Rgba, Stroke } from '../note';
import type { Vec } from './geometry';

export type ShapeKind = 'line' | 'rectangle' | 'ellipse';

export const SHAPE_KINDS: readonly ShapeKind[] = ['line', 'rectangle', 'ellipse'];

export function isShapeKind(value: string): value is ShapeKind {
  return (SHAPE_KINDS as readonly string[]).includes(value);
}

/** The three options of the tools, as the toggles keep them. */
export interface ShapeToggles {
  /** Square, circle; a line on the horizontal, the vertical or a diagonal. */
  regular: boolean;
  /** The first point is the centre. */
  centre: boolean;
  /** The points snap to the lines of the page's template. */
  snap: boolean;
}

/**
 * What the toggles mean for one stroke: Shift inverts « Forme régulière », Alt inverts « Depuis le centre » (the
 * snapping has no key).
 */
export function effectiveToggles(toggles: ShapeToggles, keys: { shift?: boolean; alt?: boolean }): ShapeToggles {
  return { regular: toggles.regular !== !!keys.shift, centre: toggles.centre !== !!keys.alt, snap: toggles.snap };
}

// --- The snap grid ---------------------------------------------------------------------------

/** One axis of the points to snap to: `first + i × pitch` for `i` in `0..count`, in device units. */
export interface SnapAxis {
  first: number;
  pitch: number;
  count: number;
}

/**
 * The lines (or dots) the page's template prints, one lattice per axis; an axis without one is free (the vertical of
 * ruled paper). Rust gives it (`template_snap_grid`, from the geometry the template is drawn with).
 */
export interface SnapGrid {
  x: SnapAxis | null;
  y: SnapAxis | null;
}

/** The nearest position of the axis to `value` (the ends of the lattice included), or `value` without an axis. */
export function snapToAxis(value: number, axis: SnapAxis | null): number {
  if (!axis || axis.count <= 0 || !(axis.pitch > 0)) return value;
  const index = Math.min(axis.count - 1, Math.max(0, Math.round((value - axis.first) / axis.pitch)));
  return axis.first + index * axis.pitch;
}

export function snapPoint(point: Vec, grid: SnapGrid | null): Vec {
  return grid ? { x: snapToAxis(point.x, grid.x), y: snapToAxis(point.y, grid.y) } : point;
}

// --- What is drawn ---------------------------------------------------------------------------

export type ShapeSpec =
  | { kind: 'line'; a: Vec; b: Vec }
  /** The four corners, the first being where the stroke starts, then round the box. */
  | { kind: 'rectangle'; corners: [Vec, Vec, Vec, Vec] }
  /** `start`: the parametric angle where the stroke starts. */
  | { kind: 'ellipse'; centre: Vec; rx: number; ry: number; start: number };

/** tan 22.5°: below it a line is closer to an axis than to a diagonal. */
const TAN_22_5 = Math.tan(Math.PI / 8);

const sign = (v: number) => (v < 0 ? -1 : 1);

/**
 * The end of a line made regular: horizontal, vertical or a 45° diagonal, whichever the drag is closest to. An axis
 * keeps the length along it; a diagonal takes the larger of the two sides.
 */
function regularLineEnd(a: Vec, e: Vec): Vec {
  const dx = e.x - a.x;
  const dy = e.y - a.y;
  const [ax, ay] = [Math.abs(dx), Math.abs(dy)];
  if (ay <= ax * TAN_22_5) return { x: e.x, y: a.y };
  if (ax <= ay * TAN_22_5) return { x: a.x, y: e.y };
  const m = Math.max(ax, ay);
  return { x: a.x + sign(dx) * m, y: a.y + sign(dy) * m };
}

/** The end of a square or of a circle's box: both sides as long as the longer. */
function regularBoxEnd(a: Vec, e: Vec): Vec {
  const dx = e.x - a.x;
  const dy = e.y - a.y;
  const m = Math.max(Math.abs(dx), Math.abs(dy));
  return { x: a.x + sign(dx) * m, y: a.y + sign(dy) * m };
}

export interface ShapeRequest {
  kind: ShapeKind;
  /** Where the pointer went down, and where it is now. */
  start: Vec;
  end: Vec;
  /** The options of this stroke (`effectiveToggles`: the keys already counted). */
  toggles: ShapeToggles;
  /** The page's snap grid; null: the page has no lines to snap to (the option then does nothing). */
  grid: SnapGrid | null;
  /** Smallest side (the length of a line), page units: a click draws nothing. */
  minSize: number;
}

/**
 * The shape between the points, or null when it is smaller than `minSize` (a click, a drag that stayed put, two
 * points that snapped to the same dot). In order: the points snap, the end is made regular, the shape takes the first
 * point as a corner or as its centre.
 *
 * A regular shape beats the snapping where they disagree: on ruled paper (only the horizontals to snap to) a square
 * keeps its sides equal, so its height may stop between two rules.
 */
export function resolveShape(request: ShapeRequest): ShapeSpec | null {
  const { kind, toggles, minSize } = request;
  const grid = toggles.snap ? request.grid : null;
  const a = snapPoint(request.start, grid);
  let e = snapPoint(request.end, grid);
  if (toggles.regular) e = kind === 'line' ? regularLineEnd(a, e) : regularBoxEnd(a, e);
  const dx = e.x - a.x;
  const dy = e.y - a.y;
  if (kind === 'line') {
    const length = Math.hypot(dx, dy) * (toggles.centre ? 2 : 1);
    if (!(length >= minSize) || !(length > 0)) return null;
    return toggles.centre
      ? { kind, a: { x: a.x - dx, y: a.y - dy }, b: { x: a.x + dx, y: a.y + dy } }
      : { kind, a, b: e };
  }
  // The box: two corners, or (from the centre) the first point's mirror image about itself.
  const box = toggles.centre
    ? { x0: a.x - Math.abs(dx), y0: a.y - Math.abs(dy), x1: a.x + Math.abs(dx), y1: a.y + Math.abs(dy) }
    : { x0: Math.min(a.x, e.x), y0: Math.min(a.y, e.y), x1: Math.max(a.x, e.x), y1: Math.max(a.y, e.y) };
  const [w, h] = [box.x1 - box.x0, box.y1 - box.y0];
  if (!(w >= minSize && h >= minSize) || !(w > 0 && h > 0)) return null;
  // The stroke starts at the corner the pointer went down on or, from the centre (which is no corner), the one it is on.
  const right = toggles.centre ? dx > 0 : dx < 0;
  const bottom = toggles.centre ? dy > 0 : dy < 0;
  const [startX, endX] = right ? [box.x1, box.x0] : [box.x0, box.x1];
  const [startY, endY] = bottom ? [box.y1, box.y0] : [box.y0, box.y1];
  if (kind === 'rectangle') {
    return {
      kind,
      corners: [
        { x: startX, y: startY },
        { x: endX, y: startY },
        { x: endX, y: endY },
        { x: startX, y: endY },
      ],
    };
  }
  return {
    kind,
    centre: { x: (box.x0 + box.x1) / 2, y: (box.y0 + box.y1) / 2 },
    rx: w / 2,
    ry: h / 2,
    start: Math.atan2(bottom ? 1 : -1, right ? 1 : -1),
  };
}

// --- The points of the stroke ----------------------------------------------------------------

/**
 * The largest distance between two points of a shape, device units (0.5 mm): the one of the shapes « Redresser les
 * formes » makes (`ardoise_ink::shapes::SAMPLE_SPACING`), so that every renderer smooths them the same way.
 */
export const SAMPLE_SPACING = 50;
/** The most points of one shape (the spacing grows for huge ones). */
const MAX_POINTS = 4000;
/** The fewest points of an ellipse: even a small one is smooth through them. */
const MIN_ELLIPSE_POINTS = 72;

function pathLength(points: readonly Vec[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i++) total += Math.hypot(points[i].x - points[i - 1].x, points[i].y - points[i - 1].y);
  return total;
}

/** The polyline through `vertices` with points at most `SAMPLE_SPACING` apart; the vertices are samples, so corners stay corners. */
function densify(vertices: readonly Vec[]): Vec[] {
  const spacing = Math.max(SAMPLE_SPACING, pathLength(vertices) / MAX_POINTS);
  const out: Vec[] = [vertices[0]];
  for (let k = 1; k < vertices.length; k++) {
    const [p, q] = [vertices[k - 1], vertices[k]];
    const steps = Math.max(1, Math.ceil(Math.hypot(q.x - p.x, q.y - p.y) / spacing));
    for (let i = 1; i <= steps; i++) out.push({ x: p.x + ((q.x - p.x) * i) / steps, y: p.y + ((q.y - p.y) * i) / steps });
  }
  return out;
}

/**
 * The centre line of the shape: a line is its two ends densified; a rectangle goes round its four corners and ends on
 * the first; an ellipse is sampled at equal angles (`MIN_ELLIPSE_POINTS` at least, one every `SAMPLE_SPACING` of its
 * long axis' circle) and ends on its first point too.
 */
export function shapePath(spec: ShapeSpec): Vec[] {
  if (spec.kind === 'line') return densify([spec.a, spec.b]);
  if (spec.kind === 'rectangle') return densify([...spec.corners, spec.corners[0]]);
  const n = Math.min(MAX_POINTS, Math.max(MIN_ELLIPSE_POINTS, Math.ceil((2 * Math.PI * Math.max(spec.rx, spec.ry)) / SAMPLE_SPACING)));
  const points: Vec[] = [];
  for (let i = 0; i < n; i++) {
    const phi = spec.start + (2 * Math.PI * i) / n;
    points.push({ x: spec.centre.x + spec.rx * Math.cos(phi), y: spec.centre.y + spec.ry * Math.sin(phi) });
  }
  points.push({ ...points[0] });
  return points;
}

/**
 * Every ink draws exactly its nominal width at full pressure (`ardoise_ink`, « Widths »): a shape is as wide as the
 * width chosen, whatever the pen, where a mouse stroke (0.6) comes out a little finer.
 */
export const SHAPE_PRESSURE = 1;

/** The fewest milliseconds a shape takes to draw in a replay. */
export const MIN_SHAPE_MS = 150;
/** The most, outside a recording (a shape dragged slowly is still drawn in a moment). */
export const MAX_SHAPE_MS = 2000;

/** How long the shape takes in the replay: the drag's own time, within the limits (a recording keeps the real time). */
export function shapeDuration(dragMs: number, recording: boolean): number {
  const real = Number.isFinite(dragMs) ? Math.round(dragMs) : 0;
  return Math.max(MIN_SHAPE_MS, recording ? real : Math.min(MAX_SHAPE_MS, real));
}

/**
 * The points of the path with the pressure `SHAPE_PRESSURE` and times from `base` to `base + durationMs`, in
 * proportion to the distance along the path, so that the replay draws the shape progressively (never going back).
 */
export function shapePoints(path: readonly Vec[], base: number, durationMs: number): InkPoint[] {
  const total = pathLength(path);
  let travelled = 0;
  return path.map((p, i) => {
    if (i > 0) travelled += Math.hypot(p.x - path[i - 1].x, p.y - path[i - 1].y);
    return { x: p.x, y: p.y, pressure: SHAPE_PRESSURE, t: Math.round(base + (total > 0 ? (durationMs * travelled) / total : 0)) };
  });
}

export interface ShapeStrokeRequest extends ShapeRequest {
  tool: Tool;
  color: Rgba;
  width: number;
  /** Note time of the first point, and how long the shape takes (`shapeDuration`). */
  base: number;
  durationMs: number;
}

/** The stroke of the shape (the layer is the document's to give), or null when the drag was too small. */
export function buildShapeStroke(request: ShapeStrokeRequest): Omit<Stroke, 'layer'> | null {
  const spec = resolveShape(request);
  if (!spec) return null;
  return {
    tool: request.tool,
    color: { ...request.color },
    width: request.width,
    points: shapePoints(shapePath(spec), request.base, request.durationMs),
  };
}
