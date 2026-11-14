// @vitest-environment jsdom
// The free order of a binder, duplicating a note, changing its date, and exporting or printing a binder (Q20).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { freshStamp, mockLibrary, notePath, notifyLibraryChanged, resetMockLibrary, fileName, type StoredNote } from '../../lib/mock-data';
import { mockExportRequest, mockPrint } from '../../lib/mock-export';
import type { Note } from '../../lib/note';
import { ROOT, renderHome, type Ui } from './library-test-helpers';

let ui: Ui | null = null;

beforeEach(() => {
  resetMockLibrary();
  mockPrint.last = null;
  mockExportRequest.last = null;
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

afterEach(() => {
  ui?.destroy();
  ui = null;
  vi.restoreAllMocks();
});

/** Adds a note to the demo library, created at `created` (Unix ms), and returns its path. */
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

const DAY = 86_400_000;
const T0 = Date.UTC(2026, 9, 1, 9, 0);

/** Three notes in `Classe`, written on three days (the demo has `Cours de physique` in `Classe/3e B`). */
function seedClasse(): void {
  addNote('Classe', 'Alpha', T0);
  addNote('Classe', 'Beta', T0 + DAY);
  addNote('Classe', 'Gamma', T0 + 2 * DAY);
}

async function render(locale: 'fr' | 'en' = 'fr', expected = 6): Promise<Ui> {
  ui = await renderHome(locale, expected);
  return ui;
}

/** Shows `Classe` (its notes, then those of its sub-binder) in the free order. */
async function openClasse(view: Ui) {
  await view.click(view.q('[data-binder="Classe"]'));
  const sort = view.q<HTMLSelectElement>('select.sort')!;
  sort.value = 'binder';
  sort.dispatchEvent(new Event('change', { bubbles: true }));
  await view.settle();
}

const orderOf = (binder: string) => mockLibrary().orders.get(binder);
const idOf = (title: string) => `id-${title}`;

/** A drag event: a mouse event of that name with a data transfer (jsdom has no DragEvent). */
function dragEvent(type: string, clientX = 0): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX });
  const store = new Map<string, string>();
  Object.defineProperty(event, 'dataTransfer', {
    value: { effectAllowed: '', dropEffect: '', setData: (k: string, v: string) => store.set(k, v), getData: (k: string) => store.get(k) ?? '' },
  });
  return event;
}

/** Gives a card a box 100 wide at the origin, so that a pointer at `clientX` is on its left or right half. */
function boxed(card: Element): Element {
  Object.defineProperty(card, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({ left: 0, right: 100, top: 0, bottom: 100, width: 100, height: 100, x: 0, y: 0, toJSON: () => ({}) }),
  });
  return card;
}

describe('the free order of a binder: the sort choice', () => {
  it('is offered only in the view of a binder, and the other views show the notes by date', async () => {
    seedClasse();
    const view = await render();
    const options = () => [...view.all<HTMLOptionElement>('select.sort option')].map((o) => o.textContent?.trim());
    expect(options()).toEqual([
      'Date : la plus récente d\'abord',
      'Date : la plus ancienne d\'abord',
      'Titre : A → Z',
      'Titre : Z → A',
    ]);
    await view.click(view.q('[data-binder="Classe"]'));
    expect(options()).toContain('Ordre du classeur');
    const sort = view.q<HTMLSelectElement>('select.sort')!;
    sort.value = 'binder';
    sort.dispatchEvent(new Event('change', { bubbles: true }));
    await view.settle();
    expect(sort.value).toBe('binder');
    // The notes of the binder, oldest first (no order yet), then its sub-binder's.
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    // Another view: by date, newest first, and the choice is gone from the menu.
    await view.click(view.q('[data-view="all"]'));
    expect(options()).not.toContain('Ordre du classeur');
    expect(view.q<HTMLSelectElement>('select.sort')!.value).toBe('date-desc');
    expect(view.titles()).toEqual(['Réunion parents', 'Liste de courses', 'Cours de physique', 'Gamma', 'Beta', 'Alpha']);
    // Back in the binder, the order is still the one chosen.
    await view.click(view.q('[data-binder="Classe"]'));
    expect(view.q<HTMLSelectElement>('select.sort')!.value).toBe('binder');
  });

  it('follows the order the library holds, ignoring notes it lists that are gone and putting the others last', async () => {
    seedClasse();
    mockLibrary().orders.set('Classe', ['gone', idOf('Gamma'), 'also-gone', idOf('Beta')]);
    const view = await render();
    await openClasse(view);
    expect(view.titles()).toEqual(['Gamma', 'Beta', 'Alpha', 'Cours de physique']);
  });

  it('is in English too', async () => {
    seedClasse();
    const view = await render('en');
    await view.click(view.q('[data-binder="Classe"]'));
    const sort = view.q<HTMLSelectElement>('select.sort')!;
    expect([...sort.options].map((o) => o.textContent?.trim())).toContain('Binder order');
    sort.value = 'binder';
    sort.dispatchEvent(new Event('change', { bubbles: true }));
    await view.settle();
    expect(view.q('[data-slot="order-help"]')?.textContent).toContain('dragging them between two cards');
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
  });
});

