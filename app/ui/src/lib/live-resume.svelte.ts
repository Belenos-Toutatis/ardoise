/**
 * What the live view needs to know of a resumed sheet (M12, docs/design/tablet-live-classroom.md,
 * "Resume"): whether the current page turns with the tablet (`canTurnPage` of the snapshot) and the
 * sheets that went on in a new note (`sheetForked`). `LiveState` keeps neither, so this watches the
 * same snapshots and batches from the side: it wraps the subscription (`watch`), then every batch
 * and snapshot passes through it before `LiveState` gets them.
 *
 * Also `resumeBlock`: whether a note can be resumed now, from the tablet's status (the same rule
 * as the tablet panel's « Mode direct » button).
 */
import type { LiveBatch, LiveSnapshot, SheetFork } from './live';
import type { TabletStatus } from './tablet';

export type { SheetFork };

/** Why a note cannot be resumed now; null when it can. */
export type ResumeBlock = 'noTablet' | 'busy';

/**
 * Live mode needs the registered tablet, and not while it syncs or registers (the tablet panel
 * disables its button in the same cases). Until the status is known, nothing can be resumed.
 */
export function resumeBlock(status: TabletStatus | null): ResumeBlock | null {
  if (!status || !status.registered) return 'noTablet';
  return status.connection === 'syncing' || status.connection === 'registering' ? 'busy' : null;
}

export class ResumeWatch {
  /** The current page turns with the tablet; false on a resumed sheet, which is never turned. */
  canTurnPage = $state(true);
  /** The sheets that went on in a new note, in the order they were told; each stays until dismissed. */
  forks = $state.raw<SheetFork[]>([]);
  /** Title of the sheet resumed ('' when it has none). */
  title = $state('');

  private told = new Set<string>();

  /** The snapshot `live_resume` returned: the sheet's title, and how the page is held. */
  begin(snapshot: LiveSnapshot): void {
    this.title = snapshot.page.title.trim();
    this.see(snapshot);
  }

  /** Wraps `subscribe` (`api.liveSubscribe`) so that its snapshot and batches pass through here first. */
  watch(
    subscribe: (onBatch: (batch: LiveBatch) => void) => Promise<LiveSnapshot>,
  ): (onBatch: (batch: LiveBatch) => void) => Promise<LiveSnapshot> {
    return async (onBatch) => {
      const snapshot = await subscribe((batch) => {
        this.receive(batch);
        onBatch(batch);
      });
      this.see(snapshot);
      return snapshot;
    };
  }

  /** Forgets the notice of a fork (the user dismissed it). */
  dismiss(fork: SheetFork): void {
    this.forks = this.forks.filter((f) => f !== fork);
  }

  private see(snapshot: LiveSnapshot): void {
    this.canTurnPage = snapshot.canTurnPage !== false;
  }

  private receive(batch: LiveBatch): void {
    for (const event of batch.events) {
      if (event.type === 'snapshot') this.see(event.snapshot);
      else if (event.type === 'sheetForked') {
        const { pageIndex, reason, original, path } = event;
        // Once per fork, even if it is told twice (and not again after it was dismissed).
        const key = `${original}|${path}`;
        if (!this.told.has(key)) {
          this.told.add(key);
          this.forks = [...this.forks, { pageIndex, reason, original, path }];
        }
      }
    }
  }
}
