// @vitest-environment jsdom
/**
 * Undoing a rearrangement of the free order of a binder (Ctrl+Z, « Annuler »; `LibraryState.undoOrder`), and the notice
 * every undo of the library ends with (what was undone, polite to a screen reader, gone by itself): the order is put
 * back through the queue of the writes and only over the order the move wrote, several moves undo one by one, the moves
 * of a held key are one, a binder's steps are dropped with its view or its sort.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { undoNotice } from '../../lib/library-activity.svelte';
import { freshStamp, mockLibrary, notePath, notifyLibraryChanged, resetMockLibrary, fileName, type StoredNote } from '../../lib/mock-data';
import type { Note } from '../../lib/note';
import { ROOT, renderHome, type Ui } from './library-test-helpers';

let ui: Ui | null = null;
const realDuration = undoNotice.durationMs;

beforeEach(() => {
  resetMockLibrary();
  undoNotice.durationMs = realDuration;
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

afterEach(() => {
  ui?.destroy();
  ui = null;
  undoNotice.durationMs = realDuration;
  vi.restoreAllMocks();
});

const DAY = 86_400_000;
const T0 = Date.UTC(2026, 9, 1, 9, 0);

function addNote(binder: string, title: string, created: number): string {
  const library = mockLibrary();
  const note: Note = {
    id: `id-${title}`,
    title,
    created,
    modified: created,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [{ name: 'Calque 1', visible: true }],
    strokes: [],
    timing: 'recorded',
    origin: { kind: 'tablet' },
  };
  if (binder) library.binders.add(binder);
  const path = notePath(ROOT, binder, fileName(note));
  library.notes.set(path, { note, stamp: freshStamp(), binder } satisfies StoredNote);
  return path;
}

/** Four notes in `Classe`: Alpha, Beta, Gamma, Delta, oldest first (and `Cours de physique` in `Classe/3e B`). */
function seedClasse(): void {
  ['Alpha', 'Beta', 'Gamma', 'Delta'].forEach((title, i) => addNote('Classe', title, T0 + i * DAY));
}

const idOf = (title: string) => `id-${title}`;
const orderOf = (binder: string) => mockLibrary().orders.get(binder);
const ORIGINAL = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Cours de physique'];

async function render(locale: 'fr' | 'en' = 'fr'): Promise<Ui> {
  ui = await renderHome(locale, 7);
  await ui.click(ui.q('[data-binder="Classe"]'));
  const sort = ui.q<HTMLSelectElement>('select.sort')!;
  sort.value = 'binder';
  sort.dispatchEvent(new Event('change', { bubbles: true }));
  await ui.settle();
  return ui;
}

const open = (view: Ui, title: string) => view.card(title).querySelector<HTMLElement>('.card-open')!;
/** Alt + arrow on a card; `repeat` is a key held down. */
const move = (view: Ui, title: string, key: string, repeat = false) => view.key(open(view, title), key, { altKey: true, repeat });
const ctrlZ = (view: Ui) => view.key(document.body, 'z', { ctrlKey: true });
/** The text of the notices on display, newest last. */
const notices = (view: Ui) => view.all('.activity-offer').map((el) => el.querySelector('span')?.textContent?.trim());

