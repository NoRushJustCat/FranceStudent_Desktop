<script>
  // Carte d'un appel d'outil.
  // t : {callId, name, origin, args, status: pending|running|ok|error|denied, output}
  // onresolve(t, allow, always) : réponse à une demande de confirmation.
  let { t, onresolve } = $props();

  let always = $state(false);

  const a = $derived(t.args && typeof t.args === 'object' ? t.args : {});
  const name = $derived(String(t.name ?? ''));
  const baseName = $derived.by(() => {
    if (name.startsWith('mcp__')) {
      const [, , ...rest] = name.split('__');
      return rest.join('__') || name;
    }
    return name;
  });
  const isMcp = $derived(t.origin === 'mcp');
  const isSkill = $derived(t.origin === 'skill');

  const isCmd = $derived(baseName === 'run_command');
  const isEdit = $derived(baseName === 'edit_file' || baseName === 'fs_edit_file');
  const isWrite = $derived(baseName === 'write_file' || baseName === 'fs_write_file');
  const isList = $derived(baseName === 'list_directory' || baseName === 'fs_list_dir');
  const isSearch = $derived(baseName === 'search_files' || baseName === 'fs_search_files');

  const VERBS = {
    list_directory: 'Lister',
    fs_list_dir: 'Lister',
    read_file: 'Lire',
    fs_read_file: 'Lire',
    search_files: 'Chercher',
    fs_search_files: 'Chercher',
    edit_file: 'Modifier',
    fs_edit_file: 'Modifier',
    write_file: 'Écrire',
    fs_write_file: 'Écrire',
    run_command: 'Commande',
    git_status: 'Statut Git',
    git_diff: 'Diff Git',
    git_log: 'Historique Git',
  };
  const ARG_LABELS = {
    path: 'Chemin',
    cwd: 'Dossier',
    command: 'Commande',
    query: 'Requête',
    pattern: 'Motif',
    branch: 'Branche',
    name: 'Nom',
  };
  const STATES = { pending: 'en attente', running: 'en cours', ok: 'terminé', error: 'erreur', denied: 'refusé' };

  // mcp__serveur__outil -> « serveur · outil »
  const title = $derived.by(() => {
    if (VERBS[baseName]) return VERBS[baseName];
    if (name.startsWith('mcp__')) {
      const [, srv, ...rest] = name.split('__');
      return `${srv} · ${rest.join('__')}`;
    }
    return name;
  });

  const target = $derived.by(() => {
    if (isCmd) return String(a.command ?? '').split('\n')[0];
    if (isSearch) {
      const q = String(a.query ?? a.pattern ?? '');
      return a.path && a.path !== '.' ? `${q}  ·  ${a.path}` : q;
    }
    if (isList) return String(a.path ?? '.');
    return String(a.path ?? a.name ?? '');
  });

  const MAX = 40;
  const countLines = (s) => {
    const v = String(s ?? '');
    if (!v) return 0;
    const l = v.split('\n');
    return v.endsWith('\n') ? l.length - 1 : l.length;
  };
  const cut = (s) => {
    const v = String(s ?? '');
    const l = v ? v.split('\n') : [];
    if (l.length > 1 && l[l.length - 1] === '') l.pop();
    return { lines: l.slice(0, MAX), more: Math.max(0, l.length - MAX) };
  };
  const oldC = $derived(cut(a.old_text ?? a.old_string));
  const newC = $derived(cut(a.new_text ?? a.new_string));
  const fileC = $derived(cut(a.content));
  const writeLines = $derived(countLines(a.content));
  const argEntries = $derived(
    Object.entries(a).map(([k, v]) => ({
      label: ARG_LABELS[k] ?? k,
      value: typeof v === 'string' ? v : JSON.stringify(v),
    })),
  );

  const hasArgs = $derived(Object.keys(a).length > 0);
  const argsText = $derived.by(() => {
    const s = JSON.stringify(a, null, 2);
    return s.length > 1500 ? s.slice(0, 1500) + '\n…' : s;
  });
</script>

