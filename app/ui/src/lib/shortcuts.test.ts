// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { setLocale } from './i18n.svelte';
import { chordEvents, chordMatches, chordParts, usKeyCode } from './shortcuts';

describe('chordMatches', () => {
  it('compares single letters without case and the other key names as they are', () => {
    expect(chordMatches({ key: 'v' }, { key: 'V' })).toBe(true);
    expect(chordMatches({ key: 'V' }, { key: 'v' })).toBe(true);
    expect(chordMatches({ key: 'ArrowLeft' }, { key: 'ArrowLeft' })).toBe(true);
    expect(chordMatches({ key: 'ArrowLeft' }, { key: 'arrowleft' })).toBe(false);
    expect(chordMatches({ key: 'v' }, { key: 'b' })).toBe(false);
  });

  it('wants the modifiers to agree, Ctrl being Ctrl or Meta', () => {
    expect(chordMatches({ key: 'z', ctrl: true }, { key: 'z', ctrlKey: true })).toBe(true);
    expect(chordMatches({ key: 'z', ctrl: true }, { key: 'z', metaKey: true })).toBe(true);
    expect(chordMatches({ key: 'z', ctrl: true }, { key: 'z' })).toBe(false);
    expect(chordMatches({ key: 'z' }, { key: 'z', ctrlKey: true })).toBe(false);
    expect(chordMatches({ key: 'z' }, { key: 'z', altKey: true })).toBe(false);
    expect(chordMatches({ key: 'z', alt: true }, { key: 'z', altKey: true })).toBe(true);
    expect(chordMatches({ key: 'z' }, { key: 'z', shiftKey: true })).toBe(false);
    expect(chordMatches({ key: 'z', shift: true }, { key: 'z', shiftKey: true })).toBe(true);
    expect(chordMatches({ key: 'z', shift: true }, { key: 'z' })).toBe(false);
  });

  it('accepts either Shift state with shift: any', () => {
    expect(chordMatches({ key: '+', shift: 'any' }, { key: '+' })).toBe(true);
    expect(chordMatches({ key: '+', shift: 'any' }, { key: '+', shiftKey: true })).toBe(true);
  });

  it('compares the key code when the chord gives one', () => {
    const french = { key: 'à', code: 'Digit0', ctrl: true, shift: 'any' } as const;
    expect(chordMatches(french, { key: 'à', code: 'Digit0', ctrlKey: true })).toBe(true);
    expect(chordMatches(french, { key: 'à', code: 'Digit1', ctrlKey: true })).toBe(false);
    expect(chordMatches(french, { key: 'à', ctrlKey: true })).toBe(false);
    // Without a code in the chord, the code of the event does not matter.
    expect(chordMatches({ key: '0' }, { key: '0', code: 'Numpad0' })).toBe(true);
  });
});

describe('chordEvents', () => {
  it('gives one press, or two with shift: any', () => {
    expect(chordEvents({ key: 'z', ctrl: true })).toEqual([
      { key: 'z', code: 'KeyZ', ctrlKey: true, shiftKey: false, altKey: false, repeat: false, target: null },
    ]);
    const zoom = chordEvents({ key: '+', shift: 'any' });
    expect(zoom.map((e) => e.shiftKey)).toEqual([false, true]);
    expect(zoom.every((e) => e.key === '+' && e.code === 'Equal')).toBe(true);
  });

  it('writes a letter upper-case with Shift and keeps the code of the chord', () => {
    expect(chordEvents({ key: 'v', shift: true })[0]).toMatchObject({ key: 'V', code: 'KeyV', shiftKey: true });
    expect(chordEvents({ key: 'à', code: 'Digit0', ctrl: true })[0]).toMatchObject({ key: 'à', code: 'Digit0', ctrlKey: true });
  });

  it('uses the US key codes', () => {
    expect(usKeyCode('0')).toBe('Digit0');
    expect(usKeyCode(' ')).toBe('Space');
    expect(usKeyCode('-')).toBe('Minus');
    expect(usKeyCode('=')).toBe('Equal');
    expect(usKeyCode('ArrowLeft')).toBe('ArrowLeft');
    expect(usKeyCode('F4')).toBe('F4');
    expect(usKeyCode('à')).toBeUndefined();
  });
});

describe('chordParts', () => {
  beforeEach(() => setLocale('fr'));

  it('shows the parts in French, then in English', () => {
    expect(chordParts({ key: 'z', ctrl: true })).toEqual(['Ctrl', 'Z']);
    expect(chordParts({ key: 'z', ctrl: true, shift: true })).toEqual(['Ctrl', 'Maj', 'Z']);
    expect(chordParts({ key: 'ArrowLeft', alt: true })).toEqual(['Alt', '←']);
    expect(chordParts({ key: 'Escape' })).toEqual(['Échap']);
    expect(chordParts({ key: 'PageDown' })).toEqual(['Pg suiv']);
    expect(chordParts({ key: ' ' })).toEqual(['Espace']);
    expect(chordParts({ key: 'Delete', ctrl: true })).toEqual(['Ctrl', 'Suppr']);
    setLocale('en');
    expect(chordParts({ key: 'z', ctrl: true, shift: true })).toEqual(['Ctrl', 'Shift', 'Z']);
    expect(chordParts({ key: 'Escape' })).toEqual(['Esc']);
    expect(chordParts({ key: 'PageDown' })).toEqual(['Page Down']);
    expect(chordParts({ key: ' ' })).toEqual(['Space']);
  });

  it('shows no Shift for shift: any, − for the minus key, a letter in capitals', () => {
    expect(chordParts({ key: '+', shift: 'any' })).toEqual(['+']);
    expect(chordParts({ key: '-', shift: 'any' })).toEqual(['−']);
    expect(chordParts({ key: 'v' })).toEqual(['V']);
    expect(chordParts({ key: 'F4', ctrl: true })).toEqual(['Ctrl', 'F4']);
  });
});
