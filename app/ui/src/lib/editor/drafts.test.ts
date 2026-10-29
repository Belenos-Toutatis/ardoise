import { describe, expect, it } from 'vitest';
import type { FileStamp, Note } from '../note';
import {
  DRAFT_DELAY_MS,
  DRAFT_MAX_DELAY_MS,
  DraftKeeper,
  findOffer,
  recoveryOf,
  type Draft,
  type DraftSink,
  type DraftSnapshot,
  type DraftTimers,
} from './drafts';

function stamp(secs: number, len = 100): FileStamp {
  return { modified: { secs_since_epoch: secs, nanos_since_epoch: 0 }, len };
}

function note(strokes = 0, modified = 1): Note {
  return {
    id: 'n1',
    title: 'Cours',
    created: 1,
    modified,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: Array.from({ length: strokes }, (_, i) => ({
      tool: 'ballpoint',
      color: { r: 0, g: 0, b: 0, a: 255 },
      width: 60,
      layer: 0,
      points: [{ x: i, y: i, pressure: 0.5, t: i }],
    })),
    timing: 'recorded',
    origin: { kind: 'ardoise' },
  } as Note;
}

function snapshot(strokes: number, noteId = 'n1'): DraftSnapshot {
  return { path: 'C:\\Notes\\Cours.ardoise', noteId, note: note(strokes), baseStamp: stamp(10), imageKey: null };
}

/** Timers driven by hand. */
class Clock implements DraftTimers {
  time = 0;
  #next = 1;
  #pending = new Map<number, { at: number; run: () => void }>();
  now = () => this.time;
  set = (run: () => void, ms: number) => {
    const id = this.#next++;
    this.#pending.set(id, { at: this.time + ms, run });
    return id;
  };
  clear = (handle: unknown) => {
    this.#pending.delete(handle as number);
  };
  /** Moves the time on, running what falls due. */
  advance(ms: number) {
    this.time += ms;
    for (const [id, timer] of [...this.#pending]) {
      if (timer.at <= this.time) {
        this.#pending.delete(id);
        timer.run();
      }
    }
  }
}

/** A sink recording what it is asked, in order. */
function recorder() {
  const log: string[] = [];
  const sink: DraftSink = {
    write: async (s) => {
      log.push(`write ${s.noteId} ${s.note.strokes.length}`);
    },
    discard: async (id) => {
      log.push(`discard ${id}`);
    },
    unsaved: async (unsaved) => {
      log.push(`unsaved ${unsaved}`);
    },
  };
  return { log, sink };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('DraftKeeper', () => {
  it('writes a few seconds after the last change, once', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    keeper.update(snapshot(1));
    clock.advance(DRAFT_DELAY_MS - 1);
    keeper.update(snapshot(2));
    clock.advance(DRAFT_DELAY_MS - 1);
    await settle();
    expect(log).toEqual(['unsaved true']);
    clock.advance(1);
    await settle();
    expect(log).toEqual(['unsaved true', 'write n1 2']);
    clock.advance(DRAFT_MAX_DELAY_MS);
    await settle();
    expect(log).toHaveLength(2);
  });

  it('writes while changes keep coming, at the latest after the longest delay', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    for (let elapsed = 0; elapsed < DRAFT_MAX_DELAY_MS + 1000; elapsed += 500) {
      keeper.update(snapshot(elapsed));
      clock.advance(500);
    }
    await settle();
    expect(log.filter((l) => l.startsWith('write'))).toHaveLength(1);
    expect(log[1]).toBe(`write n1 ${DRAFT_MAX_DELAY_MS - 500}`);
  });

  it('removes its draft once nothing is unsaved, and only its own', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    // Clean from the start: a draft of an earlier session is left for the user to decide.
    keeper.update(null);
    await settle();
    expect(log).toEqual([]);
    keeper.update(snapshot(1));
    clock.advance(DRAFT_DELAY_MS);
    await settle();
    keeper.update(null);
    await settle();
    expect(log).toEqual(['unsaved true', 'write n1 1', 'unsaved false', 'discard n1']);
    // Unsaved again then saved before any write: nothing to remove.
    keeper.update(snapshot(2));
    keeper.update(null);
    clock.advance(DRAFT_MAX_DELAY_MS);
    await settle();
    expect(log.slice(4)).toEqual(['unsaved true', 'unsaved false']);
  });

