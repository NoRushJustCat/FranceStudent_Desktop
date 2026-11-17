<script>
  // Pastille flottante + panneau du pont MCP (serveur local, tunnel, enregistrement France Student).
  // Autonome : ne dépend d'aucun autre fichier de l'app, seulement des commandes Tauri `bridge_*`.
  import { invoke } from '@tauri-apps/api/core';

  let { docked = false, inline = false } = $props();

  const POS_KEY = 'fscode.bridge.pos';
  const PHASES = { stopped: 'arrêté', starting: 'démarrage', running: 'actif', error: 'erreur' };
  const EMPTY = {
    phase: 'stopped',
    step: '',
    mode: 'quick',
    url: null,
    port: null,
    tools: 0,
    servers: [],
    entries: [],
    error: null,
    warnings: [],
    registerNote: null,
    hits: 0,
    log: [],
    autoAllow: false,
    autoStart: false,
    confirmReadOnly: true,
    maxTools: 64,
    namedReady: false,
  };

  function loadPos() {
    try {
      const v = JSON.parse(localStorage.getItem(POS_KEY) ?? 'null');
      if (v && Number.isFinite(v.x) && Number.isFinite(v.y)) return { x: v.x, y: v.y };
    } catch {
      /* position illisible : on retombe sur le défaut */
    }
    return null;
  }

  let s = $state({ ...EMPTY });
  let open = $state(false);
  let busy = $state(false);
  let fault = $state('');
  let winW = $state(1280);
  let winH = $state(800);
  let root = $state(null);
  let pos = $state(loadPos()); // null = bas-droite ; sinon coin haut-gauche en px
  let urlCopied = $state(false);
  let confirmDangerous = $state(false);
  let modeDraft = $state('quick');
  let publicDraft = $state('');
  let tokenDraft = $state('');
  let accessId = $state('');
  let accessSecret = $state('');
  let modeTouched = $state(false);
  let drag = null; // non réactif : état du glissement en cours
  let copyTimer = null;

  async function copyUrl() {
    if (!s.url) return;
    try {
      await navigator.clipboard.writeText(s.url);
      urlCopied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (urlCopied = false), 1800);
    } catch {
      /* presse-papiers indisponible */
    }
  }

  const active = $derived(s.phase === 'running');
  const tone = $derived(
    s.phase === 'running' ? 'ok' : s.phase === 'starting' ? 'warn' : s.phase === 'error' ? 'danger' : 'faint',
  );
  const label = $derived.by(() => {
    if (s.phase === 'running') {
      const srv = s.servers.length ? `${s.servers.length} serveur${s.servers.length > 1 ? 's' : ''} · ` : '';
      return `Pont MCP · ${srv}${s.tools} outil${s.tools > 1 ? 's' : ''}`;
    }
    if (s.phase === 'starting') return `Pont MCP · ${s.step || 'démarrage'}…`;
    if (s.phase === 'error') return 'Pont MCP · erreur';
    return 'Pont MCP · arrêté';
  });
  const place = $derived(pos ? `left:${pos.x}px;top:${pos.y}px` : 'right:12px;bottom:12px');
  const above = $derived(pos ? pos.y > winH / 2 : true);
  const alignRight = $derived(pos ? pos.x > winW / 2 - 100 : true);
  const logText = $derived([...s.log].reverse().join('\n'));

  function clampPos(x, y) {
    const cw = root?.offsetWidth ?? 160;
    const ch = root?.offsetHeight ?? 28;
    return {
      x: Math.round(Math.min(Math.max(0, x), Math.max(0, winW - cw))),
      y: Math.round(Math.min(Math.max(0, y), Math.max(0, winH - ch))),
    };
  }

  // Fenêtre redimensionnée : on garde la pastille visible.
  $effect(() => {
    if (!pos) return;
    const c = clampPos(pos.x, pos.y);
    if (c.x !== pos.x || c.y !== pos.y) pos = c;
  });

  function savePos() {
    try {
      if (pos) localStorage.setItem(POS_KEY, JSON.stringify({ x: pos.x, y: pos.y }));
      else localStorage.removeItem(POS_KEY);
    } catch {
      /* stockage indisponible : tant pis */
    }
  }

  function down(e) {
    if (e.button !== 0) return;
    const r = root.getBoundingClientRect();
    drag = { dx: e.clientX - r.left, dy: e.clientY - r.top, sx: e.clientX, sy: e.clientY, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  }

  function move(e) {
    if (!drag) return;
    if (!drag.moved && Math.hypot(e.clientX - drag.sx, e.clientY - drag.sy) < 4) return;
    drag.moved = true;
    pos = clampPos(e.clientX - drag.dx, e.clientY - drag.dy);
  }

  function up(e) {
    if (!drag) return;
    const moved = drag.moved;
    drag = null;
    try {
      e.currentTarget.releasePointerCapture(e.pointerId);
    } catch {
      /* capture déjà libérée */
    }
    if (moved) savePos();
    else open = !open;
  }

  function cancel() {
    drag = null;
  }

  // Clavier (Entrée/Espace) : détail 0. Les clics souris sont gérés par `up`.
  function keyClick(e) {
    if (e.detail === 0) open = !open;
  }

  function reset() {
    pos = null;
    savePos();
  }

  function onKey(e) {
    if (open && e.key === 'Escape') open = false;
  }

  async function refresh() {
    try {
      s = await invoke('bridge_status');
      fault = '';
      if (!modeTouched) {
        modeDraft = s.mode || 'quick';
        publicDraft = s.publicUrl || '';
      }
    } catch (e) {
      fault = String(e);
    }
  }

  // Interrogation régulière : plus vite quand le panneau est ouvert.
  $effect(() => {
    const every = open || inline ? 1500 : 3000;
    refresh();
    const t = setInterval(refresh, every);
    return () => clearInterval(t);
  });

  async function run(cmd, args) {
    if (busy) return;
    busy = true;
    fault = '';
    try {
      s = await invoke(cmd, args);
    } catch (e) {
      fault = String(e);
      await refresh();
    } finally {
      busy = false;
    }
  }

  const toggle = () => run(active ? 'bridge_stop' : 'bridge_start');
  const clean = () => run('bridge_stop');

  async function setOpt(patch) {
    try {
      s = await invoke('bridge_set', {
        autoStart: patch.autoStart ?? null,
        autoAllow: patch.autoAllow ?? null,
        confirmReadOnly: patch.confirmReadOnly ?? null,
        mode: patch.mode ?? null,
        publicUrl: patch.publicUrl ?? null,
      });
      fault = '';
      if (patch.autoAllow === false) confirmDangerous = false;
      if (!modeTouched) {
        modeDraft = s.mode || 'quick';
        publicDraft = s.publicUrl || '';
      }
    } catch (e) {
      fault = String(e);
      await refresh();
    }
  }

  async function saveMode() {
    modeTouched = false;
    await setOpt({ mode: modeDraft, publicUrl: publicDraft });
  }

  async function saveToken() {
    try {
      s = await invoke('bridge_set_token', { token: tokenDraft });
      tokenDraft = '';
      fault = '';
    } catch (e) {
      fault = String(e);
    }
  }

  async function saveAccess() {
    try {
      s = await invoke('bridge_set_access', { clientId: accessId, clientSecret: accessSecret });
      accessId = '';
      accessSecret = '';
      fault = '';
    } catch (e) {
      fault = String(e);
    }
  }

  function onAutoAllowChange(e) {
    if (s.autoAllow) {
      setOpt({ autoAllow: false });
    } else {
      e.currentTarget.checked = false;
      confirmDangerous = true;
    }
  }
</script>

<svelte:window bind:innerWidth={winW} bind:innerHeight={winH} onkeydown={onKey} />

<div class="bridge" class:docked class:inline bind:this={root} style={docked || inline ? '' : place}>
  {#if !inline}
  <button
    class="chip"
    type="button"
    aria-expanded={open}
    title={docked ? 'Pont MCP · cliquer pour ouvrir' : 'Pont MCP · glisser pour déplacer · double-clic : position d’origine'}
    onpointerdown={docked ? undefined : down}
    onpointermove={docked ? undefined : move}
    onpointerup={docked ? undefined : up}
    onpointercancel={docked ? undefined : cancel}
    onclick={docked ? () => (open = !open) : keyClick}
    ondblclick={docked ? undefined : reset}
  >
    <span class="dot {tone}" aria-hidden="true"></span>
    <span class="txt">{label}</span>
  </button>
  {/if}

  {#if open || inline}
    <div class="panel {inline ? 'inline' : docked ? 'down right' : (above ? 'up' : 'down') + ' ' + (alignRight ? 'right' : 'left')}">
      <div class="panel-scroll">
        <div class="head">
          <strong>Pont MCP</strong>
          <div class="head-right">
            <span class="pill {tone}">{PHASES[s.phase] ?? s.phase}</span>
            {#if active}
              <button
                class="btn compact danger"
                type="button"
                disabled={busy}
                onclick={toggle}
              >
                Arrêter
              </button>
            {/if}
          </div>
        </div>

        {#if s.phase === 'starting'}
          <p class="muted">Étape : {s.step || '…'}</p>
        {/if}

        {#if !active}
          <div class="row">
            <button
              class="btn primary"
              type="button"
              disabled={busy || s.phase === 'starting'}
              onclick={toggle}
            >
              {s.phase === 'starting' ? 'Démarrage…' : 'Démarrer'}
            </button>
            <button
              class="btn"
              type="button"
              disabled={busy || s.phase === 'starting'}
              onclick={clean}
              title="Supprime l’entrée périmée restée chez France Student"
            >
              Nettoyer
            </button>
          </div>
        {/if}

      <dl class="info">
        {#if s.url}
          <dt>URL</dt>
          <dd class="url-row">
            <code class="url-code" title={s.url}>{s.url}</code>
            <button
              type="button"
              class="copy-mini"
              title={urlCopied ? 'Copiée' : 'Copier l’URL'}
              aria-label={urlCopied ? 'URL copiée' : 'Copier l’URL'}
              onclick={copyUrl}
            >
              {#if urlCopied}
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3.5 8.5 6.5 11.5 12.5 5" /></svg>
              {:else}
                <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5.5" y="5.5" width="7.5" height="7.5" rx="1.5" /><path d="M3.5 10.5H3a1.5 1.5 0 0 1-1.5-1.5V3A1.5 1.5 0 0 1 3 1.5h6A1.5 1.5 0 0 1 10.5 3v.5" /></svg>
              {/if}
            </button>
          </dd>
        {/if}
        {#if s.port}
          <dt>Port</dt>
          <dd><code>{s.port}</code></dd>
        {/if}
        <dt>Mode</dt>
        <dd>{s.mode === 'named' ? 'nommé (domaine Cloudflare)' : 'rapide (trycloudflare.com)'}</dd>
        <dt>Serveurs</dt>
        <dd>{s.servers.length ? s.servers.join(', ') : '—'}</dd>
        <dt>Outils</dt>
        <dd>{s.tools}{s.maxTools ? ` (max ${s.maxTools} par serveur)` : ''}</dd>
        <dt>Requêtes</dt>
        <dd>{s.hits}</dd>
      </dl>

      {#if s.entries?.length}
        <div class="entries">
          {#each s.entries as e}
            <div class="entry" class:bad={!!e.error}>
              <div class="entry-head">
                <b>{e.server}</b>
                <span class="muted">{e.tools} outil{e.tools > 1 ? 's' : ''}</span>
              </div>
              <code class="entry-url">{e.url}</code>
              {#if e.serverId}<span class="muted">id FS : {e.serverId}</span>{/if}
              {#if e.error}<span class="msg danger">{e.error}</span>{/if}
            </div>
          {/each}
        </div>
      {/if}

      <div class="sec">
        <div class="sec-title">Tunnel</div>
        <select bind:value={modeDraft} onchange={() => (modeTouched = true)} disabled={active}>
          <option value="quick">Rapide (trycloudflare.com)</option>
          <option value="named">Nommé (domaine Cloudflare)</option>
        </select>
        {#if modeDraft === 'named'}
          <input
            type="url"
            placeholder="https://fscode.example.com"
            bind:value={publicDraft}
            oninput={() => (modeTouched = true)}
            disabled={active}
          />
          <div class="row">
            <input
              type="password"
              placeholder={s.namedReady ? 'Jeton déjà dans le coffre' : 'Jeton du tunnel'}
              bind:value={tokenDraft}
            />
            <button class="btn" type="button" disabled={!tokenDraft.trim()} onclick={saveToken}>
              Jeton
            </button>
          </div>
          <details>
            <summary>Cloudflare Access (optionnel)</summary>
            <div class="row">
              <input placeholder="Client ID" bind:value={accessId} />
              <input type="password" placeholder="Client Secret" bind:value={accessSecret} />
            </div>
            <button class="btn" type="button" onclick={saveAccess}>Enregistrer Access</button>
          </details>
        {/if}
        <div class="row">
          <button
            class="btn"
            type="button"
            disabled={active || (modeDraft === 'named' && !publicDraft.trim())}
            onclick={saveMode}
          >
            Appliquer le mode
          </button>
        </div>
        <p class="muted">Le jeton reste dans le coffre du système, jamais dans bridge.json.</p>
      </div>

        <label class="opt">
          <input
            type="checkbox"
            checked={s.autoStart}
            onchange={(e) => setOpt({ autoStart: e.currentTarget.checked })}
          />
          Démarrer avec l’app
        </label>
        <label class="opt">
          <input
            type="checkbox"
            checked={s.confirmReadOnly}
            onchange={(e) => setOpt({ confirmReadOnly: e.currentTarget.checked })}
          />
          Confirmer aussi les outils en lecture seule
        </label>
        <label class="opt opt-danger" class:bad={s.autoAllow}>
          <input
            type="checkbox"
            checked={s.autoAllow}
            onchange={onAutoAllowChange}
          />
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
          <span>Autoriser sans confirmation pour cette session (dangereux)</span>
        </label>
      {#if confirmDangerous && !s.autoAllow}
        <div class="opt bad opt-confirm">
          <span>Exécuter tous les outils MCP sans demander&nbsp;? (jusqu’au redémarrage)</span>
          <div class="row">
            <button type="button" class="btn danger" onclick={() => setOpt({ autoAllow: true })}>Confirmer</button>
            <button type="button" class="btn" onclick={() => (confirmDangerous = false)}>Annuler</button>
          </div>
        </div>
      {/if}

      {#if s.error}
        <div class="msg danger">{s.error}</div>
      {/if}
      {#if fault}
        <div class="msg danger">{fault}</div>
      {/if}
      {#each s.warnings as note}
        <div class="msg warn">{note}</div>
      {/each}

      {#if s.registerNote}
        <details>
          <summary>
            <svg class="chev" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 2.5 8 6 4 9.5" /></svg>
            Réponse France Student
          </summary>
          <pre>{s.registerNote}</pre>
        </details>
      {/if}
      <details>
        <summary>
          <svg class="chev" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 2.5 8 6 4 9.5" /></svg>
          Journal ({s.log.length})
        </summary>
        <pre>{logText || '(vide)'}</pre>
      </details>
      </div>
    </div>
  {/if}
</div>

<style>
  .bridge {
    position: fixed;
    z-index: 9999;
    font-family: var(--sans, system-ui, sans-serif);
    font-size: 12px;
    color: var(--text, #e6e6e6);
  }

  .bridge.docked {
    position: relative;
    z-index: 45;
  }

  .bridge.docked .chip {
    cursor: pointer;
    border-radius: 999px;
    padding: 4px 10px;
    background: var(--panel2, #121a2a);
    border-color: var(--line, #202c44);
  }

  .bridge.docked .chip:hover,
  .bridge.docked .chip[aria-expanded='true'] {
    background: var(--hover, #1f2c46);
    border-color: var(--accent-line, var(--line2));
  }

  .bridge.inline {
    position: static;
    z-index: auto;
    font-size: 13px;
  }

  .panel.inline {
    position: static;
    width: auto;
    box-shadow: none;
    background: var(--panel2, #121a2a);
  }

  .panel.inline .panel-scroll {
    max-height: none;
    overflow: visible;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: 300px;
    padding: 4px 10px;
    background: var(--panel, #161616);
    color: inherit;
    border: 1px solid var(--line2, #3a3a3a);
    border-radius: 999px;
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    cursor: grab;
    user-select: none;
    touch-action: none;
  }
  .chip:hover {
    background: var(--raised, #202020);
  }
  .chip:active {
    cursor: grabbing;
  }
  .txt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--faint, #777);
  }
  .dot.ok {
    background: var(--ok, #3fb950);
  }
  .dot.warn {
    background: var(--warn, #d29922);
  }
  .dot.danger {
    background: var(--danger, #f85149);
  }

  .panel {
    position: absolute;
    box-sizing: border-box;
    width: min(350px, calc(100vw - 24px));
    background: var(--panel, #161616);
    border: 1px solid var(--line2, #3a3a3a);
    border-radius: var(--radius-sm, 10px);
    box-shadow: 0 16px 44px rgba(0, 0, 0, 0.54), 0 0 0 1px rgba(0, 0, 0, 0.16);
  }
  .panel-scroll {
    max-height: min(520px, calc(100vh - 60px));
    overflow: auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .panel.down.right::before {
    content: '';
    position: absolute;
    top: -6px;
    right: 22px;
    width: 10px;
    height: 10px;
    background: var(--panel, #161616);
    border-top: 1px solid var(--line2, #3a3a3a);
    border-left: 1px solid var(--line2, #3a3a3a);
    transform: rotate(45deg);
    pointer-events: none;
  }
  .panel.up {
    bottom: calc(100% + 6px);
  }
  .panel.down {
    top: calc(100% + 9px);
  }
  .panel.right {
    right: 0;
  }
  .panel.left {
    left: 0;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .head-right {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .btn.compact {
    padding: 3px 9px;
    font-size: 11.5px;
    line-height: 1.3;
  }
  .pill {
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--line2, #3a3a3a);
    color: var(--dim, #aaa);
    font-family: var(--font-sans, inherit);
    font-size: 11px;
    font-weight: 500;
  }
  .pill.ok {
    color: var(--ok, #3fb950);
    border-color: var(--ok, #3fb950);
  }
  .pill.warn {
    color: var(--warn, #d29922);
    border-color: var(--warn, #d29922);
  }
  .pill.danger {
    color: var(--danger, #f85149);
    border-color: var(--danger, #f85149);
  }

  .muted {
    margin: 0;
    color: var(--dim, #aaa);
  }

  .row {
    display: flex;
    gap: 6px;
  }
  .btn {
    padding: 4px 10px;
    background: var(--panel2, #1c1c1c);
    color: var(--text, #e6e6e6);
    border: 1px solid var(--line2, #3a3a3a);
    border-radius: var(--r, 0px);
    font: inherit;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    background: var(--raised, #202020);
  }
  .btn.primary {
    background: var(--accent, #4c9aff);
    border-color: var(--accent, #4c9aff);
    color: var(--on-accent, #ffffff);
  }
  .btn.danger {
    border-color: color-mix(in srgb, var(--danger, #f85149) 45%, var(--line2));
    color: var(--danger, #f85149);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .info {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 10px;
    margin: 0;
    align-items: center;
  }
  .info dt {
    color: var(--dim, #aaa);
  }
  .info dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .url-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .url-code {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1 1 auto;
  }
  .copy-mini {
    flex: none;
    width: 24px;
    height: 24px;
    padding: 0;
    display: inline-grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid var(--line2);
    background: var(--panel2);
    color: var(--dim);
    cursor: pointer;
  }
  .copy-mini:hover {
    color: var(--text);
    border-color: var(--accent-line);
  }
  code {
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 11px;
    user-select: text;
  }

  .opt {
    display: flex;
    align-items: center;
    gap: 7px;
    cursor: pointer;
  }
  .opt-danger {
    border-top: 1px solid var(--line);
    padding-top: 9px;
    margin-top: 2px;
    color: var(--danger, #f85149);
    font-weight: 500;
  }
  .opt.bad {
    margin-top: 2px;
    padding: 7px 9px;
    border-radius: 7px;
    border: 1px solid color-mix(in srgb, var(--danger, #f85149) 35%, transparent);
    background: var(--danger-soft, rgba(248, 81, 73, 0.1));
    color: var(--danger, #f85149);
    font-size: 11.5px;
  }
  .opt-confirm {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    cursor: default;
  }

  .msg {
    padding: 6px 8px;
    border: 1px solid var(--line2, #3a3a3a);
    border-radius: 7px;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .msg.danger {
    color: var(--danger, #f85149);
    border-color: var(--danger, #f85149);
  }
  .msg.warn {
    color: var(--warn, #d29922);
    border-color: var(--warn, #d29922);
  }

  summary {
    display: flex;
    align-items: center;
    gap: 6px;
    list-style: none;
    cursor: pointer;
    color: var(--dim, #aaa);
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary .chev {
    width: 12px;
    height: 12px;
    margin-left: -2px;
    flex: none;
    transition: transform 0.15s ease;
  }
  details[open] > summary .chev {
    transform: rotate(90deg);
  }
  pre {
    margin: 6px 0 0;
    padding: 6px;
    max-height: 160px;
    overflow: auto;
    background: var(--bg, #0e0e0e);
    border: 1px solid var(--line, #2a2a2a);
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 11px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
</style>
