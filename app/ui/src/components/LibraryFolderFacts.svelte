<script lang="ts">
  /**
   * What a folder chosen as the library holds (first-run wizard and Settings › change library):
   * whether it already is an Ardoise library (`_ardoise/`), and how many notes it has.
   */
  import type { LibraryInspection } from '../lib/api';
  import { t, tn } from '../lib/i18n.svelte';

  let { folder, inspection }: { folder: string; inspection: LibraryInspection } = $props();

  const notes = $derived(
    inspection.notes === 0
      ? t('libraryChoice.noNotes')
      : inspection.complete
        ? tn('libraryChoice.notes', inspection.notes)
        : tn('libraryChoice.notesAtLeast', inspection.notes),
  );
</script>

<div class="chosen">
  <span class="label">{t('wizard.chosen')}</span>
  <code>{folder}</code>
  <p class="small">{inspection.existing ? t('wizard.existing') : t('wizard.new')}</p>
  <p class="small" data-notes={inspection.notes}>{notes}</p>
</div>
