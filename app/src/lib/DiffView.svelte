<script>
  // Visualiseur de diff pour les outils edit_file / write_file.
  // Calculé côté interface à partir des arguments de l'appel : aucune commande Rust.
  let { t } = $props();

  const LIMIT = 40;
  let open = $state(true);
  let all = $state(false);

  const norm = (s) => String(s ?? '').replace(/\r\n/g, '\n');

  function toArgs(a) {
    if (a && typeof a === 'object') return a;
    if (typeof a === 'string') {
      try {
        const v = JSON.parse(a);
        return v && typeof v === 'object' ? v : {};
      } catch {
        return {};
      }
    }
    return {};
  }

  function pick(a, keys) {
    for (const k of keys) if (typeof a[k] === 'string') return a[k];
    return null;
  }

  function splitLines(s) {
    if (s === '') return [];
    const l = s.split('\n');
    if (l.length > 1 && l[l.length - 1] === '') l.pop();
    return l;
  }

  // Diff par lignes : préfixe/suffixe communs puis plus longue sous-séquence commune.
  function diffLines(a, b) {
    let p = 0;
    while (p < a.length && p < b.length && a[p] === b[p]) p++;
    let e = 0;
    while (e < a.length - p && e < b.length - p && a[a.length - 1 - e] === b[b.length - 1 - e]) e++;
    const am = a.slice(p, a.length - e);
    const bm = b.slice(p, b.length - e);
    const ops = [];
    for (let i = 0; i < p; i++) ops.push(['c', a[i]]);
    const n = am.length;
    const m = bm.length;
    if (n * m > 4000000) {
      for (const x of am) ops.push(['d', x]);
      for (const x of bm) ops.push(['a', x]);
    } else {
      const w = m + 1;
      const L = new Uint32Array((n + 1) * w);
      for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
          L[i * w + j] =
            am[i] === bm[j] ? L[(i + 1) * w + j + 1] + 1 : Math.max(L[(i + 1) * w + j], L[i * w + j + 1]);
        }
      }
      let i = 0;
      let j = 0;
      while (i < n && j < m) {
        if (am[i] === bm[j]) {
          ops.push(['c', am[i]]);
          i++;
          j++;
        } else if (L[(i + 1) * w + j] >= L[i * w + j + 1]) {
          ops.push(['d', am[i++]]);
        } else {
          ops.push(['a', bm[j++]]);
        }
      }
      while (i < n) ops.push(['d', am[i++]]);
      while (j < m) ops.push(['a', bm[j++]]);
    }
    for (let i = 0; i < e; i++) ops.push(['c', a[a.length - e + i]]);
    return ops;
  }

  const d = $derived.by(() => {
    const name = String(t?.name ?? '');
    if (name !== 'edit_file' && name !== 'write_file') return null;
    const a = toArgs(t?.args);
    const path = pick(a, ['path', 'file_path', 'file', 'filename']) ?? '(fichier)';
    let oldT = '';
    let newT = '';
    let verb = 'Modifié';
    if (name === 'write_file') {
      newT = pick(a, ['content', 'text', 'new_text']) ?? '';
      verb = 'Écrit';
    } else {
      oldT = pick(a, ['old_text', 'old_string']) ?? '';
      newT = pick(a, ['new_text', 'new_string', 'content', 'text']) ?? '';
      if (oldT === '') verb = 'Écrit';
    }
    const ops = diffLines(splitLines(norm(oldT)), splitLines(norm(newT)));
    let o = 0;
    let nn = 0;
    let add = 0;
    let del = 0;
    const rows = ops.map(([k, s]) => {
      if (k === 'c') {
        o++;
        nn++;
        return { k, o, n: nn, s };
      }
      if (k === 'd') {
        o++;
        del++;
        return { k, o, n: null, s };
      }
      nn++;
      add++;
      return { k, o: null, n: nn, s };
    });
    const base = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
    return { path, base, verb, rows, add, del };
  });

  const STATE = { pending: 'à confirmer', running: 'en cours', error: 'échec', denied: 'refusé' };
  const label = $derived(STATE[t?.status] ?? '');
  const shown = $derived(d ? (all ? d.rows : d.rows.slice(0, LIMIT)) : []);
</script>

{#if d}
  <div class="dv" class:off={t?.status === 'error' || t?.status === 'denied'}>
    <button class="head" onclick={() => (open = !open)} title={d.path}>
      <span class="chev">{open ? '▾' : '▸'}</span>
      <span class="verb">{d.verb}</span>
      <span class="file">{d.base}</span>
      <span class="add">+{d.add}</span>
      <span class="del">−{d.del}</span>
      {#if label}<span class="st">{label}</span>{/if}
    </button>
    {#if open}
      <div class="path">{d.path}</div>
      <div class="rows">
        {#each shown as r}
          <div class="r {r.k}">
            <span class="g">{r.o ?? ''}</span>
            <span class="g">{r.n ?? ''}</span>
            <span class="sg">{r.k === 'a' ? '+' : r.k === 'd' ? '−' : ''}</span>
            <code>{r.s === '' ? ' ' : r.s}</code>
          </div>
        {/each}
      </div>
      {#if d.rows.length > LIMIT}
        <button class="more" onclick={() => (all = !all)}>
          {all ? 'Réduire' : `Afficher tout (${d.rows.length} lignes)`}
        </button>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .dv {
    width: 100%;
    margin: 4px 0;
    border: 1px solid var(--line);
    border-radius: var(--r, 8px);
    background: var(--code-bg, var(--panel));
    overflow: hidden;
    font-family: var(--mono);
    font-size: 12px;
  }
  .dv.off {
    opacity: 0.6;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    background: var(--code-head, var(--panel2));
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    background: var(--hover);
  }
  .chev {
    color: var(--faint);
    width: 10px;
  }
  .verb {
    color: var(--dim);
  }
  .file {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .add {
    color: var(--ok);
  }
  .del {
    color: var(--danger);
  }
  .st {
    margin-left: auto;
    color: var(--warn);
    font-size: 11px;
  }
  .path {
    padding: 4px 10px;
    color: var(--faint);
    border-top: 1px solid var(--line);
    word-break: break-all;
  }
  .rows {
    border-top: 1px solid var(--line);
    max-height: 420px;
    overflow: auto;
  }
  .r {
    display: grid;
    grid-template-columns: 38px 38px 16px 1fr;
    line-height: 1.5;
  }
  .r.a {
    background: rgba(52, 211, 153, 0.14);
  }
  .r.d {
    background: rgba(248, 113, 113, 0.14);
  }
  .g {
    padding-right: 6px;
    text-align: right;
    color: var(--faint);
    user-select: none;
  }
  .sg {
    text-align: center;
    user-select: none;
  }
  .r.a .sg {
    color: var(--ok);
  }
  .r.d .sg {
    color: var(--danger);
  }
  code {
    font-family: inherit;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text);
    padding-right: 10px;
  }
  .more {
    width: 100%;
    padding: 6px;
    border: 0;
    border-top: 1px solid var(--line);
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
  .more:hover {
    background: var(--hover);
  }
</style>
