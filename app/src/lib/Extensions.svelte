<script>
  // Tiroir « Extensions » : skills (locaux + compte) et serveurs MCP locaux.
  // Ajout / modification / retrait directement depuis l'interface.
  import { onMount } from 'svelte';
  import Bridge from './Bridge.svelte';
  import {
    getSettings,
    pickFolder,
    listSkills,
    skillRead,
    skillWrite,
    skillCreate,
    skillDelete,
    accountSkills,
    accountSkillUpdate,
    accountSkillDelete,
    mcpStatus,
    mcpGetConfig,
    mcpSaveConfig,
    mcpConnect,
    mcpDisconnect,
    mcpGetServer,
    mcpUpsert,
    mcpRemove,
    mcpImport,
    trustTool,
  } from './api.js';

  // `workdir` : dossier du mode Code (pour les skills du projet). `onchange` : optionnel.
  // `embedded` : affiché dans la fenêtre Réglages (le parent choisit l'onglet via `section`).
  // `oninsert(nom)` : si fourni, appelé à la place de l'écriture directe dans le composer.
  let { onclose = null, workdir = null, onchange, embedded = false, section = 'skills', oninsert } = $props();

  let root = $state();
  let tab = $state('skills');
  $effect(() => {
    if (embedded) tab = section;
  });
  // list | skill-edit | skill-new | acc-edit | mcp-form | mcp-import | mcp-json
  let view = $state('list');
  let savedWorkdir = $state(null);

  let local = $state([]);
  let account = $state([]);
  let accountError = $state('');
  let accountLoading = $state(false);
  let mcp = $state({ servers: [], error: null, customRejected: false });
  let msg = $state(null);
  let confirm = $state('');
  let busy = $state({});

  const dir = () => workdir ?? savedWorkdir ?? null;
  const err = (e) => String(e?.message ?? e);
  const safeDisplay = (s) =>
    String(s ?? '')
      .replace(/[A-Za-z]:\\Users\\[^\\\s"']+/gi, '%USERPROFILE%')
      .replace(/\/(?:home|Users)\/[^/\s"']+/g, '$HOME')
      .replace(/:\/\/([^:@/\s]+):[^@/\s]+@/g, '://$1:****@');
  const flash = (ok, text) => {
    msg = { ok, text };
  };

  function changed() {
    onchange?.({
      skills: local.length,
      mcp: mcp.servers.filter((s) => s.connected).length,
    });
    window.dispatchEvent(new CustomEvent('fs:extensions-changed'));
  }

  function back() {
    view = 'list';
    msg = null;
    confirm = '';
  }

  function onkey(e) {
    if (e.key !== 'Escape') return;
    if (view !== 'list') back();
    else if (!embedded) onclose?.();
  }

  // ---------- chargement ----------
  async function loadLocal() {
    try {
      local = await listSkills(dir());
    } catch (e) {
      flash(false, err(e));
    }
  }

  const accItems = (v) => {
    if (Array.isArray(v)) return v;
    for (const k of ['skills', 'items', 'data', 'results']) {
      if (Array.isArray(v?.[k])) return v[k];
    }
    return [];
  };

  async function loadAccount() {
    accountLoading = true;
    accountError = '';
    try {
      account = accItems(await accountSkills());
    } catch (e) {
      account = [];
      accountError = err(e);
    }
    accountLoading = false;
  }

  async function loadMcp() {
    try {
      mcp = await mcpStatus();
    } catch (e) {
      flash(false, err(e));
    }
  }

  onMount(async () => {
    try {
      const s = await getSettings();
      savedWorkdir = s?.lastWorkdir ?? null;
    } catch {
      /* sans importance */
    }
    await Promise.all([loadLocal(), loadMcp(), loadAccount()]);
  });

  // ---------- skills locaux ----------
  const SOURCES = { projet: 'PROJET', utilisateur: 'UTILISATEUR', app: 'APP', plugin: 'PLUGIN' };
  const clean = (s) =>
    String(s ?? '')
      .replace(/\\n/g, ' ')
      .replace(/\\"/g, '"')
      .replace(/\s+/g, ' ')
      .trim();

  // Écrit `$nom` dans le composer (le textarea qui n'est pas dans ce tiroir).
  function insertSkill(name) {
    if (oninsert) {
      oninsert(name);
      onclose?.();
      return;
    }
    const token = '$' + name;
    const tas = [...document.querySelectorAll('textarea')].filter((t) => !root?.contains(t));
    const ta = tas[tas.length - 1];
    if (!ta) {
      navigator.clipboard?.writeText(token);
      flash(true, `${token} copié dans le presse-papiers.`);
      return;
    }
    const sep = ta.value && !/\s$/.test(ta.value) ? ' ' : '';
    const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set;
    set.call(ta, ta.value + sep + token + ' ');
    ta.dispatchEvent(new Event('input', { bubbles: true }));
    ta.focus();
    flash(true, `${token} ajouté au message.`);
  }

  let editName = $state('');
  let editText = $state('');

  async function openEdit(s) {
    try {
      editText = await skillRead(s.name, dir());
      editName = s.name;
      view = 'skill-edit';
      msg = null;
    } catch (e) {
      flash(false, err(e));
    }
  }

  async function saveEdit() {
    try {
      await skillWrite(editName, editText, dir());
      view = 'list';
      await loadLocal();
      flash(true, `« ${editName} » enregistré.`);
      changed();
    } catch (e) {
      flash(false, err(e));
    }
  }

  let nName = $state('');
  let nDesc = $state('');
  let nBody = $state('');
  let nameInvalid = $derived(nName.trim().length > 0 && !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(nName.trim()));
  const fmtArgs = (args) => (args || []).map((a) => (/\s/.test(a) ? `"${a}"` : a)).join(' ');

  function openNew() {
    nName = '';
    nDesc = '';
    nBody = '';
    view = 'skill-new';
    msg = null;
  }

  async function createSkill() {
    try {
      await skillCreate(nName.trim(), nDesc, nBody, dir());
      const name = nName.trim();
      view = 'list';
      await loadLocal();
      flash(true, `« ${name} » créé.`);
      changed();
    } catch (e) {
      flash(false, err(e));
    }
  }

  async function removeSkill(s) {
    if (confirm !== 'sk:' + s.name) {
      confirm = 'sk:' + s.name;
      return;
    }
    confirm = '';
    try {
      await skillDelete(s.name, dir());
      await loadLocal();
      flash(true, `« ${s.name} » retiré.`);
      changed();
    } catch (e) {
      flash(false, err(e));
    }
  }

  // ---------- skills du compte ----------
  const accId = (s) => String(s?.id ?? s?.skillId ?? s?.slug ?? s?.name ?? '');
  let accEditId = $state('');
  let accText = $state('');

  function openAcc(s) {
    accEditId = accId(s);
    accText = JSON.stringify(s, null, 2);
    view = 'acc-edit';
    msg = null;
  }

  async function saveAcc() {
    let obj;
    try {
      obj = JSON.parse(accText);
    } catch (e) {
      flash(false, 'JSON invalide : ' + err(e));
      return;
    }
    for (const k of ['id', 'skillId', 'createdAt', 'updatedAt', 'userId']) delete obj[k];
    try {
      await accountSkillUpdate(accEditId, obj);
      view = 'list';
      await loadAccount();
      onchange?.();
      flash(true, 'Skill du compte mis à jour.');
    } catch (e) {
      flash(false, err(e) + ' (endpoint supposé : PATCH /skills/{id})');
    }
  }

  async function removeAcc(s) {
    const id = accId(s);
    if (confirm !== 'ac:' + id) {
      confirm = 'ac:' + id;
      return;
    }
    confirm = '';
    try {
      await accountSkillDelete(id);
      await loadAccount();
      onchange?.();
      flash(true, 'Skill du compte retiré.');
    } catch (e) {
      flash(false, err(e) + ' (endpoint supposé : DELETE /skills/{id})');
    }
  }

  // ---------- MCP ----------
  const PRESET_GROUPS = [
    {
      title: 'Boost IA & Dev',
      items: [
        {
          name: 'playwright',
          label: 'Playwright Web',
          runtime: 'npx',
          desc: 'Navigation web rapide par arbre d’accessibilité (@playwright/mcp).',
          kind: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@playwright/mcp@latest'],
        },
        {
          name: 'context7',
          label: 'Context7 Docs',
          runtime: 'npx',
          desc: 'Documentation à jour des frameworks & SDK (@upstash/context7-mcp).',
          kind: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@upstash/context7-mcp@latest'],
        },
        {
          name: 'fichiers',
          label: 'Fichiers locaux',
          runtime: 'npx',
          desc: 'Accès fichiers borné aux dossiers autorisés.',
          kind: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@modelcontextprotocol/server-filesystem'],
        },
        {
          name: 'memoire',
          label: 'Mémoire graphe',
          runtime: 'npx',
          desc: 'Graphe de connaissances persistant inter-sessions.',
          kind: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@modelcontextprotocol/server-memory'],
        },
        {
          name: 'thinking',
          label: 'Sequential Thinking',
          runtime: 'npx',
          desc: 'Raisonnement structuré étape par étape officiel MCP.',
          kind: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@modelcontextprotocol/server-sequential-thinking'],
        },
        {
          name: 'markitdown',
          label: 'MarkItDown PDF/Office',
          runtime: 'pip',
          desc: 'Conversion PDF, DOCX, PPTX, XLSX en Markdown (pip install markitdown-mcp).',
          kind: 'stdio',
          command: 'markitdown-mcp',
          args: [],
        },
        {
          name: 'windows-mcp',
          label: 'Windows UIA',
          runtime: 'uvx · Py 3.14+',
          desc: 'Automatisation native Windows UIA & DOM navigateur (CursorTouch/Windows-MCP).',
          kind: 'stdio',
          command: 'uvx',
          args: ['windows-mcp'],
        },
        {
          name: 'serena',
          label: 'Serena LSP',
          runtime: 'uvx',
          desc: 'Navigation symboles & refactoring sémantique multi-langages (oraios/serena).',
          kind: 'stdio',
          command: 'uvx',
          args: ['--from', 'git+https://github.com/oraios/serena', 'serena', 'start-mcp-server'],
        },
        {
          name: 'fetch',
          label: 'Web Fetch',
          runtime: 'uvx',
          desc: 'Récupération HTTP→Markdown officielle (modelcontextprotocol/servers).',
          kind: 'stdio',
          command: 'uvx',
          args: ['mcp-server-fetch'],
        },
        {
          name: 'git',
          label: 'Git',
          runtime: 'uvx',
          desc: 'Inspection & opérations Git officielles (mcp-server-git).',
          kind: 'stdio',
          command: 'uvx',
          args: ['mcp-server-git'],
        },
      ],
    },
    {
      title: 'Apps essentielles (Mail, Agenda, GitHub)',
      items: [
        {
          name: 'github',
          label: 'GitHub officiel',
          runtime: 'HTTP · OAuth/PAT',
          desc: 'Dépôts, issues, PRs & Actions via serveur MCP distant GitHub.',
          kind: 'http',
          url: 'https://api.githubcopilot.com/mcp/',
          token: '',
        },
        {
          name: 'google-calendar',
          label: 'Google Calendar',
          runtime: 'npx · OAuth',
          desc: 'Agenda Google Calendar (requiert GOOGLE_OAUTH_CREDENTIALS).',
          kind: 'stdio',
          command: 'cmd',
          args: ['/c', 'npx', '-y', '@cocal/google-calendar-mcp'],
          env: 'GOOGLE_OAUTH_CREDENTIALS=',
        },
        {
          name: 'google-workspace',
          label: 'Gmail + Calendar + Drive',
          runtime: 'uvx · OAuth',
          desc: 'Suite Google Workspace personnelle (taylorwilsdon/google_workspace_mcp).',
          kind: 'stdio',
          command: 'uvx',
          args: ['workspace-mcp', '--tool-tier', 'core'],
          env: 'GOOGLE_OAUTH_CLIENT_ID=\nGOOGLE_OAUTH_CLIENT_SECRET=',
        },
        {
          name: 'mail',
          label: 'Mail IMAP/SMTP',
          runtime: 'Rust · IMAP/SMTP',
          desc: 'Boîtes Gmail, Outlook ou universitaires via binaire mail-mcp.',
          kind: 'stdio',
          command: 'mail-mcp',
          args: [],
        },
      ],
    },
  ];

  const KINDS = [
    { id: 'stdio', label: 'Local (stdio)', hint: 'Lance une commande sur ce PC (npx, uvx, node, python…).' },
    { id: 'http', label: 'Distant (HTTP)', hint: 'Serveur HTTP « Streamable » (URL en /mcp).' },
    { id: 'sse', label: 'Distant (SSE)', hint: 'Serveur SSE (URL en /sse).' },
  ];
  const emptyForm = () => ({ orig: null, kind: 'stdio', name: '', command: '', args: '', env: '', cwd: '', url: '', token: '', headers: '', disabled: false });
  let f = $state(emptyForm());

  // Formulaire valide : nom + (commande en local | URL http(s) en distant).
  let formOk = $derived(
    !!f.name.trim() && (f.kind === 'stdio' ? !!f.command.trim() : /^https?:\/\//i.test(f.url.trim())),
  );

  function parseHeaders(text) {
    const o = {};
    for (const raw of text.split(/\r?\n/)) {
      const l = raw.trim();
      if (!l || l.startsWith('#')) continue;
      const i = l.indexOf(':');
      if (i < 1) throw new Error(`En-tête invalide : « ${l} » (attendu : Nom: valeur)`);
      o[l.slice(0, i).trim()] = l.slice(i + 1).trim();
    }
    return o;
  }

  function openServerForm(p = null) {
    const kind = p?.kind === 'http' || p?.kind === 'sse' ? p.kind : 'stdio';
    f = {
      ...emptyForm(),
      kind,
      name: p?.name ?? '',
      command: kind === 'stdio' ? (p?.command ?? '') : '',
      args: kind === 'stdio' ? (p?.args ?? []).join('\n') : '',
      env: kind === 'stdio' ? (p?.env ?? '') : '',
      url: kind !== 'stdio' ? (p?.url ?? '') : '',
      token: kind !== 'stdio' ? (p?.token ?? '') : '',
    };
    view = 'mcp-form';
    msg = p?.desc ? { ok: true, text: `Modèle pré-rempli : ${p.desc}` } : null;
  }

  async function editServer(s) {
    try {
      const c = await mcpGetServer(s.name);
      const k = (c.type ?? '').toLowerCase();
      const remote = !!(c.url || /^https?:\/\//i.test(c.command ?? ''));
      f = {
 ...emptyForm(),
        orig: s.name,
        kind: k === 'sse' ? 'sse' : k.includes('http') || remote ? 'http' : 'stdio',
        name: s.name,
        command: c.command ?? '',
        args: (c.args ?? []).join('\n'),
        env: Object.entries(c.env ?? {})
          .map(([k, v]) => `${k}=${v}`)
          .join('\n'),
 cwd: c.cwd ?? '',
 url: c.url ?? '',
        token: c.token ?? '',
        headers: Object.entries(c.headers ?? {})
          .map(([hk, hv]) => `${hk}: ${hv}`)
   .join('\n'),
        disabled: !!c.disabled,
      };
      view = 'mcp-form';
      msg = null;
    } catch (e) {
      flash(false, err(e));
    }
  }

  function parseEnv(text) {
    const o = {};
    for (const raw of text.split(/\r?\n/)) {
      const l = raw.trim();
      if (!l || l.startsWith('#')) continue;
      const i = l.indexOf('=');
      if (i < 1) throw new Error(`Variable invalide : « ${l} » (attendu : NOM=valeur)`);
      o[l.slice(0, i).trim()] = l.slice(i + 1).trim();
    }
    return o;
  }

  async function addDirArg() {
    const d = await pickFolder();
    if (d) f.args = (f.args.trim() ? f.args.replace(/\s+$/, '') + '\n' : '') + d;
  }

  async function connectServer(name) {
    busy[name] = true;
    msg = null;
    try {
      const n = await mcpConnect(name);
      flash(true, `« ${name} » connecté : ${n} outil(s).`);
    } catch (e) {
      flash(false, err(e));
    }
    busy[name] = false;
    await loadMcp();
    changed();
  }

  async function disconnectServer(name) {
    busy[name] = true;
    try {
      await mcpDisconnect(name);
    } catch (e) {
      flash(false, err(e));
    }
    busy[name] = false;
    await loadMcp();
    changed();
  }

  async function saveServer(connect) {
    const name = f.name.trim();
    try {
      const remote = f.kind !== 'stdio';
      const config = {
        command: remote ? '' : f.command.trim(),
        args: remote ? [] : f.args.split(/\r?\n/).map((a) => a.trim()).filter(Boolean),
        env: remote ? {} : parseEnv(f.env),
 cwd: remote ? null : f.cwd.trim() || null,
        url: remote ? f.url.trim() : null,
        token: remote ? f.token.trim() || null : null,
        headers: remote ? parseHeaders(f.headers) : {},
 type: f.kind === 'stdio' ? null : f.kind,
        disabled: f.disabled,
      };
      await mcpUpsert(f.orig, name, config);
    } catch (e) {
      flash(false, err(e));
      return;
    }
    view = 'list';
    await loadMcp();
    if (connect) await connectServer(name);
    else flash(true, `« ${name} » enregistré. Clique sur Connecter pour le (re)lancer.`);
    changed();
  }

  async function removeServer(s) {
    if (confirm !== 'mc:' + s.name) {
      confirm = 'mc:' + s.name;
      return;
    }
    confirm = '';
    try {
      await mcpRemove(s.name);
      await loadMcp();
      flash(true, `« ${s.name} » retiré de mcp.json.`);
      changed();
    } catch (e) {
      flash(false, err(e));
    }
  }

  const allTrusted = (s) => s.tools.length > 0 && s.tools.every((t) => t.trusted);

  async function setTrust(tools, on) {
    try {
      for (const t of tools) await trustTool(t.exposedName, on);
    } catch (e) {
      flash(false, err(e));
    }
    await loadMcp();
  }

  let importText = $state('');
  async function doImport() {
    try {
      const names = await mcpImport(importText);
      view = 'list';
      await loadMcp();
      flash(true, `Importé : ${names.join(', ')}. Clique sur Connecter pour les lancer.`);
      changed();
    } catch (e) {
      flash(false, err(e));
    }
  }

  let rawText = $state('');
  async function openRaw() {
    try {
      rawText = await mcpGetConfig();
      view = 'mcp-json';
      msg = null;
    } catch (e) {
      flash(false, err(e));
    }
  }
  async function saveRaw() {
    try {
      await mcpSaveConfig(rawText);
      view = 'list';
      await loadMcp();
      flash(true, 'mcp.json enregistré.');
      changed();
    } catch (e) {
      flash(false, err(e));
    }
  }

  const TITLES = {
    list: 'Extensions',
    'skill-edit': 'Modifier le skill',
    'skill-new': 'Nouveau skill',
    'acc-edit': 'Skill du compte',
    'mcp-form': 'Nouveau serveur MCP',
    'mcp-import': 'Importer du JSON',
    'mcp-json': 'mcp.json',
  };
  let title = $derived(view === 'mcp-form' && f.orig ? 'Modifier le serveur MCP' : TITLES[view] ?? 'Extensions');
</script>

<svelte:window onkeydown={onkey} />

<aside class="xt" class:embedded bind:this={root} aria-label="Extensions">
  {#if embedded && view === 'list'}
    <header class="sec-head sec-head-row">
      <div>
        <h2>{tab === 'skills' ? 'Skills' : 'MCP'}</h2>
        <p class="sec-sub">
          {#if tab === 'skills'}
            Skills locaux, skills du compte et insertion rapide avec <code>$nom</code>.
          {:else}
            Serveurs MCP locaux, autorisations par outil et configuration <code>mcp.json</code>.
          {/if}
        </p>
      </div>
      {#if onclose}
        <button type="button" class="head-x" onclick={() => onclose?.()} aria-label="Fermer" title="Fermer">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      {/if}
    </header>
  {:else if embedded}
    <header class="sec-head sub-head">
      <div class="sec-head-row">
        <div class="sub-title-row">
          <button type="button" class="icon" onclick={back} aria-label="Retour" title="Retour">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <polyline points="15 18 9 12 15 6" />
            </svg>
          </button>
          <h2>{title}</h2>
        </div>
        {#if onclose}
          <button type="button" class="head-x" onclick={() => onclose?.()} aria-label="Fermer" title="Fermer">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        {/if}
      </div>
      <p class="sec-sub">
        {#if view === 'skill-edit'}
          Édition directe du fichier <code>SKILL.md</code>.
        {:else if view === 'skill-new'}
          Création d’un skill local invoquable avec <code>$nom</code>.
        {:else if view === 'acc-edit'}
          Édition JSON du skill synchronisé sur ton compte.
        {:else if view === 'mcp-form'}
          Configuration d’un serveur MCP local et de ses variables.
        {:else if view === 'mcp-import'}
          Importation groupée depuis un extrait Claude Desktop.
        {:else if view === 'mcp-json'}
          Édition brute du fichier de configuration <code>mcp.json</code>.
        {/if}
      </p>
    </header>
  {:else}
    <header class="drawer-head">
      {#if view !== 'list'}
        <button type="button" class="icon" onclick={back} aria-label="Retour" title="Retour">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
      {/if}
      <h2>{title}</h2>
      {#if onclose}
        <button type="button" class="icon" onclick={() => onclose?.()} aria-label="Fermer" title="Fermer">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      {/if}
    </header>
  {/if}

  {#if view === 'list' && !embedded}
    <nav class="tabs">
      <button type="button" class:on={tab === 'skills'} onclick={() => (tab = 'skills')}>Skills</button>
      <button type="button" class:on={tab === 'mcp'} onclick={() => (tab = 'mcp')}>
        MCP{#if mcp.servers.length} ({mcp.servers.filter((s) => s.connected).length}/{mcp.servers.length}){/if}
      </button>
    </nav>
  {/if}

  {#if msg}
    <div class="msg" class:bad={!msg.ok} role="status">{msg.text}</div>
  {/if}

  <div class="body">
    <!-- ================= SKILLS ================= -->
    {#if view === 'list' && tab === 'skills'}
      {#if !embedded}
        <p class="hint">
          Écris <code>$nom</code> dans un message pour joindre un skill. Le modèle peut aussi en charger un lui-même.
        </p>
      {/if}
      <div class="bar top-bar">
        <button type="button" class="pri" onclick={openNew}>+ Nouveau skill</button>
        <button type="button" onclick={() => { loadLocal(); loadAccount(); }}>Actualiser</button>
      </div>

      <h3>Locaux et intégrés ({local.length})</h3>
      {#if local.length === 0}
        <p class="hint">Aucun skill local. Crée-en un ou ajoute un dossier avec un <code>SKILL.md</code>.</p>
      {/if}
      {#each local as s (s.name)}
        <section class="card">
          <div class="row">
            <strong>{s.name}</strong>
            <span class="badge {s.source}">{SOURCES[s.source] ?? s.source}</span>
          </div>
          <p class="desc">{clean(s.description) || 'Sans description.'}</p>
          {#if confirm === 'sk:' + s.name}
            <p class="warn">
              Supprime définitivement le dossier <code>{s.path}</code>
              {#if s.source === 'app'} (intégré au dossier de l’app){:else if s.source === 'utilisateur'} — il peut être partagé avec d’autres outils{/if}.
            </p>
            <div class="row card-actions">
              <button type="button" class="danger" onclick={() => removeSkill(s)}>Confirmer la suppression</button>
              <button type="button" onclick={() => (confirm = '')}>Annuler</button>
            </div>
          {:else}
            <div class="row card-actions">
              <button type="button" onclick={() => insertSkill(s.name)}>Insérer</button>
              <button type="button" onclick={() => openEdit(s)}>Modifier</button>
              <button type="button" class="ghostdanger push-right" onclick={() => removeSkill(s)}>Retirer</button>
            </div>
          {/if}
        </section>
      {/each}

      <h3>Compte France Student ({account.length}) {#if accountLoading}<span class="mut">…</span>{/if}</h3>
      {#if accountError}
        <p class="warn">Liste indisponible : {accountError}</p>
      {:else if !accountLoading && account.length === 0}
        <p class="hint">Aucun skill sur le compte.</p>
      {/if}
      {#each account as s, i (accId(s) || i)}
        <section class="card">
          <div class="row">
            <strong>{s.name ?? s.title ?? accId(s)}</strong>
            <span class="badge account">COMPTE</span>
          </div>
          <p class="desc">{clean(s.description) || 'Sans description.'}</p>
          {#if confirm === 'ac:' + accId(s)}
            <p class="warn">Retire ce skill de ton compte France Student (définitif).</p>
            <div class="row card-actions">
              <button type="button" class="danger" onclick={() => removeAcc(s)}>Confirmer la suppression</button>
              <button type="button" onclick={() => (confirm = '')}>Annuler</button>
            </div>
          {:else}
            <div class="row card-actions">
              <button type="button" onclick={() => insertSkill(s.name ?? accId(s))}>Insérer</button>
              <button type="button" onclick={() => openAcc(s)}>Modifier</button>
              <button type="button" class="ghostdanger push-right" onclick={() => removeAcc(s)}>Retirer</button>
            </div>
          {/if}
        </section>
      {/each}

    {:else if view === 'skill-new'}
      <section class="card form-card">
        <p class="hint">
          Enregistré dans <code>skills/&lt;nom&gt;/SKILL.md</code> (badge <strong>APP</strong>).
        </p>
        <label>
          <span>Nom <span class="req">*</span></span>
          <input bind:value={nName} placeholder="mon-skill" spellcheck="false" aria-invalid={nameInvalid} />
          <span class="field-hint" class:err-txt={nameInvalid}>Minuscules, chiffres et tirets uniquement (ex.&nbsp;<code>mon-skill</code>).</span>
        </label>
        <label>
          <span>Description (quand l’utiliser)</span>
          <input bind:value={nDesc} placeholder="Quand je veux résumer un cours en fiches…" />
        </label>
        <label>
          <span>Instructions <span class="req">*</span></span>
          <textarea bind:value={nBody} rows="8" placeholder="Explique au modèle comment faire…"></textarea>
        </label>
        <div class="bar form-bar">
          <button type="button" class="pri" onclick={createSkill} disabled={!nName.trim() || nameInvalid || !nBody.trim()}>Créer</button>
          <button type="button" class="ghost" onclick={back}>Annuler</button>
        </div>
      </section>

    {:else if view === 'skill-edit'}
      <section class="card form-card">
        <p class="hint">
          Fichier <code>SKILL.md</code> complet (en-tête <code>---</code> compris) de <strong>{editName}</strong>. Si tu changes <code>name:</code>, le skill change de nom.
        </p>
        <textarea class="code tall" bind:value={editText} spellcheck="false"></textarea>
        <div class="bar form-bar">
          <button type="button" class="pri" onclick={saveEdit} disabled={!editText.trim()}>Enregistrer</button>
          <button type="button" class="ghost" onclick={back}>Annuler</button>
        </div>
      </section>

    {:else if view === 'acc-edit'}
      <section class="card form-card">
        <p class="hint">
          Données du skill, en JSON. Enregistrer envoie <code>PATCH /skills/{accEditId}</code>.
        </p>
        <textarea class="code tall" bind:value={accText} spellcheck="false"></textarea>
        <div class="bar form-bar">
          <button type="button" class="pri" onclick={saveAcc}>Enregistrer</button>
          <button type="button" class="ghost" onclick={back}>Annuler</button>
        </div>
      </section>

    <!-- ================= MCP ================= -->
    {:else if view === 'list' && tab === 'mcp'}
      <p class="hint">
        Aucun serveur ne démarre seul. Chaque appel d’outil MCP demande confirmation, sauf <span class="nowrap">«&#8239;sans confirmation&#8239;»</span> <span class="nowrap">(session seulement).</span>
      </p>
      <Bridge inline />
      {#if mcp.error}<p class="warn">{mcp.error}</p>{/if}
      {#if mcp.customRejected}
        <p class="warn">Le serveur France Student refuse les outils personnalisés : les MCP ne seront pas utilisables par le modèle.</p>
      {/if}
      <div class="bar top-bar">
        <button type="button" class="pri" onclick={() => openServerForm()}>+ Ajouter un serveur</button>
        <button type="button" onclick={() => { importText = ''; view = 'mcp-import'; msg = null; }}>Importer du JSON</button>
        <button type="button" onclick={loadMcp}>Actualiser</button>
      </div>

      {#if mcp.servers.length > 0}
        <h3>Serveurs ({mcp.servers.length})</h3>
      {/if}

      <details class="presets-fold" open={mcp.servers.length === 0}>
        <summary class="tools-sum">
          <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polyline points="9 18 15 12 9 6" />
          </svg>
          Catalogue de modèles MCP vérifiés (Boost IA &amp; Apps)
        </summary>
        {#each PRESET_GROUPS as group (group.title)}
          <div class="preset-group">
            <h4>{group.title}</h4>
            <div class="preset-grid">
              {#each group.items as p (p.name)}
                <button type="button" class="preset-card" onclick={() => openServerForm(p)}>
                  <span class="preset-head">
                    <strong>{p.label}</strong>
                    <span class="badge">{p.runtime}</span>
                  </span>
                  <span class="preset-desc">{p.desc}</span>
                </button>
              {/each}
            </div>
          </div>
        {/each}
        <p class="hint">Les modèles <code>npx</code> fonctionnent dès que Node.js est installé ; les autres nécessitent <code>uv</code> (<code>uvx</code>), <code>pip</code> ou le binaire indiqué.</p>
      </details>

      {#each mcp.servers as s (s.name)}
        <section class="card">
          <div class="row">
            <span class="dot" class:up={s.connected} aria-hidden="true"></span>
            <strong>{s.name}</strong>
            <span class="mut">
              {s.connected ? `connecté · ${s.tools.length} outil${s.tools.length > 1 ? 's' : ''}` : 'arrêté'}
            </span>
          </div>
          <p class="desc mono cmd-line" title={safeDisplay(`${s.command} ${fmtArgs(s.args)}`)}>{safeDisplay(`${s.command} ${fmtArgs(s.args)}`)}</p>
          {#if confirm === 'mc:' + s.name}
            <p class="warn">Retire « {s.name} » de <code>mcp.json</code>.</p>
            <div class="row card-actions">
              <button type="button" class="danger" onclick={() => removeServer(s)}>Confirmer</button>
              <button type="button" onclick={() => (confirm = '')}>Annuler</button>
            </div>
          {:else}
            <div class="row card-actions">
              {#if s.connected}
                <button type="button" disabled={busy[s.name]} onclick={() => disconnectServer(s.name)}>Déconnecter</button>
              {:else}
                <button type="button" class="pri" disabled={busy[s.name]} onclick={() => connectServer(s.name)}>
                  {busy[s.name] ? 'Connexion…' : 'Connecter'}
                </button>
              {/if}
              <button type="button" onclick={() => editServer(s)}>Modifier</button>
              <button type="button" class="ghostdanger push-right" onclick={() => removeServer(s)}>Retirer</button>
            </div>
          {/if}
          {#if s.connected && s.tools.length}
            <label class="chk" class:chk-danger={allTrusted(s)}>
              <input type="checkbox" checked={allTrusted(s)} onchange={(e) => setTrust(s.tools, e.currentTarget.checked)} />
              Sans confirmation (cette session)
            </label>
            <details>
              <summary class="tools-sum">
                <svg class="chev" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 2.5 8 6 4 9.5" /></svg>
                <span>Outils ({s.tools.length})</span>
              </summary>
              {#each s.tools as t (t.exposedName)}
                <label class="chk tool" title={t.description}>
                  <input type="checkbox" checked={t.trusted} onchange={(e) => setTrust([t], e.currentTarget.checked)} />
                  <span>{t.name}{#if t.readOnly} <em>(lecture)</em>{/if}</span>
                </label>
              {/each}
            </details>
          {/if}
        </section>
      {/each}

      <h3>Configuration</h3>
      <div class="bar">
        <button type="button" onclick={openRaw}>Modifier mcp.json</button>
      </div>

    {:else if view === 'mcp-form'}
      <section class="card form-card mcp-form-card">
        <p class="hint">
          {#if f.orig}Enregistrer déconnecte le serveur&#8239;; reconnecte-le ensuite.{:else}Écrit dans <code>mcp.json</code>&#8239;; démarre après un clic sur «&#8239;Connecter&#8239;» ou «&#8239;Enregistrer et connecter&#8239;».{/if}
        </p>
        <div class="kind-tabs" role="tablist" aria-label="Type de serveur">
          {#each KINDS as k}
            <button type="button" role="tab" class="kind-tab" class:on={f.kind === k.id} aria-selected={f.kind === k.id} onclick={() => (f.kind = k.id)}>{k.label}</button>
          {/each}
        </div>
 <p class="hint">{KINDS.find((k) => k.id === f.kind)?.hint}</p>
 <label><span>Nom <span class="req">*</span></span><input bind:value={f.name} placeholder="fichiers" spellcheck="false" /></label>
        {#if f.kind === 'stdio'}
   <label><span>Commande <span class="req">*</span></span><input class="mono" bind:value={f.command} placeholder="npx" spellcheck="false" /></label>
        <label>
          <span>Arguments (un par ligne)</span>
          <textarea class="code" bind:value={f.args} rows="2" spellcheck="false" placeholder={'-y @modelcontextprotocol/server-filesystem'}></textarea>
        </label>
        <div class="bar compact-bar">
          <button type="button" class="with-ico" onclick={addDirArg}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            Ajouter un dossier aux arguments…
          </button>
        </div>
        <label>
          <span>Variables d’environnement (<code>NOM=valeur</code>, une par ligne)</span>
          <textarea class="code" bind:value={f.env} rows="2" spellcheck="false" placeholder="API_KEY=…"></textarea>
        </label>
          <label><span>Dossier de lancement</span><input class="mono" bind:value={f.cwd} placeholder={'C:\\Projet'} spellcheck="false" /></label>
 {:else}
   <label><span>URL <span class="req">*</span></span><input class="mono" bind:value={f.url} placeholder={f.kind === 'sse' ? 'https://exemple.com/sse' : 'https://exemple.com/mcp'} spellcheck="false" /></label>
   <label><span>Jeton Bearer (optionnel)</span><input class="mono" type="password" bind:value={f.token} placeholder={'${MON_JETON} ou jeton'} spellcheck="false" autocomplete="off" /></label>
          <label>
     <span>En-têtes (<code>Nom: valeur</code>, un par ligne)</span>
     <textarea class="code" bind:value={f.headers} rows="2" spellcheck="false" placeholder="X-Api-Key: …"></textarea>
          </label>
        {/if}
        <p class="hint">Les variables <code>{'${NOM}'}</code> sont remplacées par l’environnement à la connexion.</p>
        <label class="check"><input type="checkbox" bind:checked={f.disabled} /> <span>Désactivé (jamais lancé ni contacté)</span></label>
        <div class="bar form-bar">
   <button type="button" class="pri" onclick={() => saveServer(true)} disabled={!formOk || f.disabled}>
     Enregistrer et connecter
   </button>
          <button type="button" class="sec" onclick={() => saveServer(false)} disabled={!formOk}>Enregistrer</button>
          <button type="button" class="ghost" onclick={back}>Annuler</button>
        </div>
      </section>

    {:else if view === 'mcp-import'}
      <section class="card form-card">
        <p class="hint">
          Colle un extrait au format Claude Desktop&#8239;: <code class="wrap-code">{'{ "mcpServers": { "nom": { "command": …, "args": […] } } }'}</code>. Les noms déjà présents sont refusés.
        </p>
        <textarea class="code tall" bind:value={importText} spellcheck="false" placeholder={'{\n  "mcpServers": {\n    …\n  }\n}'}></textarea>
        <div class="bar form-bar">
          <button type="button" class="pri" onclick={doImport} disabled={!importText.trim()}>Importer</button>
          <button type="button" class="ghost" onclick={back}>Annuler</button>
        </div>
      </section>

    {:else if view === 'mcp-json'}
      <section class="card form-card">
        <p class="hint">Édition directe (avancé). Le JSON est validé avant l’enregistrement.</p>
        <textarea class="code tall" bind:value={rawText} spellcheck="false"></textarea>
        <div class="bar form-bar">
          <button type="button" class="pri" onclick={saveRaw}>Enregistrer</button>
          <button type="button" class="ghost" onclick={back}>Annuler</button>
        </div>
      </section>
    {/if}
  </div>
</aside>

<style>
  .xt {
    --xt-bg: var(--bg, #141821);
    --xt-card: var(--panel, #1a1f2b);
    --xt-line: var(--line, #2a303d);
    --xt-line2: var(--line2, #384152);
    --xt-txt: var(--text, #d9dde6);
    --xt-mut: var(--dim, #8a92a3);
    --xt-acc: var(--accent, #3d7bff);
    --xt-bad: var(--danger, #ff5d6c);
    --xt-ok: var(--ok, #3ecf8e);
    --xt-warn: var(--warn, #e3b341);
    --xt-input: var(--code-bg, #0f131a);
    --xt-panel2: var(--panel2, #232938);
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(440px, 100vw);
    z-index: 60;
    display: flex;
    flex-direction: column;
    background: var(--xt-bg);
    color: var(--xt-txt);
    border-left: 1px solid var(--xt-line);
    border-top: 2px solid var(--xt-acc);
    font-size: 13px;
    box-shadow: none;
  }
  .xt.embedded {
    position: static;
    width: auto;
    height: auto;
    z-index: auto;
    border: 0;
    box-shadow: none;
    background: transparent;
  }
  .drawer-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--xt-line);
  }
  .drawer-head h2 {
    flex: 1;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .sec-head {
    margin-bottom: 20px;
  }
  .sec-head-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .sub-head {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 20px;
  }
  .sub-head .sec-head-row {
    width: 100%;
    align-items: center;
  }
  .sub-title-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .sec-head h2 {
    margin: 0 0 6px;
    font-size: 20px;
    font-weight: 600;
    color: var(--xt-txt);
  }
  .sub-head h2 {
    margin: 0;
  }
  .sec-sub {
    margin: 0;
    font-size: 13px;
    color: var(--xt-mut);
  }
  .head-x {
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--xt-txt);
    opacity: 0.72;
    display: inline-grid;
    place-items: center;
    cursor: pointer;
    flex-shrink: 0;
  }
  .head-x:hover {
    opacity: 1;
    background: var(--xt-bg2);
    border-color: var(--xt-line);
  }
  h3 {
    margin: 20px 0 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--xt-mut);
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 0 14px;
    border-bottom: 1px solid var(--xt-line);
  }
  .tabs button {
    background: none;
    height: auto;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    padding: 10px 12px;
    color: var(--xt-mut);
    cursor: pointer;
  }
  .tabs button.on {
    color: var(--xt-txt);
    border-bottom-color: var(--xt-acc);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 14px 24px;
  }
  .xt.embedded .body {
    padding: 0 0 28px;
    overflow: visible;
  }
  .msg {
    margin: 10px 14px 0;
    padding: 9px 12px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, var(--xt-ok) 45%, transparent);
    background: color-mix(in srgb, var(--xt-ok) 10%, transparent);
    color: var(--xt-txt);
    word-break: break-word;
  }
  .xt.embedded .msg {
    margin: 0 0 12px;
  }
  .msg.bad {
    border-color: color-mix(in srgb, var(--xt-bad) 45%, transparent);
    background: color-mix(in srgb, var(--xt-bad) 10%, transparent);
    color: var(--xt-bad);
  }
  .hint {
    color: var(--xt-mut);
    line-height: 1.5;
    margin: 0 0 12px;
  }
  .warn {
    color: var(--xt-warn);
    line-height: 1.45;
    margin: 6px 0;
    word-break: break-word;
  }
  .mut {
    color: var(--xt-mut);
    font-size: 12px;
  }
  .bar,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 8px 0;
  }
  .top-bar {
    margin: 0 0 14px;
  }
  .card {
    background: var(--xt-card);
    border: 1px solid var(--xt-line);
    border-radius: 12px;
    padding: 15px 17px;
    margin-bottom: 10px;
  }
  .form-card {
    padding: 16px 18px;
  }
  .mcp-form-card .hint {
    margin-bottom: 10px;
    font-size: 12.5px;
  }
  .mcp-form-card label {
    margin: 0 0 9px;
  }
  .form-grid-2 {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }
  .compact-bar {
    margin: -2px 0 9px;
  }
  .kind-tabs {
    display: flex;
    gap: 6px;
    margin: 0 0 8px;
  }
  .kind-tab {
    flex: 1;
    padding: 7px 10px;
    border: 1px solid var(--xt-line2);
    border-radius: 8px;
    background: transparent;
    color: var(--xt-mut);
    cursor: pointer;
  }
  .kind-tab.on {
    border-color: var(--xt-acc);
    color: var(--xt-txt);
    background: color-mix(in srgb, var(--xt-acc) 14%, transparent);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0;
  }
  .check input {
    width: auto;
  }
  .nowrap {
    white-space: nowrap;
  }
  .form-bar {
    margin: 12px 0 0;
  }
  .mcp-form-card .form-bar {
    margin: 8px 0 0;
  }
  .card .row {
    margin: 2px 0;
  }
  .card .card-actions {
    margin: 10px 0 0;
  }
  .desc {
    margin: 6px 0 2px;
    color: var(--xt-mut);
    font-size: 12.5px;
    line-height: 1.45;
    overflow-wrap: anywhere;
  }
  .cmd-line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
  }
  .mono,
  code,
  .code {
    font-family: var(--font-mono, Consolas, 'Cascadia Mono', monospace);
  }
  code {
    font-size: 12px;
    color: var(--xt-txt);
    background: var(--xt-panel2);
    border: 1px solid var(--xt-line);
    border-radius: 5px;
    padding: 1px 5px;
    white-space: nowrap;
  }
  code.wrap-code {
    white-space: normal;
    word-break: break-word;
  }
  .badge {
    font-family: var(--font-sans, inherit);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    padding: 2px 7px;
    border: 1px solid color-mix(in srgb, var(--xt-acc) 45%, var(--xt-line2));
    background: color-mix(in srgb, var(--xt-acc) 12%, transparent);
    color: var(--xt-acc);
    border-radius: 999px;
  }
  .badge.utilisateur {
    border-color: color-mix(in srgb, #8b5cf6 45%, var(--xt-line2));
    background: color-mix(in srgb, #8b5cf6 12%, transparent);
    color: color-mix(in srgb, #a78bfa 85%, var(--xt-txt));
  }
  .badge.projet {
    border-color: color-mix(in srgb, var(--xt-ok) 45%, var(--xt-line2));
    background: color-mix(in srgb, var(--xt-ok) 12%, transparent);
    color: var(--xt-ok);
  }
  .badge.account {
    border-color: var(--xt-line2);
    background: var(--xt-panel2);
    color: var(--xt-txt);
    opacity: 0.88;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--faint, #555d6e);
    flex-shrink: 0;
  }
  .dot.up {
    background: var(--xt-ok);
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 36px;
    padding: 0 14px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--xt-txt);
    background: var(--xt-panel2);
    border: 1px solid var(--xt-line2);
    border-radius: 8px;
    cursor: pointer;
    transition: border-color 0.14s, background 0.14s, color 0.14s;
  }
  button:hover:not(:disabled) {
    border-color: var(--xt-line2);
    background: var(--hover, var(--xt-panel2));
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  button.pri {
    background: var(--xt-acc);
    border-color: var(--xt-acc);
    color: var(--on-accent, #ffffff);
    font-weight: 600;
  }
  button.pri:disabled {
    opacity: 1;
    background: var(--xt-panel2);
    border-color: var(--xt-line2);
    color: var(--xt-mut);
  }
  button.pri:hover:not(:disabled) {
    filter: brightness(1.08);
    border-color: var(--xt-acc);
  }
  button.sec {
    background: var(--xt-panel2);
    border-color: var(--xt-line2);
    color: var(--xt-txt);
  }
  button.ghost {
    background: transparent;
    border-color: transparent;
    color: var(--xt-mut);
  }
  button.ghost:hover:not(:disabled) {
    background: var(--hover, var(--xt-panel2));
    border-color: transparent;
    color: var(--xt-txt);
  }
  button.danger {
    background: var(--xt-bad);
    border-color: var(--xt-bad);
    color: #fff;
    font-weight: 600;
  }
  button.ghostdanger {
    background: transparent;
    color: color-mix(in srgb, var(--xt-bad) 82%, var(--xt-txt));
  }
  button.ghostdanger:hover:not(:disabled) {
    border-color: var(--xt-bad);
    background: color-mix(in srgb, var(--xt-bad) 10%, transparent);
    color: var(--xt-bad);
  }
  .push-right {
    margin-left: auto;
  }
  button.icon {
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 8px;
    background: transparent;
    border: 1px solid var(--xt-line);
    color: var(--xt-txt);
    display: inline-grid;
    place-items: center;
    flex-shrink: 0;
  }
  button.icon:hover {
    background: var(--hover, var(--xt-panel2));
    border-color: var(--xt-line2);
  }
  label {
    display: block;
    margin: 0 0 14px;
    color: var(--xt-txt);
    font-size: 13px;
    font-weight: 500;
  }
  .req {
    color: var(--xt-bad);
  }
  .field-hint {
    display: block;
    margin-top: 5px;
    font-size: 11.5px;
    font-weight: 400;
    color: var(--xt-mut);
  }
  .field-hint.err-txt {
    color: var(--xt-bad);
  }
  label input:not([type='checkbox']),
  label textarea,
  textarea {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin-top: 6px;
    padding: 8px 11px;
    font: inherit;
    font-weight: 400;
    color: var(--xt-txt);
    background: var(--xt-input);
    border: 1px solid var(--xt-line2);
    border-radius: 8px;
    resize: none;
  }
  label input.mono:not([type='checkbox']),
  label input.mono:not([type='checkbox'])::placeholder,
  label textarea.code,
  textarea.code {
    font-family: var(--font-mono, Consolas, 'Cascadia Mono', monospace);
    font-size: 12.5px;
  }
  label input:not([type='checkbox']) {
    height: 36px;
    padding: 0 11px;
  }
  input[aria-invalid='true'] {
    border-color: var(--xt-bad);
  }
  textarea.code {
    font-size: 12px;
    line-height: 1.45;
  }
  textarea.tall {
    min-height: 48vh;
  }
  input:focus,
  textarea:focus {
    outline: none;
    border-color: var(--xt-acc);
  }
  label.chk {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 10px 0 0;
    font-size: 12.5px;
    font-weight: 400;
    color: var(--xt-txt);
    cursor: pointer;
  }
  label.chk-danger {
    padding: 7px 10px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, var(--xt-warn) 40%, transparent);
    background: color-mix(in srgb, var(--xt-warn) 10%, transparent);
    color: var(--xt-warn);
  }
  label.chk.tool {
    margin: 6px 0 0 18px;
    color: var(--xt-mut);
  }
  details {
    margin-top: 10px;
  }
  summary.tools-sum {
    display: flex;
    align-items: center;
    gap: 7px;
    list-style: none;
    cursor: pointer;
    color: var(--xt-mut);
    font-size: 12.5px;
  }
  summary.tools-sum::-webkit-details-marker {
    display: none;
  }
  summary.tools-sum .chev {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    transition: transform 0.15s ease;
  }
  details[open] > summary.tools-sum .chev {
    transform: rotate(90deg);
  }
  .presets-fold {
    margin: 6px 0 12px;
    padding: 10px 12px;
    background: var(--xt-card);
    border: 1px solid var(--xt-line);
    border-radius: 10px;
  }
  .preset-group {
    margin-top: 10px;
  }
  .preset-group h4 {
    margin: 0 0 7px;
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--xt-mut);
  }
  .preset-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 8px;
  }
  button.preset-card {
    height: auto;
    min-height: 58px;
    padding: 9px 11px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: flex-start;
    text-align: left;
    gap: 4px;
    background: var(--xt-panel2);
    border: 1px solid var(--xt-line);
    border-radius: 9px;
  }
  button.preset-card:hover {
    border-color: var(--xt-acc);
    background: color-mix(in srgb, var(--xt-acc) 8%, var(--xt-panel2));
  }
  .preset-head {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    font-size: 12.5px;
    color: var(--xt-txt);
  }
  .preset-desc {
    font-size: 11.5px;
    font-weight: 400;
    line-height: 1.35;
    color: var(--xt-mut);
    white-space: normal;
  }
</style>
