<script>
  // FS Code : fenêtre principale.
  // Chaque chat ouvert = une « session » en mémoire. Changer de chat ne fait que changer
  // la session affichée : les générations en cours continuent en arrière-plan.
  import { onMount, tick, untrack } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import * as api from './lib/api.js';
  import Message from './lib/Message.svelte';
  import Tool from './lib/Tool.svelte';
  import Settings from './lib/Settings.svelte';

  const CODE_AGENT = -2003;
  const MEMO_KEY = 'fscode.chatmeta';
  const STATE_LABEL = {
    busy: 'Génération en cours',
    wait: 'Attend ta confirmation',
    unread: 'Nouvelle réponse',
  };
  const CHAT_CARDS = [
    { t: 'Expliquer un cours', d: 'Une notion difficile, expliquée simplement', p: 'Explique-moi simplement : ' },
    { t: 'Résumer un texte', d: 'Les idées clés en quelques points', p: 'Résume ce texte en 5 points clés :\n\n' },
    { t: 'Préparer un exposé', d: 'Un plan clair et des arguments', p: 'Aide-moi à structurer un exposé sur : ' },
  ];
  const CODE_CARDS = [
    { t: 'Comprendre le projet', d: 'Structure et rôle des fichiers', p: 'Explore ce projet et explique-moi sa structure.' },
    { t: 'Corriger un bug', d: 'Trouver la cause et la réparer', p: 'Trouve et corrige ce bug : ' },
    { t: 'Ajouter une fonction', d: 'Écrire et brancher le code', p: 'Ajoute cette fonctionnalité : ' },
  ];

  // mouvement : durée nulle si le système demande moins d'animations
  const calm = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const ms = (n) => (calm ? 0 : n);

  // Notices du pont MCP : masque le bruit, met en forme le reste.
  function parseNote(t) {
    const s = String(t ?? '');
    let m;
    if (/^France Student a émis un événement\s+«\s*response\.mcp_call/i.test(s)) return { kind: 'hide' };
    if ((m = /^MCP\s+«\s*(.+?)\s*»\s*:\s*(\d+)\s+outils?/i.exec(s))) return { kind: 'list', server: m[1], count: m[2] };
    if ((m = /^MCP appel\s+(\S+)\s*:\s*(\S+?)(?:\s+[—–-]+\s+(sortie|erreur)\s*:\s*([\s\S]*))?$/i.exec(s))) {
      const [srv, ...rest] = m[1].split('__');
      return {
        kind: 'call',
        server: rest.length ? srv : '',
        tool: rest.length ? rest.join('__') : m[1],
        status: m[2].toLowerCase(),
        label: m[3] ?? '',
        body: (m[4] ?? '').trim(),
      };
    }
    return { kind: 'plain' };
  }
  const CALL_LABEL = { completed: 'terminé', failed: 'échec', in_progress: 'en cours' };

  // ---- état global ----
  let settings = $state(null);
  let agents = $state([]);
  let agentId = $state(null); // agent du brouillon (nouveau chat)
  let pickedAgent = false;
  let chatAgent = null;
  let chats = $state([]);
  let chatsLoaded = $state(false);
  let input = $state('');
  let draftMode = $state('chat'); // mode du brouillon
  let workdir = $state(null); // dernier dossier choisi (valeur par défaut)
  let gerr = $state('');
  let eco = $state(false);
  let autoAccept = $state(false);
  let usage = $state(null);
  let skills = $state([]);
  let last = $state(null); // dernier message : tokens et appels
  let tally = $state({ calls: 0, prompt: 0, output: 0, messages: 0 });

  let showSettings = $state(false);
  let settingsTab = $state('compte');
  let q = $state('');
  let renaming = $state(null);
  let renameText = $state('');
  let confirmDel = $state(null);
  let showJump = $state(false);
  let sel = $state(0);

  // ---- sessions : {uid, chatId, items, busy, loading, error, runId, unread, mode, workdir,
  //      agentId, ctx, scroll, atBottom} ----
  let sessions = $state([]);
  let viewUid = $state(null); // null = brouillon « nouveau chat »

  let box; // zone de défilement
  let ta; // textarea
  let uidSeq = 0;
  let runSeq = 0;
  let skillSeq = 0;
  let raf = 0;
  const drafts = new Map(); // texte en cours de frappe, par session (ou 'new')

  // ---- utilitaires tolérants (formats API pas tous connus) ----
  const asArray = (r, keys) => (Array.isArray(r) ? r : (keys.map((k) => r?.[k]).find(Array.isArray) ?? []));
  const num = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null);
  const fmt = (n) => (n == null ? '–' : n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 0 : 1) + 'k' : String(n));
  const baseName = (p) => (p ? (p.split(/[\\/]/).filter(Boolean).pop() ?? p) : '');

  function readBudget(u) {
    const b = u?.chatBudget ?? u?.budget ?? u;
    if (!b || typeof b !== 'object') return null;
    let pct = null;
    for (const k of ['usedPercent', 'percentUsed', 'usagePercent', 'percent']) {
      if (num(b[k]) != null) {
        pct = b[k];
        break;
      }
    }
    if (pct == null) {
      const used = num(b.usedUsd ?? b.used);
      const lim = num(b.limitUsd ?? b.limit);
      const rem = num(b.remainingUsd ?? b.remaining);
      if (used != null && lim) pct = (used / lim) * 100;
      else if (rem != null && lim) pct = (1 - rem / lim) * 100;
    }
    if (pct == null) return null;
    return {
      pct: Math.max(0, Math.min(100, pct)),
      uncertain: !!(b.accountingUncertain ?? u?.accountingUncertain),
    };
  }
  function readCtx(c) {
    if (!c || typeof c !== 'object') return null;
    const pick = (ks) => ks.map((k) => num(c[k])).find((v) => v != null) ?? null;
    const used = pick(['promptTokens', 'tokens', 'usedTokens', 'contextTokens', 'totalTokens', 'used']);
    if (used == null) return null;
    return { used, max: pick(['maxTokens', 'contextWindow', 'contextSize', 'limit', 'max']) };
  }
  const parseArgs = (a) => {
    if (typeof a !== 'string') return a ?? {};
    try {
      return JSON.parse(a);
    } catch {
      return { raw: a };
    }
  };
  // Historique d'un chat -> éléments d'affichage. Ne doit jamais planter, même si le format change.
  function toItems(chat) {
    const arr = chat?.chatMessages ?? chat?.messages ?? [];
    const out = [];
    for (const m of Array.isArray(arr) ? arr : []) {
      try {
        const role = String(m?.role ?? m?.author ?? m?.sender ?? '').toLowerCase();
        if (role === 'tool' || role === 'system') continue;
        const isUser = role.startsWith('user') || role === 'human';
        let text = m?.content ?? m?.text ?? '';
        if (Array.isArray(text)) text = text.map((p) => (typeof p === 'string' ? p : (p?.text ?? ''))).join('');
        text = String(text ?? '');
        if (isUser) {
          if (text.trim()) out.push({ kind: 'user', text });
          continue;
        }
        if (text.trim()) out.push({ kind: 'assistant', text, reasoning: '' });
        const results = new Map();
        for (const r of m?.toolResults ?? m?.tool_results ?? []) {
          results.set(String(r?.callId ?? r?.call_id ?? r?.id), r);
        }
        for (const tc of m?.toolCalls ?? m?.tool_calls ?? []) {
          const name = tc?.name ?? tc?.function?.name;
          if (!name) continue;
          const id = String(tc?.callId ?? tc?.call_id ?? tc?.id ?? out.length);
          const r = results.get(id);
          out.push({
            kind: 'tool',
            callId: id,
            name,
            origin: 'local',
            args: parseArgs(tc?.arguments ?? tc?.args ?? tc?.function?.arguments),
            status: r?.isError || r?.is_error ? 'error' : 'ok',
            output: String(r?.output ?? ''),
          });
        }
      } catch {
        /* message illisible : on l'ignore */
      }
    }
    return out;
  }

  // mémo local (mode et dossier par chat, car l'API ne les renvoie pas)
  function readMemo() {
    try {
      return JSON.parse(localStorage.getItem(MEMO_KEY) || '{}') || {};
    } catch {
      return {};
    }
  }
  function saveMemo(s) {
    if (!s?.chatId) return;
    try {
      const m = readMemo();
      m[s.chatId] = { mode: s.mode, workdir: s.workdir };
      localStorage.setItem(MEMO_KEY, JSON.stringify(m));
    } catch {
      /* facultatif */
    }
  }
  function dropMemo(id) {
    try {
      const m = readMemo();
      delete m[id];
      localStorage.setItem(MEMO_KEY, JSON.stringify(m));
    } catch {
      /* facultatif */
    }
  }

  // ---- dérivés ----
  const cur = $derived(sessions.find((s) => s.uid === viewUid) ?? null);
  const mode = $derived(cur ? cur.mode : draftMode);
  const curWorkdir = $derived(cur ? cur.workdir : workdir);
  const curAgent = $derived(cur ? cur.agentId : agentId);
  const busy = $derived(!!cur?.busy);
  const items = $derived(cur?.items ?? []);
  const error = $derived(cur?.error || gerr);
  const ctxInfo = $derived(cur?.ctx ?? null);
  const budget = $derived(readBudget(usage));
  const ctxText = $derived(ctxInfo ? fmt(ctxInfo.used) + (ctxInfo.max ? ' / ' + fmt(ctxInfo.max) : '') : '–');
  const topTitle = $derived(
    cur?.chatId ? (chats.find((c) => c.id === cur.chatId)?.title ?? 'Chat') : 'Nouveau chat',
  );
  const cards = $derived(mode === 'code' ? CODE_CARDS : CHAT_CARDS);
  const shown = $derived.by(() => {
    const f = q.trim().toLowerCase();
    return f ? chats.filter((c) => c.title.toLowerCase().includes(f)) : chats;
  });
  // points de suspension « l'IA réfléchit » : dernier élément = message de l'utilisateur ou outil terminé
  const typing = $derived.by(() => {
    if (!busy) return false;
    const l = items[items.length - 1];
    if (!l || l.kind === 'user') return true;
    if (l.kind === 'tool') return l.status !== 'pending' && l.status !== 'running';
    return false;
  });
  const typingAvatar = $derived(items.length === 0 || items[items.length - 1].kind === 'user');

  // ---- menu d'autocomplétion : « / » = commandes + skills, « $ » = skills ----
  const COMMANDS = [
    { name: 'nouveau', alias: ['new'], desc: 'Nouveau chat', run: () => newChat() },
    { name: 'chat', desc: 'Passer en mode Chat', run: () => setMode('chat') },
    { name: 'code', desc: 'Passer en mode Code', run: () => setMode('code') },
    { name: 'dossier', alias: ['folder', 'cd'], only: 'code', desc: 'Choisir le dossier de travail', run: () => chooseFolder() },
    { name: 'auto', only: 'code', desc: 'Accepter auto les modifs : activer / désactiver', run: () => setAuto(!autoAccept) },
    { name: 'eco', desc: 'Mode économie : activer / désactiver', run: () => setEco(!eco) },
    { name: 'compacter', alias: ['compact'], desc: 'Compacter le contexte du chat', run: () => compact() },
    { name: 'stop', alias: ['arreter'], desc: 'Arrêter la génération', run: () => stop() },
    { name: 'reglages', alias: ['settings', 'options'], desc: 'Ouvrir les réglages', run: () => openSettings('compte') },
    { name: 'contexte', alias: ['context'], desc: 'Contexte et mode économie', run: () => openSettings('eco') },
    { name: 'compteurs', alias: ['stats', 'reset'], desc: 'Remettre les compteurs à zéro', run: () => resetTally() },
    { name: 'skills', desc: 'Lister les skills ($)', run: () => (input = '$') },
  ];
  let menuOff = $state(null); // texte pour lequel Échap a fermé le menu
  const fold = (s) => String(s ?? '').toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '');
  const menuTrig = $derived.by(() => {
    let m = /^\/([\p{L}\d_.-]*)$/u.exec(input); // « / » seulement en tout début de message
    if (m) return { ch: '/', q: fold(m[1]) };
    m = /(?:^|\s)\$([\p{L}\d_.-]*)$/u.exec(input);
    return m ? { ch: '$', q: fold(m[1]) } : null;
  });
  const menu = $derived.by(() => {
    const t = menuTrig;
    if (!t || menuOff === input) return [];
    // 0 = commence par la saisie, 1 = la contient, 2 = saisie vide, -1 = sans rapport
    const rank = (names) => {
      const ns = names.map(fold);
      if (!t.q) return 2;
      if (ns.some((n) => n.startsWith(t.q))) return 0;
      return ns.some((n) => n.includes(t.q)) ? 1 : -1;
    };
    const out = [];
    if (t.ch === '/') {
      for (const c of COMMANDS) {
        if (c.only && c.only !== mode) continue;
        const sc = rank([c.name, ...(c.alias ?? [])]);
        if (sc >= 0) out.push({ kind: 'cmd', label: '/' + c.name, desc: c.desc, tag: c.only ?? '', sc, c });
      }
    }
    for (const s of skills) {
      const sc = rank([s.name]);
      if (sc >= 0) out.push({ kind: 'skill', label: '$' + s.name, desc: s.description, tag: s.source ?? '', sc, name: s.name });
    }
    out.sort((a, b) => a.sc - b.sc || (a.kind === b.kind ? 0 : a.kind === 'cmd' ? -1 : 1));
    return out.slice(0, 8);
  });
  $effect(() => {
    menuTrig;
    sel = 0;
  });
  $effect(() => {
    sel;
    menu;
    tick().then(() => document.querySelector('.sugg button.on')?.scrollIntoView({ block: 'nearest' }));
  });
  function pickMenu(m) {
    if (!m) return;
    if (m.kind === 'skill') {
      const re = menuTrig?.ch === '/' ? /^\/[\p{L}\d_.-]*$/u : /\$[\p{L}\d_.-]*$/u;
      input = input.replace(re, () => '$' + m.name + ' ');
    } else {
      input = '';
      Promise.resolve(m.c.run()).catch((e) => (gerr = String(e)));
    }
    tick().then(() => ta?.focus());
  }
  function insertSkill(name) {
    const sep = input && !/\s$/.test(input) ? ' ' : '';
    input = input + sep + '$' + name + ' ';
    showSettings = false;
    tick().then(() => ta?.focus());
  }

  // hauteur du textarea : suit le contenu, 220 px max
  $effect(() => {
    input;
    const el = ta;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 220) + 'px';
  });

  // ---- sessions ----
  function mkSession(init) {
    sessions.push({
      uid: ++uidSeq,
      chatId: null,
      items: [],
      busy: false,
      loading: false,
      error: '',
      runId: null,
      unread: false,
      mode: 'chat',
      workdir: null,
      agentId: null,
      ctx: null,
      scroll: 0,
      atBottom: true,
      ...init,
    });
    return sessions[sessions.length - 1]; // le proxy réactif, pas l'objet brut
  }
  const byChat = (id) => sessions.find((s) => s.chatId === id);
  function dropSession(s) {
    const i = sessions.findIndex((x) => x.uid === s.uid);
    if (i >= 0) sessions.splice(i, 1);
    drafts.delete(s.uid);
    if (viewUid === s.uid) {
      viewUid = null;
      arrive(null);
    }
  }
  function chatState(c) {
    const s = byChat(c.id);
    if (!s) return '';
    if (s.items.some((i) => i.kind === 'tool' && i.status === 'pending')) return 'wait';
    if (s.busy) return 'busy';
    return s.unread ? 'unread' : '';
  }

  // ---- défilement ----
  const nearBottom = () => !box || box.scrollHeight - box.scrollTop - box.clientHeight < 140;
  function toBottom(smooth = false) {
    if (raf) return;
    raf = requestAnimationFrame(async () => {
      raf = 0;
      await tick();
      if (box) box.scrollTo({ top: box.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
    });
  }
  const onScroll = () => {
    showJump = !nearBottom();
  };

  // quitter la vue actuelle : mémorise le brouillon et la position
  function leave() {
    const s = cur;
    drafts.set(s ? s.uid : 'new', input);
    if (s && box) {
      s.scroll = box.scrollTop;
      s.atBottom = nearBottom();
    }
  }
  // arriver sur une vue : restaure le brouillon et la position
  async function arrive(s) {
    input = drafts.get(s ? s.uid : 'new') ?? '';
    await tick();
    if (!box) return;
    box.scrollTop = !s || s.atBottom !== false ? box.scrollHeight : (s.scroll ?? 0);
    showJump = !nearBottom();
  }
  function go(uid) {
    if (uid === viewUid) return;
    leave();
    viewUid = uid;
    gerr = '';
    const s = uid == null ? null : (sessions.find((x) => x.uid === uid) ?? null);
    if (s) s.unread = false;
    arrive(s);
  }
  function newChat() {
    go(null);
    tick().then(() => ta?.focus());
  }
  async function openChat(id) {
    const ex = byChat(id);
    if (ex) {
      go(ex.uid);
      return;
    }
    leave();
    const memo = readMemo()[id];
    const s = mkSession({
      chatId: id,
      loading: true,
      agentId,
      mode: memo?.mode ?? 'chat',
      workdir: memo?.workdir ?? workdir,
    });
    viewUid = s.uid;
    gerr = '';
    input = '';
    try {
      const chat = await api.getChat(id);
      s.items = toItems(chat);
      const aid = chat?.agent?.id;
      if (aid != null && agents.some((a) => a.id === aid)) {
        s.agentId = aid;
        if (!memo?.mode && aid === CODE_AGENT) s.mode = 'code';
      }
      s.loading = false;
      if (s.uid === viewUid) {
        await tick();
        if (box) box.scrollTop = box.scrollHeight;
        showJump = false;
      }
      loadCtx(s);
    } catch (e) {
      const i = sessions.findIndex((x) => x.uid === s.uid);
      if (i >= 0) sessions.splice(i, 1);
      if (viewUid === s.uid) viewUid = null;
      gerr = String(e);
    }
  }

  // ---- chargement ----
  async function loadChats() {
    try {
      const r = await api.listChats(40, null);
      const fresh = asArray(r, ['chats', 'items', 'data']).map((c) => ({
        id: String(c.id ?? c.chatId),
        title: c.title ?? c.name ?? 'Sans titre',
      }));
      // un chat tout juste créé peut ne pas encore figurer dans la liste du serveur
      const keep = chats.filter(
        (c) => !fresh.some((f) => f.id === c.id) && sessions.some((s) => s.chatId === c.id && s.busy),
      );
      chats = [...keep, ...fresh];
    } catch (e) {
      gerr = String(e);
    }
    chatsLoaded = true;
  }
  async function loadUsage() {
    try {
      usage = await api.getUsage();
    } catch {
      /* jauge facultative */
    }
  }
  async function loadCtx(s) {
    if (!s?.chatId) return;
    const id = s.chatId;
    try {
      const c = readCtx(await api.getContext(id));
      if (s.chatId === id) s.ctx = c;
    } catch {
      /* facultatif */
    }
  }
  async function loadSkills(m, w) {
    const n = ++skillSeq;
    try {
      const r = await api.listSkills(m === 'code' ? w : null);
      if (n === skillSeq) skills = r;
    } catch {
      if (n === skillSeq) skills = [];
    }
  }
  $effect(() => {
    // recharge les skills quand le mode ou le dossier de la vue change
    const m = mode;
    const w = curWorkdir;
    untrack(() => loadSkills(m, w));
  });
  async function loadAll() {
    try {
      agents = (await api.listAgents()).filter((a) => a.isActive !== false);
      const saved = settings?.lastAgentId;
      pickedAgent = saved != null && agents.some((a) => a.id === saved);
      agentId = pickedAgent ? saved : (agents.find((a) => a.isDefault)?.id ?? agents[0]?.id ?? null);
    } catch (e) {
      gerr = String(e);
    }
    await Promise.all([loadChats(), loadUsage()]);
  }
  async function loadSettings() {
    settings = await api.getSettings();
    eco = settings.eco;
    autoAccept = settings.autoAccept;
    workdir = settings.lastWorkdir ?? null;
  }

  onMount(async () => {
    try {
      await loadSettings();
    } catch (e) {
      gerr = String(e);
      return;
    }
    if (!settings.hasKey) {
      settingsTab = 'compte';
      showSettings = true;
      return;
    }
    await loadAll();
    tick().then(() => ta?.focus());
  });

  // ---- liste des chats ----
  function startRename(c) {
    renaming = c.id;
    renameText = c.title;
  }
  async function renameKey(e, c) {
    if (e.key === 'Escape') renaming = null;
    if (e.key !== 'Enter') return;
    const title = renameText.trim();
    renaming = null;
    if (!title || title === c.title) return;
    try {
      await api.renameChat(c.id, title);
      chats = chats.map((x) => (x.id === c.id ? { ...x, title } : x));
    } catch (err) {
      gerr = String(err);
    }
  }
  async function del(c) {
    if (confirmDel !== c.id) {
      confirmDel = c.id;
      return;
    }
    confirmDel = null;
    const s = byChat(c.id);
    try {
      if (s?.busy) await api.cancelChat(s.chatId, s.runId).catch(() => {});
      await api.deleteChat(c.id);
      chats = chats.filter((x) => x.id !== c.id);
      dropMemo(c.id);
      if (s) dropSession(s);
    } catch (e) {
      gerr = String(e);
    }
  }
  const focus = (n) => {
    n.focus();
    n.select();
  };

  // ---- réglages de la vue ----
  const putAgent = (id) => {
    if (cur) cur.agentId = id;
    else agentId = id;
  };
  function setMode(m) {
    if (busy || m === mode) return;
    if (m === 'code') {
      chatAgent = curAgent;
      if (!pickedAgent && agents.some((a) => a.id === CODE_AGENT)) putAgent(CODE_AGENT);
    } else if (chatAgent != null) {
      putAgent(chatAgent);
    }
    if (cur) {
      cur.mode = m;
      saveMemo(cur);
    } else {
      draftMode = m;
    }
  }
  function onAgent(e) {
    const a = agents.find((x) => String(x.id) === e.currentTarget.value);
    if (!a) return;
    pickedAgent = true;
    putAgent(a.id);
    api.saveSettings({ lastAgentId: a.id }).catch(() => {});
  }
  async function chooseFolder() {
    try {
      const p = await api.pickFolder();
      if (!p) return;
      workdir = p;
      if (cur) {
        cur.workdir = p;
        saveMemo(cur);
      }
      await api.saveSettings({ lastWorkdir: p });
    } catch (e) {
      gerr = String(e);
    }
  }
  async function setAuto(v) {
    autoAccept = v;
    try {
      await api.saveSettings({ autoAccept: v });
    } catch (e) {
      gerr = String(e);
    }
  }
  async function setEco(v) {
    eco = v;
    try {
      await api.saveSettings({ eco: v });
    } catch (e) {
      gerr = String(e);
    }
  }
  async function compact() {
    const s = cur;
    if (!s?.chatId || s.busy) return;
    try {
      await api.compactChat(s.chatId);
      await loadCtx(s);
    } catch (e) {
      gerr = String(e);
    }
  }
  function resetTally() {
    tally = { calls: 0, prompt: 0, output: 0, messages: 0 };
    last = null;
  }
  function openSettings(tab) {
    settingsTab = tab;
    showSettings = true;
  }
  async function onsaved() {
    try {
      await loadSettings();
    } catch (e) {
      gerr = String(e);
      return;
    }
    if (settings?.hasKey) {
      showSettings = false;
      await loadAll();
    }
  }
  function clearError() {
    if (cur) cur.error = '';
    gerr = '';
  }
  function useCard(p) {
    input = p;
    tick().then(() => {
      if (!ta) return;
      ta.focus();
      ta.setSelectionRange(p.length, p.length);
    });
  }

  // ---- envoi ----
  async function resolve(t, allow, always) {
    try {
      if (allow && always && t.origin === 'mcp') await api.trustTool(t.name, true);
      t.status = allow ? 'running' : 'denied';
      await api.resolveTool(t.callId, allow);
    } catch (e) {
      gerr = String(e);
    }
  }

  async function send() {
    const content = input.trim();
    if (!content || busy || cur?.loading) return;
    if (mode === 'code' && !curWorkdir) {
      gerr = 'Choisis d’abord un dossier de travail (puce 📁 sous la zone de texte).';
      return;
    }
    if (curAgent == null) {
      gerr = 'Aucun agent disponible.';
      return;
    }
    let s = cur;
    if (!s) {
      s = mkSession({ mode: draftMode, workdir, agentId });
      drafts.delete('new');
      viewUid = s.uid;
    }
    drafts.delete(s.uid);
    input = '';
    gerr = '';
    s.error = '';
    s.busy = true;
    s.unread = false;
    s.atBottom = true;
    const runId = `r${Date.now().toString(36)}${++runSeq}`;
    s.runId = runId;
    s.items.push({ kind: 'user', text: content });
    showJump = false;
    toBottom();

    let curA = null; // message de l'IA en cours d'écriture
    const assistant = () => {
      if (!curA) {
        s.items.push({ kind: 'assistant', text: '', reasoning: '' });
        curA = s.items[s.items.length - 1];
      }
      return curA;
    };
    const visible = () => s.uid === viewUid;

    try {
      await api.sendMessage(
        {
          chatId: s.chatId,
          agentId: s.agentId,
          content,
          workdir: s.mode === 'code' ? s.workdir : null,
          autoAccept,
          eco,
          runId,
        },
        (ev) => {
          // on ne suit le bas de page que si cette session est affichée ET que l'utilisateur y est déjà
          const follow = visible() && nearBottom();
          switch (ev.type) {
            case 'opened':
              s.chatId = ev.chat_id;
              if (!chats.some((c) => c.id === ev.chat_id)) {
                chats = [{ id: ev.chat_id, title: 'Nouveau chat' }, ...chats];
              }
              saveMemo(s);
              break;
            case 'title':
              chats = chats.map((c) => (c.id === ev.chat_id ? { ...c, title: ev.title } : c));
              break;
            case 'delta':
              assistant().text += ev.text;
              break;
            case 'reasoning':
              assistant().reasoning += ev.text;
              break;
            case 'tool_call':
              curA = null;
              s.items.push({
                kind: 'tool',
                callId: ev.call_id,
                name: ev.name,
                origin: ev.origin,
                args: ev.args,
                status: ev.needs_confirm ? 'pending' : 'running',
                output: '',
              });
              break;
            case 'tool_result': {
              const t = s.items.find((i) => i.kind === 'tool' && i.callId === ev.call_id);
              if (t) {
                t.output = ev.output;
                t.status = ev.denied ? 'denied' : ev.is_error ? 'error' : 'ok';
              }
              break;
            }
            case 'notice':
              curA = null;
              s.items.push({ kind: 'notice', text: ev.text });
              break;
            case 'done': {
              s.chatId = ev.chat_id;
              const p = ev.total_prompt_tokens ?? 0;
              const o = ev.total_output_tokens ?? 0;
              const n = ev.api_calls ?? 1;
              last = { prompt: p, output: o, calls: n, eco: ev.eco };
              tally = {
                calls: tally.calls + n,
                prompt: tally.prompt + p,
                output: tally.output + o,
                messages: tally.messages + 1,
              };
              if (ev.prompt_tokens != null) s.ctx = { used: ev.prompt_tokens, max: s.ctx?.max ?? null };
              saveMemo(s);
              break;
            }
            case 'failed':
              s.error = `${ev.code} : ${ev.message}${ev.retryable ? ' (réessayable)' : ''}`;
              break;
            case 'cancelled':
              curA = null;
              s.items.push({ kind: 'notice', text: 'Annulé.' });
              break;
          }
          if (follow) toBottom();
        },
      );
    } catch (e) {
      s.error = String(e);
    } finally {
      s.busy = false;
      s.runId = null;
      if (!visible()) s.unread = true; // terminé pendant que tu étais ailleurs
      loadChats();
      loadUsage();
      loadCtx(s);
    }
  }

  function stop() {
    const s = cur;
    if (s?.busy) api.cancelChat(s.chatId, s.runId).catch((e) => (s.error = String(e)));
  }

  function onKey(e) {
    if (e.isComposing) return;
    if (menu.length) {
      const n = menu.length;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        sel = (Math.min(sel, n - 1) + 1) % n;
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        sel = (Math.min(sel, n - 1) - 1 + n) % n;
        return;
      }
      if (e.key === 'Tab' || (e.key === 'Enter' && !e.shiftKey)) {
        e.preventDefault();
        pickMenu(menu[Math.min(sel, n - 1)]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        menuOff = input;
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }
  function onWindowKey(e) {
    if (e.key === 'Escape' && showSettings) showSettings = false;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
      const k = e.key.toLowerCase();
      if (k === 'n') {
        e.preventDefault();
        newChat();
      } else if (k === ',') {
        e.preventDefault();
        openSettings('compte');
      }
    }
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#snippet mark()}
  <svg viewBox="32 32 448 448" aria-hidden="true">
    <rect x="32" y="32" width="448" height="448" rx="112" fill="#3d7bff" style="fill: var(--accent)" />
    <rect x="32" y="32" width="448" height="224" rx="112" fill="#fff" opacity="0.14" />
    <path d="M156 178 L276 256 L156 334" fill="none" stroke="#fff" stroke-width="44" stroke-linecap="round" stroke-linejoin="round" />
    <rect x="304" y="316" width="96" height="40" rx="20" fill="#fff" opacity="0.92" />
  </svg>
{/snippet}

<div class="app" class:code={mode === 'code'}>
  <!-- ============ barre latérale ============ -->
  <aside class="side">
    <div class="brand">
      <span class="logo">{@render mark()}</span>
      <span class="bname">FS Code</span>
    </div>

    <button class="newbtn" onclick={newChat}>＋ Nouveau chat</button>

    <div class="find">
      <input bind:value={q} placeholder="Rechercher un chat…" spellcheck="false" aria-label="Rechercher un chat" />
    </div>

    <div class="list">
      {#each shown as c, i (c.id)}
        {@const st = chatState(c)}
        <div
          class="chatrow"
          class:active={c.id === cur?.chatId}
          style="animation-delay: {Math.min(i, 12) * 22}ms"
          role="presentation"
          onmouseleave={() => confirmDel === c.id && (confirmDel = null)}
        >
          {#if renaming === c.id}
            <input
              class="renamein"
              use:focus
              bind:value={renameText}
              onkeydown={(e) => renameKey(e, c)}
              onblur={() => (renaming = null)}
            />
          {:else}
            <button class="chat" onclick={() => openChat(c.id)} title={c.title}>
              <span class="ct">{c.title}</span>
              {#if st}<i class="ind {st}" title={STATE_LABEL[st]}></i>{/if}
            </button>
            <div class="acts" class:show={confirmDel === c.id}>
              <button class="mini" title="Renommer" onclick={() => startRename(c)}>✎</button>
              <button class="mini danger" title="Supprimer" onclick={() => del(c)}>
                {confirmDel === c.id ? 'Supprimer ?' : '✕'}
              </button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="none">
          {chatsLoaded ? (q ? 'Aucun résultat.' : 'Aucun chat pour l’instant.') : 'Chargement…'}
        </div>
      {/each}
    </div>

    <div class="foot">
      <button class="gear" onclick={() => openSettings('compte')}>⚙ Réglages</button>
      {#if eco}<span class="pill eco anim-fade">ÉCO</span>{/if}
    </div>
  </aside>

  <!-- ============ zone principale ============ -->
  <section class="main">
    <header class="top">
      {#key topTitle}
        <h1 class="ttl anim-fade" title={topTitle}>{topTitle}</h1>
      {/key}
      {#if busy}
        <span class="pill live" out:fade={{ duration: ms(120) }}><i></i> Génération en cours</span>
      {/if}
      {#key mode}<span class="pill modep anim-fade">{#if mode === 'code'}&lt;/&gt; Code{:else}Chat{/if}</span>{/key}
      <span class="spacer"></span>
      {#if eco}
        <button class="pill eco anim-fade" onclick={() => openSettings('eco')} title="Mode économie actif">⚡ ÉCO</button>
      {/if}
      {#if ctxInfo}
        <button class="pill anim-fade" onclick={() => openSettings('eco')} title="Contexte du chat">
          Contexte {ctxText}
        </button>
      {/if}
      {#if budget}
        <button class="pill anim-fade" onclick={() => openSettings('compte')} title="Budget utilisé">
          Budget {budget.uncertain ? '~' : ''}{Math.round(budget.pct)} %
          <span class="mg" class:warn={budget.pct >= 80}><i style="width:{budget.pct}%"></i></span>
        </button>
      {/if}
      {#if busy}
        <div class="prog" transition:fade={{ duration: ms(160) }}></div>
      {/if}
    </header>

    <div class="stage">
      <div class="scroll" bind:this={box} onscroll={onScroll}>
        <div class="col">
          {#key viewUid}
            <div class="view">
              {#if cur?.loading}
                <div class="loading"><span class="spin"></span> Chargement du chat…</div>
              {:else if items.length === 0}
                <div class="hero stagger">
                  <div class="herologo">{@render mark()}</div>
                  <h2>{mode === 'code' ? 'Que veux-tu construire ?' : 'Comment puis-je t’aider ?'}</h2>
                  <p class="dim">
                    {mode === 'code'
                      ? 'L’IA lit et modifie les fichiers du dossier choisi.'
                      : 'Pose ta question ou pars d’une idée.'}
                  </p>
                  <div class="cards">
                    {#each cards as k, i (k.t)}
                      <button class="card" style="animation-delay: {170 + i * 50}ms" onclick={() => useCard(k.p)}>
                        <b>{k.t}</b>
                        <span>{k.d}</span>
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}

              {#each items as it, i (i)}
                {#if it.kind === 'user'}
                  <div class="u"><div class="ubub">{it.text}</div></div>
                {:else if it.kind === 'assistant'}
                  <div class="ai">
                    {#if i === 0 || !['assistant', 'tool'].includes(items[i - 1].kind)}<div class="av">{@render mark()}</div>{:else}<div></div>{/if}
                    <div class="aib">
                      <Message text={it.text} reasoning={it.reasoning} streaming={busy && i === items.length - 1} />
                    </div>
                  </div>
                {:else if it.kind === 'tool'}
                  <div class="toolrow"><Tool t={it} onresolve={resolve} /></div>
                {:else}
                  {@const n = parseNote(it.text)}
                  {#if n.kind === 'list'}
                    <div class="mcpnote"><i class="dot"></i>MCP <b>{n.server}</b> · {n.count} outils</div>
                  {:else if n.kind === 'call'}
                    <details class="mcpcall {n.status}" class:nobody={!n.body} class:bad={n.label === 'erreur'}>
                      <summary>
                        <i class="dot"></i>
                        <span class="mname">{n.tool}</span>
                        {#if n.server}<span class="msrv">{n.server}</span>{/if}
                        <span class="mst">{CALL_LABEL[n.status] ?? n.status}</span>
                      </summary>
                      {#if n.body}<pre>{n.body}</pre>{/if}
                    </details>
                  {:else if n.kind !== 'hide'}
                    <div class="note">{it.text}</div>
                  {/if}
                {/if}
              {/each}

              {#if typing}
                <div class="ai">
                  {#if typingAvatar}<div class="av">{@render mark()}</div>{:else}<div></div>{/if}
                  <div class="aib"><Message text="" streaming /></div>
                </div>
              {/if}
            </div>
          {/key}
        </div>
      </div>

      {#if showJump}
        <button
          class="jump"
          transition:fly={{ y: 10, duration: ms(160) }}
          onclick={() => toBottom(true)}
          title="Aller en bas"
          aria-label="Aller en bas">↓</button
        >
      {/if}
    </div>

    <!-- ============ zone de saisie ============ -->
    <div class="dock">
      <div class="dockin">
        {#if error}
          <div class="error errbar" out:fade={{ duration: ms(120) }}>
            <span>{error}</span>
            <button class="x" onclick={clearError} title="Fermer" aria-label="Fermer">✕</button>
          </div>
        {/if}

        <div class="composer">
          {#if menu.length}
            <div class="sugg" role="listbox" out:fade={{ duration: ms(90) }}>
              {#each menu as m, i}
                <button
                  class:on={i === Math.min(sel, menu.length - 1)}
                  class:cmd={m.kind === 'cmd'}
                  onmousedown={(e) => e.preventDefault()}
                  onclick={() => pickMenu(m)}
                >
                  <b>{m.label}</b><small>{m.desc}</small>
                  {#if m.tag}<em class="stag">{m.tag}</em>{/if}
                </button>
              {/each}
            </div>
          {/if}

          <textarea
            bind:this={ta}
            bind:value={input}
            onkeydown={onKey}
            rows="1"
            aria-label="Message"
            placeholder={busy
              ? 'Génération en cours… tu peux changer de chat, elle continue.'
              : mode === 'code'
                ? 'Demande quelque chose sur le projet…  (/ commandes · $ skills)'
                : 'Écris ton message…  (/ commandes · $ skills)'}
          ></textarea>

          <div class="bar">
            <select class="agent" onchange={onAgent} disabled={busy} title="Agent" aria-label="Agent">
              {#each agents as a (a.id)}
                <option value={a.id} selected={a.id === curAgent}>{a.name ?? a.id}</option>
              {/each}
            </select>

            <div class="seg">
              <button class:on={mode === 'chat'} onclick={() => setMode('chat')} disabled={busy}>Chat</button>
              <button class:on={mode === 'code'} onclick={() => setMode('code')} disabled={busy}>Code</button>
            </div>

            {#if mode === 'code'}
              <button class="pill chip" onclick={chooseFolder} disabled={busy} title={curWorkdir ?? 'Choisir un dossier'}>
                {curWorkdir ? '📁 ' + baseName(curWorkdir) : '📁 Choisir un dossier…'}
              </button>
            {/if}

            <span class="spacer"></span>

            {#if mode === 'code'}
              <label class="chk small" title="Valable pour les modifications de fichiers, jamais pour les commandes">
                <input type="checkbox" checked={autoAccept} onchange={(e) => setAuto(e.currentTarget.checked)} />
                Accepter auto les modifs
              </label>
            {/if}

            {#if busy}
              <button class="round stop" onclick={stop} title="Arrêter" aria-label="Arrêter">■</button>
            {:else}
              <button
                class="round go"
                onclick={send}
                disabled={!input.trim() || !!cur?.loading}
                title="Envoyer"
                aria-label="Envoyer">↑</button
              >
            {/if}
          </div>
        </div>

        <div class="help">Entrée : envoyer · Maj+Entrée : saut de ligne · / : commandes · $ : skills · Ctrl+N : nouveau chat</div>
      </div>
    </div>
  </section>

  {#key mode}<div class="sweep" aria-hidden="true"></div>{/key}
</div>

{#if showSettings && settings}
  <Settings
    {settings}
    {usage}
    {budget}
    workdir={curWorkdir}
    {autoAccept}
    {eco}
    {last}
    session={tally}
    ctx={ctxInfo}
    chatId={cur?.chatId ?? null}
    {busy}
    tab={settingsTab}
    onclose={() => (showSettings = false)}
    {onsaved}
    oneco={setEco}
    onauto={setAuto}
    onpick={chooseFolder}
    oncompact={compact}
    onreset={resetTally}
    oninsert={insertSkill}
    onextchange={() => loadSkills(mode, curWorkdir)}
  />
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: 272px minmax(0, 1fr);
    height: 100vh;
    background: var(--bg);
  }

  /* ---------- barre latérale ---------- */
  .side {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    padding: 14px 10px 10px;
    background: var(--panel);
    border-right: 1px solid var(--line);
    animation: fs-left var(--t-slow) var(--ease) both;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 6px 4px;
  }
  /* logo, grand logo, avatar : carré plein, bord clair, ombre dure */
  .logo,
  .herologo,
  .av {
    display: grid;
    place-items: center;
    flex: none;
    color: #fff;
    font-family: var(--mono);
    font-weight: 800;
    background: var(--accent);
    border: 1px solid #6b97ff;
  }
  .logo {
    width: 30px;
    height: 30px;
    font-size: 12px;
    letter-spacing: 0.5px;
    box-shadow: 3px 3px 0 rgba(61, 123, 255, 0.3);
  }
  .bname {
    font-family: var(--mono);
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .newbtn {
    width: 100%;
    padding: 9px 12px;
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: #cfe0ff;
    font-weight: 600;
  }
  .newbtn:hover:not(:disabled) {
    background: rgba(61, 123, 255, 0.24);
    border-color: var(--accent);
    box-shadow: 2px 2px 0 rgba(61, 123, 255, 0.35);
  }
  .find input {
    width: 100%;
    padding: 6px 10px;
    font-size: 13px;
    background: var(--bg);
    border-color: var(--line);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0 -4px;
    padding: 0 4px;
  }
  .none {
    padding: 14px 10px;
    color: var(--faint);
    font-size: 13px;
    text-align: center;
  }
  .chatrow {
    position: relative;
    animation: fs-left var(--t-slow) var(--ease) both;
  }
  /* ligne de chat : filet gauche (accent si actif), léger décalage au survol */
  .chat {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    text-align: left;
    background: transparent;
    border: 0;
    border-left: 2px solid transparent;
    color: var(--dim);
  }
  .chat:hover:not(:disabled) {
    background: var(--panel2);
    border: 0;
    border-left: 2px solid var(--line2);
    box-shadow: none;
    transform: translateX(2px);
    color: var(--text);
  }
  .chatrow.active .chat,
  .chatrow.active .chat:hover:not(:disabled) {
    background: var(--raised);
    border: 0;
    border-left: 2px solid var(--accent);
    color: var(--text);
  }
  .ct {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ind {
    flex: none;
    width: 8px;
    height: 8px;
  }
  .ind.busy {
    background: var(--accent);
    animation: fs-pulse 1.2s ease-in-out infinite;
  }
  .ind.wait {
    background: var(--warn);
    box-shadow: 0 0 0 3px rgba(229, 165, 10, 0.22);
  }
  .ind.unread {
    background: var(--ok);
  }
  .acts {
    position: absolute;
    top: 50%;
    right: 4px;
    transform: translateY(-50%);
    display: none;
    gap: 3px;
    padding: 2px;
    background: var(--panel2);
  }
  .chatrow.active .acts {
    background: var(--raised);
  }
  .chatrow:hover .acts,
  .acts.show {
    display: flex;
    animation: fs-fade var(--t-fast) both;
  }
  .chatrow:hover .ind {
    opacity: 0;
  }
  .renamein {
    width: 100%;
    padding: 6px 10px;
    font-size: 13px;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
  }
  .gear {
    flex: 1;
    text-align: left;
    background: transparent;
    border-color: transparent;
    color: var(--dim);
  }
  .gear:hover:not(:disabled) {
    background: var(--panel2);
    border-color: transparent;
    box-shadow: none;
    transform: none;
    color: var(--text);
  }

  /* ---------- puces ---------- */
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: var(--panel2);
    border: 1px solid var(--line);
    color: var(--dim);
    font-size: 12px;
    line-height: 1;
    white-space: nowrap;
  }
  button.pill:hover:not(:disabled) {
    background: var(--raised);
    border-color: var(--line2);
    color: var(--text);
  }
  .pill.eco {
    color: #ffd479;
    background: rgba(229, 165, 10, 0.1);
    border-color: rgba(229, 165, 10, 0.4);
  }
  button.pill.eco:hover:not(:disabled) {
    color: #ffd479;
    background: rgba(229, 165, 10, 0.18);
    border-color: rgba(229, 165, 10, 0.6);
  }
  .pill.live {
    color: #cfe0ff;
    background: var(--accent-soft);
    border-color: var(--accent-line);
    animation: fs-left var(--t) var(--ease) both;
  }
  .pill.live i {
    width: 7px;
    height: 7px;
    background: var(--accent);
    animation: fs-pulse 1.1s ease-in-out infinite;
  }
  .pill.chip {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mg {
    width: 36px;
    height: 4px;
    background: var(--raised);
    overflow: hidden;
  }
  .mg i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width var(--t-slow) var(--ease);
  }
  .mg.warn i {
    background: var(--warn);
  }

  /* ---------- zone principale ---------- */
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100vh;
  }
  .top {
    position: relative;
    display: flex;
    align-items: center;
    flex: none;
    gap: 8px;
    height: 48px;
    padding: 0 18px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }
  /* liseré qui défile pendant la génération */
  .prog {
    position: absolute;
    right: 0;
    bottom: -1px;
    left: 0;
    z-index: 1;
    height: 2px;
    overflow: hidden;
    pointer-events: none;
  }
  .prog::after {
    content: '';
    position: absolute;
    inset: 0;
    width: 30%;
    background: var(--accent);
    animation: sweep 1.2s var(--ease) infinite;
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .ttl {
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 14px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stage {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .scroll {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 22px 0 10px;
  }
  .col {
    display: flex;
    flex-direction: column;
    width: min(820px, calc(100% - 36px));
    min-height: 100%;
    margin: 0 auto;
  }
  /* contenu de la vue : se re-crée (fondu) à chaque changement de chat */
  .view {
    display: flex;
    flex-direction: column;
    flex: 1;
    gap: 14px;
    min-width: 0;
    animation: fs-fade var(--t) both;
  }
  .loading {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: auto;
    color: var(--dim);
  }
  .spin {
    width: 16px;
    height: 16px;
    border: 2px solid var(--line2);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: fs-spin 0.7s linear infinite;
  }

  /* écran d'accueil (les enfants arrivent en cascade : classe .stagger) */
  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    margin: auto 0;
    padding: 24px 0;
    text-align: center;
  }
  .herologo {
    width: 54px;
    height: 54px;
    font-size: 20px;
    box-shadow: 5px 5px 0 rgba(61, 123, 255, 0.3);
  }
  .hero h2 {
    margin: 10px 0 0;
    font-size: 24px;
    font-weight: 650;
  }
  .hero p {
    margin: 0 0 14px;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
    width: 100%;
    animation: none; /* les cartes ont leur propre délai */
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 12px 14px;
    text-align: left;
    background: var(--panel);
    border-color: var(--line);
    box-shadow:
      inset 0 2px 0 transparent,
      0 0 0 rgba(0, 0, 0, 0);
    animation: fs-rise var(--t-slow) var(--ease) backwards;
  }
  .card:hover:not(:disabled) {
    background: var(--panel2);
    border-color: var(--line2);
    box-shadow:
      inset 0 2px 0 var(--accent),
      3px 3px 0 rgba(0, 0, 0, 0.5);
  }
  .card b {
    font-weight: 600;
  }
  .card span {
    color: var(--dim);
    font-size: 12.5px;
  }

  /* messages */
  .u {
    display: flex;
    justify-content: flex-end;
  }
  .ubub {
    max-width: min(640px, 82%);
    padding: 9px 14px;
    border: 1px solid var(--accent-line);
    border-right: 3px solid var(--accent);
    background: var(--accent-soft);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    animation: u-in var(--t) var(--ease) both;
  }
  @keyframes u-in {
    from {
      opacity: 0;
      transform: translateX(14px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  .ai {
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr);
    gap: 12px;
    animation: fs-rise var(--t) var(--ease) both;
  }
  .av {
    width: 30px;
    height: 30px;
    font-size: 11px;
    box-shadow: 2px 2px 0 rgba(61, 123, 255, 0.3);
  }
  .aib {
    min-width: 0;
    padding-top: 3px;
  }
  .toolrow {
    margin-left: 42px;
  }
  .note {
    align-self: center;
    padding: 3px 12px;
    border: 1px solid var(--line);
    background: var(--panel2);
    color: var(--dim);
    font: 11px/1.5 var(--mono);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    animation: fs-fade var(--t) both;
  }
  .jump {
    position: absolute;
    bottom: 12px;
    left: 50%;
    width: 34px;
    height: 34px;
    margin-left: -17px;
    padding: 0;
    box-shadow: 3px 3px 0 rgba(0, 0, 0, 0.45);
  }

  /* ---------- saisie ---------- */
  .dock {
    flex: none;
    padding: 6px 0 14px;
  }
  .dockin {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(820px, calc(100% - 36px));
    margin: 0 auto;
  }
  .errbar {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .errbar span {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .x {
    padding: 0 6px;
    border: 0;
    background: transparent;
    color: inherit;
  }
  .x:hover:not(:disabled) {
    border: 0;
    background: rgba(255, 255, 255, 0.08);
    box-shadow: none;
    transform: none;
  }
  .composer {
    position: relative;
    padding: 10px 12px 8px;
    border: 1px solid var(--line2);
    border-top: 2px solid var(--line2);
    background: var(--panel);
    box-shadow: 4px 4px 0 rgba(0, 0, 0, 0.4);
    transition:
      border-color var(--t-fast),
      box-shadow var(--t-fast) var(--ease),
      transform var(--t-fast) var(--ease);
  }
  .composer:focus-within {
    border-color: var(--accent-line);
    border-top-color: var(--accent);
    box-shadow: 5px 5px 0 rgba(61, 123, 255, 0.28);
    transform: translate(-1px, -1px);
  }
  .composer textarea {
    display: block;
    width: 100%;
    min-height: 24px;
    max-height: 220px;
    padding: 4px 2px;
    border: 0;
    background: transparent;
    line-height: 1.5;
    resize: none;
  }
  .composer textarea:focus {
    border: 0;
    box-shadow: none;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }
  .agent {
    max-width: 200px;
    padding: 5px 10px;
    border-color: var(--line);
    background: var(--panel2);
    font-size: 12px;
  }
  /* sélecteur Chat / Code : filet accent sous l'option active */
  .seg {
    display: inline-flex;
    border: 1px solid var(--line);
    background: var(--panel2);
  }
  .seg button {
    padding: 4px 12px;
    border: 0;
    background: transparent;
    box-shadow: inset 0 -2px 0 transparent;
    color: var(--dim);
    font-size: 12px;
  }
  .seg button:hover:not(:disabled) {
    border: 0;
    background: transparent;
    box-shadow: inset 0 -2px 0 var(--line2);
    transform: none;
    color: var(--text);
  }
  .seg button.on,
  .seg button.on:hover:not(:disabled) {
    background: var(--raised);
    box-shadow: inset 0 -2px 0 var(--accent);
    color: var(--text);
  }
  .small {
    color: var(--dim);
    font-size: 12px;
  }
  .round {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    padding: 0;
    font-size: 16px;
    line-height: 1;
  }
  .round.go {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
    font-weight: 700;
    animation: fs-pop var(--t) var(--ease);
  }
  .round.go:hover:not(:disabled) {
    background: #5a8fff;
    border-color: #5a8fff;
    box-shadow: 2px 2px 0 rgba(61, 123, 255, 0.35);
  }
  .round.go:disabled {
    background: var(--raised);
    border-color: var(--line2);
    color: var(--faint);
    opacity: 0.7;
  }
  .round.stop {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
    animation: fs-pulse 1.4s ease-in-out infinite;
  }
  .round.stop:hover:not(:disabled) {
    background: #ff8585;
    border-color: #ff8585;
    box-shadow: 2px 2px 0 rgba(255, 107, 107, 0.35);
  }
  .help {
    color: var(--faint);
    font-size: 11px;
    text-align: center;
  }

  /* suggestions de skills */
  .sugg {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    border: 1px solid var(--line2);
    border-top: 2px solid var(--accent);
    background: var(--panel2);
    box-shadow: 4px 4px 0 rgba(0, 0, 0, 0.45);
    animation: fs-rise var(--t-fast) var(--ease) both;
  }
  .sugg button {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 7px 10px;
    text-align: left;
    border: 0;
    border-left: 2px solid transparent;
    background: transparent;
  }
  .sugg button.on,
  .sugg button:hover:not(:disabled) {
    border: 0;
    border-left: 2px solid var(--accent);
    background: var(--accent-soft);
    box-shadow: none;
    transform: none;
  }
  .sugg b {
    color: #cfe0ff;
    font-family: var(--mono);
    font-weight: 600;
  }
  .sugg small {
    min-width: 0;
    overflow: hidden;
    color: var(--dim);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ================= REFONTE : formes douces ================= */
  /* --r = 8px (petits éléments), --r-lg = 14px (conteneurs), 999px = pastilles, 50% = ronds */
  .logo {
    border-radius: 10px;
    box-shadow: none;
  }
  .herologo {
    border-radius: 18px;
    box-shadow: 0 10px 30px rgba(61, 123, 255, 0.3);
  }
  .av {
    border-radius: 10px;
    box-shadow: none;
  }
  .logo,
  .herologo,
  .av {
    background: none;
    border: none;
    overflow: visible;
  }
  .logo svg,
  .herologo svg,
  .av svg {
    display: block;
    width: 100%;
    height: 100%;
  }
  .newbtn {
    border-radius: var(--r-lg);
  }
  .newbtn:hover:not(:disabled) {
    box-shadow: none;
  }
  .find input {
    padding: 7px 14px;
    border-radius: 999px;
  }
  .chat,
  .chat:hover:not(:disabled),
  .chatrow.active .chat,
  .chatrow.active .chat:hover:not(:disabled) {
    border: 1px solid transparent;
    border-radius: 10px;
  }
  .chatrow.active .chat,
  .chatrow.active .chat:hover:not(:disabled) {
    border-color: var(--line2);
  }
  .ind {
    border-radius: 50%;
  }
  .acts,
  .gear {
    border-radius: 10px;
  }
  .pill {
    border-radius: 999px;
    padding: 6px 12px;
  }
  .pill.live i,
  .mg,
  .mg i {
    border-radius: 999px;
  }
  .ubub {
    padding: 10px 16px;
    border: 1px solid var(--accent-line);
    border-radius: 18px 18px 6px 18px;
  }
  .card {
    border-radius: var(--r-lg);
    box-shadow: none;
  }
  .card:hover:not(:disabled) {
    border-color: var(--accent-line);
    box-shadow: 0 8px 22px rgba(0, 0, 0, 0.35);
  }
  .note {
    max-width: 100%;
    text-align: center;
    border-radius: 14px;
    font: 12px/1.5 var(--sans);
    letter-spacing: 0;
    text-transform: none;
  }
  .jump {
    border-radius: 50%;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.4);
  }
  .composer,
  .composer:focus-within {
    border: 1px solid var(--line2);
    border-radius: 20px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
    transform: none;
  }
  .composer:focus-within {
    border-color: var(--accent-line);
    box-shadow:
      0 10px 30px rgba(0, 0, 0, 0.28),
      0 0 0 3px var(--accent-soft);
  }
  .agent {
    padding: 5px 12px;
    border-radius: 999px;
  }
  .seg {
    padding: 2px;
    border-radius: 999px;
  }
  .seg button,
  .seg button:hover:not(:disabled),
  .seg button.on,
  .seg button.on:hover:not(:disabled) {
    border-radius: 999px;
  }
  .seg button,
  .seg button:hover:not(:disabled) {
    box-shadow: none;
  }
  .seg button.on,
  .seg button.on:hover:not(:disabled) {
    box-shadow: 0 0 0 1px var(--line2) inset;
  }
  .round {
    border-radius: 50%;
  }
  .round.go:hover:not(:disabled),
  .round.stop:hover:not(:disabled) {
    box-shadow: none;
  }
  .sugg {
    border: 1px solid var(--line2);
    border-radius: var(--r-lg);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45);
  }
  .sugg button,
  .sugg button.on,
  .sugg button:hover:not(:disabled) {
    border: 0;
    border-radius: 9px;
  }
  /* appels MCP : une ligne repliable par appel */
  .mcpnote {
    display: inline-flex;
    align-self: center;
    align-items: center;
    gap: 7px;
    padding: 3px 12px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel);
    color: var(--faint);
    font-size: 12px;
    animation: fs-fade var(--t) both;
  }
  .mcpnote b {
    color: var(--dim);
    font-weight: 600;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
  }
  .mcpcall {
    margin-left: 42px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--panel);
    font-size: 12.5px;
    animation: fs-rise var(--t) var(--ease) both;
  }
  .mcpcall.failed {
    border-color: rgba(255, 107, 107, 0.4);
  }
  .mcpcall summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 12px;
    list-style: none;
    cursor: pointer;
  }
  .mcpcall summary::-webkit-details-marker {
    display: none;
  }
  .mcpcall.nobody summary {
    cursor: default;
  }
  .mcpcall.in_progress .dot {
    background: var(--accent);
    animation: fs-pulse 1.1s ease-in-out infinite;
  }
  .mcpcall.failed .dot {
    background: var(--danger);
  }
  .mname {
    min-width: 0;
    overflow: hidden;
    font-family: var(--mono);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msrv {
    padding: 1px 8px;
    border: 1px solid var(--accent-line);
    border-radius: 999px;
    background: var(--accent-soft);
    color: #cfe0ff;
    font-size: 11px;
  }
  .mst {
    margin-left: auto;
    color: var(--dim);
    font-size: 11px;
  }
  .mcpcall pre {
    max-height: 240px;
    margin: 0;
    padding: 8px 12px;
    overflow: auto;
    border-top: 1px solid var(--line);
    background: var(--bg);
    color: #c9d4e5;
    font: 12px/1.45 var(--mono);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .mcpcall.bad pre {
    color: #ffb4b4;
  }
  .mcpcall + .mcpcall {
    margin-top: -6px;
  }
  .mcpnote + .mcpnote {
    margin-top: -8px;
  }
  /* mode Code : accent vert d'eau, changement en douceur (voir @property dans theme.css) */
  .app {
    transition:
      --accent 0.6s var(--ease),
      --accent-soft 0.6s var(--ease),
      --accent-line 0.6s var(--ease),
      --accent-text 0.6s var(--ease),
      --accent-glow 0.6s var(--ease);
  }
  .app.code {
    --accent: #1fbf9f;
    --accent-soft: rgba(31, 191, 159, 0.12);
    --accent-line: rgba(31, 191, 159, 0.5);
    --accent-text: #c4fff0;
    --accent-glow: rgba(31, 191, 159, 0.3);
  }
  .app.code .top {
    box-shadow: inset 0 -1px 0 var(--accent-line);
  }
  .app.code .side {
    box-shadow: inset -1px 0 0 var(--accent-line);
  }
  /* lueur qui balaie l'écran à chaque changement de mode */
  .sweep {
    position: fixed;
    inset: 0;
    z-index: 30;
    pointer-events: none;
    background: radial-gradient(circle at 50% 100%, var(--accent-glow), transparent 55%);
    opacity: 0;
    transform-origin: 50% 100%;
    animation: fs-sweep 0.8s var(--ease) both;
  }
  @keyframes fs-sweep {
    0% {
      opacity: 0;
      transform: scale(0.7);
    }
    35% {
      opacity: 1;
    }
    100% {
      opacity: 0;
      transform: scale(1.5);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sweep {
      display: none;
    }
  }
  .modep {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent-text);
    font-family: var(--mono);
    font-size: 11.5px;
  }
  /* menu d'autocomplétion : commandes (/) et skills ($) */
  .sugg {
    max-height: 320px;
    overflow-y: auto;
  }
  .sugg b {
    flex: none;
    color: var(--accent-text);
    white-space: nowrap;
  }
  .sugg button:not(.cmd) b {
    color: #d8c2ff;
  }
  .stag {
    flex: none;
    margin-left: auto;
    padding: 0 8px;
    border: 1px solid var(--line2);
    border-radius: 999px;
    color: var(--faint);
    font-size: 11px;
    font-style: normal;
  }
  .msrv {
    color: var(--accent-text);
  }

  @media (max-width: 860px) {
    .app {
      grid-template-columns: 224px minmax(0, 1fr);
    }
    .cards {
      grid-template-columns: 1fr;
    }
    .help {
      display: none;
    }
  }
</style>
