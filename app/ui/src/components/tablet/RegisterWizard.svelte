<script lang="ts">
  /**
   * Registration wizard (SPEC §4.1). First a warning (registering makes the tablet unusable with
   * the phone app and with Inkspace until it is registered there again), then the steps the
   * service reports (`registerStep`: hold the button, searching, connecting, press the button),
   * the result with the tablet's information, or an error with a way forward. Cancel is offered
   * at every step. The step shown is in `[data-step]`.
   */
  import { tick, untrack } from 'svelte';
  import { errorMessage } from '../../lib/api';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { formatDate } from '../../lib/library-logic';
  import type { TabletStatus } from '../../lib/tablet';
  import { sizeInMillimetres, tablet } from '../../lib/tablet-state.svelte';
  import Modal from '../Modal.svelte';
  import Battery from './Battery.svelte';
  import TabletDrawing from './TabletDrawing.svelte';

  let { onclose }: { onclose: () => void } = $props();

  type Phase = 'warning' | 'running' | 'done' | 'error';
  type Step = 'holdButton' | 'searching' | 'connecting' | 'pressButton';

  const DRAWING = { holdButton: 'hold', searching: 'wait', connecting: 'wait', pressButton: 'press' } as const;
  /**
   * (supposé) How long the service takes, at most, to say something once asked: it takes a request at once when it is not
   * busy. It says nothing when it refuses with the error its status holds already (Bluetooth still off): its status
   * is published only when it changes (`Publisher::update`, crates/ardoise-device/src/service/worker.rs).
   */
  const NO_NEWS_MS = 1500;

  // A registration already under way (started before the wizard was opened) is followed, not restarted.
  const underWay = untrack(() => tablet.status?.connection === 'registering');
  let phase = $state<Phase>(underWay ? 'running' : 'warning');
  /** Why the registration did not succeed, translated. */
  let failure = $state<string | null>(null);
  let root = $state<HTMLElement | null>(null);

  // What the status looked like when the registration started, to tell new news from old.
  let sawStep = underWay;
  let startError: TabletStatus['error'] = null;
  let startRegistered = false;
  let leaving = false;
  let noNews: ReturnType<typeof setTimeout> | undefined;
  /** The status the wizard gave up on for want of news; a later one with a step says the registration began after all. */
  let quiet: TabletStatus | null = null;
  $effect(() => () => clearTimeout(noNews));

  const status = $derived(tablet.status);
  const step = $derived<Step>((status?.registerStep as Step | null) ?? 'holdButton');
  const device = $derived(status?.device ?? null);
  const info = $derived(status?.registered ?? null);

  function sameError(a: TabletStatus['error'], b: TabletStatus['error']): boolean {
    return a?.code === b?.code && a?.message === b?.message;
  }

  // Follows the service while a registration is under way.
  $effect(() => {
    if (!status || leaving) return;
    if (phase === 'error' && quiet !== null && status !== quiet && status.registerStep !== null) {
      quiet = null;
      failure = null;
      phase = 'running';
    }
    if (phase !== 'running') return;
    const reported = status.registerStep;
    if (reported === 'done') {
      phase = 'done';
    } else if (reported !== null) {
      sawStep = true;
    } else if (status.error && (sawStep || !sameError(status.error, startError))) {
      // The service gave up (timeout, refused, Bluetooth off…).
      failure = errorMessage(status.error);
      phase = 'error';
    } else if (sawStep) {
      // The steps stopped without an error: registered, or cancelled from elsewhere.
      if (status.registered && !startRegistered) phase = 'done';
      else onclose();
    }
  });

  // Each new screen puts the keyboard focus on its main button.
  $effect(() => {
    void phase;
    void step;
    void tick().then(() => root?.querySelector<HTMLElement>('[data-autofocus]')?.focus());
  });

  async function begin() {
    clearTimeout(noNews);
    quiet = null;
    sawStep = false;
    startError = status?.error ?? null;
    startRegistered = status?.registered != null;
    failure = null;
    phase = 'running';
    const asked = tablet.status;
    if (!(await tablet.register())) {
      failure = tablet.actionError;
      phase = 'error';
      return;
    }
    // Nothing came since the request, and the status holds an error: that error is the service's answer.
    noNews = setTimeout(() => {
      const now = tablet.status;
      if (phase === 'running' && !leaving && !sawStep && now !== null && now === asked && now.error) {
        failure = errorMessage(now.error);
        quiet = now;
        phase = 'error';
      }
    }, NO_NEWS_MS);
  }

  async function leave() {
    // The service reporting the end of the steps is then expected, not news.
    leaving = true;
    if (phase === 'running') await tablet.cancel();
    onclose();
  }
