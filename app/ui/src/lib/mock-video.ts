/**
 * Browser stand-in for the video export and OpenH264 commands (`estimate_video_export`,
 * `export_video`, `cancel_video_export`, `openh264_*`, `set_openh264_allowed`). Nothing is
 * written: the numbers come from the same model as `ardoise_media::export::plan_export` and
 * `estimate::estimate_export_time` (docs/design/media.md), over the demo library.
 */
import { UNHANDLED, mockLibrary } from './mock-data';
import { failure } from './mock-library';
import { mockNarrationInvoke } from './mock-narration';
import type { ExportSource, Note } from './note';
import {
  FRAME_RATES,
  MAX_SPEED,
  MIN_SPEED,
  TRIMMED_PAUSE_MS,
  VIDEO_FORMATS,
  VIDEO_RESOLUTIONS,
  frameSize,
  supportsSound,
  supportsTransparency,
  validHold,
  validTiming,
  type DownloadProgress,
  type NarrationInfo,
  type OpenH264State,
  type VideoEstimate,
  type VideoExportOptions,
  type VideoExportResult,
  type VideoFormat,
  type VideoProgress,
} from './video';

/** What a test (or a developer) can switch in the browser stand-in. */
export const mockVideo = {
  /** How long an export or a download takes, milliseconds; 0 answers at once. */
  delayMs: 0,
  /** Progress events during a delayed export or download. */
  steps: 5,
  /** Windows Media Foundation writes MP4 on this PC (false: an « N » edition without the pack). */
  mediaFoundation: true,
  /** The Cisco download fails (`openh264DownloadFailed`). */
  downloadFails: false,
  /** Narrations by note path, for notes whose narration the tests invent. */
  narrations: {} as Record<string, NarrationInfo>,
};

/** Notes the stand-in has exported, in order (path of the file and its result). */
export const exportedVideos: VideoExportResult[] = [];

export const OPENH264_NOTICE = 'OpenH264 Video Codec provided by Cisco Systems, Inc.';
const OPENH264_LICENSE =
  'OpenH264 binary licence (placeholder of the browser stand-in).\n\n' +
  'Copyright (c) 2014, Cisco Systems. All rights reserved.\n' +
  'Redistribution and use in binary form, without modification, are permitted provided that ' +
  'the licence conditions of Cisco are reproduced.';
const OPENH264_SIZE = 860_000;

function initialOpenH264(): OpenH264State {
  return {
    status: 'notInstalled',
    bytes: null,
    allowed: false,
    mediaFoundation: true,
    version: '2.6.0',
    notice: OPENH264_NOTICE,
    license: OPENH264_LICENSE,
  };
}

let openH264 = initialOpenH264();
/** Settles the export or the download under way with `cancelled`; null while none is running. */
let cancelExportRunning: (() => void) | null = null;
let cancelDownloadRunning: (() => void) | null = null;

/** Back to the defaults (the tests call it between two cases). */
export function resetMockVideo(): void {
  mockVideo.delayMs = 0;
  mockVideo.steps = 5;
  mockVideo.mediaFoundation = true;
  mockVideo.downloadFails = false;
  mockVideo.narrations = {};
  exportedVideos.length = 0;
  openH264 = initialOpenH264();
  cancelExportRunning = null;
  cancelDownloadRunning = null;
}

/** Changes the OpenH264 state directly (a damaged file, an installed codec…). */
export function setMockOpenH264(patch: Partial<OpenH264State>): void {
  openH264 = { ...openH264, ...patch };
}

function openH264State(): OpenH264State {
  return { ...openH264, mediaFoundation: mockVideo.mediaFoundation };
}

// --- Estimate ---------------------------------------------------------------------------------

/** Frames per second at 1080p of each format's encoder (docs/design/media.md, `estimate.rs`). */
const THROUGHPUT_1080P: Record<VideoFormat, number> = { gif: 310, webp: 24, webm: 20, mp4: 100 };
const PIXELS_1080P = 1920 * 1080;

function sourceNote(source: ExportSource): Note {
  return source.note ?? mockLibrary().notes.get(source.path)?.note ?? failure('noteNotFound', source.path);
}

/** The narration of a note: invented by a test, else the browser stand-in of the narration. */
async function narrationOf(path: string): Promise<NarrationInfo | null> {
  const given = mockVideo.narrations[path];
  if (given) return given;
  try {
    const found = await mockNarrationInvoke('narration_info', { path });
    return found === UNHANDLED || found === undefined ? null : (found as NarrationInfo | null);
  } catch {
    return null;
  }
}

/**
 * Length of the replay at speed 1, milliseconds: the end of the last stroke, minus the pauses
 * cut when `maxPauseMs` is given (the gaps between two strokes, running maximum of their ends).
 */
function replayLength(note: Note, maxPauseMs: number | null): number {
  const spans = note.strokes
    .filter((stroke) => note.layers[stroke.layer]?.visible && stroke.points.length > 0)
    .map((stroke) => [stroke.points[0].t, stroke.points[stroke.points.length - 1].t] as const)
    .sort((a, b) => a[0] - b[0]);
  let end = 0;
  let cut = 0;
  for (const [start, stop] of spans) {
    if (maxPauseMs !== null && start - end > maxPauseMs) cut += start - end - maxPauseMs;
    end = Math.max(end, stop);
  }
  return end - cut;
}

