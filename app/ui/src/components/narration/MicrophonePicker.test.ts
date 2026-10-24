// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockNarration, resetMockNarration } from '../../lib/mock-narration';
import MicrophonePicker, { monitorSettled } from './MicrophonePicker.svelte';
import { levelPercent } from './format';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockNarration();
  setLocale('fr');
  mockNarration.receivingAfterMs = 0;
  mockNarration.silentAfterMs = 0;
});

afterEach(async () => {
  cleanup?.();
  cleanup = null;
  await monitorSettled();
  vi.restoreAllMocks();
});

function render(props: { device?: string | null; monitor?: boolean; disabled?: boolean } = {}) {
  const target = document.createElement('div');
  document.body.append(target);
  const onchange = vi.fn();
  const component = mount(MicrophonePicker, { target, props: { device: null, onchange, ...props } });
  const view = {
    target,
    onchange,
    select: () => target.querySelector<HTMLSelectElement>('select[data-field="microphone"]')!,
    meter: () => target.querySelector<HTMLElement>('[role="meter"]'),
    line: () => target.querySelector<HTMLElement>('p[data-capture]'),
    unmount: () => {
      unmount(component);
      target.remove();
    },
  };
  cleanup = view.unmount;
  return view;
}

describe('microphone picker', () => {
  it('lists the microphones after the system default, and reports a choice', async () => {
    const view = render({ device: 'mock-mic-2' });
    await vi.waitFor(() => expect(view.select().options.length).toBe(3));
    const options = [...view.select().options].map((o) => [o.value, o.textContent?.trim()]);
    expect(options).toEqual([
      ['', 'Micro par défaut de Windows'],
      ['mock-mic-1', 'Microphone Array (Realtek Audio)'],
      ['mock-mic-2', 'Headset Microphone (USB Audio)'],
    ]);
    expect(view.select().value).toBe('mock-mic-2');
    view.select().value = 'mock-mic-1';
    view.select().dispatchEvent(new Event('change', { bubbles: true }));
    expect(view.onchange).toHaveBeenLastCalledWith('mock-mic-1');
    view.select().value = '';
    view.select().dispatchEvent(new Event('change', { bubbles: true }));
    expect(view.onchange).toHaveBeenLastCalledWith(null);
    // No capture was asked for: no meter, no status.
    expect(view.meter()).toBeNull();
    expect(view.line()).toBeNull();
  });

  it('keeps a chosen microphone that is no longer connected visible', async () => {
    const view = render({ device: 'unplugged' });
    await vi.waitFor(() => expect(view.select().options.length).toBe(4));
    expect(view.select().value).toBe('unplugged');
    expect(view.select().selectedOptions[0].textContent).toContain('Micro absent');
  });

  it('says when no microphone is detected', async () => {
    mockNarration.microphones = [];
    const view = render({ monitor: true });
    await vi.waitFor(() => expect(view.target.textContent).toContain('Aucun micro détecté'));
    expect(view.select().disabled).toBe(true);
    expect(view.meter()).toBeNull();
  });

  it('listens to the level while mounted and stops when it goes', async () => {
    const monitor = vi.spyOn(api, 'microphoneMonitor');
    const stop = vi.spyOn(api, 'microphoneMonitorStop');
    const discard = vi.spyOn(api, 'narrationStop');
    const view = render({ device: 'mock-mic-1', monitor: true });
    await vi.waitFor(() => expect(monitor).toHaveBeenCalledWith('mock-mic-1'));
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('receiving'));
    expect(view.line()?.textContent).toBe('Le micro fonctionne.');
    const meter = view.meter()!;
    expect(meter.getAttribute('aria-label')).toBe('Niveau du micro');
    expect(meter.getAttribute('aria-valuemin')).toBe('0');
    expect(meter.getAttribute('aria-valuemax')).toBe('100');
    await vi.waitFor(() => expect(Number(view.meter()!.getAttribute('aria-valuenow'))).toBeGreaterThan(0));
    expect(view.meter()!.getAttribute('aria-valuetext')).toMatch(/^Niveau \d+ %, crête \d+ %$/);
    expect(view.meter()!.querySelectorAll('.bar')).toHaveLength(2);
    expect(stop).not.toHaveBeenCalled();

    view.unmount();
    cleanup = null;
    await monitorSettled();
    expect(stop).toHaveBeenCalledTimes(1);
    // Only the level capture is stopped, with its own command: never `narrationStop`.
    expect(discard).not.toHaveBeenCalled();
    expect((await api.narrationStatus()).state).toBe('idle');
  });

  it('never discards a recording when it goes away', async () => {
    const view = render({ device: 'mock-mic-1', monitor: true });
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('receiving'));
    // A recording takes the microphone over from the level capture, as in the editor.
    await api.narrationStart('mock-mic-1', { kind: 'live' });
    view.unmount();
    cleanup = null;
    await monitorSettled();
    expect((await api.narrationStatus()).state).toBe('recording');
    await api.narrationStop(false, null, null, null);
  });

  it('restarts the level capture on another microphone', async () => {
    const monitor = vi.spyOn(api, 'microphoneMonitor');
    const view = render({ device: null, monitor: true });
    await vi.waitFor(() => expect(monitor).toHaveBeenCalledWith(null));
    view.unmount();
    cleanup = null;
    await monitorSettled();
    const again = render({ device: 'mock-mic-2', monitor: true });
    await vi.waitFor(() => expect(monitor).toHaveBeenLastCalledWith('mock-mic-2'));
    await vi.waitFor(() => expect(again.line()?.dataset.capture).toBe('receiving'));
    expect((await api.narrationStatus()).device).toBe('Headset Microphone (USB Audio)');
  });

  it('waits for the sound, then reports each way the microphone can fail', async () => {
    mockNarration.receivingAfterMs = 60_000;
    const view = render({ monitor: true });
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('waiting'));
    expect(view.line()?.textContent).toBe('En attente du son du micro…');

    mockNarration.receivingAfterMs = 0;
    mockNarration.noData = true;
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('noData'));
    expect(view.line()?.textContent).toContain('Le micro ne transmet rien');
    mockNarration.noData = false;

    mockNarration.failure = 'WASAPI: device invalidated';
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('failed'));
    expect(view.line()?.textContent).toBe('Le micro a échoué : WASAPI: device invalidated');
    mockNarration.failure = null;

    mockNarration.silent = true;
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('silent'));
    expect(view.line()?.textContent).toBe(
      'Windows bloque peut-être l\'accès au micro : ouvrez Paramètres > Confidentialité et sécurité > Microphone et activez « Autoriser les applications de bureau à accéder à votre microphone ». Un interrupteur de coupure du micro donne le même résultat.',
    );
    expect(view.meter()!.getAttribute('aria-valuenow')).toBe('0');
  });

  it('explains the Windows privacy setting in English too', async () => {
    setLocale('en');
    mockNarration.silent = true;
    const view = render({ monitor: true });
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('silent'));
    expect(view.line()?.textContent).toContain('Settings > Privacy & security > Microphone');
    expect(view.line()?.textContent).toContain('Let desktop apps access your microphone');
  });

  it('shows why a level capture could not start', async () => {
    const view = render({ device: 'nope', monitor: true });
    await vi.waitFor(() => expect(view.target.querySelector('[role="alert"]')).toBeTruthy());
    // The explanation replaces the status line.
    expect(view.line()?.getAttribute('role')).toBe('alert');
    expect(view.target.querySelectorAll('p[data-capture]')).toHaveLength(1);
    await tick();
  });

  it('can be frozen while a recording runs, and follows it', async () => {
    await api.narrationStart(null, { kind: 'live' });
    const view = render({ disabled: true });
    await vi.waitFor(() => expect(view.line()?.dataset.capture).toBe('receiving'));
    expect(view.select().disabled).toBe(true);
    expect(view.meter()).toBeTruthy();
    await api.narrationStop(false, null, null, null);
    await vi.waitFor(() => expect(view.meter()).toBeNull());
  });
});

describe('level scale', () => {
  it('puts a speaking voice in the middle of the meter, silence at zero and full scale at 100', () => {
    expect(levelPercent(0)).toBe(0);
    expect(levelPercent(NaN)).toBe(0);
    expect(levelPercent(1)).toBe(100);
    expect(levelPercent(2)).toBe(100);
    expect(levelPercent(0.001)).toBe(0);
    expect(levelPercent(0.0316)).toBe(50);
    expect(levelPercent(0.1)).toBe(67);
  });
});
