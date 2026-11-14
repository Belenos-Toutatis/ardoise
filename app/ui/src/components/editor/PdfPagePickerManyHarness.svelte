<script lang="ts">
  /** Test harness of `PdfPagePicker` with `multiple`: holds the bound pages, and records what the user chose. */
  import PdfPagePicker from './PdfPagePicker.svelte';

  let {
    path,
    pages,
    start = [],
    disabled = false,
  }: { path: string; pages: { width: number; height: number }[]; start?: number[]; disabled?: boolean } = $props();

  // svelte-ignore state_referenced_locally
  let picked = $state<number[]>(start);
  let told = $state<number[][]>([]);
</script>

<PdfPagePicker {path} name="fiche.pdf" {pages} multiple bind:picked onpick={(pages) => (told = [...told, pages])} {disabled} />
<output data-slot="picked">{picked.join(',')}</output>
<output data-slot="told">{told.map((pages) => pages.join(',')).join(' | ')}</output>
