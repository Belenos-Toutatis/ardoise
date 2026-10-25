/**
 * Browser stand-in for the drag-out commands (`prepare_note_drag`, `start_note_drag`,
 * `note_drag_supported`; Rust side: `drag.rs`). It only records what it was asked.
 */
import { UNHANDLED } from './mock-data';

export const mockDrag = {
  supported: true,
  /** What the system drag ends with. */
  outcome: 'dropped' as 'dropped' | 'cancelled',
  /** Pending delay, in milliseconds, of the preparation (tests make it long to watch the wait). */
  prepareMs: 0,
  /** Every call, for tests. */
  prepared: [] as string[][],
  started: [] as string[][],
};

export function resetMockDrag(): void {
  mockDrag.supported = true;
  mockDrag.outcome = 'dropped';
  mockDrag.prepareMs = 0;
  mockDrag.prepared = [];
  mockDrag.started = [];
}

/** The PNG file the real command would write for the note file at `path`. */
function pngOf(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? 'Note';
  return `C:\\Users\\demo\\AppData\\Local\\Temp\\Ardoise\\drag\\mock\\${name.replace(/\.ardoise$/i, '')}.png`;
}

export function mockDragInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'note_drag_supported':
      return mockDrag.supported;
    case 'prepare_note_drag': {
      const paths = args.paths as string[];
      mockDrag.prepared.push(paths);
      const files = paths.map(pngOf);
      if (mockDrag.prepareMs <= 0) return files;
      return new Promise((resolve) => setTimeout(() => resolve(files), mockDrag.prepareMs));
    }
    case 'start_note_drag':
      mockDrag.started.push(args.files as string[]);
      return mockDrag.outcome;
    default:
      return UNHANDLED;
  }
}
