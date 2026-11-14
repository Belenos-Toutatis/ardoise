/**
 * Pure operations on a note. Each returns a new note (or the same object when nothing changed)
 * and keeps every untouched stroke object, so that cached outlines and bounds stay valid.
 */
import { mapImage, rotateImage, shiftImage, turnImageAlone } from '../background';
import type { InkPoint, Tool } from '../ink';
import { isBookmarkObject, type Background, type Layer, type Note, type PageObject, type PageSize, type Rgba, type Stroke } from '../note';
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
import { layerVisible, onLayer, strokesTouched } from './hit';
import { objectBounds, objectsBounds, objectsOf, remapObjectLayers, transformAllObjects, withObjects } from './objects';
import { orientationOf, type Orientation } from './page-size';

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

/** Removes every visible stroke the eraser path touches (those of `layer` alone when it is given). */
export function eraseWhole(note: Note, path: readonly Vec[], radius: number, layer?: number): Note {
  return removeStrokes(note, strokesTouched(layer === undefined ? note : onLayer(note, layer), path, radius));
}

/**
 * Removes the parts of visible strokes within `radius` of the eraser path: samples closer than
 * that go (long segments are first subdivided so that the eraser cuts them too), and what is
 * left of a stroke becomes one stroke per remaining run of at least two samples, in place of
 * the original (same drawing order). With `layer`, the strokes of that layer alone are touched.
 */
