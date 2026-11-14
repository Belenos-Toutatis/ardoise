// @vitest-environment jsdom
/// <reference types="node" />
// The summary of a binder (SPEC §4.3 « Sommaire du classeur », Q20 item 5): the bookmarks of its notes in the binder's
// order (its free order when it has one; the sub-binders after), a click opens the note at that bookmark, the library's
// search finds a note by its bookmarks, in French and in English, and the dialog is usable with the keyboard.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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

/** The summary as it reads: one line per note (its title) and per bookmark (« - label »). */
function lines(dialog: Element): string[] {
  return [...dialog.querySelectorAll('.summary-note-title, .summary-item')].map((el) =>
    el.classList.contains('summary-item')
      ? `- ${el.querySelector('.summary-label')?.textContent?.trim()}`
      : (el.childNodes[0].textContent ?? '').trim(),
  );
}

describe('the summary of a binder', () => {
  it('lists the bookmarks of the binder\'s notes by date when it has no free order, its sub-binders after', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    expect(dialog).not.toBeNull();
    expect(view.q('.modal h2')?.textContent).toBe('Sommaire du classeur « Classe »');
    // Alpha (oldest), then Gamma; Beta has no bookmark and is not listed; the sub-binder's note last. A note's
    // bookmarks are in reading order (top to bottom), a bookmark on a hidden layer included.
    expect(lines(dialog)).toEqual(['Alpha', '- Introduction', '- Bilan du chapitre', 'Gamma', '- Corrigé', 'Cours de physique', '- Loi d\'Ohm']);
    // The sub-binder's note says where it is.
    expect(dialog.querySelector('[data-note$="physique.ardoise"] .summary-binder, .summary-binder')?.textContent).toBe('3e B');
    expect(view.q('.binder-summary .muted')?.textContent).toContain('4 signets');
  });

  it('follows the free order of the binder, the unlisted notes after by date', async () => {
    seedClasse();
    mockLibrary().orders.set('Classe', ['id-Gamma', 'id-Beta']);
    const view = await render();
    const dialog = await openSummary(view);
    expect(lines(dialog)).toEqual(['Gamma', '- Corrigé', 'Alpha', '- Introduction', '- Bilan du chapitre', 'Cours de physique', '- Loi d\'Ohm']);
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

  it('says so when the binder has no bookmark, and closes with its button', async () => {
    addNote('Classe', 'Alpha', T0);
    const view = await render('fr', 4);
    await view.click(view.q('[data-binder="Classe"]'));
    await view.click(view.q('[data-action="binder-summary"]'));
    expect(view.q('[data-slot="summary-empty"]')?.textContent).toContain('aucun signet');
    expect(view.q('.summary-item')).toBeNull();
    await view.click(view.button('Fermer'));
    expect(view.q('[data-dialog="binder-summary"]')).toBeNull();
  });

  it('has no button outside a binder', async () => {
    seedClasse();
    const view = await render();
    expect(view.q('[data-action="binder-summary"]')).toBeNull();
    await view.click(view.q('[data-binder="Classe"]'));
    expect(view.q('[data-action="binder-summary"]')).not.toBeNull();
  });

  it('filters the bookmarks by their words, accents and case ignored, or by a note\'s title', async () => {
    seedClasse();
    const view = await render();
    const dialog = await openSummary(view);
    await view.type(dialog.querySelector('input[type="search"]'), 'CORRIGE');
    expect(lines(dialog)).toEqual(['Gamma', '- Corrigé']);
    await view.type(dialog.querySelector('input[type="search"]'), 'alpha');
    expect(lines(dialog)).toEqual(['Alpha', '- Introduction', '- Bilan du chapitre']);
    await view.type(dialog.querySelector('input[type="search"]'), 'zzz');
    expect(view.q('[data-slot="summary-none"]')?.textContent).toContain('« zzz »');
  });

  it('is usable with the keyboard: the arrows walk through the bookmarks, Enter opens the note', async () => {
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
    await vi.waitFor(() => expect(view.q('[data-section="selection"] .pane-help')?.textContent?.trim()).toBe('Traits sélectionnés : 0 — textes : 1'));
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
    expect(dialog.querySelector('.summary-item')?.getAttribute('aria-label')).toBe('Open the note "Alpha" at the bookmark "Introduction"');
    expect(dialog.querySelector('.muted')?.textContent).toContain('4 bookmarks in this binder');
    await view.type(dialog.querySelector('input[type="search"]'), 'zzz');
    expect(view.q('[data-slot="summary-none"]')?.textContent).toBe('No bookmark matches “zzz”.');
  });
});
