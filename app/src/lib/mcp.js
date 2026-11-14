// ====================================================================
// FranceStudent CLI - Client MCP Local Universel (JSON-RPC 2.0)
// Supporte les transports stdio (npx, uvx, node, python) et SSE / HTTP
// ====================================================================

import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { loadMcpConfig } from './config.js';

const MCP_PROTOCOL_VERSION = '2024-11-05';
const CLIENT_INFO = { name: 'francestudent-cli', version: '3.8.0' };

// Pool de connexions actives et cache des outils découverts
const activeClients = new Map(); // key: `${workingFolder}::${serverName}` -> McpClient
const discoveredToolsCache = new Map(); // key: `${workingFolder}::${serverName}` -> { tools, serverInfo, error, updatedAt }

function clientKey(workingFolder, serverName) {
  return `${resolve(workingFolder || process.cwd())}::${serverName}`;
}

/**
 * Client MCP JSON-RPC 2.0 (stdio ou SSE / Streamable HTTP).
 */
export class McpClient {
  constructor(name, config, workingFolder) {
    this.name = name;
    const rawCfg = config || {};
    if (!rawCfg.url && typeof rawCfg.command === 'string' && /^https?:\/\//i.test(rawCfg.command.trim())) {
      const normalized = { ...rawCfg, url: rawCfg.command.trim() };
      delete normalized.command;
      if (!normalized.token && Array.isArray(rawCfg.args) && rawCfg.args[0] && !String(rawCfg.args[0]).startsWith('-')) {
        normalized.token = String(rawCfg.args[0]).trim();
      }
      this.config = normalized;
    } else {
      this.config = rawCfg;
    }
    this.workingFolder = resolve(workingFolder || process.cwd());
    this.nextId = 1;
    this.pending = new Map();
    this.proc = null;
    this.buffer = '';
    this.stderrLog = '';
    this.connected = false;
    this.serverInfo = null;
    this.tools = [];

    // État SSE / Streamable HTTP
    this.sseEndpoint = null;
    this.sseAbort = null;
    this.mcpSessionId = null;
    this.resolvedUrl = this.config.url || null;
    this.transportMode = this.config.url ? 'http' : 'stdio';
  }

  async connect(timeoutMs = 10000) {
    if (this.connected) return;

    if (this.config.url) {
      await this._connectHttpOrSse(timeoutMs);
    } else if (this.config.command) {
      await this._connectStdio(timeoutMs);
    } else {
      throw new Error('Configuration MCP invalide : ni "command" ni "url" défini.');
    }

    const initRes = await this.request(
      'initialize',
      {
        protocolVersion: MCP_PROTOCOL_VERSION,
        capabilities: { tools: {} },
        clientInfo: CLIENT_INFO,
      },
      timeoutMs
    );

    this.serverInfo = initRes?.serverInfo || null;
    this.connected = true;

    // Notification obligatoire selon la spec MCP
    await this.notify('notifications/initialized', {}).catch(() => {});

    // Découverte automatique des outils
    try {
      const toolsRes = await this.request('tools/list', {}, timeoutMs);
      this.tools = Array.isArray(toolsRes?.tools) ? toolsRes.tools : [];
    } catch {
      this.tools = [];
    }
  }

