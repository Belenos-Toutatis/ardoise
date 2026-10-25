/**
 * The editor's immutable state: the note being edited with its selection and active layer (a
 * `Doc`, what undo/redo restores), and the tool settings (not part of the history: undo never
 * changes the pen). Doc operations keep the selection and the active layer consistent.
 */
import type { PixelsRef } from '../background';
import type { Tool } from '../ink';
import { INKSPACE_COLORS, type Background, type Note, type Rgba, type Stroke } from '../note';
import { type Affine, type Rect, type Vec } from './geometry';
import * as ops from './ops';

export type Mode = 'select' | 'pen' | 'eraser' | 'pan' | 'crop';
export type EraserMode = 'whole' | 'partial';

export interface ToolSettings {
  mode: Mode;
  tool: Tool;
  color: Rgba;
  /** Nominal width of new strokes, device units. */
  width: number;
  eraser: EraserMode;
  /** Eraser radius, device units. */
  eraserRadius: number;
}

export interface Doc {
  readonly note: Note;
  /** Indices of selected strokes, ascending. */
  readonly selection: readonly number[];
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

/** New ink goes on the active layer only while it is visible (never on a hidden layer). */
export function canDraw(doc: Doc): boolean {
  return doc.note.layers[doc.activeLayer]?.visible ?? true;
}

export function initialState(note: Note, tools: ToolSettings = DEFAULT_TOOLS): EditorState {
  return { ...docFromNote(note), tools };
}

function sorted(indices: Iterable<number>): number[] {
  return [...new Set(indices)].sort((a, b) => a - b);
}

/** The doc with `note`, keeping the doc itself when the note did not change. */
function withNote(doc: Doc, note: Note, selection: readonly number[] = doc.selection): Doc {
  if (note === doc.note && selection === doc.selection) return doc;
  return { ...doc, note, selection };
}

export function select(doc: Doc, indices: Iterable<number>): Doc {
  const selection = sorted(indices).filter((i) => i >= 0 && i < doc.note.strokes.length);
  const same = selection.length === doc.selection.length && selection.every((i, k) => i === doc.selection[k]);
  return same ? doc : { ...doc, selection };
}

export function clearSelection(doc: Doc): Doc {
  return doc.selection.length === 0 ? doc : { ...doc, selection: [] };
}

export function selectAll(doc: Doc): Doc {
  return select(
    doc,
    doc.note.strokes.flatMap((s, i) => (doc.note.layers[s.layer]?.visible === false ? [] : [i])),
  );
}

export function setActiveLayer(doc: Doc, layer: number): Doc {
  if (layer < 0 || layer >= doc.note.layers.length || layer === doc.activeLayer) return doc;
  return { ...doc, activeLayer: layer };
}

/** A new stroke drawn with the tool settings on the active layer (none if it is hidden). */
export function addStroke(doc: Doc, stroke: Omit<Stroke, 'layer'>): Doc {
  if (!canDraw(doc)) return doc;
  return withNote(doc, ops.addStroke(doc.note, { ...stroke, layer: doc.activeLayer }), []);
}

export function erase(doc: Doc, path: readonly Vec[], radius: number, mode: EraserMode): Doc {
  const note = mode === 'whole' ? ops.eraseWhole(doc.note, path, radius) : ops.erasePartial(doc.note, path, radius);
  return note === doc.note ? doc : withNote(doc, note, []);
}

/** The style on the selection, or on the whole note when nothing is selected. */
export function applyStyle(doc: Doc, style: ops.StrokeStyle, wholeNote = doc.selection.length === 0): Doc {
  return withNote(doc, ops.setStyle(doc.note, wholeNote ? null : doc.selection, style));
}

export function transformSelection(doc: Doc, m: Affine): Doc {
  return withNote(doc, ops.transformStrokes(doc.note, doc.selection, m));
}

export function deleteSelection(doc: Doc): Doc {
  return withNote(doc, ops.removeStrokes(doc.note, doc.selection), []);
}

export function selectedStrokes(doc: Doc): Stroke[] {
  return doc.selection.map((i) => doc.note.strokes[i]).filter((s): s is Stroke => s !== undefined);
}

/**
 * Pastes copies of `strokes` on the active layer, shifted; the copies become the selection.
 * Nothing happens while the active layer is hidden.
 */
export function paste(doc: Doc, strokes: readonly Stroke[], offset: Vec = { x: PASTE_OFFSET, y: PASTE_OFFSET }): Doc {
  if (!canDraw(doc)) return doc;
  const { note, indices } = ops.insertCopies(doc.note, strokes, offset, doc.activeLayer);
  return withNote(doc, note, indices);
}

export function duplicateSelection(doc: Doc): Doc {
  return paste(doc, selectedStrokes(doc));
}

export function selectionBounds(doc: Doc): Rect | null {
  return doc.selection.length === 0 ? null : ops.strokesBounds(doc.note, doc.selection);
}

// --- Layers -------------------------------------------------------------------------------

export function addLayer(doc: Doc, name: string): Doc {
  const note = ops.addLayer(doc.note, name);
  return { ...doc, note, activeLayer: note.layers.length - 1 };
}

export function setLayerVisible(doc: Doc, index: number, visible: boolean): Doc {
  const note = ops.setLayerVisible(doc.note, index, visible);
  if (note === doc.note) return doc;
  // Hidden strokes cannot stay selected.
  const selection = visible ? doc.selection : doc.selection.filter((i) => doc.note.strokes[i]?.layer !== index);
  // New ink must not land on a hidden layer: draw on the nearest visible one instead.
  const activeLayer =
    !visible && index === doc.activeLayer ? (nearestVisibleLayer(note, index) ?? doc.activeLayer) : doc.activeLayer;
  return { ...withNote(doc, note, selection), activeLayer };
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
  return { ...doc, note, selection: [], activeLayer: Math.max(0, active) };
}

// --- Page ---------------------------------------------------------------------------------

export function setBackground(doc: Doc, background: Background): Doc {
  return withNote(doc, ops.setBackground(doc.note, background));
}

export function rotate(doc: Doc, direction: 'cw' | 'ccw'): Doc {
  return withNote(doc, ops.rotateQuarter(doc.note, direction));
}

export function crop(doc: Doc, rect: Rect): Doc {
  const note = ops.cropTo(doc.note, rect);
  return note === doc.note ? doc : withNote(doc, note, []);
}

export function autoCrop(doc: Doc, margin?: number): Doc {
  const rect = ops.autoCropRect(doc.note, margin);
  return rect ? crop(doc, rect) : doc;
}
