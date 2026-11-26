import http from 'node:http';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import {
  loadState,
  fetchModelCatalog,
  fetchQuotaSnapshot,
  fetchAgentsAndModels,
  pickModel,
  modelSupportsClientTools,
  isQuotaExemptModel,
  streamGenerationWithRefusalFallback,
} from './fs_core.mjs';

const PORT = Number(process.env.FS_BRIDGE_PORT || 52000);
const HOST = process.env.FS_BRIDGE_HOST || '127.0.0.1';
const QUOTA_TTL_MS = 20_000;
const MODELS_TTL_MS = 60_000;

let cachedQuota = { at: 0, value: null };
let cachedBundle = { at: 0, value: null };

const VIRTUAL_MODELS = [
  { id: 'fs-auto', label: 'France Student (Auto)', directive: null, effort: null },
  { id: 'fs-quality', label: 'France Student (Fort)', directive: '/quality', effort: 'deep' },
  { id: 'fs-max', label: 'France Student (Max)', directive: '/max', effort: 'max' },
  { id: 'fs-eco', label: 'France Student (Éco)', directive: '/eco', effort: 'standard' },
  { id: 'fs-hors-quota', label: 'France Student (Hors quota Mistral)', directive: '/mistral', effort: 'standard' },
];

async function getFreshQuota(state) {
  if (Date.now() - cachedQuota.at < QUOTA_TTL_MS && cachedQuota.value !== null) {
    return cachedQuota.value;
  }
  const q = await fetchQuotaSnapshot(state).catch(() => null);
  cachedQuota = { at: Date.now(), value: q };
  return q;
}

async function getFreshBundle(state) {
  if (Date.now() - cachedBundle.at < MODELS_TTL_MS && cachedBundle.value) {
    return cachedBundle.value;
  }
  const bundle = await fetchAgentsAndModels(state).catch(() => ({ ids: [], agentMap: new Map(), metaMap: new Map() }));
  cachedBundle = { at: Date.now(), value: bundle };
  return bundle;
}

function normalizeSchemaType(val) {
  if (!val) return undefined;
  const s = String(val).toLowerCase();
  if (['object', 'string', 'number', 'integer', 'boolean', 'array', 'null'].includes(s)) return s;
  return s;
}

export function normalizeJsonSchema(schema) {
  if (!schema || typeof schema !== 'object') return { type: 'object', properties: {} };
  if (Array.isArray(schema)) return schema.map(normalizeJsonSchema);
  const out = {};
  for (const [k, v] of Object.entries(schema)) {
    if (k === 'type') {
      out.type = normalizeSchemaType(v);
    } else if (k === 'properties' && v && typeof v === 'object') {
      out.properties = {};
      for (const [pk, pv] of Object.entries(v)) out.properties[pk] = normalizeJsonSchema(pv);
    } else if (k === 'items' && v && typeof v === 'object') {
      out.items = normalizeJsonSchema(v);
    } else if ((k === 'anyOf' || k === 'oneOf' || k === 'allOf') && Array.isArray(v)) {
      out[k] = v.map(normalizeJsonSchema);
    } else {
      out[k] = v;
    }
  }
  if (!out.type && out.properties) out.type = 'object';
  return out;
}

export function geminiToolsToClientTools(geminiTools) {
  const out = [];
  if (!Array.isArray(geminiTools)) return out;
  for (const group of geminiTools) {
    const decls = Array.isArray(group?.functionDeclarations)
      ? group.functionDeclarations
      : (Array.isArray(group?.function_declarations) ? group.function_declarations : []);
    for (const d of decls) {
      if (!d?.name) continue;
      out.push({
        name: String(d.name),
        description: String(d.description || ''),
        parameters: normalizeJsonSchema(d.parameters || d.parametersJsonSchema || { type: 'object', properties: {} }),
      });
    }
  }
  return out;
}

