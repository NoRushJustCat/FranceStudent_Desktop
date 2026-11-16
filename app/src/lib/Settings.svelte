<script>
  import { fade, fly } from 'svelte/transition';
  import { saveSettings, openExternal } from './api.js';
  import Extensions from './Extensions.svelte';
  import { getUserInstructions, updateUserInstructions } from './instructions.js';
  import { THEMES, normalizeTheme } from './themes.js';

  let {
    settings = { baseUrl: '', hasKey: false },
    usage = null,
    budget = null,
    workdir = null,
    autoAccept = false,
    eco = false,
    smartRoute = 'eco',
    conciseMode = true,
    ecoShield = true,
    autoCompactPct = 65,
    last = null,
    session = { calls: 0, prompt: 0, output: 0, messages: 0 },
    ctx = null,
    chatId = null,
    busy = false,
    theme = 'sombre',
    notifSettings = {
      enabled: false,
      desktopComplete: true,
      toolConfirm: true,
      appUpdates: true,
      offersAndSubscriptions: false,
    },
    tab: initialTab = 'apparence',
    onclose = () => {},
    onsaved = () => {},
    oneco = () => {},
    onauto = () => {},
    onsmartroute = () => {},
    onconcise = () => {},
    onecoshield = () => {},
    onautocompact = () => {},
    onpick = () => {},
    oncompact = () => {},
    onreset = () => {},
    oninsert = () => {},
    onextchange = () => {},
    ontheme = () => {},
    onrefreshusage = () => {},
    onnotifchange = () => {},
    ontestnotif = () => {},
  } = $props();

  const resolveTab = (t) => {
    if (t === 'usage' || t === 'eco') return 'eco';
    if (t === 'code' || t === 'skills' || t === 'mcp' || t === 'pc') return 'pc';
    if (t === 'notifications' || t === 'diag' || t === 'compte') return 'compte';
    if (t === 'instructions') return 'instructions';
    return 'apparence';
  };

  let activeTab = $state(resolveTab(initialTab));
  let baseUrl = $state(settings?.baseUrl || '');
  let apiKey = $state('');
  let showKey = $state(false);
  let saving = $state(false);
  let saveState = $state('');
  let saveError = $state('');

  let instr = $state('');
  let instrSaved = $state('');
  let instrLoaded = $state(false);
  let instrLoading = $state(false);
  let instrSaving = $state(false);
  let instrMsg = $state('');
  let instrErr = $state('');
  let instrTried = false;
  const instrDirty = $derived(instrLoaded && instr !== instrSaved);

  const pickInstr = (d) =>
    String(d?.customInstructions ?? d?.instructions?.customInstructions ?? '');

  async function loadInstructions() {
    instrLoading = true;
    instrErr = '';
    try {
      const d = await getUserInstructions();
      instrSaved = pickInstr(d);
      instr = instrSaved;
      instrLoaded = true;
    } catch (err) {
      instrErr = String(err?.message || err);
    } finally {
      instrLoading = false;
    }
  }

  async function saveInstructions() {
    if (instrSaving || !instrDirty) return;
    instrSaving = true;
    instrMsg = '';
    instrErr = '';
    try {
      const d = await updateUserInstructions({ customInstructions: instr });
      const back = d?.customInstructions ?? d?.instructions?.customInstructions;
      instrSaved = back == null ? instr : String(back);
      instr = instrSaved;
      instrMsg = 'Instructions enregistrées.';
    } catch (err) {
      instrErr = String(err?.message || err);
    } finally {
      instrSaving = false;
    }
  }

  $effect(() => {
    if (initialTab) {
      activeTab = resolveTab(initialTab);
    }
  });

  $effect(() => {
    if (activeTab === 'instructions' && !instrTried) {
      instrTried = true;
      loadInstructions();
    }
  });

  $effect(() => {
    if (settings?.baseUrl !== undefined && baseUrl === '') {
      baseUrl = settings.baseUrl || '';
    }
  });

  const currentTheme = $derived(normalizeTheme(theme));

  const tabs = [
    { id: 'apparence', label: 'Apparence' },
    { id: 'eco', label: 'Économie & Quotas' },
    { id: 'pc', label: 'PC & Extensions' },
    { id: 'instructions', label: 'Instructions' },
    { id: 'compte', label: 'Compte' },
  ];

  const routeOptions = [
    { id: 'eco', title: 'Éco', desc: 'Modèles légers en priorité, coût minimal.' },
    { id: 'balanced', title: 'Équilibré', desc: 'Adapte le modèle à la complexité.' },
    { id: 'max', title: 'Max', desc: 'Performance maximale sans compromis.' },
  ];

  function fmtNum(n) {
    const v = Number(n || 0);
    if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
    if (v >= 1_000) return `${(v / 1_000).toFixed(1)}k`;
    return String(v);
  }

  const quotaPct = $derived.by(() => {
    if (budget && typeof budget.pct === 'number') return Math.max(0, Math.min(100, Math.round(budget.pct)));
    if (usage?.chatBudget && typeof usage.chatBudget.usedPercent === 'number') return Math.max(0, Math.min(100, Math.round(usage.chatBudget.usedPercent)));
    if (usage && typeof usage.percent === 'number') return Math.max(0, Math.min(100, Math.round(usage.percent)));
    if (budget && typeof budget.percent === 'number') return Math.max(0, Math.min(100, Math.round(budget.percent)));
    if (usage && usage.used && usage.limit) return Math.max(0, Math.min(100, Math.round((usage.used / usage.limit) * 100)));
    return 0;
  });

  const ctxPct = $derived.by(() => {
    if (ctx && typeof ctx.percent === 'number') return Math.max(0, Math.min(100, Math.round(ctx.percent)));
    if (ctx && ctx.used && ctx.max) return Math.max(0, Math.min(100, Math.round((ctx.used / ctx.max) * 100)));
    return 0;
  });

  async function handleSaveAccount(e) {
    e?.preventDefault?.();
    if (saving) return;
    saving = true;
    saveState = '';
    saveError = '';
    try {
      await saveSettings(baseUrl.trim(), apiKey.trim());
      apiKey = '';
      saveState = 'Enregistré';
      onsaved();
      setTimeout(() => {
        if (saveState === 'Enregistré') saveState = '';
      }, 2400);
    } catch (err) {
      saveError = err?.message || 'Erreur lors de la sauvegarde';
    } finally {
      saving = false;
    }
  }

  function toggleNotif(key) {
    onnotifchange({ ...notifSettings, [key]: !notifSettings[key] });
  }

  function handleKeydown(e) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="backdrop"
  transition:fade={{ duration: 150 }}
  onclick={(e) => {
    if (e.target === e.currentTarget) onclose();
  }}
  role="presentation"
