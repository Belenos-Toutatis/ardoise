// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import Home from '../Home.svelte';

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
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  mockTablet.inRange = true;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  resetMockTablet();
  vi.useRealTimers();
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await tick();
  }
}

async function render() {
  setLocale('fr');
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app, onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  return target;
}

describe('the tablet in the main window', () => {
  it('shares one subscription between the sidebar and the panel, which both follow a sync', async () => {
    const subscribe = vi.spyOn(api, 'tabletSubscribe');
    const target = await render();
    const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);

    expect(q('.tablet-panel')).toBeNull();
    q('.tablet-status')!.click();
    await settle();
    expect(q('.tablet-panel')).not.toBeNull();
    expect(q('.tablet-status')!.getAttribute('aria-expanded')).toBe('true');
    expect(subscribe).toHaveBeenCalledOnce();

    // The tablet comes into range and syncs its pages by itself (automatic sync is on).
    await vi.advanceTimersByTimeAsync(2000);
    await settle();
    expect(q('.tablet-status')!.getAttribute('data-connection')).toBe('syncing');
    expect(q('.tablet-panel')!.getAttribute('data-connection')).toBe('syncing');
    expect(q('.tablet-status progress')).not.toBeNull();
    expect(q('.tablet-panel [data-slot="sync-progress"]')).not.toBeNull();

    await vi.advanceTimersByTimeAsync(3000);
    await settle();
    expect(q('.tablet-status')!.getAttribute('data-connection')).toBe('connected');
    expect(q('.tablet-panel [data-slot="last-sync"]')!.textContent).toContain('3 notes enregistrées.');
    // The three pages are in the library grid beside the three demo notes.
    expect(target.querySelectorAll('.note-card').length).toBe(6);

    q('.tablet-status')!.click();
    await settle();
    expect(q('.tablet-panel')).toBeNull();
    expect(subscribe).toHaveBeenCalledOnce();
  });

  it('opens the registration from the panel when no tablet is registered', async () => {
    await api.forgetTablet();
    const target = await render();
    target.querySelector<HTMLElement>('.tablet-status')!.click();
    await settle();
    target.querySelector<HTMLElement>('button[data-action="register"]')!.click();
    await settle();
    expect(target.querySelector('.register-wizard [data-step]')?.getAttribute('data-step')).toBe('warning');
  });
});
