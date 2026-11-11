/**
 * "Redresser les formes" in the editor: the selected strokes that are a clear line, ellipse,
 * rectangle or arrow are replaced by their straightened version (`Ink.straighten`, the Rust
 * code of `ardoise_ink::shapes` compiled to WebAssembly), in one undoable step. The recognition
 * follows the user's setting (`ShapeTuning`, M15); the panel next to the button previews what
 * would change with `previewStraightening`, and applying uses the same groups.
 */
import type { Ink, ShapeTuning, Straightened } from '../ink';
import type { Stroke } from '../note';
import type { Doc } from './state';

export interface StraightenOutcome {
  /** The new doc (the same object when nothing was recognised). */
  doc: Doc;
  /** How many shapes were straightened. */
  shapes: number;
}

/** What straightening the selection would do, without doing it. */
export interface StraightenPreview {
  /** How many strokes are selected. */
  selected: number;
  /** The shapes recognised among them. */
  groups: Straightened[];
  /** Indices (in the doc's strokes) of the strokes that the shapes replace. */
  sources: number[];
  /** The straightened strokes. */
  strokes: Stroke[];
}

/** The doc's selected strokes, as indices. */
function selectedIndices(doc: Doc): number[] {
  return doc.selection.filter((i) => doc.note.strokes[i] !== undefined);
}

/** The shapes recognised among the selected strokes with `tuning` (the defaults when absent). */
export function previewStraightening(doc: Doc, ink: Pick<Ink, 'straighten'>, tuning?: ShapeTuning): StraightenPreview {
  const selected = selectedIndices(doc);
  const groups = selected.length === 0 ? [] : ink.straighten(selected.map((i) => doc.note.strokes[i]), tuning);
  const sources: number[] = [];
  const strokes: Stroke[] = [];
  for (const group of groups) {
    for (const s of group.sources) {
      const index = selected[s];
      if (index !== undefined) sources.push(index);
    }
    strokes.push(...group.strokes);
  }
  return { selected: selected.length, groups, sources, strokes };
}

/**
 * Puts the recognised shapes in place of their sources. The new strokes take the place of the
 * first stroke they replace (so they stay above and below the same ink) and are selected with
 * the selected strokes that did not change.
 */
export function applyStraightened(doc: Doc, groups: readonly Straightened[]): StraightenOutcome {
  const selected = selectedIndices(doc);
  const removed = new Set<number>();
  const placed = new Map<number, Stroke[]>();
  for (const group of groups) {
    const indices = group.sources.map((s) => selected[s]).filter((i) => i !== undefined);
    if (indices.length === 0) continue;
    for (const i of indices) removed.add(i);
    placed.set(Math.min(...indices), group.strokes);
  }
  if (placed.size === 0) return { doc, shapes: 0 };
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

/** Straightens the shapes among the selected strokes, recognised with `tuning` (the defaults when absent). */
export function straightenSelection(doc: Doc, ink: Pick<Ink, 'straighten'>, tuning?: ShapeTuning): StraightenOutcome {
  const { groups } = previewStraightening(doc, ink, tuning);
  return groups.length === 0 ? { doc, shapes: 0 } : applyStraightened(doc, groups);
}

/** Runs `run` on the next animation frame (a timer where there is none: tests); returns how to cancel it. */
export function nextFrame(run: () => void): () => void {
  if (typeof requestAnimationFrame === 'function') {
    const frame = requestAnimationFrame(run);
    return () => cancelAnimationFrame(frame);
  }
  const timer = setTimeout(run, 16);
  return () => clearTimeout(timer);
}
