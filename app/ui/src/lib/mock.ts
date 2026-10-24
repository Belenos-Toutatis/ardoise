/**
 * In-memory stand-in for the Rust commands, used when the interface runs in a plain browser
 * (`npm run dev`) so screens can be developed and tested without the desktop app.
 */
import type { AppState } from './api';

const state: AppState = {
  version: '0.0.0-browser',
  machine: 'BROWSER',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: new URLSearchParams(location.search).get('library'),
  systemLocale: navigator.language,
  selfTest: false,
};

const libraries = new Set<string>();

export async function mockInvoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const result = (() => {
    switch (command) {
      case 'app_state':
        return { ...state };
      case 'set_language':
        state.language = args.language as AppState['language'];
        return { ...state };
      case 'set_theme':
        state.theme = args.theme as AppState['theme'];
        return { ...state };
      case 'inspect_library':
        return { existing: libraries.has(args.path as string) };
      case 'set_library':
        libraries.add(args.path as string);
        state.library = args.path as string;
        return { ...state };
      case 'self_test_report':
        return undefined;
      default:
        throw { code: 'unknownCommand', message: command };
    }
  })();
  return result as T;
}