describe('undoing a rearrangement', () => {
  it('shows a notice with « Annuler » after a move, and Ctrl+Z puts the former order back and says what came back', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    expect(view.titles()).toEqual(['Alpha', 'Gamma', 'Beta', 'Delta', 'Cours de physique']);
    expect(notices(view)).toEqual(['Gamma déplacée en position 2 sur 4.']);
    // The screen reader is told by the grid's own region: the notice is not a second live region.
    expect(view.q('.activity-offer')!.getAttribute('role')).toBeNull();
    expect(view.q('.activity-offer')!.getAttribute('aria-live')).toBeNull();
    expect(view.button('Annuler', view.q('.activity-offer')!).getAttribute('aria-keyshortcuts')).toBe('Control+Z');

    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    expect(notices(view)).toEqual(['Rangement annulé : « Gamma » est revenue à sa place.']);
    // The confirmation is a polite live region, with no button, and goes by itself.
    const done = view.q('.activity-offer')!;
    expect(done.getAttribute('role')).toBe('status');
    expect(done.getAttribute('aria-live')).toBe('polite');
    expect(view.all('.activity-offer button[data-action="undo"]')).toHaveLength(0);
    // Nothing more to undo: Ctrl+Z does nothing.
    await ctrlZ(view);
    expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    expect(view.all('.activity-offer')).toHaveLength(1);
  });

  it('is undone by the « Annuler » button of the notice too', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Delta', 'Home');
    expect(view.titles()).toEqual(['Delta', 'Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    await view.click(view.button('Annuler', view.q('.activity-offer')!));
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(notices(view)).toEqual(['Rangement annulé : « Delta » est revenue à sa place.']);
  });

  it('undoes several moves one by one, each saying what came back, even after their notices went', async () => {
    seedClasse();
    undoNotice.durationMs = 30;
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    await move(view, 'Delta', 'Home');
    await move(view, 'Alpha', 'End');
    expect(view.titles()).toEqual(['Delta', 'Gamma', 'Beta', 'Alpha', 'Cours de physique']);
    // The notices are gone (the steps are kept beyond them).
    await vi.waitFor(() => expect(view.all('.activity-offer')).toHaveLength(0));
    undoNotice.durationMs = realDuration;

    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(['Delta', 'Alpha', 'Gamma', 'Beta', 'Cours de physique']));
    expect(notices(view)).toEqual(['Rangement annulé : « Alpha » est revenue à sa place.']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(['Alpha', 'Gamma', 'Beta', 'Delta', 'Cours de physique']));
    expect(orderOf('Classe')).toEqual(['Alpha', 'Gamma', 'Beta', 'Delta'].map(idOf));
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    expect(view.all('.activity-failures')).toHaveLength(0);
  });

  it('counts the moves of a held key as one', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Delta', 'ArrowLeft');
    await move(view, 'Delta', 'ArrowLeft', true);
    await move(view, 'Delta', 'ArrowLeft', true);
    expect(view.titles()).toEqual(['Delta', 'Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    // One notice, the last position.
    expect(notices(view)).toEqual(['Delta déplacée en position 1 sur 4.']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(notices(view)).toEqual(['Rangement annulé : « Delta » est revenue à sa place.']);
    // Pressed again, not held: two steps.
    await move(view, 'Delta', 'ArrowLeft');
    await move(view, 'Delta', 'ArrowLeft');
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(['Alpha', 'Beta', 'Delta', 'Gamma', 'Cours de physique']));
  });

  it('puts several cards back together and counts them', async () => {
    seedClasse();
    const view = await render();
    await view.select('Alpha', 'Beta');
    await move(view, 'Beta', 'End');
    expect(view.titles()).toEqual(['Gamma', 'Delta', 'Alpha', 'Beta', 'Cours de physique']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(notices(view)).toEqual(['Rangement annulé : 2 notes sont revenues à leur place.']);
  });

  it('undoes a drag between the cards', async () => {
    seedClasse();
    const view = await render();
    const drag = (type: string, clientX = 0) => {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX });
      const store = new Map<string, string>();
      Object.defineProperty(event, 'dataTransfer', {
        value: { effectAllowed: '', dropEffect: '', setData: (k: string, v: string) => store.set(k, v), getData: (k: string) => store.get(k) ?? '' },
      });
      return event;
    };
    view.card('Delta').querySelector('.card-open')!.dispatchEvent(drag('dragstart'));
    await view.settle();
    const alpha = view.card('Alpha');
    Object.defineProperty(alpha, 'getBoundingClientRect', {
      configurable: true,
      value: () => ({ left: 0, right: 100, top: 0, bottom: 100, width: 100, height: 100, x: 0, y: 0, toJSON: () => ({}) }),
    });
    alpha.dispatchEvent(drag('dragover', 10));
    await view.settle();
    alpha.dispatchEvent(drag('drop', 10));
    await view.settle();
    expect(view.titles()).toEqual(['Delta', 'Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(notices(view)).toEqual(['Rangement annulé : « Delta » est revenue à sa place.']);
  });

  it('goes through the queue of the writes: an undo asked while a move is being written waits for it', async () => {
    seedClasse();
    const view = await render();
    const real = api.setBinderOrder;
    const written: string[][] = [];
    let first = true;
    vi.spyOn(api, 'setBinderOrder').mockImplementation(async (binder, ids, expected) => {
      if (first) {
        first = false;
        await new Promise((resolve) => setTimeout(resolve, 60));
      }
      written.push(ids);
      return real(binder, ids, expected);
    });
    await move(view, 'Gamma', 'ArrowLeft');
    await ctrlZ(view);
    await vi.waitFor(() => expect(written).toHaveLength(2));
    await view.settle();
    expect(written[1]).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    expect(view.titles()).toEqual(ORIGINAL);
  });

  it('writes the former order only over the order the move wrote', async () => {
    seedClasse();
    const view = await render();
    const write = vi.spyOn(api, 'setBinderOrder');
    await move(view, 'Gamma', 'ArrowLeft');
    await ctrlZ(view);
    await vi.waitFor(() => expect(write).toHaveBeenCalledTimes(2));
    expect(write.mock.calls[0]).toEqual(['Classe', ['Alpha', 'Gamma', 'Beta', 'Delta'].map(idOf)]);
    expect(write.mock.calls[1]).toEqual(['Classe', ['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf), ['Alpha', 'Gamma', 'Beta', 'Delta'].map(idOf)]);
  });
});

describe('an undo that must be refused', () => {
  it('leaves an order written meanwhile alone, says so, and forgets the older steps of the binder', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    await move(view, 'Delta', 'Home');
    // Another computer (through the sync) arranged the binder differently since.
    const theirs = ['Beta', 'Delta', 'Alpha', 'Gamma'].map(idOf);
    mockLibrary().orders.set('Classe', theirs);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    expect(view.q('.activity-failures')!.getAttribute('role')).toBe('alert');
    expect(view.q('.activity-failures')!.textContent).toContain(
      "Rangement non annulé : l'ordre du classeur a changé depuis (une note copiée, un autre ordinateur, une synchronisation). Le nouvel ordre est conservé.",
    );
    expect(orderOf('Classe')).toEqual(theirs);
    // The grid shows the order that was kept, and the older step is gone: the next Ctrl+Z does nothing.
    await vi.waitFor(() => expect(view.titles()).toEqual(['Beta', 'Delta', 'Alpha', 'Gamma', 'Cours de physique']));
    const write = vi.spyOn(api, 'setBinderOrder');
    write.mockClear();
    await ctrlZ(view);
    expect(write).not.toHaveBeenCalled();
    expect(orderOf('Classe')).toEqual(theirs);
  });

  it('refuses before writing when the order the library showed has changed (a sync brought another one)', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    const theirs = ['Delta', 'Beta', 'Gamma', 'Alpha'].map(idOf);
    mockLibrary().orders.set('Classe', theirs);
    notifyLibraryChanged();
    await view.settle();
    expect(view.titles()).toEqual(['Delta', 'Beta', 'Gamma', 'Alpha', 'Cours de physique']);
    const write = vi.spyOn(api, 'setBinderOrder');
    write.mockClear();
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    expect(write).not.toHaveBeenCalled();
    expect(orderOf('Classe')).toEqual(theirs);
  });

  it('says why when the library cannot store it (an order of a newer version)', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    vi.spyOn(api, 'setBinderOrder').mockRejectedValueOnce({ code: 'orderNewer', message: 'newer' });
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    expect(view.q('.activity-failures')!.textContent).toContain("Le rangement n'a pas pu être annulé :");
    expect(view.q('.activity-failures')!.textContent).toContain('version plus récente');
  });

  it('keeps no step for a move the library could not write', async () => {
    seedClasse();
    const view = await render();
    vi.spyOn(api, 'setBinderOrder').mockRejectedValueOnce({ code: 'orderNewer', message: 'newer' });
    await move(view, 'Gamma', 'ArrowLeft');
    await vi.waitFor(() => expect(view.q('.banner.error')).not.toBeNull());
    expect(view.all('.activity-offer')).toHaveLength(0);
    const write = vi.spyOn(api, 'setBinderOrder');
    write.mockClear();
    await ctrlZ(view);
    expect(write).not.toHaveBeenCalled();
  });
});

describe('what ends the steps of the undo', () => {
  it('leaving the binder drops them, and their notice with them', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    expect(view.all('.activity-offer')).toHaveLength(1);
    await view.click(view.q('[data-view="all"]'));
    expect(view.all('.activity-offer')).toHaveLength(0);
    const write = vi.spyOn(api, 'setBinderOrder');
    await ctrlZ(view);
    expect(write).not.toHaveBeenCalled();
    expect(orderOf('Classe')).toEqual(['Alpha', 'Gamma', 'Beta', 'Delta'].map(idOf));
  });

  it('changing the sort drops them, showing the binder again does not', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    // The same binder shown again: nothing changes.
    await view.click(view.q('[data-binder="Classe"]'));
    expect(view.all('.activity-offer')).toHaveLength(1);
    const sort = view.q<HTMLSelectElement>('select.sort')!;
    sort.value = 'title-asc';
    sort.dispatchEvent(new Event('change', { bubbles: true }));
    await view.settle();
    expect(view.all('.activity-offer')).toHaveLength(0);
    const write = vi.spyOn(api, 'setBinderOrder');
    await ctrlZ(view);
    expect(write).not.toHaveBeenCalled();
  });

  it('is in English too', async () => {
    seedClasse();
    const view = await render('en');
    await move(view, 'Gamma', 'ArrowLeft');
    expect(notices(view)).toEqual(['Gamma moved to position 2 of 4.']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(notices(view)).toEqual(['Rearrangement undone: "Gamma" is back where it was.']);
    await view.select('Alpha', 'Beta');
    await move(view, 'Beta', 'End');
    await ctrlZ(view);
    await vi.waitFor(() => expect(notices(view)).toContain('Rearrangement undone: 2 notes are back where they were.'));
    // Refused.
    await move(view, 'Gamma', 'ArrowLeft');
    mockLibrary().orders.set('Classe', ['Delta', 'Gamma', 'Beta', 'Alpha'].map(idOf));
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.q('.activity-failures')?.textContent).toContain("Rearrangement not undone: the binder's order has changed since"));
  });
});

