/** The tag colours of the library, as the screens share them (palette and helpers: `tag-colors.ts`). */
import { api, errorMessage, onLibraryChanged } from './api';
import { colorOf, type TagColor } from './tag-colors';

export class TagColorState {
  colors = $state.raw<Readonly<Record<string, string>>>({});
  #sequence = 0;

  /** `onerror` shows the translated message of a failed change. */
  constructor(private readonly onerror: (message: string) => void) {}

  of(tag: string): TagColor | null {
    return colorOf(this.colors, tag);
  }

  /** Loads the colours and reloads them whenever the library changes. Returns the function that stops it. */
  start(): () => void {
    let stopped = false;
    let unlisten: (() => void) | null = null;
    void this.load();
    void onLibraryChanged(() => void this.load()).then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    });
    return () => {
      stopped = true;
      unlisten?.();
    };
  }

  async load(): Promise<void> {
    const mine = ++this.#sequence;
    try {
      const colors = await api.tagColors();
      if (mine === this.#sequence) this.colors = colors;
    } catch {
      // No colours shown: the notes and their tags are unaffected.
    }
  }

  /** Gives `tag` a colour of the palette, or none. Returns whether it worked. */
  async set(tag: string, color: TagColor | null): Promise<boolean> {
    const mine = ++this.#sequence;
    try {
      const colors = await api.setTagColor(tag, color);
      if (mine === this.#sequence) this.colors = colors;
      return true;
    } catch (error) {
      this.onerror(errorMessage(error));
      return false;
    }
  }
}
