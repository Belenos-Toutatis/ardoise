// @vitest-environment jsdom
// Milestone M12: « Reprendre en mode direct » in a note card's menu, and the main window handing
// the note to live mode (from a card, or from the editor). The editor and the live view are
// stand-ins that show what Home gives them.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveTick, resetMockLive } from '../../lib/mock-live';
import { resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import { renderHome, type Ui } from './library-test-helpers';

// The editor and the live view are tested on their own: here they are stand-ins that show what
// the main window gives them.
vi.mock('../editor/Editor.svelte', async () => ({ default: (await import('./ResumeEditorDouble.svelte')).default }));
vi.mock('../live/LiveView.svelte', async () => ({ default: (await import('./ResumeLiveDouble.svelte')).default }));

let ui: Ui | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockLive();
  resetMockTablet();
  tablet.reset();
});

afterEach(() => {
  ui?.destroy();
  ui = null;
  vi.restoreAllMocks();
});

function pathOf(title: string): string {
  const found = [...mockLibrary().notes].find(([, s]) => s.note.title === title);
  if (!found) throw new Error(`no note ${title}`);
  return found[0];
}

/** Opens the menu of a card; the menu items. */
async function menuOf(home: Ui, title: string) {
  await home.click(home.card(title).querySelector('.card-menu button[aria-haspopup="menu"]'));
  return [...home.card(title).querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
}

const item = (items: HTMLElement[], label: string) => {
  const found = items.find((el) => el.textContent?.trim() === label);
  if (!found) throw new Error(`no menu item "${label}" among ${items.map((el) => el.textContent?.trim()).join(' | ')}`);
  return found;
};

async function ready(locale: 'fr' | 'en' = 'fr') {
  ui = await renderHome(locale);
  // The tablet's status comes in: the item depends on it.
  await vi.waitFor(() => expect(tablet.status?.registered).toBeTruthy());
  return ui;
}

describe('« Reprendre en mode direct » in the note card menu', () => {
  it.each([
    ['fr', ['Renommer', 'Ajouter aux favoris', 'Reprendre en mode direct', 'Mettre à la corbeille']],
    ['en', ['Rename', 'Add to favourites', 'Resume in live mode', 'Move to trash']],
  ] as const)('is offered between the card actions in %s, and enabled with a registered tablet', async (locale, labels) => {
    const home = await ready(locale);
    const items = await menuOf(home, 'Réunion parents');
    expect(items.map((el) => el.textContent?.trim())).toEqual(labels);
    expect(items[2].getAttribute('aria-disabled')).toBeNull();
    expect(items[2].title).toBe('');
  });

  it('hands the note to live mode: the library is hidden, then comes back with the focus on the card', async () => {
    const home = await ready();
    const path = pathOf('Liste de courses');
    const items = await menuOf(home, 'Liste de courses');
    await home.click(item(items, 'Reprendre en mode direct'));
    expect(home.q('[data-double="live"]')?.getAttribute('data-resume')).toBe(path);
    expect(home.q('.shell')?.classList.contains('shell-hidden')).toBe(true);
    expect(home.q('[data-double="editor"]')).toBeNull();

    await home.click(home.q('[data-double-close="live"]'));
    expect(home.q('[data-double="live"]')).toBeNull();
    expect(home.q('.shell')?.classList.contains('shell-hidden')).toBe(false);
    expect(document.activeElement).toBe(home.card('Liste de courses').querySelector('.card-open'));
  });

  it('starts a new session, not a resume, from the tablet panel afterwards', async () => {
    const home = await ready();
    const items = await menuOf(home, 'Liste de courses');
    await home.click(item(items, 'Reprendre en mode direct'));
    await home.click(home.q('[data-double-close="live"]'));
    await home.click(home.q('.tablet-status'));
    await home.click(home.q('.tablet-panel button[data-action="live"]'));
    expect(home.q('[data-double="live"]')?.getAttribute('data-resume')).toBe('');
  });

  it('is disabled, with the reason, while several notes are selected', async () => {
    const home = await ready();
    await home.select('Liste de courses', 'Réunion parents');
    const items = await menuOf(home, 'Liste de courses');
    const resume = item(items, 'Reprendre en mode direct');
    expect(resume.getAttribute('aria-disabled')).toBe('true');
    expect(resume.title).toBe('Sélectionnez une seule note pour reprendre le mode direct.');
    await home.click(resume);
    expect(home.q('[data-double="live"]')).toBeNull();
    // The menu stays open (nothing was chosen), and the other items still work.
    expect(home.card('Liste de courses').querySelector('[role="menu"]')).not.toBeNull();
  });

  it('is available again with a single note selected', async () => {
    const home = await ready();
    await home.select('Liste de courses');
    const items = await menuOf(home, 'Liste de courses');
    expect(item(items, 'Reprendre en mode direct').getAttribute('aria-disabled')).toBeNull();
  });

  it.each([
    ['fr', 'Reprendre en mode direct demande une tablette enregistrée : enregistrez-la depuis le panneau de la tablette.'],
    ['en', 'Resuming in live mode needs a registered tablet: register it from the tablet panel.'],
  ] as const)('is disabled, with a hint, when no tablet is registered (%s)', async (locale, hint) => {
    const home = await ready(locale);
    tablet.status = { ...tablet.status!, registered: null, connection: 'notRegistered' };
    const items = await menuOf(home, 'Liste de courses');
    const resume = item(items, locale === 'fr' ? 'Reprendre en mode direct' : 'Resume in live mode');
    expect(resume.getAttribute('aria-disabled')).toBe('true');
    expect(resume.title).toBe(hint);
    await home.click(resume);
    expect(home.q('[data-double="live"]')).toBeNull();
  });

  it('is disabled while the tablet syncs or registers, as the tablet panel\'s button is', async () => {
    const home = await ready();
    tablet.status = { ...tablet.status!, connection: 'syncing' };
    const items = await menuOf(home, 'Liste de courses');
    const resume = item(items, 'Reprendre en mode direct');
    expect(resume.getAttribute('aria-disabled')).toBe('true');
    expect(resume.title).toBe('La tablette est occupée (synchronisation ou enregistrement en cours).');
  });
});

describe('resuming from the editor', () => {
  it('opens live mode on the note the editor reports, and gives the library back afterwards', async () => {
    const home = await ready();
    const path = pathOf('Réunion parents');
    await home.click(home.card('Réunion parents').querySelector('.card-open'));
    expect(home.q('[data-double="editor"]')?.getAttribute('data-path')).toBe(path);
    expect(home.q('[data-double="editor"]')?.getAttribute('data-can-resume')).toBe('yes');

    await home.click(home.q('[data-double-resume="editor"]'));
    expect(home.q('[data-double="editor"]')).toBeNull();
    expect(home.q('[data-double="live"]')?.getAttribute('data-resume')).toBe(path);

    await home.click(home.q('[data-double-close="live"]'));
    expect(home.q('.shell')?.classList.contains('shell-hidden')).toBe(false);
    expect(document.activeElement).toBe(home.card('Réunion parents').querySelector('.card-open'));
  });
});

describe('after the session', () => {
  it('shows the note updated and asks for its thumbnail again', async () => {
    mockLiveConfig.waitMs = 0;
    mockLiveConfig.pen = false;
    const thumbnail = vi.spyOn(api, 'noteThumbnail');
    const home = await ready();
    const path = pathOf('Liste de courses');
    const before = mockLibrary().notes.get(path)!;
    const strokes = before.note.strokes.length;
    await vi.waitFor(() => expect(thumbnail).toHaveBeenCalledWith(path, before.stamp));

    const items = await menuOf(home, 'Liste de courses');
    await home.click(item(items, 'Reprendre en mode direct'));
    // What the live view does with the note it was given.
    mockLiveInvoke('live_resume', { path });
    await new Promise((resolve) => setTimeout(resolve, 20));
    mockLiveTick(150);
    mockLiveInvoke('live_stop', {});
    await home.click(home.q('[data-double-close="live"]'));

    const after = mockLibrary().notes.get(path)!;
    expect(after.stamp).not.toEqual(before.stamp);
    expect(after.note.strokes.length).toBeGreaterThan(strokes);
    await vi.waitFor(() => expect(thumbnail).toHaveBeenCalledWith(path, after.stamp));
    // The card is the same note (not a new one), still titled as before.
    expect(home.titles()).toEqual(expect.arrayContaining(['Liste de courses']));
    expect(home.all('.note-card')).toHaveLength(3);
  });
});
