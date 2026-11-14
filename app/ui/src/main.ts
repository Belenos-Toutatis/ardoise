import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import './tag-colors.css';
import { installBrowserGuard } from './lib/browser-guard';
import Presentation from './components/live/Presentation.svelte';
import EditorPresentation from './components/projection/EditorPresentation.svelte';

// No browser keys (reload, print, find…) and no web page's right-click menu in any of the app's windows; the keys only in a
// production build, where the developer tools are not the developer's (`lib/browser-guard.ts`).
installBrowserGuard(window, { keys: import.meta.env.PROD });

const target = document.getElementById('app');
if (!target) throw new Error('#app element missing from index.html');

// The classroom's presentation window (`presentation.rs`) loads `index.html?view=presentation`, the editor's
// projection window (`projection.rs`, M17) `index.html?view=projection`.
const view = new URLSearchParams(location.search).get('view');

export default view === 'presentation'
  ? mount(Presentation, { target })
  : view === 'projection'
    ? mount(EditorPresentation, { target })
    : mount(App, { target });
