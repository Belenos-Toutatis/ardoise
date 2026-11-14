// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { isGuideKey } from './guide';
import { dismissGuideNotice, guideNotice, openGuide } from './guide.svelte';
import { setLocale, t } from './i18n.svelte';
import { mockGuide } from './mock-about';

beforeEach(() => {
  mockGuide.asked = [];
  mockGuide.source = 'local';
  mockGuide.fail = false;
});
afterEach(() => {
  dismissGuideNotice();
  setLocale('fr');
});

describe('F1', () => {
  it('is the guide key alone, and only once while it is held', () => {
    expect(isGuideKey({ key: 'F1' })).toBe(true);
    expect(isGuideKey({ key: 'F1', repeat: true })).toBe(false);
    for (const mods of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }, { shiftKey: true }]) {
      expect(isGuideKey({ key: 'F1', ...mods })).toBe(false);
    }
    expect(isGuideKey({ key: 'F2' })).toBe(false);
    expect(isGuideKey({ key: 'f' })).toBe(false);
  });
});

describe('openGuide', () => {
  it('asks for the page in the language of the interface, and says nothing when the local page opened', async () => {
    setLocale('en');
    expect(await openGuide('assistant')).toBe('local');
    expect(mockGuide.asked).toEqual([{ page: 'assistant', language: 'en' }]);
    expect(guideNotice.text).toBeNull();
    setLocale('fr');
    expect(await openGuide()).toBe('local');
    expect(mockGuide.asked.at(-1)).toEqual({ page: 'guide', language: 'fr' });
  });

  it('says that GitHub was opened, in both languages, and that the folder is missing', async () => {
    mockGuide.source = 'web';
    for (const locale of ['fr', 'en'] as const) {
      setLocale(locale);
      expect(await openGuide('guide')).toBe('web');
      expect(guideNotice.text).toBe(t('guide.web'));
      expect(guideNotice.failed).toBe(false);
      expect(guideNotice.text).toMatch(/GitHub/);
      expect(guideNotice.text).toContain(locale === 'fr' ? 'dossier « guide » est absent' : '"guide" folder is missing');
    }
  });

  it('clears an old notice when the next opening goes well', async () => {
    mockGuide.source = 'web';
    await openGuide('guide');
    expect(guideNotice.text).not.toBeNull();
    mockGuide.source = 'local';
    await openGuide('guide');
    expect(guideNotice.text).toBeNull();
  });

  it('says it when the browser could not be started', async () => {
    mockGuide.fail = true;
    expect(await openGuide('guide')).toBeNull();
    expect(guideNotice.failed).toBe(true);
    expect(guideNotice.text).toBe("Une erreur est survenue. Le navigateur n'a pas pu ouvrir le guide (no handler).");
  });
});
