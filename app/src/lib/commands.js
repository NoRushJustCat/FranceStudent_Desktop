// Autocomplétion de la zone de saisie : commandes « / » et skills « $ ».
// Module pur (aucun état Svelte) : App.svelte lui passe les actions dont il a besoin.

// minuscules sans accents : « Réglages » == « reglages »
export const fold = (s) =>
  String(s ?? '')
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '');

const hit = (q, ...names) => !q || names.some((n) => fold(n).includes(q));
// 0 = identique, 1 = commence par, 2 = contient
const rank = (q, ...names) =>
  names.some((n) => fold(n) === q) ? 0 : names.some((n) => fold(n).startsWith(q)) ? 1 : 2;

const TAGS = { projet: 'projet', utilisateur: 'perso', app: 'app' };

// Onglets des réglages (ceux de Settings.svelte)
export const TABS = [
  { value: 'compte', desc: 'Compte et budget' },
  { value: 'code', desc: 'Mode Code' },
  { value: 'eco', desc: 'Économie et contexte' },
  { value: 'skills', desc: 'Skills' },
  { value: 'mcp', desc: 'Serveurs MCP' },
  { value: 'diag', desc: 'Diagnostic' },
];
const ONOFF = [
  { value: 'on', desc: 'Activer' },
  { value: 'off', desc: 'Désactiver' },
];

// « on » / « off » ; sans argument (ou argument inconnu) : bascule
function flag(a, now) {
  const f = fold(a).trim();
  if (['on', 'oui', '1', 'true', 'activer'].includes(f)) return true;
  if (['off', 'non', '0', 'false', 'desactiver'].includes(f)) return false;
  return !now;
}

// Commandes locales : jamais envoyées à l'IA.
//   args : liste de valeurs proposées après la commande (facultatif)
//   free : la commande attend un texte libre (ex. un titre)
//   run(arg) : exécute ; `h` = actions fournies par App.svelte
export function makeCommands(h) {
  return [
    { name: 'nouveau', alias: ['new', 'clear', 'effacer'], desc: 'Nouveau chat', run: () => h.newChat() },
    {
      name: 'chat',
      alias: [],
      desc: 'Passer en mode Chat',
      run: () => (h.busy() ? h.say('Attends la fin de la génération.') : h.setMode('chat')),
    },
    {
      name: 'code',
      alias: [],
      desc: 'Passer en mode Code',
      run: () => (h.busy() ? h.say('Attends la fin de la génération.') : h.setMode('code')),
    },
    {
      name: 'dossier',
      alias: ['cd', 'folder'],
      desc: 'Choisir le dossier de travail',
      run: () => (h.busy() ? h.say('Attends la fin de la génération.') : h.chooseFolder()),
    },
    {
      name: 'eco',
      alias: [],
      desc: 'Mode économie (on / off)',
      args: () => ONOFF,
      run: (a) => {
        const v = flag(a, h.eco());
        h.setEco(v);
        h.say(v ? 'Mode économie activé' : 'Mode économie désactivé');
      },
    },
    {
      name: 'auto',
      alias: ['accept'],
      desc: 'Accepter auto les modifs de fichiers (on / off)',
      args: () => ONOFF,
      run: (a) => {
        const v = flag(a, h.auto());
        h.setAuto(v);
        h.say(v ? 'Modifs acceptées automatiquement' : 'Modifs : confirmation demandée');
      },
    },
    {
      name: 'agent',
      alias: [],
      desc: 'Changer d’agent',
      args: () => h.agents().map((x) => ({ value: String(x.name ?? x.id), desc: '' })),
      run: (a) => {
        if (h.busy()) return h.say('Attends la fin de la génération.');
        const q = fold(a).trim();
        const name = (x) => fold(x.name ?? x.id);
        const ag = q && (h.agents().find((x) => name(x) === q) ?? h.agents().find((x) => name(x).includes(q)));
        if (!ag) return h.say(q ? 'Agent introuvable : ' + a : 'Usage : /agent nom');
        h.pickAgent(ag);
        h.say('Agent : ' + (ag.name ?? ag.id));
      },
    },
    {
      name: 'compacter',
      alias: ['compact'],
      desc: 'Compacter le contexte du chat',
      run: async () => {
        if (!h.chatId()) return h.say('Aucun chat à compacter.');
        if (await h.compact()) h.say('Contexte compacté');
      },
    },
    {
      name: 'renommer',
      alias: ['rename'],
      free: true,
      desc: 'Renommer le chat : /renommer titre',
      run: async (a) => {
        if (!h.chatId()) return h.say('Envoie d’abord un message.');
        if (!a) return h.say('Usage : /renommer nouveau titre');
        await h.rename(a);
        h.say('Chat renommé');
      },
    },
    {
      name: 'copier',
      alias: ['copy'],
      desc: 'Copier la dernière réponse',
      run: async () => {
        const t = h.lastAnswer();
        if (!t) return h.say('Rien à copier.');
        await navigator.clipboard.writeText(t);
        h.say('Dernière réponse copiée');
      },
    },
    { name: 'stop', alias: ['annuler', 'arreter'], desc: 'Arrêter la génération', run: () => h.stop() },
    {
      name: 'reglages',
      alias: ['settings', 'options'],
      desc: 'Ouvrir les réglages',
      args: () => TABS,
      run: (a) => h.openSettings(TABS.find((t) => t.value === fold(a).trim())?.value ?? 'compte'),
    },
    { name: 'skills', alias: [], desc: 'Gérer les skills (importer, créer…)', run: () => h.openSettings('skills') },
    { name: 'mcp', alias: [], desc: 'Serveurs MCP', run: () => h.openSettings('mcp') },
    { name: 'aide', alias: ['help', '?'], desc: 'Lister toutes les commandes', run: () => h.help() },
  ];
}

