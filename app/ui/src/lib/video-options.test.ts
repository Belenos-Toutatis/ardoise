// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_VIDEO_OPTIONS, loadVideoOptions, saveVideoOptions } from './video-options';
import type { VideoExportOptions } from './video';

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

const GIF: VideoExportOptions = {
  format: 'gif',
  resolution: '720p',
  frameRate: 10,
  timing: { kind: 'duration', seconds: 20 },
  trimPauses: true,
  background: false,
  holdEndSeconds: 7,
  narration: false,
};

describe('remembered video choices', () => {
  it('starts with MP4 1080p at 30 images per second, the final frame held 3 s', () => {
    expect(loadVideoOptions()).toEqual(DEFAULT_VIDEO_OPTIONS);
    expect(DEFAULT_VIDEO_OPTIONS).toMatchObject({ format: 'mp4', resolution: '1080p', frameRate: 30, holdEndSeconds: 3 });
  });

  it('gives back what was saved', () => {
    saveVideoOptions(GIF);
    expect(loadVideoOptions()).toEqual(GIF);
    saveVideoOptions({ ...GIF, format: 'webp', timing: { kind: 'speed', speed: 4 } });
    expect(loadVideoOptions()).toMatchObject({ format: 'webp', timing: { kind: 'speed', speed: 4 } });
  });

  it('never hands out the default object itself', () => {
    const first = loadVideoOptions();
    first.timing = { kind: 'speed', speed: 16 };
    expect(loadVideoOptions().timing).toEqual({ kind: 'speed', speed: 1 });
  });

  it('replaces each invalid value by its default', () => {
    localStorage.setItem(
      'ardoise.videoExport',
      JSON.stringify({
        format: 'avi',
        resolution: '8k',
        frameRate: 7,
        timing: { kind: 'speed', speed: 99 },
        trimPauses: 'yes',
        background: 'no',
        holdEndSeconds: 99,
        narration: 1,
      }),
    );
    expect(loadVideoOptions()).toEqual(DEFAULT_VIDEO_OPTIONS);
  });

  it('keeps the valid values next to invalid ones', () => {
    localStorage.setItem('ardoise.videoExport', JSON.stringify({ format: 'webm', resolution: '4k', frameRate: 'x' }));
    expect(loadVideoOptions()).toMatchObject({ format: 'webm', resolution: '4k', frameRate: 30 });
    localStorage.setItem('ardoise.videoExport', JSON.stringify({ format: 'gif', frameRate: 99 }));
    // The frame rate default follows the format.
    expect(loadVideoOptions().frameRate).toBe(15);
  });

  it('keeps a speed that is not among the offered ones out', () => {
    localStorage.setItem('ardoise.videoExport', JSON.stringify({ timing: { kind: 'speed', speed: 3 } }));
    expect(loadVideoOptions().timing).toEqual({ kind: 'speed', speed: 1 });
  });

  it('forgets a transparent background for a format that has none', () => {
    localStorage.setItem('ardoise.videoExport', JSON.stringify({ format: 'mp4', background: false }));
    expect(loadVideoOptions().background).toBe(true);
    localStorage.setItem('ardoise.videoExport', JSON.stringify({ format: 'webp', background: false }));
    expect(loadVideoOptions().background).toBe(false);
  });

  it('survives garbage and blocked storage', () => {
    localStorage.setItem('ardoise.videoExport', '{not json');
    expect(loadVideoOptions()).toEqual(DEFAULT_VIDEO_OPTIONS);
    localStorage.setItem('ardoise.videoExport', '42');
    expect(loadVideoOptions()).toEqual(DEFAULT_VIDEO_OPTIONS);
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(loadVideoOptions()).toEqual(DEFAULT_VIDEO_OPTIONS);
    expect(() => saveVideoOptions(GIF)).not.toThrow();
  });
});
