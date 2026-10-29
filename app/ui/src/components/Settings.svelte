<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { api, errorMessage, type AppState, type ThemePreference } from '../lib/api';
  import { currentLocale, t } from '../lib/i18n.svelte';
  import { pickFolder } from '../lib/library';
  import { formatDate } from '../lib/library-logic';
  import { sizeInMillimetres, tablet } from '../lib/tablet-state.svelte';
  import LanguagePicker from './LanguagePicker.svelte';
  import MicrophonePicker from './narration/MicrophonePicker.svelte';
  import AssistantSection from './AssistantSection.svelte';
  import HandwritingSection from './HandwritingSection.svelte';
  import OpenH264Section from './OpenH264Section.svelte';
  import PenTouchSection from './PenTouchSection.svelte';
  import TabletOptions from './tablet/TabletOptions.svelte';

  let { app, onchange, onclose, ontest }: {
    app: AppState;
    onchange: (next: AppState) => void;
    onclose: () => void;
    /** Opens the tablet test. */
    ontest?: () => void;
  } = $props();

  let error = $state<string | null>(null);
  /** The registered tablet (from the window's shared tablet status); undefined while loading. */
  const registered = $derived(tablet.status ? tablet.status.registered : undefined);
  let confirmingForget = $state(false);

  /** The automatic export as being edited; saved with its button. */
  let autoDraft = $state(untrack(() => ({ ...app.autoExport })));
  let autoError = $state<string | null>(null);
  let autoSaved = $state(false);
  let autoSaving = $state(false);
  const autoChanged = $derived(autoDraft.format !== app.autoExport.format || autoDraft.folder !== app.autoExport.folder);

  onMount(() => tablet.start());

  async function forgetTablet() {
    error = null;
    confirmingForget = false;
    if (!(await tablet.forget())) error = tablet.actionError;
  }

  async function run(action: () => Promise<AppState>) {
    error = null;
    try {
      onchange(await action());
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function saveAutoExport() {
    autoError = null;
    autoSaved = false;
    autoSaving = true;
    try {
      const next = await api.setAutoExport({ format: autoDraft.format, folder: autoDraft.folder });
      autoDraft = { ...next.autoExport };
      onchange(next);
      autoSaved = true;
    } catch (e) {
      autoError = errorMessage(e);
    } finally {
      autoSaving = false;
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
    {#if registered}
      <dl class="facts">
        <dt>{t('settings.tabletName')}</dt>
        <dd>{registered.name}</dd>
        <dt>{t('settings.tabletAddress')}</dt>
        <dd><code>{registered.address}</code></dd>
        <dt>{t('settings.tabletRegistered')}</dt>
        <dd>{formatDate(Date.parse(registered.registered), currentLocale())}</dd>
        <dt>{t('settings.tabletSize')}</dt>
        <dd>{sizeInMillimetres(registered.width, registered.height)}</dd>
      </dl>
      <h4 class="options-title">{t('tablet.options.title')}</h4>
      <TabletOptions />
      {#if confirmingForget}
        <div class="confirm" role="alertdialog" aria-label={t('settings.tabletForget')}>
          <p>{t('settings.tabletForgetConfirm', { name: registered.name })}</p>
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
    {:else if registered === null}
      <p class="muted">{t('settings.tabletNone')}</p>
    {/if}
    {#if ontest}
      <div class="actions">
        <button class="button" onclick={ontest}>{t('settings.tabletTest')}</button>
      </div>
    {/if}
  </section>

  <section class="settings-section" aria-labelledby="settings-auto-export">
    <h3 id="settings-auto-export">{t('settings.autoExport.title')}</h3>
    <p class="muted small">{t('settings.autoExport.hint')}</p>
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void saveAutoExport();
      }}
    >
      <div class="field" role="radiogroup" aria-label={t('settings.autoExport.format')}>
        <span>{t('settings.autoExport.format')}</span>
        {#each ['off', 'pdf', 'png'] as const as format (format)}
          <label class="choice">
            <input
              type="radio"
              name="auto-export-format"
              value={format}
              bind:group={autoDraft.format}
              oninput={() => (autoSaved = false)}
            />
            {t(`settings.autoExport.${format}`)}
          </label>
        {/each}
      </div>
      <label class="field">
        <span>{t('settings.autoExport.folder')}</span>
        <input
          class="text-input"
          type="text"
          bind:value={autoDraft.folder}
          aria-describedby="settings-auto-export-folder-hint"
          oninput={() => (autoSaved = false)}
        />
        <span id="settings-auto-export-folder-hint" class="muted small field-hint">{t('settings.autoExport.folderHint')}</span>
      </label>
      <div class="actions">
        <button type="submit" class="button primary" disabled={!autoChanged || autoSaving}>{t('settings.autoExport.save')}</button>
      </div>
    </form>
    {#if autoSaved}
      <p class="muted small" role="status">{t('settings.autoExport.saved')}</p>
    {/if}
    {#if autoError}
      <p class="error" role="alert">{t('errors.generic', { message: autoError })}</p>
    {/if}
  </section>

  <section class="settings-section" aria-labelledby="settings-microphone">
    <h3 id="settings-microphone">{t('settings.microphone.title')}</h3>
    <p class="muted small">{t('settings.microphone.hint')}</p>
    <MicrophonePicker device={app.microphone} monitor onchange={(device) => run(() => api.setMicrophone(device))} />
  </section>

  <OpenH264Section />

  <PenTouchSection />

  <HandwritingSection />

  <AssistantSection />

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

<style>
  .options-title {
    margin: 1rem 0 0.4rem;
    font-size: 0.9rem;
  }
</style>
