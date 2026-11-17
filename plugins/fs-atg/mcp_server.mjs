import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import {
  ATG,
  extractTargetFilesFromPrompt,
  fetchAgentsAndModels,
  fetchQuotaSnapshot,
  fsJson,
  loadState,
  parseFileBlocks,
  pickModel,
  resolveSafe,
  saveState,
  streamGeneration,
  stripFileBlocks,
  writeFileBlocks,
  writeLiveJobs,
} from './fs_core.mjs';

const INSTRUCTIONS = `Développeur expert. Sois direct et concis.
RÈGLES STRICTES DE FORMAT ET QUALITÉ :
1. AFFICHE TOUJOURS L'INTÉGRALITÉ DU CODE dans ton message. Ne réponds JAMAIS uniquement "Je crée les fichiers..." sans écrire le code complet.
2. Pour chaque fichier demandé, fournis le code source complet et exhaustif au format standard :
<<<FILE chemin/relatif
contenu intégral et directement exécutable (sans placeholder, sans coupure, sans TODO)
>>>END
3. Si le projet comporte plusieurs fichiers (ex: index.html, style.css, audio.js, main.js), procède avec la plus grande rigueur technique fichier par fichier.
4. Qualité visuelle & technique de haut niveau :
   - Design soigné, moderne, responsive, animations fluides (60 FPS), palette harmonieuse et contrastée (néon / glassmorphism si jeu arcade/cyberpunk).
   - Code JavaScript propre, modulaire, sans variable non déclarée, compatible ES Modules natif sans build step.
   - Web Audio API procédurale intégrée avec gestion de l'AudioContext sur interaction utilisateur.
5. Appelle également l'outil write_file({ path, content }) pour garantir l'écriture directe sur disque.`;

function validateFileSyntax(absPath, relPath) {
  try {
    if (!existsSync(absPath)) return null;
    const lower = relPath.toLowerCase();
    if (lower.endsWith('.js') || lower.endsWith('.mjs') || lower.endsWith('.cjs')) {
      try {
        execFileSync(process.execPath, ['-c', absPath], { stdio: 'pipe', encoding: 'utf-8', timeout: 5000 });
      } catch (err) {
        return `Erreur de syntaxe JavaScript dans ${relPath} :\n${err.stderr || err.message}`;
      }
    } else if (lower.endsWith('.json')) {
      try {
        const content = readFileSync(absPath, 'utf-8');
        JSON.parse(content);
      } catch (err) {
        return `Erreur de syntaxe JSON dans ${relPath} :\n${err.message}`;
      }
    }
  } catch { /* ignore */ }
  return null;
}

const FINAL = new Set(['done', 'error', 'cancelled']);
const jobs = new Map();
const running = new Map();
const queue = [];
const listeners = new Set();

function notify() {
  const live = [...jobs.values()].map(publicView);
  writeLiveJobs(live);
  for (const fn of listeners) fn();
}

function publicView(j) {
  const end = j.endedAt ?? Date.now();
  return {
    jobId: j.id,
    title: j.title,
    status: j.status,
    model: j.model,
    route: j.routeReason,
    elapsedSec: j.startedAt ? Math.round((end - j.startedAt) / 1000) : 0,
    receivedChars: j.receivedChars,
    summary: j.summary,
    filesWritten: j.written ?? [],
    lastThoughts: (j.thoughts || []).slice(-3),
    error: j.error,
    startedAt: j.startedAt,
    endedAt: j.endedAt,
    usage: j.usage,
    chatId: j.chatId,
  };
}

function finishJob(job, status) {
  job.status = status;
  job.endedAt = Date.now();
  running.delete(job.id);
  jobs.delete(job.id);
  const st = loadState();
  const histItem = {
    ...publicView(job),
    task: job.task,
    isRaw: Boolean(job.isRaw || job.raw),
  };
  st.history = [histItem, ...(st.history || []).filter((h) => (h.jobId || h.id) !== job.id)].slice(0, 50);
  saveState(st);
  notify();
  pump();
}

