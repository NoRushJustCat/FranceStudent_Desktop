import { randomUUID } from 'node:crypto';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { basename, isAbsolute, resolve } from 'node:path';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import {
  ATG,
  ANTIGRAVITY_QUOTA_VISIBILITY,
  CONNECTOR_VERSION,
  MCP_TOOL_CATALOG,
  REASONING_EFFORTS,
  acquireGlobalSlot,
  apiCredentialStatus,
  buildReasoningDirective,
  clearCancelRequest,
  compactChatContext,
  compactCatalog,
  enrichModelsCatalog,
  estimateAntigravityJobSavings,
  estimateTokens,
  fetchAgentsAndModels,
  fetchChatContext,
  fetchChatStreamMetrics,
  fetchModelCatalog,
  fetchQuotaSnapshot,
  fetchRemoteCatalog,
  formatWorkspaceSymbolIndex,
  fsJson,
  getGlobalCooldown,
  globalSlotStatus,
  isJobCancelRequested,
  isQuotaExemptModel,
  loadState,
  modelSupportsClientTools,
  normalizeProviderTokenUsage,
  normalizeReasoningEffort,
  parseTaskDirectives,
  pickModel,
  recordAntigravitySavings,
  resolveSafe,
  saveState,
  streamGenerationWithRefusalFallback,
  workspaceSketch,
  workspaceSymbolIndex,
  writeLiveJobs,
  HISTORY_LIMIT,
  buildApprovalFields,
  prepareVisionAttachments,
  runAutonomousComputerLoop,
  VISION_LIVE_INSTRUCTIONS,
} from './fs_core.mjs';
import { LOG_PATH, countRecentErrors, logEvent, readLogs } from './logger.mjs';
import {
  addProjectNote,
  buildTurn1Annex,
  clearProjectMemory,
  forgetFact,
  getProjectMemory,
  learnFact,
  observePromptSignals,
  removeProjectNote,
  setPreprompt,
} from './memory_store.mjs';
import {
  PERMISSION_MODES,
  clipText,
  createWorkspaceToolRunner,
  listUndoEntries,
  toolsForPermissionMode,
  undoLastChange,
} from './tools_engine.mjs';
import { COMPUTER_ACTIONS, executeComputerAction } from './computer_use.mjs';
import { executeCanvasAction } from './canvas_tools.mjs';
import { THINKING_ACTIONS, executeThinkingAction } from './thinking_tools.mjs';
import { executeWebFetch } from './web_fetch.mjs';
import { executeDocRead } from './doc_tools.mjs';
import { GIT_ACTIONS, executeGitAction } from './git_tools.mjs';
import { MEDIA_ACTIONS, executeMediaAction } from './media_tools.mjs';

const CLIENT_TOOLS_INSTRUCTIONS = `Utilise directement les outils locaux fournis pour lire et modifier les fichiers demandés, puis termine par un court bilan.`;

const NATURAL_INSTRUCTIONS = `Réponds clairement et directement en français, avec le code complet en bloc Markdown si du code est demandé.`;

const CANVAS_INSTRUCTIONS = `Réponds clairement en français. Mets le code ou document complet dans un document Canvas (createDocument) ou en bloc Markdown prêt à copier.`;

const PLAN_INSTRUCTIONS = `Propose un plan court et clair (étapes et fichiers clés), sans écrire tout le code.`;

const PATCH_INSTRUCTIONS = `Donne uniquement les extraits à modifier au format TARGET: puis REPLACE: en blocs de code.`;

const PLAN_B_PART_INSTRUCTIONS = `Donne directement la réponse ou le bloc de code complet demandé pour cette partie.`;

const FINAL = new Set(['done', 'error', 'cancelled', 'needs_approval']);
const EXECUTION_MODES = new Set(['generate', 'apply', 'plan', 'patch', 'dry-run']);
const ROUTING_PROFILES = new Set(['auto', 'eco', 'fast', 'quality', 'security', 'refactor']);
const jobs = new Map();
const running = new Map();
const queue = [];
const listeners = new Set();
const diagnosticState = { lastApiError: null, lastApiAt: null, lastModelsError: null, lastModelsAt: null };

function notify() {
  const live = [...jobs.values()].map(publicView);
  writeLiveJobs(live);
  for (const fn of listeners) fn();
}

const liveHeartbeat = setInterval(() => {
  if (!jobs.size) return;
  for (const [id, job] of [...jobs.entries()]) {
    if (isJobCancelRequested(id)) {
      clearCancelRequest(id);
      if (job.status === 'queued') {
        const idx = queue.indexOf(job);
        if (idx >= 0) queue.splice(idx, 1);
        finishJob(job, 'cancelled');
      } else {
        running.get(id)?.abort();
      }
    }
  }
  if (jobs.size) notify();
}, 3_500);
liveHeartbeat.unref?.();

function directSessionKey(job) {
  return job.workspaceRoot ? `workspace:${resolve(job.workspaceRoot).toLowerCase()}` : 'mcp:global';
}

function modelSessionKey(baseKey, modelId) {
  const m = String(modelId || '').trim().toLowerCase();
  return m ? `${baseKey}::${m}` : baseKey;
}

async function prepareDirectChat(st, baseKey, modelId) {
  if (st.reuseChat === false) return undefined;
  const fullKey = modelSessionKey(baseKey, modelId);
  const chatId = st.directChats?.[fullKey];
  if (!chatId) return undefined;
  try {
    const context = await fetchChatContext(st, chatId);
    const threshold = Number(st.contextCompactPercent ?? 80);
    if (typeof context?.usagePercent === 'number' && context.usagePercent >= threshold) {
      await compactChatContext(st, chatId);
    }
    return chatId;
  } catch (e) {
    if (String(e?.message ?? e).includes('HTTP 404')) {
      delete st.directChats[fullKey];
      saveState(st);
      return undefined;
    }
    return chatId;
  }
}

function rememberDirectChat(baseKey, chatId, modelId) {
  if (!chatId) return;
  const st = loadState();
  if (st.reuseChat === false) return;
  const fullKey = modelSessionKey(baseKey, modelId);
  st.directChats = { ...(st.directChats || {}), [fullKey]: chatId };
  saveState(st);
}

function contextModeOf(job, st) {
  return ['minimal', 'targeted', 'smart'].includes(job.contextMode) ? job.contextMode
    : (['minimal', 'targeted', 'smart'].includes(st.contextMode) ? st.contextMode : 'minimal');
}

function connectorSnapshot(st) {
  const perm = PERMISSION_MODES.includes(st.permissionMode) ? st.permissionMode : 'acceptEdits';
  return {
    name: 'fs-atg',
    version: CONNECTOR_VERSION,
    transport: 'stdio',
    registration: 'declared',
    readOnlyWorkspace: perm === 'plan' || st.useClientTools === false,
    permissionMode: perm,
    useClientTools: st.useClientTools !== false,
    autoDelegate: st.autoDelegate !== false,
    toolCount: MCP_TOOL_CATALOG.length,
    tools: MCP_TOOL_CATALOG,
    slots: globalSlotStatus(st),
    cooldown: getGlobalCooldown(st),
  };
}

function normalizedDelegation(task, requestedMode, requestedProfile, requestedPerm, requestedEffort) {
  const rawTask = String(task ?? '').trim();
  const directives = parseTaskDirectives(rawTask);
  const executionMode = EXECUTION_MODES.has(requestedMode) ? requestedMode : directives.executionMode;
  const routingProfile = ROUTING_PROFILES.has(requestedProfile) ? requestedProfile
    : (ROUTING_PROFILES.has(directives.profile) ? directives.profile : undefined);
  const permissionMode = PERMISSION_MODES.includes(requestedPerm) ? requestedPerm
    : (PERMISSION_MODES.includes(directives.permissionMode) ? directives.permissionMode : undefined);
  const reasoningEffort = REASONING_EFFORTS.includes(requestedEffort) ? requestedEffort
    : (REASONING_EFFORTS.includes(directives.reasoningEffort) ? directives.reasoningEffort : undefined);
  return { task: directives.prompt, rawTask, executionMode, routingProfile, permissionMode, reasoningEffort };
}

// Ciblage explicite du projet : même contrat que le panneau (`lastWorkspaceRoot` + `workspaceRootConfirmed`).
// Un `workspaceRoot` passé à fs_delegate est validé (absolu, dossier existant) puis mémorisé comme confirmé ;
// sans lui, seul un workspace déjà confirmé sert de repli et toute écriture sans cible confirmée est refusée.
function checkWorkspaceDir(raw) {
  const input = String(raw ?? '').trim().replace(/^"(.*)"$/, '$1').trim();
  if (!input) return { valid: false, root: '', reason: 'vide' };
  if (!isAbsolute(input)) return { valid: false, root: '', reason: 'le chemin doit être absolu' };
  const root = resolve(input);
  try {
    if (!existsSync(root) || !statSync(root).isDirectory()) return { valid: false, root: '', reason: 'dossier introuvable' };
  } catch {
    return { valid: false, root: '', reason: 'dossier inaccessible' };
  }
  return { valid: true, root, reason: null };
}

