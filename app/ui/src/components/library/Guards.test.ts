// @vitest-environment jsdom
/**
 * What the library's keys and buttons do while something else is in front: live mode (the library stays mounted,
 * hidden, under it) or a dialog. Ctrl+Z, Ctrl+N and Ctrl+F belong to the screen that is shown, and a second « Nouvelle note »
 * under way does not make a second note. The editor and the live view are stand-ins.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { mockLibrary, notifyLibraryChanged, resetMockLibrary } from '../../lib/mock-data';
import { resetMockLive } from '../../lib/mock-live';
import { resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import { renderHome, type Ui } from './library-test-helpers';

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

async function ready(): Promise<Ui> {
  ui = await renderHome('fr');
  await vi.waitFor(() => expect(tablet.status?.registered).toBeTruthy());
  return ui;
}

const ctrl = (view: Ui, key: string) => view.key(document.body, key, { ctrlKey: true });
const noteCount = () => mockLibrary().notes.size;

/** Trashes a note: a notice with « Annuler » (and Ctrl+Z) now undoes it. */
async function trashOne(view: Ui) {
  await view.choose('Liste de courses', 'Mettre à la corbeille');
  await vi.waitFor(() => expect(view.all('.activity-offer button[data-action="undo"]')).toHaveLength(1));
}

describe('Ctrl+Z in the library', () => {
  it('undoes the latest step when the library is the screen shown', async () => {
    const view = await ready();
    const restore = vi.spyOn(api, 'trashRestore');
    await trashOne(view);
    await ctrl(view, 'z');
    await vi.waitFor(() => expect(restore).toHaveBeenCalledTimes(1));
  });

  it('leaves it to live mode while live mode is in front of the hidden library', async () => {
    const view = await ready();
    const restore = vi.spyOn(api, 'trashRestore');
    await trashOne(view);
    const items = await (async () => {
      await view.click(view.card('Réunion parents').querySelector('.card-menu button[aria-haspopup="menu"]'));
      return [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    })();
    await view.click(items.find((el) => el.textContent?.trim() === 'Reprendre en mode direct')!);
    expect(view.q('[data-double="live"]')).not.toBeNull();
    const event = new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true, cancelable: true });
    document.body.dispatchEvent(event);
    await view.settle();
    expect(restore).not.toHaveBeenCalled();
    // Live mode's own undo still gets the key.
    expect(event.defaultPrevented).toBe(false);
  });

  it('does not undo an operation behind a dialog', async () => {
    const view = await ready();
    const restore = vi.spyOn(api, 'trashRestore');
    await trashOne(view);
    await view.choose('Réunion parents', 'Étiquettes…');
    expect(view.q('[data-dialog="tags"], .modal')).not.toBeNull();
    await ctrl(view, 'z');
    expect(restore).not.toHaveBeenCalled();
  });
});

describe('Ctrl+N and Ctrl+F in the library', () => {
  it('make a note and reach the search field when no dialog is open', async () => {
    const view = await ready();
    const before = noteCount();
    await ctrl(view, 'f');
    expect(document.activeElement).toBe(view.q('input.search'));
    await ctrl(view, 'n');
    await vi.waitFor(() => expect(noteCount()).toBe(before + 1));
  });

  it.each(['Étiquettes…', 'Déplacer vers…', 'Changer la date…'])('do nothing under the dialog « %s »', async (entry) => {
    const view = await ready();
    const before = noteCount();
    await view.choose('Réunion parents', entry);
    expect(document.querySelector('.modal')).not.toBeNull();
    (document.activeElement as HTMLElement | null)?.blur();
    await ctrl(view, 'n');
    await ctrl(view, 'f');
    expect(noteCount()).toBe(before);
    expect(document.activeElement).not.toBe(view.q('input.search'));
    expect(view.q('[data-double="editor"]')).toBeNull();
  });
});

describe('« Nouvelle note »', () => {
  it('makes one note when it is asked again while the first is being made', async () => {
    const view = await ready();
    const before = noteCount();
    const create = vi.spyOn(api, 'createNote');
    const button = view.button('Nouvelle note');
    button.click();
    button.click();
    await ctrl(view, 'n');
    await view.settle();
    await vi.waitFor(() => expect(view.q('[data-double="editor"]')).not.toBeNull());
    expect(create).toHaveBeenCalledTimes(1);
    expect(noteCount()).toBe(before + 1);
  });
});

