// @vitest-environment jsdom
import { mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import Wizard from './Wizard.svelte';

const FOLDER = 'C:\\Users\\demo\\Nextcloud\\Ardoise';

// Stands in for the native folder picker.
vi.mock('../lib/library', () => ({ pickFolder: vi.fn(async () => FOLDER) }));

const firstRun: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: null,
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
};

let cleanup: (() => void) | null = null;
afterEach(() => cleanup?.());

function render(onchange: (next: AppState) => void) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Wizard, { target, props: { app: firstRun, onchange } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.includes(label));
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  return { target, button };
}

describe('first-run wizard', () => {
  it('lets the user pick a folder and saves it as this computer’s library', async () => {
    setLocale('fr');
    const onchange = vi.fn();
    const { target, button } = render(onchange);

    expect(target.textContent).toContain('Bienvenue dans Ardoise');
    expect(target.textContent).toContain('PC-TEST');
    expect(target.textContent).not.toContain('Continuer');

    button('Choisir un dossier').click();
    await vi.waitFor(() => expect(target.textContent).toContain(FOLDER));
    expect(target.textContent).toContain('Une nouvelle bibliothèque sera créée');

    button('Continuer').click();
    await vi.waitFor(() => expect(onchange).toHaveBeenCalledOnce());
    expect(onchange.mock.calls[0][0].library).toBe(FOLDER);
  });

  it('recognises a folder that already holds a library, in English', async () => {
    setLocale('en');
    await api.setLibrary(FOLDER);
    const { target, button } = render(vi.fn());

    expect(target.textContent).toContain('Welcome to Ardoise');
    button('Choose a folder').click();
    await vi.waitFor(() => expect(target.textContent).toContain('already contains an Ardoise library'));
  });
});
