<script>
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import hljs from 'highlight.js';

  let {
    text = '',
    copyText = '',
    reasoning = '',
    streaming = false,
    showActions = true,
    images = [],
    pinned = false,
    onopenurl,
    onfork,
    onpin
  } = $props();

  let bodyEl = $state(null);
  let thinkOpen = $state(false);
  let copied = $state(false);
  let activityIndex = $state(0);
  const activityWords = ['Réflexion', 'Exploration', 'Analyse', 'En cours'];

  $effect(() => {
    if (!streaming || text) return;
    activityIndex = 0;
    const timer = setInterval(() => {
      activityIndex = (activityIndex + 1) % activityWords.length;
    }, 3000);
    return () => clearInterval(timer);
  });

  marked.setOptions({
    gfm: true,
    breaks: true
  });

  let html = $derived.by(() => {
    const raw = marked.parse(text || '');
    return DOMPurify.sanitize(typeof raw === 'string' ? raw : '');
  });

  function enhanceCodeBlocks(el) {
    const blocks = el.querySelectorAll('pre > code');
    blocks.forEach((codeEl) => {
      const pre = codeEl.parentElement;
      if (!pre) return;

      const langMatch = codeEl.className.match(/language-([\w-]+)/);
      const lang = langMatch ? langMatch[1] : '';

      if (!codeEl.dataset.highlighted) {
        if (lang && hljs.getLanguage(lang)) {
          codeEl.innerHTML = hljs.highlight(codeEl.textContent || '', { language: lang }).value;
        } else {
          hljs.highlightElement(codeEl);
        }
        codeEl.dataset.highlighted = '1';
      }

      if (!pre.parentElement?.classList.contains('code-wrap')) {
        const wrap = document.createElement('div');
        wrap.className = 'code-wrap';

        const head = document.createElement('div');
        head.className = 'code-head';

        const label = document.createElement('span');
        label.className = 'code-lang';
        label.textContent = lang || 'code';

        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'code-copy';
        btn.setAttribute('aria-label', 'Copier le code');
        const copyIconSvg =
          '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>';
        const checkIconSvg =
          '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>';
        btn.innerHTML = `${copyIconSvg}<span>Copier le code</span>`;
        btn.addEventListener('click', async () => {
          try {
            await navigator.clipboard.writeText(codeEl.textContent || '');
            btn.innerHTML = `${checkIconSvg}<span>Copié</span>`;
            btn.classList.add('ok');
            setTimeout(() => {
              if (!btn.isConnected) return;
              btn.innerHTML = `${copyIconSvg}<span>Copier le code</span>`;
              btn.classList.remove('ok');
            }, 1500);
          } catch {
            // ignore clipboard errors
          }
        });

        head.append(label, btn);
        pre.parentNode?.insertBefore(wrap, pre);
        wrap.append(head, pre);
      }
    });
  }

  $effect(() => {
    const el = bodyEl;
    const _content = html;
    if (!el) return;

    if (!streaming) {
      enhanceCodeBlocks(el);
      return;
    }

    const timer = setTimeout(() => {
      enhanceCodeBlocks(el);
    }, 150);

    return () => clearTimeout(timer);
  });

  function handleBodyClick(e) {
    const anchor = e.target?.closest?.('a[href]');
    if (!anchor) return;
    const href = anchor.getAttribute('href');
    if (!href) return;
    e.preventDefault();
    onopenurl?.(href);
  }

  async function copyMessage() {
    const payload = copyText || text;
    if (!payload) return;
    try {
      await navigator.clipboard.writeText(payload);
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 1500);
    } catch {
      // ignore clipboard errors
    }
  }
</script>

