/**
 * Pure operations on a note. Each returns a new note (or the same object when nothing changed)
 * and keeps every untouched stroke object, so that cached outlines and bounds stay valid.
 */
import type { InkPoint, Tool } from '../ink';
import type { Background, Layer, Note, Rgba, Stroke } from '../note';
import {
  affineScale,
  applyAffine,
  boundsOfPoints,
  densify,
  inflate,
  intersection,
  overlaps,
  polylineDistanceSq,
  strokeBounds,
  strokeCentreBounds,
  translation,
  union,
  type Affine,
  type Rect,
  type Vec,
} from './geometry';
import { layerVisible, strokesTouched } from './hit';

export interface StrokeStyle {
  color?: Rgba;
  tool?: Tool;
  width?: number;
}

function withStrokes(note: Note, strokes: Stroke[]): Note {
  return { ...note, strokes };
}

function indexSet(indices: Iterable<number>): Set<number> {
  return new Set(indices);
}

export function addStroke(note: Note, stroke: Stroke): Note {
  return withStrokes(note, [...note.strokes, stroke]);
}

export function removeStrokes(note: Note, indices: Iterable<number>): Note {
  const remove = indexSet(indices);
  if (remove.size === 0) return note;
  const strokes = note.strokes.filter((_, i) => !remove.has(i));
  return strokes.length === note.strokes.length ? note : withStrokes(note, strokes);
}

/** Removes every visible stroke the eraser path touches. */
export function eraseWhole(note: Note, path: readonly Vec[], radius: number): Note {
  return removeStrokes(note, strokesTouched(note, path, radius));
}

/**
 * Removes the parts of visible strokes within `radius` of the eraser path: samples closer than
 * that go (long segments are first subdivided so that the eraser cuts them too), and what is
 * left of a stroke becomes one stroke per remaining run of at least two samples, in place of
 * the original (same drawing order).
 */
export function erasePartial(note: Note, path: readonly Vec[], radius: number): Note {
  const area = boundsOfPoints(path);
  if (!area || radius <= 0) return note;
  const reach = inflate(area, radius);
  const r2 = radius * radius;
  let changed = false;
  const strokes: Stroke[] = [];
  for (const stroke of note.strokes) {
    const bounds = strokeCentreBounds(stroke);
    if (!bounds || !layerVisible(note, stroke.layer) || !overlaps(bounds, reach)) {
      strokes.push(stroke);
      continue;
    }
    const points = densify(stroke.points, radius / 2);
    const keep = points.map((p) => polylineDistanceSq(p, path) > r2);
    if (keep.every(Boolean)) {
      strokes.push(stroke);
      continue;
    }
    changed = true;
    let run: InkPoint[] = [];
    const flush = () => {
      if (run.length >= 2) strokes.push({ ...stroke, points: run });
      run = [];
    };
    points.forEach((p, i) => {
      if (keep[i]) run.push(p);
      else flush();
    });
    flush();
  }
  return changed ? withStrokes(note, strokes) : note;
}

/** Changes colour, tool and/or width of the strokes at `indices`, or of every stroke if null. */
export function setStyle(note: Note, indices: Iterable<number> | null, style: StrokeStyle): Note {
  const only = indices ? indexSet(indices) : null;
  let changed = false;
  const strokes = note.strokes.map((stroke, i) => {
    if (only && !only.has(i)) return stroke;
    const color = style.color ?? stroke.color;
    const tool = style.tool ?? stroke.tool;
    const width = style.width ?? stroke.width;
    const sameColor =
      color.r === stroke.color.r && color.g === stroke.color.g && color.b === stroke.color.b && color.a === stroke.color.a;
    if (sameColor && tool === stroke.tool && width === stroke.width) return stroke;
    changed = true;
    return { ...stroke, color: { ...color }, tool, width };
  });
  return changed ? withStrokes(note, strokes) : note;
}

