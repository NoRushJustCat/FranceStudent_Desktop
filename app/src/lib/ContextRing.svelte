<script>
  let { ctx = null, eco = false, length = 0, maxChars = 1048576 } = $props();

  const nf = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 1 });
  const nf0 = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 0 });
  const tk = (n) =>
    n >= 1e6 ? nf.format(n / 1e6) + '\u202f M' : n >= 1e3 ? nf.format(n / 1e3) + '\u202f k' : nf0.format(n);

  // Ordre d'affichage = ordre du tableau. `prio` = ordre de test des clés du serveur.
  const CATS = [
    { id: 'sys', label: 'Prompt système', color: '#9ca3af', re: /system|syst[eè]me|prompt/i, prio: 5 },
    { id: 'tools', label: 'Définitions d’outils', color: '#a78bfa', re: /tool|outil|definition|d[ée]finition|schema/i, prio: 4 },
    { id: 'rules', label: 'Règles', color: '#34d399', re: /rule|r[eè]gle|instruction|custom|memory|m[ée]moire/i, prio: 3 },
    { id: 'skills', label: 'Skills', color: '#fbbf24', re: /skill/i, prio: 2 },
    { id: 'mcp', label: 'MCP & outils dynamiques', color: '#f472b6', re: /mcp|dynamic|dynamique/i, prio: 1 },
    { id: 'conv', label: 'Conversation', color: '#fb923c', re: /conversation|history|historique/i, prio: 6 }
  ];
  const MATCH = [...CATS].sort((a, b) => a.prio - b.prio);
  const SKIP = /^(prompt_?tokens|tokens|used|max|total|limit|window|percent|pct|threshold|context|compact)|count|^nb|^num|length|size|chars|bytes/i;

  // Cherche des paires (nom, tokens) dans la réponse du serveur, sans connaître sa forme exacte.
  function extract(raw) {
    const out = [];
    const take = (name, v) => {
      const n = Number(v);
      if (name != null && Number.isFinite(n) && n >= 0) out.push({ name: String(name), tokens: Math.round(n) });
    };
    const walk = (o, depth) => {
      if (!o || typeof o !== 'object' || depth > 2) return;
      if (Array.isArray(o)) {
        for (const it of o) {
          if (!it || typeof it !== 'object') continue;
          take(it.name ?? it.label ?? it.id ?? it.key ?? it.category, it.tokens ?? it.value ?? it.used);
        }
        return;
      }
      for (const [k, v] of Object.entries(o)) {
        if (v && typeof v === 'object') walk(v, depth + 1);
        else if (!SKIP.test(k)) take(k, v);
      }
    };
    walk(raw, 0);
    return out;
  }

  const used = $derived(ctx?.used ?? null);
  const max = $derived(ctx?.max ?? null);
  const pct = $derived(used != null && max ? Math.min(100, (used / max) * 100) : 0);

  const rows = $derived.by(() => {
    const acc = new Map();
    for (const r of extract(ctx?.raw)) {
      const cat = MATCH.find((c) => c.re.test(r.name));
      if (!cat) continue;
      acc.set(cat.id, { ...cat, tokens: (acc.get(cat.id)?.tokens ?? 0) + r.tokens });
    }
    const list = [...acc.values()];
    if (list.length && used != null && !acc.has('conv')) {
      const sum = list.reduce((s, r) => s + r.tokens, 0);
      if (used > sum) list.push({ ...CATS.find((c) => c.id === 'conv'), tokens: used - sum });
    }
    return list.sort((a, b) => CATS.indexOf(CATS.find((c) => c.id === a.id)) - CATS.indexOf(CATS.find((c) => c.id === b.id)));
  });

  const compactAt = $derived.by(() => {
    const r = ctx?.raw;
    if (r && typeof r === 'object') {
      for (const k of ['autoCompactAt', 'compactAt', 'compactionThreshold', 'autoCompactThreshold', 'compactThreshold']) {
        const n = Number(r[k]);
        if (Number.isFinite(n) && n > 0) return n;
      }
    }
    return eco && max ? Math.max(20000, Math.floor(max / 3)) : null;
  });

  const segW = (t) => (max ? Math.min(100, (t / max) * 100) : used ? (t / used) * 100 : 0);
  const C = 2 * Math.PI * 9;
  const label = $derived(
    used != null ? `Contexte : ${tk(used)}${max ? ' / ' + tk(max) : ''} jetons` : 'Contexte : pas encore de données'
  );
