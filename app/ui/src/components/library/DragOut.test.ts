// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { resetDragOutSupport } from '../../lib/drag-out';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockDrag, resetMockDrag } from '../../lib/mock-drag';
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
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockDrag();
  resetDragOutSupport();
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
});

const settle = async () => {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
};

async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app, onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelectorAll('.note-card').length).toBe(3));
  await settle();
  const card = (title: string) => {
    const found = [...target.querySelectorAll('.note-card')].find(
      (el) => el.querySelector('.card-title')?.textContent?.trim() === title,
    );
    if (!found) throw new Error(`no card "${title}"`);
    return found;
  };
  const handle = (title: string) => card(title).querySelector<HTMLButtonElement>('.drag-handle');
  const open = (title: string) => card(title).querySelector<HTMLElement>('.card-open')!;
  const click = async (el: Element, init: MouseEventInit = {}) => {
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
    await settle();
  };
  return { target, card, handle, open, click };
}

/** A pointer event as the desktop WebView sends it (jsdom has no PointerEvent). */
function pointer(type: string, x: number, y: number, init: MouseEventInit = {}) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y, ...init });
  Object.defineProperty(event, 'pointerId', { value: 1 });
  Object.defineProperty(event, 'isPrimary', { value: true });
  return event;
}

const press = (el: Element) => el.dispatchEvent(pointer('pointerdown', 100, 100));
const moveBy = (px: number) => window.dispatchEvent(pointer('pointermove', 100 + px, 100));
const release = () => window.dispatchEvent(pointer('pointerup', 120, 100));

describe('drag a note out: the handle', () => {
  it('is on every card, with a tooltip that says where the note can be dropped', async () => {
    const ui = await render();
    for (const title of ['Réunion parents', 'Liste de courses', 'Cours de physique']) {
      const handle = ui.handle(title)!;
      expect(handle).not.toBeNull();
      expect(handle.title).toMatch(/Explorateur, Word/);
      expect(handle.getAttribute('aria-label')).toBe(handle.title);
      expect(handle.getAttribute('draggable')).not.toBe('true');
      // Not inside the card's own (HTML5 draggable) button.
      expect(handle.closest('.card-open')).toBeNull();
    }
  });

  it('speaks English in English', async () => {
    const ui = await render('en');
    expect(ui.handle('Réunion parents')!.title).toMatch(/Explorer, Word/);
  });

  it('is absent where the app cannot drag notes out', async () => {
    mockDrag.supported = false;
    const ui = await render();
    expect(ui.target.querySelector('.drag-handle')).toBeNull();
  });

  it('names the number of notes when the card is part of a selection', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    expect(ui.handle('Réunion parents')!.title).toMatch(/2 notes sélectionnées/);
    expect(ui.handle('Cours de physique')!.title).not.toMatch(/sélectionnées/);
  });

  it('is explained in the help line of the selection bar', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    expect(ui.target.querySelector('.selection-bar')?.textContent).toMatch(/poignée/);
  });
});

