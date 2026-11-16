<script>
  import { tick } from 'svelte';
  import { fly } from 'svelte/transition';
  import ContextRing from './ContextRing.svelte';

  let {
    input = $bindable(''),
    mode = 'chat',
    onModeChange = (m) => {},
    centered = true,
    disabled = false,
    running = false,
    agents = [],
    agent = '',
    onAgentChange = (id) => {},
    smartRoute = 'eco',
    onSmartRouteChange = (r) => {},
    conciseMode = true,
    onConciseChange = (v) => {},
    workdir = '',
    attachments = [],
    skills = [],
    ctx = null,
    eco = false,
    enableWebSearch = false,
    onWebSearchChange = () => {},
    extraCommands = [],
    onSend = (text) => {},
    onCancel = () => {},
    onAttachFiles = (files) => {},
    onRemoveAttachment = (id) => {},
    onInjectRange = () => {},
    onPickFolder = () => {},
  } = $props();

  const SLASH_COMMANDS = [
    { name: '/sys', description: 'Infos système locales (0 token)' },
    { name: '/git', description: 'État du dépôt git (0 token)' },
    { name: '/files', description: 'Lister les fichiers du dossier (0 token)' },
    { name: '/clean', description: 'Nettoyer le contexte de la session' },
    { name: '/calc', description: 'Calculatrice locale rapide (0 token)' },
    { name: '/compact', description: 'Compacter l’historique de conversation' },
    { name: '/help', description: 'Afficher les commandes disponibles' },
  ];

  const ROUTE_LABELS = {
    eco: 'Éco',
    balanced: 'Équilibré',
    max: 'Max',
  };

  let textareaEl = $state(null);
  let fileInputEl = $state(null);

  let plusOpen = $state(false);
  let modelMenuOpen = $state(false);
  let activeSuggestIndex = $state(0);

  let plusWrapEl = $state(null);
  let modelWrapEl = $state(null);

  const currentAgent = $derived(
    agents.find((a) => a.id === agent) || agents[0] || { id: '', name: 'Auto', label: 'Auto' }
  );

  const groupedAgents = $derived.by(() => {
    const groups = [
      { key: 'anthropic', label: 'Anthropic', items: [] },
      { key: 'openai', label: 'OpenAI · GPT', items: [] },
      { key: 'google', label: 'Google · Gemini', items: [] },
      { key: 'other', label: 'Autres', items: [] },
    ];
    for (const a of agents || []) {
      const txt = `${a.label || ''} ${a.name || ''}`.toLowerCase();
      if (/claude|opus|sonnet|haiku|anthropic/.test(txt)) groups[0].items.push(a);
      else if (/gpt|openai|o1|o3|o4|sol|terra/.test(txt)) groups[1].items.push(a);
      else if (/gemini|hydrogen|argon|google/.test(txt)) groups[2].items.push(a);
      else groups[3].items.push(a);
    }
    for (const g of groups) {
      g.items.sort((x, y) => String(x.label || x.name).localeCompare(String(y.label || y.name)));
    }
    return groups.filter((g) => g.items.length > 0);
  });

  const canSend = $derived(
    !disabled && !running && (input.trim().length > 0 || attachments.length > 0)
  );

  // Autocomplétion `/` ou `$`
  const completionState = $derived.by(() => {
    const val = input;
    if (!val) return null;
    // Détecte un token en début de texte ou après un espace
    const match = val.match(/(?:^|\s)([/$][^\s]*)$/);
    if (!match) return null;
    const token = match[1];
    const trigger = token[0];
    const query = token.slice(1).toLowerCase();

    if (trigger === '/') {
      const seen = new Set(SLASH_COMMANDS.map((c) => c.name));
      const allCmds = [
        ...SLASH_COMMANDS,
        ...(extraCommands || [])
          .filter((c) => c?.name && !seen.has('/' + c.name) && !seen.has(c.name))
          .map((c) => ({
            name: c.name.startsWith('/') ? c.name : '/' + c.name,
            description: c.desc || c.description || '',
          })),
      ];
      const items = allCmds.filter((c) =>
        c.name.slice(1).toLowerCase().startsWith(query)
      ).map((c) => ({
        key: c.name,
        label: c.name,
        description: c.description,
        insert: c.name + ' ',
      }));
      return items.length ? { trigger: '/', token, items } : null;
    }

    if (trigger === '$') {
      const items = (skills || [])
        .filter((s) => s.name.toLowerCase().includes(query))
        .slice(0, 8)
        .map((s) => ({
          key: s.name,
          label: `$${s.name}`,
          description: s.description || '',
          insert: `$${s.name} `,
        }));
      return items.length ? { trigger: '$', token, items } : null;
    }

    return null;
  });

  $effect(() => {
    if (completionState) {
      activeSuggestIndex = 0;
    }
  });

  $effect(() => {
    function handleOutside(e) {
      if (plusOpen && plusWrapEl && !plusWrapEl.contains(e.target)) {
        plusOpen = false;
      }
      if (modelMenuOpen && modelWrapEl && !modelWrapEl.contains(e.target)) {
        modelMenuOpen = false;
      }
    }
    window.addEventListener('pointerdown', handleOutside);
    return () => window.removeEventListener('pointerdown', handleOutside);
  });

  export function focus() {
    textareaEl?.focus();
  }

  export function insertSnippet(snippet) {
    if (!snippet) return;
    input = input ? `${input.trimEnd()}\n${snippet}` : snippet;
    autoResize();
    textareaEl?.focus();
  }

  async function autoResize() {
    await tick();
    if (!textareaEl) return;
    textareaEl.style.height = 'auto';
    const lineHeight = 22;
    const maxHeight = lineHeight * 6 + 4;
    textareaEl.style.height = `${Math.min(textareaEl.scrollHeight, maxHeight)}px`;
  }

  function applySuggestion(item) {
    if (!completionState) return;
    const idx = input.lastIndexOf(completionState.token);
    if (idx === -1) return;
    input = input.slice(0, idx) + item.insert;
    autoResize();
    textareaEl?.focus();
  }

  function submit() {
    if (!canSend) return;
    const payload = input.trim();
    input = '';
    plusOpen = false;
    modelMenuOpen = false;
    autoResize();
    onSend(payload);
  }

  function handleKeydown(e) {
    if (completionState && completionState.items.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        activeSuggestIndex = (activeSuggestIndex + 1) % completionState.items.length;
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        activeSuggestIndex =
          (activeSuggestIndex - 1 + completionState.items.length) %
          completionState.items.length;
        return;
      }
      if (e.key === 'Tab' || (e.key === 'Enter' && !e.shiftKey)) {
        e.preventDefault();
        applySuggestion(completionState.items[activeSuggestIndex]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        input = input.slice(0, input.lastIndexOf(completionState.token));
        autoResize();
        return;
      }
    }

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (running) return;
      submit();
    }
  }

  function handlePaste(e) {
    const items = e.clipboardData?.items;
    if (!items) return;
    const files = [];
    for (const item of items) {
      if (item.kind === 'file') {
        const f = item.getAsFile();
        if (f) files.push(f);
      }
    }
    if (files.length > 0) {
      e.preventDefault();
      onAttachFiles(files);
    }
  }

  function handleFilePick(e) {
    const files = Array.from(e.target.files || []);
    if (files.length > 0) {
      onAttachFiles(files);
    }
    e.target.value = '';
    plusOpen = false;
  }