</script>

<div class="cr">
  <button type="button" class="ring" class:warn={pct >= 80} aria-label={label}>
    <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
      <circle class="track" cx="12" cy="12" r="9" fill="none" stroke-width="3" />
      {#if pct > 0}
        <circle
          class="val"
          cx="12"
          cy="12"
          r="9"
          fill="none"
          stroke-width="3"
          stroke-linecap="round"
          stroke-dasharray="{(pct / 100) * C} {C}"
          transform="rotate(-90 12 12)"
        />
      {/if}
    </svg>
  </button>

  <div class="pop" role="tooltip">
    <div class="card">
      <div class="row">
        <span class="dot" style="background:#fff"></span>
        <b class="grow">Caractères du message</b>
        <span class="mono">{nf0.format(length)} / {nf0.format(maxChars)}</span>
      </div>
      <hr />
      <div class="row">
        <b class="grow">{Math.round(pct)}&#8239;% utilisé</b>
        <span class="mono">{used != null ? tk(used) : '—'} / {max ? tk(max) : '?'} jetons</span>
      </div>
      <div class="bar">
        {#each rows as r (r.id)}
          <i style="width:{segW(r.tokens)}%;background:{r.color}"></i>
        {:else}
          {#if used != null}<i style="width:{segW(used)}%;background:var(--accent)"></i>{/if}
        {/each}
      </div>
      {#each rows as r (r.id)}
        <div class="row">
          <span class="dot" style="background:{r.color}"></span>
          <span class="grow">{r.label}</span>
          <span class="mono">{tk(r.tokens)}</span>
        </div>
      {/each}
      {#if !rows.length}
        <div class="note">
          {used != null ? 'Détail par catégorie non fourni par le serveur.' : 'Envoie un message pour voir l’occupation.'}
        </div>
      {/if}
      {#if compactAt}
        <div class="note">Compaction automatique à partir de {tk(compactAt)} jetons</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .cr {
    position: relative;
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
  }

  .ring {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    cursor: help;
  }

  .ring:hover {
    background: var(--hover, var(--accent-soft));
  }

  .track {
    stroke: var(--line2, var(--line));
  }

  .val {
    stroke: var(--accent);
    transition: stroke-dasharray 0.3s ease;
  }

  .ring.warn .val {
    stroke: var(--danger, #ef4444);
  }

  .pop {
    display: none;
    position: absolute;
    right: -8px;
    bottom: 100%;
    padding-bottom: 10px;
    z-index: 40;
  }

  .cr:hover .pop,
  .cr:focus-within .pop {
    display: block;
  }

  .card {
    width: 330px;
    max-width: calc(100vw - 32px);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 7px;
    background: var(--raised);
    border: 1px solid var(--accent-line, var(--line2));
    border-radius: 12px;
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.42);
    color: var(--text);
    font-size: 0.8rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .grow {
    flex: 1;
  }

  .mono {
    font-family: var(--font-mono, var(--mono, monospace));
    font-size: 0.76rem;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  hr {
    width: 100%;
    margin: 2px 0;
    border: none;
    border-top: 1px solid var(--line);
  }

  .bar {
    display: flex;
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: var(--line);
    gap: 1px;
  }

  .bar i {
    display: block;
    height: 100%;
    min-width: 2px;
  }

  .note {
    margin-top: 2px;
    color: var(--dim);
    font-size: 0.74rem;
  }
</style>
