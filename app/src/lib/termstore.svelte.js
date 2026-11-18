// État du terminal intégré. Partagé : il survit à la fermeture du panneau et au changement d'onglet.
// Importé par App.svelte, il écoute les événements du Rust dès le démarrage de l'app,
// donc les commandes de l'IA sont conservées même si le terminal n'a jamais été ouvert.
// Plusieurs terminaux possibles : `term` est le principal (« main », reçoit les commandes de l'IA),
// les autres sont créés avec addTerm().
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const MAX_LINES = 2000;
// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07/g;

function make(id, name) {
  const s = $state({
    id,
    name,
    /** { k: 'in' | 'out' | 'err' | 'sys' | 'ai', t, open?, cwd?, cmd?, err? } */
    lines: [],
    alive: false,
    starting: false,
    /** Commandes envoyées dont le marqueur de fin n'est pas encore revenu. */
    pending: 0,
    cwd: '',
    sid: 0,
    unread: 0,
    history: []
  });
  return s;
}

export const term = make('main', 'Terminal 1');
export const tabs = $state({ list: [term], active: 'main' });
let seq = 1;
let sidSeq = 0;

export function addTerm() {
  seq += 1;
  const t = make('t' + seq, 'Terminal ' + seq);
  tabs.list.push(t);
  tabs.active = t.id;
}

export async function closeTerm(t) {
  if (!t || t.id === 'main') return;
  const i = tabs.list.findIndex((s) => s.id === t.id);
  if (i < 0) return;
  if (tabs.active === t.id) tabs.active = (tabs.list[i - 1] ?? term).id;
  tabs.list.splice(i, 1);
  try {
    await invoke('term_kill', { id: t.id });
  } catch {
    /* déjà arrêté */
  }
}

function trim(t) {
  if (t.lines.length > MAX_LINES) t.lines.splice(0, t.lines.length - MAX_LINES);
}

function closeOpen(t) {
  const last = t.lines[t.lines.length - 1];
  if (last) last.open = false;
}

function put(t, k, text) {
  text = String(text ?? '').replace(ANSI, '').replace(/\r\n/g, '\n').replace(/\r/g, '');
  if (!text) return;
  const parts = text.split('\n');
  parts.forEach((p, i) => {
    const last = t.lines[t.lines.length - 1];
    if (last && last.open && last.k === k) last.t += p;
    else if (p !== '' || i < parts.length - 1) t.lines.push({ k, t: p, open: true });
    if (i < parts.length - 1) closeOpen(t);
  });
  trim(t);
}

let listening = false;
export async function initTerm() {
  if (listening) return;
  listening = true;
  try {
    await listen('term:event', (e) => {
      const p = e.payload;
      const t = p && tabs.list.find((s) => s.sid === p.sid);
      if (!t) return;
      if (p.kind === 'out' || p.kind === 'err') {
        put(t, p.kind, p.data);
      } else if (p.kind === 'done') {
        closeOpen(t);
        t.pending = Math.max(0, t.pending - 1);
        if (p.data) t.cwd = p.data;
      } else if (p.kind === 'exit') {
        closeOpen(t);
 t.alive = false;
        t.pending = 0;
 put(t, 'sys', '[shell terminé]\n');
      }
    });
    await listen('term:ai', (e) => {
      const p = e.payload || {};
      closeOpen(term);
      term.lines.push({
        k: 'ai',
 cmd: String(p.command ?? ''),
        t: String(p.output ?? '').replace(ANSI, '').replace(/\r\n/g, '\n'),
 err: !!p.isError,
 cwd: p.cwd || '',
        open: false
      });
      term.unread += 1;
      trim(term);
    });
  } catch {
    listening = false; // hors Tauri (navigateur) : pas de terminal
  }
}

export async function startTerm(t, cwd) {
  if (t.alive || t.starting) return;
  t.starting = true;
  const sid = Date.now() * 10 + (sidSeq++ % 10);
  t.sid = sid;
  t.pending = 1; // le marqueur initial
  try {
    t.sid = await invoke('term_start', { id: t.id, sid, cwd: cwd || null });
    t.alive = true;
  } catch (e) {
    t.pending = 0;
    put(t, 'sys', `Impossible de lancer le shell : ${e}\n`);
  }
  t.starting = false;
}

export async function runTerm(t, text, cwd) {
  const cmd = String(text ?? '').replace(/\s+$/, '');
  if (!cmd.trim()) return;
  if (!t.alive) await startTerm(t, cwd);
  if (!t.alive) return;
  if (t.history[t.history.length - 1] !== cmd) t.history.push(cmd);
  closeOpen(t);
  t.lines.push({ k: 'in', t: cmd, cwd: t.cwd, open: false });
  trim(t);
  t.pending += 1;
  try {
    await invoke('term_write', { id: t.id, text: cmd });
  } catch (e) {
    t.pending = Math.max(0, t.pending - 1);
    t.alive = false;
    put(t, 'sys', `Erreur : ${e}\n`);
  }
}

export async function killTerm(t) {
  try {
    await invoke('term_kill', { id: t.id });
  } catch {
    /* déjà arrêté */
  }
  t.alive = false;
  t.pending = 0;
  closeOpen(t);
  put(t, 'sys', '[interrompu : le shell redémarre à la prochaine commande]\n');
}

export function clearTerm(t) {
  t.lines.splice(0, t.lines.length);
}

initTerm();
