/**
 * One narration recording, as the interface drives it: start with a target (the replayed note, or
 * live mode), follow the capture's state about 10 times a second (level, elapsed time, failures),
 * stop with or without saving. The editor and the live view each own one; the commands are the
 * `api` ones, so the browser stand-in (`mock-narration.ts`) answers in tests and in development.
 */
import { tick } from 'svelte';
import { api, errorMessage } from '../../lib/api';
import type { FileStamp } from '../../lib/note';
import type { NarrationSaved, NarrationStatus, NarrationTarget } from '../../lib/video';
import { monitorSettled } from './MicrophonePicker.svelte';

/** How often the capture's state is read, milliseconds (SPEC §4.5: about 10 times a second). */
export const STATUS_POLL_MS = 100;

type Commands = Pick<typeof api, 'narrationStart' | 'narrationStatus' | 'narrationStop'>;

export type RecorderPhase = 'idle' | 'starting' | 'recording' | 'saving';

/** What `stop` gives back: whether it worked, and what was saved (null when discarded). */
export interface StopResult {
  ok: boolean;
  saved: NarrationSaved | null;
}

export class NarrationRecorder {
  phase = $state<RecorderPhase>('idle');
  /** The last reading of the capture; null before the first. */
  status = $state.raw<NarrationStatus | null>(null);
  /** Why the last start or stop failed, as a sentence; null otherwise. */
  error = $state<string | null>(null);

  #timer: ReturnType<typeof setInterval> | undefined;
  #reading = false;
  readonly #commands: Commands;

  constructor(commands: Commands = api) {
    this.#commands = commands;
  }

  /** Recording time so far, milliseconds. */
  get elapsedMs(): number {
    return this.status?.elapsedMs ?? 0;
  }

  /** A recording is being made or saved: leaving the screen now would lose it. */
  get busy(): boolean {
    return this.phase !== 'idle';
  }

  /** Starts recording; false, with `error` set, when it could not start. */
  async start(device: string | null, target: NarrationTarget): Promise<boolean> {
    if (this.phase !== 'idle') return false;
    this.error = null;
    this.status = null;
    this.phase = 'starting';
    try {
      // The microphone picker listening for the level stops with the phase change: wait until it
      // has, because stopping it afterwards would stop this recording too.
      await tick();
      await monitorSettled();
      await this.#commands.narrationStart(device, target);
    } catch (error) {
      this.error = errorMessage(error);
      this.phase = 'idle';
      return false;
    }
    this.phase = 'recording';
    this.#watch();
    return true;
  }

  /**
   * Stops the recording. With `save` it is written (`stamp`: the note's, for a replay target); a
   * failure leaves `error` set and the capture ended, so the user can start again.
   */
  async stop(save: boolean, stamp: FileStamp | null): Promise<StopResult> {
    if (this.phase !== 'recording') return { ok: false, saved: null };
    this.phase = save ? 'saving' : 'recording';
    this.error = null;
    try {
      const saved = await this.#commands.narrationStop(save, stamp);
      this.#end();
      return { ok: true, saved };
    } catch (error) {
      this.error = errorMessage(error);
      // Whatever went wrong, no capture may be left running behind the screen.
      await this.#commands.narrationStop(false, null).catch(() => {});
      this.#end();
      return { ok: false, saved: null };
    }
  }

  /** Stops following the capture (the screen goes away); does not stop the capture itself. */
  dispose(): void {
    clearInterval(this.#timer);
    this.#timer = undefined;
  }

  #end(): void {
    this.dispose();
    this.phase = 'idle';
  }

  #watch(): void {
    this.dispose();
    const read = async () => {
      if (this.#reading) return;
      this.#reading = true;
      try {
        const next = await this.#commands.narrationStatus();
        if (this.phase === 'recording') this.status = next;
      } catch {
        // The next reading may work.
      } finally {
        this.#reading = false;
      }
    };
    void read();
    this.#timer = setInterval(read, STATUS_POLL_MS);
  }
}
