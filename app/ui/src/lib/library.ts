import { open } from '@tauri-apps/plugin-dialog';
import { inDesktopApp } from './api';

/** Native folder picker; `null` when the user cancels. A text prompt stands in for it in a browser. */
export async function pickFolder(title: string): Promise<string | null> {
  if (!inDesktopApp()) return window.prompt(title, 'C:\\Users\\demo\\Nextcloud\\Ardoise');
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === 'string' ? picked : null;
}
