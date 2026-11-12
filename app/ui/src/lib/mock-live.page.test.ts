// Paper and image of the current page (M17): the browser stand-in follows the Rust side
// (docs/design/paper-projection-narration.md, §1.2 to §1.4): the two commands and their refusals, the page's own paper under
// the blackboard, a new page that keeps the paper and not the image, a page with an image that is written and held.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BackgroundImage, Background } from './note';
import type { LiveBatch, LivePageImageOp, LiveSnapshot, LiveStopped } from './live';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, mockLiveTick, resetMockLive, setMockLiveOrientation } from './mock-live';

async function subscribe() {
  const batches: LiveBatch[] = [];
  await mockLiveSubscribe((b) => batches.push(b));
  const snapshots = () => batches.flatMap((b) => b.events).flatMap((e) => (e.type === 'snapshot' ? [e.snapshot] : []));
  return { batches, snapshots, last: () => snapshots().at(-1)! };
}

const invoke = (command: string, args: Record<string, unknown> = {}) => mockLiveInvoke(command, args);
const start = () => invoke('live_start') as LiveSnapshot;
const stop = () => invoke('live_stop') as LiveStopped;
const paper = (background: Background, pageIndex = 0) => invoke('live_page_paper', { pageIndex, background });
const image = (op: LivePageImageOp, pageIndex = 0) => invoke('live_page_image', { pageIndex, op });
const command = (kind: string) => invoke('live_command', { command: { kind } });
const view = (board: boolean) => invoke('live_set_view', { view: { zoom: 1, x: 14850, y: 10800, board, laser: false } });
const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];

const SEYES: Background = { paper: 'cream', template: 'seyes', texture: true };
const PDF: BackgroundImage = { name: 'fiche.pdf', pdfPage: 1, rect: { x: 5000, y: 0, width: 15270, height: 21600 }, quarterTurns: 0, opacity: 1 };
const PHOTO: BackgroundImage = { name: 'photo.jpg', rect: { x: 0, y: 0, width: 29700, height: 21600 }, quarterTurns: 0, opacity: 1 };
/** What `stage_background_image` makes a key look like. */
const KEY = 'mock-7';

function refusal(run: () => unknown): { code: string; message: string } {
  try {
    run();
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error('nothing was refused');
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(Date.UTC(2026, 10, 11, 9, 0));
  resetMockLive();
  resetMockLibrary();
  mockLiveConfig.waitMs = 0;
  mockLiveConfig.pen = false;
});

afterEach(() => {
  resetMockLive();
  vi.useRealTimers();
});

