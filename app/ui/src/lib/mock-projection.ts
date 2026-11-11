/**
 * Browser stand-in for the editor's projection commands (`projection_*`, milestone M17; Rust side
 * `projection.rs`, docs/design/paper-projection-narration.md §2.3). Package 0: a skeleton that answers as if the
 * projection were closed and never opens; package D completes it with the rules of Rust's projection (a scene, the
 * events live's own vocabulary, the stale and invalid refusals).
 */
import { UNHANDLED } from './mock-data';
import type { LiveBatch, LiveSnapshot } from './live';
import type { ProjectionStatus } from './projection';

/** What `projection_watch` answers while the projection is closed. */
const CLOSED: ProjectionStatus = { open: null };

/** The snapshot `projection_subscribe` answers while there is no scene: an idle, empty A4 page (the window stays black). */
function idleSnapshot(): LiveSnapshot {
  const page = { width: 29700, height: 21600 };
  return {
    state: 'idle',
    page: {
      id: '',
      title: '',
      created: 0,
      modified: 0,
      page,
      background: { paper: 'white', template: 'blank', texture: false },
      tags: [],
      layers: [{ name: 'Calque 1', visible: true }],
      strokes: [],
      timing: 'recorded',
      origin: { kind: 'live' },
    },
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    canTurnPage: false,
    layer: 0,
    computerLayer: 0,
    style: { tool: 'ballpoint', color: { r: 0, g: 0, b: 0, a: 255 }, width: 60 },
    view: { zoom: 1, x: page.width / 2, y: page.height / 2, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
    revision: 0,
    canUndo: false,
    canRedo: false,
    pagePaper: 'white',
    imagePixels: null,
  };
}

export async function mockProjectionWatch(onStatus: (status: ProjectionStatus) => void): Promise<ProjectionStatus> {
  void onStatus;
  return { ...CLOSED };
}

export async function mockProjectionSubscribe(onBatch: (batch: LiveBatch) => void): Promise<LiveSnapshot> {
  void onBatch;
  return idleSnapshot();
}

export function mockProjectionInvoke(command: string, args: Record<string, unknown>): unknown {
  void args;
  switch (command) {
    case 'projection_open':
      throw { code: 'noMonitor', message: 'no screen is connected' };
    case 'projection_close':
    case 'projection_update':
      return undefined;
    default:
      return UNHANDLED;
  }
}
