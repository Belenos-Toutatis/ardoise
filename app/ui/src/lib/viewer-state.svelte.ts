/**
 * What the viewer page shows (docs/design/lan-share.md, "The viewer"): the screen, the pause
 * badge, the paper image, and a `LiveState` fed by the messages of the event stream. Pure
 * logic: the page (`viewer/Viewer.svelte`) only reads it and draws.
 *
 * - `live` with `reset`: the batch starts with a snapshot; the viewer starts again from a fresh
 *   `LiveState` (what a stroke in progress of the old subscription left behind is gone).
 *   Without `reset`: the batch is added to the session.
 * - `note`: the whole page of the note open in the editor becomes a `LiveSnapshot` (state `idle`,
 *   one page, the whole page in view) in a fresh `LiveState`, so that the strokes are always
 *   drawn again (an eraser changes strokes in the middle, which a snapshot comparison would not
 *   see). `noteStrokes` add finished strokes at the end.
 * - `livePaper`: the picture of the paper of a live page that is not plain (a resumed sheet). It is
 *   kept apart from the sheet, which each `live` with `reset` replaces, and shown only while the
 *   page's snapshot asks for its key: a page change hides or shows it at once, in whatever order
 *   the snapshot and the picture arrive, and coming back to the sheet needs nothing from the PC,
 *   which sends each viewer a picture once.
 * - `view`: this screen's own view of the sheet (whole page, width, zoom, move; `ViewerView`),
 *   which survives the sheets replaced above and goes when the sheet goes (waiting, ended, code).
 */
import type { LiveBatch, LiveEvent, LivePoint, LiveSnapshot } from './live';
import { DEFAULT_STYLE, LiveState } from './live-state.svelte';
import type { Note, PageSize, Stroke } from './note';
import type { ShareMessage, SharedPage } from './share';
import type { StreamStatus } from './viewer-stream';
import { ViewerView } from './viewer-view.svelte';

/** `connecting`: nothing received yet; `code`: the code is needed. */
export type ViewerScreen = 'connecting' | 'code' | 'waiting' | 'live' | 'note' | 'ended';

/** The whole page in view, as the presentation window shows it at zoom 1. */
function wholePageView(page: PageSize) {
  return { zoom: 1, x: page.width / 2, y: page.height / 2, board: false, laser: false };
}

/** The snapshot a shared note stands for. */
export function noteSnapshot(shared: SharedPage): LiveSnapshot {
  const page: Note = {
    id: shared.id,
    title: '',
    created: 0,
    modified: 0,
    page: shared.page,
    background: shared.background,
    tags: [],
    layers: shared.layers,
    strokes: shared.strokes,
    timing: 'recorded',
    origin: { kind: 'ardoise' },
  };
  return {
    state: 'idle',
    page,
    pageIndex: 0,
    pageCount: 1,
    orientation: shared.page.width >= shared.page.height ? 'landscape' : 'portraitLeft',
    layer: 0,
    style: DEFAULT_STYLE,
    view: wholePageView(shared.page),
    hover: null,
    presentation: null,
    saved: [],
  };
}

/** A finished stroke as the events of live mode carry it. */
function liveStroke(stroke: Stroke): Extract<LiveEvent, { type: 'strokeStart' }>['stroke'] {
  const points: LivePoint[] = stroke.points.map((p) => ({ x: p.x, y: p.y, p: p.pressure, t: p.t, recv: 0 }));
  return { ...stroke, points };
}

export class ViewerState {
  screen = $state<ViewerScreen>('connecting');
  /** The PC cannot be reached; the last image stays and the page says so. */
  reconnecting = $state(false);
  /** The PC refused for lack of room (`503`). */
  tooMany = $state(false);
  /** The image is frozen by the teacher. */
  paused = $state(false);
  /** The paper of the note, a PNG data URL; null for plain paper (and in live mode). */
  background = $state<string | null>(null);
  /** The last picture of the paper of a live page. */
  private livePaper = $state.raw<{ key: string; picture: string } | null>(null);
  /** The sheet. Replaced (not reset) by each `note` and each `live` with `reset`: draw the one in hand. */
  live = $state.raw<LiveState>(new LiveState());
  /** How this screen shows the sheet: the PC's view, or its own. */
  readonly view = new ViewerView(() => ({ live: this.live, shown: this.shown }));