export function openAiToolsToClientTools(openAiTools) {
  const out = [];
  if (!Array.isArray(openAiTools)) return out;
  for (const t of openAiTools) {
    const fn = t?.function || t;
    if (!fn?.name) continue;
    out.push({
      name: String(fn.name),
      description: String(fn.description || ''),
      parameters: normalizeJsonSchema(fn.parameters || { type: 'object', properties: {} }),
    });
  }
  return out;
}

export function geminiRequestToPrompt(body) {
  let instructions = '';
  const sys = body?.systemInstruction || body?.system_instruction;
  if (typeof sys === 'string') {
    instructions = sys.trim();
  } else if (Array.isArray(sys?.parts)) {
    instructions = sys.parts.map((p) => p?.text || '').filter(Boolean).join('\n').trim();
  }

  const contents = Array.isArray(body?.contents) ? body.contents : [];
  const turns = [];
  for (const item of contents) {
    const role = item?.role === 'model' ? 'Assistant' : 'User';
    const parts = Array.isArray(item?.parts) ? item.parts : [];
    const lines = [];
    for (const p of parts) {
      if (typeof p?.text === 'string' && p.text.trim()) {
        if (!p.thought) lines.push(p.text);
      } else if (p?.functionCall || p?.function_call) {
        const fc = p.functionCall || p.function_call;
        lines.push(`[Tool Call: ${fc.name}(${JSON.stringify(fc.args ?? {})})]`);
      } else if (p?.functionResponse || p?.function_response) {
        const fr = p.functionResponse || p.function_response;
        lines.push(`[Tool Result (${fr.name}): ${JSON.stringify(fr.response ?? {})}]`);
      }
    }
    if (lines.length) turns.push(`${role}:\n${lines.join('\n')}`);
  }
  const prompt = turns.length === 1 && turns[0].startsWith('User:\n')
    ? turns[0].slice('User:\n'.length)
    : (turns.join('\n\n') || 'Bonjour');
  return { instructions, prompt };
}

export function openAiMessagesToPrompt(messages) {
  let instructions = '';
  const turns = [];
  for (const msg of Array.isArray(messages) ? messages : []) {
    const rawContent = typeof msg?.content === 'string'
      ? msg.content
      : (Array.isArray(msg?.content)
        ? msg.content.map((c) => (typeof c === 'string' ? c : (c?.text || ''))).filter(Boolean).join('\n')
        : (msg?.content ? JSON.stringify(msg.content) : ''));

    if (msg?.role === 'system' || msg?.role === 'developer') {
      if (rawContent) instructions += (instructions ? '\n\n' : '') + rawContent;
    } else if (msg?.role === 'tool') {
      turns.push(`Tool Result (${msg.name || msg.tool_call_id || 'tool'}):\n${rawContent}`);
    } else if (msg?.role === 'assistant') {
      const blocks = [];
      if (rawContent) blocks.push(rawContent);
      if (Array.isArray(msg.tool_calls)) {
        for (const tc of msg.tool_calls) {
          const fn = tc?.function || {};
          blocks.push(`[Tool Call: ${fn.name || 'tool'}(${fn.arguments || '{}'})]`);
        }
      }
      if (blocks.length) turns.push(`Assistant:\n${blocks.join('\n')}`);
    } else if (rawContent) {
      turns.push(`User:\n${rawContent}`);
    }
  }
  const prompt = turns.length === 1 && turns[0].startsWith('User:\n')
    ? turns[0].slice('User:\n'.length)
    : (turns.join('\n\n') || 'Bonjour');
  return { instructions, prompt };
}

function extractLatestUserTurn(prompt) {
  const text = String(prompt ?? '').trim();
  const marker = /(?:^|\n\n)User:\n/g;
  let lastIdx = -1;
  let m;
  while ((m = marker.exec(text)) !== null) lastIdx = m.index + m[0].length;
  return lastIdx >= 0 ? text.slice(lastIdx).trim() : text;
}

