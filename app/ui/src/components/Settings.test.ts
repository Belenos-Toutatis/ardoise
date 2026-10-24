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
  const component = mount(Settings, { target, props: { app, onchange: vi.fn(), onclose: vi.fn(), ontest } });
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
  return { target, button };
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
