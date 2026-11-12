<script lang="ts" module>
  /** When the narration is recorded, outside a replay (M17): while writing in the editor, or during a replay. */
  export type NarrationMode = 'writing' | 'replay';

  /** Where the mode is remembered on this PC (browser storage, as the side panel's choices). */
  export const NARRATION_MODE_KEY = 'ardoise.editor.narrationMode';

  /** The mode remembered on this PC; « Pendant que j'écris » by default (O12). */
  export function loadNarrationMode(): NarrationMode {
    try {
      return localStorage.getItem(NARRATION_MODE_KEY) === 'replay' ? 'replay' : 'writing';
    } catch {
      // No storage (a private window, blocked site data): the default.
      return 'writing';
    }
  }

  function saveNarrationMode(mode: NarrationMode): void {
    try {
      localStorage.setItem(NARRATION_MODE_KEY, mode);
    } catch {
      // Not remembered: the choice holds for this editor only.
    }
  }
</script>

<script lang="ts">
  /**
   * The editor's Son tab (milestones M16 and M17): the narration. During a replay it holds what the narration block
   * held (what the note stores, the recording controls of a replay, the deletion), whatever the mode. Outside one, a
   * segmented control chooses when the sound is recorded (remembered per PC, « Pendant que j'écris » by default), under
   * it the narration the note stores, then:
   * - « Pendant que j'écris » (M17, docs/design/paper-projection-narration.md §3.4): the recording controls of a writing
   *   recording (the strokes written meanwhile take the time of the sound; « Arrêter » saves the note with it), the
   *   sound kept in memory when that save failed, and the deletion;
   * - « Pendant une relecture »: that the narration is recorded during a replay, and a button to start one.
   * In both modes a new recording asks first whether it replaces the note's narration (O11). The editor owns the state
   * (recorder, replay, stored and staged narration); this component shows it.
   */
  import { untrack } from 'svelte';
  import { t } from '../../lib/i18n.svelte';
  import type { NarrationInfo } from '../../lib/video';
  import { clock } from '../narration/format';
  import RecordPanel from '../narration/RecordPanel.svelte';
  import type { NarrationRecorder } from '../narration/recorder.svelte';

  let {
    replaying,
    narration,
    staged = null,
    recorder,
    open = $bindable(false),
    confirmDelete = $bindable(false),
    device,
    ondevice,
    onstart,
    onstop,
    oncancel,
    onretry,
    ondelete,
    onreplay,
    replayDisabled = false,
    recordInfo,
    unsavedWarning = false,
    replayPlaying,
  }: {
    replaying: boolean;
    /** The narration stored in the note; null when there is none. */
    narration: NarrationInfo | null;
    /** How long the sound of a writing recording kept in memory is (its save failed), ms; null without one. */
    staged?: number | null;
    recorder: NarrationRecorder;
    /** The recording panel is unfolded. */
    open?: boolean;
    /** The deletion of the stored narration waits for its confirmation. */
    confirmDelete?: boolean;
    /** The microphone chosen; null for the system's default. */
    device: string | null;
    ondevice: (device: string | null) => void;
    /** « Démarrer », once a replacement was accepted: a replay recording in a replay, a writing one outside. */
    onstart: () => void;
    /** Stop and save. */
    onstop: () => void;
    /** Stop and throw the recording away. */
    oncancel: () => void;
    onretry: () => void;
    /** The deletion was confirmed. */
    ondelete: () => void;
    /** « Lancer la relecture »: outside a replay. */
    onreplay: () => void;
    /** The replay cannot start now (the split panel is open, a recording is under way). */
    replayDisabled?: boolean;
    /** The replay options a recording would use, as a sentence. */
    recordInfo: string;
    /** The note has unsaved changes: the recording panel of a replay warns. */
    unsavedWarning?: boolean;
    /** The replay is playing (the recording panel says what happens after its end otherwise). */
    replayPlaying: boolean;
  } = $props();

  /** The two modes, in the segmented control's order, with their labels. */
  const MODES: readonly (readonly [NarrationMode, string])[] = [
    ['writing', 'editor.sound.modeWriting'],
    ['replay', 'editor.sound.modeReplay'],
  ];

  let mode = $state<NarrationMode>(loadNarrationMode());

  /** A new recording replaces a narration: the stored one, or the sound waiting to be saved. */
  const replaces = $derived(narration !== null || staged !== null);
  const replacedMs = $derived(staged ?? narration?.durationMs ?? null);

  function choose(next: NarrationMode) {
    if (next === mode) return;
    mode = next;
    saveNarrationMode(next);
    // What was open belongs to the other mode.
    untrack(() => {
      open = false;
      confirmDelete = false;
    });
  }
