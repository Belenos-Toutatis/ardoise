/**
 * In-memory stand-in for the Rust commands, used when the interface runs in a plain browser
 * (`npm run dev`) so screens can be developed and tested without the desktop app.
 */
import type { AppState, NoteOutlines } from './api';
import { mockBackgroundInvoke } from './mock-background';
import { UNHANDLED, setMockAutoExportSettings } from './mock-data';
import { checkAutoExport, mockExportInvoke } from './mock-export';
import { mockDragInvoke } from './mock-drag';
import { mockHandwritingInvoke } from './mock-handwriting';
import { mockLibraryInvoke } from './mock-library';
import { mockLiveInvoke } from './mock-live';
import { mockNarrationInvoke } from './mock-narration';
import { mockNotesInvoke } from './mock-notes';
import { mockTabletInvoke } from './mock-tablet';
import { mockVideoInvoke } from './mock-video';

export { mockOnLibraryChanged } from './mock-data';
import {
  STEP_IDS,
  type Decision,
  type Detail,
  type DiagnosticEvent,
  type DiagnosticOptions,
  type Instruction,
  type StepId,
  type StepOutcome,
  type StepStatus,
} from './diagnostic';

const state: AppState = {
  version: '0.0.0-browser',
  machine: 'BROWSER',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: new URLSearchParams(location.search).get('library'),
  systemLocale: navigator.language,
  selfTest: false,
  openDiagnostic: new URLSearchParams(location.search).has('diagnostic'),
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
};

const libraries = new Set<string>();

export async function mockInvoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = (() => {
    switch (command) {
      case 'app_state':
        return { ...state };
      case 'set_language':
        state.language = args.language as AppState['language'];
        return { ...state };
      case 'set_theme':
        state.theme = args.theme as AppState['theme'];
        return { ...state };
      case 'set_auto_export':
        state.autoExport = checkAutoExport(args.autoExport as AppState['autoExport']);
        setMockAutoExportSettings(state.autoExport);
        return { ...state };
      case 'inspect_library':
        return { existing: libraries.has(args.path as string) };
      case 'set_microphone':
        state.microphone = (args.device as string | null) ?? null;
        return { ...state };
      case 'set_library':
        libraries.add(args.path as string);
        state.library = args.path as string;
        return { ...state };
      case 'note_outlines':
        return {
          width: 29700,
          height: 21600,
          strokes: [{ color: '#000000ff', tool: 'ballpoint', layer: 0, path: 'M1000 1000L2000 1000L2000 1050L1000 1050Z' }],
        } satisfies NoteOutlines;
      case 'self_test_report':
        return undefined;
      case 'diagnostic_answer':
        answerMockPrompt(args.decision as Decision);
        return undefined;
      case 'diagnostic_cancel':
        cancelMockRun();
        return undefined;
      default: {
        // Library, editor and export commands live in their own modules, over the demo library.
        for (const handle of [
          mockLibraryInvoke,
          mockNotesInvoke,
          mockExportInvoke,
          mockDragInvoke,
          mockTabletInvoke,
          mockLiveInvoke,
          mockVideoInvoke,
          mockNarrationInvoke,
          mockBackgroundInvoke,
          mockHandwritingInvoke,
        ]) {
          const handled = handle(command, args);
          if (handled !== UNHANDLED) return handled;
        }
        throw { code: 'unknownCommand', message: command };
      }
    }
  })();
  return result as T;
}

/** Pace of the scripted tablet test, in milliseconds per step; tests set it to 0. */
export const mockDiagnostic = { stepMs: 700 };

interface MockRun {
  cancelled: boolean;
  /** Settles when the run is over. */
  finished: Promise<void>;
  /** Resolves the pending prompt, if any. */
  answer: ((decision: Decision) => void) | null;
}

let currentRun: MockRun | null = null;

function answerMockPrompt(decision: Decision): void {
  const answer = currentRun?.answer;
  if (currentRun) currentRun.answer = null;
  answer?.(decision);
}

function cancelMockRun(): void {
  if (!currentRun) return;
  currentRun.cancelled = true;
  answerMockPrompt('cancel');
}

/** Starts a scripted run of the tablet test, shaped like the events the desktop app streams. */
export function mockDiagnosticStart(
  options: DiagnosticOptions,
  onEvent: (event: DiagnosticEvent) => void,
): Promise<void> {
  if (currentRun) return Promise.reject({ code: 'diagnosticRunning', message: 'already running' });
  const run: MockRun = { cancelled: false, finished: Promise.resolve(), answer: null };
  currentRun = run;
  run.finished = scriptedDiagnostic(options, onEvent, run).finally(() => {
    currentRun = null;
  });
  return Promise.resolve();
}

/** Stops a run in progress and waits for it to end (tests use it to start from a clean slate). */
export async function stopMockDiagnostic(): Promise<void> {
  const run = currentRun;
  cancelMockRun();
  await run?.finished;
}

