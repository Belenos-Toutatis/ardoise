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

describe('chordParts: the help in both languages', () => {
  beforeEach(() => setLocale('fr'));

  it('writes the arrows, the paging keys and the editing keys the same way in a chord and alone', () => {
    expect(chordParts({ key: 'ArrowRight', alt: true })).toEqual(['Alt', '→']);
    expect(chordParts({ key: 'ArrowUp', shift: true })).toEqual(['Maj', '↑']);
    expect(chordParts({ key: 'ArrowDown' })).toEqual(['↓']);
    expect(chordParts({ key: 'PageUp' })).toEqual(['Pg préc']);
    expect(chordParts({ key: 'Backspace', ctrl: true })).toEqual(['Ctrl', 'Retour arrière']);
    expect(chordParts({ key: 'Spacebar' })).toEqual(['Espace']);
    setLocale('en');
    expect(chordParts({ key: 'PageUp' })).toEqual(['Page Up']);
    expect(chordParts({ key: 'Delete', ctrl: true })).toEqual(['Ctrl', 'Delete']);
    expect(chordParts({ key: 'Backspace' })).toEqual(['Backspace']);
    expect(chordParts({ key: 'ArrowLeft', alt: true })).toEqual(['Alt', '←']);
  });

  it('lists Ctrl, Alt, Shift in that order, and shows a key that has a code the way its character reads', () => {
    expect(chordParts({ key: 'à', code: 'Digit0', ctrl: true, alt: true, shift: true })).toEqual(['Ctrl', 'Alt', 'Maj', 'À']);
    expect(chordParts({ key: '0', code: 'Numpad0', ctrl: true })).toEqual(['Ctrl', '0']);
  });
});

describe('chordMatches and chordEvents agree', () => {
  it('matches every press chordEvents builds for the chord, and no press with another modifier', () => {
    const chords = [
      { key: 'z', ctrl: true },
      { key: 'z', ctrl: true, shift: true },
      { key: '+', shift: 'any' as const },
      { key: ' ' },
      { key: 'à', code: 'Digit0', ctrl: true, shift: 'any' as const },
      { key: 'ArrowLeft', alt: true },
    ];
    for (const chord of chords) {
      for (const press of chordEvents(chord)) expect(chordMatches(chord, press), JSON.stringify(chord)).toBe(true);
    }
    const plain = chordEvents({ key: 'z' })[0];
    expect(chordMatches({ key: 'z', ctrl: true }, plain)).toBe(false);
    expect(chordMatches({ key: 'z', ctrl: true }, { ...plain, ctrlKey: true, altKey: true })).toBe(false);
  });

  it('builds a Ctrl press on a letter as a lower-case key, and a Shift one upper-case', () => {
    expect(chordEvents({ key: 'Z', ctrl: true })[0].key).toBe('z');
    expect(chordEvents({ key: 'z', ctrl: true, shift: true })[0].key).toBe('Z');
    expect(chordEvents({ key: 'Escape' })[0]).toMatchObject({ key: 'Escape', code: 'Escape', shiftKey: false });
  });
});
