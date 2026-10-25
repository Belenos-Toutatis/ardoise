/**
 * "Redresser les formes" in the editor: the selected strokes that are a clear line, ellipse,
 * rectangle or arrow are replaced by their straightened version (`Ink.straighten`, the Rust
 * code of `ardoise_ink::shapes` compiled to WebAssembly), in one undoable step.
 */
import type { Ink } from '../ink';
import type { Stroke } from '../note';
import type { Doc } from './state';

export interface StraightenOutcome {
  /** The new doc (the same object when nothing was recognised). */
  doc: Doc;
  /** How many shapes were straightened. */
  shapes: number;
}

/**
 * Straightens the shapes among the selected strokes. The new strokes take the place of the
 * first stroke they replace (so they stay above and below the same ink) and are selected with
 * the selected strokes that did not change.
 */
export function straightenSelection(doc: Doc, ink: Pick<Ink, 'straighten'>): StraightenOutcome {
  const selected = doc.selection.filter((i) => doc.note.strokes[i] !== undefined);
  if (selected.length === 0) return { doc, shapes: 0 };
  const groups = ink.straighten(selected.map((i) => doc.note.strokes[i]));
  if (groups.length === 0) return { doc, shapes: 0 };

  const removed = new Set<number>();
  const placed = new Map<number, Stroke[]>();
  for (const group of groups) {
    const indices = group.sources.map((s) => selected[s]).filter((i) => i !== undefined);
    if (indices.length === 0) continue;
    for (const i of indices) removed.add(i);
    placed.set(Math.min(...indices), group.strokes);
  }
  const wasSelected = new Set(selected);
  const strokes: Stroke[] = [];
  const selection: number[] = [];
  doc.note.strokes.forEach((stroke, i) => {
    const made = placed.get(i);
    if (made) {
      for (const s of made) {
        selection.push(strokes.length);
        strokes.push(s);
      }
    } else if (!removed.has(i)) {
      if (wasSelected.has(i)) selection.push(strokes.length);
      strokes.push(stroke);
    }
  });
  return { doc: { ...doc, note: { ...doc.note, strokes }, selection }, shapes: placed.size };
}
