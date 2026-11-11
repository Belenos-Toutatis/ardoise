// Milestone M12: the keys of touching up in the control window (V, undo, redo, Delete and Esc with a selection).
// M14: P, the computer's Pen tool.
import { describe, expect, it } from 'vitest';
import { liveEditKeyAction, liveKeyAction, type KeyInput } from './live-keys';

const key = (k: string, extra: Partial<KeyInput> = {}): KeyInput => ({ key: k, ...extra });
const field = (tagName: string, type?: string) => ({ tagName, type }) as unknown as EventTarget;

describe('keys of touching up', () => {
  it('V turns the Select tool on or off, with or without a selection, in either case', () => {
    for (const has of [false, true]) {
      expect(liveEditKeyAction(key('v'), has)).toEqual({ kind: 'select' });
      expect(liveEditKeyAction(key('V'), has)).toEqual({ kind: 'select' });
    }
    expect(liveEditKeyAction(key('v', { repeat: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('v', { altKey: true }), false)).toBeNull();
  });

  it('P turns the Pen tool on or off (M14), not with Ctrl, Alt or a held key, nor in a field', () => {
    for (const has of [false, true]) {
      expect(liveEditKeyAction(key('p'), has)).toEqual({ kind: 'pen' });
      expect(liveEditKeyAction(key('P'), has)).toEqual({ kind: 'pen' });
    }
    expect(liveEditKeyAction(key('p', { repeat: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('p', { altKey: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('p', { ctrlKey: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('p', { target: field('INPUT', 'text') }), false)).toBeNull();
    // The presentation window has no Pen tool.
    expect(liveKeyAction(key('p'))).toBeNull();
  });

  it('Ctrl+Z undoes, Ctrl+Y and Ctrl+Shift+Z redo (Cmd as well), whatever the selection', () => {
    for (const has of [false, true]) {
      expect(liveEditKeyAction(key('z', { ctrlKey: true }), has)).toEqual({ kind: 'undo' });
      expect(liveEditKeyAction(key('Z', { ctrlKey: true }), has)).toEqual({ kind: 'undo' });
      expect(liveEditKeyAction(key('z', { metaKey: true }), has)).toEqual({ kind: 'undo' });
      expect(liveEditKeyAction(key('y', { ctrlKey: true }), has)).toEqual({ kind: 'redo' });
      expect(liveEditKeyAction(key('Z', { ctrlKey: true, shiftKey: true }), has)).toEqual({ kind: 'redo' });
    }
    expect(liveEditKeyAction(key('z', { ctrlKey: true, repeat: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('z', { ctrlKey: true, altKey: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('Y', { ctrlKey: true, shiftKey: true }), false)).toBeNull();
    expect(liveEditKeyAction(key('a', { ctrlKey: true }), false)).toBeNull();
  });

  it('Delete, Backspace and Esc are the selection\'s only when there is one', () => {
    expect(liveEditKeyAction(key('Delete'), true)).toEqual({ kind: 'deleteSelection' });
    expect(liveEditKeyAction(key('Backspace'), true)).toEqual({ kind: 'deleteSelection' });
    expect(liveEditKeyAction(key('Escape'), true)).toEqual({ kind: 'deselect' });
    // Without a selection they keep doing what they did.
    for (const k of ['Delete', 'Backspace', 'Escape']) {
      expect(liveEditKeyAction(key(k), false)).toBeNull();
      expect(liveKeyAction(key(k))).not.toBeNull();
    }
  });

  it('Ctrl+Delete still clears the page, with a selection too', () => {
    expect(liveEditKeyAction(key('Delete', { ctrlKey: true }), true)).toBeNull();
    expect(liveKeyAction(key('Delete', { ctrlKey: true }))).toEqual({ kind: 'command', command: 'clearPage' });
  });

  it('leaves the other keys, and fields, alone', () => {
    for (const k of ['b', 'l', 'w', '0', '+', 'ArrowRight', ' ', 'PageDown']) expect(liveEditKeyAction(key(k), true)).toBeNull();
    for (const target of [field('INPUT', 'text'), field('TEXTAREA'), field('SELECT')]) {
      expect(liveEditKeyAction(key('v', { target }), true)).toBeNull();
      expect(liveEditKeyAction(key('z', { ctrlKey: true, target }), true)).toBeNull();
      expect(liveEditKeyAction(key('Delete', { target }), true)).toBeNull();
    }
    // A focused button is not a field.
    expect(liveEditKeyAction(key('Delete', { target: field('BUTTON') }), true)).toEqual({ kind: 'deleteSelection' });
  });

  it('does not change what the presentation window\'s mapping returns for the same keys', () => {
    expect(liveKeyAction(key('v'))).toBeNull();
    expect(liveKeyAction(key('z', { ctrlKey: true }))).toBeNull();
    expect(liveKeyAction(key('Delete'))).toEqual({ kind: 'clearMarks' });
    expect(liveKeyAction(key('Escape'))).toEqual({ kind: 'leave' });
  });
});
