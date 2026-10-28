import { describe, expect, it } from 'vitest';
import { isTypingTarget, liveKeyAction, type KeyInput } from './live-keys';

const key = (k: string, extra: Partial<KeyInput> = {}) => liveKeyAction({ key: k, ...extra });
const element = (tagName: string, extra: Record<string, unknown> = {}) => ({ tagName, ...extra }) as unknown as EventTarget;

describe('live mode keyboard', () => {
  it('turns pages with the arrows, Page Up / Page Down and Space', () => {
    const next = { kind: 'command', command: 'nextPage' };
    const previous = { kind: 'command', command: 'previousPage' };
    expect(key('ArrowRight')).toEqual(next);
    expect(key('PageDown')).toEqual(next);
    expect(key(' ')).toEqual(next);
    expect(key('ArrowLeft')).toEqual(previous);
    expect(key('PageUp')).toEqual(previous);
    // A presenter's remote sends Page Down even with a modifier held by mistake.
    expect(key('PageDown', { shiftKey: true })).toEqual(next);
  });

  it('clears the page with Ctrl + Delete or Ctrl + Backspace only', () => {
    const clear = { kind: 'command', command: 'clearPage' };
    expect(key('Delete', { ctrlKey: true })).toEqual(clear);
    expect(key('Backspace', { ctrlKey: true })).toEqual(clear);
    expect(key('Backspace', { metaKey: true })).toEqual(clear);
    expect(key('Delete', { ctrlKey: true, shiftKey: true })).toBeNull();
    expect(key('Delete', { ctrlKey: true, repeat: true })).toBeNull();
    // Other Ctrl shortcuts are not live mode's.
    expect(key('b', { ctrlKey: true })).toBeNull();
    expect(key('ArrowRight', { ctrlKey: true })).toBeNull();
  });

  it('toggles the blackboard and the laser pointer', () => {
    expect(key('b')).toEqual({ kind: 'board' });
    expect(key('B', { shiftKey: true })).toEqual({ kind: 'board' });
    expect(key('l')).toEqual({ kind: 'laser' });
    expect(key('L')).toEqual({ kind: 'laser' });
    expect(key('b', { altKey: true })).toBeNull();
  });

  it('zooms with + and − on every layout and the keypad, 0 for the whole page', () => {
    const zoomIn = { kind: 'zoom', direction: 1 };
    const zoomOut = { kind: 'zoom', direction: -1 };
    expect(key('+')).toEqual(zoomIn);
    expect(key('+', { shiftKey: true })).toEqual(zoomIn);
    expect(key('=')).toEqual(zoomIn);
    expect(key('Add', { code: 'NumpadAdd' })).toEqual(zoomIn);
    expect(key('-')).toEqual(zoomOut);
    expect(key('Subtract', { code: 'NumpadSubtract' })).toEqual(zoomOut);
    expect(key('0')).toEqual({ kind: 'wholePage' });
    // French keyboard: the 0 key types "à" without Shift.
    expect(key('à', { code: 'Digit0' })).toEqual({ kind: 'wholePage' });
    expect(key('0', { code: 'Numpad0' })).toEqual({ kind: 'wholePage' });
    // Zoom repeats while held; the whole page acts once.
    expect(key('+', { repeat: true })).toEqual(zoomIn);
    expect(key('0', { repeat: true })).toBeNull();
  });

  it('moves the projected area with Shift + arrows (repeating)', () => {
    expect(key('ArrowLeft', { shiftKey: true })).toEqual({ kind: 'pan', dx: -1, dy: 0 });
    expect(key('ArrowRight', { shiftKey: true, repeat: true })).toEqual({ kind: 'pan', dx: 1, dy: 0 });
    expect(key('ArrowUp', { shiftKey: true })).toEqual({ kind: 'pan', dx: 0, dy: -1 });
    expect(key('ArrowDown', { shiftKey: true })).toEqual({ kind: 'pan', dx: 0, dy: 1 });
    expect(key('ArrowUp')).toBeNull();
    expect(key('ArrowDown')).toBeNull();
  });

  it('leaves with Esc, once per press', () => {
    expect(key('Escape')).toEqual({ kind: 'leave' });
    expect(key('Escape', { repeat: true })).toBeNull();
    // Holding → does not race through the pages (or create new ones at the end).
    expect(key('ArrowRight', { repeat: true })).toBeNull();
  });

  it('leaves keys typed in a field alone, and Space on a button presses it', () => {
    expect(key('b', { target: element('INPUT', { type: 'text' }) })).toBeNull();
    expect(key('ArrowRight', { target: element('INPUT', { type: 'range' }) })).toBeNull();
    expect(key('ArrowRight', { target: element('SELECT') })).toBeNull();
    expect(key('Escape', { target: element('TEXTAREA') })).toBeNull();
    expect(key('l', { target: element('DIV', { isContentEditable: true }) })).toBeNull();
    expect(key(' ', { target: element('BUTTON') })).toBeNull();
    expect(key(' ', { target: element('INPUT', { type: 'button' }) })).toBeNull();
    expect(key('ArrowRight', { target: element('BUTTON') })).toEqual({ kind: 'command', command: 'nextPage' });
    expect(key(' ', { target: element('CANVAS') })).toEqual({ kind: 'command', command: 'nextPage' });
    expect(isTypingTarget(element('INPUT', { type: 'submit' }))).toBe(false);
    expect(isTypingTarget(element('input'))).toBe(true);
    expect(isTypingTarget(null)).toBe(false);
  });

  it('ignores the keys it does not use', () => {
    for (const k of ['a', 'Enter', 'Tab', 'F5', 'Home', 'x']) expect(key(k), k).toBeNull();
  });
});

describe('page width and marks (M8)', () => {
  it('shows the page width with W', () => {
    expect(key('w')).toEqual({ kind: 'fitWidth' });
    expect(key('W', { shiftKey: true })).toEqual({ kind: 'fitWidth' });
    expect(key('w', { ctrlKey: true })).toBeNull();
    expect(key('w', { repeat: true })).toBeNull();
  });

  it('clears the marks written over the presentation with Delete or Backspace', () => {
    expect(key('Delete')).toEqual({ kind: 'clearMarks' });
    expect(key('Backspace')).toEqual({ kind: 'clearMarks' });
    // Clearing the page itself stays Ctrl + Delete.
    expect(key('Delete', { ctrlKey: true })).toEqual({ kind: 'command', command: 'clearPage' });
    expect(key('Delete', { target: element('INPUT', { type: 'text' }) })).toBeNull();
  });
});
