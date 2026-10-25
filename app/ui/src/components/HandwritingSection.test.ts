// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockLibrary } from '../lib/mock-data';
import {
  ENGLISH_RECOGNIZER,
  FRENCH_RECOGNIZER,
  mockHandwriting,
  resetMockHandwriting,
} from '../lib/mock-handwriting';
import HandwritingSection from './HandwritingSection.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockHandwriting();
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function settle() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(HandwritingSection, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const slot = (name: string) => target.querySelector<HTMLElement>(`[data-slot="${name}"]`);
  const button = (text: string) => [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
  return { target, slot, button };
}

describe('settings: handwriting recognition', () => {
  it('shows the recognizers of this PC and the automatic language', async () => {
    const ui = await render('fr');
    expect(ui.target.querySelector('h3')?.textContent).toBe("Reconnaissance d'écriture");
    expect(ui.slot('recognizers')?.textContent).toContain("Reconnaissance d'écriture Microsoft - Français");
    const select = ui.slot('language') as HTMLSelectElement;
    expect(select.value).toBe('auto');
    expect([...select.options].map((o) => o.textContent?.trim())).toEqual([
      'Automatique (Français)',
      'Français',
      'Anglais',
    ]);
    expect(ui.slot('missing')).toBeNull();
    expect(ui.slot('status')?.textContent).toBe('Toutes les notes sont lues (notes : 3).');
  });

  it('explains how to add the English pack when English is chosen and missing', async () => {
    const ui = await render('fr');
    const select = ui.slot('language') as HTMLSelectElement;
    select.value = 'en';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(mockHandwriting.setting).toBe('en');
    const missing = ui.slot('missing')?.textContent ?? '';
    expect(missing).toContain('(Anglais)');
    expect(missing).toContain('Heure et langue > Langue et région');
    expect(missing).toContain('Écriture manuscrite');
    expect(ui.slot('status')?.textContent).toContain('pas de module de reconnaissance');
    // Installed: no more explanation.
    mockHandwriting.recognizers = [FRENCH_RECOGNIZER, ENGLISH_RECOGNIZER];
    cleanup?.();
    const again = await render('en');
    expect(again.slot('missing')).toBeNull();
    expect((again.slot('language') as HTMLSelectElement).value).toBe('en');
  });

  it('in English with only the French pack, auto falls back to French and says how to add English', async () => {
    mockHandwriting.interfaceLanguage = 'en';
    const ui = await render('en');
    const select = ui.slot('language') as HTMLSelectElement;
    expect(select.options[0].textContent?.trim()).toBe('Automatic (French)');
    expect(ui.slot('missing')?.textContent).toContain('Settings > Time & language > Language & region');
  });

  it('stops and resumes the indexing', async () => {
    mockHandwriting.status = { state: 'indexing', reason: null, language: 'fr', done: 1, total: 3, failed: 0 };
    const ui = await render('fr');
    expect(ui.slot('status')?.textContent).toBe("Lecture de l'écriture en cours (notes lues : 1 sur 3).");
    mockHandwriting.status = null;
    ui.button('Arrêter la lecture')!.click();
    await settle();
    expect(mockHandwriting.stopped).toBe(true);
    expect(ui.slot('status')?.textContent).toBe('Lecture arrêtée (notes lues : 3 sur 3).');
    ui.button('Reprendre la lecture')!.click();
    await settle();
    expect(mockHandwriting.stopped).toBe(false);
    expect(ui.slot('status')?.textContent).toBe('Toutes les notes sont lues (notes : 3).');
  });

  it('says that recognition needs Windows elsewhere', async () => {
    mockHandwriting.supported = false;
    const ui = await render('fr');
    expect(ui.slot('unsupported')?.textContent).toBe("La reconnaissance d'écriture n'est disponible que sous Windows.");
    expect(ui.slot('language')).toBeNull();
  });
});
