/**
 * Page objects in the editor (format 1.4, `docs/design/page-objects.md`): their place on the page, and the pure
 * operations that move them with the page or the selection, the way `ops.ts` moves strokes. The placement is the same
 * for every kind (`PageObject` in `note.ts`), so these work on kinds this version does not know too; only text frames
 * and bookmarks are measured (`measureText`, `measureBookmark`) and drawn.
 *
 * The rules match Rust's (`ardoise_core::objects`, `ardoise_core::ops`): a lasso or a zone takes an object when the
 * centre of its box is inside; a crop drops a visible object whose box is wholly outside; objects are drawn with their
 * layer, above its strokes.
 */
import { isBookmarkObject, isTextObject, type BookmarkObject, type BookmarkRef, type Note, type PageObject, type Rgba, type TextObject, type FontFamily } from '../note';
import type { TextEngine, TextStyle } from '../text';
import { affineScale, applyAffine, containsPoint, inflate, pointInPolygon, union, type Affine, type Rect, type Vec } from './geometry';

/** The objects of a note (none when the key is absent). */
export function objectsOf(note: Pick<Note, 'objects'>): readonly PageObject[] {
  return note.objects ?? [];
}

/** The note with `objects` (the key left out when there is none, as Rust writes it). */
export function withObjects(note: Note, objects: PageObject[]): Note {
  if (objects.length === 0) {
    if (!note.objects) return note;
    const rest = { ...note };
    delete rest.objects;
    return rest;
  }
  return { ...note, objects };
}

/** Where the content's point (u, v) is on the page. */
export function pagePoint(object: PageObject, u: number, v: number): Vec {
  const su = u * object.scale;
  const sv = v * object.scale;
  switch (((object.quarterTurns % 4) + 4) % 4) {
    case 1:
      return { x: object.x - sv, y: object.y + su };
    case 2:
      return { x: object.x - su, y: object.y - sv };
    case 3:
      return { x: object.x + sv, y: object.y - su };
    default:
      return { x: object.x + su, y: object.y + sv };
  }
}

/** The content's point under the page point `p` (the inverse of `pagePoint`). */
export function contentPoint(object: PageObject, p: Vec): Vec {
  const dx = (p.x - object.x) / object.scale;
  const dy = (p.y - object.y) / object.scale;
  switch (((object.quarterTurns % 4) + 4) % 4) {
    case 1:
      return { x: dy, y: -dx };
    case 2:
      return { x: -dx, y: -dy };
    case 3:
      return { x: -dy, y: dx };
    default:
      return { x: dx, y: dy };
  }
}

/** The box of the object's content on the page. */
export function objectBounds(object: PageObject): Rect {
  const corners = [
    pagePoint(object, 0, 0),
    pagePoint(object, object.width, 0),
    pagePoint(object, object.width, object.height),
    pagePoint(object, 0, object.height),
  ];
  const x = Math.min(...corners.map((c) => c.x));
  const y = Math.min(...corners.map((c) => c.y));
  return { x, y, width: Math.max(...corners.map((c) => c.x)) - x, height: Math.max(...corners.map((c) => c.y)) - y };
}

/** The centre of the object's box on the page (what a lasso or a zone must hold). */
export function objectCentre(object: PageObject): Vec {
  return pagePoint(object, object.width / 2, object.height / 2);
}

/** The affine map that places the content on the page (`x' = a·u + c·v + e`, `y' = b·u + d·v + f`). */
export function objectMatrix(object: PageObject): Affine {
  const s = object.scale;
  const [a, b, c, d] = [
    [s, 0, 0, s],
    [0, s, -s, 0],
    [-s, 0, 0, -s],
    [0, -s, s, 0],
  ][((object.quarterTurns % 4) + 4) % 4];
  return { a, b, c, d, e: object.x, f: object.y };
}

/** The SVG transform that places the content on the page. */
export function objectTransform(object: PageObject): string {
  const { a, b, c, d, e, f } = objectMatrix(object);
  return `matrix(${a} ${b} ${c} ${d} ${e} ${f})`;
}

/** What the page draws, in order (`draw_items` in `ardoise-render`): visible layers bottom first, each its strokes then its objects. */
export type DrawItem = { kind: 'stroke'; index: number } | { kind: 'object'; index: number };

