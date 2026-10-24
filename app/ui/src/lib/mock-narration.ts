/**
 * Browser stand-in for the microphone and narration commands (`list_microphones`,
 * `microphone_monitor`, `narration_*`, `delete_narration`).
 *
 * Contract stub: filled in by the editor replay and narration work package.
 */
import { UNHANDLED } from './mock-data';

export function mockNarrationInvoke(
  command: string,
  args: Record<string, unknown>,
): unknown {
  void command;
  void args;
  return UNHANDLED;
}
