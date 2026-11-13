// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M12: live mode resumed on an existing sheet (`live_resume`): the failed resume, the
// quick position buttons that wait, how to hold the tablet, and the sheet that went on elsewhere.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { freshStamp, mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveRefuse, mockLiveTick, resetMockLive, setMockLiveTablet } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
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
  // jsdom has no canvas: every canvas draws on a recording fake.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];

function render(resume: string | null = sheetPath()) {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(LiveView, { target, props: { app, resume, onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  const canvas = () => target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const slot = (name: string) => target.querySelector<HTMLElement>(`[data-slot="${name}"]`);
  const slots = (name: string) => [...target.querySelectorAll<HTMLElement>(`[data-slot="${name}"]`)];
  const positions = () => [...target.querySelectorAll<HTMLButtonElement>('[data-slot="orientation"] button')];
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.getAttribute('aria-label') === label || b.textContent?.trim() === label);
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  return { target, onclose, root, canvas, slot, slots, positions, button };
}

async function live(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
}

describe('live view resumed on a sheet', () => {
  it('calls live_resume with the note, and shows its ink as the first page', async () => {
    const resume = vi.spyOn(api, 'liveResume');
    const start = vi.spyOn(api, 'liveStart');
    const path = sheetPath();
    const view = render(path);
    await live(view);
    expect(resume).toHaveBeenCalledExactlyOnceWith(path);
    expect(start).not.toHaveBeenCalled();
    expect(view.canvas().dataset.strokes).toBe(String(mockLibrary().notes.get(path)!.note.strokes.length));
    expect(view.target.querySelector('.page-position')?.textContent).toBe('Page 1 / 1');
    expect(view.slot('sheet-forked')).toBeNull();
  });

  it('still starts a new session without a note', async () => {
    const resume = vi.spyOn(api, 'liveResume');
    const start = vi.spyOn(api, 'liveStart');
    const view = render(null);
    await live(view);
    expect(start).toHaveBeenCalledOnce();
    expect(resume).not.toHaveBeenCalled();
    expect(view.canvas().dataset.strokes).toBe('0');
    expect(view.positions().every((b) => !b.disabled)).toBe(true);
  });

  it.each([
    ['noteNotFound', 'fr', "la note n'existe plus"],
    ['noteDamaged', 'fr', 'le fichier de la note est endommagé'],
    ['noteNewerVersion', 'fr', 'cette note a été créée par une version plus récente'],
    ['notInLibrary', 'fr', "ce fichier n'est pas dans la bibliothèque"],
    ['noteDamaged', 'en', 'the note file is damaged'],
    ['noteNewerVersion', 'en', 'this note was made by a newer version of Ardoise'],
  ] as const)('shows the translated error of a refused resume (%s, %s) and goes back', async (code, locale, text) => {
    setLocale(locale);
    mockLiveConfig.resumeError = code;
    const stop = vi.spyOn(api, 'liveStop');
    const view = render();
    await vi.waitFor(() => expect(view.slot('resume-failed')).not.toBeNull());
    const banner = view.slot('resume-failed')!;
    expect(banner.getAttribute('role')).toBe('alert');
    expect(banner.textContent).toContain(text);
    expect(banner.textContent).toContain(locale === 'fr' ? "Le mode direct n'a pas pu reprendre cette feuille." : 'Live mode could not resume this sheet.');
    expect(banner.textContent).toContain(locale === 'fr' ? 'Retour à la bibliothèque' : 'Back to the library');
    // Not the message of a start that failed.
    expect(banner.textContent).not.toContain('could not start');
    expect(view.root().dataset.state).toBe('idle');

    view.target.querySelector<HTMLButtonElement>('button[data-action="resume-back"]')!.click();
    expect(view.onclose).toHaveBeenCalledOnce();
    // Nothing was started on this side: nothing is stopped.
    expect(stop).not.toHaveBeenCalled();
  });

  it('tries again after a refused resume', async () => {
    mockLiveConfig.resumeError = 'noteNotFound';
    const view = render();
    await vi.waitFor(() => expect(view.slot('resume-failed')).not.toBeNull());
    mockLiveConfig.resumeError = null;
    view.button('Réessayer').click();
    await live(view);
    expect(view.slot('resume-failed')).toBeNull();
  });

  it('keeps the start-failed banner of a new session as it was', async () => {
    vi.spyOn(api, 'liveStart').mockRejectedValue({ code: 'tabletBusy', message: 'busy' });
    const view = render(null);
    await vi.waitFor(() => expect(view.slot('start-failed')).not.toBeNull());
    expect(view.slot('start-failed')!.textContent).toContain("Le mode direct n'a pas pu démarrer.");
    expect(view.target.querySelector('button[data-action="resume-back"]')).toBeNull();
  });

  it('disables the position buttons on the resumed sheet, with the reason, and enables them on the next page', async () => {
    const view = render();
    await live(view);
    expect(view.positions()).toHaveLength(4);
    for (const b of view.positions()) {
      expect(b.disabled).toBe(true);
      expect(b.title).toContain('Cette page ne se tourne pas avec la tablette');
      // The name of the position stays for assistive technology.
      expect(b.getAttribute('aria-label')).toMatch(/^(Paysage|Portrait)/);
    }
    expect(view.slot('orientation')!.title).toContain('ne se tourne pas');

    // A click that would still arrive changes nothing.
    const turn = vi.spyOn(api, 'tabletSetOrientation');
    view.positions()[2].click();
    expect(turn).not.toHaveBeenCalled();

    view.button('Page suivante').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    await vi.waitFor(() => expect(view.positions().every((b) => !b.disabled)).toBe(true));
    expect(view.positions()[0].title).toBe('Paysage, bouton en bas');
    view.button('Page précédente').click();
    await vi.waitFor(() => expect(view.positions().every((b) => b.disabled)).toBe(true));
  });

  it.each([
    ['fr', "Cette page ne se tourne pas avec la tablette : c'est une feuille reprise, ou une page écrite sur une image. Une nouvelle page suivra de nouveau la position de la tablette."],
    ['en', "This page does not turn with the tablet: it is a resumed sheet, or a page written over an image. A new page follows the tablet's position again."],
  ] as const)('explains the disabled buttons in %s', async (locale, text) => {
    setLocale(locale);
    const view = render();
    await live(view);
    expect(view.positions()[0].title).toBe(text);
  });

  it('says how to hold the tablet when the sheet is held the other way round, and only then', async () => {
    const path = sheetPath();
    // A portrait sheet; the tablet is set landscape.
    mockLibrary().notes.get(path)!.note.page = { width: 21600, height: 29700 };
    const view = render(path);
    await live(view);
    await vi.waitFor(() => expect(view.slot('resume-hold')).not.toBeNull());
    const notice = view.slot('resume-hold')!;
    expect(notice.dataset.orientation).toBe('portraitLeft');
    expect(notice.textContent).toContain('Pour écrire sur cette feuille, tenez la tablette ainsi : Portrait, bouton à gauche.');
    expect(notice.getAttribute('role')).toBe('status');

    // On the next page the tablet is held as set again: nothing to say.
    view.button('Page suivante').click();
    await vi.waitFor(() => expect(view.slot('resume-hold')).toBeNull());
  });

  it('says it in English, and says nothing for a sheet held like the setting', async () => {
    setLocale('en');
    const path = sheetPath();
    mockLibrary().notes.get(path)!.note.page = { width: 21600, height: 29700 };
    const portrait = render(path);
    await live(portrait);
    await vi.waitFor(() => expect(portrait.slot('resume-hold')).not.toBeNull());
    expect(portrait.slot('resume-hold')!.textContent).toContain('To write on this sheet, hold the tablet like this: Portrait, button on the left.');
    cleanup?.();
    resetMockLive();

    mockLibrary().notes.get(path)!.note.page = { width: 29700, height: 21600 };
    mockLiveConfig.waitMs = 30;
    const landscape = render(path);
    await live(landscape);
    // Let the tablet's status come in: the comparison needs it.
    await vi.waitFor(() => expect(tablet.status).not.toBeNull());
    await tick();
    expect(landscape.slot('resume-hold')).toBeNull();
  });
});

describe('a sheet that went on in a new note', () => {
  /** Another PC saved the sheet while live mode had it open, and the pen writes. */
  async function forkedView(locale: 'fr' | 'en', change: 'changed' | 'deleted') {
    setLocale(locale);
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const path = sheetPath();
    const view = render(path);
    await live(view);
    const stored = mockLibrary().notes.get(path)!;
    if (change === 'changed') mockLibrary().notes.set(path, { ...stored, stamp: freshStamp(4000) });
    else mockLibrary().notes.delete(path);
    mockLiveTick(150);
    await vi.waitFor(() => expect(view.slot('sheet-forked')).not.toBeNull());
    return view;
  }

  it.each([
    ['fr', 'changed', 'Feuille « Cours de physique »', "Cette feuille a été modifiée ailleurs pendant le direct : pour ne rien écraser, l'écriture continue dans une nouvelle note, à côté d'elle."],
    ['fr', 'deleted', 'Feuille « Cours de physique »', "Cette feuille a été supprimée ailleurs pendant le direct : l'écriture continue dans une nouvelle note."],
    ['en', 'changed', 'Sheet “Cours de physique”', 'This sheet was changed elsewhere during live mode: so that nothing is overwritten, the writing continues in a new note, next to it.'],
    ['en', 'deleted', 'Sheet “Cours de physique”', 'This sheet was deleted elsewhere during live mode: the writing continues in a new note.'],
  ] as const)('tells it in %s (%s), naming only the title', async (locale, change, title, sentence) => {
    const view = await forkedView(locale, change);
    const notice = view.slot('sheet-forked')!;
    expect(notice.dataset.reason).toBe(change);
    expect(notice.getAttribute('role')).toBe('status');
    expect(notice.textContent).toContain(title);
    expect(notice.textContent).toContain(sentence);
    // No path, no file name in it.
    expect(notice.textContent).not.toMatch(/ardoise|\\/i);
  });

  it('stays until it is dismissed, and only one notice per fork', async () => {
    const view = await forkedView('fr', 'changed');
    // Neither the next page, the commands nor a long wait take it away.
    vi.advanceTimersByTime(10 * 60_000);
    view.button('Nouvelle page').click();
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    vi.advanceTimersByTime(60_000);
    expect(view.slots('sheet-forked')).toHaveLength(1);
    // The commands' own notices are not mixed with it.
    view.target.querySelector<HTMLButtonElement>('button[data-action="dismiss-fork"]')!.click();
    await tick();
    expect(view.slots('sheet-forked')).toHaveLength(0);
    // Dismissed, it does not come back.
    mockLiveTick(20);
    await tick();
    expect(view.slots('sheet-forked')).toHaveLength(0);
  });

  it('shows the notice without a title when the sheet has none', async () => {
    const path = sheetPath();
    mockLibrary().notes.get(path)!.note.title = '';
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const view = render(path);
    await live(view);
    const stored = mockLibrary().notes.get(path)!;
    mockLibrary().notes.set(path, { ...stored, stamp: freshStamp(4000) });
    mockLiveTick(150);
    await vi.waitFor(() => expect(view.slot('sheet-forked')).not.toBeNull());
    expect(view.slot('sheet-forked')!.querySelector('strong')).toBeNull();
    expect(view.slot('sheet-forked')!.textContent).toContain('Cette feuille a été modifiée ailleurs');
  });
});

describe('live view: a sheet the connected tablet cannot write on', () => {
  const SENTENCE = {
    fr: ['Cette feuille ne peut pas être continuée avec cette tablette', 'une page de la tablette Xiaomi avec la tablette Xiaomi', "Rien n'a été écrit sur la note"],
    en: ['This sheet cannot be continued with this tablet', 'a page of the Xiaomi tablet with the Xiaomi tablet', 'Nothing was written on the note'],
  } as const;

  it.each(['fr', 'en'] as const)('shows why at once when the Xiaomi tablet is connected and the sheet is the Slate\'s (%s)', async (locale) => {
    setLocale(locale);
    setMockLiveTablet('blackboard');
    const stop = vi.spyOn(api, 'liveStop');
    const view = render();
    await vi.waitFor(() => expect(view.slot('resume-failed')).not.toBeNull());
    const banner = view.slot('resume-failed')!;
    expect(banner.getAttribute('role')).toBe('alert');
    for (const part of SENTENCE[locale]) expect(banner.textContent).toContain(part);
    expect(view.root().dataset.state).toBe('idle');
    view.target.querySelector<HTMLButtonElement>('button[data-action="resume-back"]')!.click();
    expect(view.onclose).toHaveBeenCalledOnce();
    expect(stop).not.toHaveBeenCalled();
  });

  it('refuses the Xiaomi page with the Slate, and resumes the sheets each tablet can write on', async () => {
    const path = sheetPath();
    const stored = mockLibrary().notes.get(path)!;
    // A page of the Xiaomi tablet, resumed while the Slate is connected.
    stored.note.page = { width: 20500, height: 27200 };
    setMockLiveTablet('slate');
    const refused = render();
    await vi.waitFor(() => expect(refused.slot('resume-failed')).not.toBeNull());
    cleanup?.();
    cleanup = null;
    // The same page with the Xiaomi tablet: it goes on.
    setMockLiveTablet('blackboard');
    const view = render();
    await live(view);
    expect(view.slot('resume-failed')).toBeNull();
  });

  it('shows the refusal that comes later, when the tablet connects, and lets the user try again', async () => {
    mockLiveConfig.waitMs = 5000;
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('waiting'));
    expect(view.slot('resume-failed')).toBeNull();

    mockLiveRefuse();
    await vi.waitFor(() => expect(view.slot('resume-failed')).not.toBeNull());
    const banner = view.slot('resume-failed')!;
    expect(banner.getAttribute('role')).toBe('alert');
    expect(banner.textContent).toContain('Cette feuille ne peut pas être continuée avec cette tablette');
    expect(banner.textContent).toContain("Le mode direct n'a pas pu reprendre cette feuille.");
    expect(view.root().dataset.state).toBe('idle');

    // Starting again (the Slate is the tablet that connects now): the banner goes.
    mockLiveConfig.waitMs = 30;
    setMockLiveTablet('slate');
    view.button('Réessayer').click();
    await live(view);
    expect(view.slot('resume-failed')).toBeNull();
  });

  it('can be left after a refusal that came later', async () => {
    mockLiveConfig.waitMs = 5000;
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('waiting'));
    mockLiveRefuse();
    await vi.waitFor(() => expect(view.slot('resume-failed')).not.toBeNull());
    view.target.querySelector<HTMLButtonElement>('button[data-action="resume-back"]')!.click();
    expect(view.onclose).toHaveBeenCalledOnce();
  });
});
