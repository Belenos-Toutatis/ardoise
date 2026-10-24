// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../lib/mock-data';
import { mockVideo, resetMockVideo } from '../lib/mock-video';
import { pickExportFile } from '../lib/dialogs';
import type { ExportSource } from '../lib/note';
import type { NarrationInfo, VideoEstimate, VideoExportOptions, VideoProgress } from '../lib/video';
import VideoExportDialog from './VideoExportDialog.svelte';

// The save dialog is the browser stand-in's fake path, except in the test that cancels it.
vi.mock('../lib/dialogs', async (importOriginal) => {
  const original = await importOriginal<typeof import('../lib/dialogs')>();
  return { ...original, pickExportFile: vi.fn(original.pickExportFile) };
});

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockVideo();
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
  vi.restoreAllMocks();
});

const TITLE = 'Liste de courses';

function pathOf(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

/** A narration recorded during a replay at 2×, with pauses cut after half a second. */
const NARRATION: NarrationInfo = { durationMs: 12_000, base: 'replay', speed: 2, maxPauseMs: 500, startMs: 0 };

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** Lets the debounced estimate (250 ms) run and arrive. */
async function estimated() {
  await sleep(300);
  await settle();
}

async function render(locale: 'fr' | 'en' = 'fr', title = TITLE) {
  setLocale(locale);
  const source: ExportSource = { path: pathOf(title) };
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const mountIt = () => mount(VideoExportDialog, { target, props: { source, subject: 'Sujet', onclose } });
  let component = mountIt();
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const dialog = () => target.querySelector<HTMLElement>('[role="dialog"]')!;
  const button = (text: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  const hasButton = (text: string) => [...target.querySelectorAll('button')].some((b) => b.textContent?.trim() === text);
  /** A radio of a format card (named by the format) or of a group (named by its label). */
  const radio = (label: string) => {
    const found = [...target.querySelectorAll<HTMLLabelElement>('label.choice')].find(
      (l) => (l.querySelector('.choice-name') ?? l).textContent?.trim() === label,
    );
    const input = found?.querySelector('input');
    if (!input) throw new Error(`no choice "${label}"`);
    return input;
  };
  const hasChoice = (label: string) =>
    [...target.querySelectorAll<HTMLLabelElement>('label.choice')].some(
      (l) => (l.querySelector('.choice-name') ?? l).textContent?.trim() === label,
    );
  const choose = async (label: string) => {
    radio(label).click();
    await settle();
  };
  /** The select or input of the `.export-row` whose caption is `label`. */
  const field = (label: string) => {
    const row = [...target.querySelectorAll<HTMLLabelElement>('label.export-row')].find(
      (l) => l.querySelector('span')?.textContent?.trim() === label,
    );
    const control = row?.querySelector<HTMLSelectElement | HTMLInputElement>('select, input');
    if (!control) throw new Error(`no field "${label}"`);
    return control;
  };
  const set = async (el: HTMLSelectElement | HTMLInputElement, value: string) => {
    el.value = value;
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
    await settle();
  };
  const checkbox = (label: string) => {
    const found = [...target.querySelectorAll<HTMLLabelElement>('label.check')].find((l) => l.textContent?.trim() === label);
    const input = found?.querySelector('input');
    if (!input) throw new Error(`no checkbox "${label}"`);
    return input;
  };
  const hasCheckbox = (label: string) =>
    [...target.querySelectorAll<HTMLLabelElement>('label.check')].some((l) => l.textContent?.trim() === label);
  const remount = async () => {
    unmount(component);
    component = mountIt();
    await settle();
  };
  return { target, source, onclose, dialog, button, hasButton, radio, hasChoice, choose, field, set, checkbox, hasCheckbox, remount };
}

const DEFAULTS: VideoExportOptions = {
  format: 'mp4',
  resolution: '1080p',
  frameRate: 30,
  timing: { kind: 'speed', speed: 1 },
  trimPauses: false,
  background: true,
  holdEndSeconds: 3,
  narration: false,
};

const ESTIMATE: VideoEstimate = {
  width: 1920,
  height: 1080,
  frames: 1950,
  durationMs: 65_000,
  exportMs: 14_300,
  speed: 1,
  narrationMs: null,
};

describe('video export dialog: choosing a format', () => {
  it('gives the strengths and limits of every format, and compares them', async () => {
    for (const locale of ['fr', 'en'] as const) {
      const ui = await render(locale);
      const cards = [...ui.target.querySelectorAll<HTMLLabelElement>('label.format-card')];
      expect(cards).toHaveLength(4);
      const names = cards.map((card) => {
        const radio = card.querySelector('input')!;
        // Named by the format alone, described by its strengths and limits.
        const name = document.getElementById(radio.getAttribute('aria-labelledby')!)!.textContent!.trim();
        const described = radio
          .getAttribute('aria-describedby')!
          .split(' ')
          .map((id) => document.getElementById(id)!.textContent!.trim());
        expect(described).toHaveLength(2);
        expect(described[0]).toMatch(locale === 'fr' ? /^Atouts :/ : /^Strengths:/);
        expect(described[1]).toMatch(locale === 'fr' ? /^Limites :/ : /^Limits:/);
        // Readable without hovering: the sentences are in the card itself.
        expect(described[0].length).toBeGreaterThan(30);
        return name;
      });
      expect(names).toEqual(
        locale === 'fr'
          ? ['MP4 (vidéo)', 'GIF animé', 'WebP animé', 'WebM (vidéo)']
          : ['MP4 (video)', 'Animated GIF', 'Animated WebP', 'WebM (video)'],
      );
      // MP4 is the default; choosing another card moves the mark.
      expect(ui.target.querySelector('.format-card.selected .choice-name')!.textContent).toBe(names[0]);
      await ui.choose(names[1]);
      expect(ui.target.querySelector('.format-card.selected .choice-name')!.textContent).toBe(names[1]);

      const compare = ui.target.querySelector('details.format-compare')!;
      expect(compare.querySelector('summary')!.textContent).toBe(
        locale === 'fr' ? 'Quel format choisir ?' : 'Which format should I choose?',
      );
      expect([...compare.querySelectorAll('thead th')].map((th) => th.textContent!.trim())).toEqual([
        locale === 'fr' ? 'Critère' : 'Criterion',
        'MP4',
        'GIF',
        'WEBP',
        'WEBM',
      ]);
      const rows = [...compare.querySelectorAll('tbody tr')];
      expect(rows.map((r) => r.querySelector('th')!.textContent)).toEqual(
        locale === 'fr'
          ? [
              'Se lit avec',
              'Son (narration)',
              'Fond transparent possible',
              'Taille du fichier',
              "Durée de l'export",
              'Idéal pour',
            ]
          : ['Plays with', 'Sound (narration)', 'Transparent background possible', 'File size', 'Export time', 'Best for'],
      );
      for (const row of rows) {
        expect(row.querySelectorAll('td')).toHaveLength(4);
        for (const cell of row.querySelectorAll('td')) expect(cell.textContent!.trim()).not.toBe('');
      }
      cleanup?.();
      cleanup = null;
    }
  });

  it('says what each format is good for and what it cannot do', async () => {
    const ui = await render('fr');
    const text = ui.target.textContent!;
    expect(text).toContain('PowerPoint');
    expect(text).toContain('256 couleurs');
    expect(text).toContain('Moodle');
    expect(text).toContain('encodage AV1');
    const rows = [...ui.target.querySelectorAll('details.format-compare tbody tr')];
    // Sound: MP4 and WebM only. Transparency: GIF and WebP only.
    expect([...rows[1].querySelectorAll('td')].map((td) => td.textContent)).toEqual(['Oui', 'Non', 'Non', 'Oui']);
    expect([...rows[2].querySelectorAll('td')].map((td) => td.textContent)).toEqual(['Non', 'Oui', 'Oui', 'Non']);
  });

  it('offers transparency only for GIF and WebP', async () => {
    const ui = await render('fr');
    // MP4: no choice, a line saying the paper is always there.
    expect(ui.hasChoice('Transparent')).toBe(false);
    expect(ui.target.querySelector('.video-background-fixed')?.textContent).toContain('ne gère pas la transparence');

    await ui.choose('GIF animé');
    expect(ui.radio('Avec le papier').checked).toBe(true);
    expect(ui.radio('Transparent').checked).toBe(false);
    expect(ui.target.querySelector('.video-background-fixed')).toBeNull();
    await ui.choose('WebP animé');
    expect(ui.hasChoice('Transparent')).toBe(true);
    await ui.choose('WebM (vidéo)');
    expect(ui.hasChoice('Transparent')).toBe(false);
  });

  it('gives back the paper when the format cannot be transparent', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    await ui.choose('GIF animé');
    await ui.choose('Transparent');
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][1]).toMatchObject({ format: 'gif', background: false });

    await ui.choose('MP4 (vidéo)');
    ui.button('Exporter de nouveau…').click();
    await settle();
    expect(spy.mock.calls[1][1]).toMatchObject({ format: 'mp4', background: true });
  });

  it('proposes 15 images per second for GIF and WebP and 30 for MP4 and WebM, unless the user chose', async () => {
    const ui = await render('fr');
    const fps = () => ui.field('Images par seconde') as HTMLSelectElement;
    expect([...fps().options].map((o) => o.value)).toEqual(['10', '15', '24', '30', '60']);
    expect(fps().value).toBe('30');
    await ui.choose('GIF animé');
    expect(fps().value).toBe('15');
    await ui.choose('WebM (vidéo)');
    expect(fps().value).toBe('30');
    await ui.set(fps(), '24');
    await ui.choose('WebP animé');
    expect(fps().value).toBe('24');
  });

  it('offers the four resolutions and the speeds of the replay', async () => {
    const ui = await render('fr');
    const resolution = ui.field('Taille de l\'image') as HTMLSelectElement;
    expect([...resolution.options].map((o) => [o.value, o.textContent])).toEqual([
      ['720p', '720p (1280 × 720)'],
      ['1080p', '1080p (1920 × 1080)'],
      ['4k', '4K (3840 × 2160)'],
      ['page', 'Taille de la page'],
    ]);
    expect(resolution.value).toBe('1080p');
    const speed = ui.target.querySelector<HTMLSelectElement>('select[aria-label="Vitesse"]')!;
    expect([...speed.options].map((o) => o.textContent)).toEqual(['0,25×', '0,5×', '1×', '2×', '4×', '8×', '16×']);
    expect(speed.value).toBe('1');
  });
});

