<script>
  let {
    open = false,
    workdir = '',
    gitSummary = null,
    sysInfo = null,
    ecoShield = true,
    onToggleEcoShield = (v) => {},
    savedTokens = 0,
    savedRequests = 0,
    contextPct = 0,
    onPickFolder = () => {},
    onRunZeroToken = (cmd) => {},
    onCompact = () => {},
    onClose = () => {}
  } = $props();

  let folderName = $derived(
    workdir ? workdir.split(/[/\\]/).filter(Boolean).pop() || workdir : 'Aucun dossier'
  );

  let topFiles = $derived(
    gitSummary?.files ? gitSummary.files.slice(0, 6) : []
  );

  let totalChanges = $derived(
    (gitSummary?.modified_count || 0) +
    (gitSummary?.staged_count || 0) +
    (gitSummary?.untracked_count || 0)
  );

  let clampedContext = $derived(Math.min(100, Math.max(0, Math.round(contextPct || 0))));

  function statusBadgeClass(status) {
    if (!status) return 'badge-mod';
    const s = status.trim().toUpperCase();
    if (s.includes('A') || s === '??' || s === '?') return 'badge-add';
    if (s.includes('D')) return 'badge-del';
    return 'badge-mod';
  }

  function statusLabel(status) {
    if (!status) return 'M';
    const s = status.trim().toUpperCase();
    if (s === '??' || s === '?') return '?';
    if (s.includes('A')) return 'A';
    if (s.includes('D')) return 'D';
    return 'M';
  }

  function formatNumber(n) {
    return new Intl.NumberFormat('fr-FR').format(n || 0);
  }
</script>

