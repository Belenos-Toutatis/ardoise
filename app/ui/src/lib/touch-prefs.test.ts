import { describe, expect, it } from 'vitest';
import { TOUCH_DRAW_KEY, loadDrawWithFinger, saveDrawWithFinger } from './touch-prefs.svelte';

function store(initial: Record<string, string> = {}) {
  const data = { ...initial };
  return {
    data,
    getItem: (k: string) => data[k] ?? null,
    setItem: (k: string, v: string) => {
      data[k] = v;
    },
  };
}

describe('the "Draw with a finger" preference', () => {
  it('is off until chosen', () => {
    expect(loadDrawWithFinger(store())).toBe(false);
    expect(loadDrawWithFinger(store({ [TOUCH_DRAW_KEY]: 'nonsense' }))).toBe(false);
    expect(loadDrawWithFinger(null)).toBe(false);
  });

  it('is remembered on this computer', () => {
    const s = store();
    saveDrawWithFinger(true, s);
    expect(s.data[TOUCH_DRAW_KEY]).toBe('1');
    expect(loadDrawWithFinger(s)).toBe(true);
    saveDrawWithFinger(false, s);
    expect(loadDrawWithFinger(s)).toBe(false);
  });

  it('survives a storage that refuses to answer', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadDrawWithFinger(broken)).toBe(false);
    expect(() => saveDrawWithFinger(true, broken)).not.toThrow();
  });
});