describe('video export dialog: timing and the end of the video', () => {
  it('uses the speed or the target duration, one at a time', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    const speed = ui.target.querySelector<HTMLSelectElement>('select[aria-label="Vitesse"]')!;
    const seconds = ui.target.querySelector<HTMLInputElement>('input[type="number"][aria-label^="Durée cible"]')!;
    expect(speed.disabled).toBe(false);
    expect(seconds.disabled).toBe(true);
    await ui.set(speed, '4');
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][1].timing).toEqual({ kind: 'speed', speed: 4 });

    await ui.choose('Durée cible');
    expect(speed.disabled).toBe(true);
    expect(seconds.disabled).toBe(false);
    expect(seconds.value).toBe('30');
    await ui.set(seconds, '45');
    ui.button('Exporter de nouveau…').click();
    await settle();
    expect(spy.mock.calls[1][1].timing).toEqual({ kind: 'duration', seconds: 45 });
  });

  it('holds the final frame for 3 seconds by default, 0 to 60', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    const hold = ui.field('Image finale figée') as HTMLInputElement;
    expect(hold.value).toBe('3');
    await ui.set(hold, '0');
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][1].holdEndSeconds).toBe(0);
  });

  it('refuses an invalid target duration or final frame, and explains', async () => {
    const ui = await render('fr');
    await ui.choose('Durée cible');
    const seconds = ui.target.querySelector<HTMLInputElement>('input[type="number"][aria-label^="Durée cible"]')!;
    await ui.set(seconds, '');
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toContain('entre 1 et 3600 secondes');
    expect(ui.button('Exporter…').disabled).toBe(true);
    await ui.set(seconds, '20');
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.button('Exporter…').disabled).toBe(false);

    const hold = ui.field('Image finale figée') as HTMLInputElement;
    await ui.set(hold, '61');
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toContain('de 0 à 60 secondes');
    expect(ui.button('Exporter…').disabled).toBe(true);
    await ui.set(hold, '60');
    expect(ui.button('Exporter…').disabled).toBe(false);
  });

  it('passes the dead time option', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    expect(ui.checkbox('Supprimer les temps morts').checked).toBe(false);
    ui.checkbox('Supprimer les temps morts').click();
    await settle();
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][1].trimPauses).toBe(true);
  });
});