describe('browser stand-in: the paper of the current page', () => {
  it('starts white, with its own paper and no image in the snapshot', () => {
    const first = start();
    expect(first.page.background).toEqual({ paper: 'white', template: 'blank', texture: false });
    expect(first.pagePaper).toBe('white');
    expect(first.imagePixels).toBeNull();
  });

  it('sets the colour, the template and the texture of the page on screen, with one snapshot', async () => {
    const sub = await subscribe();
    start();
    const before = sub.snapshots().length;
    expect(paper(SEYES)).toBeUndefined();
    expect(sub.snapshots()).toHaveLength(before + 1);
    expect(sub.last().page.background).toEqual(SEYES);
    expect(sub.last().pagePaper).toBe('cream');
    // The same value again: nothing is told.
    paper(SEYES);
    expect(sub.snapshots()).toHaveLength(before + 1);
  });

  it('refuses a page that is not the one on screen, and a session that does not run', () => {
    expect(refusal(() => paper(SEYES)).code).toBe('liveNotRunning');
    start();
    expect(refusal(() => paper(SEYES, 3)).code).toBe('liveEditStale');
    expect(refusal(() => image({ kind: 'remove' }, 1)).code).toBe('liveEditStale');
    stop();
    expect(refusal(() => paper(SEYES)).code).toBe('liveNotRunning');
  });

  it('keeps the page slate under the blackboard: only the template and the texture show, the colour waits', async () => {
    const sub = await subscribe();
    start();
    paper({ paper: 'cream', template: 'lined', texture: false });
    view(true);
    expect(sub.last().page.background.paper).toBe('slate');
    expect(sub.last().pagePaper).toBe('cream');
    paper({ paper: 'kraft', template: 'grid5mm', texture: true });
    expect(sub.last().page.background).toEqual({ paper: 'slate', template: 'grid5mm', texture: true });
    expect(sub.last().pagePaper).toBe('kraft');
    // Board off: the paper chosen meanwhile comes back.
    view(false);
    expect(sub.last().page.background).toEqual({ paper: 'kraft', template: 'grid5mm', texture: true });
    expect(sub.last().pagePaper).toBe('kraft');
  });

  it('chooses slate as a paper without the board: the page is slate, and it is its own paper', async () => {
    const sub = await subscribe();
    start();
    paper({ paper: 'slate', template: 'blank', texture: false });
    expect(sub.last().page.background.paper).toBe('slate');
    expect(sub.last().pagePaper).toBe('slate');
    view(true);
    view(false);
    expect(sub.last().page.background.paper).toBe('slate');
  });

  it('gives a new page the paper, the template and the texture of the page it follows, and no image', async () => {
    const sub = await subscribe();
    start();
    paper(SEYES);
    image({ kind: 'set', key: KEY, image: PDF });
    command('nextPage');
    expect(sub.last().pageIndex).toBe(1);
    expect(sub.last().page.background).toEqual(SEYES);
    expect(sub.last().pagePaper).toBe('cream');
    expect(sub.last().page.backgroundImage).toBeUndefined();
    expect(sub.last().imagePixels).toBeNull();
    // Back: the first page kept its own, image included.
    command('previousPage');
    expect(sub.last().page.backgroundImage?.name).toBe('fiche.pdf');
    expect(sub.last().imagePixels?.key).toBe(KEY);
    // « New page » from the first page takes that page's paper, not the last one's.
    paper({ paper: 'kraft', template: 'dotted', texture: false });
    command('newPage');
    expect(sub.last().pageIndex).toBe(2);
    expect(sub.last().page.background).toEqual({ paper: 'kraft', template: 'dotted', texture: false });
  });

  it('makes a page under the blackboard slate, with the paper of the page left as its own', async () => {
    const sub = await subscribe();
    start();
    paper(SEYES);
    view(true);
    command('nextPage');
    expect(sub.last().page.background).toEqual({ paper: 'slate', template: 'seyes', texture: true });
    expect(sub.last().pagePaper).toBe('cream');
    view(false);
    expect(sub.last().page.background.paper).toBe('cream');
  });

  it('starts every session white, whatever the last one had', async () => {
    start();
    paper(SEYES);
    stop();
    expect(start().page.background).toEqual({ paper: 'white', template: 'blank', texture: false });
  });
});

