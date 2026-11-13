import { describe, expect, it } from 'vitest';
import type { KeyInput } from '../live-keys';
import { keyPress, type EditorKeyContext } from '../shortcuts';
import { editorKeyAction, isFitKey, isFitWidthKey } from './keys';

const editing: EditorKeyContext = { replaying: false };
const replaying: EditorKeyContext = { replaying: true };
const BOTH = [editing, replaying];

type Mods = { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean; repeat?: boolean; code?: string };

/** A key press; a letter is upper-case with Shift, and the code is the US one unless `code` says otherwise. */
function press(key: string, mods: Mods = {}): KeyInput {
  const e = keyPress(key, mods.code, mods);
  if (mods.meta) e.metaKey = true;
  if (mods.repeat) e.repeat = true;
  return e;
}

function kind(key: string, mods: Mods = {}, context: EditorKeyContext = editing): string | null {
  return editorKeyAction(press(key, mods), context)?.kind ?? null;
}

describe('editorKeyAction: close and save', () => {
  it('Ctrl + W and Ctrl + F4 close in both contexts; held down, with Shift or Alt, or alone they do not', () => {
    for (const context of BOTH) {
      expect(kind('w', { ctrl: true }, context)).toBe('close');
      expect(kind('F4', { ctrl: true }, context)).toBe('close');
      expect(kind('w', { meta: true }, context)).toBe('close');
      expect(kind('w', { ctrl: true, repeat: true }, context)).toBeNull();
      expect(kind('w', { ctrl: true, shift: true }, context)).toBeNull();
      expect(kind('w', { ctrl: true, alt: true }, context)).toBeNull();
      expect(kind('F4', { alt: true }, context)).toBeNull();
    }
    expect(kind('w')).toBeNull();
  });

  it("Ctrl + Q is the app's: the editor maps nothing for it", () => {
    for (const context of BOTH) expect(kind('q', { ctrl: true }, context)).toBeNull();
  });

  it('Ctrl + S saves, with or without Shift, in a replay too, and again when held down', () => {
    for (const context of BOTH) {
      expect(kind('s', { ctrl: true }, context)).toBe('save');
      expect(kind('s', { ctrl: true, shift: true }, context)).toBe('save');
      expect(kind('s', { meta: true }, context)).toBe('save');
      expect(kind('s', { ctrl: true, repeat: true }, context)).toBe('save');
    }
    expect(kind('s')).toBeNull();
  });

  it('Ctrl + S takes Alt along, as the handler always did (AltGr is Ctrl + Alt on Windows)', () => {
    expect(kind('s', { ctrl: true, alt: true })).toBe('save');
  });
});

describe('editorKeyAction: Alt + arrows', () => {
  it('Alt + ← and Alt + → turn the page, in a replay too', () => {
    for (const context of BOTH) {
      expect(kind('ArrowLeft', { alt: true }, context)).toBe('previousPage');
      expect(kind('ArrowRight', { alt: true }, context)).toBe('nextPage');
    }
  });

  it('is ignored with Shift or Ctrl, and without Alt the arrows do nothing', () => {
    for (const context of BOTH) {
      expect(kind('ArrowLeft', { alt: true, shift: true }, context)).toBeNull();
      expect(kind('ArrowRight', { alt: true, ctrl: true }, context)).toBeNull();
      expect(kind('ArrowRight', { alt: true, meta: true }, context)).toBeNull();
      expect(kind('ArrowLeft', {}, context)).toBeNull();
      expect(kind('ArrowUp', { alt: true }, context)).toBeNull();
    }
  });
});