</script>

<div class="composer-shell" class:centered>
  {#if !centered}
    <div class="compact-mode-bar">
      <div class="mode-switch compact" role="tablist" aria-label="Mode">
        <button
          type="button"
          role="tab"
          aria-selected={mode === 'chat'}
          class="mode-btn"
          class:active={mode === 'chat'}
          onclick={() => onModeChange('chat')}
        >
          Discussion
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={mode === 'code'}
          class="mode-btn"
          class:active={mode === 'code'}
          onclick={() => onModeChange('code')}
        >
          Tâche PC
        </button>
      </div>
      {#if mode === 'code' && workdir}
        <button
          type="button"
          class="workdir-pill"
          title={workdir}
          onclick={() => onPickFolder()}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path
              d="M2 4.25C2 3.56 2.56 3 3.25 3H6.1c.33 0 .65.13.88.37l.78.78c.14.14.33.22.53.22H12.75C13.44 4.37 14 4.93 14 5.62v6.13c0 .69-.56 1.25-1.25 1.25H3.25C2.56 13 2 12.44 2 11.75V4.25Z"
              stroke="currentColor"
              stroke-width="1.3"
            />
          </svg>
          <span>{workdir.split(/[\\/]/).filter(Boolean).pop() || workdir}</span>
        </button>
      {/if}
    </div>
  {/if}

  <div class="pill-anchor">
    {#if completionState}
      <div class="suggest-popover" role="listbox">
        {#each completionState.items as item, idx (item.key)}
          <button
            type="button"
            role="option"
            aria-selected={idx === activeSuggestIndex}
            class="suggest-item"
            class:active={idx === activeSuggestIndex}
            onmouseenter={() => (activeSuggestIndex = idx)}
            onclick={() => applySuggestion(item)}
          >
            <span class="suggest-label">{item.label}</span>
            {#if item.description}
              <span class="suggest-desc">{item.description}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    <div class="pill" class:disabled>
      {#if attachments.length > 0}
        <div class="attachments-row">
          {#each attachments as att (att.localId ?? att.id)}
            <div class="att-chip">
              {#if att.preview || att.previewUrl}
                <img src={att.preview || att.previewUrl} alt={att.name} class="att-thumb" />
              {:else}
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M4.5 2.5h4.75L12.5 5.75v7.75a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1Z"
                    stroke="currentColor"
                    stroke-width="1.3"
                  />
                  <path d="M9 2.5v3.5h3.5" stroke="currentColor" stroke-width="1.3" />
                </svg>
              {/if}
              <span class="att-name">{att.name}</span>
              <button
                type="button"
                class="att-remove"
                aria-label="Retirer {att.name}"
                onclick={() => onRemoveAttachment(att.localId ?? att.id)}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M4.5 4.5l7 7M11.5 4.5l-7 7"
                    stroke="currentColor"
                    stroke-width="1.4"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
            </div>
          {/each}
        </div>
      {/if}

      <div class="pill-row">
        <!-- Menu + -->
        <div class="plus-wrap" bind:this={plusWrapEl}>
          <button
            type="button"
            class="icon-btn"
            aria-label="Options et pièces jointes"
            aria-expanded={plusOpen}
            {disabled}
            onclick={() => {
              plusOpen = !plusOpen;
              modelMenuOpen = false;
            }}
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path
                d="M8 3.25v9.5M3.25 8h9.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
            </svg>
            {#if enableWebSearch}
              <span class="plus-dot" title="Recherche Web active"></span>
            {/if}
          </button>

          {#if plusOpen}
            <div class="popover plus-popover" role="menu">
              <button
                type="button"
                role="menuitem"
                class="menu-item"
                onclick={() => fileInputEl?.click()}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M11.5 5.25l-4.6 4.6a1.75 1.75 0 0 1-2.47-2.48l4.95-4.95a2.75 2.75 0 0 1 3.89 3.89l-5.3 5.3a3.75 3.75 0 0 1-5.3-5.3l4.6-4.6"
                    stroke="currentColor"
                    stroke-width="1.3"
                    stroke-linecap="round"
                  />
                </svg>
                <span>Joindre fichier / image</span>
              </button>

              <button
                type="button"
                role="menuitem"
                class="menu-item"
                onclick={() => {
                  plusOpen = false;
                  onInjectRange();
                }}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M5.5 4.5L2.5 8l3 3.5M10.5 4.5l3 3.5-3 3.5M8.75 3.5l-1.5 9"
                    stroke="currentColor"
                    stroke-width="1.3"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  />
                </svg>
                <span>Injecter plage de lignes (L1-L2)</span>
              </button>

              <button
                type="button"
                role="menuitemcheckbox"
                aria-checked={conciseMode}
                class="menu-item"
                onclick={() => onConciseChange(!conciseMode)}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M3 4.5h10M3 8h7M3 11.5h5"
                    stroke="currentColor"
                    stroke-width="1.3"
                    stroke-linecap="round"
                  />
                </svg>
                <span>Mode concis (économie tokens)</span>
                <span class="check-dot" class:on={conciseMode}></span>
              </button>

              <button
                type="button"
                role="menuitemcheckbox"
                aria-checked={enableWebSearch}
                class="menu-item"
                onclick={() => onWebSearchChange(!enableWebSearch)}
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <circle cx="8" cy="8" r="5.5" stroke="currentColor" stroke-width="1.3" />
                  <path d="M2.5 8h11M8 2.5a8.5 8.5 0 0 1 0 11M8 2.5a8.5 8.5 0 0 0 0 11" stroke="currentColor" stroke-width="1.2" />
                </svg>
                <span>Recherche Web</span>
                <span class="check-dot" class:on={enableWebSearch}></span>
              </button>

              {#if mode === 'code'}
                <div class="menu-sep"></div>
                <button
                  type="button"
                  role="menuitem"
                  class="menu-item"
                  onclick={() => {
                    plusOpen = false;
                    onPickFolder();
                  }}
                >
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                    <path
                      d="M2 4.25C2 3.56 2.56 3 3.25 3H6.1c.33 0 .65.13.88.37l.78.78c.14.14.33.22.53.22H12.75C13.44 4.37 14 4.93 14 5.62v6.13c0 .69-.56 1.25-1.25 1.25H3.25C2.56 13 2 12.44 2 11.75V4.25Z"
                      stroke="currentColor"
                      stroke-width="1.3"
                    />
                  </svg>
                  <span>Dossier PC...</span>
                </button>
              {/if}
            </div>
          {/if}

          <input
            bind:this={fileInputEl}
            type="file"
            multiple
            class="sr-only"
            onchange={handleFilePick}
          />
        </div>

        <!-- Zone de saisie -->
        <textarea
          bind:this={textareaEl}
          bind:value={input}
          rows="1"
          {disabled}
          placeholder={mode === 'code'
            ? 'Décrire une tâche sur ce PC… (/ commandes, $ skills)'
            : 'Poser une question… (/ commandes, $ skills)'}
          oninput={autoResize}
          onkeydown={handleKeydown}
          onpaste={handlePaste}
        ></textarea>

        <!-- Contrôles droite : Modèle · Route + Envoyer/Stop -->
        <div class="right-controls">
          <div class="model-wrap" bind:this={modelWrapEl}>
            <button
              type="button"
              class="model-trigger"
              aria-expanded={modelMenuOpen}
              {disabled}
              onclick={() => {
                modelMenuOpen = !modelMenuOpen;
                plusOpen = false;
              }}
            >
              <span class="model-name">{currentAgent.label || currentAgent.name}</span>
              <span class="model-sep">·</span>
              <span class="route-name">{ROUTE_LABELS[smartRoute] || 'Éco'}</span>
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path
                  d="M4.75 6.5L8 9.75l3.25-3.25"
                  stroke="currentColor"
                  stroke-width="1.3"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>

            {#if modelMenuOpen}
              <div class="popover model-popover" transition:fly={{ y: 6, duration: 150 }} role="menu">
                <div class="menu-section-label">Niveau</div>
                <div class="route-grid">
                  {#each ['eco', 'balanced', 'max'] as r}
                    <button
                      type="button"
                      class="route-chip"
                      class:active={smartRoute === r}
                      onclick={() => onSmartRouteChange(r)}
                    >
                      {ROUTE_LABELS[r]}
                    </button>
                  {/each}
                </div>

                {#if groupedAgents.length > 0}
                  {#each groupedAgents as grp (grp.key)}
                    <div class="menu-sep"></div>
                    <div class="menu-section-label">{grp.label}</div>
                    {#each grp.items as a (a.id)}
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={a.id === currentAgent.id}
                        class="menu-item"
                        class:selected={a.id === currentAgent.id}
                        onclick={() => {
                          onAgentChange(a.id);
                          modelMenuOpen = false;
                        }}
                      >
                        <span>{a.label || a.name}</span>
                        {#if a.id === currentAgent.id}
                          <span class="check-dot on"></span>
                        {/if}
                      </button>
                    {/each}
                  {/each}
                {/if}
              </div>
            {/if}
          </div>

          <ContextRing {ctx} {eco} length={input.length} />

          {#if running}
            <button
              type="button"
              class="send-btn stop"
              aria-label="Arrêter"
              onclick={() => onCancel()}
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <rect x="4.5" y="4.5" width="7" height="7" rx="1.2" fill="currentColor" />
              </svg>
            </button>
          {:else}
            <button
              type="button"
              class="send-btn"
              aria-label="Envoyer"
              disabled={!canSend}
              onclick={submit}
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                <path
                  d="M8 12.25V3.75M4.25 7.25L8 3.5l3.75 3.75"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>

  {#if centered}
    <div class="centered-mode-bar">
      <div class="mode-switch" role="tablist" aria-label="Mode">
        <button
          type="button"
          role="tab"
          aria-selected={mode === 'chat'}
          class="mode-btn"
          class:active={mode === 'chat'}
          onclick={() => onModeChange('chat')}
        >
          Discussion
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={mode === 'code'}
          class="mode-btn"
          class:active={mode === 'code'}
          onclick={() => onModeChange('code')}
        >
          Tâche PC
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .composer-shell {
    width: 100%;
    max-width: 760px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .compact-mode-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 4px;
  }

  .centered-mode-bar {
    display: flex;
    justify-content: center;
    padding-top: 2px;
  }

  .mode-switch {
    display: inline-flex;
    align-items: center;
    padding: 3px;
    border-radius: 999px;
    background: var(--panel);
    border: 1px solid var(--line);
    gap: 2px;
  }

  .mode-switch.compact {
    padding: 2px;
  }

  .mode-btn {
    border: 0;
    background: transparent;
    color: var(--dim);
    font-size: 12.5px;
    font-weight: 500;
    padding: 5px 14px;
    border-radius: 999px;
    cursor: pointer;
    transition: background-color 0.14s ease, color 0.14s ease;
  }

  .mode-switch.compact .mode-btn {
    font-size: 12px;
    padding: 3px 10px;
  }

  .mode-btn:hover {
    color: var(--text);
  }

  .mode-btn.active {
    background: var(--raised);
    color: var(--text);
  }

  .workdir-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--dim);
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 999px;
    cursor: pointer;
    max-width: 240px;
  }

  .workdir-pill:hover {
    background: var(--hover);
    color: var(--text);
  }

  .workdir-pill span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pill-anchor {
    position: relative;
  }

  .pill {
    width: 100%;
    box-sizing: border-box;
    background: var(--panel);
    border: 1px solid var(--line2);
    border-radius: 24px;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    transition: border-color 0.15s ease;
  }

  .pill:focus-within {
    border-color: var(--dim);
  }

  .pill.disabled {
    opacity: 0.6;
  }

  .attachments-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 2px 4px 0;
  }

  .att-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 4px 6px 4px 8px;
    font-size: 12px;
    color: var(--text);
    max-width: 200px;
  }

  .att-thumb {
    width: 18px;
    height: 18px;
    object-fit: cover;
    border-radius: 4px;
  }

  .att-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .att-remove {
    border: 0;
    background: transparent;
    color: var(--dim);
    padding: 2px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    cursor: pointer;
  }

  .att-remove:hover {
    background: var(--hover);
    color: var(--text);
  }

  .pill-row {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 10px;
  }

  .plus-wrap,
  .model-wrap {
    position: relative;
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--panel2);
    color: var(--dim);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    flex-shrink: 0;
  }

  .icon-btn:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }

  textarea {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 14px;
    line-height: 22px;
    padding: 5px 2px;
    resize: none;
    outline: none;
    max-height: 136px;
  }

  textarea::placeholder {
    color: var(--faint);
  }

  .right-controls {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .model-trigger {
    height: 32px;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--panel2);
    color: var(--dim);
    font-size: 12px;
    padding: 0 10px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
    white-space: nowrap;
  }

  .model-trigger:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }

  .model-name {
    color: var(--text);
    font-weight: 500;
  }

  .model-sep {
    color: var(--faint);
  }

  .send-btn {
    width: 32px;
    height: 32px;
    border-radius: 999px;
    border: 0;
    background: var(--accent);
    color: #fff;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    flex-shrink: 0;
    transition: opacity 0.14s ease, background-color 0.14s ease;
  }

  .send-btn:disabled {
    background: var(--raised);
    color: var(--faint);
    cursor: default;
  }

  .send-btn.stop {
    background: var(--raised);
    color: var(--text);
    border: 1px solid var(--line2);
  }

  .send-btn.stop:hover {
    background: var(--hover);
  }

  .popover {
    position: absolute;
    bottom: calc(100% + 14px);
    background: var(--panel2);
    border: 1px solid var(--line2);
    border-radius: 14px;
    padding: 6px;
    min-width: 230px;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 2px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.28);
  }

  .plus-popover {
    left: 0;
  }

  .model-popover {
    right: 0;
    width: 288px;
    max-width: calc(100vw - 32px);
    max-height: min(360px, 60vh);
    overflow-y: auto;
    box-sizing: border-box;
  }

  .menu-section-label {
    font-size: 11px;
    color: var(--faint);
    padding: 4px 8px 2px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .route-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 4px;
    padding: 4px 0;
  }

  .route-chip {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--dim);
    font-size: 12px;
    padding: 6px 4px;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 0.14s ease, color 0.14s ease, border-color 0.14s ease;
  }

  .route-chip:hover {
    color: var(--text);
    background: var(--hover);
  }

  .route-chip.active {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--text);
  }

  .menu-item {
    border: 0;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    padding: 7px 9px;
    border-radius: 9px;
    display: flex;
    align-items: center;
    gap: 9px;
    cursor: pointer;
    text-align: left;
    width: 100%;
  }

  .menu-item:hover,
  .menu-item.selected {
    background: var(--hover);
  }

  .menu-item span:first-of-type {
    flex: 1;
  }

  .menu-sep {
    height: 1px;
    background: var(--line);
    margin: 4px 6px;
  }

  .check-dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    border: 1px solid var(--line2);
    flex-shrink: 0;
  }

  .check-dot.on {
    background: var(--ok);
    border-color: var(--ok);
  }

  .suggest-popover {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    right: 0;
    background: var(--panel2);
    border: 1px solid var(--line2);
    border-radius: 14px;
    padding: 6px;
    z-index: 25;
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 220px;
    overflow-y: auto;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.28);
  }

  .suggest-item {
    border: 0;
    background: transparent;
    color: var(--text);
    padding: 7px 10px;
    border-radius: 9px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    cursor: pointer;
    text-align: left;
    font-size: 13px;
  }

  .suggest-item.active,
  .suggest-item:hover {
    background: var(--hover);
  }

  .suggest-label {
    font-weight: 500;
  }

  .suggest-desc {
    color: var(--dim);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .plus-wrap .icon-btn {
    position: relative;
  }

  .plus-dot {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
</style>