/** Applies `m` to the samples of the strokes at `indices`; widths follow the change of scale. */
export function transformStrokes(note: Note, indices: Iterable<number>, m: Affine): Note {
  const only = indexSet(indices);
  if (only.size === 0) return note;
  const factor = affineScale(m);
  const strokes = note.strokes.map((stroke, i) =>
    only.has(i)
      ? {
          ...stroke,
          width: stroke.width * factor,
          points: stroke.points.map((p) => ({ ...p, ...applyAffine(m, p) })),
        }
      : stroke,
  );
  return withStrokes(note, strokes);
}

/**
 * Copies of `strokes` shifted by `offset`, appended on `layer`; returns the note and the
 * indices of the copies.
 */
export function insertCopies(
  note: Note,
  strokes: readonly Stroke[],
  offset: Vec,
  layer: number,
): { note: Note; indices: number[] } {
  if (strokes.length === 0) return { note, indices: [] };
  const m = translation(offset.x, offset.y);
  const copies = strokes.map((stroke) => ({
    ...stroke,
    color: { ...stroke.color },
    layer,
    points: stroke.points.map((p) => ({ ...p, ...applyAffine(m, p) })),
  }));
  const start = note.strokes.length;
  return { note: withStrokes(note, [...note.strokes, ...copies]), indices: copies.map((_, i) => start + i) };
}

/** Ink bounds of the strokes at `indices` (or of every visible stroke if null). */
export function strokesBounds(note: Note, indices: Iterable<number> | null): Rect | null {
  let bounds: Rect | null = null;
  if (indices) {
    for (const i of indices) {
      const stroke = note.strokes[i];
      if (stroke) bounds = union(bounds, strokeBounds(stroke));
    }
  } else {
    for (const stroke of note.strokes) {
      if (layerVisible(note, stroke.layer)) bounds = union(bounds, strokeBounds(stroke));
    }
  }
  return bounds;
}

// --- Layers -------------------------------------------------------------------------------

function withLayers(note: Note, layers: Layer[], strokes: Stroke[] = note.strokes): Note {
  return { ...note, layers, strokes };
}

/** Moves strokes to other layers through `map` (old index → new index). */
function remapLayers(strokes: Stroke[], map: (layer: number) => number): Stroke[] {
  return strokes.map((stroke) => {
    const layer = map(stroke.layer);
    return layer === stroke.layer ? stroke : { ...stroke, layer };
  });
}

/** A new layer on top. */
export function addLayer(note: Note, name: string): Note {
  return withLayers(note, [...note.layers, { name, visible: true }]);
}

export function setLayerVisible(note: Note, index: number, visible: boolean): Note {
  const layer = note.layers[index];
  if (!layer || layer.visible === visible) return note;
  return withLayers(
    note,
    note.layers.map((l, i) => (i === index ? { ...l, visible } : l)),
  );
}

export function renameLayer(note: Note, index: number, name: string): Note {
  const layer = note.layers[index];
  if (!layer || layer.name === name) return note;
  return withLayers(
    note,
    note.layers.map((l, i) => (i === index ? { ...l, name } : l)),
  );
}

/** Index map of moving the item at `from` to `to` in a list of `count` items. */
export function moveIndex(index: number, from: number, to: number): number {
  if (index === from) return to;
  if (from < to && index > from && index <= to) return index - 1;
  if (from > to && index >= to && index < from) return index + 1;
  return index;
}

/** Moves layer `from` to position `to` (0 = bottom); strokes follow their layer. */
export function moveLayer(note: Note, from: number, to: number): Note {
  const count = note.layers.length;
  if (from === to || from < 0 || to < 0 || from >= count || to >= count) return note;
  const layers = [...note.layers];
  const [moved] = layers.splice(from, 1);
  layers.splice(to, 0, moved);
  return withLayers(
    note,
    layers,
    remapLayers(note.strokes, (l) => (l < count ? moveIndex(l, from, to) : l)),
  );
}

