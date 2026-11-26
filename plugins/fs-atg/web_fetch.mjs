import { lookup } from 'node:dns/promises';
import net from 'node:net';
import { logEvent } from './logger.mjs';

const MAX_BYTES = 2 * 1024 * 1024;
const MAX_REDIRECTS = 5;

function isPrivateIp(ip) {
  const addr = String(ip || '').trim().toLowerCase().replace(/^\[|\]$/g, '');
  if (!addr) return true;
  if (addr.startsWith('::ffff:')) return isPrivateIp(addr.slice(7));
  if (net.isIPv4(addr)) {
    const parts = addr.split('.').map(Number);
    if (parts.length !== 4 || parts.some((n) => Number.isNaN(n))) return true;
    const [a, b] = parts;
    if (a === 0 || a === 10 || a === 127) return true;
    if (a === 169 && b === 254) return true;
    if (a === 172 && b >= 16 && b <= 31) return true;
    if (a === 192 && b === 168) return true;
    if (a === 100 && b >= 64 && b <= 127) return true;
    if (a >= 224) return true;
    return false;
  }
  if (net.isIPv6(addr)) {
    if (addr === '::' || addr === '::1') return true;
    if (addr.startsWith('fe80:') || addr.startsWith('fc') || addr.startsWith('fd')) return true;
    return false;
  }
  return true;
}

async function validatePublicHttpUrl(rawUrl) {
  let parsed;
  try {
    parsed = new URL(String(rawUrl || '').trim());
  } catch {
    throw new Error(`URL invalide : "${rawUrl}"`);
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error(`Protocole refusé (${parsed.protocol}) : seuls http:// et https:// sont autorisés.`);
  }
  const host = parsed.hostname.toLowerCase();
  if (!host || host === 'localhost' || host.endsWith('.local') || host.endsWith('.internal') || host === 'metadata.google.internal') {
    throw new Error(`Accès hôte local/interne bloqué (SSRF) : ${host}`);
  }
  if (net.isIP(host.replace(/^\[|\]$/g, ''))) {
    if (isPrivateIp(host)) throw new Error(`Accès IP privée/locale bloqué (SSRF) : ${host}`);
    return parsed;
  }
  let records;
  try {
    records = await lookup(host, { all: true });
  } catch (err) {
    throw new Error(`Impossible de résoudre l'hôte "${host}" : ${err.message}`);
  }
  if (!records?.length || records.some((r) => isPrivateIp(r.address))) {
    throw new Error(`Résolution DNS vers une adresse privée/locale bloquée (SSRF) : ${host}`);
  }
  return parsed;
}

function decodeHtmlEntities(str) {
  return String(str || '')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&#(\d+);/g, (_, n) => {
      const code = Number(n);
      return Number.isFinite(code) ? String.fromCodePoint(code) : '';
    })
    .replace(/&#x([0-9a-f]+);/gi, (_, hex) => {
      const code = parseInt(hex, 16);
      return Number.isFinite(code) ? String.fromCodePoint(code) : '';
    });
}

function htmlToMarkdown(html) {
  const raw = String(html || '');
  const titleMatch = /<title[^>]*>([\s\S]*?)<\/title>/i.exec(raw);
  const title = titleMatch ? decodeHtmlEntities(titleMatch[1].replace(/\s+/g, ' ').trim()) : '';

  let body = raw
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<(script|style|noscript|svg|iframe|object|embed)\b[^>]*>[\s\S]*?<\/\1>/gi, '');

  body = body.replace(/<h([1-6])\b[^>]*>([\s\S]*?)<\/h\1>/gi, (_, lvl, inner) => {
    const clean = decodeHtmlEntities(inner.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim());
    return clean ? `\n\n${'#'.repeat(Number(lvl))} ${clean}\n\n` : '\n';
  });
  body = body.replace(/<pre\b[^>]*>([\s\S]*?)<\/pre>/gi, (_, inner) => {
    const clean = decodeHtmlEntities(inner.replace(/<[^>]+>/g, '')).trim();
    return clean ? `\n\n\`\`\`\n${clean}\n\`\`\`\n\n` : '\n';
  });
  body = body.replace(/<code\b[^>]*>([\s\S]*?)<\/code>/gi, (_, inner) => {
    const clean = decodeHtmlEntities(inner.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim());
    return clean ? `\`${clean}\`` : '';
  });
  body = body.replace(/<a\b[^>]*href=["']([^"']+)["'][^>]*>([\s\S]*?)<\/a>/gi, (_, href, inner) => {
    const label = decodeHtmlEntities(inner.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim());
    const cleanHref = decodeHtmlEntities(href.trim());
    if (!label) return '';
    if (!cleanHref || cleanHref.startsWith('javascript:')) return label;
    return `[${label}](${cleanHref})`;
  });
  body = body.replace(/<li\b[^>]*>([\s\S]*?)<\/li>/gi, (_, inner) => {
    const clean = decodeHtmlEntities(inner.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim());
    return clean ? `\n- ${clean}` : '';
  });
  body = body
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<\/(?:p|div|section|article|header|footer|aside|main|nav|tr|blockquote|ul|ol|table)>/gi, '\n\n')
    .replace(/<(?:td|th)\b[^>]*>/gi, ' | ')
    .replace(/<[^>]+>/g, '');

  const markdown = decodeHtmlEntities(body)
    .replace(/\r\n/g, '\n')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();

  return { title, markdown };
}

