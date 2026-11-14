// @vitest-environment jsdom
/// <reference types="node" />
// The library's right-click menus (user feedback 2026-11-14): never the web page's menu; a card's and a binder's menu open at
// the pointer (under their « ⋯ » button for the keyboard's Menu key); the sidebar's « Toutes les notes », « Favoris », tags
// and trash and the trash's rows have theirs. And Ctrl + P, which prints what is in front with Ardoise's own dialog.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resetMockLibrary } from '../../lib/mock-data';
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
  document.body.querySelectorAll('[role="menu"]').forEach((menu) => menu.remove());
  vi.restoreAllMocks();
});

/** A mouse's right click at (x, y): Chromium gives the event the device's pointer type. */
function rightClick(el: Element, x = 120, y = 90) {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: x, clientY: y });
  Object.defineProperty(event, 'pointerType', { value: 'mouse' });
  el.dispatchEvent(event);
  return event;
}

/** The Menu key on a focused element: a `contextmenu` event with no button and no position. */
function menuKey(el: Element) {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 0 });
  el.dispatchEvent(event);
  return event;
}

const openMenu = () => document.querySelector<HTMLElement>('[role="menu"]');
const entryLabels = () => [...document.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent?.trim());
const entry = (label: string) => {
  const found = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((e) => e.textContent?.trim() === label);
  if (!found) throw new Error(`no entry "${label}" among ${entryLabels().join(' | ')}`);
  return found;
};

async function ready(locale: 'fr' | 'en' = 'fr') {
  ui = await renderHome(locale, 3);
  return ui;
}

async function press(view: Ui, key: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ctrlKey: true, ...init });
  window.dispatchEvent(event);
  await view.settle();
  return event;
}

describe('the menu of a card', () => {
  it('opens at the pointer on a right click, and not as the web page\'s menu', async () => {
    const view = await ready();
    const event = rightClick(view.card('Liste de courses'), 300, 210);
    await view.settle();
    expect(event.defaultPrevented).toBe(true);
    const menu = openMenu()!;
    expect(menu.getAttribute('aria-label')).toBe('Actions pour Liste de courses');
    expect(menu.style.left).toBe('300px');
    expect(menu.style.top).toBe('210px');
    expect(menu.parentElement).toBe(document.body);
    expect(entryLabels()).toContain('Renommer');
  });

  it('opens under its « ⋯ » button for the Menu key, and for a click on the button', async () => {
    const view = await ready();
    const card = view.card('Liste de courses');
    menuKey(card);
    await view.settle();
    let menu = openMenu()!;
    expect(card.contains(menu)).toBe(true);
    expect(menu.style.left).toBe('');
    // Escape, then the button itself.
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await view.settle();
    expect(openMenu()).toBeNull();
    await view.click(card.querySelector('.card-menu button'));
    menu = openMenu()!;
    expect(card.contains(menu)).toBe(true);
    expect(menu.style.left).toBe('');
  });

  it('chooses an entry, and Escape gives the focus to the « ⋯ » button', async () => {
    const view = await ready();
    rightClick(view.card('Liste de courses'));
    await view.settle();
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    document.activeElement!.dispatchEvent(escape);
    await view.settle();
    expect(openMenu()).toBeNull();
    expect(document.activeElement).toBe(view.card('Liste de courses').querySelector('.card-menu button'));
    rightClick(view.card('Liste de courses'));
    await view.settle();
    await view.click(entry('Renommer'));
    expect(openMenu()).toBeNull();
    // The card shows the field for its title.
    expect(view.q('.note-card .rename-input')).toBeTruthy();
  });

  it('is one at a time, and a right click on another card moves it', async () => {
    const view = await ready();
    rightClick(view.card('Liste de courses'));
    await view.settle();
    rightClick(view.card('Réunion parents'), 40, 50);
    await view.settle();
    expect(document.querySelectorAll('[role="menu"]').length).toBe(1);
    expect(openMenu()!.getAttribute('aria-label')).toBe('Actions pour Réunion parents');
    expect(openMenu()!.style.left).toBe('40px');
  });

  it('keeps the system\'s menu on the title being renamed', async () => {
    const view = await ready();
    rightClick(view.card('Liste de courses'));
    await view.settle();
    await view.click(entry('Renommer'));
    const field = view.q('.note-card .rename-input')!;
    const event = rightClick(field);
    await view.settle();
    expect(event.defaultPrevented).toBe(false);
    expect(openMenu()).toBeNull();
  });
});

