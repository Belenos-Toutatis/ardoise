<script lang="ts">
  /**
   * The recording controls shared by the editor (narration of a replay) and the live view: the
   * microphone (picker with its level and status), then start, and while recording the time, a
   * stop that saves and a cancel. The caller starts and stops through the recorder (`onstart`,
   * `onstop`, `oncancel`), because the target and the saving differ. A recording that could not be
   * saved is kept by Rust (`recorder.pending`): the panel then offers to retry (`onretry`, which
   * knows the note's current path and stamp; no retry for the live view) or to abandon it.
   *
   * M17 (O11): with `replaces`, « Démarrer » first asks whether the narration the note has (`replacedMs` long) is to be
   * replaced; only « Remplacer » starts. The live view passes no `replaces`.
   */
  import { onMount, tick } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import { clock } from './format';
  import MicrophonePicker from './MicrophonePicker.svelte';
  import type { NarrationRecorder } from './recorder.svelte';

  let {
    recorder,
    device,
    ondevice,
    onstart,
    onstop,
    oncancel,
    onretry,
    open = $bindable(false),
    openLabel,
    openHint,
    replaces = false,
    replacedMs = null,
    info = null,
    saveNote = null,
    warning = null,
    duringInfo = null,
  }: {
    recorder: NarrationRecorder;
    /** Chosen microphone id; null for the system's default. */
    device: string | null;
    ondevice: (device: string | null) => void;
    onstart: () => void;
    /** Stop and save. */
    onstop: () => void;
    /** Stop and throw the recording away. */
    oncancel: () => void;
    /** Save the pending recording again, into the note as it is now. */
    onretry?: () => void;
    /** The panel is unfolded (the microphone is listened to). */
    open?: boolean;
    openLabel: string;
    openHint?: string;
    /** A narration is already stored: a new recording replaces it, which « Démarrer » asks first. */
    replaces?: boolean;
    /** How long the narration a new recording replaces is, milliseconds (said in the question). */
    replacedMs?: number | null;
    /** Said before starting. */
    info?: string | null;
    /** Said before starting, after `info`: where the sound goes. */
    saveNote?: string | null;
    /** Said before starting as a warning (the note has unsaved changes). */
    warning?: string | null;
    /** Said while recording. */
    duringInfo?: string | null;
  } = $props();

  const idle = $derived(recorder.phase === 'idle');
  const pending = $derived(idle && recorder.pending);
  let confirmDiscard = $state(false);
  /** « Démarrer » asked whether to replace the stored narration: the answer waits. */
  let askingReplace = $state(false);
  let replaceYes = $state<HTMLButtonElement>();

  // A recording kept by an earlier failure (this screen, or one left before) is offered again.
  onMount(() => {
    void recorder.refreshPending();
  });

  // The question goes with what it was about: the panel folded, a recording started elsewhere, no narration to replace.
  $effect(() => {
    if (!open || !idle || !replaces) askingReplace = false;
  });

  async function discard() {
    confirmDiscard = false;
    await recorder.discard();
  }

  /** « Démarrer »: at once, or after the question when a narration would be replaced. */
  async function start() {
    if (!replaces) return onstart();
    askingReplace = true;
    // The button pressed went: the focus goes to the answer that goes on with it.
    await tick();
    replaceYes?.focus();
  }

  function replace() {
    askingReplace = false;
    onstart();
  }
</script>

