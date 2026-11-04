// M12: what the control window knows of a resumed sheet, kept by `LiveState` beside the snapshots
// and batches that tell it: whether the page turns with the tablet, and the sheets that went on in
// a new note.
import { describe, expect, it } from 'vitest';
import type { LiveBatch, LiveEvent, LiveSnapshot, SheetFork } from './live';
import { LiveState, subscribeLive } from './live-state.svelte';
import type { Note } from './note';

function snapshot(change: Partial<LiveSnapshot> = {}): LiveSnapshot {
  return {
    state: 'live',
    page: {
      id: 'p1',
      title: 'Cours',
      page: { width: 29700, height: 21600 },
      background: { paper: 'white', template: 'blank', texture: false },
      layers: [{ name: 'Calque 1', visible: true }],
      strokes: [],
    } as unknown as Note,
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60 },
    view: { zoom: 1, x: 0, y: 0, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
    ...change,
  } as LiveSnapshot;
}

const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: 0 });
const forked = (original = 'a.ardoise', path = 'a (2).ardoise'): LiveEvent => ({
  type: 'sheetForked',
  pageIndex: 0,
  reason: 'changed',
  original,
  path,
});
const fork = (original = 'a.ardoise', path = 'a (2).ardoise'): SheetFork => ({ pageIndex: 0, reason: 'changed', original, path });

describe('live state: canTurnPage', () => {
  it('follows each snapshot, the first and the ones in batches', () => {
    const live = new LiveState();
    expect(live.canTurnPage).toBe(true);
    live.initial(snapshot({ canTurnPage: false }));
    expect(live.canTurnPage).toBe(false);
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ canTurnPage: true, pageIndex: 1, pageCount: 2 }) }));
    expect(live.canTurnPage).toBe(true);
    live.receive(batch({ type: 'snapshot', snapshot: snapshot({ canTurnPage: false }) }));
    expect(live.canTurnPage).toBe(false);
    // Absent (the share's viewers): the page turns.
    live.receive(batch({ type: 'snapshot', snapshot: snapshot() }));
    expect(live.canTurnPage).toBe(true);
  });

  it('is not undone by the older snapshot of the subscription when a newer batch came before it', async () => {
    const live = new LiveState();
    await subscribeLive(live, async (onBatch) => {
      // The channel delivers a batch before the command's answer, which holds an older snapshot.
      onBatch(batch({ type: 'snapshot', snapshot: snapshot({ canTurnPage: true, pageIndex: 1, pageCount: 2 }) }));
      return snapshot({ canTurnPage: false });
    });
    expect(live.pageIndex).toBe(1);
    expect(live.canTurnPage).toBe(true);
  });
});

describe('live state: sheets that went on in a new note', () => {
  it('keeps each fork, once, until it is dismissed', () => {
    const live = new LiveState();
    live.initial(snapshot({ canTurnPage: false }));
    live.receive(batch(forked()));
    live.receive(batch(forked()));
    expect(live.forks).toEqual([fork()]);
    live.dismissFork(live.forks[0]);
    expect(live.forks).toEqual([]);
    // Dismissed, it does not come back.
    live.receive(batch(forked()));
    expect(live.forks).toEqual([]);
    live.receive(batch(forked('b.ardoise', 'b (2).ardoise')));
    expect(live.forks).toEqual([fork('b.ardoise', 'b (2).ardoise')]);
  });

  it('takes the forks told before the snapshot of the subscription', async () => {
    const live = new LiveState();
    await subscribeLive(live, async (onBatch) => {
      onBatch(batch(forked()));
      return snapshot();
    });
    expect(live.forks).toEqual([fork()]);
  });

  it('adopts the forks of live_stop, and gives back only those it was not told', () => {
    const live = new LiveState();
    live.initial(snapshot());
    live.receive(batch(forked()));
    const fresh = live.adoptForks([fork(), fork('b.ardoise', 'b (2).ardoise')]);
    expect(fresh).toEqual([fork('b.ardoise', 'b (2).ardoise')]);
    expect(live.forks).toEqual([fork(), fork('b.ardoise', 'b (2).ardoise')]);
    // A fork the user dismissed is not new either.
    live.dismissFork(live.forks[0]);
    expect(live.adoptForks([fork()])).toEqual([]);
    expect(live.forks).toEqual([fork('b.ardoise', 'b (2).ardoise')]);
  });
});
