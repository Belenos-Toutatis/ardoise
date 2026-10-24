/**
 * Browser stand-in for the library commands (`library_list`, `note_thumbnail`, `create_note`,
 * binders, tags, favourites, trash, `tablet_info`…), over the demo library of `mock-data.ts`.
 */
import { UNHANDLED } from './mock-data';

export function mockLibraryInvoke(command: string, args: Record<string, unknown>): unknown {
  void command;
  void args;
  return UNHANDLED;
}
