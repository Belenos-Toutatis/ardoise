// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { fileName, freshStamp, mockLibrary, notePath, notifyLibraryChanged, resetMockLibrary } from '../../lib/mock-data';
import { mockProblems } from '../../lib/mock-library';
import Home from '../Home.svelte';

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: 'C:\\demo\\Ardoise',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
};

const ROOT = 'C:\\demo\\Ardoise';
let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  mockProblems.length = 0;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
});

/** Mounts the main window over the demo library and waits for the three demo notes. */
async function render(locale: 'fr' | 'en' = 'fr', expected = 3) {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app, onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const all = <T extends Element = HTMLElement>(selector: string) => [...target.querySelectorAll<T>(selector)];
  const titles = () => all('.note-card .card-title').map((el) => el.textContent?.trim());
  const card = (title: string) => {
    const found = all('.note-card').find((el) => el.querySelector('.card-title')?.textContent?.trim() === title);
    if (!found) throw new Error(`no card "${title}" among ${titles().join(', ')}`);
    return found;
  };
  const open = (title: string) => card(title).querySelector<HTMLButtonElement>('.card-open')!;
  const byLabel = (label: string, within: ParentNode = target) => {
    const found = [...within.querySelectorAll<HTMLElement>('[aria-label]')].find((el) => el.getAttribute('aria-label') === label);
    if (!found) throw new Error(`no element labelled "${label}"`);
    return found;
  };
  const button = (text: string, within: ParentNode = target) => {
    const found = [...within.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  const click = async (el: Element, init: MouseEventInit = {}) => {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
    await settle();
  };
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await Promise.resolve();
      await tick();
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  };
  const type = async (input: HTMLInputElement, value: string) => {
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  const key = async (el: Element, name: string, init: KeyboardEventInit = {}) => {
    el.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));
    await settle();
  };
  const view = async (selector: string) => click(q(selector)!);
  const drag = (type: string, dataTransfer = new DataTransferStub()) => {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'dataTransfer', { value: dataTransfer });
    return event;
  };
  await vi.waitFor(() => expect(q('.library-section .loading')).toBeNull());
  await vi.waitFor(() => expect(all('.note-card').length).toBe(expected));
  return { target, q, all, titles, card, open, byLabel, button, click, settle, type, key, view, drag };
}

class DataTransferStub {
  data = new Map<string, string>();
  effectAllowed = 'none';
  dropEffect = 'none';
  setData(type: string, value: string) {
    this.data.set(type, value);
  }
}

const notesIn = (binder: string) =>
  [...mockLibrary().notes.values()].filter((s) => s.binder === binder).map((s) => s.note.title);

