<script lang="ts">
  /**
   * The editor's Son tab (milestone M16): the narration. During a replay it holds what the narration block held
   * (what the note stores, the recording controls, the deletion); outside one it says that a narration is heard and
   * recorded during a replay and offers to start it (the narration's length is only read by a replay, so it is not
   * shown here). M17 adds « Pendant que j'écris » above the recording. The editor owns the state (recorder, replay,
   * stored narration); this component shows it.
   */
  import { t } from '../../lib/i18n.svelte';
  import type { NarrationInfo } from '../../lib/video';
  import { clock } from '../narration/format';
  import RecordPanel from '../narration/RecordPanel.svelte';
  import type { NarrationRecorder } from '../narration/recorder.svelte';

  let {
    replaying,
    narration,
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
    /** The narration stored in the note (read by the replay); null when there is none yet. */
    narration: NarrationInfo | null;
    recorder: NarrationRecorder;
    /** The recording panel is unfolded. */
    open?: boolean;
    /** The deletion of the stored narration waits for its confirmation. */
    confirmDelete?: boolean;
    /** The microphone chosen; null for the system's default. */
    device: string | null;
    ondevice: (device: string | null) => void;
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
    /** The note has unsaved changes: the recording panel warns. */
    unsavedWarning?: boolean;
    /** The replay is playing (the recording panel says what happens after its end otherwise). */
    replayPlaying: boolean;
  } = $props();
</script>

<section class="pane-block" aria-labelledby="editor-narration-heading" data-section="narration">
  <h3 class="pane-title" id="editor-narration-heading">{t('narration.title')}</h3>
  {#if replaying}
    {#if narration}
      <p class="pane-help" data-slot="narration-stored">
        {t('narration.stored.info', { duration: clock(narration.durationMs) })}
      </p>
    {:else}
      <p class="pane-help">{t('narration.stored.none')}</p>
    {/if}
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
      replaces={narration !== null}
      info={recordInfo}
      saveNote={t('narration.record.saveNote')}
      warning={unsavedWarning ? t('narration.record.unsavedWarning') : null}
      duringInfo={replayPlaying ? null : t('narration.record.afterEnd')}
    />
    {#if narration && !recorder.busy}
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
  {:else}
    <p class="pane-help">{t('editor.sound.replayFirst')}</p>
    <div class="pane-actions">
      <button type="button" class="button small-button" data-action="sound-replay" disabled={replayDisabled} onclick={onreplay}>
        <span aria-hidden="true">▶</span>
        {t('editor.sound.startReplay')}
      </button>
    </div>
  {/if}
</section>

<style>
  .confirm-delete {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
</style>
