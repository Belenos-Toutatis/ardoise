// @vitest-environment jsdom
/// <reference types="node" />
// The summary of a binder (SPEC §4.3 « Sommaire du classeur », Q20 item 5): the bookmarks of its notes in the binder's
// order (its free order when it has one; the sub-binders after), a click opens the note at that bookmark, the library's
// search finds a note by its bookmarks, in French and in English, and the dialog is usable with the keyboard.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { fileName, freshStamp, mockLibrary, notePath, resetMockLibrary, type StoredNote } from '../../lib/mock-data';
import type { Note, PageObject } from '../../lib/note';
import { ROOT, renderHome, type Ui } from './library-test-helpers';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('../editor/VideoExportDialogDouble.svelte')).default,
}));

// The real geometry and text layout, from the module's bytes (jsdom cannot fetch it): the editor opens a note.
vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

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

const DAY = 86_400_000;
const T0 = Date.UTC(2026, 9, 1, 9, 0);

/** A bookmark as `note.json` writes it. */
function mark(id: string, label: string, x: number, y: number, layer = 0): PageObject {
  return { kind: 'bookmark', id, layer, t: 0, x, y, scale: 1, quarterTurns: 0, width: 3000, height: 925, label };
}

/** Adds a note to the demo library with these bookmarks; returns its path. */
function addNote(binder: string, title: string, created: number, objects: PageObject[] = []): string {
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
  if (objects.length > 0) note.objects = objects;
  if (binder) library.binders.add(binder);
  const path = notePath(ROOT, binder, fileName(note));
  library.notes.set(path, { note, stamp: freshStamp(), binder } satisfies StoredNote);
  return path;
}

/**
 * `Classe` holds Alpha (two bookmarks, the lower one written first), Beta (none), Gamma (one, on a hidden layer's index),
 * and its sub-binder `3e B` holds the demo's « Cours de physique », given a bookmark.
 */
function seedClasse(): void {
  addNote('Classe', 'Alpha', T0, [mark('a-low', 'Bilan du chapitre', 1000, 9000), mark('a-high', 'Introduction', 1000, 1000)]);
  addNote('Classe', 'Beta', T0 + DAY);
  addNote('Classe', 'Gamma', T0 + 2 * DAY, [mark('g-1', 'Corrigé', 2000, 2000, 1)]);
  const physics = [...mockLibrary().notes.values()].find((stored) => stored.note.title === 'Cours de physique')!;
  physics.note.objects = [mark('p-1', 'Loi d\'Ohm', 500, 500)];
}

async function render(locale: 'fr' | 'en' = 'fr', cards = 6): Promise<Ui> {
  ui = await renderHome(locale, cards);
  return ui;
}

/** Shows the binder `Classe` and opens its summary with the button of the library's top bar. */
async function openSummary(view: Ui) {
  await view.click(view.q('[data-binder="Classe"]'));
  await view.click(view.q('[data-action="binder-summary"]'));
  return view.q('[data-dialog="binder-summary"]')!;
}

/** The summary as it reads: one line per row, `#` before a sub-binder's name, `⚑` before a bookmark, a dot per level. */
function lines(dialog: Element): string[] {
  return [...dialog.querySelectorAll<HTMLElement>('[data-row]')].map((el) => {
    const mark = el.dataset.row === 'binder' ? '#' : el.dataset.row === 'bookmark' ? '⚑' : '';
    const label = (el.querySelector('.summary-label') ?? el).textContent?.trim();
    return `${'.'.repeat(Number(el.style.getPropertyValue('--level')))}${mark}${label}`;
  });
}

/** Chooses the notes mode (« Titre », « Titre ou date », « Non ») or toggles a box of « Dans le sommaire ». */
async function choose(view: Ui, dialog: Element, option: string) {
  await view.click(dialog.querySelector(`input[data-option="${option}"]`));
}