describe('library: listing and sidebar', () => {
  it('shows the demo notes, newest first, with title, date and a thumbnail', async () => {
    const ui = await render();
    expect(ui.titles()).toEqual(['Réunion parents', 'Liste de courses', 'Cours de physique']);
    await vi.waitFor(() => expect(ui.all('.note-card img').length).toBe(3));
    expect(ui.q('.note-card img')?.getAttribute('src')).toMatch(/^data:image\/svg\+xml/);
    expect(ui.card('Réunion parents').querySelector('.card-date')?.textContent).toMatch(/2026/);
  });

  it('shows the date for an untitled note', async () => {
    const ui = await render();
    await api.createNote('', '');
    await vi.waitFor(() => expect(ui.all('.note-card').length).toBe(4));
    const titles = ui.titles();
    expect(titles.filter((title) => title === '')).toEqual([]);
    expect(titles[0]).toMatch(/\d{4}/);
  });

  it('filters by favourites, binder (with sub-binders) and tag, and shows counts', async () => {
    const ui = await render();
    expect(ui.q('[data-view="all"] .count')?.textContent).toBe('3');
    expect(ui.q('[data-view="favorites"] .count')?.textContent).toBe('1');

    await ui.view('[data-view="favorites"]');
    expect(ui.titles()).toEqual(['Liste de courses']);

    // The tree is collapsed: expand "Classe" to reach its sub-binder.
    expect(ui.q('[data-binder="Classe/3e B"]')).toBeNull();
    await ui.click(ui.byLabel('Déplier Classe'));
    expect(ui.q('[data-binder="Classe/3e B"] .count')?.textContent).toBe('1');
    await ui.view('[data-binder="Classe/3e B"]');
    expect(ui.titles()).toEqual(['Cours de physique']);
    expect(ui.q('[data-binder="Classe/3e B"]')?.getAttribute('aria-current')).toBe('true');

    await ui.view('[data-binder="Classe"]');
    expect(ui.titles()).toEqual(['Cours de physique']);

    await ui.view('[data-tag="réunion"]');
    expect(ui.titles()).toEqual(['Réunion parents']);
    expect(ui.q('[data-tag="réunion"] .count')?.textContent).toBe('1');

    await ui.view('[data-view="all"]');
    expect(ui.titles().length).toBe(3);
  });

  it('explains empty views', async () => {
    const ui = await render();
    mockLibrary().binders.add('Vide');
    notifyLibraryChanged();
    await ui.settle();
    await ui.view('[data-binder="Vide"]');
    expect(ui.target.textContent).toContain('Ce classeur est vide');
    await ui.view('[data-view="trash"]');
    expect(ui.target.textContent).toContain('La corbeille est vide.');
  });

  it('opens the tablet test from the sidebar', async () => {
    const ui = await render();
    await ui.click(ui.button('Tester la tablette'));
    expect(ui.q('.panel')?.getAttribute('aria-label')).toBe('Test de la tablette');
    expect(ui.q('[data-slot="tablet-status"]')?.textContent).toContain('Bamboo Slate');
  });

  it('creates a binder, and a sub-binder, from the sidebar', async () => {
    const ui = await render();
    await ui.click(ui.byLabel('Nouveau classeur'));
    const input = ui.q<HTMLInputElement>('.binder-form input')!;
    await ui.type(input, 'Archives');
    await ui.click(ui.button('Créer'));
    expect(mockLibrary().binders.has('Archives')).toBe(true);
    expect(ui.q('[data-binder="Archives"]')).toBeTruthy();

    await ui.click(ui.byLabel('Nouveau sous-classeur dans Archives'));
    await ui.type(ui.q<HTMLInputElement>('.binder-form input')!, '2026');
    await ui.click(ui.button('Créer'));
    expect(mockLibrary().binders.has('Archives/2026')).toBe(true);
  });

  it('refuses a binder name the library would not accept', async () => {
    const ui = await render();
    await ui.click(ui.byLabel('Nouveau classeur'));
    await ui.type(ui.q<HTMLInputElement>('.binder-form input')!, 'a:b');
    await ui.click(ui.button('Créer'));
    expect(ui.q('[role="alert"]')?.textContent).toContain("ce nom n'est pas utilisable");
    expect(mockLibrary().binders.has('a:b')).toBe(false);
  });
});

describe('library: search and sort', () => {
  it('finds "Réunion" with "reunion", whatever the case, in French', async () => {
    const ui = await render('fr');
    const search = ui.q<HTMLInputElement>('input[type="search"]')!;
    await ui.type(search, 'reunion');
    expect(ui.titles()).toEqual(['Réunion parents']);
    await ui.type(search, 'RÉUNION');
    expect(ui.titles()).toEqual(['Réunion parents']);
    await ui.type(search, 'physique');
    expect(ui.titles()).toEqual(['Cours de physique']);
    await ui.type(search, 'nothing');
    expect(ui.titles()).toEqual([]);
    expect(ui.target.textContent).toContain('Aucune note ne correspond à « nothing ».');
    await ui.type(search, '');
    expect(ui.titles().length).toBe(3);
  });

  it('finds the same notes with the English interface', async () => {
    const ui = await render('en');
    const search = ui.q<HTMLInputElement>('input[type="search"]')!;
    expect(search.getAttribute('aria-label')).toBe('Search notes');
    await ui.type(search, 'reunion');
    expect(ui.titles()).toEqual(['Réunion parents']);
    await ui.type(search, '3e b');
    expect(ui.titles()).toEqual(['Cours de physique']);
    await ui.type(search, 'zzz');
    expect(ui.target.textContent).toContain('No note matches “zzz”.');
  });

  it('searches within the current view', async () => {
    const ui = await render();
    await ui.view('[data-view="favorites"]');
    await ui.type(ui.q<HTMLInputElement>('input[type="search"]')!, 'reunion');
    expect(ui.titles()).toEqual([]);
  });

  it('sorts by date or title, ascending or descending', async () => {
    const ui = await render();
    const sort = ui.q<HTMLSelectElement>('select.sort')!;
    const choose = async (value: string) => {
      sort.value = value;
      sort.dispatchEvent(new Event('change', { bubbles: true }));
      await ui.settle();
    };
    await choose('date-asc');
    expect(ui.titles()).toEqual(['Cours de physique', 'Liste de courses', 'Réunion parents']);
    await choose('title-asc');
    expect(ui.titles()).toEqual(['Cours de physique', 'Liste de courses', 'Réunion parents']);
    await choose('title-desc');
    expect(ui.titles()).toEqual(['Réunion parents', 'Liste de courses', 'Cours de physique']);
    await choose('date-desc');
    expect(ui.titles()).toEqual(['Réunion parents', 'Liste de courses', 'Cours de physique']);
    expect([...sort.options].map((o) => o.textContent)).toContain('Titre : A → Z');
  });
});

