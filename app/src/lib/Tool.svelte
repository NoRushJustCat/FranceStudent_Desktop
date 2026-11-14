<script>
  // Carte d'un appel d'outil.
  // t : {callId, name, origin, args, status: pending|running|ok|error|denied, output}
  // onresolve(t, allow, always) : réponse à une demande de confirmation.
  let { t, onresolve } = $props();

  let always = $state(false);

  const a = $derived(t.args && typeof t.args === 'object' ? t.args : {});
  const name = $derived(String(t.name ?? ''));
  const isMcp = $derived(t.origin === 'mcp');
  const isSkill = $derived(t.origin === 'skill');

  const VERBS = {
    list_directory: 'Lister',
    read_file: 'Lire',
    search_files: 'Chercher',
    edit_file: 'Modifier',
    write_file: 'Écrire',
    run_command: 'Commande',
  };
  const STATES = { pending: 'en attente', running: 'en cours', ok: 'terminé', error: 'erreur', denied: 'refusé' };

  // mcp__serveur__outil -> « serveur · outil »
  const title = $derived.by(() => {
    if (VERBS[name]) return VERBS[name];
    if (name.startsWith('mcp__')) {
      const [, srv, ...rest] = name.split('__');
      return `${srv} · ${rest.join('__')}`;
    }
    return name;
  });

  const target = $derived.by(() => {
    switch (name) {
      case 'run_command':
        return String(a.command ?? '').split('\n')[0];
      case 'search_files': {
        const q = String(a.query ?? a.pattern ?? '');
        return a.path && a.path !== '.' ? `${q}  ·  ${a.path}` : q;
      }
      case 'list_directory':
        return String(a.path ?? '.');
      default:
        return String(a.path ?? a.name ?? '');
    }
  });

  const MAX = 40;
  const cut = (s) => {
    const l = String(s ?? '').split('\n');
    return { lines: l.slice(0, MAX), more: Math.max(0, l.length - MAX) };
  };
  const oldC = $derived(cut(a.old_text ?? a.old_string));
  const newC = $derived(cut(a.new_text ?? a.new_string));
  const fileC = $derived(cut(a.content));

  const hasArgs = $derived(Object.keys(a).length > 0);
  const argsText = $derived.by(() => {
    const s = JSON.stringify(a, null, 2);
    return s.length > 1500 ? s.slice(0, 1500) + '\n…' : s;
  });
</script>