describe('the free order of a binder: the keyboard', () => {
  it('moves a card with Alt + arrows, says so, and keeps the order in the library', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    const open = (title: string) => view.card(title).querySelector<HTMLElement>('.card-open')!;

    await view.key(open('Beta'), 'ArrowLeft', { altKey: true });
    expect(view.titles()).toEqual(['Beta', 'Alpha', 'Gamma', 'Cours de physique']);
    expect(orderOf('Classe')).toEqual([idOf('Beta'), idOf('Alpha'), idOf('Gamma')]);
    expect(view.q('[data-slot="order-announcement"]')?.textContent).toBe('Beta déplacée en position 1 sur 3.');

    await view.key(open('Beta'), 'ArrowRight', { altKey: true });
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    await view.key(open('Alpha'), 'End', { altKey: true });
    expect(view.titles()).toEqual(['Beta', 'Gamma', 'Alpha', 'Cours de physique']);
    await view.key(open('Alpha'), 'Home', { altKey: true });
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);

    // At the start: nothing moves, and it is said.
    await view.key(open('Alpha'), 'ArrowLeft', { altKey: true });
    expect(view.q('[data-slot="order-announcement"]')?.textContent).toBe('Déjà au début du classeur.');
    // The notes of a sub-binder stay after the binder's own: a card moves among its binder's.
    await view.key(open('Cours de physique'), 'ArrowLeft', { altKey: true });
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    expect(orderOf('Classe/3e B')).toBeUndefined();

    // The order survives a reload of the listing.
    notifyLibraryChanged();
    await view.settle();
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    await view.key(open('Gamma'), 'ArrowLeft', { altKey: true });
    notifyLibraryChanged();
    await view.settle();
    expect(view.titles()).toEqual(['Alpha', 'Gamma', 'Beta', 'Cours de physique']);
  });

  it('keeps the focus on the card that moved', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    const open = view.card('Gamma').querySelector<HTMLElement>('.card-open')!;
    open.focus();
    await view.key(open, 'ArrowLeft', { altKey: true });
    await view.settle();
    expect(document.activeElement).toBe(view.card('Gamma').querySelector('.card-open'));
  });

  it('moves the selected cards together', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    await view.select('Alpha', 'Gamma');
    await view.key(view.card('Gamma').querySelector('.card-open'), 'ArrowLeft', { altKey: true });
    expect(view.titles()).toEqual(['Alpha', 'Gamma', 'Beta', 'Cours de physique']);
    expect(view.q('[data-slot="order-announcement"]')?.textContent).toBe('2 notes déplacées à partir de la position 1 sur 3.');
  });

  it('does nothing outside the free order, nor during a search', async () => {
    seedClasse();
    const view = await render();
    await view.click(view.q('[data-binder="Classe"]'));
    // By date, newest first: Alt + arrow does not reorder.
    await view.key(view.card('Beta').querySelector('.card-open'), 'ArrowLeft', { altKey: true });
    expect(orderOf('Classe')).toBeUndefined();
    expect(view.q('[data-slot="order-help"]')).toBeNull();
    // The free order with a search: shown, but the cards stay where they are.
    const sort = view.q<HTMLSelectElement>('select.sort')!;
    sort.value = 'binder';
    sort.dispatchEvent(new Event('change', { bubbles: true }));
    await view.settle();
    await view.type(view.q<HTMLInputElement>('input.search'), 'a');
    expect(view.q('[data-slot="order-help"]')?.textContent).toContain('ne se déplacent pas');
    await view.key(view.card('Beta').querySelector('.card-open'), 'ArrowLeft', { altKey: true });
    expect(orderOf('Classe')).toBeUndefined();
  });

  it('writes the orders of a key held down one after the other, in the order of the moves', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    const written: string[][] = [];
    const real = api.setBinderOrder;
    let first = true;
    vi.spyOn(api, 'setBinderOrder').mockImplementation(async (binder, ids) => {
      // The first write is slow: without a queue the second one would be over before it.
      if (first) {
        first = false;
        await new Promise((resolve) => setTimeout(resolve, 60));
      }
      written.push(ids);
      return real(binder, ids);
    });
    const gamma = () => view.card('Gamma').querySelector('.card-open')!;
    gamma().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true, cancelable: true }));
    gamma().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(written.length).toBe(2));
    await view.settle();
    expect(written).toEqual([
      [idOf('Alpha'), idOf('Gamma'), idOf('Beta')],
      [idOf('Gamma'), idOf('Alpha'), idOf('Beta')],
    ]);
    expect(orderOf('Classe')).toEqual([idOf('Gamma'), idOf('Alpha'), idOf('Beta')]);
    expect(view.titles()).toEqual(['Gamma', 'Alpha', 'Beta', 'Cours de physique']);
  });

  it('keeps a move that is made while the listing is being read again', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    const real = api.libraryList;
    // A listing read before the move, delivered after it: it holds the older order.
    vi.spyOn(api, 'libraryList').mockImplementationOnce(async () => {
      const listing = await real();
      await new Promise((resolve) => setTimeout(resolve, 80));
      return listing;
    });
    notifyLibraryChanged();
    await view.key(view.card('Beta').querySelector('.card-open'), 'ArrowLeft', { altKey: true });
    await new Promise((resolve) => setTimeout(resolve, 200));
    await view.settle();
    expect(view.titles()).toEqual(['Beta', 'Alpha', 'Gamma', 'Cours de physique']);
    expect(orderOf('Classe')).toEqual([idOf('Beta'), idOf('Alpha'), idOf('Gamma')]);
  });

  it('shows what the library could not store', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    vi.spyOn(api, 'setBinderOrder').mockRejectedValueOnce({ code: 'orderNewer', message: 'newer' });
    await view.key(view.card('Beta').querySelector('.card-open'), 'ArrowLeft', { altKey: true });
    expect(view.q('.banner.error')?.textContent).toContain('version plus récente');
    // The grid is back to what the library holds.
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
  });
});

