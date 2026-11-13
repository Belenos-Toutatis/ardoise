// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M18: live mode with the Xiaomi blackboard as the connected tablet: no position switch (its portrait page is
// never turned), no hint about the Slate's button, and a waiting screen that asks for the right tablet.
import { mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveReconnect, resetMockLive, setMockLiveOrientation } from '../../lib/mock-live';
import { boardRunOrientation, type BoardPosition } from '../../lib/board-position';
import { resetMockNarration } from '../../lib/mock-narration';
import { mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import LiveView from './LiveView.svelte';

// The real stroke geometry, instantiated from the module's bytes (jsdom cannot fetch it).
vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  // Tests run from `app/` (the Vite root).
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

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
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
  tablet.reset();
  setLocale('fr');
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
  mockTablet.stepMs = 10;
  mockTablet.inRange = false;
  // Live mode is not started while the blackboard holds pages that may not be synced first (§9.5): these tests are not about that.
  mockTablet.boardPages = 0;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  resetMockTablet();
  vi.restoreAllMocks();
});

/** Knows the blackboard (and the Slate unless `boardOnly`), and connected to it when `connect`. */
async function knowBoard({ boardOnly = false, connect = true, position = 'buttonsBottom' as BoardPosition } = {}) {
  if (boardOnly) await api.forgetTablet();
  await api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'landscape', boardPosition: position, button: 'newPage' });
  tablet.start();
  await api.tabletFindBlackboards();
  await vi.waitFor(() => expect(tablet.status?.boardSearch?.found.length).toBe(1));
  // Not switched on: the waiting screen stays.
  if (!connect) mockTablet.boardInRange = false;
  await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
  if (connect) await vi.waitFor(() => expect(tablet.status?.active).toBe('blackboard'));
}

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveView, { target, props: { app, onclose: vi.fn(), onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  return { target, root };
}

describe('live mode with the Xiaomi blackboard', () => {
  it('has no position switch, and says the position comes from the settings', async () => {
    await knowBoard();
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
    const note = view.target.querySelector('[data-slot="xiaomi-note"]')!;
    expect(note.textContent).toContain('Tablette Xiaomi : position des réglages');
    expect(note.getAttribute('title')).toContain('position choisie dans les réglages de la tablette');
    expect(note.getAttribute('title')).toContain('sans arrêter le direct');
    expect(note.getAttribute('title')).toContain('« Page suivante » se fait depuis l\'ordinateur');
  });

  it('keeps the position switch hidden while a Xiaomi session reconnects (no tablet is connected then)', async () => {
    await knowBoard();
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(view.target.querySelector('[data-slot="xiaomi-note"]')).not.toBeNull();
    // The link is lost: the tablet service waits again (`active` is null), the session reconnects.
    mockTabletPatch({ connection: 'searching' });
    mockLiveReconnect(2000);
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('reconnecting'));
    expect(tablet.status?.active).toBeNull();
    expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
    expect(view.target.querySelector('[data-slot="xiaomi-note"]')).not.toBeNull();
  });

  /** What the snapshot of the session says about how the page is held (`LiveSnapshot.orientation`). */
  const snapshotOrientation = () => api.liveSubscribe(() => {}).then((snapshot) => snapshot.orientation);
  const pageSize = (view: ReturnType<typeof render>) =>
    view.target.querySelector<HTMLElement>('canvas.live-canvas')!.dataset.pageSize!.split('x').map(Number);

  it.each([
    ['buttonsBottom', [20500, 27200]],
    ['buttonsTop', [20500, 27200]],
    ['buttonsLeft', [27200, 20500]],
    ['buttonsRight', [27200, 20500]],
  ] as const)('shows the page of the blackboard held %s: %j', async (position, size) => {
    await knowBoard({ position });
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    // A landscape page is a page like another for the live window: its size comes with the snapshot.
    expect(pageSize(view)).toEqual(size);
    expect(view.target.querySelector('[data-slot="xiaomi-note"]')).not.toBeNull();
  });

  it.each(['buttonsBottom', 'buttonsTop', 'buttonsLeft', 'buttonsRight'] as const)(
    'never offers or shows the quarter turns of a run held %s as the Slate orientation',
    async (position) => {
      await knowBoard({ position });
      const view = render();
      await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
      // The snapshot counts the blackboard's quarter turns under the Slate's names (`portraitLeft` for `buttonsLeft`)…
      expect(await snapshotOrientation()).toBe(boardRunOrientation(position));
      // …but neither the switch, nor the Slate's names, nor the "hold the tablet so" notice appear.
      expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
      expect(view.target.querySelector('[data-slot="resume-hold"]')).toBeNull();
      expect(view.target.querySelector('[data-orientation]')).toBeNull();
      for (const name of ['Paysage', 'Portrait']) expect(view.target.textContent).not.toContain(name);
    },
  );

  it('keeps the position the run started with when the settings or the Slate orientation change', async () => {
    await knowBoard({ position: 'buttonsLeft' });
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(pageSize(view)).toEqual([27200, 20500]);
    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'portraitRight', boardPosition: 'buttonsBottom', button: 'newPage' });
    setMockLiveOrientation('landscapeFlipped');
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(pageSize(view)).toEqual([27200, 20500]);
    expect(await snapshotOrientation()).toBe('portraitLeft');
    expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
  });

  it('keeps the position switch of the Slate when the Slate is the connected tablet', async () => {
    mockTablet.inRange = true;
    tablet.start();
    await vi.waitFor(() => expect(tablet.status?.active).toBe('slate'));
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(view.target.querySelectorAll('[data-slot="orientation"] button')).toHaveLength(4);
    expect(view.target.querySelector('[data-slot="xiaomi-note"]')).toBeNull();
  });

  it('asks for the Xiaomi tablet while it waits, when it is the only one known', async () => {
    mockLiveConfig.waitMs = 400;
    await knowBoard({ boardOnly: true, connect: false });
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('waiting'));
    expect(view.target.textContent).toContain('Allumez la tablette Xiaomi pour la connecter');
    expect(view.target.textContent).toContain('Fermez Mi Home');
    expect(view.target.textContent).not.toContain('Appuyez brièvement sur le bouton de la tablette');
  });

  it('asks for either tablet while it waits, when both are known', async () => {
    mockLiveConfig.waitMs = 400;
    await knowBoard({ connect: false });
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('waiting'));
    expect(view.target.textContent).toContain('Allumez la tablette Xiaomi, ou appuyez brièvement sur le bouton de la Bamboo Slate');
  });
});
