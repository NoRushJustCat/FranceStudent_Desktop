<script>
  import { onMount } from 'svelte';
  import { mcpStatus, mcpConnect, mcpCall } from './api.js';

  // Exécute UN outil MCP à la demande de l'utilisateur (aucune boucle automatique).
  // code : texte du bloc de code d'où l'on vient (préremplit l'argument choisi).
  // oninsert(text) : renvoie le résultat dans la zone de saisie (optionnel).
  let { code = '', oninsert = null, onclose } = $props();

  const KEY = 'fs.mcprun';
  const CODE_KEYS = ['code', 'script', 'source', 'command', 'cmd', 'query', 'text', 'content', 'input'];
  const MAX_INSERT = 8000;

  const saved = (() => {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? '{}') ?? {};
    } catch {
      return {};
    }
  })();

  let servers = $state([]);
  let loading = $state(true);
  let loadErr = $state('');
  let server = $state(saved.server ?? '');
  let tool = $state(saved.tool ?? '');
  let codeKey = $state(saved.codeKey ?? '');
  let argsText = $state('{}');
  let running = $state(false);
  let connecting = $state(false);
  let result = $state(null); // {text, isError}
  let err = $state('');
  let copied = $state(false);

  const srv = $derived(servers.find((s) => s.name === server));
  const tools = $derived(srv?.tools ?? []);
  const tl = $derived(tools.find((t) => t.name === tool));
  const props = $derived(tl?.schema?.properties ?? {});
  const strKeys = $derived(Object.keys(props).filter((k) => props[k]?.type === 'string'));

  function sample(p) {
    switch (p?.type) {
      case 'string':
        return '';
      case 'number':
      case 'integer':
        return 0;
      case 'boolean':
        return false;
      case 'array':
        return [];
      case 'object':
        return {};
      default:
        return null;
    }
  }

  function remember() {
    try {
      localStorage.setItem(KEY, JSON.stringify({ server, tool, codeKey }));
    } catch {}
  }

  // Reconstruit le JSON des arguments d'après le schéma de l'outil.
  function rebuild() {
    const o = {};
    const req = Array.isArray(tl?.schema?.required) ? tl.schema.required : [];
    for (const k of req) o[k] = sample(props[k]);
    if (codeKey) o[codeKey] = code;
    argsText = JSON.stringify(o, null, 2);
  }

  // Remet serveur / outil / argument de code sur des valeurs valides.
  function choose() {
    if (!servers.some((s) => s.name === server)) {
      server = (servers.find((s) => s.connected) ?? servers[0])?.name ?? '';
    }
    if (!tools.some((t) => t.name === tool)) tool = tools[0]?.name ?? '';
    if (!strKeys.includes(codeKey)) {
      codeKey = strKeys.find((k) => CODE_KEYS.includes(k.toLowerCase())) ?? strKeys[0] ?? '';
    }
    remember();
    rebuild();
  }

  async function load() {
    loading = true;
    loadErr = '';
    try {
      const st = await mcpStatus();
      servers = st.servers ?? [];
      if (st.error) loadErr = st.error;
    } catch (e) {
      loadErr = String(e?.message ?? e);
    } finally {
      loading = false;
    }
    choose();
  }

  async function connect() {
    if (!server) return;
    connecting = true;
    err = '';
    try {
      await mcpConnect(server);
      await load();
    } catch (e) {
      err = String(e?.message ?? e);
    } finally {
      connecting = false;
    }
  }

  async function run() {
    err = '';
    result = null;
    copied = false;
    let args;
    try {
      args = argsText.trim() ? JSON.parse(argsText) : {};
    } catch (e) {
      err = 'JSON invalide : ' + e.message;
      return;
    }
    if (args === null || typeof args !== 'object' || Array.isArray(args)) {
      err = 'Les arguments doivent être un objet JSON.';
      return;
    }
    running = true;
    try {
      result = await mcpCall(server, tool, args);
      remember();
    } catch (e) {
      err = String(e?.message ?? e);
    } finally {
      running = false;
    }
  }

  function insert() {
    if (!result || !oninsert) return;
    let body = result.text ?? '';
    if (body.length > MAX_INSERT) body = body.slice(0, MAX_INSERT) + '\n… (tronqué)';
    let fence = '```';
    while (body.includes(fence)) fence += '`';
    const head = `Résultat de l'outil MCP \`${server}.${tool}\`${result.isError ? ' (erreur)' : ''} :`;
    oninsert(`${head}\n${fence}\n${body}\n${fence}`);
    onclose();
  }

  function copy() {
    navigator.clipboard
      ?.writeText(result?.text ?? '')
      .then(() => {
        copied = true;
        setTimeout(() => (copied = false), 1400);
      })
      .catch(() => {});
  }

  function onKey(e) {
    if (e.key === 'Escape') onclose();
  }

  function onArgsKey(e) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (!running && tl) run();
    }
  }

  function pickServer(e) {
    server = e.currentTarget.value;
    tool = '';
    choose();
  }

  function pickTool(e) {
    tool = e.currentTarget.value;
    choose();
  }

  function pickKey(e) {
    codeKey = e.currentTarget.value;
    remember();
    rebuild();
  }

  onMount(load);
</script>