async function scriptedDiagnostic(
  options: DiagnosticOptions,
  onEvent: (event: DiagnosticEvent) => void,
  run: MockRun,
): Promise<void> {
  const outcomes: StepOutcome[] = [];
  const pause = (factor = 1) => new Promise<void>((resolve) => setTimeout(resolve, mockDiagnostic.stepMs * factor));
  const detail = (key: string, value: string): Detail => ({ key, value });
  const finish = (step: StepId, status: StepStatus, details: Detail[] = []) => {
    const outcome = { step, status, details };
    outcomes.push(outcome);
    onEvent({ type: 'finished', outcome });
  };
  const ask = (step: StepId, instruction: Instruction) =>
    new Promise<Decision>((resolve) => {
      run.answer = resolve;
      onEvent({ type: 'prompt', step, instruction });
    });

  /** Runs one step; `false` when the user stopped the test. */
  const runStep = async (step: StepId): Promise<boolean> => {
    onEvent({ type: 'started', step });
    // The steps that begin with an instruction can be skipped or stop the test.
    const instruction = ((): Instruction | null => {
      switch (step) {
        case 'referencePages':
          return { kind: 'drawReferencePages', count: 3 };
        case 'find':
          return { kind: 'holdButtonToRegister' };
        case 'live':
          return { kind: 'drawLive', seconds: 30 };
        default:
          return null;
      }
    })();
    if (instruction) {
      const decision = await ask(step, instruction);
      if (decision === 'cancel' || run.cancelled) return false;
      if (decision === 'skip') {
        finish(step, 'skipped');
        return true;
      }
    }
    onEvent({ type: 'progress', step, progress: { kind: 'waiting' } });
    await pause();
    switch (step) {
      case 'adapter':
        finish(step, 'passed', [detail('adapter', 'Intel Wireless Bluetooth')]);
        break;
      case 'referencePages':
        finish(step, 'passed');
        break;
      case 'find':
        finish(step, 'passed', [
          detail('name', 'Bamboo Slate'),
          detail('address', 'AA:BB:CC:DD:EE:FF'),
          detail('signal', '-58 dBm'),
          detail('registerMode', 'oui'),
        ]);
        break;
      case 'connect':
        onEvent({ type: 'notify', step, instruction: { kind: 'pressButtonToConfirm' } });
        await pause();
        finish(step, 'passed', [detail('mode', 'enregistrement'), detail('identity', 'a1b2c3d4…')]);
        break;
      case 'info':
        finish(step, 'passed', [
          detail('name', 'Bamboo Slate'),
          detail('firmware', 'B160805A-C170704A'),
          detail('battery', '87 %'),
          detail('size', '297 × 216 mm'),
          detail('pages', '3'),
        ]);
        break;
      case 'sync':
        for (let page = 1; page <= 3; page++) {
          onEvent({ type: 'progress', step, progress: { kind: 'downloading', page, total: 3 } });
          await pause(0.5);
          if (run.cancelled) return false;
        }
        finish(step, 'passed', [
          detail('pages', '3'),
          detail('strokes', '41'),
          detail('points', '5210'),
          detail('deleted', options.deleteAfterSync ? '3' : '0'),
          detail('folder', '_ardoise\\diagnostics\\2026-10-24_14-02-11'),
        ]);
        break;
      case 'live': {
        let samples = 0;
        for (let tick = 1; tick <= 6; tick++) {
          samples += 380;
          onEvent({ type: 'progress', step, progress: { kind: 'live', samples, elapsedMs: tick * 5000 } });
          await pause(0.5);
          if (run.cancelled) return false;
        }
        finish(step, 'passed', [
          detail('samples', String(samples)),
          detail('rate', '76 Hz'),
          detail('xRange', '0 – 21600'),
          detail('yRange', '0 – 14800'),
          detail('pressureMax', '1023'),
        ]);
        break;
      }
      case 'reconnect':
        finish(step, 'passed', [detail('reconnections', '1')]);
        break;
    }
    return !run.cancelled;
  };

  let cancelled = false;
  for (const step of STEP_IDS) {
    if (!(await runStep(step))) {
      cancelled = true;
      break;
    }
  }
  const folder = state.library ?? 'C:\\demo';
  // Released before `done` so that the interface may start another run from its handler.
  if (currentRun === run) currentRun = null;
  onEvent({
    type: 'done',
    report: {
      appVersion: state.version,
      transport: options.simulated ? 'sim' : 'ble',
      started: new Date().toISOString(),
      steps: outcomes,
      logPath: `${folder}\\_ardoise\\diagnostics\\2026-10-24_14-02-11.jsonl`,
      reportPath: `${folder}\\_ardoise\\diagnostics\\2026-10-24_14-02-11.txt`,
      cancelled,
    },
  });
}
