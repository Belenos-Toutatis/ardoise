/** The video export dialog's choices, remembered on this computer between two exports. */
import {
  DEFAULT_HOLD_SECONDS,
  FRAME_RATES,
  SPEEDS,
  VIDEO_FORMATS,
  VIDEO_RESOLUTIONS,
  defaultFrameRate,
  supportsTransparency,
  validHold,
  validTiming,
  type VideoExportOptions,
  type VideoFormat,
  type VideoResolution,
} from './video';

export const DEFAULT_VIDEO_OPTIONS: VideoExportOptions = {
  format: 'mp4',
  resolution: '1080p',
  frameRate: 30,
  timing: { kind: 'speed', speed: 1 },
  trimPauses: false,
  background: true,
  holdEndSeconds: DEFAULT_HOLD_SECONDS,
  narration: true,
};

const KEY = 'ardoise.videoExport';

function defaults(): VideoExportOptions {
  return { ...DEFAULT_VIDEO_OPTIONS, timing: { ...DEFAULT_VIDEO_OPTIONS.timing } };
}

/** The remembered choices, or the defaults for whatever is missing or invalid (or storage blocked). */
export function loadVideoOptions(): VideoExportOptions {
  try {
    const stored = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<VideoExportOptions> | null;
    if (!stored || typeof stored !== 'object') return defaults();
    const base = defaults();
    const format: VideoFormat = VIDEO_FORMATS.includes(stored.format as VideoFormat) ? (stored.format as VideoFormat) : base.format;
    const timing = stored.timing && validTiming(stored.timing) ? stored.timing : base.timing;
    return {
      format,
      resolution: VIDEO_RESOLUTIONS.includes(stored.resolution as VideoResolution) ? (stored.resolution as VideoResolution) : base.resolution,
      frameRate: (FRAME_RATES as readonly number[]).includes(stored.frameRate as number) ? (stored.frameRate as number) : defaultFrameRate(format),
      timing:
        timing.kind === 'speed'
          ? { kind: 'speed', speed: (SPEEDS as readonly number[]).includes(timing.speed) ? timing.speed : 1 }
          : { kind: 'duration', seconds: timing.seconds },
      trimPauses: typeof stored.trimPauses === 'boolean' ? stored.trimPauses : base.trimPauses,
      // Transparency only exists for GIF and WebP.
      background: supportsTransparency(format) && stored.background === false ? false : true,
      holdEndSeconds: validHold(stored.holdEndSeconds) ? stored.holdEndSeconds : base.holdEndSeconds,
      narration: typeof stored.narration === 'boolean' ? stored.narration : base.narration,
    };
  } catch {
    return defaults();
  }
}

export function saveVideoOptions(options: VideoExportOptions): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(options));
  } catch {
    // Nothing to remember it with: the next export starts from the defaults.
  }
}
