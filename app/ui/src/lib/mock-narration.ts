/**
 * Browser stand-in for the microphone and narration commands (`list_microphones`,
 * `microphone_monitor*`, `narration_*`, `delete_narration`; `set_microphone` is in `mock.ts`, with
 * the app state). Two fake microphones, a level that moves with the time, the states of the real
 * capture (waiting, receiving, silent, no data, failed), and narrations stored per note (by note
 * id, so that a rename keeps them) whose saving and deletion change the note's stamp like the
 * Rust commands do. A replay recording that cannot be saved stays pending, as in Rust, until it is
 * retried or discarded.
 */
import { UNHANDLED, freshStamp, mockLibrary, notifyLibraryChanged, sameStamp } from './mock-data';
import type { FileStamp } from './note';
import type { Microphone, NarrationInfo, NarrationSaved, NarrationStatus, NarrationTarget } from './video';

/**
 * Switches for tests and for development. `silent` makes the capture deliver exact digital silence
 * (Windows blocks the microphone, or a hardware mute), `noData` that nothing arrives at all,
 * `failure` that the capture fails with that detail, `empty` that saving finds nothing recorded
 * (`emptyRecording`), `saveConflict` that saving a replay recording fails with `noteConflict`
 * (as if the note changed on disk). Delays are in milliseconds since the start.
 */
export const mockNarration = {
  microphones: [
    { id: 'mock-mic-1', name: 'Microphone Array (Realtek Audio)', isDefault: true },
    { id: 'mock-mic-2', name: 'Headset Microphone (USB Audio)', isDefault: false },
  ] as Microphone[],
  silent: false,
  noData: false,
  failure: null as string | null,
  empty: false,
  saveConflict: false,
  /** How long the capture is "waiting" before the first samples arrive. */
  receivingAfterMs: 200,
  /** How long exact silence lasts before the capture reports `silent`. */
  silentAfterMs: 1000,
  /** Pages a live recording gives narration to when it is saved. */
  livePages: 1,
};

/** A narration in the stand-in's store. */
interface Stored {
  info: NarrationInfo;
}

const stored = new Map<string, Stored>();

interface Capture {
  kind: 'monitoring' | 'recording';
  device: string | null;
  target: NarrationTarget | null;
  startedAt: number;
}

let capture: Capture | null = null;

/** A replay recording whose saving failed: kept until retried or discarded. */
interface Pending {
  target: Extract<NarrationTarget, { kind: 'replay' }>;
  durationMs: number;
  startMs: number;
}

let pending: Pending | null = null;

/** Forgets the capture and the stored narrations, and restores the switches (tests). */
export function resetMockNarration(): void {
  capture = null;
  pending = null;
  stored.clear();
  mockNarration.silent = false;
  mockNarration.noData = false;
  mockNarration.failure = null;
  mockNarration.empty = false;
  mockNarration.saveConflict = false;
  mockNarration.receivingAfterMs = 200;
  mockNarration.silentAfterMs = 1000;
  mockNarration.livePages = 1;
  mockNarration.microphones = [
    { id: 'mock-mic-1', name: 'Microphone Array (Realtek Audio)', isDefault: true },
    { id: 'mock-mic-2', name: 'Headset Microphone (USB Audio)', isDefault: false },
  ];
}

/** Puts a narration into the note at `path` (null removes it) without touching its stamp (tests). */
export function setMockNarration(path: string, info: NarrationInfo | null): void {
  const id = noteOf(path).note.id;
  if (info) stored.set(id, { info });
  else stored.delete(id);
}

/** The narration of the note at `path`, as the commands see it. */
export function mockNarrationOf(path: string): NarrationInfo | null {
  return stored.get(noteOf(path).note.id)?.info ?? null;
}

function failure(code: string, message: string): never {
  throw { code, message };
}

function noteOf(path: string) {
  return mockLibrary().notes.get(path) ?? failure('noteNotFound', path);
}

function microphoneNamed(device: string | null): Microphone {
  const list = mockNarration.microphones;
  if (list.length === 0) return failure('noMicrophone', 'no microphone');
  if (device === null) return list.find((m) => m.isDefault) ?? list[0];
  return list.find((m) => m.id === device) ?? failure('microphoneNotFound', device);
}

function begin(kind: Capture['kind'], device: string | null, target: NarrationTarget | null): void {
  microphoneNamed(device);
  if (capture) failure('narrationBusy', 'a capture is already running');
  capture = { kind, device, target, startedAt: Date.now() };
}

/** Status of the capture under way: a level that moves with the time, or the switch's state. */
function status(): NarrationStatus {
  if (!capture) {
    return {
      state: 'idle',
      capture: null,
      level: { peak: 0, rms: 0 },
      elapsedMs: 0,
      device: null,
      failure: null,
      pending: pending !== null,
    };
  }
  const elapsedMs = Math.max(0, Date.now() - capture.startedAt);
  const device = microphoneNamed(capture.device).name;
  const base = { state: capture.kind, elapsedMs, device, pending: pending !== null };
  const none = { peak: 0, rms: 0 };
  if (mockNarration.failure !== null) {
    return { ...base, capture: 'failed', level: none, failure: mockNarration.failure };
  }
  if (mockNarration.noData) return { ...base, capture: 'noData', level: none, failure: null };
  if (elapsedMs < mockNarration.receivingAfterMs) return { ...base, capture: 'waiting', level: none, failure: null };
  if (mockNarration.silent) {
    return {
      ...base,
      capture: elapsedMs >= mockNarration.silentAfterMs ? 'silent' : 'waiting',
      level: none,
      failure: null,
    };
  }
  const rms = 0.12 + 0.1 * Math.sin(elapsedMs / 170) ** 2;
  const peak = Math.min(1, rms * (2 + Math.abs(Math.sin(elapsedMs / 53))));
  return { ...base, capture: 'receiving', level: { peak, rms }, failure: null };
}

