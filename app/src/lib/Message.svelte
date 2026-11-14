<script>
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';

  // Contenu d'une réponse de l'IA : raisonnement (repliable) + markdown.
  // streaming : true tant que la réponse est en cours d'écriture.
  let { text = '', reasoning = '', streaming = false } = $props();

  // Nettoyage des blocs ```mcp-call ... ``` (affichés sous forme de cartes d'outils)
  const cleanText = $derived.by(() => {
    const raw = String(text ?? '');
    return raw.replace(/```mcp-call[\s\S]*?```/gi, '').replace(/\n{3,}/g, '\n\n');
  });

  const html = $derived(
    DOMPurify.sanitize(marked.parse(cleanText, { gfm: true, breaks: true, async: false })),
  );

  let root = $state();

  // Un bouton « Copier » sur chaque bloc de code (re-posé à chaque rendu).
  $effect(() => {
    html;
    if (!root) return;
    for (const pre of root.querySelectorAll('pre')) {
      if (pre.querySelector(':scope > button.copy')) continue;
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'copy';
      b.textContent = 'Copier';
      pre.appendChild(b);
    }
  });

  function onClick(e) {
    const el = e.target instanceof Element ? e.target : null;
    if (!el) return;
    // Les liens ne doivent pas faire naviguer la webview.
    if (el.closest('a')) {
      e.preventDefault();
      return;
    }
    const btn = el.closest('button.copy');
    if (!btn) return;
    const pre = btn.closest('pre');
    const src = pre?.querySelector('code') ?? pre;
    const code = (src?.textContent ?? '').replace(/\n$/, '');
    navigator.clipboard
      ?.writeText(code)
      .then(() => {
        btn.textContent = 'Copié ✓';
        setTimeout(() => {
          if (btn.isConnected) btn.textContent = 'Copier';
        }, 1400);
      })
      .catch(() => {
        btn.textContent = 'Échec';
      });
  }

  const thinking = $derived(streaming && !text);
</script>

<div class="msg">
  {#if reasoning}
    <details class="think" open={thinking}>
      <summary>{thinking ? 'Réflexion en cours…' : 'Raisonnement'}</summary>
      <pre>{reasoning}</pre>
    </details>
  {/if}

  {#if text}
    <div class="md" class:streaming bind:this={root} role="presentation" onclick={onClick}>{@html html}</div>
  {:else if streaming && !reasoning}
    <div class="dots" aria-label="L'IA écrit"><i></i><i></i><i></i></div>
  {/if}
</div>

<style>
  .msg {
    min-width: 0;
  }

  /* ---- raisonnement ---- */
  .think {
    margin: 0 0 10px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--violet);
    background: var(--panel);
    font-size: 13px;
  }
  .think summary {
    cursor: pointer;
    padding: 6px 12px;
    color: var(--dim);
    user-select: none;
    transition: color var(--t-fast), background var(--t-fast);
  }
  .think summary:hover {
    background: var(--panel2);
    color: var(--text);
  }
  .think[open] summary {
    border-bottom: 1px solid var(--line);
  }
  .think[open] pre {
    animation: fs-fade var(--t) both;
  }
  .think pre {
    margin: 0;
    padding: 10px 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: 12.5px/1.55 var(--sans);
    color: var(--dim);
    max-height: 260px;
    overflow: auto;
  }

  /* ---- indicateur de frappe : trois carrés ---- */
  .dots {
    display: inline-flex;
    gap: 5px;
    padding: 10px 2px;
  }
  .dots i {
    width: 7px;
    height: 7px;
    background: var(--accent);
    animation: fs-dot 1.2s infinite ease-in-out;
  }
  .dots i:nth-child(2) {
    animation-delay: 0.15s;
  }
  .dots i:nth-child(3) {
    animation-delay: 0.3s;
  }

  /* ---- markdown ---- */
  .md {
    line-height: 1.65;
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .md > :global(:first-child) {
    margin-top: 0;
  }
  .md > :global(:last-child) {
    margin-bottom: 0;
  }
  .md :global(p) {
    margin: 0 0 0.85em;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4) {
    margin: 1.3em 0 0.5em;
    line-height: 1.3;
    letter-spacing: -0.2px;
  }
  .md :global(h1) {
    font-size: 1.45em;
  }
  .md :global(h2) {
    font-size: 1.25em;
  }
  .md :global(h3) {
    font-size: 1.08em;
  }
  .md :global(ul),
  .md :global(ol) {
    margin: 0 0 0.85em;
    padding-left: 1.5em;
  }
  .md :global(li) {
    margin: 0.2em 0;
  }
  .md :global(li > p) {
    margin: 0.3em 0;
  }
  .md :global(a) {
    color: #7fa8ff;
    text-decoration: underline;
    text-decoration-color: rgba(127, 168, 255, 0.4);
    text-underline-offset: 2px;
    transition: text-decoration-color var(--t-fast);
  }
  .md :global(a:hover) {
    text-decoration-color: #7fa8ff;
  }
  .md :global(strong) {
    font-weight: 650;
  }
  .md :global(hr) {
    border: none;
    border-top: 1px solid var(--line);
    margin: 1.2em 0;
  }
  .md :global(blockquote) {
    margin: 0 0 0.85em;
    padding: 2px 0 2px 14px;
    border-left: 3px solid var(--accent-line);
    color: var(--dim);
  }
  .md :global(code) {
    font-family: var(--mono);
    font-size: 0.88em;
    background: var(--raised);
    border: 1px solid var(--line);
    padding: 1px 5px;
  }
  .md :global(pre) {
    position: relative;
    margin: 0 0 0.95em;
    padding: 14px 16px;
    background: #090c11;
    border: 1px solid var(--line);
    border-top: 2px solid var(--line2);
    overflow-x: auto;
    line-height: 1.5;
    transition: border-color var(--t);
  }
  .md :global(pre:hover) {
    border-top-color: var(--accent);
  }
  .md :global(pre code) {
    background: none;
    border: none;
    padding: 0;
    font-size: 12.8px;
    white-space: pre;
  }
  .md :global(pre button.copy) {
    position: absolute;
    top: 8px;
    right: 8px;
    padding: 2px 9px;
    font-size: 11.5px;
    background: var(--raised);
    color: var(--dim);
    opacity: 0;
  }
  .md :global(pre:hover button.copy),
  .md :global(pre button.copy:focus-visible) {
    opacity: 1;
  }
  .md :global(table) {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    border-collapse: collapse;
    margin: 0 0 0.95em;
    font-size: 13.5px;
  }
  .md :global(th),
  .md :global(td) {
    border: 1px solid var(--line);
    padding: 6px 12px;
    text-align: left;
  }
  .md :global(th) {
    background: var(--panel2);
    font-weight: 600;
  }
  .md :global(img) {
    max-width: 100%;
  }

  /* curseur bloc qui clignote pendant l'écriture */
  .md.streaming > :global(p:last-child)::after,
  .md.streaming > :global(li:last-child)::after {
    content: '';
    display: inline-block;
    width: 0.55em;
    height: 1.05em;
    margin-left: 3px;
    vertical-align: text-bottom;
    background: var(--accent);
    animation: fs-blink 1s steps(1) infinite;
  }
</style>