describe('browser stand-in: the image of the current page', () => {
  it('sets an image staged before, with the size of its pixels', async () => {
    const sub = await subscribe();
    start();
    image({ kind: 'set', key: KEY, image: PDF });
    expect(sub.last().page.backgroundImage).toEqual(PDF);
    expect(sub.last().imagePixels).toEqual({ key: KEY, width: 2480, height: 3508 });
    image({ kind: 'set', key: 'mock-8', image: PHOTO });
    expect(sub.last().imagePixels).toEqual({ key: 'mock-8', width: 1600, height: 1200 });
  });

  it('places the same pixels otherwise, clamps the opacity and takes the turns modulo 4', async () => {
    const sub = await subscribe();
    start();
    image({ kind: 'set', key: KEY, image: PDF });
    image({ kind: 'place', image: { ...PDF, opacity: 3, quarterTurns: 5 } });
    expect(sub.last().page.backgroundImage).toMatchObject({ opacity: 1, quarterTurns: 1 });
    image({ kind: 'place', image: { ...PDF, opacity: -2 } });
    expect(sub.last().page.backgroundImage?.opacity).toBe(0);
    const count = sub.snapshots().length;
    image({ kind: 'place', image: { ...PDF, opacity: 0 } });
    expect(sub.snapshots()).toHaveLength(count);
    expect(sub.last().imagePixels?.key).toBe(KEY);
  });

  it('removes it, and says nothing when there is none', async () => {
    const sub = await subscribe();
    start();
    const count = sub.snapshots().length;
    image({ kind: 'remove' });
    expect(sub.snapshots()).toHaveLength(count);
    image({ kind: 'set', key: KEY, image: PDF });
    image({ kind: 'remove' });
    expect(sub.last().page.backgroundImage).toBeUndefined();
    expect(sub.last().imagePixels).toBeNull();
  });

  it('refuses an image that is not staged, and the descriptions Rust refuses', () => {
    start();
    expect(refusal(() => image({ kind: 'set', key: 'nowhere', image: PDF })).code).toBe('backgroundMissing');
    const invalid = (change: Partial<BackgroundImage>) => refusal(() => image({ kind: 'set', key: KEY, image: { ...PDF, ...change } })).code;
    expect(invalid({ name: '' })).toBe('liveEditInvalid');
    expect(invalid({ name: 'x'.repeat(256) })).toBe('liveEditInvalid');
    expect(invalid({ pdfPage: 0 })).toBe('liveEditInvalid');
    expect(invalid({ rect: { x: 0, y: 0, width: 0, height: 10 } })).toBe('liveEditInvalid');
    expect(invalid({ rect: { x: Number.NaN, y: 0, width: 10, height: 10 } })).toBe('liveEditInvalid');
    expect(invalid({ rect: { x: 29700 * 17, y: 0, width: 10, height: 10 } })).toBe('liveEditInvalid');
    expect(invalid({ opacity: Number.NaN })).toBe('liveEditInvalid');
    // `place` needs an image, and the page's own.
    expect(refusal(() => image({ kind: 'place', image: PDF })).code).toBe('liveEditInvalid');
    image({ kind: 'set', key: KEY, image: PDF });
    expect(refusal(() => image({ kind: 'place', image: { ...PDF, name: 'autre.pdf' } })).code).toBe('liveEditInvalid');
    expect(refusal(() => image({ kind: 'place', image: { ...PDF, pdfPage: 2 } })).code).toBe('liveEditInvalid');
  });

  it('draws the image in the paper the session renders', () => {
    start();
    image({ kind: 'set', key: KEY, image: PDF });
    const url = String(invoke('live_page_background', { pageIndex: 0, page: { width: 29700, height: 21600 }, background: SEYES, widthPx: 256 }));
    expect(decodeURIComponent(url)).toContain('fiche.pdf');
  });
});

describe('browser stand-in: writing the page', () => {
  it('writes a page with an image even without ink, and no page with paper alone', () => {
    start();
    paper(SEYES);
    command('nextPage');
    expect(stop().saved).toEqual([]);
    start();
    image({ kind: 'set', key: KEY, image: PDF });
    const answer = stop();
    expect(answer.saved).toHaveLength(1);
    const written = mockLibrary().notes.get(answer.saved[0])!.note;
    expect(written.backgroundImage?.name).toBe('fiche.pdf');
    expect(written.strokes).toHaveLength(0);
  });

  it('writes a resumed sheet whose paper was changed, with that paper and not the one read', () => {
    const path = sheetPath();
    const before = mockLibrary().notes.get(path)!.note.background;
    invoke('live_resume', { path });
    paper({ paper: 'kraft', template: 'millimetric', texture: true });
    stop();
    const after = mockLibrary().notes.get(path)!.note;
    expect(after.background).toEqual({ paper: 'kraft', template: 'millimetric', texture: true });
    expect(after.background).not.toEqual(before);
  });

  it('writes the own paper of a resumed sheet changed under the blackboard, not the slate', () => {
    const path = sheetPath();
    invoke('live_resume', { path });
    view(true);
    paper({ paper: 'cream', template: 'seyes', texture: false });
    stop();
    expect(mockLibrary().notes.get(path)!.note.background).toEqual({ paper: 'cream', template: 'seyes', texture: false });
  });

  it('leaves a resumed sheet alone when nothing was changed on purpose', () => {
    const path = sheetPath();
    const stamp = mockLibrary().notes.get(path)!.stamp;
    invoke('live_resume', { path });
    stop();
    expect(mockLibrary().notes.get(path)!.stamp).toEqual(stamp);
  });

  it('writes a resumed sheet whose image was removed, without it', () => {
    const path = sheetPath();
    mockLibrary().notes.get(path)!.note.backgroundImage = { ...PHOTO };
    invoke('live_resume', { path });
    expect((invoke('live_resume', { path }) as LiveSnapshot).imagePixels?.width).toBe(1600);
    image({ kind: 'remove' });
    stop();
    expect(mockLibrary().notes.get(path)!.note.backgroundImage).toBeUndefined();
  });
});

