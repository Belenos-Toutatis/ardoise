import { mount } from 'svelte';
import { resolveLocale } from '../lib/i18n-core';
import { setLocale } from '../lib/i18n.svelte';
import Viewer from './Viewer.svelte';

// Entry of `viewer.html`: the page the Ardoise PC serves to the browsers of the local network
// (docs/design/lan-share.md). It imports nothing of the desktop app (no Tauri, no `lib/api.ts`).
const target = document.getElementById('viewer');
if (!target) throw new Error('#viewer element missing from viewer.html');

/** What the viewer needs besides ES modules and ES2022 (a browser without them never gets here). */
function supported(): boolean {
  return (
    typeof WebAssembly === 'object' &&
    typeof fetch === 'function' &&
    typeof ReadableStream === 'function' &&
    typeof TextDecoder === 'function'
  );
}

// French if the browser is in French, English otherwise.
setLocale(resolveLocale('auto', navigator.language));

// `viewer.html` holds a message for a browser too old to get here; it stays if this one lacks
// something the viewer needs.
if (supported()) target.replaceChildren();

export default supported() ? mount(Viewer, { target }) : undefined;
