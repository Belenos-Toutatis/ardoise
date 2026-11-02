/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// `viewer.html` is served with a policy that forbids inline scripts (docs/design/lan-share.md,
// "Pages"), so the message for a browser too old to run the viewer is plain HTML in the page.
const html = readFileSync(resolve(process.cwd(), 'viewer.html'), 'utf8');

describe('viewer.html', () => {
  it('holds a message in French and English for a browser that cannot run the viewer', () => {
    expect(html).toMatch(/Ce navigateur est trop ancien/);
    expect(html).toMatch(/This browser is too old/);
    // Inside the element the viewer mounts into: it replaces the message when it starts.
    expect(html).toMatch(/<div id="viewer">\s*<div class="too-old"/);
    // With scripts disabled the message shows at once.
    expect(html).toMatch(/<noscript>[\s\S]*too-old[\s\S]*<\/noscript>/);
  });

  it('has no inline script, which the policy of the page would refuse', () => {
    const scripts = [...html.matchAll(/<script\b([^>]*)>/gi)];
    expect(scripts.length).toBeGreaterThan(0);
    for (const [, attributes] of scripts) expect(attributes).toMatch(/\bsrc=/);
    expect(html).not.toMatch(/\son[a-z]+\s*=/i);
  });
});