describe('video export dialog: narration', () => {
  it('is not offered when the note has none', async () => {
    const spy = vi.spyOn(api, 'narrationInfo').mockResolvedValue(null);
    const ui = await render('fr');
    expect(spy).toHaveBeenCalledWith(pathOf(TITLE));
    expect(ui.hasCheckbox('Inclure la narration')).toBe(false);
    await ui.choose('WebM (vidéo)');
    expect(ui.hasCheckbox('Inclure la narration')).toBe(false);
  });

  it('is offered for MP4 and WebM only, with a word for GIF and WebP', async () => {
    vi.spyOn(api, 'narrationInfo').mockResolvedValue(NARRATION);
    const ui = await render('fr');
    expect(ui.checkbox('Inclure la narration').checked).toBe(true);
    await ui.choose('WebM (vidéo)');
    expect(ui.hasCheckbox('Inclure la narration')).toBe(true);
    await ui.choose('GIF animé');
    expect(ui.hasCheckbox('Inclure la narration')).toBe(false);
    expect(ui.target.textContent).toContain("le GIF et le WebP n'ont pas de son");
    await ui.choose('WebP animé');
    expect(ui.hasCheckbox('Inclure la narration')).toBe(false);
  });

  it('imposes the recorded speed: speed, target duration and dead time are disabled', async () => {
    vi.spyOn(api, 'narrationInfo').mockResolvedValue(NARRATION);
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    const speed = ui.target.querySelector<HTMLSelectElement>('select[aria-label="Vitesse"]')!;
    const seconds = ui.target.querySelector<HTMLInputElement>('input[type="number"][aria-label^="Durée cible"]')!;
    expect(speed.disabled).toBe(true);
    expect(ui.radio('Vitesse').disabled).toBe(true);
    expect(ui.radio('Durée cible').disabled).toBe(true);
    expect(seconds.disabled).toBe(true);
    // The dead time shown is the one of the recording (cut after 0.5 s): ticked, not editable.
    expect(ui.checkbox('Supprimer les temps morts').disabled).toBe(true);
    expect(ui.checkbox('Supprimer les temps morts').checked).toBe(true);
    expect(ui.target.textContent).toContain('La narration garde la vitesse avec laquelle elle a été enregistrée (2×)');

    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][1]).toMatchObject({ narration: true });

    // Without the narration, the user is in charge again.
    ui.checkbox('Inclure la narration').click();
    await settle();
    expect(speed.disabled).toBe(false);
    expect(ui.radio('Vitesse').disabled).toBe(false);
    expect(ui.checkbox('Supprimer les temps morts').disabled).toBe(false);
    expect(ui.checkbox('Supprimer les temps morts').checked).toBe(false);
    expect(ui.target.textContent).not.toContain('La narration garde la vitesse');
    ui.button('Exporter de nouveau…').click();
    await settle();
    expect(spy.mock.calls[1][1]).toMatchObject({ narration: false });
  });

  it('shows the sound of the result', async () => {
    vi.spyOn(api, 'narrationInfo').mockResolvedValue(NARRATION);
    mockVideo.narrations[pathOf(TITLE)] = NARRATION;
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Son : oui (narration)');
  });

  it('remembers the wish of having no narration', async () => {
    vi.spyOn(api, 'narrationInfo').mockResolvedValue(NARRATION);
    const ui = await render('fr');
    ui.checkbox('Inclure la narration').click();
    await settle();
    ui.button('Exporter…').click();
    await settle();
    await ui.remount();
    expect(ui.checkbox('Inclure la narration').checked).toBe(false);
  });
});

