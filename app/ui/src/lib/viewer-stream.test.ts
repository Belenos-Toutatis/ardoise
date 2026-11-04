import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ShareMessage } from './share';
import {
  RECONNECT_DELAYS_MS,
  SILENCE_MS,
  SLOW_RETRY_MS,
  SseParser,
  ViewerStream,
  join,
  normalizeCode,
  parseMessage,
  type StreamEnv,
  type StreamStatus,
} from './viewer-stream';

// --- The parser -------------------------------------------------------------------------------

function parseAll(chunks: string[]): { payloads: string[]; retry: number | null } {
  const parser = new SseParser();
  const payloads = chunks.flatMap((chunk) => parser.feed(chunk));
  return { payloads, retry: parser.retry };
}

describe('SSE parser', () => {
  it('ignores the padding sent through a proxy, line after line, and finds the message after it', () => {
    const parser = new SseParser();
    const line = `:${' '.repeat(1022)}\n`;
    const padded = `retry: 2000\n\n${line.repeat(2048)}data: {"type":"waiting"}\n\n`;
    const messages: string[] = [];
    for (let at = 0; at < padded.length; at += 4096) messages.push(...parser.feed(padded.slice(at, at + 4096)));
    expect(messages).toEqual(['{"type":"waiting"}']);
  });

  it('reads a message of megabytes arriving in small chunks in one pass', () => {
    const big = JSON.stringify({ type: 'note', page: { strokes: 'x'.repeat(3_000_000) } });
    const text = `data: ${big}\n\ndata: {"type":"waiting"}\r\n\r\n`;
    const parser = new SseParser();
    const payloads: string[] = [];
    const began = performance.now();
    for (let i = 0; i < text.length; i += 4096) payloads.push(...parser.feed(text.slice(i, i + 4096)));
    const took = performance.now() - began;
    expect(payloads).toEqual([big, '{"type":"waiting"}']);
    // Searching the whole buffer again at every chunk took seconds here.
    expect(took).toBeLessThan(1500);
  });

  it('returns the data of each event once its blank line arrived', () => {
    const parser = new SseParser();
    expect(parser.feed('data: {"type":"waiting"}\n')).toEqual([]);
    expect(parser.feed('\n')).toEqual(['{"type":"waiting"}']);
    expect(parser.feed('data: a\n\ndata: b\n\n')).toEqual(['a', 'b']);
  });

  it('ignores comments (pings), `event:`, `id:` and unknown fields', () => {
    expect(parseAll([': ping\n\n', 'event: x\nid: 4\nfoo: bar\ndata: 1\n\n']).payloads).toEqual(['1']);
  });

  it('keeps the last retry delay', () => {
    const { payloads, retry } = parseAll(['retry: 2000\n\n', 'retry: abc\n\n', 'data: x\n\n']);
    expect(payloads).toEqual(['x']);
    expect(retry).toBe(2000);
  });

  it('joins the data lines of an event with a newline', () => {
    expect(parseAll(['data: a\ndata:b\ndata\ndata:  c\n\n']).payloads).toEqual(['a\nb\n\n c']);
  });

  it('handles \\r\\n and a lone \\r as line ends', () => {
    expect(parseAll(['data: a\r\n\r\ndata: b\r\rdata: c\r\n\r\n']).payloads).toEqual(['a', 'b', 'c']);
  });

  it('does not take the halves of a \\r\\n split between chunks for two line ends', () => {
    expect(parseAll(['data: a\r', '\n\r', '\n', 'data: b\r\n\r\n']).payloads).toEqual(['a', 'b']);
  });

  it('gives the same events whatever the chunks', () => {
    const text = 'retry: 2000\n\n: ping\n\ndata: {"a":1}\r\n\r\ndata: x\ndata: y\n\ndata: é\n\n';
    const whole = parseAll([text]);
    expect(whole.payloads).toEqual(['{"a":1}', 'x\ny', 'é']);
    for (let size = 1; size <= 7; size++) {
      const chunks = [];
      for (let i = 0; i < text.length; i += size) chunks.push(text.slice(i, i + size));
      expect(parseAll(chunks), `chunks of ${size}`).toEqual(whole);
    }
  });

  it('does not return an event that is not complete, nor an empty one', () => {
    expect(parseAll(['data: a\n', '\n\n\n']).payloads).toEqual(['a']);
    expect(parseAll(['data: partial']).payloads).toEqual([]);
  });

  it('skips a byte order mark at the start', () => {
    expect(parseAll(['﻿data: a\n\n']).payloads).toEqual(['a']);
  });
});