export function erasePartial(note: Note, path: readonly Vec[], radius: number, layer?: number): Note {
  const hittable = layer === undefined ? note : onLayer(note, layer);
  const area = boundsOfPoints(path);
  if (!area || radius <= 0) return note;
  const reach = inflate(area, radius);
  const r2 = radius * radius;
  let changed = false;
  const strokes: Stroke[] = [];
  for (const stroke of note.strokes) {
    const bounds = strokeCentreBounds(stroke);
    if (!bounds || !layerVisible(hittable, stroke.layer) || !overlaps(bounds, reach)) {
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

/** Pause between the end of the note's ink and the first sample of pasted strokes, milliseconds. */
export const PASTE_GAP_MS = 500;
/** Shift between a paste and an identical copy already lying there, device units (4 mm). */
export const PASTE_STEP = 400;

/** Time of the last sample (or object, a bookmark apart: no replay shows it) of the note, milliseconds since it was created (0 without content). */
function noteEnd(note: Note): number {
  let end = 0;
  for (const stroke of note.strokes) for (const p of stroke.points) if (p.t > end) end = p.t;
  for (const object of objectsOf(note)) if (!isBookmarkObject(object) && object.t > end) end = object.t;
  return end;
}

/**
 * Where strokes pasted into `note` go (an offset to add to their points):
 * - their bounds lie wholly outside the page (copied from a larger or turned page): centred on
 *   the page, or top-left aligned when larger than it;
 * - otherwise where they were, but while a stroke with the same first and last sample already
 *   lies there (the source, or an earlier paste), diagonally further by `PASTE_STEP`.
 */
export function pasteOffset(note: Note, strokes: readonly Stroke[], objects: readonly PageObject[] = []): Vec {
  let ink: Rect | null = null;
  for (const stroke of strokes) ink = union(ink, strokeBounds(stroke));
  for (const object of objects) ink = union(ink, objectBounds(object));
  const lead = strokes[0]?.points;
  const first = lead?.[0];
  const last = lead?.[lead.length - 1];
  if (!ink) return { x: 0, y: 0 };
  if (!lead || !first || !last) return objectPasteOffset(note, objects, ink);
  const page: Rect = { x: 0, y: 0, width: note.page.width, height: note.page.height };
  let base: Vec = { x: 0, y: 0 };
  if (!overlaps(ink, page)) {
    base = {
      x: Math.max(0, (page.width - ink.width) / 2) - ink.x,
      y: Math.max(0, (page.height - ink.height) / 2) - ink.y,
    };
  }
  const near = (a: number, b: number) => Math.abs(a - b) < 1;
  const occupied = (o: Vec) =>
    note.strokes.some((s) => {
      const f = s.points[0];
      const l = s.points[s.points.length - 1];
      return (
        f !== undefined &&
        l !== undefined &&
        s.points.length === lead.length &&
        near(f.x, first.x + o.x) &&
        near(f.y, first.y + o.y) &&
        near(l.x, last.x + o.x) &&
        near(l.y, last.y + o.y)
      );
    });
  let offset = base;
  for (let step = 0; step < 100 && occupied(offset); step++) {
    offset = { x: base.x + (step + 1) * PASTE_STEP, y: base.y + (step + 1) * PASTE_STEP };
  }
  return offset;
}

/** `pasteOffset` for objects alone: the same rule, an object of the same kind at the same place standing for the copy. */
function objectPasteOffset(note: Note, objects: readonly PageObject[], box: Rect): Vec {
  const lead = objects[0];
  if (!lead) return { x: 0, y: 0 };
  const page: Rect = { x: 0, y: 0, width: note.page.width, height: note.page.height };
  let base: Vec = { x: 0, y: 0 };
  if (!overlaps(box, page)) {
    base = {
      x: Math.max(0, (page.width - box.width) / 2) - box.x,
      y: Math.max(0, (page.height - box.height) / 2) - box.y,
    };
  }
  const near = (a: number, b: number) => Math.abs(a - b) < 1;
  const occupied = (o: Vec) =>
    objectsOf(note).some((other) => other.kind === lead.kind && near(other.x, lead.x + o.x) && near(other.y, lead.y + o.y));
  let offset = base;
  for (let step = 0; step < 100 && occupied(offset); step++) {
    offset = { x: base.x + (step + 1) * PASTE_STEP, y: base.y + (step + 1) * PASTE_STEP };
  }
  return offset;
}

/**
 * Copies of `strokes` for pasting into `note` (the clipboard may come from another note): the
 * sample times shifted so that the first one comes `PASTE_GAP_MS` after the last sample of the
 * note, keeping the rhythm of the copied strokes: the replay draws them last, as written. With
 * `at` (M17: a paste during a writing recording takes the sound's current time, O15; after a
 * note-time narration, its end), the first sample comes at that note time instead (whole
 * milliseconds, never below 0).
 */
export function retimedForPaste(note: Note, strokes: readonly Stroke[], at?: number): Stroke[] {
  return retimedPaste(note, strokes, at).strokes;
}

/**
 * `retimedForPaste` for strokes and page objects pasted together: one shift for all, the first of them (sample or
 * object) coming at the start; `objectTimes` are the objects' new times, in their order.
 */
export function retimedPaste(
  note: Note,
  strokes: readonly Stroke[],
  at?: number,
  objects: readonly PageObject[] = [],
): { strokes: Stroke[]; objectTimes: number[] } {
  let first = Infinity;
  for (const stroke of strokes) for (const p of stroke.points) if (p.t < first) first = p.t;
  for (const object of objects) if (object.t < first) first = object.t;
  const start = at !== undefined && Number.isFinite(at) ? Math.max(0, Math.round(at)) : noteEnd(note) + PASTE_GAP_MS;
  const shift = Number.isFinite(first) ? start - first : 0;
  return {
    strokes: strokes.map((stroke) => ({ ...stroke, points: stroke.points.map((p) => ({ ...p, t: p.t + shift })) })),
    objectTimes: objects.map((o) => Math.max(0, o.t + shift)),
  };
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

function withLayers(note: Note, layers: Layer[], strokes: Stroke[] = note.strokes, objects?: PageObject[]): Note {
  const next = { ...note, layers, strokes };
  return objects ? withObjects(next, objects) : next;
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

/** Moves layer `from` to position `to` (0 = bottom); strokes and objects follow their layer. */
export function moveLayer(note: Note, from: number, to: number): Note {
  const count = note.layers.length;
  if (from === to || from < 0 || to < 0 || from >= count || to >= count) return note;
  const layers = [...note.layers];
  const [moved] = layers.splice(from, 1);
  layers.splice(to, 0, moved);
  const map = (l: number) => (l < count ? moveIndex(l, from, to) : l);
  return withLayers(note, layers, remapLayers(note.strokes, map), note.objects ? remapObjectLayers(note.objects, map) : undefined);
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
  // The same for objects: those of the merged layer after the lower layer's, so that they stay above them.
  const keptObjects: PageObject[] = [];
  const mergedObjects: PageObject[] = [];
  for (const object of objectsOf(note)) {
    if (object.layer === index) mergedObjects.push({ ...object, layer: index - 1 });
    else keptObjects.push(object.layer > index ? { ...object, layer: object.layer - 1 } : object);
  }
  return withLayers(note, layers, [...kept, ...merged], note.objects ? [...keptObjects, ...mergedObjects] : undefined);
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
 * down): exact arithmetic, page width and height swapped. The background image turns with the
 * strokes.
 */
export function rotateQuarter(note: Note, direction: 'cw' | 'ccw'): Note {
  const { width, height } = note.page;
  const map =
    direction === 'cw'
      ? (p: InkPoint): InkPoint => ({ ...p, x: height - p.y, y: p.x })
      : (p: InkPoint): InkPoint => ({ ...p, x: p.y, y: width - p.x });
  const turned: Note = {
    ...note,
    page: { width: height, height: width },
    strokes: note.strokes.map((stroke) => ({ ...stroke, points: stroke.points.map(map) })),
  };
  if (note.backgroundImage) turned.backgroundImage = rotateImage(note.backgroundImage, note.page, direction);
  // Page objects turn with the strokes: (x, y) → (h − y, x) clockwise, (y, w − x) counter-clockwise.
  const m: Affine =
    direction === 'cw' ? { a: 0, b: 1, c: -1, d: 0, e: height, f: 0 } : { a: 0, b: -1, c: 1, d: 0, e: 0, f: width };
  return transformAllObjects(turned, m);
}

/** Largest page side, device units (`MAX_PAGE_SIDE` in `ardoise_core::ops`). */
export const MAX_PAGE_SIDE = 10_000_000;

/** Bounds of the samples of every stroke and of every object, hidden layers included (they move with the page). */
function allInkBounds(note: Note): Rect | null {
  let bounds: Rect | null = null;
  for (const stroke of note.strokes) bounds = union(bounds, strokeCentreBounds(stroke));
  for (const object of objectsOf(note)) bounds = union(bounds, objectBounds(object));
  return bounds;
}

/**
 * How far `ink` has to move to lie inside a page of `page`'s size, or null when it cannot (it is
 * larger than the page on an axis). Zero on an axis where it already lies inside.
 */
function shiftInside(ink: Rect, page: PageSize): Vec | null {
  if (ink.width > page.width || ink.height > page.height) return null;
  const fix = (start: number, size: number, limit: number) =>
    start < 0 ? -start : start + size > limit ? limit - (start + size) : 0;
  return { x: fix(ink.x, ink.width, page.width), y: fix(ink.y, ink.height, page.height) };
}

/** Strokes with a sample outside the page (they are not drawn there, but kept). */
export function strokesOutsidePage(note: Note): number {
  const { width, height } = note.page;
  let count = 0;
  for (const stroke of note.strokes) {
    if (stroke.points.some((p) => p.x < 0 || p.y < 0 || p.x > width || p.y > height)) count++;
  }
  return count;
}

/**
 * Gives the page another size without turning the ink (milestone M8: orientation and paper
 * size of an existing note). The strokes and the background image move together:
 *
 * - `scaleInk` false: the ink keeps its size and its place. When part of it would fall outside
 *   the new page but all of it fits, it is moved back inside by the smallest shift; when it
 *   does not fit, it stays where it is (what is outside is not drawn but is kept: the former
 *   size brings it back, and the crop does not delete it).
 * - `scaleInk` true: the old page is scaled to fit the new one as a whole and centred in it,
 *   ink and stroke widths with it (the proportions of the ink never change).
 *
 * Returns the same note for a side outside `MIN_CROP`..`MAX_PAGE_SIDE` or for the same size.
 */
export function resizePage(note: Note, size: PageSize, scaleInk = false): Note {
  const width = Math.round(size.width);
  const height = Math.round(size.height);
  if (width < MIN_CROP || height < MIN_CROP || width > MAX_PAGE_SIDE || height > MAX_PAGE_SIDE) return note;
  if (width === note.page.width && height === note.page.height) return note;
  let scale = 1;
  let shift: Vec = { x: 0, y: 0 };
  if (scaleInk) {
    scale = Math.min(width / note.page.width, height / note.page.height);
    shift = { x: (width - note.page.width * scale) / 2, y: (height - note.page.height * scale) / 2 };
  } else {
    const ink = allInkBounds(note);
    shift = (ink && shiftInside(ink, { width, height })) || shift;
  }
  const moved = scale !== 1 || shift.x !== 0 || shift.y !== 0;
  const resized: Note = { ...note, page: { width, height } };
  if (!moved) return resized;
  resized.strokes = note.strokes.map((stroke) => ({
    ...stroke,
    width: stroke.width * scale,
    points: stroke.points.map((p) => ({ ...p, x: p.x * scale + shift.x, y: p.y * scale + shift.y })),
  }));
  if (note.backgroundImage) resized.backgroundImage = mapImage(note.backgroundImage, scale, shift.x, shift.y);
  return transformAllObjects(resized, { a: scale, b: 0, c: 0, d: scale, e: shift.x, f: shift.y });
}

/**
 * Makes the page portrait or landscape by swapping its width and height, the ink keeping its
 * orientation (`resizePage`). The same note when the page already has that orientation.
 */
export function setOrientation(note: Note, orientation: Orientation, scaleInk = false): Note {
  if (orientationOf(note.page) === orientation) return note;
  return resizePage(note, { width: note.page.height, height: note.page.width }, scaleInk);
}

/**
 * Turns the background image alone a quarter turn (`turnImageAlone`): the page and the strokes
 * stay. `pixels` is the size of the image's stored pixels, when known.
 */
export function rotateBackgroundImage(
  note: Note,
  pixels: { width: number; height: number } | undefined,
  direction: 'cw' | 'ccw',
): Note {
  if (!note.backgroundImage) return note;
  return { ...note, backgroundImage: turnImageAlone(note.backgroundImage, note.page, pixels, direction) };
}

/** Smallest page side accepted by a crop, device units (1 cm). */
export const MIN_CROP = 1000;

/**
 * Crops the page to `rect` (page coordinates): the page takes its size (rounded to whole
 * units) and its top-left corner becomes the origin. Visible strokes entirely outside are
 * removed; the others are kept whole. Strokes on hidden layers are always kept (moved with the
 * page, even outside it): the user cannot see them, so a crop must not delete them silently.
 * The background image moves with the strokes (what falls outside the page is not drawn).
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
    if (layerVisible(note, stroke.layer) && (!bounds || !overlaps(bounds, area))) continue;
    strokes.push({ ...stroke, points: stroke.points.map((p) => ({ ...p, x: p.x - x, y: p.y - y })) });
  }
  const cropped: Note = { ...note, page: { width, height }, strokes };
  if (note.backgroundImage) cropped.backgroundImage = shiftImage(note.backgroundImage, x, y);
  // Objects follow the strokes' rule: a visible one wholly outside goes, the others move with the page (a bookmark counts as visible).
  if (!note.objects) return cropped;
  const objects = note.objects
    .filter((o) => !((layerVisible(note, o.layer) || isBookmarkObject(o)) && !overlaps(objectBounds(o), area)))
    .map((o) => ({ ...o, x: o.x - x, y: o.y - y }));
  return withObjects(cropped, objects);
}

/**
 * The automatic crop: the ink of the visible strokes plus `margin` all around, within the page;
 * null when there is no ink on the page.
 */
export function autoCropRect(note: Note, margin = 500): Rect | null {
  const ink = union(strokesBounds(note, null), objectsBounds(note, null));
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
