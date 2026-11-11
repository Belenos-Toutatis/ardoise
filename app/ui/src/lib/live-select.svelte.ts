/**
 * The selection tool of the control window (SPEC §4.2 « Retoucher pendant le direct »;
 * docs/design/tablet-live-classroom.md, "M12"): which strokes of the page on screen are chosen, and
 * how a lasso or a click chooses them. It lives in the control window only: nothing of it is sent
 * to Rust, to the projector or to the share, whose screens only see the result of an edit.
 *
 * The indices are those of `LiveState.strokes` (the finished strokes of the page, hidden layers
 * included). They stay valid while the tablet writes (it only appends) and are dropped as soon as
 * the page or its revision changes (`follow`): a delete or an undo moves the indices; a change of
 * colour does not, so it keeps the selection. The stroke in progress cannot be chosen: it is not in
 * the list yet.
 */
import { boundsOfPoints, type Vec } from './editor/geometry';
import { hitStroke, selectInLasso, type Hittable } from './editor/hit';

/** What a gesture does to the selection: replaces it, adds to it (Shift), removes from it (Ctrl). */
export type Combine = 'replace' | 'add' | 'remove';

/** A gesture shorter than this (screen pixels) is a click, as in the editor. */
export const CLICK_PX = 4;
/** How close (screen pixels) a click must be to a stroke's ink to pick it, as in the editor. */
export const PICK_PX = 6;

/** What the selection reads of the live session. */
export interface SelectionSource extends Hittable {
  noteId: string;
  pageIndex: number;
  revision: number;
}

/**
 * The strokes a gesture chooses: a click picks the topmost stroke under it, a lasso the strokes
 * with at least half of their samples inside (`selectInLasso`). `scale`: screen pixels per page
 * unit. Strokes of hidden layers are never chosen.
 */
export function pickStrokes(page: Hittable, gesture: readonly Vec[], scale: number): number[] {
  const bounds = boundsOfPoints(gesture);
  if (!bounds) return [];
  if (Math.max(bounds.width, bounds.height) * scale < CLICK_PX) {
    const hit = hitStroke(page, gesture[0], PICK_PX / scale);
    return hit === null ? [] : [hit];
  }
  return selectInLasso(page, gesture);
}

/** The selection once `picked` is combined with `current` (ascending, without repeats). */
export function combineSelection(current: readonly number[], picked: readonly number[], combine: Combine): number[] {
  const set = new Set(current);
  if (combine === 'replace') return [...new Set(picked)].sort((a, b) => a - b);
  for (const index of picked) {
    if (combine === 'add') set.add(index);
    else set.delete(index);
  }
  return [...set].sort((a, b) => a - b);
}

export class LiveSelection {
  /** The Select tool is on. */
  active = $state(false);
  /** The chosen strokes (ascending); a new array at each change. */
  indices = $state.raw<readonly number[]>([]);
  /** What the indices mean: the revision and the page they were chosen at (sent with an edit). */
  revision = 0;
  noteId = '';
  pageIndex = -1;

  get count(): number {
    return this.indices.length;
  }

  get any(): boolean {
    return this.indices.length > 0;
  }

  /** Turns the tool on or off; turned off, it drops the selection. */
  toggle(on = !this.active): void {
    this.active = on;
    if (!on) this.clear();
  }

  clear(): void {
    if (this.indices.length > 0) this.indices = [];
  }

  /** Chooses the strokes a gesture picked, combined with the current selection, on the page as it is now. */
  choose(source: SelectionSource, picked: readonly number[], combine: Combine): void {
    // A selection of another page or revision is not extended: the new gesture starts again.
    const kept = this.stillValid(source) ? this.indices : [];
    const next = combineSelection(kept, picked, combine);
    this.revision = source.revision;
    this.noteId = source.noteId;
    this.pageIndex = source.pageIndex;
    if (next.length === 0 && this.indices.length === 0) return;
    this.indices = next;
  }

  /**
   * Drops the selection when the page or its revision is not the one it was chosen at (the page
   * turned, an edit or an undo moved the indices). A change of colour and the tablet's writing do
   * not change the revision. Strokes of a layer hidden since leave the selection (M14, as in the
   * editor): the session refuses to edit a hidden stroke, and nothing would show what is chosen.
   */
  follow(source: SelectionSource): void {
    if (this.indices.length === 0) return;
    if (!this.stillValid(source)) {
      this.clear();
      return;
    }
    const shown = this.indices.filter((index) => {
      const stroke = source.strokes[index];
      return stroke === undefined || source.layers[stroke.layer]?.visible !== false;
    });
    if (shown.length !== this.indices.length) this.indices = shown;
  }

  private stillValid(source: SelectionSource): boolean {
    return source.revision === this.revision && source.noteId === this.noteId && source.pageIndex === this.pageIndex;
  }
}
