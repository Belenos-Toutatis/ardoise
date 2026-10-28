/**
 * Keyboard of live mode, the same in the control window and in the presentation window
 * (docs/design/tablet-live-classroom.md, "Interface"):
 *
 * | Keys                                   | Action                  |
 * |----------------------------------------|-------------------------|
 * | → / Page Down / Space                  | next page               |
 * | ← / Page Up                            | previous page           |
 * | Ctrl + Delete, Ctrl + Backspace        | clear the page          |
 * | B                                      | blackboard              |
 * | L                                      | laser pointer           |
 * | + / −                                  | zoom in / out           |
 * | 0                                      | whole page              |
 * | W                                      | page width              |
 * | Delete, Backspace                      | clear the marks written |
 * |                                        | over the presentation   |
 * | Shift + arrows                         | move the projected area |
 * | Esc                                    | leave                   |
 *
 * Presentation remotes send Page Up / Page Down. Keys typed in a field are left alone, and so
 * are Space and Enter on a focused button (they press it).
 */

export type LiveKeyAction =
  | { kind: 'command'; command: 'nextPage' | 'previousPage' | 'clearPage' }
  | { kind: 'board' }
  | { kind: 'laser' }
  | { kind: 'zoom'; direction: 1 | -1 }
  | { kind: 'wholePage' }
  | { kind: 'fitWidth' }
  | { kind: 'clearMarks' }
  | { kind: 'pan'; dx: -1 | 0 | 1; dy: -1 | 0 | 1 }
  | { kind: 'leave' };

/** What the mapping reads from a `KeyboardEvent`. */
export interface KeyInput {
  key: string;
  code?: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  repeat?: boolean;
  target?: EventTarget | null;
}

/** Duck-typed, so that the mapping also runs outside a browser (unit tests in Node). */
interface ElementLike {
  tagName?: unknown;
  isContentEditable?: unknown;
  type?: unknown;
}

const BUTTON_INPUTS = new Set(['button', 'submit', 'reset']);

/** A field the user types in (text, number, range, select…): its keys are its own. */
export function isTypingTarget(target: EventTarget | null | undefined): boolean {
  const el = target as ElementLike | null | undefined;
  if (!el || typeof el.tagName !== 'string') return false;
  if (el.isContentEditable === true) return true;
  const tag = el.tagName.toUpperCase();
  if (tag === 'TEXTAREA' || tag === 'SELECT') return true;
  return tag === 'INPUT' && !BUTTON_INPUTS.has(String(el.type ?? 'text').toLowerCase());
}

function isButton(target: EventTarget | null | undefined): boolean {
  const el = target as ElementLike | null | undefined;
  if (!el || typeof el.tagName !== 'string') return false;
  const tag = el.tagName.toUpperCase();
  return tag === 'BUTTON' || (tag === 'INPUT' && BUTTON_INPUTS.has(String(el.type ?? '').toLowerCase()));
}

/** The action of a key press in live mode, or null when live mode does not use it. */
export function liveKeyAction(e: KeyInput): LiveKeyAction | null {
  if (isTypingTarget(e.target)) return null;
  const key = e.key;
  const ctrl = !!(e.ctrlKey || e.metaKey);

  if (ctrl) {
    if (e.altKey || e.shiftKey || e.repeat) return null;
    return key === 'Delete' || key === 'Backspace' ? { kind: 'command', command: 'clearPage' } : null;
  }
  if (e.altKey) return null;

  // Moving and zooming repeat while the key is held; the rest acts once per press.
  if (e.shiftKey) {
    switch (key) {
      case 'ArrowLeft':
        return { kind: 'pan', dx: -1, dy: 0 };
      case 'ArrowRight':
        return { kind: 'pan', dx: 1, dy: 0 };
      case 'ArrowUp':
        return { kind: 'pan', dx: 0, dy: -1 };
      case 'ArrowDown':
        return { kind: 'pan', dx: 0, dy: 1 };
    }
  }
  // + and − are typed with or without Shift depending on the keyboard layout (and the keypad).
  if (key === '+' || key === '=' || e.code === 'NumpadAdd') return { kind: 'zoom', direction: 1 };
  if (key === '-' || key === '−' || e.code === 'NumpadSubtract') return { kind: 'zoom', direction: -1 };

  if (e.repeat) return null;
  // On French keyboards 0 is Shift + à: the key code says it too.
  if (key === '0' || e.code === 'Digit0' || e.code === 'Numpad0') return { kind: 'wholePage' };
  if (e.shiftKey && key !== 'B' && key !== 'L' && key !== 'W' && key !== 'PageDown' && key !== 'PageUp') return null;

  switch (key) {
    case 'ArrowRight':
    case 'PageDown':
      return { kind: 'command', command: 'nextPage' };
    case ' ':
    case 'Spacebar':
      return isButton(e.target) ? null : { kind: 'command', command: 'nextPage' };
    case 'ArrowLeft':
    case 'PageUp':
      return { kind: 'command', command: 'previousPage' };
    case 'b':
    case 'B':
      return { kind: 'board' };
    case 'l':
    case 'L':
      return { kind: 'laser' };
    case 'w':
    case 'W':
      return { kind: 'fitWidth' };
    case 'Delete':
    case 'Backspace':
      return { kind: 'clearMarks' };
    case 'Escape':
      return { kind: 'leave' };
    default:
      return null;
  }
}
