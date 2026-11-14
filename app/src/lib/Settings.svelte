<script>
  // Réglages : fenêtre avec navigation à gauche.
  // Compte | Code | Économie | Skills | MCP | Diagnostic
  import { untrack } from 'svelte';
  import { saveSettings } from './api.js';
  import Extensions from './Extensions.svelte';

  let {
    settings,
    usage = null,
    budget = null,
    workdir = null,
    autoAccept = false,
    eco = false,
    last = null,
    session = { calls: 0, prompt: 0, output: 0, messages: 0 },
    ctx = null,
    chatId = null,
    busy = false,
    tab: initialTab = 'compte',
    onclose,
    onsaved,
    oneco,
    onauto,
    onpick,
    oncompact,
    onreset,
    oninsert,
    onextchange,
  } = $props();

  const TABS = [
    { id: 'compte', label: 'Compte' },
    { id: 'code', label: 'Code' },
    { id: 'eco', label: 'Économie' },
    { id: 'skills', label: 'Skills' },
    { id: 'mcp', label: 'MCP' },
    { id: 'diag', label: 'Diagnostic' },
  ];

  // l'état initial vient des props : on le copie une seule fois, volontairement
  let tab = $state(untrack(() => initialTab));
  let baseUrl = $state(untrack(() => settings?.baseUrl ?? ''));
  let apiKey = $state('');
  let saving = $state(false);
  let saved = $state(false);
  let err = $state('');

  $effect(() => {
    tab = initialTab;
  });

  const n = (v) => (typeof v === 'number' ? v.toLocaleString('fr-FR') : '–');
  const isExt = $derived(tab === 'skills' || tab === 'mcp');
  const ctxPct = $derived(ctx?.max ? Math.min(100, Math.round((ctx.used / ctx.max) * 100)) : null);
  const diag = $derived(
    JSON.stringify(
      { settings: { ...settings, apiKey: undefined }, usage, budget, last, session, ctx },
      null,
      2,
    ),
  );

  async function save() {
    saving = true;
    saved = false;
    err = '';
    try {
      await saveSettings({ baseUrl: baseUrl.trim() || null, apiKey: apiKey.trim() || null });
      apiKey = '';
      saved = true;
      await onsaved?.();
    } catch (e) {
      err = String(e);
    }
    saving = false;
  }

  function onBackdrop(e) {
    if (e.target === e.currentTarget) onclose?.();
  }
</script>