  /** Which content is on the sheet: `live` or `note`, else null. */
  get shown(): 'live' | 'note' | null {
    return this.screen === 'live' || this.screen === 'note' ? this.screen : null;
  }

  /** The paper to draw under the sheet shown, a PNG data URL; null for the paper colour alone. */
  get paper(): string | null {
    if (this.screen === 'note') return this.background;
    if (this.screen !== 'live') return null;
    const paper = this.livePaper;
    return paper && paper.key === this.live.paperKey ? paper.picture : null;
  }

  /** The code was accepted: the form goes, the stream comes next (a `401` brings the form back). */
  joined(): void {
    this.screen = 'connecting';
  }

  /** The connection changed. */
  status(status: StreamStatus): void {
    switch (status) {
      case 'connecting':
        break;
      case 'open':
        this.reconnecting = false;
        this.tooMany = false;
        // A paused share says so to every stream it opens: a badge from before a cut must not stay.
        this.paused = false;
        break;
      case 'needsCode':
        this.reconnecting = false;
        this.tooMany = false;
        this.paused = false;
        this.clear();
        this.livePaper = null;
        this.screen = 'code';
        break;
      case 'tooMany':
        this.tooMany = true;
        this.reconnecting = false;
        break;
      case 'reconnecting':
        this.reconnecting = true;
        this.tooMany = false;
        break;
      case 'ended':
        this.ended();
        break;
    }
  }

  /** A message of the stream. */
  handle(message: ShareMessage): void {
    switch (message.type) {
      case 'waiting':
        this.clear();
        this.screen = 'waiting';
        break;
      case 'live':
        this.liveBatch(message.batch, message.reset);
        break;
      case 'livePaper':
        this.livePaper = { key: message.key, picture: message.picture };
        break;
      case 'note':
        // Without `background`, the paper is the one already shown (the server sends its
        // picture, which can be large, only when it changes).
        this.setNote(message.page, 'background' in message ? (message.background ?? null) : undefined);
        break;
      case 'noteStrokes':
        this.appendStrokes(message.strokes);
        break;
      case 'pause':
        this.paused = message.paused;
        break;
      case 'end':
        this.ended();
        break;
    }
  }

  /** The page goes away (the viewer leaves): later batches are ignored. */
  close(): void {
    this.live.close();
  }

  private ended(): void {
    // The stream says "ended" again at every retry.
    if (this.screen === 'ended') return;
    this.clear();
    this.livePaper = null;
    this.paused = false;
    this.reconnecting = false;
    this.screen = 'ended';
  }

  /** A new, empty sheet; the old one stops listening. */
  private clear(): void {
    this.live.close();
    this.live = new LiveState();
    this.background = null;
    this.view.reset();
  }

  private liveBatch(batch: LiveBatch, reset: boolean): void {
    if (!reset) {
      // Increments of a session this page has the start of; any other batch has nothing to add to.
      if (this.screen === 'live') this.live.receive(batch);
      return;
    }
    const [first, ...rest] = batch.events;
    // A subscription starts with a snapshot; without one there is nothing to start from.
    if (first?.type !== 'snapshot') return;
    this.live.close();
    const fresh = new LiveState();
    fresh.initial(first.snapshot);
    fresh.receive({ ...batch, events: rest });
    this.live = fresh;
    this.background = null;
    this.screen = 'live';
  }

  /** `background` undefined: the paper of the note shown stays. */
  private setNote(shared: SharedPage, background: string | null | undefined): void {
    const keep = background === undefined && this.screen === 'note' ? this.background : null;
    this.live.close();
    const fresh = new LiveState();
    fresh.initial(noteSnapshot(shared));
    this.live = fresh;
    this.background = background === undefined ? keep : background;
    this.screen = 'note';
  }

  private appendStrokes(strokes: readonly Stroke[]): void {
    // Without a note on the sheet there is nothing to append to; the next `note` brings the page.
    if (this.screen !== 'note' || strokes.length === 0) return;
    // As strokes the pen just wrote: each is finished by the next one or by its end.
    const events: LiveEvent[] = [];
    for (const stroke of strokes) {
      events.push({ type: 'strokeStart', pageIndex: 0, stroke: liveStroke(stroke) }, { type: 'strokeEnd', pageIndex: 0 });
    }
    this.live.receive({ events, sentAt: 0 });
  }
}
