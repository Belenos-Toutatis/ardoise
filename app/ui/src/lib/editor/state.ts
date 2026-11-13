/**
 * The editor's immutable state: the note being edited with its selection and active layer (a
 * `Doc`, what undo/redo restores), and the tool settings (not part of the history: undo never
 * changes the pen). Doc operations keep the selection and the active layer consistent.
 *
 * The selection holds strokes (`selection`) and page objects (`objectSelection`, text frames: `lib/editor/objects.ts`),
 * chosen and changed together: a lasso takes both, a move, a resize, a delete, a copy or a duplicate acts on both.
 */
import type { PixelsRef } from '../background';
import type { Tool } from '../ink';
import { INKSPACE_COLORS, type Background, type FontFamily, type Note, type PageObject, type PageSize, type Rgba, type Stroke } from '../note';
import type { TextEngine } from '../text';
import { type Affine, type Rect, type Vec } from './geometry';
import { layerVisible, onLayer } from './hit';
import {
  changeTexts,
  insertObjectCopies,
  objectsBounds,
  objectsOf,
  remapObjectLayers,
  removeObjects,
  selectable,
  transformObjects,
  withObjects,
  type TextChange,
} from './objects';
import * as ops from './ops';
import type { Orientation } from './page-size';
import type { ShapeKind } from './shape-tools';

/** What a drag (or a tap) on the page does: select, write, draw a line, a rectangle or an ellipse (`ShapeKind`), erase, type text, move the page, crop. */
export type Mode = 'select' | 'pen' | ShapeKind | 'eraser' | 'text' | 'pan' | 'crop';

/** The modes whose drag puts new ink on the active layer (they need it visible). */
export function inksNewStrokes(mode: Mode): boolean {
  return mode === 'pen' || mode === 'line' || mode === 'rectangle' || mode === 'ellipse';
}

/** The modes that put something new on the active layer (they need it visible). */
export function addsToLayer(mode: Mode): boolean {
  return inksNewStrokes(mode) || mode === 'text';
}
export type EraserMode = 'whole' | 'partial';

/** The text tool's settings (SPEC §4.4): its colour is the tool's colour (`ToolSettings.color`). */
export interface TextSettings {
  font: FontFamily;
  bold: boolean;
  /** One of `TEXT_SIZES` (`lib/text.ts`), device units. */
  size: number;
}

export const DEFAULT_TEXT: TextSettings = { font: 'latinModern', bold: false, size: 600 };

export interface ToolSettings {
  mode: Mode;
  tool: Tool;
  color: Rgba;
  /** Nominal width of new strokes, device units. */
  width: number;
  eraser: EraserMode;
  /** Eraser radius, device units. */
  eraserRadius: number;
  /** The text tool's family, weight and size; absent: `DEFAULT_TEXT`. */
  text?: TextSettings;
}

export interface Doc {
  readonly note: Note;
  /** Indices of selected strokes, ascending. */
  readonly selection: readonly number[];
  /** Indices of selected page objects (`note.objects`), ascending; absent when none is. */
  readonly objectSelection?: readonly number[];
  readonly activeLayer: number;
  /**
   * The pixels of the note's background image, kept by Rust under their key (`lib/editor/image.ts`);
   * absent when the note has no image (or its pixels cannot be read). Undo brings them back with
   * the image.
   */
  readonly backgroundPixels?: PixelsRef;
}

export interface EditorState extends Doc {
  readonly tools: ToolSettings;
}

export const DEFAULT_TOOLS: ToolSettings = {
  mode: 'pen',
  tool: 'ballpoint',
  color: INKSPACE_COLORS[0],
  width: 60,
  eraser: 'whole',
  eraserRadius: 300,
};

/** The text tool's settings of `tools`. */
export function textSettings(tools: ToolSettings): TextSettings {
  return tools.text ?? DEFAULT_TEXT;
}

/** Offset of pasted copies, device units (4 mm). */
export const PASTE_OFFSET = 400;

/** A note ready for editing: at least one layer, drawing on the top visible one. */
export function docFromNote(note: Note, defaultLayerName = 'Layer 1'): Doc {
  const fixed = note.layers.length > 0 ? note : { ...note, layers: [{ name: defaultLayerName, visible: true }] };
  const top = fixed.layers.length - 1;
  return { note: fixed, selection: [], activeLayer: nearestVisibleLayer(fixed, top) ?? top };
}

