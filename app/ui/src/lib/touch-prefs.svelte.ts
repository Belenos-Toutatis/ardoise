/**
 * The pen and touch preference of this computer, kept in the browser storage like the other
 * interface preferences (custom colours, export choices): "Draw with a finger" lets a finger
 * draw (for a touch screen without a pen); by default a finger only pans, pinches and double
 * taps. The control window and the presentation window share the storage and follow each
 * other's changes.
 */

export const TOUCH_DRAW_KEY = 'ardoise.touch.drawWithFinger';

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** The remembered choice (false when nothing is stored or the storage is blocked). */
export function loadDrawWithFinger(store: Pick<Storage, 'getItem'> | null = storage()): boolean {
  try {
    return store?.getItem(TOUCH_DRAW_KEY) === '1';
  } catch {
    return false;
  }
}

export function saveDrawWithFinger(on: boolean, store: Pick<Storage, 'setItem'> | null = storage()): void {
  try {
    store?.setItem(TOUCH_DRAW_KEY, on ? '1' : '0');
  } catch {
    // Private mode or storage full: the choice holds for this session only.
  }
}

class TouchPrefs {
  drawWithFinger = $state(loadDrawWithFinger());

  constructor() {
    if (typeof window !== 'undefined') {
      // The other window changed it.
      window.addEventListener('storage', (e) => {
        if (e.key === TOUCH_DRAW_KEY || e.key === null) this.drawWithFinger = loadDrawWithFinger();
      });
    }
  }

  set(on: boolean): void {
    this.drawWithFinger = on;
    saveDrawWithFinger(on);
  }
}

export const touchPrefs = new TouchPrefs();
