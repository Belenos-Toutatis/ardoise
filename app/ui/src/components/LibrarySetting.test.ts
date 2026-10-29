// @vitest-environment jsdom
// Milestone M10: Settings › change library shows what the chosen folder holds and asks before
// switching, as the first-run wizard does.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { sameFolder } from '../lib/library-choice';
import { mockFolders } from '../lib/mock';
import LibrarySetting from './LibrarySetting.svelte';
import Wizard from './Wizard.svelte';

const picked = vi.hoisted(() => ({ folder: null as string | null }));
// Stands in for the native folder picker.
vi.mock('../lib/library', () => ({ pickFolder: vi.fn(async () => picked.folder) }));

const CURRENT = 'C:\\Users\\demo\\Nextcloud\\Ardoise';
const OTHER = 'D:\\Nextcloud\\Ardoise du collège';

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: CURRENT,
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  mockFolders.clear();
  picked.folder = OTHER;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

function render(component: typeof LibrarySetting | typeof Wizard, locale: 'fr' | 'en' = 'fr', shown: AppState = app) {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const onchange = vi.fn();
  const mounted = mount(component, { target, props: { app: shown, onchange } });
  cleanup = () => {
    unmount(mounted);
    target.remove();
  };
  const button = (text: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  const click = async (el: Element) => {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();
  };
  return { target, onchange, button, click };
}

describe('Settings › change library', () => {
  it('shows what the folder holds and switches only once confirmed', async () => {
    mockFolders.set(OTHER, { existing: true, notes: 42, complete: true });
    const setLibrary = vi.spyOn(api, 'setLibrary');
    const ui = render(LibrarySetting);
    expect(ui.target.textContent).toContain(CURRENT);

    await ui.click(ui.button('Changer'));
    const dialog = ui.target.querySelector('[role="alertdialog"]')!;
    expect(dialog.getAttribute('aria-label')).toBe('Changer de bibliothèque');
    expect(dialog.textContent).toContain(OTHER);
    expect(dialog.textContent).toContain('Ce dossier contient déjà une bibliothèque Ardoise');
    expect(dialog.textContent).toContain('Il contient 42 notes.');
    expect(dialog.textContent).toContain('sur ce PC seulement (PC-TEST)');
    // Nothing switched yet.
    expect(setLibrary).not.toHaveBeenCalled();
    expect(ui.onchange).not.toHaveBeenCalled();

    await ui.click(ui.button('Utiliser ce dossier'));
    expect(setLibrary).toHaveBeenCalledWith(OTHER);
    expect(ui.onchange).toHaveBeenCalledOnce();
    expect(ui.onchange.mock.calls[0][0].library).toBe(OTHER);
    expect(ui.target.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('can be cancelled', async () => {
    mockFolders.set(OTHER, { existing: false, notes: 0, complete: true });
    const setLibrary = vi.spyOn(api, 'setLibrary');
    const ui = render(LibrarySetting);
    await ui.click(ui.button('Changer'));
    expect(ui.target.textContent).toContain('Une nouvelle bibliothèque sera créée dans ce dossier.');
    expect(ui.target.textContent).toContain('Il ne contient aucune note.');
    await ui.click(ui.button('Annuler'));
    expect(ui.target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(setLibrary).not.toHaveBeenCalled();
  });

  it('does nothing when no folder is picked', async () => {
    picked.folder = null;
    const ui = render(LibrarySetting);
    await ui.click(ui.button('Changer'));
    expect(ui.target.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('says when the folder is already the library, in English', async () => {
    picked.folder = 'c:/users/demo/nextcloud/ardoise/';
    mockFolders.set(picked.folder, { existing: true, notes: 1, complete: true });
    const ui = render(LibrarySetting, 'en');
    await ui.click(ui.button('Change'));
    expect(ui.target.textContent).toContain('It holds 1 note.');
    expect(ui.target.textContent).toContain('This is already the library of this PC: nothing changes.');
    expect([...ui.target.querySelectorAll('button')].some((b) => b.textContent?.includes('Use this folder'))).toBe(false);
  });

  it('says when a very large folder was not counted to the end', async () => {
    mockFolders.set(OTHER, { existing: false, notes: 120, complete: false });
    const ui = render(LibrarySetting);
    await ui.click(ui.button('Changer'));
    expect(ui.target.textContent).toContain('Il contient au moins 120 notes');
  });

  it('shows why a folder cannot be used', async () => {
    vi.spyOn(api, 'inspectLibrary').mockRejectedValue({ code: 'notAFolder', message: OTHER });
    const ui = render(LibrarySetting);
    await ui.click(ui.button('Changer'));
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toContain("Impossible d'utiliser ce dossier");
    expect(ui.target.querySelector('[role="alertdialog"]')).toBeNull();
  });
});

describe('first-run wizard', () => {
  it('counts the notes of the chosen folder too', async () => {
    mockFolders.set(OTHER, { existing: false, notes: 3, complete: true });
    const ui = render(Wizard, 'fr', { ...app, library: null });
    await ui.click([...ui.target.querySelectorAll('button')].find((b) => b.textContent?.includes('Choisir un dossier'))!);
    expect(ui.target.textContent).toContain('Il contient 3 notes.');
  });
});

describe('sameFolder', () => {
  it('compares folders as Windows does', () => {
    expect(sameFolder('C:\\A\\B', 'c:/a/b/')).toBe(true);
    expect(sameFolder('C:\\A\\B', 'C:\\A\\BC')).toBe(false);
    expect(sameFolder(null, 'C:\\A')).toBe(false);
  });
});
