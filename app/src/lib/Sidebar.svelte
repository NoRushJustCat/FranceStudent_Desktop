<script>
  import { onDestroy } from 'svelte';
  import BrandLogo from './BrandLogo.svelte';
  import { normalizeTheme } from './themes.js';

  let {
    chats = [],
    chatsLoaded = false,
    activeChatId = null,
    getState = () => '',
    theme = 'sombre',
    eco = false,
    searchQuery = '',
    searching = false,
    savedMessages = [],
    archivedChats = [],
    filterTab = $bindable('all'),
    ontheme,
    onsearch,
    onnew,
    onopen,
    onrename,
    onpin,
    onarchive,
    ondelete,
    onsettings,
    onopensaved,
    onunsavemsg,
    oncopysaved,
    onrestore
  } = $props();

  const FAV_THEMES = [
    { id: 'sombre', label: 'Sombre', color: '#09090b' },
    { id: 'gradien', label: 'Gradien', color: '#5865f2' },
    { id: 'aurora', label: 'Aurora', color: '#6ee7c8' },
    { id: 'clair', label: 'Clair', color: '#f4f4f5' }
  ];

  let activeTheme = $derived(normalizeTheme(theme));
  let editingId = $state(null);
  let editingTitle = $state('');
  let confirmDeleteId = $state(null);
  let confirmTimer = null;

  onDestroy(() => clearTimeout(confirmTimer));

  let pinnedChatsCount = $derived((Array.isArray(chats) ? chats : []).filter((c) => c.pinned || c.isPinned).length);

  let filteredChats = $derived.by(() => {
    let list = Array.isArray(chats) ? [...chats] : [];
    if (filterTab === 'pinned') {
      list = list.filter((c) => c.pinned || c.isPinned);
    }
    list.sort((a, b) => {
      const ap = !!(a.pinned || a.isPinned);
      const bp = !!(b.pinned || b.isPinned);
      if (ap && !bp) return -1;
      if (!ap && bp) return 1;
      return 0;
    });
    return list;
  });

  let filteredSaved = $derived.by(() => {
    const q = (searchQuery || '').trim().toLowerCase();
    let list = Array.isArray(savedMessages) ? savedMessages : [];
    if (q) {
      list = list.filter(
        (m) =>
          (m.content || m.text || '').toLowerCase().includes(q) ||
          (m.chatTitle || '').toLowerCase().includes(q)
      );
    }
    return list;
  });

  let filteredArchived = $derived.by(() => {
    const q = (searchQuery || '').trim().toLowerCase();
    let list = Array.isArray(archivedChats) ? archivedChats : [];
    if (q) {
      list = list.filter((c) => (c.title || 'Sans titre').toLowerCase().includes(q));
    }
    return list;
  });

  function truncate(str, max = 24) {
    if (!str) return 'Sans titre';
    const s = String(str).replace(/\s+/g, ' ').trim();
    if (s.length <= max) return s;
    return s.slice(0, max).trimEnd() + '\u2026';
  }

  const focusSelect = (node) => {
    node.focus();
    node.select();
  };

  function startRename(chat, e) {
    e.stopPropagation();
    editingId = chat.id;
    editingTitle = chat.title || '';
    confirmDeleteId = null;
  }

  function commitRename(chat) {
    if (editingId !== chat.id) return;
    const next = editingTitle.trim();
    editingId = null;
    if (next && next !== chat.title) {
      onrename?.(chat.id, next);
    }
  }

  function cancelRename() {
    editingId = null;
    editingTitle = '';
  }

  function handleRenameKey(e, chat) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename(chat);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelRename();
    }
  }

  function handleDeleteClick(chat, e) {
    e.stopPropagation();
    if (confirmDeleteId === chat.id) {
      clearTimeout(confirmTimer);
      confirmDeleteId = null;
      ondelete?.(chat.id);
    } else {
      confirmDeleteId = chat.id;
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => {
        if (confirmDeleteId === chat.id) {
          confirmDeleteId = null;
        }
      }, 2800);
    }
  }

  function handleSearchInput(e) {
    onsearch?.(e.currentTarget.value);
  }

  function clearSearch() {
    onsearch?.('');
  }
</script>