export async function resolveBridgeRoute(requestedModelId, prompt, state, bundle, quota) {
  const cleanReq = String(requestedModelId || 'fs-auto')
    .replace(/^models\//i, '')
    .replace(/^fs:/i, '')
    .trim();
  const virtual = VIRTUAL_MODELS.find((v) => v.id === cleanReq.toLowerCase());
  const latestTurn = extractLatestUserTurn(prompt);
  const rawTask = virtual?.directive ? `${virtual.directive} ${latestTurn}` : latestTurn;
  const explicitModel = !virtual && cleanReq && cleanReq !== 'auto' ? cleanReq : null;
  const effectiveState = virtual?.effort ? { ...state, reasoningEffort: virtual.effort } : state;

  const route = pickModel(rawTask, bundle.ids || [], effectiveState, explicitModel, {
    metaMap: bundle.metaMap,
    quotaRemainingPct: quota?.remainingPct ?? null,
    cutoffPercent: state?.cutoffPercent ?? 15,
    webSearchMode: 'off',
    canvasMode: 'off',
    imageGenMode: 'off',
  });
  return { route, effectiveState };
}

export function buildAntigravityCliConfig(port = PORT) {
  const base = `http://${HOST}:${port}`;
  const customModels = {};
  for (const vm of VIRTUAL_MODELS) {
    customModels[vm.id] = {
      modelName: `gemini-api://${base}/models/${vm.id}`,
      label: vm.label,
      maxTokens: 128000,
      toolFormatterType: 'TOOL_FORMATTER_TYPE_XML',
      modelFeatures: {
        supportsToolCalls: true,
        supportsThinking: true,
        supportsImages: false,
        supportsVideo: false,
        supportsPdf: false,
        supportsAudio: false,
      },
    };
  }
  return {
    modelProvider: 'gemini',
    customModelsConfig: {
      customModels,
    },
  };
}

function sendJson(res, status, payload) {
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(payload));
}

function geminiUsageFrom(usage) {
  if (!usage) return undefined;
  const promptTokenCount = Number(usage.input_tokens ?? usage.promptTokenCount ?? 0);
  const candidatesTokenCount = Number(usage.output_tokens ?? usage.candidatesTokenCount ?? 0);
  const totalTokenCount = Number(usage.total_tokens ?? usage.totalTokenCount ?? (promptTokenCount + candidatesTokenCount));
  return { promptTokenCount, candidatesTokenCount, totalTokenCount };
}

function geminiPartsFromResult(result) {
  const parts = [];
  if (result?.text) parts.push({ text: result.text });
  for (const tc of result?.toolCalls || []) {
    parts.push({
      functionCall: {
        name: tc.name,
        args: tc.arguments && typeof tc.arguments === 'object' ? tc.arguments : {},
      },
    });
  }
  if (!parts.length) parts.push({ text: '' });
  return parts;
}

