// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { pickExportFile } from './dialogs';

afterEach(() => {
  delete (window as { __ardoiseE2eSavePath?: string }).__ardoiseE2eSavePath;
});

describe('save dialog of the exports', () => {
  it('answers with a fake path in the browser stand-in', async () => {
    expect(await pickExportFile('Enregistrer', 'Note.gif', 'gif')).toBe('C:\\Users\\demo\\Exports\\Note.gif');
  });

  it('answers with the path scripted by an end-to-end journey, without any dialog', async () => {
    (window as { __ardoiseE2eSavePath?: string }).__ardoiseE2eSavePath = 'C:\\Temp\\a.gif';
    expect(await pickExportFile('Enregistrer', 'Note.gif', 'gif')).toBe('C:\\Temp\\a.gif');
  });
});
