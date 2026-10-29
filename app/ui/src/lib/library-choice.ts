/** Choosing the library folder (first-run wizard, Settings › change library): pure helpers. */

/**
 * Whether two folder paths name the same folder as Windows sees them: separators, a trailing
 * separator and case do not matter.
 */
export function sameFolder(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const clean = (path: string) => path.replace(/[\\/]+/g, '\\').replace(/\\$/, '').toLowerCase();
  return clean(a) === clean(b);
}
