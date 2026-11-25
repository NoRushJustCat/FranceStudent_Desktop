import http from 'node:http';
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { logEvent } from './logger.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
export const CANVAS_DIR = join(__dirname, 'sidecars', 'fs_atg_panel', '.fsatg_canvas');

let canvasServer = null;
let canvasPort = null;

const DARK_SCROLLBAR_CSS = '<style id="fs-canvas-chrome">html{color-scheme:dark}::-webkit-scrollbar{width:6px;height:6px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:rgba(255,255,255,.16);border-radius:999px}::-webkit-scrollbar-thumb:hover{background:rgba(255,255,255,.28)}</style>';

function injectDarkChrome(html) {
  if (/id=["']fs-canvas-chrome["']/i.test(html)) return html;
  if (/<\/head>/i.test(html)) return html.replace(/<\/head>/i, `${DARK_SCROLLBAR_CSS}\n</head>`);
  if (/<body[\s>]/i.test(html)) return html.replace(/<body/i, `${DARK_SCROLLBAR_CSS}<body`);
  return DARK_SCROLLBAR_CSS + html;
}

function wrapArtifactHtml({ title = 'Artifact France Student', type = 'html', content = '' }) {
  const kind = String(type || 'html').toLowerCase();
  if (kind === 'html' && /<!doctype html>|<html[\s>]/i.test(content)) {
    return injectDarkChrome(content);
  }
  if (kind === 'svg') {
    return injectDarkChrome(`<!doctype html><html lang="fr"><head><meta charset="utf-8"><title>${title}</title><style>body{margin:0;background:#0D0D0F;color:#F5F5F7;display:flex;align-items:center;justify-content:center;min-height:100vh;font-family:-apple-system,BlinkMacSystemFont,system-ui,sans-serif}</style></head><body>${content}</body></html>`);
  }
  if (kind === 'mermaid') {
    return injectDarkChrome(`<!doctype html><html lang="fr"><head><meta charset="utf-8"><title>${title}</title><script type="module">import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';mermaid.initialize({startOnLoad:true,theme:'dark'});</script><style>body{margin:0;padding:24px;background:#0D0D0F;color:#F5F5F7;font-family:-apple-system,BlinkMacSystemFont,system-ui,sans-serif}</style></head><body><pre class="mermaid">${content}</pre></body></html>`);
  }
  return injectDarkChrome(`<!doctype html><html lang="fr"><head><meta charset="utf-8"><title>${title}</title><style>body{margin:0;padding:20px;background:#0D0D0F;color:#F5F5F7;font-family:-apple-system,BlinkMacSystemFont,system-ui,sans-serif}</style></head><body>${content}</body></html>`);
}

async function ensureCanvasServer() {
  if (canvasServer && canvasPort) return canvasPort;
  mkdirSync(CANVAS_DIR, { recursive: true });
  return await new Promise((resolve, reject) => {
    const srv = http.createServer((req, res) => {
      const urlPath = decodeURIComponent((req.url || '/').split('?')[0]).replace(/^\/+/, '');
      const target = join(CANVAS_DIR, basename(urlPath || 'index.html'));
      if (!existsSync(target)) {
        res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
        res.end('Artifact introuvable.');
        return;
      }
      const ext = extname(target).toLowerCase();
      const mime = ext === '.svg' ? 'image/svg+xml' : (ext === '.json' ? 'application/json' : 'text/html; charset=utf-8');
      res.writeHead(200, { 'Content-Type': mime, 'Cache-Control': 'no-store' });
      const raw = readFileSync(target);
      if (ext === '.html' || ext === '') {
        res.end(injectDarkChrome(raw.toString('utf8')));
      } else {
        res.end(raw);
      }
    });
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      canvasServer = srv;
      canvasPort = srv.address().port;
      srv.unref();
      resolve(canvasPort);
    });
  });
}

export async function executeCanvasAction(params = {}) {
  const action = String(params?.action || 'list').trim().toLowerCase();
  mkdirSync(CANVAS_DIR, { recursive: true });

  if (action === 'create' || action === 'update') {
    const id = String(params.id || params.name || `artifact-${Date.now()}`).replace(/[^a-zA-Z0-9._-]/g, '-');
    const type = String(params.type || 'html').toLowerCase();
    const title = String(params.title || id);
    const content = String(params.content || '');
    if (!content.trim()) throw new Error('Paramètre "content" requis pour créer un artifact Canvas.');

    const html = wrapArtifactHtml({ title, type, content });
    const fileName = id.endsWith('.html') ? id : `${id}.html`;
    const filePath = join(CANVAS_DIR, fileName);
    writeFileSync(filePath, html, 'utf-8');
    const port = await ensureCanvasServer();
    const previewUrl = `http://127.0.0.1:${port}/${encodeURIComponent(fileName)}`;
    logEvent('info', 'canvas', 'canvas.save', `Artifact Canvas créé : ${fileName}`, { previewUrl });
    return {
      ok: true,
      action,
      id: fileName,
      title,
      type,
      path: filePath,
      previewUrl,
      bytes: Buffer.byteLength(html, 'utf-8'),
    };
  }

  if (action === 'list') {
    const files = existsSync(CANVAS_DIR)
      ? readdirSync(CANVAS_DIR).filter((f) => f.endsWith('.html') || f.endsWith('.svg'))
      : [];
    const port = files.length ? await ensureCanvasServer() : canvasPort;
    const artifacts = files.map((f) => {
      const full = join(CANVAS_DIR, f);
      const st = statSync(full);
      return {
        id: f,
        path: full,
        bytes: st.size,
        updatedAt: st.mtimeMs,
        previewUrl: port ? `http://127.0.0.1:${port}/${encodeURIComponent(f)}` : null,
      };
    }).sort((a, b) => b.updatedAt - a.updatedAt);
    return { ok: true, action: 'list', count: artifacts.length, artifacts };
  }

  if (action === 'get') {
    const id = basename(String(params.id || ''));
    const full = join(CANVAS_DIR, id.endsWith('.html') || id.endsWith('.svg') ? id : `${id}.html`);
    if (!existsSync(full)) throw new Error(`Artifact introuvable : ${id}`);
    const port = await ensureCanvasServer();
    return {
      ok: true,
      id: basename(full),
      path: full,
      previewUrl: `http://127.0.0.1:${port}/${encodeURIComponent(basename(full))}`,
      content: readFileSync(full, 'utf-8'),
    };
  }

  if (action === 'clear') {
    const files = existsSync(CANVAS_DIR) ? readdirSync(CANVAS_DIR) : [];
    for (const f of files) {
      try { unlinkSync(join(CANVAS_DIR, f)); } catch {}
    }
    return { ok: true, action: 'clear', removed: files.length };
  }

  throw new Error(`Action fs_canvas inconnue : "${action}". Utilise create, update, list, get ou clear.`);
}
