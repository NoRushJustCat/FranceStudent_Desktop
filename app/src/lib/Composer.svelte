<script>
  import ContextRing from './ContextRing.svelte';
  import PermMode from './PermMode.svelte';
  let {
    input = $bindable(''),
    enableWebSearch = $bindable(false),
    busy = false,
    loading = false,
    mode = 'chat',
    workdir = null,
    agents = [],
    curAgent = null,
    ctx = null,
    eco = false,
    skills = [],
    commands = [],
    attachments = [],
    onerror = () => {},
    onattachfiles = () => {},
    onremoveattachment = () => {},
    onagentchange = () => {},
    onmodechange = () => {},
    onchoosefolder = () => {},
    onsend = () => {},
    onstop = () => {}
  } = $props();

  let ta = $state(null);
  let fileInput = $state(null);
  let dragging = $state(false);
  let dragDepth = $state(0);
  let moreOpen = $state(false);

  let acOpen = $state(false);
  let acMenuEl = $state(null);
  let acTrigger = $state(''); // '/' | '$'
  let acQuery = $state('');
  let acIndex = $state(0);

  export function focus() {
    ta?.focus();
  }

  let baseName = $derived.by(() => {
    if (!workdir) return '';
    const parts = String(workdir).replace(/[/\\]+$/, '').split(/[/\\]/);
    return parts[parts.length - 1] || workdir;
  });

  let hasUploading = $derived(
    attachments.some((a) => a.status === 'uploading' || a.status === 'pending')
  );
  let hasReadyAttachment = $derived(
    attachments.some((a) => a.status === 'ready')
  );
  let canSend = $derived.by(() => {
    const t = input.trim();
    const isBareTrigger = t === '/' || t === '$';
    return !busy && !loading && !hasUploading && ((t.length > 0 && !isBareTrigger) || hasReadyAttachment);
  });

  let acItems = $derived.by(() => {
    if (!acOpen) return [];
    const q = acQuery.toLowerCase();
    if (acTrigger === '/') {
      const cmdList = commands
        .filter((c) => !c.only || c.only === mode)
        .map((c) => ({
          type: 'cmd',
          name: c.name,
          aliases: c.alias || [],
          label: `/${c.name}`,
          desc: c.desc || '',
          insert: `/${c.name} `,
          disabled: c.name === 'stop' && !busy,
          run: c.run || null
        }));
      const skillList = skills.map((s) => ({
        type: 'skill',
        name: s.name,
        aliases: [],
        label: `$${s.name}`,
        desc: String(s.description || s.source || 'Skill').replace(/\.$/, ''),
        insert: `$${s.name} `,
        disabled: false,
        run: null
      }));
      return [...cmdList, ...skillList]
        .filter(
          (item) =>
            item.name.toLowerCase().includes(q) ||
            item.desc.toLowerCase().includes(q) ||
            item.aliases.some((a) => String(a).toLowerCase().includes(q))
        )
        .slice(0, 12);
    }
    if (acTrigger === '$') {
      return skills
        .map((s) => ({
          type: 'skill',
          name: s.name,
          label: `$${s.name}`,
          desc: String(s.description || s.source || 'Skill').replace(/\.$/, ''),
          insert: `$${s.name} `,
          run: null
        }))
        .filter((item) => item.name.toLowerCase().includes(q) || item.desc.toLowerCase().includes(q))
        .slice(0, 12);
    }
    return [];
  });

  $effect(() => {
    if (acIndex >= acItems.length) {
      acIndex = Math.max(0, acItems.length - 1);
    }
  });

  $effect(() => {
    input;
    adjustHeight();
  });

  function adjustHeight() {
    if (!ta) return;
    ta.style.height = 'auto';
    const next = Math.min(ta.scrollHeight, 220);
    ta.style.height = `${Math.max(next, 24)}px`;
  }

  function updateAutocomplete() {
    if (!ta) {
      acOpen = false;
      return;
    }
    const pos = ta.selectionStart ?? input.length;
    const before = input.slice(0, pos);

    // '/' en debut de ligne
    const slashMatch = before.match(/(?:^|\n)\/([^\s]*)$/);
    if (slashMatch) {
      acTrigger = '/';
      acQuery = slashMatch[1] || '';
      acOpen = true;
      acIndex = 0;
      return;
    }

    // '$' n'importe ou apres un espace ou debut de ligne
    const dollarMatch = before.match(/(?:^|\s)\$([^\s]*)$/);
    if (dollarMatch) {
      acTrigger = '$';
      acQuery = dollarMatch[1] || '';
      acOpen = true;
      acIndex = 0;
      return;
    }

    acOpen = false;
  }

  function applyAcItem(item) {
    if (!item || !ta || item.disabled) return;
    acOpen = false;
    if (item.type === 'cmd' && typeof item.run === 'function') {
      input = '';
      Promise.resolve(item.run()).catch((err) => onerror(String(err)));
      queueMicrotask(() => ta?.focus());
      return;
    }
    const pos = ta.selectionStart ?? input.length;
    const before = input.slice(0, pos);
    const after = input.slice(pos);

    const re = acTrigger === '/' ? /(?:^|\n)\/[^\s]*$/ : /(?:^|\s)\$[^\s]*$/;
    const m = before.match(re);
    if (!m) {
      return;
    }

    const prefixKeep = m[0].startsWith('\n') || m[0].startsWith(' ') ? m[0][0] : '';
    const startIdx = pos - m[0].length + prefixKeep.length;
    const nextVal = input.slice(0, startIdx) + item.insert + after;
    input = nextVal;

    const newCursor = startIdx + item.insert.length;
    queueMicrotask(() => {
      if (ta) {
        ta.focus();
        ta.setSelectionRange(newCursor, newCursor);
        adjustHeight();
      }
    });
  }

  function handleKeydown(e) {
    if (acOpen && acItems.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        acIndex = (acIndex + 1) % acItems.length;
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        acIndex = (acIndex - 1 + acItems.length) % acItems.length;
        return;
      }
      if (e.key === 'Tab' || e.key === 'Enter') {
        e.preventDefault();
        applyAcItem(acItems[acIndex]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        acOpen = false;
        return;
      }
    }

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (canSend) {
        onsend();
      }
    }
  }

  function handleInput() {
    adjustHeight();
    updateAutocomplete();
  }

  function handlePaste(e) {
    const items = e.clipboardData?.items;
    if (!items) return;
    const imageFiles = [];
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (item.type && item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) imageFiles.push(file);
      }
    }
    if (imageFiles.length > 0) {
      onattachfiles(imageFiles);
    }
  }

  function handleDragOver(e) {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'copy';
    }
    if (!dragging) dragging = true;
  }

  function handleDragEnter(e) {
    e.preventDefault();
    dragDepth += 1;
    dragging = true;
  }

  function handleDragLeave(e) {
    e.preventDefault();
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) {
      dragging = false;
    }
  }

  function handleDrop(e) {
    e.preventDefault();
    dragDepth = 0;
    dragging = false;
    const files = Array.from(e.dataTransfer?.files || []);
    if (files.length > 0) {
      onattachfiles(files);
    }
  }

  function handleFileChange(e) {
    const files = Array.from(e.currentTarget.files || []);
    if (files.length > 0) {
      onattachfiles(files);
    }
    e.currentTarget.value = '';
  }

  function formatSize(bytes) {
    if (!bytes && bytes !== 0) return '';
    if (bytes < 1024) return `${bytes} o`;
    if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} Ko`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} Mo`;
  }
