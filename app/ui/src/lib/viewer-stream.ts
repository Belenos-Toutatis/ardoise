/**
 * The viewer's connection to the Ardoise PC (docs/design/lan-share.md, "The event stream"):
 * `POST /join` with the code, then `GET /events`, a Server-Sent Events stream read with `fetch`
 * (so that the status is visible: `401` means "needs the code", `503` "too many screens"; the
 * browser's `EventSource` hides both). The viewer page never imports Tauri nor `api.ts`.
 *
 * - `SseParser`: the text of the stream in chunks, the `data` payloads out.
 * - `ViewerStream`: connects, parses, hands every `ShareMessage` to the page, tells what the
 *   connection is doing, and reconnects by itself (0.5 s, 1, 2, 4, then every 5 s; the count
 *   starts again after a successful connection). `fetch` and timers are injected for the tests.
 * - `join`: the outcome of `POST /join`.
 */
import type { JoinRefusal, ShareMessage } from './share';

// --- Server-Sent Events ----------------------------------------------------------------------

/**
 * Parser of an event stream (WHATWG HTML, "Server-sent events"): lines end with `\n`, `\r\n` or
 * `\r`; a line starting with `:` is a comment; `data:` lines add to the event, which a blank line
 * completes (several `data:` lines are joined with `\n`); `retry:` gives the reconnection delay.
 * `event:` and `id:` are ignored (the server sends neither).
 */
export class SseParser {
  /** The last `retry:` value (ms) seen, or null. */
  retry: number | null = null;
  private buffer = '';
  private data: string[] = [];
  private first = true;

  /** Adds the next chunk of text; returns the payloads of the events it completed. */
  feed(chunk: string): string[] {
    this.buffer += chunk;
    if (this.first && this.buffer.length > 0) {
      this.first = false;
      if (this.buffer.charCodeAt(0) === 0xfeff) this.buffer = this.buffer.slice(1);
    }
    const out: string[] = [];
    let start = 0;
    for (;;) {
      let end = start;
      while (end < this.buffer.length && this.buffer[end] !== '\n' && this.buffer[end] !== '\r') end++;
      if (end >= this.buffer.length) break;
      // A `\r` at the very end may be the first half of `\r\n`: wait for the next chunk.
      if (this.buffer[end] === '\r' && end + 1 >= this.buffer.length) break;
      const line = this.buffer.slice(start, end);
      start = end + (this.buffer[end] === '\r' && this.buffer[end + 1] === '\n' ? 2 : 1);
      const payload = this.line(line);
      if (payload !== null) out.push(payload);
    }
    this.buffer = this.buffer.slice(start);
    return out;
  }

  /** One complete line; the payload of the event it completes, if any. */
  private line(line: string): string | null {
    if (line === '') {
      if (this.data.length === 0) return null;
      const payload = this.data.join('\n');
      this.data = [];
      return payload;
    }
    if (line.startsWith(':')) return null;
    const colon = line.indexOf(':');
    const field = colon < 0 ? line : line.slice(0, colon);
    let value = colon < 0 ? '' : line.slice(colon + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'data') this.data.push(value);
    else if (field === 'retry' && /^\d+$/.test(value)) this.retry = Number(value);
    return null;
  }
}

// --- The connection --------------------------------------------------------------------------

/** What the connection is doing. */
export type StreamStatus =
  /** Connecting (the first time, or the page asked to try again now). */
  | 'connecting'
  /** `200`: messages arrive. */
  | 'open'
  /** `401`: the code is needed (never given, or the share restarted). */
  | 'needsCode'
  /** `503`: the Ardoise PC has all the screens it accepts; tries again in 5 s. */
  | 'tooMany'
  /** The stream was cut or the PC is unreachable; tries again. */
  | 'reconnecting'
  /** The share stopped (`end` message); tries again every 5 s. */
  | 'ended';

/** Delays before the 1st, 2nd… attempt after a cut; the last one repeats. */
export const RECONNECT_DELAYS_MS = [500, 1000, 2000, 4000, 5000];
/** Retry delay after `503` and after the end of the share. */
export const SLOW_RETRY_MS = 5000;
/** The server sends a ping every 15 s: a stream silent for this long is dead (cable, sleep). */
export const SILENCE_MS = 45_000;

