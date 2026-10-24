<script lang="ts">
  import { api, errorMessage, type AppState } from '../lib/api';
  import type { LanguagePreference } from '../lib/i18n-core';
  import { t } from '../lib/i18n.svelte';

  let { app, onchange, onerror }: {
    app: AppState;
    onchange: (next: AppState) => void;
    onerror?: (message: string) => void;
  } = $props();

  async function select(event: Event) {
    const language = (event.currentTarget as HTMLSelectElement).value as LanguagePreference;
    try {
      onchange(await api.setLanguage(language));
    } catch (error) {
      onerror?.(errorMessage(error));
    }
  }
</script>

<label class="field">
  <span>{t('settings.language')}</span>
  <select value={app.language} onchange={select}>
    <option value="auto">{t('settings.languageAuto')}</option>
    <option value="fr">{t('language.fr')}</option>
    <option value="en">{t('language.en')}</option>
  </select>
</label>
