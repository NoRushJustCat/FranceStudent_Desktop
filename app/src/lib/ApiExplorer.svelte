<script>
  import { onMount } from 'svelte';
  import { apiRoutes, apiCall } from './api.js';

  let routes = $state([]);
  let filter = $state('');
  let sel = $state(null);
  let pathVals = $state({});
  let queryText = $state('');
  let headerText = $state('');
  let bodyText = $state('');
  let busy = $state(false);
  let out = $state('');
  let err = $state('');
  let loadErr = $state('');

  const RISK = { read: 'lecture', write: 'écriture', destructive: 'destructif' };

  onMount(async () => {
    try {
      routes = (await apiRoutes()).filter((r) => r.kind === 'json');
    } catch (e) {
      loadErr = String(e);
    }
  });

  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    return routes.filter((r) => !f || `${r.id} ${r.path} ${r.summary} ${r.group}`.toLowerCase().includes(f));
  });
  const groups = $derived.by(() => {
    const m = new Map();
    for (const r of shown) m.set(r.group, [...(m.get(r.group) ?? []), r]);
    return [...m.entries()];
  });
  const pathNames = $derived(sel ? [...sel.path.matchAll(/\{([^}]+)\}/g)].map((m) => m[1]) : []);

  function pick(r) {
    sel = r;
    pathVals = {};
    queryText = '';
    headerText = '';
    bodyText = r.hasBody ? '{}' : '';
    out = '';
    err = '';
  }

  // « a=1&b=2 » ou une paire par ligne ; clés et valeurs décodées.
  function pairs(text, sep) {
    return text
      .split(/\r?\n|&/)
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const i = l.indexOf(sep);
        return i < 0 ? [l, ''] : [l.slice(0, i).trim(), l.slice(i + 1).trim()];
      });
  }

  async function run() {
    if (!sel || busy) return;
    err = '';
    out = '';
    let body = null;
    if (sel.hasBody) {
      try {
        body = JSON.parse(bodyText || '{}');
      } catch (e) {
        err = `Corps JSON invalide : ${e.message}`;
        return;
      }
    }
    let confirmed = false;
    if (sel.risk === 'destructive') {
      const target = pathNames.map((n) => `${n}=${pathVals[n] ?? ''}`).join(', ');
      confirmed = window.confirm(
        `Action destructive\n\n${sel.method} ${sel.path}\n${sel.summary}${target ? `\n${target}` : ''}\n\nContinuer ?`
      );
      if (!confirmed) return;
    }
    busy = true;
    try {
      const res = await apiCall({
        id: sel.id,
        pathParams: pathVals,
 query: pairs(queryText, '='),
        headers: pairs(headerText, ':'),
        body,
        confirmed
      });
      out = typeof res === 'string' ? res : JSON.stringify(res, null, 2);
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<header class="sec-head">
  <div>
    <h2>Explorateur d’API</h2>
    <p class="sec-sub">Toutes les routes JSON du compte. Les actions destructives demandent confirmation.</p>
  </div>
</header>

{#if loadErr}
  <div class="msg-err">{loadErr}</div>
{/if}

<div class="ax">
  <div class="ax-list">
    <input class="input" placeholder="Filtrer (chemin, groupe, texte)…" bind:value={filter} />
    <div class="ax-scroll">
      {#each groups as [name, items]}
        <div class="ax-group">{name}</div>
        {#each items as r}
          <button type="button" class="ax-row" class:active={sel?.id === r.id} onclick={() => pick(r)}>
     <span class="ax-m ax-{r.method.toLowerCase()}">{r.method}</span>
     <span class="ax-p">{r.path}</span>
     {#if r.risk === 'destructive'}<span class="ax-warn" title="Destructif">!</span>{/if}
   </button>
 {/each}
      {/each}
      {#if !shown.length}<div class="ax-empty">Aucune route.</div>{/if}
    </div>
  </div>

  <div class="ax-main">
    {#if sel}
      <div class="ax-title">
        <b>{sel.method} {sel.path}</b>
 <span class="ax-tag ax-{sel.risk}">{RISK[sel.risk]}</span>
      </div>
      <p class="sec-sub">{sel.summary}</p>

      {#each pathNames as n}
        <label class="ax-f">{n}<input class="input" bind:value={pathVals[n]} /></label>
      {/each}
      {#if sel.query.length}
 <label class="ax-f">
          Requête ({sel.query.join(', ')}) — <code>clé=valeur</code>, une par ligne
   <textarea class="input" rows="2" bind:value={queryText}></textarea>
        </label>
      {/if}
      {#if sel.headers.length}
        <label class="ax-f">
   En-têtes ({sel.headers.join(', ')}) — <code>Nom: valeur</code>
   <textarea class="input" rows="2" bind:value={headerText}></textarea>
        </label>
      {/if}
      {#if sel.hasBody}
        <label class="ax-f">
          Corps JSON
          <textarea class="input ax-mono" rows="6" bind:value={bodyText}></textarea>
        </label>
      {/if}

      <div class="form-actions">
 <button type="button" class="btn-primary" disabled={busy} onclick={run}>
          {busy ? 'Envoi…' : 'Envoyer'}
        </button>
      </div>
      {#if err}<div class="msg-err">{err}</div>{/if}
      {#if out}<pre class="ax-out">{out}</pre>{/if}
    {:else}
      <p class="sec-sub">Choisis une route à gauche.</p>
    {/if}
  </div>
</div>

<style>
  .ax { display: grid; grid-template-columns: minmax(220px, 300px) 1fr; gap: 14px; min-height: 360px; }
  .ax-list, .ax-main { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .ax-scroll { overflow: auto; max-height: 60vh; border: 1px solid var(--border, #3333); border-radius: 8px; }
  .ax-group { position: sticky; top: 0; padding: 4px 8px; font-size: 11px; opacity: 0.7; background: var(--bg, #111); }
  .ax-row { display: flex; gap: 6px; align-items: center; width: 100%; padding: 4px 8px; text-align: left; background: none; border: 0; color: inherit; cursor: pointer; font-size: 12px; }
  .ax-row:hover, .ax-row.active { background: rgba(127, 127, 127, 0.18); }
  .ax-p { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .ax-m { font: 600 10px monospace; min-width: 46px; }
  .ax-get { color: #4a9; } .ax-post, .ax-put { color: #c90; } .ax-patch { color: #69c; } .ax-delete { color: #d55; }
  .ax-warn { color: #d55; font-weight: 700; }
  .ax-tag { font-size: 11px; padding: 1px 6px; border-radius: 6px; border: 1px solid currentColor; margin-left: 8px; }
  .ax-read { color: #4a9; } .ax-write { color: #c90; } .ax-destructive { color: #d55; }
  .ax-f { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  .ax-mono, .ax-out { font-family: ui-monospace, monospace; font-size: 12px; }
  .ax-out { max-height: 40vh; overflow: auto; padding: 8px; border-radius: 8px; background: rgba(127, 127, 127, 0.12); white-space: pre-wrap; word-break: break-word; user-select: text; }
  .ax-empty { padding: 10px; opacity: 0.6; font-size: 12px; }
</style>
