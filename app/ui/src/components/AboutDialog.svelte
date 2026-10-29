<script lang="ts">
  /**
   * The « À propos » screen (SPEC §9): Ardoise and its version, its licence (GPL-3.0-or-later,
   * in plain words and in full), the credit to Tuhi, Cisco's notice and whether OpenH264 is
   * installed here, and the components Ardoise is built with (searchable) with their licences
   * in full. The long texts come from the Rust side (`about.rs`) and are only fetched when the
   * user asks to see them.
   */
  import { onMount } from 'svelte';
  import { filterComponents, type AboutInfo, type AboutText } from '../lib/about';
  import { api, errorMessage } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import type { OpenH264State } from '../lib/video';
  import Modal from './Modal.svelte';

  let { version, onclose }: { version: string; onclose: () => void } = $props();

  let info = $state.raw<AboutInfo | null>(null);
  /** Null until known (or when it cannot be read: the rest of the screen does not depend on it). */
  let codec = $state.raw<OpenH264State | null>(null);
  let error = $state<string | null>(null);
  let query = $state('');

  /** The long texts: loaded on first display, then kept. */
  let texts = $state<Partial<Record<AboutText, string>>>({});
  let shown = $state<Record<AboutText, boolean>>({ license: false, notices: false });
  let loading = $state<Record<AboutText, boolean>>({ license: false, notices: false });

  const visible = $derived(info ? filterComponents(info.components, query) : []);

  onMount(() => {
    let alive = true;
    api
      .aboutInfo()
      .then((next) => {
        if (alive) info = next;
      })
      .catch((e) => {
        if (alive) error = errorMessage(e);
      });
    api
      .openH264State()
      .then((next) => {
        if (alive) codec = next;
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  });

  async function toggle(which: AboutText) {
    if (shown[which]) {
      shown[which] = false;
      return;
    }
    shown[which] = true;
    if (texts[which] !== undefined) return;
    error = null;
    loading[which] = true;
    try {
      texts[which] = await api.aboutText(which);
    } catch (e) {
      shown[which] = false;
      error = errorMessage(e);
    } finally {
      loading[which] = false;
    }
  }
</script>

<Modal title={t('about.title')} {onclose}>
  <div class="about" data-dialog="about">
    <p class="about-name" data-slot="version">
      <strong>{t('app.name')}</strong>
      {t('about.version', { version: info?.version ?? version })}
    </p>
    <p class="muted">{t('app.tagline')}</p>

    <section aria-labelledby="about-license">
      <h3 id="about-license">{t('about.license.title')}</h3>
      <p data-slot="license">{t('about.license.line', { license: info?.license ?? 'GPL-3.0-or-later' })}</p>
      <p class="muted small" data-slot="license-meaning">{t('about.license.meaning')}</p>
      <button
        type="button"
        class="button"
        data-action="toggle-license"
        aria-expanded={shown.license}
        aria-controls="about-license-text"
        onclick={() => toggle('license')}
      >
        {shown.license ? t('about.license.hide') : t('about.license.show')}
      </button>
      {#if shown.license}
        <div id="about-license-text">
          {#if loading.license}
            <p class="muted small" role="status">{t('common.loading')}</p>
          {:else if texts.license !== undefined}
            <!-- A scrollable text must be reachable and scrollable with the keyboard. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <pre class="about-text" tabindex="0" aria-label={t('about.license.textLabel')} data-slot="license-text">{texts.license}</pre>
          {/if}
        </div>
      {/if}
    </section>

    <section aria-labelledby="about-credits">
      <h3 id="about-credits">{t('about.credits.title')}</h3>
      <p data-slot="tuhi">{t('about.credits.tuhi')}</p>
    </section>

    <section aria-labelledby="about-cisco">
      <h3 id="about-cisco">{t('about.cisco.title')}</h3>
      <!-- Cisco's notice, as the app sends it. -->
      <p class="openh264-notice" data-slot="cisco-notice">{codec?.notice ?? t('about.cisco.notice')}</p>
      <p class="small" data-slot="cisco-download">{t('about.cisco.download')}</p>
      {#if codec}
        <p class="small" data-slot="cisco-status">
          {#if codec.status === 'installed'}
            {t('about.cisco.installed', { version: codec.version })}
          {:else}
            {t(`about.cisco.${codec.status}`)}
          {/if}
        </p>
      {/if}
      <p class="muted small">{t('about.cisco.license')}</p>
    </section>

    <section aria-labelledby="about-components">
      <h3 id="about-components">{t('about.components.title')}</h3>
      <p class="muted small">{t('about.components.intro')}</p>
      {#if info}
        <input
          class="text-input"
          type="search"
          bind:value={query}
          aria-label={t('about.components.search')}
          placeholder={t('about.components.placeholder')}
        />
        <p class="muted small" role="status" data-slot="component-count">
          {t('about.components.count', { shown: visible.length, total: info.components.length })}
        </p>
        {#if visible.length > 0}
          <!-- A scrollable list must be reachable and scrollable with the keyboard. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <div class="about-components" tabindex="0" role="region" aria-label={t('about.components.title')}>
            <table>
              <thead>
                <tr>
                  <th scope="col">{t('about.components.name')}</th>
                  <th scope="col">{t('about.components.versionColumn')}</th>
                  <th scope="col">{t('about.components.licenseColumn')}</th>
                </tr>
              </thead>
              <tbody>
                {#each visible as component (`${component.kind}:${component.name}@${component.version}`)}
                  <tr data-component={component.name}>
                    <th scope="row">{component.name}</th>
                    <td>{component.version}</td>
                    <td>{component.license}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <p class="muted" data-slot="no-component">{t('about.components.none')}</p>
        {/if}
      {:else if !error}
        <p class="muted small" role="status">{t('common.loading')}</p>
      {/if}

      <button
        type="button"
        class="button"
        data-action="toggle-notices"
        aria-expanded={shown.notices}
        aria-controls="about-notices-text"
        onclick={() => toggle('notices')}
      >
        {shown.notices ? t('about.notices.hide') : t('about.notices.show')}
      </button>
      {#if shown.notices}
        <div id="about-notices-text">
          {#if loading.notices}
            <p class="muted small" role="status">{t('common.loading')}</p>
          {:else if texts.notices !== undefined}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <pre class="about-text" tabindex="0" aria-label={t('about.notices.textLabel')} data-slot="notices-text">{texts.notices}</pre>
          {/if}
        </div>
      {/if}
    </section>

    {#if error}
      <p class="error" role="alert">{t('errors.generic', { message: error })}</p>
    {/if}

    <div class="actions">
      <button type="button" class="button primary" data-autofocus onclick={onclose}>{t('common.close')}</button>
    </div>
  </div>
</Modal>
