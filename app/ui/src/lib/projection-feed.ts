/**
 * What the editor sends to its projection (milestone M17; docs/design/paper-projection-narration.md §2.4): small
 * updates computed by object identity, the whole page only when its layout changes, the stroke in progress point by
 * point, one call in flight (`lib/projection.ts` is what Rust receives).
 *
 * Package 0: the final API with no behaviour (`projectionDiff` finds nothing to send, `ProjectionFeed` does nothing);
 * package D builds it.
 */
import type { InkPoint, Tool } from './ink';
import type { Background, BackgroundImage, Layer, Note, PageSize, Rgba, Stroke } from './note';
import type { ProjectionUpdate, ProjectionView } from './projection';

/** What the editor tells of its stroke in progress (`Canvas`'s `oninprogress`, package E calls it). */
export type InProgress =
  | {
      kind: 'begin';
      id: number;
      tool: Tool;
      color: Rgba;
      width: number;
      layer: number;
      points: readonly InkPoint[];
      /** Wall clock (Unix ms) at note time `base`: a point's wall time is `wallAtBase + (p.t - base)`. */
      wallAtBase: number;
      base: number;
    }
  /** All the points so far (the feed sends the new ones). */
  | { kind: 'grow'; id: number; points: readonly InkPoint[] }
  /** `committed`: the doc gets the stroke next. */
  | { kind: 'end'; id: number; committed: boolean };

/** The last page sent: what a new note is compared with (`projectionDiff`). */
export interface SentPage {
  id: string;
  page: PageSize;
  background: Background;
  backgroundImage: BackgroundImage | undefined;
  layers: readonly Layer[];
  imageKey: string | null;
  /** The stroke objects as sent: the common prefix is found by `===`. */
  strokes: readonly Stroke[];
}

/** The update that brings what was sent to `note`, or null when nothing changed (pure; tested alone). */
export function projectionDiff(
  sent: SentPage | null,
  note: Note,
  imageKey: string | null,
  view: ProjectionView,
): ProjectionUpdate | null {
  void [sent, note, imageKey, view];
  return null;
}

export class ProjectionFeed {
  /** `send` delivers one update to Rust (`api.projectionUpdate`); the feed never has two calls in flight. */
  constructor(send: (update: ProjectionUpdate) => Promise<void>) {
    void send;
  }

  /** The editor shows `note` (with the staged pixels of its image under `imageKey`, or null). */
  show(note: Note, imageKey: string | null): void {
    void [note, imageKey];
  }

  /** The projected area changed. */
  view(view: ProjectionView): void {
    void view;
  }

  /** The editor's stroke in progress. */
  stroke(event: InProgress): void {
    void event;
  }

  /** The next `show` sends the whole page (the projection opened again, `projectionStale`). */
  reset(): void {}
}
