import { describe, expect, it, vi } from 'vitest';
import { PAPER_FETCH_TIMEOUT_MS, PAPER_RETRY_DELAYS_MS, PaperLoader, type LoadedPaper, type PaperEnv } from './viewer-paper';

const PNG = () => new Response('png', { status: 200, headers: { 'Content-Type': 'image/png' } });

/** A fetch that the test answers. */
interface Pending {
  url: string;
  init: RequestInit;
  answer(response: Response): void;
  fail(): void;
  aborted(): boolean;
}

/** The browser as the loader sees it: fetches and timers in the test's hands. */
class Fake {
  fetches: Pending[] = [];
  urls: string[] = [];
  revoked: string[] = [];
  timers = new Map<number, { handler: () => void; ms: number }>();
  private nextTimer = 1;
  shown: (LoadedPaper | null)[] = [];

  env: PaperEnv = {
    fetch: (url, init) =>
      new Promise<Response>((resolve, reject) => {
        init.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
        this.fetches.push({
          url,
          init,
          answer: resolve,
          fail: () => reject(new TypeError('network')),
          aborted: () => init.signal?.aborted ?? false,
        });
      }),
    setTimeout: (handler, ms) => {
      const id = this.nextTimer++;
      this.timers.set(id, { handler, ms });
      return id;
    },
    clearTimeout: (handle) => {
      this.timers.delete(handle as number);
    },
    createUrl: () => {
      const url = `blob:picture-${this.urls.length + 1}`;
      this.urls.push(url);
      return url;
    },
    revokeUrl: (url) => {
      this.revoked.push(url);
    },
  };

  loader = new PaperLoader((paper) => this.shown.push(paper), this.env);

  last(): Pending {
    return this.fetches[this.fetches.length - 1];
  }

  /** Delays of the retries waiting (the watchdog of a fetch under way is not one). */
  retries(): number[] {
    return [...this.timers.values()].map((timer) => timer.ms).filter((ms) => ms !== PAPER_FETCH_TIMEOUT_MS);
  }

  fire(ms: number): void {
    for (const [id, timer] of this.timers) {
      if (timer.ms === ms) {
        this.timers.delete(id);
        timer.handler();
        return;
      }
    }
    throw new Error(`no timer of ${ms} ms`);
  }
}