describe('editorKeyAction: the keys of a replay', () => {
  it('R and Escape leave it, with or without Shift; Space plays or pauses', () => {
    expect(kind('r', {}, replaying)).toBe('leaveReplay');
    expect(kind('r', { shift: true }, replaying)).toBe('leaveReplay');
    expect(kind('Escape', {}, replaying)).toBe('leaveReplay');
    expect(kind('Escape', { shift: true }, replaying)).toBe('leaveReplay');
    expect(kind(' ', {}, replaying)).toBe('playPause');
    expect(kind(' ', { shift: true }, replaying)).toBe('playPause');
  });

  it('with Ctrl only the fits work; nothing edits the note', () => {
    expect(kind('0', { ctrl: true }, replaying)).toBe('fit');
    expect(kind('2', { ctrl: true, shift: true }, replaying)).toBe('fitWidth');
    expect(kind('à', { ctrl: true, code: 'Digit0' }, replaying)).toBe('fit');
    for (const k of ['z', 'y', 'a', 'c', 'x', 'v', 'r', 'Delete', 'Backspace', 'Escape', ' ']) {
      expect(kind(k, { ctrl: true }, replaying), `Ctrl + ${k}`).toBeNull();
    }
  });

  it('with Alt, or for any other key, nothing', () => {
    for (const k of ['r', 'Escape', ' ', 'v', 'p', 'e', 'h', 'f', 'Delete', 'Backspace']) {
      expect(kind(k, { alt: true }, replaying), `Alt + ${k}`).toBeNull();
    }
    for (const k of ['v', 'p', 'e', 'h', 'f', 'Delete', 'Backspace', 'Enter', 'a']) {
      expect(kind(k, {}, replaying), k).toBeNull();
    }
  });
});

describe('editorKeyAction: Ctrl keys', () => {
  it('copy, cut and paste', () => {
    expect(kind('c', { ctrl: true })).toBe('copy');
    expect(kind('x', { ctrl: true })).toBe('cut');
    expect(kind('v', { ctrl: true })).toBe('paste');
    expect(kind('c', { meta: true })).toBe('copy');
    expect(kind('c', { ctrl: true, repeat: true })).toBe('copy');
  });

  it('Ctrl + Shift + C, X and V, and Ctrl + Alt + C, X and V do nothing', () => {
    for (const k of ['c', 'x', 'v']) {
      expect(kind(k, { ctrl: true, shift: true }), `Ctrl + Shift + ${k}`).toBeNull();
      expect(kind(k, { ctrl: true, alt: true }), `Ctrl + Alt + ${k}`).toBeNull();
    }
  });

  it('undo and redo: Ctrl + Z, Ctrl + Shift + Z, Ctrl + Y (with or without Shift)', () => {
    expect(kind('z', { ctrl: true })).toBe('undo');
    expect(kind('z', { ctrl: true, shift: true })).toBe('redo');
    expect(kind('y', { ctrl: true })).toBe('redo');
    expect(kind('y', { ctrl: true, shift: true })).toBe('redo');
    expect(kind('z', { meta: true })).toBe('undo');
  });

  it('select all, with or without Shift', () => {
    expect(kind('a', { ctrl: true })).toBe('selectAll');
    expect(kind('a', { ctrl: true, shift: true })).toBe('selectAll');
  });

  it('Ctrl + 0 and Ctrl + 2, whatever the layout: the key or its code (Digit, Numpad), with or without Shift', () => {
    expect(kind('0', { ctrl: true })).toBe('fit');
    expect(kind('0', { ctrl: true, shift: true })).toBe('fit');
    expect(kind('à', { ctrl: true, code: 'Digit0' })).toBe('fit');
    expect(kind('à', { ctrl: true, shift: true, code: 'Digit0' })).toBe('fit');
    expect(kind('0', { ctrl: true, code: 'Numpad0' })).toBe('fit');
    expect(kind('2', { ctrl: true })).toBe('fitWidth');
    expect(kind('2', { ctrl: true, shift: true })).toBe('fitWidth');
    expect(kind('é', { ctrl: true, code: 'Digit2' })).toBe('fitWidth');
    expect(kind('é', { ctrl: true, shift: true, code: 'Digit2' })).toBe('fitWidth');
    expect(kind('2', { ctrl: true, code: 'Numpad2' })).toBe('fitWidth');
    expect(kind('1', { ctrl: true })).toBeNull();
  });

  it('the fit keys are checked before the letters (a key code of Digit0 wins over a letter)', () => {
    expect(kind('z', { ctrl: true, code: 'Digit0' })).toBe('fit');
    expect(kind('z', { ctrl: true, code: 'Digit2' })).toBe('fitWidth');
  });

  it('other Ctrl keys do nothing, and never fall through to the plain keys', () => {
    for (const k of ['p', 'e', 'h', 'r', 'f', 'Delete', 'Backspace', 'Escape', ' ', 'b', '1', '+', '-', 'ArrowLeft']) {
      expect(kind(k, { ctrl: true }), `Ctrl + ${k}`).toBeNull();
    }
    expect(kind('v', { ctrl: true, shift: true })).toBeNull();
  });

  it('Ctrl + Alt keys: undo, redo, select all and the fits still work (AltGr), the clipboard keys do not', () => {
    expect(kind('z', { ctrl: true, alt: true })).toBe('undo');
    expect(kind('a', { ctrl: true, alt: true })).toBe('selectAll');
    expect(kind('0', { ctrl: true, alt: true })).toBe('fit');
  });
});