<aside class="sidebar">
  <header class="brand">
    <div class="brand-left">
      <BrandLogo class="logo" size={32} radius={8} />
      <div class="brand-text">
        <span class="brand-title">France Student</span>
        <span class="brand-badge">Desktop</span>
      </div>
    </div>

    <div class="themes" role="group" aria-label="Sélecteur de thème">
      {#each FAV_THEMES as t (t.id)}
        <button
          type="button"
          class="theme-dot"
          class:active={activeTheme === t.id}
          style="--dot-color: {t.color}"
          title={t.label}
          aria-label={`Thème ${t.label}`}
          aria-pressed={activeTheme === t.id}
          onclick={() => ontheme?.(t.id)}
        ></button>
      {/each}
      <button
        type="button"
        class="palette-btn"
        title="Tous les thèmes (Apparence)"
        aria-label="Ouvrir les 20 thèmes"
        onclick={() => onsettings?.('apparence')}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="13.5" cy="6.5" r="0.5" fill="currentColor" />
          <circle cx="17.5" cy="10.5" r="0.5" fill="currentColor" />
          <circle cx="8.5" cy="7.5" r="0.5" fill="currentColor" />
          <circle cx="6.5" cy="12.5" r="0.5" fill="currentColor" />
          <path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z" />
        </svg>
      </button>
    </div>
  </header>

  <div class="top-actions">
    <button type="button" class="newbtn" onclick={() => onnew?.()}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <line x1="12" y1="5" x2="12" y2="19" />
        <line x1="5" y1="12" x2="19" y2="12" />
      </svg>
      <span>Nouveau chat</span>
    </button>

    <div class="search-wrap">
      <svg class="search-ico" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <circle cx="11" cy="11" r="7" />
        <path d="M20 20L16.2 16.2" stroke-linecap="round" />
      </svg>
      <input
        type="text"
        class="search-input"
        placeholder="Rechercher…"
        value={searchQuery}
        oninput={handleSearchInput}
        aria-label="Rechercher dans les chats"
      />
      {#if searching}
        <span class="search-spin" title="Recherche en cours" aria-hidden="true"></span>
      {:else if searchQuery}
        <button
          type="button"
          class="search-clear"
          title="Effacer la recherche"
          aria-label="Effacer la recherche"
          onclick={clearSearch}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      {/if}
    </div>

    <div class="tabs" role="tablist" aria-label="Filtrer les éléments">
      <button
        type="button"
        role="tab"
        class="tab"
        class:active={filterTab === 'all'}
        aria-selected={filterTab === 'all'}
        title="Toutes les discussions"
        onclick={() => (filterTab = 'all')}
      >
        <span>Tous</span>
      </button>
      <button
        type="button"
        role="tab"
        class="tab"
        class:active={filterTab === 'pinned'}
        aria-selected={filterTab === 'pinned'}
        onclick={() => (filterTab = 'pinned')}
        title="Discussions épinglées"
      >
        <span>Épinglés</span>
        {#if pinnedChatsCount > 0}
          <span class="tab-count">{pinnedChatsCount}</span>
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        class="tab"
        class:active={filterTab === 'messages'}
        aria-selected={filterTab === 'messages'}
        aria-label="Signets"
        title="Messages ajoutés aux signets"
        onclick={() => (filterTab = 'messages')}
      >
        <span>Signets</span>
        {#if savedMessages.length > 0}
          <span class="tab-count">{savedMessages.length}</span>
        {/if}
      </button>
      <button
        type="button"
        role="tab"
        class="tab"
        class:active={filterTab === 'archived'}
        aria-selected={filterTab === 'archived'}
        title="Discussions archivées"
        onclick={() => (filterTab = 'archived')}
      >
        <span>Archivés</span>
        {#if archivedChats.length > 0}
          <span class="tab-count">{archivedChats.length}</span>
        {/if}
      </button>
    </div>
  </div>

  <div class="chat-list" role="list">
    {#if filterTab === 'messages'}
      {#if filteredSaved.length === 0}
        <div class="empty-state">
          <div class="empty-title">Aucun signet</div>
          <div class="empty-sub">Ajoute une réponse aux signets depuis une discussion pour la retrouver ici.</div>
        </div>
      {:else}
        {#each filteredSaved as msg (msg.id || `${msg.chatId}:${msg.messageId}`)}
          <div class="saved-card" role="listitem">
            <button
              type="button"
              class="saved-main"
              onclick={() => onopensaved?.(msg)}
              title="Ouvrir dans la discussion"
            >
              <span class="saved-text">{truncate(msg.content || msg.text)}</span>
              <span class="saved-meta">{msg.chatTitle || 'Discussion'}</span>
            </button>
            <div class="saved-actions">
              <button
                type="button"
                class="act-btn"
                title="Copier le message"
                aria-label="Copier le message"
                onclick={(e) => {
                  e.stopPropagation();
                  oncopysaved?.(msg);
                }}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                </svg>
              </button>
              <button
                type="button"
                class="act-btn danger"
                title="Retirer des signets"
                aria-label="Retirer des signets"
                onclick={(e) => {
                  e.stopPropagation();
                  onunsavemsg?.(msg);
                }}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z" />
                  <line x1="9.5" y1="8.5" x2="14.5" y2="13.5" />
                  <line x1="14.5" y1="8.5" x2="9.5" y2="13.5" />
                </svg>
              </button>
            </div>
          </div>
        {/each}
      {/if}
    {:else if filterTab === 'archived'}
      {#if filteredArchived.length === 0}
        <div class="empty-state">
          <div class="empty-title">Aucune archive</div>
          <div class="empty-sub">Les discussions archivées apparaîtront ici.</div>
        </div>
      {:else}
        {#each filteredArchived as chat (chat.id)}
          <div class="chat-row archived-row" class:confirming={confirmDeleteId === chat.id} role="listitem">
            <button
              type="button"
              class="chat-main archived-main"
              onclick={() => onopen?.(chat.id)}
              title={chat.title || 'Sans titre'}
            >
              <span class="chat-title">{truncate((chat.title || 'Sans titre').replace(/\s*\(archivé\)\s*$/i, ''), 24)}</span>
              <span class="archived-meta">Discussion archivée</span>
            </button>
            <div class="row-actions">
              <button
                type="button"
                class="restore-btn"
                title="Restaurer la discussion"
                aria-label="Restaurer la discussion"
                onclick={(e) => {
                  e.stopPropagation();
                  onrestore?.(chat);
                }}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                  <path d="M3 3v5h5" />
                </svg>
              </button>
              <button
                type="button"
                class="act-btn danger"
                class:confirm={confirmDeleteId === chat.id}
                title={confirmDeleteId === chat.id ? 'Cliquer encore pour supprimer' : 'Supprimer définitivement'}
                aria-label="Supprimer définitivement"
                onclick={(e) => handleDeleteClick(chat, e)}
              >
                {#if confirmDeleteId === chat.id}
                  <span class="confirm-txt">?</span>
                {:else}
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" stroke-linecap="round" stroke-linejoin="round" />
                  </svg>
                {/if}
              </button>
            </div>
          </div>
        {/each}
      {/if}
    {:else if !chatsLoaded}
      <div class="empty-state">Chargement…</div>
    {:else if filteredChats.length === 0}
      <div class="empty-state">
        {#if searchQuery}
          Aucun résultat pour « {searchQuery} »
        {:else if filterTab === 'pinned'}
          Aucun chat épinglé
        {:else}
          Aucun chat pour le moment
        {/if}
      </div>
    {:else}
      {#each filteredChats as chat (chat.id)}
        {@const state = getState?.(chat) || ''}
        {@const isActive = chat.id === activeChatId}
        {@const isEditing = editingId === chat.id}
        {@const isConfirming = confirmDeleteId === chat.id}

        <div
          class="chat-row"
          class:active={isActive}
          class:confirming={isConfirming}
          role="listitem"
        >
          {#if isEditing}
            <input
              type="text"
              class="rename-input"
              use:focusSelect
              bind:value={editingTitle}
              onkeydown={(e) => handleRenameKey(e, chat)}
              onblur={() => commitRename(chat)}
              aria-label="Renommer le chat"
            />
          {:else}
            <button
              type="button"
              class="chat-main"
              onclick={() => onopen?.(chat.id)}
              title={chat.title || 'Sans titre'}
            >
              <span
                class="ind"
                class:busy={state === 'busy'}
                class:wait={state === 'wait'}
                class:unread={state === 'unread'}
                aria-hidden="true"
              ></span>

              <span class="pin-slot" aria-hidden="true">
                {#if chat.pinned}
                  <svg class="pin-ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-label="Épinglé">
                    <line x1="12" y1="17" x2="12" y2="22" />
                    <path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24Z" />
                  </svg>
                {/if}
              </span>

              <span class="chat-title">{truncate(chat.title, 24)}</span>
            </button>

            <div class="row-actions">
              <button
                type="button"
                class="act-btn"
                class:pinned={chat.pinned}
                title={chat.pinned ? 'Désépingler' : 'Épingler'}
                aria-label={chat.pinned ? 'Désépingler' : 'Épingler'}
                onclick={(e) => {
                  e.stopPropagation();
                  onpin?.(chat);
                }}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M12 17v5M9 3h6l-1 7 3 3v2H7v-2l3-3-1-7z" stroke-linecap="round" stroke-linejoin="round" />
                </svg>
              </button>

              <button
                type="button"
                class="act-btn"
                title="Renommer"
                aria-label="Renommer"
                onclick={(e) => startRename(chat, e)}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" stroke-linecap="round" stroke-linejoin="round" />
                </svg>
              </button>

              <button
                type="button"
                class="act-btn"
                title="Archiver"
                aria-label="Archiver"
                onclick={(e) => {
                  e.stopPropagation();
                  onarchive?.(chat);
                }}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <rect x="3" y="4" width="18" height="4" rx="1" />
                  <path d="M5 8v11a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8M10 12h4" stroke-linecap="round" />
                </svg>
              </button>

              <button
                type="button"
                class="act-btn danger"
                class:confirm={isConfirming}
                title={isConfirming ? 'Cliquer encore pour supprimer' : 'Supprimer'}
                aria-label={isConfirming ? 'Confirmer la suppression' : 'Supprimer'}
                onclick={(e) => handleDeleteClick(chat, e)}
              >
                {#if isConfirming}
                  <span class="confirm-txt">?</span>
                {:else}
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" stroke-linecap="round" stroke-linejoin="round" />
                  </svg>
                {/if}
              </button>
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <footer class="foot">
    <button type="button" class="foot-btn" onclick={() => onsettings?.('apparence')}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
      <span>Paramètres</span>
    </button>

    {#if eco}
      <button
        type="button"
        class="eco-pill"
        title="Mode Éco actif — ouvrir les réglages Usage"
        onclick={() => onsettings?.('usage')}
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z" />
        </svg>
        <span>ÉCO</span>
      </button>
    {/if}
  </footer>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    background: var(--panel);
    border-right: 1px solid var(--line);
    color: var(--text);
    user-select: none;
    overflow: hidden;
  }

  .brand {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: var(--topbar-h, 52px);
    box-sizing: border-box;
    padding: 0 var(--side-px, 12px);
    border-bottom: 1px solid var(--line);
    flex-shrink: 0;
  }

  .brand-left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .brand-left :global(.logo) {
    flex-shrink: 0;
    border-radius: 8px;
    box-shadow: 0 2px 8px var(--accent-glow);
  }

  .brand-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .brand-title {
    font-size: 13.5px;
    font-weight: 700;
    line-height: 1.2;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .brand-badge {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--accent-text);
    letter-spacing: 0.03em;
  }

  .themes {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .theme-dot {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.34);
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.32);
    background: var(--dot-color);
    cursor: pointer;
    padding: 0;
    transition: transform 0.15s ease, box-shadow 0.15s ease;
  }

  :global(:root[data-theme='light']) .theme-dot {
    border-color: rgba(15, 23, 42, 0.28);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.35);
  }

  .theme-dot:hover {
    transform: scale(1.15);
  }

  .theme-dot.active {
    box-shadow: 0 0 0 1.5px var(--panel), 0 0 0 3px #ffffff;
  }

  :global(:root[data-theme='clair']) .theme-dot.active,
  :global(:root[data-theme='fs-light']) .theme-dot.active {
    box-shadow: 0 0 0 1.5px var(--panel), 0 0 0 3px #0f172a;
  }

  .top-actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px var(--side-px, 12px) 8px;
  }

  .newbtn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    padding: 9px 12px;
    border: 1px solid var(--accent-line);
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--on-accent, #ffffff);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 2px 10px var(--accent-glow);
    transition: filter 0.15s ease, transform 0.1s ease, background 0.18s ease, color 0.18s ease;
  }

  .newbtn:hover {
    filter: brightness(1.08);
  }

  .newbtn:active {
    transform: translateY(1px);
  }

  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-ico {
    position: absolute;
    left: 10px;
    color: var(--dim);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    padding: 7px 28px 7px 30px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--panel2);
    color: var(--text);
    font-size: 12.5px;
    outline: none;
    transition: border-color 0.15s ease, background 0.15s ease;
  }

  .search-input::placeholder {
    color: var(--faint);
  }

  .search-input:focus {
    border-color: var(--accent-line);
    background: var(--raised);
  }

  .search-clear {
    position: absolute;
    right: 6px;
    width: 20px;
    height: 20px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--dim);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .search-clear:hover {
    color: var(--text);
    background: var(--hover);
  }

  .search-spin {
    position: absolute;
    right: 9px;
    width: 12px;
    height: 12px;
    border: 2px solid var(--line2);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    padding: 3px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }

  .tab {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    height: 29px;
    padding: 0 6px;
    border: 1px solid transparent;
    border-radius: calc(var(--radius-sm) - 2px);
    background: transparent;
    color: var(--dim);
    font-size: 11.5px;
    line-height: 1;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }

  .tab:hover {
    color: var(--text);
    background: transparent;
    border-color: transparent;
  }

  .tab.active {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent-text);
    font-weight: 600;
  }

  .tab-count {
    font-size: 10px;
    font-weight: 600;
    line-height: 1.1;
    padding: 1px 5px;
    border-radius: 999px;
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    color: var(--accent-text);
  }

  .palette-btn {
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--panel2);
    color: var(--dim);
    cursor: pointer;
    padding: 0;
    transition: background 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  }

  .palette-btn:hover {
    color: var(--text);
    background: var(--hover);
    border-color: var(--accent-line);
  }

  .empty-title {
    font-weight: 600;
    color: var(--dim);
    margin-bottom: 4px;
  }

  .empty-sub {
    font-size: 11.5px;
    color: var(--faint);
    line-height: 1.4;
  }

  .saved-card {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--line);
    background: var(--panel2);
    margin-bottom: 4px;
    transition: border-color 0.15s ease, background 0.15s ease;
  }

  .saved-card:hover {
    border-color: var(--line2);
    background: var(--hover);
  }

  .saved-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 0;
    border: none;
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .saved-text {
    font-size: 12.5px;
    line-height: 1.35;
    color: var(--text);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .saved-meta {
    font-size: 11px;
    color: var(--dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .saved-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  .restore-btn {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    padding: 0;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .restore-btn:hover {
    background: var(--raised);
    color: var(--accent-text);
  }

  .eco-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .chat-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px var(--side-px, 12px) 10px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .empty-state {
    padding: 24px 12px;
    text-align: center;
    font-size: 12.5px;
    color: var(--faint);
  }

  .chat-row {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: var(--radius-sm);
    border: 1px solid transparent;
    transition: background 0.12s ease, border-color 0.12s ease;
  }

  .chat-row:hover {
    background: var(--hover);
  }

  .chat-row.active {
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }

  .chat-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 6px 8px 8px;
    border: none;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    font-size: 12.5px;
  }

  .chat-main:hover:not(:disabled) {
    background: transparent;
    border-color: transparent;
  }

  .chat-row.active .chat-main {
    color: var(--accent-text);
    font-weight: 600;
  }

  .archived-row {
    background: var(--panel2);
    border-color: var(--line);
    padding: 2px 4px;
  }

  .archived-row:hover {
    border-color: var(--line2);
  }

  .archived-main {
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    padding: 7px 8px;
  }

  .archived-main .chat-title {
    flex: 0 0 auto;
    width: 100%;
    font-weight: 500;
  }

  .archived-meta {
    font-size: 11px;
    color: var(--dim);
  }

  .archived-row .row-actions {
    display: flex;
    padding-right: 4px;
  }

  .ind {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: transparent;
  }

  .ind.busy {
    background: var(--state-running, var(--warn));
    box-shadow: 0 0 6px var(--accent-glow);
    animation: pulse 1.2s ease-in-out infinite;
  }

  .ind.wait {
    background: var(--state-running, var(--warn));
  }

  .ind.unread {
    background: var(--ok);
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  .pin-slot {
    width: 14px;
    height: 14px;
    display: inline-grid;
    place-items: center;
    flex-shrink: 0;
  }

  .pin-ico {
    flex-shrink: 0;
    color: var(--accent-text);
    opacity: 0.9;
  }

  .chat-title {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-actions {
    display: none;
    align-items: center;
    gap: 2px;
    padding-right: 4px;
    flex-shrink: 0;
  }

  .chat-row:hover .row-actions,
  .chat-row.confirming .row-actions {
    display: flex;
  }

  .act-btn {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    padding: 0;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .act-btn:hover {
    background: var(--raised);
    color: var(--text);
  }

  .act-btn.pinned {
    color: var(--accent-text);
  }

  .act-btn.danger:hover,
  .act-btn.danger.confirm {
    background: var(--danger);
    color: #ffffff;
  }

  .confirm-txt {
    font-size: 12px;
    font-weight: 700;
    line-height: 1;
  }

  .rename-input {
    width: 100%;
    margin: 4px 6px;
    padding: 5px 8px;
    border: 1px solid var(--accent-line);
    border-radius: var(--radius-sm);
    background: var(--raised);
    color: var(--text);
    font-size: 12.5px;
    outline: none;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 10px 12px;
    border-top: 1px solid var(--line);
    background: var(--panel);
  }

  .foot-btn {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--dim);
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }

  .foot-btn:hover {
    background: var(--hover);
    color: var(--text);
  }

  .eco-pill {
    padding: 3px 8px;
    border: 1px solid var(--accent-line);
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: filter 0.15s ease;
  }

  .eco-pill:hover {
    filter: brightness(1.12);
  }
</style>
