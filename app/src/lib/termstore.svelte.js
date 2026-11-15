// État du terminal intégré. Partagé : il survit à la fermeture du panneau et au changement d'onglet.
// Importé par App.svelte, il écoute les événements du Rust dès le démarrage de l'app,
// donc les commandes de l'IA sont conservées même si le terminal n'a jamais été ouvert.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const ID = 'main';
const MAX_LINES = 2000;
// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07/g;

export const term = $state({
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

function trim() {
  if (term.lines.length > MAX_LINES) term.lines.splice(0, term.lines.length - MAX_LINES);
}

function closeOpen() {
  const last = term.lines[term.lines.length - 1];
  if (last) last.open = false;
}

function put(k, text) {
  text = String(text ?? '').replace(ANSI, '').replace(/\r\n/g, '\n').replace(/\r/g, '');
  if (!text) return;
  const parts = text.split('\n');
  parts.forEach((p, i) => {
    const last = term.lines[term.lines.length - 1];
    if (last && last.open && last.k === k) last.t += p;
    else if (p !== '' || i < parts.length - 1) term.lines.push({ k, t: p, open: true });
    if (i < parts.length - 1) closeOpen();
  });
  trim();
}

let listening = false;
export async function initTerm() {
  if (listening) return;
  listening = true;
  try {
    await listen('term:event', (e) => {
      const p = e.payload;
      if (!p || p.sid !== term.sid) return;
      if (p.kind === 'out' || p.kind === 'err') {
        put(p.kind, p.data);
      } else if (p.kind === 'done') {
        closeOpen();
        term.pending = Math.max(0, term.pending - 1);
        if (p.data) term.cwd = p.data;
      } else if (p.kind === 'exit') {
        closeOpen();
        term.alive = false;
        term.pending = 0;
        put('sys', '[shell terminé]\n');
      }
    });
    await listen('term:ai', (e) => {
      const p = e.payload || {};
      closeOpen();
      term.lines.push({
        k: 'ai',
        cmd: String(p.command ?? ''),
        t: String(p.output ?? '').replace(ANSI, '').replace(/\r\n/g, '\n'),
        err: !!p.isError,
        cwd: p.cwd || '',
        open: false
      });
      term.unread += 1;
      trim();
    });
  } catch {
    listening = false; // hors Tauri (navigateur) : pas de terminal
  }
}

export async function startTerm(cwd) {
  if (term.alive || term.starting) return;
  term.starting = true;
  const sid = Date.now();
  term.sid = sid;
  term.pending = 1; // le marqueur initial
  try {
    term.sid = await invoke('term_start', { id: ID, sid, cwd: cwd || null });
    term.alive = true;
  } catch (e) {
    term.pending = 0;
    put('sys', `Impossible de lancer le shell : ${e}\n`);
  }
  term.starting = false;
}

export async function runTerm(text, cwd) {
  const cmd = String(text ?? '').replace(/\s+$/, '');
  if (!cmd.trim()) return;
  if (!term.alive) await startTerm(cwd);
  if (!term.alive) return;
  if (term.history[term.history.length - 1] !== cmd) term.history.push(cmd);
  closeOpen();
  term.lines.push({ k: 'in', t: cmd, cwd: term.cwd, open: false });
  trim();
  term.pending += 1;
  try {
    await invoke('term_write', { id: ID, text: cmd });
  } catch (e) {
    term.pending = Math.max(0, term.pending - 1);
    term.alive = false;
    put('sys', `Erreur : ${e}\n`);
  }
}

export async function killTerm() {
  try {
    await invoke('term_kill', { id: ID });
  } catch {
    /* déjà arrêté */
  }
  term.alive = false;
  term.pending = 0;
  closeOpen();
  put('sys', '[interrompu : le shell redémarre à la prochaine commande]\n');
}

export function clearTerm() {
  term.lines.splice(0, term.lines.length);
}

initTerm();
