/** The print dialog's choices, remembered on this computer between two prints. */
import type { PrintFit, PrintOptions } from './note';

export const PRINT_FITS: readonly PrintFit[] = ['actual', 'fit'];

export const DEFAULT_PRINT: PrintOptions = { background: true, fit: 'actual' };

const KEY = 'ardoise.print';

/** The remembered choice, or the default when nothing valid is stored (or storage is blocked). */
export function loadPrintChoice(): PrintOptions {
  try {
    const stored = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<PrintOptions> | null;
    return {
      background: typeof stored?.background === 'boolean' ? stored.background : DEFAULT_PRINT.background,
      fit: PRINT_FITS.includes(stored?.fit as PrintFit) ? (stored?.fit as PrintFit) : DEFAULT_PRINT.fit,
    };
  } catch {
    return { ...DEFAULT_PRINT };
  }
}

export function savePrintChoice(options: PrintOptions): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(options));
  } catch {
    // Nothing to remember it with: the next print starts from the defaults.
  }
}
