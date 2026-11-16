<script>
  // Petit bouton « Agents » : visible seulement quand un agent travaille.
  // Il ouvre un panneau qui montre en direct ce que font les agents (voir agentlive.svelte.js).
  import { onMount, tick } from 'svelte';
  import { live, view, init, cancel, cancelAll, clearFinished, allow } from './agentlive.svelte.js';

  let open = $state(false);
  let sel = $state(''); // id de l'agent affiché ('' = automatique)
  let now = $state(Date.now());
  let follow = $state(true); // suit le bas du flux tant que l'utilisateur ne remonte pas
  let body = $state();
  let copied = $state(false);
  let err = $state('');

  onMount(() => {
    init();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const current = $derived(
    live.agents.find((a) => a.id === sel) ??
      live.agents.find((a) => a.status === 'running') ??
      live.agents[live.agents.length - 1] ??
      null
  );
  const feed = $derived(current ? (live.feeds[current.id] ?? []) : []);
  const hasFinished = $derived(live.agents.some((a) => a.status !== 'running'));

  // Suivi automatique du bas du flux.
  $effect(() => {
    const f = feed;
    void f.length;
    const last = f[f.length - 1];
    void last?.text?.length;
    void last?.result;
    if (open && follow && body) {
      tick().then(() => {
        if (body) body.scrollTop = body.scrollHeight;
      });
    }
  });

  // Échap ferme d'abord le panneau (sans fermer les paramètres).
  $effect(() => {
    if (!open) return;
    const h = (e) => {
      if (e.key === 'Escape') {
        e.stopImmediatePropagation();
        e.preventDefault();
        open = false;
      }
    };
    window.addEventListener('keydown', h, true);
    return () => window.removeEventListener('keydown', h, true);
  });

  function onScroll() {
    if (!body) return;
    follow = body.scrollHeight - body.scrollTop - body.clientHeight < 40;
  }

  const STATUS = { running: 'En cours', done: 'Terminé', failed: 'Échec', cancelled: 'Annulé' };

  function dur(a) {
    if (!a?.started) return '';
    const end = a.status === 'running' || !a.ended ? now : a.ended;
    const s = Math.max(0, Math.round((end - a.started) / 1000));
    return s >= 60 ? `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, '0')} s` : `${s} s`;
  }

  function fmtArgs(a) {
    if (a == null) return '';
    if (typeof a === 'string') return a;
    try {
      return JSON.stringify(a, null, 2);
    } catch {
      return String(a);
    }
  }

  function toolState(b) {
    if (b.result) {
      if (b.result.denied) return { label: 'Refusé', cls: 'bad' };
      if (b.result.error) return { label: 'Erreur', cls: 'bad' };
      return { label: 'OK', cls: 'ok' };
    }
    return b.confirm ? { label: 'Autorisation demandée', cls: 'wait' } : { label: 'En cours', cls: 'run' };
  }

  const agentName = (id) => live.agents.find((a) => a.id === id)?.name || id;

  async function run(fn) {
    err = '';
    try {
      await fn();
    } catch (e) {
      err = String(e?.message ?? e);
    }
  }

  async function copyPath() {
    if (!current?.file) return;
    try {
      await navigator.clipboard.writeText(current.file);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch {
      copied = false;
    }
  }
</script>

{#if view.active || open}
  <button
    type="button"
    class="al-btn"
    class:wait={live.waiting.length > 0}
    title="Voir les agents en direct"
    aria-label="Voir les agents en direct"
    onclick={() => (open = !open)}
  >
    <span class="al-dot" class:pulse={view.count > 0}></span>
    <span>Agents</span>
    {#if view.count > 0}<span class="al-badge">{view.count}</span>{/if}
  </button>
{/if}

{#if open}
  <button type="button" class="al-back" aria-label="Fermer le panneau des agents" onclick={() => (open = false)}></button>
  <section class="al-panel" role="dialog" aria-label="Agents en direct">
    <header class="al-head">
      <div class="al-title">
        <strong>Agents en direct</strong>
        <span class="al-sub">
          {view.count > 0 ? `${view.count} en cours` : 'Aucun agent en cours'} · {live.agents.length} au total
        </span>
      </div>
      <div class="al-actions">
        {#if view.count > 0}
          <button type="button" class="al-ghost danger" onclick={() => run(cancelAll)}>Tout annuler</button>
        {/if}
        {#if hasFinished}
          <button type="button" class="al-ghost" onclick={() => run(clearFinished)}>Effacer les terminés</button>
        {/if}
        <button type="button" class="al-x" aria-label="Fermer" onclick={() => (open = false)}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
    </header>

    {#if live.error}<div class="al-err">{live.error}</div>{/if}
    {#if err}<div class="al-err">{err}</div>{/if}

    {#if live.waiting.length > 0}
      <div class="al-wait">
        {#each live.waiting as w (w.uid)}
          <div class="al-wait-row">
            <div class="al-wait-txt">
              <strong>{agentName(w.agent)}</strong> demande l’autorisation d’utiliser <code>{w.name}</code>
              {#if fmtArgs(w.args)}<pre class="al-pre">{fmtArgs(w.args)}</pre>{/if}
            </div>
            <div class="al-wait-btns">
              <button type="button" class="al-btn-ok" onclick={() => run(() => allow(w.uid, true))}>Autoriser</button>
              <button type="button" class="al-ghost" onclick={() => run(() => allow(w.uid, false))}>Refuser</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if live.agents.length === 0}
      <div class="al-empty">Aucun agent pour le moment.</div>
    {:else}
      <div class="al-tabs" role="tablist">
        {#each live.agents as a (a.id)}
          <button
            type="button"
            role="tab"
            aria-selected={current?.id === a.id}
            class="al-tab"
            class:on={current?.id === a.id}
            onclick={() => {
              sel = a.id;
              follow = true;
            }}
          >
            <span class="al-st {a.status}"></span>
            <span class="al-tab-name">{a.name || a.id}</span>
          </button>
        {/each}
      </div>

      {#if current}
        <div class="al-meta">
          <div class="al-meta-top">
            <span class="al-pill {current.status}">{STATUS[current.status] ?? current.status}</span>
            <span>{dur(current)}</span>
            {#if current.role}<span>· {current.role}</span>{/if}
            {#if current.wave}<span>· vague {current.wave}</span>{/if}
            {#if current.model}<span>· {current.model}</span>{/if}
            {#if current.steps}<span>· {current.steps} étape{current.steps > 1 ? 's' : ''}</span>{/if}
            {#if current.status === 'running'}
              <button type="button" class="al-ghost danger sm" onclick={() => run(() => cancel(current.id))}>Annuler</button>
            {/if}
          </div>
          {#if current.task}<div class="al-task">{current.task}</div>{/if}
          {#if current.last && current.status === 'running'}<div class="al-last">Dernière action : {current.last}</div>{/if}
        </div>

        <div class="al-body" bind:this={body} onscroll={onScroll}>
          {#if feed.length === 0}
            <div class="al-empty">En attente de la première réponse…</div>
          {/if}
          {#each feed as b (b.key)}
            {#if b.type === 'text'}
              <div class="al-blk al-text">{b.text}</div>
            {:else if b.type === 'reasoning'}
              <div class="al-blk al-reason"><span class="al-lbl">Réflexion</span>{b.text}</div>
            {:else if b.type === 'tool'}
              <div class="al-blk al-tool">
                <div class="al-tool-head">
                  <code>{b.name}</code>
                  <span class="al-pill {toolState(b).cls}">{toolState(b).label}</span>
                </div>
                {#if fmtArgs(b.args)}<pre class="al-pre">{fmtArgs(b.args)}</pre>{/if}
                {#if b.confirm && !b.result}
                  <div class="al-wait-btns">
                    <button type="button" class="al-btn-ok" onclick={() => run(() => allow(b.uid, true))}>Autoriser</button>
                    <button type="button" class="al-ghost" onclick={() => run(() => allow(b.uid, false))}>Refuser</button>
                  </div>
                {/if}
                {#if b.result?.output}
                  <details>
                    <summary>Résultat</summary>
                    <pre class="al-pre">{b.result.output}</pre>
                  </details>
                {/if}
              </div>
            {:else if b.type === 'result'}
              <div class="al-blk al-tool">
                <span class="al-pill {b.error || b.denied ? 'bad' : 'ok'}">Résultat</span>
                <pre class="al-pre">{b.output}</pre>
              </div>
            {:else if b.type === 'note'}
              <div class="al-blk al-note">{b.text}</div>
            {/if}
          {/each}
        </div>

        <footer class="al-foot">
          {#if current.file}
            <span class="al-path" title={current.file}>{current.file}</span>
            <button type="button" class="al-ghost sm" onclick={copyPath}>{copied ? 'Copié' : 'Copier le chemin'}</button>
          {:else if live.dir}
            <span class="al-path" title={live.dir}>{live.dir}</span>
          {/if}
          {#if current.chatDeleted}<span class="al-note-s">Chat supprimé sur France Student</span>{/if}
        </footer>
      {/if}
    {/if}

    {#if live.notes.length > 0}
      <details class="al-journal">
        <summary>Journal général ({live.notes.length})</summary>
        <div class="al-journal-body">
          {#each live.notes.slice(-40) as n (n.key)}
            <div>{n.text}</div>
          {/each}
        </div>
      </details>
    {/if}
  </section>
{/if}

<style>
  .al-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin-left: 8px;
    padding: 0 9px;
    border-radius: 999px;
    border: 1px solid var(--accent-line, var(--line));
    background: var(--accent-soft, transparent);
    color: var(--text);
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    vertical-align: middle;
  }
  .al-btn:hover {
    filter: brightness(1.1);
  }
  .al-btn.wait {
    border-color: var(--warn, #f59e0b);
  }
  .al-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok, #34d399);
  }
  .al-btn.wait .al-dot {
    background: var(--warn, #f59e0b);
  }
  .al-dot.pulse {
    animation: al-pulse 1.2s ease-in-out infinite;
  }
  @keyframes al-pulse {
    50% {
      opacity: 0.25;
    }
  }
  .al-badge {
    min-width: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--accent, #6366f1);
    color: var(--on-accent, #fff);
    font-size: 10.5px;
    line-height: 16px;
    text-align: center;
  }

  .al-back {
    position: fixed;
    inset: 0;
    z-index: 100;
    border: none;
    padding: 0;
    background: rgba(0, 0, 0, 0.25);
    cursor: default;
  }
  .al-panel {
    position: fixed;
    top: 12px;
    right: 12px;
    bottom: 12px;
    z-index: 101;
    width: min(600px, calc(100vw - 24px));
    display: flex;
    flex-direction: column;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 14px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
    overflow: hidden;
    font-size: 12.5px;
    font-weight: 400;
    letter-spacing: normal;
    text-align: left;
  }
  .al-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }
  .al-title {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .al-title strong {
    font-size: 14px;
  }
  .al-sub {
    color: var(--dim);
    font-size: 11.5px;
  }
  .al-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .al-ghost,
  .al-btn-ok,
  .al-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--line2, var(--line));
    background: transparent;
    color: var(--text);
    font-size: 12px;
    cursor: pointer;
  }
  .al-ghost:hover,
  .al-x:hover {
    background: var(--hover);
  }
  .al-ghost.sm {
    height: 24px;
    padding: 0 8px;
    font-size: 11.5px;
  }
  .al-ghost.danger {
    color: var(--danger, #f87171);
    border-color: color-mix(in srgb, var(--danger, #f87171) 50%, transparent);
  }
  .al-btn-ok {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent, #fff);
    font-weight: 600;
  }
  .al-x {
    width: 28px;
    padding: 0;
    border-color: transparent;
  }

  .al-err {
    margin: 8px 14px 0;
    color: var(--danger, #f87171);
  }
  .al-empty {
    padding: 22px 14px;
    color: var(--dim);
    text-align: center;
  }

  .al-wait {
    margin: 10px 14px 0;
    padding: 8px 10px;
    border-radius: 9px;
    border: 1px solid color-mix(in srgb, var(--warn, #f59e0b) 55%, transparent);
    background: color-mix(in srgb, var(--warn, #f59e0b) 10%, transparent);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 32%;
    overflow: auto;
  }
  .al-wait-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }
  .al-wait-txt {
    min-width: 0;
    flex: 1;
  }
  .al-wait-btns {
    display: flex;
    gap: 6px;
    margin-top: 6px;
    flex-shrink: 0;
  }
  .al-wait-row .al-wait-btns {
    margin-top: 0;
  }

  .al-tabs {
    display: flex;
    gap: 4px;
    padding: 10px 14px 0;
    overflow-x: auto;
    scrollbar-width: thin;
  }
  .al-tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    max-width: 170px;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--dim);
    font-size: 12px;
    cursor: pointer;
  }
  .al-tab.on {
    background: var(--accent-soft, var(--hover));
    border-color: var(--accent-line, var(--line2, var(--line)));
    color: var(--text);
  }
  .al-tab-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .al-st {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--dim);
  }
  .al-st.running {
    background: var(--ok, #34d399);
    animation: al-pulse 1.2s ease-in-out infinite;
  }
  .al-st.done {
    background: var(--ok, #34d399);
  }
  .al-st.failed {
    background: var(--danger, #f87171);
  }
  .al-st.cancelled {
    background: var(--warn, #f59e0b);
  }

  .al-meta {
    padding: 10px 14px 6px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .al-meta-top {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    color: var(--dim);
  }
  .al-task {
    padding: 7px 9px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--panel);
    max-height: 84px;
    overflow: auto;
    white-space: pre-wrap;
    line-height: 1.45;
  }
  .al-last {
    color: var(--dim);
    font-size: 11.5px;
  }

  .al-pill {
    display: inline-block;
    padding: 1px 8px;
    border-radius: 999px;
    border: 1px solid var(--line);
    font-size: 11px;
    font-weight: 600;
    color: var(--text);
  }
  .al-pill.running,
  .al-pill.run {
    color: var(--ok, #34d399);
  }
  .al-pill.done,
  .al-pill.ok {
    color: var(--ok, #34d399);
  }
  .al-pill.failed,
  .al-pill.bad {
    color: var(--danger, #f87171);
  }
  .al-pill.cancelled,
  .al-pill.wait {
    color: var(--warn, #f59e0b);
  }

  .al-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 6px 14px 10px;
    display: flex;
    flex-direction: column;
    gap: 7px;
    scrollbar-width: thin;
  }
  .al-blk {
    line-height: 1.5;
    word-break: break-word;
  }
  .al-text {
    white-space: pre-wrap;
  }
  .al-reason {
    white-space: pre-wrap;
    color: var(--dim);
    font-style: italic;
    padding-left: 9px;
    border-left: 2px solid var(--line2, var(--line));
  }
  .al-lbl {
    display: block;
    font-style: normal;
    font-size: 10.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 2px;
  }
  .al-note {
    color: var(--dim);
    font-size: 11.5px;
  }
  .al-tool {
    padding: 7px 9px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--panel);
  }
  .al-tool-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  code {
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: 12px;
  }
  .al-pre {
    margin: 6px 0 0;
    padding: 6px 8px;
    max-height: 180px;
    overflow: auto;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--code-bg, var(--panel2));
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: 11.5px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-all;
  }
  details > summary {
    cursor: pointer;
    color: var(--dim);
    margin-top: 6px;
  }

  .al-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-top: 1px solid var(--line);
    background: var(--panel);
  }
  .al-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    color: var(--dim);
    font-family: var(--font-mono, ui-monospace, Consolas, monospace);
    font-size: 11px;
  }
  .al-note-s {
    color: var(--dim);
    font-size: 11px;
    flex-shrink: 0;
  }

  .al-journal {
    border-top: 1px solid var(--line);
    padding: 6px 14px;
    background: var(--panel);
  }
  .al-journal summary {
    margin: 0;
  }
  .al-journal-body {
    max-height: 120px;
    overflow: auto;
    margin-top: 6px;
    color: var(--dim);
    font-size: 11.5px;
    line-height: 1.5;
  }
</style>
