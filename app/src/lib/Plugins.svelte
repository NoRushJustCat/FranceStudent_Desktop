<script>
  import { onMount } from 'svelte';
  import { pluginList, pluginAdd, pluginRemove, pluginEnable, pluginUpdate } from './api.js';

  // `onchange` : appelé après un changement (recharge les skills).
  let { onchange } = $props();

  let plugins = $state([]);
  let loading = $state(true);
  let busy = $state(false);
  let source = $state('');
  let installAll = $state(true);
  let error = $state('');
  let info = $state('');

  const msg = (e) => (typeof e === 'string' ? e : e?.message || String(e));

  async function refresh() {
    try {
      plugins = await pluginList();
      error = '';
    } catch (e) {
      error = msg(e);
    } finally {
      loading = false;
    }
  }

  // Exécute une action, affiche le résultat, recharge la liste et les skills.
  async function act(fn) {
    if (busy) return;
    busy = true;
    error = '';
    info = '';
    try {
      info = (await fn()) || '';
      await refresh();
      onchange?.();
    } catch (e) {
      error = msg(e);
    } finally {
      busy = false;
    }
  }

  const add = () =>
    act(async () => {
      const l = await pluginAdd(source.trim(), installAll);
      source = '';
      return installAll
        ? `${l.length} plugin(s) ajouté(s) et activé(s).`
 : `Marketplace ajoutée : ${l.length} plugin(s) disponible(s), à activer ci-dessous.`;
    });

  const toggle = (p) =>
    act(async () => {
      await pluginEnable(`${p.name}@${p.market}`, !p.enabled);
      return `« ${p.name}@${p.market} » ${p.enabled ? 'désactivé' : 'activé'}.`;
    });

  const remove = (p) =>
    act(async () => {
      await pluginRemove(p.store);
      return `Marketplace « ${p.store} » supprimée.`;
    });

  const update = () => act(() => pluginUpdate(null));

  onMount(refresh);
</script>

