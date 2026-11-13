/**
 * The thumbnails of a PDF's pages for the question « which page goes under the writing? » (`PdfPagePicker.svelte`;
 * Rust side: `pdf_preview.rs`). A page is drawn only when the picker says it is in view (`show`), at most
 * `CONCURRENCY` at a time, the lowest page number first; a page that scrolled away before its turn is not drawn
 * (`hide`). The images stay here (data URLs) while the question is open: `close` forgets them and frees the PDF that
 * Rust holds, whatever is still on its way.
 */
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import { api } from './api';

/** Thumbnails asked of Rust at once (two hide the round trip; Rust draws them one at a time). */
const CONCURRENCY = 2;

/**
 * Pictures kept at once: a PDF of thousands of pages scrolled through would otherwise keep every one. The oldest
 * pictures that are not in view are forgotten first (and drawn again if their page comes back into view).
 */
export const KEPT = 120;

/** CSS width of a thumbnail the picker asks for: about this at the screen's pixel ratio. */
export const THUMBNAIL_CSS_WIDTH = 128;

/** The pixel width to ask for on a screen of `pixelRatio` (a data URL grows with it: not above 2.5). */
export function thumbnailPixelWidth(pixelRatio: number): number {
  const ratio = Number.isFinite(pixelRatio) && pixelRatio > 0 ? Math.min(pixelRatio, 2.5) : 1;
  return Math.round(THUMBNAIL_CSS_WIDTH * ratio);
}

export class PdfThumbnails {
  /** The images by page (from 1) as they arrive. */
  readonly urls = new SvelteMap<number, string>();
  /** The pages Rust could not draw (they stay chooseable, with a neutral placeholder). */
  readonly failed = new SvelteSet<number>();

  private token: Promise<number | null>;
  private closed = false;
  private readonly wanted = new Set<number>();
  private readonly inView = new Set<number>();
  private readonly loading = new Set<number>();
  private running = 0;

  private readonly width: number;

  constructor(path: string, width: number) {
    this.width = width;
    // A PDF that cannot be opened here (it opened a moment ago for its page sizes) draws no thumbnail at all.
    this.token = api.openPdfPreview(path).then(
      (token) => {
        if (this.closed) {
          void api.closePdfPreview(token).catch(() => {});
          return null;
        }
        return token;
      },
      () => null,
    );
    void this.token.then((token) => {
      if (token === null && !this.closed) for (const page of this.wanted) this.failed.add(page);
      this.pump();
    });
  }

  /** The page `page` (from 1) is in view: its thumbnail is wanted. */
  show(page: number): void {
    this.inView.add(page);
    if (this.closed || this.urls.has(page) || this.failed.has(page) || this.loading.has(page)) return;
    this.wanted.add(page);
    this.pump();
  }

  /** The page left the view: if its turn has not come, it is not drawn. */
  hide(page: number): void {
    this.inView.delete(page);
    this.wanted.delete(page);
  }

  /** Forgets every image and frees the PDF; what is still being drawn is dropped when it arrives. */
  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.wanted.clear();
    this.inView.clear();
    this.urls.clear();
    this.failed.clear();
    void this.token.then((token) => {
      if (token !== null) void api.closePdfPreview(token).catch(() => {});
    });
  }

  private pump(): void {
    while (!this.closed && this.running < CONCURRENCY && this.wanted.size > 0) {
      const page = Math.min(...this.wanted);
      this.wanted.delete(page);
      this.running += 1;
      this.loading.add(page);
      void this.draw(page).finally(() => {
        this.running -= 1;
        this.loading.delete(page);
        this.pump();
      });
    }
  }

  /** Keeps at most [`KEPT`] pictures: the oldest ones out of view go. */
  private forgetTheOldest(): void {
    if (this.urls.size <= KEPT) return;
    for (const page of [...this.urls.keys()]) {
      if (this.urls.size <= KEPT) break;
      if (!this.inView.has(page)) this.urls.delete(page);
    }
  }

  private async draw(page: number): Promise<void> {
    const token = await this.token;
    if (this.closed) return;
    if (token === null) {
      this.failed.add(page);
      return;
    }
    try {
      const url = await api.renderPdfThumbnail(token, page, this.width);
      if (this.closed) return;
      this.urls.set(page, url);
      this.forgetTheOldest();
    } catch {
      if (!this.closed) this.failed.add(page);
    }
  }
}