describe('library: selection, moving, trash', () => {
  it('selects with Ctrl- and Shift-click and shows the selection bar', async () => {
    const ui = await render();
    expect(ui.q('.selection-bar')).toBeNull();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    expect(ui.q('.selection-count')?.textContent).toBe('Sélection : 1');
    // The bar says how to change the selection.
    expect(ui.q('.selection-help')?.textContent).toBe(
      'Ctrl+clic : ajouter ou retirer une note · Maj+clic : sélectionner une plage · Échap : tout désélectionner',
    );
    await ui.click(ui.open('Cours de physique'), { shiftKey: true });
    expect(ui.q('.selection-count')?.textContent).toBe('Sélection : 3');
    expect(ui.all('.note-card.selected').length).toBe(3);
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    expect(ui.q('.selection-count')?.textContent).toBe('Sélection : 2');
    await ui.click(ui.button('Désélectionner'));
    expect(ui.q('.selection-bar')).toBeNull();
  });

  it('selects and navigates with the keyboard', async () => {
    const ui = await render();
    const first = ui.open('Réunion parents');
    first.focus();
    await ui.key(first, ' ');
    expect(ui.q('.selection-count')?.textContent).toBe('Sélection : 1');
    await ui.key(first, 'ArrowRight');
    expect(document.activeElement).toBe(ui.open('Liste de courses'));
    await ui.key(document.activeElement!, 'ArrowRight', { shiftKey: true });
    expect(document.activeElement).toBe(ui.open('Cours de physique'));
    // The range runs from the note selected with Space (the anchor) to the focused one.
    expect(ui.all('.note-card.selected').length).toBe(3);
    await ui.key(document.activeElement!, 'Home');
    expect(document.activeElement).toBe(first);
    await ui.key(first, 'a', { ctrlKey: true });
    expect(ui.all('.note-card.selected').length).toBe(3);
    await ui.key(first, 'Escape');
    expect(ui.q('.selection-bar')).toBeNull();
  });

  it('moves the selection to a binder with the picker', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Déplacer vers…'));
    await ui.click(ui.q('.selection-panel [data-binder="Classe"]')!);
    expect(notesIn('Classe').sort()).toEqual(['Liste de courses', 'Réunion parents']);
    expect(ui.q('.selection-bar')).toBeNull();
  });

  it('moves several notes to the trash, then restores one from the trash view', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Cours de physique'), { ctrlKey: true });
    await ui.click(ui.button('Mettre à la corbeille'));
    expect(ui.titles()).toEqual(['Liste de courses']);
    expect(ui.q('[data-view="trash"] .count')?.textContent).toBe('2');

    await ui.view('[data-view="trash"]');
    const rows = ui.all('.trash-table tbody tr');
    expect(rows.map((row) => row.querySelector('th')?.textContent)).toEqual(
      expect.arrayContaining(['Réunion parents', 'Cours de physique']),
    );
    const physics = rows.find((row) => row.textContent?.includes('Cours de physique'))!;
    expect(physics.textContent).toContain('Classe/3e B');
    await ui.click(ui.button('Restaurer', physics));
    expect(ui.all('.trash-table tbody tr').length).toBe(1);
    expect(notesIn('Classe/3e B')).toEqual(['Cours de physique']);

    await ui.view('[data-view="all"]');
    expect(ui.titles()).toEqual(['Liste de courses', 'Cours de physique']);
  });

  it('deletes a trashed note for good only after a confirmation', async () => {
    const ui = await render();
    await api.trashNotes([notePath(ROOT, '', fileName(mockLibrary().notes.get(notePath(ROOT, '', '2026-10-22 Liste de courses.ardoise'))!.note))]);
    await ui.settle();
    await ui.view('[data-view="trash"]');
    await ui.click(ui.button('Supprimer définitivement'));
    expect(mockLibrary().trash.size).toBe(1);
    expect(ui.target.textContent).toContain('Supprimer définitivement « Liste de courses » ?');
    await ui.click(ui.button('Annuler'));
    expect(mockLibrary().trash.size).toBe(1);
    await ui.click(ui.button('Supprimer définitivement'));
    await ui.click(ui.button('Confirmer'));
    expect(mockLibrary().trash.size).toBe(0);
  });

  it('empties the trash after a confirmation', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Mettre à la corbeille'));
    await ui.view('[data-view="trash"]');

    await ui.click(ui.button('Vider la corbeille'));
    expect(ui.q('[role="alertdialog"]')?.textContent).toContain('Supprimer définitivement les 2 notes');
    expect(mockLibrary().trash.size).toBe(2);
    await ui.click(ui.button('Annuler'));
    expect(ui.q('[role="alertdialog"]')).toBeNull();
    expect(mockLibrary().trash.size).toBe(2);

    await ui.click(ui.button('Vider la corbeille'));
    await ui.click(ui.button('Confirmer'));
    expect(mockLibrary().trash.size).toBe(0);
    expect(ui.target.textContent).toContain('La corbeille est vide.');
  });

  it('empties only the notes the confirmation showed', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.button('Mettre à la corbeille'));
    await ui.view('[data-view="trash"]');
    await ui.click(ui.button('Vider la corbeille'));
    expect(ui.q('[role="alertdialog"]')?.textContent).toContain('Supprimer définitivement');

    // Another PC trashes a note while the question is on screen.
    await api.trashNotes([notePath(ROOT, '', fileName(mockLibrary().notes.get(notePath(ROOT, '', '2026-10-22 Liste de courses.ardoise'))!.note))]);
    await ui.click(ui.button('Confirmer'));
    expect(mockLibrary().trash.size).toBe(1);
  });

  it('trashes the focused note with Delete', async () => {
    const ui = await render();
    const card = ui.open('Liste de courses');
    card.focus();
    await ui.key(card, 'Delete');
    expect(ui.titles()).toEqual(['Réunion parents', 'Cours de physique']);
  });
});