describe('the free order of a binder: dragging between the cards', () => {
  it('shows where the cards would land and moves them there on drop', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    const gamma = view.card('Gamma');
    gamma.querySelector('.card-open')!.dispatchEvent(dragEvent('dragstart'));
    await view.settle();

    // Over the left half of Alpha: the mark is before it, and the drop is accepted.
    const alpha = boxed(view.card('Alpha'));
    const over = dragEvent('dragover', 10);
    alpha.dispatchEvent(over);
    await view.settle();
    expect(over.defaultPrevented).toBe(true);
    expect(alpha.classList.contains('insert-before')).toBe(true);
    expect(view.all('.insert-before, .insert-after').length).toBe(1);

    // Over the right half of Alpha: the mark moves to the other side of the same card.
    alpha.dispatchEvent(dragEvent('dragover', 90));
    await view.settle();
    expect(alpha.classList.contains('insert-after')).toBe(true);
    expect(alpha.classList.contains('insert-before')).toBe(false);
    // Over the right half of Beta, the spot just before the card being dragged: it would change nothing, so no mark.
    const beta = boxed(view.card('Beta'));
    const nothing = dragEvent('dragover', 90);
    beta.dispatchEvent(nothing);
    await view.settle();
    expect(nothing.defaultPrevented).toBe(false);
    expect(view.all('.insert-before, .insert-after').length).toBe(0);

    // Back on Alpha, then dropped.
    alpha.dispatchEvent(dragEvent('dragover', 10));
    await view.settle();
    alpha.dispatchEvent(dragEvent('drop', 10));
    await view.settle();
    expect(view.titles()).toEqual(['Gamma', 'Alpha', 'Beta', 'Cours de physique']);
    expect(orderOf('Classe')).toEqual([idOf('Gamma'), idOf('Alpha'), idOf('Beta')]);
    expect(view.all('.insert-before, .insert-after').length).toBe(0);
  });

  it('refuses a drop that changes nothing, or where no card of the dragged notes binder is next', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    view.card('Beta').querySelector('.card-open')!.dispatchEvent(dragEvent('dragstart'));
    await view.settle();
    // On the dragged card itself.
    const own = dragEvent('dragover', 10);
    boxed(view.card('Beta')).dispatchEvent(own);
    expect(own.defaultPrevented).toBe(false);
    // On the card of another binder, at the far end: the sub-binder's card is not next to a card of Classe on that side.
    const far = dragEvent('dragover', 90);
    boxed(view.card('Cours de physique')).dispatchEvent(far);
    expect(far.defaultPrevented).toBe(false);
    expect(view.all('.insert-before, .insert-after').length).toBe(0);
    boxed(view.card('Alpha')).dispatchEvent(dragEvent('drop', 10));
    await view.settle();
    expect(orderOf('Classe')).toBeUndefined();
  });

  it('drags the whole selection, in its own order', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    await view.select('Alpha', 'Beta');
    view.card('Beta').querySelector('.card-open')!.dispatchEvent(dragEvent('dragstart'));
    await view.settle();
    boxed(view.card('Gamma')).dispatchEvent(dragEvent('dragover', 90));
    await view.settle();
    boxed(view.card('Gamma')).dispatchEvent(dragEvent('drop', 90));
    await view.settle();
    expect(view.titles()).toEqual(['Gamma', 'Alpha', 'Beta', 'Cours de physique']);
  });

  it('still moves a note to another binder when it is dropped on the sidebar', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    view.card('Gamma').querySelector('.card-open')!.dispatchEvent(dragEvent('dragstart'));
    await view.settle();
    const target = view.q('[data-binder="Perso"]')!;
    const over = dragEvent('dragover');
    target.dispatchEvent(over);
    expect(over.defaultPrevented).toBe(true);
    target.dispatchEvent(dragEvent('drop'));
    await view.settle();
    expect(view.titles()).toEqual(['Alpha', 'Beta', 'Cours de physique']);
    expect([...mockLibrary().notes.values()].find((s) => s.note.title === 'Gamma')?.binder).toBe('Perso');
  });

  it('does not accept a drop between cards in the other sorts', async () => {
    seedClasse();
    const view = await render();
    await view.click(view.q('[data-binder="Classe"]'));
    view.card('Gamma').querySelector('.card-open')!.dispatchEvent(dragEvent('dragstart'));
    await view.settle();
    const over = dragEvent('dragover', 10);
    boxed(view.card('Alpha')).dispatchEvent(over);
    expect(over.defaultPrevented).toBe(false);
    expect(view.all('.insert-before, .insert-after').length).toBe(0);
  });
});

