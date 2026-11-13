// @vitest-environment jsdom
// H8: the words and the flow of the « pages first » refusal of live mode (docs/design/blackboard.md §9.5), without the
// view: the sentence for a count in both languages, `errorMessage` never showing `{count}`, and the controller's steps.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, errorMessage } from './api';
import { catalogs, setLocale } from './i18n.svelte';
import { placeholders } from './i18n-core';
import { isPagesFirst, pagesFirstText, pagesInMessage } from './live-pages-first';
import { PagesFirst } from './live-pages-first.svelte';
import type { DeviceDetails, LastSync, TabletStatus } from './tablet';
import { tablet } from './tablet-state.svelte';

const MESSAGE = (pages: number) => `the Xiaomi tablet holds ${pages} stored pages: import them before live mode`;
const refusal = (pages: number) => ({ code: 'blackboardPagesFirst', message: MESSAGE(pages) });

const board = (storedPages: number): DeviceDetails => ({
  kind: 'blackboard',
  name: 'Mijia LCD blackboard',
  firmware: '',
  batteryPercent: 90,
  charging: false,
  width: 20500,
  height: 27200,
  storedPages,
  model: 'Xiaomi Mijia LCD blackboard',
  readAt: 0,
  memory: null,
});

function status(change: Partial<TabletStatus> = {}): TabletStatus {
  return {
    connection: 'connected',
    registered: null,
    device: board(3),
    registerStep: null,
    sync: null,
    lastSync: null,
    pending: null,
    error: null,
    simulated: true,
    options: { autoSync: false, deleteAfterImport: false, orientation: 'landscape', boardPosition: 'buttonsBottom', button: 'newPage' },
    active: 'blackboard',
    known: [],
    boardSearch: null,
    ...change,
  };
}

const synced = (at: number, error: LastSync['error'] = null): LastSync => ({ at, saved: [], deleted: 3, error });

beforeEach(() => {
  tablet.reset();
  setLocale('fr');
});

afterEach(() => {
  vi.restoreAllMocks();
  tablet.reset();
});

describe('the sentence of the refusal', () => {
  it.each([
    ['fr', 1, 'La tablette Xiaomi contient encore 1 page enregistrée.'],
    ['fr', 3, 'La tablette Xiaomi contient encore 3 pages enregistrées.'],
    ['fr', 12, 'La tablette Xiaomi contient encore 12 pages enregistrées.'],
    ['en', 1, 'The Xiaomi tablet still holds 1 stored page.'],
    ['en', 3, 'The Xiaomi tablet still holds 3 stored pages.'],
  ] as const)('is %s, for %i pages', (locale, count, start) => {
    setLocale(locale);
    const text = pagesFirstText(count);
    expect(text.startsWith(start)).toBe(true);
    expect(text).not.toContain('{');
  });

  it('has the same placeholders in both languages, the singular without any', () => {
    for (const locale of ['fr', 'en'] as const) {
      const errors = catalogs[locale].errorCodes as Record<string, string>;
      expect(placeholders(errors.blackboardPagesFirst), locale).toEqual(['count']);
      expect(placeholders(errors.blackboardPagesFirstOne), locale).toEqual([]);
    }
  });

  it('reads the count of the service message', () => {
    expect(pagesInMessage(MESSAGE(3))).toBe(3);
    expect(pagesInMessage('the Xiaomi tablet holds 1 stored page')).toBe(1);
    expect(pagesInMessage('nothing to count here')).toBeNull();
  });

  it('is the refusal only for its code', () => {
    expect(isPagesFirst(refusal(2))).toBe(true);
    expect(isPagesFirst({ code: 'tabletBusy', message: '' })).toBe(false);
    expect(isPagesFirst(new Error('x'))).toBe(false);
    expect(isPagesFirst(null)).toBe(false);
  });

  it('never reaches the screen with its {count}, whoever shows the error', () => {
    expect(errorMessage(refusal(3))).toBe(pagesFirstText(3));
    expect(errorMessage(refusal(1))).toBe(pagesFirstText(1));
    setLocale('en');
    expect(errorMessage(refusal(5))).toContain('still holds 5 stored pages');
    // A message without a count (an older service): the service's own words, not a text with a hole.
    expect(errorMessage({ code: 'blackboardPagesFirst', message: 'import the pages first' })).toBe('import the pages first');
  });
});