/** The visible layer closest to `index` (itself if visible; the upper one on a tie), or null. */
export function nearestVisibleLayer(note: Note, index: number): number | null {
  const count = note.layers.length;
  for (let d = 0; d < count; d++) {
    for (const i of [index + d, index - d]) {
      if (i >= 0 && i < count && note.layers[i].visible) return i;
    }
  }
  return null;
}

/** New ink goes on the active layer only while it is visible (never on a hidden layer: the pen says so instead). */
export function canDraw(doc: Doc): boolean {
  return doc.note.layers[doc.activeLayer]?.visible ?? true;
}

export function initialState(note: Note, tools: ToolSettings = DEFAULT_TOOLS): EditorState {
  return { ...docFromNote(note), tools };
}

function sorted(indices: Iterable<number>): number[] {
  return [...new Set(indices)].sort((a, b) => a - b);
}

function sameList(a: readonly number[], b: readonly number[]): boolean {
  return a.length === b.length && a.every((v, k) => v === b[k]);
}

/** The selected objects' indices of `doc`. */
export function objectSelection(doc: Doc): readonly number[] {
  return doc.objectSelection ?? [];
}

/** Whether anything (stroke or object) is selected. */
export function hasSelection(doc: Doc): boolean {
  return doc.selection.length > 0 || objectSelection(doc).length > 0;
}

/** How many things are selected (strokes and objects). */
export function selectionCount(doc: Doc): number {
  return doc.selection.length + objectSelection(doc).length;
}

/** The doc with this selection (the object selection's key left out when it is empty). */
function withSelection(doc: Doc, strokes: readonly number[], objects: readonly number[]): Doc {
  if (sameList(strokes, doc.selection) && sameList(objects, objectSelection(doc))) return doc;
  const next: Doc = { ...doc, selection: strokes, objectSelection: objects };
  if (objects.length === 0) delete (next as { objectSelection?: readonly number[] }).objectSelection;
  return next;
}

/** The doc with `note`, keeping the doc itself when nothing changed. */
function withNote(
  doc: Doc,
  note: Note,
  selection: readonly number[] = doc.selection,
  objects: readonly number[] = objectSelection(doc),
): Doc {
  if (note === doc.note && selection === doc.selection && objects === objectSelection(doc)) return doc;
  return withSelection({ ...doc, note }, selection, objects);
}

export function select(doc: Doc, indices: Iterable<number>, objects: Iterable<number> = []): Doc {
  const selection = sorted(indices).filter((i) => i >= 0 && i < doc.note.strokes.length);
  const count = objectsOf(doc.note).length;
  const chosen = sorted(objects).filter((i) => i >= 0 && i < count);
  return withSelection(doc, selection, chosen);
}

export function clearSelection(doc: Doc): Doc {
  return hasSelection(doc) ? withSelection(doc, [], []) : doc;
}

/** Ctrl+A: the strokes and objects of the chosen layer, when it shows (the selection belongs to the chosen layer). */
export function selectAll(doc: Doc): Doc {
  const only = onLayer(doc.note, doc.activeLayer);
  const objects = doc.note.layers[doc.activeLayer]?.visible
    ? objectsOf(doc.note).flatMap((o, i) => (o.layer === doc.activeLayer && selectable(o) ? [i] : []))
    : [];
  return select(
    doc,
    doc.note.strokes.flatMap((s, i) => (layerVisible(only, s.layer) ? [i] : [])),
    objects,
  );
}

/** Chooses the layer the pen, the eraser and the selection act on; the selection of the old one goes. */
export function setActiveLayer(doc: Doc, layer: number): Doc {
  if (layer < 0 || layer >= doc.note.layers.length || layer === doc.activeLayer) return doc;
  return withSelection({ ...doc, activeLayer: layer }, [], []);
}

/** A new stroke drawn with the tool settings on the active layer (none if it is hidden). */
export function addStroke(doc: Doc, stroke: Omit<Stroke, 'layer'>): Doc {
  if (!canDraw(doc)) return doc;
  return withNote(doc, ops.addStroke(doc.note, { ...stroke, layer: doc.activeLayer }), [], []);
}

/** The eraser touches the strokes of the active layer only (none when it is hidden); text frames are not erased. */
export function erase(doc: Doc, path: readonly Vec[], radius: number, mode: EraserMode): Doc {
  const layer = doc.activeLayer;
  const note =
    mode === 'whole' ? ops.eraseWhole(doc.note, path, radius, layer) : ops.erasePartial(doc.note, path, radius, layer);
  return note === doc.note ? doc : withNote(doc, note, [], []);
}

