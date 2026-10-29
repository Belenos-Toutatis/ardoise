<script lang="ts">
  /**
   * Live mode and the classroom's control window (SPEC §4.2; docs/design/tablet-live-classroom.md,
   * "Interface"). Starts live mode when it opens (`live_start`) and follows the session
   * (`live_subscribe`): toolbar (tool, colour, width), pages, layer, clear, blackboard, laser
   * pointer, the preview of the whole page with the projected area (drag to move it), zoom,
   * the presentation window (screen, project / stop), the measured latency, the narration (SPEC
   * §4.5: record the microphone during the session; it is cut per page and saved with the notes),
   * and "Finish" (the narration stops first, then `live_stop`, then `onclose`).
   */
  import { onMount } from 'svelte';
  import { api, errorMessage, type AppState } from '../../lib/api';
  import { bridgeScreens, drivePresentation, until } from '../../lib/app-bridge.svelte';
  import { INKSPACE_COLOR_NAMES, sameColor } from '../../lib/editor/colors';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { TOOLS, loadInk, type Ink, type Tool } from '../../lib/ink';
  import { LatencyRecorder, exposeLatency } from '../../lib/latency';
  import type { LiveCommandKind, LiveStyle, LiveViewState, MonitorInfo } from '../../lib/live';
  import {
    changeView,
    runKeyAction,
    sendCommand,
    showPageWidth,
    showWholePage,
    toggleBoard,
    toggleLaser,
    zoomStep,
    type LiveActions,
  } from '../../lib/live-actions';
  import { liveKeyAction } from '../../lib/live-keys';
  import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
  import { MAX_ZOOM, MIN_ZOOM, aspectOf, defaultMonitor } from '../../lib/live-view';
  import { INKSPACE_COLORS, cssColor, type Rgba } from '../../lib/note';
  import { ORIENTATIONS, type Orientation } from '../../lib/orientation';
  import type { NarrationSaved } from '../../lib/video';
  import RecordPanel from '../narration/RecordPanel.svelte';
  import OrientationIcon from '../tablet/OrientationIcon.svelte';
  import { NarrationRecorder } from '../narration/recorder.svelte';
  import LiveCanvas from './LiveCanvas.svelte';

  let {
    app,
    onclose,
    onchange,
  }: {
    app: AppState;
    onclose: () => void;
    /** The app state changed (the microphone chosen here): the screen holding it takes the new one. */
    onchange?: (next: AppState) => void;
  } = $props();

  /** Nominal width per ink tool (device units), as in the editor. */
  const DEFAULT_WIDTHS: Record<Tool, number> = {
    ballpoint: 60,
    fountainPen: 80,
    feltTip: 120,
    pencil: 50,
    brush: 150,
    highlighter: 500,
    chalk: 150,
  };
  /** The Inkspace palette, then white chalk and yellow for the blackboard. */
  const COLORS: { color: Rgba; label: string }[] = [
    ...INKSPACE_COLORS.map((color, i) => ({ color, label: `editor.colors.${INKSPACE_COLOR_NAMES[i]}` })),
    { color: { r: 255, g: 255, b: 255, a: 255 }, label: 'live.colors.white' },
    { color: { r: 255, g: 214, b: 0, a: 255 }, label: 'live.colors.yellow' },
  ];
  /** Why stopping the narration may fail without anything being lost: there was no sound to keep. */
  const NOTHING_TO_KEEP = ['emptyRecording', 'microphoneSilent'];
  /** How often the latency shown is refreshed. */
  const LATENCY_REFRESH_MS = 500;
  const KEYS = ['next', 'previous', 'clear', 'board', 'laser', 'zoom', 'whole', 'width', 'pan', 'leave'] as const;

  const live = new LiveState();
  const latency = new LatencyRecorder();
  const recorder = new NarrationRecorder();

  let ink = $state.raw<Ink | null>(null);
  let starting = $state(true);
  let startError = $state<string | null>(null);
  let stopping = $state(false);
  let stopError = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let monitors = $state.raw<MonitorInfo[]>([]);
  let monitorKey = $state('');
  let monitorsLoaded = $state(false);
  let projecting = $state(false);
  let projectionError = $state<string | null>(null);
  let latencyMedian = $state<number | null>(null);
  let latencyP95 = $state<number | null>(null);
  let latencyUi = $state<number | null>(null);
  let latencyCount = $state(0);
  let narrationOpen = $state(false);
  /** What the last narration recording saved; null before one, or after a cancelled one. */
  let narrated = $state.raw<NarrationSaved | null>(null);
  /** The microphone chosen here, until the app state says so. */
  let chosenMicrophone = $state<string | null | undefined>(undefined);
  /** Width last used per tool in this session. */
  const widths: Partial<Record<Tool, number>> = {};

  const sessionState = $derived(live.ready ? live.state : 'idle');
  const selectedMonitor = $derived(monitors.find((m) => m.key === monitorKey) ?? null);
  const aspect = $derived(aspectOf(live.presentation ?? selectedMonitor));
  const microphone = $derived(chosenMicrophone === undefined ? app.microphone : chosenMicrophone);
  const narratedText = $derived(
    narrated === null
      ? null
      : narrated.pages === 0
        ? t('live.narration.savedNone')
        : narrated.pages === 1
          ? t('live.narration.savedOne')
          : t('live.narration.savedMany', { count: narrated.pages }),
  );
  const savedCount = $derived(live.saved.length);
  const savedText = $derived(
    savedCount === 0 ? t('live.saved.none') : savedCount === 1 ? t('live.saved.one') : t('live.saved.many', { count: savedCount }),
  );
  const pageLabel = $derived(t('live.pages.position', { n: live.pageIndex + 1, count: live.pageCount }));
  const decimal = $derived(new Intl.NumberFormat(currentLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }));
  /** p95 and the interface's share, in the latency's tooltip. */
  const latencyDetail = $derived(
    latencyMedian === null
      ? ''
      : t('live.latency.detail', {
          count: latencyCount,
          p95: latencyP95 === null ? t('live.latency.none') : decimal.format(latencyP95),
          ui: latencyUi === null ? t('live.latency.none') : decimal.format(latencyUi),
        }),
  );

  const actions: LiveActions = {
    live,
    commands: api,
    aspect: () => aspect,
    onerror: (error) => (notice = t('live.commandFailed', { message: errorMessage(error) })),
  };

  onMount(() => {
    let alive = true;
    exposeLatency(latency);
    loadInk()
      .then((module) => {
        if (alive) ink = module;
      })
      .catch((error: unknown) => {
        if (alive) notice = errorMessage(error);
      });
    void (async () => {
      try {
        await subscribeLive(live, api.liveSubscribe);
      } catch {
        // `live_start` gives the session too; batches then follow it.
      }
      if (alive) await start();
    })();
    void refreshMonitors();
    const timer = setInterval(refreshLatency, LATENCY_REFRESH_MS);
    // The assistant's bridge drives the projection and the pages as these buttons and keys do.
    const unregister = bridgeScreens.register('live', {
      presentation: (action, page) =>
        drivePresentation(
          {
            get pageIndex() {
              return live.pageIndex;
            },
            get pageCount() {
              return live.pageCount;
            },
            get presentation() {
              return live.presentation;
            },
            project: openProjection,
            stopProjecting: closeProjection,
            command: (kind) => api.liveCommand({ kind }),
            until,
          },
          action,
          page,
        ),
    });
    return () => {
      unregister();
      alive = false;
      clearInterval(timer);
      live.close();
      recorder.dispose();
      // A recording is never thrown away silently: the pages get the sound recorded so far.
      if (recorder.phase === 'recording') void api.narrationStop(true, null, null, null).catch(() => {});
    };
  });

  async function start() {
    starting = true;
    startError = null;
    try {
      live.adoptStart(await api.liveStart());
    } catch (error) {
      startError = errorMessage(error);
    } finally {
      starting = false;
    }
  }

  function refreshLatency() {
    latencyMedian = latency.median();
    latencyP95 = latency.p95();
    latencyUi = latency.uiMedian();
    latencyCount = latency.size;
  }

  async function chooseMicrophone(device: string | null) {
    chosenMicrophone = device;
    try {
      const next = await api.setMicrophone(device);
      chosenMicrophone = next.microphone;
      onchange?.(next);
    } catch (error) {
      chosenMicrophone = undefined;
      notice = errorMessage(error);
    }
  }

  async function startNarration() {
    narrated = null;
    await recorder.start(microphone, { kind: 'live' });
  }

  /** Stops the recording; saved, the sound is cut per page and written with the pages. */
  async function stopNarration(save: boolean) {
    // Closing the panel while it is still busy keeps its microphone picker from listening again.
    if (save) narrationOpen = false;
    const result = await recorder.stop(save, null);
    if (result.ok) narrated = result.saved;
  }

  async function finish() {
    if (stopping) return;
    stopping = true;
    stopError = null;
    // The narration is handed to the session, which writes it with the pages: it stops first.
    if (recorder.phase === 'recording') {
      narrationOpen = false;
      const result = await recorder.stop(true, null);
      if (!result.ok && !NOTHING_TO_KEEP.includes(result.code ?? '')) {
        stopError = recorder.error;
        stopping = false;
        return;
      }
      // A recording with nothing in it (silent or blocked microphone) is nothing to keep: leaving goes on.
      narrated = result.saved;
    }
    try {
      if (live.presentation) {
        await api.closePresentation().catch(() => {
          // Rust closes it with the session anyway.
        });
      }
      await api.liveStop();
    } catch (error) {
      const code = (error as { code?: unknown } | null)?.code;
      if (code !== 'liveNotRunning') {
        stopError = errorMessage(error);
        stopping = false;
        return;
      }
    }
    live.close();
    onclose();
  }

  function closeAnyway() {
    live.close();
    onclose();
  }

  function command(kind: LiveCommandKind) {
    notice = null;
    sendCommand(actions, kind);
  }

  /** The quick switch of the tablet's position: applies at once, the snapshot that follows shows it. */
  function setOrientation(orientation: Orientation) {
    if (orientation === live.orientation) return;
    notice = null;
    api.tabletSetOrientation(orientation).catch((error: unknown) => actions.onerror(error));
  }

  function setStyle(style: LiveStyle) {
    live.changeStyle(style, api.liveSetStyle).catch((error: unknown) => actions.onerror(error));
  }

  function setTool(tool: Tool) {
    const style = live.style;
    widths[style.tool] = style.width;
    setStyle({ ...style, tool, width: widths[tool] ?? DEFAULT_WIDTHS[tool] });
  }

  function setView(view: LiveViewState) {
    changeView(actions, view);
  }

  async function refreshMonitors() {
    try {
      monitors = await api.listMonitors();
      if (!monitors.some((m) => m.key === monitorKey)) monitorKey = defaultMonitor(monitors)?.key ?? '';
    } catch (error) {
      projectionError = errorMessage(error);
    } finally {
      monitorsLoaded = true;
    }
  }

  /** "Project": the presentation window on the chosen screen; a failure is shown, and thrown. */
  async function openProjection(): Promise<MonitorInfo> {
    projecting = true;
    projectionError = null;
    try {
      const shown = await api.openPresentation(monitorKey || null);
      monitorKey = shown.key;
      return shown;
    } catch (error) {
      projectionError = errorMessage(error);
      throw error;
    } finally {
      projecting = false;
    }
  }

  async function project() {
    await openProjection().catch(() => {});
  }

  /** "Stop projecting"; a failure is shown, and thrown. */
  async function closeProjection(): Promise<void> {
    projectionError = null;
    try {
      await api.closePresentation();
    } catch (error) {
      projectionError = errorMessage(error);
      throw error;
    }
  }

  async function stopProjecting() {
    await closeProjection().catch(() => {});
  }

  /** `\\.\DISPLAY2` → "Screen 2"; other names as they are. */
  function monitorName(m: MonitorInfo): string {
    const match = /DISPLAY(\d+)$/i.exec(m.name);
    const name = match ? t('live.projection.screen', { n: match[1] }) : m.name || m.key;
    return m.primary ? t('live.projection.primary', { name }) : name;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented) return;
    const action = liveKeyAction(e);
    if (!action) return;
    e.preventDefault();
    runKeyAction(actions, action, () => void finish());
  }

  const mm = (units: number) => (units / 100).toFixed(units < 100 ? 2 : 1);
  const ms = (value: number | null) => (value === null ? t('live.latency.none') : t('live.latency.value', { ms: decimal.format(value) }));