<div class="plugins">
  <p class="hint">
    Plugins au format Claude Code, identifiés par <code>nom@marketplace</code> : leurs skills et serveurs MCP sont
    chargés. Les hooks, commandes et agents sont ignorés. N'installez que des sources de confiance.
  </p>

  <form class="card add" onsubmit={(e) => (e.preventDefault(), add())}>
    <label>
      Ajouter une marketplace ou un plugin
      <input
        class="mono"
        bind:value={source}
        placeholder="owner/repo, https://github.com/…, ou dossier local"
        disabled={busy}
        spellcheck="false"
      />
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={installAll} disabled={busy} />
      Activer tous les plugins (sinon, à activer un par un)
    </label>
    <div class="row">
      <button type="submit" class="pri" disabled={busy || !source.trim()}>
        {busy ? 'En cours…' : 'Ajouter'}
      </button>
      <button type="button" class="ghost" onclick={refresh} disabled={busy}>Actualiser</button>
      <button type="button" class="ghost" onclick={update} disabled={busy}>Mettre à jour</button>
    </div>
  </form>

  {#if error}<p class="msg bad">{error}</p>{/if}
  {#if info}<p class="msg ok">{info}</p>{/if}

  {#if loading}
    <p class="mut">Chargement…</p>
  {:else if plugins.length === 0}
    <p class="mut">Aucun plugin installé.</p>
  {:else}
    {#each plugins as p (p.name + '@' + p.market + p.source)}
      <div class="card" class:off={!p.enabled}>
 <div class="head">
          <strong>{p.name}</strong>
          <span class="mut">@{p.market}</span>
   <span class="badge" class:claude={p.source === 'claude'}>{p.source === 'claude' ? 'CLAUDE CODE' : 'APP'}</span>
          <span class="mut">{p.enabled ? 'Activé' : 'Désactivé'}</span>
          {#if p.source === 'app'}
     <button class="push-right" onclick={() => toggle(p)} disabled={busy}>
       {p.enabled ? 'Désactiver' : 'Activer'}
            </button>
     <button class="ghostdanger" onclick={() => remove(p)} disabled={busy} title="Supprime la marketplace « {p.store} » et tous ses plugins">
       Supprimer
     </button>
          {/if}
        </div>
 <div class="counts mut">
          {p.skills} skill{p.skills > 1 ? 's' : ''} · {p.mcp} serveur{p.mcp > 1 ? 's' : ''} MCP
   {#if p.hooks}· <span class="warn">hooks ignorés</span>{/if}
        </div>
        <code class="path">{p.path}</code>
        {#if p.source === 'claude'}
   <div class="mut">Installé via Claude Code : à gérer depuis Claude Code.</div>
        {/if}
      </div>
    {/each}
  {/if}
</div>

<style>
  .plugins {
    --xt-card: var(--panel, #1a1f2b);
    --xt-line: var(--line, #2a303d);
    --xt-line2: var(--line2, #384152);
    --xt-txt: var(--text, #d9dde6);
    --xt-mut: var(--dim, #8a92a3);
    --xt-acc: var(--accent, #3d7bff);
    --xt-bad: var(--danger, #ff5d6c);
    --xt-ok: var(--ok, #3ecf8e);
    --xt-warn: var(--warn, #e3b341);
    --xt-panel2: var(--panel2, #232938);
    color: var(--xt-txt);
  }
  .hint {
    color: var(--xt-mut);
    line-height: 1.5;
    margin: 0 0 12px;
  }
  .mut {
    color: var(--xt-mut);
    font-size: 12px;
    margin: 4px 0;
  }
  .warn {
    color: var(--xt-warn);
  }
  .card {
    background: var(--xt-card);
    border: 1px solid var(--xt-line);
    border-radius: 12px;
    padding: 15px 17px;
    margin-bottom: 10px;
  }
  .row,
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .row {
    margin-top: 10px;
  }
  .push-right {
    margin-left: auto;
  }
  .path {
    display: block;
    margin-top: 6px;
    font-size: 11.5px;
    color: var(--xt-mut);
    word-break: break-all;
    white-space: normal;
  }
  .badge {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    padding: 2px 7px;
    border: 1px solid color-mix(in srgb, var(--xt-acc) 45%, var(--xt-line2));
    background: color-mix(in srgb, var(--xt-acc) 12%, transparent);
    color: var(--xt-acc);
    border-radius: 999px;
  }
  .badge.claude {
    border-color: var(--xt-line2);
    background: var(--xt-panel2);
    color: var(--xt-txt);
  }
  .msg {
    margin: 0 0 10px;
    line-height: 1.45;
    word-break: break-word;
  }
  .msg.bad {
    color: var(--xt-bad);
  }
  .msg.ok {
    color: var(--xt-ok);
  }
  label {
    display: block;
    font-weight: 500;
  }
  label input {
    display: block;
    width: 100%;
    box-sizing: border-box;
    height: 36px;
    margin-top: 6px;
    padding: 0 11px;
    font: inherit;
    font-size: 12.5px;
    color: var(--xt-txt);
    background: var(--input, var(--xt-panel2));
    border: 1px solid var(--xt-line2);
    border-radius: 8px;
  }
  .mono {
    font-family: var(--font-mono, Consolas, 'Cascadia Mono', monospace);
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 36px;
    padding: 0 14px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--xt-txt);
    background: var(--xt-panel2);
    border: 1px solid var(--xt-line2);
    border-radius: 8px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  button.pri {
    background: var(--xt-acc);
    border-color: var(--xt-acc);
    color: var(--on-accent, #fff);
    font-weight: 600;
  }
  button.ghost {
    background: transparent;
    border-color: transparent;
    color: var(--xt-mut);
  }
  button.ghostdanger {
    background: transparent;
    color: color-mix(in srgb, var(--xt-bad) 82%, var(--xt-txt));
  }
  button.ghostdanger:hover:not(:disabled) {
    border-color: var(--xt-bad);
    background: color-mix(in srgb, var(--xt-bad) 10%, transparent);
  }
  .card.off {
    opacity: 0.65;
  }
  label.check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-weight: 400;
    font-size: 12.5px;
    color: var(--xt-mut);
  }
  label.check input {
    display: inline-block;
    width: auto;
    height: auto;
    margin: 0;
  }
</style>
