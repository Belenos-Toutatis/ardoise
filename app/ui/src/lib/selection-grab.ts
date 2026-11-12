/**
 * Taking hold of a selection to move it or to resize it (SPEC §4.2 « Retoucher pendant le direct », and the
 * editor's own Select tool): which part of the selection's frame a press lands on, with what tolerance for the
 * pointer in use, and the affine map a drag makes. Free of the DOM, shared by the editor (`Canvas.svelte`) and the
 * live control window (`LiveCanvas.svelte`).
 *
 * The frame is the box around the chosen strokes' ink (their samples plus half their width). A press on one of its
 * four corners (a handle) resizes around the opposite corner, a press inside it, or within a margin around it,
 * moves. A finger is less precise than a pen, a pen less than a mouse: the tolerances grow in that order.
 */
import { applyAffine, containsPoint, inflate, rectFromCorners, strokeBounds, union, type Affine, type Rect, type Vec } from './editor/geometry';
import type { PointerKind } from './gestures';
import type { Layer, Stroke } from './note';

/** How far (screen pixels) from a handle, and from the frame, a pointer still takes hold of it. */
export interface GrabTolerance {
  handle: number;
  margin: number;
}

export const GRAB_TOLERANCE: Readonly<Record<PointerKind, GrabTolerance>> = {
  mouse: { handle: 8, margin: 4 },
  pen: { handle: 16, margin: 12 },
  // The back of the pen is the pen's.
  eraser: { handle: 16, margin: 12 },
  touch: { handle: 22, margin: 16 },
};

/** The handles drawn at the corners of the frame, in screen pixels (the side of the square). */
export const HANDLE_SIZE_PX = 10;
/** A resize cannot make the strokes smaller than this share of their size, nor larger than this many times. */
export const MIN_SCALE = 0.05;
export const MAX_SCALE = 20;

/**
 * What a press took hold of: the whole selection (`move`), or the corner `corner` (0 top left, then clockwise)
 * whose opposite corner `anchor` (page units) stays where it is.
 */
export type Grab = { kind: 'move' } | { kind: 'scale'; corner: number; anchor: Vec };

/** The four corners of `r`, top left first, clockwise. */
export function corners(r: Rect): Vec[] {
  return [
    { x: r.x, y: r.y },
    { x: r.x + r.width, y: r.y },
    { x: r.x + r.width, y: r.y + r.height },
    { x: r.x, y: r.y + r.height },
  ];
}

/**
 * What a press at `p` (page units) takes hold of on `frame`, or null. `scale`: screen pixels per page unit.
 *
 * A corner (a handle) is taken from outside the frame, in the corner's own quadrant, within the pointer's full handle
 * reach; and from inside, within a third of the frame's shorter side at most (and the handle reach), so that the
 * inside of even a very small frame (a dot, an accent) is mostly the frame, which moves. Elsewhere, within the pointer's
 * margin around the frame, it moves. Of two corners in reach (a very small frame), the nearer one.
 */
export function grabAt(frame: Rect, p: Vec, scale: number, kind: PointerKind): Grab | null {
  if (!(scale > 0)) return null;
  const tolerance = GRAB_TOLERANCE[kind];
  const smaller = Math.min(frame.width, frame.height) * scale;
  const inside = Math.min(tolerance.handle, smaller / 3);
  const cs = corners(frame);
  let nearest = -1;
  let best = Infinity;
  for (let k = 0; k < 4; k++) {
    // How far past the corner, outwards, on each axis (screen pixels): negative inside the frame.
    const ox = (k === 0 || k === 3 ? cs[k].x - p.x : p.x - cs[k].x) * scale;
    const oy = (k === 0 || k === 1 ? cs[k].y - p.y : p.y - cs[k].y) * scale;
    const reached =
      ox >= 0 && oy >= 0 ? ox <= tolerance.handle && oy <= tolerance.handle : ox <= 0 && oy <= 0 && -ox <= inside && -oy <= inside;
    if (reached && Math.abs(ox) + Math.abs(oy) < best) {
      best = Math.abs(ox) + Math.abs(oy);
      nearest = k;
    }
  }
  if (nearest >= 0) return { kind: 'scale', corner: nearest, anchor: cs[(nearest + 2) % 4] };
  return containsPoint(inflate(frame, tolerance.margin / scale), p) ? { kind: 'move' } : null;
}

/** The resize cursor of a handle: the diagonal it lies on. */
export function resizeCursor(corner: number): 'nwse-resize' | 'nesw-resize' {
  return corner % 2 === 0 ? 'nwse-resize' : 'nesw-resize';
}

/**
 * The factor a resize drag makes: the pointer's position projected onto the diagonal that goes from `anchor` to
 * the grabbed corner `start` (1 at the corner, 0 at the anchor), kept within `[MIN_SCALE, max]`.
 */
export function resizeFactor(anchor: Vec, start: Vec, p: Vec, max = Infinity): number {
  const ax = start.x - anchor.x;
  const ay = start.y - anchor.y;
  const length = ax * ax + ay * ay;
  const f = length > 0 ? ((p.x - anchor.x) * ax + (p.y - anchor.y) * ay) / length : 1;
  return Math.min(max, Math.max(MIN_SCALE, f));
}

/**
 * A change of the page: each point (x, y) becomes (x·scale + tx, y·scale + ty), widths follow `scale` (what
 * `live_edit_strokes` takes as a `transform`). Uniform: a move has `scale` 1.
 */
export interface PageTransform {
  scale: number;
  x: number;
  y: number;
}

export const NO_TRANSFORM: PageTransform = { scale: 1, x: 0, y: 0 };

/** Moving by (`dx`, `dy`). */
export function moveTransform(dx: number, dy: number): PageTransform {
  return { scale: 1, x: dx, y: dy };
}

/** Scaling by `factor` around `anchor`, which stays where it is. */
export function scaleTransform(anchor: Vec, factor: number): PageTransform {
  return { scale: factor, x: anchor.x * (1 - factor), y: anchor.y * (1 - factor) };
}

/** What `t` changes nothing of (to the pixel, at `scale` screen pixels per page unit). */
export function isNoTransform(t: PageTransform, scale: number): boolean {
  return Math.abs(t.scale - 1) < 1e-4 && Math.abs(t.x) * scale < 0.05 && Math.abs(t.y) * scale < 0.05;
}

/** `t` as the editor's affine map. */
export function toAffine(t: PageTransform): Affine {
  return { a: t.scale, b: 0, c: 0, d: t.scale, e: t.x, f: t.y };
}

/** The box `r` becomes under `t`. */
export function transformRect(r: Rect, t: PageTransform): Rect {
  const m = toAffine(t);
  return rectFromCorners(applyAffine(m, { x: r.x, y: r.y }), applyAffine(m, { x: r.x + r.width, y: r.y + r.height }));
}

/** The frame of the chosen strokes of a page, hidden layers left out: null when none of them shows. */
export function selectionFrame(strokes: readonly Stroke[], layers: readonly Layer[], indices: readonly number[]): Rect | null {
  let box: Rect | null = null;
  for (const i of indices) {
    const stroke = strokes[i];
    if (!stroke || layers[stroke.layer]?.visible === false) continue;
    box = union(box, strokeBounds(stroke));
  }
  return box;
}