describe('parseMessage', () => {
  it('accepts the messages of the contract and nothing else', () => {
    for (const type of ['waiting', 'end', 'live', 'note', 'noteStrokes', 'pause']) {
      expect(parseMessage(JSON.stringify({ type }))).toEqual({ type });
    }
    expect(parseMessage('{"type":"other"}')).toBeNull();
    expect(parseMessage('{"nothing":1}')).toBeNull();
    expect(parseMessage('null')).toBeNull();
    expect(parseMessage('12')).toBeNull();
    expect(parseMessage('{oops')).toBeNull();
  });
});

// --- The connection ---------------------------------------------------------------------------

const encoder = new TextEncoder();

/** The body of a `200` answer, written by the test; it errors when the request is aborted. */
function eventBody(signal?: AbortSignal | null) {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      controller = c;
    },
  });
  signal?.addEventListener('abort', () => {
    try {
      controller.error(new DOMException('aborted', 'AbortError'));
    } catch {
      // Already closed.
    }
  });
  return {
    body,
    push: (text: string) => controller.enqueue(encoder.encode(text)),
    pushBytes: (bytes: Uint8Array) => controller.enqueue(bytes),
    close: () => controller.close(),
  };
}

const sse = (message: ShareMessage) => `data: ${JSON.stringify(message)}\n\n`;

type Answer = (signal: AbortSignal | null | undefined) => Response | Promise<Response>;

/** A `fetch` that answers from a queue, and timers that run when the test says. */
function fakeEnv(answers: Answer[]) {
  const timers: { handle: number; ms: number; handler: () => void }[] = [];
  let next = 1;
  const requests: { url: string; init: RequestInit | undefined }[] = [];
  const env: StreamEnv = {
    fetch: ((input: RequestInfo | URL, init?: RequestInit) => {
      requests.push({ url: String(input), init });
      const answer = answers.shift();
      if (!answer) return Promise.reject(new TypeError('no more answers'));
      return Promise.resolve(answer(init?.signal));
    }) as typeof fetch,
    setTimeout(handler, ms) {
      const handle = next++;
      timers.push({ handle, ms, handler });
      return handle;
    },
    clearTimeout(handle) {
      const i = timers.findIndex((t) => t.handle === handle);
      if (i >= 0) timers.splice(i, 1);
    },
  };
  return {
    env,
    requests,
    timers,
    /** Delay of the pending timers. */
    pending: () => timers.map((t) => t.ms),
    /** Runs the earliest pending timer. */
    fire: () => {
      const t = timers.shift();
      if (!t) throw new Error('no timer pending');
      t.handler();
    },
    answers,
  };
}

const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function connect(answers: Answer[]) {
  const env = fakeEnv(answers);
  const statuses: StreamStatus[] = [];
  const messages: ShareMessage[] = [];
  const stream = new ViewerStream({ onMessage: (m) => messages.push(m), onStatus: (s) => statuses.push(s) }, env.env);
  return { ...env, stream, statuses, messages };
}

const status = (code: number) => () => new Response(null, { status: code });

afterEach(() => {
  vi.restoreAllMocks();
});

