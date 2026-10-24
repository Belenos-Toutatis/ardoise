import { describe, expect, it } from 'vitest';
import { commit, createHistory, undo } from './history';
import { hitStroke, selectInLasso } from './hit';
import { eraseWhole, erasePartial, rotateQuarter, setStyle } from './ops';
import { bigNote } from './test-fixtures';

function time(run: () => unknown): number {
  const start = performance.now();
  run();
  return performance.now() - start;
}

describe('big notes (300 strokes, 20 000 samples)', () => {
  // Generous bounds: they catch accidental quadratic work, not small regressions.
  it('keeps hit tests and edits fast', () => {
    const n = bigNote();
    expect(n.strokes.reduce((sum, s) => sum + s.points.length, 0)).toBe(20100);
    const lasso = [
      { x: 0, y: 0 },
      { x: 15000, y: 0 },
      { x: 15000, y: 21600 },
      { x: 0, y: 21600 },
    ];
    let selected: number[] = [];
    expect(time(() => (selected = selectInLasso(n, lasso)))).toBeLessThan(150);
    expect(selected.length).toBeGreaterThan(100);
    expect(time(() => hitStroke(n, { x: 5000, y: 5000 }, 50))).toBeLessThan(50);
    const segment = [
      { x: 5000, y: 0 },
      { x: 5200, y: 21600 },
    ];
    expect(time(() => eraseWhole(n, segment, 200))).toBeLessThan(100);
    expect(time(() => erasePartial(n, segment, 200))).toBeLessThan(100);
    // One eraser move (a short segment) touches only nearby strokes.
    expect(time(() => erasePartial(n, [{ x: 5000, y: 5000 }, { x: 5050, y: 5030 }], 200))).toBeLessThan(20);
    expect(time(() => setStyle(n, null, { width: 80 }))).toBeLessThan(50);
    expect(time(() => rotateQuarter(n, 'cw'))).toBeLessThan(100);
  });

  it('keeps a long history cheap', () => {
    let h = createHistory(bigNote());
    const elapsed = time(() => {
      for (let i = 0; i < 1000; i++) h = commit(h, setStyle(h.present, [i % 300], { width: 61 + (i % 5) }));
      for (let i = 0; i < 1000; i++) h = undo(h);
    });
    expect(h.past).toHaveLength(0);
    expect(elapsed).toBeLessThan(2000);
  });
});