function checkOptions(options: VideoExportOptions): void {
  const bad = (what: string) => failure('invalidVideoSettings', what);
  if (!VIDEO_FORMATS.includes(options.format)) bad(`format ${options.format}`);
  if (!VIDEO_RESOLUTIONS.includes(options.resolution)) bad(`resolution ${options.resolution}`);
  if (!(FRAME_RATES as readonly number[]).includes(options.frameRate)) bad(`frame rate ${options.frameRate}`);
  if (!validTiming(options.timing)) bad('timing');
  if (!validHold(options.holdEndSeconds)) bad(`hold ${options.holdEndSeconds}`);
  if (!options.background && !supportsTransparency(options.format)) bad(`transparent ${options.format}`);
}

async function estimate(source: ExportSource, options: VideoExportOptions): Promise<VideoEstimate> {
  checkOptions(options);
  const note = sourceNote(source);
  const narration = options.narration && supportsSound(options.format) ? await narrationOf(source.path) : null;
  // A narration imposes the replay options it was recorded with.
  const maxPause = narration ? narration.maxPauseMs : options.trimPauses ? TRIMMED_PAUSE_MS : null;
  const length = replayLength(note, maxPause);
  const speed = narration
    ? narration.speed
    : options.timing.kind === 'speed'
      ? options.timing.speed
      : Math.min(MAX_SPEED, Math.max(MIN_SPEED, length / (options.timing.seconds * 1000) || 1));
  const replayMs = length / speed;
  const leadIn = narration ? Math.max(0, -narration.startMs) : 0;
  const pastEnd = narration ? Math.max(0, narration.startMs + narration.durationMs - replayMs) : 0;
  const durationMs = leadIn + replayMs + Math.max(options.holdEndSeconds * 1000, pastEnd);
  const frames = Math.max(1, Math.ceil((durationMs / 1000) * options.frameRate));
  const { width, height } = frameSize(options.resolution, note.page);
  const framesPerSecond = THROUGHPUT_1080P[options.format] * (PIXELS_1080P / (width * height));
  return {
    width,
    height,
    frames,
    durationMs: Math.round(durationMs),
    exportMs: Math.round(500 + (frames / framesPerSecond) * 1000),
    speed,
    narrationMs: narration ? narration.durationMs : null,
  };
}

// --- Export -----------------------------------------------------------------------------------

/** Runs `step(i)` for i = 1..steps, `delayMs / steps` apart; `cancel` ends it with `cancelled`. */
function paced(steps: number, step: (i: number) => void, assign: (cancel: (() => void) | null) => void): Promise<void> {
  if (mockVideo.delayMs <= 0) {
    step(steps);
    return Promise.resolve();
  }
  return new Promise<void>((resolve, reject) => {
    let done = 0;
    const timer = setInterval(() => {
      done += 1;
      step(done);
      if (done >= steps) {
        clearInterval(timer);
        assign(null);
        resolve();
      }
    }, mockVideo.delayMs / steps);
    assign(() => {
      clearInterval(timer);
      assign(null);
      reject({ code: 'cancelled', message: 'cancelled' });
    });
  });
}

/** MP4 needs Windows' encoder, or the OpenH264 fallback installed and allowed. */
function mp4Available(): boolean {
  return mockVideo.mediaFoundation || (openH264.status === 'installed' && openH264.allowed);
}

export async function mockVideoExport(
  source: ExportSource,
  options: VideoExportOptions,
  destination: string,
  onProgress: (progress: VideoProgress) => void,
): Promise<VideoExportResult> {
  const planned = await estimate(source, options);
  if (options.format === 'mp4' && !mp4Available()) failure('codecUnavailable', 'no H.264 encoder');
  const steps = Math.max(1, mockVideo.steps);
  const started = Date.now();
  await paced(
    steps,
    (i) =>
      onProgress({
        framesDone: i >= steps ? planned.frames : Math.floor((planned.frames * i) / steps),
        framesTotal: planned.frames,
        elapsedMs: Date.now() - started,
      }),
    (cancel) => (cancelExportRunning = cancel),
  );
  const result: VideoExportResult = {
    path: destination,
    frames: planned.frames,
    durationMs: planned.durationMs,
    audio: planned.narrationMs !== null,
    encoder: options.format === 'mp4' ? (mockVideo.mediaFoundation ? 'Media Foundation' : 'OpenH264') : options.format,
  };
  exportedVideos.push(result);
  return result;
}

// --- OpenH264 ---------------------------------------------------------------------------------

export async function mockOpenH264Download(onProgress: (progress: DownloadProgress) => void): Promise<OpenH264State> {
  if (mockVideo.downloadFails) failure('openh264DownloadFailed', 'ciscobinary.openh264.org: connection refused');
  const steps = Math.max(1, mockVideo.steps);
  await paced(
    steps,
    (i) => onProgress({ received: Math.floor((OPENH264_SIZE * i) / steps), total: OPENH264_SIZE }),
    (cancel) => (cancelDownloadRunning = cancel),
  );
  openH264 = { ...openH264, status: 'installed', bytes: OPENH264_SIZE, allowed: true };
  return openH264State();
}

export function mockVideoInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'estimate_video_export':
      return estimate(args.source as ExportSource, args.options as VideoExportOptions);
    case 'cancel_video_export':
      cancelExportRunning?.();
      return undefined;
    case 'openh264_state':
      return openH264State();
    case 'openh264_cancel_download':
      cancelDownloadRunning?.();
      return undefined;
    case 'openh264_remove':
      openH264 = { ...openH264, status: 'notInstalled', bytes: null, allowed: false };
      return openH264State();
    case 'set_openh264_allowed':
      openH264 = { ...openH264, allowed: args.allowed as boolean };
      return openH264State();
    default:
      return UNHANDLED;
  }
}
