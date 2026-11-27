// Vérifie la syntaxe du <script> inline du panneau (équivalent du `node -e "... new vm.Script(...)"`).
// Usage : node plugins/fs-atg/tests/check-html-script.mjs   (ou via `npm run check` / `npm test`)
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
export const PANEL_HTML = join(here, '..', 'sidecars', 'fs_atg_panel', 'index.html');

/** Renvoie les <script> inline (sans attribut src) : [{ code, line }]. */
export function extractInlineScripts(html) {
  const out = [];
  const re = /<script(?![^>]*\bsrc=)[^>]*>([\s\S]*?)<\/script>/gi;
  let m;
  while ((m = re.exec(html))) {
    const line = html.slice(0, m.index).split('\n').length;
    out.push({ code: m[1], line });
  }
  return out;
}

export function checkPanelScripts(file = PANEL_HTML) {
  const html = readFileSync(file, 'utf8');
  const scripts = extractInlineScripts(html);
  if (!scripts.length) throw new Error(`Aucun <script> inline trouvé dans ${file}`);
  for (const s of scripts) new vm.Script(s.code, { filename: `${file}#script@L${s.line}` });
  return scripts.length;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    const n = checkPanelScripts();
    console.log(`JS OK (${n} script(s) inline compilé(s))`);
  } catch (e) {
    console.error(`JS ERREUR : ${e.message}`);
    process.exit(1);
  }
}
