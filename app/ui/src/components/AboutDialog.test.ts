// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../lib/api';
import { setLocale, t } from '../lib/i18n.svelte';
import { MOCK_COMPONENTS } from '../lib/mock-about';
import { resetMockVideo, setMockOpenH264 } from '../lib/mock-video';
import AboutSection from './AboutSection.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockVideo();
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
});

async function settle() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** The Settings section, with the dialog already opened unless `open` is false. */
async function render(locale: 'fr' | 'en' = 'fr', open = true) {
  cleanup?.();
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(AboutSection, { target, props: { version: '0.1.0' } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  if (open) {
    click(document.body, '[data-action="open-about"]');
    await settle();
  }
  return target;
}

const dialog = () => document.body.querySelector<HTMLElement>('[role="dialog"]')!;
const slot = (name: string) => dialog().querySelector(`[data-slot="${name}"]`)?.textContent?.trim();
function click(root: ParentNode, selector: string) {
  const element = root.querySelector<HTMLElement>(selector);
  if (!element) throw new Error(`nothing matches ${selector}`);
  element.click();
}
async function search(text: string) {
  const input = dialog().querySelector<HTMLInputElement>('input[type="search"]')!;
  input.value = text;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await settle();
}
const rows = () => [...dialog().querySelectorAll('tr[data-component]')].map((r) => r.getAttribute('data-component'));

describe('settings: about', () => {
  it('shows the version and the licence in Settings, and opens the screen with a button', async () => {
    const target = await render('fr', false);
    expect(target.querySelector('h3')?.textContent).toBe('À propos');
    expect(target.querySelector('[data-slot="about-version"]')?.textContent).toBe('Ardoise, version 0.1.0');
    expect(target.textContent).toContain('GPL-3.0');
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();

    click(target, '[data-action="open-about"]');
    await settle();
    expect(dialog().querySelector('h2')?.textContent).toBe("À propos d'Ardoise");
  });

  it('closes with its button and with Escape', async () => {
    await render();
    click(dialog(), '[data-autofocus]');
    await settle();
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();

    click(document.body, '[data-action="open-about"]');
    await settle();
    expect(dialog()).not.toBeNull();
    dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  });

  it('closes with Ctrl + W and Ctrl + F4, like every dialog', async () => {
    await render();
    for (const key of ['w', 'F4']) {
      if (!document.body.querySelector('[role="dialog"]')) click(document.body, '[data-action="open-about"]');
      await settle();
      expect(dialog()).not.toBeNull();
      dialog().dispatchEvent(new KeyboardEvent('keydown', { key, ctrlKey: true, bubbles: true }));
      await settle();
      expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    }
  });

  it('says what the licence is, in plain words, in both languages', async () => {
    await render('fr');
    expect(slot('version')).toContain('version 0.0.0-browser');
    expect(slot('license')).toContain('GPL-3.0-or-later');
    expect(slot('license-meaning')).toBe(t('about.license.meaning'));
    expect(slot('license-meaning')).toContain('sans aucune garantie');
    await render('en');
    expect(slot('license-meaning')).toContain('no warranty');
    expect(dialog().querySelector('h2')?.textContent).toBe('About Ardoise');
  });

  it('shows the full licence only when asked, as a disclosure the keyboard can use', async () => {
    const asked = vi.spyOn(api, 'aboutText');
    await render();
    const button = dialog().querySelector<HTMLButtonElement>('[data-action="toggle-license"]')!;
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(asked).not.toHaveBeenCalled();

    button.click();
    await settle();
    expect(asked).toHaveBeenCalledWith('license');
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const pre = dialog().querySelector<HTMLElement>('[data-slot="license-text"]')!;
    expect(pre.textContent).toContain('GNU GENERAL PUBLIC LICENSE');
    expect(pre.getAttribute('tabindex')).toBe('0');

    button.click();
    await settle();
    expect(dialog().querySelector('[data-slot="license-text"]')).toBeNull();
    button.click();
    await settle();
    // Kept: the text is not fetched twice.
    expect(asked).toHaveBeenCalledTimes(1);
  });

  it('credits Tuhi with its licence, and says the code is not Tuhi code', async () => {
    await render('en');
    const credit = slot('tuhi')!;
    expect(credit).toContain('Tuhi');
    expect(credit).toContain('GPL-2.0 or later');
    expect(credit).toContain('contains no Tuhi code');
    await render('fr');
    expect(slot('tuhi')).toContain('GPL-2.0 ou ultérieure');
  });

  it('shows the Cisco notice as is, that the user downloads it, and whether it is installed', async () => {
    await render('fr');
    expect(slot('cisco-notice')).toBe('OpenH264 Video Codec provided by Cisco Systems, Inc.');
    expect(slot('cisco-download')).toContain('Ardoise ne contient pas OpenH264');
    expect(slot('cisco-download')).toContain('chez Cisco');
    expect(slot('cisco-status')).toBe('Pas installé sur cet ordinateur.');

    setMockOpenH264({ status: 'installed', bytes: 860_000, allowed: true });
    const installed = await api.openH264State();
    await render('en');
    expect(slot('cisco-status')).toBe(`Installed on this computer (version ${installed.version}).`);

    setMockOpenH264({ status: 'damaged' });
    await render('en');
    expect(slot('cisco-status')).toContain('damaged');
  });

  it('lists the components with their versions and licences, and filters them', async () => {
    await render('fr');
    expect(rows()).toEqual(MOCK_COMPONENTS.map((c) => c.name));
    expect(slot('component-count')).toBe(`Composants affichés : ${MOCK_COMPONENTS.length} sur ${MOCK_COMPONENTS.length}`);
    const serde = dialog().querySelector('tr[data-component="serde"]')!;
    expect(serde.textContent).toContain('1.0.229');
    expect(serde.textContent).toContain('MIT OR Apache-2.0');

    await search('mpl');
    expect(rows()).toEqual(['webm-sys']);
    expect(slot('component-count')).toBe(`Composants affichés : 1 sur ${MOCK_COMPONENTS.length}`);

    await search('introuvable');
    expect(rows()).toEqual([]);
    expect(slot('no-component')).toBe('Aucun composant ne correspond.');

    await search('');
    expect(rows().length).toBe(MOCK_COMPONENTS.length);
  });

  it('labels the search field and the scrolling areas for the keyboard and screen readers', async () => {
    await render('en');
    expect(dialog().querySelector('input[type="search"]')?.getAttribute('aria-label')).toBe('Filter the components');
    const list = dialog().querySelector('.about-components')!;
    expect(list.getAttribute('tabindex')).toBe('0');
    expect(list.getAttribute('role')).toBe('region');
    expect([...dialog().querySelectorAll('thead th')].every((th) => th.getAttribute('scope') === 'col')).toBe(true);
    click(dialog(), '[data-action="toggle-notices"]');
    await settle();
    const notices = dialog().querySelector('[data-slot="notices-text"]')!;
    expect(notices.getAttribute('aria-label')).toBe('Licence texts of the components (THIRD-PARTY-NOTICES file)');
    expect(notices.getAttribute('tabindex')).toBe('0');
  });

  it('shows the full notices on demand', async () => {
    await render('fr');
    click(dialog(), '[data-action="toggle-notices"]');
    await settle();
    expect(dialog().querySelector('[data-slot="notices-text"]')?.textContent).toContain('THIRD-PARTY NOTICES');
    expect(dialog().querySelector('[data-action="toggle-notices"]')?.textContent?.trim()).toBe(
      'Masquer les textes des licences des composants',
    );
  });

  it('reports a failure with its translated sentence and stays usable', async () => {
    vi.spyOn(api, 'aboutText').mockRejectedValue({ code: 'unexpected', message: 'no such text' });
    await render('fr');
    click(dialog(), '[data-action="toggle-license"]');
    await settle();
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain('erreur inattendue (no such text)');
    expect(dialog().querySelector('[data-slot="license-text"]')).toBeNull();
    expect(dialog().querySelector('[data-action="toggle-license"]')?.getAttribute('aria-expanded')).toBe('false');
    expect(rows().length).toBe(MOCK_COMPONENTS.length);
  });

  it('reports a failure to read the components', async () => {
    vi.spyOn(api, 'aboutInfo').mockRejectedValue({ code: 'unexpected', message: 'boom' });
    await render('en');
    expect(dialog().querySelector('[role="alert"]')?.textContent).toContain('unexpected error (boom)');
    expect(dialog().querySelector('input[type="search"]')).toBeNull();
    // The rest of the screen does not depend on the list.
    expect(slot('tuhi')).toContain('Tuhi');
  });
});
