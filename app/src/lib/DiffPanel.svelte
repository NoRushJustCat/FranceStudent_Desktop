<script>
  // Panneau latéral : onglet « Fichiers » (arborescence des fichiers modifiés du chat + diff du fichier choisi)
  // et onglet « Terminal » (shell intégré + commandes de l'IA).
  // Largeur réglable en glissant le bord gauche (variable CSS --dpw, mémorisée).
  import { onMount } from 'svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import DiffView from './DiffView.svelte';
  import Terminal from './Terminal.svelte';
  import { term, tabs, addTerm, closeTerm } from './termstore.svelte.js';
  import { pathOf, statsOf } from './diffcalc.js';

  let { items = [], cwd = null, tab = $bindable('files'), onclose } = $props();

  const activeTerm = $derived(tabs.list.find((s) => s.id === tabs.active) ?? term);

  const EDITS = ['edit_file', 'write_file'];
  const norm = (p) => String(p || '(fichier)').replace(/\\/g, '/');

  let sel = $state(null);
  let collapsed = $state({});

  // --- largeur réglable ---
  const KEY = 'fscode.dpw';
  const MIN = 300;
  let el;
  let w = $state(null);
  let drag = $state(false);

  // Pendant le glissement, on coupe la transition de la grille (sinon la poignée traîne).
  $effect(() => {
    document.documentElement.classList.toggle('dp-drag', drag);
    return () => document.documentElement.classList.remove('dp-drag');
  });

  const maxW = () => Math.max(MIN, Math.min(900, window.innerWidth - 276 - 340));
  const clampW = (v) => Math.round(Math.min(maxW(), Math.max(MIN, v)));

  function apply(v) {
    w = v;
    const root = document.documentElement.style;
    if (v == null) root.removeProperty('--dpw');
    else root.setProperty('--dpw', v + 'px');
  }
  function save() {
    try {
      if (w == null) localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, String(w));
    } catch {}
  }

  onMount(() => {
    try {
      const s = Number(localStorage.getItem(KEY));
      if (s >= MIN) apply(clampW(s));
    } catch {}
    return () => document.documentElement.style.removeProperty('--dpw');
  });

  function down(e) {
    if (e.button !== 0) return;
    drag = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    e.preventDefault();
  }
  function move(e) {
    if (drag) apply(clampW(window.innerWidth - e.clientX));
  }
  function up(e) {
    if (!drag) return;
    drag = false;
    try {
      e.currentTarget.releasePointerCapture(e.pointerId);
    } catch {}
    save();
  }
  function reset() {
    apply(null);
    save();
  }
  function keydown(e) {
    const step = e.shiftKey ? 60 : 20;
    const cur = w ?? el?.getBoundingClientRect().width ?? 400;
    if (e.key === 'ArrowLeft') apply(clampW(cur + step));
    else if (e.key === 'ArrowRight') apply(clampW(cur - step));
    else if (e.key === 'Home' || e.key === 'Escape') apply(null);
    else return;
    e.preventDefault();
    save();
  }
  function onresize() {
    if (w != null) apply(clampW(w));
  }

  const list = $derived(items.filter((i) => EDITS.includes(i?.name)));

  // Un fichier = toutes ses modifs (clé : chemin normalisé).
  const files = $derived.by(() => {
    const m = new Map();
    for (const it of list) {
      const key = norm(pathOf(it));
      let f = m.get(key);
      if (!f) {
        f = { key, edits: [], add: 0, del: 0, bad: false, wait: false };
        m.set(key, f);
      }
      f.edits.push(it);
      if (it.status === 'error' || it.status === 'denied') {
        f.bad = true;
      } else {
        const s = statsOf(it);
        f.add += s.add;
        f.del += s.del;
      }
      if (it.status === 'pending' || it.status === 'running') f.wait = true;
    }
    return [...m.values()];
  });

  function sum(n) {
    let add = 0;
    let del = 0;
    let cnt = 0;
    for (const c of n.kids.values()) {
      if (c.file) {
        add += c.file.add;
        del += c.file.del;
        cnt++;
      } else {
        const s = sum(c);
        add += s.add;
        del += s.del;
        cnt += s.cnt;
      }
    }
    return { add, del, cnt };
  }

  // Arbre aplati en lignes. On retire le préfixe commun, sauf son dernier dossier (racine).
  const rows = $derived.by(() => {
    const fl = files;
    if (!fl.length) return [];
    const parts = fl.map((f) => f.key.split('/').filter(Boolean));
    const dirs = parts.map((p) => p.slice(0, -1));
    const min = Math.min(...dirs.map((d) => d.length));
    let c = 0;
    while (c < min && dirs.every((d) => d[c] === dirs[0][c])) c++;
    const keep = Math.max(0, c - 1);

    const root = { kids: new Map() };
    fl.forEach((f, i) => {
      const segs = parts[i].slice(keep);
      let n = root;
      let id = '';
      segs.forEach((s, k) => {
        const last = k === segs.length - 1;
        id += '/' + s;
        const kk = (last ? 'f:' : 'd:') + s;
        let ch = n.kids.get(kk);
        if (!ch) {
          ch = last ? { name: s, file: f } : { name: s, id, kids: new Map() };
          n.kids.set(kk, ch);
        }
        n = ch;
      });
    });

    const out = [];
    const walk = (n, depth) => {
      const ks = [...n.kids.values()].sort(
        (a, b) => (a.file ? 1 : 0) - (b.file ? 1 : 0) || a.name.localeCompare(b.name)
      );
      for (const ch of ks) {
        if (ch.file) {
          out.push({ t: 'f', depth, name: ch.name, f: ch.file });
        } else {
          out.push({ t: 'd', depth, name: ch.name, id: ch.id, ...sum(ch) });
          if (!collapsed[ch.id]) walk(ch, depth + 1);
        }
      }
    };
    walk(root, 0);
    return out;
  });

  const lastKey = $derived(list.length ? norm(pathOf(list[list.length - 1])) : null);
  const curKey = $derived(sel && files.some((f) => f.key === sel) ? sel : lastKey);
  const curFile = $derived(files.find((f) => f.key === curKey) ?? null);