describe('browser stand-in: the tablet and a page with an image', () => {
  it('turns a page with an image and no ink with the tablet, the image placed again with its fit', async () => {
    const sub = await subscribe();
    start();
    image({ kind: 'set', key: 'mock-9', image: { ...PHOTO, rect: { x: 0, y: 0, width: 28800, height: 21600 } } });
    expect(sub.last().canTurnPage).toBe(true);
    setMockLiveOrientation('portraitLeft');
    const turned = sub.last();
    expect(turned.page.page).toEqual({ width: 21600, height: 29700 });
    expect(turned.canTurnPage).toBe(true);
    // Contained on the new page, upright.
    const placed = turned.page.backgroundImage!;
    expect(placed.quarterTurns).toBe(0);
    expect(placed.rect.width).toBeCloseTo(21600, 0);
    expect(placed.rect.height).toBeCloseTo(21600 * (1200 / 1600), 0);
    expect(placed.rect.y).toBeGreaterThan(0);
  });

  it('holds a page with an image and ink like a resumed sheet: the tablet follows it', async () => {
    const sub = await subscribe();
    start();
    vi.advanceTimersByTime(1);
    image({ kind: 'set', key: KEY, image: PDF });
    // The simulated pen writes a stroke on the page (stepped by hand), and lifts.
    mockLiveTick(80);
    command('nextPage');
    command('previousPage');
    expect(sub.last().page.strokes.length).toBeGreaterThan(0);
    expect(sub.last().canTurnPage).toBe(false);
    const size = sub.last().page.page;
    setMockLiveOrientation('portraitLeft');
    expect(sub.last().page.page).toEqual(size);
    // The tablet is to be held for the page (landscape), not the page turned for the tablet.
    expect(sub.last().orientation).toBe('landscape');
    // The image removed: the page follows the tablet at once, ink and all, and the view is fitted to it (as Rust).
    const strokes = sub.last().page.strokes.length;
    image({ kind: 'remove' });
    const turned = sub.last();
    expect(turned.canTurnPage).toBe(true);
    expect(turned.page.page).toEqual({ width: 21600, height: 29700 });
    expect(turned.page.strokes).toHaveLength(strokes);
    expect(turned.orientation).toBe('portraitLeft');
    expect(turned.view).toMatchObject({ x: 10800, y: 14850 });
  });

  it('keeps the quarter turns the user gave the image when the tablet turns (as Rust)', async () => {
    const sub = await subscribe();
    start();
    // A sideways scan (1600 × 1200 px) turned once in the Fond tab: shown 1200 × 1600, contained on the landscape page.
    image({ kind: 'set', key: 'mock-9', image: { ...PHOTO, rect: { x: 6750, y: 0, width: 16200, height: 21600 }, quarterTurns: 1 } });
    setMockLiveOrientation('portraitLeft');
    const placed = sub.last().page.backgroundImage!;
    expect(placed.quarterTurns).toBe(1);
    expect(placed.rect.width).toBeCloseTo(21600, 0);
    expect(placed.rect.height).toBeCloseTo(28800, 0);
  });
});