/**
 * The strokes (`strokeOrder`: indices of visible strokes in drawing order, `hit.ts` `drawingOrder`) and the objects of
 * visible layers interleaved: on each layer its strokes, then its objects; what has no layer entry last.
 */
export function drawItems(note: Pick<Note, 'layers' | 'strokes' | 'objects'>, strokeOrder: readonly number[]): DrawItem[] {
  const objects = objectsOf(note);
  if (objects.length === 0) return strokeOrder.map((index) => ({ kind: 'stroke', index }));
  // -1 stands for every layer without an entry (drawn on top).
  const key = (layer: number) => (note.layers[layer] ? layer : -1);
  const strokesBy = new Map<number, number[]>();
  const objectsBy = new Map<number, number[]>();
  const add = (map: Map<number, number[]>, k: number, i: number) => {
    const list = map.get(k);
    if (list) list.push(i);
    else map.set(k, [i]);
  };
  for (const i of strokeOrder) add(strokesBy, key(note.strokes[i]?.layer ?? -1), i);
  objects.forEach((o, i) => {
    const k = key(o.layer);
    // A bookmark is not hidden with its layer (SPEC §4.4 « Signets »).
    if (k === -1 || note.layers[k].visible || isBookmarkObject(o)) add(objectsBy, k, i);
  });
  const items: DrawItem[] = [];
  const emit = (k: number) => {
    for (const index of strokesBy.get(k) ?? []) items.push({ kind: 'stroke', index });
    for (const index of objectsBy.get(k) ?? []) items.push({ kind: 'object', index });
  };
  note.layers.forEach((_, l) => emit(l));
  emit(-1);
  return items;
}

/** The clockwise quarter turns an affine map makes, rounded to the nearest one. */
export function affineTurns(m: Affine): number {
  const turns = Math.round(Math.atan2(m.b, m.a) / (Math.PI / 2));
  return ((turns % 4) + 4) % 4;
}

/** The object moved with the page under `m` (a move, a uniform change of scale, quarter turns). */
export function transformObject<T extends PageObject>(object: T, m: Affine): T {
  const origin = applyAffine(m, object);
  const factor = affineScale(m);
  return {
    ...object,
    x: origin.x,
    y: origin.y,
    scale: factor > 0 && Number.isFinite(factor) ? object.scale * factor : object.scale,
    quarterTurns: (object.quarterTurns + affineTurns(m)) % 4,
  };
}

/** The style of a text frame. */
export function textStyle(object: Pick<TextObject, 'font' | 'bold' | 'size'>): TextStyle {
  return { font: object.font, bold: object.bold, size: object.size };
}

/** The frame with its box measured as the text engine lays it out. */
export function measureText<T extends TextObject>(object: T, engine: TextEngine): T {
  const laid = engine.layout(object.text, textStyle(object));
  return laid.width === object.width && laid.height === object.height ? object : { ...object, width: laid.width, height: laid.height };
}

/**
 * The note with every text frame and every bookmark measured by `engine` (the same note when none changes: a note read by
 * Rust is).
 */
export function measureTexts(note: Note, engine: TextEngine): Note {
  const objects = objectsOf(note);
  const measured = objects.map((o) => (isTextObject(o) ? measureText(o, engine) : isBookmarkObject(o) ? measureBookmark(o, engine) : o));
  return measured.some((o, i) => o !== objects[i]) ? withObjects(note, measured) : note;
}

/** The text of the note's text frames on visible layers, top to bottom then left to right, one per line (`Note::typed_text`). */
export function typedText(note: Pick<Note, 'layers' | 'objects'>): string {
  return objectsOf(note)
    .filter((o): o is TextObject => isTextObject(o) && note.layers[o.layer]?.visible !== false && o.text.trim() !== '')
    .map((o) => ({ o, box: objectBounds(o) }))
    .sort((a, b) => a.box.y - b.box.y || a.box.x - b.box.x)
    .map(({ o }) => o.text)
    .join('\n');
}

/** A new text frame (its box measured). */
export function newTextObject(
  engine: TextEngine,
  fields: { id: string; layer: number; t: number; x: number; y: number; text: string; font: FontFamily; bold: boolean; size: number; color: Rgba },
): TextObject {
  return measureText(
    { kind: 'text', scale: 1, quarterTurns: 0, width: 0, height: 0, ...fields, color: { ...fields.color } },
    engine,
  );
}