describe('the menu of a binder', () => {
  it('opens at the pointer on a right click, under its button for the Menu key', async () => {
    const view = await ready();
    const row = view.q('[data-binder="Perso"]')!;
    const event = rightClick(row, 80, 400);
    await view.settle();
    expect(event.defaultPrevented).toBe(true);
    expect(openMenu()!.style.left).toBe('80px');
    expect(openMenu()!.style.top).toBe('400px');
    expect(entryLabels()).toContain('Imprimer le classeur…');
    document.body.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await view.settle();
    menuKey(row);
    await view.settle();
    expect(row.parentElement!.querySelector('.binder-menu')!.contains(openMenu())).toBe(true);
  });
});

describe('the menu of « Toutes les notes » and « Favoris »', () => {
  it('shows the view or selects its notes', async () => {
    const view = await ready();
    rightClick(view.q('[data-view="favorites"]')!);
    await view.settle();
    expect(openMenu()!.getAttribute('aria-label')).toBe('Menu de « Favoris »');
    expect(entryLabels()).toEqual(['Afficher', 'Tout sélectionner']);
    await view.click(entry('Tout sélectionner'));
    expect(view.q('[data-view="favorites"]')!.getAttribute('aria-current')).toBe('true');
    expect(view.titles()).toEqual(['Liste de courses']);
    expect(view.q('.selection-bar')!.textContent).toContain('Sélection : 1');

    rightClick(view.q('[data-view="all"]')!);
    await view.settle();
    await view.click(entry('Afficher'));
    expect(view.titles().length).toBe(3);
    rightClick(view.q('[data-view="all"]')!);
    await view.settle();
    await view.click(entry('Tout sélectionner'));
    expect(view.q('.selection-bar')!.textContent).toContain('Sélection : 3');
  });

  it('speaks English', async () => {
    const view = await ready('en');
    rightClick(view.q('[data-view="all"]')!);
    await view.settle();
    expect(entryLabels()).toEqual(['Show', 'Select all']);
  });
});

describe('the menu of a tag', () => {
  it('shows the tag or opens its colour palette', async () => {
    const view = await ready();
    const tag = view.q('[data-tag="réunion"]')!;
    const event = rightClick(tag);
    await view.settle();
    expect(event.defaultPrevented).toBe(true);
    expect(openMenu()!.getAttribute('aria-label')).toBe('Menu de l\'étiquette réunion');
    expect(entryLabels()).toEqual(['Afficher', 'Couleur…']);
    await view.click(entry('Afficher'));
    expect(tag.getAttribute('aria-current')).toBe('true');
    expect(view.titles()).toEqual(['Réunion parents']);

    rightClick(tag);
    await view.settle();
    await view.click(entry('Couleur…'));
    await vi.waitFor(() => expect(view.q('[data-tag="réunion"]')!.closest('.nav-row')!.querySelector('[role="menuitemradio"]')).toBeTruthy());
  });
});

describe('the trash', () => {
  async function trashOne(view: Ui) {
    await view.select('Réunion parents');
    await view.click(view.button('Mettre à la corbeille', view.q('.selection-bar')!));
    await vi.waitFor(() => expect(view.titles().length).toBe(2));
  }

  it('offers to open it and to empty it, which asks the usual question in the trash view', async () => {
    const view = await ready();
    rightClick(view.q('[data-view="trash"]')!);
    await view.settle();
    expect(entry('Vider la corbeille…').getAttribute('aria-disabled')).toBe('true');
    expect(entry('Vider la corbeille…').title).toBe('La corbeille est vide.');
    await view.click(entry('Ouvrir la corbeille'));
    expect(view.q('[data-view="trash"]')!.getAttribute('aria-current')).toBe('true');
    await view.click(view.q('[data-view="all"]'));

    await trashOne(view);
    rightClick(view.q('[data-view="trash"]')!);
    await view.settle();
    expect(entry('Vider la corbeille…').getAttribute('aria-disabled')).toBeNull();
    await view.click(entry('Vider la corbeille…'));
    expect(view.q('[data-view="trash"]')!.getAttribute('aria-current')).toBe('true');
    expect(view.q('[role="alertdialog"]')!.textContent).toContain('Supprimer définitivement la note de la corbeille');
    // Nothing is deleted until it is confirmed.
    expect(view.all('tr[data-id]').length).toBe(1);
  });

  it('gives a trashed note a menu: restore, or delete for good after a confirmation', async () => {
    const view = await ready();
    await trashOne(view);
    await view.click(view.q('[data-view="trash"]'));
    const row = view.q('tr[data-id]')!;
    const event = rightClick(row, 200, 150);
    await view.settle();
    expect(event.defaultPrevented).toBe(true);
    expect(openMenu()!.style.left).toBe('200px');
    expect(entryLabels()).toEqual(['Restaurer', 'Supprimer définitivement…']);

    await view.click(entry('Supprimer définitivement…'));
    expect(view.q('tr[data-id]')!.textContent).toContain('Supprimer définitivement « Réunion parents » ?');
    expect(view.all('tr[data-id]').length).toBe(1);

    rightClick(view.q('tr[data-id]')!);
    await view.settle();
    await view.click(entry('Restaurer'));
    await vi.waitFor(() => expect(view.all('tr[data-id]').length).toBe(0));
    await view.click(view.q('[data-view="all"]'));
    expect(view.titles()).toContain('Réunion parents');
  });
});

