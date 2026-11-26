// @vitest-environment jsdom
// The recording panel's replace question (milestone M17, O11; docs/design/paper-projection-narration.md §3.4): with a
// narration to replace, « Démarrer » asks first, and only « Remplacer » starts; without one it starts at once (the live
// view passes no `replaces`).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockNarration } from '../../lib/mock-narration';
import type { CaptureState, NarrationStatus } from '../../lib/video';
import { monitorSettled } from './MicrophonePicker.svelte';
import RecordPanel from './RecordPanel.svelte';
import { NarrationRecorder, type RecorderCommands } from './recorder.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockNarration();
  setLocale('fr');
});

afterEach(async () => {
  cleanup?.();
  cleanup = null;
  await monitorSettled();
});

function render(props: { replaces?: boolean; replacedMs?: number | null } = {}) {
  const target = document.createElement('div');
  document.body.append(target);
  const onstart = vi.fn();
  const component = mount(RecordPanel, {
    target,
    props: {
      recorder: new NarrationRecorder(),
      device: null,
      ondevice: vi.fn(),
      onstart,
      onstop: vi.fn(),
      oncancel: vi.fn(),
      open: true,
      openLabel: 'Enregistrer',
      ...props,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const q = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`[data-action="${name}"]`);
  return { target, onstart, q, action };
}

describe('the replace question', () => {
  it('starts at once when there is nothing to replace', async () => {
    const { action, q, onstart } = render();
    action('narration-start')!.click();
    await tick();
    expect(onstart).toHaveBeenCalledTimes(1);
    expect(q('[data-slot="replace-question"]')).toBeNull();
  });

  it('asks before replacing, with the narration’s length, and starts only on « Remplacer »', async () => {
    const { action, q, onstart } = render({ replaces: true, replacedMs: 83_000 });
    expect(q('[data-slot="replaces"]')?.textContent).toBe('Un nouvel enregistrement remplace la narration actuelle.');
    action('narration-start')!.click();
    await tick();
    expect(onstart).not.toHaveBeenCalled();
    const question = q('[data-slot="replace-question"]')!;
    expect(question.getAttribute('role')).toBe('alert');
    expect(question.textContent).toContain('Cette note a déjà une narration (1:23). La remplacer par ce nouvel enregistrement ?');
    // The start button gives way to the two answers; the focus is on the one that keeps the narration.
    expect(action('narration-start')).toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(action('narration-replace-no')));
    expect(action('narration-replace-yes')!.textContent?.trim()).toBe('Remplacer');
    expect(action('narration-replace-no')!.textContent?.trim()).toBe("Garder l'ancienne");

    action('narration-replace-yes')!.click();
    await tick();
    expect(onstart).toHaveBeenCalledTimes(1);
    expect(q('[data-slot="replace-question"]')).toBeNull();
  });

  it('« Garder l’ancienne » starts nothing and gives « Démarrer » back', async () => {
    const { action, q, onstart } = render({ replaces: true, replacedMs: 2000 });
    action('narration-start')!.click();
    await tick();
    action('narration-replace-no')!.click();
    await tick();
    expect(onstart).not.toHaveBeenCalled();
    expect(q('[data-slot="replace-question"]')).toBeNull();
    expect(action('narration-start')).toBeTruthy();
    await vi.waitFor(() => expect(document.activeElement).toBe(action('narration-start')));
    // Asked again the next time.
    action('narration-start')!.click();
    await tick();
    expect(q('[data-slot="replace-question"]')).toBeTruthy();
  });

  it('speaks English', async () => {
    setLocale('en');
    const { action, q } = render({ replaces: true, replacedMs: 5000 });
    action('narration-start')!.click();
    await tick();
    expect(q('[data-slot="replace-question"]')?.textContent).toContain(
      'This note already has a narration (0:05). Replace it with this new recording?',
    );
    expect(action('narration-replace-yes')!.textContent?.trim()).toBe('Replace');
    expect(action('narration-replace-no')!.textContent?.trim()).toBe('Keep the old one');
  });
});

describe('a recording that reaches its longest length', () => {
  it('is stopped and saved as « Arrêter et enregistrer » does, once, and the panel says why', async () => {
    let capture: CaptureState = 'receiving';
    const status = (): NarrationStatus => ({
      state: 'recording',
      capture,
      level: { peak: 0, rms: 0 },
      elapsedMs: 14_340_000,
      device: 'Fake microphone',
      failure: null,
      pending: false,
    });
    const commands = {
      narrationStart: vi.fn(async () => undefined),
      narrationStatus: vi.fn(async () => status()),
      narrationStop: vi.fn(async () => ({ stamp: null, durationMs: 14_340_000, pages: 1 })),
      narrationRetry: vi.fn(),
      narrationDiscard: vi.fn(),
    };
    const recorder = new NarrationRecorder(commands as unknown as RecorderCommands);
    const onstop = vi.fn(() => void recorder.stop(true, null));
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(RecordPanel, {
      target,
      props: {
        recorder,
        device: null,
        ondevice: vi.fn(),
        onstart: vi.fn(),
        onstop,
        oncancel: vi.fn(),
        open: true,
        openLabel: 'Enregistrer',
      },
    });
    cleanup = () => {
      recorder.dispose();
      unmount(component);
      target.remove();
    };
    const notice = () => target.querySelector('[data-slot="record-limit"]');

    expect(await recorder.start(null, { kind: 'live' })).toBe(true);
    await vi.waitFor(() => expect(recorder.status?.capture).toBe('receiving'));
    expect(onstop).not.toHaveBeenCalled();
    capture = 'full';
    await vi.waitFor(() => expect(recorder.phase).toBe('idle'));
    expect(onstop).toHaveBeenCalledTimes(1);
    expect(commands.narrationStop).toHaveBeenCalledTimes(1);
    expect(commands.narrationStop).toHaveBeenCalledWith(true, null, null, null);
    await tick();
    expect(notice()?.textContent).toBe("L'enregistrement s'est arrêté de lui-même : il a atteint sa durée maximale (4 heures).");
    setLocale('en');
    await tick();
    expect(notice()?.textContent).toBe('The recording stopped by itself: it reached its longest length (4 hours).');

    // The next recording starts afresh.
    capture = 'receiving';
    expect(await recorder.start(null, { kind: 'live' })).toBe(true);
    await tick();
    expect(notice()).toBeNull();
    await recorder.stop(false, null);
  });
});
