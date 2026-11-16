// Suivi en direct des agents du « swarm » (voir src-tauri/src/agents_live.rs).
// Flux : événement Tauri `agents:live`. Rattrapage à l'ouverture : commande `agents_live_snapshot`.
// Rien ne quitte l'appli ; la transcription complète de chaque agent est sur le disque (fiche.file).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { resolveTool } from './api.js';

const FEED_MAX = 400; // blocs gardés par agent
const NOTES_MAX = 200; // lignes du journal général
const TEXT_MAX = 60000; // taille max d'un bloc de texte fusionné
const AGENTS_MAX = 80; // fiches gardées côté interface

// État partagé : on ne le réassigne jamais en entier, on modifie ses champs.
export const live = $state({
  agents: [], // fiches (id, run, name, role, wave, model, task, status, started, ended, steps, last, file, chatDeleted)
  feeds: {}, // id d'agent -> blocs { key, type: text|reasoning|tool|result|note, ... }
  notes: [], // journal général (vagues, bilans) { key, text, t }
  waiting: [], // demandes d'autorisation en attente { uid, agent, name, args }
  dir: '', // dossier racine des transcriptions sur le PC
  error: ''
});

// Valeurs calculées à la demande (les getters restent réactifs dans les composants Svelte).
export const view = {
  get running() {
    return live.agents.filter((a) => a.status === 'running');
  },
  get count() {
    return live.agents.filter((a) => a.status === 'running').length;
  },
  get active() {
    return live.waiting.length > 0 || live.agents.some((a) => a.status === 'running');
  }
};

// ---------- Déduplication ----------
// Les événements peuvent arriver deux fois (rattrapage + flux) ou dans le désordre entre agents.
let maxSeq = 0;
const seen = new Set();

function accept(n) {
  if (typeof n !== 'number' || seen.has(n)) return false;
  seen.add(n);
  if (n > maxSeq) maxSeq = n;
  if (seen.size > 8000) {
    for (const s of seen) if (s < maxSeq - 4000) seen.delete(s);
  }
  return true;
}

// ---------- Mise à jour de l'état ----------
let keyN = 0;

function feed(id) {
  // Pas de `??=` : l'expression renverrait le tableau brut, pas le proxy réactif.
  if (!live.feeds[id]) live.feeds[id] = [];
  return live.feeds[id];
}

function push(id, block) {
  const f = feed(id);
  f.push({ key: ++keyN, ...block });
  if (f.length > FEED_MAX) f.splice(0, f.length - FEED_MAX);
}

function upsert(a) {
  const i = live.agents.findIndex((x) => x.id === a.id);
  if (i < 0) live.agents.push(a);
  else live.agents[i] = a;
}

function trim() {
  while (live.agents.length > AGENTS_MAX) {
    const i = live.agents.findIndex((a) => a.status !== 'running');
    if (i < 0) return;
    const [gone] = live.agents.splice(i, 1);
    delete live.feeds[gone.id];
  }
}

function unwait(test) {
  const keep = live.waiting.filter((w) => !test(w));
  if (keep.length !== live.waiting.length) live.waiting = keep;
}

function dropIdle() {
  // Une demande d'autorisation n'a de sens que pour un agent encore en cours.
  unwait((w) => !live.agents.some((a) => a.id === w.agent && a.status === 'running'));
}

function purgeFinished() {
  const gone = live.agents.filter((a) => a.status !== 'running');
  live.agents = live.agents.filter((a) => a.status === 'running');
  for (const a of gone) delete live.feeds[a.id];
  live.notes = [];
  dropIdle();
}

function apply(e) {
  if (!e || !accept(e.seq)) return;
  const id = e.agent ?? '';
  switch (e.kind) {
    case 'agent': {
      if (!e.data) break;
      upsert(e.data);
      if (e.data.status !== 'running') dropIdle();
      trim();
      break;
    }
    case 'text':
    case 'reasoning': {
      const piece = e.text ?? '';
      const f = feed(id);
      const last = f[f.length - 1];
      if (last && last.type === e.kind) {
        last.text += piece;
        if (last.text.length > TEXT_MAX) last.text = '…' + last.text.slice(-TEXT_MAX);
      } else {
        push(id, { type: e.kind, text: piece });
      }
      break;
    }
    case 'tool': {
      push(id, {
        type: 'tool',
        uid: e.uid,
        name: e.name,
        args: e.args,
        confirm: !!e.confirm,
        result: null,
        t: e.t
      });
      if (e.confirm) {
        live.waiting.push({ uid: e.uid, agent: id, name: e.name, args: e.args });
      }
      break;
    }
    case 'tool_result': {
      const result = { output: e.output ?? '', error: !!e.error, denied: !!e.denied };
      const f = feed(id);
      let blk = null;
      for (let i = f.length - 1; i >= 0; i--) {
        if (f[i].type === 'tool' && f[i].uid === e.uid) {
          blk = f[i];
          break;
        }
      }
      if (blk) blk.result = result;
      else push(id, { type: 'result', ...result, t: e.t });
      unwait((w) => w.uid === e.uid);
      break;
    }
    case 'note': {
      if (id) {
        push(id, { type: 'note', text: e.text ?? '', t: e.t });
      } else {
        live.notes.push({ key: ++keyN, text: e.text ?? '', t: e.t });
        if (live.notes.length > NOTES_MAX) live.notes.splice(0, live.notes.length - NOTES_MAX);
      }
      break;
    }
    case 'clear': {
      purgeFinished();
      break;
    }
  }
}

// ---------- Démarrage (idempotent) ----------
let starting = null;
let unlisten = null;
let booting = false;
let buffer = [];

// Branche le flux puis rattrape l'historique. Les événements reçus pendant le rattrapage
// sont mis en attente, puis rejoués (la déduplication évite les doublons).
export function init() {
  if (starting) return starting;
  starting = (async () => {
    booting = true;
    try {
      if (!unlisten) {
        unlisten = await listen('agents:live', (ev) => {
          if (booting) buffer.push(ev.payload);
          else apply(ev.payload);
        });
      }
      const snap = await invoke('agents_live_snapshot', { since: 0 });
      live.dir = snap?.dir ?? '';
      for (const a of snap?.agents ?? []) upsert(a);
      for (const e of snap?.entries ?? []) apply(e);
      dropIdle();
      trim();
      live.error = '';
    } catch (e) {
      live.error = String(e?.message ?? e);
      starting = null; // autorise un nouvel essai au prochain init()
    } finally {
      booting = false;
      const queued = buffer;
      buffer = [];
      for (const e of queued) apply(e);
    }
  })();
  return starting;
}

// ---------- Actions ----------

// Annule un agent (id) ; renvoie false si rien à annuler.
export const cancel = (id) => invoke('agents_live_cancel', { id });

// Annule tous les agents en cours.
export const cancelAll = () => invoke('agents_live_cancel', { id: '*' });

// Retire du panneau les agents terminés (les transcriptions sur le PC restent).
export async function clearFinished() {
  await invoke('agents_live_clear');
  purgeFinished();
}

// Répond à une demande d'autorisation d'un agent (même identifiant que la carte du chat principal).
export async function allow(uid, ok) {
  await resolveTool(uid, ok);
  unwait((w) => w.uid === uid);
}
