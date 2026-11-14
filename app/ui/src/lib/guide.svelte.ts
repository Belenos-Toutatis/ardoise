/**
 * Opening the user guide (`open_guide`, `guide.rs`): the button of the library, the entry of the Aide tab of the editor
 * and of live mode, the link of the settings' Assistant section and the F1 key all come here. The page opens in the
 * default browser, from the `guide` folder next to the executable (offline) or, when it is missing, from GitHub: the
 * only time Ardoise goes online, at the user's request, so the notice says it (`GuideNotice.svelte` shows it, whatever
 * screen is on display).
 */
import { api, errorMessage, genericError } from './api';
import type { GuideOpened, GuidePage } from './guide';
import { currentLocale, t } from './i18n.svelte';

/** What the screen says about the last opening; `text` null when there is nothing to say. */
export const guideNotice = $state<{ text: string | null; failed: boolean }>({ text: null, failed: false });

/** Clears the notice (its close button, its timer). */
export function dismissGuideNotice(): void {
  guideNotice.text = null;
  guideNotice.failed = false;
}

/** Opens `page` in the language of the interface; says so when it was the web's, or when it could not be opened. */
export async function openGuide(page: GuidePage = 'guide'): Promise<GuideOpened | null> {
  try {
    const opened = await api.openGuide(page, currentLocale());
    guideNotice.failed = false;
    guideNotice.text = opened === 'web' ? t('guide.web') : null;
    return opened;
  } catch (error) {
    guideNotice.failed = true;
    guideNotice.text = genericError(errorMessage(error));
    return null;
  }
}