  it('writes at once when the session ends, and nothing when nothing is pending', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    await keeper.flush();
    expect(log).toEqual([]);
    keeper.update(snapshot(3));
    await keeper.flush();
    expect(log).toEqual(['unsaved true', 'write n1 3']);
    clock.advance(DRAFT_MAX_DELAY_MS);
    await settle();
    expect(log).toHaveLength(2);
  });

  it('holds the writes while a draft of an earlier session waits for the user', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    keeper.hold(true);
    keeper.update(snapshot(1));
    clock.advance(DRAFT_MAX_DELAY_MS);
    await keeper.flush();
    expect(log).toEqual(['unsaved true']);
    keeper.hold(false);
    clock.advance(0);
    await settle();
    expect(log).toEqual(['unsaved true', 'write n1 1']);
  });

  it('discards the draft of the note, whoever wrote it, and the one it wrote', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    keeper.update(snapshot(1, 'old'));
    await keeper.flush();
    keeper.update(snapshot(2, 'n1'));
    await keeper.discard('n1');
    clock.advance(DRAFT_MAX_DELAY_MS);
    await settle();
    expect(log).toEqual(['unsaved true', 'write old 1', 'unsaved false', 'discard n1', 'discard old']);
  });

  it('writes what is pending when the editor goes away, and keeps it', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    keeper.update(snapshot(4));
    await keeper.dispose();
    expect(log).toEqual(['unsaved true', 'write n1 4', 'unsaved false']);
  });

  it('removes an adopted draft of an earlier session once the note is saved', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    keeper.hold(true);
    keeper.update(null);
    keeper.adopt('n1');
    keeper.hold(false);
    keeper.update(snapshot(2));
    // Saved before the next write.
    keeper.update(null);
    await settle();
    expect(log).toEqual(['unsaved true', 'unsaved false', 'discard n1']);
  });

  it('leaves the draft of the note it moves away from', async () => {
    const clock = new Clock();
    const { log, sink } = recorder();
    const keeper = new DraftKeeper(sink, { timers: clock });
    keeper.update(snapshot(4));
    await keeper.detach();
    keeper.update(null);
    await settle();
    expect(log).toEqual(['unsaved true', 'write n1 4', 'unsaved false']);
  });

  it('runs one operation at a time, in order, and reports failures', async () => {
    const clock = new Clock();
    const log: string[] = [];
    const errors: unknown[] = [];
    let release: () => void = () => {};
    const sink: DraftSink = {
      write: (s) =>
        new Promise<void>((resolve, reject) => {
          log.push(`start ${s.note.strokes.length}`);
          if (s.note.strokes.length === 9) return reject(new Error('disk full'));
          release = () => {
            log.push(`end ${s.note.strokes.length}`);
            resolve();
          };
        }),
      discard: async (id) => {
        log.push(`discard ${id}`);
      },
      unsaved: async () => {},
    };
    const keeper = new DraftKeeper(sink, { timers: clock, onerror: (e) => errors.push(e) });
    keeper.update(snapshot(1));
    void keeper.flush();
    await settle();
    // Saved meanwhile: the removal waits for the write under way (else it would come back).
    keeper.update(null);
    await settle();
    expect(log).toEqual(['start 1']);
    release();
    await settle();
    await settle();
    expect(log).toEqual(['start 1', 'end 1', 'discard n1']);
    keeper.update(snapshot(9));
    await keeper.flush();
    expect(errors).toHaveLength(1);
  });
});

describe('recoveryOf', () => {
  const draft = (strokes: number, base: FileStamp | null): Draft => ({
    id: 'n1',
    path: 'C:\\Notes\\Cours.ardoise',
    title: 'Cours',
    savedAt: 1000,
    baseStamp: base,
    hasImage: false,
    note: note(strokes, 999),
  });

  it('restores over the note when the file is the version the edits started from', () => {
    expect(recoveryOf(draft(3, stamp(10)), { note: note(1), stamp: stamp(10) })).toBe('restore');
  });

  it('offers only a copy when the note changed since, or its version is unknown', () => {
    // Another PC saved the note meanwhile (Nextcloud brought it here).
    expect(recoveryOf(draft(3, stamp(10)), { note: note(1), stamp: stamp(11) })).toBe('copy');
    expect(recoveryOf(draft(3, stamp(10)), { note: note(1), stamp: stamp(10, 101) })).toBe('copy');
    expect(recoveryOf(draft(3, null), { note: note(1), stamp: stamp(10) })).toBe('copy');
  });

  it('offers nothing when the draft holds what the file holds', () => {
    expect(recoveryOf(draft(2, stamp(10)), { note: note(2, 5), stamp: stamp(99) })).toBe('none');
  });

  it('removes a draft with nothing more than the file, and offers the others', async () => {
    const removed: string[] = [];
    const discard = async (id: string) => {
      removed.push(id);
    };
    const same = draft(2, stamp(10));
    expect(await findOffer(async () => same, discard, 'n1', { note: note(2), stamp: stamp(10) })).toBeNull();
    expect(removed).toEqual(['n1']);
    const newer = draft(5, stamp(10));
    expect(await findOffer(async () => newer, discard, 'n1', { note: note(2), stamp: stamp(10) })).toEqual({
      draft: newer,
      recovery: 'restore',
    });
    expect(await findOffer(async () => null, discard, 'n1', { note: note(2), stamp: stamp(10) })).toBeNull();
    // Looking fails: nothing is offered, nothing removed.
    expect(
      await findOffer(
        () => Promise.reject(new Error('no library')),
        discard,
        'n1',
        { note: note(2), stamp: stamp(10) },
      ),
    ).toBeNull();
    expect(removed).toEqual(['n1']);
  });
});
