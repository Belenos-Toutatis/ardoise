// Milestone M14: the computer's ink, remembered on this PC (docs/design/tablet-live-classroom.md, "The computer's ink").
import { describe, expect, it } from 'vitest';
import { COMPUTER_STYLE_KEY, DEFAULT_COMPUTER_STYLE, loadComputerStyle, saveComputerStyle } from './live-computer-style';

/** A storage of one page's life. */
function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

const blocked = {
  getItem: (): string | null => {
    throw new Error('SecurityError');
  },
  setItem: (): void => {
    throw new Error('QuotaExceededError');
  },
};

describe('the computer style', () => {
  it('is a red felt tip of 1.2 mm by default', () => {
    expect(DEFAULT_COMPUTER_STYLE).toEqual({ tool: 'feltTip', color: { r: 0xd8, g: 0, b: 0, a: 255 }, width: 120 });
    expect(loadComputerStyle(memory())).toEqual(DEFAULT_COMPUTER_STYLE);
    expect(loadComputerStyle(null)).toEqual(DEFAULT_COMPUTER_STYLE);
  });

  it('comes back as it was saved, the colour as #rrggbb', () => {
    const storage = memory();
    saveComputerStyle({ tool: 'highlighter', color: { r: 0x51, g: 0xa7, b: 0xf9, a: 255 }, width: 500 }, storage);
    expect(JSON.parse(storage.values.get(COMPUTER_STYLE_KEY)!)).toEqual({ tool: 'highlighter', color: '#51a7f9', width: 500 });
    expect(loadComputerStyle(storage)).toEqual({ tool: 'highlighter', color: { r: 0x51, g: 0xa7, b: 0xf9, a: 255 }, width: 500 });
  });

  it('takes the default for what makes no sense, field by field', () => {
    const load = (raw: string) => loadComputerStyle(memory({ [COMPUTER_STYLE_KEY]: raw }));
    expect(load('not json')).toEqual(DEFAULT_COMPUTER_STYLE);
    expect(load('42')).toEqual(DEFAULT_COMPUTER_STYLE);
    expect(load('null')).toEqual(DEFAULT_COMPUTER_STYLE);
    expect(load(JSON.stringify({ tool: 'laser', color: 'red', width: 'thick' }))).toEqual(DEFAULT_COMPUTER_STYLE);
    expect(load(JSON.stringify({ tool: 'pencil', color: '#zz0000', width: 5 }))).toEqual({ ...DEFAULT_COMPUTER_STYLE, tool: 'pencil' });
    expect(load(JSON.stringify({ color: '#00ff00', width: 1001 }))).toEqual({ ...DEFAULT_COMPUTER_STYLE, color: { r: 0, g: 255, b: 0, a: 255 } });
    expect(load(JSON.stringify({ width: 10 }))).toEqual({ ...DEFAULT_COMPUTER_STYLE, width: 10 });
    expect(load(JSON.stringify({ width: 1000 }))).toEqual({ ...DEFAULT_COMPUTER_STYLE, width: 1000 });
  });

  it('never throws with a blocked storage', () => {
    expect(loadComputerStyle(blocked)).toEqual(DEFAULT_COMPUTER_STYLE);
    expect(() => saveComputerStyle(DEFAULT_COMPUTER_STYLE, blocked)).not.toThrow();
  });

  it('hands out a copy of the default: changing it changes nothing', () => {
    const style = loadComputerStyle(memory());
    style.color.r = 1;
    expect(loadComputerStyle(memory()).color.r).toBe(0xd8);
  });
});
