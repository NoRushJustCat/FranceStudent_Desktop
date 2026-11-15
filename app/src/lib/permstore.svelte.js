// Mode d'autorisation des outils (miroir de `perm.rs`).
// Importé par App.svelte : il restaure le mode au démarrage et le pousse au Rust.
// Le mode « système » n'est JAMAIS restauré : au redémarrage on retombe sur « ask ».

import { invoke } from '@tauri-apps/api/core';

const KEY = 'fscode.perm';

export const MODES = [
  { id: 'ask', label: 'Demander', desc: 'Confirmation avant toute édition ou commande (comportement habituel).' },
  { id: 'plan', label: 'Planification', desc: "Lecture seule : l'IA explore et propose un plan. Éditions et commandes refusées." },
  { id: 'auto', label: 'Auto', desc: "Règles automatiques : lectures et commandes connues sûres passent, le reste est confirmé." },
  { id: 'project', label: 'Projet', desc: 'Tout dans le dossier du projet sans confirmation. Commandes dangereuses ou hors projet : confirmées.' },
  { id: 'full', label: 'Système', desc: 'Accès complet sans confirmation, y compris hors du projet. À vos risques.' },
];

function saved() {
  try {
    const v = localStorage.getItem(KEY);
    if (v && v !== 'full' && MODES.some((m) => m.id === v)) return v;
  } catch {}
  return 'ask';
}

export const perm = $state({ mode: saved() });

function push(id) {
  return invoke('set_perm_mode', { mode: id }).catch(() => {});
}

/** Change le mode. Renvoie true si appliqué (le mode « système » demande confirmation). */
export async function setMode(id) {
  if (!MODES.some((m) => m.id === id) || id === perm.mode) return false;
  if (id === 'full') {
    const ok = window.confirm(
      "Mode Système : l'IA pourra lire, écrire et exécuter des commandes sur tout votre PC, " +
        'sans confirmation. Activer ?'
    );
    if (!ok) return false;
  }
  await push(id);
  perm.mode = id;
  try {
    // « full » n'est pas mémorisé : au redémarrage on revient à « ask ».
    localStorage.setItem(KEY, id === 'full' ? 'ask' : id);
  } catch {}
  return true;
}

// Synchro initiale avec le Rust (qui démarre toujours en « ask »).
push(perm.mode);
