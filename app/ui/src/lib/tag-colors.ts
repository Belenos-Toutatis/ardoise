/**
 * Tag colours (SPEC §4.3, milestone M10): the palette and how a tag finds its colour. They
 * belong to the library, not to the notes: `_ardoise/tags.json`, shared by every PC and merged
 * when Nextcloud makes conflict copies (`ardoise_core::library::tag_colors`). A colour is a
 * palette id; `tag-colors.css` gives each one a dot, a background and a text colour readable in
 * the light and the dark theme. The state the screens share is in `tag-colors.svelte.ts`.
 */

/** The palette, in the order the picker shows it. */
export const TAG_COLORS = ['red', 'orange', 'yellow', 'green', 'teal', 'blue', 'purple', 'pink'] as const;
export type TagColor = (typeof TAG_COLORS)[number];

export function isTagColor(value: unknown): value is TagColor {
  return TAG_COLORS.includes(value as TagColor);
}

/** How `tags.json` names a tag (`tag_key` in Rust): spaces collapsed, lower case. */
export function tagKey(tag: string): string {
  return tag.split(/\s+/).filter(Boolean).join(' ').toLowerCase();
}

/** The colour of `tag` in `colors` (as `tag_colors` returns them); null for none or an id this version does not know. */
export function colorOf(colors: Readonly<Record<string, string>>, tag: string): TagColor | null {
  const color = colors[tagKey(tag)];
  return isTagColor(color) ? color : null;
}