async function runJob(job) {
  const ac = new AbortController();
  running.set(job.id, ac);
  job.status = 'running';
  job.startedAt = Date.now();
  notify();

  let thoughtBuf = '';
  const flushThought = () => {
    const t = thoughtBuf.trim();
    if (t) { job.thoughts.push(t); job.thoughts = job.thoughts.slice(-20); }
    thoughtBuf = '';
  };

  try {
    const st = loadState();
    const q = await fetchQuotaSnapshot(st);
    if (q?.remainingPct !== null && q.remainingPct < (st.cutoffPercent ?? 15)) {
      throw new Error(`Coupe-circuit : il reste ${q.remainingPct.toFixed(0)} % (seuil ${st.cutoffPercent} %).`);
    }

    const { ids, agentMap } = await fetchAgentsAndModels(st);
    const route = pickModel(job.task, ids, st, job.explicitModel);
    job.model = route.model;
    job.routeReason = route.reason;
    job.isRaw = Boolean(route.isRaw || job.raw);
    notify();

    let prompt = route.prompt.replace(/^\s*Tu es un agent de code délégué[^.]*\.\s*/i, '');
    const modelKey = route.model;
    let resumeChatId = job.chatId || null;
    if (!resumeChatId && job.parentId) {
      const parent = st.history?.find((h) => (h.jobId || h.id) === job.parentId);
      if (parent) {
        if (!resumeChatId && parent.chatId) resumeChatId = parent.chatId;
        prompt = `Contexte (tâche précédente « ${parent.title} ») :\n${parent.summary}\n` +
          `Fichiers touchés : ${(parent.written || []).join(', ') || 'aucun'}\n\nNouvelle demande : ${prompt}`;
      }
    }
    // Réutilisation automatique de la discussion active pour ce modèle
    if (!resumeChatId && st.activeChats?.[modelKey]) {
      resumeChatId = st.activeChats[modelKey];
    }

    if (job.files?.length && job.workspaceRoot) {
      const fileChunks = [];
      for (const rel of job.files) {
        const { abs } = resolveSafe(job.workspaceRoot, rel);
        const content = readFileSync(abs, 'utf-8');
        fileChunks.push(`<<<FILE ${rel}\n${content}\n>>>END`);
      }
      prompt += '\n\nFichiers joints :\n' + fileChunks.join('\n\n');
    }

    let liveText = '';
    let lastPush = 0;
    const result = await streamGeneration({
      state: st,
      model: route.model,
      agentMap,
      instructions: job.isRaw
        ? 'Tu es un assistant IA expert, bienveillant et direct. Réponds naturellement et clairement.'
        : INSTRUCTIONS,
      prompt,
      workspaceRoot: job.isRaw ? undefined : job.workspaceRoot,
      clientTools: (job.isRaw || !job.workspaceRoot) ? undefined : ['list_directory', 'read_file', 'search_files', 'edit_file', 'write_file'],
      chatId: resumeChatId,
      isRaw: job.isRaw,
      signal: ac.signal,
      onChatId: (cid) => {
        job.chatId = cid;
        st.activeChats = st.activeChats || {};
        st.activeChats[modelKey] = cid;
        saveState(st);
      },
      onText: (d) => {
        job.receivedChars += d.length;
        liveText += d;
        job.summary = liveText.slice(-300);
        if (Date.now() - lastPush > 600) { lastPush = Date.now(); notify(); }
      },
      onToolCall: (call) => {
        job.thoughts.push(`Outil : ${call.name}(${JSON.stringify(call.arguments ?? {}).slice(0, 100)})`);
        job.thoughts = job.thoughts.slice(-20);
        notify();
      },
      onReasoning: (d) => {
        thoughtBuf += d;
        if (thoughtBuf.includes('\n') || thoughtBuf.length > 240) { flushThought(); notify(); }
      },
    });
    flushThought();

    const targetPaths = extractTargetFilesFromPrompt(job.task);
    const blocks = parseFileBlocks(result.text, targetPaths);
    const blockWritten = job.workspaceRoot ? writeFileBlocks(job.workspaceRoot, blocks) : [];
    const allWritten = [...new Set([...(result.written ?? []), ...blockWritten])];

    // Vérification finale sur PC : si des fichiers cibles ont été écrits ou existent
    if (job.workspaceRoot && targetPaths.length > 0) {
      for (const tp of targetPaths) {
        try {
          const { rel, abs } = resolveSafe(job.workspaceRoot, tp);
          if (existsSync(abs) && statSync(abs).size > 10 && !allWritten.includes(rel)) {
            allWritten.push(rel);
          }
        } catch { /* ignore */ }
      }
    }
    // Auto-correction intelligente : vérification de syntaxe et réparation automatique
    if (job.workspaceRoot && allWritten.length > 0 && !ac.signal.aborted) {
      for (const rel of allWritten) {
        try {
          const { abs } = resolveSafe(job.workspaceRoot, rel);
          const syntaxErr = validateFileSyntax(abs, rel);
          if (syntaxErr) {
            job.thoughts.push(`[Auto-correction] Erreur détectée dans ${rel} : ${syntaxErr.split('\n')[0]}`);
            notify();

            const fixPrompt = `[ATG] Auto-correction requise pour le fichier "${rel}".\n` +
              `Une erreur de syntaxe a été détectée :\n${syntaxErr}\n\n` +
              `Fournis le code source complet, corrigé et immédiatement fonctionnel de "${rel}" au format :\n` +
              `<<<FILE ${rel}\ncontenu complet corrigé\n>>>END`;

            const fixResult = await streamGeneration({
              state: st,
              model: route.model,
              agentMap,
              instructions: INSTRUCTIONS,
              prompt: fixPrompt,
              workspaceRoot: job.workspaceRoot,
              clientTools: ['write_file'],
              chatId: job.chatId || resumeChatId,
              signal: ac.signal,
              onText: (d) => { job.receivedChars += d.length; },
              onReasoning: (d) => { thoughtBuf += d; if (thoughtBuf.length > 200) flushThought(); },
            });

            const fixBlocks = parseFileBlocks(fixResult.text, [rel]);
            if (fixBlocks.length > 0) {
              writeFileBlocks(job.workspaceRoot, fixBlocks);
              const recheck = validateFileSyntax(abs, rel);
              if (!recheck) {
                job.thoughts.push(`[Auto-correction] ${rel} corrigé avec succès.`);
              } else {
                job.thoughts.push(`[Auto-correction] Avertissement : ${rel} contient encore une erreur.`);
              }
              notify();
            }
          }
        } catch (err) {
          console.error(`[FS-ATG] Auto-correction error on ${rel}:`, err?.message);
        }
      }
    }

    job.written = allWritten;
    job.summary = stripFileBlocks(result.text).slice(0, 2000);
    job.usage = result.usage;
    finishJob(job, 'done');
  } catch (e) {
    flushThought();
    if (ac.signal.aborted) finishJob(job, 'cancelled');
    else { job.error = e.message; finishJob(job, 'error'); }
  }
}

