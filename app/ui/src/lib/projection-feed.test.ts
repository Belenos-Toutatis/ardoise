// The feed of the editor's projection (M17, docs/design/paper-projection-narration.md §2.4): the diff by identity, the
// stroke in progress, one call in flight, the repairs after a refusal.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InkPoint } from './ink';
import type { Note, Stroke } from './note';
import { MAX_PROJECTION_POINTS, type ProjectionUpdate, type ProjectionView } from './projection';
import { END_WAIT_MS, PAGE_RETRY_MAX_MS, PAGE_RETRY_MS, ProjectionFeed, projectionDiff, sentPageOf, type InProgress } from './projection-feed';
import { ReplayTimeline } from './replay';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const RED = { r: 220, g: 0, b: 0, a: 255 };
const VIEW: ProjectionView = { zoom: 1, x: 14850, y: 10800 };

let made = 0;
function stroke(layer = 0, t = (made += 100)): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer,
    points: [0, 1, 2, 3].map((i) => ({ x: 1000 + t + i * 10, y: 2000 + i * 5, pressure: 0.5, t: t + i * 10 })),
  };
}

function note(strokes: Stroke[], over: Partial<Note> = {}): Note {
  return {
    id: 'note-1',
    title: 'Projetée',
    created: 0,
    modified: 0,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
    ...over,
  };
}

const kinds = (updates: readonly ProjectionUpdate[]) => updates.map((u) => u.kind);
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** A feed whose calls answer at once (`auto`) or when the test says so. */
function harness({ auto = true } = {}) {
  const sent: ProjectionUpdate[] = [];
  const answers: { resolve: () => void; reject: (error: unknown) => void }[] = [];
  let refuse: unknown = null;
  const feed = new ProjectionFeed((update) => {
    sent.push(update);
    if (refuse !== null) {
      const error = refuse;
      refuse = null;
      return Promise.reject(error);
    }
    if (auto) return Promise.resolve();
    return new Promise<void>((resolve, reject) => answers.push({ resolve, reject }));
  });
  return {
    feed,
    sent,
    /** The next call is refused with `error`. */
    refuseNext: (error: unknown) => (refuse = error),
    /** Answers the oldest call in flight, then lets the feed send the next one. */
    async answer() {
      answers.shift()?.resolve();
      await flush();
    },
    async fail(error: unknown) {
      answers.shift()?.reject(error);
      await flush();
    },
    inFlight: () => answers.length,
  };
}

function pts(n: number, from = 0): InkPoint[] {
  return Array.from({ length: n }, (_, i) => ({ x: 100 + from + i, y: 200, pressure: 0.4, t: 5000 + (from + i) * 8 }));
}