describe('stream: answers', () => {
  it('asks the code on 401 and does not retry by itself', async () => {
    const c = connect([status(401)]);
    c.stream.start();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'needsCode']);
    expect(c.pending()).toEqual([]);
    expect(c.requests).toHaveLength(1);
    expect(c.requests[0].url).toBe('/events');
    expect(c.requests[0].init?.credentials).toBe('same-origin');
  });

  it('says there are too many screens on 503 and tries again in 5 s', async () => {
    const body = eventBody();
    const c = connect([status(503), () => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'tooMany']);
    expect(c.pending()).toEqual([SLOW_RETRY_MS]);
    c.fire();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'tooMany', 'open']);
    body.push(sse({ type: 'waiting' }));
    await settle();
    expect(c.messages).toEqual([{ type: 'waiting' }]);
  });

  it('opens on 200 and hands the messages over, in order, whatever the chunks', async () => {
    const body = eventBody();
    const c = connect([() => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'open']);
    const first = sse({ type: 'waiting' });
    body.push('retry: 2000\n\n: ping\n\n');
    body.push(first.slice(0, 10));
    body.push(first.slice(10) + 'data: not json\n\n' + sse({ type: 'pause', paused: true }).slice(0, 5));
    body.push(sse({ type: 'pause', paused: true }).slice(5) + 'data: {"type":"unknown"}\n\n');
    await settle();
    expect(c.messages).toEqual([{ type: 'waiting' }, { type: 'pause', paused: true }]);
  });

  it('decodes a character cut between two chunks', async () => {
    const body = eventBody();
    const c = connect([() => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    const bytes = encoder.encode(sse({ type: 'note', page: { id: 'é€' } as never, background: null }));
    const cut = bytes.indexOf(0xc3) + 1;
    body.pushBytes(bytes.slice(0, cut));
    body.pushBytes(bytes.slice(cut));
    await settle();
    expect((c.messages[0] as { page: { id: string } }).page.id).toBe('é€');
  });

  it('treats any other status like a cut', async () => {
    const c = connect([status(500)]);
    c.stream.start();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'reconnecting']);
    expect(c.pending()).toEqual([RECONNECT_DELAYS_MS[0]]);
  });
});

