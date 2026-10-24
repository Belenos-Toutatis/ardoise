<script lang="ts">
  /**
   * Side panel of the tablet (SPEC §4.1): information, sync, live mode, options, registration.
   * Placeholder of the contracts commit; work package C (docs/design/tablet-live-classroom.md).
   */
  import { t } from '../../lib/i18n.svelte';

  let { onclose, onlive, ontest }: { onclose: () => void; onlive: () => void; ontest: () => void } = $props();
</script>

<aside class="side-panel tablet-panel" aria-label={t('tablet.title')}>
  <h2>{t('tablet.title')}</h2>
  <button type="button" class="button" data-action="live" onclick={onlive}>{t('live.title')}</button>
  <button type="button" class="button" onclick={ontest}>{t('diagnostic.open')}</button>
  <button type="button" class="button ghost" onclick={onclose}>{t('common.close')}</button>
</aside>
