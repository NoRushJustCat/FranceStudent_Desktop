<script>
  import DevConsole from './DevConsole.svelte';
  import { dev, setEnabled } from './devlog.svelte.js';
  import { untrack } from 'svelte';
  import { saveSettings, storeApiToken, logout, openExternal, getProfile, getTokenUsage } from './api.js';
  import { getUserInstructions, updateUserInstructions } from './instructions.js';
  import Extensions from './Extensions.svelte';
  import Plugins from './Plugins.svelte';
  import { instructionsStatus, instructionsSync, instructionsRemove, instructionsSetEnabled } from './api.js';
  import { THEMES, normalizeTheme } from './themes.js';

  let {
    settings = { baseUrl: '', hasKey: false },
    usage = null,
    budget = null,
    workdir = null,
    autoAccept = false,
    eco = false,
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
      offersAndSubscriptions: false
    },
    tab: initialTab = 'apparence',
    onclose,
    onsaved,
    onlogout,
    oneco,
    onauto,
    onpick,
    oncompact,
    onreset,
    oninsert,
    onextchange,
    ontheme,
    onrefreshusage,
    onnotifchange,
    ontestnotif
  } = $props();

  const resolveTab = (t) => (t === 'eco' ? 'usage' : t || 'apparence');

  let tab = $state(untrack(() => resolveTab(initialTab)));
  let baseUrl = $state(untrack(() => settings?.baseUrl || ''));
  let apiKey = $state('');
  let showKey = $state(false);
  let saving = $state(false);
  let saveMsg = $state('');
  let saveErr = $state('');
  let refreshing = $state(false);
  let copiedDiag = $state(false);

  $effect(() => {
    if (initialTab) {
      tab = resolveTab(initialTab);
    }
  });

  $effect(() => {
    baseUrl = settings?.baseUrl || '';
  });

  let profile = $state(null);
  let profileErr = $state('');
  $effect(() => {
    if (tab !== 'compte' || !settings?.hasKey || profile) return;
    getProfile()
      .then((p) => (profile = p))
      .catch((e) => (profileErr = String(e?.message || e)));
  });

  const activeTheme = $derived(normalizeTheme(theme));

  // Activité de tokens (grille façon contributions : 1 colonne = 1 semaine, 7 lignes lun→dim)
  const ACT_DAYS = 343;
  let tokAct = $state(null);
  let tokActErr = $state('');
  let tokActBusy = false;
  $effect(() => {
    if (tab !== 'usage' || !settings?.hasKey || tokAct || tokActBusy) return;
    tokActBusy = true;
    getTokenUsage(ACT_DAYS)
      .then((r) => (tokAct = r ?? {}))
      .catch((e) => (tokActErr = String(e?.message || e)))
      .finally(() => (tokActBusy = false));
  });

  let actWrap = $state(null);
  let actTip = $state(null);
  function showTip(e, c) {
    const r = e.currentTarget.getBoundingClientRect();
    const w = actWrap.getBoundingClientRect();
    const x = r.left - w.left + r.width / 2;
    const side = x < 90 ? 'l' : x > w.width - 90 ? 'r' : '';
    const day = c.date.toLocaleDateString('fr-FR', { day: 'numeric', month: 'short' });
    actTip = { x, y: r.top - w.top - 8, side, text: `${fmtNum(c.v)} tokens le ${day}` };
  }
  const ymd = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const actGrid = $derived.by(() => {
    const raw = tokAct?.last7DaysTokenUsage ?? tokAct?.dailyTokenUsage ?? (Array.isArray(tokAct) ? tokAct : []);
    const byDay = new Map();
    for (const d of raw) {
      const k = String(d?.date ?? '').slice(0, 10);
      byDay.set(k, (byDay.get(k) ?? 0) + Number(d?.totalTokens ?? (d?.inputTokens ?? 0) + (d?.outputTokens ?? 0)));
    }
    const end = new Date();
    end.setHours(12, 0, 0, 0);
    const start = new Date(end);
    start.setDate(end.getDate() - (ACT_DAYS - 1));
    const pad = (start.getDay() + 6) % 7;
    const cells = Array.from({ length: pad }, () => null);
    let total = 0;
    let max = 0;
    for (let i = 0; i < ACT_DAYS; i++) {
      const d = new Date(start);
      d.setDate(start.getDate() + i);
      const v = byDay.get(ymd(d)) ?? 0;
      total += v;
      max = Math.max(max, v);
      cells.push({ date: d, v });
    }
    for (const c of cells) if (c) c.lvl = !c.v ? 0 : Math.min(4, 1 + Math.floor((c.v / max) * 3.999));
    const cols = Math.ceil(cells.length / 7);
    const months = [];
    let prev = -1;
    for (let c = 0; c < cols; c++) {
      const first = cells.slice(c * 7, c * 7 + 7).find(Boolean);
      if (first && first.date.getMonth() !== prev) {
        prev = first.date.getMonth();
        months.push({ col: c + 1, name: first.date.toLocaleDateString('fr-FR', { month: 'short' }) });
      }
    }
    return { cells, cols, months, total: total || Number(tokAct?.totalTokenCount ?? 0) };
  });

  const budgetPct = $derived(
    Math.min(100, Math.max(0, Math.round(budget?.pct ?? usage?.chatBudget?.usedPercent ?? 0)))
  );

  const audioLimit = $derived.by(() => {
    const list = usage?.rateLimits;
    if (!Array.isArray(list)) return null;
    return (
      list.find((r) => {
        const k = String(r?.name || r?.id || r?.key || '').toLowerCase();
        return k.includes('audio') || k.includes('transcri');
      }) || null
    );
  });

  const audioPct = $derived(
    Math.min(100, Math.max(0, Math.round(audioLimit?.usedPercent ?? audioLimit?.pct ?? 0)))
  );

  const limitDesc = (lim) => {
    if (lim?.desc || lim?.subtitle) return lim.desc || lim.subtitle;
    const id = String(lim?.id || lim?.key || '').toLowerCase();
    if (id.includes('daily') || id.includes('req')) return 'Fenêtre glissante de 24 h · libération progressive.';
    if (id.includes('tool') || id.includes('mcp')) return 'Exécutions d’outils MCP sur 24 heures glissantes.';
    return 'Limite de fréquence de ton abonnement.';
  };

  const sortedExtraLimits = $derived.by(() => {
    const list = usage?.rateLimits;
    if (!Array.isArray(list)) return [];
    return list
      .filter((r) => r !== audioLimit)
      .map((r) => ({ item: r, pct: Math.min(100, Math.max(0, Math.round(r?.usedPercent ?? r?.pct ?? 0))) }))
      .sort((a, b) => b.pct - a.pct);
  });

  const criticalLimits = $derived(sortedExtraLimits.filter((x) => x.pct >= 80).map((x) => x.item));
  const otherLimits = $derived(sortedExtraLimits.filter((x) => x.pct < 80).map((x) => x.item));

  const ctxPct = $derived.by(() => {
    if (!ctx) return 0;
    if (typeof ctx.pct === 'number') return Math.min(100, Math.max(0, Math.round(ctx.pct)));
    if (ctx.max && ctx.used) return Math.min(100, Math.max(0, Math.round((ctx.used / ctx.max) * 100)));
    return 0;
  });

  const extraLimitCount = $derived(otherLimits.length);
  let lastUsageRefresh = $state('à l’instant');
  const fmtNum = (n) => new Intl.NumberFormat('fr-FR').format(Math.round(Number(n) || 0));
  const safePath = (p) =>
    p
      ? String(p)
          .replace(/[A-Za-z]:\\Users\\[^\\]+/gi, '%USERPROFILE%')
          .replace(/\/(?:home|Users)\/[^/]+/g, '$HOME')
      : p;
  const gaugeTone = (pct) => (pct >= 95 ? 'crit' : pct >= 80 ? 'warn' : '');
  const accountDirty = $derived(
    baseUrl.trim() !== (settings?.baseUrl || '').trim() || apiKey.trim().length > 0
  );

  const diagJson = $derived(
    JSON.stringify(
      {
        appVersion: '0.1.0',
        tab,
        theme: activeTheme,
        baseUrl: settings?.baseUrl || '',
        hasKey: !!settings?.hasKey,
        workdir: safePath(workdir),
        chatId,
        busy,
        eco,
        autoAccept,
        usage,
        ctx,
        session,
        notifSettings
      },
      null,
      2
    )
  );

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      onclose?.();
    }
  }

  async function handleSaveAccount(e) {
    e.preventDefault();
    saving = true;
    saveMsg = '';
    saveErr = '';
    try {
      if (baseUrl.trim() !== (settings?.baseUrl || '').trim()) {
        await saveSettings({ baseUrl: baseUrl.trim() });
      }
      if (apiKey.trim()) {
        await storeApiToken(apiKey.trim());
        apiKey = '';
      }
      saveMsg = 'Paramètres enregistrés.';
      onsaved?.();
    } catch (err) {
      saveErr = err?.message || "Erreur lors de l'enregistrement.";
    } finally {
      saving = false;
    }
  }

  async function handleRefreshUsage() {
    if (refreshing) return;
    refreshing = true;
    try {
      await onrefreshusage?.();
      lastUsageRefresh = new Date().toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
    } finally {
      setTimeout(() => {
        refreshing = false;
      }, 400);
    }
  }

  function updateNotif(patch) {
    const next = { ...notifSettings, ...patch };
    onnotifchange?.(next);
  }

  async function copyDiag() {
    try {
      await navigator.clipboard.writeText(diagJson);
      copiedDiag = true;
      setTimeout(() => {
        copiedDiag = false;
      }, 1800);
    } catch {
      copiedDiag = false;
    }
  }

  let sync = $state(null);
  let syncBusy = $state(false);
  let syncErr = $state('');
  async function syncCall(fn) {
    syncBusy = true;
    syncErr = '';
    try {
      sync = await fn();
    } catch (e) {
      syncErr = String(e);
    } finally {
      syncBusy = false;
    }
  }
  $effect(() => {
    if (tab === 'instructions' && !sync) syncCall(instructionsStatus);
  });

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
    if (tab === 'instructions' && !instrTried) {
      instrTried = true;
      loadInstructions();
    }
  });

  const baseNav = [
    { id: 'apparence', label: 'Apparence' },
    { id: 'notifications', label: 'Notifications' },
    { id: 'usage', label: 'Usage' },
    { id: 'instructions', label: 'Instructions' },
    { id: 'compte', label: 'Compte' },
    { id: 'code', label: 'Mode Code' },
    { id: 'skills', label: 'Skills' },
    { id: 'mcp', label: 'MCP' },
    { id: 'plugins', label: 'Plugins' },
    { id: 'diag', label: 'Diagnostic' }
  ];
  const navItems = $derived(dev.enabled ? [...baseNav, { id: 'console', label: 'Console' }] : baseNav);