</script>

<section class="pane-block" aria-labelledby="editor-narration-heading" data-section="narration" data-mode={replaying ? 'replay' : mode}>
  <h3 class="pane-title" id="editor-narration-heading">{t('narration.title')}</h3>
  {#if !replaying}
    <div class="pane-seg" role="radiogroup" aria-label={t('editor.sound.mode')} data-field="narration-mode">
      {#each MODES as [value, label] (value)}
        <label class:on={mode === value} class:disabled={recorder.busy}>
          <input
            type="radio"
            name="editor-narration-mode"
            {value}
            checked={mode === value}
            disabled={recorder.busy}
            onchange={() => choose(value)}
          />
          {t(label)}
        </label>
      {/each}
    </div>
  {/if}
  {#if narration}
    <p class="pane-help" data-slot="narration-stored">
      {t('narration.stored.info', { duration: clock(narration.durationMs) })}
    </p>
  {:else}
    <p class="pane-help" data-slot="narration-none">{t('narration.stored.none')}</p>
  {/if}
  {#if staged !== null}
    <p class="pane-help staged" role="status" data-slot="narration-staged">{t('editor.sound.notSaved')}</p>
  {/if}
  {#if replaying}
    <RecordPanel
      bind:open
      {recorder}
      {device}
      {ondevice}
      {onstart}
      {onstop}
      {oncancel}
      {onretry}
      openLabel={t('narration.record.open')}
      openHint={t('narration.record.openHint')}
      {replaces}
      {replacedMs}
      info={recordInfo}
      saveNote={t('narration.record.saveNote')}
      warning={unsavedWarning ? t('narration.record.unsavedWarning') : null}
      duringInfo={replayPlaying ? null : t('narration.record.afterEnd')}
    />
  {:else if mode === 'writing'}
    <p class="pane-help" data-slot="writing-help">{t('editor.sound.writingHelp')}</p>
    <RecordPanel
      bind:open
      {recorder}
      {device}
      {ondevice}
      {onstart}
      {onstop}
      {oncancel}
      {onretry}
      openLabel={t('editor.sound.recordWriting')}
      openHint={t('editor.sound.recordWritingHint')}
      {replaces}
      {replacedMs}
      info={t('editor.sound.writingInfo')}
      saveNote={t('editor.sound.writingSave')}
      duringInfo={t('editor.sound.writingDuring')}
    />
  {:else}
    <p class="pane-help">{t('editor.sound.replayFirst')}</p>
    <div class="pane-actions">
      <button type="button" class="button small-button" data-action="sound-replay" disabled={replayDisabled} onclick={onreplay}>
        <span aria-hidden="true">▶</span>
        {t('editor.sound.startReplay')}
      </button>
    </div>
  {/if}
  {#if narration && !recorder.busy && (replaying || mode === 'writing')}
    {#if confirmDelete}
      <div class="confirm-delete" role="alert">
        <p class="pane-help">{t('narration.stored.deleteConfirm')}</p>
        <div class="pane-actions">
          <button type="button" class="button small-button danger" data-action="narration-delete-yes" onclick={ondelete}>
            {t('narration.stored.deleteYes')}
          </button>
          <button type="button" class="button small-button" onclick={() => (confirmDelete = false)}>
            {t('narration.stored.deleteNo')}
          </button>
        </div>
      </div>
    {:else}
      <div class="pane-actions">
        <button type="button" class="button small-button danger" data-action="narration-delete" onclick={() => (confirmDelete = true)}>
          {t('narration.stored.delete')}
        </button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .confirm-delete {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  /* Two radios drawn as one segmented button (the mockup's « Pendant que j'écris » / « Pendant une relecture »). */
  .pane-seg {
    display: inline-flex;
    align-self: flex-start;
    max-width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--surface);
  }

  .pane-seg label {
    position: relative;
    display: inline-flex;
    align-items: center;
    padding: 0.3rem 0.75rem;
    font-size: 0.875rem;
    cursor: pointer;
  }

  .pane-seg label + label {
    border-left: 1px solid var(--border);
  }

  .pane-seg label.on {
    background: var(--accent);
    color: var(--accent-text);
  }

  .pane-seg label.disabled {
    opacity: 0.6;
    cursor: default;
  }

  .pane-seg label:focus-within {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .pane-seg label.on:focus-within {
    outline-color: var(--text);
  }

  /* The radio itself is not drawn: its label is the button (it keeps the focus and the keys of a radio group). */
  .pane-seg input {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: 0;
    opacity: 0;
    pointer-events: none;
  }

  .staged {
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    background: var(--surface-2);
  }
</style>
