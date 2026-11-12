// @vitest-environment jsdom
/// <reference types="node" />
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import BackgroundImageSection from './BackgroundImageSection.svelte';
import Editor from './Editor.svelte';

// The real stroke geometry, instantiated from the module's bytes (jsdom cannot fetch it).
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

type E2eWindow = { __ardoiseE2eOpenPath?: string };

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  delete (window as E2eWindow).__ardoiseE2eOpenPath;
  vi.restoreAllMocks();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function open(title: string) {
  const path = demoPath(title);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Editor, { target, props: { path, app, onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  const section = () => target.querySelector<HTMLElement>('[data-section="background-image"]');
  /** Visible name of a button, without its decorative (aria-hidden) icon. */
  const name = (b: HTMLButtonElement) => {
    const copy = b.cloneNode(true) as HTMLElement;
    copy.querySelectorAll('[aria-hidden="true"]').forEach((icon) => icon.remove());
    return copy.textContent?.trim();
  };
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find(
      (b) => name(b) === label || b.title === label || b.getAttribute('aria-label') === label,
    );
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  return { path, target, section, button };
}

/** Sets a form control's value as the user would. */
function choose(control: HTMLInputElement | HTMLSelectElement, value: string, event = 'change') {
  control.value = value;
  control.dispatchEvent(new Event(event, { bubbles: true }));
}

describe('background image in the editor', () => {
  it('puts a page of a PDF under the writing, saves it, and undoes it', async () => {
    const render = vi.spyOn(api, 'renderBackground');
    const save = vi.spyOn(api, 'saveNote');
    const { path, section, button } = await open('Cours de physique');
    expect(section()?.textContent).toContain('Image ou PDF de fond');

    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() => expect(section()?.querySelector('[data-slot="background-pdf"]')).toBeTruthy());
    expect(section()?.textContent).toContain('fiche.pdf : 3 pages');
    choose(section()!.querySelector('select')!, '2');
    button('Mettre en fond').click();
    // A portrait page on a landscape note: the user is asked, and keeps the note as it is.
    await vi.waitFor(() => expect(section()?.querySelector('[data-slot="background-orientation-offer"]')).toBeTruthy());
    expect(section()?.textContent).toContain('Cette image est en portrait et la note en paysage');
    button('Garder en paysage').click();
    await vi.waitFor(() =>
      expect(section()?.querySelector('[data-slot="background-image-name"]')?.textContent).toContain('fiche.pdf, page 2'),
    );
    // The page is drawn again with the image and the key of its pixels.
    await vi.waitFor(() => expect(render.mock.lastCall?.[3]?.image.pdfPage).toBe(2));
    const key = render.mock.lastCall?.[3]?.key;
    expect(key).toMatch(/^mock-/);

    // Opacity and placement: one step each.
    choose(section()!.querySelector<HTMLInputElement>('input[type="range"]')!, '50');
    await tick();
    expect(render.mock.lastCall?.[3]?.image.opacity).toBe(0.5);
    choose(section()!.querySelector('select')!, 'cover');
    await tick();
    expect(render.mock.lastCall?.[3]?.image.rect.width).toBe(29700);

    button('Enregistrer').click();
    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.lastCall?.[3]).toBe(key);
    const stored = mockLibrary().notes.get(path)?.note.backgroundImage;
    expect(stored).toMatchObject({ name: 'fiche.pdf', pdfPage: 2, opacity: 0.5 });

    // Removed, then brought back by undo with the same pixels.
    button("Retirer l'image").click();
    await tick();
    expect(section()?.querySelector('[data-slot="background-image-name"]')).toBeNull();
    expect(render.mock.lastCall?.[3]).toBeUndefined();
    button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(render.mock.lastCall?.[3]?.key).toBe(key);
  });

  it('turns the image with the page and exports the unsaved note with its pixels', async () => {
    const copy = vi.spyOn(api, 'copyNoteImage');
    const { section, button } = await open('Cours de physique');
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() =>
      expect(section()?.querySelector('[data-slot="background-image-name"]')?.textContent).toContain('tableau.jpg'),
    );
    button("Tourner la page d'un quart de tour vers la droite").click();
    await tick();
    button("Copier l'image").click();
    await vi.waitFor(() => expect(copy).toHaveBeenCalledTimes(1));
    const source = copy.mock.lastCall?.[0];
    expect(source?.imageKey).toMatch(/^mock-/);
    expect(source?.note?.backgroundImage?.quarterTurns).toBe(1);
    expect(source?.note?.page).toEqual({ width: 21600, height: 29700 });
  });

  it('offers to give the note the image orientation, in one undo step', async () => {
    const render = vi.spyOn(api, 'renderBackground');
    const { target, section, button } = await open('Cours de physique');
    const svg = () => target.querySelector<SVGSVGElement>('svg.canvas')!;
    expect(svg().getAttribute('data-page-width')).toBe('29700');
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() => expect(section()?.querySelector('[data-slot="background-pdf"]')).toBeTruthy());
    button('Mettre en fond').click();
    await vi.waitFor(() => expect(section()?.querySelector('[data-slot="background-orientation-offer"]')).toBeTruthy());
    // Nothing is placed until the answer.
    expect(section()?.querySelector('[data-slot="background-image-name"]')).toBeNull();
    button('Passer la note en portrait').click();
    await vi.waitFor(() => expect(section()?.querySelector('[data-slot="background-image-name"]')).toBeTruthy());
    expect(svg().getAttribute('data-page-width')).toBe('21600');
    expect(svg().getAttribute('data-page-height')).toBe('29700');
    // The image fills the portrait page: placed on the switched page.
    await vi.waitFor(() => expect(render.mock.lastCall?.[3]?.image.rect.height).toBeCloseTo(29700, 0));
    // One step: undo brings back the landscape page without the image.
    button('Annuler (Ctrl+Z)').click();
    await tick();
    expect(svg().getAttribute('data-page-width')).toBe('29700');
    expect(section()?.querySelector('[data-slot="background-image-name"]')).toBeNull();
  });

  it('does not ask when the image has the note orientation, and turns only the image', async () => {
    const render = vi.spyOn(api, 'renderBackground');
    const { target, section, button } = await open('Cours de physique');
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Photos\\tableau.jpg';
    button('Choisir un PDF ou une image…').click();
    await vi.waitFor(() =>
      expect(section()?.querySelector('[data-slot="background-image-name"]')?.textContent).toContain('tableau.jpg'),
    );
    expect(section()?.querySelector('[data-slot="background-orientation-offer"]')).toBeNull();
    const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
    button('Image à droite').click();
    await tick();
    // The image turned; the page did not.
    await vi.waitFor(() => expect(render.mock.lastCall?.[3]?.image.quarterTurns).toBe(1));
    expect(svg.getAttribute('data-page-width')).toBe('29700');
    button('Image à gauche').click();
    await tick();
    await vi.waitFor(() => expect(render.mock.lastCall?.[3]?.image.quarterTurns).toBe(0));
  });

  it('opens a note that has an image with its pixels, and says when a file cannot be used', async () => {
    const path = demoPath('Cours de physique');
    const entry = mockLibrary().notes.get(path)!;
    entry.note.backgroundImage = { name: 'ancienne.png', rect: { x: 0, y: 0, width: 29700, height: 21600 } };
    const render = vi.spyOn(api, 'renderBackground');
    const { section, button } = await open('Cours de physique');
    await vi.waitFor(() => expect(render.mock.lastCall?.[3]?.key).toMatch(/^mock-note-/));
    expect(section()?.textContent).toContain('ancienne.png');
    expect(section()?.querySelector<HTMLSelectElement>('select')?.value).toBe('stretch');

    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\notes.txt';
    button('Remplacer…').click();
    await vi.waitFor(() => expect(section()?.querySelector('[role="alert"]')).toBeTruthy());
    expect(section()?.textContent).toContain("ce fichier n'est ni un PDF ni une image PNG ou JPEG");
    expect(section()?.textContent).toContain('ancienne.png');
  });
});

