<script lang="ts">
  import { api, errorMessage, type AppState } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { pickFolder } from '../lib/library';
  import LanguagePicker from './LanguagePicker.svelte';
  import Logo from './Logo.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  let chosen = $state<string | null>(null);
  let existing = $state(false);
  let error = $state<string | null>(null);
  let busy = $state(false);

  async function choose() {
    error = null;
    try {
      const folder = await pickFolder(t('wizard.pickerTitle'));
      if (!folder) return;
      existing = (await api.inspectLibrary(folder)).existing;
      chosen = folder;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function confirm() {
    if (!chosen) return;
    busy = true;
    error = null;
    try {
      onchange(await api.setLibrary(chosen));
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<main class="wizard">
  <section class="card">
    <header class="wizard-header">
      <Logo size={56} />
      <div>
        <h1>{t('wizard.title')}</h1>
        <p class="muted">{t('app.tagline')}</p>
      </div>
    </header>

    <p class="lead">{t('wizard.intro')}</p>
    <p>{t('wizard.syncHint')}</p>
    <p class="muted small">{t('wizard.machineHint', { machine: app.machine })}</p>

    <button class="button" onclick={choose}>{t('wizard.choose')}</button>

    {#if chosen}
      <div class="chosen">
        <span class="label">{t('wizard.chosen')}</span>
        <code>{chosen}</code>
        <p class="small">{existing ? t('wizard.existing') : t('wizard.new')}</p>
      </div>
      <button class="button primary" disabled={busy} onclick={confirm}>{t('common.continue')}</button>
    {/if}

    {#if error}
      <p class="error" role="alert">{t('errors.library', { message: error })}</p>
    {/if}

    <footer class="wizard-footer">
      <LanguagePicker {app} {onchange} onerror={(message) => (error = message)} />
    </footer>
  </section>
</main>
