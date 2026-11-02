import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveMark } from './live';
import { MARKS_SEND_MS, MarksSender } from './live-marks-sender';
import { markStyle, newMark } from './live-marks';
import type { Stroke } from './note';

function mark(x: number, n = 2): Stroke {
  const stroke = newMark(markStyle(false), { x, y: 100, pressure: 0.5, t: 0 }, 0.1);
  for (let i = 1; i < n; i++) stroke.points.push({ x: x + 10 * i, y: 100, pressure: 0.5, t: i });
  return stroke;
}

/** A command that answers when the test says so. */
function fakeSend() {
  const calls: { pageIndex: number; marks: LiveMark[] }[] = [];
  const resolvers: (() => void)[] = [];
  const send = vi.fn((pageIndex: number, marks: LiveMark[]) => {
    calls.push({ pageIndex, marks });
    return new Promise<void>((resolve) => resolvers.push(resolve));
  });
  const answer = async () => {
    resolvers.shift()?.();
    await vi.advanceTimersByTimeAsync(0);
  };
  return { send, calls, answer };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('sending the marks to Rust', () => {
  it('sends the first change at once, then at most one list every interval, the latest', async () => {
    const { send, calls, answer } = fakeSend();
    const sender = new MarksSender(send);
    const current = mark(0, 1);
    sender.update(0, [], current);
    expect(calls).toHaveLength(1);
    expect(calls[0].marks[0].points).toHaveLength(1);
    await answer();

    // The mark grows in place while the pen moves: nothing goes before the interval.
    for (let i = 1; i <= 10; i++) {
      current.points.push({ x: 10 * i, y: 100, pressure: 0.5, t: i });
      sender.update(0, [], current);
      await vi.advanceTimersByTimeAsync(MARKS_SEND_MS / 20);
    }
    expect(calls).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(MARKS_SEND_MS);
    expect(calls).toHaveLength(2);
    expect(calls[1].marks[0].points).toHaveLength(11);
    await answer();

    // The pen lifts: the finished mark is in the list, sent when its turn comes.
    sender.update(0, [current], null);
    expect(calls).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(MARKS_SEND_MS);
    expect(calls).toHaveLength(3);
    expect(calls[2].marks).toHaveLength(1);
  });

  it('sends one list at a time, in order, even when the answer is slow', async () => {
    const { send, calls, answer } = fakeSend();
    const sender = new MarksSender(send);
    sender.update(0, [mark(0)], null);
    sender.update(0, [mark(0), mark(100)], null);
    await vi.advanceTimersByTimeAsync(5 * MARKS_SEND_MS);
    expect(calls).toHaveLength(1);
    await answer();
    expect(calls).toHaveLength(2);
    expect(calls[1].marks).toHaveLength(2);
  });

  it('sends an empty list at once, ahead of what waited, and nothing when Rust has none', async () => {
    const { send, calls, answer } = fakeSend();
    const sender = new MarksSender(send);
    // Nothing to remove: nothing sent (a page change with no mark on it).
    sender.update(0, [], null);
    expect(calls).toHaveLength(0);

    sender.update(0, [mark(0)], null);
    await answer();
    sender.update(0, [mark(0), mark(50)], null);
    // Cleared before that list left: only the empty one goes, and straight away.
    sender.update(1, [], null);
    expect(calls).toHaveLength(2);
    expect(calls[1]).toEqual({ pageIndex: 1, marks: [] });
    await answer();
    await vi.advanceTimersByTimeAsync(5 * MARKS_SEND_MS);
    expect(calls).toHaveLength(2);
    // Again nothing to say.
    sender.update(1, [], null);
    expect(calls).toHaveLength(2);
  });

  it('removes its marks when the window goes away, and says nothing if there were none', async () => {
    const first = fakeSend();
    const sender = new MarksSender(first.send);
    sender.update(2, [mark(0)], null);
    sender.close();
    expect(first.calls.map((c) => [c.pageIndex, c.marks.length])).toEqual([
      [2, 1],
      [2, 0],
    ]);
    sender.update(2, [mark(0)], null);
    expect(first.calls).toHaveLength(2);

    const second = fakeSend();
    new MarksSender(second.send).close();
    expect(second.calls).toHaveLength(0);
  });

  it('survives a command that fails', async () => {
    const failing = vi.fn().mockRejectedValueOnce(new Error('not running')).mockResolvedValue(undefined);
    const throwing = vi.fn(() => {
      throw new Error('no bridge');
    });
    const sender = new MarksSender(failing);
    sender.update(0, [mark(0)], null);
    await vi.advanceTimersByTimeAsync(0);
    sender.update(0, [mark(0), mark(20)], null);
    await vi.advanceTimersByTimeAsync(MARKS_SEND_MS);
    expect(failing).toHaveBeenCalledTimes(2);
    expect(() => new MarksSender(throwing).update(0, [mark(0)], null)).not.toThrow();
  });
});