/**
 * The style on the selection, or on the whole note when nothing is selected. Text frames take the colour (the tool and
 * the width are the ink's).
 */
export function applyStyle(doc: Doc, style: ops.StrokeStyle, wholeNote = !hasSelection(doc)): Doc {
  let note = ops.setStyle(doc.note, wholeNote ? null : doc.selection, style);
  if (style.color) {
    const color = style.color;
    const objects = objectsOf(note).map((o, i) =>
      o.kind === 'text' && (wholeNote || objectSelection(doc).includes(i)) && !sameRgba(o.color as Rgba | undefined, color)
        ? { ...o, color: { ...color } }
        : o,
    );
    if (objects.some((o, i) => o !== objectsOf(note)[i])) note = withObjects(note, objects);
  }
  return withNote(doc, note);
}

function sameRgba(a: Rgba | undefined, b: Rgba): boolean {
  return !!a && a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
}

/** The text tool's family, weight or size on the selected text frames (re-measured by `engine`). */
export function applyTextStyle(doc: Doc, change: TextChange, engine: TextEngine): Doc {
  if (objectSelection(doc).length === 0) return doc;
  return withNote(doc, changeTexts(doc.note, objectSelection(doc), change, engine));
}

export function transformSelection(doc: Doc, m: Affine): Doc {
  const strokes = ops.transformStrokes(doc.note, doc.selection, m);
  return withNote(doc, transformObjects(strokes, objectSelection(doc), m));
}

export function deleteSelection(doc: Doc): Doc {
  return withNote(doc, removeObjects(ops.removeStrokes(doc.note, doc.selection), objectSelection(doc)), [], []);
}

export function selectedStrokes(doc: Doc): Stroke[] {
  return doc.selection.map((i) => doc.note.strokes[i]).filter((s): s is Stroke => s !== undefined);
}

export function selectedObjects(doc: Doc): PageObject[] {
  const objects = objectsOf(doc.note);
  return objectSelection(doc)
    .map((i) => objects[i])
    .filter((o): o is PageObject => o !== undefined);
}

/**
 * Pastes copies of `strokes` (and `objects`) on the active layer, shifted; the copies become the selection.
 * Nothing happens while the active layer is hidden.
 */
export function paste(
  doc: Doc,
  strokes: readonly Stroke[],
  offset: Vec = { x: PASTE_OFFSET, y: PASTE_OFFSET },
  objects: readonly PageObject[] = [],
): Doc {
  if (!canDraw(doc)) return doc;
  const inked = ops.insertCopies(doc.note, strokes, offset, doc.activeLayer);
  const placed = insertObjectCopies(inked.note, objects, offset, doc.activeLayer);
  return withNote(doc, placed.note, inked.indices, placed.indices);
}

/**
 * Pastes strokes and objects from the clipboard (copied in this note or another one) on the active layer:
 * placed by `ops.pasteOffset`, retimed to follow the note's content (`ops.retimedForPaste`; from note
 * time `at` when given: during a writing recording, the sound's current time, M17 O15), and
 * selected afterwards. One undoable step. Nothing happens while the active layer is hidden.
 */
export function pasteStrokes(doc: Doc, strokes: readonly Stroke[], at?: number, objects: readonly PageObject[] = []): Doc {
  if (!canDraw(doc) || (strokes.length === 0 && objects.length === 0)) return doc;
  const offset = ops.pasteOffset(doc.note, strokes, objects);
  const { strokes: timed, objectTimes } = ops.retimedPaste(doc.note, strokes, at, objects);
  const inked = ops.insertCopies(doc.note, timed, offset, doc.activeLayer);
  const placed = insertObjectCopies(inked.note, objects, offset, doc.activeLayer, objectTimes);
  return withNote(doc, placed.note, inked.indices, placed.indices);
}

export function duplicateSelection(doc: Doc): Doc {
  return paste(doc, selectedStrokes(doc), undefined, selectedObjects(doc));
}

export function selectionBounds(doc: Doc): Rect | null {
  if (!hasSelection(doc)) return null;
  const ink = doc.selection.length > 0 ? ops.strokesBounds(doc.note, doc.selection) : null;
  const objects = objectSelection(doc).length > 0 ? objectsBounds(doc.note, objectSelection(doc)) : null;
  if (!ink) return objects;
  if (!objects) return ink;
  const x = Math.min(ink.x, objects.x);
  const y = Math.min(ink.y, objects.y);
  return {
    x,
    y,
    width: Math.max(ink.x + ink.width, objects.x + objects.width) - x,
    height: Math.max(ink.y + ink.height, objects.y + objects.height) - y,
  };
}