describe('editorKeyAction: the plain keys', () => {
  it('Delete and Backspace delete, Escape acts by steps, Space holds the pan, with or without Shift', () => {
    for (const shift of [false, true]) {
      expect(kind('Delete', { shift })).toBe('delete');
      expect(kind('Backspace', { shift })).toBe('delete');
      expect(kind('Escape', { shift })).toBe('escape');
      expect(kind(' ', { shift })).toBe('panHold');
    }
  });

  it('V, P, E, H pick the tool, R replays, F straightens, in either case', () => {
    const tools: Record<string, string> = { v: 'select', p: 'pen', e: 'eraser', h: 'pan', r: 'replay', f: 'straighten' };
    for (const [letter, expected] of Object.entries(tools)) {
      expect(kind(letter), letter).toBe(expected);
      expect(kind(letter, { shift: true }), `Shift + ${letter}`).toBe(expected);
      expect(kind(letter, { alt: true }), `Alt + ${letter}`).toBeNull();
    }
  });

  it('Alt makes every plain key nothing', () => {
    for (const k of ['Delete', 'Backspace', 'Escape', ' ']) expect(kind(k, { alt: true }), `Alt + ${k}`).toBeNull();
  });

  it('other keys do nothing, nor does the old name of Space', () => {
    for (const k of ['a', 'z', 'b', 's', 'w', 'q', '0', '2', '+', '-', '=', 'Enter', 'Tab', 'F1', 'ArrowUp', 'Home', 'Spacebar']) {
      expect(kind(k), k).toBeNull();
    }
  });

  it('a held key maps as a pressed one (the handler has no repeat test)', () => {
    expect(kind('v', { repeat: true })).toBe('select');
    expect(kind('Delete', { repeat: true })).toBe('delete');
  });
});

describe('isFitKey and isFitWidthKey', () => {
  it('read the key or the code', () => {
    expect(isFitKey({ key: '0' })).toBe(true);
    expect(isFitKey({ key: 'à', code: 'Digit0' })).toBe(true);
    expect(isFitKey({ key: '0', code: 'Numpad0' })).toBe(true);
    expect(isFitKey({ key: 'o', code: 'KeyO' })).toBe(false);
    expect(isFitWidthKey({ key: '2' })).toBe(true);
    expect(isFitWidthKey({ key: 'é', code: 'Digit2' })).toBe(true);
    expect(isFitWidthKey({ key: '2', code: 'Numpad2' })).toBe(true);
    expect(isFitWidthKey({ key: '0', code: 'Digit0' })).toBe(false);
  });
});