/** A silent 16-bit mono WAV file of `durationMs` (at most two seconds are written). */
export function silentWav(durationMs: number, sampleRate = 8000): ArrayBuffer {
  const samples = Math.round((Math.min(Math.max(durationMs, 0), 2000) / 1000) * sampleRate);
  const buffer = new ArrayBuffer(44 + samples * 2);
  const view = new DataView(buffer);
  const text = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i++) view.setUint8(offset + i, value.charCodeAt(i));
  };
  text(0, 'RIFF');
  view.setUint32(4, 36 + samples * 2, true);
  text(8, 'WAVE');
  text(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, 'data');
  view.setUint32(40, samples * 2, true);
  return buffer;
}

function newStamp(path: string): FileStamp {
  const entry = noteOf(path);
  entry.stamp = freshStamp(entry.stamp.len + 1);
  notifyLibraryChanged();
  return { modified: { ...entry.stamp.modified }, len: entry.stamp.len };
}

function checkStamp(path: string, stamp: FileStamp | null): void {
  const entry = noteOf(path);
  if (!stamp || !sameStamp(entry.stamp, stamp)) failure('noteConflict', path);
}

/** Writes a replay recording into the note at `path`, like `Library::save` with the stamp. */
function saveReplay(
  target: Extract<NarrationTarget, { kind: 'replay' }>,
  path: string,
  stamp: FileStamp | null,
  durationMs: number,
  startMs: number,
): NarrationSaved {
  if (mockNarration.saveConflict) failure('noteConflict', path);
  checkStamp(path, stamp);
  const id = noteOf(path).note.id;
  stored.set(id, {
    info: { durationMs, base: 'replay', speed: target.speed, maxPauseMs: target.maxPauseMs, startMs },
  });
  return { stamp: newStamp(path), durationMs, pages: 0 };
}

export function mockNarrationInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'list_microphones':
      return mockNarration.microphones.map((m) => ({ ...m }));
    case 'microphone_monitor':
      begin('monitoring', (args.device as string | null) ?? null, null);
      return undefined;
    case 'microphone_monitor_stop':
      // Only a level-only capture: a recording is never touched.
      if (capture?.kind === 'monitoring') capture = null;
      return undefined;
    case 'narration_start': {
      const target = args.target as NarrationTarget;
      if (target.kind === 'replay') noteOf(target.path);
      if (pending) failure('narrationBusy', 'a recording is waiting to be saved or discarded');
      // A monitor gives way to the recording.
      if (capture?.kind === 'monitoring') capture = null;
      begin('recording', (args.device as string | null) ?? null, target);
      return undefined;
    }
    case 'narration_status':
      return status();
    case 'narration_stop': {
      const current = capture;
      capture = null;
      if (!current || current.kind === 'monitoring') {
        if (args.save && !current) failure('noRecording', 'no recording is running');
        return null;
      }
      if (!args.save) return null;
      if (mockNarration.empty) failure('emptyRecording', 'nothing was recorded');
      const durationMs = Math.max(0, Date.now() - current.startedAt);
      const target = current.target;
      if (target?.kind === 'replay') {
        // The replay instant of the first sample: the recording started this long after the
        // replay was at instant 0 (negative when it started before).
        const at = args.replayStartedAtMs as number | null | undefined;
        const startMs = typeof at === 'number' ? Math.round(current.startedAt - at) : target.startMs;
        try {
          const path = (args.path as string | null) ?? target.path;
          return saveReplay(target, path, (args.stamp as FileStamp | null) ?? null, durationMs, startMs);
        } catch (error) {
          // Kept, not lost: the user retries or discards.
          pending = { target, durationMs, startMs };
          throw error;
        }
      }
      return { stamp: null, durationMs, pages: mockNarration.livePages } satisfies NarrationSaved;
    }
    case 'narration_retry': {
      if (!pending) failure('noPendingNarration', 'no recording is waiting');
      const saved = saveReplay(
        pending.target,
        args.path as string,
        (args.stamp as FileStamp | null) ?? null,
        pending.durationMs,
        pending.startMs,
      );
      pending = null;
      return saved;
    }
    case 'narration_discard':
      if (!pending) failure('noPendingNarration', 'no recording is waiting');
      pending = null;
      return undefined;
    case 'narration_info':
      return mockNarrationOf(args.path as string);
    case 'narration_audio': {
      const info = mockNarrationOf(args.path as string);
      if (!info) failure('noNarration', args.path as string);
      return silentWav(info.durationMs);
    }
    case 'delete_narration': {
      const path = args.path as string;
      checkStamp(path, args.stamp as FileStamp);
      if (!stored.has(noteOf(path).note.id)) failure('noNarration', path);
      stored.delete(noteOf(path).note.id);
      return newStamp(path);
    }
    default:
      return UNHANDLED;
  }
}
