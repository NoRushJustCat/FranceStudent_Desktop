<script>
  // Pastille flottante + panneau du pont MCP (serveur local, tunnel, enregistrement France Student).
  // Autonome : ne dépend d'aucun autre fichier de l'app, seulement des commandes Tauri `bridge_*`.
  import { invoke } from '@tauri-apps/api/core';

  const POS_KEY = 'fscode.bridge.pos';
  const PHASES = { stopped: 'arrêté', starting: 'démarrage', running: 'actif', error: 'erreur' };
  const EMPTY = {
    phase: 'stopped',
    step: '',
    url: null,
    serverId: null,
    port: null,
    tools: 0,
    servers: [],
    error: null,
    warnings: [],
    registerNote: null,
    hits: 0,
    log: [],
    autoAllow: false,
    autoStart: false,
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
  let drag = null; // non réactif : état du glissement en cours

  const active = $derived(s.phase === 'running');
  const tone = $derived(
    s.phase === 'running' ? 'ok' : s.phase === 'starting' ? 'warn' : s.phase === 'error' ? 'danger' : 'faint',
  );
  const label = $derived.by(() => {
    if (s.phase === 'running') return `Pont MCP · actif · ${s.tools} outil${s.tools > 1 ? 's' : ''}`;
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
    } catch (e) {
      fault = String(e);
    }
  }

  // Interrogation régulière : plus vite quand le panneau est ouvert.
  $effect(() => {
    const every = open ? 1500 : 3000;
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

  async function setOpt(autoStart, autoAllow) {
    try {
      s = await invoke('bridge_set', { autoStart, autoAllow });
      fault = '';
    } catch (e) {
      fault = String(e);
      await refresh();
    }
  }
</script>

<svelte:window bind:innerWidth={winW} bind:innerHeight={winH} onkeydown={onKey} />

<div class="bridge" bind:this={root} style={place}>
  <button
    class="chip"
    type="button"
    aria-expanded={open}
    title="Pont MCP · glisser pour déplacer · double-clic : position d’origine"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={cancel}
    onclick={keyClick}
    ondblclick={reset}
  >
    <span class="dot {tone}"></span>
    <span class="txt">{label}</span>
  </button>

  {#if open}
    <div class="panel {above ? 'up' : 'down'} {alignRight ? 'right' : 'left'}">
      <div class="head">
        <strong>Pont MCP</strong>
        <span class="pill {tone}">{PHASES[s.phase] ?? s.phase}</span>
      </div>

      {#if s.phase === 'starting'}
        <p class="muted">Étape : {s.step || '…'}</p>
      {/if}

      <div class="row">
        <button class="btn primary" type="button" disabled={busy || s.phase === 'starting'} onclick={toggle}>
   {active ? 'Arrêter' : s.phase === 'starting' ? 'Démarrage…' : 'Démarrer'}
        </button>
        {#if !active}
   <button
     class="btn"
     type="button"
     disabled={busy || s.phase === 'starting'}
     onclick={clean}
     title="Supprime l’entrée périmée restée chez France Student"
          >
            Nettoyer
          </button>
        {/if}
      </div>

      <dl class="info">
        {#if s.url}
   <dt>URL</dt>
   <dd><code>{s.url}</code></dd>
 {/if}
 {#if s.serverId}
   <dt>ID FS</dt>
          <dd><code>{s.serverId}</code></dd>
 {/if}
        {#if s.port}
          <dt>Port</dt>
   <dd>{s.port}</dd>
 {/if}
        <dt>Serveurs</dt>
 <dd>{s.servers.length ? s.servers.join(', ') : '—'}</dd>
        <dt>Outils</dt>
 <dd>{s.tools}</dd>
 <dt>Requêtes</dt>
 <dd>{s.hits}</dd>
      </dl>

      <label class="opt">
 <input
          type="checkbox"
          checked={s.autoStart}
          onchange={(e) => setOpt(e.currentTarget.checked, null)}
        />
        Démarrer avec l’app
      </label>
      <label class="opt" class:bad={s.autoAllow}>
        <input
   type="checkbox"
   checked={s.autoAllow}
   onchange={(e) => setOpt(null, e.currentTarget.checked)}
        />
 Autoriser sans confirmation (dangereux)
      </label>

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
   <summary>Réponse France Student</summary>
          <pre>{s.registerNote}</pre>
 </details>
      {/if}
      <details>
 <summary>Journal ({s.log.length})</summary>
        <pre>{logText || '(vide)'}</pre>
      </details>
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

  .chip {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 300px;
    padding: 4px 8px;
    background: var(--panel, #161616);
    color: inherit;
    border: 1px solid var(--line2, #3a3a3a);
    border-radius: var(--r, 0px);
    font: inherit;
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 11px;
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
    width: min(340px, calc(100vw - 24px));
    max-height: min(520px, calc(100vh - 60px));
    overflow: auto;
    padding: 10px;
    background: var(--panel, #161616);
    border: 1px solid var(--line2, #3a3a3a);
    border-radius: var(--r, 0px);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .panel.up {
    bottom: calc(100% + 6px);
  }
  .panel.down {
    top: calc(100% + 6px);
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
  .pill {
    padding: 1px 6px;
    border: 1px solid var(--line2, #3a3a3a);
    color: var(--dim, #aaa);
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 11px;
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
    border-color: var(--accent, #4c9aff);
    color: var(--accent, #4c9aff);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .info {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 10px;
    margin: 0;
  }
  .info dt {
    color: var(--dim, #aaa);
  }
  .info dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  code {
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 11px;
    user-select: text;
  }

  .opt {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }
  .opt.bad {
    color: var(--danger, #f85149);
  }

  .msg {
    padding: 6px 8px;
    border: 1px solid var(--line2, #3a3a3a);
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
    cursor: pointer;
    color: var(--dim, #aaa);
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
