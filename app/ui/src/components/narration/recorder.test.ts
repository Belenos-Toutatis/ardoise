// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import type { FileStamp } from '../../lib/note';
import type { NarrationSaved, NarrationStatus } from '../../lib/video';
import { NarrationRecorder, type RecorderCommands } from './recorder.svelte';

const STAMP: FileStamp = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 10 };
const SAVED: NarrationSaved = { stamp: STAMP, durationMs: 1500, pages: 0 };

function status(over: Partial<NarrationStatus> = {}): NarrationStatus {
  return {
    state: 'idle',
    capture: null,
    level: { peak: 0, rms: 0 },
    elapsedMs: 0,
    device: null,
    failure: null,
    pending: false,
    ...over,
  };
}

const conflict = async (): Promise<never> => {
  throw { code: 'noteConflict', message: 'x' };
};

type Commands = RecorderCommands;

/** Scripted commands; `over` replaces some of them. */
function scripted(over: Partial<Commands> = {}) {
  return {
    narrationStart: vi.fn(async () => {}),
    narrationStatus: vi.fn(async () => status({ state: 'recording' })),
    narrationStop: vi.fn(async (): Promise<NarrationSaved | null> => SAVED),
    narrationRetry: vi.fn(async () => SAVED),
    narrationDiscard: vi.fn(async () => {}),
    ...over,
  } satisfies Commands;
}

/** A recorder over the scripted commands, already recording. */
async function recording(over: Partial<Commands> = {}) {
  setLocale('fr');
  const commands = scripted(over);
  const recorder = new NarrationRecorder(commands);
  expect(await recorder.start(null, { kind: 'live' })).toBe(true);
  return { recorder, commands };
}

describe('narration recorder: a recording that cannot be saved', () => {
  it('passes the path and the replay start to the stop command', async () => {
    const { recorder, commands } = await recording();
    const result = await recorder.stop(true, STAMP, 'C:\\notes\\a.ardoise', 123456);
    expect(commands.narrationStop).toHaveBeenCalledWith(true, STAMP, 'C:\\notes\\a.ardoise', 123456);
    expect(result).toEqual({ ok: true, saved: SAVED });
    expect(recorder.phase).toBe('idle');
    expect(recorder.pending).toBe(false);
  });

  it('keeps it pending when Rust kept it, with the error code, and never discards it', async () => {
    const { recorder, commands } = await recording({
      narrationStop: conflict,
      narrationStatus: async () => status({ pending: true }),
    });
    const result = await recorder.stop(true, STAMP, 'a', 1);
    expect(result).toEqual({ ok: false, saved: null, code: 'noteConflict' });
    expect(recorder.pending).toBe(true);
    expect(recorder.error).toContain('la note a été modifiée entre-temps');
    expect(recorder.phase).toBe('idle');
    // Starting another waits for the choice.
    expect(await recorder.start(null, { kind: 'live' })).toBe(false);
    expect(commands.narrationStart).toHaveBeenCalledTimes(1);
  });

  it('still stops a capture the failed stop left running', async () => {
    const narrationStop = vi
      .fn<Commands['narrationStop']>()
      .mockRejectedValueOnce({ code: 'microphoneFailed', message: 'x' })
      .mockResolvedValue(null);
    const { recorder } = await recording({ narrationStop });
    const result = await recorder.stop(true, null);
    expect(result.ok).toBe(false);
    expect(recorder.pending).toBe(false);
    expect(narrationStop).toHaveBeenCalledTimes(2);
    expect(narrationStop).toHaveBeenLastCalledWith(false, null, null, null);
  });

  it('retries into the given note, and stays pending while that fails', async () => {
    const narrationRetry = vi
      .fn<Commands['narrationRetry']>()
      .mockRejectedValueOnce({ code: 'noteNotFound', message: 'gone' })
      .mockResolvedValueOnce(SAVED);
    const { recorder } = await recording({
      narrationStop: conflict,
      narrationStatus: async () => status({ pending: true }),
      narrationRetry,
    });
    await recorder.stop(true, STAMP, 'a', 1);
    expect(await recorder.retry('b', STAMP)).toBeNull();
    expect(recorder.pending).toBe(true);
    expect(recorder.error).toContain("la note n'existe plus");
    expect(recorder.retrying).toBe(false);
    expect(await recorder.retry('c', STAMP)).toEqual(SAVED);
    expect(narrationRetry).toHaveBeenNthCalledWith(2, 'c', STAMP);
    expect(recorder.pending).toBe(false);
    expect(recorder.error).toBeNull();
    // Nothing is waiting any more.
    expect(await recorder.retry('c', STAMP)).toBeNull();
    expect(narrationRetry).toHaveBeenCalledTimes(2);
  });

  it('forgets a pending recording Rust no longer has', async () => {
    const gone = async (): Promise<never> => {
      throw { code: 'noPendingNarration', message: 'none' };
    };
    const { recorder } = await recording({
      narrationStop: conflict,
      narrationStatus: async () => status({ pending: true }),
      narrationRetry: gone,
      narrationDiscard: gone,
    });
    await recorder.stop(true, STAMP, 'a', 1);
    await recorder.retry('a', STAMP);
    expect(recorder.pending).toBe(false);
    recorder.pending = true;
    expect(await recorder.discard()).toBe(true);
    expect(recorder.pending).toBe(false);
  });

  it('discards on request, and stays pending when discarding fails', async () => {
    const narrationDiscard = vi
      .fn<Commands['narrationDiscard']>()
      .mockRejectedValueOnce({ code: 'videoExportFailed', message: 'x' })
      .mockResolvedValue(undefined);
    const { recorder } = await recording({
      narrationStop: conflict,
      narrationStatus: async () => status({ pending: true }),
      narrationDiscard,
    });
    await recorder.stop(true, STAMP, 'a', 1);
    expect(await recorder.discard()).toBe(false);
    expect(recorder.pending).toBe(true);
    expect(await recorder.discard()).toBe(true);
    expect(narrationDiscard).toHaveBeenCalledTimes(2);
    expect(recorder.pending).toBe(false);
    expect(recorder.error).toBeNull();
  });

  it('finds a pending recording when asked (the panel appears)', async () => {
    setLocale('fr');
    const narrationStatus = vi.fn<Commands['narrationStatus']>(async () => status({ pending: true }));
    const recorder = new NarrationRecorder(scripted({ narrationStatus }));
    await recorder.refreshPending();
    expect(recorder.pending).toBe(true);
    narrationStatus.mockResolvedValue(status());
    await recorder.refreshPending();
    expect(recorder.pending).toBe(false);
  });
});
