// @vitest-environment jsdom
// The notices of the library's activity: how long they stay while the pointer rests on one of them.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LibraryActivity, undoNotice } from './library-activity.svelte';

const realDuration = undoNotice.durationMs;

beforeEach(() => {
  vi.useFakeTimers();
  undoNotice.durationMs = 1000;
});

afterEach(() => {
  vi.useRealTimers();
  undoNotice.durationMs = realDuration;
});

describe('LibraryActivity notices', () => {
  it('holds every notice while the pointer is on one, and lets them go once released', () => {
    const activity = new LibraryActivity(async () => undefined);
    activity.show('A', null);
    activity.show('B', null);
    activity.hold();
    vi.advanceTimersByTime(5000);
    expect(activity.offers).toHaveLength(2);
    activity.release();
    vi.advanceTimersByTime(1000);
    expect(activity.offers).toHaveLength(0);
  });

  it('releases the others when the notice under the pointer is closed (no mouseleave comes from a removed element)', () => {
    const activity = new LibraryActivity(async () => undefined);
    const first = activity.show('A', null);
    activity.show('B', null);
    activity.hold();
    activity.dismissOffer(first);
    vi.advanceTimersByTime(1000);
    expect(activity.offers).toHaveLength(0);
    // The next notices go by themselves too.
    activity.show('C', null);
    vi.advanceTimersByTime(1000);
    expect(activity.offers).toHaveLength(0);
  });
});
