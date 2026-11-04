// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M12: what `live_stop` says of the last write (a sheet that went on in a new note, a
// narration that could not join its pages) is told by the view, which stays until it is dismissed;
// and a recording that cannot be kept as the view goes is said in the library.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import type { LiveStopped, SheetFork } from '../../lib/live';
import { freshStamp, mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveTick, resetMockLive } from '../../lib/mock-live';
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
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
  const slot = (name: string) => target.querySelector<HTMLElement>(`[data-slot="${name}"]`);
  const slots = (name: string) => [...target.querySelectorAll<HTMLElement>(`[data-slot="${name}"]`)];
  const button = (label: string) => [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!;
  return { target, onclose, root, action, slot, slots, button };
}

async function live(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
}

const FORK: SheetFork = { pageIndex: 0, reason: 'changed', original: 'C:\\demo\\a.ardoise', path: 'C:\\demo\\a (2).ardoise' };

/** `live_stop` ends the session for real, and answers with `answer` added (what the last write found). */
function stopsWith(answer: Partial<LiveStopped>) {
  const real = api.liveStop;
  return vi.spyOn(api, 'liveStop').mockImplementationOnce(async () => ({ ...(await real()), ...answer }));
}

describe('what live_stop says of the last write', () => {
  it('keeps the view open on a fork the window was not told of, until it is dismissed', async () => {
    stopsWith({ forks: [FORK] });
    const view = render();
    await live(view);
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.slot('sheet-forked')).not.toBeNull());
    expect(view.onclose).not.toHaveBeenCalled();
    const notice = view.slot('sheet-forked')!;
    expect(notice.dataset.reason).toBe('changed');
    expect(notice.textContent).toContain("l'écriture continue dans une nouvelle note");
    expect(notice.textContent).not.toMatch(/ardoise|\\/i);
    // The session is over; the way out is plain.
    expect(view.root().dataset.state).toBe('idle');
    expect(view.action('stop-live')).toBeNull();
    expect(view.action('stopped-back')!.textContent?.trim()).toBe('Retour à la bibliothèque');

    view.action('dismiss-fork')!.click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledOnce());
    expect(view.onclose).toHaveBeenCalledWith(undefined);
  });

  it('closes at once when the fork was told while it lived', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const path = sheetPath();
    const view = render(path);
    await live(view);
    const stored = mockLibrary().notes.get(path)!;
    mockLibrary().notes.set(path, { ...stored, stamp: freshStamp(4000) });
    mockLiveTick(150);
    await vi.waitFor(() => expect(view.slot('sheet-forked')).not.toBeNull());
    // Rust's answer repeats that fork (the mock does): this window has it already.
    const stop = vi.spyOn(api, 'liveStop');
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledOnce());
    expect((await stop.mock.results[0].value).forks).toHaveLength(1);
  });

  it('keeps every fork it was not told of, and closes on the last dismissal', async () => {
    stopsWith({ forks: [FORK, { ...FORK, original: 'C:\\demo\\b.ardoise', path: 'C:\\demo\\b (2).ardoise', reason: 'deleted' }] });
    const view = render();
    await live(view);
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.slots('sheet-forked')).toHaveLength(2));
    view.action('dismiss-fork')!.click();
    await tick();
    expect(view.onclose).not.toHaveBeenCalled();
    expect(view.slots('sheet-forked')).toHaveLength(1);
    view.action('dismiss-fork')!.click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledOnce());
  });

  it.each([
    ['fr', 'narrationUnreadable', 'La narration de la page ne peut pas être lue : elle reste telle quelle, et le nouveau son est gardé à part, dans C:\\demo\\son.ardoise'],
    ['fr', 'narrationTooLong', 'La narration de la page et le nouveau son dépasseraient 4 heures'],
    ['fr', 'narrationNotKept', "Le nouveau son n'a pas pu rejoindre la narration de la page, ni être gardé à part (C:\\demo\\son.ardoise)."],
    ['en', 'narrationUnreadable', "The page's narration cannot be read: it stays as it is, and the new sound is kept apart, in C:\\demo\\son.ardoise"],
    ['en', 'narrationTooLong', "The page's narration and the new sound would run over 4 hours"],
    ['en', 'narrationNotKept', "The new sound could not join the page's narration, nor be kept apart (C:\\demo\\son.ardoise)."],
  ] as const)('says a narration that could not join its pages in %s (%s), and waits for it to be dismissed', async (locale, code, text) => {
    setLocale(locale);
    stopsWith({ narration: { code, message: 'C:\\demo\\son.ardoise' } });
    const view = render(null);
    await live(view);
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.slot('narration-not-added')).not.toBeNull());
    const notice = view.slot('narration-not-added')!;
    expect(notice.getAttribute('role')).toBe('alert');
    expect(notice.textContent).toContain(text);
    expect(notice.querySelector('strong')?.textContent).toBe(
      locale === 'fr' ? "La narration n'a pas pu être ajoutée aux pages" : 'The narration could not be added to the pages',
    );
    expect(view.onclose).not.toHaveBeenCalled();
    view.action('dismiss-narration-problem')!.click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledOnce());
  });

  it('closes by the button without dismissing each notice', async () => {
    stopsWith({ forks: [FORK], narration: { code: 'narrationTooLong', message: 'C:\\demo\\son.ardoise' } });
    const view = render();
    await live(view);
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.action('stopped-back')).not.toBeNull());
    expect(view.slot('sheet-forked')).not.toBeNull();
    expect(view.slot('narration-not-added')).not.toBeNull();
    view.action('stopped-back')!.click();
    expect(view.onclose).toHaveBeenCalledOnce();
  });

  it('does not ask about ending live mode on Esc once it is over', async () => {
    stopsWith({ forks: [FORK] });
    const view = render();
    await live(view);
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.action('stopped-back')).not.toBeNull());
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await tick();
    expect(view.action('leave-confirm')).toBeNull();
  });

  it('says what « Close anyway » finds too', async () => {
    vi.spyOn(api, 'liveStop').mockRejectedValueOnce({ code: 'noteConflict', message: 'busy' });
    const view = render();
    await live(view);
    view.action('stop-live')!.click();
    await vi.waitFor(() => expect(view.target.textContent).toContain("Le mode direct ne s'est pas arrêté correctement."));
    stopsWith({ forks: [FORK] });
    view.button('Fermer quand même').click();
    await vi.waitFor(() => expect(view.slot('sheet-forked')).not.toBeNull());
    expect(view.onclose).not.toHaveBeenCalled();
    // The failure is behind: only the notice is left.
    expect(view.target.textContent).not.toContain("Le mode direct ne s'est pas arrêté correctement.");
  });
});