function resolveMcpWorkspace(explicitRaw, st, { requireForWrite = false } = {}) {
  if (String(explicitRaw ?? '').trim()) {
    const chk = checkWorkspaceDir(explicitRaw);
    if (!chk.valid) throw new Error(`workspaceRoot invalide (${chk.reason}) : ${String(explicitRaw).trim()}`);
    return { root: chk.root, explicit: true };
  }
  const stored = checkWorkspaceDir(st.lastWorkspaceRoot);
  if (stored.valid && st.workspaceRootConfirmed === true) return { root: stored.root, explicit: false };
  if (requireForWrite && stored.valid) {
    // Un workspace mémorisé mais jamais confirmé ne doit pas recevoir d'écriture silencieuse.
    throw new Error(
      `workspaceRoot requis : « ${stored.root} » n’a jamais été confirmé comme cible. Passe workspaceRoot `
      + '(chemin absolu du projet) pour autoriser l’écriture, ou utilise permissionMode="plan" / executionMode="plan" pour rester en lecture seule.',
    );
  }
  return { root: '', explicit: false };
}

function profileState(st, profile) {
  const routingProfile = ROUTING_PROFILES.has(profile) ? profile : (ROUTING_PROFILES.has(st.routingProfile) ? st.routingProfile : 'auto');
  return { ...st, routingProfile };
}

function buildNaturalPrompt(basePrompt, job, st, hasPersistentChat, { useTools = false } = {}) {
  const mode = contextModeOf(job, st);
  let next = basePrompt;
  if (job.parentId && !hasPersistentChat) {
    const parent = st.history?.find((h) => (h.id || h.jobId) === job.parentId);
    if (parent) {
      const snippet = clipText(parent.response || parent.summary || '', 1200);
      if (snippet) next = `Contexte précédent :\n${snippet}\n\nNouvelle demande : ${next}`;
    }
  }
  if (job.workspaceRoot && job.files?.length) {
    let remaining = 24_000;
    const chunks = [];
    for (const rel of job.files.slice(0, 6)) {
      try {
        const safe = resolveSafe(job.workspaceRoot, rel);
        const raw = readFileSync(safe.abs, 'utf-8');
        const slice = clipText(raw, Math.max(512, remaining));
        remaining -= slice.length;
        chunks.push(`Fichier \`${safe.rel}\` :\n\`\`\`\n${slice}\n\`\`\``);
        if (remaining <= 0) break;
      } catch (e) {
        logEvent('warn', 'mcp', 'context.file_skipped', `Impossible de lire ${rel} : ${e?.message ?? e}`, { jobId: job.id, rel });
      }
    }
    if (chunks.length) next += `\n\n${chunks.join('\n\n')}`;
  }
  if (job.workspaceRoot && !hasPersistentChat) {
    try { observePromptSignals(job.workspaceRoot, basePrompt); } catch { /* ignore */ }
    const annex = buildTurn1Annex(job.workspaceRoot, { includeShortContext: Boolean(useTools || mode === 'smart') });
    if (annex) next += `\n\n${annex}`;
  } else if (job.workspaceRoot && mode === 'smart') {
    const index = formatWorkspaceSymbolIndex(workspaceSymbolIndex(job.workspaceRoot, 30), 2_000);
    if (index) next += `\n\nAperçu de structure :\n${index}`;
    else {
      const sketch = workspaceSketch(job.workspaceRoot, 30);
      if (sketch.length) next += `\n\nArborescence :\n${sketch.join('\n')}`;
    }
  }
  return { prompt: next, mode };
}

async function sessionAction(action, workspaceRoot) {
  const st = loadState();
  const baseKey = workspaceRoot ? `workspace:${resolve(workspaceRoot).toLowerCase()}` : 'mcp:global';
  const matchingKeys = Object.keys(st.directChats || {}).filter((k) => k === baseKey || k.startsWith(`${baseKey}::`));
  const chatId = matchingKeys.length ? st.directChats[matchingKeys[matchingKeys.length - 1]] : undefined;
  if (action === 'new') {
    if (matchingKeys.length) {
      for (const k of matchingKeys) delete st.directChats[k];
      saveState(st);
    }
    logEvent('info', 'mcp', 'session.new', 'Session réinitialisée pour la prochaine requête.', { key: baseKey, cleared: matchingKeys.length });
    return { ok: true, action, chatId: null, message: 'La prochaine demande ouvrira une nouvelle session.' };
  }
  if (!chatId) return { ok: true, action, chatId: null, usagePercent: null, message: 'Aucune session active.' };
  if (action === 'compact') {
    await compactChatContext(st, chatId);
    logEvent('info', 'mcp', 'session.compact', `Contexte compacté pour la session ${chatId}.`, { chatId });
  }
  const context = await fetchChatContext(st, chatId).catch(() => ({}));
  return { ok: true, action, chatId, activeChats: matchingKeys.length, usagePercent: context?.usagePercent ?? null };
}

async function diagnosticsSnapshot() {
  const st = loadState();
  const credential = await apiCredentialStatus(st);
  const keyConfigured = credential.configured === true;
  let quota = null;
  let apiVerified = false;
  let models = { available: false, source: 'not-authenticated', count: 0 };
  if (keyConfigured) {
    try {
      quota = await fetchQuotaSnapshot(st);
      apiVerified = true;
      diagnosticState.lastApiAt = Date.now();
      diagnosticState.lastApiError = null;
    } catch (error) {
      diagnosticState.lastApiAt = Date.now();
      diagnosticState.lastApiError = String(error?.message ?? error).slice(0, 220);
      logEvent('error', 'mcp', 'diagnostics.api_error', diagnosticState.lastApiError);
    }
    try {
      const bundle = await fetchAgentsAndModels(st);
      models = { available: Boolean(bundle.available), source: bundle.source ?? 'unknown', count: bundle.ids?.length ?? 0 };
      diagnosticState.lastModelsAt = Date.now();
      diagnosticState.lastModelsError = null;
    } catch (error) {
      diagnosticState.lastModelsAt = Date.now();
      diagnosticState.lastModelsError = String(error?.message ?? error).slice(0, 220);
      logEvent('error', 'mcp', 'diagnostics.models_error', diagnosticState.lastModelsError);
    }
  }
  const session = await sessionAction('status').catch(() => ({ chatId: null, usagePercent: null }));
  return {
    credential,
    api: { verified: apiVerified, lastCheckedAt: diagnosticState.lastApiAt, error: diagnosticState.lastApiError },
    models: { ...models, lastCheckedAt: diagnosticState.lastModelsAt, error: diagnosticState.lastModelsError },
    quota: quota ? { remainingPct: quota.remainingPct ?? null, resetAt: quota.resetAt ?? null } : null,
    mcp: { ...connectorSnapshot(st), activeJobs: jobs.size, queuedJobs: queue.length },
    antigravityQuota: ANTIGRAVITY_QUOTA_VISIBILITY,
    session: { savedChatReference: Boolean(session.chatId), usagePercent: session.usagePercent ?? null },
    autoDelegate: st.autoDelegate !== false,
    recentErrors24h: countRecentErrors(),
    logPath: LOG_PATH,
  };
}

function publicView(j) {
  const end = j.endedAt ?? Date.now();
  const usage = j.usage ?? null;
  const tokenMeasurement = j.tokenMeasurement ?? {
    localPromptEstimateTokens: j.inputEstimateTokens ?? null,
    providerUsage: normalizeProviderTokenUsage(usage),
  };
  return {
    jobId: j.id || j.jobId,
    id: j.id || j.jobId,
    chatId: j.chatId ?? null,
    title: j.title,
    task: j.task || '',
    status: j.status,
    model: j.model,
    route: j.routeReason,
    planB: Boolean(j.planB),
    elapsedSec: (j.startedAt || j.createdAt) ? Math.max(0, Math.round((end - (j.startedAt || j.createdAt)) / 1000)) : 0,
    receivedChars: j.receivedChars || 0,
    response: j.response ?? j.summary ?? '',
    summary: j.summary,
    filesWritten: j.filesWritten ?? [],
    commandsRun: j.commandsRun ?? [],
    toolTurns: j.toolTurns ?? 0,
    permissionMode: j.permissionMode ?? null,
    reasoningEffort: j.reasoningEffort ?? null,
    lastThoughts: (j.thoughts || []).slice(-8),
    thoughts: (j.thoughts || []).slice(-12),
    error: j.error,
    planBRecommended: Boolean(j.planBRecommended),
    planBHint: j.planBHint ?? null,
    createdAt: j.createdAt,
    startedAt: j.startedAt,
    endedAt: j.endedAt,
    usage,
    tokenUsage: {
      inputEstimateTokens: tokenMeasurement.localPromptEstimateTokens ?? j.inputEstimateTokens ?? null,
      provider: tokenMeasurement.providerUsage ?? null,
    },
    inputEstimateTokens: j.inputEstimateTokens,
    contextMode: j.contextMode,
    executionMode: j.executionMode ?? 'generate',
    routingProfile: j.routingProfile ?? null,
    slot: j.slot,
    refusals: j.refusals ?? [],
    attemptedModels: j.attemptedModels ?? (j.model ? [j.model] : []),
    triggeredCapabilities: j.triggeredCapabilities ?? null,
    webSearchSources: j.webSearchSources ?? [],
    generatedImages: j.generatedImages ?? [],
    canvasDocuments: j.canvasDocuments ?? [],
    canvasPreview: j.canvasPreview ?? null,
    computerSteps: j.computerSteps ?? null,
    antigravitySavings: j.antigravitySavings ?? null,
  };
}