// --- Bookmarks (SPEC §4.4 « Signets », format 1.6) ----------------------------------------------

/**
 * The geometry of a bookmark's content, device units: the label in Andika at `fontSize`, a flag of `flagWidth` before it,
 * `pad` all around (`BOOKMARK_*` in `ardoise-core`'s `objects.rs`: the same numbers, so that both measure the same box).
 * `maxLabel`: how many characters a label may have.
 */
export const BOOKMARK = { fontSize: 500, pad: 150, flagWidth: 300, maxLabel: 60 } as const;

const BOOKMARK_STYLE: TextStyle = { font: 'andika', bold: false, size: BOOKMARK.fontSize };

/**
 * A label as a bookmark keeps it (`clean_bookmark_label` in Rust): one line (runs of white space, line breaks included,
 * become one space), no white space at either end, at most `BOOKMARK.maxLabel` characters. Empty: the bookmark goes.
 */
export function cleanBookmarkLabel(label: string): string {
  const oneLine = label.split(/\s+/).filter(Boolean).join(' ');
  return Array.from(oneLine).slice(0, BOOKMARK.maxLabel).join('').trimEnd();
}

/** What a bookmark looks like, in its own units: its box, the flag's path, and where the label's glyphs start. */
export interface BookmarkLook {
  width: number;
  height: number;
  /** SVG path data of the flag (a ribbon with a notch), to fill with the non-zero rule. */
  flag: string;
  /** Where the label's origin (its top-left corner) is. */
  textX: number;
  textY: number;
}

/** The look of a bookmark with this label, measured as `engine` lays the label out. */
export function bookmarkLook(label: string, engine: TextEngine): BookmarkLook {
  const laid = engine.layout(label, BOOKMARK_STYLE);
  const { pad, flagWidth } = BOOKMARK;
  const line = Math.max(laid.height, laid.lineHeight);
  const left = pad;
  const right = pad + flagWidth;
  const top = pad;
  const bottom = pad + line;
  const notch = top + line * 0.72;
  return {
    width: pad + flagWidth + pad + laid.width + pad,
    height: line + 2 * pad,
    flag: `M${left} ${top}H${right}V${bottom}L${(left + right) / 2} ${notch}L${left} ${bottom}Z`,
    textX: pad + flagWidth + pad,
    textY: pad,
  };
}

/** The glyphs of a bookmark's label, SVG path data in the label's own units (to place at `textX`, `textY`). */
export function bookmarkLabelPath(label: string, engine: TextEngine): string {
  return engine.path(label, BOOKMARK_STYLE);
}

/** The bookmark with its box measured as `engine` lays it out. */
export function measureBookmark<T extends BookmarkObject>(object: T, engine: TextEngine): T {
  const { width, height } = bookmarkLook(object.label, engine);
  return width === object.width && height === object.height ? object : { ...object, width, height };
}

/** A new bookmark with its top-left corner at (`x`, `y`), its box measured. The label is cleaned. */
export function newBookmarkObject(
  engine: TextEngine,
  fields: { id: string; layer: number; t: number; x: number; y: number; label: string },
): BookmarkObject {
  return measureBookmark(
    { kind: 'bookmark', scale: 1, quarterTurns: 0, width: 0, height: 0, ...fields, label: cleanBookmarkLabel(fields.label) },
    engine,
  );
}

/**
 * The bookmarks of the page that say something (a blank label is not listed), in reading order: top to bottom, then left to
 * right, by the top-left corner of their box (`Note::bookmark_objects`); every layer.
 */
export function bookmarkObjects(note: Pick<Note, 'objects'>): BookmarkObject[] {
  return objectsOf(note)
    .filter((o): o is BookmarkObject => isBookmarkObject(o) && o.label.trim() !== '')
    .map((o) => ({ o, box: objectBounds(o) }))
    .sort((a, b) => a.box.y - b.box.y || a.box.x - b.box.x)
    .map(({ o }) => o);
}

/** The bookmarks as the listing gives them (`Note::bookmarks`): label trimmed, the box's centre rounded. */
export function bookmarkRefs(note: Pick<Note, 'objects'>): BookmarkRef[] {
  return bookmarkObjects(note).map((o) => {
    const centre = objectCentre(o);
    return { id: o.id, label: o.label.trim(), x: Math.round(centre.x), y: Math.round(centre.y) };
  });
}