describe('the summary of a binder', () => {
  it('lists the bookmarks under the names of the sub-binders at first, in the binder\'s order (by date without a free order)', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    expect(dialog).not.toBeNull();
    expect(view.q('.modal h2')?.textContent).toBe('Sommaire du classeur « Classe »');
    // Alpha's two bookmarks in reading order (top to bottom), then Gamma's (on a hidden layer), then the sub-binder's heading
    // and its note's bookmark. No line per note: Beta has no bookmark and is nowhere.
    expect(lines(dialog)).toEqual(['⚑Introduction', '⚑Bilan du chapitre', '⚑Corrigé', '#3e B', '.⚑Loi d\'Ohm']);
    expect(view.q('[data-slot="summary-intro"]')?.textContent).toContain('4 lignes');
    // The choices: sub-binders and bookmarks, no line per note.
    expect(dialog.querySelector<HTMLInputElement>('input[data-option="summary-binders"]')!.checked).toBe(true);
    expect(dialog.querySelector<HTMLInputElement>('input[data-option="summary-notes-none"]')!.checked).toBe(true);
    expect(dialog.querySelector<HTMLInputElement>('input[data-option="summary-bookmarks"]')!.checked).toBe(true);
    expect(dialog.querySelector('[data-slot="summary-fallback"]')).toBeNull();
  });

  it('lists the notes too (title), and without the sub-binders or the bookmarks, as asked', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    await choose(view, dialog, 'summary-notes-title');
    expect(lines(dialog)).toEqual(['Alpha', '.⚑Introduction', '.⚑Bilan du chapitre', 'Beta', 'Gamma', '.⚑Corrigé', '#3e B', '.Cours de physique', '..⚑Loi d\'Ohm']);
    // Without the sub-binders: no level for them.
    await choose(view, dialog, 'summary-binders');
    expect(lines(dialog)).toEqual(['Alpha', '.⚑Introduction', '.⚑Bilan du chapitre', 'Beta', 'Gamma', '.⚑Corrigé', 'Cours de physique', '.⚑Loi d\'Ohm']);
    await choose(view, dialog, 'summary-binders');
    // Without the bookmarks.
    await choose(view, dialog, 'summary-bookmarks');
    expect(lines(dialog)).toEqual(['Alpha', 'Beta', 'Gamma', '#3e B', '.Cours de physique']);
    expect(view.q('[data-slot="summary-intro"]')?.textContent).toContain('4 lignes');
  });

  it('follows the free order of the binder, the unlisted notes after by date', async () => {
    seedClasse();
    mockLibrary().orders.set('Classe', ['id-Gamma', 'id-Beta']);
    const view = await render();
    const dialog = await openSummary(view);
    expect(lines(dialog)).toEqual(['⚑Corrigé', '⚑Introduction', '⚑Bilan du chapitre', '#3e B', '.⚑Loi d\'Ohm']);
  });

  it('is also reached by the binder\'s menu in the sidebar', async () => {
    seedClasse();
    const view = await render();
    view.q('[data-binder="Classe"]')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await view.settle();
    const item = [...view.target.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((el) => el.textContent?.trim() === 'Sommaire du classeur…');
    expect(item).toBeTruthy();
    await view.click(item!);
    expect(view.q('[data-dialog="binder-summary"]')).not.toBeNull();
  });

  it('lists the notes, and says why, while the binder has no bookmark and nothing was chosen', async () => {
    addNote('Classe', 'Alpha', T0);
    addNote('Classe', 'Beta', T0 + DAY);
    const view = await render('fr', 5);
    const dialog = await openSummary(view);
    expect(lines(dialog)).toEqual(['Alpha', 'Beta', '#3e B', '.Cours de physique']);
    expect(dialog.querySelector('[data-slot="summary-fallback"]')?.textContent).toContain("n'a pas de signet");
    expect(dialog.querySelector<HTMLInputElement>('input[data-option="summary-notes-title"]')!.checked).toBe(true);
    // Nothing is remembered by looking.
    expect(localStorage.getItem('ardoise.binderSummary')).toBeNull();
    // The user's choice wins, even an empty summary: the sentence says what is missing.
    await choose(view, dialog, 'summary-notes-none');
    expect(view.q('[data-slot="summary-empty"]')?.textContent).toContain('aucun signet');
    expect(dialog.querySelector('[data-slot="summary-fallback"]')).toBeNull();
    expect(view.q('[data-row]')).toBeNull();
    await choose(view, dialog, 'summary-bookmarks');
    expect(view.q('[data-slot="summary-empty"]')?.textContent).toContain('le sommaire est vide');
    // Back to something.
    await choose(view, dialog, 'summary-notes-titleOrDate');
    expect(lines(dialog).length).toBe(4);
    await view.click(view.button('Fermer'));
    expect(view.q('[data-dialog="binder-summary"]')).toBeNull();
  });

  it('says that a binder without any note has nothing to summarise', async () => {
    await api.createBinder('Vide');
    const view = await render('fr', 3);
    await view.click(view.q('[data-binder="Vide"]'));
    await view.click(view.q('[data-action="binder-summary"]'));
    expect(view.q('[data-slot="summary-empty"]')?.textContent).toBe('Ce classeur ne contient aucune note.');
  });

  it('names an untitled note « Note sans titre » (numbered when several), or by its date, and remembers the choices', async () => {
    addNote('Classe', '', T0, [mark('u-1', 'Un', 100, 100)]);
    addNote('Classe', '', T0 + DAY, [mark('u-2', 'Deux', 100, 100)]);
    addNote('Classe', 'Titrée', T0 + 2 * DAY, [mark('u-3', 'Trois', 100, 100)]);
    const view = await render('fr', 6);
    const dialog = await openSummary(view);
    await choose(view, dialog, 'summary-notes-title');
    expect(lines(dialog).slice(0, 6)).toEqual(['Note sans titre', '.⚑Un', 'Note sans titre (2)', '.⚑Deux', 'Titrée', '.⚑Trois']);
    expect(dialog.querySelector('[data-row="bookmark"]')!.getAttribute('aria-label')).toBe('Ouvrir la note « Note sans titre » au signet « Un »');
    // With the dates: the notes are named by theirs.
    await choose(view, dialog, 'summary-notes-titleOrDate');
    const named = lines(dialog);
    expect(named[0]).toMatch(/2026/);
    expect(named[0]).not.toContain('Note sans titre');
    expect(named[4]).toBe('Titrée');
    // Without the notes the bookmarks still say which note they are in (to a screen reader), by title: dates are for the note lines.
    await choose(view, dialog, 'summary-notes-none');
    expect(dialog.querySelector('[data-row="bookmark"]')!.getAttribute('aria-label')).toBe('Ouvrir la note « Note sans titre » au signet « Un »');
    // Remembered for the next time (and by the PDF of the binder).
    await choose(view, dialog, 'summary-binders');
    await view.click(view.button('Fermer'));
    const again = await openSummary(view);
    expect(again.querySelector<HTMLInputElement>('input[data-option="summary-notes-none"]')!.checked).toBe(true);
    expect(again.querySelector<HTMLInputElement>('input[data-option="summary-binders"]')!.checked).toBe(false);
    expect(JSON.parse(localStorage.getItem('ardoise.binderSummary')!)).toEqual({ binders: false, notes: 'none', bookmarks: true });
  });

  it('starts from the first version\'s « dates » box', async () => {
    localStorage.setItem('ardoise.binderDates', 'true');
    addNote('Classe', '', T0, [mark('u-1', 'Un', 100, 100)]);
    const view = await render('fr', 4);
    const dialog = await openSummary(view);
    expect(dialog.querySelector<HTMLInputElement>('input[data-option="summary-notes-titleOrDate"]')!.checked).toBe(true);
    expect(lines(dialog)[0]).toMatch(/2026/);
  });

  it('has no button outside a binder', async () => {
    seedClasse();
    const view = await render();
    expect(view.q('[data-action="binder-summary"]')).toBeNull();
    await view.click(view.q('[data-binder="Classe"]'));
    expect(view.q('[data-action="binder-summary"]')).not.toBeNull();
  });

  it('filters the lines by their words, accents and case ignored, with what is above and below them', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    await choose(view, dialog, 'summary-notes-title');
    await view.type(dialog.querySelector('input[type="search"]'), 'CORRIGE');
    expect(lines(dialog)).toEqual(['Gamma', '.⚑Corrigé']);
    await view.type(dialog.querySelector('input[type="search"]'), 'alpha');
    expect(lines(dialog)).toEqual(['Alpha', '.⚑Introduction', '.⚑Bilan du chapitre']);
    await view.type(dialog.querySelector('input[type="search"]'), '3e b');
    expect(lines(dialog)).toEqual(['#3e B', '.Cours de physique', '..⚑Loi d\'Ohm']);
    await view.type(dialog.querySelector('input[type="search"]'), 'zzz');
    expect(view.q('[data-slot="summary-none"]')?.textContent).toContain('« zzz »');
  });

  it('is usable with the keyboard: the arrows walk through the lines, Escape closes it', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    const items = [...dialog.querySelectorAll<HTMLButtonElement>('.summary-item')];
    expect(items.map((b) => b.getAttribute('aria-label'))).toContain('Ouvrir la note « Alpha » au signet « Introduction »');
    items[0].focus();
    await view.key(items[0], 'ArrowDown');
    expect(document.activeElement).toBe(items[1]);
    await view.key(items[1], 'ArrowUp');
    expect(document.activeElement).toBe(items[0]);
    await view.key(items[0], 'ArrowUp');
    expect(document.activeElement).toBe(items[0]);
    // Escape closes it (the modal's own).
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await view.settle();
    expect(view.q('[data-dialog="binder-summary"]')).toBeNull();
  });

  it('opens the note at the bookmark chosen: the editor selects it and shows it', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    const target = dialog.querySelector<HTMLButtonElement>('[data-bookmark-id="g-1"]')!;
    await view.click(target);
    // The dialog is gone and the editor shows the note, with the bookmark selected.
    expect(view.q('[data-dialog="binder-summary"]')).toBeNull();
    await vi.waitFor(() => expect(view.q('section.editor')).not.toBeNull());
    await vi.waitFor(() => expect(view.q('g.bookmark[data-bookmark="Corrigé"]')).not.toBeNull());
    await vi.waitFor(() => expect(view.q('[data-section="selection"] .pane-help')?.textContent?.trim()).toBe('Traits sélectionnés : 0 — textes, équations et signets : 1'));
  });

  it('opens the note itself from a note line: no bookmark is selected', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    await choose(view, dialog, 'summary-notes-title');
    await view.click(dialog.querySelector('[data-row="note"][data-note$="Beta.ardoise"]'));
    expect(view.q('[data-dialog="binder-summary"]')).toBeNull();
    await vi.waitFor(() => expect(view.q('section.editor')).not.toBeNull());
    expect(view.q('[data-section="selection"]')).toBeNull();
  });
});