<div class="tool {t.status}">
  <details open={t.status === 'pending' || t.status === 'error'}>
    <summary>
      <span class="st {t.status}" aria-hidden="true"></span>
      <b class="act">{title}</b>
      {#if isMcp}<span class="tag">mcp</span>{/if}
      {#if isSkill}<span class="tag">skill</span>{/if}
      <span class="tgt" title={target}>{target}</span>
      <span class="state">{STATES[t.status] ?? t.status}</span>
      <span class="chev" aria-hidden="true">›</span>
    </summary>

    <div class="detail">
      {#if name === 'run_command'}
        <pre class="cmd"><span class="prompt">$</span> {a.command}</pre>
      {:else if name === 'edit_file'}
        <pre class="diff">{#each oldC.lines as l}<span class="del">- {l}</span>{/each}{#if oldC.more}<span class="more">… {oldC.more} lignes de plus</span>{/if}{#each newC.lines as l}<span class="add">+ {l}</span>{/each}{#if newC.more}<span class="more">… {newC.more} lignes de plus</span>{/if}</pre>
      {:else if name === 'write_file'}
        <pre>{fileC.lines.join('\n')}{#if fileC.more}<span class="more">… {fileC.more} lignes de plus</span>{/if}</pre>
      {:else if (isMcp || isSkill) && hasArgs}
        <pre>{argsText}</pre>
      {/if}

      {#if t.output}
        <div class="lbl">Résultat</div>
        <pre class="out">{t.output}</pre>
      {:else if t.status === 'running'}
        <div class="lbl">En cours d'exécution…</div>
      {/if}
    </div>
  </details>

  {#if t.status === 'pending'}
    <div class="ask">
      <button class="primary" onclick={() => onresolve?.(t, true, always)}>Autoriser</button>
      <button onclick={() => onresolve?.(t, false, false)}>Refuser</button>
      {#if isMcp}
        <label class="chk"><input type="checkbox" bind:checked={always} /> Toujours autoriser (session)</label>
      {/if}
      <span class="hint">{name === 'run_command' ? 'Exécutée dans ton dossier de travail.' : 'Action en attente de ton accord.'}</span>
    </div>
  {/if}
</div>

<style>
  .tool {
    background: var(--panel);
    border: 1px solid var(--line);
    border-left: 3px solid var(--line2);
    font-size: 13px;
    overflow: hidden;
    box-shadow: 3px 3px 0 rgba(0, 0, 0, 0.3);
    animation: fs-rise var(--t-slow) var(--ease) both;
    transition:
      border-color var(--t),
      box-shadow var(--t) var(--ease);
  }
  .tool.pending {
    border-left-color: var(--warn);
  }
  .tool.running {
    border-left-color: var(--accent);
  }
  .tool.ok {
    border-left-color: rgba(63, 185, 80, 0.7);
  }
  .tool.error {
    border-left-color: var(--danger);
  }

  summary {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 7px 12px;
    cursor: pointer;
    list-style: none;
    user-select: none;
    min-width: 0;
    transition: background var(--t-fast);
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary:hover {
    background: var(--panel2);
  }
  .act {
    font-weight: 600;
    white-space: nowrap;
  }
  .tag {
    font: 10px/1 var(--mono);
    padding: 3px 6px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: transparent;
    color: #8db1ff;
    border: 1px solid var(--accent-line);
  }
  .tgt {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    font-size: 12.5px;
    color: var(--dim);
  }
  .state {
    font: 10.5px/1 var(--mono);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--faint);
    white-space: nowrap;
  }
  .chev {
    color: var(--faint);
    transition: transform var(--t) var(--ease);
  }
  details[open] .chev {
    transform: rotate(90deg);
  }

  /* pastille d'état : carré */
  .st {
    flex: none;
    width: 14px;
    height: 14px;
    display: grid;
    place-items: center;
    font-size: 10px;
    font-weight: 800;
    line-height: 1;
  }
  .st.running {
    border: 2px solid var(--line2);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: fs-spin 0.8s linear infinite;
  }
  .st.pending {
    background: var(--warn);
    animation: fs-pulse 1s ease-in-out infinite;
  }
  .st.ok {
    background: rgba(63, 185, 80, 0.18);
    color: var(--ok);
    animation: fs-pop var(--t) var(--ease) both;
  }
  .st.ok::before {
    content: '✓';
  }
  .st.error {
    background: rgba(255, 107, 107, 0.18);
    color: var(--danger);
    animation: fs-pop var(--t) var(--ease) both;
  }
  .st.error::before {
    content: '!';
  }
  .st.denied {
    background: var(--raised);
    color: var(--dim);
  }
  .st.denied::before {
    content: '–';
  }

  .detail {
    border-top: 1px solid var(--line);
    padding: 10px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  details[open] .detail {
    animation: fs-fade var(--t) both;
  }
  .lbl {
    font: 10.5px/1 var(--mono);
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }
  pre {
    margin: 0;
    padding: 9px 12px;
    background: #090c11;
    border: 1px solid var(--line);
    font: 12.5px/1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 280px;
    overflow: auto;
  }
  pre.out {
    color: var(--dim);
  }
  .prompt {
    color: var(--accent);
    user-select: none;
  }
  .diff span {
    display: block;
    padding: 0 6px;
    margin: 0 -6px;
  }
  .del {
    background: rgba(255, 107, 107, 0.12);
    color: #ffb4b4;
  }
  .add {
    background: rgba(63, 185, 80, 0.12);
    color: #9be3a6;
  }
  .more {
    display: block;
    color: var(--faint);
    font-style: italic;
  }

  .ask {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    padding: 9px 12px;
    border-top: 1px solid var(--line);
    background: rgba(229, 165, 10, 0.06);
    animation: fs-fade var(--t) both;
  }
  .hint {
    font-size: 12px;
    color: var(--dim);
  }
</style>