function pump() {
  const st = loadState();
  const limit = st.mode === 'eco' ? 1 : (st.maxParallel ?? 3);
  while (running.size < limit && queue.length) {
    const job = queue.shift();
    void runJob(job);
  }
}

function waitFor(id, ms) {
  const find = () => jobs.get(id) ?? loadState().history?.find((h) => h.id === id);
  const cur = find();
  if (!cur || FINAL.has(cur.status) || ms <= 0) return Promise.resolve(cur);
  return new Promise((resolve) => {
    const done = () => {
      const j = find();
      if (j && FINAL.has(j.status)) {
        clearTimeout(t);
        listeners.delete(done);
        resolve(j);
      }
    };
    listeners.add(done);
    const t = setTimeout(() => { listeners.delete(done); resolve(find()); }, ms);
  });
}

const asText = (o) => ({ content: [{ type: 'text', text: JSON.stringify(o, null, 2) }] });
const server = new McpServer({ name: 'fs-atg', version: '0.1.0' });

server.registerTool('fs_delegate', {
  title: 'Déléguer à France Student [ATG]',
  description: 'Délègue une tâche de code ou une question à un agent France Student. Commandes slash supportées dans task : /opus, /sonnet, /haiku, /fable, /sol, /gpt6, /gpt6.1, /terra, /luna, /astra, /raw (mode direct sans [ATG]).',
  inputSchema: {
    task: z.string().min(1).describe('Consigne. Commandes slash : /opus, /sonnet, /sol, /raw (mode direct sans [ATG])...'),
    raw: z.boolean().optional().describe('Si true, envoie sans préfixe [ATG] ni contraintes de format de code.'),
    workspaceRoot: z.string().optional().describe('Chemin absolu du workspace où écrire les fichiers générés.'),
    files: z.array(z.string()).optional().describe('Chemins relatifs au workspace à joindre.'),
    model: z.string().optional(),
    priority: z.number().int().min(0).max(9).optional(),
    parentJobId: z.string().optional().describe("Suite d'un job : seul son résumé est réutilisé."),
    chatId: z.string().optional().describe("ID d'un chat existant pour continuer dans la même conversation."),
  },
}, async (a) => {
  const isDirect = Boolean(a.raw || /^\s*\/(raw|chat|direct)\b/i.test(a.task));
  const firstLine = a.task.split('\n')[0].replace(/^\s*(?:\[[^\]]+\]|\/[a-zA-ZÀ-ÿ0-9_.-]+)\s*/, '');
  const titleTag = isDirect ? '[CHAT]' : ATG;
  const job = {
    id: randomUUID().slice(0, 8),
    title: `${titleTag} ${firstLine.slice(0, 70)}${firstLine.length > 70 ? '…' : ''}`,
    task: a.task,
    raw: isDirect,
    workspaceRoot: a.workspaceRoot,
    files: a.files ?? [],
    explicitModel: a.model,
    parentId: a.parentJobId,
    chatId: a.chatId,
    priority: a.priority ?? 5,
    status: 'queued',
    createdAt: Date.now(),
    receivedChars: 0,
    thoughts: [],
    summary: '',
    written: [],
  };
  jobs.set(job.id, job);
  queue.push(job);
  queue.sort((x, y) => y.priority - x.priority || x.createdAt - y.createdAt);
  notify();
  pump();
  return asText({ jobId: job.id, status: job.status, next: 'Appelle fs_job_status avec waitSeconds=45.' });
});