describe('duplicating a note', () => {
  /** Opens the menu of a card with the right click. */
  async function rightClick(view: Ui, title: string) {
    view.card(title).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await view.settle();
  }

  it('is in the right-click menu of a card: same binder, same date, « (copie) », not a favourite, with an undo', async () => {
    const view = await render('fr', 3);
    // The demo note "Liste de courses" is a favourite and has no binder.
    const before = view.card('Liste de courses').querySelector('.card-date')?.textContent;
    await rightClick(view, 'Liste de courses');
    const menu = view.q('[role="menu"]');
    expect(menu).not.toBeNull();
    expect([...menu!.querySelectorAll('[role="menuitem"]')].map((i) => i.textContent?.trim())).toEqual([
      'Renommer',
      'Retirer des favoris',
      'Reprendre en mode direct',
      'Dupliquer',
      'Changer la date…',
      'Mettre à la corbeille',
    ]);
    await view.click(view.button('Dupliquer', menu!));
    await vi.waitFor(() => expect(view.titles()).toContain('Liste de courses (copie)'));
    expect(view.card('Liste de courses (copie)').querySelector('.card-date')?.textContent).toBe(before);
    const copy = [...mockLibrary().notes.values()].find((s) => s.note.title === 'Liste de courses (copie)')!;
    const original = [...mockLibrary().notes.values()].find((s) => s.note.title === 'Liste de courses')!;
    expect(copy.binder).toBe('');
    expect(copy.note.created).toBe(original.note.created);
    expect(copy.note.id).not.toBe(original.note.id);
    expect(copy.note.favorite).toBe(false);
    expect(copy.original).toEqual(original.original);
    expect(view.q('.activity-offer')?.textContent).toContain('Copie créée');

    // « Annuler » puts the copy in the trash.
    await view.click(view.button('Annuler'));
    await vi.waitFor(() => expect(view.titles()).not.toContain('Liste de courses (copie)'));
    expect(mockLibrary().trash.size).toBe(1);
    expect(view.q('.activity-offer')?.textContent).toContain('à la corbeille');
  });

  it('numbers the copy of a copy and puts a copy after its original in the order of a binder', async () => {
    seedClasse();
    const view = await render();
    await openClasse(view);
    await view.key(view.card('Beta').querySelector('.card-open'), 'ArrowLeft', { altKey: true });
    expect(orderOf('Classe')).toEqual([idOf('Beta'), idOf('Alpha'), idOf('Gamma')]);
    await rightClick(view, 'Alpha');
    await view.click(view.button('Dupliquer', view.q('[role="menu"]')!));
    await vi.waitFor(() => expect(view.titles()).toContain('Alpha (copie)'));
    expect(view.titles()).toEqual(['Beta', 'Alpha', 'Alpha (copie)', 'Gamma', 'Cours de physique']);
    await rightClick(view, 'Alpha (copie)');
    await view.click(view.button('Dupliquer', view.q('[role="menu"]')!));
    await vi.waitFor(() => expect(view.titles()).toContain('Alpha (copie 2)'));
  });

  it('is in the selection bar, between « Assembler… » and « Tourner », for several notes', async () => {
    const view = await render('fr', 3);
    await view.select('Liste de courses', 'Réunion parents');
    const labels = [...view.all('.selection-bar > button')].map((b) => b.textContent?.trim());
    expect(labels.indexOf('Dupliquer')).toBe(labels.indexOf('Assembler…') + 1);
    expect(labels.indexOf('Tourner')).toBe(labels.indexOf('Dupliquer') + 1);
    await view.click(view.button('Dupliquer', view.q('.selection-bar')!));
    await vi.waitFor(() => expect(view.titles().length).toBe(5));
    expect(view.titles()).toContain('Liste de courses (copie)');
    expect(view.titles()).toContain('Réunion parents (copie)');
    expect(view.q('.activity-offer')?.textContent).toContain('2 copies créées');
    // Undone together.
    await view.click(view.button('Annuler'));
    await vi.waitFor(() => expect(view.titles().length).toBe(3));
  });

  it('is in English, with the English copy word', async () => {
    const view = await render('en', 3);
    await rightClick(view, 'Réunion parents');
    await view.click(view.button('Duplicate', view.q('[role="menu"]')!));
    await vi.waitFor(() => expect(view.titles()).toContain('Réunion parents (copy)'));
  });

  it('keeps one menu open: the right click on another card closes the first one', async () => {
    const view = await render('fr', 3);
    await rightClick(view, 'Liste de courses');
    expect(view.all('[role="menu"]').length).toBe(1);
    await rightClick(view, 'Réunion parents');
    expect(view.all('[role="menu"]').length).toBe(1);
    expect(view.card('Réunion parents').querySelector('[role="menu"]')).not.toBeNull();
    expect(view.card('Liste de courses').querySelector('[role="menu"]')).toBeNull();
  });

  it('opens the menu from the keyboard too: the button of the card', async () => {
    const view = await render('fr', 3);
    await view.click(view.byLabel('Actions pour Liste de courses'));
    expect(view.q('[role="menu"]')).not.toBeNull();
    expect(view.button('Dupliquer', view.q('[role="menu"]')!)).toBeTruthy();
  });
});