</script>

<svelte:window {onresize} />

<aside class="dp" class:drag bind:this={el} transition:fly|global={{ x: 48, duration: 280, easing: cubicOut }}>
  <div
    class="grip"
    role="separator"
    aria-orientation="vertical"
    aria-label="Redimensionner le panneau"
    aria-valuenow={w ?? 0}
    aria-valuemin={MIN}
    aria-valuemax={900}
    tabindex="0"
    title="Glisser pour redimensionner · double-clic : largeur par défaut"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    ondblclick={reset}
    onkeydown={keydown}
  ></div>

  <header class="hd">
    <div class="tabs" role="tablist" aria-label="Panneau latéral">
      <button type="button" class="tb" class:on={tab === 'files'} role="tab" aria-selected={tab === 'files'} onclick={() => (tab = 'files')}>
        Fichiers{#if files.length}<span class="n">{files.length}</span>{/if}
      </button>
      <button type="button" class="tb" class:on={tab === 'term'} role="tab" aria-selected={tab === 'term'} onclick={() => (tab = 'term')}>
        Terminal{#if term.unread && tab !== 'term'}<span class="n hot" title="sorties de l'IA non lues">{term.unread}</span>{/if}
      </button>
    </div>
    <span class="sp"></span>
    {#if tab === 'files'}
      <span class="cnt">{files.length} fichier{files.length > 1 ? 's' : ''} · {list.length} modif{list.length > 1 ? 's' : ''}</span>
    {/if}
    <button type="button" class="x" onclick={() => onclose?.()} aria-label="Fermer" title="Fermer">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
    </button>
  </header>

  {#if tab === 'term'}
    <div class="ts" role="tablist" aria-label="Terminaux">
      {#each tabs.list as s (s.id)}
        <span class="tt" class:on={s.id === tabs.active}>
   <button type="button" class="tn" role="tab" aria-selected={s.id === tabs.active} onclick={() => (tabs.active = s.id)}>
     {s.name}{#if s.unread && s.id !== tabs.active}<span class="n hot">{s.unread}</span>{/if}
          </button>
          {#if s.id !== 'main'}
     <button type="button" class="tc" aria-label="Fermer {s.name}" title="Fermer ce terminal" onclick={() => closeTerm(s)}>×</button>
          {/if}
        </span>
      {/each}
      <button type="button" class="tp" aria-label="Nouveau terminal" title="Nouveau terminal" onclick={addTerm}>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
      </button>
    </div>
    {#key tabs.active}
      <Terminal t={activeTerm} {cwd} />
    {/key}
  {:else if list.length === 0}
    <p class="empty">Aucune modification de fichier dans ce chat.</p>
  {:else}
    <div class="split">
      <div class="body">
        {#if curFile}
          {#if curFile.edits.length > 1}
     <p class="sub">{curFile.edits.length} modifications de ce fichier, de la plus ancienne à la plus récente.</p>
          {/if}
   {#key curKey}
     {#each curFile.edits as it}
       <DiffView t={it} />
            {/each}
          {/key}
        {/if}
      </div>

      <div class="tree">
        {#each rows as r (r.t + (r.t === 'd' ? r.id : r.f.key))}
   {#if r.t === 'd'}
     <button
       type="button"
       class="row dir"
       style="padding-left:{4 + r.depth * 8}px"
       aria-expanded={!collapsed[r.id]}
onclick={() => (collapsed[r.id] = !collapsed[r.id])}
     >
       <span class="chev">{collapsed[r.id] ? '▸' : '▾'}</span>
       <span class="nm">{r.name}</span>
     </button>
   {:else}
     <button
       type="button"
       class="row file"
       class:on={r.f.key === curKey}
       style="padding-left:{4 + r.depth * 8}px"
       title={r.f.key}
       onclick={() => (sel = r.f.key)}
     >
       <span class="chev"></span>
<span class="nm">{r.name}</span>
       {#if r.f.wait}<span class="st w" title="en cours / à confirmer">…</span>{/if}
{#if r.f.bad}<span class="st b" title="échec ou refusé">!</span>{/if}
     </button>
   {/if}
        {/each}
      </div>
    </div>
  {/if}
</aside>

<style>
  .dp {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: calc(100vh - var(--titlebar-h, 34px));
    width: var(--dpcol, 100%);
    flex-shrink: 0;
    border-left: 1px solid var(--line);
    background: var(--panel);
    color: var(--text);
    overflow: hidden;
  }
  .dp.drag {
    user-select: none;
  }

  /* Poignée de redimensionnement (bord gauche). */
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: 7px;
    z-index: 5;
    cursor: col-resize;
    touch-action: none;
    background: transparent;
    transition: background 0.12s;
  }
  .grip:hover,
  .grip:focus-visible,
  .dp.drag .grip {
    background: var(--accent-line, rgba(255, 140, 40, 0.5));
    outline: none;
  }

  .hd {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--topbar-h, 52px);
    flex-shrink: 0;
    padding: 0 12px 0 12px;
    border-bottom: 1px solid var(--line);
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }
  .tb {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--dim);
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .tb:hover {
    background: var(--hover);
    color: var(--text);
  }
  .tb.on {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .n {
    padding: 0 6px;
    border-radius: 9px;
    background: var(--hover);
    color: var(--dim);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 16px;
  }
  .tb.on .n {
    color: var(--accent-text);
  }
  .n.hot {
    background: var(--accent-text);
    color: var(--panel);
  }
  .sp {
    flex: 1;
    min-width: 0;
  }
  .cnt {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11.5px;
    color: var(--dim);
    white-space: nowrap;
  }
  .x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
  }
  .x:hover {
    background: var(--hover);
    color: var(--text);
  }

  /* Onglets de terminaux */
  .ts {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
    padding: 4px 10px;
    overflow-x: auto;
    border-bottom: 1px solid var(--line);
  }
  .tt {
    display: inline-flex;
    align-items: center;
    border-radius: 6px;
    flex-shrink: 0;
  }
  .tt.on {
    background: var(--accent-soft);
  }
  .tn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 9px;
    border: 0;
    background: transparent;
    color: var(--dim);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .tt.on .tn {
    color: var(--accent-text);
  }
  .tn:hover {
    color: var(--text);
  }
  .tc {
    width: 18px;
    height: 18px;
    margin-right: 4px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--dim);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }
  .tc:hover {
    background: var(--hover);
    color: var(--text);
  }
  .tp {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  .tp:hover {
    background: var(--hover);
    color: var(--text);
  }

  /* Diff à gauche, arborescence minimaliste à droite. */
  .split {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .tree {
    flex: 0 0 clamp(92px, 30%, 150px);
    min-width: 0;
    overflow: auto;
    padding: 6px 4px;
    border-left: 1px solid var(--line);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    height: 24px;
    padding-right: 4px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-family: var(--mono);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.file.on {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .row.dir {
    color: var(--dim);
  }
  .chev {
    flex-shrink: 0;
    width: 10px;
    color: var(--faint);
    font-size: 10px;
  }
  .nm {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cn {
    flex-shrink: 0;
    color: var(--faint);
    font-size: 11px;
  }
  .add {
    flex-shrink: 0;
    color: var(--ok);
    font-size: 11px;
  }
  .del {
    flex-shrink: 0;
    color: var(--danger);
    font-size: 11px;
  }
  .st {
    flex-shrink: 0;
    font-weight: 700;
  }
  .st.w {
    color: var(--warn);
  }
  .st.b {
    color: var(--danger);
  }

  .body {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 10px;
  }
  .sub {
    margin: 0 0 6px;
    color: var(--faint);
    font-size: 11.5px;
  }
  .empty {
    margin: 24px 8px;
    color: var(--faint);
    font-size: 12.5px;
    text-align: center;
  }
</style>
