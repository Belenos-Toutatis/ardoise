/**
 * Types of the guided tablet test, mirroring `crates/ardoise-device/src/diagnostic.rs`
 * (serde, camelCase) and the events streamed by the `diagnostic_start` command.
 */

export type StepId =
  | 'adapter'
  | 'referencePages'
  | 'find'
  | 'connect'
  | 'info'
  | 'sync'
  | 'live'
  | 'reconnect';

/** The steps in the order they run. */
export const STEP_IDS: readonly StepId[] = [
  'adapter',
  'referencePages',
  'find',
  'connect',
  'info',
  'sync',
  'live',
  'reconnect',
];

export type Instruction =
  | { kind: 'drawReferencePages'; count: number }
  | { kind: 'pressButton' }
  | { kind: 'holdButtonToRegister' }
  | { kind: 'pressButtonToConfirm' }
  | { kind: 'drawLive'; seconds: number };

export type Decision = 'continue' | 'skip' | 'cancel';

export type Progress =
  | { kind: 'waiting' }
  | { kind: 'downloading'; page: number; total: number }
  | { kind: 'live'; samples: number; elapsedMs: number };

export type StepStatus = 'passed' | 'failed' | 'skipped';

export interface Detail {
  key: string;
  value: string;
}

export interface StepOutcome {
  step: StepId;
  status: StepStatus;
  details: Detail[];
}

export interface DiagnosticReport {
  appVersion: string;
  transport: string;
  started: string;
  steps: StepOutcome[];
  logPath: string | null;
  reportPath: string | null;
  cancelled: boolean;
}

export interface DiagnosticOptions {
  simulated: boolean;
  deleteAfterSync: boolean;
}

export type DiagnosticEvent =
  | { type: 'started'; step: StepId }
  | { type: 'prompt'; step: StepId; instruction: Instruction }
  | { type: 'notify'; step: StepId; instruction: Instruction }
  | { type: 'progress'; step: StepId; progress: Progress }
  | { type: 'finished'; outcome: StepOutcome }
  | { type: 'done'; report: DiagnosticReport }
  | { type: 'error'; code: string; message: string };

/** Placeholders of an instruction's translated sentence. */
export function instructionParams(instruction: Instruction): Record<string, number> {
  switch (instruction.kind) {
    case 'drawReferencePages':
      return { count: instruction.count };
    case 'drawLive':
      return { seconds: instruction.seconds };
    default:
      return {};
  }
}

export function countSteps(report: DiagnosticReport): Record<StepStatus, number> {
  const counts: Record<StepStatus, number> = { passed: 0, failed: 0, skipped: 0 };
  for (const outcome of report.steps) counts[outcome.status] += 1;
  return counts;
}
