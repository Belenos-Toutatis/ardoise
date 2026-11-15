// @vitest-environment jsdom
// The menu of a note card is where every action on notes lives (user request 2026-11-15: no bar of buttons above the grid):
// on a selected card it acts on the whole selection, on another card on that note alone; one note or several, the entries
// that only make sense for one are left out, and « Assembler… » needs two.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { cardMenuEntry, cardMenuLabels } from './card-menu-test-helpers';
import { renderHome, type Ui } from './library-test-helpers';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('../editor/VideoExportDialogDouble.svelte')).default,
}));

let ui: Ui | null = null;

beforeEach(() => {
  resetMockLibrary();
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

async function ready(locale: 'fr' | 'en' = 'fr') {
  ui = await renderHome(locale, 3);
  return ui;
}

const favoriteOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)!.note.favorite;
const tagsOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)!.note.tags;
const binderOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)!.binder;
const menu = () => document.querySelector<HTMLElement>('[role="menu"]')!;
const heading = () => menu().querySelector('.menu-heading')?.textContent ?? null;

const FULL_ONE = [
  'Renommer',
  'Reprendre en mode direct',
  'Déplacer vers…',
  'Étiquettes…',
  'Ajouter aux favoris',
  'Changer la date…',
  'Exporter…',
  'Imprimer…',
  'Dupliquer',
  'Tourner à gauche',
  'Tourner à droite',
  'Mettre à la corbeille',
];

describe('the menu of a card with no selection', () => {
  it('offers every action on that note, grouped, the trash last and in red, with no bar above the grid', async () => {
    const view = await ready();
    await view.openMenu('Réunion parents');
    // « Réunion parents » is not a favourite; nothing is selected, so no « Désélectionner » and no heading.
    expect(cardMenuLabels()).toEqual(FULL_ONE);
    expect(heading()).toBeNull();
    expect(menu().getAttribute('aria-label')).toBe('Actions pour Réunion parents');
    expect(menu().querySelectorAll('[role="separator"]').length).toBe(3);
    expect(cardMenuEntry('Mettre à la corbeille').classList.contains('danger')).toBe(true);
    expect(view.q('.selection-bar')).toBeNull();
    expect(view.q('[role="toolbar"]')).toBeNull();
  });

  it('leaves « Assembler… » out: it needs two notes', async () => {
    const view = await ready();
    await view.openMenu('Réunion parents');
    expect(cardMenuLabels()).not.toContain('Assembler…');
  });

  it('shows the favourite entry for what the note is', async () => {
    const view = await ready();
    await view.openMenu('Liste de courses');
    expect(cardMenuLabels()).toContain('Retirer des favoris');
  });

  it('speaks English', async () => {
    const view = await ready('en');
    await view.openMenu('Réunion parents');
    expect(cardMenuLabels()).toEqual([
      'Rename',
      'Resume in live mode',
      'Move to…',
      'Tags…',
      'Add to favourites',
      'Change the date…',
      'Export…',
      'Print…',
      'Duplicate',
      'Turn left',
      'Turn right',
      'Move to trash',
    ]);
  });
});