export interface StreamHandlers {
  onMessage(message: ShareMessage): void;
  onStatus(status: StreamStatus): void;
}

/** What the connection needs from the page; replaced by fakes in the tests. */
export interface StreamEnv {
  fetch: typeof fetch;
  setTimeout(handler: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

function browserEnv(): StreamEnv {
  return {
    fetch: (input, init) => fetch(input, init),
    setTimeout: (handler, ms) => setTimeout(handler, ms),
    clearTimeout: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
  };
}

/** A message of the stream, or null for anything that is not one (the page ignores it). */
export function parseMessage(payload: string): ShareMessage | null {
  let value: unknown;
  try {
    value = JSON.parse(payload);
  } catch {
    return null;
  }
  if (typeof value !== 'object' || value === null) return null;
  const type = (value as { type?: unknown }).type;
  switch (type) {
    case 'waiting':
    case 'end':
    case 'live':
    case 'note':
    case 'noteStrokes':
    case 'pause':
      return value as ShareMessage;
    default:
      return null;
  }
}

export class ViewerStream {
  private readonly handlers: StreamHandlers;
  private readonly env: StreamEnv;
  private running = false;
  /** Changes at every attempt and on `stop`: what a stale attempt reports is ignored. */
  private epoch = 0;
  private attempts = 0;
  /** The share said `end`: the PC is gone (its port closed), which is not a "connection lost". */
  private ended = false;
  private abort: AbortController | null = null;
  private retryTimer: unknown = null;
  private silenceTimer: unknown = null;

  constructor(handlers: StreamHandlers, env: StreamEnv = browserEnv()) {
    this.handlers = handlers;
    this.env = env;
  }

  /** Connects; later calls connect again at once (after a successful `join`). */
  start(): void {
    this.running = true;
    this.attempts = 0;
    this.ended = false;
    void this.attempt('connecting');
  }

  /** Closes the connection and stops trying. */
  stop(): void {
    this.running = false;
    this.epoch += 1;
    this.cancelTimers();
    this.abort?.abort();
    this.abort = null;
  }

  private cancelTimers(): void {
    if (this.retryTimer !== null) this.env.clearTimeout(this.retryTimer);
    if (this.silenceTimer !== null) this.env.clearTimeout(this.silenceTimer);
    this.retryTimer = null;
    this.silenceTimer = null;
  }

  private retryIn(ms: number, status: StreamStatus): void {
    this.cancelTimers();
    this.handlers.onStatus(status);
    this.retryTimer = this.env.setTimeout(() => {
      this.retryTimer = null;
      if (this.running) void this.attempt(null);
    }, ms);
  }

  /** The delay before the next attempt after a cut. */
  private reconnect(): void {
    if (this.ended) {
      this.retryIn(SLOW_RETRY_MS, 'ended');
      return;
    }
    const delay = RECONNECT_DELAYS_MS[Math.min(this.attempts, RECONNECT_DELAYS_MS.length - 1)];
    this.attempts += 1;
    this.retryIn(delay, 'reconnecting');
  }

  /** Watchdog: no byte (not even a ping) for `SILENCE_MS` means the connection is dead. */
  private watch(epoch: number, controller: AbortController): void {
    if (this.silenceTimer !== null) this.env.clearTimeout(this.silenceTimer);
    this.silenceTimer = this.env.setTimeout(() => {
      if (epoch === this.epoch) controller.abort();
    }, SILENCE_MS);
  }

  private async attempt(announce: StreamStatus | null): Promise<void> {
    this.cancelTimers();
    this.abort?.abort();
    const controller = new AbortController();
    this.abort = controller;
    const epoch = ++this.epoch;
    if (announce) this.handlers.onStatus(announce);
    const stale = () => epoch !== this.epoch || !this.running;

    let response: Response;
    try {
      response = await this.env.fetch('/events', {
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { Accept: 'text/event-stream' },
        signal: controller.signal,
      });
    } catch {
      if (!stale()) this.reconnect();
      return;
    }
    if (stale()) {
      response.body?.cancel().catch(() => undefined);
      return;
    }
    // The PC answered: whatever it says, it is no longer "ended" until it says `end` again.
    if (response.status === 401 || response.status === 503 || response.status === 200) this.ended = false;
    if (response.status === 401) {
      response.body?.cancel().catch(() => undefined);
      this.handlers.onStatus('needsCode');
      return;
    }
    if (response.status === 503) {
      response.body?.cancel().catch(() => undefined);
      this.retryIn(SLOW_RETRY_MS, 'tooMany');
      return;
    }
    if (response.status !== 200 || !response.body) {
      response.body?.cancel().catch(() => undefined);
      this.reconnect();
      return;
    }

    this.attempts = 0;
    this.handlers.onStatus('open');
    await this.read(response.body, epoch, controller);
    if (stale()) return;
    this.abort = null;
    this.reconnect();
  }

  /** Reads the stream to its end (or until the share says `end`). */
  private async read(body: ReadableStream<Uint8Array>, epoch: number, controller: AbortController): Promise<void> {
    const reader = body.getReader();
    const decoder = new TextDecoder();
    const parser = new SseParser();
    this.watch(epoch, controller);
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (epoch !== this.epoch) {
          reader.cancel().catch(() => undefined);
          return;
        }
        if (done) return;
        this.watch(epoch, controller);
        for (const payload of parser.feed(decoder.decode(value, { stream: true }))) {
          const message = parseMessage(payload);
          if (!message) continue;
          this.handlers.onMessage(message);
          if (epoch !== this.epoch) return;
          if (message.type === 'end') {
            this.ended = true;
            reader.cancel().catch(() => undefined);
            return;
          }
        }
      }
    } catch {
      return;
    } finally {
      // Not a newer attempt's timer.
      if (epoch === this.epoch && this.silenceTimer !== null) {
        this.env.clearTimeout(this.silenceTimer);
        this.silenceTimer = null;
      }
    }
  }
}