describe('stream: reconnecting', () => {
  it('tries again after the end of the stream: 0.5 s, 1, 2, 4, then every 5 s', async () => {
    const first = eventBody();
    const c = connect([() => new Response(first.body, { status: 200 })]);
    c.stream.start();
    await settle();
    first.close();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'open', 'reconnecting']);
    expect(c.pending()).toEqual([500]);

    const delays: number[] = [];
    // The network is down: every attempt fails (no answer left in the queue).
    for (let i = 0; i < 7; i++) {
      c.fire();
      await settle();
      delays.push(...c.pending());
    }
    expect(delays).toEqual([1000, 2000, 4000, 5000, 5000, 5000, 5000]);
    expect(c.statuses.filter((s) => s !== 'reconnecting')).toEqual(['connecting', 'open']);
  });

  it('starts the delays again after a successful connection', async () => {
    const bodies = [eventBody(), eventBody(), eventBody()];
    const c = connect([
      () => new Response(bodies[0].body, { status: 200 }),
      status(500),
      status(500),
      () => new Response(bodies[1].body, { status: 200 }),
    ]);
    c.stream.start();
    await settle();
    bodies[0].close();
    await settle();
    expect(c.pending()).toEqual([500]);
    c.fire();
    await settle();
    expect(c.pending()).toEqual([1000]);
    c.fire();
    await settle();
    expect(c.pending()).toEqual([2000]);
    c.fire();
    await settle();
    expect(c.statuses.at(-1)).toBe('open');
    bodies[1].close();
    await settle();
    expect(c.pending()).toEqual([500]);
  });

  it('reconnects when the network fails while reading', async () => {
    const body = eventBody();
    const c = connect([() => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    body.push(sse({ type: 'waiting' }));
    await settle();
    // A reader error: the connection dropped.
    const failing = new ReadableStream<Uint8Array>({
      pull() {
        throw new TypeError('network error');
      },
    });
    c.answers.push(() => new Response(failing, { status: 200 }));
    body.close();
    await settle();
    c.fire();
    await settle();
    expect(c.statuses.slice(-2)).toEqual(['open', 'reconnecting']);
  });

  it('gives up on a stream that stayed silent longer than the pings', async () => {
    const quiet = eventBody;
    let aborted = false;
    const c = connect([
      (signal) => {
        const body = quiet(signal);
        signal?.addEventListener('abort', () => (aborted = true));
        return new Response(body.body, { status: 200 });
      },
    ]);
    c.stream.start();
    await settle();
    expect(c.pending()).toEqual([SILENCE_MS]);
    c.fire();
    await settle();
    expect(aborted).toBe(true);
    expect(c.statuses.at(-1)).toBe('reconnecting');
    expect(c.pending()).toEqual([500]);
  });

  it('resets the silence watchdog at each chunk', async () => {
    const body = eventBody();
    const c = connect([() => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    const before = c.timers[0].handle;
    body.push(': ping\n\n');
    await settle();
    expect(c.pending()).toEqual([SILENCE_MS]);
    expect(c.timers[0].handle).not.toBe(before);
  });
});

describe('stream: end of the share', () => {
  it('shows the end, closes, and tries again every 5 s; a new share asks for the code', async () => {
    const body = eventBody();
    const c = connect([() => new Response(body.body, { status: 200 }), status(401)]);
    c.stream.start();
    await settle();
    body.push(sse({ type: 'waiting' }) + sse({ type: 'end' }) + sse({ type: 'pause', paused: true }));
    await settle();
    // Nothing after `end` is read.
    expect(c.messages).toEqual([{ type: 'waiting' }, { type: 'end' }]);
    expect(c.statuses).toEqual(['connecting', 'open', 'ended']);
    expect(c.pending()).toEqual([SLOW_RETRY_MS]);
    c.fire();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'open', 'ended', 'needsCode']);
    expect(c.pending()).toEqual([]);
  });

  it('keeps trying every 5 s, and stays "ended", while the PC does not answer (its port is closed)', async () => {
    const body = eventBody();
    const c = connect([() => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    body.push(sse({ type: 'end' }));
    await settle();
    for (let i = 0; i < 3; i++) {
      expect(c.pending()).toEqual([SLOW_RETRY_MS]);
      c.fire();
      await settle();
    }
    expect(c.statuses.filter((s) => s === 'reconnecting')).toEqual([]);
    expect(c.statuses.filter((s) => s === 'ended')).toHaveLength(4);
  });
});

describe('stream: start and stop', () => {
  it('stop closes the connection and cancels what was planned', async () => {
    let aborted = false;
    const c = connect([
      (signal) => {
        signal?.addEventListener('abort', () => (aborted = true));
        return new Response(eventBody(signal).body, { status: 200 });
      },
    ]);
    c.stream.start();
    await settle();
    c.stream.stop();
    await settle();
    expect(aborted).toBe(true);
    expect(c.pending()).toEqual([]);
    expect(c.statuses).toEqual(['connecting', 'open']);
    expect(c.requests).toHaveLength(1);
  });

  it('stop cancels a retry that is waiting', async () => {
    const c = connect([status(503)]);
    c.stream.start();
    await settle();
    expect(c.pending()).toEqual([SLOW_RETRY_MS]);
    c.stream.stop();
    expect(c.pending()).toEqual([]);
  });

  it('start again (after the code was accepted) connects at once and drops the old connection', async () => {
    const old = eventBody();
    const fresh = eventBody();
    let oldAborted = false;
    const c = connect([
      (signal) => {
        signal?.addEventListener('abort', () => (oldAborted = true));
        return new Response(old.body, { status: 200 });
      },
      () => new Response(fresh.body, { status: 200 }),
    ]);
    c.stream.start();
    await settle();
    c.stream.start();
    await settle();
    expect(oldAborted).toBe(true);
    old.push(sse({ type: 'waiting' }));
    fresh.push(sse({ type: 'pause', paused: false }));
    await settle();
    expect(c.messages).toEqual([{ type: 'pause', paused: false }]);
    expect(c.statuses.filter((s) => s === 'reconnecting')).toEqual([]);
  });

  it('reconnects after a code was refused then accepted: 401, then start, then 200', async () => {
    const body = eventBody();
    const c = connect([status(401), () => new Response(body.body, { status: 200 })]);
    c.stream.start();
    await settle();
    c.stream.start();
    await settle();
    expect(c.statuses).toEqual(['connecting', 'needsCode', 'connecting', 'open']);
  });
});

// --- Joining ----------------------------------------------------------------------------------

function fakeFetch(answer: () => Response | Promise<Response>) {
  const calls: { url: string; init: RequestInit | undefined }[] = [];
  const doFetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), init });
    return Promise.resolve().then(answer);
  }) as typeof fetch;
  return { doFetch, calls };
}

