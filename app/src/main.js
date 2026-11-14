import { mount } from 'svelte';
import './theme.css';
import App from './App.svelte';
import Bridge from './lib/Bridge.svelte';

mount(App, { target: document.getElementById('app') });
// Pastille flottante du pont MCP : montée à part pour ne toucher à aucun écran existant.
mount(Bridge, { target: document.body.appendChild(document.createElement('div')) });
