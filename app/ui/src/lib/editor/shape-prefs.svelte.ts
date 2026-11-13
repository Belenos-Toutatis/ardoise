/**
 * The state the shape tools share between the toolbar (its three toggles) and the page (`Canvas`), and the snap grid of
 * the page's template (docs/design/shape-tools.md).
 *
 * - `shapePrefs`: the toggles, kept for the life of the app (all the notes opened meanwhile) and not on disk: a
 *   restart starts again with nothing asked, so that nothing is ever straightened or snapped by a choice forgotten
 *   long ago. Shift and Alt invert two of them for one stroke (`effectiveToggles`).
 * - `SnapGridWatch`: the grid of the open note's template, asked of Rust (`template_snap_grid`, which draws it from the
 *   geometry the template is printed with) once per template and page size.
 */
import { api } from '../api';
import type { PageSize, Template } from '../note';
import type { ShapeToggles, SnapGrid } from './shape-tools';

export type ShapeToggle = keyof ShapeToggles;

class ShapePrefs implements ShapeToggles {
  regular = $state(false);
  centre = $state(false);
  snap = $state(false);

  toggle(which: ShapeToggle): void {
    this[which] = !this[which];
  }

  /** Nothing asked (a new session; the tests). */
  reset(): void {
    this.regular = false;
    this.centre = false;
    this.snap = false;
  }
}

export const shapePrefs = new ShapePrefs();

/** The grid of a page's template: null while it is asked for, and for a template with no lines to snap to. */
export class SnapGridWatch {
  grid = $state.raw<SnapGrid | null>(null);
  /** The grids known, by template and page size. */
  #known = new Map<string, SnapGrid | null>();
  #key = '';

  /** The page or its template changed (or the note changed): the grid follows. */
  follow(page: PageSize, template: Template): void {
    const key = `${template}:${page.width}x${page.height}`;
    if (key === this.#key) return;
    this.#key = key;
    if (this.#known.has(key)) {
      this.grid = this.#known.get(key) ?? null;
      return;
    }
    this.grid = null;
    api.templateSnapGrid(page, template).then(
      (grid) => {
        this.#known.set(key, grid);
        // Only the latest question counts.
        if (this.#key === key) this.grid = grid;
      },
      () => {
        // No grid: the toggle stays off, drawing is not affected.
      },
    );
  }
}