describe('video export dialog: estimate', () => {
  it('shows the frame size, the length and the export time', async () => {
    vi.spyOn(api, 'estimateVideoExport').mockResolvedValue(ESTIMATE);
    const ui = await render('fr');
    expect(ui.target.querySelector('.video-estimate')?.textContent).toContain("Calcul de l'estimation");
    await estimated();
    const text = ui.target.querySelector('.video-estimate')!.textContent!;
    expect(text).toContain("Taille de l'image : 1920 × 1080 pixels");
    expect(text).toContain('Durée de la vidéo : 1 min 5 s (1950 images)');
    // Rounded to 5 s, no warning for a short export.
    expect(text).toContain('Export estimé : environ 15 s');
    expect(ui.target.querySelector('.video-estimate .banner')).toBeNull();
  });

  it('warns when the export will take more than 2 minutes', async () => {
    vi.spyOn(api, 'estimateVideoExport').mockResolvedValue({ ...ESTIMATE, exportMs: 195_000 });
    const ui = await render('en');
    await estimated();
    const estimate = ui.target.querySelector('.video-estimate')!;
    expect(estimate.textContent).toContain('Estimated export time: about 3 min 15 s');
    expect(estimate.querySelector('.banner')?.textContent).toContain('This export will take a while');
  });

  it('computes it from the note with the browser stand-in, and refreshes on every change', async () => {
    const spy = vi.spyOn(api, 'estimateVideoExport');
    const ui = await render('fr');
    await estimated();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0][0]).toEqual({ path: pathOf(TITLE) });
    expect(spy.mock.calls[0][1]).toEqual(DEFAULTS);
    const first = ui.target.querySelector('.video-estimate')!.textContent!;
    expect(first).toContain("Taille de l'image : 1920 × 1080 pixels");

    await ui.set(ui.field("Taille de l'image"), 'page');
    await estimated();
    expect(spy).toHaveBeenCalledTimes(2);
    expect(spy.mock.calls[1][1].resolution).toBe('page');
    // A4 landscape page: 1920 on the long side, even short side.
    expect(ui.target.querySelector('.video-estimate')!.textContent).toContain("Taille de l'image : 1920 × 1396 pixels");

    await ui.choose('GIF animé');
    await estimated();
    expect(spy).toHaveBeenCalledTimes(3);
    expect(spy.mock.calls[2][1]).toMatchObject({ format: 'gif', frameRate: 15 });

    // More images per second: more frames.
    const frames = (text: string) => Number(/\((\d+) images\)/.exec(text)![1]);
    const before = frames(ui.target.querySelector('.video-estimate')!.textContent!);
    await ui.set(ui.field('Images par seconde'), '30');
    await estimated();
    expect(frames(ui.target.querySelector('.video-estimate')!.textContent!)).toBeGreaterThan(before);
  });

  it('waits for the changes to stop before asking again', async () => {
    const spy = vi.spyOn(api, 'estimateVideoExport');
    const ui = await render('fr');
    await sleep(150);
    expect(spy).not.toHaveBeenCalled();
    // Another change restarts the wait: the first timer would have fired by now.
    await ui.set(ui.field('Image finale figée'), '5');
    await sleep(150);
    expect(spy).not.toHaveBeenCalled();
    await sleep(200);
    await settle();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0][1].holdEndSeconds).toBe(5);
  });

  it('keeps the latest answer when an older one arrives late', async () => {
    let answerFirst: (estimate: VideoEstimate) => void = () => {};
    vi.spyOn(api, 'estimateVideoExport')
      .mockImplementationOnce(() => new Promise((resolve) => (answerFirst = resolve)))
      .mockResolvedValue({ ...ESTIMATE, width: 1280, height: 720 });
    const ui = await render('fr');
    await estimated();
    await ui.set(ui.field("Taille de l'image"), '720p');
    await estimated();
    answerFirst(ESTIMATE);
    await settle();
    expect(ui.target.querySelector('.video-estimate')!.textContent).toContain('1280 × 720');
  });

  it('says so when the estimate fails', async () => {
    vi.spyOn(api, 'estimateVideoExport').mockRejectedValue({ code: 'noteNotFound', message: 'x.ardoise' });
    const ui = await render('fr');
    await estimated();
    expect(ui.target.querySelector('.video-estimate')!.textContent).toContain("Impossible d'estimer l'export");
    expect(ui.target.querySelector('.video-estimate')!.textContent).toContain("la note n'existe plus");
  });
});

