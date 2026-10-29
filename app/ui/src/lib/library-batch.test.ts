// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { opposite, runBatch, sameStamp } from './library-batch';
import { setLocale } from './i18n.svelte';

const targets = ['a', 'b', 'c', 'd'].map((path) => ({ path, title: path.toUpperCase() }));

describe('runBatch', () => {
  it('handles every note in order, keeps going after a failure and translates it', async () => {
    setLocale('fr');
    const seen: string[] = [];
    const progress: [number, number][] = [];
    const outcome = await runBatch(
      targets,
      async ({ path }) => {
        seen.push(path);
        if (path === 'b') throw { code: 'noteNotFound', message: 'b' };
        return path.length;
      },
      { onprogress: (done, total) => progress.push([done, total]) },
    );
    expect(seen).toEqual(['a', 'b', 'c', 'd']);
    expect(outcome.done.map((d) => [d.path, d.result])).toEqual([
      ['a', 1],
      ['c', 1],
      ['d', 1],
    ]);
    expect(outcome.failed).toHaveLength(1);
    expect(outcome.failed[0]).toMatchObject({ path: 'b', title: 'B' });
    expect(outcome.failed[0].message).toMatch(/^la note n'existe plus/);
    expect(outcome.stopped).toBe(false);
    expect(progress).toEqual([
      [0, 4],
      [1, 4],
      [2, 4],
      [3, 4],
      [4, 4],
    ]);
  });

  it('stops between two notes when asked', async () => {
    let calls = 0;
    const outcome = await runBatch(targets, async () => ++calls, { shouldStop: () => calls === 2 });
    expect(calls).toBe(2);
    expect(outcome.done).toHaveLength(2);
    expect(outcome.stopped).toBe(true);
  });

  it('has nothing to do for no notes', async () => {
    const outcome = await runBatch([], async () => 1);
    expect(outcome).toEqual({ done: [], failed: [], stopped: false });
  });
});

describe('helpers', () => {
  it('compares stamps and turns directions around', () => {
    const stamp = { modified: { secs_since_epoch: 10, nanos_since_epoch: 5 }, len: 3 };
    expect(sameStamp(stamp, { ...stamp })).toBe(true);
    expect(sameStamp(stamp, { ...stamp, len: 4 })).toBe(false);
    expect(sameStamp(stamp, { ...stamp, modified: { secs_since_epoch: 10, nanos_since_epoch: 6 } })).toBe(false);
    expect(opposite('cw')).toBe('ccw');
    expect(opposite('ccw')).toBe('cw');
  });
});
