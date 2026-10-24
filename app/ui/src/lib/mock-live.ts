/**
 * Browser stand-in for live mode and the classroom (`live_*`, `list_monitors`,
 * `open_presentation`, `close_presentation`): a simulated pen streaming batches.
 * Work package D (docs/design/tablet-live-classroom.md).
 */
import type { LiveBatch } from './live';
import { UNHANDLED } from './mock-data';

export function mockLiveInvoke(command: string, args: Record<string, unknown>): unknown {
  void command;
  void args;
  return UNHANDLED;
}

/** `live_subscribe` in the browser. */
export async function mockLiveSubscribe(onBatch: (batch: LiveBatch) => void): Promise<never> {
  void onBatch;
  throw new Error('mock live mode not implemented yet');
}