describe('normalizeCode', () => {
  it('keeps capitals and digits, at most 6', () => {
    expect(normalizeCode('k7m q2x')).toBe('K7MQ2X');
    expect(normalizeCode(' K7M-Q2X ')).toBe('K7MQ2X');
    expect(normalizeCode('abcdefghij')).toBe('ABCDEF');
    expect(normalizeCode('')).toBe('');
  });
});

describe('join', () => {
  it('posts the code as JSON and accepts 200', async () => {
    const f = fakeFetch(() => new Response('{"joined":true}', { status: 200 }));
    expect(await join('k7m q2x', f.doFetch)).toEqual({ ok: true });
    expect(f.calls).toHaveLength(1);
    expect(f.calls[0].url).toBe('/join');
    expect(f.calls[0].init?.method).toBe('POST');
    expect(f.calls[0].init?.credentials).toBe('same-origin');
    expect(JSON.parse(String(f.calls[0].init?.body))).toEqual({ code: 'K7MQ2X' });
  });

  it('reports a wrong code with the attempts left', async () => {
    const f = fakeFetch(() => Response.json({ error: 'wrongCode', remaining: 3 }, { status: 403 }));
    expect(await join('AAAAAA', f.doFetch)).toEqual({ ok: false, reason: 'wrongCode', remaining: 3 });
    // The attempts left never go below zero, and a missing number counts as none.
    const spent = fakeFetch(() => Response.json({ error: 'wrongCode', remaining: -1 }, { status: 403 }));
    expect(await join('AAAAAA', spent.doFetch)).toEqual({ ok: false, reason: 'wrongCode', remaining: 0 });
    const bare = fakeFetch(() => Response.json({ error: 'wrongCode' }, { status: 403 }));
    expect(await join('AAAAAA', bare.doFetch)).toEqual({ ok: false, reason: 'wrongCode', remaining: 0 });
  });

  it('does not take any other 403 for a wrong code', async () => {
    // A refused Origin does not spend an attempt: "wrong code, 0 attempts left" would be false.
    const origin = fakeFetch(() => Response.json({ error: 'badOrigin' }, { status: 403 }));
    expect(await join('AAAAAA', origin.doFetch)).toEqual({ ok: false, reason: 'error', status: 403 });
    const proxy = fakeFetch(() => new Response('<html>', { status: 403 }));
    expect(await join('AAAAAA', proxy.doFetch)).toEqual({ ok: false, reason: 'error', status: 403 });
  });

  it('reports too many attempts with the wait in seconds', async () => {
    const body = fakeFetch(() => Response.json({ error: 'tooManyAttempts', retryAfter: 42 }, { status: 429 }));
    expect(await join('AAAAAA', body.doFetch)).toEqual({ ok: false, reason: 'tooManyAttempts', retryAfter: 42 });
    const header = fakeFetch(() => new Response('', { status: 429, headers: { 'Retry-After': '17' } }));
    expect(await join('AAAAAA', header.doFetch)).toEqual({ ok: false, reason: 'tooManyAttempts', retryAfter: 17 });
    const nothing = fakeFetch(() => new Response('', { status: 429 }));
    expect(await join('AAAAAA', nothing.doFetch)).toEqual({ ok: false, reason: 'tooManyAttempts', retryAfter: 1 });
  });

  it('reports any other answer, and a network failure', async () => {
    const f = fakeFetch(() => new Response('', { status: 413 }));
    expect(await join('AAAAAA', f.doFetch)).toEqual({ ok: false, reason: 'error', status: 413 });
    const down = fakeFetch(() => Promise.reject(new TypeError('Failed to fetch')));
    expect(await join('AAAAAA', down.doFetch)).toEqual({ ok: false, reason: 'network' });
  });
});
