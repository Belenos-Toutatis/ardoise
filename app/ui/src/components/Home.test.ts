// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockLibrary } from '../lib/mock-data';
import { resetMockTablet } from '../lib/mock-tablet';
import { tablet } from '../lib/tablet-state.svelte';
import Home from './Home.svelte';

// The editor and the live view are tested on their own: here they are stand-ins that report a new
// app state (the microphone chosen on their side) through the `onchange` they are given.
interface DoubleProps {
  onchange?: (next: AppState) => void;
  onclose?: (notice?: string, declined?: boolean) => void;
}

function double(name: string) {
  return function Double(anchor: Comment, props: DoubleProps) {
    const button = document.createElement('button');
    button.dataset.double = name;
    button.textContent = name;
    button.addEventListener('click', () => props.onchange?.({ ...app, microphone: 'mock-mic-2' }));
    anchor.before(button);
    const back = document.createElement('button');
    back.dataset.doubleClose = name;
    back.textContent = `close ${name}`;
    back.addEventListener('click', () => props.onclose?.());
    anchor.before(back);
    // The user gave up the question of the pages the Xiaomi tablet still holds (live view only).
    const decline = document.createElement('button');
    decline.dataset.doubleDecline = name;
    decline.textContent = `decline ${name}`;
    decline.addEventListener('click', () => props.onclose?.(undefined, true));
    anchor.before(decline);
  };
}

vi.mock('./editor/Editor.svelte', () => ({ default: double('editor') }));
vi.mock('./live/LiveView.svelte', () => ({ default: double('live') }));

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
  tablet.reset();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
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

async function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const onchange = vi.fn();
  const component = mount(Home, { target, props: { app, onchange } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  return { target, onchange };
}

describe('the main window gives the app state to the editor and the live view', () => {
  it('takes the microphone chosen in the editor', async () => {
    const { target, onchange } = await render();
    target.querySelector<HTMLButtonElement>('.note-card .card-open')!.click();
    await settle();
    target.querySelector<HTMLButtonElement>('button[data-double="editor"]')!.click();
    expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ microphone: 'mock-mic-2' }));
  });

  it('takes the microphone chosen in the live view', async () => {
    const { target, onchange } = await render();
    target.querySelector<HTMLElement>('.tablet-status')!.click();
    await settle();
    target.querySelector<HTMLButtonElement>('.tablet-panel button[data-action="live"]')!.click();
    await settle();
    target.querySelector<HTMLButtonElement>('button[data-double="live"]')!.click();
    expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ microphone: 'mock-mic-2' }));
  });
});

describe('giving up the « pages first » question of live mode', () => {
  const openPanelAndAskLive = async (target: HTMLElement) => {
    target.querySelector<HTMLElement>('.tablet-status')!.click();
    await settle();
    target.querySelector<HTMLButtonElement>('.tablet-panel button[data-action="live"]')!.click();
    await settle();
  };

  it('gives the tablet panel back, with the focus on its « Live mode » button', async () => {
    const { target } = await render();
    await openPanelAndAskLive(target);
    expect(target.querySelector('.tablet-panel')).toBeNull();
    target.querySelector<HTMLButtonElement>('button[data-double-decline="live"]')!.click();
    await settle();
    expect(target.querySelector('.shell')!.classList.contains('shell-hidden')).toBe(false);
    const button = target.querySelector<HTMLButtonElement>('.tablet-panel button[data-action="live"]');
    expect(button).not.toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it('leaves the panel closed after an ordinary end of live mode', async () => {
    const { target } = await render();
    await openPanelAndAskLive(target);
    target.querySelector<HTMLButtonElement>('button[data-double-close="live"]')!.click();
    await settle();
    expect(target.querySelector('.tablet-panel')).toBeNull();
  });
});

describe('the library stays mounted under the editor and live mode', () => {
  const shellOf = (target: HTMLElement) => target.querySelector<HTMLElement>('.shell')!;
  const cardOf = (target: HTMLElement) => target.querySelector<HTMLButtonElement>('.note-card .card-open')!;

  it('hides the library while a note is edited and shows the same one afterwards', async () => {
    const { target } = await render();
    const shell = shellOf(target);
    const card = cardOf(target);
    expect(shell.classList.contains('shell-hidden')).toBe(false);
    card.click();
    await settle();
    // The same elements, hidden: nothing was built again.
    expect(shellOf(target)).toBe(shell);
    expect(shell.classList.contains('shell-hidden')).toBe(true);
    expect(target.querySelector('button[data-double="editor"]')).not.toBeNull();
    target.querySelector<HTMLButtonElement>('button[data-double-close="editor"]')!.click();
    await settle();
    expect(shell.classList.contains('shell-hidden')).toBe(false);
    expect(cardOf(target)).toBe(card);
  });

  it('gives back the scroll position and the focus on the card that was opened', async () => {
    const { target } = await render();
    const scroller = target.querySelector<HTMLElement>('.library-scroll')!;
    scroller.scrollTop = 240;
    const card = cardOf(target);
    card.focus();
    card.click();
    await settle();
    // What the browser does to a hidden element.
    scroller.scrollTop = 0;
    (document.activeElement as HTMLElement | null)?.blur();
    target.querySelector<HTMLButtonElement>('button[data-double-close="editor"]')!.click();
    await settle();
    expect(scroller.scrollTop).toBe(240);
    expect(document.activeElement).toBe(card);
  });

  it('keeps the open panel (a tablet test, the settings being edited) while a note is opened', async () => {
    const { target } = await render();
    target.querySelector<HTMLButtonElement>('.topbar button[aria-expanded]')!.click();
    await settle();
    const settings = target.querySelector('aside.panel');
    expect(settings).not.toBeNull();
    cardOf(target).click();
    await settle();
    expect(settings!.isConnected).toBe(true);
    target.querySelector<HTMLButtonElement>('button[data-double-close="editor"]')!.click();
    await settle();
    expect(settings!.isConnected).toBe(true);
  });

  it('ignores Ctrl+N and Ctrl+F while a dialog of the library is open', async () => {
    (window as { __ardoiseE2eOpenPath?: string }).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    try {
      const { target } = await render();
      target.querySelector<HTMLButtonElement>('button[data-action="new-from-file"]')!.click();
      await settle();
      expect(document.querySelector('[role="dialog"]')).not.toBeNull();
      const search = target.querySelector<HTMLInputElement>('input.search')!;
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true, cancelable: true }));
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true, cancelable: true }));
      await settle();
      // Neither a note was made and opened (which would have unmounted the dialog), nor the search focused.
      expect(target.querySelector('button[data-double="editor"]')).toBeNull();
      expect(document.querySelector('[role="dialog"]')).not.toBeNull();
      expect(document.activeElement).not.toBe(search);
    } finally {
      delete (window as { __ardoiseE2eOpenPath?: string }).__ardoiseE2eOpenPath;
    }
  });
});
