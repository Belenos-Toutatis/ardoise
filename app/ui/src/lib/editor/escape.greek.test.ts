import { describe, expect, it } from 'vitest';
import { escapeStep, type EscapeState } from './escape';

const NOTHING: EscapeState = {
  leaving: false,
  owned: false,
  dragging: false,
  split: false,
  settings: false,
  typing: false,
  question: false,
  replaying: false,
  cropping: false,
  selected: 0,
};

describe('Escape with the Greek palette of the text tool open', () => {
  it('closes the palette first, then ends the typing, then deselects', () => {
    let state: EscapeState = { ...NOTHING, greekPalette: true, editingText: true, typing: true, selected: 1 };
    expect(escapeStep(state)).toBe('closeGreekPalette');
    state = { ...state, greekPalette: false };
    expect(escapeStep(state)).toBe('endText');
    state = { ...state, editingText: false, typing: false };
    expect(escapeStep(state)).toBe('deselect');
  });

  it('leaves a gesture under way and an open dialog before the palette', () => {
    expect(escapeStep({ ...NOTHING, greekPalette: true, dragging: true })).toBe('cancelDrag');
    expect(escapeStep({ ...NOTHING, greekPalette: true, owned: true })).toBe('ignore');
    expect(escapeStep({ ...NOTHING, greekPalette: true, leaving: true })).toBe('stay');
    // Open without a frame being typed (the hint says to touch the page): it closes too.
    expect(escapeStep({ ...NOTHING, greekPalette: true })).toBe('closeGreekPalette');
  });
});
