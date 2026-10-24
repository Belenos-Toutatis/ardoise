/** Small helpers shared by the replay bar, the microphone picker and the recording panels. */

/** `m:ss` (`h:mm:ss` from one hour) of a length in milliseconds, rounded down to the second. */
export function clock(ms: number): string {
  const total = Math.floor(Math.max(0, Number.isFinite(ms) ? ms : 0) / 1000);
  const seconds = String(total % 60).padStart(2, '0');
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}:${seconds}`;
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}:${seconds}`;
}

/** Lowest level the meters show, dBFS: quieter reads as empty. */
const FLOOR_DB = -60;

/**
 * Where a linear level (0..1) sits on a meter, in percent: on a decibel scale from -60 dBFS to 0,
 * as a speaking voice (around -25 dBFS) would hardly move a linear one.
 */
export function levelPercent(level: number): number {
  if (!(level > 0)) return 0;
  const db = 20 * Math.log10(Math.min(level, 1));
  return Math.round(Math.max(0, Math.min(100, ((db - FLOOR_DB) / -FLOOR_DB) * 100)));
}
