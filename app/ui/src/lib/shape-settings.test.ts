import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ShapeSettings } from './note';
import {
  CLOSED_EXTRA_MM,
  DEFAULT_SHAPE_SETTINGS,
  SHAPES_SAVE_DELAY_MS,
  ShapesSaver,
  clampShapeSettings,
  closedMinMm,
  lettersAtRisk,
  sameShapeSettings,
  toShapeTuning,
} from './shape-settings';

describe('shape settings', () => {
  it('the defaults are the thresholds of before M15', () => {
    expect(DEFAULT_SHAPE_SETTINGS).toEqual({
      tolerance: 3,
      minSizeMm: 10,
      snapAngle: true,
      snapAngleDeg: 4,
      snapRound: true,
      snapRoundPercent: 10,
    });
    expect(toShapeTuning({ ...DEFAULT_SHAPE_SETTINGS })).toEqual({
      tolerance: 3,
      minSizeMm: 10,
      snapAngleDeg: 4,
      snapRoundPercent: 10,
    });
  });

  it('brings every value within its limits and fills what is missing', () => {
    expect(clampShapeSettings({ tolerance: 9, minSizeMm: 99, snapAngleDeg: 0, snapRoundPercent: 50 })).toMatchObject({
      tolerance: 5,
      minSizeMm: 25,
      snapAngleDeg: 1,
      snapRoundPercent: 20,
    });
    expect(clampShapeSettings({ tolerance: -1, minSizeMm: 1, snapRoundPercent: 0 })).toMatchObject({
      tolerance: 1,
      minSizeMm: 5,
      snapRoundPercent: 2,
    });
    expect(clampShapeSettings(null)).toEqual(DEFAULT_SHAPE_SETTINGS);
    expect(clampShapeSettings({ tolerance: Number.NaN, snapAngle: 'yes' as unknown as boolean })).toEqual(DEFAULT_SHAPE_SETTINGS);
    // A value between two steps of the tolerance is rounded.
    expect(clampShapeSettings({ tolerance: 3.6 }).tolerance).toBe(4);
  });

  it('an unticked alignment box gives a null limit to the recognition', () => {
    const tuning = toShapeTuning({ ...DEFAULT_SHAPE_SETTINGS, snapAngle: false, snapRound: false, minSizeMm: 7 });
    expect(tuning).toEqual({ tolerance: 3, minSizeMm: 7, snapAngleDeg: null, snapRoundPercent: null });
  });

  it('compares two settings value by value', () => {
    expect(sameShapeSettings(DEFAULT_SHAPE_SETTINGS, { ...DEFAULT_SHAPE_SETTINGS })).toBe(true);
    const changes: Partial<ShapeSettings>[] = [
      { tolerance: 4 },
      { minSizeMm: 11 },
      { snapAngle: false },
      { snapAngleDeg: 5 },
      { snapRound: false },
      { snapRoundPercent: 11 },
    ];
    for (const change of changes) {
      expect(sameShapeSettings(DEFAULT_SHAPE_SETTINGS, { ...DEFAULT_SHAPE_SETTINGS, ...change })).toBe(false);
    }
  });

  it('a closed shape needs 3 mm more, and handwriting is at risk below 1 cm', () => {
    expect(CLOSED_EXTRA_MM).toBe(3);
    expect(closedMinMm(10)).toBe(13);
    expect(closedMinMm(5)).toBe(8);
    expect(lettersAtRisk(9)).toBe(true);
    expect(lettersAtRisk(5)).toBe(true);
    expect(lettersAtRisk(10)).toBe(false);
    expect(lettersAtRisk(25)).toBe(false);
  });
});

describe('ShapesSaver', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const values = (minSizeMm: number): ShapeSettings => ({ ...DEFAULT_SHAPE_SETTINGS, minSizeMm });

  it('writes only the last values, once, a moment after the last change', async () => {
    const save = vi.fn(async (_: ShapeSettings) => {});
    const saver = new ShapesSaver(save);
    saver.update(values(6));
    await vi.advanceTimersByTimeAsync(SHAPES_SAVE_DELAY_MS - 50);
    saver.update(values(7));
    await vi.advanceTimersByTimeAsync(SHAPES_SAVE_DELAY_MS - 50);
    // Each change restarted the wait.
    expect(save).not.toHaveBeenCalled();
    expect(saver.pending).toBe(true);
    await vi.advanceTimersByTimeAsync(100);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledWith(values(7));
    expect(saver.pending).toBe(false);
  });

  it('flushes at once, and does nothing when nothing waits', async () => {
    const save = vi.fn(async (_: ShapeSettings) => {});
    const saver = new ShapesSaver(save);
    await saver.flush();
    expect(save).not.toHaveBeenCalled();
    saver.update(values(8));
    await saver.flush();
    expect(save).toHaveBeenCalledWith(values(8));
    // The timer of the flushed change does not write it again.
    await vi.advanceTimersByTimeAsync(SHAPES_SAVE_DELAY_MS * 2);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it('keeps writes in order and reports a failure without losing the next ones', async () => {
    const done: number[] = [];
    const errors: unknown[] = [];
    const save = vi.fn(async (s: ShapeSettings) => {
      if (s.minSizeMm === 6) throw new Error('disk full');
      done.push(s.minSizeMm);
    });
    const saver = new ShapesSaver(save, (error) => errors.push(error));
    saver.update(values(6));
    const first = saver.flush();
    saver.update(values(9));
    await Promise.all([first, saver.flush()]);
    expect(done).toEqual([9]);
    expect(errors).toHaveLength(1);
  });
});