<div class="tool {t.status}">
  <details open={t.status === 'pending' || t.status === 'error'}>
    <summary>
      <span class="st {t.status}" aria-hidden="true">
        {#if t.status === 'ok'}
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
        {:else if t.status === 'pending'}
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/></svg>
        {:else if t.status === 'running'}
          <svg class="spin-ico" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>
        {:else if t.status === 'error'}
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="7" x2="12" y2="14"/><circle cx="12" cy="18" r="1" fill="currentColor"/></svg>
        {:else if t.status === 'denied'}
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="12" x2="18" y2="12"/></svg>
        {/if}
      </span>
      <b class="act" title={name}>{title}</b>
      {#if isMcp}<span class="tag">MCP</span>{/if}
      {#if isSkill}<span class="tag">SKILL</span>{/if}
      <span class="tgt" title={target}>{target}</span>
      <span class="state">{STATES[t.status] ?? t.status}</span>
      <span class="chev" aria-hidden="true">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
      </span>
    </summary>

    <div class="detail">
      {#if isCmd}
        <pre class="cmd"><span class="prompt">$</span> {a.command}</pre>
      {:else if isEdit}
        <pre class="diff">{#each oldC.lines as l}<span class="del">- {l}</span>{/each}{#if oldC.more}<span class="more">… {oldC.more} lignes de plus</span>{/if}{#each newC.lines as l}<span class="add">+ {l}</span>{/each}{#if newC.more}<span class="more">… {newC.more} lignes de plus</span>{/if}</pre>
      {:else if isWrite}
        <div class="warn-overwrite">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            <line x1="12" y1="9" x2="12" y2="13" />
            <line x1="12" y1="17" x2="12.01" y2="17" />
          </svg>
          <span>Contenu précédent inconnu · remplace tout le fichier ({writeLines} ligne{writeLines > 1 ? 's' : ''})</span>
        </div>
        <pre>{fileC.lines.join('\n')}{#if fileC.more}<span class="more">… {fileC.more} lignes de plus</span>{/if}</pre>
      {:else if (isMcp || isSkill) && hasArgs}
        <dl class="kv">
          {#each argEntries as row (row.label)}
            <dt>{row.label}</dt>
            <dd><code>{row.value}</code></dd>
          {/each}
        </dl>
        <details class="raw-args">
          <summary>
            <svg class="raw-chev" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6"/></svg>
            <span>Arguments bruts</span>
          </summary>
          <pre>{argsText}</pre>
        </details>
      {/if}

      {#if t.output}
        <div class="lbl">Résultat</div>
        <pre class="out">{t.output}</pre>
      {:else if t.status === 'running'}
        <div class="lbl">En cours d’exécution…</div>
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
      <span class="hint">{isCmd ? 'Exécutée dans ton dossier de travail.' : 'Action en attente de ton accord.'}</span>
    </div>
  {/if}
</div>

<style>
  .tool {
    background: var(--panel);
    border: 1px solid var(--line);
    border-left: 3px solid var(--line2);
    border-radius: 10px;
    font-size: 13px;
    overflow: hidden;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.08);
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
    padding: 8px 12px;
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
    display: inline-flex;
    align-items: center;
    font: 600 10px/1 var(--mono);
    padding: 3px 6px;
    border-radius: 5px;
    letter-spacing: 0.04em;
    background: var(--panel2);
    color: var(--dim);
    border: 1px solid var(--line2);
    vertical-align: middle;
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
    font: 600 10.5px/1 var(--mono);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--dim);
    white-space: nowrap;
  }
  .tool.ok .state {
    color: var(--ok);
  }
  .tool.error .state {
    color: var(--danger);
  }
  .tool.pending .state {
    color: var(--warn);
  }
  .tool.running .state {
    color: var(--accent-text);
  }
  .chev {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--dim);
    transition: transform var(--t) var(--ease);
  }
  details[open] .chev {
    transform: rotate(90deg);
  }

  /* pastille d'état unifiée */
  .st {
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    line-height: 1;
  }
  .st.running {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .st.pending {
    background: color-mix(in srgb, var(--warn) 18%, transparent);
    color: var(--warn);
    animation: fs-pulse 1.4s ease-in-out infinite;
  }
  .st.ok {
    background: rgba(63, 185, 80, 0.18);
    color: var(--ok);
    animation: fs-pop var(--t) var(--ease) both;
  }
  .st.error {
    background: rgba(255, 107, 107, 0.18);
    color: var(--danger);
    animation: fs-pop var(--t) var(--ease) both;
  }
  .st.denied {
    background: var(--raised);
    color: var(--dim);
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
    background: var(--code-bg);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 7px;
    font: 12.5px/1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 280px;
    overflow: auto;
  }
  pre.out {
    color: var(--dim);
  }
  .warn-overwrite {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 2px;
    padding: 5px 9px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, var(--danger) 42%, transparent);
    background: var(--danger-soft, rgba(248, 81, 73, 0.1));
    color: var(--danger);
    font-size: 11.5px;
    font-weight: 600;
  }
  .kv {
    margin: 0;
    padding: 8px 10px;
    border-radius: 7px;
    background: var(--code-bg);
    border: 1px solid var(--line);
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 5px 12px;
    align-items: baseline;
    font-size: 12.5px;
  }
  .kv dt {
    color: var(--dim);
    font-weight: 500;
  }
  .kv dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .kv code {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
    background: transparent;
    padding: 0;
    border: 0;
  }
  .raw-args summary {
    padding: 2px 0;
    font-size: 11.5px;
    color: var(--dim);
    background: transparent;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .raw-args .raw-chev {
    transition: transform var(--t-fast);
  }
  .raw-args[open] .raw-chev {
    transform: rotate(90deg);
  }
  .raw-args summary:hover {
    background: transparent;
    color: var(--text);
  }
  .raw-args pre {
    margin-top: 6px;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--dim);
    cursor: pointer;
  }
  .spin-ico {
    animation: fs-spin 0.9s linear infinite;
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
    background: var(--diff-del-bg);
    color: var(--diff-del-fg);
  }
  .add {
    background: var(--diff-add-bg);
    color: var(--diff-add-fg);
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
    background: color-mix(in srgb, var(--warn) 8%, transparent);
    animation: fs-fade var(--t) both;
  }
  .hint {
    font-size: 12px;
    color: var(--text);
    opacity: 0.82;
  }
</style>
