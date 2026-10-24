/**
 * Plane geometry for the editor, in page coordinates (device units, 10 µm, y down): rectangles,
 * distances, point-in-polygon, affine maps, stroke bounds.
 */
import type { InkPoint } from '../ink';
import type { Stroke } from '../note';

export interface Vec {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** `x' = a·x + c·y + e`, `y' = b·x + d·y + f` (the SVG `matrix(a b c d e f)`). */
export interface Affine {
  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  f: number;
}

export const IDENTITY: Affine = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };

export function translation(dx: number, dy: number): Affine {
  return { a: 1, b: 0, c: 0, d: 1, e: dx, f: dy };
}

/** Scaling by `sx`, `sy` around `origin`. */
export function scalingAround(origin: Vec, sx: number, sy: number = sx): Affine {
  return { a: sx, b: 0, c: 0, d: sy, e: origin.x * (1 - sx), f: origin.y * (1 - sy) };
}

export function applyAffine(m: Affine, p: Vec): Vec {
  return { x: m.a * p.x + m.c * p.y + m.e, y: m.b * p.x + m.d * p.y + m.f };
}

/** Factor by which lengths change under `m` (square root of the determinant). */
export function affineScale(m: Affine): number {
  return Math.sqrt(Math.abs(m.a * m.d - m.b * m.c));
}

export function svgMatrix(m: Affine): string {
  return `matrix(${m.a} ${m.b} ${m.c} ${m.d} ${m.e} ${m.f})`;
}

/** The rectangle spanned by two corners, in any order. */
export function rectFromCorners(p: Vec, q: Vec): Rect {
  return { x: Math.min(p.x, q.x), y: Math.min(p.y, q.y), width: Math.abs(q.x - p.x), height: Math.abs(q.y - p.y) };
}

export function inflate(r: Rect, by: number): Rect {
  return { x: r.x - by, y: r.y - by, width: r.width + 2 * by, height: r.height + 2 * by };
}

export function union(a: Rect | null, b: Rect | null): Rect | null {
  if (!a) return b;
  if (!b) return a;
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, width: Math.max(a.x + a.width, b.x + b.width) - x, height: Math.max(a.y + a.height, b.y + b.height) - y };
}

/** The overlap of two rectangles, or null when they do not overlap. */
export function intersection(a: Rect, b: Rect): Rect | null {
  const x = Math.max(a.x, b.x);
  const y = Math.max(a.y, b.y);
  const right = Math.min(a.x + a.width, b.x + b.width);
  const bottom = Math.min(a.y + a.height, b.y + b.height);
  return right >= x && bottom >= y ? { x, y, width: right - x, height: bottom - y } : null;
}

export function overlaps(a: Rect, b: Rect): boolean {
  return a.x <= b.x + b.width && b.x <= a.x + a.width && a.y <= b.y + b.height && b.y <= a.y + a.height;
}

export function containsPoint(r: Rect, p: Vec): boolean {
  return p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height;
}

/** Bounding box of `points`, or null for none. */
export function boundsOfPoints(points: readonly Vec[]): Rect | null {
  if (points.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of points) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

const centreBounds = new WeakMap<Stroke, Rect | null>();

/** Bounding box of a stroke's samples (cached per stroke object: strokes are immutable). */
export function strokeCentreBounds(stroke: Stroke): Rect | null {
  let bounds = centreBounds.get(stroke);
  if (bounds === undefined) {
    bounds = boundsOfPoints(stroke.points);
    centreBounds.set(stroke, bounds);
  }
  return bounds;
}

/** Bounding box of a stroke's ink: its samples plus half its nominal width. */
export function strokeBounds(stroke: Stroke): Rect | null {
  const bounds = strokeCentreBounds(stroke);
  return bounds ? inflate(bounds, stroke.width / 2) : null;
}

export function distanceSq(p: Vec, q: Vec): number {
  const dx = p.x - q.x;
  const dy = p.y - q.y;
  return dx * dx + dy * dy;
}

/** Squared distance from `p` to the segment `a`–`b`. */
export function segmentDistanceSq(p: Vec, a: Vec, b: Vec): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = dx * dx + dy * dy;
  if (length === 0) return distanceSq(p, a);
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length));
  return distanceSq(p, { x: a.x + t * dx, y: a.y + t * dy });
}

function cross(o: Vec, a: Vec, b: Vec): number {
  return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
}

function segmentsCross(a: Vec, b: Vec, c: Vec, d: Vec): boolean {
  const d1 = cross(c, d, a);
  const d2 = cross(c, d, b);
  const d3 = cross(a, b, c);
  const d4 = cross(a, b, d);
  return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

/** Squared distance between the segments `a`–`b` and `c`–`d` (0 when they cross). */
export function segmentsDistanceSq(a: Vec, b: Vec, c: Vec, d: Vec): number {
  if (segmentsCross(a, b, c, d)) return 0;
  return Math.min(
    segmentDistanceSq(a, c, d),
    segmentDistanceSq(b, c, d),
    segmentDistanceSq(c, a, b),
    segmentDistanceSq(d, a, b),
  );
}

/** Squared distance from `p` to a polyline (a single point counts as a polyline). */
export function polylineDistanceSq(p: Vec, line: readonly Vec[]): number {
  if (line.length === 0) return Infinity;
  if (line.length === 1) return distanceSq(p, line[0]);
  let best = Infinity;
  for (let i = 1; i < line.length; i++) {
    const d = segmentDistanceSq(p, line[i - 1], line[i]);
    if (d < best) best = d;
  }
  return best;
}

/** Squared distance between two polylines. */
export function polylinesDistanceSq(p: readonly Vec[], q: readonly Vec[]): number {
  if (p.length === 0 || q.length === 0) return Infinity;
  if (p.length === 1) return polylineDistanceSq(p[0], q);
  if (q.length === 1) return polylineDistanceSq(q[0], p);
  let best = Infinity;
  for (let i = 1; i < p.length; i++) {
    for (let j = 1; j < q.length; j++) {
      const d = segmentsDistanceSq(p[i - 1], p[i], q[j - 1], q[j]);
      if (d < best) {
        best = d;
        if (best === 0) return 0;
      }
    }
  }
  return best;
}

/** True when `p` lies inside the closed polygon `polygon` (even-odd rule). */
export function pointInPolygon(p: Vec, polygon: readonly Vec[]): boolean {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[i];
    const b = polygon[j];
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

/**
 * The samples with extra ones interpolated (position, pressure, time) so that no two
 * consecutive samples are more than `step` apart. Returns `points` itself when nothing is added.
 */
export function densify(points: readonly InkPoint[], step: number): readonly InkPoint[] {
  if (step <= 0 || points.length < 2) return points;
  let added = false;
  const out: InkPoint[] = [points[0]];
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const n = Math.ceil(Math.sqrt(distanceSq(a, b)) / step);
    for (let k = 1; k < n; k++) {
      const f = k / n;
      out.push({
        x: a.x + (b.x - a.x) * f,
        y: a.y + (b.y - a.y) * f,
        pressure: a.pressure + (b.pressure - a.pressure) * f,
        t: a.t + (b.t - a.t) * f,
      });
      added = true;
    }
    out.push(b);
  }
  return added ? out : points;
}
