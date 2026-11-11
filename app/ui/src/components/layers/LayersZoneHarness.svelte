<script lang="ts">
  /**
   * Test helper: a `LayersZone` whose props can be changed from outside (`set`), as an owner's
   * state changes under it, and which reports every callback to `oncall`.
   */
  import { untrack } from 'svelte';
  import type { Layer } from '../../lib/note';
  import LayersZone from './LayersZone.svelte';

  interface Model {
    where: 'editor' | 'live';
    layers: Layer[];
    chosen: number;
    tablet?: number;
    disabled?: boolean;
  }

  let { initial, oncall }: { initial: Model; oncall: (name: string, ...args: unknown[]) => void } = $props();

  let model = $state.raw<Model>(untrack(() => initial));

  export function set(patch: Partial<Model>) {
    model = { ...model, ...patch };
  }
</script>

<LayersZone
  {...model}
  onchoose={(i) => oncall('choose', i)}
  onvisible={(i, visible) => oncall('visible', i, visible)}
  onrename={(i, name) => oncall('rename', i, name)}
  onmove={(from, to) => oncall('move', from, to)}
  onmerge={(i) => oncall('merge', i)}
  onadd={() => oncall('add')}
  ontablet={(i) => oncall('tablet', i)}
/>
