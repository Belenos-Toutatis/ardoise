/**
 * Pure helpers of the layers zone (`components/layers/LayersZone.svelte`, used by the editor and
 * by the live control window): the rows in display order, the warnings, and where the chosen layer
 * and the tablet's layer go when the list is reordered (docs/design/tablet-live-classroom.md,
 * "The layers zone").
 */
import type { Layer } from './note';

/** A layer with its index in the note's list (bottom first). */
export interface LayerRow {
  index: number;
  layer: Layer;
}

/** The layers as the zone lists them: the top layer first, each with its index in `layers`. */
export function rowsTopFirst(layers: readonly Layer[]): LayerRow[] {
  return layers.map((layer, index) => ({ index, layer })).reverse();
}

export type LayerWarningKind = 'tabletHidden' | 'chosenHidden';

export interface LayerWarning {
  kind: LayerWarningKind;
  /** The layer the warning is about. */
  index: number;
  name: string;
}

/**
 * The warnings to show: `tabletHidden` first (only when `tablet` is given, in live), then
 * `chosenHidden`. A layer index without an entry gives no warning.
 */
export function layerWarnings(layers: readonly Layer[], chosen: number, tablet?: number): LayerWarning[] {
  const warnings: LayerWarning[] = [];
  const tabletLayer = tablet === undefined ? undefined : layers[tablet];
  if (tablet !== undefined && tabletLayer && !tabletLayer.visible) {
    warnings.push({ kind: 'tabletHidden', index: tablet, name: tabletLayer.name });
  }
  const chosenLayer = layers[chosen];
  if (chosenLayer && !chosenLayer.visible) {
    warnings.push({ kind: 'chosenHidden', index: chosen, name: chosenLayer.name });
  }
  return warnings;
}

/**
 * Where `index` goes once the layer at `from` has been moved to `to` (the same rule as
 * `ops.moveIndex`): the chosen layer and the tablet's follow their layer.
 */
export function followMove(index: number, from: number, to: number): number {
  if (index === from) return to;
  if (from < to && index > from && index <= to) return index - 1;
  if (from > to && index < from && index >= to) return index + 1;
  return index;
}

/** Where `index` goes once layer `merged` has been merged into the one below it: `merged` itself goes down with the rest. */
export function followMerge(index: number, merged: number): number {
  return index >= merged ? Math.max(0, index - 1) : index;
}