describe('library: drag and drop', () => {
  it('moves a dragged note onto a binder of the sidebar', async () => {
    const ui = await render();
    await ui.click(ui.byLabel('Déplier Classe'));
    const target = ui.q('[data-binder="Classe/3e B"]')!;
    const dataTransfer = new DataTransferStub();

    ui.open('Liste de courses').dispatchEvent(ui.drag('dragstart', dataTransfer));
    await ui.settle();
    expect(dataTransfer.data.get('text/plain')).toContain('Liste de courses');

    const over = ui.drag('dragover', dataTransfer);
    target.dispatchEvent(over);
    await ui.settle();
    expect(over.defaultPrevented).toBe(true);
    expect(target.classList.contains('drop')).toBe(true);

    target.dispatchEvent(ui.drag('drop', dataTransfer));
    await ui.settle();
    expect(notesIn('Classe/3e B').sort()).toEqual(['Cours de physique', 'Liste de courses']);
    expect(ui.q('[data-binder="Classe/3e B"] .count')?.textContent).toBe('2');
  });

  it('drags the whole selection when the dragged card is part of it', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    ui.open('Liste de courses').dispatchEvent(ui.drag('dragstart'));
    await ui.settle();
    ui.q('[data-binder="Classe"]')!.dispatchEvent(ui.drag('drop'));
    await ui.settle();
    expect(notesIn('Classe').sort()).toEqual(['Liste de courses', 'Réunion parents']);
  });

  it('drops onto "All notes" to move notes back to the root', async () => {
    const ui = await render();
    ui.open('Réunion parents').dispatchEvent(ui.drag('dragstart'));
    await ui.settle();
    ui.q('[data-view="all"]')!.dispatchEvent(ui.drag('drop'));
    await ui.settle();
    expect(notesIn('').sort()).toEqual(['Liste de courses', 'Réunion parents']);
  });

  it('ignores a drop when nothing is being dragged', async () => {
    const ui = await render();
    const over = ui.drag('dragover');
    ui.q('[data-binder="Perso"]')!.dispatchEvent(over);
    expect(over.defaultPrevented).toBe(false);
  });
});

