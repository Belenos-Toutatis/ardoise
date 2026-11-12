// SPEC §4.2 « Retoucher pendant le direct »: the selection survives its own move or resize. The strokes stay at the same
// places of the list, but the page's revision moves on, and the answer of the call and the event of the session may come
// in either order: the selection is held while the call is under way, takes the revision the call returns, and one that
// is ahead of the session on the same page and note waits for the event.
import { describe, expect, it } from 'vitest';
import { LiveSelection, type SelectionSource } from './live-select.svelte';
import type { Layer } from './note';

const LAYERS: Layer[] = [{ name: 'Calque 1', visible: true }];

const source = (extra: Partial<SelectionSource> = {}): SelectionSource => ({
  strokes: [],
  layers: LAYERS,
  noteId: 'n1',
  pageIndex: 0,
  revision: 7,
  ...extra,
});

describe('the selection and its own edit', () => {
  it('is held while the call is under way: the revision may move first, the page may not', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1, 2], 'replace');
    selection.hold();
    // The event came before the answer: the session is already at 8.
    selection.follow(source({ revision: 8 }));
    expect(selection.indices).toEqual([1, 2]);
    selection.follow(source({ pageIndex: 1 }));
    expect(selection.indices).toEqual([]);

    selection.choose(source(), [1, 2], 'replace');
    selection.hold();
    selection.follow(source({ noteId: 'n2', revision: 8 }));
    expect(selection.indices).toEqual([]);
  });

  it("takes the revision the call returns, so that it is the session's once the event is applied", () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1, 2], 'replace');
    selection.hold();
    selection.release(8);
    expect(selection.revision).toBe(8);
    selection.follow(source({ revision: 8 }));
    expect(selection.indices).toEqual([1, 2]);
  });

  it('ahead of the session (the answer came before the event) it waits for the event; once passed, it is dropped', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1, 2], 'replace');
    selection.hold();
    selection.release(8);
    selection.follow(source({ revision: 7 }));
    expect(selection.indices).toEqual([1, 2]);
    // The event arrives: the same revision. Then another edit moves the page on: the indices are no longer right.
    selection.follow(source({ revision: 8 }));
    expect(selection.indices).toEqual([1, 2]);
    selection.follow(source({ revision: 9 }));
    expect(selection.indices).toEqual([]);
  });

  it('being ahead is no reason to wait on another page or note', () => {
    const selection = new LiveSelection();
    selection.choose(source({ revision: 12 }), [3], 'replace');
    selection.follow(source({ revision: 2, pageIndex: 1 }));
    expect(selection.indices).toEqual([]);
    selection.choose(source({ revision: 12 }), [3], 'replace');
    selection.follow(source({ revision: 2, noteId: 'n2' }));
    expect(selection.indices).toEqual([]);
  });

  it('a call that failed, or that changed nothing, leaves the revision as it was', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1], 'replace');
    selection.hold();
    selection.release();
    expect(selection.revision).toBe(7);
    selection.hold();
    selection.release(7);
    expect(selection.revision).toBe(7);
    // Not held any more: another revision drops the selection again.
    selection.follow(source({ revision: 8 }));
    expect(selection.indices).toEqual([]);
  });

  it('two holds at once need two releases', () => {
    const selection = new LiveSelection();
    selection.choose(source(), [1], 'replace');
    selection.hold();
    selection.hold();
    selection.release();
    selection.follow(source({ revision: 9 }));
    expect(selection.indices).toEqual([1]);
    selection.release();
    selection.follow(source({ revision: 9 }));
    expect(selection.indices).toEqual([]);
  });
});