<div class="ov" role="presentation" onmousedown={onBackdrop}>
  <div class="dlg" role="dialog" aria-modal="true" aria-label="Réglages">
    <nav class="snav">
      <div class="stitle">Réglages</div>
      {#each TABS as t (t.id)}
        <button class="ni" class:on={tab === t.id} onclick={() => (tab = t.id)}>{t.label}</button>
      {/each}
      <span class="spacer"></span>
      <button class="ni close" onclick={() => onclose?.()}>Fermer <small>Échap</small></button>
    </nav>

    <div class="swpane">
      {#key tab}
        <div class="tabin" class:ext={isExt}>
          {#if tab === 'compte'}
            <h3>Compte France Student</h3>
            <label class="f">
              <span>Adresse de l’API</span>
              <input
                bind:value={baseUrl}
                placeholder="Laisser vide pour l’adresse par défaut"
                spellcheck="false"
                autocomplete="off"
              />
            </label>
            <label class="f">
              <span>
                Clé API
                {#if settings?.hasKey}<span class="badge ro">enregistrée</span>{/if}
              </span>
              <input
                type="password"
                bind:value={apiKey}
                placeholder={settings?.hasKey ? 'Laisser vide pour conserver la clé actuelle' : 'Colle ta clé ici'}
                autocomplete="off"
                spellcheck="false"
              />
            </label>
            <div class="row">
              <button class="primary" onclick={save} disabled={saving}>
                {saving ? 'Enregistrement…' : 'Enregistrer'}
              </button>
              {#if saved && !err}<span class="okmsg">Enregistré ✓</span>{/if}
            </div>
            {#if err}<div class="error">{err}</div>{/if}

            <h3>Budget</h3>
            {#if budget}
              <div class="gauge" class:warn={budget.pct >= 80}>
                <div class="gbar"><i style="width:{budget.pct}%"></i></div>
                <span class="mono">{budget.uncertain ? '~' : ''}{Math.round(budget.pct)} %</span>
              </div>
              {#if budget.uncertain}
                <p class="hint">Comptage incertain côté serveur : la valeur est approximative.</p>
              {/if}
            {:else}
              <p class="hint">Indisponible pour l’instant.</p>
            {/if}
          {:else if tab === 'code'}
            <h3>Mode Code</h3>
            <label class="chk">
              <input type="checkbox" checked={autoAccept} onchange={(e) => onauto?.(e.currentTarget.checked)} />
              Accepter automatiquement les modifications de fichiers
            </label>
            <p class="hint">Ne vaut jamais pour les commandes : elles demandent toujours ta confirmation.</p>

            <h3>Dossier de travail</h3>
            <div class="folder">
              <span class="mono path" title={workdir ?? ''}>{workdir ?? 'Aucun dossier choisi'}</span>
              <button onclick={() => onpick?.()} disabled={busy}>Changer…</button>
            </div>
            <p class="hint">Mémorisé par conversation. L’IA ne lit et n’écrit que dans ce dossier.</p>
          {:else if tab === 'eco'}
            <h3>Mode économie</h3>
            <label class="chk">
              <input type="checkbox" checked={eco} onchange={(e) => oneco?.(e.currentTarget.checked)} />
              Réduire la consommation de tokens
            </label>
            <p class="hint">Contexte et sorties d’outils plus courts : moins cher, un peu moins de détails.</p>

            <h3>Dernier message</h3>
            {#if last}
              <div class="kv">
                <span>Tokens envoyés</span><span>{n(last.prompt)}</span>
                <span>Tokens reçus</span><span>{n(last.output)}</span>
                <span>Appels API</span><span>{n(last.calls)}</span>
                <span>Mode économie</span><span>{last.eco ? 'oui' : 'non'}</span>
              </div>
            {:else}
              <p class="hint">Aucun message envoyé depuis le lancement.</p>
            {/if}

            <h3>Session</h3>
            <div class="kv">
              <span>Messages</span><span>{n(session.messages)}</span>
              <span>Appels API</span><span>{n(session.calls)}</span>
              <span>Tokens envoyés</span><span>{n(session.prompt)}</span>
              <span>Tokens reçus</span><span>{n(session.output)}</span>
            </div>
            <div class="row">
              <button onclick={() => onreset?.()}>Remettre à zéro</button>
            </div>

            <h3>Contexte du chat affiché</h3>
            {#if ctx}
              <div class="gauge" class:warn={ctxPct != null && ctxPct >= 80}>
                <div class="gbar"><i style="width:{ctxPct ?? 0}%"></i></div>
                <span class="mono">{n(ctx.used)}{ctx.max ? ' / ' + n(ctx.max) : ''}</span>
              </div>
            {:else}
              <p class="hint">Inconnu (ouvre ou lance un chat).</p>
            {/if}
            <div class="row">
              <button onclick={() => oncompact?.()} disabled={!chatId || busy}>Compacter le contexte</button>
            </div>
            <p class="hint">Résume l’historique pour libérer de la place. Indisponible pendant une génération.</p>
          {:else if tab === 'skills'}
            <Extensions embedded section="skills" {workdir} {oninsert} onchange={onextchange} />
          {:else if tab === 'mcp'}
            <Extensions embedded section="mcp" {workdir} {oninsert} onchange={onextchange} />
          {:else}
            <h3>Diagnostic</h3>
            <p class="hint">État de l’app, sans la clé API. Colle-le si tu me signales un bug.</p>
            <pre class="raw">{diag}</pre>
          {/if}
        </div>
      {/key}
    </div>
  </div>
</div>

<style>
  .ov {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgba(4, 6, 10, 0.72);
    animation: fs-fade var(--t) both;
  }
  .dlg {
    display: grid;
    grid-template-columns: 188px minmax(0, 1fr);
    width: min(900px, 94vw);
    height: min(640px, 88vh);
    overflow: hidden;
    background: var(--panel);
    border: 1px solid var(--line2);
    border-top: 2px solid var(--accent);
    box-shadow: 8px 8px 0 rgba(0, 0, 0, 0.5);
    animation: fs-pop var(--t-slow) var(--ease) both;
  }

  /* ---------- navigation ---------- */
  .snav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px;
    background: var(--bg);
    border-right: 1px solid var(--line);
  }
  .stitle {
    padding: 2px 10px 12px;
    color: var(--faint);
    font: 11px/1 var(--mono);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .ni {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    width: 100%;
    padding: 8px 10px;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-left: 2px solid transparent;
    color: var(--dim);
  }
  .ni:hover:not(:disabled) {
    background: var(--panel2);
    border-color: transparent;
    color: var(--text);
    transform: translateX(2px);
    box-shadow: none;
  }
  .ni.on,
  .ni.on:hover:not(:disabled) {
    background: var(--raised);
    border-color: var(--line2);
    border-left-color: var(--accent);
    color: var(--text);
    transform: none;
  }
  .ni.close {
    color: var(--faint);
  }
  .ni small {
    font: 10px/1 var(--mono);
    color: var(--faint);
  }

  /* ---------- contenu ---------- */
  .swpane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .tabin {
    flex: 1;
    min-height: 0;
    padding: 20px 26px 26px;
    overflow-y: auto;
    animation: fs-left var(--t-slow) var(--ease) both;
  }
  .tabin.ext {
    display: flex;
    padding: 0;
    overflow: hidden;
  }
  .tabin.ext > :global(.xt) {
    flex: 1 1 0;
    min-height: 0;
    height: auto;
  }
  h3 {
    margin: 22px 0 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
    color: var(--dim);
    font: 11px/1 var(--mono);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  h3:first-child {
    margin-top: 0;
  }
  .f {
    display: block;
    margin: 0 0 14px;
  }
  .f > span {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 5px;
    color: var(--dim);
    font-size: 12.5px;
  }
  .f input {
    width: 100%;
  }
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin: 12px 0 4px;
  }
  .okmsg {
    color: var(--ok);
    font-size: 13px;
    animation: fs-left var(--t) var(--ease) both;
  }
  .hint {
    margin: 8px 0 0;
    color: var(--faint);
    font-size: 12.5px;
    line-height: 1.5;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .path {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    overflow: hidden;
    background: var(--bg);
    border: 1px solid var(--line);
    font-size: 12.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .gauge {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .gbar {
    flex: 1;
    height: 8px;
    overflow: hidden;
    background: var(--raised);
    border: 1px solid var(--line);
  }
  .gbar i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width var(--t-slow) var(--ease);
  }
  .gauge.warn .gbar i {
    background: var(--warn);
  }
  .raw {
    margin: 10px 0 0;
    padding: 12px 14px;
    max-height: 420px;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--line);
    font: 12px/1.5 var(--mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
