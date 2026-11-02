// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The live control window and the share on the network (M11): the live view says « live » while
 * it is open, its toolbar has « Partager sur le réseau », and its keys leave the share panel alone.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { mockShareSetViewers, mockShareShown, resetMockShare } from '../../lib/mock-share';
import { resetMockTablet } from '../../lib/mock-tablet';
import { share } from '../../lib/share-state.svelte';
import LiveView from './LiveView.svelte';

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
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
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
  resetMockShare();
  share.reset();
  setLocale('fr');
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  share.reset();
  resetMockShare();
  resetMockLive();
  vi.restoreAllMocks();
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(LiveView, { target, props: { app, onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
  return { target, onclose, action };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

describe('live view and the share on the network', () => {
  it('says « live » to a share that runs when it opens', async () => {
    share.follow();
    expect(await share.start()).toBe(true);
    mockShareShown.length = 0;
    render();
    await vi.waitFor(() => expect(mockShareShown).toEqual([{ kind: 'live' }]));
    expect(share.status.shown).toBe('live');
  });

  it('keeps « live » for a share started later', async () => {
    render();
    await settle();
    expect(mockShareShown).toEqual([]);
    share.follow();
    await share.start();
    await vi.waitFor(() => expect(mockShareShown).toEqual([{ kind: 'live' }]));
  });

  it('has the button « Partager sur le réseau » in its toolbar, which opens the panel and starts the share', async () => {
    const view = render();
    const button = view.action('share')!;
    expect(button.closest('[role="toolbar"]')).not.toBeNull();
    expect(button.textContent).toContain('Partager sur le réseau');
    expect(button.classList.contains('active')).toBe(false);
    expect(view.target.querySelector('.share-indicator')).toBeNull();

    button.click();
    await tick();
    expect(share.panelOpen).toBe(true);
    await vi.waitFor(() => expect(share.status.active).toBe(true));
    await tick();
    expect(button.classList.contains('active')).toBe(true);
    expect(view.target.querySelector('.share-indicator')?.textContent).toBe('0');

    mockShareSetViewers(2);
    await tick();
    expect(view.target.querySelector('.share-indicator')?.textContent).toBe('2');
    expect(button.getAttribute('aria-label')).toContain('2 écrans connectés');
  });

  it('is in English too', async () => {
    setLocale('en');
    const view = render();
    expect(view.action('share')!.textContent).toContain('Share on the network');
  });

  it('leaves Escape to the share panel instead of asking to end live mode', async () => {
    const view = render();
    await settle();
    share.panelOpen = true;
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await tick();
    expect(view.target.querySelector('[data-action="leave-confirm"]')).toBeNull();
    expect(document.querySelector('[data-action="leave-confirm"]')).toBeNull();
    share.panelOpen = false;
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(document.querySelector('[data-action="leave-confirm"]')).not.toBeNull());
  });
});