describe('Ctrl + P in the library', () => {
  it('prints the selected notes, with Ardoise\'s dialog, and never the web view\'s', async () => {
    const view = await ready();
    await view.select('Liste de courses', 'Réunion parents');
    const event = await press(view, 'p');
    expect(event.defaultPrevented).toBe(true);
    const dialog = view.q('[data-dialog="print"]')!;
    expect(dialog).toBeTruthy();
    expect(dialog.textContent).toContain('Notes sélectionnées : 2');
  });

  it('does the same with Ctrl + Shift + P', async () => {
    const view = await ready();
    await view.select('Liste de courses');
    const event = await press(view, 'P', { shiftKey: true });
    expect(event.defaultPrevented).toBe(true);
    expect(view.q('[data-dialog="print"]')).toBeTruthy();
  });

  it('prints the binder shown when nothing is selected', async () => {
    const view = await ready();
    await view.click(view.q('[data-binder="Perso"]'));
    await press(view, 'p');
    const dialog = view.q('[data-dialog="print"]')!;
    expect(dialog.textContent).toContain('Classeur « Perso »');
  });

  it('says so for a binder without a note', async () => {
    const view = await ready();
    // A binder emptied of its notes has nothing to print.
    await view.click(view.q('.twisty'));
    await view.click(view.q('[data-binder="Classe/3e B"]'));
    await view.select('Cours de physique');
    await view.click(view.button('Mettre à la corbeille', view.q('.selection-bar')!));
    await vi.waitFor(() => expect(view.titles().length).toBe(0));
    await press(view, 'p');
    expect(view.q('[data-dialog="print"]')).toBeNull();
    expect(view.q('.notice')!.textContent).toContain('Ce classeur ne contient aucune note.');
  });

  it('asks to select notes rather than print a whole view on a guess', async () => {
    const view = await ready();
    const event = await press(view, 'p');
    expect(event.defaultPrevented).toBe(true);
    expect(view.q('[data-dialog="print"]')).toBeNull();
    expect(view.q('.notice')!.textContent).toContain('Sélectionnez des notes à imprimer, ou ouvrez un classeur.');
    await view.click(view.q('[data-view="trash"]'));
    await press(view, 'p');
    expect(view.q('[data-dialog="print"]')).toBeNull();
  });

  it('does nothing under a dialog, but still keeps the browser\'s print off', async () => {
    const view = await ready();
    await view.select('Liste de courses');
    await view.click(view.button('Exporter…', view.q('.selection-bar')!));
    expect(view.q('[role="dialog"]')).toBeTruthy();
    const event = await press(view, 'p');
    expect(event.defaultPrevented).toBe(true);
    expect(view.q('[data-dialog="print"]')).toBeNull();
  });

  it('does not print when held down', async () => {
    const view = await ready();
    await view.select('Liste de courses');
    await press(view, 'p', { repeat: true });
    expect(view.q('[data-dialog="print"]')).toBeNull();
  });

  it('is not an Alt Gr + P', async () => {
    const view = await ready();
    await view.select('Liste de courses');
    const event = await press(view, 'p', { altKey: true });
    expect(event.defaultPrevented).toBe(false);
    expect(view.q('[data-dialog="print"]')).toBeNull();
  });
});