function mcpCompactView(j, verbose = false) {
  if (j.status === 'needs_approval') {
    return {
      jobId: j.id ?? j.jobId,
      status: 'needs_approval',
      model: j.approvalModel || j.model,
      policy: 'ask',
      reason: j.approvalReason || j.routeReason,
      alternatives: j.alternatives ?? [],
      message: j.approvalMessage,
      nextStep: `Après accord explicite de l’utilisateur : fs_delegate({ task, approveModel: "${j.approvalModel || j.model}" })`
        + ((j.alternatives || []).length ? `, ou fs_delegate({ task, model: "${j.alternatives[0]}" }).` : '.'),
    };
  }
  if (verbose) return publicView(j);
  const responseText = String(j.response ?? '').trim() || String(j.summary ?? '').trim();
  const filesWritten = Array.isArray(j.filesWritten) && j.filesWritten.length
    ? j.filesWritten.map(({ path, action, added, removed, undoId }) => ({ path, action, added, removed, ...(undoId ? { undoId } : {}) }))
    : undefined;
  const commandsRun = Array.isArray(j.commandsRun) && j.commandsRun.length ? j.commandsRun : undefined;
  return {
    jobId: j.id ?? j.jobId,
    ...(j.chatId ? { chatId: j.chatId } : {}),
    status: j.status,
    model: j.model,
    ...(j.permissionMode ? { permissionMode: j.permissionMode } : {}),
    ...(j.toolTurns ? { toolTurns: j.toolTurns } : {}),
    ...(j.computerSteps?.length ? { computerSteps: j.computerSteps.length } : {}),
    ...(j.triggeredCapabilities ? { triggeredCapabilities: j.triggeredCapabilities } : {}),
    ...(j.webSearchSources?.length ? { webSearchSources: j.webSearchSources.slice(0, 8) } : {}),
    ...(j.generatedImages?.length ? { generatedImages: j.generatedImages } : {}),
    ...(j.canvasPreview ? { canvasPreview: j.canvasPreview } : {}),
    ...(filesWritten ? { filesWritten } : {}),
    ...(commandsRun ? { commandsRun } : {}),
    ...(j.planB ? { planB: true } : {}),
    ...(j.partsResponses?.length ? { partsResponses: j.partsResponses } : (responseText ? { response: responseText } : {})),
    ...(j.error ? { error: j.error } : {}),
    ...(j.planBRecommended ? { planBRecommended: true, planBHint: j.planBHint } : {}),
    ...(j.refusals?.length ? { refusals: j.refusals } : {}),
    ...(j.antigravitySavings?.totalTokensSavedEst ? { savedAgTokensEst: j.antigravitySavings.totalTokensSavedEst } : {}),
  };
}

function finishJob(job, status) {
  job.status = status;
  job.endedAt = Date.now();
  running.delete(job.id);
  jobs.delete(job.id);
  const st = loadState();
  if (status === 'done') {
    job.antigravitySavings = estimateAntigravityJobSavings(job, {
      waitedInline: Boolean(job.waitedInline),
      partsCount: Array.isArray(job.partsResponses) && job.partsResponses.length ? job.partsResponses.length : 1,
    });
    recordAntigravitySavings(st, job.antigravitySavings);
    logEvent('info', 'mcp', 'job.done', `Job ${job.id} terminé (${job.model}, ${job.response?.length ?? 0} car.).`, {
      jobId: job.id,
      model: job.model,
      chars: job.response?.length ?? 0,
      durationSec: job.startedAt ? Math.round((job.endedAt - job.startedAt) / 1000) : 0,
      planB: Boolean(job.planB),
    });
  } else if (status === 'error') {
    logEvent('error', 'mcp', 'job.error', `Job ${job.id} en échec : ${job.error || 'inconnu'}`, {
      jobId: job.id,
      model: job.model,
      attemptedModels: job.attemptedModels ?? [],
      refusals: job.refusals ?? [],
      planB: Boolean(job.planB),
    });
  } else if (status === 'cancelled') {
    logEvent('warn', 'mcp', 'job.cancelled', `Job ${job.id} annulé.`, { jobId: job.id, model: job.model });
  } else if (status === 'needs_approval') {
    logEvent('info', 'mcp', 'job.needs_approval', `Job ${job.id} en attente de confirmation utilisateur (${job.model}).`, { jobId: job.id, model: job.model });
  }
  const stored = {
    ...job,
    response: String(job.response ?? '').slice(0, 120_000),
    summary: String(job.summary ?? '').slice(0, 400),
    thoughts: (job.thoughts || []).slice(-12),
  };
  if (job.workspaceRoot) st.lastWorkspaceRoot = resolve(job.workspaceRoot);
  st.history = [stored, ...(st.history || []).filter((h) => String(h?.jobId || h?.id || '') !== String(job.id))].slice(0, HISTORY_LIMIT);
  saveState(st);
  notify();
  pump();
}

