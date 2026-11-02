/**
 * Browser stand-in for the LAN share commands (`share_*`, milestone M11; Rust side `share.rs`,
 * docs/design/lan-share.md). Starting gives an active share with the code `K7M Q2X`, one
 * address and a small fixed QR code; a screen joins a second later (`mockShare.viewerJoins`,
 * off under vitest); pause, stop and "show" behave as the contract says. `share_show` is
 * ignored while no share runs, but every call is logged in `mockShareShown` for the tests.
 */
import { UNHANDLED } from './mock-data';
import { INACTIVE_SHARE, type QrCode, type ShareContent, type ShareStatus } from './share';

/** Knobs of the stand-in; tests change them (`resetMockShare` puts them back). */
export const mockShare = {
  /** A screen joins by itself `viewerAfterMs` after the start. Off under vitest, where it would make tests move on their own. */
  viewerJoins: !import.meta.env.VITEST,
  viewerAfterMs: 1000,
  /** Error the next `share_start` fails with, instead of starting. */
  failStart: null as { code: string; message: string } | null,
  /** Addresses reported by a started share (private LAN addresses first). */
  addresses: ['http://192.168.1.23:8765/'],
  /** Pause before `share_start` answers, in milliseconds (to see the spinner). */
  startDelayMs: 0,
};

export const MOCK_SHARE_CODE = 'K7M Q2X';

/** Every `share_show` content received since the last reset, in order (also while inactive). */
export const mockShareShown: ShareContent[] = [];

/** A 21 × 21 pattern: the three finder squares and a few modules (not a readable code). */
function fixedQr(): QrCode {
  const size = 21;
  const dark = new Set<string>();
  const finder = (x0: number, y0: number) => {
    for (let y = 0; y < 7; y++) {
      for (let x = 0; x < 7; x++) {
        const ring = x === 0 || x === 6 || y === 0 || y === 6;
        const core = x >= 2 && x <= 4 && y >= 2 && y <= 4;
        if (ring || core) dark.add(`${x0 + x},${y0 + y}`);
      }
    }
  };
  finder(0, 0);
  finder(size - 7, 0);
  finder(0, size - 7);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const inFinder = (x < 8 && y < 8) || (x >= size - 8 && y < 8) || (x < 8 && y >= size - 8);
      if (!inFinder && (x * 7 + y * 13 + x * y) % 3 === 0) dark.add(`${x},${y}`);
    }
  }
  const path = [...dark].map((key) => `M${key.replace(',', ' ')}h1v1h-1z`).join('');
  return { size, path };
}

interface Sim {
  status: ShareStatus;
  listeners: Set<(status: ShareStatus) => void>;
  timer: ReturnType<typeof setTimeout> | null;
}

let sim: Sim | null = null;

function getSim(): Sim {
  sim ??= { status: { ...INACTIVE_SHARE }, listeners: new Set(), timer: null };
  return sim;
}

function change(s: Sim, patch: Partial<ShareStatus>): ShareStatus {
  s.status = { ...s.status, ...patch };
  for (const listener of [...s.listeners]) listener(structuredClone(s.status));
  return structuredClone(s.status);
}

/** Forgets the share, its timer, its subscribers and the log; puts the knobs back (tests). */
export function resetMockShare(): void {
  if (sim?.timer) clearTimeout(sim.timer);
  sim = null;
  mockShareShown.length = 0;
  mockShare.viewerJoins = !import.meta.env.VITEST;
  mockShare.viewerAfterMs = 1000;
  mockShare.addresses = ['http://192.168.1.23:8765/'];
  mockShare.failStart = null;
  mockShare.startDelayMs = 0;
}

/** Sets the number of screens connected (tests, and the browser stand-in's demo). */
export function mockShareSetViewers(viewers: number): void {
  const s = getSim();
  if (s.status.active) change(s, { viewers });
}

export async function mockShareSubscribe(onStatus: (status: ShareStatus) => void): Promise<ShareStatus> {
  const s = getSim();
  s.listeners.add(onStatus);
  return structuredClone(s.status);
}

function shownBy(content: ShareContent): ShareStatus['shown'] {
  return content.kind;
}

export function mockShareInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'share_status':
      return structuredClone(getSim().status);

    case 'share_start': {
      const s = getSim();
      const failure = mockShare.failStart;
      const begin = (): ShareStatus => {
        if (failure) throw { ...failure };
        if (s.status.active) return structuredClone(s.status);
        const last = mockShareShown[mockShareShown.length - 1];
        const started = change(s, {
          active: true,
          code: MOCK_SHARE_CODE,
          addresses: [...mockShare.addresses],
          qr: fixedQr(),
          viewers: 0,
          paused: false,
          shown: last ? shownBy(last) : 'waiting',
        });
        if (mockShare.viewerJoins) {
          s.timer = setTimeout(() => {
            s.timer = null;
            if (s.status.active) change(s, { viewers: 1 });
          }, mockShare.viewerAfterMs);
        }
        return started;
      };
      if (mockShare.startDelayMs <= 0) return begin();
      return new Promise<ShareStatus>((resolve, reject) => {
        setTimeout(() => {
          try {
            resolve(begin());
          } catch (error) {
            reject(error);
          }
        }, mockShare.startDelayMs);
      });
    }

    case 'share_stop': {
      const s = getSim();
      if (s.timer) clearTimeout(s.timer);
      s.timer = null;
      return change(s, { ...INACTIVE_SHARE });
    }

    case 'share_set_paused': {
      const s = getSim();
      return change(s, { paused: s.status.active && args.paused === true });
    }

    case 'share_show': {
      const content = args.content as ShareContent;
      mockShareShown.push(content);
      const s = getSim();
      if (s.status.active && !s.status.paused && s.status.shown !== shownBy(content)) change(s, { shown: shownBy(content) });
      return undefined;
    }

    default:
      return UNHANDLED;
  }
}