describe('library: favourites, tags, rename, conflicts', () => {
  it('toggles the favourite star', async () => {
    const ui = await render();
    const star = () => ui.byLabel('Ajouter aux favoris', ui.card('Cours de physique'));
    await ui.click(star());
    const stored = [...mockLibrary().notes.values()].find((s) => s.note.title === 'Cours de physique')!;
    expect(stored.note.favorite).toBe(true);
    expect(ui.q('[data-view="favorites"] .count')?.textContent).toBe('2');
    await ui.click(ui.byLabel('Retirer des favoris', ui.card('Cours de physique')));
    expect(stored.note.favorite).toBe(false);
  });

  it('adds and removes tags on the selection', async () => {
    const ui = await render();
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.open('Cours de physique'), { ctrlKey: true });
    await ui.click(ui.button('Étiquettes…'));
    await ui.type(ui.byLabel('Ajouter une étiquette') as HTMLInputElement, 'urgent, maison');
    await ui.click(ui.button('Ajouter'));
    const tagsOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)!.note.tags;
    expect(tagsOf('Liste de courses')).toEqual(['urgent', 'maison']);
    expect(tagsOf('Cours de physique')).toEqual(['physique', '3e B', 'urgent', 'maison']);
    expect(ui.q('[data-tag="urgent"] .count')?.textContent).toBe('2');

    await ui.click(ui.byLabel("Retirer l'étiquette urgent"));
    expect(tagsOf('Liste de courses')).toEqual(['maison']);
    expect(ui.q('[data-tag="urgent"]')).toBeNull();
  });

  it('puts the selection in favourites then takes it out', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Cours de physique'), { ctrlKey: true });
    await ui.click(ui.button('Mettre en favoris'));
    expect([...mockLibrary().notes.values()].every((s) => s.note.favorite)).toBe(true);
    await ui.click(ui.button('Retirer des favoris', ui.q('.selection-bar')!));
    // Only the two selected notes were taken out; the one that was already a favourite stays.
    expect([...mockLibrary().notes.values()].filter((s) => s.note.favorite).map((s) => s.note.title)).toEqual(['Liste de courses']);
  });

  it('renames a note from its menu', async () => {
    const ui = await render();
    await ui.click(ui.byLabel('Actions pour Liste de courses'));
    await ui.click(ui.button('Renommer'));
    const input = ui.q<HTMLInputElement>('.rename-input')!;
    expect(input.value).toBe('Liste de courses');
    await ui.type(input, 'Courses de la semaine');
    await ui.key(input, 'Enter');
    expect(ui.titles()).toContain('Courses de la semaine');
    expect(ui.titles()).not.toContain('Liste de courses');
    expect([...mockLibrary().notes.keys()].some((p) => p.endsWith('2026-10-22 Courses de la semaine.ardoise'))).toBe(true);
  });

  it('cancels a rename with Escape', async () => {
    const ui = await render();
    await ui.click(ui.byLabel('Actions pour Liste de courses'));
    await ui.click(ui.button('Renommer'));
    const input = ui.q<HTMLInputElement>('.rename-input')!;
    await ui.type(input, 'Autre');
    await ui.key(input, 'Escape');
    expect(ui.q('.rename-input')).toBeNull();
    expect(ui.titles()).toContain('Liste de courses');
  });

  it('reports a note that changed elsewhere instead of overwriting it', async () => {
    const ui = await render();
    // Another computer modified the note after the last listing.
    const stored = [...mockLibrary().notes.values()].find((s) => s.note.title === 'Cours de physique')!;
    stored.stamp = freshStamp();
    await ui.click(ui.byLabel('Ajouter aux favoris', ui.card('Cours de physique')));
    expect(ui.q('[role="alert"]')?.textContent).toContain('modifiée entre-temps');
    expect(stored.note.favorite).toBeFalsy();
    await ui.click(ui.button('Fermer le message'));
    expect(ui.q('[role="alert"]')).toBeNull();
    // The listing was refreshed: trying again now works.
    await ui.click(ui.byLabel('Ajouter aux favoris', ui.card('Cours de physique')));
    expect(stored.note.favorite).toBe(true);
  });

  it('flags a conflict copy', async () => {
    const ui = await render();
    const original = [...mockLibrary().notes].find(([, s]) => s.note.title === 'Liste de courses')!;
    const copy = structuredClone(original[1]);
    copy.note.id = 'conflict-id';
    mockLibrary().notes.set(notePath(ROOT, '', '2026-10-22 Liste de courses (conflicted copy 2026-10-23).ardoise'), copy);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(ui.all('.note-card').length).toBe(4));
    const badges = ui.all('.conflict-badge');
    expect(badges.length).toBe(1);
    expect(badges[0].textContent?.trim()).toBe('Copie en conflit');
    expect(badges[0].getAttribute('title')).toContain('2026-10-22 Liste de courses.ardoise');
  });

  it('lists unreadable files discreetly', async () => {
    mockProblems.push({ path: 'C:\\demo\\Ardoise\\cassée.ardoise', message: 'fichier endommagé' });
    const ui = await render();
    const details = ui.q<HTMLDetailsElement>('details.problems')!;
    expect(details.querySelector('summary')?.textContent).toBe('Fichiers illisibles : 1');
    expect(details.open).toBe(false);
    expect(details.textContent).toContain('cassée.ardoise');
  });

  it('shows the tag of a card and filters by it', async () => {
    const ui = await render();
    await ui.click(ui.byLabel("Afficher les notes de l'étiquette réunion"));
    expect(ui.titles()).toEqual(['Réunion parents']);
  });
});

