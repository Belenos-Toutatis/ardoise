import { describe, expect, it } from 'vitest';
import { escapeStep, type EscapeState, type EscapeStep } from './escape';

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

const step = (state: Partial<EscapeState>) => escapeStep({ ...NOTHING, ...state });

describe('what Escape does', () => {
  it('goes back to the library when nothing is going on, open or selected', () => {
    expect(step({})).toBe('leave');
  });

  it('does one step for each thing in the order: drag, panels, question, replay, crop, selection, library', () => {
    // Everything on at once: each press removes the smallest thing, and the order is the one of the specification.
    const all: EscapeState = { ...NOTHING, dragging: true, split: true, settings: true, question: true, replaying: true, cropping: true, selected: 3 };
    const expected: EscapeStep[] = ['cancelDrag', 'closeSplit', 'closeSettings', 'question', 'leaveReplay', 'leaveCrop', 'deselect', 'leave'];
    let state = all;
    for (const want of expected) {
      expect(escapeStep(state), want).toBe(want);
      state = {
        ...state,
        dragging: want === 'cancelDrag' ? false : state.dragging,
        split: want === 'closeSplit' ? false : state.split,
        settings: want === 'closeSettings' ? false : state.settings,
        question: want === 'question' ? false : state.question,
        replaying: want === 'leaveReplay' ? false : state.replaying,
        cropping: want === 'leaveCrop' ? false : state.cropping,
        selected: want === 'deselect' ? 0 : state.selected,
      };
    }
  });

  it('answers the unsaved-changes question before anything, even a dialog', () => {
    expect(step({ leaving: true, owned: true, dragging: true, selected: 2 })).toBe('stay');
  });

  it('leaves a dialog its Escape, and a field typed in its own, except where a panel is closed first', () => {
    expect(step({ owned: true, selected: 2 })).toBe('ignore');
    expect(step({ typing: true, selected: 2 })).toBe('ignore');
    expect(step({ typing: true })).toBe('ignore');
    // The cutting panel and the setting close from their own fields too, as they always did.
    expect(step({ typing: true, split: true })).toBe('closeSplit');
    expect(step({ typing: true, settings: true })).toBe('closeSettings');
  });

  it('the gesture under way comes before everything but a dialog', () => {
    expect(step({ dragging: true, split: true, selected: 4, replaying: true })).toBe('cancelDrag');
    expect(step({ dragging: true, owned: true })).toBe('ignore');
  });
});

describe('Escape and the equation being written', () => {
  it('ends the equation from its own field as Enter does, after a gesture and before the panels and the selection', () => {
    expect(step({ editingEquation: true, typing: true, selected: 1 })).toBe('endEquation');
    expect(step({ editingEquation: true, split: true })).toBe('endEquation');
    // At the same step as the text frame being typed: the selection it leaves is the next press's.
    expect(step({ editingText: true, typing: true, selected: 1 })).toBe('endText');
    expect(step({ selected: 1 })).toBe('deselect');
    expect(step({ editingEquation: true, dragging: true })).toBe('cancelDrag');
    expect(step({ editingEquation: true, owned: true })).toBe('ignore');
    expect(step({ editingEquation: false, typing: true })).toBe('ignore');
  });
});
