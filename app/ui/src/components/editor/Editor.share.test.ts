// @vitest-environment jsdom
/// <reference types="node" />
/**
 * The editor and the share on the network (M11): the note being edited, unsaved changes
 * included, is what the share shows; its Projection tab has « Partager sur le réseau ».
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockShareShown, resetMockShare } from '../../lib/mock-share';
import type { ShareContent } from '../../lib/share';
import { share } from '../../lib/share-state.svelte';
import Editor from './Editor.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

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
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockShare();
  share.reset();
  setLocale('fr');
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
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function render(title = 'Cours de physique') {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn(), onsaved: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  /** Visible name of a button, without its decorative (aria-hidden) icon. */
  const name = (b: HTMLButtonElement) => {
    const copy = b.cloneNode(true) as HTMLElement;
    copy.querySelectorAll('[aria-hidden="true"]').forEach((icon) => icon.remove());
    return copy.textContent?.trim();
  };
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find(
      (b) => b.getAttribute('aria-label') === label || b.title === label || name(b) === label,
    );
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  /** Recolours the whole note red: one unsaved change. */
  const edit = async () => {
    button('Rouge').click();
    await tick();
    button('Couleur').click();
    await tick();
  };
  return { path, target, button, edit };
}

const notes = (): Extract<ShareContent, { kind: 'note' }>[] =>
  mockShareShown.filter((c): c is Extract<ShareContent, { kind: 'note' }> => c.kind === 'note');

async function startShare() {
  share.follow();
  expect(await share.start()).toBe(true);
  mockShareShown.length = 0;
}

describe('editor and the share on the network', () => {
  it('shows the note it opens, with its title', async () => {
    await startShare();
    const view = await render();
    await vi.waitFor(() => expect(notes()).toHaveLength(1));
    const [shown] = notes();
    const stored = mockLibrary().notes.get(view.path)!.note;
    expect(shown.note.title).toBe(stored.title);
    expect(shown.note.strokes).toHaveLength(stored.strokes.length);
    expect(shown.imageKey).toBeNull();
    expect(share.status.shown).toBe('note');
  });

  it('follows every change of the note, unsaved ones included', async () => {
    await startShare();
    const view = await render();
    await vi.waitFor(() => expect(notes()).toHaveLength(1));
    expect(notes()[0].note.strokes.every((s) => s.color.r === 216)).toBe(false);

    await view.edit();
    // Debounced: sent about 100 ms after the change (and 300 ms after the previous one).
    await vi.waitFor(() => expect(notes()).toHaveLength(2), { timeout: 2000 });
    const latest = notes()[1].note;
    expect(latest.strokes.length).toBeGreaterThan(0);
    expect(latest.strokes.every((s) => s.color.r === 216)).toBe(true);
    // Nothing was saved to the library.
    expect(mockLibrary().notes.get(view.path)!.note.strokes.every((s) => s.color.r === 216)).toBe(false);
  });

  it('gives the open note to a share started after it', async () => {
    await render();
    expect(mockShareShown).toEqual([]);
    share.follow();
    await share.start();
    await vi.waitFor(() => expect(notes()).toHaveLength(1));
  });

  it('has the button « Partager sur le réseau » in its Projection tab, not in its toolbar, which opens the panel and starts the share', async () => {
    const view = await render();
    expect(view.target.querySelector('.toolbar [data-action="share"]')).toBeNull();
    const tab = view.target.querySelector<HTMLElement>('[role="tab"][data-tab="projection"]')!;
    expect(tab.hasAttribute('data-dot')).toBe(false);
    const button = view.target.querySelector<HTMLButtonElement>('[data-tab-panel="projection"] button[data-action="share"]')!;
    expect(button).not.toBeNull();
    expect(button.textContent).toContain('Partager sur le réseau');
    button.click();
    await tick();
    expect(share.panelOpen).toBe(true);
    await vi.waitFor(() => expect(share.status.active).toBe(true));
    await tick();
    expect(button.classList.contains('active')).toBe(true);
    expect(view.target.querySelector('.share-indicator')?.textContent).toBe('0');
    // The blue dot says a share runs, even with the pane collapsed.
    expect(tab.getAttribute('data-dot')).toBe('live');
  });

  it('leaves the editing keys alone while the share panel is open', async () => {
    const view = await render();
    const selected = () => view.target.textContent?.includes('Les changements s\'appliquent à la sélection') ?? false;
    const selectAll = async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true, cancelable: true }));
      await tick();
    };
    share.panelOpen = true;
    await selectAll();
    expect(selected()).toBe(false);
    share.panelOpen = false;
    await selectAll();
    expect(selected()).toBe(true);
  });
});
