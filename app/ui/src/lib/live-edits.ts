/**
 * Applying a `strokesEdited` event to a list of finished strokes (docs/design/tablet-live-classroom.md,
 * "M12"), in place and in one pass whatever the number of strokes named: a stroke keeps its object,
 * so the renderer keeps its outline. Shared by `LiveState` and the browser stand-in.
 */
import type { LiveStrokesEdited, PlacedStroke, RecolouredStrokes } from './live';
import type { Stroke } from './note';

/** What one edit changed on the page, for a canvas to redraw only there (`LiveRenderer`). */
export interface StrokeEdit {
  /** `LiveState.generation` right after this edit. */
  generation: number;
  /** The strokes that left the page, came to it or changed colour. */
  touched: readonly Stroke[];
}

/** Takes out the strokes at `removed` (ascending). */
export function removeStrokes(strokes: Stroke[], removed: readonly number[]): void {
  if (removed.length === 0) return;
  let next = 0;
  let write = removed[0];
  for (let read = removed[0]; read < strokes.length; read++) {
    if (removed[next] === read) next += 1;
    else strokes[write++] = strokes[read];
  }
  strokes.length = write;
}

/** Puts each stroke at its index in the list as it ends up (ascending). */
export function insertStrokes(strokes: Stroke[], inserted: readonly PlacedStroke[]): void {
  if (inserted.length === 0) return;
  let read = strokes.length - 1;
  let write = strokes.length + inserted.length - 1;
  strokes.length = write + 1;
  for (let i = inserted.length - 1; i >= 0; i--) {
    const { index, stroke } = inserted[i];
    while (write > index) strokes[write--] = strokes[read--];
    strokes[write--] = stroke;
  }
}

/**
 * Gives strokes their new colour. A new `color` on the same object: an outline does not depend on
 * the colour (`Ink.contours`), and the colour objects themselves are never changed.
 */
export function recolourStrokes(strokes: readonly Stroke[], recoloured: readonly RecolouredStrokes[]): void {
  for (const { color, indices } of recoloured) {
    for (const index of indices) {
      const stroke = strokes[index];
      if (stroke) stroke.color = color;
    }
  }
}

/** Applies a `strokesEdited` event to `strokes`; returns the strokes it touched (those it removes included). */
export function applyStrokeEdit(strokes: Stroke[], edit: Pick<LiveStrokesEdited, 'removed' | 'inserted' | 'recoloured'>): Stroke[] {
  const touched: Stroke[] = [];
  for (const index of edit.removed) {
    const stroke = strokes[index];
    if (stroke) touched.push(stroke);
  }
  removeStrokes(strokes, edit.removed);
  insertStrokes(strokes, edit.inserted);
  for (const { stroke } of edit.inserted) touched.push(stroke);
  recolourStrokes(strokes, edit.recoloured);
  for (const { indices } of edit.recoloured) {
    for (const index of indices) {
      const stroke = strokes[index];
      if (stroke) touched.push(stroke);
    }
  }
  return touched;
}