server.registerTool('fs_job_status', {
  title: "État d'un job [ATG]",
  description: "Attend jusqu'à waitSeconds que le job finisse, puis renvoie son état, son résumé et les fichiers écrits.",
  inputSchema: { jobId: z.string(), waitSeconds: z.number().min(0).max(45).optional() },
}, async ({ jobId, waitSeconds }) => {
  const job = await waitFor(jobId, (waitSeconds ?? 30) * 1000);
  return asText(job ? publicView(job) : { error: 'jobId inconnu' });
});

server.registerTool('fs_quota', {
  title: 'Quota France Student',
  description: 'Pourcentage de quota restant et seuil du coupe-circuit.',
  inputSchema: {},
}, async () => {
  const st = loadState();
  const q = await fetchQuotaSnapshot(st);
  return asText({ remainingPct: q.remainingPct, cutoff: st.cutoffPercent, resetAt: q.resetAt });
});

server.registerTool('fs_cancel', {
  title: 'Annuler un job [ATG]',
  description: 'Annule un job en file ou en cours.',
  inputSchema: { jobId: z.string() },
}, async ({ jobId }) => {
  const job = jobs.get(jobId);
  if (job) {
    if (job.status === 'queued') {
      const idx = queue.indexOf(job);
      if (idx >= 0) queue.splice(idx, 1);
      finishJob(job, 'cancelled');
    } else {
      if (job.chatId) {
        const st = loadState();
        void fsJson(st, 'POST', `/chats/${job.chatId}/stream/cancel`).catch(() => {});
      }
      running.get(jobId)?.abort();
    }
  }
  return asText({ ok: true });
});

server.registerTool('fs_reset_chat', {
  title: 'Réinitialiser la discussion active [ATG]',
  description: 'Réinitialise le lien de discussion pour un modèle (ou tous les modèles) afin de démarrer une nouvelle conversation.',
  inputSchema: { model: z.string().optional().describe('Modèle spécifique à réinitialiser (ex: claude-opus-5-5). Si omis, réinitialise tous les modèles.') },
}, async ({ model }) => {
  const st = loadState();
  if (model) {
    if (st.activeChats?.[model]) delete st.activeChats[model];
  } else {
    st.activeChats = {};
  }
  saveState(st);
  return asText({ ok: true, activeChats: st.activeChats || {} });
});

server.registerTool('fs_write_file', {
  title: 'Écrire un fichier dans le workspace [ATG]',
  description: 'Écrit ou met à jour directement un fichier sur le PC dans le workspace.',
  inputSchema: {
    path: z.string().describe('Chemin relatif du fichier dans le workspace.'),
    content: z.string().describe('Contenu complet du fichier.'),
    workspaceRoot: z.string().optional().describe('Chemin absolu du workspace.'),
  },
}, async ({ path, content, workspaceRoot }) => {
  const root = workspaceRoot || process.cwd();
  const { rel, abs } = resolveSafe(root, path);
  mkdirSync(dirname(abs), { recursive: true });
  writeFileSync(abs, content, 'utf-8');
  return asText({ ok: true, path: rel, bytes: content.length });
});

const transport = new StdioServerTransport();
await server.connect(transport);