describe('the search finds the bookmarks', () => {
  it('shows a note whose bookmark has the words, accents and case ignored', async () => {
    seedClasse();
    const view = await render();
    await view.type(view.q<HTMLInputElement>('input.search'), 'LOI OHM');
    expect(view.titles()).toEqual(['Cours de physique']);
    await view.type(view.q<HTMLInputElement>('input.search'), 'bilan chapitre');
    expect(view.titles()).toEqual(['Alpha']);
    // A bookmark on a hidden layer is found too: a bookmark is not hidden with its layer.
    await view.type(view.q<HTMLInputElement>('input.search'), 'corrige');
    expect(view.titles()).toEqual(['Gamma']);
    await view.type(view.q<HTMLInputElement>('input.search'), 'introduction ohm');
    expect(view.titles()).toEqual([]);
    expect(view.q<HTMLInputElement>('input.search')!.placeholder).toContain('signet');
  });
});

describe('in English', () => {
  it('has its texts', async () => {
    seedClasse();
    const view = await renderHome('en', 6);
    ui = view;
    await view.click(view.q('[data-binder="Classe"]'));
    const button = view.q('[data-action="binder-summary"]')!;
    expect(button.textContent?.trim()).toBe('Summary');
    await view.click(button);
    expect(view.q('.modal h2')?.textContent).toBe('Summary of the binder "Classe"');
    const dialog = view.q('[data-dialog="binder-summary"]')!;
    expect(dialog.querySelector('legend')?.textContent).toBe('In the summary');
    expect(dialog.textContent).toContain('The sub-binders');
    expect(dialog.textContent).toContain('Title or date');
    expect(dialog.querySelector('[data-row="bookmark"]')?.getAttribute('aria-label')).toBe('Open the note "Alpha" at the bookmark "Introduction"');
    expect(dialog.querySelector('[data-slot="summary-intro"]')?.textContent).toContain('4 lines');
    await view.type(dialog.querySelector('input[type="search"]'), 'zzz');
    expect(view.q('[data-slot="summary-none"]')?.textContent).toBe('No line matches “zzz”.');
    await choose(view, dialog, 'summary-bookmarks');
    expect(view.q('[data-slot="summary-empty"]')?.textContent).toContain('With these choices the summary is empty');
  });
});