/**
 * Merges layer `index` into the one below it. Its strokes are moved after the others in the
 * stroke list, so that they stay drawn above the lower layer's strokes.
 */
export function mergeDown(note: Note, index: number): Note {
  if (index < 1 || index >= note.layers.length) return note;
  const lower = note.layers[index - 1];
  const upper = note.layers[index];
  const layers = note.layers.filter((_, i) => i !== index);
  layers[index - 1] = { ...lower, visible: lower.visible || upper.visible };
  const kept: Stroke[] = [];
  const merged: Stroke[] = [];
  for (const stroke of note.strokes) {
    if (stroke.layer === index) merged.push({ ...stroke, layer: index - 1 });
    else kept.push(stroke.layer > index ? { ...stroke, layer: stroke.layer - 1 } : stroke);
  }
  return withLayers(note, layers, [...kept, ...merged]);
}

// --- Page ---------------------------------------------------------------------------------

export function setBackground(note: Note, background: Background): Note {
  const b = note.background;
  const samePaper = JSON.stringify(b.paper) === JSON.stringify(background.paper);
  if (samePaper && b.template === background.template && b.texture === background.texture) return note;
  return { ...note, background };
}

/**
 * Turns the page a quarter turn (clockwise or counter-clockwise, as seen on screen with y
 * down): exact arithmetic, page width and height swapped.
 */
export function rotateQuarter(note: Note, direction: 'cw' | 'ccw'): Note {
  const { width, height } = note.page;
  const map =
    direction === 'cw'
      ? (p: InkPoint): InkPoint => ({ ...p, x: height - p.y, y: p.x })
      : (p: InkPoint): InkPoint => ({ ...p, x: p.y, y: width - p.x });
  return {
    ...note,
    page: { width: height, height: width },
    strokes: note.strokes.map((stroke) => ({ ...stroke, points: stroke.points.map(map) })),
  };
}

/** Smallest page side accepted by a crop, device units (1 cm). */
export const MIN_CROP = 1000;

/**
 * Crops the page to `rect` (page coordinates): the page takes its size (rounded to whole
 * units) and its top-left corner becomes the origin. Strokes entirely outside are removed;
 * the others are kept whole.
 */
export function cropTo(note: Note, rect: Rect): Note {
  const x = Math.round(rect.x);
  const y = Math.round(rect.y);
  const width = Math.round(rect.width);
  const height = Math.round(rect.height);
  if (width < MIN_CROP || height < MIN_CROP) return note;
  if (x === 0 && y === 0 && width === note.page.width && height === note.page.height) return note;
  const area = { x, y, width, height };
  const strokes: Stroke[] = [];
  for (const stroke of note.strokes) {
    const bounds = strokeBounds(stroke);
    if (!bounds || !overlaps(bounds, area)) continue;
    strokes.push({ ...stroke, points: stroke.points.map((p) => ({ ...p, x: p.x - x, y: p.y - y })) });
  }
  return { ...note, page: { width, height }, strokes };
}

/**
 * The automatic crop: the ink of the visible strokes plus `margin` all around, within the page;
 * null when there is no ink on the page.
 */
export function autoCropRect(note: Note, margin = 500): Rect | null {
  const ink = strokesBounds(note, null);
  if (!ink) return null;
  const page = { x: 0, y: 0, width: note.page.width, height: note.page.height };
  const area = intersection(inflate(ink, margin), page);
  if (!area) return null;
  // A crop never makes the page smaller than the minimum: grow around the centre if needed.
  const grow = (start: number, size: number, limit: number) => {
    if (size >= MIN_CROP || limit < MIN_CROP) return [start, size];
    const s = Math.min(Math.max(0, start - (MIN_CROP - size) / 2), limit - MIN_CROP);
    return [s, MIN_CROP];
  };
  const [x, width] = grow(area.x, area.width, page.width);
  const [y, height] = grow(area.y, area.height, page.height);
  return { x, y, width, height };
}
