import http from 'node:http';
import { randomUUID } from 'node:crypto';
import {
  loadState,
  fetchModelCatalog,
  resolveApiKey,
  streamGenerationWithRefusalFallback,
  fetchAgentsAndModels,
} from './fs_core.mjs';

const PORT = 52000;
const state = loadState();

const server = http.createServer(async (req, res) => {
  // CORS
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', '*');

  if (req.method === 'OPTIONS') {
    res.writeHead(204);
    res.end();
    return;
  }

  const url = (req.url || '').split('?')[0];

  // 1. GET /v1/models
  if ((url === '/v1/models' || url === '/models') && req.method === 'GET') {
    try {
      const cat = await fetchModelCatalog(state);
      const modelsList = (cat?.models || []).map((m) => ({
        id: m.id || m.name,
        object: 'model',
        created: Math.floor(Date.now() / 1000),
        owned_by: 'francestudent',
        name: m.name || m.id,
      }));
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ object: 'list', data: modelsList }));
    } catch (err) {
      res.writeHead(500, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: { message: err.message } }));
    }
    return;
  }

  // 2. POST /v1/chat/completions
  if ((url === '/v1/chat/completions' || url === '/chat/completions') && req.method === 'POST') {
    let raw = '';
    for await (const chunk of req) raw += chunk;
    try {
      const body = JSON.parse(raw || '{}');
      const model = body.model || 'claude-sonnet-5-5';
      const messages = Array.isArray(body.messages) ? body.messages : [];
      
      let instructions = '';
      const promptParts = [];
      for (const msg of messages) {
        const content = typeof msg.content === 'string' ? msg.content : JSON.stringify(msg.content);
        if (msg.role === 'system') {
          instructions += (instructions ? '\n\n' : '') + content;
        } else if (msg.role === 'user') {
          promptParts.push(`User: ${content}`);
        } else if (msg.role === 'assistant') {
          promptParts.push(`Assistant: ${content}`);
        }
      }
      const prompt = promptParts.join('\n\n') || 'Bonjour';

      const stream = body.stream !== false;
      const completionId = 'chatcmpl-' + randomUUID();

      if (stream) {
        res.writeHead(200, {
          'Content-Type': 'text/event-stream',
          'Cache-Control': 'no-cache',
          Connection: 'keep-alive',
        });

        const agentBundle = await fetchAgentsAndModels(state).catch(() => ({ agentMap: new Map() }));

        await streamGenerationWithRefusalFallback({
          state,
          model,
          prompt,
          instructions,
          agentMap: agentBundle.agentMap,
          availableModels: agentBundle.ids || [],
          onText: (chunk) => {
            const data = {
              id: completionId,
              object: 'chat.completion.chunk',
              created: Math.floor(Date.now() / 1000),
              model,
              choices: [{ index: 0, delta: { content: chunk }, finish_reason: null }],
            };
            res.write(`data: ${JSON.stringify(data)}\n\n`);
          },
          onReasoning: (delta) => {
            const data = {
              id: completionId,
              object: 'chat.completion.chunk',
              created: Math.floor(Date.now() / 1000),
              model,
              choices: [{ index: 0, delta: { reasoning_content: delta }, finish_reason: null }],
            };
            res.write(`data: ${JSON.stringify(data)}\n\n`);
          },
        });

        // Final done chunk
        const doneData = {
          id: completionId,
          object: 'chat.completion.chunk',
          created: Math.floor(Date.now() / 1000),
          model,
          choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
        };
        res.write(`data: ${JSON.stringify(doneData)}\n\n`);
        res.write('data: [DONE]\n\n');
        res.end();
      } else {
        let fullText = '';
        const agentBundle = await fetchAgentsAndModels(state).catch(() => ({ agentMap: new Map() }));
        await streamGenerationWithRefusalFallback({
          state,
          model,
          prompt,
          instructions,
          agentMap: agentBundle.agentMap,
          availableModels: agentBundle.ids || [],
          onText: (chunk) => { fullText += chunk; },
        });

        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({
          id: completionId,
          object: 'chat.completion',
          created: Math.floor(Date.now() / 1000),
          model,
          choices: [{ index: 0, message: { role: 'assistant', content: fullText }, finish_reason: 'stop' }],
        }));
      }
    } catch (err) {
      if (!res.headersSent) {
        res.writeHead(500, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: { message: err.message } }));
      } else {
        res.end();
      }
    }
    return;
  }

  res.writeHead(404, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ error: { message: 'Not found' } }));
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`FS_OPENAI_BRIDGE_READY http://127.0.0.1:${PORT}`);
});