describe('changing the date of a note', () => {
  async function openDateDialog(view: Ui, title: string) {
    view.card(title).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await view.settle();
    await view.click(view.button('Changer la date…', view.q('[role="menu"]')!));
  }

  const field = (view: Ui) => view.q<HTMLInputElement>('[data-dialog="change-date"] input[type="datetime-local"]')!;

  it('sets the creation date to a date typed in the field, renames the file, and can be undone', async () => {
    const view = await render('fr', 3);
    await openDateDialog(view, 'Réunion parents');
    expect(view.q('[data-dialog="change-date"]')).not.toBeNull();
    // The field starts at the current date.
    expect(field(view).value).toMatch(/^2026-10-23T\d\d:\d\d$/);
    await view.type(field(view), '2025-03-04T12:30');
    await view.click(view.q('[data-action="date-apply"]'));
    await vi.waitFor(() => expect(view.q('[data-dialog="change-date"]')).toBeNull());
    const stored = [...mockLibrary().notes.entries()].find(([, s]) => s.note.title === 'Réunion parents')!;
    expect(stored[1].note.created).toBe(new Date('2025-03-04T12:30').getTime());
    expect(stored[0]).toContain('2025-03-04 Réunion parents');
    // The grid sorts by it: now the oldest.
    expect(view.titles()).toEqual(['Liste de courses', 'Cours de physique', 'Réunion parents']);
    expect(view.q('.activity-offer')?.textContent).toContain('Date de « Réunion parents » changée');

    await view.click(view.button('Annuler'));
    await vi.waitFor(() => expect(view.titles()).toEqual(['Réunion parents', 'Liste de courses', 'Cours de physique']));
    expect(view.q('.activity-offer')?.textContent).toContain('rétablie');
  });

  it('« Aujourd\'hui » fills the field with now', async () => {
    const view = await render('fr', 3);
    await openDateDialog(view, 'Réunion parents');
    const before = Date.now();
    await view.click(view.q('[data-action="date-today"]'));
    expect(new Date(field(view).value).getTime()).toBeGreaterThanOrEqual(before - 60_000);
    expect(new Date(field(view).value).getTime()).toBeLessThanOrEqual(Date.now());
    await view.click(view.q('[data-action="date-apply"]'));
    await vi.waitFor(() => expect(view.q('[data-dialog="change-date"]')).toBeNull());
    const created = [...mockLibrary().notes.values()].find((s) => s.note.title === 'Réunion parents')!.note.created;
    expect(Date.now() - created).toBeLessThan(2 * 60_000);
  });

  it('writes nothing when the field is left as it was, and refuses an empty one', async () => {
    const view = await render('fr', 3);
    const spy = vi.spyOn(api, 'setNoteCreated');
    await openDateDialog(view, 'Réunion parents');
    await view.click(view.q('[data-action="date-apply"]'));
    expect(spy).not.toHaveBeenCalled();
    expect(view.q('[data-dialog="change-date"]')).toBeNull();
    await openDateDialog(view, 'Réunion parents');
    await view.type(field(view), '');
    expect(view.q<HTMLButtonElement>('[data-action="date-apply"]')!.disabled).toBe(true);
  });

  it('shows the library error and stays open', async () => {
    const view = await render('en', 3);
    vi.spyOn(api, 'setNoteCreated').mockRejectedValueOnce({ code: 'invalidDate', message: 'x' });
    view.card('Réunion parents').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await view.settle();
    await view.click(view.button('Change the date…', view.q('[role="menu"]')!));
    await view.type(field(view), '2025-03-04T12:30');
    await view.click(view.q('[data-action="date-apply"]'));
    expect(view.q('[data-dialog="change-date"] [role="alert"]')?.textContent).toContain('cannot be a note');
  });
});

