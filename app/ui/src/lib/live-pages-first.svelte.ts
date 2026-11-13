/**
 * The « pages first » dialog of live mode (SPEC §4.2, docs/design/blackboard.md §9.5). The Xiaomi blackboard that still
 * holds stored pages does not start live mode unless the automatic sync and « Delete after import » are both on: the
 * service refuses `live_start` / `live_resume` (`blackboardPagesFirst`), or ends a run that was waiting with the
 * `refused` live event. The live view is the one place that starts live mode, whoever asked for it (the tablet panel,
 * a card's menu, the editor): it hands the refusal to this controller, whose dialog (`PagesFirstDialog.svelte`)
 * offers to import and delete the pages (`tablet_sync` with `confirmBlackboardDelete`), follows the sync on the
 * window's tablet status, then asks live mode again, or shows itself again when pages are left, or says why the
 * import stopped. Cancelling leaves live mode idle.
 */
import { errorMessage } from './api';
import { t } from './i18n.svelte';
import { isPagesFirst, pagesInMessage } from './live-pages-first';
import type { TabletStatus } from './tablet';
import { tablet } from './tablet-state.svelte';

/** What the dialog waits for: the user's answer, or the sync it asked for. */
export type PagesFirstPhase = 'asking' | 'importing';

export class PagesFirst {
  /** The dialog shows. */
  open = $state(false);
  phase = $state<PagesFirstPhase>('asking');
  /** Pages stored on the tablet, as the last refusal or sync said. */
  pages = $state(0);
  /** Why the import stopped (translated); null while there is nothing to report. */
  error = $state<string | null>(null);

  /** `lastSync.at` when the import was asked: a different one is the end of the sync that followed. */
  private baseline: number | null = null;
  /** The service's own failure at that time: only a different one is the import's. */
  private baselineError: string | null = null;
  /** Cancel was asked during the import: the sync is stopped, and live mode is left once it has ended. */
  private leaving = false;

  /**
   * @param again live mode can be asked again (the pages are imported).
   * @param leave the user gave up: the live view closes, nothing is started.
   */
  constructor(
    private readonly again: () => void,
    private readonly leave: () => void,
  ) {}

  /**
   * Takes a refused start (a rejected command or the `refused` event): true when it is this refusal, the dialog then
   * shows. Any other error is left to the caller.
   */
  refuse(error: unknown): boolean {
    if (!isPagesFirst(error)) return false;
    const message = (error as { message?: unknown }).message;
    // The count the service made the refusal with, else what the tablet last read (the same number, §9.5).
    const stored = tablet.status?.device?.kind === 'blackboard' ? tablet.status.device.storedPages : 0;
    this.pages = Math.max(1, (typeof message === 'string' ? pagesInMessage(message) : null) ?? stored);
    this.phase = 'asking';
    this.error = null;
    this.leaving = false;
    this.open = true;
    return true;
  }

  /** « Import and delete »: once; a second press while the sync runs does nothing. */
  async importPages(): Promise<void> {
    if (!this.open || this.phase !== 'asking') return;
    this.phase = 'importing';
    this.error = null;
    this.leaving = false;
    this.baseline = tablet.status?.lastSync?.at ?? null;
    this.baselineError = errorKey(tablet.status);
    if (await tablet.sync(true)) return;
    // The service did not take the request (the tablet went away, is busy...).
    this.stop(tablet.actionError ?? t('live.pagesFirst.lost'));
    tablet.dismissError();
  }

  /**
   * « Cancel » and Escape. Before the import: the dialog goes and the live view closes. During it: the sync is stopped
   * between two pages (what was imported stays imported), then the dialog goes.
   */
  cancel(): void {
    if (!this.open) return;
    if (this.phase === 'asking') {
      this.close();
      this.leave();
      return;
    }
    this.leaving = true;
    void tablet.cancel();
  }

  /** The window's status moved: follows the sync that was asked for (the dialog calls it on every change). */
  follow(status: TabletStatus | null): void {
    if (!this.open || this.phase !== 'importing' || status === null) return;
    if (status.connection === 'syncing') return;
    const last = status.lastSync;
    if (last !== null && last.at !== this.baseline) {
      this.synced(status, last.error);
      return;
    }
    // Not over, and the tablet is not there any more: the link was lost (`connected` or `connecting` is the wait before
    // the sync starts).
    if (status.connection !== 'connected' && status.connection !== 'connecting') {
      this.stop(t('live.pagesFirst.lost'));
      return;
    }
    // The service failed without a sync result (`status.error` is the service's own, a stale one is not).
    if (status.error !== null && !tablet.confirmDeleteAsked && errorKey(status) !== this.baselineError) {
      this.stop(errorMessage(status.error));
    }
  }

  /** A sync ended after the import was asked. */
  private synced(status: TabletStatus, failure: { code: string; message: string } | null): void {
    if (this.leaving) {
      this.close();
      this.leave();
      return;
    }
    if (failure !== null) {
      this.stop(errorMessage(failure));
      return;
    }
    const left = status.device?.kind === 'blackboard' ? status.device.storedPages : 0;
    if (left > 0) {
      // The sync ended with pages on the tablet (it was stopped, or not all were read): the dialog again, its count new.
      this.pages = left;
      this.phase = 'asking';
      return;
    }
    // Nothing is left: live mode is asked again. If the service still refuses, `refuse` shows the dialog again.
    this.close();
    this.again();
  }

  /** The import ended without making live mode possible: the reason is shown and the user decides (again, or cancel). */
  private stop(reason: string): void {
    if (this.leaving) {
      this.close();
      this.leave();
      return;
    }
    this.error = reason;
    this.phase = 'asking';
  }

  private close(): void {
    this.open = false;
    this.phase = 'asking';
    this.error = null;
    this.leaving = false;
  }
}

/** The service's own failure of a status as a comparable string (`''` when none). */
function errorKey(status: TabletStatus | null): string | null {
  return status?.error ? `${status.error.code}:${status.error.message}` : null;
}