// --- Joining ---------------------------------------------------------------------------------

export type JoinResult =
  | { ok: true }
  | { ok: false; reason: 'wrongCode'; remaining: number }
  | { ok: false; reason: 'tooManyAttempts'; retryAfter: number }
  /** The page could not reach the PC. */
  | { ok: false; reason: 'network' }
  /** Any other answer. */
  | { ok: false; reason: 'error'; status: number };

/** The characters of a code: what the user typed, in capitals, without spaces or dashes, at most 6. */
export function normalizeCode(input: string): string {
  return input.toUpperCase().replace(/[^A-Z0-9]/g, '').slice(0, 6);
}

async function refusal(response: Response): Promise<Partial<JoinRefusal> | null> {
  try {
    const value: unknown = await response.json();
    return typeof value === 'object' && value !== null ? (value as Partial<JoinRefusal>) : null;
  } catch {
    return null;
  }
}

/** `POST /join` with `code`: `204` and the cookie, or why not. */
export async function join(code: string, doFetch: typeof fetch = (input, init) => fetch(input, init)): Promise<JoinResult> {
  let response: Response;
  try {
    response = await doFetch('/join', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ code: normalizeCode(code) }),
    });
  } catch {
    return { ok: false, reason: 'network' };
  }
  if (response.status === 204) return { ok: true };
  if (response.status === 403) {
    const body = await refusal(response);
    const remaining = body && 'remaining' in body && typeof body.remaining === 'number' ? Math.max(0, body.remaining) : 0;
    return { ok: false, reason: 'wrongCode', remaining };
  }
  if (response.status === 429) {
    const body = await refusal(response);
    let seconds = body && 'retryAfter' in body && typeof body.retryAfter === 'number' ? body.retryAfter : NaN;
    if (!Number.isFinite(seconds)) seconds = Number(response.headers.get('Retry-After'));
    return { ok: false, reason: 'tooManyAttempts', retryAfter: Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 1 };
  }
  return { ok: false, reason: 'error', status: response.status };
}
