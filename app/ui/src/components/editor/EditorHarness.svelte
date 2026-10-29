<script lang="ts">
  /** Test helper: an `Editor` whose `path` the test can change, as the library screen does. */
  import { untrack } from 'svelte';
  import type { AppState } from '../../lib/api';
  import Editor from './Editor.svelte';

  let { initial, app, onclose, onsaved }: { initial: string; app: AppState; onclose: () => void; onsaved: (path: string) => void } = $props();
  let path = $state(untrack(() => initial));

  export function go(next: string) {
    path = next;
  }
</script>

<!-- Like the library screen: the path follows what the editor reports. -->
<Editor
  {path}
  {app}
  {onclose}
  onsaved={(saved) => {
    path = saved;
    onsaved(saved);
  }}
/>
