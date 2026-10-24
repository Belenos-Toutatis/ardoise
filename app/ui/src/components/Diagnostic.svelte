<script lang="ts">
  import { onDestroy } from 'svelte';
  import { api, errorMessage } from '../lib/api';
  import {
    STEP_IDS,
    countSteps,
    instructionParams,
    type Decision,
    type Detail,
    type DiagnosticEvent,
    type DiagnosticReport,
    type Instruction,
    type Progress,
    type StepId,
    type StepStatus,
  } from '../lib/diagnostic';
  import { t } from '../lib/i18n.svelte';

  let { onclose }: { onclose: () => void } = $props();

  type StepState = 'pending' | 'running' | StepStatus;
  type Phase = 'idle' | 'running' | 'finished';

  const ICONS: Record<StepState, string> = {
    pending: '○',
    running: '▶',
    passed: '✓',
    failed: '✗',
    skipped: '↷',
  };

  const freshSteps = () => Object.fromEntries(STEP_IDS.map((id) => [id, 'pending'])) as Record<StepId, StepState>;

  let phase = $state<Phase>('idle');
  let deleteAfterSync = $state(true);
  let simulated = $state(false);
  let steps = $state<Record<StepId, StepState>>(freshSteps());
  let details = $state<Partial<Record<StepId, Detail[]>>>({});
  let prompt = $state<{ step: StepId; instruction: Instruction } | null>(null);
  let notice = $state<{ step: StepId; instruction: Instruction } | null>(null);
  let progress = $state<Progress | null>(null);
  let report = $state<DiagnosticReport | null>(null);
  let error = $state<string | null>(null);
  let primary = $state<HTMLButtonElement | null>(null);
  let destroyed = false;

  const running = $derived(phase === 'running');
  const summary = $derived(report ? countSteps(report) : null);

  // A new prompt takes the keyboard focus so that Enter answers it.
  $effect(() => {
    if (prompt && primary) primary.focus();
  });

  // Closing the panel stops a test in progress: nobody would be left to answer its prompts.
  onDestroy(() => {
    destroyed = true;
    if (phase === 'running') void api.diagnosticCancel().catch(() => {});
  });

  function clearLive() {
    prompt = null;
    notice = null;
    progress = null;
  }

  function handle(event: DiagnosticEvent) {
    if (destroyed) return;
    switch (event.type) {
      case 'started':
        clearLive();
        steps[event.step] = 'running';
        break;
      case 'prompt':
        notice = null;
        prompt = { step: event.step, instruction: event.instruction };
        break;
      case 'notify':
        notice = { step: event.step, instruction: event.instruction };
        break;
      case 'progress':
        progress = event.progress;
        break;
      case 'finished':
        clearLive();
        steps[event.outcome.step] = event.outcome.status;
        details[event.outcome.step] = event.outcome.details;
        break;
      case 'done':
        clearLive();
        // A step interrupted by "stop" never reports an outcome: it did not run.
        for (const id of STEP_IDS) if (steps[id] === 'running') steps[id] = 'pending';
        report = event.report;
        phase = 'finished';
        break;
      case 'error':
        clearLive();
        error = errorMessage({ code: event.code, message: event.message });
        phase = 'finished';
        break;
    }
  }

  async function start() {
    steps = freshSteps();
    details = {};
    report = null;
    error = null;
    clearLive();
    phase = 'running';
    try {
      await api.diagnosticStart({ simulated, deleteAfterSync }, handle);
    } catch (e) {
      error = errorMessage(e);
      phase = 'idle';
    }
  }

  async function answer(decision: Decision) {
    prompt = null;
    try {
      await api.diagnosticAnswer(decision);
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function stop() {
    prompt = null;
    try {
      await api.diagnosticCancel();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function progressText(p: Progress): string {
    switch (p.kind) {
      case 'waiting':
        return t('diagnostic.progress.waiting');
      case 'downloading':
        return t('diagnostic.progress.page', { page: p.page, total: p.total });
      case 'live':
        return t('diagnostic.progress.live', { samples: p.samples, seconds: Math.floor(p.elapsedMs / 1000) });
    }
  }

  /** Translated label of a detail, or the raw key when the tablet test reports one we do not know. */
  function detailLabel(key: string): string {
    const label = t(`diagnostic.details.${key}`);
    return label === `diagnostic.details.${key}` ? key : label;
  }

  /** Detail values written `@key` are fixed phrases to translate; others are shown as they are. */
  function detailValue(value: string): string {
    if (!value.startsWith('@')) return value;
    const key = `diagnostic.values.${value.slice(1)}`;
    const text = t(key);
    return text === key ? value.slice(1) : text;
  }
</script>

{#snippet instructionText(instruction: Instruction)}
  <p class="instruction">{t(`diagnostic.instruction.${instruction.kind}`, instructionParams(instruction))}</p>
  {#if instruction.kind === 'drawReferencePages'}
    <ol class="instruction-list">
      <li>{t('diagnostic.instruction.referenceItem1')}</li>
      <li>{t('diagnostic.instruction.referenceItem2')}</li>
      <li>{t('diagnostic.instruction.referenceItem3')}</li>
    </ol>
  {:else if instruction.kind === 'holdButtonToRegister'}
    <p class="callout">{t('diagnostic.instruction.registerWarning')}</p>
  {/if}
{/snippet}

<aside class="panel wide" aria-label={t('diagnostic.title')}>
  <header class="panel-header">
    <h2>{t('diagnostic.title')}</h2>
    <button class="button ghost" onclick={onclose}>{t('common.close')}</button>
  </header>

  <p class="muted">{t('diagnostic.intro')}</p>

  <div class="options">
    <label class="check">
      <input type="checkbox" bind:checked={deleteAfterSync} disabled={running} />
      <span>{t('diagnostic.deleteAfterSync')}</span>
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={simulated} disabled={running} />
      <span>{t('diagnostic.simulated')}</span>
    </label>
  </div>

  {#if phase !== 'finished'}
    <button class="button primary" onclick={start} disabled={running}>{t('diagnostic.start')}</button>
  {/if}

  {#if error}
    <p class="banner error inline" role="alert">{t('diagnostic.error', { message: error })}</p>
  {/if}

  <ol class="steps" aria-label={t('diagnostic.stepsLabel')}>
    {#each STEP_IDS as id (id)}
      <li class="step {steps[id]}" data-step={id} data-status={steps[id]}>
        <span class="step-icon" aria-hidden="true">{ICONS[steps[id]]}</span>
        <div class="step-body">
          <div class="step-head">
            <span class="step-name">{t(`diagnostic.steps.${id}`)}</span>
            <span class="step-status">{t(`diagnostic.status.${steps[id]}`)}</span>
          </div>
          {#if details[id]?.length}
            <dl class="facts step-details">
              {#each details[id] ?? [] as detail (detail.key)}
                <dt>{detailLabel(detail.key)}</dt>
                <dd>{detailValue(detail.value)}</dd>
              {/each}
            </dl>
          {/if}
        </div>
      </li>
    {/each}
  </ol>

  <div class="live" aria-live="polite">
    {#if prompt}
      <section class="card-prompt" aria-label={t(`diagnostic.steps.${prompt.step}`)}>
        <h3>{t(`diagnostic.steps.${prompt.step}`)}</h3>
        {@render instructionText(prompt.instruction)}
        <div class="actions">
          <button class="button primary" bind:this={primary} onclick={() => answer('continue')}>
            {t('diagnostic.actions.continue')}
          </button>
          <button class="button" onclick={() => answer('skip')}>{t('diagnostic.actions.skip')}</button>
          <button class="button ghost danger" onclick={() => answer('cancel')}>{t('diagnostic.actions.stop')}</button>
        </div>
      </section>
    {:else if running}
      {#if notice}
        <section class="card-prompt notify" aria-label={t(`diagnostic.steps.${notice.step}`)}>
          <h3>{t(`diagnostic.steps.${notice.step}`)}</h3>
          {@render instructionText(notice.instruction)}
        </section>
      {/if}
      {#if progress}
        <p class="progress">{progressText(progress)}</p>
      {/if}
      <div class="actions">
        <button class="button ghost danger" onclick={stop}>{t('diagnostic.actions.stop')}</button>
      </div>
    {/if}
  </div>

  {#if report && summary}
    <section class="summary" role="status">
      <p class="summary-line">{t('diagnostic.summary', summary)}</p>
      {#if report.cancelled}
        <p class="callout">{t('diagnostic.cancelled')}</p>
      {/if}
      {#if report.logPath}
        <p class="small muted path-text">{t('diagnostic.log', { path: report.logPath })}</p>
      {/if}
    </section>
  {/if}

  {#if phase === 'finished'}
    <div class="actions">
      <button class="button primary" onclick={start}>{t('diagnostic.again')}</button>
      <button class="button" onclick={onclose}>{t('common.close')}</button>
    </div>
  {/if}
</aside>
