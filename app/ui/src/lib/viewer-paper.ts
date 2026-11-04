/**
 * Fetches the picture of the paper of a live page for the viewer page (docs/design/lan-share.md,
 * "The paper of a live page"): the stream only says which one (`livePaper`), so that a picture of
 * megabytes on a slow Wi-Fi never holds up the strokes. Whatever happens here leaves the stream and
 * the sheet alone: until the picture is there, the paper colour shows.
 *
 * - One fetch at a time: another paper wanted drops the fetch under way (and its retries).
 * - A fetch that fails is tried again a few times, then left until the PC tells of the paper again
 *   (each subscription does: a page turned, a resume, a reconnection). `401` is not tried again: the
 *   session is gone, which the stream finds out too and shows the code form.
 * - The picture is held as one blob; its URL (`blob:`, which the page's CSP allows) is given up when
 *   another one takes its place or the viewer forgets it.
 */

/** Waits before the 2nd, 3rd… attempt; after the last one, the paper colour stays. */
export const PAPER_RETRY_DELAYS_MS = [1000, 2000, 4000, 8000];
/**
 * A fetch that lasts longer is dropped and tried again: the PC gives up by itself after 5 minutes, so
 * this is a connection that died without a word (a Wi-Fi that dropped), which the browser may never
 * notice on a socket it only reads.
 */
export const PAPER_FETCH_TIMEOUT_MS = 6 * 60_000;

/** The picture in hand. */
export interface LoadedPaper {
  key: string;
  /** What an image can load. */
  url: string;
}

/** What the loader uses of the browser (the tests give fakes). */
export interface PaperEnv {
  fetch(input: string, init: RequestInit): Promise<Response>;
  setTimeout(handler: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  createUrl(blob: Blob): string;
  revokeUrl(url: string): void;
}

function browserEnv(): PaperEnv {
  return {
    fetch: (input, init) => fetch(input, init),
    setTimeout: (handler, ms) => setTimeout(handler, ms),
    clearTimeout: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
    createUrl: (blob) => URL.createObjectURL(blob),
    revokeUrl: (url) => URL.revokeObjectURL(url),
  };
}

/** Why an attempt gave no picture. */
type Failure = 'retry' | 'sessionGone';

export class PaperLoader {
  private readonly onPaper: (paper: LoadedPaper | null) => void;
  private readonly env: PaperEnv;
  /** What the PC last told of; null once forgotten. */
  private wanted: { key: string; version: string } | null = null;
  /** The picture in hand and which version it is. */
  private loaded: (LoadedPaper & { version: string }) | null = null;
  /** A fetch is under way for `wanted`. */
  private fetching = false;
  /** Failed attempts for `wanted`. */
  private failures = 0;
  private abort: AbortController | null = null;
  private retryTimer: unknown = null;
  /** Changes when `wanted` does: what a dropped fetch reports is ignored. */
  private epoch = 0;

  constructor(onPaper: (paper: LoadedPaper | null) => void, env: PaperEnv = browserEnv()) {
    this.onPaper = onPaper;
    this.env = env;
  }

  /**
   * The PC tells of a paper. Nothing to do when it is the picture in hand or the one on its way;
   * after failures it is tried again at once, the PC having just said that it is there.
   */
  want(key: string, version: string): void {
    const inHand = this.loaded?.key === key && this.loaded.version === version;
    const same = this.wanted?.key === key && this.wanted.version === version;
    if (same && (inHand || this.fetching)) return;
    this.drop();
    this.wanted = { key, version };
    if (!inHand) void this.attempt(key);
  }

  /** The share ended, or the code is asked again: nothing is kept, a new share tells again. */
  forget(): void {
    this.drop();
    this.wanted = null;
    const url = this.loaded?.url;
    this.loaded = null;
    this.onPaper(null);
    if (url) this.env.revokeUrl(url);
  }

  /** Whatever is under way or waiting stops. */
  private drop(): void {
    this.epoch += 1;
    this.fetching = false;
    this.failures = 0;
    if (this.retryTimer !== null) this.env.clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.abort?.abort();
    this.abort = null;
  }

  private async attempt(key: string): Promise<void> {
    const epoch = this.epoch;
    const controller = new AbortController();
    this.abort = controller;
    this.fetching = true;
    const watchdog = this.env.setTimeout(() => controller.abort(), PAPER_FETCH_TIMEOUT_MS);
    let result: Blob | Failure;
    try {
      result = await this.download(key, controller.signal);
    } catch {
      result = 'retry';
    }
    this.env.clearTimeout(watchdog);
    if (epoch !== this.epoch) return;
    this.fetching = false;
    this.abort = null;
    if (result === 'retry') this.retry(key);
    else if (result !== 'sessionGone') this.install(key, result);
  }

  /** The picture, or why not. */
  private async download(key: string, signal: AbortSignal): Promise<Blob | Failure> {
    const response = await this.env.fetch(`/paper/${encodeURIComponent(key)}`, {
      credentials: 'same-origin',
      cache: 'no-store',
      signal,
    });
    if (!response.ok) {
      response.body?.cancel().catch(() => undefined);
      return response.status === 401 ? 'sessionGone' : 'retry';
    }
    const blob = await response.blob();
    // A filtering proxy's page is not a picture.
    return blob.type.startsWith('image/') ? blob : 'retry';
  }

  private install(key: string, blob: Blob): void {
    const old = this.loaded?.url;
    const url = this.env.createUrl(blob);
    this.loaded = { key, url, version: this.wanted?.version ?? '' };
    this.onPaper({ key, url });
    if (old) this.env.revokeUrl(old);
  }

  private retry(key: string): void {
    const delay = PAPER_RETRY_DELAYS_MS[this.failures];
    this.failures += 1;
    if (delay === undefined) return;
    this.retryTimer = this.env.setTimeout(() => {
      this.retryTimer = null;
      void this.attempt(key);
    }, delay);
  }
}