  async _connectStdio(timeoutMs) {
    const cmd = this.config.command;
    const args = Array.isArray(this.config.args) ? this.config.args : [];
    const cwd = this.config.cwd ? resolve(this.workingFolder, this.config.cwd) : this.workingFolder;
    const env = { ...process.env, ...(this.config.env || {}) };

    if (process.platform === 'win32') {
      const quoteArg = (a) => (/[\s"&|<>^()]/.test(String(a)) ? `"${String(a).replace(/"/g, '\\"')}"` : String(a));
      const fullCmd = [cmd, ...args].map(quoteArg).join(' ');
      this.proc = spawn(fullCmd, [], {
        cwd,
        env,
        stdio: ['pipe', 'pipe', 'pipe'],
        shell: true,
        windowsHide: true,
      });
    } else {
      this.proc = spawn(cmd, args, {
        cwd,
        env,
        stdio: ['pipe', 'pipe', 'pipe'],
        windowsHide: true,
      });
    }

    this.proc.stdout.setEncoding('utf-8');
    this.proc.stdout.on('data', (chunk) => this._onStdioData(chunk));

    this.proc.stderr.setEncoding('utf-8');
    this.proc.stderr.on('data', (chunk) => {
      this.stderrLog = (this.stderrLog + chunk).slice(-3000);
    });

    await new Promise((resolveReady, rejectReady) => {
      let settled = false;
      const timer = setTimeout(() => {
        if (!settled) {
          settled = true;
          resolveReady();
        }
      }, Math.min(350, timeoutMs));

      this.proc.once('error', (err) => {
        clearTimeout(timer);
        if (!settled) {
          settled = true;
          rejectReady(new Error(`Impossible de lancer "${cmd}" : ${err.message}`));
        }
      });

      this.proc.once('exit', (code) => {
        clearTimeout(timer);
        this.connected = false;
        const errText = this.stderrLog.trim() || `Processus terminé prématurément (code ${code})`;
        for (const [, pending] of this.pending) {
          clearTimeout(pending.timer);
          pending.reject(new Error(errText));
        }
        this.pending.clear();
        if (!settled) {
          settled = true;
          rejectReady(new Error(errText));
        }
      });

      // Si stdout reçoit déjà quelque chose ou le process reste vivant
      setTimeout(() => {
        if (!settled && this.proc && !this.proc.killed && this.proc.exitCode === null) {
          settled = true;
          clearTimeout(timer);
          resolveReady();
        }
      }, 150);
    });
  }

  _onStdioData(chunk) {
    this.buffer += chunk;
    const lines = this.buffer.split(/\r?\n/);
    this.buffer = lines.pop() || '';

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      // Ignore les logs non-JSON éventuellement émis sur stdout par certains serveurs
      if (!trimmed.startsWith('{')) continue;
      try {
        const msg = JSON.parse(trimmed);
        this._handleJsonRpcMessage(msg);
      } catch {
        // Ignorer ligne JSON partielle ou bruit
      }
    }
  }

  _buildHttpHeaders(extra = {}) {
    const headers = { ...extra, ...(this.config.headers || {}) };
    if (this.mcpSessionId && !headers['Mcp-Session-Id'] && !headers['mcp-session-id']) {
      headers['Mcp-Session-Id'] = this.mcpSessionId;
    }
    const rawToken = this.config.token || this.config.bearerToken || this.config.apiKey;
    if (rawToken && !headers.Authorization && !headers.authorization) {
      const clean = String(rawToken).replace(/^Bearer\s+/i, '').trim();
      if (clean) headers.Authorization = `Bearer ${clean}`;
    }
    return headers;
  }

  async _fetchWithLocalhostFallback(url, options) {
    try {
      return await fetch(url, options);
    } catch (err) {
      if (/^https?:\/\/localhost(?::\d+)?(\/|$)/i.test(url) && err.name !== 'AbortError') {
        const ipv4Url = url.replace(/^(https?:\/\/)localhost(?=(?::\d+)?(?:\/|$))/i, '$1127.0.0.1');
        const res = await fetch(ipv4Url, options);
        if (url === this.resolvedUrl) this.resolvedUrl = ipv4Url;
        return res;
      }
      throw err;
    }
  }

  async _connectHttpOrSse(timeoutMs) {
    const baseUrl = this.resolvedUrl || this.config.url;
    const isExplicitSse = /\/sse(\?|$)/i.test(baseUrl);
    if (!isExplicitSse) {
      // Streamable HTTP en priorité (POST direct avec capture de Mcp-Session-Id)
      this.transportMode = 'http';
      return;
    }

    const headers = this._buildHttpHeaders({ Accept: 'text/event-stream, application/json' });
    const controller = new AbortController();
    this.sseAbort = controller;
    const timer = setTimeout(() => controller.abort(), Math.min(timeoutMs, 4000));

    try {
      const res = await this._fetchWithLocalhostFallback(baseUrl, {
        method: 'GET',
        headers,
        signal: controller.signal,
      });
      clearTimeout(timer);

      const contentType = (res.headers.get('content-type') || '').toLowerCase();
      if (res.ok && contentType.includes('text/event-stream') && res.body) {
        this.transportMode = 'sse';
        await this._readSseStream(res.body, this.resolvedUrl || baseUrl, timeoutMs);
        return;
      }

      this.transportMode = 'http';
      try { await res.body?.cancel(); } catch {}
      this.sseAbort = null;
    } catch (err) {
      clearTimeout(timer);
      if (err.name === 'AbortError') {
        throw new Error(`Délai d'attente dépassé sur ${baseUrl}`);
      }
      this.transportMode = 'http';
    }
  }

  async _readSseStream(body, baseUrl, timeoutMs) {
    const reader = body.getReader();
    const decoder = new TextDecoder('utf-8');
    let sseBuffer = '';

    let resolveEndpoint;
    const endpointPromise = new Promise((res) => {
      resolveEndpoint = res;
    });

    const pump = async () => {
      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          sseBuffer += decoder.decode(value, { stream: true });
          const events = sseBuffer.split(/\r?\n\r?\n/);
          sseBuffer = events.pop() || '';

          for (const rawEv of events) {
            let evName = 'message';
            const dataLines = [];
            for (const l of rawEv.split(/\r?\n/)) {
              if (l.startsWith('event:')) evName = l.slice(6).trim();
              else if (l.startsWith('data:')) dataLines.push(l.slice(5).trim());
            }
            const dataStr = dataLines.join('\n');
            if (!dataStr) continue;

            if (evName === 'endpoint') {
              this.sseEndpoint = new URL(dataStr, baseUrl).toString();
              resolveEndpoint(this.sseEndpoint);
            } else if (dataStr.startsWith('{')) {
              try {
                this._handleJsonRpcMessage(JSON.parse(dataStr));
              } catch {}
            }
          }
        }
      } catch {
        this.connected = false;
      }
    };

    pump();

    const waitTimer = setTimeout(() => resolveEndpoint(null), Math.min(timeoutMs, 3000));
    await endpointPromise;
    clearTimeout(waitTimer);
    if (!this.sseEndpoint) {
      // Fallback vers Streamable HTTP si aucun event endpoint n'a été envoyé
      this.transportMode = 'http';
    }
  }

  _handleJsonRpcMessage(msg) {
    if (!msg || typeof msg !== 'object') return;
    if (msg.id !== undefined && this.pending.has(msg.id)) {
      const { resolve: res, reject: rej, timer } = this.pending.get(msg.id);
      this.pending.delete(msg.id);
      clearTimeout(timer);
      if (msg.error) {
        rej(new Error(msg.error.message || JSON.stringify(msg.error)));
      } else {
        res(msg.result);
      }
    }
  }

  async request(method, params = {}, timeoutMs = 15000) {
    const id = this.nextId++;
    const payload = { jsonrpc: '2.0', id, method, params };

    if (this.transportMode === 'stdio') {
      if (!this.proc || !this.proc.stdin.writable) {
        throw new Error(`Le serveur MCP "${this.name}" n'est pas actif.`);
      }
      return new Promise((resolveReq, rejectReq) => {
        const timer = setTimeout(() => {
          this.pending.delete(id);
          rejectReq(new Error(`Timeout (${timeoutMs}ms) sur ${this.name}.${method}`));
        }, timeoutMs);

        this.pending.set(id, { resolve: resolveReq, reject: rejectReq, timer });
        this.proc.stdin.write(JSON.stringify(payload) + '\n');
      });
    }

    const targetUrl = this.transportMode === 'sse' && this.sseEndpoint
      ? this.sseEndpoint
      : (this.resolvedUrl || this.config.url);
    const headers = this._buildHttpHeaders({
      'Content-Type': 'application/json',
      Accept: 'application/json, text/event-stream',
    });

    if (this.transportMode === 'sse' && this.sseEndpoint) {
      return new Promise(async (resolveReq, rejectReq) => {
        const timer = setTimeout(() => {
          this.pending.delete(id);
          rejectReq(new Error(`Timeout (${timeoutMs}ms) sur ${this.name}.${method}`));
        }, timeoutMs);
        this.pending.set(id, { resolve: resolveReq, reject: rejectReq, timer });

        try {
          const postRes = await this._fetchWithLocalhostFallback(targetUrl, {
            method: 'POST',
            headers,
            body: JSON.stringify(payload),
          });
          const sid = postRes.headers.get('mcp-session-id');
          if (sid) this.mcpSessionId = sid;
          if (!postRes.ok) {
            clearTimeout(timer);
            this.pending.delete(id);
            const errBody = await postRes.text().catch(() => '');
            rejectReq(new Error(`HTTP ${postRes.status} sur ${targetUrl}${errBody ? ` : ${errBody.slice(0, 160)}` : ''}`));
            return;
          }
          const ct = (postRes.headers.get('content-type') || '').toLowerCase();
          if (ct.includes('application/json')) {
            const json = await postRes.json().catch(() => null);
            if (json && (json.result !== undefined || json.error !== undefined)) {
              this._handleJsonRpcMessage(json);
            }
          }
        } catch (err) {
          clearTimeout(timer);
          this.pending.delete(id);
          rejectReq(err);
        }
      });
    }

    // Streamable HTTP POST direct
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const res = await this._fetchWithLocalhostFallback(targetUrl, {
        method: 'POST',
        headers,
        body: JSON.stringify(payload),
        signal: controller.signal,
      });
      clearTimeout(timer);
      const sid = res.headers.get('mcp-session-id');
      if (sid) this.mcpSessionId = sid;
      if (!res.ok) {
        const errBody = await res.text().catch(() => '');
        throw new Error(`HTTP ${res.status} ${res.statusText}${errBody ? ` : ${errBody.slice(0, 180)}` : ''}`);
      }
      const ct = (res.headers.get('content-type') || '').toLowerCase();
      if (ct.includes('text/event-stream')) {
        const rawText = await res.text();
        for (const line of rawText.split(/\r?\n/)) {
          if (line.startsWith('data:')) {
            const d = line.slice(5).trim();
            if (d.startsWith('{')) {
              const parsed = JSON.parse(d);
              if (parsed.id === id) {
                if (parsed.error) throw new Error(parsed.error.message || JSON.stringify(parsed.error));
                return parsed.result;
              }
            }
          }
        }
        throw new Error('Réponse SSE vide du serveur MCP.');
      }
      const json = await res.json();
      if (json.error) throw new Error(json.error.message || JSON.stringify(json.error));
      return json.result;
    } catch (err) {
      clearTimeout(timer);
      throw err;
    }
  }

