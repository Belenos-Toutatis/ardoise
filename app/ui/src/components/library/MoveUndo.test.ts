// @vitest-environment jsdom
/**
 * Undoing a move of notes to another binder (Ctrl+Z, « Annuler »; `LibraryState.undoMove`) and the two drops beside it
 * (« Favoris », a tag): every way of moving has its notice and its step, each note goes back to the binder it came from
 * and to its place in that binder's free order, an undo is refused for a note that changed or went, and the step
 * outlives the notice and the view (the card left the binder being shown).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { undoNotice } from '../../lib/library-activity.svelte';
import { fileName, freshStamp, mockLibrary, notePath, notifyLibraryChanged, resetMockLibrary, type StoredNote } from '../../lib/mock-data';
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

/** Four notes in `Classe`: Alpha, Beta, Gamma, Delta, oldest first. */
function seedClasse(): void {
  ['Alpha', 'Beta', 'Gamma', 'Delta'].forEach((title, i) => addNote('Classe', title, T0 + i * DAY));
}

const idOf = (title: string) => `id-${title}`;
const orderOf = (binder: string) => mockLibrary().orders.get(binder);
const stored = (title: string) => [...mockLibrary().notes].find(([, s]) => s.note.title === title)!;
const binderOf = (title: string) => stored(title)[1].binder;
const pathOf = (title: string) => stored(title)[0];

class DataTransferStub {
  data = new Map<string, string>();
  effectAllowed = 'none';
  dropEffect = 'none';
  setData(type: string, value: string) {
    this.data.set(type, value);
  }
}

function drag(type: string): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: new DataTransferStub() });
  return event;
}

async function render(locale: 'fr' | 'en' = 'fr', expected = 3): Promise<Ui> {
  ui = await renderHome(locale, expected);
  return ui;
}

const open = (view: Ui, title: string) => view.card(title).querySelector<HTMLElement>('.card-open')!;

/** Drags the card of `title` (the whole selection when it is part of it) onto the sidebar entry `selector`. */
async function dropOn(view: Ui, title: string, selector: string): Promise<void> {
  open(view, title).dispatchEvent(drag('dragstart'));
  await view.settle();
  view.q(selector)!.dispatchEvent(drag('drop'));
  await view.settle();
}

const ctrlZ = (view: Ui) => view.key(document.body, 'z', { ctrlKey: true });
/** The text of the notices on display, newest last. */
const notices = (view: Ui) => view.all('.activity-offer').map((el) => el.querySelector('span')?.textContent?.trim());