// --- Layers -------------------------------------------------------------------------------

/** A new top layer, which becomes the chosen one (the selection of the old one goes). */
export function addLayer(doc: Doc, name: string): Doc {
  const note = ops.addLayer(doc.note, name);
  return withSelection({ ...doc, note, activeLayer: note.layers.length - 1 }, [], []);
}

export function setLayerVisible(doc: Doc, index: number, visible: boolean): Doc {
  const note = ops.setLayerVisible(doc.note, index, visible);
  if (note === doc.note) return doc;
  // Hidden strokes and objects cannot stay selected.
  const selection = visible ? doc.selection : doc.selection.filter((i) => doc.note.strokes[i]?.layer !== index);
  const objects = visible ? objectSelection(doc) : objectSelection(doc).filter((i) => objectsOf(doc.note)[i]?.layer !== index);
  // The chosen layer stays chosen even hidden (the zone warns, the pen refuses, the eraser and the selection find nothing).
  return withNote(doc, note, selection, objects);
}

export function renameLayer(doc: Doc, index: number, name: string): Doc {
  return withNote(doc, ops.renameLayer(doc.note, index, name));
}

export function moveLayer(doc: Doc, from: number, to: number): Doc {
  const note = ops.moveLayer(doc.note, from, to);
  if (note === doc.note) return doc;
  return { ...doc, note, activeLayer: ops.moveIndex(doc.activeLayer, from, to) };
}

export function mergeDown(doc: Doc, index: number): Doc {
  const note = ops.mergeDown(doc.note, index);
  if (note === doc.note) return doc;
  const active = doc.activeLayer >= index ? doc.activeLayer - 1 : doc.activeLayer;
  return withSelection({ ...doc, note, activeLayer: Math.max(0, active) }, [], []);
}

// --- Page ---------------------------------------------------------------------------------

export function setBackground(doc: Doc, background: Background): Doc {
  return withNote(doc, ops.setBackground(doc.note, background));
}

export function rotate(doc: Doc, direction: 'cw' | 'ccw'): Doc {
  return withNote(doc, ops.rotateQuarter(doc.note, direction));
}

/** The page of the other orientation (`ops.setOrientation`); the selection stays. */
export function setOrientation(doc: Doc, orientation: Orientation, scaleInk = false): Doc {
  return withNote(doc, ops.setOrientation(doc.note, orientation, scaleInk));
}

/** The page at another paper size, the ink staying in place or scaled with it (`ops.resizePage`). */
export function resizePage(doc: Doc, size: PageSize, scaleInk = false): Doc {
  return withNote(doc, ops.resizePage(doc.note, size, scaleInk));
}

/** The background image alone turned a quarter turn. */
export function rotateBackgroundImage(doc: Doc, direction: 'cw' | 'ccw'): Doc {
  return withNote(doc, ops.rotateBackgroundImage(doc.note, doc.backgroundPixels, direction));
}

export function crop(doc: Doc, rect: Rect): Doc {
  const note = ops.cropTo(doc.note, rect);
  return note === doc.note ? doc : withNote(doc, note, [], []);
}

export function autoCrop(doc: Doc, margin?: number): Doc {
  const rect = ops.autoCropRect(doc.note, margin);
  return rect ? crop(doc, rect) : doc;
}

// --- Text frames ----------------------------------------------------------------------------

/** The doc with the object at `index` replaced (a text being typed); the selection stays. */
export function replaceObjectAt(doc: Doc, index: number, object: PageObject): Doc {
  const objects = [...objectsOf(doc.note)];
  if (index < 0 || index >= objects.length) return doc;
  objects[index] = object;
  return withNote(doc, withObjects(doc.note, objects));
}

/** The doc without the object at `index` (a frame left empty); the selection loses it. */
export function removeObjectAt(doc: Doc, index: number): Doc {
  const note = removeObjects(doc.note, [index]);
  if (note === doc.note) return doc;
  const objects = objectSelection(doc)
    .filter((i) => i !== index)
    .map((i) => (i > index ? i - 1 : i));
  return withNote(doc, note, doc.selection, objects);
}

/** A new object at the end of the list, on its own layer (the caller checks `canDraw`); nothing is selected. */
export function addObjectToDoc(doc: Doc, object: PageObject): Doc {
  return withNote(doc, withObjects(doc.note, [...objectsOf(doc.note), object]), [], []);
}

export { remapObjectLayers };