</script>

<Modal title={t('tablet.register.title')} onclose={leave}>
  <div bind:this={root} class="register-wizard" data-slot="register-wizard">
    {#if phase === 'warning'}
      <div class="wizard-step" data-step="warning">
        <h3>{t('tablet.register.warningTitle')}</h3>
        <p class="warning" role="note">{t('tablet.register.warning')}</p>
        <p>{t('tablet.register.warningPhone')}</p>
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-action="continue" data-autofocus onclick={begin}>
          {t('common.continue')}
        </button>
        <button type="button" class="button" data-action="cancel-register" onclick={leave}>
          {t('tablet.register.cancel')}
        </button>
      </div>
    {:else if phase === 'running'}
      <div class="wizard-step" data-step={step}>
        <TabletDrawing mode={DRAWING[step]} />
        <div aria-live="polite">
          <h3>{t(`tablet.register.step.${step}.title`)}</h3>
          <p>{t(`tablet.register.step.${step}.text`)}</p>
        </div>
      </div>
      <div class="actions">
        <button type="button" class="button" data-action="cancel-register" data-autofocus onclick={leave}>
          {t('tablet.register.cancel')}
        </button>
      </div>
    {:else if phase === 'done'}
      <div class="wizard-step" data-step="done">
        <TabletDrawing mode="done" />
        <div aria-live="polite">
          <h3>{t('tablet.register.step.done.title')}</h3>
          <p>{t('tablet.register.step.done.text', { name: device?.name ?? info?.name ?? '' })}</p>
        </div>
        {#if info}
          <dl class="facts">
            <dt>{t('tablet.info.name')}</dt>
            <dd>{device?.name ?? info.name}</dd>
            {#if device}
              <dt>{t('tablet.info.battery')}</dt>
              <dd><Battery percent={device.batteryPercent} charging={device.charging} /></dd>
              <dt>{t('tablet.info.firmware')}</dt>
              <dd>{device.firmware}</dd>
              <dt>{t('tablet.info.pages')}</dt>
              <dd>{device.storedPages}</dd>
              <dt>{t('tablet.info.model')}</dt>
              <dd>{device.model}</dd>
            {/if}
            <dt>{t('tablet.info.size')}</dt>
            <dd>{sizeInMillimetres(info.width, info.height)}</dd>
            <dt>{t('settings.tabletRegistered')}</dt>
            <dd>{formatDate(Date.parse(info.registered), currentLocale())}</dd>
          </dl>
        {/if}
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-action="finish" data-autofocus onclick={onclose}>
          {t('tablet.register.finish')}
        </button>
      </div>
    {:else}
      <div class="wizard-step" data-step="error" role="alert">
        <h3>{t('tablet.register.error.title')}</h3>
        {#if failure}
          <p class="error">{failure}</p>
        {/if}
        <p>{t('tablet.register.error.hint')}</p>
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-action="retry" data-autofocus onclick={begin}>
          {t('tablet.register.retry')}
        </button>
        <button type="button" class="button" data-action="cancel-register" onclick={leave}>
          {t('common.close')}
        </button>
      </div>
    {/if}
  </div>
</Modal>

<style>
  /* Not `.step`: the diagnostic's global rule (app.css) would lay a step's parts out side by side. */
  .wizard-step h3 {
    margin: 0.5rem 0 0.25rem;
  }

  .wizard-step p {
    margin: 0.4rem 0;
  }

  .warning {
    padding: 0.75rem 1rem;
    border: 1px solid var(--danger);
    border-radius: var(--radius);
    background: var(--danger-bg);
  }
</style>
