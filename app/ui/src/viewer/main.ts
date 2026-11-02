import { mount } from 'svelte';
import { resolveLocale } from '../lib/i18n-core';
import { setLocale } from '../lib/i18n.svelte';
import Viewer from './Viewer.svelte';

// Entry of `viewer.html`: the page the Ardoise PC serves to the browsers of the local network
// (docs/design/lan-share.md). It imports nothing of the desktop app (no Tauri, no `lib/api.ts`).
const target = document.getElementById('viewer');
if (!target) throw new Error('#viewer element missing from viewer.html');

// French if the browser is in French, English otherwise.
setLocale(resolveLocale('auto', navigator.language));

export default mount(Viewer, { target });
