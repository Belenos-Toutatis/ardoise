// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';
import { api, type AppState } from './lib/api';
import { dismissGuideNotice } from './lib/guide.svelte';
import { setLocale } from './lib/i18n.svelte';
import { mockGuide } from './lib/mock-about';
import { resetMockLibrary } from './lib/mock-data';
import { resetMockTablet } from './lib/mock-tablet';
import { tablet } from './lib/tablet-state.svelte';

// The guide's entry points of the main window and F1, over the whole application: the library's top bar, the notice of
// the web fallback (shown over any screen) and the key. The Aide tabs and the settings' Assistant section have their own
// tests next to their components.

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'fr',
  theme: 'system',
  library: 'C:\\demo\\Ardoise',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  mockGuide.asked = [];
  mockGuide.source = 'local';
  mockGuide.fail = false;
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  dismissGuideNotice();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetMockTablet();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(language: 'fr' | 'en' = 'fr') {
  vi.spyOn(api, 'appState').mockResolvedValue({ ...app, language });
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(App, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('header.topbar')).not.toBeNull());
  await settle();
  return target;
}

const topbarButton = (target: HTMLElement) => target.querySelector<HTMLButtonElement>('header.topbar button[data-action="open-guide"]')!;
const notice = (target: HTMLElement) => target.querySelector<HTMLElement>('[data-slot="guide-notice"]');
const press = (init: KeyboardEventInit) => {
  const event = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true });
  window.dispatchEvent(event);
  return event;
};

describe('the library "Guide" button', () => {
  it('is in the top bar, named and described, in French', async () => {
    const target = await render('fr');
    const button = topbarButton(target);
    expect(button.textContent?.trim()).toBe('Guide');
    expect(button.getAttribute('aria-label')).toBe("Guide d'utilisation");
    expect(button.title).toContain('F1');
    expect(button.title).toContain('GitHub');
    expect(button.title).toContain('hors connexion');
  });

  it('is in the top bar, named and described, in English', async () => {
    const target = await render('en');
    const button = topbarButton(target);
    expect(button.textContent?.trim()).toBe('Guide');
    expect(button.getAttribute('aria-label')).toBe('User guide');
    expect(button.title).toContain('F1');
    expect(button.title).toContain('GitHub');
    expect(button.title).toContain('offline');
  });

  it('opens the guide in the language of the interface', async () => {
    const target = await render('en');
    topbarButton(target).click();
    await settle();
    expect(mockGuide.asked).toEqual([{ page: 'guide', language: 'en' }]);
    expect(notice(target)).toBeNull();
  });
});

describe('F1', () => {
  it("opens the guide, and the browser's own handling of the key is cancelled", async () => {
    const target = await render('fr');
    const event = press({ key: 'F1', code: 'F1' });
    expect(event.defaultPrevented).toBe(true);
    await settle();
    expect(mockGuide.asked).toEqual([{ page: 'guide', language: 'fr' }]);
    expect(notice(target)).toBeNull();
  });

  it('works from a text field, once while the key is held, and not as another chord', async () => {
    const target = await render('fr');
    const search = target.querySelector<HTMLInputElement>('input[type="search"]')!;
    search.focus();
    search.dispatchEvent(new KeyboardEvent('keydown', { key: 'F1', bubbles: true, cancelable: true }));
    await settle();
    expect(mockGuide.asked).toHaveLength(1);
    press({ key: 'F1', repeat: true });
    press({ key: 'F1', ctrlKey: true });
    press({ key: 'F1', shiftKey: true });
    press({ key: 'F1', altKey: true });
    press({ key: 'F2' });
    await settle();
    expect(mockGuide.asked).toHaveLength(1);
  });
});

describe('the notice when the guide came from GitHub', () => {
  it('says so, in French, and can be closed', async () => {
    mockGuide.source = 'web';
    const target = await render('fr');
    topbarButton(target).click();
    await vi.waitFor(() => expect(notice(target)).not.toBeNull());
    const shown = notice(target)!;
    expect(shown.getAttribute('role')).toBe('status');
    expect(shown.textContent).toContain('Guide ouvert sur GitHub');
    expect(shown.textContent).toContain('le dossier « guide » est absent');
    shown.querySelector<HTMLButtonElement>('button[data-action="dismiss-guide-notice"]')!.click();
    await settle();
    expect(notice(target)).toBeNull();
  });

  it('says so in English, whichever way the guide was asked for (F1 here)', async () => {
    mockGuide.source = 'web';
    const target = await render('en');
    press({ key: 'F1' });
    await vi.waitFor(() => expect(notice(target)).not.toBeNull());
    expect(notice(target)!.textContent).toContain('Guide opened on GitHub');
    expect(notice(target)!.querySelector('button')!.textContent?.trim()).toBe('Close this message');
  });

  it('goes by itself after a while', async () => {
    mockGuide.source = 'web';
    const target = await render('fr');
    vi.useFakeTimers();
    topbarButton(target).click();
    await vi.advanceTimersByTimeAsync(50);
    expect(notice(target)).not.toBeNull();
    await vi.advanceTimersByTimeAsync(13_000);
    expect(notice(target)).toBeNull();
  });

  it('says it when the browser could not be started', async () => {
    mockGuide.fail = true;
    const target = await render('fr');
    topbarButton(target).click();
    await vi.waitFor(() => expect(notice(target)).not.toBeNull());
    expect(notice(target)!.getAttribute('data-state')).toBe('failed');
    expect(notice(target)!.textContent).toContain("Le navigateur n'a pas pu ouvrir le guide");
  });
});
