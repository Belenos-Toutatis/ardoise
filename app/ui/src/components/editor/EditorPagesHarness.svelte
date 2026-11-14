<script lang="ts">
  /**
   * Test helper: an `Editor` held like the library screen holds it, for the page buttons. The
   * path follows what the editor reports (`onsaved`, `onpage`), and the list of pages is the one
   * the test sets, which the editor reads at each move.
   */
  import { untrack } from 'svelte';
  import type { AppState } from '../../lib/api';
  import type { WindowClose } from '../../lib/editor/window-close';
  import Editor from './Editor.svelte';

  let {
    initial,
    pages: initialPages,
    app,
    onclose,
    onsaved,
    onpage,
    windowClose,
  }: {
    initial: string;
    pages: string[];
    app: AppState;
    onclose: () => void;
    onsaved: (path: string) => void;
    onpage: (path: string) => void;
    windowClose?: WindowClose;
  } = $props();
  let path = $state(untrack(() => initial));
  let pages = $state.raw(untrack(() => initialPages));

  /** The path the screen holds now (what `Home` calls `openPath`). */
  export function held(): string {
    return path;
  }

  export function setPages(next: string[]) {
    pages = next;
  }

  /** The screen gives the editor another note (what `Home` does when another note is opened). */
  export function setPath(next: string) {
    path = next;
  }
</script>

<Editor
  {path}
  {app}
  {pages}
  {windowClose}
  {onclose}
  onsaved={(saved) => {
    path = saved;
    onsaved(saved);
  }}
  onpage={(next) => {
    path = next;
    onpage(next);
  }}
/>
