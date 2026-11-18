<script>
  import { onDestroy } from 'svelte';
  import BrandLogo from './BrandLogo.svelte';

  let {
    collapsed = $bindable(false),
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
    onrestore,
    projects = [],
    projectId = null,
    onproject
  } = $props();

  const FILTERS = [
    { id: 'all', label: 'Tous' },
    { id: 'pinned', label: 'Épinglés' },
    { id: 'messages', label: 'Signets' },
    { id: 'archived', label: 'Archivés' }
  ];

  let filterOpen = $state(false);
  let projOpen = $state(false);
  let projName = $derived(projects.find((p) => p.id === projectId)?.name || 'Projet');

  function countFor(id) {
    if (id === 'pinned') return pinnedChatsCount;
    if (id === 'messages') return savedMessages.length;
    if (id === 'archived') return archivedChats.length;
    return 0;
  }

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

<svelte:window
  onclick={(e) => {
    if (filterOpen && !e.target.closest?.('.search-wrap')) filterOpen = false;
    if (projOpen && !e.target.closest?.('.proj-wrap')) projOpen = false;
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      filterOpen = false;
      projOpen = false;
    }
  }}
/>

<aside class="sidebar" class:collapsed>
  <header class="brand">
    <div class="brand-left">
      {#if collapsed}
        <button class="logo-toggle" type="button" title="Déplier le menu" aria-label="Déplier le menu" aria-expanded="false" onclick={() => { collapsed = false; }}>
          <BrandLogo class="rail-logo" size={30} radius={8} />
          <svg class="reopen-icon" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M9 4v16m4-12 3 4-3 4"/></svg>
        </button>
      {:else}<BrandLogo class="rail-logo" size={30} radius={8} />{/if}
      <div class="brand-text">
        <span class="brand-title">France Student</span>
        <span class="brand-badge">Desktop</span>
      </div>
    </div>
    <button class="collapse-btn" type="button" title={collapsed ? 'Déplier le menu' : 'Replier le menu'} aria-label={collapsed ? 'Déplier le menu' : 'Replier le menu'} aria-expanded={!collapsed} onclick={() => { collapsed = !collapsed; projOpen = false; filterOpen = false; }}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M9 4v16m6-12-3 4 3 4"/></svg>
    </button>
  </header>

  {#if collapsed}
    <button class="rail-btn" title="Conversations" aria-label="Conversations" onclick={() => { collapsed = false; filterTab = 'all'; onproject?.(null); }}>
      <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8z"/></svg>
    </button>
    <button class="rail-btn" title="Projets" aria-label="Projets" onclick={() => { collapsed = false; setTimeout(() => (projOpen = true), 0); }}>
      <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
    </button>
  {/if}
  <div class="sidebar-content">
  <div class="top-actions">
    <div class="new-row">
      <button type="button" class="newbtn" onclick={() => onnew?.()}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="12" y1="5" x2="12" y2="19" />
          <line x1="5" y1="12" x2="19" y2="12" />
        </svg>
        <span>Nouveau chat</span>
      </button>

      <div class="proj-wrap">
        <button
          type="button"
          class="projbtn"
          class:active={projectId}
          aria-haspopup="menu"
          aria-expanded={projOpen}
          title="Projets"
          onclick={() => (projOpen = !projOpen)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </svg>
          <span>Projets</span>
        </button>
        {#if projOpen}
          <div class="filter-menu proj-menu" role="menu">
            <button
              type="button"
              role="menuitemradio"
              aria-checked={!projectId}
              class="filter-item"
              class:active={!projectId}
              onclick={() => {
                projOpen = false;
                onproject?.(null);
              }}
            >
              <span class="filter-label">Chats</span>
            </button>
            {#each projects as p (p.id)}
              <button
                type="button"
                role="menuitemradio"
                aria-checked={projectId === p.id}
                class="filter-item"
                class:active={projectId === p.id}
                onclick={() => {
                  projOpen = false;
                  onproject?.(p.id);
                }}
              >
                <span class="filter-label">{truncate(p.name, 26)}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>

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
      <button
        type="button"
        class="filter-btn"
        class:active={filterTab !== 'all'}
        aria-haspopup="menu"
        aria-expanded={filterOpen}
        aria-label="Filtrer"
        title="Filtrer : Tous, Épinglés, Signets, Archivés"
        onclick={() => (filterOpen = !filterOpen)}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
        </svg>
        {#if filterTab !== 'all'}
          <span class="filter-dot"></span>
        {/if}
      </button>
      {#if filterOpen}
        <div class="filter-menu" role="menu">
          {#each FILTERS as f (f.id)}
            <button
              type="button"
              role="menuitemradio"
              aria-checked={filterTab === f.id}
              class="filter-item"
              class:active={filterTab === f.id}
              onclick={() => {
                filterTab = f.id;
                filterOpen = false;
              }}
            >
              <span class="filter-label">{f.label}</span>
              {#if countFor(f.id) > 0}
                <span class="tab-count">{countFor(f.id)}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  <div class="proj-now">
    <span class="proj-now-name" title={projectId ? projName : 'Chats'}>Projet ouvert : {projectId ? projName : 'Chats'}</span>
    {#if projectId}
      <button type="button" class="proj-back" onclick={() => onproject?.(null)}>← Chats</button>
    {/if}
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
              <span class="chat-title">{(chat.title || 'Sans titre').replace(/\s*\(archivé\)\s*$/i, '')}</span>
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
          {projectId ? 'Aucun chat dans ce projet' : 'Aucun chat pour le moment'}
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

       <span class="pin-slot" class:empty={!chat.pinned} aria-hidden="true">
                {#if chat.pinned}
                  <svg class="pin-ico" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-label="Épinglé">
                    <line x1="12" y1="17" x2="12" y2="22" />
                    <path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24Z" />
                  </svg>
                {/if}
              </span>

              <span class="chat-title">{chat.title || 'Sans titre'}</span>
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

  </div>
  <footer class="foot">
    <button type="button" class="foot-btn" title="Paramètres" aria-label="Paramètres" onclick={() => onsettings?.('apparence')}>
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
  .sidebar-content { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .collapse-btn, .rail-btn { display: inline-flex; align-items: center; justify-content: center; padding: 0; border: 0; border-radius: 7px; background: transparent; color: var(--dim); cursor: pointer; flex-shrink: 0; }
  .collapse-btn { width: 26px; height: 26px; }
  .collapse-btn:hover, .rail-btn:hover { background: var(--hover); color: var(--text); }
  .rail-btn { width: 36px; height: 36px; margin: 4px auto; }
  :global(.rail-logo) { width: 30px; height: 30px; object-fit: contain; }
  .sidebar.collapsed .brand { height: 76px; padding: 10px 0 6px; flex-direction: column; gap: 4px; border-bottom: 0; }
  .logo-toggle { position: relative; display: grid; place-items: center; width: 36px; height: 36px; padding: 0; border: 0; border-radius: 8px; background: transparent; color: var(--text); cursor: pointer; }
  .reopen-icon { position: absolute; opacity: 0; }
  .logo-toggle:hover, .logo-toggle:focus-visible { background: var(--hover); }
  .logo-toggle:hover :global(.rail-logo), .logo-toggle:focus-visible :global(.rail-logo) { opacity: 0; }
  .logo-toggle:hover .reopen-icon, .logo-toggle:focus-visible .reopen-icon { opacity: 1; }
  .sidebar.collapsed .brand { height: var(--topbar-h, 52px); padding: 8px 0; }
  .sidebar.collapsed .collapse-btn, .sidebar.collapsed .brand-text, .sidebar.collapsed .sidebar-content, .sidebar.collapsed .foot-btn span, .sidebar.collapsed .eco-pill { display: none; }
  .sidebar.collapsed .foot { margin-top: auto; padding: 10px 0; justify-content: center; }
  .sidebar.collapsed .foot-btn { width: 36px; height: 36px; padding: 0; justify-content: center; }
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
    box-shadow: none;
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
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--bg);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    box-shadow: none;
    transition: filter 0.15s ease, transform 0.1s ease, background 0.18s ease, color 0.18s ease;
  }

  .newbtn:hover {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent-text);
  }

  .newbtn:active {
    transform: translateY(1px);
  }

  .new-row {
    display: flex;
    gap: 8px;
  }

  .new-row .newbtn {
    flex: 1;
    width: auto;
    min-width: 0;
  }

  .proj-wrap {
    position: relative;
    display: flex;
  }

  .projbtn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--panel2);
    color: var(--text);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .projbtn:hover {
    background: var(--hover);
  }

  .projbtn.active {
    border-color: var(--accent-line);
    color: var(--accent-text);
  }

  .proj-menu {
    max-height: 260px;
    overflow-y: auto;
  }

  .proj-now {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 8px;
    padding: 0 var(--side-px, 12px) 4px;
    text-align: left;
    font-size: 11.5px;
    color: var(--dim);
  }

  .proj-now-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .proj-back {
    flex: none;
    margin-left: auto;
    padding: 2px 6px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--accent-text);
    font-size: 11.5px;
    cursor: pointer;
  }

  .proj-back:hover {
    background: var(--hover);
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
    padding: 7px 60px 7px 30px;
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
    right: 34px;
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
    right: 38px;
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

  .filter-btn {
    position: absolute;
    right: 4px;
    top: 50%;
    margin-top: -13px;
    width: 26px;
    height: 26px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }

  .filter-btn:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
    transform: none;
    box-shadow: none;
  }

  .filter-btn.active {
    color: var(--accent-text);
  }

  .filter-dot {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 2px var(--panel2);
  }

  .filter-menu {
    position: absolute;
    right: 0;
    top: calc(100% + 6px);
    z-index: 40;
    min-width: 170px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    border: 1px solid var(--line2);
    border-radius: 12px;
    background: var(--raised);
    box-shadow: var(--shadow);
  }

  .filter-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .filter-item:hover:not(:disabled) {
    background: var(--hover);
    transform: none;
    box-shadow: none;
  }

  .filter-item.active {
    background: var(--hover);
    color: var(--text);
    font-weight: 500;
  }

  .tab-count {
    font-size: 10px;
    font-weight: 500;
    line-height: 1.1;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--hover);
    border: 1px solid var(--line);
    color: var(--dim);
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
    gap: 1px;
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
    background: var(--hover);
    border-color: transparent;
  }

  .chat-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 6px 6px 8px;
    border: none;
    background: transparent;
    color: var(--dim);
    text-align: left;
    justify-content: flex-start;
    cursor: pointer;
    font-size: 13px;
  }

  .chat-main:hover:not(:disabled) {
    background: transparent;
    border-color: transparent;
  }

  .chat-row:hover .chat-main,
  .chat-row.active .chat-main {
    color: var(--text);
  }

  .chat-row.active .chat-main {
    font-weight: 500;
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

  /* Pas d'espace réservé quand il n'y a ni indicateur ni épingle : titres collés à gauche */
  .ind:not(.busy):not(.wait):not(.unread),
  .pin-slot.empty {
    display: none;
  }

  .ind.busy {
    background: var(--state-running, var(--warn));
    box-shadow: none;
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
    text-align: left;
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