<svelte:window onkeydown={onKey} />

<div
  class="ov"
  role="presentation"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}
>
  <div class="box" role="dialog" aria-modal="true" aria-label="Exécuter via MCP">
    <header>
      <b>Exécuter via MCP</b>
      <span class="badge mcp">MCP</span>
      <span class="sp"></span>
      <button class="x" type="button" aria-label="Fermer" onclick={onclose}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </header>

    <div class="body">
      {#if loading}
        <p class="dim">Chargement…</p>
      {:else}
        {#if loadErr}
          <div class="error">{loadErr}</div>
        {/if}

        {#if !servers.length}
          <p class="dim">Aucun serveur MCP configuré. Ajoute-en un dans Réglages › MCP.</p>
        {:else}
          <div class="row">
            <label>
              <span>Serveur</span>
              <select value={server} onchange={pickServer}>
                {#each servers as s (s.name)}
                  <option value={s.name}>{s.name}{s.connected ? '' : ' (arrêté)'}</option>
                {/each}
              </select>
            </label>

            {#if srv?.connected && tools.length}
              <label>
                <span>Outil</span>
                <select value={tool} onchange={pickTool}>
                  {#each tools as t (t.name)}
                    <option value={t.name}>{t.name}</option>
                  {/each}
                </select>
              </label>
            {/if}
          </div>

          {#if !srv?.connected}
            <div class="row">
              <button class="primary" type="button" disabled={connecting} onclick={connect}>
                {connecting ? 'Connexion…' : 'Connecter ' + server}
              </button>
              <span class="dim">Le serveur est arrêté.</span>
            </div>
          {:else if !tools.length}
            <p class="dim">Ce serveur n'expose aucun outil.</p>
          {:else}
            {#if tl?.description}
              <p class="desc">
                {#if tl.readOnly}<span class="badge ro">lecture seule</span>{/if}
                {tl.description}
              </p>
            {/if}

            {#if strKeys.length > 1}
              <label class="inl">
                <span>Placer le code dans</span>
                <select value={codeKey} onchange={pickKey}>
                  {#each strKeys as k (k)}
                    <option value={k}>{k}</option>
                  {/each}
                </select>
              </label>
            {:else if !strKeys.length}
              <p class="dim">Cet outil n'a pas d'argument texte : écris les arguments à la main.</p>
            {/if}

            <label>
              <span>Arguments (JSON) · Ctrl+Entrée pour exécuter</span>
              <textarea rows="9" spellcheck="false" bind:value={argsText} onkeydown={onArgsKey}></textarea>
            </label>

            <div class="row">
              <button class="primary" type="button" disabled={running || !tl} onclick={run}>
                {running ? 'Exécution…' : 'Exécuter'}
              </button>
              <span class="dim">Cliquer = confirmer cet appel.</span>
            </div>
          {/if}
        {/if}

        {#if err}
          <div class="error">{err}</div>
        {/if}

        {#if result}
          <div class="res" class:bad={result.isError}>
            <div class="rh">
              <span class="badge" class:ro={!result.isError}>{result.isError ? 'erreur' : 'résultat'}</span>
              <span class="sp"></span>
              <button class="mini" type="button" onclick={copy}>{copied ? 'Copié' : 'Copier'}</button>
              {#if oninsert}
                <button class="mini primary" type="button" onclick={insert}>Insérer dans le message</button>
              {/if}
            </div>
            <pre>{result.text || '(réponse vide)'}</pre>
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .ov {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: rgba(5, 7, 10, 0.72);
    animation: fs-fade var(--t) var(--ease) both;
  }
  .box {
    width: min(720px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-top: 2px solid var(--accent);
    box-shadow: var(--shadow);
    animation: fs-pop var(--t-slow) var(--ease) both;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--line);
  }
  .sp {
    flex: 1;
  }
  .body {
    padding: 14px 16px 16px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  label span {
    font: 11px var(--mono);
    letter-spacing: 0.4px;
    text-transform: uppercase;
    color: var(--faint);
  }
  label.inl {
    flex-direction: row;
    align-items: center;
    gap: 10px;
  }
  .row label {
    flex: 1 1 200px;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    font: 12.5px/1.5 var(--mono);
    background: var(--bg);
    min-height: 120px;
  }
  .dim,
  .desc {
    margin: 0;
    color: var(--dim);
    font-size: 13px;
    line-height: 1.5;
  }
  .desc {
    border-left: 2px solid var(--line);
    padding-left: 10px;
  }
  .res {
    border: 1px solid var(--line);
    border-left: 3px solid var(--ok);
    background: var(--bg);
    animation: fs-rise var(--t) var(--ease) both;
  }
  .res.bad {
    border-left-color: var(--danger);
  }
  .rh {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--line);
  }
  .res pre {
    margin: 0;
    padding: 10px 12px;
    max-height: 280px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: 12.5px/1.55 var(--mono);
  }
  .x {
    padding: 2px 8px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--dim);
  }
  .x:hover:not(:disabled) {
    transform: none;
    box-shadow: none;
    color: var(--text);
    border-color: var(--line);
  }
  @media (prefers-reduced-motion: reduce) {
    .ov,
    .box,
    .res {
      animation: none;
    }
  }
</style>
