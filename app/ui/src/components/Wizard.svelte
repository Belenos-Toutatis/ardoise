<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage, type AppState, type LibraryInspection } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { DEFAULT_BINDER } from '../lib/inkspace';
  import { inkspaceOffer } from '../lib/inkspace-offer.svelte';
  import { pickFolder } from '../lib/library';
  import LanguagePicker from './LanguagePicker.svelte';
  import LibraryFolderFacts from './LibraryFolderFacts.svelte';
  import Logo from './Logo.svelte';

  let { app, onchange }: { app: AppState; onchange: (next: AppState) => void } = $props();

  let chosen = $state<string | null>(null);
  let inspection = $state<LibraryInspection | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  /** Wacom Inkspace's notes found on this PC (`notes` null when the folder could not be read). */
  let inkspace = $state<{ notes: number | null } | null>(null);
  let importInkspace = $state(true);

  onMount(() => {
    void (async () => {
      try {
        const path = await api.inkspaceDetect();
        if (!path) return;
        // Offered even when the count fails: the import dialog says what is wrong.
        const notes = await api.inkspaceInspect(path).then(
          (source) => source.notes,
          () => null,
        );
        inkspace = { notes };
      } catch {
        // No detection, no offer.
      }
    })();
  });

  async function choose() {
    error = null;
    try {
      const folder = await pickFolder(t('wizard.pickerTitle'));
      if (!folder) return;
      inspection = await api.inspectLibrary(folder);
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
      const next = await api.setLibrary(chosen);
      // The library screen that follows opens the import dialog (the user confirms it there).
      inkspaceOffer.pending = inkspace !== null && importInkspace;
      onchange(next);
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

    {#if chosen && inspection}
      <LibraryFolderFacts folder={chosen} {inspection} />
    {/if}

    {#if inkspace}
      <div class="chosen" data-slot="wizard-inkspace">
        <p>
          {inkspace.notes === null
            ? t('inkspace.wizard.found')
            : t(inkspace.notes === 1 ? 'inkspace.wizard.foundCountOne' : 'inkspace.wizard.foundCount', {
                count: inkspace.notes,
              })}
        </p>
        <label class="choice">
          <input type="checkbox" bind:checked={importInkspace} />
          {t('inkspace.wizard.check', { binder: DEFAULT_BINDER })}
        </label>
        <p class="muted small">{t('inkspace.wizard.hint')}</p>
      </div>
    {/if}

    {#if chosen && inspection}
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
