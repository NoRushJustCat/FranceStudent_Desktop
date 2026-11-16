<script>
  // Panneau latéral « Agents » (même emplacement que Diff / Terminal, colonne de droite).
  // Données : agentlive.svelte.js (flux Tauri `agents:live`). Transcriptions complètes : sur le PC.
  import { onMount } from 'svelte';
  import { live, view, init, cancel, cancelAll, clearFinished, allow } from './agentlive.svelte.js';

  let { onclose = () => {} } = $props();

  let sel = $state(null); // id d'agent, ou '__log' pour le journal général
  let box = $state(null);
  let stick = true;
  let now = $state(Date.now());
  let copied = $state(false);
  let err = $state('');

  const LABEL = { running: 'en cours', done: 'terminé', failed: 'échec', cancelled: 'annulé' };

  const n = $derived(view.count);
  const finished = $derived(live.agents.filter((a) => a.status !== 'running').length);

  const current = $derived.by(() => {
    if (sel === '__log') return null;
    return (
      live.agents.find((a) => a.id === sel) ??
      live.agents.find((a) => a.status === 'running') ??
      live.agents[live.agents.length - 1] ??
      null
    );
  });
  const blocks = $derived(current ? (live.feeds[current.id] ?? []) : []);
  // Change à chaque ajout ou morceau de texte reçu : sert au défilement automatique.
  const sig = $derived.by(() => {
    const last = blocks[blocks.length - 1];
    return sel === '__log'
      ? live.notes.length
      : blocks.length + ':' + (last?.text?.length ?? 0) + ':' + (last?.result ? 1 : 0);
  });

  onMount(() => {
    init();
  });

  // Horloge : seulement quand un agent tourne.
  $effect(() => {
    if (!n) return;
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  // Défilement automatique, sauf si l'utilisateur a remonté dans le flux.
  $effect(() => {
    sig;
    if (box && stick) queueMicrotask(() => box && (box.scrollTop = box.scrollHeight));
  });

  function onScroll() {
    if (box) stick = box.scrollHeight - box.scrollTop - box.clientHeight < 48;
  }

  function pick(id) {
    sel = id;
    stick = true;
  }

  const fmt = (v) => {
    if (v == null) return '';
    if (typeof v === 'string') return v;
    try {
      return JSON.stringify(v);
    } catch {
      return String(v);
    }
  };
  const short = (s, max = 180) => (s.length > max ? s.slice(0, max) + '…' : s);

  function dur(a) {
    const s = Math.max(0, Math.round(((a.ended ?? now) - a.started) / 1000));
    return s >= 60 ? Math.floor(s / 60) + ' min ' + String(s % 60).padStart(2, '0') + ' s' : s + ' s';
  }

  const nameOf = (id) => live.agents.find((a) => a.id === id)?.name ?? id;

  async function run(fn) {
    err = '';
    try {
      await fn();
    } catch (e) {
      err = String(e?.message ?? e);
    }
  }

  async function copyPath() {
    const p = current?.file || live.dir;
    if (!p) return;
    try {
      await navigator.clipboard.writeText(p);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      copied = false;
    }
  }
</script>

<aside class="ap" aria-label="Agents en direct">
  <div class="ap-head">
    <b>Agents</b>
    <span class="ap-dim">{n} en cours</span>
    <span class="ap-sp"></span>
    <button type="button" class="ap-x" aria-label="Fermer" title="Fermer" onclick={onclose}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
    </button>
  </div>

  {#if n > 0 || finished > 0}
    <div class="ap-bar">
      {#if n > 0}
        <button type="button" class="ap-act danger" onclick={() => run(cancelAll)}>Tout annuler</button>
      {/if}
      {#if finished > 0}
        <button type="button" class="ap-act" onclick={() => run(clearFinished)}>Effacer les terminés</button>
      {/if}
    </div>
  {/if}

  {#if err || live.error}
    <div class="ap-err">{err || live.error}</div>
  {/if}

  {#if live.waiting.length > 0}
    <div class="ap-ask">
      {#each live.waiting as w (w.uid)}
        <div class="ap-ask-row">
   <div class="ap-ask-t">
     <b>{nameOf(w.agent)}</b> demande l'autorisation : <code>{w.name}</code>
     <div class="ap-args">{short(fmt(w.args), 140)}</div>
   </div>
          <div class="ap-ask-b">
     <button type="button" class="ap-act ok" onclick={() => run(() => allow(w.uid, true))}>Autoriser</button>
     <button type="button" class="ap-act" onclick={() => run(() => allow(w.uid, false))}>Refuser</button>
   </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="ap-tabs" role="tablist">
    {#each live.agents as a (a.id)}
      <button type="button" role="tab" aria-selected={current?.id === a.id && sel !== '__log'} class="ap-chip" class:sel={current?.id === a.id && sel !== '__log'} onclick={() => pick(a.id)} title={a.name}>
 <i class="st {a.status}"></i>
 <span class="ap-name">{a.name}</span>
      </button>
    {/each}
    <button type="button" role="tab" aria-selected={sel === '__log'} class="ap-chip" class:sel={sel === '__log'} onclick={() => pick('__log')}>
      <i class="st log"></i>
      <span class="ap-name">Journal</span>
    </button>
  </div>

  <section class="ap-view">
    {#if sel === '__log'}
      <div class="ap-feed" bind:this={box} onscroll={onScroll}>
        {#each live.notes as m (m.key)}
          <div class="b note">{m.text}</div>
        {:else}
   <div class="ap-empty">Rien pour l'instant.</div>
        {/each}
      </div>
    {:else if current}
      <div class="ap-card">
 <div class="ap-card-top">
          <b class="ap-card-name">{current.name}</b>
          <span class="state {current.status}">{LABEL[current.status] ?? current.status}</span>
          {#if current.status === 'running'}
     <button type="button" class="ap-act danger" onclick={() => run(() => cancel(current.id))}>Annuler</button>
   {/if}
 </div>
        <div class="ap-tags">
   <span class="tag">{current.role}</span>
   {#if current.model}<span class="tag">{current.model}</span>{/if}
   {#if current.wave != null}<span class="tag">vague {current.wave}</span>{/if}
   <span class="ap-dim">{dur(current)} · {current.steps ?? 0} étape{(current.steps ?? 0) > 1 ? 's' : ''}</span>
 </div>
        {#if current.task}<div class="ap-task">{short(current.task, 400)}</div>{/if}
 {#if current.file}
   <div class="ap-file">
     <span title={current.file}>{current.file}</span>
     <button type="button" class="ap-act" onclick={copyPath}>{copied ? 'Copié' : 'Copier'}</button>
   </div>
        {/if}
      </div>

      <div class="ap-feed" bind:this={box} onscroll={onScroll}>
        {#each blocks as b (b.key)}
   {#if b.type === 'text'}
     <div class="b txt">{b.text}</div>
   {:else if b.type === 'reasoning'}
     <div class="b rs"><span class="lbl">Réflexion</span>{b.text}</div>
   {:else if b.type === 'tool'}
     <div class="b tool" class:pending={b.confirm && !b.result}>
       <div class="tool-h">
  <code>{b.name}</code>
         {#if b.confirm && !b.result}<span class="state running">attend ton accord</span>{/if}
         {#if b.result?.denied}<span class="state failed">refusé</span>{/if}
  {#if b.result?.error}<span class="state failed">erreur</span>{/if}
       </div>
       <div class="ap-args wrap">{short(fmt(b.args), 240)}</div>
{#if b.result?.output}
         <details>
           <summary>Résultat</summary>
    <pre>{b.result.output}</pre>
  </details>
       {/if}
     </div>
   {:else if b.type === 'result'}
     <pre class="b res">{b.output}</pre>
   {:else}
     <div class="b note">{b.text}</div>
   {/if}
        {:else}
   <div class="ap-empty">En attente de la première action…</div>
        {/each}
      </div>
    {:else}
      <div class="ap-empty pad">Aucun agent à afficher.</div>
    {/if}
  </section>
</aside>

<style>
  .ap {
    height: 100%;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
    color: var(--text);
    border-left: 1px solid var(--line);
    font-size: 12.5px;
  }

  .ap-head {
    height: var(--topbar-h, 52px);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px 0 14px;
    border-bottom: 1px solid var(--line);
  }

  .ap-head b {
    font-size: 13.5px;
  }

  .ap-sp {
    flex: 1;
  }

  .ap-dim {
    color: var(--dim);
    font-size: 11.5px;
  }

  .ap-x {
    width: 28px;
    height: 28px;
    display: inline-grid;
    place-items: center;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
  }

  .ap-x:hover {
    background: var(--hover);
    color: var(--text);
  }

  .ap-bar {
    flex-shrink: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
  }

  .ap-act {
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: 7px;
    background: var(--panel2);
    color: var(--text);
    font-size: 11.5px;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .ap-act:hover {
    background: var(--hover);
  }

  .ap-act.danger {
    color: var(--danger, #f87171);
  }

  .ap-act.ok {
    background: var(--accent);
    color: var(--on-accent, #fff);
  }

  .ap-err {
    flex-shrink: 0;
    padding: 8px 12px;
    color: var(--danger, #f87171);
    border-bottom: 1px solid var(--line);
    overflow-wrap: anywhere;
  }

  .ap-ask {
    flex-shrink: 0;
    max-height: 38%;
    overflow-y: auto;
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--warn, #f59e0b) 10%, transparent);
  }

  .ap-ask-row {
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .ap-ask-t {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .ap-ask-b {
    display: flex;
    gap: 6px;
  }

  .ap-args {
    color: var(--dim);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ap-args.wrap {
    white-space: normal;
    overflow-wrap: anywhere;
    margin-top: 3px;
  }

  .ap-tabs {
    flex-shrink: 0;
    display: flex;
    gap: 4px;
    padding: 8px 10px;
    overflow-x: auto;
    border-bottom: 1px solid var(--line);
  }

  .ap-chip {
    flex-shrink: 0;
    max-width: 160px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--dim);
    font-size: 12px;
    cursor: pointer;
  }

  .ap-chip:hover {
    background: var(--hover);
    color: var(--text);
  }

  .ap-chip.sel {
    background: var(--accent-soft, var(--hover));
    color: var(--text);
  }

  .ap-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .st {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--dim);
  }

  .st.running {
    background: var(--state-running, var(--warn, #f59e0b));
    animation: ap-pulse 1.6s ease-in-out infinite;
  }

  .st.done {
    background: var(--ok, #34d399);
  }

  .st.failed {
    background: var(--danger, #f87171);
  }

  @keyframes ap-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  .ap-view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .ap-card {
    flex-shrink: 0;
    padding: 10px 14px;
    border-bottom: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .ap-card-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ap-card-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ap-tags {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }

  .tag {
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--panel2);
    color: var(--dim);
    font-size: 10.5px;
  }

  .state {
    font-size: 11px;
    font-weight: 600;
    color: var(--dim);
  }

  .state.running {
    color: var(--warn, #f59e0b);
  }

  .state.done {
    color: var(--ok, #34d399);
  }

  .state.failed {
    color: var(--danger, #f87171);
  }

  .ap-task {
    color: var(--dim);
    line-height: 1.4;
    max-height: 4.2em;
    overflow: hidden;
  }

  .ap-file {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--dim);
    font-family: var(--font-mono, monospace);
    font-size: 10.5px;
  }

  .ap-file span {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ap-feed {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .b {
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .b.rs {
    color: var(--dim);
    font-style: italic;
    font-size: 11.5px;
  }

  .lbl {
    display: block;
    font-style: normal;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    margin-bottom: 2px;
  }

  .b.note {
    color: var(--dim);
    font-size: 11.5px;
  }

  .b.tool {
    padding: 7px 9px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--code-bg, var(--panel2));
    white-space: normal;
  }

  .b.tool.pending {
    border-color: var(--warn, #f59e0b);
  }

  .tool-h {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .tool-h code,
  .ap-ask code {
    font-family: var(--font-mono, monospace);
    font-size: 11.5px;
  }

  summary {
    cursor: pointer;
    color: var(--dim);
    font-size: 11px;
    margin-top: 4px;
  }

  pre {
    margin: 4px 0 0;
    max-height: 200px;
    overflow: auto;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  pre.res {
    padding: 7px 9px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--code-bg, var(--panel2));
  }

  .ap-empty {
    color: var(--dim);
    padding: 6px 2px;
  }

  .ap-empty.pad {
    padding: 14px;
  }
</style>