<div class="msg-wrap" class:streaming>
  {#if streaming && !text}
    <div class="activity" role="status" aria-label="Génération en cours">
      <span class="activity-star" aria-hidden="true">✳</span>
      <span aria-hidden="true">{activityWords[activityIndex]}…</span>
    </div>
  {/if}
  {#if reasoning}
    <div class="think">
      <button
        type="button"
        class="think-toggle"
        aria-expanded={thinkOpen}
        onclick={() => (thinkOpen = !thinkOpen)}
      >
        <span class="think-dot" class:running={streaming}></span>
        <span>Raisonnement</span>
        <span class="think-chevron" class:open={thinkOpen} aria-hidden="true">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </span>
      </button>
      {#if thinkOpen}
        <div class="think-body">{reasoning}</div>
      {/if}
    </div>
  {/if}

  {#if images?.length}
    <div class="msg-images">
      {#each images as src, i (i)}
        <img {src} alt="Pièce jointe {i + 1}" class="msg-img" />
      {/each}
    </div>
  {/if}

  {#if text || !streaming}
    {#if text}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="msg-body" bind:this={bodyEl} onclick={handleBodyClick}>
        {@html html}
      </div>
    {/if}
  {/if}

  {#if !streaming && showActions && (text || copyText)}
    <div class="msg-actions" class:visible={pinned}>
      <button type="button" class="act-btn" class:ok={copied} onclick={copyMessage}>
        {#if copied}
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>Copié</span>
        {:else}
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
          <span>Copier</span>
        {/if}
      </button>
      {#if onpin}
        <button type="button" class="act-btn" class:pinned={pinned} aria-pressed={pinned} onclick={() => onpin?.()}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill={pinned ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z" />
          </svg>
          <span>{pinned ? 'Dans les signets' : 'Ajouter aux signets'}</span>
        </button>
      {/if}
      {#if onfork}
        <button type="button" class="act-btn" onclick={() => onfork?.()}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 20h9" />
            <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
          </svg>
          <span>Créer une branche</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .activity { display: flex; align-items: center; gap: 8px; color: var(--text-muted, #999); font-size: 13px; margin: 4px 0; }
  .activity-star { color: var(--accent, #d69b73); display: inline-block; animation: activity-pulse 1.4s ease-in-out infinite; }
  @keyframes activity-pulse { 50% { opacity: .4; transform: rotate(45deg); } }
  @media (prefers-reduced-motion: reduce) { .activity-star { animation: none; } }
  .msg-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
    animation: fs-rise 180ms ease-out;
  }

  .think {
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--panel2);
    overflow: hidden;
  }

  .think-toggle {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border: none;
    background: transparent;
    color: var(--dim);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    text-align: left;
  }

  .think-toggle:hover {
    color: var(--text);
    background: var(--raised);
  }

  .think-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }

  .think-dot.running {
    background: var(--state-running, var(--warn));
    box-shadow: 0 0 8px var(--accent-glow);
    animation: fs-pulse 1.4s ease-in-out infinite;
  }

  .think-chevron {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: transform 150ms ease;
  }

  .think-chevron.open {
    transform: rotate(90deg);
  }

  .think-body {
    padding: 8px 12px 10px;
    border-top: 1px solid var(--line);
    color: var(--dim);
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    max-height: 260px;
    overflow-y: auto;
  }

  .msg-images {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .msg-img {
    max-width: 240px;
    max-height: 180px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--line);
    object-fit: cover;
  }

  .msg-body {
    color: var(--text);
    font-size: 14px;
    line-height: 1.62;
    word-break: break-word;
  }

  .msg-body :global(p) {
    margin: 0 0 0.7em;
  }

  .msg-body :global(p:last-child) {
    margin-bottom: 0;
  }

  .msg-body :global(h1),
  .msg-body :global(h2),
  .msg-body :global(h3) {
    margin: 1em 0 0.45em;
    font-weight: 600;
    line-height: 1.3;
    color: var(--text);
  }

  .msg-body :global(h1) {
    font-size: 1.25em;
  }

  .msg-body :global(h2) {
    font-size: 1.12em;
  }

  .msg-body :global(h3) {
    font-size: 1.02em;
  }

  .msg-body :global(ul),
  .msg-body :global(ol) {
    margin: 0.4em 0 0.8em;
    padding-left: 1.4em;
  }

  .msg-body :global(li) {
    margin: 0.2em 0;
  }

  .msg-body :global(a) {
    color: var(--accent-text);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .msg-body :global(a:hover) {
    opacity: 0.85;
  }

  .msg-body :global(> *:first-child) {
    margin-top: 0;
  }

  .msg-body :global(> *:last-child) {
    margin-bottom: 0;
  }

  .msg-body :global(p:empty) {
    display: none;
  }

  .msg-body :global(blockquote) {
    margin: 0.6em 0;
    padding: 4px 12px;
    border-left: 3px solid var(--accent-line);
    color: var(--dim);
    background: var(--accent-soft);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  .msg-body :global(code:not(pre code)) {
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--panel2);
    border: 1px solid var(--line);
    font-size: 0.88em;
    color: var(--accent-text);
  }

  .msg-body :global(.code-wrap) {
    margin: 0.75em 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--code-bg);
    overflow: hidden;
  }

  .msg-body :global(.code-head) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 5px 14px;
    background: var(--code-head);
    border-bottom: 1px solid var(--line);
    font-size: 11.5px;
  }

  .msg-body :global(.code-lang) {
    color: var(--dim);
    text-transform: lowercase;
    font-family: var(--font-mono);
  }

  .msg-body :global(.code-copy) {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--line2);
    background: transparent;
    color: var(--dim);
    border-radius: 5px;
    padding: 3px 8px;
    font-size: 11px;
    line-height: 1.2;
    cursor: pointer;
    transition: all 140ms ease;
  }

  .msg-body :global(.code-copy:hover) {
    color: var(--text);
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .msg-body :global(.code-copy.ok) {
    color: var(--ok);
    border-color: var(--ok);
  }

  .msg-body :global(pre) {
    margin: 0;
    padding: 10px 14px;
    overflow-x: auto;
    font-size: 12.5px;
    line-height: 1.5;
  }

  .msg-body :global(table) {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    margin: 0.7em 0;
    font-size: 13px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .msg-body :global(th),
  .msg-body :global(td) {
    border-right: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
    padding: 6px 10px;
    text-align: left;
  }

  .msg-body :global(th:last-child),
  .msg-body :global(td:last-child) {
    border-right: none;
  }

  .msg-body :global(tr:last-child td) {
    border-bottom: none;
  }

  .msg-body :global(th) {
    background: var(--panel2);
    font-weight: 600;
  }

  .msg-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    opacity: 0.86;
    transition: opacity 150ms ease;
  }

  .msg-wrap:hover .msg-actions,
  .msg-wrap:focus-within .msg-actions,
  .msg-actions.visible {
    opacity: 1;
  }

  .act-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--line2, var(--line));
    background: var(--panel2);
    color: color-mix(in srgb, var(--text) 82%, var(--dim));
    border-radius: 6px;
    padding: 3px 9px;
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 140ms ease, color 140ms ease, border-color 140ms ease;
  }

  .act-btn:hover {
    color: var(--text);
    border-color: var(--accent-line);
    background: var(--raised);
  }

  .act-btn.ok {
    color: var(--ok);
    border-color: var(--ok);
  }

  .act-btn.pinned {
    color: var(--accent-text);
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
</style>
