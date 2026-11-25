/**
 * Native Sequential Thinking / Raisonnement approfondi MCP engine for France Student [ATG].
 * Zero external subprocess overhead: supports numbered thoughts, revisions, branches,
 * hypothesis verification, and cross-process persistent session history.
 */
import { existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { logEvent } from './logger.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
export const THINKING_PATH = join(__dirname, 'sidecars', 'fs_atg_panel', '.fsatg_thinking.json');
export const THINKING_ACTIONS = Object.freeze(['think', 'history', 'clear', 'status']);

const MAX_THOUGHTS = 80;

function loadThinkingStore() {
  try {
    if (existsSync(THINKING_PATH)) {
      const parsed = JSON.parse(readFileSync(THINKING_PATH, 'utf-8').replace(/^\uFEFF/, ''));
      if (parsed && typeof parsed === 'object') {
        const thoughts = Array.isArray(parsed.thoughts) ? parsed.thoughts.slice(-MAX_THOUGHTS) : [];
        const branches = (parsed.branches && typeof parsed.branches === 'object' && !Array.isArray(parsed.branches))
          ? parsed.branches
          : {};
        return { thoughts, branches };
      }
    }
  } catch { /* ignore */ }
  return { thoughts: [], branches: {} };
}

function saveThinkingStore(store) {
  const payload = JSON.stringify({
    thoughts: (store.thoughts || []).slice(-MAX_THOUGHTS),
    branches: store.branches || {},
    updatedAt: Date.now(),
  });
  const tmp = `${THINKING_PATH}.${process.pid}.tmp`;
  try {
    mkdirSync(dirname(THINKING_PATH), { recursive: true });
    writeFileSync(tmp, payload, 'utf-8');
    try {
      renameSync(tmp, THINKING_PATH);
    } catch {
      try { unlinkSync(tmp); } catch { /* ignore */ }
      writeFileSync(THINKING_PATH, payload, 'utf-8');
    }
  } catch {
    try { unlinkSync(tmp); } catch { /* ignore */ }
  }
}

export function getThinkingSnapshot() {
  const { thoughts, branches } = loadThinkingStore();
  const branchKeys = Object.keys(branches);
  return {
    totalSteps: thoughts.length,
    branchCount: branchKeys.length,
    branches: branchKeys,
    recent: thoughts.slice(-6).map((t) => ({
      thoughtNumber: t.thoughtNumber,
      totalThoughts: t.totalThoughts,
      isRevision: t.isRevision,
      branchId: t.branchId,
      thought: String(t.thought || '').slice(0, 160),
      at: t.at,
    })),
  };
}

export async function executeThinkingAction(params = {}, { deepThinkingEnabled = true } = {}) {
  const action = String(params?.action || (params?.thought ? 'think' : 'status')).trim().toLowerCase();

  if (action === 'status' || action === 'history') {
    const store = loadThinkingStore();
    return {
      ok: true,
      enabled: deepThinkingEnabled !== false,
      action,
      ...getThinkingSnapshot(),
      thoughts: action === 'history' ? store.thoughts.slice(-30) : undefined,
    };
  }

  if (action === 'clear') {
    const store = loadThinkingStore();
    const cleared = store.thoughts.length;
    saveThinkingStore({ thoughts: [], branches: {} });
    return { ok: true, action: 'clear', cleared, totalSteps: 0 };
  }

  if (action !== 'think') {
    throw new Error(`Action fs_thinking inconnue : "${action}". Utilise think, history, status ou clear.`);
  }

  if (deepThinkingEnabled === false) {
    throw new Error('L’extension MCP Raisonnement approfondi est désactivée dans Réglages.');
  }

  const thought = String(params.thought ?? '').trim();
  if (!thought) throw new Error('Paramètre "thought" requis pour une étape de raisonnement.');

  const store = loadThinkingStore();
  const thoughtNumber = Math.max(1, Math.round(Number(params.thoughtNumber) || (store.thoughts.length + 1)));
  const totalThoughts = Math.max(thoughtNumber, Math.round(Number(params.totalThoughts) || Math.max(thoughtNumber, 3)));
  const nextThoughtNeeded = params.nextThoughtNeeded !== false;
  const isRevision = Boolean(params.isRevision);
  const revisesThought = params.revisesThought != null ? Math.max(1, Math.round(Number(params.revisesThought))) : null;
  const branchFromThought = params.branchFromThought != null ? Math.max(1, Math.round(Number(params.branchFromThought))) : null;
  const branchId = params.branchId ? String(params.branchId).trim().slice(0, 48) : null;
  const hypothesis = params.hypothesis ? String(params.hypothesis).trim().slice(0, 400) : null;
  const verified = typeof params.verified === 'boolean' ? params.verified : null;

  const entry = {
    thoughtNumber,
    totalThoughts,
    nextThoughtNeeded,
    isRevision,
    revisesThought,
    branchFromThought,
    branchId,
    hypothesis,
    verified,
    thought: thought.slice(0, 4000),
    at: Date.now(),
  };

  store.thoughts.push(entry);
  if (store.thoughts.length > MAX_THOUGHTS) store.thoughts.splice(0, store.thoughts.length - MAX_THOUGHTS);

  if (branchId) {
    const list = Array.isArray(store.branches[branchId]) ? store.branches[branchId] : [];
    list.push(entry);
    store.branches[branchId] = list.slice(-30);
  }

  saveThinkingStore(store);
  logEvent('info', 'thinking', 'thinking.step', `Étape ${thoughtNumber}/${totalThoughts}${branchId ? ` [${branchId}]` : ''}${isRevision ? ' (révision)' : ''}`);

  return {
    ok: true,
    action: 'think',
    thoughtNumber,
    totalThoughts,
    nextThoughtNeeded,
    isRevision,
    revisesThought,
    branchId,
    hypothesis,
    verified,
    branches: Object.keys(store.branches),
    thoughtHistoryLength: store.thoughts.length,
  };
}