// Milestone M17: live mode's Fond tab uses the same section, with no offer to turn the page and a sentence of its own.
describe('the background image section on its own (M17)', () => {
  const LANDSCAPE = { width: 29700, height: 21600 };

  function mountSection(props: Record<string, unknown> = {}) {
    const target = document.createElement('div');
    document.body.append(target);
    const onset = vi.fn();
    const component = mount(BackgroundImageSection, {
      target,
      props: {
        page: LANDSCAPE,
        image: undefined,
        pixels: undefined,
        onset,
        onturn: vi.fn(),
        onfit: vi.fn(),
        onopacity: vi.fn(),
        onremove: vi.fn(),
        ...props,
      },
    });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    return { target, onset };
  }

  /** The user picks the PDF (three portrait pages in the stand-in) and puts page `pdfPage` of it. */
  async function placePdf(target: HTMLElement, pdfPage = '1') {
    (window as E2eWindow).__ardoiseE2eOpenPath = 'C:\\Documents\\fiche.pdf';
    target.querySelector<HTMLButtonElement>('button[data-action="background-choose"]')!.click();
    await vi.waitFor(() => expect(target.querySelector('[data-slot="background-pdf"]')).toBeTruthy());
    choose(target.querySelector('select')!, pdfPage);
    [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Mettre en fond')!.click();
  }

  it('says its own sentence under the title when it is given one, the editor\'s otherwise', () => {
    const editor = mountSection();
    expect(editor.target.querySelector('.pane-help')?.textContent).toContain('Pour annoter une fiche');
    cleanup?.();
    const live = mountSection({ hint: 'Sous l\'écriture de la page en cours.' });
    expect(live.target.querySelector('.pane-help')?.textContent).toBe('Sous l\'écriture de la page en cours.');
  });

  it('offers to switch the orientation by default, and puts the image as it is without the offer', async () => {
    const asking = mountSection();
    await placePdf(asking.target);
    await vi.waitFor(() => expect(asking.target.querySelector('[data-slot="background-orientation-offer"]')).toBeTruthy());
    expect(asking.onset).not.toHaveBeenCalled();
    cleanup?.();

    const plain = mountSection({ offerOrientation: false });
    await placePdf(plain.target, '2');
    await vi.waitFor(() => expect(plain.onset).toHaveBeenCalledTimes(1));
    expect(plain.target.querySelector('[data-slot="background-orientation-offer"]')).toBeNull();
    const [image, pixels, orientation] = plain.onset.mock.calls[0];
    // Contained on the landscape page, whole and upright: the page is not turned.
    expect(image).toMatchObject({ name: 'fiche.pdf', pdfPage: 2, quarterTurns: 0, opacity: 1 });
    expect(image.rect.height).toBeCloseTo(LANDSCAPE.height, 0);
    expect(image.rect.width).toBeLessThan(LANDSCAPE.width);
    expect(pixels.key).toMatch(/^mock-/);
    expect(orientation).toBeUndefined();
  });

  it('stays busy until an asynchronous `onset` is done', async () => {
    let finish = () => {};
    const { target } = mountSection({
      offerOrientation: false,
      onset: vi.fn(() => new Promise<void>((resolve) => (finish = resolve))),
    });
    await placePdf(target);
    await vi.waitFor(() => expect(target.querySelector('[role="status"]')?.textContent).toContain('Import'));
    finish();
    await vi.waitFor(() => expect(target.querySelector('[role="status"]')).toBeNull());
  });
});