describe('the menu of a card of the selection', () => {
  it('acts on the whole selection: a heading with the count, and only the entries that fit several notes', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Cours de physique', 'Liste de courses');
    await view.openMenu('Liste de courses');
    expect(heading()).toBe('3 notes sélectionnées');
    expect(menu().getAttribute('aria-label')).toBe('Actions pour 3 notes sélectionnées');
    // The heading is text, not an entry: it cannot be chosen or focused.
    expect(menu().querySelector('.menu-heading [role="menuitem"]')).toBeNull();
    // Not every selected note is a favourite: the entry adds them.
    expect(cardMenuLabels()).toEqual([
      'Déplacer vers…',
      'Étiquettes…',
      'Ajouter aux favoris',
      'Exporter…',
      'Imprimer…',
      'Assembler…',
      'Dupliquer',
      'Tourner à gauche',
      'Tourner à droite',
      'Désélectionner',
      'Mettre à la corbeille',
    ]);
  });

  it('offers the favourite entry the other way once every selected note is a favourite', async () => {
    const view = await ready();
    // « Liste de courses » is one already; « Réunion parents » becomes one (its own menu, nothing selected).
    await view.choose('Réunion parents', 'Ajouter aux favoris');
    await view.select('Liste de courses', 'Réunion parents');
    await view.openMenu('Réunion parents');
    expect(cardMenuLabels()).toContain('Retirer des favoris');
  });

  it('with one note selected, is the menu of that note and says Désélectionner', async () => {
    const view = await ready();
    await view.select('Réunion parents');
    await view.openMenu('Réunion parents');
    expect(heading()).toBeNull();
    expect(cardMenuLabels()).toEqual([...FULL_ONE.slice(0, -1), 'Désélectionner', 'Mettre à la corbeille']);
  });

  it('adds the favourite to all of them, then takes it out of all of them', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Cours de physique');
    await view.choose('Cours de physique', 'Ajouter aux favoris');
    expect([favoriteOf('Réunion parents'), favoriteOf('Cours de physique')]).toEqual([true, true]);
    await view.choose('Cours de physique', 'Retirer des favoris');
    expect([favoriteOf('Réunion parents'), favoriteOf('Cours de physique')]).toEqual([false, false]);
  });

  it('tags all of them from the « Étiquettes… » dialog', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Étiquettes…');
    const dialog = view.q('[data-dialog="tags"]')!;
    expect(dialog.closest('[role="dialog"]')?.querySelector('h2')?.textContent).toBe('Étiquettes');
    expect(dialog.textContent).toContain('Notes sélectionnées : 2');
    await view.type(view.byLabel('Ajouter une étiquette') as HTMLInputElement, 'urgent');
    await view.click(view.button('Ajouter', dialog));
    expect(tagsOf('Réunion parents')).toContain('urgent');
    expect(tagsOf('Liste de courses')).toContain('urgent');
    expect(tagsOf('Cours de physique')).not.toContain('urgent');
    await view.click(view.button('Fermer', dialog));
    expect(view.q('[data-dialog="tags"]')).toBeNull();
    // The selection is kept.
    expect(view.all('.note-card.selected').length).toBe(2);
  });

  it('moves all of them from the « Déplacer vers… » dialog, which Escape closes without moving', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Déplacer vers…');
    expect(view.q('[data-dialog="move"]')!.textContent).toContain('Notes sélectionnées : 2');
    await view.key(document.body, 'Escape');
    expect(view.q('[data-dialog="move"]')).toBeNull();
    expect(binderOf('Réunion parents')).toBe('Perso');
    await view.choose('Liste de courses', 'Déplacer vers…');
    await view.click(view.q('[data-dialog="move"] [data-binder="Classe"]')!);
    expect([binderOf('Réunion parents'), binderOf('Liste de courses')]).toEqual(['Classe', 'Classe']);
    expect(view.q('.activity-offer')?.textContent).toContain('2 notes déplacées dans « Classe »');
  });

  it('exports, prints and assembles exactly the selected notes', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Exporter…');
    expect(view.q('[role="dialog"]')!.textContent).toContain('Notes sélectionnées : 2');
    await view.key(document.body, 'Escape');
    await view.choose('Liste de courses', 'Imprimer…');
    expect(view.q('[data-dialog="print"]')!.textContent).toContain('Notes sélectionnées : 2');
    await view.key(document.body, 'Escape');
    await view.choose('Liste de courses', 'Assembler…');
    expect(view.q('[role="dialog"]')!.textContent).toContain('Notes à assembler : 2');
  });

  it('copies and turns all of them, and the notice offers to undo', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Dupliquer');
    await vi.waitFor(() => expect(view.titles().length).toBe(5));
    expect(view.q('.activity-offer')?.textContent).toContain('2 copies créées');
    await view.choose('Liste de courses', 'Tourner à droite');
    await vi.waitFor(() => expect(view.all('.activity-offer').map((o) => o.textContent).join(' | ')).toContain('2 notes tournées'));
  });

  it('trashes all of them with the usual notice, and Désélectionner drops the selection', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Désélectionner');
    expect(view.all('.note-card.selected').length).toBe(0);
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Mettre à la corbeille');
    await vi.waitFor(() => expect(view.titles()).toEqual(['Cours de physique']));
    expect(view.q('.activity-offer')?.textContent).toContain('2 notes mises à la corbeille');
    expect(mockLibrary().trash.size).toBe(2);
  });

  it('reports a failure the way the bar did', async () => {
    vi.spyOn(api, 'trashNotes').mockRejectedValueOnce({ code: 'notFound', message: 'x' });
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Liste de courses', 'Mettre à la corbeille');
    expect(view.q('.banner.error')).not.toBeNull();
    expect(view.titles().length).toBe(3);
  });

  it('is the same from the Menu key and from the « ⋯ » button of the card', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    const card = view.card('Liste de courses');
    // The Menu key: a contextmenu event with no button and no position.
    card.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 0 }));
    await view.settle();
    expect(heading()).toBe('2 notes sélectionnées');
    expect(cardMenuLabels()).toContain('Assembler…');
    await view.key(document.activeElement!, 'Escape');
    expect(document.querySelector('[role="menu"]')).toBeNull();
    await view.click(card.querySelector('.card-menu button'));
    expect(heading()).toBe('2 notes sélectionnées');
    expect(document.querySelectorAll('[role="menu"]').length).toBe(1);
  });

  it('leaves the « ⋯ » button named after its note once the menu of the selection is closed', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.openMenu('Liste de courses');
    await view.key(document.activeElement!, 'Escape');
    expect(view.card('Liste de courses').querySelector('.card-menu button')!.getAttribute('aria-label')).toBe('Actions pour Liste de courses');
  });

  it('says the count in English', async () => {
    const view = await ready('en');
    await view.select('Réunion parents', 'Liste de courses');
    await view.openMenu('Liste de courses');
    expect(heading()).toBe('2 notes selected');
    expect(cardMenuLabels()).toContain('Deselect');
    expect(cardMenuLabels()).toContain('Assemble…');
  });
});

