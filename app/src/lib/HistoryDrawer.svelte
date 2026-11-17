<script>
  import { fade, fly } from 'svelte/transition';

  let {
    open = false,
    chats = [],
    archivedChats = [],
    savedMessages = [],
    activeChatId = null,
    onClose = () => {},
    onSelectChat = (id) => {},
    onNewChat = () => {},
    onPinChat = (id, pinned) => {},
    onRenameChat = (id, title) => {},
    onArchiveChat = (id) => {},
    onRestoreChat = (id) => {},
    onDeleteChat = (id) => {}
  } = $props();

  let activeTab = $state('recent');
  let searchQuery = $state('');
  let editingId = $state(null);
  let editingTitle = $state('');

  let filteredChats = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    const list = q
      ? chats.filter((c) => (c.title || '').toLowerCase().includes(q))
      : chats;
    const pinned = list.filter((c) => c.pinned);
    const others = list.filter((c) => !c.pinned);
    return [...pinned, ...others];
  });

  let filteredArchived = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    return q
      ? archivedChats.filter((c) => (c.title || '').toLowerCase().includes(q))
      : archivedChats;
  });

  let filteredSaved = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    return q
      ? savedMessages.filter((m) =>
          (m.title || m.content || m.text || '').toLowerCase().includes(q)
        )
      : savedMessages;
  });

  function handleKeydown(e) {
    if (open && e.key === 'Escape') {
      if (editingId) {
        editingId = null;
        editingTitle = '';
      } else {
        onClose();
      }
    }
  }

  function startRename(chat, e) {
    e.stopPropagation();
    editingId = chat.id;
    editingTitle = chat.title || '';
  }

  function commitRename(id) {
    const trimmed = editingTitle.trim();
    if (trimmed) {
      onRenameChat(id, trimmed);
    }
    editingId = null;
    editingTitle = '';
  }

  function handleRenameKeydown(id, e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename(id);
    } else if (e.key === 'Escape') {
      e.stopPropagation();
      editingId = null;
      editingTitle = '';
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div
    class="drawer-backdrop"
    transition:fade={{ duration: 180 }}
    onclick={onClose}
    role="presentation"
    aria-hidden="true"
  ></div>

  <aside class="history-drawer" transition:fly={{ x: -290, duration: 220 }} aria-label="Historique des conversations">
    <div class="drawer-header">
      <div class="search-box">
        <svg class="search-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="11" cy="11" r="8" />
          <path d="m21 21-4.3-4.3" />
        </svg>
        <input
          type="text"
          class="search-input"
          placeholder="Rechercher..."
          bind:value={searchQuery}
        />
        {#if searchQuery}
          <button
            type="button"
            class="clear-btn"
            onclick={() => (searchQuery = '')}
            aria-label="Effacer la recherche"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            </svg>
          </button>
        {/if}
      </div>

      <button type="button" class="new-btn" onclick={onNewChat} title="Nouvelle conversation">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 5v14" />
          <path d="M5 12h14" />
        </svg>
        <span>Nouveau</span>
      </button>
    </div>

    <div class="drawer-tabs" role="tablist">
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === 'recent'}
        class="tab-btn"
        class:active={activeTab === 'recent'}
        onclick={() => (activeTab = 'recent')}
      >
        Récents
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === 'favorites'}
        class="tab-btn"
        class:active={activeTab === 'favorites'}
        onclick={() => (activeTab = 'favorites')}
      >
        Favoris
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === 'archives'}
        class="tab-btn"
        class:active={activeTab === 'archives'}
        onclick={() => (activeTab = 'archives')}
      >
        Archives
      </button>
    </div>

    <div class="drawer-content">
      {#if activeTab === 'recent'}
        {#if filteredChats.length === 0}
          <div class="empty-state">Aucune conversation</div>
        {:else}
          <ul class="item-list">
            {#each filteredChats as chat (chat.id)}
              <li
                class="chat-row"
                class:active={chat.id === activeChatId}
              >
                {#if editingId === chat.id}
                  <input
                    type="text"
                    class="rename-input"
                    bind:value={editingTitle}
                    onkeydown={(e) => handleRenameKeydown(chat.id, e)}
                    onblur={() => commitRename(chat.id)}
                    autofocus
                  />
                {:else}
                  <button
                    type="button"
                    class="chat-select-btn"
                    onclick={() => onSelectChat(chat.id)}
                    title={chat.title || 'Sans titre'}
                  >
                    {#if chat.pinned}
                      <svg class="pin-indicator" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 17v5" />
                        <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />
                      </svg>
                    {:else}
                      <span class="pin-slot" aria-hidden="true" style="width: 12px; flex-shrink: 0;"></span>
                    {/if}
                    <span class="chat-title">{chat.title || 'Sans titre'}</span>
                  </button>

                  <div class="row-actions">
                    <button
                      type="button"
                      class="icon-btn"
                      class:pinned={chat.pinned}
                      onclick={(e) => {
                        e.stopPropagation();
                        onPinChat(chat.id, !chat.pinned);
                      }}
                      title={chat.pinned ? 'Désépingler' : 'Épingler'}
                      aria-label={chat.pinned ? 'Désépingler' : 'Épingler'}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 17v5" />
                        <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />
                      </svg>
                    </button>

                    <button
                      type="button"
                      class="icon-btn"
                      onclick={(e) => startRename(chat, e)}
                      title="Renommer"
                      aria-label="Renommer"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
                      </svg>
                    </button>

                    <button
                      type="button"
                      class="icon-btn"
                      onclick={(e) => {
                        e.stopPropagation();
                        onArchiveChat(chat.id);
                      }}
                      title="Archiver"
                      aria-label="Archiver"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                        <rect width="20" height="5" x="2" y="3" rx="1" />
                        <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
                        <path d="M10 12h4" />
                      </svg>
                    </button>

                    <button
                      type="button"
                      class="icon-btn danger"
                      onclick={(e) => {
                        e.stopPropagation();
                        onDeleteChat(chat.id);
                      }}
                      title="Supprimer"
                      aria-label="Supprimer"
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M3 6h18" />
                        <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
                        <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
                      </svg>
                    </button>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      {:else if activeTab === 'favorites'}
        {#if filteredSaved.length === 0}
          <div class="empty-state">Aucun message favori</div>
        {:else}
          <ul class="item-list">
            {#each filteredSaved as item (item.id)}
              <li class="chat-row">
                <button
                  type="button"
                  class="chat-select-btn"
                  onclick={() => onSelectChat(item.chatId || item.id)}
                  title={item.title || item.content || item.text || ''}
                >
                  <svg class="fav-indicator" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                    <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
                  </svg>
                  <span class="chat-title">
                    {item.title || item.content || item.text || 'Message enregistré'}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {:else if activeTab === 'archives'}
        {#if filteredArchived.length === 0}
          <div class="empty-state">Aucune archive</div>
        {:else}
          <ul class="item-list">
            {#each filteredArchived as chat (chat.id)}
              <li class="chat-row">
                <span class="archive-title" title={chat.title || 'Sans titre'}>
                  {chat.title || 'Sans titre'}
                </span>
                <button
                  type="button"
                  class="restore-btn"
                  onclick={() => onRestoreChat(chat.id)}
                  title="Restaurer"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                    <path d="M3 3v5h5" />
                  </svg>
                  <span>Restaurer</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>
  </aside>
{/if}

<style>
  .drawer-backdrop {
    position: fixed;
    top: 48px;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.35);
    z-index: 39;
  }

  .history-drawer {
    position: fixed;
    top: 48px;
    left: 0;
    bottom: 0;
    width: 290px;
    background: var(--panel);
    border-right: 1px solid var(--line);
    z-index: 40;
    display: flex;
    flex-direction: column;
    color: var(--text, #e5e7eb);
    font-size: 13px;
  }

  .drawer-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px;
    border-bottom: 1px solid var(--line);
  }

  .search-box {
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 8px;
    color: var(--muted, #9ca3af);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    height: 30px;
    padding: 0 24px 0 28px;
    background: var(--bg, #111318);
    border: 1px solid var(--line);
    border-radius: 6px;
    color: inherit;
    font-size: 12.5px;
    outline: none;
  }

  .search-input:focus {
    border-color: var(--accent, #6b7280);
  }

  .clear-btn {
    position: absolute;
    right: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: transparent;
    border: none;
    color: var(--muted, #9ca3af);
    cursor: pointer;
    padding: 0;
  }

  .new-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 30px;
    padding: 0 10px;
    background: var(--bg, #111318);
    border: 1px solid var(--line);
    border-radius: 6px;
    color: inherit;
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
  }

  .new-btn:hover {
    background: var(--hover, rgba(255, 255, 255, 0.06));
  }

  .drawer-tabs {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border-bottom: 1px solid var(--line);
    padding: 0 6px;
  }

  .tab-btn {
    height: 34px;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted, #9ca3af);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }

  .tab-btn:hover {
    color: var(--text, #e5e7eb);
  }

  .tab-btn.active {
    color: var(--text, #e5e7eb);
    border-bottom-color: var(--accent, #e5e7eb);
  }

  .drawer-content {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
  }

  .empty-state {
    padding: 24px 12px;
    text-align: center;
    color: var(--muted, #9ca3af);
    font-size: 12.5px;
  }

  .item-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .chat-row {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 34px;
    border-radius: 6px;
    padding: 0 6px;
  }

  .chat-row:hover {
    background: var(--hover, rgba(255, 255, 255, 0.05));
  }

  .chat-row.active {
    background: var(--active, rgba(255, 255, 255, 0.09));
  }

  .chat-select-btn {
    flex: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    padding: 0;
  }

  .chat-title,
  .archive-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
  }

  .pin-indicator,
  .fav-indicator {
    flex-shrink: 0;
    color: var(--muted, #9ca3af);
  }

  .row-actions {
    display: none;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
    margin-left: 4px;
  }

  .chat-row:hover .row-actions {
    display: flex;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted, #9ca3af);
    cursor: pointer;
    padding: 0;
  }

  .icon-btn:hover {
    background: var(--hover, rgba(255, 255, 255, 0.08));
    color: var(--text, #e5e7eb);
  }

  .icon-btn.pinned {
    color: var(--text, #e5e7eb);
  }

  .icon-btn.danger:hover {
    color: #ef4444;
  }

  .rename-input {
    width: 100%;
    height: 26px;
    padding: 0 6px;
    background: var(--bg, #111318);
    border: 1px solid var(--accent, #6b7280);
    border-radius: 4px;
    color: inherit;
    font-size: 12.5px;
    outline: none;
  }

  .restore-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    background: transparent;
    border: 1px solid var(--line);
    border-radius: 4px;
    color: var(--muted, #9ca3af);
    font-size: 11.5px;
    cursor: pointer;
    flex-shrink: 0;
    margin-left: 6px;
  }

  .restore-btn:hover {
    color: var(--text, #e5e7eb);
    background: var(--hover, rgba(255, 255, 255, 0.06));
  }
</style>
