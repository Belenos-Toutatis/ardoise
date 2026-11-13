/** The print dialog's choices, remembered on this computer between two prints. */
import type { PrintFit, PrintOptions, PrintPerSheet } from './note';

export const PRINT_FITS: readonly PrintFit[] = ['actual', 'fit'];

/** The numbers of notes per sheet the dialog offers. */
export const PRINT_PER_SHEET: readonly PrintPerSheet[] = [1, 2, 4];

export const DEFAULT_PRINT: PrintOptions = { background: true, fit: 'actual', perSheet: 1 };

const KEY = 'ardoise.print';

/** The remembered choice, or the default when nothing valid is stored (or storage is blocked). */
export function loadPrintChoice(): PrintOptions {
  try {
    const stored = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<PrintOptions> | null;
    return {
      background: typeof stored?.background === 'boolean' ? stored.background : DEFAULT_PRINT.background,
      fit: PRINT_FITS.includes(stored?.fit as PrintFit) ? (stored?.fit as PrintFit) : DEFAULT_PRINT.fit,
      perSheet: PRINT_PER_SHEET.includes(stored?.perSheet as PrintPerSheet) ? (stored?.perSheet as PrintPerSheet) : DEFAULT_PRINT.perSheet,
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
