/**
 * The « À propos » screen (M10): the shapes the Rust commands of `app/src-tauri/src/about.rs`
 * return, and the component filter (docs/design/interface.md, "M10: About and distribution").
 */
import { queryWords } from './library-logic';

/** `AboutComponent` in `about.rs`: one third-party package built into the app. */
export interface AboutComponent {
  name: string;
  version: string;
  /** SPDX expression, as the package declares it (for a font, its licence as the notices name it). */
  license: string;
  /** `rust`: compiled into the executable; `npm`: bundled into the interface; `font`: a font file of the text tool. */
  kind: 'rust' | 'npm' | 'font';
}

/** `AboutInfo` in `about.rs`. */
export interface AboutInfo {
  version: string;
  /** SPDX identifier of Ardoise's own licence. */
  license: string;
  components: AboutComponent[];
}

/** The two long texts: Ardoise's licence, and the licences of the components. */
export type AboutText = 'license' | 'notices';

/**
 * The components whose name, version or licence contain every word of `query` (case and accents
 * ignored); all of them for an empty query. The order is kept.
 */
export function filterComponents(components: readonly AboutComponent[], query: string): AboutComponent[] {
  const words = queryWords(query);
  if (words.length === 0) return [...components];
  return components.filter((component) => {
    const haystack = queryWords(`${component.name} ${component.version} ${component.license}`).join(' ');
    return words.every((word) => haystack.includes(word));
  });
}
