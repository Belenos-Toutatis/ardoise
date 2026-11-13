<script lang="ts">
  /** Test harness of `PdfPagePicker`: holds the bound page, and lets a test change the file. */
  import PdfPagePicker from './PdfPagePicker.svelte';

  let {
    path: first,
    name = 'fiche.pdf',
    pages,
    disabled = false,
  }: { path: string; name?: string; pages: { width: number; height: number }[]; disabled?: boolean } = $props();

  // The first file and its pages: only the start (`setFile` changes them).
  // svelte-ignore state_referenced_locally
  let path = $state(first);
  // svelte-ignore state_referenced_locally
  let sizes = $state.raw(pages);
  let selected = $state(1);

  /** Another file, of other pages: the choice starts again at the first page, as the section does. */
  export function setFile(next: string, nextPages: { width: number; height: number }[]) {
    path = next;
    sizes = nextPages;
    selected = 1;
  }
</script>

<PdfPagePicker {path} {name} pages={sizes} bind:selected {disabled} />
<output data-slot="selected">{selected}</output>