/** A fresh id for an object (unique for all practical purposes; Rust repairs a clash when it reads). */
export function newObjectId(): string {
  const random = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function' ? crypto.randomUUID() : null;
  return random ?? `o-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

// --- Operations on the note's objects ---------------------------------------------------------

function indexSet(indices: Iterable<number>): Set<number> {
  return new Set(indices);
}

export function addObject(note: Note, object: PageObject): Note {
  return withObjects(note, [...objectsOf(note), object]);
}

/** The note with the object at `index` replaced. */
export function replaceObject(note: Note, index: number, object: PageObject): Note {
  const objects = [...objectsOf(note)];
  if (index < 0 || index >= objects.length || objects[index] === object) return note;
  objects[index] = object;
  return withObjects(note, objects);
}

export function removeObjects(note: Note, indices: Iterable<number>): Note {
  const remove = indexSet(indices);
  if (remove.size === 0) return note;
  const objects = objectsOf(note).filter((_, i) => !remove.has(i));
  return objects.length === objectsOf(note).length ? note : withObjects(note, objects);
}

/** Applies `m` to the objects at `indices` (the selection moved or resized). */
export function transformObjects(note: Note, indices: Iterable<number>, m: Affine): Note {
  const only = indexSet(indices);
  if (only.size === 0) return note;
  return withObjects(
    note,
    objectsOf(note).map((o, i) => (only.has(i) ? transformObject(o, m) : o)),
  );
}

/** Applies `m` to every object (page operations: turn, resize). */
export function transformAllObjects(note: Note, m: Affine): Note {
  if (objectsOf(note).length === 0) return note;
  return withObjects(note, objectsOf(note).map((o) => transformObject(o, m)));
}

/**
 * Copies of `objects` shifted by `offset`, appended on `layer` with new ids and the times `times` (one per copy, or
 * their own); returns the note and the indices of the copies.
 */
export function insertObjectCopies(
  note: Note,
  objects: readonly PageObject[],
  offset: Vec,
  layer: number,
  times?: readonly number[],
): { note: Note; indices: number[] } {
  if (objects.length === 0) return { note, indices: [] };
  const copies = objects.map((o, i) => ({
    ...o,
    id: newObjectId(),
    layer,
    t: times?.[i] ?? o.t,
    x: o.x + offset.x,
    y: o.y + offset.y,
  }));
  const start = objectsOf(note).length;
  return { note: withObjects(note, [...objectsOf(note), ...copies]), indices: copies.map((_, i) => start + i) };
}

/**
 * Whether `object` can be touched: its layer shows, and it is `only` when given. A bookmark always can: it is not hidden
 * with its layer, nor tied to the chosen one (SPEC §4.4 « Signets »).
 */
function reachable(note: Pick<Note, 'layers'>, object: PageObject, only?: number): boolean {
  if (isBookmarkObject(object)) return true;
  if (only !== undefined && object.layer !== only) return false;
  const entry = note.layers[object.layer];
  return entry ? entry.visible : only === undefined;
}

/** Indices of the objects on visible layers (of `only` alone when given), in drawing order: layer by layer. */
export function objectDrawingOrder(note: Pick<Note, 'layers' | 'objects'>, only?: number): number[] {
  const objects = objectsOf(note);
  const order: number[] = [];
  note.layers.forEach((layer, l) => {
    objects.forEach((o, i) => {
      if (o.layer !== l) return;
      if (isBookmarkObject(o) || (layer.visible && (only === undefined || l === only))) order.push(i);
    });
  });
  objects.forEach((o, i) => {
    if (!note.layers[o.layer] && (only === undefined || isBookmarkObject(o))) order.push(i);
  });
  return order;
}

/**
 * Whether the editor can select and change an object: a kind it draws (text frames, bookmarks). An object of a kind this
 * version does not know is drawn nowhere, so it is not selectable either: it only moves with the page.
 */
export function selectable(object: PageObject): boolean {
  return isTextObject(object) || isBookmarkObject(object);
}

/** The objects a lasso `polygon` takes: selectable ones of visible layers (of `only` alone when given) whose centre is inside. */
export function objectsInLasso(note: Pick<Note, 'layers' | 'objects'>, polygon: readonly Vec[], only?: number): number[] {
  if (polygon.length < 3) return [];
  const picked: number[] = [];
  objectsOf(note).forEach((o, i) => {
    if (selectable(o) && reachable(note, o, only) && pointInPolygon(objectCentre(o), polygon)) picked.push(i);
  });
  return picked;
}

/** The topmost selectable object (of visible layers, of `only` alone when given) whose box, grown by `tolerance`, holds `p`. */
export function hitObject(note: Pick<Note, 'layers' | 'objects'>, p: Vec, tolerance: number, only?: number): number | null {
  const order = objectDrawingOrder(note, only);
  for (let k = order.length - 1; k >= 0; k--) {
    const object = objectsOf(note)[order[k]];
    if (selectable(object) && containsPoint(inflate(objectBounds(object), tolerance), p)) return order[k];
  }
  return null;
}

/** The box of the objects at `indices` (or of every visible one if null). */
export function objectsBounds(note: Pick<Note, 'layers' | 'objects'>, indices: Iterable<number> | null): Rect | null {
  let bounds: Rect | null = null;
  const objects = objectsOf(note);
  if (indices) {
    for (const i of indices) {
      const o = objects[i];
      if (o) bounds = union(bounds, objectBounds(o));
    }
  } else {
    for (const o of objects) if (note.layers[o.layer]?.visible !== false || isBookmarkObject(o)) bounds = union(bounds, objectBounds(o));
  }
  return bounds;
}

/** What can change on text frames: the colour (the tool's), the family, bold, the size. */
export interface TextChange {
  color?: Rgba;
  font?: FontFamily;
  bold?: boolean;
  /** A size of the tool: the frame takes it at scale 1 on the page, its top-left corner staying where it is. */
  size?: number;
}

/**
 * The text frames at `indices` (every one if null) changed by `change`, measured again by `engine`; the other objects,
 * and the frames already so, stay the same objects.
 */
export function changeTexts(note: Note, indices: Iterable<number> | null, change: TextChange, engine: TextEngine): Note {
  const only = indices ? indexSet(indices) : null;
  let changed = false;
  const objects = objectsOf(note).map((o, i) => {
    if ((only && !only.has(i)) || !isTextObject(o)) return o;
    const color = change.color ?? o.color;
    const font = change.font ?? o.font;
    const bold = change.bold ?? o.bold;
    const resized = change.size !== undefined && (change.size !== o.size || o.scale !== 1);
    const sameColor = color.r === o.color.r && color.g === o.color.g && color.b === o.color.b && color.a === o.color.a;
    if (sameColor && font === o.font && bold === o.bold && !resized) return o;
    changed = true;
    let next: TextObject = { ...o, color: { ...color }, font, bold };
    if (resized) {
      // The new size replaces the frame's change of scale; its top-left corner on the page stays.
      const corner = objectBounds(o);
      next = { ...next, size: change.size as number, scale: 1 };
      next = measureText(next, engine);
      const box = objectBounds(next);
      next = { ...next, x: next.x + corner.x - box.x, y: next.y + corner.y - box.y };
    }
    return measureText(next, engine);
  });
  return changed ? withObjects(note, objects) : note;
}

/** Moves objects to other layers through `map` (old index → new index). */
export function remapObjectLayers(objects: readonly PageObject[], map: (layer: number) => number): PageObject[] {
  return objects.map((o) => {
    const layer = map(o.layer);
    return layer === o.layer ? o : { ...o, layer };
  });
}

/** The topmost bookmark whose box, grown by `margin`, holds `p` (a tap with the bookmark tool; layers do not matter). */
export function bookmarkAt(note: Pick<Note, 'layers' | 'objects'>, p: Vec, margin: number): number | null {
  const order = objectDrawingOrder(note);
  for (let k = order.length - 1; k >= 0; k--) {
    const object = objectsOf(note)[order[k]];
    if (isBookmarkObject(object) && containsPoint(inflate(objectBounds(object), margin), p)) return order[k];
  }
  return null;
}

/** Whether a point of the page lies within any object's box grown by `margin` (for a tap with the text tool). */
export function textAt(note: Pick<Note, 'layers' | 'objects'>, p: Vec, margin: number, only?: number): number | null {
  const hit = hitObject(note, p, margin, only);
  return hit !== null && isTextObject(objectsOf(note)[hit]) ? hit : null;
}