</script>

<svelte:window {onkeydown} />

<section class="live-view" data-state={sessionState} aria-label={t('live.title')}>
  <header class="live-top">
    <h2>{t('live.title')}</h2>
    <span class="live-state" data-state={sessionState} role="status">
      <span class="dot" aria-hidden="true"></span>
      {starting && !live.ready ? t('live.starting') : t(`live.state.${sessionState}`)}
    </span>
    <span class="latency" title={latencyDetail ? `${t('live.latency.hint')} ${latencyDetail}` : t('live.latency.hint')}>
      <span class="muted">{t('live.latency.label')}</span>
      <strong data-latency-median={latencyMedian === null ? '' : latencyMedian.toFixed(1)}>{ms(latencyMedian)}</strong>
      {#if latencyDetail}
        <span class="visually-hidden">{latencyDetail}</span>
      {/if}
    </span>
    <span class="saved muted" data-saved={savedCount}>{savedText}</span>
    <button
      type="button"
      class="button primary"
      data-action="stop-live"
      disabled={stopping}
      title={t('live.stopHint')}
      onclick={finish}
    >
      {stopping ? t('live.stopping') : t('live.stop')}
    </button>
  </header>

  {#if startError}
    <div class="banner error live-banner" role="alert">
      <strong>{t('live.startFailed')}</strong>
      <p>{startError}</p>
      <button type="button" class="button small-button primary" onclick={start}>{t('live.retry')}</button>
      <button type="button" class="button small-button" onclick={closeAnyway}>{t('common.close')}</button>
    </div>
  {/if}
  {#if stopError}
    <div class="banner error live-banner" role="alert">
      <strong>{t('live.stopFailed')}</strong>
      <p>{stopError}</p>
      <button type="button" class="button small-button" onclick={closeAnyway}>{t('live.closeAnyway')}</button>
    </div>
  {/if}
  {#if live.saveErrors.length > 0}
    <div class="banner error live-banner" role="alert" data-slot="save-failed">
      <strong>
        {live.saveErrors.length === 1 ? t('live.saveFailed.one') : t('live.saveFailed.many', { count: live.saveErrors.length })}
      </strong>
      <ul>
        {#each live.saveErrors as error (error.pageIndex)}
          <li>{t('live.saveFailed.item', { page: error.pageIndex + 1, message: error.message })}</li>
        {/each}
      </ul>
      <p>{t('live.saveFailed.advice')}</p>
    </div>
  {/if}
  {#if notice}
    <div class="banner error live-banner" role="alert">
      <p>{notice}</p>
      <button type="button" class="button small-button" onclick={() => (notice = null)}>{t('live.dismiss')}</button>
    </div>
  {/if}

  <div class="live-toolbar" role="toolbar" aria-label={t('live.toolbar')}>
    <div class="group" role="group" aria-label={t('live.style')}>
      <label class="group">
        <span class="label">{t('live.tool')}</span>
        <select value={live.style.tool} onchange={(e) => setTool(e.currentTarget.value as Tool)}>
          {#each TOOLS as tool (tool)}
            <option value={tool}>{t(`editor.ink.tools.${tool}`)}</option>
          {/each}
        </select>
      </label>
      <div class="group swatches" role="group" aria-label={t('live.color')}>
        {#each COLORS as { color, label } (label)}
          <button
            type="button"
            class="swatch"
            class:active={sameColor(color, live.style.color)}
            style:background={cssColor(color)}
            aria-pressed={sameColor(color, live.style.color)}
            aria-label={t(label)}
            title={t(label)}
            onclick={() => setStyle({ ...live.style, color: { ...color } })}
          ></button>
        {/each}
      </div>
      <label class="group slider">
        <span class="label">{t('live.width')}</span>
        <input
          type="range"
          min="10"
          max="1000"
          step="5"
          value={live.style.width}
          oninput={(e) => live.previewStyle({ ...live.style, width: Number(e.currentTarget.value) })}
          onchange={(e) => setStyle({ ...live.style, width: Number(e.currentTarget.value) })}
        />
        <span class="value">{t('editor.width.value', { mm: mm(live.style.width) })}</span>
      </label>
    </div>

    <div class="group" role="group" aria-label={t('live.pages.label')}>
      <button
        type="button"
        class="button small-button"
        aria-label={t('live.pages.previous')}
        title={t('live.pages.previous')}
        disabled={live.pageIndex === 0}
        onclick={() => command('previousPage')}><span aria-hidden="true">◀</span></button
      >
      <span class="page-position" aria-live="polite" data-page-position="{live.pageIndex + 1}/{live.pageCount}">{pageLabel}</span>
      <button
        type="button"
        class="button small-button"
        aria-label={t('live.pages.next')}
        title={t('live.pages.next')}
        onclick={() => command('nextPage')}><span aria-hidden="true">▶</span></button
      >
      <button type="button" class="button small-button" data-action="new-page" onclick={() => command('newPage')}>
        {t('live.pages.new')}
      </button>
    </div>

    <div class="group" role="group">
      <button
        type="button"
        class="button small-button"
        data-action="new-layer"
        title={t('live.layer.current', { n: live.layer + 1, count: Math.max(1, live.layers.length) })}
        onclick={() => command('newLayer')}
      >
        {t('live.layer.new')}
      </button>
      <button type="button" class="button small-button" data-action="clear-page" onclick={() => command('clearPage')}>
        {t('live.clear')}
      </button>
      <button
        type="button"
        class="button small-button toggle"
        data-action="board"
        class:active={live.view.board}
        aria-pressed={live.view.board}
        onclick={() => toggleBoard(actions)}
      >
        {t('live.board')}
      </button>
      <button
        type="button"
        class="button small-button toggle"
        data-action="laser"
        class:active={live.view.laser}
        aria-pressed={live.view.laser}
        onclick={() => toggleLaser(actions)}
      >
        <span class="laser-dot" aria-hidden="true"></span>
        {t('live.laser')}
      </button>
    </div>

    <div class="group" role="group" aria-label={t('live.orientation.label')} title={t('live.orientation.hint')} data-slot="orientation">
      {#each ORIENTATIONS as orientation (orientation)}
        <button
          type="button"
          class="button small-button toggle orientation"
          class:active={live.orientation === orientation}
          data-orientation={orientation}
          aria-pressed={live.orientation === orientation}
          aria-label={t(`tablet.orientation.${orientation}`)}
          title={t(`tablet.orientation.${orientation}`)}
          onclick={() => setOrientation(orientation)}
        >
          <OrientationIcon {orientation} size={26} />
        </button>
      {/each}
    </div>
  </div>

  <div class="live-main">
    <div class="live-preview">
      <LiveCanvas
        {live}
        {ink}
        mode="preview"
        {aspect}
        {latency}
        label={t('live.preview.label', { page: `${live.pageIndex + 1} / ${live.pageCount}` })}
        onview={setView}
      />
      {#if sessionState === 'waiting'}
        <div class="live-overlay" role="status">
          <span class="pulse" aria-hidden="true"></span>
          <strong>{t('live.waiting.title')}</strong>
          <p>{t('live.waiting.hint')}</p>
        </div>
      {:else if sessionState === 'reconnecting'}
        <p class="live-reconnecting" role="status">{t('live.reconnecting')}</p>
      {/if}
    </div>

    <aside class="live-side">
      <section class="side-block" aria-labelledby="live-projection-title">
        <h3 id="live-projection-title">{t('live.projection.title')}</h3>
        {#if monitorsLoaded && monitors.length === 0}
          <p class="muted small">{t('live.projection.none')}</p>
        {:else}
          <label class="field-row">
            <span class="label">{t('live.projection.monitor')}</span>
            <select bind:value={monitorKey} disabled={monitors.length === 0}>
              {#each monitors as monitor (monitor.key)}
                <option value={monitor.key}>
                  {t('live.projection.option', { name: monitorName(monitor), width: monitor.width, height: monitor.height })}
                </option>
              {/each}
            </select>
          </label>
        {/if}
        <div class="row">
          {#if live.presentation}
            <button type="button" class="button small-button" data-action="stop-projecting" onclick={stopProjecting}>
              {t('live.projection.stop')}
            </button>
          {:else}
            <button
              type="button"
              class="button small-button primary"
              data-action="project"
              disabled={projecting || monitors.length === 0}
              onclick={project}
            >
              {t('live.projection.project')}
            </button>
          {/if}
          <button
            type="button"
            class="button small-button ghost"
            aria-label={t('live.projection.refresh')}
            title={t('live.projection.refresh')}
            onclick={refreshMonitors}><span aria-hidden="true">↻</span></button
          >
        </div>
        <p class="muted small" data-presentation={live.presentation?.key ?? ''}>
          {live.presentation ? t('live.projection.on', { name: monitorName(live.presentation) }) : t('live.projection.off')}
        </p>
        {#if monitors.length === 1 && !live.presentation}
          <p class="muted small">{t('live.projection.single')}</p>
        {/if}
        {#if projectionError}
          <p class="error small" role="alert">{projectionError}</p>
        {/if}
      </section>

      <section class="side-block" aria-labelledby="live-narration-title" data-section="narration">
        <h3 id="live-narration-title">{t('live.narration.title')}</h3>
        <RecordPanel
          bind:open={narrationOpen}
          {recorder}
          device={microphone}
          ondevice={chooseMicrophone}
          onstart={startNarration}
          onstop={() => void stopNarration(true)}
          oncancel={() => void stopNarration(false)}
          openLabel={t('live.narration.open')}
          openHint={t('live.narration.openHint')}
          info={t('live.narration.hint')}
        />
        {#if narratedText}
          <p class="small" role="status" data-slot="narration-saved" data-pages={narrated?.pages ?? 0}>{narratedText}</p>
        {/if}
      </section>

      <section class="side-block" aria-labelledby="live-zoom-title">
        <h3 id="live-zoom-title">{t('live.zoom.label')}</h3>
        <div class="row">
          <button
            type="button"
            class="button small-button"
            aria-label={t('live.zoom.out')}
            title={t('live.zoom.out')}
            disabled={live.view.zoom <= MIN_ZOOM + 1e-3}
            onclick={() => zoomStep(actions, -1)}>−</button
          >
          <span class="zoom-level" aria-live="polite" data-zoom={live.view.zoom}>
            {t('live.zoom.level', { zoom: decimal.format(live.view.zoom) })}
          </span>
          <button
            type="button"
            class="button small-button"
            aria-label={t('live.zoom.in')}
            title={t('live.zoom.in')}
            disabled={live.view.zoom >= MAX_ZOOM - 1e-3}
            onclick={() => zoomStep(actions, 1)}>+</button
          >
          <button type="button" class="button small-button" data-action="whole-page" onclick={() => showWholePage(actions)}>
            {t('live.zoom.fit')}
          </button>
          <button type="button" class="button small-button" data-action="page-width" title={t('live.zoom.fitWidthTitle')} onclick={() => showPageWidth(actions)}>
            {t('live.zoom.fitWidth')}
          </button>
        </div>
        <p class="muted small">{t('live.preview.hint')}</p>
      </section>

      <section class="side-block" aria-labelledby="live-keys-title">
        <h3 id="live-keys-title">{t('live.keys.title')}</h3>
        <dl class="keys">
          {#each KEYS as key (key)}
            <dt><kbd>{t(`live.keys.list.${key}.keys`)}</kbd></dt>
            <dd>{t(`live.keys.list.${key}.action`)}</dd>
          {/each}
        </dl>
      </section>
    </aside>
  </div>
</section>

<style>
  .live-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  .live-top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .live-top h2 {
    margin-right: auto;
  }

  .live-state {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.875rem;
    font-weight: 600;
  }

  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--muted);
  }

  .live-state[data-state='live'] .dot {
    background: #1f9d55;
  }

  .live-state[data-state='waiting'] .dot,
  .live-state[data-state='reconnecting'] .dot {
    background: #e0a100;
  }

  .latency {
    display: inline-flex;
    align-items: baseline;
    gap: 0.35rem;
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }

  .saved {
    font-size: 0.875rem;
  }

  .live-banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 1rem;
    margin: 0.5rem 1rem;
  }

  .live-banner p {
    flex-basis: 100%;
    margin: 0;
  }

  .live-banner ul {
    flex-basis: 100%;
    margin: 0;
    padding-left: 1.2rem;
  }

  .live-toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1.25rem;
    padding: 0.4rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem;
    min-height: 2.25rem;
  }

  .label {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .small-button {
    padding: 0.3rem 0.65rem;
    font-size: 0.875rem;
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .button.orientation {
    padding: 0.1rem 0.25rem;
  }

  .button.toggle.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }

  .laser-dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: #ff1f1f;
    box-shadow: 0 0 0 3px rgb(255 31 31 / 25%);
  }

  .swatch {
    width: 1.6rem;
    height: 1.6rem;
    padding: 0;
    border: 2px solid var(--border);
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch.active {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .swatch:focus-visible,
  .slider input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .slider input {
    width: 8rem;
  }

  .value,
  .page-position,
  .zoom-level {
    min-width: 4.5rem;
    color: var(--muted);
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  .page-position {
    color: var(--text);
    font-weight: 600;
  }

  .live-main {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .live-preview {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    background: var(--surface-2);
  }

  .live-overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    max-width: min(32rem, 90%);
    padding: 1.25rem 1.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: center;
    transform: translate(-50%, -50%);
  }

  .live-overlay p {
    margin-bottom: 0;
    color: var(--muted);
  }

  .pulse {
    display: block;
    width: 0.9rem;
    height: 0.9rem;
    margin: 0 auto 0.75rem;
    border-radius: 50%;
    background: #2f6fdf;
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.15);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }

  .live-reconnecting {
    position: absolute;
    top: 0.75rem;
    left: 50%;
    margin: 0;
    padding: 0.4rem 0.9rem;
    border-radius: 999px;
    background: #e0a100;
    color: #1d232b;
    font-size: 0.875rem;
    font-weight: 600;
    transform: translateX(-50%);
  }

  .live-side {
    width: 290px;
    flex: none;
    overflow: auto;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  .side-block {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.85rem 1rem;
    border-bottom: 1px solid var(--border);
  }

  .side-block h3 {
    margin: 0;
    font-size: 0.95rem;
  }

  .side-block p {
    margin: 0;
  }

  .field-row {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .field-row select {
    width: 100%;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem;
  }

  .keys {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.3rem 0.75rem;
    margin: 0;
    font-size: 0.85rem;
  }

  .keys dt {
    white-space: nowrap;
  }

  .keys dd {
    margin: 0;
    color: var(--muted);
  }

  kbd {
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-2);
    font-family: inherit;
    font-size: 0.8rem;
  }
</style>
