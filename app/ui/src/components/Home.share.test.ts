// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The main window and the share on the network (M11), with the real editor and live view: what
 * the share shows follows what the window shows (library → waiting, note → the note, live mode →
 * live), the share panel opens from the library's indicator, and the order of the effects holds
 * when one screen replaces another.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { FakeContext } from '../lib/live-fake-canvas';
import { resetMockLibrary } from '../lib/mock-data';
import { mockLiveConfig, resetMockLive } from '../lib/mock-live';
import { resetMockNarration } from '../lib/mock-narration';
import { mockShareSetViewers, mockShareShown, resetMockShare } from '../lib/mock-share';
import { resetMockTablet } from '../lib/mock-tablet';
import { share } from '../lib/share-state.svelte';
import { tablet } from '../lib/tablet-state.svelte';
import Home from './Home.svelte';

vi.mock('../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

vi.mock('./VideoExportDialog.svelte', async () => ({
  default: (await import('./editor/VideoExportDialogDouble.svelte')).default,
}));

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

beforeEach(() => {
  resetMockLibrary();
  resetMockTablet();
  resetMockLive();
  resetMockNarration();
  resetMockShare();
  tablet.reset();
  share.reset();
  setLocale('fr');
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
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
  resetMockShare();
  resetMockLive();
  resetMockTablet();
  vi.restoreAllMocks();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app, onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const openNote = async () => {
    target.querySelector<HTMLButtonElement>('.note-card .card-open')!.click();
    await vi.waitFor(() => expect(target.querySelector('section.editor svg.canvas')).toBeTruthy());
  };
  const backToLibrary = async () => {
    [...target.querySelectorAll<HTMLButtonElement>('section.editor .editor-top button')].find((b) => b.textContent?.includes('Bibliothèque'))!.click();
    await vi.waitFor(() => expect(target.querySelector('section.editor')).toBeNull());
    await settle();
  };
  const openLive = async () => {
    target.querySelector<HTMLElement>('.tablet-status')!.click();
    await settle();
    target.querySelector<HTMLButtonElement>('.tablet-panel button[data-action="live"]')!.click();
    await vi.waitFor(() => expect(target.querySelector('section.live-view')).toBeTruthy());
  };
  const stopLive = async () => {
    target.querySelector<HTMLButtonElement>('button[data-action="stop-live"]')!.click();
    await vi.waitFor(() => expect(target.querySelector('section.live-view')).toBeNull());
    await settle();
  };
  return { target, openNote, backToLibrary, openLive, stopLive };
}

async function startShare() {
  share.follow();
  expect(await share.start()).toBe(true);
  await settle();
  mockShareShown.length = 0;
}

const kinds = () => mockShareShown.map((content) => content.kind);
const lastKind = () => kinds()[kinds().length - 1];

describe('what the share shows follows what the main window shows', () => {
  it('shows the waiting screen for the library, the note when one is edited, then waiting again', async () => {
    const view = await render();
    await startShare();
    // A share started in the library gets « waiting » right away.
    share.showWaiting();
    await settle();
    expect(kinds()).toEqual(['waiting']);

    await view.openNote();
    await vi.waitFor(() => expect(lastKind()).toBe('note'));
    expect(share.status.shown).toBe('note');

    await view.backToLibrary();
    await vi.waitFor(() => expect(lastKind()).toBe('waiting'));
    expect(share.status.shown).toBe('waiting');
    // No note comes after it, not even a late debounced one.
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(lastKind()).toBe('waiting');
  });

  it('shows the live sheet in live mode, then waiting again in the library', async () => {
    const view = await render();
    await startShare();
    await view.openLive();
    await vi.waitFor(() => expect(lastKind()).toBe('live'));
    await view.stopLive();
    await vi.waitFor(() => expect(lastKind()).toBe('waiting'));
    expect(share.status.shown).toBe('waiting');
  });

  it('shows the note of an editor opened right after live mode ends', async () => {
    const view = await render();
    await startShare();
    await view.openLive();
    await vi.waitFor(() => expect(lastKind()).toBe('live'));
    await view.stopLive();
    await view.openNote();
    await vi.waitFor(() => expect(lastKind()).toBe('note'));
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(lastKind()).toBe('note');
    expect(share.status.shown).toBe('note');
  });

  it('starts a share begun in the editor with the note that is open', async () => {
    const view = await render();
    await view.openNote();
    await settle();
    share.follow();
    await share.start();
    await vi.waitFor(() => expect(kinds()).toEqual(['note']));
  });

  it('sends nothing while no share runs', async () => {
    const view = await render();
    await view.openNote();
    await view.backToLibrary();
    await view.openLive();
    await view.stopLive();
    expect(mockShareShown).toEqual([]);
  });
});

describe('the indicator and the panel in the main window', () => {
  it('shows no indicator while nothing is shared', async () => {
    const view = await render();
    expect(view.target.querySelector('.topbar .share-indicator')).toBeNull();
    expect(document.querySelector('.share-panel')).toBeNull();
  });

  it('shows the indicator with the number of screens, which reopens the panel', async () => {
    const view = await render();
    await startShare();
    const indicator = () => view.target.querySelector<HTMLButtonElement>('.topbar .share-indicator')!;
    expect(indicator()).not.toBeNull();
    expect(indicator().textContent).toContain('0');
    mockShareSetViewers(3);
    await settle();
    expect(indicator().textContent).toContain('3');
    expect(indicator().getAttribute('aria-label')).toContain('3 écrans connectés');
    expect(document.querySelector('.share-panel')).toBeNull();

    indicator().click();
    await settle();
    expect(document.querySelector('.share-panel')).not.toBeNull();
    expect(document.querySelector('[data-share-viewers="3"]')).not.toBeNull();

    // Escape closes the panel and the share goes on.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settle();
    expect(document.querySelector('.share-panel')).toBeNull();
    expect(share.status.active).toBe(true);
    expect(indicator()).not.toBeNull();
  });

  it('opens over the live view, from its button, and the share keeps running when it closes', async () => {
    const view = await render();
    await view.openLive();
    view.target.querySelector<HTMLButtonElement>('section.live-view button[data-action="share"]')!.click();
    await vi.waitFor(() => expect(document.querySelector('.share-panel [data-share-code]')).not.toBeNull());
    expect(share.status.active).toBe(true);
    await vi.waitFor(() => expect(share.status.shown).toBe('live'));

    document.querySelector<HTMLButtonElement>('button[data-action="share-stop"]')!.click();
    await settle();
    expect(share.status.active).toBe(false);
    expect(document.querySelector('.share-panel')).toBeNull();
    // The live view is still there.
    expect(view.target.querySelector('section.live-view')).not.toBeNull();
  });

  it('removes the indicator once the share is stopped', async () => {
    const view = await render();
    await startShare();
    expect(view.target.querySelector('.topbar .share-indicator')).not.toBeNull();
    await share.stop();
    await settle();
    expect(view.target.querySelector('.topbar .share-indicator')).toBeNull();
  });
});