describe('the controller', () => {
  function make() {
    const again = vi.fn();
    const leave = vi.fn();
    return { flow: new PagesFirst(again, leave), again, leave };
  }

  it('takes only its own refusal, with the count of the message, else the one the tablet read', () => {
    const { flow } = make();
    expect(flow.refuse({ code: 'tabletBusy', message: '' })).toBe(false);
    expect(flow.open).toBe(false);
    expect(flow.refuse(refusal(4))).toBe(true);
    expect(flow.open).toBe(true);
    expect(flow.pages).toBe(4);
    flow.cancel();
    tablet.status = status({ device: board(6) });
    expect(flow.refuse({ code: 'blackboardPagesFirst', message: '' })).toBe(true);
    expect(flow.pages).toBe(6);
  });

  it('never shows a count below one', () => {
    const { flow } = make();
    flow.refuse({ code: 'blackboardPagesFirst', message: '' });
    expect(flow.pages).toBe(1);
  });

  it('asks live mode again when the sync is over and nothing is left, and shows itself again if it is refused', () => {
    const { flow, again } = make();
    tablet.status = status({ lastSync: synced(1) });
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    return flow.importPages().then(() => {
      expect(flow.phase).toBe('importing');
      flow.follow(status({ connection: 'syncing' }));
      expect(flow.phase).toBe('importing');
      flow.follow(status({ lastSync: synced(2), device: board(0) }));
      expect(flow.open).toBe(false);
      expect(again).toHaveBeenCalledOnce();
      // Live mode was asked and refused again (a page was stored meanwhile).
      flow.refuse(refusal(1));
      expect(flow.open).toBe(true);
      expect(flow.phase).toBe('asking');
      expect(flow.pages).toBe(1);
      expect(flow.error).toBeNull();
    });
  });

  it('does not take a sync that was already over before the import for its end', async () => {
    const { flow, again } = make();
    tablet.status = status({ lastSync: synced(1) });
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    await flow.importPages();
    flow.follow(status({ lastSync: synced(1), device: board(0) }));
    expect(flow.phase).toBe('importing');
    expect(again).not.toHaveBeenCalled();
  });

  it('asks the import once', async () => {
    const { flow } = make();
    flow.refuse(refusal(3));
    const sync = vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    const first = flow.importPages();
    const second = flow.importPages();
    await Promise.all([first, second]);
    expect(sync).toHaveBeenCalledExactlyOnceWith(true);
  });

  it('keeps the service failure of before the import from ending it', async () => {
    const { flow } = make();
    const stale = status({ error: { code: 'syncRefused', message: 'old' } });
    tablet.status = stale;
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    await flow.importPages();
    flow.follow(stale);
    expect(flow.phase).toBe('importing');
    flow.follow(status({ error: { code: 'pageUnreadable', message: 'new' } }));
    expect(flow.phase).toBe('asking');
    expect(flow.error).toContain('ne sait pas lire');
  });

  it('shows the error of the sync and goes back to the question', async () => {
    const { flow, again } = make();
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    await flow.importPages();
    flow.follow(status({ lastSync: synced(2, { code: 'pageNotDeleted', message: '' }), device: board(1) }));
    expect(flow.open).toBe(true);
    expect(flow.phase).toBe('asking');
    expect(flow.error).toContain('garde une page');
    expect(again).not.toHaveBeenCalled();
  });

  it('gives up on a lost link', async () => {
    const { flow } = make();
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    await flow.importPages();
    flow.follow(status({ connection: 'searching', active: null }));
    expect(flow.error).toBe("La tablette n'est plus connectée : l'import est interrompu. Les pages qui n'ont pas été importées restent sur la tablette.");
    expect(flow.phase).toBe('asking');
  });

  it('shows the dialog again with the pages that are left', async () => {
    const { flow, again } = make();
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    await flow.importPages();
    flow.follow(status({ lastSync: synced(2), device: board(2) }));
    expect(flow.open).toBe(true);
    expect(flow.phase).toBe('asking');
    expect(flow.pages).toBe(2);
    expect(again).not.toHaveBeenCalled();
  });

  it('leaves once the sync it asked to stop has ended', async () => {
    const { flow, leave } = make();
    flow.refuse(refusal(3));
    vi.spyOn(api, 'tabletSync').mockResolvedValue(undefined);
    const cancel = vi.spyOn(api, 'tabletCancel').mockResolvedValue(undefined);
    await flow.importPages();
    flow.cancel();
    expect(cancel).toHaveBeenCalledOnce();
    expect(leave).not.toHaveBeenCalled();
    flow.follow(status({ lastSync: synced(2), device: board(1) }));
    expect(flow.open).toBe(false);
    expect(leave).toHaveBeenCalledOnce();
  });

  it('leaves at once when cancelled before the import', () => {
    const { flow, leave, again } = make();
    flow.refuse(refusal(3));
    flow.cancel();
    expect(flow.open).toBe(false);
    expect(leave).toHaveBeenCalledOnce();
    expect(again).not.toHaveBeenCalled();
    // And nothing follows a closed dialog.
    flow.follow(status({ lastSync: synced(2), device: board(0) }));
    expect(again).not.toHaveBeenCalled();
  });
});
