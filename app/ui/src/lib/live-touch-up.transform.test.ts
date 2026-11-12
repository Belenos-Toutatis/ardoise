// SPEC §4.2 « Retoucher pendant le direct »: what the control window asks of the session to move or resize the
// selection (`transformSelected`): the edit sent, one at a time and through the queue, and the selection kept across
// the change of revision whichever of the call's answer and the session's event comes first.
import { describe, expect, it } from 'vitest';
import type { LiveBatch, LiveStrokesEdit } from './live';
import { LiveSelection } from './live-select.svelte';
import { LiveState } from './live-state.svelte';
import { transformSelected, type TouchUp, type TouchUpCommands } from './live-touch-up';
import type { Note, Rgba, Stroke } from './note';

const A4 = { width: 29700, height: 21600 };
const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 255 };
const line = (y: number): Stroke => ({
  tool: 'ballpoint',
  color: BLACK,
  width: 60,
  layer: 0,
  points: Array.from({ length: 5 }, (_, i) => ({ x: 1000 + 100 * i, y, pressure: 0.6, t: i })),
});

/** A window on a page of three strokes (revision 5) whose events the test delivers, and commands that answer when it says so. */
function manual() {
  const live = new LiveState();
  const note: Note = {
    id: 'p1',
    title: '',
    created: 0,
    modified: 0,
    page: A4,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: [line(1000), line(2000), line(3000)],
    timing: 'recorded',
    origin: { kind: 'live' },
  };
  live.initial({
    state: 'live',
    page: note,
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
    revision: 5,
  });
  const sent: { pageIndex: number; revision: number; edit: LiveStrokesEdit }[] = [];
  let answer!: { resolve: (revision: number) => void; reject: (error: unknown) => void };
  const stub: TouchUpCommands = {
    liveEditStrokes: (pageIndex, revision, edit) => {
      sent.push({ pageIndex, revision, edit });
      return new Promise<number>((resolve, reject) => (answer = { resolve, reject }));
    },
    liveUndoEdit: async () => false,
    liveRedoEdit: async () => false,
  };
  const selection = new LiveSelection();
  const errors: unknown[] = [];
  const touch: TouchUp = { live, selection, commands: stub, onerror: (error) => errors.push(error) };
  /** The session's event for a move of strokes 0 and 1 by 500: both replaced at their places, a new revision. */
  const event = (revision: number): LiveBatch => ({
    sentAt: 0,
    events: [
      {
        type: 'strokesEdited',
        pageIndex: 0,
        revision,
        removed: [0, 1],
        inserted: [0, 1].map((index) => ({
          index,
          stroke: { ...live.strokes[index], points: live.strokes[index].points.map((p) => ({ ...p, x: p.x + 500 })) },
        })),
        recoloured: [],
        canUndo: true,
        canRedo: false,
      },
    ],
  });
  return { live, selection, touch, errors, sent, answer: () => answer, event };
}

describe('moving and resizing the selection', () => {
  it("sends the transform with the selection's indices and revision, one edit at a time", async () => {
    const { live, selection, touch, sent, answer } = manual();
    selection.choose(live, [1, 0], 'replace');
    const first = transformSelected(touch, { scale: 0.5, x: 100, y: -20 });
    // A second one while the first is under way is ignored (it would be refused as stale).
    expect(await transformSelected(touch, { scale: 2, x: 0, y: 0 })).toBe(false);
    expect(sent).toEqual([{ pageIndex: 0, revision: 5, edit: { kind: 'transform', indices: [0, 1], scale: 0.5, x: 100, y: -20 } }]);
    answer().resolve(6);
    expect(await first).toBe(true);
  });

  it('does nothing without a selection', async () => {
    const { touch, sent } = manual();
    expect(await transformSelected(touch, { scale: 1, x: 10, y: 0 })).toBe(false);
    expect(sent).toEqual([]);
  });

  it('keeps the selection when the event comes before the answer, and then sends the next edit at the new revision', async () => {
    const { live, selection, touch, sent, answer, event } = manual();
    selection.choose(live, [0, 1], 'replace');
    const done = transformSelected(touch, { scale: 1, x: 500, y: 0 });
    live.receive(event(6));
    expect(live.revision).toBe(6);
    // What the window's effect does at every change of the revision.
    selection.follow(live);
    expect(selection.indices).toEqual([0, 1]);
    answer().resolve(6);
    expect(await done).toBe(true);
    selection.follow(live);
    expect(selection.indices).toEqual([0, 1]);
    expect(selection.revision).toBe(6);

    // Another move: the revision sent is the new one, the strokes are where the event put them.
    const second = transformSelected(touch, { scale: 1, x: 10, y: 10 });
    expect(sent[1].revision).toBe(6);
    answer().resolve(7);
    await second;
  });

  it('keeps the selection when the answer comes before the event: it waits for it', async () => {
    const { live, selection, touch, answer, event } = manual();
    selection.choose(live, [0, 1], 'replace');
    const done = transformSelected(touch, { scale: 1, x: 500, y: 0 });
    answer().resolve(6);
    expect(await done).toBe(true);
    // The session has not heard of the edit yet: the selection is ahead of it, and stays.
    expect(live.revision).toBe(5);
    selection.follow(live);
    expect(selection.indices).toEqual([0, 1]);
    live.receive(event(6));
    selection.follow(live);
    expect(selection.indices).toEqual([0, 1]);
    expect(live.strokes[0].points[0].x).toBe(1500);
  });

  it('a stale refusal drops the selection and tells it; another refusal keeps it to try again', async () => {
    const { live, selection, touch, errors, answer } = manual();
    selection.choose(live, [0], 'replace');
    const invalid = transformSelected(touch, { scale: 1, x: 5, y: 5 });
    answer().reject({ code: 'liveEditInvalid', message: 'no' });
    expect(await invalid).toBe(false);
    expect(selection.indices).toEqual([0]);
    expect(errors).toEqual([expect.objectContaining({ code: 'liveEditInvalid' })]);

    const stale = transformSelected(touch, { scale: 1, x: 5, y: 5 });
    answer().reject({ code: 'liveEditStale', message: 'gone' });
    expect(await stale).toBe(false);
    expect(selection.indices).toEqual([]);
    expect(errors).toHaveLength(2);
  });

  it("goes through the window's queue, after the commands asked for before it", async () => {
    const { live, selection, touch, sent, answer } = manual();
    selection.choose(live, [0], 'replace');
    let open!: () => void;
    const queue = {
      run: <T>(call: () => Promise<T>): Promise<T> => new Promise<void>((resolve) => (open = resolve)).then(call),
    } as unknown as TouchUp['queue'];
    const done = transformSelected({ ...touch, queue }, { scale: 1, x: 5, y: 5 });
    expect(sent).toEqual([]);
    open();
    await Promise.resolve();
    await Promise.resolve();
    expect(sent).toHaveLength(1);
    answer().resolve(6);
    expect(await done).toBe(true);
  });
});