  async notify(method, params = {}) {
    const payload = { jsonrpc: '2.0', method, params };
    if (this.transportMode === 'stdio') {
      if (this.proc && this.proc.stdin.writable) {
        this.proc.stdin.write(JSON.stringify(payload) + '\n');
      }
      return;
    }
    const targetUrl = this.transportMode === 'sse' && this.sseEndpoint
      ? this.sseEndpoint
      : (this.resolvedUrl || this.config.url);
    await this._fetchWithLocalhostFallback(targetUrl, {
      method: 'POST',
      headers: this._buildHttpHeaders({
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
      }),
      body: JSON.stringify(payload),
    }).catch(() => {});
  }

  async callTool(toolName, args = {}, timeoutMs = 25000) {
    if (!this.connected) {
      await this.connect(timeoutMs);
    }
    const res = await this.request('tools/call', { name: toolName, arguments: args }, timeoutMs);
    return res;
  }

  close() {
    this.connected = false;
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new Error('Client MCP fermé.'));
    }
    this.pending.clear();
    if (this.sseAbort) {
      try { this.sseAbort.abort(); } catch {}
      this.sseAbort = null;
    }
    if (this.proc) {
      try { this.proc.kill(); } catch {}
      this.proc = null;
    }
  }
}

/**
 * Récupère ou crée une connexion MCP persistante pour un serveur configuré.
 */
