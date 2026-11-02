<script lang="ts">
  /**
   * The code form of the viewer: the 6 characters the Ardoise PC shows, typed in any case (shown
   * in capitals), Enter or the button joins. Says why not: wrong code (with the attempts left),
   * too many attempts (with the wait, counting down), no answer from the PC.
   */
  import { onMount, tick } from 'svelte';
  import { t, tn } from '../lib/i18n.svelte';
  import { normalizeCode, type JoinResult } from '../lib/viewer-stream';

  let {
    onjoin,
    cookiesRefused = false,
  }: {
    /** Asks the PC to let this browser in. */
    onjoin: (code: string) => Promise<JoinResult>;
    /** The code was accepted but the browser did not keep the permission (cookies blocked). */
    cookiesRefused?: boolean;
  } = $props();

  let input = $state<HTMLInputElement>();
  let code = $state('');
  let busy = $state(false);
  let failure = $state.raw<Exclude<JoinResult, { ok: true }> | null>(null);
  /** When too many attempts end (ms since the epoch), and the clock the countdown reads. */
  let blockedUntil = $state(0);
  let now = $state(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;

  const waitSeconds = $derived(Math.max(0, Math.ceil((blockedUntil - now) / 1000)));
  const blocked = $derived(waitSeconds > 0);

  onMount(() => {
    input?.focus();
    return () => clearInterval(timer);
  });

  function startCountdown(seconds: number) {
    blockedUntil = Date.now() + seconds * 1000;
    now = Date.now();
    clearInterval(timer);
    timer = setInterval(() => {
      now = Date.now();
      if (now >= blockedUntil) {
        clearInterval(timer);
        failure = null;
        input?.focus();
      }
    }, 250);
  }

  function oninput(e: Event & { currentTarget: HTMLInputElement }) {
    code = normalizeCode(e.currentTarget.value);
    // Capitals, no space: what the field shows is what is sent.
    e.currentTarget.value = code;
  }

  async function submit(e: Event) {
    e.preventDefault();
    if (busy || blocked || code.length !== 6) return;
    busy = true;
    failure = null;
    const result = await onjoin(code);
    busy = false;
    if (result.ok) return;
    failure = result;
    if (result.reason === 'tooManyAttempts') {
      startCountdown(result.retryAfter);
    } else {
      // The field is enabled again after this update: then back to it.
      await tick();
      input?.select();
    }
  }

  const message = $derived.by(() => {
    if (failure?.reason === 'wrongCode') return tn('viewer.code.wrong', failure.remaining);
    if (failure?.reason === 'tooManyAttempts') return t('viewer.code.tooMany', { seconds: waitSeconds });
    if (failure?.reason === 'network') return t('viewer.code.network');
    if (failure?.reason === 'error') return t('viewer.code.error', { status: failure.status });
    if (cookiesRefused) return t('viewer.code.cookies');
    return '';
  });
</script>

<section class="panel" data-view="code">
  <h1 class="brand">{t('viewer.brand')}</h1>
  <p class="lead">{t('viewer.code.help')}</p>
  <form onsubmit={submit}>
    <label for="viewer-code">{t('viewer.code.label')}</label>
    <input
      id="viewer-code"
      bind:this={input}
      class="code"
      type="text"
      value={code}
      {oninput}
      maxlength="12"
      autocomplete="off"
      autocapitalize="characters"
      autocorrect="off"
      spellcheck="false"
      inputmode="text"
      enterkeyhint="go"
      disabled={busy}
      aria-invalid={message !== ''}
      aria-describedby="viewer-code-error"
    />
    <button type="submit" disabled={busy || blocked || code.length !== 6}>
      {busy ? t('viewer.code.checking') : t('viewer.code.submit')}
    </button>
  </form>
  <p id="viewer-code-error" class="error" role="alert" data-error={failure?.reason ?? (cookiesRefused ? 'cookies' : '')}>
    {message}
  </p>
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
    width: min(26rem, 100%);
    text-align: center;
  }

  .brand {
    margin: 0;
    font-size: 2.4rem;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .lead {
    margin: 0;
    color: var(--muted);
  }

  form {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 0.75rem;
    width: 100%;
  }

  label {
    color: var(--muted);
    font-size: 0.9rem;
  }

  .code {
    padding: 0.6rem 0.8rem;
    border: 2px solid var(--border);
    border-radius: 10px;
    background: var(--field);
    color: var(--text);
    font: 600 2rem ui-monospace, 'Cascadia Mono', Consolas, monospace;
    letter-spacing: 0.35em;
    text-align: center;
    text-transform: uppercase;
  }

  .code:focus-visible {
    outline: 3px solid var(--accent);
    outline-offset: 2px;
  }

  button {
    padding: 0.7rem 1rem;
    border: 0;
    border-radius: 10px;
    background: var(--accent);
    color: var(--accent-text);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .error {
    min-height: 1.5em;
    margin: 0;
    color: var(--danger);
  }
</style>
