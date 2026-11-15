/**
 * The way the Select tool selects (a rectangle or a lasso: `SelectWay`, docs/questions.md Q22), as one preference of
 * this computer for the editor and for live mode ("la même manière"): choosing the tool from another one gives the way
 * used last (the rectangle the very first time), and a click on the tool's button while it is the tool (or its key V)
 * turns it. Kept in the browser storage like the other interface preferences ("Draw with a finger", the side panel's
 * tab), so it survives a restart; the control window and the other windows follow each other's changes.
 */
import { nextSelectWay, type SelectWay } from './editor/state';

export const SELECT_WAY_KEY = 'ardoise.select.way';

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** The remembered way (the rectangle when nothing is stored, or the value is not one, or the storage is blocked). */
export function loadSelectWay(store: Pick<Storage, 'getItem'> | null = storage()): SelectWay {
  try {
    return store?.getItem(SELECT_WAY_KEY) === 'lasso' ? 'lasso' : 'rectangle';
  } catch {
    return 'rectangle';
  }
}

export function saveSelectWay(way: SelectWay, store: Pick<Storage, 'setItem'> | null = storage()): void {
  try {
    store?.setItem(SELECT_WAY_KEY, way);
  } catch {
    // Private mode or storage full: the choice holds for this session only.
  }
}

class SelectPrefs {
  way = $state<SelectWay>(loadSelectWay());

  constructor() {
    if (typeof window !== 'undefined') {
      // Another window changed it.
      window.addEventListener('storage', (e) => {
        if (e.key === SELECT_WAY_KEY || e.key === null) this.way = loadSelectWay();
      });
    }
  }

  set(way: SelectWay): void {
    this.way = way;
    saveSelectWay(way);
  }

  /** The tool's button (or V) was used while the tool is on: the other way. */
  turn(): void {
    this.set(nextSelectWay(this.way));
  }

  /** Back to the first-time state (the tests). */
  reset(): void {
    this.way = 'rectangle';
    try {
      storage()?.removeItem(SELECT_WAY_KEY);
    } catch {
      // Nothing stored.
    }
  }
}

export const selectPrefs = new SelectPrefs();
