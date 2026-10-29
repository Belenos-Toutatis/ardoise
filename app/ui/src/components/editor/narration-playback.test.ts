import { describe, expect, it } from 'vitest';
import { NarrationPlayback, narrationType, type AudioLike } from './narration-playback';

/** An audio element that only remembers what it was told. */
function fakeAudio(duration = 10) {
  const audio = {
    currentTime: 0,
    duration,
    paused: true,
    plays: 0,
    pauses: 0,
    play() {
      this.paused = false;
      this.plays += 1;
      return Promise.resolve();
    },
    pause() {
      this.paused = true;
      this.pauses += 1;
    },
  };
  return audio as AudioLike & typeof audio;
}

describe('narration playback', () => {
  it('starts the sound with the replay and keeps it in step', () => {
    const audio = fakeAudio();
    const sync = new NarrationPlayback(audio, 0);
    sync.sync(0, true);
    expect(audio.plays).toBe(1);
    expect(audio.paused).toBe(false);
    // The sound runs by itself: small differences are left alone.
    audio.currentTime = 1.02;
    sync.sync(1000, true);
    expect(audio.currentTime).toBe(1.02);
    expect(audio.plays).toBe(1);
    // A seek puts it back in place.
    sync.sync(6000, true);
    expect(audio.currentTime).toBe(6);
  });

  it('follows pause and seeking while paused', () => {
    const audio = fakeAudio();
    const sync = new NarrationPlayback(audio, 0);
    sync.sync(2000, true);
    sync.sync(2500, false);
    expect(audio.paused).toBe(true);
    expect(audio.pauses).toBe(1);
    sync.sync(4000, false);
    expect(audio.currentTime).toBe(4);
    expect(audio.pauses).toBe(1);
    sync.sync(4000, true);
    expect(audio.paused).toBe(false);
    expect(audio.currentTime).toBe(4);
  });

  it('waits for a narration that starts after the replay and starts at once for one that began before it', () => {
    const late = fakeAudio();
    const afterwards = new NarrationPlayback(late, 2000);
    afterwards.sync(500, true);
    expect(late.paused).toBe(true);
    expect(late.currentTime).toBe(0);
    afterwards.sync(2000, true);
    expect(late.paused).toBe(false);
    afterwards.sync(3500, true);
    expect(late.currentTime).toBe(1.5);

    const early = fakeAudio();
    const before = new NarrationPlayback(early, -1500);
    before.sync(0, true);
    expect(early.paused).toBe(false);
    expect(early.currentTime).toBe(1.5);
  });

  it('stops at the end of the sound, and tolerates sounds that refuse to start', async () => {
    const audio = fakeAudio(3);
    const sync = new NarrationPlayback(audio, 0);
    sync.sync(2000, true);
    expect(audio.paused).toBe(false);
    sync.sync(3500, true);
    expect(audio.paused).toBe(true);
    expect(audio.currentTime).toBe(3);

    const refusing = fakeAudio();
    refusing.play = () => Promise.reject(new Error('NotAllowedError'));
    await expect(Promise.resolve(new NarrationPlayback(refusing, 0).sync(0, true))).resolves.toBeUndefined();
  });

  it('can be silenced', () => {
    const audio = fakeAudio();
    const sync = new NarrationPlayback(audio, 0);
    sync.sync(0, true);
    sync.stop();
    expect(audio.paused).toBe(true);
    sync.stop();
    expect(audio.pauses).toBe(1);
  });
});

describe('narrationType', () => {
  const bytes = (text: string) => new TextEncoder().encode(text).buffer as ArrayBuffer;

  it('plays the stored Ogg Opus stream, and the WAV of the browser stand-in', () => {
    expect(narrationType(bytes('OggS rest of the page'))).toBe('audio/ogg; codecs=opus');
    expect(narrationType(bytes('RIFF....WAVEfmt '))).toBe('audio/wav');
    expect(narrationType(new ArrayBuffer(0))).toBe('audio/wav');
    expect(narrationType(bytes('Ogg'))).toBe('audio/wav');
  });
});