{#if open}
  <aside class="inspector" aria-label="Inspecteur PC">
    <header class="inspector-header">
      <div class="header-title">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="2" y="3" width="20" height="14" rx="2" />
          <line x1="8" y1="21" x2="16" y2="21" />
          <line x1="12" y1="17" x2="12" y2="21" />
        </svg>
        <span>Inspecteur PC</span>
      </div>
      <button class="icon-btn" type="button" onclick={onClose} title="Fermer" aria-label="Fermer">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </header>

    <div class="inspector-body">
      <section class="section">
        <div class="section-head">
          <span class="section-title">Dossier &amp; Git (0 token)</span>
          <button
            class="icon-btn"
            type="button"
            onclick={() => onRunZeroToken('/git')}
            title="Rafraîchir Git"
            aria-label="Rafraîchir Git"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="23 4 23 10 17 10" />
              <polyline points="1 20 1 14 7 14" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
          </button>
        </div>

        <div class="folder-row">
          <div class="folder-info" title={workdir || 'Aucun dossier sélectionné'}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            </svg>
            <span class="folder-name">{folderName}</span>
          </div>
          <button class="btn-subtle" type="button" onclick={onPickFolder}>
            Changer
          </button>
        </div>

        {#if gitSummary?.is_git}
          <div class="git-meta">
            <span class="git-branch">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <line x1="6" y1="3" x2="6" y2="15" />
                <circle cx="18" cy="6" r="3" />
                <circle cx="6" cy="18" r="3" />
                <path d="M18 9a9 9 0 0 1-9 9" />
              </svg>
              <span>{gitSummary.branch || 'HEAD'}</span>
            </span>
            <span class="git-count">{totalChanges} mod.</span>
          </div>

          {#if topFiles.length > 0}
            <ul class="file-list">
              {#each topFiles as file}
                <li class="file-item" title={file.path}>
                  <span class="file-badge {statusBadgeClass(file.status)}">
                    {statusLabel(file.status)}
                  </span>
                  <span class="file-path">{file.path}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="empty-note">Arbre de travail propre.</p>
          {/if}
        {:else}
          <p class="empty-note">Dépôt Git non détecté.</p>
        {/if}
      </section>

      <section class="section">
        <div class="section-head">
          <span class="section-title">Actions Locales (0 token)</span>
        </div>
        <div class="actions-grid">
          <button class="action-btn" type="button" onclick={() => onRunZeroToken('/git')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="3" />
              <line x1="3" y1="12" x2="9" y2="12" />
              <line x1="15" y1="12" x2="21" y2="12" />
            </svg>
            <span>Statut Git</span>
          </button>
          <button class="action-btn" type="button" onclick={() => onRunZeroToken('/sys')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="4" y="4" width="16" height="16" rx="2" />
              <rect x="9" y="9" width="6" height="6" />
            </svg>
            <span>Infos PC</span>
          </button>
          <button class="action-btn" type="button" onclick={() => onRunZeroToken('/files')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" />
              <polyline points="13 2 13 9 20 9" />
            </svg>
            <span>Fichiers</span>
          </button>
          <button class="action-btn" type="button" onclick={() => onRunZeroToken('/clean')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="3 6 5 6 21 6" />
              <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
              <path d="M10 11v6" />
              <path d="M14 11v6" />
            </svg>
            <span>Caches</span>
          </button>
        </div>
        {#if sysInfo}
          <div class="sys-mini">
            <span>{sysInfo.os === 'windows' ? 'Windows' : sysInfo.os || 'PC'}</span>
            {#if sysInfo.memory_hint}
              <span>{sysInfo.memory_hint}</span>
            {/if}
          </div>
        {/if}
      </section>

      <section class="section">
        <div class="section-head">
          <span class="section-title">Économie France Student</span>
        </div>

        <div class="context-block">
          <div class="context-row">
            <span class="context-label">Contexte utilisé</span>
            <span class="context-val">{clampedContext}%</span>
          </div>
          <div class="progress-track">
            <div
              class="progress-fill"
              class:warn={clampedContext > 75}
              style="width: {clampedContext}%"
            ></div>
          </div>
          {#if clampedContext > 40}
            <button class="btn-compact" type="button" onclick={onCompact}>
              Compacter
            </button>
          {/if}
        </div>

        <label class="shield-row">
          <span class="shield-label">Bouclier Quota 80%</span>
          <input
            type="checkbox"
            checked={ecoShield}
            onchange={(e) => onToggleEcoShield(e.currentTarget.checked)}
          />
        </label>

        <div class="stats-list">
          <div class="stat-row">
            <span>Tokens économisés</span>
            <strong>{formatNumber(savedTokens)}</strong>
          </div>
          <div class="stat-row">
            <span>Requêtes évitées</span>
            <strong>{formatNumber(savedRequests)}</strong>
          </div>
        </div>
      </section>
    </div>
  </aside>
{/if}

<style>
  .inspector {
    width: 280px;
    min-width: 280px;
    height: 100%;
    background: var(--panel);
    border-left: 1px solid var(--line);
    color: var(--text);
    display: flex;
    flex-direction: column;
    font-size: 12px;
  }

  .inspector-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    border-bottom: 1px solid var(--line);
    flex-shrink: 0;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    font-size: 12.5px;
    color: var(--text);
  }

  .inspector-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--line);
  }

  .section:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--dim);
  }

  .icon-btn {
    background: transparent;
    border: 1px solid transparent;
    color: var(--dim);
    border-radius: 6px;
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }

  .icon-btn:hover {
    background: var(--panel2);
    color: var(--text);
    border-color: var(--line);
  }

  .folder-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 8px;
  }

  .folder-info {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--text);
  }

  .folder-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }

  .btn-subtle {
    background: transparent;
    border: 1px solid var(--line);
    color: var(--dim);
    border-radius: 4px;
    padding: 2px 7px;
    font-size: 11px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .btn-subtle:hover {
    color: var(--text);
    border-color: var(--accent);
  }

  .git-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--dim);
    font-size: 11.5px;
  }

  .git-branch {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--text);
    font-weight: 500;
  }

  .git-count {
    color: var(--faint);
  }

  .file-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    min-width: 0;
  }

  .file-badge {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 600;
    flex-shrink: 0;
    background: var(--panel2);
    border: 1px solid var(--line);
  }

  .badge-add {
    color: var(--diff-add-fg);
  }

  .badge-del {
    color: var(--diff-del-fg);
  }

  .badge-mod {
    color: var(--warn);
  }

  .file-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--dim);
  }

  .empty-note {
    margin: 0;
    font-size: 11.5px;
    color: var(--faint);
  }

  .actions-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--panel2);
    border: 1px solid var(--line);
    color: var(--text);
    border-radius: 6px;
    padding: 7px 8px;
    font-size: 11.5px;
    cursor: pointer;
    text-align: left;
  }

  .action-btn:hover {
    border-color: var(--accent);
  }

  .sys-mini {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--faint);
    padding-top: 2px;
  }

  .context-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .context-row {
    display: flex;
    justify-content: space-between;
    font-size: 11.5px;
    color: var(--dim);
  }

  .context-val {
    color: var(--text);
    font-weight: 600;
  }

  .progress-track {
    width: 100%;
    height: 6px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 999px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--ok);
    transition: width 0.2s ease;
  }

  .progress-fill.warn {
    background: var(--warn);
  }

  .btn-compact {
    align-self: flex-end;
    background: var(--panel2);
    border: 1px solid var(--line);
    color: var(--text);
    border-radius: 5px;
    padding: 4px 10px;
    font-size: 11px;
    cursor: pointer;
  }

  .btn-compact:hover {
    border-color: var(--accent);
  }

  .shield-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 8px;
    cursor: pointer;
  }

  .shield-label {
    font-size: 11.5px;
    color: var(--text);
  }

  .shield-row input[type='checkbox'] {
    accent-color: var(--accent);
    cursor: pointer;
  }

  .stats-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    color: var(--dim);
  }

  .stat-row {
    display: flex;
    justify-content: space-between;
  }

  .stat-row strong {
    color: var(--text);
    font-weight: 600;
  }
</style>
