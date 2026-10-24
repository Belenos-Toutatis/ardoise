// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { hline, note } from './editor/test-fixtures';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { exportedVideos, mockVideo, resetMockVideo, setMockOpenH264 } from './mock-video';
import type { ExportSource } from './note';
import type { NarrationInfo, VideoExportOptions } from './video';

const OPTIONS: VideoExportOptions = {
  format: 'mp4',
  resolution: '1080p',
  frameRate: 30,
  timing: { kind: 'speed', speed: 1 },
  trimPauses: false,
  background: true,
  holdEndSeconds: 3,
  narration: false,
};

beforeEach(() => {
  resetMockLibrary();
  resetMockVideo();
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** A note of two strokes, 0 to 1 s and 7 to 8 s: a 6 s pause between them, 8 s in all. */
function source(): ExportSource {
  const first = hline(1000, 2000, 1000);
  const second = hline(1000, 2000, 2000);
  first.points.forEach((p, i) => (p.t = i * 100));
  second.points.forEach((p, i) => (p.t = 7000 + i * 100));
  return { path: 'C:\\demo\\x.ardoise', note: note([first, second]) };
}

async function code(promise: Promise<unknown>): Promise<string | undefined> {
  try {
    await promise;
  } catch (error) {
    return (error as { code?: string }).code;
  }
  return undefined;
}

describe('estimate', () => {
  it('turns the replay length, the final frame and the frame rate into frames', async () => {
    const estimate = await api.estimateVideoExport(source(), OPTIONS);
    expect(estimate).toMatchObject({ width: 1920, height: 1080, speed: 1, narrationMs: null });
    expect(estimate.durationMs).toBe(11_000);
    expect(estimate.frames).toBe(330);
  });

  it('divides the replay by the speed, but not the final frame', async () => {
    const s = source();
    const normal = await api.estimateVideoExport(s, OPTIONS);
    const fast = await api.estimateVideoExport(s, { ...OPTIONS, timing: { kind: 'speed', speed: 4 } });
    expect(fast.durationMs).toBe((normal.durationMs - 3000) / 4 + 3000);
  });

  it('cuts the pauses longer than half a second', async () => {
    const s = source();
    const full = await api.estimateVideoExport(s, { ...OPTIONS, holdEndSeconds: 0 });
    const trimmed = await api.estimateVideoExport(s, { ...OPTIONS, holdEndSeconds: 0, trimPauses: true });
    // The 6 s gap between the two strokes keeps 0.5 s: 5.5 s are cut.
    expect(full.durationMs).toBe(8000);
    expect(trimmed.durationMs).toBe(2500);
  });

  it('reaches a target duration by changing the speed, within 0.25× to 16×', async () => {
    const s = source();
    const replay = (await api.estimateVideoExport(s, { ...OPTIONS, holdEndSeconds: 0 })).durationMs;
    const twice = await api.estimateVideoExport(s, {
      ...OPTIONS,
      holdEndSeconds: 0,
      timing: { kind: 'duration', seconds: replay / 2000 },
    });
    expect(twice.speed).toBeCloseTo(2, 5);
    const clamped = await api.estimateVideoExport(s, {
      ...OPTIONS,
      holdEndSeconds: 0,
      timing: { kind: 'duration', seconds: 3600 },
    });
    expect(clamped.speed).toBe(0.25);
    const tiny = await api.estimateVideoExport(s, { ...OPTIONS, timing: { kind: 'duration', seconds: 1 } });
    expect(tiny.speed).toBeLessThanOrEqual(16);
  });

  it('sizes the frame: fixed resolutions, or the page\'s proportions', async () => {
    const s = source();
    const size = async (resolution: VideoExportOptions['resolution']) => {
      const e = await api.estimateVideoExport(s, { ...OPTIONS, resolution });
      return [e.width, e.height];
    };
    expect(await size('720p')).toEqual([1280, 720]);
    expect(await size('4k')).toEqual([3840, 2160]);
    // A4 landscape (29700 × 21600): 1920 × 1396.
    expect(await size('page')).toEqual([1920, 1396]);
  });

  it('estimates the export time from the encoder throughputs scaled by the pixels', async () => {
    const s = source();
    const time = async (format: VideoExportOptions['format'], resolution: VideoExportOptions['resolution']) => {
      const e = await api.estimateVideoExport(s, { ...OPTIONS, format, resolution, frameRate: 30 });
      return { frames: e.frames, ms: e.exportMs };
    };
    const mp4 = await time('mp4', '1080p');
    // 0.5 s fixed, then frames / 100 fps.
    expect(mp4.ms).toBe(Math.round(500 + (mp4.frames / 100) * 1000));
    const webm = await time('webm', '1080p');
    expect(webm.ms).toBe(Math.round(500 + (webm.frames / 20) * 1000));
    // 4K has four times the pixels: four times slower than 1080p, apart from the fixed part.
    const webm4k = await time('webm', '4k');
    expect((webm4k.ms - 500) / (webm.ms - 500)).toBeCloseTo(4, 2);
    // A long WebM at 4K takes over two minutes: the dialog warns.
    const long = await api.estimateVideoExport(s, { ...OPTIONS, format: 'webm', resolution: '4k', holdEndSeconds: 60 });
    expect(long.exportMs).toBeGreaterThan(120_000);
    const gif = await time('gif', '1080p');
    expect(gif.ms).toBeLessThan(webm.ms);
  });

  it('follows the narration: its speed, its pauses, and its end when it outlasts the replay', async () => {
    const s = source();
    const narration: NarrationInfo = { durationMs: 30_000, base: 'replay', speed: 2, maxPauseMs: 500, startMs: 0 };
    mockVideo.narrations[s.path] = narration;
    const estimate = await api.estimateVideoExport(s, { ...OPTIONS, narration: true, timing: { kind: 'speed', speed: 8 } });
    expect(estimate.speed).toBe(2);
    expect(estimate.narrationMs).toBe(30_000);
    // The narration is the longest part of the video (replay 1.25 s, final frame 3 s).
    expect(estimate.durationMs).toBe(30_000);
    // A format without sound ignores it.
    const gif = await api.estimateVideoExport(s, { ...OPTIONS, format: 'gif', frameRate: 15, narration: true });
    expect(gif.narrationMs).toBeNull();
    expect(gif.speed).toBe(1);
  });

  it('starts with blank page when the narration began before the note', async () => {
    const s = source();
    mockVideo.narrations[s.path] = { durationMs: 2000, base: 'note', speed: 1, maxPauseMs: null, startMs: -1500 };
    const withNarration = await api.estimateVideoExport(s, { ...OPTIONS, narration: true, holdEndSeconds: 0 });
    const without = await api.estimateVideoExport(s, { ...OPTIONS, holdEndSeconds: 0 });
    expect(withNarration.durationMs).toBe(without.durationMs + 1500);
  });

  it('reads the note of the library when the source carries none', async () => {
    const path = [...mockLibrary().notes.keys()][0];
    const estimate = await api.estimateVideoExport({ path }, OPTIONS);
    expect(estimate.frames).toBeGreaterThan(0);
    expect(await code(api.estimateVideoExport({ path: 'C:\\nope.ardoise' }, OPTIONS))).toBe('noteNotFound');
  });

  it('refuses invalid settings with invalidVideoSettings', async () => {
    const s = source();
    const bad: Partial<VideoExportOptions>[] = [
      { frameRate: 7 },
      { resolution: '8k' as never },
      { format: 'avi' as never },
      { holdEndSeconds: 61 },
      { holdEndSeconds: -1 },
      { timing: { kind: 'speed', speed: 32 } },
      { timing: { kind: 'duration', seconds: 0 } },
      // Transparency exists for GIF and WebP only.
      { background: false },
      { format: 'webm', background: false },
    ];
    for (const change of bad) {
      expect(await code(api.estimateVideoExport(s, { ...OPTIONS, ...change })), JSON.stringify(change)).toBe('invalidVideoSettings');
    }
    expect(await code(api.estimateVideoExport(s, { ...OPTIONS, format: 'gif', frameRate: 15, background: false }))).toBeUndefined();
    expect(await code(api.estimateVideoExport(s, { ...OPTIONS, format: 'webp', frameRate: 15, background: false }))).toBeUndefined();
  });
});

describe('export', () => {
  it('returns the path, the frames, the length and whether there is sound', async () => {
    const s = source();
    const progress: number[] = [];
    const result = await api.exportVideo(s, OPTIONS, 'C:\\out\\a.mp4', (p) => progress.push(p.framesDone));
    const estimate = await api.estimateVideoExport(s, OPTIONS);
    expect(result).toEqual({
      path: 'C:\\out\\a.mp4',
      frames: estimate.frames,
      durationMs: estimate.durationMs,
      audio: false,
      encoder: 'Media Foundation',
    });
    // Without a delay, one event at the end.
    expect(progress).toEqual([estimate.frames]);
    expect(exportedVideos).toEqual([result]);
  });

  it('sends progress events over the delay, increasing up to the last frame', async () => {
    mockVideo.delayMs = 200;
    mockVideo.steps = 4;
    const events: { framesDone: number; framesTotal: number }[] = [];
    const result = await api.exportVideo(source(), OPTIONS, 'C:\\out\\a.mp4', (p) => events.push(p));
    expect(events).toHaveLength(4);
    expect(events.map((e) => e.framesDone)).toEqual([...events.map((e) => e.framesDone)].sort((a, b) => a - b));
    expect(events[0].framesDone).toBeGreaterThan(0);
    expect(events[0].framesDone).toBeLessThan(result.frames);
    expect(events[3]).toMatchObject({ framesDone: result.frames, framesTotal: result.frames });
  });

  it('is cancelled by cancel_video_export: it rejects with cancelled and records nothing', async () => {
    mockVideo.delayMs = 5000;
    const pending = api.exportVideo(source(), OPTIONS, 'C:\\out\\a.mp4', () => {});
    await Promise.resolve();
    await api.cancelVideoExport();
    expect(await code(pending)).toBe('cancelled');
    expect(exportedVideos).toEqual([]);
    // Cancelling with nothing running is harmless.
    await api.cancelVideoExport();
  });

  it('has sound when the narration is included', async () => {
    const s = source();
    mockVideo.narrations[s.path] = { durationMs: 5000, base: 'replay', speed: 1, maxPauseMs: null, startMs: 0 };
    expect((await api.exportVideo(s, { ...OPTIONS, narration: true }, 'C:\\a.mp4', () => {})).audio).toBe(true);
    expect((await api.exportVideo(s, { ...OPTIONS, narration: false }, 'C:\\a.mp4', () => {})).audio).toBe(false);
    expect((await api.exportVideo(s, { ...OPTIONS, format: 'gif', frameRate: 15, narration: true }, 'C:\\a.gif', () => {})).audio).toBe(false);
  });

  it('refuses invalid settings', async () => {
    expect(await code(api.exportVideo(source(), { ...OPTIONS, frameRate: 7 }, 'C:\\a.mp4', () => {}))).toBe('invalidVideoSettings');
  });

  it('cannot write MP4 without Media Foundation, unless the backup is installed and allowed', async () => {
    const write = (format: VideoExportOptions['format'] = 'mp4') =>
      api.exportVideo(source(), { ...OPTIONS, format }, `C:\\a.${format}`, () => {});
    mockVideo.mediaFoundation = false;
    expect(await code(write())).toBe('codecUnavailable');
    // The other formats do not need it.
    expect(await code(write('webm'))).toBeUndefined();
    setMockOpenH264({ status: 'installed', bytes: 1000, allowed: false });
    expect(await code(write())).toBe('codecUnavailable');
    await api.setOpenH264Allowed(true);
    expect((await write()).encoder).toBe('OpenH264');
  });
});

describe('OpenH264', () => {
  it('starts not installed, with the Cisco notice and a licence', async () => {
    const state = await api.openH264State();
    expect(state).toMatchObject({ status: 'notInstalled', bytes: null, allowed: false, mediaFoundation: true, version: '2.6.0' });
    expect(state.notice).toBe('OpenH264 Video Codec provided by Cisco Systems, Inc.');
    expect(state.license).toContain('Cisco');
  });

  it('downloads with progress, then is installed and allowed', async () => {
    mockVideo.delayMs = 100;
    mockVideo.steps = 2;
    const events: number[] = [];
    const state = await api.openH264Download((p) => {
      events.push(p.received);
      expect(p.total).toBe(860_000);
    });
    expect(events).toEqual([430_000, 860_000]);
    expect(state).toMatchObject({ status: 'installed', bytes: 860_000, allowed: true });
    expect(await api.openH264State()).toEqual(state);
  });

  it('can be cancelled during the download', async () => {
    mockVideo.delayMs = 5000;
    const pending = api.openH264Download(() => {});
    await Promise.resolve();
    await api.openH264CancelDownload();
    expect(await code(pending)).toBe('cancelled');
    expect((await api.openH264State()).status).toBe('notInstalled');
  });

  it('can fail', async () => {
    mockVideo.downloadFails = true;
    expect(await code(api.openH264Download(() => {}))).toBe('openh264DownloadFailed');
  });

  it('is allowed, disallowed, and removed (which disallows it)', async () => {
    await api.openH264Download(() => {});
    expect((await api.setOpenH264Allowed(false)).allowed).toBe(false);
    expect((await api.setOpenH264Allowed(true)).allowed).toBe(true);
    const removed = await api.openH264Remove();
    expect(removed).toMatchObject({ status: 'notInstalled', bytes: null, allowed: false });
  });

  it('reports whether Windows writes MP4 itself', async () => {
    mockVideo.mediaFoundation = false;
    expect((await api.openH264State()).mediaFoundation).toBe(false);
  });
});
