<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage, type AppState, type ThemePreference } from '../lib/api';
  import { currentLocale, t } from '../lib/i18n.svelte';
  import { pickFolder } from '../lib/library';
  import { formatDate } from '../lib/library-logic';
  import type { TabletInfo } from '../lib/note';
  import LanguagePicker from './LanguagePicker.svelte';

  let { app, onchange, onclose, ontest }: {
    app: AppState;
    onchange: (next: AppState) => void;
    onclose: () => void;
    /** Opens the tablet test. */
    ontest?: () => void;
  } = $props();

  let error = $state<string | null>(null);
  /** The registered tablet; undefined while loading. */
  let tablet = $state<TabletInfo | null | undefined>(undefined);
  let confirmingForget = $state(false);

  onMount(() => {
    api.tabletInfo().then(
      (info) => (tablet = info),
      (e) => {
        tablet = null;
        error = errorMessage(e);
      },
    );
  });

  async function forgetTablet() {
    error = null;
    try {
      await api.forgetTablet();
      tablet = null;
    } catch (e) {
      error = errorMessage(e);
    }
    confirmingForget = false;
  }

  async function run(action: () => Promise<AppState>) {
    error = null;
    try {
      onchange(await action());
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function selectTheme(event: Event) {
    const theme = (event.currentTarget as HTMLSelectElement).value as ThemePreference;
    void run(() => api.setTheme(theme));
  }

  async function changeLibrary() {
    const folder = await pickFolder(t('wizard.pickerTitle'));
    if (folder) await run(() => api.setLibrary(folder));
  }
</script>

<aside class="panel" aria-label={t('settings.title')}>
  <header class="panel-header">
    <h2>{t('settings.title')}</h2>
    <button class="button ghost" onclick={onclose}>{t('common.close')}</button>
  </header>

  <LanguagePicker {app} {onchange} onerror={(message) => (error = message)} />

  <label class="field">
    <span>{t('settings.theme')}</span>
    <select value={app.theme} onchange={selectTheme}>
      <option value="system">{t('settings.themeSystem')}</option>
      <option value="light">{t('settings.themeLight')}</option>
      <option value="dark">{t('settings.themeDark')}</option>
    </select>
  </label>

  <div class="field">
    <span>{t('settings.library')}</span>
    <code>{app.library}</code>
    <button class="button" onclick={changeLibrary}>{t('common.change')}</button>
  </div>

  <section class="settings-section" aria-labelledby="settings-tablet">
    <h3 id="settings-tablet">{t('settings.tablet')}</h3>
    {#if tablet}
      <dl class="facts">
        <dt>{t('settings.tabletName')}</dt>
        <dd>{tablet.name}</dd>
        <dt>{t('settings.tabletAddress')}</dt>
        <dd><code>{tablet.address}</code></dd>
        <dt>{t('settings.tabletRegistered')}</dt>
        <dd>{formatDate(Date.parse(tablet.registered), currentLocale())}</dd>
        <dt>{t('settings.tabletSize')}</dt>
        <dd>{Math.round(tablet.width / 100)} × {Math.round(tablet.height / 100)} mm</dd>
      </dl>
      {#if confirmingForget}
        <div class="confirm" role="alertdialog" aria-label={t('settings.tabletForget')}>
          <p>{t('settings.tabletForgetConfirm', { name: tablet.name })}</p>
          <div class="actions">
            <button class="button danger" onclick={forgetTablet}>{t('settings.tabletForgetYes')}</button>
            <button class="button" onclick={() => (confirmingForget = false)}>{t('settings.cancel')}</button>
          </div>
        </div>
      {:else}
        <div class="actions">
          <button class="button danger" onclick={() => (confirmingForget = true)}>{t('settings.tabletForget')}</button>
        </div>
      {/if}
    {:else if tablet === null}
      <p class="muted">{t('settings.tabletNone')}</p>
    {/if}
    {#if ontest}
      <div class="actions">
        <button class="button" onclick={ontest}>{t('settings.tabletTest')}</button>
      </div>
    {/if}
  </section>

  <dl class="facts">
    <dt>{t('settings.machine')}</dt>
    <dd>{app.machine}</dd>
    <dt>{t('settings.settingsFile')}</dt>
    <dd><code>{app.settingsPath}</code></dd>
  </dl>
  <p class="muted small">{t('settings.version', { version: app.version })}</p>

  {#if error}
    <p class="error" role="alert">{t('errors.generic', { message: error })}</p>
  {/if}
</aside>
