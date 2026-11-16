import { mount } from 'svelte';
import './lib/devlog.svelte.js';
import './theme.css';
import App from './App.svelte';


mount(App, { target: document.getElementById('app') });
