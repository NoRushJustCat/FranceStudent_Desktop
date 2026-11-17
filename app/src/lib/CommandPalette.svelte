<script>
  import { tick } from 'svelte';

  let {
    open = false,
    mode = 'chat',
    chats = [],
    themes = [],
    currentTheme = 'sombre',
    onClose = () => {},
    onRunAction = () => {}
  } = $props();

  let query = $state('');
  let selectedIndex = $state(0);
  let inputEl = $state(null);
  let listEl = $state(null);

  const zeroTokenCmds = [
    { id: 'cmd-sys', label: '/sys', desc: 'Infos système PC', badge: '0 token', action: { type: 'slash_cmd', value: '/sys' }, icon: 'cpu' },
    { id: 'cmd-git', label: '/git', desc: 'Statut Git du dossier', badge: '0 token', action: { type: 'slash_cmd', value: '/git' }, icon: 'git' },
    { id: 'cmd-files', label: '/files', desc: 'Lister fichiers du projet', badge: '0 token', action: { type: 'slash_cmd', value: '/files' }, icon: 'folder' },
    { id: 'cmd-clean', label: '/clean', desc: 'Aperçu caches lourds', badge: '0 token', action: { type: 'slash_cmd', value: '/clean' }, icon: 'trash' }
  ];

  const baseActions = $derived([
    { id: 'act-new', label: 'Nouvelle discussion', desc: 'Démarrer un fil vierge', action: { type: 'new_chat' }, icon: 'plus' },
    {
      id: 'act-mode-chat',
      label: 'Basculer en Discussion',
      desc: mode === 'chat' ? 'Actif' : 'Mode conversation',
      action: { type: 'set_mode', value: 'chat' },
      icon: 'chat'
    },
    {
      id: 'act-mode-agent',
      label: 'Basculer en Tâche PC',
      desc: mode === 'code' ? 'Actif' : 'Mode exécution locale',
      action: { type: 'set_mode', value: 'code' },
      icon: 'terminal'
    },
    { id: 'act-compact', label: 'Compacter la conversation', desc: 'Résumer le contexte actuel', action: { type: 'compact' }, icon: 'compress' },
    { id: 'act-folder', label: 'Choisir un dossier PC', desc: 'Définir le répertoire de travail', action: { type: 'pick_folder' }, icon: 'folder' },
    { id: 'act-settings', label: 'Ouvrir les Réglages', desc: 'Modèles, clés et préférences', action: { type: 'open_settings' }, icon: 'settings' }
  ]);

  const themeItems = $derived(
    (themes || []).map((t) => {
      const id = typeof t === 'string' ? t : t.id;
      const label = typeof t === 'string' ? t.charAt(0).toUpperCase() + t.slice(1) : (t.label || t.name || id);
      return {
        id: `theme-${id}`,
        label: `Thème : ${label}`,
        desc: id === currentTheme ? 'Actif' : 'Appliquer le thème',
        action: { type: 'set_theme', value: id },
        icon: 'palette'
      };
    })
  );

  const recentChats = $derived(
    (chats || []).slice(0, 8).map((c) => ({
      id: `chat-${c.id}`,
      label: c.title || 'Discussion sans titre',
      desc: 'Discussion récente',
      action: { type: 'select_chat', value: c.id },
      icon: 'history'
    }))
  );

  const allItems = $derived([
    ...zeroTokenCmds.map((i) => ({ ...i, group: 'Commandes 0-token' })),
    ...baseActions.map((i) => ({ ...i, group: 'Modes & Actions' })),
    ...themeItems.map((i) => ({ ...i, group: 'Thèmes' })),
    ...recentChats.map((i) => ({ ...i, group: 'Discussions récentes' }))
  ]);

  const filteredItems = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return allItems;
    return allItems.filter(
      (item) =>
        item.label.toLowerCase().includes(q) ||
        (item.desc && item.desc.toLowerCase().includes(q)) ||
        item.group.toLowerCase().includes(q)
    );
  });

  const groupedItems = $derived.by(() => {
    const groups = [];
    const map = new Map();
    for (const item of filteredItems) {
      if (!map.has(item.group)) {
        const g = { name: item.group, items: [] };
        map.set(item.group, g);
        groups.push(g);
      }
      map.get(item.group).items.push(item);
    }
    return groups;
  });

  $effect(() => {
    if (open) {
      query = '';
      selectedIndex = 0;
      tick().then(() => {
        inputEl?.focus();
      });
    }
  });

  $effect(() => {
    if (selectedIndex >= filteredItems.length) {
      selectedIndex = Math.max(0, filteredItems.length - 1);
    }
  });

  function runItem(item) {
    if (!item) return;
    onRunAction(item.action);
    onClose();
  }

  function scrollSelectedIntoView() {
    tick().then(() => {
      const el = listEl?.querySelector('[data-selected="true"]');
      el?.scrollIntoView({ block: 'nearest' });
    });
  }

  function handleKeydown(e) {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filteredItems.length > 0) {
        selectedIndex = (selectedIndex + 1) % filteredItems.length;
        scrollSelectedIntoView();
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filteredItems.length > 0) {
        selectedIndex = (selectedIndex - 1 + filteredItems.length) % filteredItems.length;
        scrollSelectedIntoView();
      }
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filteredItems[selectedIndex]) {
        runItem(filteredItems[selectedIndex]);
      }
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div
    class="cp-backdrop"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}
  >
    <div class="cp-modal" role="dialog" aria-modal="true" aria-label="Palette de commandes">
      <div class="cp-header">
        <svg class="cp-search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="8"></circle>
          <path d="m21 21-4.3-4.3"></path>
        </svg>
        <input
          bind:this={inputEl}
          bind:value={query}
          type="text"
          class="cp-input"
          placeholder="Rechercher une commande, un thème, une discussion..."
          autocomplete="off"
          spellcheck="false"
        />
        <kbd class="cp-esc">ESC</kbd>
      </div>

      <div class="cp-list" bind:this={listEl} role="listbox">
        {#if filteredItems.length === 0}
          <div class="cp-empty">Aucun résultat pour « {query} »</div>
        {:else}
          {#each groupedItems as group}
            <div class="cp-group-label">{group.name}</div>
            {#each group.items as item}
              {@const idx = filteredItems.indexOf(item)}
              {@const isSelected = idx === selectedIndex}
              <button
                type="button"
                class="cp-item"
                class:selected={isSelected}
                data-selected={isSelected}
                role="option"
                aria-selected={isSelected}
                onmouseenter={() => (selectedIndex = idx)}
                onclick={() => runItem(item)}
              >
                <span class="cp-item-icon" aria-hidden="true">
                  {#if item.icon === 'cpu'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M15 2v2M15 20v2M2 15h2M2 9h2M20 15h2M20 9h2M9 2v2M9 20v2"/></svg>
                  {:else if item.icon === 'git'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7"/><line x1="6" y1="9" x2="6" y2="21"/></svg>
                  {:else if item.icon === 'folder'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                  {:else if item.icon === 'trash'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                  {:else if item.icon === 'plus'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                  {:else if item.icon === 'chat'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
                  {:else if item.icon === 'terminal'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
                  {:else if item.icon === 'compress'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/><line x1="14" y1="10" x2="21" y2="3"/><line x1="3" y1="21" x2="10" y2="14"/></svg>
                  {:else if item.icon === 'settings'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
                  {:else if item.icon === 'palette'}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="13.5" cy="6.5" r=".5"/><circle cx="17.5" cy="10.5" r=".5"/><circle cx="8.5" cy="7.5" r=".5"/><circle cx="6.5" cy="12.5" r=".5"/><path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z"/></svg>
                  {:else}
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
                  {/if}
                </span>
                <span class="cp-item-main">
                  <span class="cp-item-label">{item.label}</span>
                  {#if item.desc}
                    <span class="cp-item-desc">{item.desc}</span>
                  {/if}
                </span>
                {#if item.badge}
                  <span class="cp-badge">{item.badge}</span>
                {/if}
              </button>
            {/each}
          {/each}
        {/if}
      </div>

      <div class="cp-footer">
        <span><kbd>↑</kbd><kbd>↓</kbd> naviguer</span>
        <span><kbd>↵</kbd> exécuter</span>
        <span><kbd>ESC</kbd> fermer</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .cp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 12vh 16px 16px;
  }

  .cp-modal {
    width: 100%;
    max-width: 540px;
    background: var(--panel);
    border: 1px solid var(--line2);
    border-radius: 10px;
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.38);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    color: var(--text);
  }

  .cp-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--line);
  }

  .cp-search-icon {
    color: var(--muted);
    flex-shrink: 0;
  }

  .cp-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text);
    font-size: 14px;
    font-family: inherit;
    min-width: 0;
  }

  .cp-input::placeholder {
    color: var(--muted);
  }

  .cp-esc {
    font-size: 10px;
    font-family: inherit;
    padding: 2px 6px;
    border-radius: 4px;
    border: 1px solid var(--line2);
    color: var(--muted);
    background: var(--bg2, transparent);
    flex-shrink: 0;
  }

  .cp-list {
    max-height: 360px;
    overflow-y: auto;
    padding: 6px;
  }

  .cp-group-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
    padding: 8px 10px 4px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .cp-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: none;
    background: transparent;
    color: var(--text);
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    font-size: 13px;
  }

  .cp-item.selected {
    background: var(--hover, rgba(255, 255, 255, 0.07));
  }

  .cp-item-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    flex-shrink: 0;
  }

  .cp-item.selected .cp-item-icon {
    color: var(--text);
  }

  .cp-item-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .cp-item-label {
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cp-item-desc {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .cp-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 4px;
    border: 1px solid var(--line2);
    color: var(--muted);
    flex-shrink: 0;
  }

  .cp-empty {
    padding: 24px 12px;
    text-align: center;
    font-size: 13px;
    color: var(--muted);
  }

  .cp-footer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 14px;
    border-top: 1px solid var(--line);
    font-size: 11px;
    color: var(--muted);
  }

  .cp-footer kbd {
    font-family: inherit;
    font-size: 10px;
    padding: 1px 4px;
    border-radius: 3px;
    border: 1px solid var(--line2);
    margin-right: 3px;
  }
</style>
