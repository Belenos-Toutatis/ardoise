// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M18: live mode with the Xiaomi blackboard as the connected tablet: its own position switch with its four
// drawings (Q19: one click turns the page and its ink, the choice is kept in the settings; on a page that keeps its shape
// only the two positions of its way round), never the Slate's switch nor the hint about the Slate's button, and a
// waiting screen that asks for the right tablet.
import { mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveReconnect, mockLiveTick, resetMockLive, setMockLiveOrientation } from '../../lib/mock-live';
import { BOARD_POSITIONS, boardRunOrientation, type BoardPosition } from '../../lib/board-position';
import { turnStrokes } from '../../lib/orientation';
import TabletOptions from '../tablet/TabletOptions.svelte';
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

function render(resume: string | null = null) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveView, { target, props: { app, resume, onclose: vi.fn(), onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  return { target, root };
}

/** The size of the page the live window shows. */
const pageSize = (view: ReturnType<typeof render>) =>
  view.target.querySelector<HTMLElement>('canvas.live-canvas')!.dataset.pageSize!.split('x').map(Number);
/** The blackboard's switch: its four buttons, in the order of `BOARD_POSITIONS`. */
const boardButtons = (target: HTMLElement) => [...target.querySelectorAll<HTMLButtonElement>('[data-slot="board-position"] button')];
const pressed = (target: HTMLElement) =>
  boardButtons(target)
    .filter((button) => button.getAttribute('aria-pressed') === 'true')
    .map((button) => button.dataset.position);

describe('live mode with the Xiaomi blackboard', () => {
  it('has its own position switch with its four drawings, never the Slate one', async () => {
    await knowBoard();
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
    expect(view.target.querySelector('.orientation-icon')).toBeNull();
    const group = view.target.querySelector<HTMLElement>('[data-slot="board-position"]')!;
    expect(group.getAttribute('role')).toBe('group');
    expect(group.getAttribute('aria-label')).toBe('Position de la tablette Xiaomi');
    expect(group.getAttribute('title')).toContain('la page en cours tourne avec ce qui y est déjà écrit');
    expect(group.getAttribute('title')).toContain('reste dans les réglages de la tablette Xiaomi');
    expect(group.getAttribute('title')).toContain('sans arrêter le direct');
    expect(group.getAttribute('title')).toContain('« Page suivante » se fait depuis l\'ordinateur');
    const buttons = boardButtons(view.target);
    expect(buttons.map((button) => button.dataset.position)).toEqual([...BOARD_POSITIONS]);
    // Each one is the blackboard's own drawing in its position (the bin and the floppy disk), and says it in words.
    for (const button of buttons) {
      expect(button.querySelector('svg.board-drawing')?.getAttribute('data-position')).toBe(button.dataset.position);
      expect(button.disabled).toBe(false);
    }
    expect(buttons.map((button) => button.getAttribute('aria-label'))).toEqual([
      'En hauteur, boutons en bas',
      'En hauteur, boutons en haut',
      'En largeur, boutons à droite',
      'En largeur, boutons à gauche',
    ]);
    expect(pressed(view.target)).toEqual(['buttonsBottom']);
    expect(view.target.querySelector('[data-slot="board-fixed"]')).toBeNull();
  });

  it('keeps the blackboard switch, never the Slate one, while a Xiaomi session reconnects (no tablet is connected then)', async () => {
    await knowBoard();
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(boardButtons(view.target)).toHaveLength(4);
    // The link is lost: the tablet service waits again (`active` is null), the session reconnects.
    mockTabletPatch({ connection: 'searching' });
    mockLiveReconnect(2000);
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('reconnecting'));
    expect(tablet.status?.active).toBeNull();
    expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
    expect(boardButtons(view.target)).toHaveLength(4);
  });

  /** What the snapshot of the session says about how the page is held (`LiveSnapshot.orientation`). */
  const snapshotOrientation = () => api.liveSubscribe(() => {}).then((snapshot) => snapshot.orientation);

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
    expect(pressed(view.target)).toEqual([position]);
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

  it('follows a position set in the settings during the run (Q19), never the Slate orientation', async () => {
    await knowBoard({ position: 'buttonsLeft' });
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(pageSize(view)).toEqual([27200, 20500]);
    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'portraitRight', boardPosition: 'buttonsBottom', button: 'newPage' });
    await vi.waitFor(() => expect(pageSize(view)).toEqual([20500, 27200]));
    expect(pressed(view.target)).toEqual(['buttonsBottom']);
    setMockLiveOrientation('landscapeFlipped');
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(pageSize(view)).toEqual([20500, 27200]);
    expect(await snapshotOrientation()).toBe('landscape');
    expect(view.target.querySelector('[data-slot="orientation"]')).toBeNull();
  });

  it('keeps the position switch of the Slate when the Slate is the connected tablet, never the blackboard one', async () => {
    mockTablet.inRange = true;
    tablet.start();
    await vi.waitFor(() => expect(tablet.status?.active).toBe('slate'));
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    expect(view.target.querySelectorAll('[data-slot="orientation"] button')).toHaveLength(4);
    expect(view.target.querySelector('[data-slot="board-position"]')).toBeNull();
    expect(view.target.querySelector('.board-drawing')).toBeNull();
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

describe('the position switch of the Xiaomi blackboard (Q19)', () => {
  /** The strokes of the page the live window shows (its session's snapshot). */
  const shownStrokes = () => api.liveSubscribe(() => {}).then((snapshot) => snapshot.page.strokes);

  it('turns the page and what is written on it at once, and keeps the choice in the settings', async () => {
    await knowBoard();
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    // The settings tab of the tablet panel, open at the same time.
    const settings = document.createElement('div');
    document.body.append(settings);
    const options = mount(TabletOptions, { target: settings, props: { kind: 'blackboard' } });
    const checked = () => settings.querySelector<HTMLInputElement>('input[data-option="boardPosition"]:checked')?.value;
    try {
      expect(checked()).toBe('buttonsBottom');
      mockLiveTick(400);
      const before = await shownStrokes();
      expect(before.length).toBeGreaterThan(0);

      // A quarter turn: the page goes wide, its ink turned with it.
      boardButtons(view.target)[3].click();
      await vi.waitFor(() => expect(pageSize(view)).toEqual([27200, 20500]));
      expect(pressed(view.target)).toEqual(['buttonsLeft']);
      const turned = await shownStrokes();
      expect(turned.slice(0, before.length)).toEqual(turnStrokes(before, 20500, 27200, 1));
      expect(view.target.querySelector<HTMLElement>('canvas.live-canvas')!.dataset.strokes).toBe(String(turned.length));
      await vi.waitFor(() => expect(tablet.status?.options.boardPosition).toBe('buttonsLeft'));
      await vi.waitFor(() => expect(checked()).toBe('buttonsLeft'));

      // A half turn more: still wide, the ink turned again.
      boardButtons(view.target)[2].click();
      await vi.waitFor(() => expect(pressed(view.target)).toEqual(['buttonsRight']));
      expect(pageSize(view)).toEqual([27200, 20500]);
      expect((await shownStrokes()).slice(0, before.length)).toEqual(turnStrokes(before, 20500, 27200, 3));

      // Back upright: the ink as it was written.
      boardButtons(view.target)[0].click();
      await vi.waitFor(() => expect(pageSize(view)).toEqual([20500, 27200]));
      expect((await shownStrokes()).slice(0, before.length)).toEqual(before);
      await vi.waitFor(() => expect(checked()).toBe('buttonsBottom'));
      // A click on the position held does nothing.
      const set = vi.spyOn(api, 'tabletSetBoardPosition');
      boardButtons(view.target)[0].click();
      expect(set).not.toHaveBeenCalled();
    } finally {
      unmount(options);
      settings.remove();
    }
  });

  it('says why a switch the tablet refuses did not happen', async () => {
    await knowBoard();
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    vi.spyOn(api, 'tabletSetBoardPosition').mockRejectedValue({ code: 'boardPositionFixed', message: 'fixed' });
    boardButtons(view.target)[1].click();
    await vi.waitFor(() => expect(view.target.textContent).toContain('seules les deux positions du même sens que cette page'));
    expect(pressed(view.target)).toEqual(['buttonsBottom']);
  });

  /** The demo sheet « Cours de physique », made a page of the blackboard of this size. */
  function boardSheet(size: { width: number; height: number }): string {
    const [path, stored] = [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')!;
    stored.note.page = { ...size };
    return path;
  }

  it.each([
    ['wide', { width: 27200, height: 20500 }, 'buttonsRight', ['buttonsRight', 'buttonsLeft'], 'Page en largeur : boutons à droite ou à gauche'],
    ['upright', { width: 20500, height: 27200 }, 'buttonsTop', ['buttonsBottom', 'buttonsTop'], 'Page en hauteur : boutons en bas ou en haut'],
  ] as const)('on a resumed %s sheet, offers the two positions of its way round only, and says why', async (_, sheet, held, offered, why) => {
    await knowBoard({ position: held });
    const path = boardSheet(sheet);
    const view = render(path);
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    const buttons = boardButtons(view.target);
    expect(buttons.filter((button) => !button.disabled).map((button) => button.dataset.position)).toEqual(offered);
    for (const button of buttons.filter((b) => b.disabled)) {
      expect(button.title).toContain('garde sa forme (une feuille reprise, ou une page écrite sur une image)');
      expect(button.title).toContain('Les deux autres positions écriraient de travers');
    }
    const group = view.target.querySelector<HTMLElement>('[data-slot="board-position"]')!;
    const fixed = view.target.querySelector<HTMLElement>('[data-slot="board-fixed"]')!;
    expect(fixed.textContent?.trim()).toBe(why);
    expect(group.getAttribute('aria-describedby')).toBe(fixed.id);
    expect(pressed(view.target)).toEqual([held]);
    const strokes = JSON.stringify(await shownStrokes());

    // The other position of the same way round: taken and saved; the sheet keeps its shape and its ink.
    const other = offered.find((position) => position !== held)!;
    buttons.find((button) => button.dataset.position === other)!.click();
    await vi.waitFor(() => expect(pressed(view.target)).toEqual([other]));
    expect(pageSize(view)).toEqual([sheet.width, sheet.height]);
    expect(JSON.stringify(await shownStrokes())).toBe(strokes);
    await vi.waitFor(() => expect(tablet.status?.options.boardPosition).toBe(other));

    // A disabled button does nothing; asked anyway, the tablet refuses and nothing changes.
    const refused = buttons.find((button) => button.disabled)!;
    refused.click();
    const asked = await api.tabletSetBoardPosition(refused.dataset.position as BoardPosition).catch((error: unknown) => error);
    expect(asked).toMatchObject({ code: 'boardPositionFixed' });
    expect(tablet.status?.options.boardPosition).toBe(other);
    expect(pageSize(view)).toEqual([sheet.width, sheet.height]);
  });

  it('names the positions and the reason in English too', async () => {
    setLocale('en');
    await knowBoard({ position: 'buttonsLeft' });
    const view = render(boardSheet({ width: 27200, height: 20500 }));
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    const group = view.target.querySelector<HTMLElement>('[data-slot="board-position"]')!;
    expect(group.getAttribute('aria-label')).toBe('Xiaomi tablet position');
    expect(group.title).toContain('the page in progress turns with what is already written on it');
    expect(boardButtons(view.target).map((button) => button.getAttribute('aria-label'))).toEqual([
      'Upright, buttons at the bottom',
      'Upright, buttons at the top',
      'Wide, buttons on the right',
      'Wide, buttons on the left',
    ]);
    expect(view.target.querySelector('[data-slot="board-fixed"]')?.textContent?.trim()).toBe('Wide page: buttons on the right or on the left');
    expect(boardButtons(view.target)[0].title).toContain('keeps its shape (a resumed sheet, or a page written over an image)');
  });
});
