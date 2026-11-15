// Calcul de diff partagé par le panneau « Fichiers modifiés ».
// DiffView.svelte garde sa propre copie du même algorithme.

export function toArgs(a) {
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

export function pathOf(item) {
  return pick(toArgs(item?.args), ['path', 'file_path', 'file', 'filename']) ?? '';
}

const norm = (s) => String(s ?? '').replace(/\r\n/g, '\n');

function splitLines(s) {
  if (s === '') return [];
  const l = s.split('\n');
  if (l.length > 1 && l[l.length - 1] === '') l.pop();
  return l;
}

// Préfixe/suffixe communs puis plus longue sous-séquence commune.
// Retourne le nombre de lignes ajoutées / retirées.
function countDiff(a, b) {
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p++;
  let e = 0;
  while (e < a.length - p && e < b.length - p && a[a.length - 1 - e] === b[b.length - 1 - e]) e++;
  const am = a.slice(p, a.length - e);
  const bm = b.slice(p, b.length - e);
  const n = am.length;
  const m = bm.length;
  if (n * m > 4000000) return { add: m, del: n };
  const w = m + 1;
  const L = new Uint32Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      L[i * w + j] =
        am[i] === bm[j] ? L[(i + 1) * w + j + 1] + 1 : Math.max(L[(i + 1) * w + j], L[i * w + j + 1]);
    }
  }
  const common = L[0];
  return { add: m - common, del: n - common };
}

const cache = new Map();

export function statsOf(item) {
  const name = String(item?.name ?? '');
  const a = toArgs(item?.args);
  const key = name + '\0' + JSON.stringify(a);
  const hit = cache.get(key);
  if (hit) return hit;
  let oldT = '';
  let newT = '';
  if (name === 'write_file') {
    newT = pick(a, ['content', 'text', 'new_text']) ?? '';
  } else {
    oldT = pick(a, ['old_text', 'old_string']) ?? '';
    newT = pick(a, ['new_text', 'new_string', 'content', 'text']) ?? '';
  }
  const r = countDiff(splitLines(norm(oldT)), splitLines(norm(newT)));
  if (cache.size > 200) cache.clear();
  cache.set(key, r);
  return r;
}
