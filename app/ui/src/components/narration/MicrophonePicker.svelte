<script module lang="ts">
  // Starting and stopping a level-only capture go through one queue shared by every picker, so that
  // a caller about to record (which takes the microphone over) can wait until the monitor is gone:
  // stopping it later would stop the recording too.
  let operations: Promise<void> = Promise.resolve();

  function enqueue(task: () => Promise<unknown>): void {
    operations = operations.then(task).then(
      () => {},
      () => {},
    );
  }

  /** Resolves once every monitor start or stop requested so far is done. */
  export function monitorSettled(): Promise<void> {
    return operations;
  }
</script>

<script lang="ts">
  /**
   * Microphone list, level meter and capture status (with the « Windows bloque le micro »
   * explanation). It follows the capture under way, whoever started it (10 times a second), and
   * with `monitor` it starts a level-only capture while it is mounted and stops it when it goes
   * (`monitorSettled()` tells when). Choosing a microphone is reported to `onchange`: the caller
   * remembers it.
   */
  import { api, errorMessage } from '../../lib/api';
  import { t } from '../../lib/i18n.svelte';
  import type { Microphone, NarrationStatus } from '../../lib/video';
  import LevelMeter from './LevelMeter.svelte';

  let {
    device,
    onchange,
    monitor = false,
    disabled = false,
  }: {
    /** Chosen microphone id; null for the system's default. */
    device: string | null;
    onchange: (device: string | null) => void;
    /** Show a live level meter (starts a level-only capture while mounted). */
    monitor?: boolean;
    /** The choice is frozen (a recording is under way). */
    disabled?: boolean;
  } = $props();

  /** How often the capture's state is read, milliseconds. */
  const POLL_MS = 100;

  /** null while the list loads. */
  let microphones = $state.raw<Microphone[] | null>(null);
  let listError = $state<string | null>(null);
  let status = $state.raw<NarrationStatus | null>(null);
  let monitorError = $state<string | null>(null);

  const running = $derived(status !== null && status.state !== 'idle');
  const known = $derived(device === null || (microphones?.some((m) => m.id === device) ?? true));
  const level = $derived(running ? status!.level : { peak: 0, rms: 0 });
  /** What the status line says: a key and its parameters, or null for nothing. */
  const line = $derived.by((): { key: string; capture: string; params?: Record<string, string> } | null => {
    if (monitorError) return null;
    if (microphones && microphones.length === 0) return null;
    if (running) {
      switch (status!.capture) {
        case 'receiving':
          return { key: 'narration.status.receiving', capture: 'receiving' };
        case 'noData':
          return { key: 'narration.status.noData', capture: 'noData' };
        case 'silent':
          return { key: 'narration.status.silent', capture: 'silent' };
        case 'failed':
          return status!.failure
            ? { key: 'narration.status.failed', capture: 'failed', params: { detail: status!.failure } }
            : { key: 'narration.status.failedUnknown', capture: 'failed' };
        default:
          return { key: 'narration.status.waiting', capture: 'waiting' };
      }
    }
    return monitor ? { key: 'narration.status.starting', capture: 'starting' } : null;
  });

  /** The microphone does not deliver: the line is a warning, not a progress report. */
  const problem = $derived(line !== null && ['silent', 'noData', 'failed'].includes(line.capture));

  async function loadList() {
    try {
      microphones = await api.listMicrophones();
      listError = null;
    } catch (error) {
      microphones = [];
      listError = errorMessage(error);
    }
  }

  $effect(() => {
    void loadList();
  });

  // Follow the capture under way.
  $effect(() => {
    let alive = true;
    let reading = false;
    const poll = async () => {
      if (reading) return;
      reading = true;
      try {
        const next = await api.narrationStatus();
        if (alive) status = next;
      } catch {
        // The next reading may work.
      } finally {
        reading = false;
      }
    };
    void poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  // The level-only capture: restarted when the microphone changes, stopped with the picker.
  $effect(() => {
    if (!monitor) return;
    const chosen = device;
    monitorError = null;
    enqueue(async () => {
      try {
        await api.microphoneMonitor(chosen);
      } catch (error) {
        monitorError = errorMessage(error);
      }
    });
    return () => {
      // Only a level-only capture: a recording that took the microphone over is never touched.
      enqueue(() => api.microphoneMonitorStop());
    };
  });

  function choose(e: Event) {
    const value = (e.currentTarget as HTMLSelectElement).value;
    onchange(value === '' ? null : value);
  }
</script>

<div class="picker-box" data-capture={line?.capture ?? (running ? 'idle' : 'off')}>
  <div class="choose">
    <label class="select">
      <span class="label">{t('narration.picker.label')}</span>
      <select
        value={device ?? ''}
        disabled={disabled || microphones === null || microphones.length === 0}
        onchange={choose}
        data-field="microphone"
      >
        <option value="">{t('narration.picker.default')}</option>
        {#each microphones ?? [] as microphone (microphone.id)}
          <option value={microphone.id}>{microphone.name}</option>
        {/each}
        {#if !known && device !== null}
          <option value={device}>{t('narration.picker.missing')}</option>
        {/if}
      </select>
    </label>
    <button
      type="button"
      class="button ghost refresh"
      disabled={disabled}
      aria-label={t('narration.picker.refresh')}
      title={t('narration.picker.refresh')}
      onclick={loadList}><span aria-hidden="true">↻</span></button
    >
  </div>

  {#if microphones && microphones.length === 0}
    <p class="note" role="status" data-capture="none">{t('narration.picker.none')}</p>
  {:else if microphones}
    {#if monitor || running}
      <LevelMeter rms={level.rms} peak={level.peak} active={running} />
    {/if}
    {#if monitorError}
      <p class="note error" role="alert" data-capture="failed">{monitorError}</p>
    {:else if line}
      <p class="note" class:problem role="status" data-capture={line.capture}>{t(line.key, line.params)}</p>
    {/if}
  {/if}
  {#if listError}
    <p class="note error" role="alert">{listError}</p>
  {/if}
</div>

<style>
  .picker-box {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .choose {
    display: flex;
    align-items: flex-end;
    gap: 0.25rem;
  }

  .select {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 0.2rem;
    min-width: 0;
  }

  .label {
    color: var(--muted);
    font-size: 0.875rem;
  }

  select {
    width: 100%;
    min-width: 0;
    font-size: 0.875rem;
  }

  select:disabled {
    opacity: 0.6;
  }

  .refresh {
    padding: 0.35rem 0.55rem;
  }

  .refresh:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .note {
    margin: 0;
    font-size: 0.875rem;
  }

  .note.problem {
    padding: 0.5rem 0.65rem;
    border-left: 4px solid var(--danger);
    border-radius: 6px;
    background: var(--danger-bg);
  }

  .note.error {
    color: var(--danger);
  }
</style>
