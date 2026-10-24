import { open, save } from '@tauri-apps/plugin-dialog';
import { inDesktopApp } from './api';

/** Where the browser stand-in "saves": no native dialog there. */
const BROWSER_FOLDER = 'C:\\Users\\demo\\Exports';

/**
 * Native "save as" dialog for one export file; `null` when the user cancels. The browser
 * stand-in skips the dialog and answers with a fake path.
 */
export async function pickExportFile(title: string, defaultName: string, extension: string): Promise<string | null> {
  if (!inDesktopApp()) return `${BROWSER_FOLDER}\\${defaultName}`;
  return save({ title, defaultPath: defaultName, filters: [{ name: extension.toUpperCase(), extensions: [extension] }] });
}

/** Native folder picker for an export of several files; `null` when cancelled. */
export async function pickExportFolder(title: string): Promise<string | null> {
  if (!inDesktopApp()) return BROWSER_FOLDER;
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === 'string' ? picked : null;
}
