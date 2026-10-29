import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import './tag-colors.css';
import Presentation from './components/live/Presentation.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('#app element missing from index.html');

// The classroom's presentation window (`presentation.rs`) loads `index.html?view=presentation`.
const presentation = new URLSearchParams(location.search).get('view') === 'presentation';

export default presentation ? mount(Presentation, { target }) : mount(App, { target });
