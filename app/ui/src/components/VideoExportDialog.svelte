<script lang="ts">
  /**
   * Exports the replay of a note to MP4, GIF, WebP or WebM. The format is chosen among cards that
   * say what each one is good for and what it cannot do; the estimate (size, length, export time)
   * is refreshed after every change; the destination is asked last, with the native dialog.
   * `onrunning` tells the caller while an export is under way (the window must not close then).
   */
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../lib/api';
  import { pickExportFile } from '../lib/dialogs';
  import { exportStem } from '../lib/export-options';
  import { currentLocale, t } from '../lib/i18n.svelte';
  import type { ExportSource } from '../lib/note';
  import {
    FRAME_RATES,
    LONG_EXPORT_MS,
    MAX_HOLD_SECONDS,
    MAX_TARGET_SECONDS,
    SPEEDS,
    VIDEO_FORMATS,
    VIDEO_RESOLUTIONS,
    defaultFrameRate,
    roundEstimate,
    splitSeconds,
    supportsSound,
    supportsTransparency,
    validHold,
    type NarrationInfo,
    type VideoEstimate,
    type VideoExportOptions,
    type VideoExportResult,
    type VideoFormat,
    type VideoProgress,
    type VideoResolution,
    type VideoTiming,
  } from '../lib/video';
  import { loadVideoOptions, saveVideoOptions } from '../lib/video-options';
  import Modal from './Modal.svelte';

  let {
    source,
    subject,
    onclose,
    onrunning,
  }: {
    /** The note (with the editor's unsaved strokes) and its file, which holds the narration. */
    source: ExportSource;
    /** The note's title, as a sentence part. */
    subject: string;
    onclose: () => void;
    /** Called with true when an export starts and false when it ends (or the dialog goes away). */
    onrunning?: (running: boolean) => void;
  } = $props();

  /** Prefix of the element ids that tie each format to its description. */
  const uid = $props.id();
  /** Rows of the "Which format?" comparison, in `videoExport.compare.rows`. */
  const COMPARE_ROWS = ['plays', 'sound', 'transparent', 'size', 'time', 'bestFor'] as const;
  /** Wait this long after the last change before asking for a new estimate, milliseconds. */
  const ESTIMATE_DELAY_MS = 250;

  const initial = loadVideoOptions();
  let format = $state<VideoFormat>(initial.format);
  let resolution = $state<VideoResolution>(initial.resolution);
  let frameRate = $state(initial.frameRate);
  let timingKind = $state<VideoTiming['kind']>(initial.timing.kind);
  let speed = $state(initial.timing.kind === 'speed' ? initial.timing.speed : 1);
  // Number inputs hold null while they are empty.
  let seconds = $state<number | null>(initial.timing.kind === 'duration' ? initial.timing.seconds : 30);
  let trimPauses = $state(initial.trimPauses);
  let background = $state(initial.background);
  let holdEnd = $state<number | null>(initial.holdEndSeconds);
  let narration = $state(initial.narration);

  /** The note's narration, once known; null when it has none. */
  let narrationInfo = $state.raw<NarrationInfo | null>(null);
  let estimate = $state.raw<VideoEstimate | null>(null);
  let estimateError = $state<string | null>(null);
  let phase = $state<'idle' | 'running' | 'done'>('idle');
  let progress = $state.raw<VideoProgress | null>(null);
  let result = $state.raw<VideoExportResult | null>(null);
  let error = $state<string | null>(null);
  /** The last export was cancelled by the user: a neutral notice, not an error. */
  let cancelled = $state(false);
  /** Cancellation was asked and the export has not stopped yet. */
  let cancelling = $state(false);

  const running = $derived(phase === 'running');
  const canBeTransparent = $derived(supportsTransparency(format));
  const hasSound = $derived(supportsSound(format));
  /** The narration goes into the video, and imposes the speed and pauses it was recorded with. */
  const narrationUsed = $derived(narrationInfo !== null && narration && hasSound);
  const timingValid = $derived(
    timingKind === 'speed' ||
      narrationUsed ||
      (seconds !== null && Number.isFinite(seconds) && seconds >= 1 && seconds <= MAX_TARGET_SECONDS),
  );
  const holdValid = $derived(validHold(holdEnd));
  const trimShown = $derived(narrationUsed && narrationInfo ? narrationInfo.maxPauseMs !== null : trimPauses);

  /** The user's timing choice; a speed of 1 stands in for a target duration that is not valid yet. */
  function timing(): VideoTiming {
    return timingKind === 'duration' && seconds !== null && Number.isFinite(seconds) && seconds >= 1
      ? { kind: 'duration', seconds }
      : { kind: 'speed', speed: timingKind === 'speed' ? speed : 1 };
  }

  /** What the dialog asks for, or null while a field is invalid. */
  const request = $derived.by<VideoExportOptions | null>(() => {
    if (!timingValid || !holdValid || holdEnd === null) return null;
    return {
      format,
      resolution,
      frameRate,
      timing: timing(),
      trimPauses,
      background: canBeTransparent ? background : true,
      holdEndSeconds: holdEnd,
      narration: narrationUsed,
    };
  });

  /** The choices to remember: the request, but with the narration wish kept as the user left it. */
  function remembered(options: VideoExportOptions): VideoExportOptions {
    return { ...options, narration };
  }

  onMount(() => {
    let alive = true;
    api
      .narrationInfo(source.path)
      .then((info) => {
        if (alive) narrationInfo = info;
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  });

  // The estimate follows every change, a moment after the last one.
  let estimateRequest = 0;
  $effect(() => {
    const options = request;
    if (!options) {
      estimate = null;
      return;
    }
    const timer = setTimeout(() => void refreshEstimate(options), ESTIMATE_DELAY_MS);
    return () => clearTimeout(timer);
  });

  async function refreshEstimate(options: VideoExportOptions) {
    const mine = ++estimateRequest;
    try {
      const next = await api.estimateVideoExport({ ...source }, options);
      if (mine !== estimateRequest) return;
      estimate = next;
      estimateError = null;
    } catch (e) {
      if (mine !== estimateRequest) return;
      estimate = null;
      estimateError = errorMessage(e);
    }
  }

  function chooseFormat(next: VideoFormat) {
    // The default frame rate follows the format unless the user picked another one.
    if (frameRate === defaultFrameRate(format)) frameRate = defaultFrameRate(next);
    if (!supportsTransparency(next)) background = true;
    format = next;
  }

  function timeText(ms: number): string {
    const { minutes, seconds: s } = splitSeconds(ms);
    if (minutes === 0) return t('videoExport.time.seconds', { seconds: s });
    return s === 0
      ? t('videoExport.time.minutes', { minutes })
      : t('videoExport.time.minutesSeconds', { minutes, seconds: s });
  }

  /** "about …" after the estimate: rounded, and never "0 s". */
  function estimateText(ms: number): string {
    return ms < 1000 ? t('videoExport.time.lessThanSecond') : timeText(roundEstimate(ms));
  }

  function speedText(value: number): string {
    return t('videoExport.speedValue', { speed: new Intl.NumberFormat(currentLocale()).format(value) });
  }

  async function run() {
    if (!request || running) return;
    error = null;
    cancelled = false;
    let chosen: string | null;
    try {
      chosen = await pickExportFile(t('videoExport.pickFile'), `${exportStem(source.path)}.${format}`, format);
    } catch (e) {
      error = errorMessage(e);
      return;
    }
    if (!chosen) return;
    const options = request;
    saveVideoOptions(remembered(options));
    phase = 'running';
    cancelling = false;
    result = null;
    progress = null;
    try {
      result = await api.exportVideo({ ...source }, options, chosen, (next) => {
        if (phase === 'running') progress = next;
      });
      phase = 'done';
    } catch (e) {
      if (typeof e === 'object' && e !== null && 'code' in e && e.code === 'cancelled') cancelled = true;
      else error = errorMessage(e);
      phase = 'idle';
    } finally {
      cancelling = false;
    }
  }

  /** Asks the desktop app to stop the export; `run` then ends with the `cancelled` rejection. */
  async function cancel() {
    // Once every frame is written the file is being finalised: it is too late to cancel.
    if (!running || cancelling || finalizing) return;
    cancelling = true;
    try {
      await api.cancelVideoExport();
    } catch (e) {
      cancelling = false;
      error = errorMessage(e);
    }
  }

  /** Escape and the Cancel button: stop the export under way, else close the dialog. */
  function close() {
    if (running) void cancel();
    else onclose();
  }

  const percent = $derived(
    progress && progress.framesTotal > 0 ? Math.floor((100 * progress.framesDone) / progress.framesTotal) : 0,
  );
  /** Every frame is written: the file is being finalised, and cancelling no longer makes sense. */
  const finalizing = $derived(
    progress !== null && progress.framesTotal > 0 && progress.framesDone >= progress.framesTotal,
  );

  // The caller holds the window close while this runs.
  $effect(() => {
    const active = running;
    onrunning?.(active);
    return () => {
      if (active) onrunning?.(false);
    };
  });
</script>

<Modal title={t('videoExport.title')} onclose={close}>
  <div aria-busy={running}>
    <p class="muted">{subject}</p>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('videoExport.format')}</legend>
      <div class="export-choices" role="radiogroup" aria-label={t('videoExport.format')}>
        {#each VIDEO_FORMATS as value (value)}
          <!-- The whole card selects the format; the radio is named by the format alone and
               described by its strengths and limits. -->
          <label class="choice format-card" class:selected={format === value}>
            <span class="format-name">
              <input
                type="radio"
                name="video-format"
                {value}
                checked={format === value}
                onchange={() => chooseFormat(value)}
                aria-labelledby="{uid}-{value}-name"
                aria-describedby="{uid}-{value}-pros {uid}-{value}-cons"
              />
              <span class="choice-name" id="{uid}-{value}-name">{t(`videoExport.formats.${value}`)}</span>
            </span>
            <span class="format-pros" id="{uid}-{value}-pros">
              <strong>{t('export.prosLabel')}</strong>
              {t(`videoExport.formatPros.${value}`)}
            </span>
            <span class="format-cons" id="{uid}-{value}-cons">
              <strong>{t('export.consLabel')}</strong>
              {t(`videoExport.formatCons.${value}`)}
            </span>
          </label>
        {/each}
      </div>
      <details class="format-compare">
        <summary>{t('videoExport.compare.title')}</summary>
        <table>
          <thead>
            <tr>
              <th scope="col"><span class="visually-hidden">{t('export.compare.criterion')}</span></th>
              {#each VIDEO_FORMATS as value (value)}
                <th scope="col">{value.toUpperCase()}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each COMPARE_ROWS as row (row)}
              <tr>
                <th scope="row">{t(`videoExport.compare.rows.${row}.label`)}</th>
                {#each VIDEO_FORMATS as value (value)}
                  <td>{t(`videoExport.compare.rows.${row}.${value}`)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted small">{t('videoExport.compare.advice')}</p>
      </details>
    </fieldset>

    <fieldset class="export-fieldset" disabled={running}>
      <legend>{t('videoExport.options')}</legend>

      <label class="export-row">
        <span>{t('videoExport.resolution')}</span>
        <select bind:value={resolution}>
          {#each VIDEO_RESOLUTIONS as value (value)}
            <option {value}>{t(`videoExport.resolutions.${value}`)}</option>
          {/each}
        </select>
      </label>
      {#if resolution === 'page'}
        <p class="muted small export-hint">{t('videoExport.pageHint')}</p>
      {/if}

      <label class="export-row">
        <span>{t('videoExport.frameRate')}</span>
        <select bind:value={frameRate}>
          {#each FRAME_RATES as rate (rate)}
            <option value={rate}>{t('videoExport.fps', { count: rate })}</option>
          {/each}
        </select>
      </label>

      <div class="export-row video-timing" role="radiogroup" aria-label={t('videoExport.timing')}>
        <span>{t('videoExport.timing')}</span>
        <label class="choice">
          <input type="radio" name="video-timing" value="speed" bind:group={timingKind} disabled={narrationUsed} />
          {t('videoExport.speed')}
        </label>
        <select
          bind:value={speed}
          disabled={narrationUsed || timingKind !== 'speed'}
          aria-label={t('videoExport.speed')}
        >
          {#each SPEEDS as value (value)}
            <option {value}>{speedText(value)}</option>
          {/each}
        </select>
        <label class="choice">
          <input type="radio" name="video-timing" value="duration" bind:group={timingKind} disabled={narrationUsed} />
          {t('videoExport.targetDuration')}
        </label>
        <input
          class="text-input video-seconds"
          type="number"
          min="1"
          max={MAX_TARGET_SECONDS}
          step="1"
          bind:value={seconds}
          disabled={narrationUsed || timingKind !== 'duration'}
          aria-label="{t('videoExport.targetDuration')} ({t('videoExport.seconds')})"
        />
        <span class="muted">{t('videoExport.seconds')}</span>
      </div>
      {#if !timingValid}
        <p class="error small export-hint" role="alert">{t('videoExport.invalidTiming')}</p>
      {/if}

      <label class="check">
        <input
          type="checkbox"
          checked={trimShown}
          disabled={narrationUsed}
          onchange={(event) => (trimPauses = event.currentTarget.checked)}
        />
        {t('videoExport.trimPauses')}
      </label>
      <p class="muted small">{t('videoExport.trimPausesHint')}</p>

      {#if canBeTransparent}
        <div class="export-row" role="radiogroup" aria-label={t('videoExport.background')}>
          <span>{t('videoExport.background')}</span>
          <label class="choice">
            <input type="radio" name="video-background" checked={background} onchange={() => (background = true)} />
            {t('videoExport.backgroundPaper')}
          </label>
          <label class="choice">
            <input type="radio" name="video-background" checked={!background} onchange={() => (background = false)} />
            {t('videoExport.backgroundTransparent')}
          </label>
        </div>
      {:else}
        <p class="muted small video-background-fixed">{t('videoExport.backgroundFixed')}</p>
      {/if}

      <label class="export-row">
        <span>{t('videoExport.holdEnd')}</span>
        <input
          class="text-input video-seconds"
          type="number"
          min="0"
          max={MAX_HOLD_SECONDS}
          step="1"
          bind:value={holdEnd}
        />
        <span class="muted">{t('videoExport.seconds')}</span>
      </label>
      {#if !holdValid}
        <p class="error small export-hint" role="alert">{t('videoExport.invalidHold')}</p>
      {/if}

      {#if narrationInfo && hasSound}
        <label class="check">
          <input type="checkbox" bind:checked={narration} />
          {t('videoExport.narration')}
        </label>
        <p class="muted small">
          {narrationUsed
            ? t('videoExport.narrationLocked', { speed: new Intl.NumberFormat(currentLocale()).format(narrationInfo.speed) })
            : t('videoExport.narrationHint')}
        </p>
      {:else if narrationInfo}
        <p class="muted small">{t('videoExport.narrationNoSound')}</p>
      {/if}
    </fieldset>

    <div class="video-estimate" aria-live="polite">
      {#if estimate}
        <p>{t('videoExport.estimate.size', { width: estimate.width, height: estimate.height })}</p>
        <p>
          {t('videoExport.estimate.length', { duration: timeText(estimate.durationMs), frames: estimate.frames })}
        </p>
        {#if estimate.narrationMs !== null}
          <p>{t('videoExport.estimate.narration', { duration: timeText(estimate.narrationMs) })}</p>
        {:else if timingKind === 'duration'}
          <p>{t('videoExport.estimate.speed', { speed: new Intl.NumberFormat(currentLocale(), { maximumFractionDigits: 2 }).format(estimate.speed) })}</p>
        {/if}
        <p class="export-status">{t('videoExport.estimate.time', { time: estimateText(estimate.exportMs) })}</p>
        {#if estimate.exportMs > LONG_EXPORT_MS}
          <p class="banner notice inline" role="note">{t('videoExport.estimate.long')}</p>
        {/if}
      {:else if estimateError}
        <p class="error small">{t('videoExport.estimate.failed', { message: estimateError })}</p>
      {:else if request}
        <p class="muted small">{t('videoExport.estimate.pending')}</p>
      {/if}
    </div>

    <p class="muted small">{t('videoExport.destination')}</p>

    {#if running}
      <div class="video-progress" role="status">
        <p class="export-status">
          {cancelling ? t('videoExport.cancelling') : finalizing ? t('videoExport.finalizing') : t('videoExport.running')}
        </p>
        {#if progress}
          <progress
            max={progress.framesTotal}
            value={progress.framesDone}
            aria-label={t('videoExport.progressLabel')}
          ></progress>
          <p class="muted small">
            {t('videoExport.progress', { done: progress.framesDone, total: progress.framesTotal, percent })}
          </p>
        {/if}
      </div>
    {:else if phase === 'done' && result}
      <div class="export-result" role="status">
        <p class="export-status">{t('videoExport.done')}</p>
        <code>{result.path}</code>
        <p class="muted small">
          {t('videoExport.resultLength', { duration: timeText(result.durationMs) })}
          ·
          {t('videoExport.resultSound', { sound: result.audio ? t('videoExport.soundYes') : t('videoExport.soundNo') })}
        </p>
      </div>
    {/if}
    {#if cancelled}
      <p class="export-notice muted" role="status">{t('videoExport.cancelled')}</p>
    {/if}
    {#if error}
      <p class="banner error inline" role="alert">{t('videoExport.failed', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="button" class="button primary" data-autofocus disabled={running || !request} onclick={run}>
        {phase === 'done' ? t('videoExport.again') : running ? t('videoExport.running') : t('videoExport.run')}
      </button>
      {#if running}
        <button type="button" class="button" disabled={cancelling || finalizing} onclick={cancel}>
          {cancelling ? t('videoExport.cancelling') : finalizing ? t('videoExport.finalizing') : t('videoExport.cancelRun')}
        </button>
      {:else}
        <button type="button" class="button" onclick={onclose}>
          {phase === 'done' ? t('common.close') : t('library.cancel')}
        </button>
      {/if}
    </div>
  </div>
</Modal>
