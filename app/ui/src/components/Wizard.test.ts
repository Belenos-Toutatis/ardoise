// @vitest-environment jsdom
import { mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { inkspaceOffer } from '../lib/inkspace-offer.svelte';
import { mockInkspace, resetMockInkspace } from '../lib/mock-inkspace';
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
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
};

let cleanup: (() => void) | null = null;
beforeEach(() => {
  resetMockInkspace();
  inkspaceOffer.pending = false;
});
afterEach(() => {
  cleanup?.();
  vi.restoreAllMocks();
});

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

describe('first-run offer to import Wacom Inkspace', () => {
  const offer = (target: HTMLElement) => target.querySelector('[data-slot="wizard-inkspace"]');
  const checkbox = (target: HTMLElement) => target.querySelector<HTMLInputElement>('[data-slot="wizard-inkspace"] input')!;

  async function continueWith(checked: boolean) {
    setLocale('fr');
    const onchange = vi.fn();
    const { target, button } = render(onchange);
    await vi.waitFor(() => expect(offer(target)).toBeTruthy());
    if (checkbox(target).checked !== checked) checkbox(target).click();
    button('Choisir un dossier').click();
    await vi.waitFor(() => expect(target.textContent).toContain(FOLDER));
    button('Continuer').click();
    await vi.waitFor(() => expect(onchange).toHaveBeenCalledOnce());
  }

  it('offers the notes found, with their count, checked by default', async () => {
    setLocale('fr');
    const { target } = render(vi.fn());
    await vi.waitFor(() => expect(offer(target)).toBeTruthy());
    expect(offer(target)!.textContent).toContain('Ardoise a trouvé des notes de Wacom Inkspace sur cet ordinateur (5 notes).');
    expect(offer(target)!.textContent).toContain('Les importer dans la bibliothèque (classeur « Inkspace »)');
    expect(checkbox(target).checked).toBe(true);
  });

  it('still offers without a count when the folder cannot be read', async () => {
    setLocale('fr');
    vi.spyOn(api, 'inkspaceInspect').mockRejectedValue({ code: 'inkspaceUnreadable', message: 'locked' });
    const { target } = render(vi.fn());
    await vi.waitFor(() => expect(offer(target)).toBeTruthy());
    expect(offer(target)!.textContent).toContain('Ardoise a trouvé des notes de Wacom Inkspace sur cet ordinateur.');
    expect(offer(target)!.textContent).not.toMatch(/\(\d+ notes?\)/);
  });

  it('shows nothing when Inkspace is not there', async () => {
    mockInkspace.detected = null;
    const detect = vi.spyOn(api, 'inkspaceDetect');
    const { target } = render(vi.fn());
    await vi.waitFor(() => expect(detect).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(offer(target)).toBeNull();
    expect(target.textContent).not.toContain('Inkspace');
  });

  it('asks the library screen to open the import only when the box is checked', async () => {
    await continueWith(true);
    expect(inkspaceOffer.pending).toBe(true);
    cleanup?.();
    inkspaceOffer.pending = false;

    await continueWith(false);
    expect(inkspaceOffer.pending).toBe(false);
  });

  it('speaks English', async () => {
    setLocale('en');
    const { target } = render(vi.fn());
    await vi.waitFor(() => expect(offer(target)).toBeTruthy());
    expect(offer(target)!.textContent).toContain('found Wacom Inkspace notes on this computer (5 notes).');
    expect(offer(target)!.textContent).toContain('Import them into the library (binder “Inkspace”)');
  });
});
