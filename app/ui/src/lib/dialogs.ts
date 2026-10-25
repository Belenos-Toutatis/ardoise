import { open, save } from '@tauri-apps/plugin-dialog';
import { inDesktopApp } from './api';

/** Where the browser stand-in "saves": no native dialog there. */
const BROWSER_FOLDER = 'C:\\Users\\demo\\Exports';

/**
 * Native "save as" dialog for one export file (an image, a PDF or a video); `null` when the user
 * cancels. The browser stand-in skips the dialog and answers with a fake path. The end-to-end
 * journeys (`app/e2e`) cannot drive a native dialog: they set `window.__ardoiseE2eSavePath` to
 * the file the dialog would have returned.
 */
export async function pickExportFile(title: string, defaultName: string, extension: string): Promise<string | null> {
  const scripted = (window as { __ardoiseE2eSavePath?: unknown }).__ardoiseE2eSavePath;
  if (typeof scripted === 'string') return scripted;
  if (!inDesktopApp()) return `${BROWSER_FOLDER}\\${defaultName}`;
  return save({ title, defaultPath: defaultName, filters: [{ name: extension.toUpperCase(), extensions: [extension] }] });
}

/** Native folder picker for an export of several files; `null` when cancelled. */
export async function pickExportFolder(title: string): Promise<string | null> {
  if (!inDesktopApp()) return BROWSER_FOLDER;
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === 'string' ? picked : null;
}

/** Extensions of the files a background image or a new note can come from. */
export const BACKGROUND_EXTENSIONS = ['pdf', 'png', 'jpg', 'jpeg'];

/**
 * Native "open" dialog for a PDF or an image (a background, or new notes); `null` when the user
 * cancels. The browser stand-in asks for a path (`.pdf` gives a PDF of three pages). The
 * end-to-end journeys set `window.__ardoiseE2eOpenPath` to the file the dialog would return.
 */
export async function pickBackgroundFile(title: string, filterName: string): Promise<string | null> {
  const scripted = (window as { __ardoiseE2eOpenPath?: unknown }).__ardoiseE2eOpenPath;
  if (typeof scripted === 'string') return scripted;
  if (!inDesktopApp()) return window.prompt(title, 'C:\Users\demo\Documents\fiche.pdf');
  const picked = await open({
    title,
    multiple: false,
    directory: false,
    filters: [{ name: filterName, extensions: BACKGROUND_EXTENSIONS }],
  });
  return typeof picked === 'string' ? picked : null;
}