describe('the menu of a card outside the selection', () => {
  it('acts on that note alone and leaves the selection as it is', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.openMenu('Cours de physique');
    expect(heading()).toBeNull();
    expect(menu().getAttribute('aria-label')).toBe('Actions pour Cours de physique');
    // The full list for one note, and a way out of the selection that exists elsewhere.
    expect(cardMenuLabels()).toEqual([...FULL_ONE.slice(0, -1), 'Désélectionner', 'Mettre à la corbeille']);
    await view.click(cardMenuEntry('Ajouter aux favoris'));
    expect(favoriteOf('Cours de physique')).toBe(true);
    expect(favoriteOf('Réunion parents')).toBeFalsy();
    expect(view.all('.note-card.selected').map((el) => el.querySelector('.card-title')?.textContent?.trim())).toEqual([
      'Réunion parents',
      'Liste de courses',
    ]);
  });

  it('trashes only that note', async () => {
    const view = await ready();
    await view.select('Réunion parents', 'Liste de courses');
    await view.choose('Cours de physique', 'Mettre à la corbeille');
    await vi.waitFor(() => expect(view.titles()).toEqual(['Réunion parents', 'Liste de courses']));
    expect(view.q('.selection-count')?.textContent).toBe('2 notes sélectionnées');
  });
});

describe('what stays above the grid', () => {
  it('is a count, polite for screen readers, and no button', async () => {
    const view = await ready();
    const count = view.q('.selection-count')!;
    expect(count.textContent).toBe('');
    await view.select('Réunion parents', 'Liste de courses', 'Cours de physique');
    expect(count.textContent).toBe('3 notes sélectionnées');
    expect(count.getAttribute('aria-live')).toBe('polite');
    expect(count.closest('.library-section')!.querySelectorAll('.section-header button').length).toBe(0);
    await view.key(document.body, 'Escape');
    expect(count.textContent).toBe('');
  });

  it('keeps the keyboard shortcuts of the selection: Delete, Ctrl+A and Escape', async () => {
    const view = await ready();
    await view.key(document.body, 'a', { ctrlKey: true });
    expect(view.all('.note-card.selected').length).toBe(3);
    await view.key(view.card('Réunion parents').querySelector('.card-open'), 'Delete');
    await vi.waitFor(() => expect(view.titles()).toEqual([]));
    expect(mockLibrary().trash.size).toBe(3);
  });
});