</script>

<svelte:window onclick={(e) => { if (moreOpen && !e.target.closest?.('.more-wrap')) moreOpen = false; }} onkeydown={(e) => { if (e.key === 'Escape') moreOpen = false; }} />

<div
  class="composer"
  class:dragging
  ondragenter={handleDragEnter}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
  role="region"
  aria-label="Zone de saisie"
>
  {#if dragging}
    <div class="drop-overlay">
      <div class="drop-box">
        <span class="drop-icon" aria-hidden="true">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48" />
          </svg>
        </span>
        <span>Déposer des images ou fichiers ici…</span>
      </div>
    </div>
  {/if}

  {#if acOpen && acItems.length > 0}
    <div class="ac-menu" role="listbox" bind:this={acMenuEl}>
      <div class="ac-list">
        {#each acItems as item, idx (item.type + ':' + item.name)}
          <button
            type="button"
            class="ac-item"
            class:active={idx === acIndex && !item.disabled}
            class:disabled={!!item.disabled}
            disabled={!!item.disabled}
            role="option"
            aria-selected={idx === acIndex && !item.disabled}
            onmousedown={(e) => {
              e.preventDefault();
              if (!item.disabled) applyAcItem(item);
            }}
          >
            <span class="ac-label">{item.label}</span>
            {#if item.desc}
              <span class="ac-desc">{item.desc}</span>
            {/if}
          </button>
        {/each}
      </div>
      <div class="ac-foot" aria-hidden="true">
        <span><kbd>↑↓</kbd> naviguer</span>
        <span>·</span>
        <span><kbd>Entrée</kbd> ou <kbd>Tab</kbd> valider</span>
        <span>·</span>
        <span><kbd>Échap</kbd> fermer</span>
      </div>
    </div>
  {/if}

  {#if attachments.length > 0}
    <div class="attachments-bar">
      {#each attachments as item (item.localId)}
        <div
          class="att-chip"
          class:error={item.status === 'error'}
          class:uploading={item.status === 'uploading' || item.status === 'pending'}
        >
          {#if item.previewUrl}
            <img class="att-thumb" src={item.previewUrl} alt={item.name} />
          {:else}
            <span class="att-icon" aria-hidden="true">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
              </svg>
            </span>
          {/if}
          <div class="att-meta">
            <span class="att-name" title={item.name}>{item.name}</span>
            <span class="att-status">
              {#if item.status === 'uploading' || item.status === 'pending'}
                Envoi…
              {:else if item.status === 'error'}
                <span class="err-txt">{item.error || 'Erreur'}</span>
              {:else}
                Prêt {#if item.size}· {formatSize(item.size)}{/if}
              {/if}
            </span>
          </div>
          <button
            type="button"
            class="att-remove"
            title="Retirer la pièce jointe"
            aria-label="Retirer {item.name}"
            onclick={() => onremoveattachment(item.localId)}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <div class="input-wrap">
    <textarea
      bind:this={ta}
      bind:value={input}
      rows="1"
      placeholder={mode === 'code'
        ? 'Demander une modification de code, /commande ou $skill…'
        : 'Écrire un message, /commande ou $skill…'}
      disabled={loading}
      oninput={handleInput}
      onkeydown={handleKeydown}
      onpaste={handlePaste}
      onblur={(e) => {
        if (!acMenuEl?.contains(e.relatedTarget)) {
          acOpen = false;
        }
      }}
    ></textarea>
  </div>

  <div class="bar">
    <div class="bar-left">
      {#if agents.length > 0}
        <div class="select-wrap">
          <select
            class="agent-select"
            value={curAgent ?? agents[0]?.id ?? ''}
            disabled={busy}
            onchange={(e) => onagentchange(Number(e.currentTarget.value))}
            title="Choisir un agent"
          >
            {#each agents as a (a.id)}
              <option value={a.id}>
                {a.name}{a.modelProvider ? ` (${a.modelProvider})` : ''}
              </option>
            {/each}
          </select>
          <svg class="select-chev" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </div>
      {/if}

      <div class="seg" role="group" aria-label="Mode">
        <button
          type="button"
          class="seg-btn"
          class:active={mode === 'chat'}
          disabled={busy}
          onclick={() => onmodechange('chat')}
        >
          Chat
        </button>
        <button
          type="button"
          class="seg-btn"
          class:active={mode === 'code'}
          disabled={busy}
          onclick={() => onmodechange('code')}
        >
          Code
        </button>
      </div>

      <div class="folder-slot" class:show={mode === 'code'} inert={mode !== 'code'}>
        <div class="folder-clip">
          <button
            type="button"
            class="tool-btn folder-btn"
            disabled={busy}
            onclick={onchoosefolder}
            title={workdir || 'Choisir un dossier de travail'}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            </svg>
            <span>{workdir ? baseName : 'Choisir un dossier…'}</span>
          </button>
        </div>
      </div>

      <PermMode up />
    </div>

    <div class="bar-right">
      <input
        bind:this={fileInput}
        type="file"
        multiple
        accept="image/*,.pdf,.txt,.md,.csv,.json,.js,.ts,.py,.rs,.html,.css,.xml,.yaml,.yml"
        class="hidden-file"
        onchange={handleFileChange}
      />

      <div class="more-wrap">
        <button
          type="button"
          class="tool-btn more-btn"
          class:active={moreOpen || enableWebSearch}
          aria-haspopup="menu"
          aria-expanded={moreOpen}
          onclick={() => (moreOpen = !moreOpen)}
          title="Pièce jointe, recherche Web…"
          aria-label="Plus d'options"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
          {#if enableWebSearch}<span class="more-dot" aria-hidden="true"></span>{/if}
        </button>

        {#if moreOpen}
          <div class="more-menu" role="menu">
            <button
              type="button"
              role="menuitem"
              class="more-item"
              onclick={() => {
                moreOpen = false;
                fileInput?.click();
              }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48" />
              </svg>
              <span>Pièce jointe</span>
            </button>
            <button
              type="button"
              role="menuitemcheckbox"
              aria-checked={enableWebSearch}
              class="more-item"
              class:active={enableWebSearch}
              onclick={() => (enableWebSearch = !enableWebSearch)}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <circle cx="12" cy="12" r="10" />
                <line x1="2" y1="12" x2="22" y2="12" />
                <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
              </svg>
              <span>Recherche Web</span>
              <span class="more-check" aria-hidden="true">{enableWebSearch ? '✓' : ''}</span>
            </button>
          </div>
        {/if}
      </div>

      <ContextRing {ctx} {eco} length={input.length} />
      {#if busy}
        <button
          type="button"
          class="send-btn stop"
          onclick={onstop}
          title="Arrêter la génération"
          aria-label="Arrêter"
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <rect x="4" y="4" width="16" height="16" rx="2" />
          </svg>
        </button>
      {:else}
        <button
          type="button"
          class="send-btn"
          disabled={!canSend}
          onclick={onsend}
          title="Envoyer (Entrée)"
          aria-label="Envoyer"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="12" y1="19" x2="12" y2="5" />
            <polyline points="6 11 12 5 18 11" />
          </svg>
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .composer {
    position: relative;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 11px 14px 9px;
    gap: 8px;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .composer:focus-within {
    border-color: var(--accent-line);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }

  .composer.dragging {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-glow);
  }

  .drop-overlay {
    position: absolute;
    inset: 0;
    z-index: 20;
    background: color-mix(in srgb, var(--panel) 88%, transparent);
    backdrop-filter: blur(4px);
    border-radius: var(--radius);
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  .drop-box {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 20px;
    border: 1.5px dashed var(--accent);
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 500;
    font-size: 0.92rem;
  }

  .ac-menu {
    position: absolute;
    left: -1px;
    right: -1px;
    bottom: calc(100% + 6px);
    z-index: 30;
    background: var(--raised);
    border: 1px solid var(--accent-line, var(--line2));
    border-radius: var(--radius-sm);
    padding: 6px 6px 4px;
    box-shadow: 0 -8px 26px rgba(0, 0, 0, 0.24), 0 14px 36px rgba(0, 0, 0, 0.42);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .ac-list {
    max-height: 220px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    scrollbar-gutter: stable;
    scrollbar-width: thin;
    scrollbar-color: color-mix(in srgb, var(--dim) 45%, transparent) transparent;
  }

  .ac-list::-webkit-scrollbar {
    width: 6px;
  }

  .ac-list::-webkit-scrollbar-thumb {
    background: color-mix(in srgb, var(--dim) 45%, transparent);
    border-radius: 3px;
  }

  .ac-item.disabled {
    opacity: 0.42;
    cursor: not-allowed;
  }

  .ac-foot {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 8px 3px;
    border-top: 1px solid var(--line);
    font-size: 0.72rem;
    color: var(--dim);
  }

  .ac-foot kbd {
    font-family: var(--mono);
    font-size: 0.68rem;
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--panel);
    border: 1px solid var(--line);
    color: var(--text);
  }

  .ac-item {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 16px;
    width: 100%;
    min-height: 36px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 6px;
    padding: 7px 10px;
    color: var(--text);
    cursor: pointer;
    font-size: 0.86rem;
    flex-shrink: 0;
  }

  .ac-item:hover,
  .ac-item.active {
    background: var(--accent-soft);
    color: var(--accent-text);
  }

  .ac-label {
    font-family: var(--font-mono);
    font-weight: 600;
    min-width: 145px;
    white-space: nowrap;
  }

  .ac-desc {
    color: var(--dim);
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attachments-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--line);
  }

  .att-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 4px 6px 4px 5px;
    max-width: 240px;
  }

  .att-chip.error {
    border-color: var(--danger);
    background: var(--danger-soft);
  }

  .att-thumb {
    width: 30px;
    height: 30px;
    object-fit: cover;
    border-radius: 4px;
    flex-shrink: 0;
  }

  .att-icon {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    background: var(--raised);
    border-radius: 4px;
    font-size: 0.95rem;
    flex-shrink: 0;
  }

  .att-meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.2;
  }

  .att-name {
    font-size: 0.78rem;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .att-status {
    font-size: 0.7rem;
    color: var(--dim);
  }

  .err-txt {
    color: var(--danger);
  }

  .att-remove {
    background: transparent;
    border: none;
    color: var(--dim);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border-radius: 4px;
  }

  .att-remove:hover {
    color: var(--text);
    background: var(--raised);
  }

  .input-wrap {
    display: flex;
  }

  textarea {
    width: 100%;
    resize: none;
    border: none;
    outline: none;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 0.95rem;
    line-height: 1.45;
    max-height: 220px;
    overflow-y: auto;
    padding: 2px 0;
    box-shadow: none;
  }

  textarea:hover:not(:focus):not(:disabled),
  textarea:focus {
    border: none;
    box-shadow: none;
    background: transparent;
    outline: none;
  }

  textarea::placeholder {
    color: var(--faint);
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: nowrap;
    gap: 8px;
    padding-top: 4px;
  }

  .bar-left,
  .bar-right {
    display: flex;
    align-items: center;
    flex-wrap: nowrap;
    gap: 6px;
  }

  .bar-left {
    flex: 1;
    min-width: 0;
  }

  .bar-right {
    /* "+" passe à gauche (order), envoi reste à droite */
    display: contents;
  }

  .select-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .agent-select {
    appearance: none;
    -webkit-appearance: none;
    background: var(--panel2);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    height: 30px;
    padding: 0 28px 0 10px;
    font-size: 0.8rem;
    outline: none;
    cursor: pointer;
    max-width: 170px;
  }

  .select-chev {
    position: absolute;
    right: 9px;
    pointer-events: none;
    color: var(--dim);
  }

  .agent-select:hover {
    border-color: var(--line2);
  }

  .seg {
    display: inline-flex;
    align-items: center;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    height: 30px;
    padding: 2px;
  }

  .seg-btn {
    background: transparent;
    border: none;
    color: var(--dim);
    font-size: 0.78rem;
    height: 100%;
    padding: 0 9px;
    border-radius: 5px;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }

  .seg-btn.active {
    background: var(--raised);
    color: var(--text);
    font-weight: 500;
  }

  .tool-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--panel2);
    color: var(--dim);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    height: 30px;
    padding: 0 10px;
    font-size: 0.78rem;
    cursor: pointer;
    transition: border-color 0.15s ease, color 0.15s ease;
  }

  .tool-btn:hover {
    color: var(--text);
    border-color: var(--line2);
  }

  .folder-btn {
    max-width: 140px;
  }

  .folder-btn span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hidden-file {
    display: none;
  }

  .more-wrap {
    position: relative;
    display: inline-flex;
    order: 9;
  }

  .more-btn {
    position: relative;
    width: 30px;
    padding: 0;
    justify-content: center;
    border-radius: 999px;
  }

  .more-btn.active {
    color: var(--accent-text);
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .more-dot {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 2px var(--panel);
  }

  .more-menu {
    position: absolute;
    left: 0;
    bottom: calc(100% + 8px);
    z-index: 30;
    min-width: 190px;
    padding: 5px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--raised);
    border: 1px solid var(--accent-line, var(--line2));
    border-radius: 12px;
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.42);
  }

  .more-item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font-size: 0.82rem;
    text-align: left;
    cursor: pointer;
  }

  .more-item:hover:not(:disabled) {
    background: var(--hover);
    transform: none;
    box-shadow: none;
  }

  .more-item.active {
    color: var(--accent-text);
  }

  .more-item span:not(.more-check) {
    flex: 1;
  }

  .more-check {
    width: 14px;
    text-align: right;
    font-weight: 700;
  }

  .send-btn {
    width: 32px;
    height: 32px;
    padding: 0;
    flex-shrink: 0;
    border-radius: 50%;
    border: 1px solid transparent;
    background: var(--accent);
    color: var(--on-accent, #ffffff);
    font-size: 0.95rem;
    font-weight: 600;
    display: grid;
    place-items: center;
    cursor: pointer;
    box-shadow: 0 2px 8px color-mix(in srgb, var(--accent) 32%, transparent);
    transition: opacity 0.15s ease, transform 0.1s ease, background 0.15s ease, color 0.15s ease;
  }

  .send-btn:hover:not(:disabled) {
    transform: translateY(-1px);
    filter: brightness(1.08);
  }

  .send-btn:disabled {
    background: transparent;
    color: var(--dim);
    border-color: var(--line);
    opacity: 0.4;
    cursor: not-allowed;
    transform: none;
    box-shadow: none;
  }

  .send-btn.stop {
    background: var(--danger);
    color: #ffffff;
    font-size: 0.75rem;
  }

  /* fs-bar-wrap : la barre du bas s'adapte a la largeur (plus de chevauchement) */
  .composer {
    min-width: 0;
  }
  .bar {
    display: flex;
    flex-wrap: nowrap;
    align-items: flex-end;
    gap: 8px;
    min-width: 0;
  }
  .bar-left {
    display: flex;
    flex: 1 1 0;
    min-width: 0;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
  }
  .bar-right {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    margin-left: auto;
  }
  .select-wrap {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
  }
  .agent-select {
    min-width: 0;
    max-width: 100%;
    text-overflow: ellipsis;
  }
  .seg {
    flex: 0 0 auto;
  }
  .folder-btn {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 100%;
  }
  .folder-btn span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Bouton dossier : toujours monté, replié en douceur hors mode Code (largeur, espacement et opacité animés) */
  .folder-slot {
    display: grid;
    grid-template-columns: 0fr;
    flex: 0 1 auto;
    min-width: 0;
    margin-right: -8px;
    opacity: 0;
    pointer-events: none;
    transition:
      grid-template-columns 0.26s cubic-bezier(0.22, 1, 0.36, 1),
      margin-right 0.26s cubic-bezier(0.22, 1, 0.36, 1),
      opacity 0.2s ease;
  }
  .folder-slot.show {
    grid-template-columns: 1fr;
    margin-right: 0;
    opacity: 1;
    pointer-events: auto;
  }
  .folder-clip {
    display: flex;
    min-width: 0;
    overflow: hidden;
  }
  @media (prefers-reduced-motion: reduce) {
    .folder-slot {
      transition: none;
    }
  }
</style>
