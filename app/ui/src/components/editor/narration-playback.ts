/**
 * Plays a note's narration along with its replay. The narration is in the replay time of the
 * options it was recorded with (docs/design/media.md), so while the replay uses those options the
 * sound at replay instant `r` is at `(r - startMs) / 1000` seconds into the audio: it starts when
 * the replay reaches `startMs` (at once when that is negative: the sound began before the
 * drawing), and stops by itself at its end.
 */

/** How far the sound may drift from the replay before it is put back in place, seconds. */
const DRIFT_S = 0.25;

/** Small differences of position are not worth a seek, seconds. */
const SEEK_EPSILON_S = 0.05;

/** The part of `HTMLAudioElement` used, so that tests can stand in for it. */
export type AudioLike = Pick<HTMLAudioElement, 'currentTime' | 'duration' | 'paused' | 'play' | 'pause'>;

export class NarrationPlayback {
  readonly #audio: AudioLike;
  readonly #startMs: number;

  constructor(audio: AudioLike, startMs: number) {
    this.#audio = audio;
    this.#startMs = startMs;
  }

  /** Puts the sound where the replay is (`positionMs`), playing or not. Call at every frame. */
  sync(positionMs: number, playing: boolean): void {
    const audio = this.#audio;
    const wanted = (positionMs - this.#startMs) / 1000;
    const length = Number.isFinite(audio.duration) ? audio.duration : Infinity;
    if (!playing || wanted < 0 || wanted >= length) {
      if (!audio.paused) audio.pause();
      // Ready to start from the right place.
      const place = Math.min(Math.max(wanted, 0), Number.isFinite(length) ? length : Math.max(wanted, 0));
      if (Math.abs(audio.currentTime - place) > SEEK_EPSILON_S) audio.currentTime = place;
      return;
    }
    if (Math.abs(audio.currentTime - wanted) > (audio.paused ? SEEK_EPSILON_S : DRIFT_S)) {
      audio.currentTime = wanted;
    }
    if (audio.paused) {
      // The browser may refuse to start sound without a gesture: the replay then goes on silently.
      void Promise.resolve(audio.play()).catch(() => {});
    }
  }

  /** Silences the sound (the narration is switched off, the replay is left). */
  stop(): void {
    if (!this.#audio.paused) this.#audio.pause();
  }
}
