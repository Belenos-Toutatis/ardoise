/**
 * The share on the local network as the main window sees it (milestone M11; contract:
 * docs/design/lan-share.md, "Interface"): one subscription to Rust's `share_subscribe` for the
 * whole window, the status, whether the panel is open, the failure of the last action, and
 * **what is shown** — `showLive()`, `showNote(note, imageKey)`, `showWaiting()` — which the live
 * view, the editor and the library call as they open and close.
 *
 * The last content is always remembered, also while no share runs; it is sent when the share
 * starts (the status turns active) and whenever it changes while active. While inactive nothing
 * is sent. A note changes at the pen's pace, so it goes out debounced: 100 ms after the last
 * change, and at most every 300 ms while changes keep coming; live and waiting go at once.
 */
import { api, errorMessage } from './api';
import type { Note } from './note';
import { INACTIVE_SHARE, type ShareContent, type ShareStatus } from './share';

/** A note is sent this long after its last change… */
export const NOTE_DEBOUNCE_MS = 100;
/** …but never less than this long after the previous note, and never later than this after the first unsent change. */
export const NOTE_INTERVAL_MS = 300;

export class ShareState {
  /** Last status received. */
  status = $state.raw<ShareStatus>(INACTIVE_SHARE);
  /** The share panel is open (it lives in `Home.svelte`, over the editor and live mode). */
  panelOpen = $state(false);
  /** Translated message of the last action that failed; null after a success. */
  error = $state<string | null>(null);
  /** `share_start` is under way. */
  starting = $state(false);
  /** Pause or stop is under way. */
  busy = $state(false);

  /**
   * `status.active` as a plain field: `showLive`, `showNote` and `showWaiting` are called from
   * `$effect`s, which must not start depending on the status by calling them.
   */
  private running = false;
  private following = false;
  /** Events received so far: an answer to a call is older than the status when one arrived meanwhile. */
  private events = 0;
  /** What the main window shows; remembered while inactive. */
  private content: ShareContent = { kind: 'waiting' };
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** When the first change not yet sent was made. */
  private firstPending: number | null = null;
  /** When the last note was sent. */
  private lastSent = Number.NEGATIVE_INFINITY;
  /** Sends go out one after the other, in order. */
  private queue: Promise<void> = Promise.resolve();

  /** Whether a share runs. */
  get active(): boolean {
    return this.status.active;
  }

  /** Starts following Rust's status (once for the whole window). */
  follow(): void {
    if (this.following) return;
    this.following = true;
    const before = this.events;
    api
      .shareSubscribe((status) => {
        this.events += 1;
        this.apply(status);
      })
      .then(
        (status) => {
          if (this.events === before) this.apply(status);
        },
        () => {
          // No service (a build without the share): the button's start reports it.
          this.following = false;
        },
      );
  }

  /** Forgets everything and subscribes again at the next `follow()` (tests). */
  reset(): void {
    this.cancelPending();
    this.following = false;
    this.events = 0;
    this.status = INACTIVE_SHARE;
    this.running = false;
    this.panelOpen = false;
    this.error = null;
    this.starting = false;
    this.busy = false;
    this.content = { kind: 'waiting' };
    this.lastSent = Number.NEGATIVE_INFINITY;
    this.queue = Promise.resolve();
  }

  /** Takes a status; a share that turns active gets what the window shows right away. */
  private apply(status: ShareStatus): void {
    const was = this.running;
    this.status = status;
    this.running = status.active;
    if (!status.active) this.cancelPending();
    else if (!was) this.sendNow();
  }

  /** The answer to a call, unless an event (which is newer) was received while it was pending. */
  private answered(eventsBefore: number, status: ShareStatus): void {
    if (this.events === eventsBefore) this.apply(status);
  }

  // --- What is shown --------------------------------------------------------------------------

  /** Live mode is open. */
  showLive(): void {
    this.content = { kind: 'live' };
    this.sendNow();
  }

  /** The library (or anything but the editor and live mode) is shown. */
  showWaiting(): void {
    this.content = { kind: 'waiting' };
    this.sendNow();
  }

  /** The editor shows `note` (unsaved changes included; `imageKey`: its staged background image). */
  showNote(note: Note, imageKey: string | null): void {
    this.content = { kind: 'note', note, imageKey };
    if (!this.running) return;
    const now = Date.now();
    this.firstPending ??= now;
    const due = Math.max(Math.min(now + NOTE_DEBOUNCE_MS, this.firstPending + NOTE_INTERVAL_MS), this.lastSent + NOTE_INTERVAL_MS);
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => this.sendNow(), Math.max(0, due - now));
  }

  private cancelPending(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    this.firstPending = null;
  }

  /** Sends the current content now, if a share runs. */
  private sendNow(): void {
    this.cancelPending();
    if (!this.running) return;
    const content = this.content;
    if (content.kind === 'note') this.lastSent = Date.now();
    this.queue = this.queue
      .then(() => api.shareShow(content))
      .catch((error: unknown) => {
        this.error = errorMessage(error);
      });
  }

  // --- Actions ---------------------------------------------------------------------------------

  /** Opens the panel, and starts the share if none runs. */
  openPanel(): void {
    this.follow();
    this.panelOpen = true;
    if (!this.running && !this.starting) void this.start();
  }

  closePanel(): void {
    this.panelOpen = false;
  }

  /** Starts the share (or finds it running); false when it failed, the reason in `error`. */
  async start(): Promise<boolean> {
    this.follow();
    if (this.starting) return false;
    this.error = null;
    this.starting = true;
    try {
      const before = this.events;
      this.answered(before, await api.shareStart());
      return true;
    } catch (error) {
      this.error = errorMessage(error);
      return false;
    } finally {
      this.starting = false;
    }
  }

  /** Stops the share and closes the panel. */
  async stop(): Promise<boolean> {
    return this.run(async () => {
      const before = this.events;
      this.answered(before, await api.shareStop());
      this.panelOpen = false;
    });
  }

  /** Freezes the shared image, or lets it move again. */
  async setPaused(paused: boolean): Promise<boolean> {
    return this.run(async () => {
      const before = this.events;
      this.answered(before, await api.shareSetPaused(paused));
    });
  }

  private async run(action: () => Promise<void>): Promise<boolean> {
    if (this.busy) return false;
    this.error = null;
    this.busy = true;
    try {
      await action();
      return true;
    } catch (error) {
      this.error = errorMessage(error);
      return false;
    } finally {
      this.busy = false;
    }
  }

  dismissError(): void {
    this.error = null;
  }
}

/** The window's share. */
export const share = new ShareState();
