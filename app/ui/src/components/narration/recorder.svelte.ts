/**
 * One narration recording, as the interface drives it: start with a target (the replayed note, or
 * live mode), follow the capture's state about 10 times a second (level, elapsed time, failures),
 * stop with or without saving. The editor and the live view each own one; the commands are the
 * `api` ones, so the browser stand-in (`mock-narration.ts`) answers in tests and in development.
 *
 * A recording that cannot be saved is not lost: Rust keeps it (`pending`, with a backup file) until
 * `retry` saves it somewhere or `discard` throws it away.
 */
import { tick } from 'svelte';
import { api, errorMessage } from '../../lib/api';
import type { FileStamp } from '../../lib/note';
import type { NarrationSaved, NarrationStatus, NarrationTarget } from '../../lib/video';
import { monitorSettled } from './MicrophonePicker.svelte';

/** How often the capture's state is read, milliseconds (SPEC §4.5: about 10 times a second). */
export const STATUS_POLL_MS = 100;

export type RecorderCommands = Pick<
  typeof api,
  'narrationStart' | 'narrationStatus' | 'narrationStop' | 'narrationRetry' | 'narrationDiscard'
>;

export type RecorderPhase = 'idle' | 'starting' | 'recording' | 'saving';

/** What `stop` gives back: whether it worked, and what was saved (null when discarded). */
export interface StopResult {
  ok: boolean;
  saved: NarrationSaved | null;
  /** The error code when it failed (`emptyRecording`, `noteConflict`…), if Rust gave one. */
  code?: string | null;
}

/** The code of an error thrown by a command, if it has one. */
function codeOf(error: unknown): string | null {
  const code = typeof error === 'object' && error !== null ? (error as { code?: unknown }).code : null;
  return typeof code === 'string' ? code : null;
}

export class NarrationRecorder {
  phase = $state<RecorderPhase>('idle');
  /** The last reading of the capture; null before the first. */
  status = $state.raw<NarrationStatus | null>(null);
  /** Why the last start or stop failed, as a sentence; null otherwise. */
  error = $state<string | null>(null);
  /** A recording that could not be saved is kept, waiting for `retry` or `discard`. */
  pending = $state(false);
  /** `retry` is writing the pending recording. */
  retrying = $state(false);

  #timer: ReturnType<typeof setInterval> | undefined;
  #reading = false;
  readonly #commands: RecorderCommands;

  constructor(commands: RecorderCommands = api) {
    this.#commands = commands;
  }

  /** Recording time so far, milliseconds. */
  get elapsedMs(): number {
    return this.status?.elapsedMs ?? 0;
  }

  /** A recording is being made or saved: leaving the screen now would lose it. */
  get busy(): boolean {
    return this.phase !== 'idle' || this.retrying;
  }

  /** Starts recording; false, with `error` set, when it could not start. */
  async start(device: string | null, target: NarrationTarget): Promise<boolean> {
    if (this.phase !== 'idle' || this.pending || this.retrying) return false;
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
   * Stops the recording. With `save` it is written (`stamp`, `path` and `replayStartedAtMs` for a
   * replay target: the note's current stamp and path, and when the replay was at instant 0). A
   * failure leaves `error` set and the capture ended; if Rust kept the recording, `pending` is set
   * and the user chooses between `retry` and `discard`.
   */
  async stop(
    save: boolean,
    stamp: FileStamp | null,
    path: string | null = null,
    replayStartedAtMs: number | null = null,
  ): Promise<StopResult> {
    if (this.phase !== 'recording') return { ok: false, saved: null };
    this.phase = save ? 'saving' : 'recording';
    this.error = null;
    try {
      const saved = await this.#commands.narrationStop(save, stamp, path, replayStartedAtMs);
      this.#end();
      return { ok: true, saved };
    } catch (error) {
      this.error = errorMessage(error);
      await this.#afterFailure();
      this.#end();
      return { ok: false, saved: null, code: codeOf(error) };
    }
  }

  /** Saves the pending recording into the note at `path`; null, with `error` set, when it fails again. */
  async retry(path: string, stamp: FileStamp | null): Promise<NarrationSaved | null> {
    if (!this.pending || this.phase !== 'idle' || this.retrying) return null;
    this.error = null;
    this.retrying = true;
    try {
      const saved = await this.#commands.narrationRetry(path, stamp);
      this.pending = false;
      return saved;
    } catch (error) {
      this.error = errorMessage(error);
      // The recording stays pending, unless Rust says there is none (any more).
      if (codeOf(error) === 'noPendingNarration') this.pending = false;
      return null;
    } finally {
      this.retrying = false;
    }
  }

  /** Throws the pending recording away. */
  async discard(): Promise<boolean> {
    if (!this.pending || this.phase !== 'idle') return false;
    this.error = null;
    try {
      await this.#commands.narrationDiscard();
    } catch (error) {
      // Already gone is as good as discarded; anything else leaves it pending.
      if (codeOf(error) !== 'noPendingNarration') {
        this.error = errorMessage(error);
        return false;
      }
    }
    this.pending = false;
    return true;
  }

  /** Asks Rust whether a recording is waiting (the panel was just opened, or a stop failed). */
  async refreshPending(): Promise<void> {
    try {
      const status = await this.#commands.narrationStatus();
      if (this.phase === 'idle') this.pending = status.pending;
    } catch {
      // Unknown: nothing is offered.
    }
  }

  /** Stops following the capture (the screen goes away); does not stop the capture itself. */
  dispose(): void {
    clearInterval(this.#timer);
    this.#timer = undefined;
  }

  /**
   * After a failed stop: a recording Rust kept is `pending`; a capture still running (the stop
   * failed before ending it) is thrown away, because none may be left running behind the screen.
   */
  async #afterFailure(): Promise<void> {
    try {
      const status = await this.#commands.narrationStatus();
      this.pending = status.pending;
      if (status.state === 'recording') await this.#commands.narrationStop(false, null, null, null);
    } catch {
      // Nothing more can be known.
    }
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