describe('video export dialog: exporting', () => {
  it('exports to the file chosen, with the dialog\'s options, and shows where it went', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    mockVideo.narrations = {};
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    expect(spy).toHaveBeenCalledWith(
      { path: pathOf(TITLE) },
      DEFAULTS,
      'C:\\Users\\demo\\Exports\\2026-10-22 Liste de courses.mp4',
      expect.any(Function),
    );
    const result = ui.target.querySelector('.export-result')!;
    expect(result.textContent).toContain('Export terminé.');
    expect(result.querySelector('code')?.textContent).toBe('C:\\Users\\demo\\Exports\\2026-10-22 Liste de courses.mp4');
    expect(result.textContent).toMatch(/Durée : \d+ s|Durée : \d+ min/);
    expect(result.textContent).toContain('Son : non');
    expect(ui.button('Fermer')).toBeTruthy();
    expect(ui.button('Exporter de nouveau…')).toBeTruthy();
  });

  it('names the file after the format', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    await ui.choose('WebM (vidéo)');
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][2]).toMatch(/Liste de courses\.webm$/);
  });

  it('exports the note it was given, unsaved changes included', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const path = pathOf(TITLE);
    const { note } = await api.openNote(path);
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(VideoExportDialog, {
      target,
      props: { source: { path, note: { ...note, title: 'Brouillon' } }, subject: 'S', onclose: vi.fn() },
    });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await settle();
    [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Exporter…')!.click();
    await settle();
    expect(spy.mock.calls[0][0].note?.title).toBe('Brouillon');
  });

  it('shows the progress, with the frames done and a bar, while the settings are locked', async () => {
    let report: (progress: VideoProgress) => void = () => {};
    let finish: (result: Awaited<ReturnType<typeof api.exportVideo>>) => void = () => {};
    vi.spyOn(api, 'exportVideo').mockImplementationOnce((_s, _o, _d, onProgress) => {
      report = onProgress;
      return new Promise((resolve) => (finish = resolve));
    });
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('.video-progress')?.textContent).toContain('Export en cours…');
    expect(ui.button('Export en cours…').disabled).toBe(true);
    expect(ui.target.querySelector('fieldset')?.disabled).toBe(true);
    expect(ui.hasButton('Fermer')).toBe(false);

    report({ framesDone: 30, framesTotal: 120, elapsedMs: 1000 });
    await settle();
    const bar = ui.target.querySelector<HTMLProgressElement>('progress')!;
    expect([bar.value, bar.max]).toEqual([30, 120]);
    expect(ui.target.querySelector('.video-progress')?.textContent).toContain('Images : 30 sur 120 (25 %)');

    finish({ path: 'C:\\x\\a.mp4', frames: 120, durationMs: 4000, audio: false, encoder: 'Media Foundation' });
    await settle();
    // A late progress event does not bring the bar back.
    report({ framesDone: 120, framesTotal: 120, elapsedMs: 2000 });
    await settle();
    expect(ui.target.querySelector('progress')).toBeNull();
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('Durée : 4 s');
    expect(ui.button('Fermer').disabled).toBe(false);
  });

  it('shows the progress of the browser stand-in', async () => {
    mockVideo.delayMs = 1000;
    mockVideo.steps = 4;
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await sleep(600);
    await settle();
    const text = ui.target.querySelector('.video-progress')?.textContent ?? '';
    expect(text).toMatch(/Images : \d+ sur \d+ \(\d+ %\)/);
    await sleep(700);
    await settle();
    expect(ui.target.querySelector('.export-result')).toBeTruthy();
  });

  it('cancels the export with the button: a neutral notice, no red banner, and the dialog stays', async () => {
    let fail: (reason: unknown) => void = () => {};
    vi.spyOn(api, 'exportVideo').mockImplementationOnce(() => new Promise((_, reject) => (fail = reject)));
    const cancelSpy = vi.spyOn(api, 'cancelVideoExport').mockResolvedValue(undefined);
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    ui.button("Annuler l'export").click();
    await settle();
    expect(cancelSpy).toHaveBeenCalledTimes(1);
    // Waiting for the desktop app to stop: no second request, a visible state.
    expect(ui.target.querySelector('.video-progress .export-status')?.textContent).toBe('Annulation…');
    expect(ui.button('Annulation…').disabled).toBe(true);

    fail({ code: 'cancelled', message: 'cancelled' });
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.target.querySelector('.banner.error')).toBeNull();
    expect(ui.target.querySelector('.export-notice')?.textContent).toBe('Export annulé.');
    expect(ui.target.querySelector('.export-result')).toBeNull();
    expect(ui.onclose).not.toHaveBeenCalled();
    expect(ui.button('Exporter…').disabled).toBe(false);
    expect(ui.button('Annuler').disabled).toBe(false);
  });

  it('cancels a pending export of the browser stand-in', async () => {
    mockVideo.delayMs = 5000;
    const ui = await render('en');
    ui.button('Export…').click();
    await settle();
    expect(ui.target.querySelector('.video-progress .export-status')?.textContent).toBe('Exporting…');
    ui.button('Cancel the export').click();
    await settle();
    expect(ui.target.querySelector('.export-notice')?.textContent).toBe('Export cancelled.');
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.button('Export…').disabled).toBe(false);
  });

  it('cancels the export with Escape instead of closing the dialog', async () => {
    vi.spyOn(api, 'exportVideo').mockImplementationOnce(() => new Promise(() => {}));
    const cancelSpy = vi.spyOn(api, 'cancelVideoExport').mockResolvedValue(undefined);
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    ui.dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(cancelSpy).toHaveBeenCalledTimes(1);
    expect(ui.onclose).not.toHaveBeenCalled();
  });

  it('reports a failure to cancel as an error', async () => {
    vi.spyOn(api, 'exportVideo').mockImplementationOnce(() => new Promise(() => {}));
    vi.spyOn(api, 'cancelVideoExport').mockRejectedValueOnce({ code: 'videoExportFailed', message: 'x' });
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    ui.button("Annuler l'export").click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeTruthy();
    expect(ui.button("Annuler l'export").disabled).toBe(false);
  });

  it('sends the user to the settings when Windows cannot write MP4, and WebM then works', async () => {
    mockVideo.mediaFoundation = false;
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    const alert = ui.target.querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain("L'export a échoué : Windows ne peut pas écrire de MP4 sur ce PC");
    expect(alert.textContent).toContain('installez le secours OpenH264 dans les Réglages');
    expect(alert.textContent).toContain('choisissez WebM');
    expect(ui.target.querySelector('.export-result')).toBeNull();
    expect(ui.button('Exporter…').disabled).toBe(false);

    await ui.choose('WebM (vidéo)');
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.target.querySelector('.export-result')?.textContent).toContain('.webm');
  });

  it('speaks English for errors too', async () => {
    mockVideo.mediaFoundation = false;
    const ui = await render('en');
    ui.button('Export…').click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toContain(
      'install the OpenH264 backup in the Settings, or choose WebM',
    );
  });

  it('shows a translated error and lets the user try again', async () => {
    vi.spyOn(api, 'exportVideo').mockRejectedValueOnce({ code: 'videoExportFailed', message: 'disque plein' });
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')?.textContent).toContain(
      "L'export a échoué : la vidéo n'a pas pu être écrite (disque plein)",
    );
    ui.button('Exporter…').click();
    await settle();
    expect(ui.target.querySelector('[role="alert"]')).toBeNull();
    expect(ui.target.querySelector('.export-result')).toBeTruthy();
  });

  it('does nothing when the save dialog is cancelled', async () => {
    vi.mocked(pickExportFile).mockResolvedValueOnce(null);
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    ui.button('Exporter…').click();
    await settle();
    expect(spy).not.toHaveBeenCalled();
    expect(ui.target.querySelector('.export-notice')).toBeNull();
    expect(ui.button('Exporter…').disabled).toBe(false);
  });
});