export async function getOrConnectMcpClient(workingFolder, serverName, timeoutMs = 12000) {
  const config = loadMcpConfig(workingFolder);
  const servers = config?.mcpServers || {};
  const matchedKey = Object.keys(servers).find((k) => k === serverName)
    || Object.keys(servers).find((k) => k.toLowerCase() === String(serverName || '').toLowerCase());
  const effectiveName = matchedKey || serverName;
  const serverCfg = matchedKey ? servers[matchedKey] : null;
  if (!serverCfg) {
    throw new Error(`Serveur MCP "${serverName}" introuvable dans mcp.json.`);
  }
  if (serverCfg.disabled) {
    throw new Error(`Le serveur MCP "${effectiveName}" est désactivé (activez-le avec /mcp on ${effectiveName}).`);
  }

  const key = clientKey(workingFolder, effectiveName);
  let client = activeClients.get(key);
  if (client && client.connected) {
    return client;
  }

  if (client) {
    client.close();
  }

  client = new McpClient(effectiveName, serverCfg, workingFolder);
  await client.connect(timeoutMs);
  activeClients.set(key, client);
  discoveredToolsCache.set(key, {
    tools: client.tools,
    serverInfo: client.serverInfo,
    error: null,
    updatedAt: Date.now(),
  });
  return client;
}

