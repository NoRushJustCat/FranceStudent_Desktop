// Mode développeur : journal en mémoire (500 entrées max), actif seulement si activé dans Paramètres > Diagnostic.
// Il observe : appels Tauri (invoke), erreurs JS, console.error/warn. Rien n'est envoyé à l'extérieur.
const KEY = 'fs_dev_mode';
const MAX = 500;
const SECRET = /(api[_-]?key|authorization|token|password|secret|cookie)/i;

export const dev = $state({ enabled: false, entries: [] });

let seq = 0;
let installed = false;
let inLog = false;

function redact(v, depth = 0) {
  if (v == null || depth > 6) return v;
  if (Array.isArray(v)) return v.map((x) => redact(x, depth + 1));
  if (typeof v === 'object') {
    const out = {};
    for (const [k, x] of Object.entries(v)) out[k] = SECRET.test(k) ? '***' : redact(x, depth + 1);
    return out;
  }
  return v;
}

function short(v, n = 4000) {
  let s;
  try {
    if (v instanceof Error) s = `${v.name}: ${v.message}`;
    else s = typeof v === 'string' ? v : JSON.stringify(redact(v), null, 2);
  } catch {
    s = String(v);
  }
  if (s == null) s = String(v);
  return s.length > n ? `${s.slice(0, n)}… (+${s.length - n} car.)` : s;
}

export function log(level, source, message, data) {
  if (!dev.enabled || inLog) return;
  inLog = true;
  try {
    dev.entries.push({
      id: ++seq,
      t: Date.now(),
      level,
      source,
      message: short(message, 1000),
      data: data === undefined ? null : short(data, 20000)
    });
    if (dev.entries.length > MAX) dev.entries.splice(0, dev.entries.length - MAX);
  } finally {
    inLog = false;
  }
}

export function clearLog() {
  dev.entries.length = 0;
}

export function exportText() {
  return dev.entries
    .map((e) => {
      const ts = new Date(e.t).toISOString();
      return `[${ts}] ${e.level.toUpperCase()} ${e.source}: ${e.message}${e.data ? `\n${e.data}` : ''}`;
    })
    .join('\n');
}

function install() {
  if (installed || typeof window === 'undefined') return;
  installed = true;

  // Appels vers le backend Rust (get_settings, save_settings, etc.) : durée, arguments (secrets masqués), résultat ou erreur brute.
  const ti = window.__TAURI_INTERNALS__;
  if (ti && typeof ti.invoke === 'function' && !ti.__devPatched) {
    const orig = ti.invoke.bind(ti);
    ti.invoke = async (cmd, args, opts) => {
      if (!dev.enabled || String(cmd).startsWith('plugin:event|')) return orig(cmd, args, opts);
      const t0 = performance.now();
      try {
        const r = await orig(cmd, args, opts);
        log('info', 'invoke', `${cmd} ok (${Math.round(performance.now() - t0)} ms)`, { args, result: r });
        return r;
      } catch (e) {
        log('error', 'invoke', `${cmd} échec (${Math.round(performance.now() - t0)} ms)`, { args, error: e });
        throw e;
      }
    };
    ti.__devPatched = true;
  }

  window.addEventListener('error', (e) => log('error', 'window', e.message, `${e.filename}:${e.lineno}:${e.colno}`));
  window.addEventListener('unhandledrejection', (e) => log('error', 'promise', e.reason?.message || String(e.reason), e.reason));

  for (const lvl of ['error', 'warn']) {
    const orig = console[lvl].bind(console);
    console[lvl] = (...a) => {
      orig(...a);
      log(lvl === 'warn' ? 'warn' : 'error', 'console', a.map((x) => short(x, 500)).join(' '));
    };
  }
}

// Prompt système local : localStorage de cette machine, jamais envoyé au compte. Actif seulement en mode développeur.
const PKEY = 'fs_local_prompt';
export function getLocalPrompt() {
  try {
    return localStorage.getItem(PKEY) ?? '';
  } catch {
    return '';
  }
}
export function setLocalPrompt(v) {
  try {
    localStorage.setItem(PKEY, v);
  } catch {}
}
export const activeLocalPrompt = () => (dev.enabled ? getLocalPrompt().trim() : '');

export function setEnabled(v) {
  dev.enabled = !!v;
  try {
    localStorage.setItem(KEY, v ? '1' : '0');
  } catch {}
  if (v) {
    install();
    log('info', 'dev', 'Mode développeur activé');
  }
}

try {
  if (typeof localStorage !== 'undefined' && localStorage.getItem(KEY) === '1') setEnabled(true);
} catch {}