describe('video export dialog: remembered choices', () => {
  it('starts from the remembered choices, and keeps them for the next time', async () => {
    const spy = vi.spyOn(api, 'exportVideo');
    const ui = await render('fr');
    await ui.choose('GIF animé');
    await ui.set(ui.field("Taille de l'image"), '720p');
    await ui.set(ui.field('Images par seconde'), '10');
    await ui.choose('Durée cible');
    await ui.set(ui.target.querySelector<HTMLInputElement>('input[type="number"][aria-label^="Durée cible"]')!, '20');
    ui.checkbox('Supprimer les temps morts').click();
    await ui.choose('Transparent');
    await ui.set(ui.field('Image finale figée'), '7');
    ui.button('Exporter…').click();
    await settle();
    expect(spy.mock.calls[0][1]).toEqual({
      format: 'gif',
      resolution: '720p',
      frameRate: 10,
      timing: { kind: 'duration', seconds: 20 },
      trimPauses: true,
      background: false,
      holdEndSeconds: 7,
      narration: false,
    });

    await ui.remount();
    expect(ui.radio('GIF animé').checked).toBe(true);
    expect((ui.field("Taille de l'image") as HTMLSelectElement).value).toBe('720p');
    expect((ui.field('Images par seconde') as HTMLSelectElement).value).toBe('10');
    expect(ui.radio('Durée cible').checked).toBe(true);
    expect(ui.target.querySelector<HTMLInputElement>('input[type="number"][aria-label^="Durée cible"]')!.value).toBe('20');
    expect(ui.checkbox('Supprimer les temps morts').checked).toBe(true);
    expect(ui.radio('Transparent').checked).toBe(true);
    expect((ui.field('Image finale figée') as HTMLInputElement).value).toBe('7');
  });

  it('forgets an invalid memory', async () => {
    localStorage.setItem('ardoise.videoExport', JSON.stringify({ format: 'avi', resolution: '8k', frameRate: 7, holdEndSeconds: 99 }));
    const ui = await render('fr');
    expect(ui.radio('MP4 (vidéo)').checked).toBe(true);
    expect((ui.field("Taille de l'image") as HTMLSelectElement).value).toBe('1080p');
    expect((ui.field('Images par seconde') as HTMLSelectElement).value).toBe('30');
    expect((ui.field('Image finale figée') as HTMLInputElement).value).toBe('3');
  });
});

describe('video export dialog: the dialog itself', () => {
  it('speaks English', async () => {
    const ui = await render('en');
    expect(ui.dialog().querySelector('h2')?.textContent).toBe('Export as video');
    expect(ui.dialog().textContent).toContain('Sujet');
    expect(ui.target.textContent).toContain('Freeze the last frame');
    expect(ui.target.textContent).toContain('Remove dead time');
    expect(ui.button('Export…')).toBeTruthy();
    expect(ui.button('Cancel')).toBeTruthy();
  });

  it('closes with Escape and with the cancel button when idle', async () => {
    const ui = await render('fr');
    ui.dialog().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(ui.onclose).toHaveBeenCalledTimes(1);
    ui.button('Annuler').click();
    expect(ui.onclose).toHaveBeenCalledTimes(2);
  });

  it('moves the focus into the dialog, onto the export button', async () => {
    const outside = document.createElement('button');
    document.body.append(outside);
    outside.focus();
    const ui = await render('fr');
    expect(ui.dialog().contains(document.activeElement)).toBe(true);
    expect(document.activeElement?.textContent?.trim()).toBe('Exporter…');
    outside.remove();
  });
});