describe('the loading error', () => {
  it('goes by itself once the listing is read again', async () => {
    const view = await ready();
    vi.spyOn(api, 'libraryList').mockRejectedValueOnce(new Error('boom'));
    notifyLibraryChanged();
    await vi.waitFor(() => expect(view.q('.banner.error')).not.toBeNull());
    notifyLibraryChanged();
    await vi.waitFor(() => expect(view.q('.banner.error')).toBeNull());
  });

  it('does not take away the error of an action that failed', async () => {
    const view = await ready();
    vi.spyOn(api, 'trashNotes').mockRejectedValueOnce(new Error('refused'));
    await view.choose('Liste de courses', 'Mettre à la corbeille');
    await vi.waitFor(() => expect(view.q('.banner.error')?.textContent).toContain('refused'));
    notifyLibraryChanged();
    await view.settle();
    expect(view.q('.banner.error')?.textContent).toContain('refused');
  });
});

describe('quick writes on the same note', () => {
  const tagsOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)!.note.tags;
  const favoriteOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)!.note.favorite ?? false;

  it('two clicks on the star in a row are two toggles, not a « modified meanwhile » error', async () => {
    const view = await ready();
    const star = view.card('Réunion parents').querySelector<HTMLButtonElement>('button.star')!;
    const before = favoriteOf('Réunion parents');
    star.click();
    star.click();
    await view.settle();
    await vi.waitFor(() => expect(favoriteOf('Réunion parents')).toBe(before));
    expect(view.q('.banner.error')).toBeNull();
  });

  it('two tags clicked in a row in « Étiquettes… » are both added', async () => {
    const view = await ready();
    await view.choose('Réunion parents', 'Étiquettes…');
    const dialog = document.querySelector('[data-dialog="tags"]')!;
    const first = dialog.querySelector<HTMLButtonElement>('[data-add-tag="physique"]')!;
    const second = dialog.querySelector<HTMLButtonElement>('[data-add-tag="3e B"]')!;
    first.click();
    second.click();
    await view.settle();
    await vi.waitFor(() => expect(tagsOf('Réunion parents')).toEqual(expect.arrayContaining(['physique', '3e B'])));
    expect(view.q('.banner.error')).toBeNull();
  });
});

describe('renaming a card in place', () => {
  const rename = async (view: Ui, title: string) => {
    await view.choose(title, 'Renommer');
    return view.q<HTMLInputElement>('.note-card .rename-input')!;
  };
  const opener = (view: Ui, title: string) => view.card(title).querySelector<HTMLElement>('.card-open');

  it('gives the focus back to the card when Escape gives up', async () => {
    const view = await ready();
    const field = await rename(view, 'Réunion parents');
    await view.key(field, 'Escape');
    expect(view.q('.rename-input')).toBeNull();
    expect(document.activeElement).toBe(opener(view, 'Réunion parents'));
  });

  it('gives the focus back to the card when Enter changes nothing', async () => {
    const view = await ready();
    const field = await rename(view, 'Réunion parents');
    await view.key(field, 'Enter');
    expect(document.activeElement).toBe(opener(view, 'Réunion parents'));
  });

  it('gives the focus back to the card when the new title is refused', async () => {
    const view = await ready();
    vi.spyOn(api, 'retitleNote').mockRejectedValueOnce(new Error('refused'));
    const field = await rename(view, 'Réunion parents');
    await view.type(field, 'Autre titre');
    await view.key(field, 'Enter');
    await view.settle();
    expect(view.q('.banner.error')).not.toBeNull();
    expect(document.activeElement).toBe(opener(view, 'Réunion parents'));
  });

  it('does not take the focus from where the user went (a blur)', async () => {
    const view = await ready();
    const field = await rename(view, 'Réunion parents');
    const search = view.q<HTMLInputElement>('input.search')!;
    search.focus();
    field.dispatchEvent(new FocusEvent('blur'));
    await view.settle();
    expect(document.activeElement).toBe(search);
  });
});