describe('moving notes to a binder, then undoing', () => {
  it('says where a dropped card went, and Ctrl+Z puts it back in its binder and says so', async () => {
    const view = await render();
    const original = pathOf('Liste de courses');
    await dropOn(view, 'Liste de courses', '[data-binder="Perso"]');
    expect(binderOf('Liste de courses')).toBe('Perso');
    expect(notices(view)).toEqual(['« Liste de courses » déplacée dans « Perso ».']);
    // A polite live region with « Annuler ».
    const offer = view.q('.activity-offer')!;
    expect(offer.getAttribute('role')).toBe('status');
    expect(offer.getAttribute('aria-live')).toBe('polite');
    expect(view.button('Annuler', offer).getAttribute('aria-keyshortcuts')).toBe('Control+Z');

    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Liste de courses')).toBe(''));
    expect(pathOf('Liste de courses')).toBe(original);
    await vi.waitFor(() =>
      expect(notices(view)).toContain('Déplacement annulé : « Liste de courses » est revenue dans « Bibliothèque (racine) ».'),
    );
    // The confirmation has no button, and nothing more can be undone.
    const done = view.all('.activity-offer').at(-1)!;
    expect(done.getAttribute('role')).toBe('status');
    expect(done.getAttribute('aria-live')).toBe('polite');
    expect(done.querySelector('button[data-action="undo"]')).toBeNull();
    expect(view.card('Liste de courses').querySelector('.card-binder')!.classList.contains('unsorted')).toBe(true);
  });

  it('is undone by the « Annuler » button, and « Toutes les notes » is the root', async () => {
    const view = await render();
    await dropOn(view, 'Réunion parents', '[data-view="all"]');
    expect(binderOf('Réunion parents')).toBe('');
    expect(notices(view)).toEqual(['« Réunion parents » déplacée dans « Bibliothèque (racine) ».']);
    await view.click(view.button('Annuler', view.q('.activity-offer')!));
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    expect(notices(view).at(-1)).toBe('Déplacement annulé : « Réunion parents » est revenue dans « Perso ».');
  });

  it('puts every note of a selection from several binders back in its own binder', async () => {
    const view = await render();
    const before = Object.fromEntries(['Réunion parents', 'Liste de courses', 'Cours de physique'].map((t) => [t, pathOf(t)]));
    await view.select('Réunion parents', 'Liste de courses', 'Cours de physique');
    await dropOn(view, 'Liste de courses', '[data-binder="Classe"]');
    expect(['Réunion parents', 'Liste de courses', 'Cours de physique'].map(binderOf)).toEqual(['Classe', 'Classe', 'Classe']);
    expect(notices(view)).toEqual(['3 notes déplacées dans « Classe ».']);

    await ctrlZ(view);
    await vi.waitFor(() => expect(['Réunion parents', 'Liste de courses', 'Cours de physique'].map(binderOf)).toEqual(['Perso', '', 'Classe/3e B']));
    for (const [title, path] of Object.entries(before)) expect(pathOf(title)).toBe(path);
    expect(notices(view).at(-1)).toBe("Déplacement annulé : 3 notes sont revenues dans leur classeur d'origine.");
  });

  it('puts several notes of one binder back, naming it', async () => {
    const view = await render();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Réunion parents', 'Déplacer vers…');
    await view.click(view.q('[data-dialog="move"] [data-binder="Classe"]')!);
    expect(notices(view)).toEqual(['2 notes déplacées dans « Classe ».']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    expect(binderOf('Liste de courses')).toBe('');
  });

  it('undoes « Déplacer vers… » of the card menu, root included', async () => {
    const view = await render();
    await view.select('Réunion parents');
    await view.choose('Réunion parents', 'Déplacer vers…');
    await view.click(view.button('Bibliothèque (racine)', view.q('[data-dialog="move"]')!));
    expect(notices(view)).toEqual(['« Réunion parents » déplacée dans « Bibliothèque (racine) ».']);
    expect(binderOf('Réunion parents')).toBe('');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
  });

  it('says nothing and keeps no step for a note moved where it already is', async () => {
    const view = await render();
    await view.select('Réunion parents');
    await view.choose('Réunion parents', 'Déplacer vers…');
    await view.click(view.q('[data-dialog="move"] [data-binder="Perso"]')!);
    expect(view.all('.activity-offer')).toHaveLength(0);
    const move = vi.spyOn(api, 'moveNotesBack');
    await ctrlZ(view);
    expect(move).not.toHaveBeenCalled();
  });

  it('speaks English', async () => {
    const view = await render('en');
    await dropOn(view, 'Liste de courses', '[data-binder="Perso"]');
    expect(notices(view)).toEqual(['"Liste de courses" moved to "Perso".']);
    expect(view.button('Undo', view.q('.activity-offer')!)).toBeDefined();
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Liste de courses')).toBe(''));
    expect(notices(view).at(-1)).toBe('Move undone: "Liste de courses" is back in "Library (root)".');
  });

  it('lists the notes that could not move, and still undoes the others', async () => {
    const view = await render();
    await view.select('Réunion parents', 'Liste de courses');
    open(view, 'Liste de courses').dispatchEvent(drag('dragstart'));
    await view.settle();
    // The note went meanwhile (another computer).
    mockLibrary().notes.delete(pathOf('Réunion parents'));
    view.q('[data-binder="Classe"]')!.dispatchEvent(drag('drop'));
    await view.settle();
    expect(binderOf('Liste de courses')).toBe('Classe');
    expect(view.q('.activity-failures')!.textContent).toContain("Une note n'a pas pu être déplacée :");
    expect(view.q('.activity-failures')!.textContent).toContain('« Réunion parents » : la note n\'existe plus');
    expect(notices(view)).toEqual(['« Liste de courses » déplacée dans « Classe ».']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Liste de courses')).toBe(''));
  });
});

describe('the step outlives the notice and the view', () => {
  it('is still there when the card left the binder shown, the notice is gone and another view is open', async () => {
    const view = await render();
    undoNotice.durationMs = 30;
    await view.click(view.q('[data-binder="Perso"]'));
    expect(view.titles()).toEqual(['Réunion parents']);
    await dropOn(view, 'Réunion parents', '[data-binder="Classe"]');
    // The card is gone from the view, the notice offers « Annuler » all the same.
    expect(view.titles()).toEqual([]);
    expect(notices(view)).toEqual(['« Réunion parents » déplacée dans « Classe ».']);
    await vi.waitFor(() => expect(view.all('.activity-offer')).toHaveLength(0));
    undoNotice.durationMs = realDuration;
    // The user goes to look at the target, and only then sees the mistake.
    await view.click(view.q('[data-binder="Classe"]'));
    expect(view.titles()).toEqual(['Réunion parents', 'Cours de physique']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    await vi.waitFor(() => expect(view.titles()).toEqual(['Cours de physique']));
  });

  it('undoes the newest thing first, whatever its kind', async () => {
    const view = await render();
    undoNotice.durationMs = 30;
    await dropOn(view, 'Liste de courses', '[data-binder="Perso"]');
    await vi.waitFor(() => expect(view.all('.activity-offer')).toHaveLength(0));
    // A favourite dropped later is undone first, then the move.
    await dropOn(view, 'Réunion parents', '[data-view="favorites"]');
    undoNotice.durationMs = realDuration;
    expect(stored('Réunion parents')[1].note.favorite).toBe(true);
    await ctrlZ(view);
    await vi.waitFor(() => expect(stored('Réunion parents')[1].note.favorite).toBe(false));
    expect(binderOf('Liste de courses')).toBe('Perso');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Liste de courses')).toBe(''));
  });

  it('undoes a trashing made after a move first, then the move', async () => {
    const view = await render();
    await dropOn(view, 'Liste de courses', '[data-binder="Perso"]');
    await view.choose('Réunion parents', 'Mettre à la corbeille');
    expect(mockLibrary().trash.size).toBe(1);
    await ctrlZ(view);
    await vi.waitFor(() => expect(mockLibrary().trash.size).toBe(0));
    expect(binderOf('Liste de courses')).toBe('Perso');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Liste de courses')).toBe(''));
  });

  it('keeps several moves and undoes them one by one', async () => {
    const view = await render();
    await dropOn(view, 'Liste de courses', '[data-binder="Perso"]');
    await dropOn(view, 'Réunion parents', '[data-binder="Classe"]');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    expect(binderOf('Liste de courses')).toBe('Perso');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Liste de courses')).toBe(''));
  });
});

describe('the place in the free order', () => {
  /** The view of `Classe` in the binder's own order. */
  async function renderClasse(): Promise<Ui> {
    seedClasse();
    mockLibrary().orders.set('Classe', ['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    const view = await render('fr', 7);
    await view.click(view.q('[data-binder="Classe"]'));
    const sort = view.q<HTMLSelectElement>('select.sort')!;
    sort.value = 'binder';
    sort.dispatchEvent(new Event('change', { bubbles: true }));
    await view.settle();
    return view;
  }

  it('gives the note back its place: the order file still lists it, and nothing is written', async () => {
    const view = await renderClasse();
    const write = vi.spyOn(api, 'setBinderOrder');
    await dropOn(view, 'Beta', '[data-binder="Perso"]');
    expect(view.titles()).toEqual(['Alpha', 'Gamma', 'Delta', 'Cours de physique']);
    // The note left the order of its binder in view only: the file keeps its id.
    expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta', 'Cours de physique']));
    expect(write).not.toHaveBeenCalled();
    expect(notices(view).at(-1)).toBe('Déplacement annulé : « Beta » est revenue dans « Classe ».');
  });

  it('puts the note back after its neighbour when the file was rewritten without it and the others kept their order', async () => {
    const view = await renderClasse();
    await dropOn(view, 'Beta', '[data-binder="Perso"]');
    // Something rewrote the order without the note that was away (a copy made meanwhile, say).
    mockLibrary().orders.set('Classe', ['Alpha', 'Gamma', 'Delta'].map(idOf));
    notifyLibraryChanged();
    await view.settle();
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta', 'Cours de physique']));
    expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf));
    expect(notices(view).at(-1)).toBe('Déplacement annulé : « Beta » est revenue dans « Classe ».');
  });

  it('puts the note at the end, and says so, when the binder was rearranged meanwhile', async () => {
    const view = await renderClasse();
    await dropOn(view, 'Beta', '[data-binder="Perso"]');
    mockLibrary().orders.set('Classe', ['Delta', 'Alpha', 'Gamma'].map(idOf));
    notifyLibraryChanged();
    await view.settle();
    await ctrlZ(view);
    await vi.waitFor(() => expect(orderOf('Classe')).toEqual(['Delta', 'Alpha', 'Gamma', 'Beta'].map(idOf)));
    await vi.waitFor(() => expect(view.titles()).toEqual(['Delta', 'Alpha', 'Gamma', 'Beta', 'Cours de physique']));
    expect(notices(view).at(-1)).toBe(
      "Déplacement annulé : « Beta » est revenue dans « Classe ». L'ordre du classeur a changé depuis : elle a été placée à la fin.",
    );
  });

  it('never overwrites an order that changes again while the undo runs, and loses no note', async () => {
    const view = await renderClasse();
    await dropOn(view, 'Beta', '[data-binder="Perso"]');
    mockLibrary().orders.set('Classe', ['Alpha', 'Gamma', 'Delta'].map(idOf));
    notifyLibraryChanged();
    await view.settle();
    const real = api.setBinderOrder;
    vi.spyOn(api, 'setBinderOrder').mockImplementation(async (binder, ids, expected) => {
      // Another computer writes in the meantime: the checked write is refused.
      mockLibrary().orders.set('Classe', ['Delta', 'Gamma', 'Alpha'].map(idOf));
      return real(binder, ids, expected);
    });
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Beta')).toBe('Classe'));
    await view.settle();
    expect(orderOf('Classe')).toEqual(['Delta', 'Gamma', 'Alpha'].map(idOf));
    expect(view.titles()).toContain('Beta');
    expect(view.q('.activity-failures')).toBeNull();
  });

  it("takes the note out of the target binder's order", async () => {
    const view = await renderClasse();
    mockLibrary().orders.set('Perso', ['autre', idOf('Beta')]);
    await dropOn(view, 'Beta', '[data-binder="Perso"]');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Beta')).toBe('Classe'));
    await vi.waitFor(() => expect(orderOf('Perso')).toEqual(['autre']));
  });

  it('leaves alone a binder that has no free order', async () => {
    const view = await render();
    const write = vi.spyOn(api, 'setBinderOrder');
    await dropOn(view, 'Réunion parents', '[data-binder="Classe"]');
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    expect(write).not.toHaveBeenCalled();
    expect(mockLibrary().orders.size).toBe(0);
  });

  it('goes through the queue of the order writes: an undo asked while a rearrangement is being written waits for it', async () => {
    const view = await renderClasse();
    await dropOn(view, 'Beta', '[data-binder="Perso"]');
    const real = api.setBinderOrder;
    const realBack = api.moveNotesBack;
    // What happened, in the order it happened: the writes are slow, the files move when the writes before are over.
    const events: string[] = [];
    vi.spyOn(api, 'setBinderOrder').mockImplementation(async (binder, ids, expected) => {
      events.push(`write start`);
      await new Promise((resolve) => setTimeout(resolve, 40));
      events.push(`write end`);
      return real(binder, ids, expected);
    });
    vi.spyOn(api, 'moveNotesBack').mockImplementation((items) => {
      events.push('files back');
      return realBack(items);
    });
    // Alpha moves right in the binder (that rewrites the order without Beta, which is away); then the rearrangement
    // is undone, then the move.
    await view.key(open(view, 'Alpha'), 'ArrowRight', { altKey: true });
    await ctrlZ(view);
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Beta')).toBe('Classe'));
    // Beta comes back after Alpha, its neighbour: the others had been put back in their order first.
    await vi.waitFor(() => expect(orderOf('Classe')).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta'].map(idOf)));
    expect(events).toEqual(['write start', 'write end', 'write start', 'write end', 'files back', 'write start', 'write end']);
  });
});

describe('an undo that must be refused or adapted', () => {
  async function moveTwo(view: Ui): Promise<void> {
    await view.select('Réunion parents', 'Liste de courses');
    await dropOn(view, 'Liste de courses', '[data-binder="Classe"]');
  }

  it('leaves a note that changed meanwhile where it is, puts the others back, and says how many', async () => {
    const view = await render();
    await moveTwo(view);
    const note = stored('Liste de courses');
    await api.setNoteTags(note[0], ['modifiée'], note[1].stamp);
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    expect(binderOf('Liste de courses')).toBe('Classe');
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    const failures = view.q('.activity-failures')!;
    expect(failures.getAttribute('role')).toBe('alert');
    expect(failures.textContent).toContain('Une note n\'a pas pu être remise dans son classeur :');
    expect(failures.textContent).toContain('« Liste de courses » : elle a été modifiée depuis ; elle reste où elle est.');
    expect(notices(view).at(-1)).toBe('Déplacement annulé : « Réunion parents » est revenue dans « Perso ».');
    // Nothing of the changed note was lost.
    expect(stored('Liste de courses')[1].note.tags).toEqual(['modifiée']);
  });

  it('refuses a note moved again, trashed or deleted since, and changes nothing when none can go back', async () => {
    const view = await render();
    await moveTwo(view);
    await api.moveNotes([pathOf('Liste de courses')], 'Perso');
    await api.trashNotes([pathOf('Réunion parents')]);
    await ctrlZ(view);
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    const failures = view.q('.activity-failures')!.textContent ?? '';
    expect(failures).toContain('2 notes n\'ont pas pu être remises dans leur classeur :');
    expect(failures).toContain('« Liste de courses » : elle n\'est plus à cet endroit (déplacée de nouveau, renommée, mise à la corbeille ou supprimée).');
    expect(failures).toContain('« Réunion parents » : elle n\'est plus à cet endroit');
    expect(binderOf('Liste de courses')).toBe('Perso');
    expect(mockLibrary().trash.size).toBe(1);
    // No « Déplacement annulé » when nothing went back.
    expect(notices(view).filter((text) => text?.startsWith('Déplacement annulé'))).toEqual([]);
  });

  it('gives a numbered name to a note whose name was taken in its binder, and says so', async () => {
    const view = await render();
    const { created } = stored('Liste de courses')[1].note;
    const original = pathOf('Liste de courses');
    await dropOn(view, 'Liste de courses', '[data-binder="Perso"]');
    // Another note took the name at the root meanwhile.
    expect(addNote('', 'Liste de courses', created)).toBe(original);
    notifyLibraryChanged();
    await view.settle();
    await ctrlZ(view);
    await vi.waitFor(() => expect(notices(view).at(-1)).toContain('est revenue dans « Bibliothèque (racine) »'));
    expect(notices(view).at(-1)).toMatch(/Son nom de fichier étant déjà pris dans ce classeur, elle s'appelle maintenant « .*Liste de courses \(2\)\.ardoise ».$/);
    const roots = [...mockLibrary().notes].filter(([, s]) => s.binder === '');
    expect(roots).toHaveLength(2);
    // The other note was not touched.
    expect(mockLibrary().notes.get(original)!.note.id).toBe('id-Liste de courses');
  });

  it('makes the binder again when it was deleted meanwhile, and says so', async () => {
    const view = await render();
    await dropOn(view, 'Réunion parents', '[data-binder="Classe"]');
    mockLibrary().binders.delete('Perso');
    notifyLibraryChanged();
    await view.settle();
    expect(view.q('[data-binder="Perso"]')).toBeNull();
    await ctrlZ(view);
    await vi.waitFor(() => expect(binderOf('Réunion parents')).toBe('Perso'));
    expect(notices(view).at(-1)).toBe(
      "Déplacement annulé : « Réunion parents » est revenue dans « Perso ». Le classeur « Perso » n'existait plus : il a été recréé.",
    );
    expect(mockLibrary().binders.has('Perso')).toBe(true);
  });
});

describe('the drops on « Favoris » and on a tag', () => {
  it('undoes a favourite only for the notes this made favourites', async () => {
    const view = await render();
    for (const [, s] of mockLibrary().notes) s.note.favorite = s.note.title === 'Liste de courses';
    notifyLibraryChanged();
    await view.settle();
    await view.select('Réunion parents', 'Liste de courses');
    await dropOn(view, 'Réunion parents', '[data-view="favorites"]');
    expect(stored('Réunion parents')[1].note.favorite).toBe(true);
    expect(notices(view)).toEqual(['« Réunion parents » ajoutée aux favoris.']);
    const offer = view.q('.activity-offer')!;
    expect(offer.getAttribute('role')).toBe('status');
    await view.click(view.button('Annuler', offer));
    await vi.waitFor(() => expect(stored('Réunion parents')[1].note.favorite).toBe(false));
    // The favourite it already was stays one.
    expect(stored('Liste de courses')[1].note.favorite).toBe(true);
    expect(notices(view).at(-1)).toBe("Ajout aux favoris annulé : « Réunion parents » n'est plus un favori.");
  });

  it('counts several favourites, in English too', async () => {
    const view = await render('en');
    for (const [, s] of mockLibrary().notes) s.note.favorite = false;
    notifyLibraryChanged();
    await view.settle();
    await view.select('Réunion parents', 'Liste de courses');
    await dropOn(view, 'Réunion parents', '[data-view="favorites"]');
    expect(notices(view)).toEqual(['2 notes added to the favourites.']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(stored('Réunion parents')[1].note.favorite).toBe(false));
    expect(stored('Liste de courses')[1].note.favorite).toBe(false);
    expect(notices(view).at(-1)).toBe('Favourites undone: 2 notes are no longer favourites.');
  });

  it('undoes a tag only for the notes that did not have it', async () => {
    const view = await render();
    expect(stored('Cours de physique')[1].note.tags).toContain('physique');
    await view.select('Liste de courses', 'Cours de physique');
    await dropOn(view, 'Liste de courses', '[data-tag="physique"]');
    expect(stored('Liste de courses')[1].note.tags).toContain('physique');
    expect(notices(view)).toEqual(['Étiquette « physique » ajoutée à « Liste de courses ».']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(stored('Liste de courses')[1].note.tags).not.toContain('physique'));
    expect(stored('Cours de physique')[1].note.tags).toContain('physique');
    expect(notices(view).at(-1)).toBe('Étiquette annulée : « Liste de courses » n\'a plus l\'étiquette « physique ».');
  });

  it('keeps the other tags of a note that gained one, and says nothing when nothing changes', async () => {
    const view = await render();
    await dropOn(view, 'Cours de physique', '[data-tag="physique"]');
    expect(view.all('.activity-offer')).toHaveLength(0);
    await dropOn(view, 'Réunion parents', '[data-view="favorites"]');
    const note = stored('Liste de courses');
    await api.setNoteTags(note[0], ['autre'], note[1].stamp);
    notifyLibraryChanged();
    await view.settle();
    await dropOn(view, 'Liste de courses', '[data-tag="physique"]');
    expect(stored('Liste de courses')[1].note.tags).toEqual(['autre', 'physique']);
    await ctrlZ(view);
    await vi.waitFor(() => expect(stored('Liste de courses')[1].note.tags).toEqual(['autre']));
  });

  it('lists a note that left the library meanwhile, and undoes the others', async () => {
    const view = await render();
    for (const [, s] of mockLibrary().notes) s.note.favorite = false;
    notifyLibraryChanged();
    await view.settle();
    await view.select('Réunion parents', 'Liste de courses');
    await dropOn(view, 'Réunion parents', '[data-view="favorites"]');
    await api.trashNotes([pathOf('Liste de courses')]);
    await ctrlZ(view);
    await vi.waitFor(() => expect(stored('Réunion parents')[1].note.favorite).toBe(false));
    await vi.waitFor(() => expect(view.q('.activity-failures')).not.toBeNull());
    expect(view.q('.activity-failures')!.textContent).toContain('Une note n\'a pas pu être retirée des favoris :');
    expect(view.q('.activity-failures')!.textContent).toContain('« Liste de courses » : la note n\'est plus dans la bibliothèque');
    expect(notices(view).at(-1)).toBe("Ajout aux favoris annulé : « Réunion parents » n'est plus un favori.");
  });
});
