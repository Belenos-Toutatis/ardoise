// @vitest-environment jsdom
// The Select tool's way is one preference of this computer, for the editor and for live mode, kept in the browser
// storage (docs/questions.md Q22): the rectangle the very first time, then the way used last.
import { beforeEach, describe, expect, it } from 'vitest';
import { SELECT_WAY_KEY, loadSelectWay, saveSelectWay, selectPrefs } from './select-prefs.svelte';

beforeEach(() => {
  selectPrefs.reset();
});

describe('the remembered way', () => {
  it('is the rectangle when nothing is stored, or when what is stored is not a way', () => {
    expect(loadSelectWay({ getItem: () => null })).toBe('rectangle');
    expect(loadSelectWay({ getItem: () => 'triangle' })).toBe('rectangle');
    expect(loadSelectWay({ getItem: () => 'rectangle' })).toBe('rectangle');
    expect(loadSelectWay({ getItem: () => 'lasso' })).toBe('lasso');
  });

  it('is the rectangle when the storage cannot be read or written (private mode)', () => {
    expect(
      loadSelectWay({
        getItem: () => {
          throw new Error('blocked');
        },
      }),
    ).toBe('rectangle');
    expect(() =>
      saveSelectWay('lasso', {
        setItem: () => {
          throw new Error('full');
        },
      }),
    ).not.toThrow();
    expect(loadSelectWay(null)).toBe('rectangle');
  });

  it('is stored under its own key as soon as it is set, and turned from one way to the other', () => {
    expect(selectPrefs.way).toBe('rectangle');
    selectPrefs.turn();
    expect(selectPrefs.way).toBe('lasso');
    expect(localStorage.getItem(SELECT_WAY_KEY)).toBe('lasso');
    selectPrefs.turn();
    expect(selectPrefs.way).toBe('rectangle');
    expect(localStorage.getItem(SELECT_WAY_KEY)).toBe('rectangle');
    selectPrefs.set('lasso');
    expect(loadSelectWay()).toBe('lasso');
  });

  it('follows the other window\'s change, and forgets everything with a reset', () => {
    localStorage.setItem(SELECT_WAY_KEY, 'lasso');
    window.dispatchEvent(new StorageEvent('storage', { key: SELECT_WAY_KEY }));
    expect(selectPrefs.way).toBe('lasso');
    // Another key is not ours.
    localStorage.setItem(SELECT_WAY_KEY, 'rectangle');
    window.dispatchEvent(new StorageEvent('storage', { key: 'ardoise.other' }));
    expect(selectPrefs.way).toBe('lasso');
    selectPrefs.reset();
    expect(selectPrefs.way).toBe('rectangle');
    expect(localStorage.getItem(SELECT_WAY_KEY)).toBeNull();
  });
});