</script>

<svelte:window onkeydown={handleKeydown} />

<div class="settings-overlay" role="dialog" aria-modal="true" aria-label="Paramètres">
  <button type="button" class="overlay-backdrop" aria-label="Fermer les paramètres" onclick={() => onclose?.()}></button>
  <div class="modal">
    <aside class="sidebar">
      <div class="sidebar-head">
        <span class="sidebar-title">Paramètres</span>
      </div>

      <nav class="nav" aria-label="Navigation paramètres">
        {#each navItems as item}
          <button
            type="button"
            class="nav-item"
            class:active={tab === item.id}
            onclick={() => (tab = item.id)}
          >
            <span class="nav-ico" aria-hidden="true">
              {#if item.id === 'apparence'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z"/></svg>
              {:else if item.id === 'notifications'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/></svg>
              {:else if item.id === 'usage'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v18h18"/><path d="m19 9-5 5-4-4-3 3"/></svg>
              {:else if item.id === 'compte'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
              {:else if item.id === 'code'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>
              {:else if item.id === 'skills'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
              {:else if item.id === 'mcp'}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></svg>
       {:else if item.id === 'plugins'}
  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg>
              {:else}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>
              {/if}
            </span>
            <span>{item.label}</span>
          </button>
        {/each}
      </nav>

      <div class="sidebar-foot">
        <button type="button" class="close-btn" onclick={() => onclose?.()}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          <span>Fermer</span>
          <kbd>Échap</kbd>
        </button>
      </div>
    </aside>

    <main class="content">
      {#if tab === 'apparence'}
        <header class="sec-head sec-head-row">
          <div>
            <h2>Apparence</h2>
            <p class="sec-sub">Ajuste les couleurs et le thème de l’interface.</p>
          </div>
          <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </header>

        <div class="theme-grid">
          {#each THEMES as t}
            {@const isSelected = activeTheme === t.id}
            <button
              type="button"
              class="theme-card"
              class:selected={isSelected}
              aria-pressed={isSelected}
              onclick={() => ontheme?.(t.id)}
            >
              <div
                class="theme-preview"
                style:background={t.id === 'gradient' || t.id === 'gradien' || t.id === 'fs-gradien' ? 'linear-gradient(135deg, #1e1b4b 0%, #4f46e5 52%, #ec4899 100%)' : t.bg}
                style:border-color={t.cardBorder}
              >
                <div class="tp-side" style:background={t.side}>
                  <span class="tp-dot" style:background={t.primary}></span>
                  <span class="tp-bar" style:background={t.light ? 'rgba(0,0,0,0.16)' : 'rgba(255,255,255,0.22)'}></span>
                  <span class="tp-bar short" style:background={t.light ? 'rgba(0,0,0,0.12)' : 'rgba(255,255,255,0.16)'}></span>
                </div>
                <div class="tp-main">
                  <div class="tp-top" style:border-color={t.light ? 'rgba(0,0,0,0.08)' : 'rgba(255,255,255,0.08)'}></div>
                  <div class="tp-cards">
                    <div class="tp-card" style:background={t.card} style:border-color={t.cardBorder}></div>
                    <div class="tp-card" style:background={t.card} style:border-color={t.cardBorder}></div>
                    <div class="tp-card" style:background={t.card} style:border-color={t.cardBorder}></div>
                  </div>
                  <div class="tp-composer" style:background={t.card} style:border-color={t.cardBorder}>
                    <span class="tp-pill" style:background={t.primary}></span>
                  </div>
                </div>
              </div>
              {#if isSelected}
                <span class="theme-check" aria-hidden="true">
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                </span>
              {/if}
              <span class="theme-label">{t.label || t.id}</span>
            </button>
          {/each}
        </div>

      {:else if tab === 'notifications'}
        <header class="sec-head sec-head-row">
          <div>
            <h2>Notifications</h2>
            <p class="sec-sub">Choisis les alertes à recevoir sur cet appareil.</p>
          </div>
          <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </header>

        <div class="card notif-hero">
          <div class="notif-hero-left">
            <div class="notif-icon-box" aria-hidden="true">
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 0 1-3.46 0"/></svg>
            </div>
            <div>
              <div class="card-title">Notifications sur cet appareil</div>
              <div class="card-desc">
                {notifSettings?.enabled
                  ? 'Choisis les alertes à recevoir ci-dessous.'
                  : 'Autorise les notifications pour choisir les catégories à activer.'}
              </div>
            </div>
          </div>
          <div class="notif-hero-actions">
            {#if notifSettings?.enabled}
              <span class="status-active"><i class="status-dot" aria-hidden="true"></i>Notifications actives</span>
              <button
                type="button"
                class="btn-ghost"
                onclick={() => ontestnotif?.()}
              >
                Tester
              </button>
              <button
                type="button"
                class="btn-ghost"
                onclick={() => updateNotif({ enabled: false })}
              >
                Désactiver
              </button>
            {:else}
              <button
                type="button"
                class="btn-primary"
                onclick={() => updateNotif({ enabled: true })}
              >
                Activer les notifications
              </button>
            {/if}
          </div>
        </div>

        <div class="card notif-list" class:locked={!notifSettings?.enabled}>
          <label class="toggle-row">
            <div>
              <div class="row-title">Réponse terminée en arrière-plan</div>
              <div class="row-sub">Notifier lorsqu’une génération se termine hors focus.</div>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={!!notifSettings?.desktopComplete}
              disabled={!notifSettings?.enabled}
              onchange={(e) => updateNotif({ desktopComplete: e.currentTarget.checked })}
            />
          </label>

          <label class="toggle-row">
            <div>
              <div class="row-title">Autorisation demandée par un outil</div>
              <div class="row-sub">Notifier lorsqu’un outil (Code ou MCP) attend ton accord.</div>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={!!notifSettings?.toolConfirm}
              disabled={!notifSettings?.enabled}
              onchange={(e) => updateNotif({ toolConfirm: e.currentTarget.checked })}
            />
          </label>

          <label class="toggle-row">
            <div>
              <div class="row-title">Mises à jour de l’application</div>
              <div class="row-sub">Informer des nouvelles versions disponibles.</div>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={!!notifSettings?.appUpdates}
              disabled={!notifSettings?.enabled}
              onchange={(e) => updateNotif({ appUpdates: e.currentTarget.checked })}
            />
          </label>

          <label class="toggle-row">
            <div>
              <div class="row-title">Offres et abonnements</div>
              <div class="row-sub">Recevoir les informations liées à ton abonnement.</div>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={!!notifSettings?.offersAndSubscriptions}
              disabled={!notifSettings?.enabled}
              onchange={(e) => updateNotif({ offersAndSubscriptions: e.currentTarget.checked })}
            />
          </label>
        </div>

      {:else if tab === 'usage'}
        <header class="sec-head sec-head-row">
          <div>
            <h2>Usage</h2>
            <p class="sec-sub">Suivi des quotas, de la fenêtre de contexte et du mode économie.</p>
          </div>
          <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </header>

        <div class="info-banner">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
          <div>
            <strong>Tu as accès aux nouvelles limites.</strong>
            Ton usage du chat suit désormais un plafond quotidien.
            <button
              type="button"
              class="link-inline"
              onclick={() => openExternal('https://ia.francestudent.org')}
            >
              Partage tes retours avec nous.
            </button>
          </div>
        </div>

        <div class="usage-header">
          <div>
            <div class="card-title">Tes limites d’utilisation</div>
            <div class="card-desc">Suis la consommation de ton compte sur chaque quota disponible.</div>
          </div>
          <div class="usage-refresh">
            <span class="refresh-time">Actualisé {lastUsageRefresh}</span>
            <button
              type="button"
              class="icon-btn"
              title="Rafraîchir"
              aria-label="Rafraîchir l’usage"
              onclick={handleRefreshUsage}
            >
              <svg class:spin={refreshing} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
            </button>
          </div>
        </div>

        <div class="card usage-card">
          {#each criticalLimits as lim}
            {@const pct = Math.min(100, Math.max(0, Math.round(lim?.usedPercent ?? lim?.pct ?? 0)))}
            <div class="quota-row">
              <div class="quota-meta">
                <div class="row-title">{lim?.name || lim?.id || 'Quota'}</div>
                <div class="row-sub">{limitDesc(lim)}</div>
              </div>
              <div class="quota-gauge">
                <div class="gauge-track">
                  <div class="gauge-fill {gaugeTone(pct)}" style:width="{pct}%"></div>
                </div>
                <span class="quota-pct" class:warn={pct >= 80 && pct < 95} class:crit={pct >= 95}>{pct}&#8239;% utilisé</span>
              </div>
            </div>
          {/each}

          <div class="quota-row">
            <div class="quota-meta">
              <div class="row-title">Budget du chat</div>
              <div class="row-sub">Réinitialisé chaque jour à 0&#8239;h&#8239;00.</div>
            </div>
            <div class="quota-gauge">
              <div class="gauge-track">
                <div class="gauge-fill {gaugeTone(budgetPct)}" style:width="{budgetPct}%"></div>
              </div>
              <span class="quota-pct" class:warn={budgetPct >= 80 && budgetPct < 95} class:crit={budgetPct >= 95}>{budgetPct}&#8239;% utilisé</span>
            </div>
          </div>

          <div class="quota-row">
            <div class="quota-meta">
              <div class="row-title">Transcription audio en texte</div>
              <div class="row-sub">
                {#if audioLimit?.used != null && audioLimit?.max != null}
                  {audioLimit.used} min sur {audioLimit.max} min ce mois-ci.
                {:else if audioLimit}
                  Quota mensuel d’écoute et de transcription.
                {:else}
                  Tu n’as pas encore utilisé la transcription audio.
                {/if}
              </div>
            </div>
            <div class="quota-gauge">
              <div class="gauge-track">
                <div class="gauge-fill {gaugeTone(audioPct)}" style:width="{audioPct}%"></div>
              </div>
              <span class="quota-pct">{audioPct}&#8239;% utilisé</span>
            </div>
          </div>
        </div>

 <h3 class="usage-kicker">Activité tokens</h3>
        <div class="card usage-card act">
   {#if tokActErr}
     <div class="row-sub">{tokActErr}</div>
          {:else if !tokAct}
     <div class="row-sub">Chargement…</div>
          {:else}
     <div class="act-title">{fmtNum(actGrid.total)} tokens sur les {ACT_DAYS} derniers jours</div>
   <div class="act-wrap" role="presentation" bind:this={actWrap} onmouseleave={() => (actTip = null)}>
            <div class="act-months" style:--cols={actGrid.cols}>
       {#each actGrid.months as m}
  <span style:grid-column="{m.col} / span 4">{m.name}</span>
       {/each}
     </div>
     <div class="act-grid" style:--cols={actGrid.cols}>
       {#each actGrid.cells as c}
  {#if c}
           <span class="act-cell l{c.lvl}" onmouseenter={(e) => showTip(e, c)} role="presentation"></span>
         {:else}
    <span class="act-cell pad"></span>
  {/if}
       {/each}
     </div>
     {#if actTip}
<div class="act-tip {actTip.side}" style:left="{actTip.x}px" style:top="{actTip.y}px">{actTip.text}</div>
            {/if}
   </div>
   {/if}
        </div>

 <h3 class="usage-kicker">Conversation en cours</h3>
        <div class="card usage-card">
          <div class="quota-row">
            <div class="quota-meta">
              <div class="row-title">Contexte du chat</div>
              <div class="row-sub">
                {#if ctx?.used != null && ctx?.max != null}
                  {fmtNum(ctx.used)} / {fmtNum(ctx.max)} tokens
                {:else}
                  Occupation de la fenêtre de contexte actuelle.
                {/if}
              </div>
            </div>
            <div class="quota-gauge">
              <div class="gauge-track">
                <div class="gauge-fill {gaugeTone(ctxPct)}" style:width="{ctxPct}%"></div>
              </div>
              <span class="quota-pct">{ctxPct}&#8239;% utilisé</span>
            </div>
          </div>
          <div class="card-foot">
            <button
              type="button"
              class="btn-ghost sm"
              disabled={!chatId || busy}
              onclick={() => oncompact?.()}
            >
              Compacter le contexte
            </button>
          </div>
        </div>

        <div class="card usage-card">
          <label class="toggle-row compact">
            <div>
              <div class="row-title">Mode économie</div>
              <div class="row-sub">Réduit la consommation de tokens sur les tâches courantes.</div>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={!!eco}
              onchange={(e) => oneco?.(e.currentTarget.checked)}
            />
          </label>

          <div class="session-stats">
            <div class="stat-box">
              <span class="stat-lbl">Messages</span>
              <span class="stat-val">{fmtNum(session?.messages ?? 0)}</span>
            </div>
            <div class="stat-box">
              <span class="stat-lbl">Appels API</span>
              <span class="stat-val">{fmtNum(session?.calls ?? 0)}</span>
            </div>
            <div class="stat-box">
              <span class="stat-lbl">Tokens envoyés</span>
              <span class="stat-val">{fmtNum(session?.prompt ?? 0)}</span>
            </div>
            <div class="stat-box">
              <span class="stat-lbl">Tokens reçus</span>
              <span class="stat-val">{fmtNum(session?.output ?? 0)}</span>
            </div>
          </div>

          <div class="card-foot">
            <button type="button" class="btn-ghost sm" onclick={() => onreset?.()}>
              Remettre à zéro
            </button>
          </div>
        </div>

      {:else if tab === 'instructions'}
        <header class="sec-head sec-head-row">
          <div>
            <h2>Instructions personnalisées</h2>
            <p class="sec-sub">Consignes appliquées par l’IA à tes conversations.</p>
          </div>
          <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </header>

        <div class="card form-card">
          <div class="field">
            <div class="field-head">
              <label for="instr-text">Comment l’IA doit te répondre</label>
              <span class="instr-count">{instr.length} caractères</span>
            </div>
            <textarea
              id="instr-text"
              class="input input-area"
              placeholder="Ex. : réponds en français, de façon concise et structurée…"
              disabled={!instrLoaded}
              bind:value={instr}
            ></textarea>
          </div>

          {#if instrErr}
            <div class="msg-err">{instrErr}</div>
          {/if}
          {#if instrMsg && !instrDirty}
            <div class="msg-ok">{instrMsg}</div>
          {/if}

          <div class="form-actions">
            <button type="button" class="btn-primary" disabled={instrSaving || !instrDirty} onclick={saveInstructions}>
              {instrSaving ? 'Enregistrement…' : 'Enregistrer'}
            </button>
            <button type="button" class="btn-ghost" disabled={!instrDirty || instrSaving} onclick={() => (instr = instrSaved)}>
              Annuler
            </button>
            <button type="button" class="btn-ghost" disabled={!instr || instrSaving} onclick={() => (instr = '')}>
              Vider
            </button>
            {#if !instrLoaded && !instrLoading}
              <button type="button" class="btn-ghost" onclick={loadInstructions}>Réessayer</button>
            {/if}
          </div>
        </div>

        <div class="card form-card">
   <div class="field">
     <div class="field-head">
       <label for="sync-on">Informer l’IA des outils locaux (serveurs MCP du pont)</label>
     </div>
     <label>
       <input id="sync-on" type="checkbox" checked={sync?.enabled ?? true} disabled={syncBusy || !sync}
         onchange={(e) => syncCall(() => instructionsSetEnabled(e.currentTarget.checked))} />
       Ajouter un bloc automatique à tes instructions tant que le pont est actif
     </label>
     <p class="sec-sub">Le bloc est retiré à la déconnexion et à la fermeture de l’app. Il ne contient aucun jeton ni chemin.</p>
     {#if sync?.block}
<textarea class="input input-area" readonly rows="6" value={sync.block}></textarea>
     {/if}
     <p class="sec-sub">
       {sync?.active ? 'Bloc publié sur ton compte.' : 'Aucun bloc publié.'}
     </p>
          </div>
   {#if sync?.cleanupPending}
            <div class="msg-err">Le retrait du bloc a échoué à la dernière fermeture. Il sera retenté au prochain démarrage, ou retire-le maintenant.</div>
   {/if}
          {#if sync?.lastError}
     <div class="msg-err">{sync.lastError}</div>
          {/if}
   {#if syncErr}
     <div class="msg-err">{syncErr}</div>
   {/if}
          <div class="form-actions">
     <button type="button" class="btn-ghost" disabled={syncBusy} onclick={() => syncCall(instructionsSync)}>Resynchroniser</button>
     <button type="button" class="btn-ghost" disabled={syncBusy || !(sync?.active || sync?.cleanupPending)} onclick={() => syncCall(instructionsRemove)}>Retirer maintenant</button>
          </div>
        </div>

      {:else if tab === 'compte'}
        <header class="sec-head sec-head-row">
          <div>
     <h2>Compte</h2>
     <p class="sec-sub">Gère ta session France Student.</p>
          </div>
          <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </header>

 <div class="card form-card">
   {#if profile}
     <div class="acct">
       <span class="acct-av" aria-hidden="true">{(profile.name || profile.email || '?').trim().charAt(0).toUpperCase()}</span>
       <div class="acct-id">
  <strong>{profile.name || '—'}</strong>
  {#if profile.email}<span>{profile.email}</span>{/if}
       </div>
     </div>
   {:else if profileErr}
     <div class="msg-err">{profileErr}</div>
   {/if}
   {#if saveErr}
     <div class="msg-err">{saveErr}</div>
   {/if}
          <div class="form-actions">
     <button type="button" class="btn-ghost" disabled={saving} onclick={async () => {
       saving = true;
       saveErr = '';
       try {
         await logout();
  await onlogout?.();
       } catch (err) {
  saveErr = err?.message || 'Impossible de se déconnecter.';
       } finally {
         saving = false;
       }
     }}>Se déconnecter</button>
          </div>
 </div>

      {:else if tab === 'code'}
        <header class="sec-head sec-head-row">
          <div>
            <h2>Mode Code</h2>
            <p class="sec-sub">Paramètres de l’espace de travail et d’édition de fichiers.</p>
          </div>
          <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
          </button>
        </header>

        <div class="card code-card">
          <div class="field">
            <div class="field-head">
              <label for="workdir-val">Dossier de travail</label>
            </div>
            <div class="workdir-row">
              <div id="workdir-val" class="workdir-path" title={safePath(workdir) || ''}>{safePath(workdir) || 'Aucun dossier sélectionné'}</div>
              <button type="button" class="btn-ghost" onclick={() => onpick?.()}>
                Changer…
              </button>
            </div>
          </div>

          <label class="toggle-row" class:warn-row={autoAccept}>
            <div>
              <div class="row-title">Accepter automatiquement les modifications de fichiers</div>
              <div class="row-sub">Applique les changements proposés sans confirmation manuelle.</div>
            </div>
            <input
              type="checkbox"
              class="switch"
              class:switch-warn={autoAccept}
              checked={!!autoAccept}
              onchange={(e) => onauto?.(e.currentTarget.checked)}
            />
          </label>
          {#if autoAccept}
            <div class="warn-callout" role="note">
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
                <line x1="12" y1="9" x2="12" y2="13" />
                <line x1="12" y1="17" x2="12.01" y2="17" />
              </svg>
              <span>Mode automatique actif : toutes les modifications et créations de fichiers s’appliqueront sans validation préalable.</span>
            </div>
          {/if}
        </div>

      {:else if tab === 'skills' || tab === 'mcp'}
        <Extensions embedded section={tab} {workdir} {oninsert} onchange={onextchange} {onclose} />
      {:else if tab === 'plugins'}
        <header class="sec-head"><div><h2>Plugins</h2><p class="sec-sub">Skills et serveurs MCP fournis par des plugins.</p></div></header>
 <Plugins onchange={onextchange} />
      {:else if tab === 'console'}
        <DevConsole />

      {:else if tab === 'diag'}
        <header class="sec-head diag-head">
          <div>
            <h2>Diagnostic</h2>
            <p class="sec-sub">État technique de la session et des paramètres.</p>
          </div>
          <div class="diag-actions">
            <button type="button" class="btn-ghost" onclick={copyDiag}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
              <span>{copiedDiag ? 'Copié' : 'Copier le diagnostic'}</span>
            </button>
            <button type="button" class="head-x" aria-label="Fermer" onclick={() => onclose?.()}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
            </button>
            <label><input type="checkbox" checked={dev.enabled} onchange={(e) => setEnabled(e.currentTarget.checked)} /> Mode développeur</label>
          </div>
        </header>

        <pre class="diag-pre">{diagJson}</pre>
      {/if}
    </main>
  </div>
</div>

<style>
  .settings-overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(4, 6, 12, 0.72);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }

  .overlay-backdrop {
    position: absolute;
    inset: 0;
    border: none;
    background: transparent;
    padding: 0;
    cursor: default;
  }

  .modal {
    position: relative;
    z-index: 1;
    width: min(980px, 100%);
    height: min(680px, 90vh);
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--line2);
    border-radius: 14px;
    display: grid;
    grid-template-columns: 224px 1fr;
    overflow: hidden;
    box-shadow: var(--shadow);
  }

  .sidebar {
    background: var(--panel);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    padding: 16px 10px;
  }

  .sidebar-head {
    padding: 4px 10px 14px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }


  .sidebar-title {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--dim);
  }

  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 38px;
    padding: 0 10px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--dim);
    font-size: 13px;
    line-height: 1;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }

  .nav-item:hover {
    background: var(--hover);
    color: var(--text);
  }

  .nav-item.active {
    background: var(--hover);
    border-color: transparent;
    color: var(--text);
    font-weight: 500;
  }

  .nav-ico {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .sidebar-foot {
    padding-top: 10px;
    border-top: 1px solid var(--line);
  }

  .close-btn {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--dim);
    font-size: 12.5px;
    cursor: pointer;
  }

  .close-btn:hover {
    color: var(--text);
    background: var(--hover);
  }

  .close-btn kbd {
    margin-left: auto;
    font-size: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--panel2);
    border: 1px solid var(--line);
    color: var(--text);
    opacity: 0.78;
    font-family: inherit;
  }

  .content {
    padding: 26px 30px 32px;
    overflow-y: auto;
    scrollbar-gutter: stable;
    scrollbar-width: thin;
    scrollbar-color: color-mix(in srgb, var(--dim) 45%, transparent) transparent;
  }

  .content::-webkit-scrollbar {
    width: 8px;
  }

  .content::-webkit-scrollbar-thumb {
    background: color-mix(in srgb, var(--dim) 45%, transparent);
    border-radius: 4px;
  }

  .sec-head {
    position: sticky;
    top: -26px;
    margin: -26px -30px 20px;
    padding: 22px 30px 14px;
    background: var(--bg);
    border-bottom: 1px solid var(--line);
    z-index: 10;
  }

  .sec-head-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .sec-head h2 {
    margin: 0 0 6px;
    font-size: 20px;
    font-weight: 600;
    color: var(--text);
  }

  .sec-sub {
    margin: 0;
    font-size: 13px;
    color: var(--dim);
  }

  .head-x {
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    opacity: 0.72;
    display: inline-grid;
    place-items: center;
    cursor: pointer;
    flex-shrink: 0;
  }

  .head-x:hover {
    opacity: 1;
    background: var(--hover);
    border-color: var(--line);
  }

  .theme-grid {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 14px;
    padding: 8px 8px 4px;
  }

  @media (max-width: 820px) {
    .theme-grid {
      grid-template-columns: repeat(auto-fill, minmax(106px, 1fr));
    }
  }

  .theme-card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 0;
    border: none;
    background: transparent;
    cursor: pointer;
    color: var(--dim);
  }

  .theme-card:hover:not(:disabled),
  .theme-card:focus:not(:disabled) {
    background: transparent;
    border-color: transparent;
    transform: none;
    box-shadow: none;
  }

  .theme-card:hover:not(:disabled) .theme-label {
    color: var(--text);
  }

  .theme-preview {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 11;
    border-radius: 10px;
    border: 1px solid var(--line);
    overflow: hidden;
    display: grid;
    grid-template-columns: 26% 1fr;
    transition: border-color 0.15s, box-shadow 0.15s, outline-color 0.15s;
  }

  .theme-check {
    position: absolute;
    top: 0;
    right: 0;
    transform: translate(35%, -35%);
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid var(--bg);
    background: var(--accent);
    color: var(--on-accent, #ffffff);
    display: grid;
    place-items: center;
    box-shadow: none;
    z-index: 2;
  }

  .theme-card:hover .theme-preview {
    border-color: var(--line2);
  }

  .theme-card.selected .theme-preview {
    border-color: var(--accent);
    box-shadow: none;
    outline: 1px solid var(--accent);
    outline-offset: 2px;
  }

  .theme-card.selected .theme-label {
    color: var(--text);
    font-weight: 600;
  }

  .tp-side {
    padding: 6px 5px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .tp-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-bottom: 3px;
  }

  .tp-bar {
    height: 3px;
    border-radius: 2px;
    width: 100%;
  }

  .tp-bar.short {
    width: 65%;
  }

  .tp-main {
    padding: 5px 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .tp-top {
    height: 5px;
    border-bottom: 1px solid transparent;
  }

  .tp-cards {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 3px;
    flex: 1;
  }

  .tp-card {
    border-radius: 3px;
    border: 1px solid transparent;
  }

  .tp-composer {
    height: 10px;
    border-radius: 4px;
    border: 1px solid transparent;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: 0 3px;
  }

  .tp-pill {
    width: 8px;
    height: 4px;
    border-radius: 99px;
  }

  .theme-label {
    font-size: 12px;
    text-align: center;
  }

  .card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 16px 18px;
  }

  .notif-hero {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 14px;
    flex-wrap: wrap;
  }

  .notif-hero-left {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .notif-icon-box {
    width: 40px;
    height: 40px;
    border-radius: 10px;
    background: var(--panel2);
    border: 1px solid var(--line);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--text);
  }

  .notif-hero-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .card-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }

  .card-desc {
    font-size: 12.5px;
    color: var(--dim);
    margin-top: 2px;
  }

  .btn-primary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 36px;
    padding: 0 16px;
    background: var(--accent);
    color: var(--on-accent, #ffffff);
    border: 1px solid var(--accent);
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: filter 0.14s ease;
  }

  .btn-primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .btn-primary:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn-ghost {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 36px;
    background: transparent;
    color: var(--text);
    border: 1px solid var(--line2, var(--line));
    border-radius: 8px;
    padding: 0 13px;
    font-size: 12.5px;
    cursor: pointer;
  }

  .btn-ghost.sm {
    height: 30px;
    padding: 0 11px;
    font-size: 12px;
  }

  .btn-ghost:hover:not(:disabled) {
    background: var(--hover);
    border-color: var(--line2, var(--line));
    transform: none;
    box-shadow: none;
  }

  .btn-ghost:focus:not(:focus-visible),
  .btn-primary:focus:not(:focus-visible),
  .icon-btn:focus:not(:focus-visible),
  .head-x:focus:not(:focus-visible) {
    outline: none;
  }

  .btn-ghost:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .status-active {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 36px;
    padding: 0 2px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--ok);
  }

  .status-active.compact {
    height: auto;
    padding: 0;
    font-size: 12px;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ok);
    flex-shrink: 0;
  }

  .notif-list {
    display: flex;
    flex-direction: column;
    padding: 4px 18px;
  }

  .notif-list.locked {
    opacity: 0.55;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 13px 0;
    border-bottom: 1px solid var(--line);
    cursor: pointer;
  }

  .toggle-row:last-child {
    border-bottom: none;
  }

  .toggle-row.compact {
    padding: 2px 0 12px;
  }

  .row-title {
    font-size: 13.5px;
    font-weight: 500;
    color: var(--text);
  }

  .row-sub {
    font-size: 12px;
    color: var(--dim);
    margin-top: 2px;
  }

  .switch {
    appearance: none;
    width: 38px;
    height: 22px;
    padding: 0;
    border-radius: 999px;
    border: 1px solid var(--line2, var(--line));
    background: var(--raised, var(--line2));
    position: relative;
    cursor: pointer;
    flex-shrink: 0;
    transition: background 0.18s, border-color 0.18s;
  }

  .switch::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
    transition: transform 0.18s;
  }

  .switch:checked {
    background: var(--accent);
    border-color: var(--accent);
  }

  .switch.switch-warn:checked {
    background: var(--warn);
    border-color: var(--warn);
  }

  .switch:checked::after {
    transform: translateX(16px);
  }

  .switch:disabled {
    cursor: not-allowed;
  }

  .info-banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border-radius: 10px;
    background: var(--panel);
    border: 1px solid var(--line);
    color: var(--text);
    font-size: 12.5px;
    line-height: 1.45;
    margin-bottom: 18px;
  }

  .info-banner svg {
    flex-shrink: 0;
    margin-top: 2px;
    color: var(--dim);
  }

  .link-inline {
    background: none;
    border: none;
    padding: 0;
    color: var(--text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .link-inline:hover {
    color: var(--accent-text, var(--accent));
  }

  .usage-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .usage-refresh {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  .refresh-time {
    font-size: 11.5px;
    color: var(--dim);
  }

  .usage-kicker {
    margin: 18px 0 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }

  /* Activité tokens */
  .act-title {
    margin-bottom: 12px;
    font-size: 15px;
    font-weight: 600;
    color: var(--text);
  }
  .act-wrap {
    position: relative;
    width: 100%;
  }
  .act-months,
  .act-grid {
    display: grid;
    grid-template-columns: repeat(var(--cols), minmax(0, 1fr));
    gap: 3px;
    width: 100%;
  }
  .act-months {
    margin-bottom: 6px;
    font-size: 10.5px;
    color: var(--dim);
    white-space: nowrap;
  }
  .act-grid {
    grid-template-rows: repeat(7, auto);
    grid-auto-flow: column;
  }
  .act-cell {
    aspect-ratio: 1;
    min-width: 0;
    border-radius: 3px;
    background: var(--hover);
  }
  .act-cell:not(.pad):hover {
    outline: 1px solid var(--text);
    outline-offset: 0;
  }
  .act-cell.pad {
    background: transparent;
  }
  .act-cell.l1 { background: color-mix(in srgb, var(--accent) 28%, var(--hover)); }
  .act-cell.l2 { background: color-mix(in srgb, var(--accent) 50%, var(--hover)); }
  .act-cell.l3 { background: color-mix(in srgb, var(--accent) 75%, var(--hover)); }
  .act-cell.l4 { background: var(--accent); }
  .act-tip {
    position: absolute;
    z-index: 10;
    padding: 7px 12px;
    border-radius: 10px;
    background: #fff;
    color: #111;
    font-size: 12.5px;
    font-weight: 500;
    white-space: nowrap;
    pointer-events: none;
    transform: translate(-50%, -100%);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
  }
  .act-tip.l { transform: translate(-12px, -100%); }
  .act-tip.r { transform: translate(calc(-100% + 12px), -100%); }

  .icon-btn {
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--text);
    display: inline-grid;
    place-items: center;
    cursor: pointer;
    flex-shrink: 0;
  }

  .icon-btn:hover {
    background: var(--hover);
  }

  .spin {
    animation: spin 0.7s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .usage-card {
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin-bottom: 14px;
  }

  .quota-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }

  .quota-row.compact {
    padding: 6px 0;
  }

  .quota-meta {
    min-width: 200px;
    flex: 1;
  }

  .quota-gauge {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 240px;
  }

  .gauge-track {
    flex: 1;
    height: 8px;
    border-radius: 999px;
    background: var(--panel2);
    border: 1px solid var(--line);
    overflow: hidden;
  }

  .gauge-fill {
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
    transition: width 0.25s ease, background 0.25s ease;
  }

  .gauge-fill.warn {
    background: #f59e0b;
  }

  .gauge-fill.crit {
    background: var(--danger, #f87171);
  }

  .quota-pct {
    font-size: 12px;
    color: var(--dim);
    min-width: 78px;
    text-align: right;
    transition: color 0.16s ease;
  }

  .quota-pct.warn {
    color: var(--warn, #f59e0b);
    font-weight: 600;
  }

  .quota-pct.crit {
    color: var(--danger, #f87171);
    font-weight: 600;
  }

  .accordion {
    padding: 0;
  }

  .accordion-sum {
    padding: 13px 18px;
    font-size: 13.5px;
    font-weight: 500;
    cursor: pointer;
    list-style: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .accordion-sum::-webkit-details-marker {
    display: none;
  }

  .accordion .chev {
    color: var(--dim);
    transition: transform 0.16s ease;
  }

  .accordion[open] .chev {
    transform: rotate(180deg);
  }

  .accordion-body {
    padding: 10px 18px 14px;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .session-stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    margin-top: 4px;
  }

  .stat-box {
    padding: 10px;
    border-radius: 8px;
    background: var(--panel2);
    border: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .stat-lbl {
    font-size: 11px;
    color: var(--dim);
  }

  .stat-val {
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }

  .accordion-actions,
  .card-foot {
    display: flex;
    justify-content: flex-start;
    padding-top: 12px;
    border-top: 1px solid var(--line);
    margin-top: 2px;
  }

  .form-card {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field label {
    font-size: 13px;
    font-weight: 500;
  }

  .field-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 22px;
  }

  .input-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .input {
    width: 100%;
    height: 36px;
    padding: 0 12px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--code-bg);
    color: var(--text);
    font-size: 13px;
  }

  .input::placeholder {
    color: var(--dim);
    opacity: 0.78;
  }

  .acct {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
  }

  .acct-av {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-weight: 600;
    background: var(--accent-soft);
    color: var(--accent-text);
    border: 1px solid var(--accent-line);
  }

  .acct-id {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .acct-id span {
    color: var(--dim);
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .input-area {
    height: auto;
    min-height: 190px;
    padding: 10px 12px;
    resize: vertical;
    line-height: 1.5;
    font-family: inherit;
  }

  .instr-count {
    font-size: 11.5px;
    color: var(--dim);
  }

  .eye-btn {
    position: absolute;
    right: 8px;
    background: transparent;
    border: none;
    color: var(--dim);
    cursor: pointer;
    display: inline-flex;
    padding: 4px;
  }

  .form-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .msg-ok {
    font-size: 12.5px;
    color: var(--ok, #34d399);
  }

  .msg-err {
    font-size: 12.5px;
    color: var(--danger, #f87171);
  }

  .code-card {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .workdir-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .workdir-path {
    flex: 1;
    height: 36px;
    display: flex;
    align-items: center;
    padding: 0 12px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--code-bg);
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .warn-callout {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
    background: color-mix(in srgb, var(--warn) 12%, transparent);
    color: var(--warn);
    font-size: 12px;
    line-height: 1.45;
  }

  .warn-callout svg {
    flex-shrink: 0;
    margin-top: 1px;
  }

  .diag-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .diag-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .diag-actions .btn-ghost {
    height: 32px;
  }

  .diag-pre {
    margin: 0;
    padding: 14px;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--code-bg);
    font-size: 12px;
    line-height: 1.5;
    max-height: calc(min(680px, 90vh) - 165px);
    overflow: auto;
    color: var(--text);
  }
</style>
