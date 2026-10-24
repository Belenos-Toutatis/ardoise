<script lang="ts">
  /**
   * Splitting the note in two (SPEC §4.4): at an instant of the timeline, or by a zone traced on
   * the page. What goes to the new note is highlighted on the page as the settings change; the
   * editor owns the state and the canvas.
   */
  import { canSplit, formatTimecode, type SplitMode, type SplitPreview } from '../../lib/editor/split';
  import { t } from '../../lib/i18n.svelte';

  let {
    mode,
    range,
    at,
    hasZone,
    preview,
    title = $bindable(),
    dirty,
    busy,
    error,
    onmode,
    onat,
    onedittitle,
    onclearzone,
    onconfirm,
    oncancel,
  }: {
    mode: SplitMode;
    /** First and last sample time of the note; null when it has no stroke. */
    range: { min: number; max: number } | null;
    at: number;
    hasZone: boolean;
    preview: SplitPreview;
    /** Title of the new note. */
    title: string;
    /** The note has unsaved changes, which the split saves with it. */
    dirty: boolean;
    busy: boolean;
    error: string | null;
    onmode: (mode: SplitMode) => void;
    onat: (at: number) => void;
    /** The user typed in the title field (it no longer follows the mode). */
    onedittitle: () => void;
    onclearzone: () => void;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  const step = $derived(range ? Math.max(1, Math.round((range.max - range.min) / 1000)) : 1);
  const ready = $derived(canSplit(preview) && title.trim() !== '');
  // A refusal is explained once the settings are chosen: always in time, after a zone in zone mode.
  const nothing = $derived((mode === 'time' || hasZone) && !canSplit(preview));
</script>

<section class="side-section split-panel" aria-labelledby="editor-split-heading">
  <h3 id="editor-split-heading">{t('editor.split.title')}</h3>

  {#if !range}
    <p class="muted small">{t('editor.split.empty')}</p>
    <div class="buttons">
      <button type="button" class="button small-button" onclick={oncancel}>{t('editor.split.close')}</button>
    </div>
  {:else}
    <div class="buttons" role="radiogroup" aria-label={t('editor.split.mode')}>
      <label class="choice">
        <input type="radio" name="split-mode" checked={mode === 'time'} disabled={busy} onchange={() => onmode('time')} />
        {t('editor.split.modeTime')}
      </label>
      <label class="choice">
        <input type="radio" name="split-mode" checked={mode === 'zone'} disabled={busy} onchange={() => onmode('zone')} />
        {t('editor.split.modeZone')}
      </label>
    </div>

    {#if mode === 'time'}
      <p class="muted small">{t('editor.split.timeHint')}</p>
      <label class="split-slider">
        <span class="visually-hidden">{t('editor.split.instant')}</span>
        <input
          type="range"
          min={range.min}
          max={range.max}
          {step}
          value={at}
          disabled={busy}
          aria-valuetext={formatTimecode(at)}
          oninput={(e) => onat(Number(e.currentTarget.value))}
        />
      </label>
      <p class="split-time" aria-live="polite">
        {t('editor.split.instantAt', { time: formatTimecode(at), total: formatTimecode(range.max) })}
      </p>
    {:else}
      <p class="muted small">{hasZone ? t('editor.split.zoneTraced') : t('editor.split.zoneHint')}</p>
      {#if hasZone}
        <div class="buttons">
          <button type="button" class="button small-button" disabled={busy} onclick={onclearzone}>{t('editor.split.retrace')}</button>
        </div>
      {/if}
    {/if}

    <p class="small split-counts" aria-live="polite">{t('editor.split.counts', { kept: preview.keptCount, moved: preview.movedCount })}</p>
    {#if nothing}
      <p class="small error" role="status">{t('editor.split.nothing')}</p>
    {/if}

    <label class="row split-title">
      <span>{t('editor.split.newTitle')}</span>
      <input type="text" bind:value={title} disabled={busy} oninput={onedittitle} />
    </label>
    {#if dirty}
      <p class="muted small">{t('editor.split.dirtyHint')}</p>
    {/if}
    {#if error}
      <p class="banner error inline" role="alert">{t('editor.split.error', { message: error })}</p>
    {/if}

    <div class="buttons">
      <button type="button" class="button small-button primary" disabled={!ready || busy} onclick={onconfirm}>
        {busy ? t('editor.split.running') : t('editor.split.confirm')}
      </button>
      <button type="button" class="button small-button" disabled={busy} onclick={oncancel}>{t('library.cancel')}</button>
    </div>
  {/if}
</section>

<style>
  .split-slider input {
    width: 100%;
    accent-color: var(--accent);
  }

  .split-time {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .split-title input {
    flex: 1;
    min-width: 0;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.875rem;
    cursor: pointer;
  }
</style>