export function findCommand(commands, name) {
  const n = fold(name);
  return commands.find((c) => c.name === n || c.alias.includes(n));
}

// skills locaux (projet, perso, app : y compris ceux importés)
function skillItems(skills, q, text) {
  return skills
    .filter((s) => hit(q, s.name))
    .sort((a, b) => rank(q, a.name) - rank(q, b.name))
    .map((s) => ({
      id: 's:' + s.name,
      label: '$' + s.name,
      desc: s.description ?? '',
      tag: TAGS[s.source] ?? s.source ?? 'skill',
      text: text(s.name),
      final: false,
      exact: false,
    }));
}

// Suggestions pour le texte saisi : [{id, label, desc, tag, text, final, exact}]
//   text  = nouveau contenu de la zone de saisie si on choisit l'entrée
//   final = l'entrée complète une commande : Entrée l'exécute aussitôt
//   exact = le texte tapé est déjà cette commande : Entrée l'exécute
export function buildPalette(input, commands, skills) {
  // « $skill » n'importe où dans le message
  const sk = /(?:^|\s)\$([\w.-]*)$/.exec(input);
  if (sk) {
    return skillItems(skills, fold(sk[1]), (n) => input.replace(/\$([\w.-]*)$/, () => '$' + n + ' ')).slice(0, 30);
  }
  if (input.includes('\n')) return [];

  // « /commande » ou « /skill » (le skill est inséré sous la forme $nom)
  const c = /^\/(\S*)$/.exec(input);
  if (c) {
    const q = fold(c[1]);
    const cmds = commands
      .filter((x) => hit(q, x.name, ...x.alias))
      .sort((a, b) => rank(q, a.name, ...a.alias) - rank(q, b.name, ...b.alias))
      .map((x) => ({
        id: 'c:' + x.name,
        label: '/' + x.name,
        desc: x.desc,
        tag: '',
        text: '/' + x.name + (x.args || x.free ? ' ' : ''),
        final: !x.args && !x.free,
        exact: q !== '' && rank(q, x.name, ...x.alias) === 0,
      }));
    return [...cmds, ...skillItems(skills, q, (n) => '$' + n + ' ')].slice(0, 30);
  }

  // « /commande argument » : valeurs proposées par la commande
  const a = /^\/(\S+)\s+([\s\S]*)$/.exec(input);
  if (a) {
    const cmd = findCommand(commands, a[1]);
    if (!cmd?.args) return [];
    const q = fold(a[2]).trim();
    return cmd
      .args()
      .filter((o) => hit(q, o.value))
      .map((o) => ({
        id: 'a:' + o.value,
        label: o.value,
        desc: o.desc,
        tag: '',
        text: '/' + cmd.name + ' ' + o.value,
        final: true,
        exact: q !== '' && fold(o.value) === q,
      }));
  }
  return [];
}
