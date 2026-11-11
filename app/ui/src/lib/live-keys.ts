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
 * |                                        | (projected window only) |
 * | Shift + arrows                         | move the projected area |
 * | Esc, Ctrl + W, Ctrl + F4               | leave                   |
 *
 * The control window also has the keys of touching up (`liveEditKeyAction`, at the end). Ctrl + Q is
 * for each window to answer (`closeKey`).
 *
 * The same keys, as data, are `lib/live-shortcuts.ts` (`LIVE_SHORTCUTS`): what the control window's Aide tab shows
 * and what its tests confront with this mapping, both ways (`live-shortcuts.test.ts`). A key added or changed here
 * is a row to add or change there, or the sweep fails. Delete and Backspace without a selection are the
 * presentation window's alone: the control window does nothing with `clearMarks`, so its help lists them apart.
 *
 * Presentation remotes send Page Up / Page Down. Keys typed in a field are left alone. Space
 * always means "next page", even with a button focused (the button of a tool just used would
 * otherwise be pressed again: « Clear the page » by Space wiped the new page) or a checkbox or a
 * radio, such as the switches of the side panel (clicked, they keep the focus and the keys of live
 * mode must not stop). Page Up and Page Down, the class remote's keys, always turn the page (M17: the Page and Fond
 * tabs hold selects and a slider, which would otherwise take the remote and change the paper in front of the class),
 * with a select or a range input focused as well; text fields keep them. Esc closes the
 * projection when it is open and otherwise asks before ending live mode: the window decides.
 */

import { closeKey } from './close-keys';

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

// Inputs that are pressed, not typed in: like a button, they keep the keys of live mode (a switch of the side panel
// just clicked must not swallow the next Page Down), and Space turns the page instead of toggling them.
const BUTTON_INPUTS = new Set(['button', 'submit', 'reset', 'checkbox', 'radio']);

/** Space, whatever the browser calls it. */
export function isSpaceKey(key: string): boolean {
  return key === ' ' || key === 'Spacebar';
}

/** A field the user types in (text, number, range, select…): its keys are its own. Buttons, checkboxes and radios are not. */
export function isTypingTarget(target: EventTarget | null | undefined): boolean {
  const el = target as ElementLike | null | undefined;
  if (!el || typeof el.tagName !== 'string') return false;
  if (el.isContentEditable === true) return true;
  const tag = el.tagName.toUpperCase();
  if (tag === 'TEXTAREA' || tag === 'SELECT') return true;
  return tag === 'INPUT' && !BUTTON_INPUTS.has(String(el.type ?? 'text').toLowerCase());
}

/**
 * The control is a select or a slider: it takes the arrows and the letters, not the remote's Page Up and Page Down
 * (the keys a presentation remote sends), which always turn the page. A text field, a number field and the like keep them.
 */
function yieldsToRemote(target: EventTarget | null | undefined): boolean {
  const el = target as ElementLike | null | undefined;
  if (!el || typeof el.tagName !== 'string') return false;
  const tag = el.tagName.toUpperCase();
  return tag === 'SELECT' || (tag === 'INPUT' && String(el.type ?? '').toLowerCase() === 'range');
}

/** The action of a key press in live mode, or null when live mode does not use it. */
export function liveKeyAction(e: KeyInput): LiveKeyAction | null {
  if (closeKey(e) === 'close') return { kind: 'leave' };
  if (isTypingTarget(e.target) && !((e.key === 'PageDown' || e.key === 'PageUp') && yieldsToRemote(e.target))) return null;
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
      return { kind: 'command', command: 'nextPage' };
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

/** What the control window's keys do to the page's ink (M12): not used by the presentation window. */
export type LiveEditKeyAction =
  | { kind: 'select' }
  | { kind: 'pen' }
  | { kind: 'deselect' }
  | { kind: 'deleteSelection' }
  | { kind: 'undo' }
  | { kind: 'redo' };

/**
 * The keys of touching up, tried by the control window before `liveKeyAction`:
 *
 * | Keys                               | Action                                          |
 * |------------------------------------|-------------------------------------------------|
 * | V                                  | the Select tool on or off (the editor's key)    |
 * | P                                  | the Pen tool on or off (M14, the editor's key)  |
 * | Ctrl + Z                           | undo a delete, a change of colour, a clearing   |
 * | Ctrl + Y, Ctrl + Shift + Z         | redo                                            |
 * | Delete, Backspace (with selection) | delete the selection (otherwise: the marks,     |
 * |                                    | which only the projected window acts on)        |
 * | Esc (with selection)               | deselect (otherwise: what Esc does)             |
 */
export function liveEditKeyAction(e: KeyInput, hasSelection: boolean): LiveEditKeyAction | null {
  if (isTypingTarget(e.target)) return null;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (e.ctrlKey || e.metaKey) {
    if (e.altKey || e.repeat) return null;
    if (key === 'z') return e.shiftKey ? { kind: 'redo' } : { kind: 'undo' };
    return key === 'y' && !e.shiftKey ? { kind: 'redo' } : null;
  }
  if (e.altKey || e.repeat) return null;
  if (key === 'v') return { kind: 'select' };
  if (key === 'p') return { kind: 'pen' };
  if (!hasSelection || e.shiftKey) return null;
  if (key === 'Delete' || key === 'Backspace') return { kind: 'deleteSelection' };
  return key === 'Escape' ? { kind: 'deselect' } : null;
}
