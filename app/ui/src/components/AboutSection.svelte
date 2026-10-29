<script lang="ts">
  /**
   * Settings: the « À propos » entry. The version and the licence in one line, and the button
   * that opens the full screen (`AboutDialog`: licence, credits, Cisco, components).
   */
  import { t } from '../lib/i18n.svelte';
  import AboutDialog from './AboutDialog.svelte';

  let { version }: { version: string } = $props();

  let open = $state(false);
</script>

<section class="settings-section" aria-labelledby="settings-about">
  <h3 id="settings-about">{t('about.section')}</h3>
  <p data-slot="about-version">{t('about.nameVersion', { version })}</p>
  <p class="muted small">{t('about.licenseShort')}</p>
  <div class="actions">
    <button type="button" class="button" data-action="open-about" onclick={() => (open = true)}>
      {t('about.open')}
    </button>
  </div>
</section>

{#if open}
  <AboutDialog {version} onclose={() => (open = false)} />
{/if}
