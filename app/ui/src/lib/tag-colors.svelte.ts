/** The tag colours of the library, as the screens share them (palette and helpers: `tag-colors.ts`). */
import { api, errorMessage, onLibraryChanged } from './api';
import { colorOf, type TagColor } from './tag-colors';

/** How often the colours are read again while the window stays in front (another PC may change them). */
export const tagColorsPolling = { intervalMs: 60_000 };

export class TagColorState {
  colors = $state.raw<Readonly<Record<string, string>>>({});
  #sequence = 0;

  /** `onerror` shows the translated message of a failed change. */
  constructor(private readonly onerror: (message: string) => void) {}

  of(tag: string): TagColor | null {
    return colorOf(this.colors, tag);
  }

  /**
   * Loads the colours and reloads them whenever the library changes, when the window comes back
   * to the front, and every minute: the library's watcher only reports notes, and a colour chosen
   * on another PC arrives through Nextcloud in `_ardoise/tags.json` (a few bytes to read). Returns
   * the function that stops it.
   */
  start(): () => void {
    let stopped = false;
    let unlisten: (() => void) | null = null;
    const reload = () => void this.load();
    void this.load();
    void onLibraryChanged(reload).then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    });
    window.addEventListener('focus', reload);
    const timer = setInterval(reload, tagColorsPolling.intervalMs);
    return () => {
      stopped = true;
      unlisten?.();
      window.removeEventListener('focus', reload);
      clearInterval(timer);
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