<div class="record" data-phase={recorder.phase}>
  {#if pending}
    <div class="pending" role="group" aria-label={t('narration.pending.title')} data-slot="narration-pending">
      <p><strong>{t('narration.pending.title')}</strong></p>
      <p class="small">{t(onretry ? 'narration.pending.message' : 'narration.pending.messageElsewhere')}</p>
      {#if confirmDiscard}
        <p class="small" role="alert">{t('narration.pending.discardConfirm')}</p>
        <div class="buttons">
          <button type="button" class="button small-button danger" data-action="narration-discard-yes" onclick={discard}>
            {t('narration.pending.discardYes')}
          </button>
          <button type="button" class="button small-button" data-action="narration-discard-no" onclick={() => (confirmDiscard = false)}>
            {t('narration.pending.discardNo')}
          </button>
        </div>
      {:else}
        <div class="buttons">
          {#if onretry}
            <button
              type="button"
              class="button small-button primary"
              data-action="narration-retry"
              disabled={recorder.retrying}
              onclick={onretry}
            >
              {recorder.retrying ? t('narration.pending.retrying') : t('narration.pending.retry')}
            </button>
          {/if}
          <button
            type="button"
            class="button small-button danger"
            data-action="narration-discard"
            disabled={recorder.retrying}
            onclick={() => (confirmDiscard = true)}
          >
            {t('narration.pending.discard')}
          </button>
        </div>
      {/if}
    </div>
  {:else if !open && idle}
    <button type="button" class="button small-button" data-action="narration-open" title={openHint} onclick={() => (open = true)}>
      <span class="dot" aria-hidden="true"></span>
      {openLabel}
    </button>
  {:else}
    <MicrophonePicker {device} onchange={ondevice} monitor={idle} disabled={!idle} />
    {#if idle}
      {#if replaces}
        <p class="small replaces" data-slot="replaces">{t('narration.record.replaces')}</p>
      {/if}
      {#if info}
        <p class="small muted">{info}</p>
      {/if}
      {#if saveNote}
        <p class="small muted" data-slot="save-note">{saveNote}</p>
      {/if}
      {#if warning}
        <p class="small warning" role="status" data-slot="unsaved-warning">{warning}</p>
      {/if}
      {#if askingReplace}
        <div class="question" role="alert" data-slot="replace-question">
          <p class="small">{t('narration.record.replaceQuestion', { duration: clock(replacedMs ?? 0) })}</p>
          <div class="buttons">
            <button
              bind:this={replaceYes}
              type="button"
              class="button small-button danger"
              data-action="narration-replace-yes"
              onclick={replace}
            >
              {t('narration.record.replaceYes')}
            </button>
            <button type="button" class="button small-button" data-action="narration-replace-no" onclick={() => (askingReplace = false)}>
              {t('narration.record.replaceNo')}
            </button>
          </div>
        </div>
      {:else}
        <div class="buttons">
          <button type="button" class="button small-button primary" data-action="narration-start" onclick={() => void start()}>
            {t('narration.record.start')}
          </button>
          <button type="button" class="button small-button" data-action="narration-close" onclick={() => (open = false)}>
            {t('narration.record.close')}
          </button>
        </div>
      {/if}
    {:else}
      <p class="elapsed" role="timer" data-elapsed={Math.floor(recorder.elapsedMs)}>
        <span class="dot live" aria-hidden="true"></span>
        {t('narration.record.elapsed', { time: clock(recorder.elapsedMs) })}
      </p>
      {#if recorder.phase === 'saving'}
        <p class="small muted" role="status">{t('narration.record.saving')}</p>
      {:else if duringInfo}
        <p class="small muted" role="status">{duringInfo}</p>
      {/if}
      <div class="buttons">
        <button
          type="button"
          class="button small-button primary"
          data-action="narration-stop"
          disabled={recorder.phase !== 'recording'}
          onclick={onstop}
        >
          {t('narration.record.stop')}
        </button>
        <button
          type="button"
          class="button small-button"
          data-action="narration-cancel"
          disabled={recorder.phase !== 'recording'}
          onclick={oncancel}
        >
          {t('narration.record.cancel')}
        </button>
      </div>
    {/if}
  {/if}
  {#if recorder.error}
    <p class="small error" role="alert" data-slot="record-error">{t('narration.record.failed', { message: recorder.error })}</p>
  {/if}
</div>

<style>
  .record {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .record p {
    margin: 0;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .small-button {
    padding: 0.25rem 0.6rem;
    font-size: 0.875rem;
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .replaces {
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    background: var(--surface-2);
  }

  .pending {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem 0.65rem;
    border-left: 4px solid var(--danger);
    border-radius: 6px;
    background: var(--danger-bg);
  }

  .warning {
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    background: var(--surface-2);
  }

  .question {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem 0.65rem;
    border-left: 4px solid var(--accent);
    border-radius: 6px;
    background: var(--surface-2);
  }

  .elapsed {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 1.1rem;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--danger);
  }

  .dot.live {
    animation: blink 1.2s ease-in-out infinite;
  }

  @keyframes blink {
    50% {
      opacity: 0.25;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot.live {
      animation: none;
    }
  }
</style>