/**
 * Teste réellement un serveur MCP (stdio ou SSE/HTTP) avec handshake initialize + tools/list.
 */
export async function probeMcpServer(name, serverConfig, workingFolder, timeoutMs = 8000) {
  const tempClient = new McpClient(name, serverConfig, workingFolder);
  try {
    await tempClient.connect(timeoutMs);
    const tools = tempClient.tools || [];
    const serverInfo = tempClient.serverInfo;
    const key = clientKey(workingFolder, name);
    discoveredToolsCache.set(key, {
      tools,
      serverInfo,
      error: null,
      updatedAt: Date.now(),
    });
    return {
      ok: true,
      type: serverConfig.url ? tempClient.transportMode.toUpperCase() : 'stdio',
      tools,
      serverInfo,
      details: `${tools.length} outil(s) MCP découvert(s)${serverInfo?.name ? ` · ${serverInfo.name}${serverInfo.version ? ` v${serverInfo.version}` : ''}` : ''}`,
    };
  } catch (err) {
    return {
      ok: false,
      type: serverConfig.url ? 'SSE / HTTP' : 'stdio',
      error: err.message,
      tools: [],
    };
  } finally {
    tempClient.close();
  }
}

/**
 * Découvre les outils des serveurs MCP locaux actifs (sans bloquer l'interface).
 */
export async function discoverActiveLocalMcpTools(workingFolder, timeoutMs = 7000) {
  const config = loadMcpConfig(workingFolder);
  const active = Object.entries(config?.mcpServers || {}).filter(([_, s]) => !s.disabled);
  if (active.length === 0) return {};

  const results = {};
  await Promise.all(
    active.map(async ([name, srvCfg]) => {
      const key = clientKey(workingFolder, name);
      const cached = discoveredToolsCache.get(key);
      if (cached && Date.now() - cached.updatedAt < 120_000 && !cached.error) {
        results[name] = cached;
        return;
      }
      const probe = await probeMcpServer(name, srvCfg, workingFolder, timeoutMs);
      results[name] = probe;
      if (!probe.ok) {
        discoveredToolsCache.set(key, { tools: [], serverInfo: null, error: probe.error, updatedAt: Date.now() });
      }
    })
  );
  return results;
}

/**
 * Retourne les outils MCP mis en cache pour le dossier courant (synchrone pour le prompt).
 */
export function getCachedLocalMcpTools(workingFolder) {
  const config = loadMcpConfig(workingFolder);
  const active = Object.entries(config?.mcpServers || {}).filter(([_, s]) => !s.disabled);
  const out = {};
  for (const [name] of active) {
    const cached = discoveredToolsCache.get(clientKey(workingFolder, name));
    if (cached && Array.isArray(cached.tools) && cached.tools.length > 0) {
      out[name] = cached.tools;
    }
  }
  return out;
}

/**
 * Appelle un outil sur un serveur MCP local et formate la réponse en texte exploitable par l'IA.
 */
export async function callLocalMcpTool(workingFolder, serverName, toolName, args = {}) {
  const client = await getOrConnectMcpClient(workingFolder, serverName, 15000);
  const res = await client.callTool(toolName, args, 25000);

  const parts = [];
  if (Array.isArray(res?.content)) {
    for (const item of res.content) {
      if (item.type === 'text' && item.text) parts.push(item.text);
      else if (item.type === 'resource' && item.resource?.text) parts.push(item.resource.text);
      else parts.push(JSON.stringify(item));
    }
  } else if (res !== undefined) {
    parts.push(typeof res === 'string' ? res : JSON.stringify(res, null, 2));
  }

  return {
    output: parts.join('\n') || '(Réponse MCP vide)',
    isError: Boolean(res?.isError),
  };
}

/**
 * Exécute une commande interne `fs-mcp ...` appelée par l'IA via run_command.
 * Syntaxes supportées :
 *   fs-mcp list
 *   fs-mcp tools <serveur>
 *   fs-mcp call <serveur> <outil> [json_args]
 */
