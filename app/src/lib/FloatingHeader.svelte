<script>
  import BrandLogo from './BrandLogo.svelte';
  import PermMode from './PermMode.svelte';

  let {
    isLight = false,
    mode = 'chat',
    workdir = '',
    gitBranch = '',
    gitDirtyCount = 0,
    drawerOpen = false,
    inspectorOpen = false,
    ecoLabel = 'Éco',
    savedTokens = 0,
    usagePct = null,
    mcpConnected = 0,
    showDiff = false,
    panelTab = 'files',
    diffCount = 0,
    termUnread = 0,
    busy = false,
    onToggleDrawer = () => {},
    onNewChat = () => {},
    onToggleInspector = () => {},
    onOpenPalette = () => {},
    onOpenSettings = () => {},
    onPickFolder = () => {},
    onToggleDiff = () => {},
    onOpenEco = () => {}
  } = $props();

  let folderName = $derived(
    workdir ? workdir.split(/[\\/]/).filter(Boolean).pop() || 'Choisir dossier' : 'Choisir dossier'
  );
</script>

<header class="floating-header">
  <div class="header-left">
    <button
      type="button"
      class="icon-btn"
      class:active={drawerOpen}
      onclick={onToggleDrawer}
      title="Historique (Ctrl+B)"
      aria-label="Historique (Ctrl+B)"
      aria-expanded={drawerOpen}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <line x1="2.5" y1="4.5" x2="13.5" y2="4.5" />
        <line x1="2.5" y1="8" x2="13.5" y2="8" />
        <line x1="2.5" y1="11.5" x2="13.5" y2="11.5" />
      </svg>
    </button>

    <div class="brand-wrap">
      <BrandLogo variant="auto" {isLight} size={20} />
    </div>

    <button
      type="button"
      class="icon-btn"
      onclick={onNewChat}
      title="Nouvelle discussion (Ctrl+N)"
      aria-label="Nouvelle discussion (Ctrl+N)"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <line x1="8" y1="3" x2="8" y2="13" />
        <line x1="3" y1="8" x2="13" y2="8" />
      </svg>
    </button>

    {#if mode === 'code'}
      <button
        type="button"
        class="folder-pill"
        onclick={onPickFolder}
        title={workdir || 'Choisir un dossier de travail'}
      >
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path d="M2 4.5a1 1 0 0 1 1-1h3.2l1.3 1.5H13a1 1 0 0 1 1 1v5.5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4.5z" />
        </svg>
        <span class="folder-name">{folderName}</span>
        {#if gitBranch}
          <span class="sep">·</span>
          <svg class="git-icon" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="5" cy="4" r="1.5" />
            <circle cx="5" cy="12" r="1.5" />
            <circle cx="11" cy="6" r="1.5" />
            <path d="M5 5.5v5" />
            <path d="M11 7.5c0 2-2 2.5-6 2.5" />
          </svg>
          <span class="git-branch">{gitBranch}</span>
          {#if gitDirtyCount > 0}
            <span class="dirty-badge" title="{gitDirtyCount} fichier(s) modifié(s)">{gitDirtyCount}</span>
          {/if}
        {/if}
      </button>
    {/if}
  </div>

  <div class="header-right">
    <button
      type="button"
      class="status-pill"
      onclick={onOpenEco}
      title="Statut du routeur Éco et quota 24h"
    >
      {#if mcpConnected > 0}
        <span class="mcp-dot" title="{mcpConnected} serveur(s) MCP connecté(s)"></span>
      {/if}
      <span>{ecoLabel}</span>
      {#if savedTokens > 0}
        <span class="muted">· {savedTokens} tok écon.</span>
      {/if}
      {#if usagePct !== null}
        <span class="muted">· {Math.round(usagePct)}%</span>
      {/if}
    </button>

    <button
      type="button"
      class="icon-btn"
      class:active={showDiff && panelTab === 'files'}
      onclick={() => onToggleDiff('files')}
      title="Fichiers modifiés ({diffCount})"
      aria-label="Fichiers modifiés"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M9.5 2H4a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V5.5L9.5 2z" />
        <polyline points="9.5 2 9.5 5.5 13 5.5" />
        <line x1="6" y1="8.5" x2="10" y2="8.5" />
        <line x1="8" y1="6.5" x2="8" y2="10.5" />
      </svg>
      {#if diffCount > 0}
        <span class="mini-badge">{diffCount}</span>
      {/if}
    </button>

    <button
      type="button"
      class="icon-btn"
      class:active={showDiff && panelTab === 'term'}
      onclick={() => onToggleDiff('term')}
      title="Terminal"
      aria-label="Terminal"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="3.5 5 6.5 8 3.5 11" />
        <line x1="8" y1="11" x2="12.5" y2="11" />
      </svg>
      {#if termUnread > 0}
        <span class="mini-badge">{termUnread}</span>
      {/if}
    </button>

    <PermMode />

    <button
      type="button"
      class="kbd-btn"
      onclick={onOpenPalette}
      title="Palette de commandes (Ctrl+K)"
    >
      Ctrl+K
    </button>

    {#if mode === 'code'}
      <button
        type="button"
        class="icon-btn"
        class:active={inspectorOpen}
        onclick={onToggleInspector}
        title="Inspecteur PC"
        aria-label="Inspecteur PC"
        aria-expanded={inspectorOpen}
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2" y="2.5" width="12" height="8.5" rx="1.2" />
          <line x1="5.5" y1="13.5" x2="10.5" y2="13.5" />
          <line x1="8" y1="11" x2="8" y2="13.5" />
        </svg>
      </button>
    {/if}

    <button
      type="button"
      class="icon-btn"
      onclick={onOpenSettings}
      title="Réglages"
      aria-label="Réglages"
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="8" cy="8" r="2.2" />
        <path d="M8 1.8v1.4M8 12.8v1.4M1.8 8h1.4M12.8 8h1.4M3.6 3.6l1 1M11.4 11.4l1 1M12.4 3.6l-1 1M4.6 11.4l-1 1" />
      </svg>
    </button>
  </div>

  {#if busy}
    <div class="header-prog" aria-hidden="true"></div>
  {/if}
</header>

<style>
  .floating-header {
    position: relative;
    height: 48px;
    width: 100%;
    padding: 0 12px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--line);
    color: var(--text);
    font-size: 13px;
    box-sizing: border-box;
    user-select: none;
    flex-shrink: 0;
  }

  .header-left,
  .header-right {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .brand-wrap {
    display: flex;
    align-items: center;
    padding: 0 4px;
  }

  .icon-btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    transition: background-color 0.15s ease, color 0.15s ease;
  }

  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }

  .icon-btn.active {
    background: var(--raised);
    color: var(--text);
  }

  .mini-badge {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--accent);
    color: #fff;
    font-size: 9px;
    font-weight: 700;
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-variant-numeric: tabular-nums;
  }

  .folder-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    margin-left: 4px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text);
    font-size: 12px;
    cursor: pointer;
    max-width: 280px;
    transition: background-color 0.15s ease;
  }

  .folder-pill:hover {
    background: var(--hover);
  }

  .folder-name,
  .git-branch {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sep {
    color: var(--faint);
  }

  .git-icon {
    flex-shrink: 0;
    color: var(--dim);
  }

  .dirty-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--raised);
    color: var(--text);
    font-size: 10px;
    font-weight: 600;
  }

  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel);
    color: var(--text);
    font-family: inherit;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color 0.15s ease;
  }

  .status-pill:hover {
    background: var(--hover);
  }

  .status-pill .muted {
    color: var(--dim);
  }

  .mcp-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
    flex-shrink: 0;
  }

  .kbd-btn {
    display: inline-flex;
    align-items: center;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--dim);
    font-family: inherit;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 0.15s ease, color 0.15s ease;
  }

  .kbd-btn:hover {
    background: var(--hover);
    color: var(--text);
  }

  .header-prog {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--accent), transparent);
    background-size: 40% 100%;
    background-repeat: no-repeat;
    animation: header-prog-slide 1.15s ease-in-out infinite;
    pointer-events: none;
  }

  @keyframes header-prog-slide {
    0% {
      background-position: -40% 0;
    }
    100% {
      background-position: 140% 0;
    }
  }
</style>
