/**
 * Hit tests: drawing order, lasso selection, click selection, eraser contact. Only strokes on
 * visible layers can be hit; a stroke whose layer has no entry is drawn (and hit) on top.
 */
import type { Note, Stroke } from '../note';
import {
  boundsOfPoints,
  inflate,
  overlaps,
  pointInPolygon,
  polylineDistanceSq,
  polylinesDistanceSq,
  strokeBounds,
  strokeCentreBounds,
  type Vec,
} from './geometry';

/** What the hit tests read of a page: the editor's note, or the live session's strokes and layers. */
export type Hittable = Pick<Note, 'strokes' | 'layers'>;

/** A stroke on a layer index without entry counts as visible. */
export function layerVisible(note: Hittable, layer: number): boolean {
  const entry = note.layers[layer];
  return entry ? entry.visible : true;
}

/** Indices of the visible strokes, in drawing order: layer by layer, then file order. */
export function drawingOrder(note: Hittable): number[] {
  const buckets: number[][] = note.layers.map(() => []);
  const orphans: number[] = [];
  note.strokes.forEach((stroke, i) => {
    const bucket = buckets[stroke.layer];
    if (bucket) bucket.push(i);
    else orphans.push(i);
  });
  const order: number[] = [];
  note.layers.forEach((layer, l) => {
    if (layer.visible) order.push(...buckets[l]);
  });
  order.push(...orphans);
  return order;
}

/**
 * Strokes enclosed by the lasso `polygon`: at least `share` of their samples inside it. Indices
 * in file order.
 */
export function selectInLasso(note: Hittable, polygon: readonly Vec[], share = 0.5): number[] {
  const area = boundsOfPoints(polygon);
  if (!area || polygon.length < 3) return [];
  const selected: number[] = [];
  note.strokes.forEach((stroke, i) => {
    if (!layerVisible(note, stroke.layer)) return;
    const bounds = strokeCentreBounds(stroke);
    if (!bounds || !overlaps(bounds, area)) return;
    let inside = 0;
    for (const p of stroke.points) if (pointInPolygon(p, polygon)) inside++;
    if (inside > 0 && inside >= share * stroke.points.length) selected.push(i);
  });
  return selected;
}

function touches(stroke: Stroke, path: readonly Vec[], radius: number): boolean {
  const reach = radius + stroke.width / 2;
  const bounds = strokeCentreBounds(stroke);
  const area = boundsOfPoints(path);
  if (!bounds || !area || !overlaps(inflate(bounds, reach), area)) return false;
  return polylinesDistanceSq(path, stroke.points) <= reach * reach;
}

/** The topmost visible stroke whose ink passes within `tolerance` of `p`, or null. */
export function hitStroke(note: Hittable, p: Vec, tolerance: number): number | null {
  const order = drawingOrder(note);
  for (let k = order.length - 1; k >= 0; k--) {
    const stroke = note.strokes[order[k]];
    const bounds = strokeBounds(stroke);
    if (!bounds || !overlaps(inflate(bounds, tolerance), { x: p.x, y: p.y, width: 0, height: 0 })) continue;
    const reach = tolerance + stroke.width / 2;
    if (polylineDistanceSq(p, stroke.points) <= reach * reach) return order[k];
  }
  return null;
}

/** Visible strokes whose ink the eraser path (a polyline of radius `radius`) touches. */
export function strokesTouched(note: Hittable, path: readonly Vec[], radius: number): number[] {
  const touched: number[] = [];
  note.strokes.forEach((stroke, i) => {
    if (layerVisible(note, stroke.layer) && touches(stroke, path, radius)) touched.push(i);
  });
  return touched;
}
