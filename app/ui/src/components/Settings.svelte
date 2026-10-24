<script lang="ts">
  import { api, errorMessage, type AppState, type ThemePreference } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { pickFolder } from '../lib/library';
  import LanguagePicker from './LanguagePicker.svelte';

  let { app, onchange, onclose }: {
    app: AppState;
    onchange: (next: AppState) => void;
    onclose: () => void;
  } = $props();

  let error = $state<string | null>(null);

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
