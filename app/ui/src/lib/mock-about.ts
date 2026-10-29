/**
 * Browser stand-in for the « À propos » commands (`about_info`, `about_text`): a short list of
 * components and excerpts of the two texts, enough to see the screen in a plain browser.
 */
import type { AboutComponent, AboutInfo } from './about';
import { UNHANDLED } from './mock-data';

export const MOCK_COMPONENTS: AboutComponent[] = [
  { name: 'serde', version: '1.0.229', license: 'MIT OR Apache-2.0', kind: 'rust' },
  { name: 'tauri', version: '2.12.5', license: 'Apache-2.0 OR MIT', kind: 'rust' },
  { name: 'webm-sys', version: '2.2.1', license: 'MPL-2.0', kind: 'rust' },
  { name: 'openh264', version: '0.9.9', license: 'BSD-2-Clause', kind: 'rust' },
  { name: 'imagequant', version: '4.4.1', license: 'GPL-3.0-or-later', kind: 'rust' },
  { name: 'svelte', version: '5.43.0', license: 'MIT', kind: 'npm' },
  { name: 'is-reference', version: '3.0.3', license: 'MIT', kind: 'npm' },
];

const MOCK_LICENSE = 'GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007\n\n(browser stand-in: the real text is the LICENSE file)';
const MOCK_NOTICES = 'THIRD-PARTY NOTICES\n\n(browser stand-in: the real text is the THIRD-PARTY-NOTICES file)';

export function mockAboutInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'about_info':
      return { version: '0.0.0-browser', license: 'GPL-3.0-or-later', components: MOCK_COMPONENTS } satisfies AboutInfo;
    case 'about_text':
      if (args.which === 'license') return MOCK_LICENSE;
      if (args.which === 'notices') return MOCK_NOTICES;
      throw { code: 'unexpected', message: `no such text: ${String(args.which)}` };
    default:
      return UNHANDLED;
  }
}
