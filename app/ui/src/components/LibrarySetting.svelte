<script lang="ts">
  /**
   * Settings › library folder (milestone M10). Picking another folder no longer switches at
   * once: like the first-run wizard, the settings first show what the folder holds (an existing
   * library or a new one, how many notes), and switch only once the user confirms.
   */
  import { api, errorMessage, type AppState, type LibraryInspection } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { pickFolder } from '../lib/library';
  import { sameFolder } from '../lib/library-choice';
  import LibraryFolderFacts from './LibraryFolderFacts.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  let chosen = $state<{ folder: string; inspection: LibraryInspection } | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const current = $derived(chosen !== null && sameFolder(chosen.folder, app.library));

  async function choose() {
    error = null;
    try {
      const folder = await pickFolder(t('wizard.pickerTitle'));
      if (!folder) return;
      chosen = { folder, inspection: await api.inspectLibrary(folder) };
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function confirm() {
    if (!chosen || busy) return;
    busy = true;
    error = null;
    try {
      onchange(await api.setLibrary(chosen.folder));
      chosen = null;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="field">
  <span>{t('settings.library')}</span>
  <code>{app.library}</code>
  <button class="button" data-action="change-library" onclick={choose}>{t('common.change')}</button>
</div>

{#if chosen}
  <div class="confirm library-switch" role="alertdialog" aria-label={t('settings.librarySwitch.title')}>
    <LibraryFolderFacts folder={chosen.folder} inspection={chosen.inspection} />
    {#if current}
      <p class="small">{t('settings.librarySwitch.current')}</p>
      <div class="actions">
        <button class="button" onclick={() => (chosen = null)}>{t('common.close')}</button>
      </div>
    {:else}
      <p class="muted small">{t('settings.librarySwitch.hint', { machine: app.machine })}</p>
      <div class="actions">
        <button class="button primary" disabled={busy} onclick={confirm}>{t('settings.librarySwitch.confirm')}</button>
        <button class="button" onclick={() => (chosen = null)}>{t('settings.cancel')}</button>
      </div>
    {/if}
  </div>
{/if}

{#if error}
  <p class="error" role="alert">{t('errors.library', { message: error })}</p>
{/if}

<style>
  .library-switch {
    margin-bottom: 1rem;
  }
</style>