describe('drag a note out: the gesture', () => {
  it('renders the note when the pointer goes down and drags its PNG once it moves', async () => {
    const ui = await render();
    press(ui.handle('Liste de courses')!);
    await settle();
    expect(mockDrag.prepared).toHaveLength(1);
    expect(mockDrag.prepared[0]).toHaveLength(1);
    expect(mockDrag.prepared[0][0]).toMatch(/Liste de courses/);
    // Nothing is dragged before the pointer has really moved.
    moveBy(2);
    await settle();
    expect(mockDrag.started).toEqual([]);
    moveBy(20);
    await settle();
    expect(mockDrag.started).toHaveLength(1);
    expect(mockDrag.started[0]).toHaveLength(1);
    expect(mockDrag.started[0][0]).toMatch(/Liste de courses\.png$/);
    expect(ui.target.querySelector('.drag-status')).toBeNull();
    expect(ui.target.querySelector('.drag-handle.busy')).toBeNull();
  });

  it('drags nothing when the pointer is released without moving', async () => {
    const ui = await render();
    press(ui.handle('Liste de courses')!);
    release();
    moveBy(40);
    await settle();
    expect(mockDrag.started).toEqual([]);
  });

  it('drags the whole selection from the handle of a selected card', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    await ui.click(ui.open('Liste de courses'), { ctrlKey: true });
    press(ui.handle('Liste de courses')!);
    moveBy(30);
    await settle();
    expect(mockDrag.prepared[0]).toHaveLength(2);
    expect(mockDrag.started[0]).toHaveLength(2);
    expect(mockDrag.started[0].some((f) => /Réunion parents\.png$/.test(f))).toBe(true);
    expect(mockDrag.started[0].some((f) => /Liste de courses\.png$/.test(f))).toBe(true);
  });

  it('drags only its own note from the handle of a card outside the selection', async () => {
    const ui = await render();
    await ui.click(ui.open('Réunion parents'), { ctrlKey: true });
    press(ui.handle('Cours de physique')!);
    moveBy(30);
    await settle();
    expect(mockDrag.started[0]).toHaveLength(1);
    expect(mockDrag.started[0][0]).toMatch(/Cours de physique\.png$/);
  });

  it('shows the preparation while the files are being made, and starts when they are ready', async () => {
    mockDrag.prepareMs = 60;
    const ui = await render();
    press(ui.handle('Liste de courses')!);
    moveBy(30);
    await tick();
    expect(ui.target.querySelector('.drag-status')?.textContent).toMatch(/Préparation/);
    expect(mockDrag.started).toEqual([]);
    await vi.waitFor(() => expect(mockDrag.started).toHaveLength(1));
    await settle();
    expect(ui.target.querySelector('.drag-status')).toBeNull();
  });

  it('does not start a drag when the pointer was released before the files were ready', async () => {
    mockDrag.prepareMs = 40;
    const ui = await render();
    press(ui.handle('Liste de courses')!);
    moveBy(30);
    release();
    await new Promise((resolve) => setTimeout(resolve, 100));
    await settle();
    expect(mockDrag.started).toEqual([]);
    expect(ui.target.querySelector('.drag-status')).toBeNull();
  });

  it('ignores other buttons than the main one', async () => {
    const ui = await render();
    ui.handle('Liste de courses')!.dispatchEvent(pointer('pointerdown', 100, 100, { button: 2 }));
    moveBy(30);
    await settle();
    expect(mockDrag.prepared).toEqual([]);
  });

  it('shows a failure of the preparation in the library banner', async () => {
    vi.spyOn(api, 'prepareNoteDrag').mockRejectedValue({ code: 'exportFailed', message: 'disk full' });
    const ui = await render();
    press(ui.handle('Liste de courses')!);
    moveBy(30);
    await settle();
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toMatch(/disk full/);
    expect(mockDrag.started).toEqual([]);
  });

  it('shows a failure of the system drag in the library banner', async () => {
    vi.spyOn(api, 'startNoteDrag').mockRejectedValue({ code: 'dragUnavailable', message: 'no OLE' });
    const ui = await render();
    press(ui.handle('Liste de courses')!);
    moveBy(30);
    await settle();
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toMatch(/no OLE/);
  });
});

describe('drag a note out: the binder drag is unchanged', () => {
  it('still starts from the card with the notes in the drag data', async () => {
    const ui = await render();
    const open = ui.open('Liste de courses');
    expect(open.getAttribute('draggable')).toBe('true');
    const data = new Map<string, string>();
    const event = new Event('dragstart', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'dataTransfer', {
      value: { effectAllowed: 'none', setData: (type: string, value: string) => data.set(type, value) },
    });
    open.dispatchEvent(event);
    await settle();
    expect(data.get('application/x-ardoise-notes')).toContain('Liste de courses');
    // The external drag never ran.
    expect(mockDrag.prepared).toEqual([]);
  });
});