async function readBoundedText(res, maxBytes = MAX_BYTES) {
  if (!res.body?.getReader) {
    const t = await res.text();
    return t.slice(0, maxBytes);
  }
  const reader = res.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value) {
        total += value.byteLength;
        if (total > maxBytes) {
          const keep = value.subarray(0, Math.max(0, value.byteLength - (total - maxBytes)));
          if (keep.byteLength > 0) chunks.push(keep);
          break;
        }
        chunks.push(value);
      }
    }
  } finally {
    try { await reader.cancel(); } catch {}
  }
  return Buffer.concat(chunks.map((c) => Buffer.from(c))).toString('utf8');
}

/** Fetches a public HTTP(S) URL with SSRF protection and converts HTML to clean Markdown. */
export async function executeWebFetch({
  url,
  maxChars = 12_000,
  raw = false,
  timeoutMs = 15_000,
} = {}) {
  let currentUrl = (await validatePublicHttpUrl(url)).toString();
  const boundedMaxChars = Math.max(500, Math.min(80_000, Math.round(Number(maxChars) || 12_000)));
  const boundedTimeout = Math.max(3000, Math.min(30_000, Math.round(Number(timeoutMs) || 15_000)));

  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), boundedTimeout);

  try {
    let response = null;
    for (let hop = 0; hop <= MAX_REDIRECTS; hop++) {
      await validatePublicHttpUrl(currentUrl);
      response = await fetch(currentUrl, {
        method: 'GET',
        redirect: 'manual',
        signal: ac.signal,
        headers: {
          'User-Agent': 'FranceStudent-ATG/2.6.0 (+https://ia.francestudent.org)',
          Accept: 'text/html,application/xhtml+xml,application/json,text/plain;q=0.9,*/*;q=0.5',
        },
      });
      if (response.status >= 300 && response.status < 400) {
        const loc = response.headers.get('location');
        if (!loc) break;
        currentUrl = new URL(loc, currentUrl).toString();
        continue;
      }
      break;
    }

    const status = response.status;
    const contentType = String(response.headers.get('content-type') || '').toLowerCase();
    const rawBody = await readBoundedText(response, MAX_BYTES);

    let title = '';
    let content = rawBody;
    if (!raw && (contentType.includes('text/html') || contentType.includes('application/xhtml+xml') || /^\s*<!doctype\s+html|<html[\s>]/i.test(rawBody))) {
      const converted = htmlToMarkdown(rawBody);
      title = converted.title;
      content = converted.markdown;
    } else if (!raw && contentType.includes('application/json')) {
      try {
        content = JSON.stringify(JSON.parse(rawBody), null, 2);
      } catch { /* keep rawBody */ }
    }

    const totalChars = content.length;
    const truncated = totalChars > boundedMaxChars;
    const outContent = truncated ? `${content.slice(0, boundedMaxChars)}\n\n… [tronqué à ${boundedMaxChars} / ${totalChars} caractères]` : content;

    logEvent('info', 'mcp', 'web_fetch.ok', `Fetch HTTP ${status} (${currentUrl}, ${totalChars} car.).`, { status, totalChars });
    return {
      ok: status >= 200 && status < 400,
      url: String(url),
      finalUrl: currentUrl,
      status,
      contentType: contentType.split(';')[0] || 'unknown',
      title,
      content: outContent,
      totalChars,
      truncated,
    };
  } finally {
    clearTimeout(timer);
  }
}
