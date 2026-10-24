/**
 * Browser stand-in for the tablet commands (`tablet_*`, `sim_press_button`): a simulated tablet
 * that registers, connects, syncs pages into the demo library and reports its status.
 * Work package C (docs/design/tablet-live-classroom.md).
 */
import { UNHANDLED } from './mock-data';
import type { TabletEvent } from './tablet';

export function mockTabletInvoke(command: string, args: Record<string, unknown>): unknown {
  void command;
  void args;
  return UNHANDLED;
}

/** `tablet_subscribe` in the browser. */
export async function mockTabletSubscribe(onEvent: (event: TabletEvent) => void): Promise<never> {
  void onEvent;
  throw new Error('mock tablet not implemented yet');
}