describe('library: opening notes and live refresh', () => {
  it('opens the editor full-space when a card is clicked, and comes back', async () => {
    const ui = await render();
    await ui.click(ui.open('Cours de physique'));
    const editor = ui.q('.editor')!;
    expect(editor.getAttribute('data-path')).toBe(notePath(ROOT, 'Classe/3e B', '2026-10-20 Cours de physique.ardoise'));
    expect(ui.q('.topbar')).toBeNull();
    await ui.click(ui.q('.editor-top button')!);
    expect(ui.q('.editor')).toBeNull();
    expect(ui.all('.note-card').length).toBe(3);
  });

  it('opens a note with Enter', async () => {
    const ui = await render();
    await ui.key(ui.open('Liste de courses'), 'Enter');
    expect(ui.q('.editor')?.getAttribute('data-path')).toMatch(/Liste de courses/);
  });

  it('creates a note in the current binder and opens it', async () => {
    const ui = await render();
    await ui.view('[data-binder="Perso"]');
    await ui.click(ui.button('Nouvelle note'));
    const path = ui.q('.editor')?.getAttribute('data-path') ?? '';
    expect(path.startsWith(`${ROOT}\\Perso\\`)).toBe(true);
    expect(notesIn('Perso').length).toBe(2);
  });

  it('shows the empty library state with a way to create a note', async () => {
    resetMockLibrary();
    mockLibrary().notes.clear();
    mockLibrary().binders.clear();
    const ui = await render('fr', 0);
    expect(ui.target.textContent).toContain('Aucune note pour l\'instant.');
    await ui.click(ui.q('.empty-state .button')!);
    expect(ui.q('.editor')).toBeTruthy();
  });

  it('refreshes when the library changes on disk', async () => {
    const ui = await render();
    const stored = [...mockLibrary().notes.values()][0];
    const copy = structuredClone(stored);
    copy.note.id = 'from-another-pc';
    copy.note.title = 'Arrivée depuis un autre PC';
    mockLibrary().notes.set(notePath(ROOT, '', '2026-10-24 Arrivée.ardoise'), copy);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(ui.titles()).toContain('Arrivée depuis un autre PC'));
  });

  it('opens the settings panel from the top bar', async () => {
    const ui = await render();
    await ui.click(ui.button('Réglages'));
    expect(ui.q('.panel')?.getAttribute('aria-label')).toBe('Réglages');
  });
});

