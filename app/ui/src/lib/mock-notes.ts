/**
 * Browser stand-in for the editor commands (`open_note`, `save_note`, `render_background`), over
 * the demo library of `mock-data.ts`.
 */
import { UNHANDLED } from './mock-data';

export function mockNotesInvoke(command: string, args: Record<string, unknown>): unknown {
  void command;
  void args;
  return UNHANDLED;
}
