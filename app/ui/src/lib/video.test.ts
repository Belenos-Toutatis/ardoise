import { describe, expect, it } from 'vitest';
import {
  FRAME_RATES,
  frameSize,
  megabytes,
  roundEstimate,
  splitSeconds,
  supportsSound,
  supportsTransparency,
  validHold,
  validTiming,
  defaultFrameRate,
  VIDEO_FORMATS,
} from './video';

describe('what each format can do', () => {
  it('has sound for MP4 and WebM only, transparency for GIF and WebP only', () => {
    expect(VIDEO_FORMATS.filter(supportsSound)).toEqual(['mp4', 'webm']);
    expect(VIDEO_FORMATS.filter(supportsTransparency)).toEqual(['gif', 'webp']);
  });

  it('proposes 30 images per second for the videos and 15 for the animated images', () => {
    expect(VIDEO_FORMATS.map(defaultFrameRate)).toEqual([30, 15, 15, 30]);
    for (const format of VIDEO_FORMATS) expect(FRAME_RATES).toContain(defaultFrameRate(format));
  });
});

describe('validation', () => {
  it('accepts a final frame held 0 to 60 seconds', () => {
    expect([0, 3, 60, 0.5].every(validHold)).toBe(true);
    expect([-1, 60.1, NaN, Infinity, null, undefined].some(validHold)).toBe(false);
  });

  it('accepts speeds from 0.25 to 16 and target durations from 1 to 3600 seconds', () => {
    expect(validTiming({ kind: 'speed', speed: 0.25 })).toBe(true);
    expect(validTiming({ kind: 'speed', speed: 16 })).toBe(true);
    expect(validTiming({ kind: 'speed', speed: 0.2 })).toBe(false);
    expect(validTiming({ kind: 'speed', speed: 17 })).toBe(false);
    expect(validTiming({ kind: 'duration', seconds: 1 })).toBe(true);
    expect(validTiming({ kind: 'duration', seconds: 3600 })).toBe(true);
    expect(validTiming({ kind: 'duration', seconds: 0 })).toBe(false);
    expect(validTiming({ kind: 'duration', seconds: 3601 })).toBe(false);
    expect(validTiming({ kind: 'duration', seconds: NaN })).toBe(false);
  });
});

describe('frame size', () => {
  const A4 = { width: 29700, height: 21600 };

  it('has the fixed sizes', () => {
    expect(frameSize('720p', A4)).toEqual({ width: 1280, height: 720 });
    expect(frameSize('1080p', A4)).toEqual({ width: 1920, height: 1080 });
    expect(frameSize('4k', A4)).toEqual({ width: 3840, height: 2160 });
  });

  it('follows the page: 1920 on the long side, an even short side', () => {
    expect(frameSize('page', A4)).toEqual({ width: 1920, height: 1396 });
    expect(frameSize('page', { width: 21600, height: 29700 })).toEqual({ width: 1396, height: 1920 });
    expect(frameSize('page', { width: 10000, height: 10000 })).toEqual({ width: 1920, height: 1920 });
    const wide = frameSize('page', { width: 300_000, height: 21600 });
    expect(wide.width).toBe(1920);
    expect(wide.height % 2).toBe(0);
    expect(wide.height).toBeGreaterThanOrEqual(2);
  });
});

describe('times and sizes shown', () => {
  it('splits a length into minutes and seconds, rounded to the second', () => {
    expect(splitSeconds(0)).toEqual({ minutes: 0, seconds: 0 });
    expect(splitSeconds(45_400)).toEqual({ minutes: 0, seconds: 45 });
    expect(splitSeconds(65_000)).toEqual({ minutes: 1, seconds: 5 });
    expect(splitSeconds(119_600)).toEqual({ minutes: 2, seconds: 0 });
    expect(splitSeconds(-5)).toEqual({ minutes: 0, seconds: 0 });
  });

  it('rounds an estimate to the nearest 5 seconds from 10 seconds', () => {
    expect(roundEstimate(3400)).toBe(3400);
    expect(roundEstimate(14_300)).toBe(15_000);
    expect(roundEstimate(196_000)).toBe(195_000);
  });

  it('shows megabytes with one decimal', () => {
    expect(megabytes(860_000)).toBe(0.9);
    expect(megabytes(1_449_999)).toBe(1.4);
    expect(megabytes(0)).toBe(0);
  });
});
