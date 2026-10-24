import type { ThemePreference } from './api';

/** `system` lets the stylesheet follow Windows through `prefers-color-scheme`. */
export function applyTheme(theme: ThemePreference): void {
  if (theme === 'system') {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = theme;
  }
}
