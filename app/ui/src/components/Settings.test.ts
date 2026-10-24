// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockLibrary } from '../lib/mock-data';
import Settings from './Settings.svelte';

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

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(locale: 'fr' | 'en', ontest?: () => void) {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const onchange = vi.fn();
  const component = mount(Settings, { target, props: { app, onchange, onclose: vi.fn(), ontest } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const button = (text: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  await settle();
  return { target, button, onchange };
}

describe('settings: tablet section', () => {
  it('shows the registered tablet', async () => {
    const { target } = await render('fr');
    const section = target.querySelector('[aria-labelledby="settings-tablet"]')!;
    expect(section.querySelector('h3')?.textContent).toBe('Tablette');
    expect(section.textContent).toContain('Bamboo Slate');
    expect(section.textContent).toContain('AA:BB:CC:DD:EE:FF');
    expect(section.textContent).toContain('297 × 216 mm');
    expect(section.textContent).toMatch(/12 oct\. 2026/);
  });

  it('forgets the tablet only after a confirmation', async () => {
    const { target, button } = await render('fr');
    button('Oublier la tablette').click();
    await settle();
    expect(target.querySelector('[role="alertdialog"]')?.textContent).toContain('Oublier la tablette « Bamboo Slate » ?');
    expect(await api.tabletInfo()).not.toBeNull();

    button('Annuler').click();
    await settle();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(await api.tabletInfo()).not.toBeNull();

    button('Oublier la tablette').click();
    await settle();
    button("Oui, l'oublier").click();
    await settle();
    expect(await api.tabletInfo()).toBeNull();
    expect(target.textContent).toContain("Aucune tablette n'est enregistrée.");
    expect(target.textContent).not.toContain('Oublier la tablette');
  });

  it('says so when no tablet is registered, in English', async () => {
    await api.forgetTablet();
    const { target } = await render('en');
    expect(target.textContent).toContain('No tablet is registered.');
    expect(target.textContent).not.toContain('Forget the tablet');
  });

  it('keeps its English labels', async () => {
    const { target, button } = await render('en');
    expect(target.textContent).toContain('Registered on');
    button('Forget the tablet').click();
    await settle();
    expect(target.textContent).toContain('Forget the tablet “Bamboo Slate”?');
  });

  it('opens the tablet test', async () => {
    const ontest = vi.fn();
    const { button } = await render('fr', ontest);
    button('Tester la tablette…').click();
    expect(ontest).toHaveBeenCalledOnce();
  });

  it('still offers language, theme and library', async () => {
    const { target } = await render('fr');
    expect(target.querySelectorAll('select').length).toBe(2);
    expect(target.textContent).toContain('C:\\demo\\Ardoise');
  });
});

describe('settings: automatic export', () => {
  afterEach(async () => {
    await api.setAutoExport({ format: 'off', folder: 'Exports' });
  });

  const section = (target: HTMLElement) => target.querySelector<HTMLElement>('[aria-labelledby="settings-auto-export"]')!;
  const radio = (target: HTMLElement, label: string) => {
    const found = [...section(target).querySelectorAll<HTMLLabelElement>('label.choice')].find((l) => l.textContent?.trim() === label);
    if (!found) throw new Error(`no choice "${label}"`);
    return found.querySelector('input')!;
  };
  const folder = (target: HTMLElement) => section(target).querySelector<HTMLInputElement>('input[type="text"]')!;
  const type = async (input: HTMLInputElement, value: string) => {
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  const save = (target: HTMLElement) =>
    [...section(target).querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Enregistrer')!;

  it('explains the feature and shows the current choice', async () => {
    const { target } = await render('fr');
    expect(section(target).querySelector('h3')?.textContent).toBe('Export automatique après synchronisation');
    expect(section(target).textContent).toContain('application Nextcloud');
    expect(radio(target, 'Désactivé').checked).toBe(true);
    expect(radio(target, 'PDF').checked).toBe(false);
    expect(folder(target).value).toBe('Exports');
    expect(save(target).disabled).toBe(true);
  });

  it('saves the format and folder, reports the new state, and confirms', async () => {
    const spy = vi.spyOn(api, 'setAutoExport');
    const { target, onchange } = await render('fr');
    radio(target, 'PDF').click();
    await settle();
    await type(folder(target), ' /Téléphone\\PDF/ ');
    expect(save(target).disabled).toBe(false);
    save(target).click();
    await settle();
    expect(spy).toHaveBeenCalledWith({ format: 'pdf', folder: ' /Téléphone\\PDF/ ' });
    expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ autoExport: { format: 'pdf', folder: 'Téléphone/PDF' } }));
    // The field shows the folder as the app stored it.
    expect(folder(target).value).toBe('Téléphone/PDF');
    expect(section(target).textContent).toContain('Réglage enregistré.');
    expect((await api.appState()).autoExport).toEqual({ format: 'pdf', folder: 'Téléphone/PDF' });
  });

  it('refuses a folder outside the library with an actionable message', async () => {
    const { target, onchange } = await render('fr');
    radio(target, 'PNG (image)').click();
    await settle();
    await type(folder(target), '../dehors');
    save(target).click();
    await settle();
    const alert = section(target).querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain("ce dossier n'est pas utilisable");
    expect(alert.textContent).toContain('par exemple « Exports »');
    expect(onchange).not.toHaveBeenCalled();
    expect(section(target).textContent).not.toContain('Réglage enregistré.');
    expect((await api.appState()).autoExport.format).toBe('off');

    // Fixing the folder works, and clears the message.
    await type(folder(target), 'Exports');
    save(target).click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')).toBeNull();
    expect(onchange).toHaveBeenCalledOnce();
  });

  it('refuses the library\'s own folder', async () => {
    const { target } = await render('fr');
    await type(folder(target), '_ardoise/exports');
    save(target).click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')?.textContent).toContain("ce dossier n'est pas utilisable");
  });

  it('speaks English', async () => {
    const { target } = await render('en');
    expect(section(target).querySelector('h3')?.textContent).toBe('Automatic export after sync');
    expect(radio(target, 'Off').checked).toBe(true);
    expect(section(target).textContent).toContain('Nextcloud app');
    await type(folder(target), '..');
    [...section(target).querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Save')!.click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')?.textContent).toContain("this folder can't be used");
  });
});