export async function executeFsMcpCommand(workingFolder, rawCommand) {
  const rest = rawCommand.trim().replace(/^fs-mcp\s+/i, '').trim();
  if (!rest || rest.toLowerCase() === 'list') {
    const config = loadMcpConfig(workingFolder);
    const active = Object.entries(config?.mcpServers || {}).filter(([_, s]) => !s.disabled);
    if (active.length === 0) {
      return { output: 'Aucun serveur MCP local actif.', isError: false };
    }
    const lines = active.map(([name, s]) => `- ${name} (${s.url ? s.url : `${s.command} ${(s.args || []).join(' ')}`}) : ${s.description || ''}`);
    return { output: `Serveurs MCP locaux actifs :\n${lines.join('\n')}`, isError: false };
  }

  const toolsMatch = rest.match(/^tools\s+(\S+)$/i);
  if (toolsMatch) {
    const serverName = toolsMatch[1];
    try {
      const client = await getOrConnectMcpClient(workingFolder, serverName, 12000);
      if (client.tools.length === 0) {
        return { output: `Serveur "${serverName}" connecté mais aucun outil exposé.`, isError: false };
      }
      const list = client.tools.map((t) => {
        const props = Object.keys(t.inputSchema?.properties || {});
        return `- ${t.name}(${props.join(', ')}) : ${t.description || ''}`;
      });
      return { output: `Outils disponibles sur "${serverName}" :\n${list.join('\n')}`, isError: false };
    } catch (err) {
      return { output: `Erreur MCP (${serverName}) : ${err.message}`, isError: true };
    }
  }

  const callMatch = rest.match(/^call\s+(\S+)\s+(\S+)(?:\s+([\s\S]+))?$/i);
  if (callMatch) {
    const [, serverName, toolName, rawJson] = callMatch;
    let parsedArgs = {};
    if (rawJson && rawJson.trim()) {
      let cleanJson = rawJson.trim();
      // Retire les quotes simples ou doubles externes ajoutées par le shell
      if ((cleanJson.startsWith("'") && cleanJson.endsWith("'")) || (cleanJson.startsWith('"') && cleanJson.endsWith('"'))) {
        cleanJson = cleanJson.slice(1, -1);
      }
      try {
        parsedArgs = JSON.parse(cleanJson);
      } catch (e) {
        // Tente de déséchapper \" si l'IA a passé un JSON échappé entre guillemets doubles
        try {
          parsedArgs = JSON.parse(cleanJson.replace(/\\"/g, '"'));
        } catch {
          return { output: `Arguments JSON invalides pour ${serverName}.${toolName} : ${e.message}\nReçu : ${cleanJson}`, isError: true };
        }
      }
    }

    try {
      return await callLocalMcpTool(workingFolder, serverName, toolName, parsedArgs);
    } catch (err) {
      return { output: `Erreur appel MCP ${serverName}.${toolName} : ${err.message}`, isError: true };
    }
  }

  return {
    output: 'Usage fs-mcp : "fs-mcp list" | "fs-mcp tools <serveur>" | "fs-mcp call <serveur> <outil> \'{...}\'"',
    isError: true,
  };
}

/**
 * Extrait les blocs ```mcp-call ... ``` générés par l'IA (bypass universel sans cloud FS ni clientTools).
 */
export function extractMcpCallBlocks(text) {
  if (!text || typeof text !== 'string') return [];
  const calls = [];
  const regex = /```mcp-call\s*\r?\n([\s\S]*?)```/gi;
  let match;
  while ((match = regex.exec(text)) !== null) {
    const raw = (match[1] || '').trim();
    if (!raw) continue;
    try {
      const parsed = JSON.parse(raw);
      if (parsed && parsed.server && parsed.tool) {
        calls.push({
          server: String(parsed.server).trim(),
          tool: String(parsed.tool).trim(),
          arguments: parsed.arguments && typeof parsed.arguments === 'object' ? parsed.arguments : {},
        });
      }
    } catch {
      // Ignorer bloc JSON malformé
    }
  }
  return calls;
}

/**
 * Supprime les blocs ```mcp-call ... ``` du texte affiché à l'utilisateur.
 */
export function stripMcpCallBlocks(text) {
  if (!text || typeof text !== 'string') return '';
  return text.replace(/```mcp-call\s*\r?\n[\s\S]*?```/gi, '').replace(/\n{3,}/g, '\n\n').trim();
}

/**
 * Ferme proprement tous les processus MCP locaux actifs.
 */
export function closeAllMcpConnections() {
  for (const client of activeClients.values()) {
    try {
      client.close();
    } catch {}
  }
  activeClients.clear();
}