describe('a recording that cannot be kept as the view goes', () => {
  /** A refused resume, with a recording started meanwhile. */
  async function recordingOnARefusedResume() {
    mockLiveConfig.resumeError = 'noteNotFound';
    const view = render();
    await vi.waitFor(() => expect(view.slot('resume-failed')).not.toBeNull());
    view.action('narration-open')!.click();
    await vi.waitFor(() => expect(view.action('narration-start')).toBeTruthy());
    view.action('narration-start')!.click();
    await vi.waitFor(() => expect(view.action('narration-stop')).toBeTruthy());
    return view;
  }

  it('is stopped before the view goes, and the library is told when it could not be kept', async () => {
    const view = await recordingOnARefusedResume();
    vi.spyOn(api, 'narrationStop').mockRejectedValueOnce({ code: 'microphoneFailed', message: 'device lost' });
    view.action('resume-back')!.click();
    await vi.waitFor(() => expect(view.onclose).toHaveBeenCalledOnce());
    const [message] = view.onclose.mock.calls[0];
    expect(message).toContain("La narration en cours d'enregistrement n'a pas pu être conservée");
    expect(message).toContain('device lost');
  });

  it('goes without a word when there was nothing to keep, or when it was kept', async () => {
    const empty = await recordingOnARefusedResume();
    vi.spyOn(api, 'narrationStop').mockRejectedValueOnce({ code: 'emptyRecording', message: 'nothing' });
    empty.action('resume-back')!.click();
    await vi.waitFor(() => expect(empty.onclose).toHaveBeenCalledOnce());
    expect(empty.onclose).toHaveBeenCalledWith(undefined);
    cleanup?.();
    resetMockLive();
    resetMockNarration();

    const kept = await recordingOnARefusedResume();
    kept.action('resume-back')!.click();
    await vi.waitFor(() => expect(kept.onclose).toHaveBeenCalledOnce());
    expect(kept.onclose).toHaveBeenCalledWith(undefined);
  });
});
