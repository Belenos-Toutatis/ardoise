<script lang="ts">
  /**
   * Test helper: a `Canvas` whose document follows what it reports (selection and changes), as in
   * the editor, so that a gesture can build on the result of the previous one.
   */
  import { untrack } from 'svelte';
  import type { Ink } from '../../lib/ink';
  import type { Doc, ToolSettings } from '../../lib/editor/state';
  import Canvas from './Canvas.svelte';

  let {
    initial,
    tools,
    ink,
    onchange,
  }: { initial: Doc; tools: ToolSettings; ink: Ink; onchange: (doc: Doc) => void } = $props();

  let doc = $state.raw(untrack(() => initial));
</script>

<Canvas
  {doc}
  {tools}
  {ink}
  onchange={(next) => {
    doc = next;
    onchange(next);
  }}
  onselect={(next) => (doc = next)}
/>
