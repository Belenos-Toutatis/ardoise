// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { setLocale } from './i18n.svelte';
import { line, note } from './editor/test-fixtures';
import { mockShare, mockShareSetViewers, mockShareShown, resetMockShare } from './mock-share';
import { NOTE_DEBOUNCE_MS, NOTE_INTERVAL_MS, ShareState } from './share-state.svelte';

let share: ShareState;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockShare();
  setLocale('fr');
  share = new ShareState();
});

afterEach(() => {
  share.reset();
  resetMockShare();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const settle = () => vi.advanceTimersByTimeAsync(0);
const wait = (ms: number) => vi.advanceTimersByTimeAsync(ms);
const kinds = () => mockShareShown.map((content) => content.kind);
const edited = (n: number) => note([line([[100, 100], [200 + n, 200]])]);

/** Follows the (mock) service and starts the share. */
async function started() {
  share.follow();
  await settle();
  expect(await share.start()).toBe(true);
  await settle();
}

describe('what is shown, while no share runs', () => {
  it('sends nothing and remembers the last content', async () => {
    share.follow();
    await settle();
    share.showLive();
    share.showNote(edited(1), 'img');
    share.showWaiting();
    share.showNote(edited(2), null);
    await wait(2000);
    expect(mockShareShown).toEqual([]);
    expect(share.active).toBe(false);
  });

  it('gives the last content right after the start', async () => {
    share.follow();
    await settle();
    share.showLive();
    const open = edited(3);
    share.showNote(open, 'img');
    await started();
    expect(mockShareShown).toEqual([{ kind: 'note', note: open, imageKey: 'img' }]);
    expect(share.status.shown).toBe('note');
  });

  it('gives waiting after the start when nothing else was said', async () => {
    await started();
    expect(mockShareShown).toEqual([{ kind: 'waiting' }]);
  });
});

describe('what is shown, while a share runs', () => {
  it('sends live and waiting at once, each time', async () => {
    await started();
    mockShareShown.length = 0;
    share.showLive();
    await settle();
    expect(kinds()).toEqual(['live']);
    share.showWaiting();
    await settle();
    expect(kinds()).toEqual(['live', 'waiting']);
    expect(share.status.shown).toBe('waiting');
  });

  it('sends a note 100 ms after its last change, once', async () => {
    await started();
    mockShareShown.length = 0;
    share.showNote(edited(1), null);
    await wait(NOTE_DEBOUNCE_MS - 10);
    share.showNote(edited(2), null);
    await wait(NOTE_DEBOUNCE_MS - 10);
    expect(mockShareShown).toEqual([]);
    const last = edited(3);
    share.showNote(last, 'k');
    await wait(NOTE_DEBOUNCE_MS - 1);
    expect(mockShareShown).toEqual([]);
    await wait(1);
    expect(mockShareShown).toEqual([{ kind: 'note', note: last, imageKey: 'k' }]);
    await wait(1000);
    expect(mockShareShown).toHaveLength(1);
  });

  it('sends at most every 300 ms while changes keep coming, and never stays silent longer', async () => {
    await started();
    mockShareShown.length = 0;
    const sent: number[] = [];
    const real = api.shareShow;
    vi.spyOn(api, 'shareShow').mockImplementation((content) => {
      sent.push(Date.now());
      return real(content);
    });
    const begin = Date.now();
    for (let at = 0; at < 1500; at += 20) {
      share.showNote(edited(at), null);
      await wait(20);
    }
    await wait(NOTE_DEBOUNCE_MS);
    expect(sent.length).toBeGreaterThanOrEqual(4);
    expect(sent[0] - begin).toBeLessThanOrEqual(NOTE_INTERVAL_MS);
    for (let i = 1; i < sent.length; i++) {
      expect(sent[i] - sent[i - 1]).toBeGreaterThanOrEqual(NOTE_INTERVAL_MS);
      expect(sent[i] - sent[i - 1]).toBeLessThanOrEqual(NOTE_INTERVAL_MS + 20);
    }
    // The last state of the note is what ends up shown.
    const final = mockShareShown[mockShareShown.length - 1];
    expect(final).toMatchObject({ kind: 'note' });
  });

  it('waits for 300 ms after the previous note before sending the next', async () => {
    await started();
    mockShareShown.length = 0;
    share.showNote(edited(1), null);
    await wait(NOTE_DEBOUNCE_MS);
    expect(mockShareShown).toHaveLength(1);
    await wait(50);
    share.showNote(edited(2), null);
    await wait(NOTE_DEBOUNCE_MS + 50);
    expect(mockShareShown).toHaveLength(1);
    await wait(NOTE_INTERVAL_MS);
    expect(mockShareShown).toHaveLength(2);
  });

  it('drops a note still waiting when another content comes', async () => {
    await started();
    mockShareShown.length = 0;
    share.showNote(edited(1), null);
    share.showWaiting();
    await wait(1000);
    expect(kinds()).toEqual(['waiting']);
  });

  it('sends the content when the service turns active without the window asking', async () => {
    share.follow();
    await settle();
    share.showLive();
    // Another window (or a reload) started the share: the status event carries it.
    await api.shareStart();
    await settle();
    expect(kinds()).toEqual(['live']);
  });

  it('sends nothing any more once the share is stopped, and forgets pending notes', async () => {
    await started();
    mockShareShown.length = 0;
    share.showNote(edited(1), null);
    expect(await share.stop()).toBe(true);
    await wait(1000);
    expect(mockShareShown).toEqual([]);
    share.showLive();
    await wait(1000);
    expect(mockShareShown).toEqual([]);
    expect(share.active).toBe(false);
  });
});

describe('the share', () => {
  it('follows the status: screens joining, pause, stop', async () => {
    await started();
    expect(share.status.code).toBe('K7M Q2X');
    expect(share.status.viewers).toBe(0);
    mockShareSetViewers(2);
    expect(share.status.viewers).toBe(2);
    expect(await share.setPaused(true)).toBe(true);
    expect(share.status.paused).toBe(true);
    expect(await share.setPaused(false)).toBe(true);
    expect(share.status.paused).toBe(false);
    share.panelOpen = true;
    expect(await share.stop()).toBe(true);
    expect(share.status.active).toBe(false);
    expect(share.panelOpen).toBe(false);
  });

  it('has a screen join a second after the start when the stand-in is told to', async () => {
    mockShare.viewerJoins = true;
    await started();
    expect(share.status.viewers).toBe(0);
    await wait(mockShare.viewerAfterMs);
    expect(share.status.viewers).toBe(1);
  });

  it('opens the panel and starts the share, once', async () => {
    share.openPanel();
    share.openPanel();
    expect(share.panelOpen).toBe(true);
    expect(share.starting).toBe(true);
    await settle();
    expect(share.starting).toBe(false);
    expect(share.active).toBe(true);
    share.closePanel();
    expect(share.panelOpen).toBe(false);
    // Closing the panel does not stop the share; opening it again does not start another.
    expect(share.active).toBe(true);
    share.openPanel();
    await settle();
    expect(share.active).toBe(true);
  });

  it('keeps the failure of the start in words, and clears it at the next try', async () => {
    mockShare.failStart = { code: 'sharePortInUse', message: '8765' };
    expect(await share.start()).toBe(false);
    expect(share.error).toContain('port');
    expect(share.error).toContain('Port du partage');
    expect(share.active).toBe(false);
    expect(share.starting).toBe(false);
    mockShare.failStart = null;
    expect(await share.start()).toBe(true);
    expect(share.error).toBeNull();
  });

  it('says each failure with its own sentence', async () => {
    for (const code of ['sharePortInUse', 'shareUnavailable', 'shareFailed']) {
      mockShare.failStart = { code, message: 'détail technique' };
      await share.start();
      expect(share.error, code).not.toBeNull();
      expect(share.error, code).not.toContain('errorCodes');
      if (code === 'shareFailed') expect(share.error).toContain('détail technique');
    }
  });

  it('is not started twice while the first start is pending', async () => {
    mockShare.startDelayMs = 50;
    const first = share.start();
    expect(await share.start()).toBe(false);
    await wait(50);
    expect(await first).toBe(true);
  });
});