/** Lets what the fetch's answer resolves run. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('the picture of the paper', () => {
  it('fetches it apart, with the cookie, and hands over a URL that an image can load', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    expect(f.fetches).toHaveLength(1);
    expect(f.last().url).toBe('/paper/k1');
    expect(f.last().init.credentials).toBe('same-origin');
    expect(f.last().init.cache).toBe('no-store');
    expect(f.shown).toEqual([]);

    f.last().answer(PNG());
    await settle();
    expect(f.shown).toEqual([{ key: 'k1', url: 'blob:picture-1' }]);
    expect(f.timers.size).toBe(0);
  });

  it('asks once for what it has or is fetching: the PC tells of it again and again', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    f.loader.want('k1', 'v1');
    expect(f.fetches).toHaveLength(1);
    f.last().answer(PNG());
    await settle();
    f.loader.want('k1', 'v1');
    expect(f.fetches).toHaveLength(1);
    expect(f.shown).toHaveLength(1);
  });

  it('fetches again when the same key has other pixels, and shows the old ones meanwhile', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    f.last().answer(PNG());
    await settle();
    f.loader.want('k1', 'v2');
    expect(f.fetches).toHaveLength(2);
    expect(f.shown).toHaveLength(1);
    expect(f.revoked).toEqual([]);
    f.last().answer(PNG());
    await settle();
    expect(f.shown.at(-1)).toEqual({ key: 'k1', url: 'blob:picture-2' });
    // The new one is given before the old one's URL goes.
    expect(f.revoked).toEqual(['blob:picture-1']);
  });

  it('drops the fetch of a key that changed meanwhile, whatever it answers later', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    const first = f.last();
    f.loader.want('k2', 'v1');
    expect(first.aborted()).toBe(true);
    expect(f.last().url).toBe('/paper/k2');
    expect(f.last().aborted()).toBe(false);

    // An answer that was already on its way changes nothing.
    first.answer(PNG());
    await settle();
    expect(f.shown).toEqual([]);
    f.last().answer(PNG());
    await settle();
    expect(f.shown).toEqual([{ key: 'k2', url: 'blob:picture-1' }]);
    expect(f.retries()).toEqual([]);
  });

  it('drops the fetch when the picture in hand is wanted again', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    f.last().answer(PNG());
    await settle();
    f.loader.want('k2', 'v1');
    const second = f.last();
    f.loader.want('k1', 'v1');
    expect(second.aborted()).toBe(true);
    expect(f.fetches).toHaveLength(2);
    expect(f.shown).toEqual([{ key: 'k1', url: 'blob:picture-1' }]);
  });

  it('tries again a few times after a failure, then waits to be told', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    const answers = [
      () => f.last().fail(),
      () => f.last().answer(new Response(null, { status: 404 })),
      () => f.last().answer(new Response(null, { status: 503 })),
      // A filtering proxy's page.
      () => f.last().answer(new Response('<html>', { status: 200, headers: { 'Content-Type': 'text/html' } })),
    ];
    for (const [i, delay] of PAPER_RETRY_DELAYS_MS.entries()) {
      answers[i]();
      await settle();
      expect(f.retries()).toEqual([delay]);
      expect(f.fetches).toHaveLength(i + 1);
      f.fire(delay);
      expect(f.fetches).toHaveLength(i + 2);
    }
    f.last().fail();
    await settle();
    expect(f.retries()).toEqual([]);
    expect(f.shown).toEqual([]);
    const count = f.fetches.length;

    // The PC tells of it again (a new subscription): at once, and it arrives.
    f.loader.want('k1', 'v1');
    expect(f.fetches).toHaveLength(count + 1);
    f.last().answer(PNG());
    await settle();
    expect(f.shown).toEqual([{ key: 'k1', url: 'blob:picture-1' }]);
  });

  it('goes on after a retry succeeds, and forgets the retries when the key changes', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    f.last().fail();
    await settle();
    f.fire(PAPER_RETRY_DELAYS_MS[0]);
    f.last().answer(PNG());
    await settle();
    expect(f.shown).toHaveLength(1);
    expect(f.timers.size).toBe(0);

    f.loader.want('k2', 'v1');
    f.last().fail();
    await settle();
    expect(f.retries()).toEqual([PAPER_RETRY_DELAYS_MS[0]]);
    f.loader.want('k3', 'v1');
    expect(f.retries()).toEqual([]);
    expect(f.last().url).toBe('/paper/k3');
  });

  it('leaves a 401 to the stream, which brings the code form', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    f.last().answer(new Response(null, { status: 401 }));
    await settle();
    expect(f.timers.size).toBe(0);
    expect(f.fetches).toHaveLength(1);
  });

  it('gives up on a fetch that never ends, and tries again', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    const hung = f.last();
    f.fire(PAPER_FETCH_TIMEOUT_MS);
    expect(hung.aborted()).toBe(true);
    await settle();
    expect(f.retries()).toEqual([PAPER_RETRY_DELAYS_MS[0]]);
  });

  it('forgets everything when asked: nothing shows, nothing comes later', async () => {
    const f = new Fake();
    f.loader.want('k1', 'v1');
    f.last().answer(PNG());
    await settle();
    f.loader.want('k2', 'v1');
    const under = f.last();
    f.loader.forget();
    expect(under.aborted()).toBe(true);
    expect(f.shown.at(-1)).toBeNull();
    expect(f.revoked).toEqual(['blob:picture-1']);
    under.answer(PNG());
    await settle();
    expect(f.shown.at(-1)).toBeNull();
    expect(f.timers.size).toBe(0);

    // A new share tells again: the same key is fetched, its pixels may be others.
    f.loader.want('k1', 'v1');
    expect(f.last().url).toBe('/paper/k1');
  });

  it('never throws out of a browser that refuses', async () => {
    const f = new Fake();
    const env: PaperEnv = {
      ...f.env,
      fetch: vi.fn(() => {
        throw new TypeError('refused');
      }),
    };
    const loader = new PaperLoader(() => undefined, env);
    loader.want('k1', 'v1');
    await settle();
    expect(f.retries()).toEqual([PAPER_RETRY_DELAYS_MS[0]]);
  });

  it('asks for nothing but the address of the key', () => {
    const f = new Fake();
    f.loader.want('../events?x=1', 'v1');
    expect(f.last().url).toBe('/paper/..%2Fevents%3Fx%3D1');
  });
});