describe('library: export and assemble the selection', () => {
  beforeEach(() => {
    try {
      localStorage.clear();
    } catch {
      // No storage: nothing to clear.
    }
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const pathOf = (title: string) => {
    const path = [...mockLibrary().notes.entries()].find(([, s]) => s.note.title === title)?.[0];
    if (!path) throw new Error(`no note "${title}"`);
    return path;
  };

  it('exports the selected notes into one PDF, a page per note in the displayed order', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Cours de physique'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Exporter…'));
    const dialog = ui.q('[role="dialog"]')!;
    expect(dialog.textContent).toContain('Notes sélectionnées : 3');

    [...dialog.querySelectorAll<HTMLLabelElement>('label.choice')]
      .find((l) => (l.querySelector('.choice-name') ?? l).textContent?.trim() === 'PDF')!
      .querySelector('input')!
      .click();
    await ui.settle();
    await ui.click(ui.button('Exporter…', dialog));
    // The grid shows newest first: that is the page order, whatever the order of the clicks.
    expect(spy).toHaveBeenCalledWith(
      ['Réunion parents', 'Liste de courses', 'Cours de physique'].map((title) => ({ path: pathOf(title) })),
      'pdf',
      expect.objectContaining({ pdfPage: 'actual' }),
      'C:\\Users\\demo\\Exports\\Notes.pdf',
    );
    expect(dialog.textContent).toContain('Fichiers créés : 1');
    await ui.click(ui.button('Fermer', dialog));
    expect(ui.q('[role="dialog"]')).toBeNull();
    // The selection is still there.
    expect(ui.q('.selection-count')?.textContent).toBe('Sélection : 3');
  });

  it('exports one selected note to a file named after it', async () => {
    const spy = vi.spyOn(api, 'exportNotes');
    const ui = await render();
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Exporter…'));
    const dialog = ui.q('[role="dialog"]')!;
    expect(dialog.textContent).toContain('Note : Liste de courses');
    await ui.click(ui.button('Exporter…', dialog));
    expect(spy.mock.calls[0][0]).toEqual([{ path: pathOf('Liste de courses') }]);
    expect(spy.mock.calls[0][3]).toBe('C:\\Users\\demo\\Exports\\2026-10-22 Liste de courses.png');
  });

  it('closes the export dialog with Escape', async () => {
    const ui = await render();
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Exporter…'));
    await ui.key(ui.q('[role="dialog"]')!, 'Escape');
    expect(ui.q('[role="dialog"]')).toBeNull();
  });

  it('offers to assemble only from two notes', async () => {
    const ui = await render();
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    expect(ui.button('Assembler…').disabled).toBe(true);
    expect(ui.button('Assembler…').title).toBe('Sélectionnez au moins deux notes pour les assembler.');
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    expect(ui.button('Assembler…').disabled).toBe(false);
  });

  it('assembles the oldest note first, end to end, and selects the new note', async () => {
    const spy = vi.spyOn(api, 'mergeNotes');
    const ui = await render();
    // Clicked newest first; the assembly goes oldest first.
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Assembler…'));
    const dialog = ui.q('[role="dialog"]')!;
    expect(dialog.textContent).toContain('Notes à assembler : 2');
    const title = dialog.querySelector<HTMLInputElement>('input[type="text"]')!;
    expect(title.value).toBe('Liste de courses (assemblage)');
    expect(dialog.querySelector<HTMLInputElement>('input[value="overlay"]')!.checked).toBe(true);

    dialog.querySelector<HTMLInputElement>('input[value="endToEnd"]')!.click();
    await ui.type(title, 'Deux pages');
    await ui.click(ui.button('Assembler', dialog));

    expect(spy).toHaveBeenCalledWith([pathOf('Liste de courses'), pathOf('Réunion parents')], 'endToEnd', 'Deux pages');
    expect(ui.q('[role="dialog"]')).toBeNull();
    await vi.waitFor(() => expect(ui.titles()).toContain('Deux pages'));
    // The new note is the selection, and a confirmation says so.
    expect(ui.q('.selection-count')?.textContent).toBe('Sélection : 1');
    expect(ui.card('Deux pages').classList.contains('selected')).toBe(true);
    expect(ui.q('.notice')?.textContent).toContain('Nouvelle note créée : Deux pages');
    // It lives in the first note's binder, and holds both notes' strokes on a long page.
    const created = mockLibrary().notes.get(pathOf('Deux pages'))!;
    expect(created.binder).toBe('');
    expect(created.note.strokes).toHaveLength(2);
    expect(created.note.page.height).toBe(43200);
    // The sources are untouched.
    expect(notesIn('Perso')).toEqual(['Réunion parents']);

    await ui.click(ui.button('Fermer le message', ui.q('.notice')!));
    expect(ui.q('.notice')).toBeNull();
  });

  it('overlays by default', async () => {
    const spy = vi.spyOn(api, 'mergeNotes');
    const ui = await render();
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.open('Cours de physique'), { ctrlKey: true });
    await ui.click(ui.button('Assembler…'));
    const dialog = ui.q('[role="dialog"]')!;
    expect(dialog.querySelector<HTMLInputElement>('input[type="text"]')!.value).toBe('Cours de physique (assemblage)');
    await ui.click(ui.button('Assembler', dialog));
    expect(spy.mock.calls[0][1]).toBe('overlay');
    expect(spy.mock.calls[0][0]).toEqual([pathOf('Cours de physique'), pathOf('Liste de courses')]);
    const created = mockLibrary().notes.get(pathOf('Cours de physique (assemblage)'))!.note;
    expect(created.page).toEqual({ width: 29700, height: 21600 });
    expect(created.layers.map((l) => l.name)).toEqual(['Cours de physique', 'Liste de courses']);
  });

  it('bases the proposed title on the date when the oldest note has no title, without a colon', async () => {
    const ui = await render();
    const untitled = await api.createNote('', '');
    mockLibrary().notes.get(untitled.path)!.note.created = Date.UTC(2020, 0, 5, 10, 30);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(ui.all('.note-card').length).toBe(4));
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.all<HTMLElement>('.note-card .card-open').at(-1)!, { ctrlKey: true });
    await ui.click(ui.button('Assembler…'));
    const value = ui.q('[role="dialog"]')!.querySelector<HTMLInputElement>('input[type="text"]')!.value;
    expect(value).toMatch(/2020 \(assemblage\)$/);
    expect(value).not.toContain(':');
  });

  it('keeps the dialog open and explains a refusal', async () => {
    vi.spyOn(api, 'mergeNotes').mockRejectedValueOnce({ code: 'pageTooLarge', message: '1 × 2' });
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    await ui.click(ui.button('Assembler…'));
    const dialog = ui.q('[role="dialog"]')!;
    await ui.click(ui.button('Assembler', dialog));
    expect(dialog.querySelector('[role="alert"]')?.textContent).toContain("Impossible d'assembler les notes : la page obtenue serait trop grande");
    expect(ui.q('[role="dialog"]')).toBeTruthy();
    expect(ui.button('Assembler', dialog).disabled).toBe(false);
    await ui.click(ui.button('Annuler', dialog));
    expect(ui.q('[role="dialog"]')).toBeNull();
    expect(ui.titles().length).toBe(3);
  });

  it('shows the export and assemble actions in English', async () => {
    const ui = await render('en');
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    expect(ui.button('Export…')).toBeTruthy();
    await ui.click(ui.button('Assemble…'));
    const dialog = ui.q('[role="dialog"]')!;
    expect(dialog.querySelector('h2')?.textContent).toBe('Assemble notes');
    expect(dialog.textContent).toContain('Overlay on one page');
    expect(dialog.textContent).toContain('Place end to end (long page)');
    expect(dialog.querySelector<HTMLInputElement>('input[type="text"]')!.value).toBe('Liste de courses (assembled)');
  });
});