export function createBridgeServer() {
  return http.createServer(async (req, res) => {
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
    res.setHeader('Access-Control-Allow-Headers', '*');

    if (req.method === 'OPTIONS') {
      res.writeHead(204);
      res.end();
      return;
    }

    const parsedUrl = new URL(req.url || '/', `http://${HOST}:${PORT}`);
    const pathname = parsedUrl.pathname;
    const altSse = parsedUrl.searchParams.get('alt') === 'sse';

    if (pathname === '/health' && req.method === 'GET') {
      const state = loadState();
      const quota = await getFreshQuota(state);
      return sendJson(res, 200, {
        ok: true,
        bridge: 'fs-atg',
        port: PORT,
        quotaRemainingPct: quota?.remainingPct ?? null,
        virtualModels: VIRTUAL_MODELS.map((v) => v.id),
      });
    }

    // 1. OpenAI Models list: GET /v1/models
    if (pathname === '/v1/models' && req.method === 'GET') {
      try {
        const state = loadState();
        const cat = await fetchModelCatalog(state).catch(() => ({ models: [] }));
        const now = Math.floor(Date.now() / 1000);
        const modelsList = [
          ...VIRTUAL_MODELS.map((vm) => ({
            id: vm.id,
            object: 'model',
            created: now,
            owned_by: 'francestudent',
            name: vm.label,
          })),
          ...(cat?.models || []).map((m) => ({
            id: m.id || m.name,
            object: 'model',
            created: now,
            owned_by: 'francestudent',
            name: m.name || m.id,
          })),
        ];
        return sendJson(res, 200, { object: 'list', data: modelsList });
      } catch (err) {
        return sendJson(res, 500, { error: { message: err.message } });
      }
    }

    // 2. Gemini Models list: GET /v1beta/models or /models
    if ((pathname === '/v1beta/models' || pathname === '/models') && req.method === 'GET') {
      try {
        const state = loadState();
        const cat = await fetchModelCatalog(state).catch(() => ({ models: [] }));
        const toGemini = (id, displayName, ctx = 128000) => ({
          name: `models/${id}`,
          baseModelId: id,
          version: '1.0',
          displayName: displayName || id,
          description: `France Student bridge (${id})`,
          inputTokenLimit: ctx || 128000,
          outputTokenLimit: 16384,
          supportedGenerationMethods: ['generateContent', 'streamGenerateContent'],
        });
        const models = [
          ...VIRTUAL_MODELS.map((vm) => toGemini(vm.id, vm.label, 200000)),
          ...(cat?.models || []).map((m) => toGemini(m.id || m.name, m.name || m.id, m.contextWindow)),
        ];
        return sendJson(res, 200, { models });
      } catch (err) {
        return sendJson(res, 500, { error: { message: err.message } });
      }
    }

    // 3. Gemini generateContent / streamGenerateContent (for Antigravity CLI customModelsConfig)
    const geminiMatch = /^\/(?:v1beta|v1)?\/?models\/([^:/]+):(generateContent|streamGenerateContent)$/i.exec(pathname);
    if (geminiMatch && req.method === 'POST') {
      const requestedModel = decodeURIComponent(geminiMatch[1]);
      const action = geminiMatch[2];
      const isStream = action.toLowerCase() === 'streamgeneratecontent';
      let raw = '';
      for await (const chunk of req) raw += chunk;

      const ac = new AbortController();
      req.on('close', () => ac.abort());

      try {
        const body = JSON.parse(raw || '{}');
        const state = loadState();
        const [quota, bundle] = await Promise.all([getFreshQuota(state), getFreshBundle(state)]);
        const { instructions, prompt } = geminiRequestToPrompt(body);
        const { route, effectiveState } = await resolveBridgeRoute(requestedModel, prompt, state, bundle, quota);
        const rawClientTools = geminiToolsToClientTools(body.tools);
        const clientTools = modelSupportsClientTools(route.model) ? rawClientTools : [];

        if (isStream && altSse) {
          res.writeHead(200, {
            'Content-Type': 'text/event-stream; charset=utf-8',
            'Cache-Control': 'no-cache',
            Connection: 'keep-alive',
          });
          const writeEvent = (payload) => {
            if (!res.writableEnded) res.write(`data: ${JSON.stringify(payload)}\n\n`);
          };
          const result = await streamGenerationWithRefusalFallback({
            state: effectiveState,
            model: route.model,
            availableModels: bundle.ids || [],
            quotaBypass: Boolean(route.quotaBypass),
            quotaRemainingPct: quota?.remainingPct ?? null,
            agentMap: bundle.agentMap,
            instructions,
            prompt,
            clientTools,
            returnToolCalls: clientTools.length > 0,
            enableWebSearch: false,
            enableCanvas: false,
            signal: ac.signal,
            onReasoning: (delta) => {
              if (!delta) return;
              writeEvent({
                candidates: [{ index: 0, content: { role: 'model', parts: [{ thought: true, text: delta }] } }],
                modelVersion: route.model,
              });
            },
            onText: (chunk) => {
              if (!chunk) return;
              writeEvent({
                candidates: [{ index: 0, content: { role: 'model', parts: [{ text: chunk }] } }],
                modelVersion: route.model,
              });
            },
          });

          const finalParts = [];
          for (const tc of result?.toolCalls || []) {
            finalParts.push({
              functionCall: {
                name: tc.name,
                args: tc.arguments && typeof tc.arguments === 'object' ? tc.arguments : {},
              },
            });
          }
          writeEvent({
            candidates: [{
              index: 0,
              content: { role: 'model', parts: finalParts.length ? finalParts : [{ text: '' }] },
              finishReason: 'STOP',
            }],
            ...(geminiUsageFrom(result?.usage) ? { usageMetadata: geminiUsageFrom(result.usage) } : {}),
            modelVersion: result?.model || route.model,
          });
          res.end();
          return;
        }

        const result = await streamGenerationWithRefusalFallback({
          state: effectiveState,
          model: route.model,
          availableModels: bundle.ids || [],
          quotaBypass: Boolean(route.quotaBypass),
          quotaRemainingPct: quota?.remainingPct ?? null,
          agentMap: bundle.agentMap,
          instructions,
          prompt,
          clientTools,
          returnToolCalls: clientTools.length > 0,
          enableWebSearch: false,
          enableCanvas: false,
          signal: ac.signal,
        });

        const payload = {
          candidates: [{
            index: 0,
            content: { role: 'model', parts: geminiPartsFromResult(result) },
            finishReason: 'STOP',
          }],
          ...(geminiUsageFrom(result?.usage) ? { usageMetadata: geminiUsageFrom(result.usage) } : {}),
          modelVersion: result?.model || route.model,
        };
        return sendJson(res, 200, isStream ? [payload] : payload);
      } catch (err) {
        if (!res.headersSent) {
          return sendJson(res, Number(err?.status) || 500, {
            error: { code: Number(err?.status) || 500, message: err.message, status: 'INTERNAL' },
          });
        }
        res.end();
        return;
      }
    }

    // 4. OpenAI Chat Completions: POST /v1/chat/completions (for Antigravity Python SDK LocalOpenAIAgentConfig)
    if ((pathname === '/v1/chat/completions' || pathname === '/chat/completions') && req.method === 'POST') {
      let raw = '';
      for await (const chunk of req) raw += chunk;

      const ac = new AbortController();
      req.on('close', () => ac.abort());

      try {
        const body = JSON.parse(raw || '{}');
        const requestedModel = body.model || 'fs-auto';
        const state = loadState();
        const [quota, bundle] = await Promise.all([getFreshQuota(state), getFreshBundle(state)]);
        const { instructions, prompt } = openAiMessagesToPrompt(body.messages);
        const { route, effectiveState } = await resolveBridgeRoute(requestedModel, prompt, state, bundle, quota);
        const rawClientTools = openAiToolsToClientTools(body.tools);
        const clientTools = modelSupportsClientTools(route.model) ? rawClientTools : [];
        const stream = body.stream !== false;
        const completionId = 'chatcmpl-' + randomUUID();
        const created = Math.floor(Date.now() / 1000);

        if (stream) {
          res.writeHead(200, {
            'Content-Type': 'text/event-stream; charset=utf-8',
            'Cache-Control': 'no-cache',
            Connection: 'keep-alive',
          });
          const result = await streamGenerationWithRefusalFallback({
            state: effectiveState,
            model: route.model,
            availableModels: bundle.ids || [],
            quotaBypass: Boolean(route.quotaBypass),
            quotaRemainingPct: quota?.remainingPct ?? null,
            agentMap: bundle.agentMap,
            instructions,
            prompt,
            clientTools,
            returnToolCalls: clientTools.length > 0,
            enableWebSearch: false,
            enableCanvas: false,
            signal: ac.signal,
            onText: (chunk) => {
              if (!chunk || res.writableEnded) return;
              res.write(`data: ${JSON.stringify({
                id: completionId,
                object: 'chat.completion.chunk',
                created,
                model: route.model,
                choices: [{ index: 0, delta: { content: chunk }, finish_reason: null }],
              })}\n\n`);
            },
            onReasoning: (delta) => {
              if (!delta || res.writableEnded) return;
              res.write(`data: ${JSON.stringify({
                id: completionId,
                object: 'chat.completion.chunk',
                created,
                model: route.model,
                choices: [{ index: 0, delta: { reasoning_content: delta }, finish_reason: null }],
              })}\n\n`);
            },
          });

          const returnedTools = result?.toolCalls || [];
          if (returnedTools.length) {
            res.write(`data: ${JSON.stringify({
              id: completionId,
              object: 'chat.completion.chunk',
              created,
              model: result?.model || route.model,
              choices: [{
                index: 0,
                delta: {
                  tool_calls: returnedTools.map((tc, idx) => ({
                    index: idx,
                    id: tc.callId || `call_${randomUUID().slice(0, 8)}`,
                    type: 'function',
                    function: {
                      name: tc.name,
                      arguments: JSON.stringify(tc.arguments ?? {}),
                    },
                  })),
                },
                finish_reason: null,
              }],
            })}\n\n`);
          }
          res.write(`data: ${JSON.stringify({
            id: completionId,
            object: 'chat.completion.chunk',
            created,
            model: result?.model || route.model,
            choices: [{ index: 0, delta: {}, finish_reason: returnedTools.length ? 'tool_calls' : 'stop' }],
          })}\n\n`);
          res.write('data: [DONE]\n\n');
          res.end();
          return;
        }

        const result = await streamGenerationWithRefusalFallback({
          state: effectiveState,
          model: route.model,
          availableModels: bundle.ids || [],
          quotaBypass: Boolean(route.quotaBypass),
          quotaRemainingPct: quota?.remainingPct ?? null,
          agentMap: bundle.agentMap,
          instructions,
          prompt,
          clientTools,
          returnToolCalls: clientTools.length > 0,
          enableWebSearch: false,
          enableCanvas: false,
          signal: ac.signal,
        });

        const returnedTools = result?.toolCalls || [];
        return sendJson(res, 200, {
          id: completionId,
          object: 'chat.completion',
          created,
          model: result?.model || route.model,
          choices: [{
            index: 0,
            message: {
              role: 'assistant',
              content: result?.text || (returnedTools.length ? null : ''),
              ...(returnedTools.length ? {
                tool_calls: returnedTools.map((tc) => ({
                  id: tc.callId || `call_${randomUUID().slice(0, 8)}`,
                  type: 'function',
                  function: {
                    name: tc.name,
                    arguments: JSON.stringify(tc.arguments ?? {}),
                  },
                })),
              } : {}),
            },
            finish_reason: returnedTools.length ? 'tool_calls' : 'stop',
          }],
        });
      } catch (err) {
        if (!res.headersSent) {
          return sendJson(res, Number(err?.status) || 500, { error: { message: err.message } });
        }
        res.end();
        return;
      }
    }

    return sendJson(res, 404, { error: { message: 'Not found' } });
  });
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url).toLowerCase() === process.argv[1].toLowerCase();
if (isMain) {
  if (process.argv.includes('--print-cli-config')) {
    console.log(JSON.stringify(buildAntigravityCliConfig(PORT), null, 2));
    process.exit(0);
  }
  const server = createBridgeServer();
  server.listen(PORT, HOST, () => {
    console.log(`FS_BRIDGE_READY http://${HOST}:${PORT} (Gemini API + OpenAI compatible)`);
  });
}
