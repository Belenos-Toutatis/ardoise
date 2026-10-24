import { describe, expect, it } from 'vitest';
import { clock } from './format';

describe('clock', () => {
  it('writes minutes and seconds, rounding down', () => {
    expect(clock(0)).toBe('0:00');
    expect(clock(999)).toBe('0:00');
    expect(clock(1000)).toBe('0:01');
    expect(clock(61_500)).toBe('1:01');
    expect(clock(3_599_000)).toBe('59:59');
  });

  it('adds hours from one hour on, and survives odd values', () => {
    expect(clock(3_600_000)).toBe('1:00:00');
    expect(clock(3_725_000)).toBe('1:02:05');
    expect(clock(-5)).toBe('0:00');
    expect(clock(NaN)).toBe('0:00');
    expect(clock(Infinity)).toBe('0:00');
  });
});
