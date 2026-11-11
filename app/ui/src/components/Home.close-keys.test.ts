// @vitest-environment jsdom
/**
 * The keys that close, in the main window: Escape, Ctrl + W and Ctrl + F4 close the side panel and
 * nothing else, and closing the window asks first while work is under way in the library.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../lib/api';
import type { WindowClose } from '../lib/editor/window-close';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockLibrary } from '../lib/mock-data';
import { resetMockTablet } from '../lib/mock-tablet';
import { share } from '../lib/share-state.svelte';
import { tablet } from '../lib/tablet-state.svelte';
import { beginWork, workInProgress } from '../lib/work';
import Home from './Home.svelte';

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
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;
const destroy = vi.fn(async () => {});
let guard: (() => boolean) | null = null;
const windowClose: WindowClose = {
  subscribe: async (given) => {
    guard = given;
    return () => {
      guard = null;
    };
  },
  destroy,
};

async function settle() {
  for (let i = 0; i < 6; i++) {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function showHome(state: AppState = app) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app: state, onchange: () => {}, windowClose } });
  cleanup = () => {
    void unmount(component);
    target.remove();
  };
  await settle();
  return target;
}

async function press(key: string, init: KeyboardEventInit = {}) {
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
  await settle();
}

/** The settings panel, opened by its button. */
async function openSettings(target: HTMLElement) {
  target.querySelector<HTMLButtonElement>('button[aria-expanded]')?.click();
  await settle();
  expect(target.querySelector('button[aria-expanded="true"]')).not.toBeNull();
}

const panelOpen = (target: HTMLElement) => target.querySelector('button[aria-expanded="true"]') !== null;

beforeEach(() => {
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  share.reset();
  setLocale('fr');
  destroy.mockClear();
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  share.reset();
  document.body.replaceChildren();
});

describe('the keys that close, in the library', () => {
  for (const [name, key, init] of [
    ['Escape', 'Escape', {}],
    ['Ctrl + W', 'w', { ctrlKey: true }],
    ['Ctrl + F4', 'F4', { ctrlKey: true }],
  ] as const) {
    it(`${name} closes the side panel`, async () => {
      const target = await showHome();
      await openSettings(target);
      await press(key, init);
      expect(panelOpen(target)).toBe(false);
    });
  }

  it('with nothing open they do nothing: the window stays', async () => {
    const target = await showHome();
    await press('w', { ctrlKey: true });
    await press('Escape');
    expect(target.querySelector('.shell')).not.toBeNull();
    expect(destroy).not.toHaveBeenCalled();
  });

  it('a key an element took is not for the panel', async () => {
    const target = await showHome();
    await openSettings(target);
    const event = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    event.preventDefault();
    window.dispatchEvent(event);
    await settle();
    expect(panelOpen(target)).toBe(true);
  });

  it('a tablet test under way only stops by its button; once it is over, the keys close the panel', async () => {
    const target = await showHome({ ...app, openDiagnostic: true });
    const test = () => target.querySelector('aside.panel.wide');
    expect(test()).not.toBeNull();
    const end = beginWork();
    await press('Escape');
    await press('w', { ctrlKey: true });
    expect(test()).not.toBeNull();
    end();
    expect(workInProgress()).toBe(false);
    await press('Escape');
    expect(test()).toBeNull();
  });
});

describe('closing the window from the library', () => {
  it('goes through with nothing under way', async () => {
    await showHome();
    expect(guard?.()).toBe(false);
    await settle();
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('asks while work is under way: « Attendre » keeps the window, « Quitter quand même » closes it', async () => {
    await showHome();
    const end = beginWork();
    expect(guard?.()).toBe(true);
    await settle();
    const question = () => document.querySelector('[role="alertdialog"]');
    expect(question()?.textContent).toContain('Une tâche est en cours');

    document.querySelector<HTMLButtonElement>('[data-action="quit-wait"]')?.click();
    await settle();
    expect(question()).toBeNull();
    expect(destroy).not.toHaveBeenCalled();

    expect(guard?.()).toBe(true);
    await settle();
    // Under a question, the keys close the question, like Escape.
    await press('w', { ctrlKey: true });
    expect(question()).toBeNull();
    expect(destroy).not.toHaveBeenCalled();

    expect(guard?.()).toBe(true);
    await settle();
    document.querySelector<HTMLButtonElement>('[data-action="quit-anyway"]')?.click();
    await settle();
    expect(destroy).toHaveBeenCalledOnce();
    end();
  });

  it('stops listening when the library goes away', async () => {
    await showHome();
    expect(guard).not.toBeNull();
    cleanup?.();
    cleanup = null;
    await settle();
    expect(guard).toBeNull();
  });
});