>
  <div
    class="modal"
    transition:fly={{ y: 10, duration: 180 }}
    role="dialog"
    aria-modal="true"
    aria-label="Paramètres"
  >
    <header class="modal-head">
      <div class="head-title">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
        <h2>Paramètres</h2>
      </div>
      <button type="button" class="close-btn" onclick={onclose} aria-label="Fermer">×</button>
    </header>

    <nav class="tabs" aria-label="Sections des paramètres">
      {#each tabs as t}
        <button
          type="button"
          class="tab-btn"
          class:active={activeTab === t.id}
          onclick={() => (activeTab = t.id)}
        >
          {t.label}
        </button>
      {/each}
    </nav>

    <div class="modal-body">
      {#if activeTab === 'apparence'}
        <section class="section">
          <div class="section-head">
            <h3>Thème</h3>
            <span class="muted">{THEMES.length} thèmes disponibles</span>
          </div>
          <div class="themes-grid">
            {#each THEMES as t}
              {@const active = currentTheme === t.id}
              <button
                type="button"
                class="theme-card"
                class:active
                onclick={() => ontheme(t.id)}
              >
                <span class="swatch" style="background:{t.bg}; border-color:{t.line || 'var(--line2)'}">
                  <span class="swatch-bar" style="background:{t.primary}"></span>
                </span>
                <span class="theme-name">{t.label}</span>
                {#if active}
                  <svg class="theme-check" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                {/if}
              </button>
            {/each}
          </div>
        </section>

        <section class="section">
          <div class="card row-card">
            <div class="row-info">
              <div class="row-title">Mode concis par défaut</div>
              <div class="row-desc">Réponses directes et structurées (-40% tokens).</div>
            </div>
            <label class="switch">
              <input type="checkbox" checked={conciseMode} onchange={() => onconcise(!conciseMode)} />
              <span class="slider"></span>
            </label>
          </div>
        </section>
      {:else if activeTab === 'eco'}
        <section class="section">
          <div class="section-head">
            <h3>Routeur intelligent</h3>
          </div>
          <div class="route-grid">
            {#each routeOptions as opt}
              <button
                type="button"
                class="route-card"
                class:active={smartRoute === opt.id}
                onclick={() => onsmartroute(opt.id)}
              >
                <div class="route-title">{opt.title}</div>
                <div class="route-desc">{opt.desc}</div>
              </button>
            {/each}
          </div>
        </section>

        <section class="section stack">
          <div class="section-head">
            <h3>Protections</h3>
          </div>
          <div class="card row-card">
            <div class="row-info">
              <div class="row-title">Bouclier Quota 80%</div>
              <div class="row-desc">Bascule automatiquement vers un modèle éco à 80% du quota.</div>
            </div>
            <label class="switch">
              <input type="checkbox" checked={ecoShield} onchange={() => onecoshield(!ecoShield)} />
              <span class="slider"></span>
            </label>
          </div>

          <div class="card row-card">
            <div class="row-info">
              <div class="row-title">Mode Éco global</div>
              <div class="row-desc">Force l'utilisation des modèles les plus économes.</div>
            </div>
            <label class="switch">
              <input type="checkbox" checked={eco} onchange={() => oneco(!eco)} />
              <span class="slider"></span>
            </label>
          </div>
        </section>

        <section class="section">
          <div class="card">
            <div class="range-head">
              <div>
                <div class="row-title">Seuil d'auto-compaction</div>
                <div class="row-desc">Compresse l'historique dès que le contexte atteint ce seuil.</div>
              </div>
              <span class="badge">{autoCompactPct}%</span>
            </div>
            <input
              class="range"
              type="range"
              min="30"
              max="90"
              step="5"
              value={autoCompactPct}
              oninput={(e) => onautocompact(Number(e.currentTarget.value))}
            />
          </div>
        </section>

        <section class="section">
          <div class="card">
            <div class="quota-head">
              <div>
                <div class="row-title">Quota 24h & Session</div>
                <div class="row-desc">Consommation estimée sur la fenêtre glissante.</div>
              </div>
              <div class="quota-actions">
                <button type="button" class="btn-ghost" onclick={onrefreshusage} disabled={busy}>Rafraîchir</button>
                <button type="button" class="btn-ghost" onclick={oncompact} disabled={busy}>Compacter</button>
              </div>
            </div>

            <div class="progress-track">
              <div class="progress-fill" style="width:{quotaPct}%"></div>
            </div>

            <div class="stats-grid">
              <div class="stat-card">
                <span class="stat-label">Appels</span>
                <span class="stat-val">{session?.calls || 0}</span>
              </div>
              <div class="stat-card">
                <span class="stat-label">Entrée</span>
                <span class="stat-val">{fmtNum(session?.prompt)}</span>
              </div>
              <div class="stat-card">
                <span class="stat-label">Sortie</span>
                <span class="stat-val">{fmtNum(session?.output)}</span>
              </div>
              <div class="stat-card">
                <span class="stat-label">Contexte</span>
                <span class="stat-val">{ctxPct}%</span>
              </div>
            </div>
          </div>
        </section>
      {:else if activeTab === 'pc'}
        <section class="section stack">
          <div class="card row-card">
            <div class="row-info min-w-0">
              <div class="row-title">Dossier de travail</div>
              <div class="row-desc truncate" title={workdir || 'Aucun dossier sélectionné'}>
                {workdir || 'Aucun dossier sélectionné'}
              </div>
            </div>
            <button type="button" class="btn-secondary" onclick={onpick}>Choisir...</button>
          </div>

          <div class="card row-card">
            <div class="row-info">
              <div class="row-title">Accepter automatiquement les modifications de fichiers</div>
              <div class="row-desc">Applique les éditions proposées sans demander confirmation.</div>
            </div>
            <label class="switch">
              <input type="checkbox" checked={autoAccept} onchange={() => onauto(!autoAccept)} />
              <span class="slider"></span>
            </label>
          </div>
        </section>

        <section class="section">
          <div class="section-head">
            <h3>Skills & Serveurs MCP</h3>
          </div>
          <div class="card embed-card">
            <Extensions embedded={true} {workdir} {oninsert} onchange={onextchange} />
          </div>
        </section>
      {:else if activeTab === 'instructions'}
        <section class="section">
          <div class="card form-card">
            <div class="range-head">
              <div>
                <div class="row-title">Instructions personnalisées</div>
                <div class="row-desc">Consignes appliquées par l’IA à tes conversations.</div>
              </div>
              <span class="badge">{instr.length} car.</span>
            </div>
            <div class="field">
              <textarea
                id="instr-text"
                class="instr-area"
                placeholder="Ex. : réponds en français, de façon concise et structurée…"
                disabled={!instrLoaded}
                bind:value={instr}
              ></textarea>
            </div>
            {#if instrErr}
              <div class="status-err">{instrErr}</div>
            {:else if instrMsg && !instrDirty}
              <div class="status-ok">{instrMsg}</div>
            {/if}
            <div class="form-foot">
              <div class="quota-actions">
                <button type="button" class="btn-ghost" disabled={!instrDirty || instrSaving} onclick={() => (instr = instrSaved)}>Annuler</button>
                <button type="button" class="btn-ghost" disabled={!instr || instrSaving} onclick={() => (instr = '')}>Vider</button>
                {#if !instrLoaded && !instrLoading}
                  <button type="button" class="btn-ghost" onclick={loadInstructions}>Réessayer</button>
                {/if}
              </div>
              <button type="button" class="btn-primary" disabled={instrSaving || !instrDirty} onclick={saveInstructions}>
                {instrSaving ? 'Enregistrement...' : 'Enregistrer'}
              </button>
            </div>
          </div>
        </section>
      {:else if activeTab === 'compte'}
        <section class="section">
          <form class="card form-card" onsubmit={handleSaveAccount}>
            <div class="row-title">Connexion API France Student</div>
            <div class="row-desc" style="margin-bottom: 4px;">Serveur officiel France Student connecté automatiquement.</div>
            <div class="field">
              <label for="fs-api-key">Clé API</label>
              <div class="key-wrap">
                <input
                  id="fs-api-key"
                  type={showKey ? 'text' : 'password'}
                  bind:value={apiKey}
                  placeholder={settings?.hasKey ? '••••••' : 'sk-...'}
                  autocomplete="off"
                />
                <button type="button" class="eye-btn" onclick={() => (showKey = !showKey)} aria-label={showKey ? 'Masquer' : 'Afficher'}>
                  {showKey ? 'Masquer' : 'Voir'}
                </button>
              </div>
            </div>
            <div class="form-foot">
              <div class="form-status">
                {#if saveState}
                  <span class="status-ok">{saveState}</span>
                {:else if saveError}
                  <span class="status-err">{saveError}</span>
                {/if}
              </div>
              <button type="submit" class="btn-primary" disabled={saving}>
                {saving ? 'Enregistrement...' : 'Enregistrer'}
              </button>
            </div>
          </form>
        </section>

        <section class="section">
          <div class="card stack-inner">
            <div class="notif-head">
              <div class="row-title">Notifications</div>
              <button type="button" class="btn-ghost" onclick={ontestnotif}>Tester</button>
            </div>

            <div class="notif-row">
              <span>Activer les notifications</span>
              <label class="switch">
                <input type="checkbox" checked={notifSettings.enabled} onchange={() => toggleNotif('enabled')} />
                <span class="slider"></span>
              </label>
            </div>

            <div class="notif-row">
              <span>Fin de génération</span>
              <label class="switch">
                <input type="checkbox" checked={notifSettings.desktopComplete} onchange={() => toggleNotif('desktopComplete')} />
                <span class="slider"></span>
              </label>
            </div>

            <div class="notif-row">
              <span>Confirmation d'outil requise</span>
              <label class="switch">
                <input type="checkbox" checked={notifSettings.toolConfirm} onchange={() => toggleNotif('toolConfirm')} />
                <span class="slider"></span>
              </label>
            </div>

            <div class="notif-row">
              <span>Nouveautés & mises à jour</span>
              <label class="switch">
                <input type="checkbox" checked={notifSettings.appUpdates} onchange={() => toggleNotif('appUpdates')} />
                <span class="slider"></span>
              </label>
            </div>
          </div>
        </section>

        <section class="section">
          <div class="card row-card diag-card">
            <div class="row-info">
              <div class="row-title">Diagnostic</div>
              <div class="row-desc">Réinitialise l'état local de la session courante.</div>
            </div>
            <button type="button" class="btn-danger" onclick={onreset}>Réinitialiser la session</button>
          </div>
        </section>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(4px);
    padding: 16px;
  }

  .modal {
    width: min(760px, 92vw);
    height: min(620px, 86vh);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--panel);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 14px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.45);
  }

  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px 12px;
    flex-shrink: 0;
  }

  .head-title {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text);
  }

  .head-title h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--dim);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease;
  }

  .close-btn:hover {
    background: var(--hover);
    color: var(--text);
  }

  .tabs {
    display: flex;
    gap: 4px;
    margin: 0 24px 4px;
    padding: 4px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 10px;
    flex-shrink: 0;
  }

  .tab-btn {
    flex: 1;
    padding: 7px 10px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--dim);
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.14s ease, color 0.14s ease;
    white-space: nowrap;
  }

  .tab-btn:hover {
    color: var(--text);
  }

  .tab-btn.active {
    background: var(--raised);
    color: var(--text);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.16);
  }

  .modal-body {
    flex: 1;
    overflow-y: auto;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .section.stack {
    gap: 10px;
  }

  .section-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }

  .section-head h3 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .muted {
    font-size: 12px;
    color: var(--faint);
  }

  .card {
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 12px 14px;
  }

  .row-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .row-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .min-w-0 {
    min-width: 0;
  }

  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-title {
    font-size: 13.5px;
    font-weight: 500;
    color: var(--text);
  }

  .row-desc {
    font-size: 12px;
    color: var(--dim);
  }

  /* Thèmes */
  .themes-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 8px;
  }

  .theme-card {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 9px 10px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 9px;
    color: var(--text);
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.14s ease, background 0.14s ease;
  }

  .theme-card:hover {
    background: var(--hover);
    border-color: var(--line2);
  }

  .theme-card.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .swatch {
    width: 22px;
    height: 16px;
    border-radius: 5px;
    border: 1px solid var(--line2);
    display: flex;
    align-items: flex-end;
    padding: 2px;
    flex-shrink: 0;
  }

  .swatch-bar {
    width: 100%;
    height: 4px;
    border-radius: 2px;
  }

  .theme-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .theme-check {
    color: var(--accent);
    flex-shrink: 0;
  }

  /* Routeur */
  .route-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
  }

  .route-card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 10px;
    text-align: left;
    cursor: pointer;
    transition: border-color 0.14s ease, background 0.14s ease;
  }

  .route-card:hover {
    background: var(--hover);
    border-color: var(--line2);
  }

  .route-card.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .route-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }

  .route-desc {
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--dim);
  }

  /* Toggle Switch */
  .switch {
    position: relative;
    display: inline-block;
    width: 36px;
    height: 20px;
    flex-shrink: 0;
  }

  .switch input {
    opacity: 0;
    width: 0;
    height: 0;
    position: absolute;
  }

  .slider {
    position: absolute;
    inset: 0;
    background: var(--raised);
    border: 1px solid var(--line2);
    border-radius: 999px;
    cursor: pointer;
    transition: background 0.16s ease, border-color 0.16s ease;
  }

  .slider::before {
    content: '';
    position: absolute;
    width: 14px;
    height: 14px;
    left: 2px;
    top: 2px;
    border-radius: 50%;
    background: var(--dim);
    transition: transform 0.16s ease, background 0.16s ease;
  }

  .switch input:checked + .slider {
    background: var(--accent);
    border-color: var(--accent);
  }

  .switch input:checked + .slider::before {
    transform: translateX(16px);
    background: var(--bg);
  }

  /* Range slider */
  .range-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .badge {
    padding: 3px 8px;
    border-radius: 6px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .range {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 4px;
    border-radius: 999px;
    background: var(--line2);
    outline: none;
    cursor: pointer;
  }

  .range::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--accent);
    border: 2px solid var(--panel2);
    cursor: pointer;
  }

  .range::-moz-range-thumb {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--accent);
    border: 2px solid var(--panel2);
    cursor: pointer;
  }

  /* Quota & Stats */
  .quota-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .quota-actions {
    display: flex;
    gap: 6px;
  }

  .progress-track {
    width: 100%;
    height: 6px;
    background: var(--raised);
    border-radius: 999px;
    overflow: hidden;
    margin-bottom: 12px;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 0.25s ease;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }

  .stat-card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 9px 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 8px;
  }

  .stat-label {
    font-size: 11px;
    color: var(--faint);
  }

  .stat-val {
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  /* Embeds */
  .embed-card {
    padding: 12px;
    overflow: hidden;
  }

  /* Forms & Compte */
  .form-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .field label {
    font-size: 12px;
    color: var(--dim);
  }

  .field input {
    padding: 8px 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 8px;
    color: var(--text);
    font-size: 13px;
    outline: none;
    transition: border-color 0.14s ease;
  }

  .field input:focus {
    border-color: var(--accent);
  }

  .form-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 2px;
  }

  .form-status {
    font-size: 12px;
  }

  .status-ok {
    color: var(--ok);
  }

  .status-err {
    color: var(--err);
  }

  .stack-inner {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .notif-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 2px;
  }

  .notif-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 13px;
    color: var(--text);
  }

  /* Buttons */
  .btn-primary,
  .btn-secondary,
  .btn-ghost,
  .btn-danger {
    padding: 7px 12px;
    border-radius: 8px;
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.14s ease, border-color 0.14s ease, opacity 0.14s ease;
    white-space: nowrap;
  }

  .btn-primary {
    background: var(--accent);
    color: var(--bg);
    border: 1px solid var(--accent);
  }

  .btn-primary:hover:not(:disabled) {
    opacity: 0.92;
  }

  .btn-secondary {
    background: var(--raised);
    color: var(--text);
    border: 1px solid var(--line2);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--hover);
  }

  .btn-ghost {
    background: transparent;
    color: var(--dim);
    border: 1px solid var(--line);
  }

  .btn-ghost:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }

  .btn-danger {
    background: transparent;
    color: var(--err);
    border: 1px solid var(--line);
  }

  .btn-danger:hover:not(:disabled) {
    background: var(--hover);
    border-color: var(--err);
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .instr-area {
    width: 100%;
    min-height: 180px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--bg);
    color: var(--text);
    font-family: inherit;
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
    box-sizing: border-box;
  }
  .instr-area:focus {
    outline: none;
    border-color: var(--accent);
  }
  .key-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .key-wrap input {
    width: 100%;
    padding-right: 64px;
  }
  .eye-btn {
    position: absolute;
    right: 6px;
    padding: 3px 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--panel2);
    color: var(--dim);
    font-size: 11px;
    cursor: pointer;
  }
  .eye-btn:hover {
    color: var(--text);
    background: var(--hover);
  }
</style>