describe('exporting and printing a binder', () => {
  /** Opens the right-click menu of a binder of the sidebar and chooses `label`. */
  async function chooseInBinderMenu(view: Ui, binder: string, label: string) {
    view.q(`[data-binder="${binder}"]`)!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await view.settle();
    await view.click(view.button(label, view.q('[role="menu"]')!));
  }

  it('exports a binder as one PDF in its order, with a bookmark per note, and the sub-binders as an option', async () => {
    seedClasse();
    mockLibrary().orders.set('Classe', [idOf('Gamma'), idOf('Alpha'), idOf('Beta')]);
    const view = await render();
    await chooseInBinderMenu(view, 'Classe', 'Exporter le classeur en PDF…');
    const dialog = view.q('[role="dialog"]')!;
    expect(dialog.textContent).toContain('Classeur « Classe »');
    // Only the PDF: no choice of format, no text export.
    expect(dialog.querySelector('input[name="export-format"]')).toBeNull();
    expect(dialog.textContent).toContain('3 au total');
    const include = dialog.querySelector<HTMLInputElement>('input[data-option="include-sub-binders"]')!;
    expect(include.checked).toBe(false);

    await view.click(view.button('Exporter…', dialog as HTMLElement));
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    let request = mockExportRequest.last!;
    expect(request.format).toBe('pdf');
    expect(request.destination.endsWith('Classe.pdf')).toBe(true);
    expect(request.sources.map((s) => s.path.split('\\').pop()?.replace(/^\d{4}-\d\d-\d\d /, '').replace('.ardoise', ''))).toEqual(['Gamma', 'Alpha', 'Beta']);
    expect(request.outline).toEqual([
      { path: [], title: 'Gamma' },
      { path: [], title: 'Alpha' },
      { path: [], title: 'Beta' },
    ]);
    expect(request.sources.every((s) => !('outline' in s))).toBe(true);

    // With the sub-binders: their notes follow, each under its binder's name.
    await view.click(include);
    expect(dialog.textContent).toContain('4 au total');
    mockExportRequest.last = null;
    await view.click(view.button('Exporter de nouveau…', dialog as HTMLElement));
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    request = mockExportRequest.last!;
    expect(request.outline).toEqual([
      { path: [], title: 'Gamma' },
      { path: [], title: 'Alpha' },
      { path: [], title: 'Beta' },
      { path: ['3e B'], title: 'Cours de physique' },
    ]);
  });

  it('starts with the sub-binders included for a binder whose notes are all in its sub-binders, and exports them', async () => {
    // `Classe` of the demo library holds no note of its own, only « Cours de physique » in `Classe/3e B`.
    const view = await render('fr', 3);
    await chooseInBinderMenu(view, 'Classe', 'Exporter le classeur en PDF…');
    const dialog = view.q('[role="dialog"]')!;
    const include = dialog.querySelector<HTMLInputElement>('input[data-option="include-sub-binders"]')!;
    expect(include.checked).toBe(true);
    expect(dialog.textContent).toContain('1 au total');
    expect(dialog.textContent).toContain("n'a pas de note à lui");
    await view.click(view.button('Exporter…', dialog as HTMLElement));
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    expect(mockExportRequest.last!.outline).toEqual([{ path: ['3e B'], title: 'Cours de physique' }]);
    // Unchecked: nothing to export, said plainly, and the button waits.
    await view.click(include);
    expect(dialog.textContent).toContain('cochez « Inclure les sous-classeurs »');
    expect(view.button('Exporter de nouveau…', dialog as HTMLElement).disabled).toBe(true);
  });

  it('prints the notes of the sub-binders from the start for such a binder', async () => {
    const view = await render('fr', 3);
    await chooseInBinderMenu(view, 'Classe', 'Imprimer le classeur…');
    const dialog = view.q<HTMLElement>('[data-dialog="print"]')!;
    expect(dialog.querySelector<HTMLInputElement>('input[data-option="include-sub-binders"]')!.checked).toBe(true);
    expect(dialog.textContent).toContain('Notes à imprimer : 1');
    await view.click(view.button('Imprimer…', dialog));
    await vi.waitFor(() => expect(mockPrint.last).not.toBeNull());
    expect(mockPrint.last!.sources).toHaveLength(1);
  });

  it('names an untitled note in the bookmarks by « Note sans titre » unless the dates are asked for', async () => {
    addNote('Classe', '', T0);
    addNote('Classe', '', T0 + DAY);
    const view = await render('fr', 5);
    await chooseInBinderMenu(view, 'Classe', 'Exporter le classeur en PDF…');
    const dialog = view.q('[role="dialog"]')!;
    await view.click(view.button('Exporter…', dialog as HTMLElement));
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    expect(mockExportRequest.last!.outline!.map((o) => o.title)).toEqual(['Note sans titre', 'Note sans titre (2)']);
    await view.click(dialog.querySelector('input[data-option="show-dates"]'));
    mockExportRequest.last = null;
    await view.click(view.button('Exporter de nouveau…', dialog as HTMLElement));
    await vi.waitFor(() => expect(mockExportRequest.last).not.toBeNull());
    const titles = mockExportRequest.last!.outline!.map((o) => o.title);
    expect(titles.every((title) => /2026/.test(title))).toBe(true);
  });

  it('offers no sub-binders for a binder without any, and the entries are off for an empty binder', async () => {
    const view = await render('fr', 3);
    await chooseInBinderMenu(view, 'Perso', 'Exporter le classeur en PDF…');
    expect(view.q('input[data-option="include-sub-binders"]')).toBeNull();
    expect(view.q('[role="dialog"]')!.textContent).toContain('1 au total');
  });

  it('disables the entries of a binder that holds no note', async () => {
    await api.createBinder('Vide');
    const view = await render('fr', 3);
    view.q('[data-binder="Vide"]')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await view.settle();
    const items = [...view.all('[role="menu"] [role="menuitem"]')];
    const pdf = items.find((i) => i.textContent?.trim() === 'Exporter le classeur en PDF…')!;
    expect(pdf.getAttribute('aria-disabled')).toBe('true');
    const print = items.find((i) => i.textContent?.trim() === 'Imprimer le classeur…')!;
    expect(print.getAttribute('aria-disabled')).toBe('true');
  });

  it('prints a binder, 1, 2 or 4 notes per sheet, with the sub-binders as an option', async () => {
    seedClasse();
    const view = await render();
    await chooseInBinderMenu(view, 'Classe', 'Imprimer le classeur…');
    const dialog = view.q<HTMLElement>('[data-dialog="print"]')!;
    expect(dialog.textContent).toContain('Notes à imprimer : 3');
    const perSheet = (n: number) => dialog.querySelector<HTMLInputElement>(`input[name="print-per-sheet"][value="${n}"]`)!;
    expect(perSheet(1).checked).toBe(true);
    await view.click(perSheet(2));
    expect(dialog.textContent).toContain('les pages sont les feuilles');
    await view.click(dialog.querySelector('input[data-option="include-sub-binders"]'));
    expect(dialog.textContent).toContain('Notes à imprimer : 4');
    await view.click(view.button('Imprimer…', dialog));
    await vi.waitFor(() => expect(mockPrint.last).not.toBeNull());
    expect(mockPrint.last!.options).toEqual({ background: true, fit: 'actual', perSheet: 2 });
    expect(mockPrint.last!.sources.map((s) => (s.path.match(/ (Alpha|Beta|Gamma|Cours de physique)\.ardoise$/) ?? [])[1])).toEqual(['Alpha', 'Beta', 'Gamma', 'Cours de physique']);
    // Two per sheet: four notes make two sheets.
    await vi.waitFor(() => expect(dialog.querySelector('[role="status"]')?.textContent).toContain('2 pages'));
    // The choice is remembered.
    await view.click(view.button('Fermer', dialog));
    await chooseInBinderMenu(view, 'Classe', 'Imprimer le classeur…');
    expect(view.q<HTMLInputElement>('input[name="print-per-sheet"][value="2"]')!.checked).toBe(true);
  });

  it('exports and prints in English', async () => {
    seedClasse();
    const view = await render('en');
    await chooseInBinderMenu(view, 'Classe', 'Export the binder as a PDF…');
    expect(view.q('[role="dialog"]')!.textContent).toContain('Binder "Classe"');
    expect(view.q('[role="dialog"]')!.textContent).toContain('Include the sub-binders');
  });
});
