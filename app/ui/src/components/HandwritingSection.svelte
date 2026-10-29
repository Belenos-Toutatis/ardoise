<script lang="ts">
  /**
   * Settings: handwriting recognition. The language (automatic, French, English; shared by every
   * PC), the recognizers installed on this PC, how to add Windows' handwriting pack of a missing
   * language (Ardoise never downloads it), and the background indexing with its stop button.
   */
  import { onMount } from 'svelte';
  import { api, errorMessage, followHandwritingStatus, genericError } from '../lib/api';
  import {
    HANDWRITING_SETTINGS,
    missingRecognizer,
    statusNotice,
    type HandwritingInfo,
    type HandwritingSetting,
    type IndexStatus,
  } from '../lib/handwriting';
  import { currentLocale, t } from '../lib/i18n.svelte';

  /** Null until known. */
  let info = $state.raw<HandwritingInfo | null>(null);
  let status = $state.raw<IndexStatus | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const missing = $derived(info ? missingRecognizer(info, currentLocale()) : null);
  const notice = $derived(status ? statusNotice(status) : null);
  const running = $derived(status?.state === 'indexing' || status?.state === 'paused');

  onMount(() => {
    let alive = true;
    const unfollow = followHandwritingStatus((next) => {
      status = next;
    });
    api
      .handwritingInfo()
      .then((next) => {
        if (!alive) return;
        info = next;
        status ??= next.status;
      })
      .catch((e) => {
        if (alive) error = errorMessage(e);
      });
    return () => {
      alive = false;
      unfollow();
    };
  });

  async function run(action: () => Promise<void>) {
    error = null;
    busy = true;
    try {
      await action();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function chooseLanguage(event: Event) {
    const language = (event.currentTarget as HTMLSelectElement).value as HandwritingSetting;
    void run(async () => {
      info = await api.setHandwritingLanguage(language);
      status = info.status;
    });
  }

  function languageName(language: 'fr' | 'en'): string {
    return t(`handwriting.languages.${language}`);
  }
</script>

<section class="settings-section" aria-labelledby="settings-handwriting">
  <h3 id="settings-handwriting">{t('handwriting.settings.title')}</h3>
  <p class="muted small">{t('handwriting.settings.intro')}</p>

  {#if info}
    {#if !info.supported}
      <p class="small" data-slot="unsupported">{t('handwriting.settings.unsupported')}</p>
    {:else}
      <label class="field">
        <span>{t('handwriting.settings.language')}</span>
        <select value={info.setting} disabled={busy} onchange={chooseLanguage} data-slot="language">
          {#each HANDWRITING_SETTINGS as setting (setting)}
            <option value={setting}>
              {setting === 'auto'
                ? t('handwriting.settings.auto', { language: info.language ? languageName(info.language) : '—' })
                : languageName(setting)}
            </option>
          {/each}
        </select>
      </label>
      <p class="muted small">{t('handwriting.settings.autoHint')}</p>

      <dl class="facts">
        <dt>{t('handwriting.settings.installed')}</dt>
        <dd data-slot="recognizers">
          {#if info.recognizers.length === 0}
            {t('handwriting.settings.none')}
          {:else}
            <ul class="recognizers">
              {#each info.recognizers as recognizer (recognizer.name)}
                <li>{recognizer.name}</li>
              {/each}
            </ul>
          {/if}
        </dd>
      </dl>

      {#if missing}
        <div class="handwriting-missing" data-slot="missing">
          <p class="small">{t('handwriting.settings.missing', { language: languageName(missing) })}</p>
          <p class="muted small">{t(`handwriting.settings.install.${missing}`)}</p>
        </div>
      {/if}

      {#if notice}
        <p class="small" role="status" data-slot="status">{t(notice.key, notice.values)}</p>
      {/if}
      {#if status && status.state !== 'unavailable'}
        <div class="actions">
          {#if status.state === 'stopped'}
            <button type="button" class="button" disabled={busy} onclick={() => run(() => api.handwritingResume())}>
              {t('handwriting.settings.resume')}
            </button>
          {:else if running}
            <button
              type="button"
              class="button"
              disabled={busy}
              onclick={() =>
                run(async () => {
                  status = await api.handwritingStop();
                })}
            >
              {t('handwriting.settings.stop')}
            </button>
          {/if}
        </div>
      {/if}
      <p class="muted small">{t('handwriting.settings.privacy')}</p>
    {/if}
  {/if}
  {#if error}
    <p class="error" role="alert">{genericError(error)}</p>
  {/if}
</section>

<style>
  .recognizers {
    margin: 0;
    padding-left: 1.1rem;
  }

  .handwriting-missing {
    margin: 0.5rem 0;
    padding: 0.5rem 0.7rem;
    border-left: 3px solid var(--accent);
    background: var(--surface);
  }

  .handwriting-missing p {
    margin: 0.2rem 0;
  }
</style>