function begin(id: number, points: readonly InkPoint[]): InProgress {
  return { kind: 'begin', id, tool: 'feltTip', color: RED, width: 80, layer: 0, points, wallAtBase: 1_700_000_000_000, base: 5000 };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('projectionDiff', () => {
  const a = stroke();
  const b = stroke();
  const c = stroke();
  const shown = note([a, b, c]);
  const sent = sentPageOf(shown, null);

  it('sends the whole page first, and nothing when nothing changed', () => {
    expect(projectionDiff(null, shown, null, VIEW)).toEqual({ kind: 'page', note: shown, imageKey: null, view: VIEW });
    expect(projectionDiff(sent, shown, null, VIEW)).toBeNull();
    // Another note object with the same stroke objects: still nothing.
    expect(projectionDiff(sent, { ...shown, title: 'Autre titre' }, null, VIEW)).toBeNull();
  });

  it('sends a stroke appended alone, and an undo as nothing to add', () => {
    const d = stroke();
    expect(projectionDiff(sent, note([a, b, c, d]), null, VIEW)).toEqual({ kind: 'strokes', keep: 3, tail: [d], ended: null });
    expect(projectionDiff(sent, note([a, b]), null, VIEW)).toEqual({ kind: 'strokes', keep: 2, tail: [], ended: null });
    expect(projectionDiff(sent, note([]), null, VIEW)).toEqual({ kind: 'strokes', keep: 0, tail: [], ended: null });
  });

  it('sends what follows a changed stroke in the middle', () => {
    const many = note(Array.from({ length: 10 }, () => stroke()));
    const target = many.strokes[6];
    const moved = { ...target, points: target.points.map((p) => ({ ...p, x: p.x + 500 })) };
    const after = note([...many.strokes.slice(0, 6), moved, ...many.strokes.slice(7)]);
    expect(projectionDiff(sentPageOf(many, null), after, null, VIEW)).toEqual({
      kind: 'strokes',
      keep: 6,
      tail: after.strokes.slice(6),
      ended: null,
    });
  });

  it('sends the page for a tail of more than half the strokes (recolouring everything)', () => {
    // Recolouring makes new objects of the strokes it changes (the immutable doc).
    const recoloured = shown.strokes.map((s) => ({ ...s, color: RED }));
    expect(projectionDiff(sent, note(recoloured), null, VIEW)?.kind).toBe('page');
    // A single stroke, though, goes as a tail.
    const one = note([a]);
    expect(projectionDiff(sentPageOf(one, null), note([stroke()]), null, VIEW)?.kind).toBe('strokes');
  });

  it('sends the page when the layout changes: note, page size, paper, image, layers, image key', () => {
    const changes: Partial<Note>[] = [
      { id: 'note-2' },
      { page: { width: 21600, height: 29700 } },
      { background: { paper: 'cream', template: 'blank', texture: false } },
      { background: { paper: 'white', template: 'seyes', texture: false } },
      { background: { paper: { custom: { r: 1, g: 2, b: 3, a: 255 } }, template: 'blank', texture: false } },
      { backgroundImage: { name: 'fiche.pdf', pdfPage: 1, rect: { x: 0, y: 0, width: 100, height: 100 }, quarterTurns: 0, opacity: 1 } },
      { layers: [{ name: 'Calque 1', visible: false }] },
      { layers: [{ name: 'Renommé', visible: true }] },
      { layers: [{ name: 'Calque 1', visible: true }, { name: 'Calque 2', visible: true }] },
    ];
    for (const change of changes) {
      expect(projectionDiff(sent, note([a, b, c], change), null, VIEW)?.kind, JSON.stringify(change)).toBe('page');
    }
    expect(projectionDiff(sent, shown, 'staged-1', VIEW)?.kind).toBe('page');
    // The same values in new objects are the same layout.
    expect(projectionDiff(sent, note([a, b, c], { background: { ...shown.background }, layers: [{ name: 'Calque 1', visible: true }] }), null, VIEW)).toBeNull();
  });

  it('follows a replay frame by frame with one stroke or none (noteAt keeps the objects)', () => {
    const strokes = Array.from({ length: 20 }, () => stroke());
    const written = note(strokes);
    const timeline = new ReplayTimeline(written);
    let sentPage = sentPageOf(timeline.noteAt(0), null);
    let tails = 0;
    for (let at = 0; at <= timeline.duration + 16; at += 16) {
      const frame = timeline.noteAt(at);
      const update = projectionDiff(sentPage, frame, null, VIEW);
      if (!update) continue;
      if (update.kind !== 'strokes') throw new Error(`a frame sent ${update.kind}`);
      // At most the partial stroke and the one it became (or the next one).
      expect(update.tail.length).toBeLessThanOrEqual(2);
      tails += 1;
      sentPage = sentPageOf(frame, null);
    }
    expect(tails).toBeGreaterThan(10);
    expect(sentPage.strokes).toHaveLength(20);
  });

  it('takes well under a millisecond for a note of 10 000 strokes (an identity pass)', () => {
    const strokes = Array.from({ length: 10_000 }, () => stroke());
    const big = note(strokes);
    const sentBig = sentPageOf(big, null);
    const next = note([...strokes, stroke()]);
    const times: number[] = [];
    for (let i = 0; i < 30; i++) {
      const started = performance.now();
      projectionDiff(sentBig, next, null, VIEW);
      times.push(performance.now() - started);
    }
    times.sort((x, y) => x - y);
    // The median, generous for a busy test machine (measured around 0.05 ms).
    expect(times[15]).toBeLessThan(5);
  });
});

describe('ProjectionFeed', () => {
  it('sends the page first, then only what changed', async () => {
    const h = harness();
    const a = stroke();
    const first = note([a]);
    h.feed.view(VIEW);
    h.feed.show(first, null);
    await flush();
    expect(h.sent).toEqual([{ kind: 'page', note: first, imageKey: null, view: VIEW }]);
    const b = stroke();
    h.feed.show(note([a, b]), null);
    await flush();
    expect(h.sent[1]).toEqual({ kind: 'strokes', keep: 1, tail: [b], ended: null });
    h.feed.show(note([a]), null);
    await flush();
    expect(h.sent[2]).toEqual({ kind: 'strokes', keep: 1, tail: [], ended: null });
    h.feed.view({ zoom: 2, x: 7000, y: 5000 });
    await flush();
    expect(h.sent[3]).toEqual({ kind: 'view', view: { zoom: 2, x: 7000, y: 5000 } });
    // The same view and the same note again: nothing.
    h.feed.view({ zoom: 2, x: 7000, y: 5000 });
    h.feed.show(note([a]), null);
    await flush();
    expect(h.sent).toHaveLength(4);
  });

  it('gives the page the whole-page view before the editor sends one', async () => {
    const h = harness();
    h.feed.show(note([]), null);
    await flush();
    expect(h.sent[0]).toMatchObject({ kind: 'page', view: { zoom: 1, x: 14850, y: 10800 } });
  });

  it('keeps one call in flight and sends the latest note once it ends (coalescing)', async () => {
    const h = harness({ auto: false });
    const strokes = Array.from({ length: 7 }, () => stroke());
    h.feed.show(note(strokes.slice(0, 4)), null);
    await flush();
    expect(h.inFlight()).toBe(1);
    // Three changes while the page is on its way: one update for all three.
    h.feed.show(note(strokes.slice(0, 5)), null);
    h.feed.show(note(strokes.slice(0, 6)), null);
    h.feed.show(note(strokes), null);
    h.feed.view({ zoom: 1.25, x: 14000, y: 10000 });
    await flush();
    expect(h.sent).toHaveLength(1);
    await h.answer();
    expect(h.sent).toHaveLength(2);
    expect(h.sent[1]).toEqual({ kind: 'strokes', keep: 4, tail: strokes.slice(4), ended: null });
    await h.answer();
    expect(h.sent[2]).toEqual({ kind: 'view', view: { zoom: 1.25, x: 14000, y: 10000 } });
    await h.answer();
    expect(h.sent).toHaveLength(3);
  });

  it('streams the stroke in progress, then swaps it for the finished stroke in one update (ended)', async () => {
    const h = harness();
    const a = stroke();
    h.feed.show(note([a]), null);
    await flush();
    const points = pts(3);
    h.feed.stroke(begin(7, points));
    await flush();
    expect(h.sent[1]).toEqual({
      kind: 'strokeBegin',
      id: 7,
      tool: 'feltTip',
      color: RED,
      width: 80,
      layer: 0,
      points: points.map((p) => ({ x: p.x, y: p.y, p: p.pressure, t: p.t, wall: 1_700_000_000_000 + (p.t - 5000) })),
    });
    // The editor's array grows in place: only the new points go.
    points.push(...pts(2, 3));
    h.feed.stroke({ kind: 'grow', id: 7, points });
    await flush();
    expect(h.sent[2]).toMatchObject({ kind: 'strokePoints', id: 7 });
    expect((h.sent[2] as { points: unknown[] }).points).toHaveLength(2);
    // Another stroke's points are not this one's.
    h.feed.stroke({ kind: 'grow', id: 8, points: pts(4) });
    await flush();
    expect(h.sent).toHaveLength(3);
    // Lifted: nothing until the doc has it, then the strokes update says it ended.
    points.push(...pts(5, 5));
    h.feed.stroke({ kind: 'grow', id: 7, points });
    h.feed.stroke({ kind: 'end', id: 7, committed: true });
    await flush();
    expect(h.sent).toHaveLength(3);
    const finished = stroke();
    h.feed.show(note([a, finished]), null);
    await flush();
    expect(h.sent[3]).toEqual({ kind: 'strokes', keep: 1, tail: [finished], ended: 7 });
    expect(h.sent).toHaveLength(4);
  });

  it('cancels a stroke kept but never brought by the doc after a second', async () => {
    vi.useFakeTimers();
    const h = harness();
    h.feed.show(note([]), null);
    await vi.advanceTimersByTimeAsync(0);
    h.feed.stroke(begin(3, pts(2)));
    await vi.advanceTimersByTimeAsync(0);
    h.feed.stroke({ kind: 'end', id: 3, committed: true });
    await vi.advanceTimersByTimeAsync(END_WAIT_MS - 10);
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin']);
    await vi.advanceTimersByTimeAsync(20);
    expect(h.sent[2]).toEqual({ kind: 'strokeCancel', id: 3 });
    // The doc brings it later: an ordinary strokes update.
    const late = stroke();
    h.feed.show(note([late]), null);
    await vi.advanceTimersByTimeAsync(0);
    expect(h.sent[3]).toEqual({ kind: 'strokes', keep: 0, tail: [late], ended: null });
  });

  it('cancels a dropped stroke at once, and sends nothing for a stroke dropped before its begin went', async () => {
    const h = harness({ auto: false });
    h.feed.show(note([]), null);
    await flush();
    h.feed.stroke(begin(1, pts(2)));
    await h.answer();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin']);
    h.feed.stroke({ kind: 'end', id: 1, committed: false });
    await h.answer();
    expect(h.sent[2]).toEqual({ kind: 'strokeCancel', id: 1 });
    // Begun and dropped while the cancel is in flight: Rust never hears of it.
    h.feed.stroke(begin(2, pts(2)));
    h.feed.stroke({ kind: 'end', id: 2, committed: false });
    await h.answer();
    await h.answer();
    expect(h.sent).toHaveLength(3);
  });

  it('ends the stroke Rust shows before the next begins (the window would keep it as a finished stroke)', async () => {
    const h = harness({ auto: false });
    const a = stroke();
    h.feed.show(note([a]), null);
    await h.answer();
    h.feed.stroke(begin(1, pts(2)));
    await h.answer();
    h.feed.stroke({ kind: 'end', id: 1, committed: true });
    const one = stroke();
    h.feed.show(note([a, one]), null);
    // The next stroke begins before the strokes update went.
    h.feed.stroke(begin(2, pts(2)));
    await h.answer();
    await h.answer();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin', 'strokes', 'strokeBegin']);
    expect(h.sent[2]).toEqual({ kind: 'strokes', keep: 1, tail: [one], ended: 1 });
    // Kept but the doc has not brought it, and another begins: cancelled first.
    h.feed.stroke({ kind: 'end', id: 2, committed: true });
    h.feed.stroke(begin(3, pts(2)));
    await h.answer();
    await h.answer();
    expect(kinds(h.sent).slice(4)).toEqual(['strokeCancel', 'strokeBegin']);
    expect(h.sent[4]).toEqual({ kind: 'strokeCancel', id: 2 });
  });

  it('cancels the stroke Rust shows when the page goes instead of the strokes (a layout change)', async () => {
    const h = harness();
    const a = stroke();
    h.feed.show(note([a]), null);
    await flush();
    h.feed.stroke(begin(4, pts(2)));
    await flush();
    h.feed.stroke({ kind: 'end', id: 4, committed: true });
    h.feed.show(note([a, stroke()], { background: { paper: 'cream', template: 'blank', texture: false } }), null);
    await flush();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin', 'page', 'strokeCancel']);
    expect(h.sent[3]).toEqual({ kind: 'strokeCancel', id: 4 });
  });

  it('splits a long stroke into calls of at most MAX_PROJECTION_POINTS points', async () => {
    const h = harness();
    h.feed.show(note([]), null);
    await flush();
    const points = pts(MAX_PROJECTION_POINTS + 10);
    h.feed.stroke(begin(5, points));
    await flush();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin', 'strokePoints']);
    expect((h.sent[1] as { points: unknown[] }).points).toHaveLength(MAX_PROJECTION_POINTS);
    expect((h.sent[2] as { points: unknown[] }).points).toHaveLength(10);
  });

  it('sends the stroke events before a strokes update that follows them', async () => {
    const h = harness({ auto: false });
    const a = stroke();
    h.feed.show(note([a]), null);
    await h.answer();
    h.feed.stroke(begin(1, pts(2)));
    // A change of the doc (an undo with the other hand) and the stroke's begin wait together.
    h.feed.show(note([]), null);
    await h.answer();
    await h.answer();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin', 'strokes']);
  });

  it('sends the page again after a refusal (stale), and the stroke in progress from its first point', async () => {
    const h = harness();
    const a = stroke();
    const shown = note([a]);
    h.feed.show(shown, null);
    await flush();
    const points = pts(3);
    h.feed.stroke(begin(9, points));
    await flush();
    h.refuseNext({ code: 'projectionStale', message: 'keep 2 of 1 strokes' });
    const b = stroke();
    h.feed.show(note([a, b]), null);
    await flush();
    await flush();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin', 'strokes', 'page', 'strokeCancel', 'strokeBegin']);
    expect(h.sent[3]).toMatchObject({ kind: 'page', note: { strokes: [a, b] } });
    expect((h.sent[5] as { points: unknown[] }).points).toHaveLength(3);
    expect(h.feed.rejections).toBe(1);
    expect(h.feed.lastError).toEqual({ code: 'projectionStale', message: 'keep 2 of 1 strokes' });
  });

  it('does not send a refused page again until another note is shown (no loop)', async () => {
    const h = harness();
    const shown = note([stroke()]);
    h.refuseNext({ code: 'projectionInvalid', message: 'bad note' });
    h.feed.show(shown, null);
    await flush();
    h.feed.view({ zoom: 2, x: 1, y: 1 });
    h.feed.stroke(begin(1, pts(2)));
    await flush();
    expect(kinds(h.sent)).toEqual(['page']);
    h.feed.show(note(shown.strokes), null);
    await flush();
    expect(kinds(h.sent)).toEqual(['page', 'page', 'strokeBegin']);
  });

  it('tries a refused page again after a while, longer at each refusal', async () => {
    vi.useFakeTimers();
    const refusals: number[] = [];
    let refuse = 3;
    const feed = new ProjectionFeed(() => {
      refusals.push(Date.now());
      if (refuse-- > 0) return Promise.reject({ code: 'projectionStale', message: 'the projection is closed' });
      return Promise.resolve();
    });
    const started = Date.now();
    feed.show(note([stroke()]), null);
    await vi.advanceTimersByTimeAsync(0);
    expect(refusals).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(PAGE_RETRY_MS - 1);
    expect(refusals).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(refusals).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(2 * PAGE_RETRY_MS);
    expect(refusals).toHaveLength(3);
    await vi.advanceTimersByTimeAsync(4 * PAGE_RETRY_MS);
    // The fourth is taken; nothing more is sent.
    expect(refusals).toHaveLength(4);
    expect(refusals.map((at) => at - started)).toEqual([0, PAGE_RETRY_MS, 3 * PAGE_RETRY_MS, 7 * PAGE_RETRY_MS]);
    await vi.advanceTimersByTimeAsync(PAGE_RETRY_MAX_MS * 2);
    expect(refusals).toHaveLength(4);
    expect(feed.rejections).toBe(3);
  });

  it('after reset, waits for the next show and sends the whole page', async () => {
    const h = harness();
    const shown = note([stroke()]);
    h.feed.show(shown, null);
    await flush();
    h.feed.stroke(begin(1, pts(2)));
    await flush();
    h.feed.reset();
    await flush();
    expect(h.sent).toHaveLength(2);
    // The stroke drawn before the reset is no longer followed.
    h.feed.stroke({ kind: 'grow', id: 1, points: pts(4) });
    h.feed.show(shown, null);
    await flush();
    expect(kinds(h.sent)).toEqual(['page', 'strokeBegin', 'page']);
  });

  it('sends nothing once disposed', async () => {
    const h = harness();
    h.feed.dispose();
    h.feed.show(note([]), null);
    h.feed.stroke(begin(1, pts(1)));
    await flush();
    expect(h.sent).toHaveLength(0);
  });
});
