<script lang="ts">
  /**
   * The replay's controls (SPEC §4.5): play and pause, the timeline (click, drag or keys to seek),
   * the speed, "supprimer les temps morts", and, when the note has a narration, playing it along
   * (which imposes the speed and the pauses it was recorded with). Also holds the `<audio>` of the
   * narration and keeps it in step with the playhead. `locked`: a recording is under way, so the
   * replay runs on its own (seeking or changing the options would put the sound out of step).
   */
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { SPEEDS, TRIMMED_PAUSE_MS, type NarrationInfo } from '../../lib/video';
  import { clock } from '../narration/format';
  import { NarrationPlayback } from './narration-playback';
  import type { ReplayPlayer } from './replay-player.svelte';

  let {
    player,
    narration = null,
    audioUrl = null,
    loading = false,
    locked = false,
    error = null,
    onnarration,
  }: {
    player: ReplayPlayer;
    /** The note's stored narration, if any. */
    narration?: NarrationInfo | null;
    /** The narration's sound (a Blob URL) while it plays along; null otherwise. */
    audioUrl?: string | null;
    /** The narration's sound is being fetched. */
    loading?: boolean;
    locked?: boolean;
    /** Why the narration cannot be played. */
    error?: string | null;
    /** The "Lire avec la narration" switch. */
    onnarration?: (on: boolean) => void;
  } = $props();

  /** Arrow keys on the timeline move this far, milliseconds; Page Up and Page Down ten times as far. */
  const KEY_STEP_MS = 1000;

  let audio = $state<HTMLAudioElement>();
  /** The browser refused the sound (the page's security policy, a damaged file…). */
  let audioFailed = $state(false);
  let resumeAfterDrag = false;

  const alongNarration = $derived(audioUrl !== null);
  const format = $derived(new Intl.NumberFormat(currentLocale(), { maximumFractionDigits: 2 }));
  const speedLabel = (speed: number) => t('replay.speedValue', { speed: format.format(speed) });
  /** The speeds offered, plus the one of a narration when it is not one of them. */
  const speeds = $derived.by(() => {
    const speed = player.options.speed;
    return SPEEDS.some((s) => s === speed) ? [...SPEEDS] : [...SPEEDS, speed].sort((a, b) => a - b);
  });
  const timeText = $derived(t('replay.timeValue', { current: clock(player.position), total: clock(player.end) }));

  // A new sound gets a new chance.
  $effect(() => {
    void audioUrl;
    audioFailed = false;
  });

  const playback = $derived(audio && audioUrl && narration ? new NarrationPlayback(audio, narration.startMs) : null);

  // The sound is silenced when the narration is switched off or the bar goes away...
  $effect(() => {
    const current = playback;
    return () => current?.stop();
  });
  // ...and follows the playhead (every frame while playing, and at every seek), in replay instants:
  // the sound that begins before instant 0 is heard during the timeline's lead-in.
  $effect(() => {
    playback?.sync(player.instant, player.playing);
  });

  // The switch shows what is true, not what was clicked: fetching the sound may fail.
  function switchNarration(e: Event & { currentTarget: HTMLInputElement }) {
    const on = e.currentTarget.checked;
    e.currentTarget.checked = alongNarration;
    onnarration?.(on);
  }

  function toggle() {
    player.toggle();
  }

  function seekKey(e: KeyboardEvent) {
    const steps: Record<string, number> = {
      ArrowLeft: -KEY_STEP_MS,
      ArrowDown: -KEY_STEP_MS,
      ArrowRight: KEY_STEP_MS,
      ArrowUp: KEY_STEP_MS,
      PageDown: -10 * KEY_STEP_MS,
      PageUp: 10 * KEY_STEP_MS,
    };
    const step = steps[e.key];
    if (step === undefined || e.altKey || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    player.seek(player.position + step);
  }

  // Dragging the thumb pauses the replay, which goes on when it is released.
  function hold() {
    resumeAfterDrag = player.playing;
    if (resumeAfterDrag) player.pause();
  }

  function release() {
    const resume = resumeAfterDrag;
    resumeAfterDrag = false;
    if (resume && player.position < player.end) player.play();
  }
</script>

<div class="replay-bar" role="group" aria-label={t('replay.bar')} data-playing={player.playing ? 'yes' : 'no'}>
  <button
    type="button"
    class="button primary play"
    data-action="replay-play"
    disabled={locked || player.end <= 0}
    aria-label={player.playing ? t('replay.pause') : t('replay.play')}
    title="{player.playing ? t('replay.pause') : t('replay.play')} ({t('replay.spaceKey')})"
    onclick={toggle}
  >
    <span aria-hidden="true">{player.playing ? '❚❚' : '▶'}</span>
    <span class="play-label">{player.playing ? t('replay.pause') : t('replay.play')}</span>
  </button>

  <span class="time" data-role="replay-current">{clock(player.position)}</span>
  <input
    type="range"
    class="timeline"
    data-action="replay-seek"
    min="0"
    max={player.end}
    step="1"
    value={Math.round(player.position)}
    disabled={locked || player.end <= 0}
    aria-label={t('replay.timeline')}
    aria-valuetext={timeText}
    oninput={(e) => player.seek(Number(e.currentTarget.value))}
    onkeydown={seekKey}
    onpointerdown={hold}
    onpointerup={release}
    onpointercancel={release}
  />
  <span class="time" data-role="replay-total">{clock(player.end)}</span>

  <label class="control">
    <span class="label">{t('replay.speed')}</span>
    <select
      data-action="replay-speed"
      value={String(player.options.speed)}
      disabled={locked || alongNarration}
      onchange={(e) => player.setSpeed(Number(e.currentTarget.value))}
    >
      {#each speeds as speed (speed)}
        <option value={String(speed)}>{speedLabel(speed)}</option>
      {/each}
    </select>
  </label>

  <label class="check">
    <input
      type="checkbox"
      data-action="replay-trim"
      checked={player.trimmed}
      disabled={locked || alongNarration}
      onchange={(e) => player.setTrim(e.currentTarget.checked)}
    />
    {t('replay.trim')}
    <span class="muted">{t('replay.trimDetail', { seconds: format.format(TRIMMED_PAUSE_MS / 1000) })}</span>
  </label>

  {#if narration}
    <label class="check">
      <input
        type="checkbox"
        data-action="replay-narration"
        checked={alongNarration || loading}
        disabled={locked || loading}
        onchange={switchNarration}
      />
      {t('replay.withNarration')}
      <span class="muted">({clock(narration.durationMs)})</span>
    </label>
  {/if}

  {#if alongNarration}
    <span class="muted small" data-slot="narration-locks">{t('replay.narrationLocks')}</span>
    <audio bind:this={audio} src={audioUrl} preload="auto" aria-hidden="true" onerror={() => (audioFailed = true)}></audio>
  {/if}
</div>
{#if player.end <= 0}
  <p class="replay-note muted small" role="status">{t('replay.empty')}</p>
{/if}
{#if loading}
  <p class="replay-note muted small" role="status">{t('replay.narrationLoading')}</p>
{/if}
{#if locked}
  <p class="replay-note muted small" role="status">{t('replay.locked')}</p>
{/if}
{#if error}
  <p class="replay-note error small" role="alert" data-slot="replay-error">{error}</p>
{:else if audioFailed}
  <p class="replay-note error small" role="alert" data-slot="replay-error">{t('replay.audioFailed')}</p>
{/if}

<style>
  .replay-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.85rem;
    padding: 0.4rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .play {
    min-width: 7.5rem;
    justify-content: center;
    padding: 0.35rem 0.8rem;
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .time {
    min-width: 2.6rem;
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  .timeline {
    flex: 1;
    min-width: 8rem;
    height: 1.8rem;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
    touch-action: none;
  }

  .timeline:disabled {
    cursor: default;
  }

  .timeline:focus-visible,
  .check input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .control,
  .check {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.875rem;
  }

  .check {
    cursor: pointer;
  }

  .check input {
    width: 1.1rem;
    height: 1.1rem;
    accent-color: var(--accent);
  }

  .label {
    color: var(--muted);
  }

  select {
    padding: 0.25rem 0.4rem;
    font-size: 0.875rem;
  }

  select:disabled {
    opacity: 0.6;
  }

  .replay-note {
    margin: 0;
    padding: 0.3rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .error {
    color: var(--danger);
  }

  @media (max-width: 900px) {
    .play-label {
      display: none;
    }

    .play {
      min-width: 0;
    }
  }
</style>
