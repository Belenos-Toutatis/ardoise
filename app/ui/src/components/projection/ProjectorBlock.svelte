<script lang="ts">
  /**
   * « Vidéoprojecteur »: the block of a window's Projection tab that sends the page to a screen (milestone M16;
   * docs/design/side-panel.md, §8). The screen to use, « Project » / « Stop projecting » and the list's refresh, and
   * what is projected now. The window owns the state and does the acts (`openPresentation`, `closePresentation`,
   * `listMonitors`): this block only shows them and calls back, so that the live control window and, from M17, the
   * editor use the same block. The screen list is the first `select` of the live side panel (the layers zone has none,
   * the narration's microphone picker shows only once its panel is open).
   */
  import { t } from '../../lib/i18n.svelte';
  import type { MonitorInfo } from '../../lib/live';

  let {
    monitors,
    monitorKey = $bindable(),
    loaded,
    presentation,
    busy,
    error,
    onproject,
    onstop,
    onrefresh,
  }: {
    monitors: readonly MonitorInfo[];
    /** The screen chosen (`MonitorInfo.key`). */
    monitorKey: string;
    /** The list of screens was asked for at least once: an empty list then means « no screen ». */
    loaded: boolean;
    /** The screen the projection is on, or null when nothing is projected. */
    presentation: MonitorInfo | null;
    /** The projection is opening: « Project » waits. */
    busy: boolean;
    /** The last failure, as a sentence; null when none. */
    error: string | null;
    onproject: () => void;
    onstop: () => void;
    onrefresh: () => void;
  } = $props();

  /** `\\.\DISPLAY2` → "Screen 2"; other names as they are. */
  function monitorName(m: MonitorInfo): string {
    const match = /DISPLAY(\d+)$/i.exec(m.name);
    const name = match ? t('live.projection.screen', { n: match[1] }) : m.name || m.key;
    return m.primary ? t('live.projection.primary', { name }) : name;
  }
</script>

<section class="pane-block" data-section="projector" aria-labelledby="projector-title">
  <h3 class="pane-title" id="projector-title">{t('live.projection.projector')}</h3>
  {#if loaded && monitors.length === 0}
    <p class="pane-help">{t('live.projection.none')}</p>
  {:else}
    <label class="screen-field">
      <span class="pane-help">{t('live.projection.monitor')}</span>
      <select bind:value={monitorKey} disabled={monitors.length === 0}>
        {#each monitors as monitor (monitor.key)}
          <option value={monitor.key}>
            {t('live.projection.option', { name: monitorName(monitor), width: monitor.width, height: monitor.height })}
          </option>
        {/each}
      </select>
    </label>
  {/if}
  <div class="pane-actions">
    {#if presentation}
      <button type="button" class="button small-button" data-action="stop-projecting" onclick={onstop}>
        {t('live.projection.stop')}
      </button>
    {:else}
      <button
        type="button"
        class="button small-button primary"
        data-action="project"
        disabled={busy || monitors.length === 0}
        onclick={onproject}
      >
        {t('live.projection.project')}
      </button>
    {/if}
    <button
      type="button"
      class="button small-button ghost"
      aria-label={t('live.projection.refresh')}
      title={t('live.projection.refresh')}
      onclick={onrefresh}><span aria-hidden="true">↻</span></button
    >
  </div>
  <p class="pane-help" data-presentation={presentation?.key ?? ''}>
    {presentation ? t('live.projection.on', { name: monitorName(presentation) }) : t('live.projection.off')}
  </p>
  {#if monitors.length === 1 && !presentation}
    <p class="pane-help">{t('live.projection.single')}</p>
  {/if}
  {#if error}
    <p class="error pane-help" role="alert">{error}</p>
  {/if}
</section>

<style>
  .screen-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .screen-field select {
    width: 100%;
  }

  .small-button {
    padding: 0.3rem 0.65rem;
    font-size: 0.875rem;
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