describe('the order of the undos', () => {
  it('undoes the newest thing first: a copy, then the rearrangement before it', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    // Duplicate Delta from the selection bar.
    await view.select('Delta');
    await view.click(view.button('Dupliquer', view.q('.selection-bar')!));
    await vi.waitFor(() => expect(view.titles()).toContain('Delta (copie)'));
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).not.toContain('Delta (copie)'));
    expect(notices(view).at(-1)).toBe('Copie annulée : « Delta (copie) » est à la corbeille.');
    // The copy is in the trash: its place in the order does not stand in the way of the older undo.
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(ORIGINAL));
    expect(notices(view).at(-1)).toBe('Rangement annulé : « Gamma » est revenue à sa place.');
  });
});

describe('an order that lists a note the step did not write', () => {
  it('is not put back over: a note another computer put in the order is still there', async () => {
    seedClasse();
    const view = await render();
    await move(view, 'Gamma', 'ArrowLeft');
    // Another computer put a new note in the binder and in its order.
    addNote('Classe', 'Epsilon', T0 + 9 * DAY);
    mockLibrary().orders.set('Classe', ['Alpha', 'Gamma', 'Beta', 'Delta', 'Epsilon'].map(idOf));
    notifyLibraryChanged();
    await vi.waitFor(() => expect(view.titles()).toContain('Epsilon'));
    const write = vi.spyOn(api, 'setBinderOrder');
    write.mockClear();
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    expect(write).not.toHaveBeenCalled();
    expect(orderOf('Classe')).toEqual(['Alpha', 'Gamma', 'Beta', 'Delta', 'Epsilon'].map(idOf));
  });
});

describe('every undo of the library says what it undid', () => {
  it('names the note and the action, in French and English, as a polite live region', async () => {
    ui = await renderHome('fr', 3);
    const view = ui;
    // The trash.
    await view.select('Liste de courses');
    await view.click(view.button('Mettre à la corbeille', view.q('.selection-bar')!));
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toContain('Liste de courses'));
    const last = () => view.all('.activity-offer').at(-1)!;
    expect(last().textContent).toContain('Mise à la corbeille annulée : « Liste de courses » est de retour dans son classeur.');
    expect(last().getAttribute('role')).toBe('status');
    expect(last().getAttribute('aria-live')).toBe('polite');
    // A copy.
    await view.select('Liste de courses');
    await view.click(view.button('Dupliquer', view.q('.selection-bar')!));
    await vi.waitFor(() => expect(view.titles()).toContain('Liste de courses (copie)'));
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).not.toContain('Liste de courses (copie)'));
    expect(last().textContent).toContain('Copie annulée : « Liste de courses (copie) » est à la corbeille.');
    expect(last().getAttribute('role')).toBe('status');
  });
});
