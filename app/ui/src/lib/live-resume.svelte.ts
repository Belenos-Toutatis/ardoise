/**
 * Whether a note can be resumed in live mode now (M12, docs/design/tablet-live-classroom.md,
 * "Resume"), from the tablet's status (the same rule as the tablet panel's « Mode direct » button).
 * What the live view knows of a resumed sheet (does the page turn, which sheets went on in a new
 * note) is kept by `LiveState`, beside the snapshots and batches that tell it.
 */
import type { TabletStatus } from './tablet';

/** Why a note cannot be resumed now; null when it can. */
export type ResumeBlock = 'noTablet' | 'busy';

/**
 * Live mode needs a known tablet (the Slate or the blackboard), and not while it syncs, registers or adds one
 * (the tablet panel disables its button in the same cases). Until the status is known, nothing can be resumed.
 */
export function resumeBlock(status: TabletStatus | null): ResumeBlock | null {
  if (!status || (!status.registered && status.known.length === 0)) return 'noTablet';
  return status.connection === 'syncing' || status.connection === 'registering' || status.connection === 'adding'
    ? 'busy'
    : null;
}
