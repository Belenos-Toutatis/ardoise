// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { mockNarration, mockNarrationOf, resetMockNarration, setMockNarration } from './mock-narration';
import type { NarrationInfo, NarrationTarget } from './video';

beforeEach(() => {
  resetMockLibrary();
  resetMockNarration();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

function pathOf(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

async function code(promise: Promise<unknown>): Promise<string | undefined> {
  try {
    await promise;
  } catch (error) {
    return (error as { code?: string }).code;
  }
  return undefined;
}

const replayTarget = (path: string): NarrationTarget => ({
  kind: 'replay',
  path,
  speed: 2,
  maxPauseMs: 500,
  startMs: 0,
});

describe('mock microphones', () => {
  it('lists two microphones, the first being the default', async () => {
    const list = await api.listMicrophones();
    expect(list).toHaveLength(2);
    expect(list.filter((m) => m.isDefault)).toHaveLength(1);
    expect(new Set(list.map((m) => m.id)).size).toBe(2);
  });

  it('remembers the chosen microphone in the app state', async () => {
    expect((await api.setMicrophone('mock-mic-2')).microphone).toBe('mock-mic-2');
    expect((await api.appState()).microphone).toBe('mock-mic-2');
    expect((await api.setMicrophone(null)).microphone).toBeNull();
  });

  it('monitors the level of a microphone, one capture at a time', async () => {
    await api.microphoneMonitor(null);
    expect((await api.narrationStatus()).state).toBe('monitoring');
    expect(await code(api.microphoneMonitor('mock-mic-2'))).toBe('narrationBusy');
    // A monitor gives way to the recording; nothing takes over a recording.
    await api.narrationStart(null, { kind: 'live' });
    expect((await api.narrationStatus()).state).toBe('recording');
    expect(await code(api.narrationStart(null, { kind: 'live' }))).toBe('narrationBusy');
    expect(await code(api.microphoneMonitor(null))).toBe('narrationBusy');
    expect(await api.narrationStop(false, null)).toBeNull();
    expect((await api.narrationStatus()).state).toBe('idle');
    await api.microphoneMonitor('mock-mic-2');
    expect(await api.narrationStop(true, null)).toBeNull();
    expect(await code(api.microphoneMonitor('nope'))).toBe('microphoneNotFound');
    mockNarration.microphones = [];
    expect(await code(api.microphoneMonitor(null))).toBe('noMicrophone');
  });

  it('has a level that moves, and a waiting phase before the first samples', async () => {
    await api.microphoneMonitor('mock-mic-2');
    let status = await api.narrationStatus();
    expect(status.capture).toBe('waiting');
    expect(status.level).toEqual({ peak: 0, rms: 0 });
    expect(status.device).toBe('Headset Microphone (USB Audio)');
    const seen = new Set<number>();
    for (let i = 0; i < 12; i++) {
      vi.advanceTimersByTime(100);
      status = await api.narrationStatus();
      if (i === 0) {
        expect(status.capture).toBe('waiting');
        continue;
      }
      expect(status.capture).toBe('receiving');
      expect(status.level.rms).toBeGreaterThan(0);
      expect(status.level.peak).toBeGreaterThanOrEqual(status.level.rms);
      expect(status.level.peak).toBeLessThanOrEqual(1);
      seen.add(Math.round(status.level.rms * 1000));
    }
    expect(seen.size).toBeGreaterThan(3);
    expect(status.elapsedMs).toBe(1200);
  });

  it('reports silence after a second of nothing when `silent` is on, and the other failures', async () => {
    mockNarration.silent = true;
    await api.microphoneMonitor(null);
    vi.advanceTimersByTime(500);
    expect((await api.narrationStatus()).capture).toBe('waiting');
    vi.advanceTimersByTime(600);
    const silent = await api.narrationStatus();
    expect(silent.capture).toBe('silent');
    expect(silent.level).toEqual({ peak: 0, rms: 0 });
    mockNarration.silent = false;
    mockNarration.noData = true;
    expect((await api.narrationStatus()).capture).toBe('noData');
    mockNarration.noData = false;
    mockNarration.failure = 'WASAPI: device invalidated';
    const failed = await api.narrationStatus();
    expect(failed.capture).toBe('failed');
    expect(failed.failure).toBe('WASAPI: device invalidated');
  });
});

describe('mock narration of a note', () => {
  it('has none at first, and no audio to play', async () => {
    const path = pathOf('Cours de physique');
    expect(await api.narrationInfo(path)).toBeNull();
    expect(await code(api.narrationAudio(path))).toBe('noRecording');
    expect(await code(api.narrationInfo('C:\\nowhere.ardoise'))).toBe('noteNotFound');
  });

  it('records into the note: the new stamp comes back, and the narration replaces the previous one', async () => {
    const path = pathOf('Cours de physique');
    const entry = mockLibrary().notes.get(path)!;
    const before = entry.stamp;
    await api.narrationStart(null, replayTarget(path));
    vi.advanceTimersByTime(3000);
    const saved = await api.narrationStop(true, before);
    expect(saved?.durationMs).toBe(3000);
    expect(saved?.stamp).toEqual(entry.stamp);
    expect(saved?.stamp).not.toEqual(before);
    const info = await api.narrationInfo(path);
    expect(info).toEqual({ durationMs: 3000, base: 'replay', speed: 2, maxPauseMs: 500, startMs: 0 } satisfies NarrationInfo);

    // Again: replaces it.
    await api.narrationStart('mock-mic-2', { kind: 'replay', path, speed: 1, maxPauseMs: null, startMs: 0 });
    vi.advanceTimersByTime(1000);
    await api.narrationStop(true, saved!.stamp);
    expect(await api.narrationInfo(path)).toMatchObject({ durationMs: 1000, speed: 1, maxPauseMs: null });
  });

  it('refuses to overwrite a note that changed (noteConflict) and discards on cancel', async () => {
    const path = pathOf('Cours de physique');
    const stale = mockLibrary().notes.get(path)!.stamp;
    await api.narrationStart(null, replayTarget(path));
    vi.advanceTimersByTime(500);
    await api.saveNote(path, (await api.openNote(path)).note, stale);
    expect(await code(api.narrationStop(true, stale))).toBe('noteConflict');
    expect(await api.narrationInfo(path)).toBeNull();
    // The capture ended even so: another can start.
    await api.narrationStart(null, replayTarget(path));
    expect(await api.narrationStop(false, null)).toBeNull();
    expect(await api.narrationInfo(path)).toBeNull();
    expect(await code(api.narrationStop(true, null))).toBe('noRecording');
  });

  it('plays a tiny valid WAV file', async () => {
    const path = pathOf('Cours de physique');
    setMockNarration(path, { durationMs: 500, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const wav = await api.narrationAudio(path);
    expect(wav).toBeInstanceOf(ArrayBuffer);
    const view = new DataView(wav);
    const tag = (offset: number) => String.fromCharCode(...new Uint8Array(wav, offset, 4));
    expect([tag(0), tag(8), tag(12), tag(36)]).toEqual(['RIFF', 'WAVE', 'fmt ', 'data']);
    expect(view.getUint32(4, true)).toBe(wav.byteLength - 8);
    expect(view.getUint16(22, true)).toBe(1);
    const rate = view.getUint32(24, true);
    expect(view.getUint32(40, true)).toBe(wav.byteLength - 44);
    expect((wav.byteLength - 44) / 2 / rate).toBeCloseTo(0.5, 3);
  });

  it('deletes the narration and gives the note a new stamp', async () => {
    const path = pathOf('Liste de courses');
    setMockNarration(path, { durationMs: 900, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 });
    const before = mockLibrary().notes.get(path)!.stamp;
    expect(await code(api.deleteNarration(path, { ...before, len: before.len + 5 }))).toBe('noteConflict');
    const next = await api.deleteNarration(path, before);
    expect(next).not.toEqual(before);
    expect(mockLibrary().notes.get(path)!.stamp).toEqual(next);
    expect(mockNarrationOf(path)).toBeNull();
  });

  it('records live narration without a stamp and reports the pages', async () => {
    mockNarration.livePages = 3;
    await api.narrationStart(null, { kind: 'live' });
    vi.advanceTimersByTime(2500);
    expect(await api.narrationStop(true, null)).toEqual({ stamp: null, durationMs: 2500, pages: 3 });
  });
});
