import { describe, expect, it, vi } from 'vitest';
import type { LiveBatch, LiveEvent, LiveSnapshot } from './live';
import { ResumeWatch, resumeBlock } from './live-resume.svelte';
import type { Note } from './note';
import type { TabletStatus } from './tablet';

function status(change: Partial<TabletStatus>): TabletStatus {
  return { connection: 'searching', registered: { name: 'Slate', width: 29700, height: 21600 }, ...change } as TabletStatus;
}

describe('resumeBlock', () => {
  it('needs a registered tablet, which is not busy', () => {
    expect(resumeBlock(null)).toBe('noTablet');
    expect(resumeBlock(status({ registered: null, connection: 'notRegistered' }))).toBe('noTablet');
    expect(resumeBlock(status({ connection: 'syncing' }))).toBe('busy');
    expect(resumeBlock(status({ connection: 'registering' }))).toBe('busy');
    for (const connection of ['searching', 'connecting', 'connected', 'live'] as const) {
      expect(resumeBlock(status({ connection }))).toBeNull();
    }
  });
});

function snapshot(change: Partial<LiveSnapshot> = {}, title = 'Cours'): LiveSnapshot {
  return {
    state: 'live',
    page: { title } as Note,
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    ...change,
  } as LiveSnapshot;
}

describe('ResumeWatch', () => {
  const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: 0 });

  it('follows canTurnPage through the subscription, its first snapshot and the batches', async () => {
    const watch = new ResumeWatch();
    let send: (b: LiveBatch) => void = () => {};
    const forwarded: LiveBatch[] = [];
    const subscribe = watch.watch(async (onBatch) => {
      send = onBatch;
      return snapshot({ canTurnPage: false });
    });
    const first = await subscribe((b) => forwarded.push(b));
    expect(first.canTurnPage).toBe(false);
    expect(watch.canTurnPage).toBe(false);

    const next = batch({ type: 'snapshot', snapshot: snapshot({ canTurnPage: true, pageIndex: 1 }) });
    send(next);
    expect(watch.canTurnPage).toBe(true);
    expect(forwarded).toEqual([next]);
    // Absent (the share's viewers): the page turns.
    send(batch({ type: 'snapshot', snapshot: snapshot({ canTurnPage: false }) }));
    send(batch({ type: 'snapshot', snapshot: snapshot() }));
    expect(watch.canTurnPage).toBe(true);
  });

  it('keeps each fork until it is dismissed, once, and takes the sheet title from the resume', async () => {
    const watch = new ResumeWatch();
    watch.begin(snapshot({ canTurnPage: false }, '  Cours de physique '));
    expect(watch.title).toBe('Cours de physique');
    expect(watch.canTurnPage).toBe(false);

    let send: (b: LiveBatch) => void = () => {};
    await watch.watch(async (onBatch) => {
      send = onBatch;
      return snapshot();
    })(vi.fn());
    const fork: LiveEvent = { type: 'sheetForked', pageIndex: 0, reason: 'changed', original: 'a.ardoise', path: 'a (2).ardoise' };
    send(batch(fork));
    send(batch(fork));
    expect(watch.forks).toEqual([{ pageIndex: 0, reason: 'changed', original: 'a.ardoise', path: 'a (2).ardoise' }]);
    watch.dismiss(watch.forks[0]);
    expect(watch.forks).toEqual([]);
    send(batch(fork));
    expect(watch.forks).toEqual([]);
  });
});