async function runJob(job) {
  const ac = new AbortController();
  running.set(job.id, ac);
  job.status = 'waiting';
  notify();

  let thoughtBuf = '';
  const flushThought = () => {
    const t = thoughtBuf.trim();
    if (t) { job.thoughts.push(t); job.thoughts = job.thoughts.slice(-20); }
    thoughtBuf = '';
  };

  try {
    const st = profileState(loadState(), job.routingProfile);
    const q = await fetchQuotaSnapshot(st);
    const { ids, agentMap, metaMap } = await fetchAgentsAndModels(st);
    const effectiveEffort = normalizeReasoningEffort(job.reasoningEffort || st.reasoningEffort, 'deep');
    st.reasoningEffort = effectiveEffort;
    const preUseTools = Boolean(st.useClientTools !== false && job.workspaceRoot && !job.planB && job.executionMode !== 'patch');
    const hasImages = Boolean(job.attachScreenshot || job.imagePaths?.length || job.liveComputerMode);
    const route = pickModel(job.rawTask ?? job.task, ids, st, job.explicitModel, {
      approvedModel: job.approveModel || null,
      metaMap,
      workspaceRoot: job.workspaceRoot || '',
      useClientTools: preUseTools,
      hasImages,
      webSearchMode: job.webSearchMode ?? null,
      canvasMode: job.canvasMode ?? null,
      imageGenMode: job.imageGenMode ?? null,
      quotaRemainingPct: q?.remainingPct ?? null,
      cutoffPercent: st.cutoffPercent ?? 15,
    });
    if (route.quotaBypass) {
      job.thoughts.push(`Budget chat (${q?.remainingPct?.toFixed?.(0) ?? 0} %) : bascule hors-quota sur ${route.model}.`);
      notify();
    }
    job.model = route.model;
    job.routeReason = route.reason;
    job.routingProfile = route.profile === 'auto' ? null : route.profile;
    job.executionMode = job.executionMode ?? route.executionMode ?? 'generate';
    job.permissionMode = job.permissionMode ?? route.permissionMode ?? undefined;
    job.reasoningEffort = route.reasoningEffort ?? effectiveEffort;
    job.triggeredCapabilities = {
      intent: route.intent || 'general',
      specialty: route.specialty || null,
      webSearch: Boolean(route.enableWebSearch),
      canvas: Boolean(route.enableCanvas),
      imageGen: Boolean(route.enableImageGen),
    };
    job.contextMode = contextModeOf(job, st);

    if (route.needsApproval) {
      Object.assign(job, buildApprovalFields(route, st));
      job.approvalPrompt = job.rawTask ?? job.task;
      job.summary = job.approvalMessage;
      finishJob(job, 'needs_approval');
      return;
    }
    notify();

    logEvent('info', 'mcp', 'job.start', `Démarrage job ${job.id} (${route.model}, ${route.reason}).`, {
      jobId: job.id,
      model: route.model,
      profile: job.routingProfile || 'auto',
      planB: Boolean(job.planB),
      parts: job.parts?.length || 0,
    });

    job.thoughts.push('En attente d’un créneau France Student partagé.');
    notify();
    const lease = await acquireGlobalSlot(st, `mcp-${job.id}`, ac.signal);
    job.slot = lease.index + 1;
    job.status = 'running';
    job.startedAt = Date.now();
    notify();

    const makeStreamCallbacks = (labelPrefix = '') => {
      let lastPush = 0;
      let liveText = '';
      return {
        onChatId: (cid) => { job.chatId = cid; },
        onText: (d) => {
          job.receivedChars += d.length;
          liveText += d;
          job.summary = `${labelPrefix}${liveText.slice(-300)}`;
          if (Date.now() - lastPush > 600) { lastPush = Date.now(); notify(); }
        },
        onReasoning: (d) => {
          thoughtBuf += d;
          if (thoughtBuf.includes('\n') || thoughtBuf.length > 240) { flushThought(); notify(); }
        },
        onRetry: ({ from, to, reason, attempt }) => {
          job.refusals = [...(job.refusals ?? []), { model: from, reason, retryModel: to }];
          job.model = to;
          job.routeReason = `${route.reason} · repli ${from} → ${to}`;
          job.thoughts.push(`Repli (${attempt}/3) : ${from} → ${to}.`);
          job.thoughts = job.thoughts.slice(-20);
          notify();
        },
      };
    };

    try {
      // Plan B par morceaux : chaque petit bloc est généré indépendamment sur un chat neuf.
      if (Array.isArray(job.parts) && job.parts.length) {
        job.planB = true;
        const partsResponses = [];
        const allAttempted = new Set();
        const combinedUsage = { input_tokens: 0, output_tokens: 0, total_tokens: 0 };
        let hasUsage = false;
        let lastError = null;

        for (let i = 0; i < job.parts.length; i++) {
          if (ac.signal.aborted) break;
          const partText = String(job.parts[i] ?? '').trim();
          if (!partText) continue;
          const { prompt: partPrompt } = buildNaturalPrompt(partText, job, st, false);
          job.inputEstimateTokens = (job.inputEstimateTokens ?? 0) + estimateTokens(partPrompt);
          job.thoughts.push(`Plan B · bloc ${i + 1}/${job.parts.length}…`);
          notify();
          try {
            const cbs = makeStreamCallbacks(`[Bloc ${i + 1}/${job.parts.length}] `);
            const partRes = await streamGenerationWithRefusalFallback({
              state: st,
              model: route.model,
              approvedModel: job.approveModel || null,
              availableModels: ids,
              quotaBypass: Boolean(route.quotaBypass),
              quotaRemainingPct: q?.remainingPct ?? null,
              agentMap,
              instructions: PLAN_B_PART_INSTRUCTIONS,
              prompt: partPrompt,
              chatId: undefined,
              signal: ac.signal,
              ...cbs,
            });
            for (const m of partRes.attemptedModels || []) allAttempted.add(m);
            job.model = partRes.model ?? job.model;
            const cleanText = String(partRes.text ?? '').trim();
            partsResponses.push({ index: i + 1, task: partText, model: partRes.model, response: cleanText });
            if (partRes.usage) {
              hasUsage = true;
              const u = normalizeProviderTokenUsage(partRes.usage);
              if (u?.inputTokens) combinedUsage.input_tokens += u.inputTokens;
              if (u?.outputTokens) combinedUsage.output_tokens += u.outputTokens;
              if (u?.totalTokens) combinedUsage.total_tokens += u.totalTokens;
            }
            logEvent('info', 'mcp', 'planB.part.done', `Bloc ${i + 1}/${job.parts.length} généré (${partRes.model}).`, { jobId: job.id, part: i + 1, model: partRes.model });
          } catch (partErr) {
            lastError = partErr;
            partsResponses.push({ index: i + 1, task: partText, model: job.model, error: partErr?.message || 'Échec du bloc' });
            logEvent('warn', 'mcp', 'planB.part.error', `Bloc ${i + 1}/${job.parts.length} échoué : ${partErr?.message}`, { jobId: job.id, part: i + 1 });
          }
        }
        flushThought();

        const succeeded = partsResponses.filter((p) => p.response);
        if (!succeeded.length) {
          const err = new Error(`Plan B en échec sur tous les blocs : ${lastError?.message || 'aucune réponse'}`);
          err.attemptedModels = [...allAttempted];
          throw err;
        }
        job.partsResponses = partsResponses;
        job.attemptedModels = [...allAttempted];
        job.response = partsResponses
          .map((p) => (p.response ? `### Partie ${p.index} — ${p.task}\n${p.response}` : `### Partie ${p.index} (échec : ${p.error})`))
          .join('\n\n')
          .slice(0, 160_000);
        job.summary = `${succeeded.length}/${partsResponses.length} bloc(s) généré(s) en Plan B.`;
        job.usage = hasUsage ? combinedUsage : null;
        job.tokenMeasurement = { localPromptEstimateTokens: job.inputEstimateTokens, providerUsage: hasUsage ? normalizeProviderTokenUsage(combinedUsage) : null };
        finishJob(job, 'done');
        return;
      }

      // Plan A (session persistante) ou Plan B isolé (chat neuf sans historique)
      const effectivePerm = PERMISSION_MODES.includes(job.permissionMode)
        ? job.permissionMode
        : (PERMISSION_MODES.includes(st.permissionMode) ? st.permissionMode : 'acceptEdits');
      job.permissionMode = effectivePerm;
      const useTools = Boolean(
        modelSupportsClientTools(route.model) &&
        st.useClientTools !== false &&
        job.workspaceRoot &&
        !job.planB &&
        job.executionMode !== 'patch' &&
        !route.enableCanvas
      );
      const clientTools = useTools ? toolsForPermissionMode(effectivePerm, job.executionMode) : [];
      const runner = clientTools.length
        ? createWorkspaceToolRunner({
          workspaceRoot: job.workspaceRoot,
          permissionMode: effectivePerm,
          jobId: job.id,
          onMutation: (list) => { job.filesWritten = list; notify(); },
          onCommand: (list) => { job.commandsRun = list; notify(); },
        })
        : null;

      const key = directSessionKey(job);
      // Reuse the persistent France Student discussion whenever the same model is used for this scope (including standalone/plan/Ministral chats), while keeping distinct models in separate discussions.
      const usePersistentChat = !job.planB && job.executionMode !== 'dry-run' && st.reuseChat !== false;
      const chatId = usePersistentChat ? await prepareDirectChat(st, key, route.model) : undefined;
      if (chatId) job.chatId = chatId;

      // Boucle autonome & économe Computer Use pilotée par France Student (1 seule discussion, JPEG 1280px + viseur rouge clic)
      if (job.liveComputerMode && effectivePerm !== 'plan' && !job.planB) {
        const screenIdx = Number.isInteger(job.screenIndex) ? job.screenIndex : (Number(st.selectedScreenIndex ?? 0) || 0);
        const loopRes = await runAutonomousComputerLoop({
          state: st,
          goal: route.prompt,
          model: route.model,
          chatId,
          screenIndex: screenIdx,
          maxSteps: job.maxComputerSteps || 6,
          permissionMode: effectivePerm,
          reasoningEffort: job.reasoningEffort || 'standard',
          signal: ac.signal,
          executeComputerAction,
          streamTurn: (req) => streamGenerationWithRefusalFallback({
            state: st,
            model: req.model,
            approvedModel: job.approveModel || null,
            availableModels: ids,
            quotaBypass: Boolean(route.quotaBypass),
            quotaRemainingPct: q?.remainingPct ?? null,
            agentMap,
            instructions: req.instructions,
            prompt: req.prompt,
            fileIds: req.fileIds,
            chatId: req.chatId,
            enableWebSearch: false,
            signal: req.signal,
            ...makeStreamCallbacks('[Computer Use] '),
          }),
          onThought: (t) => { job.thoughts.push(t); job.thoughts = job.thoughts.slice(-20); notify(); },
          onStep: (s) => {
            job.computerSteps = [...(job.computerSteps || []), s];
            job.toolTurns = job.computerSteps.length;
            notify();
          },
        });
        flushThought();
        job.chatId = loopRes.chatId ?? job.chatId;
        job.model = loopRes.model ?? job.model;
        if (usePersistentChat && loopRes.chatId) rememberDirectChat(key, loopRes.chatId, job.model || route.model);
        job.computerSteps = loopRes.steps;
        job.toolTurns = loopRes.stepsCount;
        job.response = String(loopRes.text || '').slice(0, 160_000);
        job.summary = job.response.slice(0, 400);
        finishJob(job, 'done');
        return;
      }

      const local = buildNaturalPrompt(route.prompt, job, st, Boolean(chatId), { useTools: clientTools.length > 0 });
      job.contextMode = local.mode;
      job.inputEstimateTokens = estimateTokens(local.prompt);
      const fallbackBaseInstructions = route.enableCanvas
        ? CANVAS_INSTRUCTIONS
        : (job.executionMode === 'plan'
          ? PLAN_INSTRUCTIONS
          : (job.executionMode === 'patch'
            ? PATCH_INSTRUCTIONS
            : (job.planB ? PLAN_B_PART_INSTRUCTIONS : NATURAL_INSTRUCTIONS)));
      const baseInstructions = clientTools.length ? CLIENT_TOOLS_INSTRUCTIONS : fallbackBaseInstructions;
      let visionFileIds = [];
      if (job.attachScreenshot || job.imagePaths?.length) {
        const vision = await prepareVisionAttachments({
          job,
          state: st,
          permissionMode: effectivePerm,
          executeComputerAction,
          onThought: (t) => { job.thoughts.push(t); job.thoughts = job.thoughts.slice(-20); notify(); },
        });
        visionFileIds = vision.fileIds;
        job.attachedFiles = vision.fileIds.length;
      }
      const visionLive = Boolean(job.liveComputerMode || visionFileIds.length);
      const instrSuffix = (visionLive ? VISION_LIVE_INSTRUCTIONS : '') + buildReasoningDirective(job.reasoningEffort, job.executionMode);
      const instructions = baseInstructions + instrSuffix;
      const fallbackInstructions = fallbackBaseInstructions + instrSuffix;

      let result;
      try {
        result = await streamGenerationWithRefusalFallback({
          state: st,
          model: route.model,
          approvedModel: job.approveModel || null,
          availableModels: ids,
          quotaBypass: Boolean(route.quotaBypass),
          quotaRemainingPct: q?.remainingPct ?? null,
          agentMap,
          instructions,
          fallbackInstructions,
          prompt: local.prompt,
   fileIds: visionFileIds,
          chatId,
          clientTools,
          enableWebSearch: Boolean(route.enableWebSearch),
          enableCanvas: Boolean(route.enableCanvas),
          enableImageGen: route.enableImageGen ? true : undefined,
          executeToolCall: runner ? (call) => runner.execute(call) : null,
          maxToolTurns: 18,
          onToolCall: ({ turn, name }) => {
            job.thoughts.push(`Outil #${turn} : ${name}`);
            job.thoughts = job.thoughts.slice(-20);
            notify();
          },
          onWebSearch: ({ status, queries, sourcesCount }) => {
            const label = queries?.length ? `Recherche Web : ${queries[0].slice(0, 60)}` : `Recherche Web (${status || 'actif'})`;
            job.thoughts.push(sourcesCount ? `${label} · ${sourcesCount} sources` : label);
            job.thoughts = job.thoughts.slice(-20);
            notify();
          },
          onImageGenerated: (img) => {
            job.thoughts.push(`Image IA générée (${img.status || 'ok'})`);
            job.thoughts = job.thoughts.slice(-20);
            notify();
          },
          onCanvasProgress: (chars) => {
            job.receivedChars += chars;
            job.summary = `Génération Canvas en cours (${job.receivedChars} car.)…`;
          },
          onCanvasDocument: (doc) => {
            job.thoughts.push(`Canvas prêt : ${doc.title} (${doc.kind}, ${doc.content.length} car.)`);
            job.thoughts = job.thoughts.slice(-20);
            notify();
          },
          signal: ac.signal,
          ...makeStreamCallbacks(),
        });
      } catch (err) {
        const status = Number(err?.status ?? 0);
        const canIsolatePlanB = !err?.noPlanB && !ac.signal.aborted && status !== 401 && status !== 403 && Boolean(chatId) && !job.planB;
        if (!canIsolatePlanB) throw err;
        // Repli Plan B automatique immédiat sur chat neuf isolé (sans outils client si ceux-ci ont bloqué)
        job.planB = true;
        job.thoughts.push('Plan B automatique : nouvel essai sur session neuve isolée…');
        notify();
        logEvent('warn', 'mcp', 'planB.auto_isolated', `Plan A échoué avec chat persistant ; bascule Plan B sur chat neuf (${err?.message}).`, { jobId: job.id });
        const freshPrompt = buildNaturalPrompt(route.prompt, job, st, false, { useTools: false }).prompt;
        result = await streamGenerationWithRefusalFallback({
          state: st,
          model: route.model,
          approvedModel: job.approveModel || null,
          availableModels: ids,
          quotaBypass: Boolean(route.quotaBypass),
          quotaRemainingPct: q?.remainingPct ?? null,
          agentMap,
          instructions: route.enableCanvas ? CANVAS_INSTRUCTIONS : PLAN_B_PART_INSTRUCTIONS,
          prompt: freshPrompt,
   fileIds: visionFileIds,
          chatId: undefined,
          clientTools: [],
          enableWebSearch: Boolean(route.enableWebSearch),
          enableCanvas: Boolean(route.enableCanvas),
          executeToolCall: null,
          signal: ac.signal,
          ...makeStreamCallbacks('[Plan B] '),
        });
      }

      flushThought();
      job.chatId = result.chatId ?? job.chatId;
      job.model = result.model ?? job.model;
      if (usePersistentChat && result.chatId) rememberDirectChat(key, result.chatId, job.model || route.model);
      job.refusals = result.refusals ?? job.refusals ?? [];
      job.attemptedModels = result.attemptedModels ?? job.attemptedModels;
      job.toolTurns = result.toolTurns ?? 0;
      job.webSearchSources = result.webSearchSources ?? [];
      job.generatedImages = result.generatedImages ?? [];
      job.canvasDocuments = result.canvasDocuments ?? [];
      if (runner) {
        job.filesWritten = runner.getFilesWritten();
        job.commandsRun = runner.getCommandsRun();
      }
      job.tokenMeasurement = result.tokenMeasurement ?? {
        localPromptEstimateTokens: job.inputEstimateTokens,
        providerUsage: normalizeProviderTokenUsage(result.usage),
      };
      job.response = String(result.text ?? '').trim().slice(0, 160_000);
      // Auto-publish Canvas HTML/SVG/Mermaid documents (native createDocument or fenced code) to Live Canvas!
      if ((route.enableCanvas || job.canvasDocuments.length) && job.response) {
        const nativeVisual = job.canvasDocuments.find((d) => ['html', 'svg', 'mermaid'].includes(String(d.kind || '').toLowerCase()));
        const htmlMatch = !nativeVisual ? /```html\s*\n([\s\S]+?)```/i.exec(job.response) : null;
        const svgMatch = (!nativeVisual && !htmlMatch) ? /```svg\s*\n([\s\S]+?)```/i.exec(job.response) : null;
        const mmdMatch = (!nativeVisual && !htmlMatch && !svgMatch) ? /```mermaid\s*\n([\s\S]+?)```/i.exec(job.response) : null;
        const artContent = nativeVisual?.content || htmlMatch?.[1] || svgMatch?.[1] || mmdMatch?.[1] || null;
        const artType = nativeVisual ? String(nativeVisual.kind).toLowerCase() : (htmlMatch ? 'html' : (svgMatch ? 'svg' : (mmdMatch ? 'mermaid' : null)));
        if (artContent && artType) {
          try {
            const cv = await executeCanvasAction({
              action: 'create',
              id: `artifact-${job.id}`,
              title: (nativeVisual?.title || job.title.replace(/^\[ATG\]\s*/, '')).slice(0, 60),
              type: artType,
              content: artContent.trim(),
            });
            if (cv?.previewUrl || cv?.url) {
              job.canvasPreview = { id: cv.id, type: artType, url: cv.previewUrl || cv.url, title: nativeVisual?.title || '' };
            }
          } catch { /* non-blocking live preview */ }
        }
      }
      job.summary = job.response.slice(0, 400);
      job.usage = result.usage;
      finishJob(job, 'done');
    } finally {
      lease.release();
    }
  } catch (e) {
    flushThought();
    if (ac.signal.aborted) {
      finishJob(job, 'cancelled');
    } else {
      const status = Number(e?.status ?? 0);
      if (!e?.noPlanB && status !== 401 && status !== 403) {
        job.planBRecommended = true;
        job.planBHint = 'Relance en Plan B par petits blocs avec fs_delegate({ task, planB: true, parts: [...] }) ou complète directement les morceaux.';
      }
      job.refusals = e.refusals ?? job.refusals ?? [];
      job.attemptedModels = e.attemptedModels ?? job.attemptedModels;
      if (e?.blockedModel) {
        job.model = e.model || job.explicitModel || job.model;
        job.blockedModel = true;
        job.planBRecommended = false;
 delete job.planBHint;
      }
      job.error = e.message;
      finishJob(job, 'error');
    }
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

const asCompactText = (o) => ({ content: [{ type: 'text', text: JSON.stringify(o) }] });
const server = new McpServer({ name: 'fs-atg', version: CONNECTOR_VERSION });

server.registerTool('fs_delegate', {
  title: 'Déléguer à France Student [ATG]',
  description: 'Délègue à France Student : avec workspaceRoot (mode acceptEdits/full), France Student lit/écrit/teste directement sur le PC avec sauvegarde /undo et renvoie un bilan compact (filesWritten, commandsRun). Adapte toujours le profil et le modèle à la spécialité (même en MAX, jamais Opus H24) : recherche/exploration/Q&A/doc courte → routingProfile="eco" + reasoningEffort="standard" (GPT Luna / Haiku) ; code standard/backend/tests → "auto" + "deep" (GPT Sol) ; UI/Frontend/Canvas → "auto" + "deep" (Claude Sonnet) ; Vision Live → GPT Terra ; algo difficile → GPT Astra ; architecture multi-fichiers/sécurité critique → "quality" + "max" (Opus réservé).',
  inputSchema: {
    task: z.string().min(3).describe('Consigne claire en phrases normales (commandes possibles : /send, /plan, /patch, /mode, /eco, /fast, /low, /medium, /high, /opus, /sonnet, /gpt, /astra).'),
    workspaceRoot: z.string().optional().describe('Chemin absolu du workspace : active les outils locaux selon permissionMode (lecture/écriture/commandes bornées).'),
    files: z.array(z.string()).optional().describe('Chemins relatifs à joindre si besoin (hors mode outils autonomes).'),
    model: z.string().optional(),
    approveModel: z.string().optional().describe('ID exact d’un modèle réglé sur « À demander », après confirmation explicite de l’utilisateur.'),
    priority: z.number().int().min(0).max(9).optional(),
    contextMode: z.enum(['minimal', 'targeted', 'smart']).optional().describe('minimal recommandé = phrase seule sans surcharger.'),
    parentJobId: z.string().optional().describe('Suite d’un job précédent.'),
    executionMode: z.enum(['generate', 'apply', 'plan', 'patch', 'dry-run']).optional().describe('apply/generate autorisent les outils selon permissionMode ; plan/patch/dry-run restent en lecture seule.'),
    permissionMode: z.enum(['plan', 'acceptEdits', 'full']).optional().describe('Surcharge ponctuelle : plan (lecture seule), acceptEdits (lecture + écriture/édition avec /undo), full (+ run_command sécurisé).'),
    reasoningEffort: z.enum(['standard', 'deep', 'max']).optional().describe('standard pour recherche/texte simple/question/résumé/doc courte (jamais Opus), deep pour code courant/UI/backend, max uniquement pour algo complexe ou architecture/sécurité critique.'),
    routingProfile: z.enum(['auto', 'eco', 'fast', 'quality', 'security', 'refactor']).optional().describe('eco/fast pour recherche/texte simple/tâche légère, auto par défaut (routage par spécialité Claude vs GPT + économie Opus), quality/security/refactor pour chantiers lourds.'),
    webSearch: z.enum(['auto', 'on', 'off']).optional().describe('Déclencheur Recherche Web natif France Student : auto (défaut intelligent), on (forcé), off (désactivé).'),
    canvas: z.enum(['auto', 'on', 'off']).optional().describe('Déclencheur Canvas / Artefacts natif France Student (documentToolsEnabled + aperçu live) : auto (défaut intelligent), on, off.'),
    imageGen: z.enum(['auto', 'on', 'off']).optional().describe('Déclencheur génération d’images IA native France Student : auto, on, off.'),
    planB: z.boolean().optional().describe('Active le Plan B (chat neuf isolé ou découpage via parts) en cas d’échec du Plan A.'),
    parts: z.array(z.string().min(3)).max(6).optional().describe('Plan B : 2 à 6 petites sous-demandes simples générées séparément.'),
    waitSeconds: z.number().min(0).max(130).optional().describe('Attente intégrée (défaut 110s, max 130s pour rester sous le timeout transport MCP de 180s) ; si running, enchaîne avec fs_job_status.'),
    verbose: z.boolean().optional(),
    attachScreenshot: z.boolean().optional().describe('Boucle Vision Live : capture l’écran Windows et la joint au modèle (upload /filesv2).'),
    screenIndex: z.number().int().min(0).optional().describe('Écran à capturer (0 = premier écran, voir fs_computer screen_info). Défaut : écran choisi dans le panneau.'),
    imagePaths: z.array(z.string()).max(6).optional().describe('Images locales (png/jpg/webp/gif) à joindre au modèle, chemins absolus ou relatifs au workspace.'),
    liveComputerMode: z.boolean().optional().describe('Active la boucle Computer Use autonome pilotée par France Student (JPEG 1280px + viseur rouge de clic, 1 seule discussion).'),
    maxComputerSteps: z.number().int().min(1).max(12).optional().describe('Nombre maximal d’étapes pour la boucle Computer Use autonome (défaut 6, max 12).'),
  },
}, async (a) => {
  const delegation = normalizedDelegation(a.task, a.executionMode, a.routingProfile, a.permissionMode, a.reasoningEffort);
  if (!delegation.task && !a.parts?.length) throw new Error('Consigne vide après les commandes locales.');
  const stNow = loadState();
  const effPerm = delegation.permissionMode || (PERMISSION_MODES.includes(stNow.permissionMode) ? stNow.permissionMode : 'acceptEdits');
  const writeRequested = stNow.useClientTools !== false
    && !['plan', 'patch', 'dry-run'].includes(delegation.executionMode)
    && (effPerm === 'acceptEdits' || effPerm === 'full');
  const target = resolveMcpWorkspace(a.workspaceRoot, stNow, { requireForWrite: writeRequested });
  if (target.explicit) {
    // Stockage explicite : le chemin devient le workspace confirmé (partagé avec le panneau via /api/state).
    if (stNow.lastWorkspaceRoot !== target.root || stNow.workspaceRootConfirmed !== true) {
      stNow.lastWorkspaceRoot = target.root;
      stNow.workspaceRootConfirmed = true;
      stNow.workspaceRootSource = 'mcp';
      stNow.workspaceRootConfirmedAt = Date.now();
      saveState(stNow);
    }
  }
  const waitSec = typeof a.waitSeconds === 'number' ? Math.max(0, Math.min(130, a.waitSeconds)) : 110;
  const firstLine = (delegation.task || a.parts?.[0] || '').split('\n')[0];
  const job = {
    id: randomUUID().slice(0, 8),
    title: `${ATG} ${firstLine.slice(0, 70)}${firstLine.length > 70 ? '…' : ''}`,
    task: delegation.task || a.parts?.join(' ; '),
    rawTask: delegation.rawTask,
    workspaceRoot: target.root || undefined,
    files: a.files ?? [],
    explicitModel: a.model,
    approveModel: a.approveModel || undefined,
    parentId: a.parentJobId,
    priority: a.priority ?? 5,
    contextMode: a.contextMode,
    executionMode: delegation.executionMode,
    permissionMode: delegation.permissionMode,
    reasoningEffort: delegation.reasoningEffort,
    routingProfile: delegation.routingProfile,
    webSearchMode: a.webSearch ?? undefined,
    canvasMode: a.canvas ?? undefined,
    imageGenMode: a.imageGen ?? undefined,
    planB: Boolean(a.planB || a.parts?.length),
    parts: Array.isArray(a.parts) && a.parts.length ? a.parts.slice(0, 6) : undefined,
    attachScreenshot: Boolean(a.attachScreenshot),
    screenIndex: Number.isInteger(a.screenIndex) ? a.screenIndex : undefined,
    imagePaths: Array.isArray(a.imagePaths) ? a.imagePaths.filter(Boolean).slice(0, 6) : [],
    liveComputerMode: typeof a.liveComputerMode === 'boolean' ? a.liveComputerMode : Boolean(loadState().liveComputerMode && a.attachScreenshot),
    maxComputerSteps: Number.isInteger(a.maxComputerSteps) ? a.maxComputerSteps : undefined,
    waitedInline: waitSec > 0,
    status: 'queued',
    createdAt: Date.now(),
    receivedChars: 0,
    thoughts: [],
    response: '',
    summary: '',
    written: [],
  };
  jobs.set(job.id, job);
  queue.push(job);
  queue.sort((x, y) => y.priority - x.priority || x.createdAt - y.createdAt);
  logEvent('info', 'mcp', 'job.queued', `Job ${job.id} mis en file (${job.planB ? 'Plan B' : 'Plan A'}).`, { jobId: job.id });
  notify();
  pump();
  if (waitSec > 0) {
    const doneJob = await waitFor(job.id, waitSec * 1000);
    if (doneJob && FINAL.has(doneJob.status)) {
      return asCompactText(mcpCompactView(doneJob, Boolean(a.verbose)));
    }
    return asCompactText({
      jobId: job.id,
      status: doneJob?.status ?? job.status,
      next: `Job encore en cours après ${waitSec}s : appelle fs_job_status(waitSeconds=120).`,
    });
  }
  return asCompactText({ jobId: job.id, status: job.status, next: 'Appelle fs_job_status avec waitSeconds=120.' });
});

server.registerTool('fs_job_status', {
  title: "État d'un job [ATG]",
  description: "Attend jusqu'à waitSeconds que le job finisse, puis renvoie le texte/code généré dans response.",
  inputSchema: { jobId: z.string(), waitSeconds: z.number().min(0).max(180).optional(), verbose: z.boolean().optional() },
}, async ({ jobId, waitSeconds, verbose }) => {
  const job = await waitFor(jobId, Math.max(0, Math.min(180, waitSeconds ?? 120)) * 1000);
  return asCompactText(job ? mcpCompactView(job, Boolean(verbose)) : { error: 'jobId inconnu' });
});

server.registerTool('fs_logs', {
  title: 'Journal de bord [ATG]',
  description: 'Consulte le journal de bord persistant de France Student [ATG] pour diagnostiquer les erreurs, refus de modèles, timeouts ou problèmes API.',
  inputSchema: {
    limit: z.number().int().min(1).max(100).optional().describe('Nombre maximal d’entrées récentes (défaut 40).'),
    level: z.enum(['all', 'warn', 'error']).optional().describe('Filtre de gravité (défaut all).'),
    jobId: z.string().optional().describe('Filtrer sur un jobId précis.'),
  },
}, async ({ limit, level, jobId }) => {
  const entries = readLogs({ limit: limit ?? 40, level: level ?? 'all', jobId: jobId ?? '' });
  return asCompactText({
    logPath: LOG_PATH,
    count: entries.length,
    recentErrors24h: countRecentErrors(),
    entries,
  });
});

server.registerTool('fs_session', {
  title: 'Session France Student',
  description: 'Affiche, compacte ou renouvelle la session persistante. /compact et /new dans le chat Antigravity doivent appeler cet outil.',
  inputSchema: {
    action: z.enum(['status', 'compact', 'new']).default('status'),
    workspaceRoot: z.string().optional(),
  },
}, async ({ action, workspaceRoot }) => asCompactText(await sessionAction(action, workspaceRoot)));

server.registerTool('fs_status', {
  title: 'État du connecteur France Student [ATG]',
  description: 'Retourne sans appel fournisseur le transport MCP, la version, le mode Auto/Manuel et les outils publiés.',
  inputSchema: {},
}, async () => {
  const st = loadState();
  return asCompactText({
    mcp: { ...connectorSnapshot(st), activeJobs: jobs.size, queuedJobs: queue.length },
    recentErrors24h: countRecentErrors(),
    antigravityQuota: ANTIGRAVITY_QUOTA_VISIBILITY,
  });
});

server.registerTool('fs_diagnostics', {
  title: 'Diagnostic France Student [ATG]',
  description: 'Vérifie de façon non destructive le trousseau, l’API, les modèles, le quota, les erreurs récentes et la session. Ne révèle jamais la clé.',
  inputSchema: {},
}, async () => asCompactText(await diagnosticsSnapshot()));

server.registerTool('fs_quota', {
  title: 'Quota France Student',
  description: 'Pourcentage de quota restant et seuil du coupe-circuit.',
  inputSchema: {},
}, async () => {
  const st = loadState();
  const credential = await apiCredentialStatus(st);
  if (!credential.configured) {
    return asCompactText({ remainingPct: null, cutoff: st.cutoffPercent, resetAt: null, credential, error: 'Aucun identifiant France Student configuré.' });
  }
  try {
    const q = await fetchQuotaSnapshot(st);
    return asCompactText({ remainingPct: q.remainingPct, cutoff: st.cutoffPercent, resetAt: q.resetAt, credential });
  } catch (error) {
    logEvent('error', 'mcp', 'quota.error', error?.message || 'Erreur quota');
    return asCompactText({ remainingPct: null, cutoff: st.cutoffPercent, resetAt: null, credential, error: String(error?.message ?? error).slice(0, 220) });
  }
});

server.registerTool('fs_catalog', {
  title: 'Catalogue France Student',
  description: 'Découvre seulement par GET /models, /metrics/chat-streams, /skills, /projects, /mcp/servers ou /filesv2.',
  inputSchema: {
    resource: z.enum(['all', 'models', 'streams', 'capacity', 'skills', 'projects', 'mcpServers', 'files']).default('all'),
  },
}, async ({ resource }) => {
  const st = loadState();
  if (resource === 'models') {
    const [models, agents] = await Promise.all([fetchModelCatalog(st), fetchAgentsAndModels(st).catch(() => null)]);
    const enriched = agents?.ids?.length ? enrichModelsCatalog(agents.ids, agents.metaMap, st, models?.models) : [];
    return asCompactText(compactCatalog({ ...models, enriched }));
  }
  if (resource === 'streams' || resource === 'capacity') return asCompactText(compactCatalog(await fetchChatStreamMetrics(st)));
  if (resource === 'all') {
    const [models, streams, remote] = await Promise.all([
      fetchModelCatalog(st),
      fetchChatStreamMetrics(st),
      fetchRemoteCatalog(st, 'all'),
    ]);
    return asCompactText(compactCatalog({ models, streams, ...remote }));
  }
  return asCompactText(compactCatalog(await fetchRemoteCatalog(st, resource)));
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
  return asCompactText({ ok: true });
});

server.registerTool('fs_undo', {
  title: 'Annuler une modification (/undo) [ATG]',
  description: 'Liste les dernières sauvegardes ou annule atomiquement la dernière modification écrite par France Student dans le workspace.',
  inputSchema: {
    workspaceRoot: z.string().optional().describe('Chemin du workspace (défaut : dernier workspace utilisé).'),
    action: z.enum(['undo', 'list']).default('undo'),
    undoId: z.string().optional().describe('ID précis d’une entrée à restaurer (optionnel).'),
  },
}, async ({ workspaceRoot, action, undoId }) => {
  const ws = workspaceRoot || loadState().lastWorkspaceRoot;
  if (!ws) return asCompactText({ ok: false, error: 'workspaceRoot requis pour /undo.' });
  if (action === 'list') {
    const entries = listUndoEntries(ws, 15);
    return asCompactText({ ok: true, workspaceRoot: ws, count: entries.length, entries });
  }
  const res = await undoLastChange(ws, undoId || null);
  if (res.ok) {
    logEvent('info', 'mcp', 'undo.applied', `Restauré ${res.undone.rel} (${res.undone.action}).`, res.undone);
  }
  return asCompactText(res);
});

server.registerTool('fs_memory', {
  title: 'Mémoire projet & pré-prompt [ATG]',
  description: 'Gère la mémoire persistante par projet (/remember, /memory), les faits appris et le pré-prompt injectés au 1er tour.',
  inputSchema: {
    workspaceRoot: z.string().optional().describe('Chemin du workspace (sinon dernier workspace ou profil global).'),
    action: z.enum(['get', 'remember', 'forget', 'clear', 'preprompt', 'learn']).default('get'),
    text: z.string().optional().describe('Texte de la note, du fait ou du pré-prompt.'),
    id: z.string().optional().describe('ID ou index (1..) à supprimer pour forget.'),
  },
}, async ({ workspaceRoot, action, text, id }) => {
  const ws = workspaceRoot || loadState().lastWorkspaceRoot || '';
  switch (action) {
    case 'remember': {
      const r = addProjectNote(ws, text || '');
      logEvent('info', 'mcp', 'memory.remember', `Note mémorisée (${r.total} au total).`);
      return asCompactText({ ok: true, ...r });
    }
    case 'learn': {
      const saved = learnFact(ws, text || '');
      return asCompactText({ ok: true, fact: saved });
    }
    case 'forget': {
      const target = id || text || '';
      const r = removeProjectNote(ws, target);
      if (r.removed) return asCompactText({ ok: true, kind: 'note', ...r });
      const rf = forgetFact(ws, target);
      return asCompactText({ ok: true, kind: 'fact', ...rf });
    }
    case 'clear':
      return asCompactText({ ok: true, ...clearProjectMemory(ws) });
    case 'preprompt':
      return asCompactText({ ok: true, ...setPreprompt(ws, text ?? '') });
    case 'get':
    default:
      return asCompactText({ ok: true, workspaceRoot: ws || null, ...getProjectMemory(ws) });
  }
});

server.registerTool('fs_computer', {
  title: 'Computer Use — Écran, UIA, Fenêtres, Souris & Clavier [ATG]',
  description: 'Contrôle du bureau Windows (vrai écran de l’utilisateur, multi-écrans) : arbre UI Automation lecture seule (ui_tree, liste les contrôles avec coordonnées exactes) et clic UIA ciblé (ui_click par name/automationId), capture écran PNG renvoyée comme image (screenshot, avec screenIndex/deviceName), infos écrans/curseur (screen_info), liste et focus des fenêtres (windows, focus_window), clic/déplacement/glisser/molette souris (mouse_move, click, double_click, right_click, drag, scroll), frappe clavier et raccourcis (type, key, hotkey), presse-papiers (clipboard_get, clipboard_set). Ne modifie aucun fichier du workspace ; les actions modifiant le bureau sont bloquées en mode plan (ui_tree reste autorisé).',
  inputSchema: {
    action: z.enum(COMPUTER_ACTIONS).describe('Action Computer Use à exécuter (dont ui_tree et ui_click).'),
    x: z.number().optional().describe('Coordonnée X écran (pixels).'),
    y: z.number().optional().describe('Coordonnée Y écran (pixels).'),
    toX: z.number().optional().describe('Coordonnée X cible pour drag.'),
    toY: z.number().optional().describe('Coordonnée Y cible pour drag.'),
    deltaY: z.number().optional().describe('Défilement molette vertical (ex: -360 vers le bas, 360 vers le haut).'),
    text: z.string().optional().describe('Texte à taper (type) ou à copier dans le presse-papiers (clipboard_set).'),
    key: z.string().optional().describe('Touche ou raccourci (ex: Enter, Escape, Tab, Ctrl+C, Alt+Tab).'),
    keys: z.string().optional().describe('Alias de key pour hotkey.'),
    title: z.string().optional().describe('Filtre titre de fenêtre pour focus_window, ui_tree ou ui_click.'),
    processName: z.string().optional().describe('Nom de processus pour focus_window, ui_tree ou ui_click.'),
    pid: z.number().optional().describe('PID de fenêtre pour focus_window, ui_tree ou ui_click.'),
    name: z.string().optional().describe('Texte ou sous-chaîne du contrôle ciblé pour ui_click.'),
    automationId: z.string().optional().describe('AutomationId UIA du contrôle pour ui_click.'),
    controlType: z.string().optional().describe('Type de contrôle UIA (ex: Button, Edit, TabItem) pour ui_click.'),
    maxElements: z.number().int().min(5).max(120).optional().describe('Nombre maximal de contrôles UIA retournés pour ui_tree (défaut 50, max 120).'),
    useInvoke: z.boolean().optional().describe('Utiliser InvokePattern UIA en priorité avant le clic souris pour ui_click (défaut true).'),
    outputPath: z.string().optional().describe('Chemin PNG de sortie pour screenshot (optionnel).'),
    region: z.object({
      x: z.number(),
      y: z.number(),
      width: z.number(),
      height: z.number(),
    }).optional().describe('Sous-région de capture écran optionnelle (coordonnées absolues du bureau virtuel).'),
    screenIndex: z.number().int().min(0).optional().describe('Écran à capturer pour screenshot (0 = premier écran de screen_info). Défaut : écran choisi dans le panneau.'),
    deviceName: z.string().optional().describe('Nom Windows de l’écran (ex: \\\\.\\DISPLAY2), prioritaire sur screenIndex.'),
    windowTitle: z.string().optional().describe('Capture ciblée d’une fenêtre (screenshot, ocr_screen) ou attente de fenêtre (wait_for).'),
    format: z.enum(['png', 'jpeg', 'jpg']).optional().describe('Format de capture screenshot (jpeg recommandé pour économie de bande passante).'),
    maxWidth: z.number().int().min(0).max(3840).optional().describe('Largeur maximale de redimensionnement pour screenshot (ex: 1280).'),
    quality: z.number().int().min(30).max(100).optional().describe('Qualité JPEG pour screenshot (défaut 78).'),
    marker: z.object({ x: z.number(), y: z.number(), label: z.string().optional() }).optional().describe('Dessine un viseur rouge aux coordonnées absolues (x, y) sur la capture screenshot.'),
    timeoutMs: z.number().int().min(500).max(30000).optional().describe('Délai maximal en ms pour wait_for (défaut 8000).'),
    pollMs: z.number().int().min(200).max(2000).optional().describe('Intervalle de polling en ms pour wait_for (défaut 400).'),
  },
}, async (params) => {
  const st = loadState();
  const effective = { ...params };
  if (effective.action === 'screenshot' && effective.screenIndex === undefined && !effective.deviceName && !effective.region && !effective.windowTitle) {
    effective.screenIndex = Number(st.selectedScreenIndex ?? 0) || 0;
  }
  const res = await executeComputerAction(effective, {
    permissionMode: st.permissionMode || 'acceptEdits',
    computerUseEnabled: st.computerUseEnabled !== false,
  });
  if (effective.action === 'screenshot' && res?.dataUrl) {
    const { dataUrl, ...shotMeta } = res;
    const base64 = String(dataUrl).replace(/^data:[^;]+;base64,/, '');
    return {
      content: [
        { type: 'text', text: JSON.stringify(shotMeta) },
        { type: 'image', data: base64, mimeType: res.mimeType || 'image/png' },
      ],
    };
  }
  if (res && typeof res === 'object' && 'dataUrl' in res) {
    const { dataUrl, ...shotMeta } = res;
    return asCompactText({ ...shotMeta, note: 'Image trop lourde pour être renvoyée inline : utilise le fichier "path".' });
  }
  return asCompactText(res);
});

server.registerTool('fs_canvas', {
  title: 'Canvas Live — Artifacts HTML/SVG/Mermaid (/canvas) [ATG]',
  description: 'Crée, met à jour et prévisualise en direct des artifacts interactifs (HTML/CSS/JS, SVG, Mermaid) servis sur une URL HTTP locale dédiée.',
  inputSchema: {
    action: z.enum(['create', 'update', 'list', 'get', 'clear']).default('create').describe('Action Canvas.'),
    id: z.string().optional().describe('Identifiant ou nom de fichier de l’artifact.'),
    title: z.string().optional().describe('Titre de l’artifact.'),
    type: z.enum(['html', 'svg', 'mermaid']).optional().describe('Format de l’artifact (défaut html).'),
    content: z.string().optional().describe('Code source HTML, SVG ou Mermaid de l’artifact.'),
  },
}, async (params) => {
  const res = await executeCanvasAction(params);
  return asCompactText(res);
});

server.registerTool('fs_thinking', {
  title: 'Raisonnement Approfondi — Sequential Thinking (/think) [ATG]',
  description: 'Moteur de raisonnement structuré pas-à-pas (Sequential Thinking) : décompose un problème complexe en étapes numérotées, hypothèses vérifiées, révisions et branches alternatives avant toute implémentation.',
  inputSchema: {
    action: z.enum(THINKING_ACTIONS).default('think').describe('think (ajouter une étape), history (lire la chaîne), status ou clear.'),
    thought: z.string().optional().describe('Contenu de l’étape de raisonnement actuelle (requis pour action="think").'),
    thoughtNumber: z.number().optional().describe('Numéro de l’étape (1..N).'),
    totalThoughts: z.number().optional().describe('Estimation totale du nombre d’étapes (ajustable dynamiquement).'),
    nextThoughtNeeded: z.boolean().optional().describe('Indique si une étape suivante est encore nécessaire (défaut true).'),
    isRevision: z.boolean().optional().describe('True si cette étape révise une étape précédente.'),
    revisesThought: z.number().optional().describe('Numéro de l’étape révisée.'),
    branchFromThought: z.number().optional().describe('Numéro d’étape d’origine pour explorer une branche alternative.'),
    branchId: z.string().optional().describe('Identifiant court de la branche alternative.'),
    hypothesis: z.string().optional().describe('Hypothèse formulée à vérifier.'),
    verified: z.boolean().optional().describe('Résultat de vérification de l’hypothèse.'),
  },
}, async (params) => {
  const st = loadState();
  const res = await executeThinkingAction(params, { deepThinkingEnabled: st.deepThinkingEnabled !== false });
  return asCompactText(res);
});

server.registerTool('fs_fetch', {
  title: 'Web Fetch SSRF-safe (HTML → Markdown) [ATG]',
  description: 'Récupère une URL HTTP(S) publique avec protection SSRF et convertit le HTML en Markdown propre (sans lancer de navigateur).',
  inputSchema: {
    url: z.string().min(8).describe('URL http:// ou https:// publique à lire.'),
    maxChars: z.number().int().min(500).max(80_000).optional().describe('Limite de caractères retournés (défaut 12000).'),
    raw: z.boolean().optional().describe('True pour retourner le contenu brut sans conversion Markdown.'),
    extractLinks: z.boolean().optional().describe('True pour extraire aussi la liste structurée des liens absolus de la page HTML.'),
    timeoutMs: z.number().int().min(3000).max(30_000).optional().describe('Délai HTTP max en ms (défaut 15000).'),
  },
}, async (params) => {
  const res = await executeWebFetch(params);
  return asCompactText(res);
});

server.registerTool('fs_doc_read', {
  title: 'Lecture Documents (Office, Markdown, CSV, PDF) [ATG]',
  description: 'Lit des documents locaux en Markdown propre : Markdown/Texte/CSV/HTML, .docx/.pptx/.xlsx nativement sur Windows sans dépendance, et .pdf si markitdown/pypdf/pymupdf est disponible.',
  inputSchema: {
    path: z.string().min(1).describe('Chemin du fichier (absolu ou relatif à workspaceRoot).'),
    workspaceRoot: z.string().optional().describe('Dossier racine pour résoudre un chemin relatif.'),
    maxChars: z.number().int().min(500).max(80_000).optional().describe('Limite de caractères retournés (défaut 16000).'),
  },
}, async (params) => {
  const ws = params.workspaceRoot || loadState().lastWorkspaceRoot || '';
  const res = await executeDocRead({ ...params, workspaceRoot: ws });
  return asCompactText(res);
});

server.registerTool('fs_git', {
  title: 'Git Lecture Seule (status, diff, log, branch, show, stash_list) [ATG]',
  description: 'Inspecte un dépôt Git local en lecture seule sans permission shell : status, diff, log, branch, show, stash_list.',
  inputSchema: {
    action: z.enum(GIT_ACTIONS).default('status').describe('Action Git lecture seule : status, diff, log, branch, show ou stash_list.'),
    workspaceRoot: z.string().optional().describe('Chemin du dépôt Git (défaut : dernier workspace utilisé).'),
    staged: z.boolean().optional().describe('Pour diff : comparer la zone de staging (--staged).'),
    limit: z.number().int().min(1).max(50).optional().describe('Pour log : nombre de commits récents (défaut 15).'),
    ref: z.string().optional().describe('Pour show : révision ou commit à inspecter (défaut HEAD).'),
    path: z.string().optional().describe('Filtrer diff ou show sur un chemin précis.'),
    maxChars: z.number().int().min(500).max(80_000).optional().describe('Limite de caractères retournés (défaut 14000).'),
  },
}, async (params) => {
  const ws = params.workspaceRoot || loadState().lastWorkspaceRoot || '';
  const res = await executeGitAction({ ...params, workspaceRoot: ws });
  return asCompactText(res);
});

server.registerTool('fs_media', {
  title: 'Média, Image, SVG, FFmpeg & HyperFrames [ATG]',
  description: 'Outil média unifié et compact : infos/édition image native Windows (resize/crop/rotate/convert), export SVG→PNG, montage vidéo FFmpeg (clip/convert/frames/9:16/audio), passerelle Canvas→HyperFrames et rendu HTML→MP4 HyperFrames.',
  inputSchema: {
    action: z.enum(MEDIA_ACTIONS).default('capabilities').describe('capabilities, image_info, image_edit, svg_export, video_info, video_edit, hyperframes ou canvas_to_hyperframes.'),
    subAction: z.string().optional().describe('video_edit : clip|convert|frames|reframe_9_16|audio_extract|normalize_audio ; hyperframes : init|render|normalize-audio.'),
    inputPath: z.string().optional().describe('Fichier source (image, vidéo ou HTML Canvas).'),
    path: z.string().optional().describe('Alias de inputPath ou dossier projet HyperFrames.'),
    outputPath: z.string().optional().describe('Chemin de sortie souhaité.'),
    projectDir: z.string().optional().describe('Dossier projet pour hyperframes / canvas_to_hyperframes.'),
    workspaceRoot: z.string().optional().describe('Dossier racine pour chemins relatifs.'),
    width: z.number().int().min(1).max(8192).optional().describe('Largeur cible en pixels.'),
    height: z.number().int().min(1).max(8192).optional().describe('Hauteur cible en pixels.'),
    keepAspect: z.boolean().optional().describe('Conserver le ratio (défaut true).'),
    rotate: z.number().optional().describe('Rotation image : 90, 180 ou 270.'),
    crop: z.object({ x: z.number(), y: z.number(), width: z.number(), height: z.number() }).optional().describe('Zone de recadrage image.'),
    format: z.string().optional().describe('Format de sortie (png, jpg, bmp, gif, webp, mp4, webm, mp3, wav).'),
    quality: z.number().int().min(10).max(100).optional().describe('Qualité JPEG/WebP (10-100, défaut 90).'),
    svg: z.string().optional().describe('Code SVG complet pour svg_export.'),
    html: z.string().optional().describe('Code HTML complet pour canvas_to_hyperframes.'),
    title: z.string().optional().describe('Titre du projet pour canvas_to_hyperframes.'),
    renderPng: z.boolean().optional().describe('Pour svg_export : rendre aussi un PNG haute résolution.'),
    start: z.union([z.string(), z.number()]).optional().describe('Début de découpe vidéo (ex: "00:00:05" ou 5).'),
    duration: z.union([z.string(), z.number()]).optional().describe('Durée vidéo (en secondes ou HH:MM:SS).'),
    fps: z.number().optional().describe('Images par seconde (frames, GIF ou HyperFrames).'),
    maxFrames: z.number().int().min(1).max(30).optional().describe('Nombre max de frames PNG extraites (défaut 6).'),
  },
}, async (params) => {
  const st = loadState();
  const ws = params.workspaceRoot || st.lastWorkspaceRoot || '';
  const res = await executeMediaAction(params, {
    permissionMode: st.permissionMode || 'acceptEdits',
    mediaToolsEnabled: st.mediaToolsEnabled !== false,
    workspaceRoot: ws,
  });
  return asCompactText(res);
});

const transport = new StdioServerTransport();
await server.connect(transport);
