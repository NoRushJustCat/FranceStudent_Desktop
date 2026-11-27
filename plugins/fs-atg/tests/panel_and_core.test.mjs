import assert from 'node:assert/strict';
import { readFileSync, realpathSync, statSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { Script } from 'node:vm';
import { checkPanelScripts, extractInlineScripts, PANEL_HTML } from './check-html-script.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const html = readFileSync(PANEL_HTML, 'utf8');
const js = extractInlineScripts(html).map(({ code }) => code).join('\n');
const domMarkup = html.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '');

// Read actual opening tags, not comments or HTML strings inside the script.
const tags = [...domMarkup.replace(/<!--[\s\S]*?-->/g, '').matchAll(/<[a-z][\w-]*\b(?:"[^"]*"|'[^']*'|[^'"<>])*>/gi)]
  .map(([tag]) => Object.fromEntries([...tag.matchAll(/\s([\w:-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g)]
    .map(([, key, double, single, bare]) => [key, double ?? single ?? bare ?? ''])));

function element(id) {
  const matches = tags.filter((tag) => tag.id === id);
  assert.equal(matches.length, 1, `Il doit exister un seul élément #${id}`);
  return matches[0];
}

// Compile selected production functions in isolation, without booting Antigravity.
function panelFunction(name) {
  const match = js.match(new RegExp(`^    (?:async )?function ${name}\\([^\\n]*\\n[\\s\\S]*?^    }`, 'm'));
  assert.ok(match, `Fonction du panneau introuvable : ${name}`);
  return match[0];
}

function runPanelFunction(name, expression, context) {
  return new Script(`${panelFunction(name)}\n${expression}`).runInNewContext(context);
}

test('le JavaScript inline du panneau compile', () => {
  assert.ok(checkPanelScripts() > 0);
});

test('le compteur tient compte de chacune des quatre extensions MCP', () => {
  const ids = ['computerUseInput', 'deepThinkingInput', 'canvasEnabledInput', 'mediaToolsInput'];
  const entries = Object.fromEntries(ids.map((id) => {
    assert.equal(element(id).type, 'checkbox');
    return [id, { checked: false }];
  }));
  entries.extMcpCountTag = {};
  const render = (stateLoad) => runPanelFunction('updateExtMcpCount', 'updateExtMcpCount()', {
    $: (id) => entries[id], stateLoad,
  });
  render('ready');
  assert.match(entries.extMcpCountTag.textContent, /^0 \/ 4\b/);
  for (const id of ids) {
    entries[id].checked = true;
    render('ready');
    assert.match(entries.extMcpCountTag.textContent, /^1 \/ 4\b/, id);
    entries[id].checked = false;
  }
  for (const id of ids) entries[id].checked = true;
  render('ready');
  assert.match(entries.extMcpCountTag.textContent, /^4 \/ 4\b/);
  render('unavailable');
  assert.match(entries.extMcpCountTag.textContent, /^— \/ 4\b/);
});

test('les quatre outils locaux figurent dans le catalogue utilisé pour le compteur MCP', () => {
  const core = read('fs_core.mjs');
  const catalog = core.match(/export const MCP_TOOL_CATALOG = Object\.freeze\(\[([\s\S]*?)\]\);/)?.[1];
  assert.ok(catalog);
  const mcp = read('mcp_server.mjs');
  for (const name of ['fs_fetch', 'fs_doc_read', 'fs_git', 'fs_media']) {
    assert.match(catalog, new RegExp(`name:\\s*['"]${name}['"]`));
    assert.match(mcp, new RegExp(`registerTool\\(['"]${name}['"]`));
  }
  for (const source of [mcp, read('sidecars/fs_atg_panel/main.mjs')]) {
    assert.match(source, /toolCount:\s*MCP_TOOL_CATALOG\.length/);
  }
});

test('le sélecteur Workspace est relié au panneau et les refus sont explicites', () => {
  assert.equal(element('wsTarget').role, 'group');
  assert.equal(element('wsPill')['aria-controls'], 'wsEditor');
  assert.ok(Object.hasOwn(element('wsEditor'), 'hidden'));
  assert.match(panelFunction('send'), /workspaceRoot:\s*[^\n]*wsConfirmedRoot\(\)/);
  const sidecar = read('sidecars/fs_atg_panel/main.mjs');
  assert.match(sidecar, /new ApiError\(\s*400,\s*'WORKSPACE_INVALID'/);
  assert.match(sidecar, /new ApiError\(\s*400,\s*'WORKSPACE_REQUIRED'/);
});

test('un échec métier HTTP 200 interrompt l’action', () => {
  const check = runPanelFunction('assertOk', 'assertOk', {});
  assert.throws(() => check({ ok: false, error: 'Refus simulé' }), /Refus simulé/);
  const success = { ok: true };
  assert.equal(check(success), success);
});

test('copie, suppression et envoi ne signalent pas un faux succès', async () => {
  const check = runPanelFunction('assertOk', 'assertOk', {});
  const runHandler = (start, end, context) => {
    const opening = js.indexOf(start);
    const closing = js.indexOf(end, opening + start.length);
    assert.ok(opening >= 0 && closing > opening, `Gestionnaire introuvable : ${start}`);
    return new Script(`(async () => { ${js.slice(opening, closing)} })()`).runInNewContext(context);
  };
  const messages = [];
  const flash = (text, _target, error) => messages.push({ text, error });
  const e = { target: { closest: () => ({ dataset: { copyResp: '1', delHist: '1' } }) } };
  await runHandler('const copyRespBtn =', 'const delHistBtn =', {
    e, flash, S: { history: [{ id: '1', response: 'Réponse' }] }, copyText: async () => false,
  });
  assert.equal(messages.length, 1);
  assert.equal(messages.pop().error, true);
  const expandedHistory = new Set(['1']);
  await runHandler('const delHistBtn =', 'const rerunBtn =', {
    e, flash, expandedHistory, assertOk: check, api: async () => ({ ok: false, message: 'Refus simulé' }),
  });
  assert.equal(messages.length, 1);
  assert.equal(messages.pop().error, true);
  assert.ok(expandedHistory.has('1'));
  for (const response of [async () => ({ ok: false, message: 'Refus simulé' }), async () => { throw new Error('Réseau indisponible'); }]) {
    const task = { value: 'Demande à conserver' };
    await runPanelFunction('send', 'send()', {
      $: (id) => id === 'task' ? task : null, flash, selectedChoice: '',
      hidePopover() {}, setCmdPopover() {}, renderApprovalBox() {}, pendingApproval: null,
      apiDelegate: response, assertOk: check, wsConfirmedRoot: () => undefined,
      liveVisionAttached: false, S: {},
    });
    assert.equal(task.value, 'Demande à conserver');
    assert.equal(messages.length, 1);
    assert.equal(messages.pop().error, true);
  }
});

test('un refus de réglage restaure la valeur et le bouton Raisonnement', async () => {
  const messages = [];
  const S = { reasoningEffort: 'standard' };
  const before = { dataset: { effort: 'standard' }, on: true };
  const after = { dataset: { effort: 'deep' }, textContent: 'Approfondi', on: false };
  const buttons = [before, after];
  const group = { querySelector: () => buttons.find((b) => b.on), querySelectorAll: () => buttons };
  for (const b of buttons) {
    b.parentElement = group;
    b.classList = { toggle: (_key, active) => { b.on = active; } };
  }
  assert.match(js, /bindOptimisticSeg\('#reasoningSeg',\s*'effort',\s*'reasoningEffort',\s*'reasoningEffort'/);
  runPanelFunction('bindOptimisticSeg', "bindOptimisticSeg('#reasoningSeg', 'effort', 'reasoningEffort', 'reasoningEffort', 'Raisonnement')", {
    document: { querySelectorAll: () => buttons }, S, syncLiquidPill() {}, poll() {},
    pushSettingPatch: async () => { throw new Error('Refus simulé'); }, flash: (message, _target, error) => messages.push({ message, error }),
  });
  await after.onclick();
  assert.equal(S.reasoningEffort, 'standard');
  assert.deepEqual(buttons.map((b) => b.on), [true, false]);
  assert.equal(messages.at(-1).error, true);
  assert.match(messages.at(-1).message, /valeur précédente restaurée/);
});

test('en cas de perte du sidecar, le panneau indique son indisponibilité', () => {
  const entries = { connPill: {}, connTxt: {}, disconnectBtn: {} };
  runPanelFunction('renderOfflineState', 'renderOfflineState()', {
    $: (id) => entries[id], updateExtMcpCount() {},
  });
  assert.equal(entries.connTxt.textContent, 'Sidecar indisponible');
  assert.equal(entries.disconnectBtn.disabled, true);
  assert.match(panelFunction('poll'), /catch[\s\S]*?stateLoad\s*=\s*'unavailable';\s*renderOfflineState\(\)/);
});

for (const [list, pairs] of [
  ['mainTabs', [['tabbtn-dash', 'tab-dash'], ['tabbtn-models', 'tab-models'], ['tabbtn-settings', 'tab-settings']]],
  ['settingsSubSeg', ['essential', 'memory', 'advanced'].map((id) => [`subtab-${id}`, `settings-sub-${id}`])],
]) {
  test(`les onglets de #${list} ont des relations ARIA cohérentes`, () => {
    assert.equal(element(list).role, 'tablist');
    for (const [[tabId, panelId], index] of pairs.map((pair, index) => [pair, index])) {
      const tab = element(tabId);
      const panel = element(panelId);
      assert.equal(tab.role, 'tab');
      assert.equal(tab['aria-controls'], panelId);
      assert.equal(tab['aria-selected'], index === 0 ? 'true' : 'false');
      assert.equal(panel.role, 'tabpanel');
      assert.equal(panel['aria-labelledby'], tabId);
      assert.equal(Object.hasOwn(panel, 'hidden'), index !== 0);
    }
  });
}

test('un refus de réglage Raisonnement restaure la pilule #reasoningTag et retire le focus du bouton', async () => {
  const tag = { textContent: 'Standard' };
  let blurred = 0;
  const label = (b) => (b.dataset.effort === 'max' ? 'Maximum' : b.dataset.effort === 'standard' ? 'Standard' : 'Approfondi');
  const before = { dataset: { effort: 'standard' }, textContent: 'Standard', on: true };
  const after = { dataset: { effort: 'max' }, textContent: 'Max', on: false, blur() { blurred += 1; } };
  const buttons = [before, after];
  const group = { querySelector: () => buttons.find((b) => b.on), querySelectorAll: () => buttons };
  for (const b of buttons) {
    b.parentElement = group;
    b.classList = { toggle: (_key, active) => { b.on = active; } };
  }
  runPanelFunction('bindOptimisticSeg', "bindOptimisticSeg('#reasoningSeg', 'effort', 'reasoningEffort', 'reasoningEffort', 'Raisonnement', applyTag)", {
    document: { querySelectorAll: () => buttons }, S: { reasoningEffort: 'standard' }, syncLiquidPill() {}, poll() {}, flash() {},
    applyTag: (b) => { tag.textContent = label(b); },
    pushSettingPatch: async () => { throw new Error('Refus simulé'); },
  });
  await after.onclick();
  assert.equal(tag.textContent, 'Standard');
  assert.equal(blurred, 1);
});

test('hors ligne, seuls les placeholders Chargement… et … sont remplacés', () => {
  const entries = {
    connPill: {}, connTxt: {}, disconnectBtn: {},
    qtxt: { textContent: 'Chargement…' },
    sessionStatusLine: { textContent: 'Chargement…' },
    'cnt-heavy': { textContent: '…' },
    'cnt-medium': { textContent: '…' },
    'cnt-light': { textContent: '3' },
  };
  runPanelFunction('renderOfflineState', 'renderOfflineState()', { $: (id) => entries[id], updateExtMcpCount() {} });
  assert.equal(entries.qtxt.textContent, 'Indisponible');
  assert.equal(entries.sessionStatusLine.textContent, 'Indisponible');
  assert.equal(entries['cnt-heavy'].textContent, '—');
  assert.equal(entries['cnt-medium'].textContent, '—');
  assert.equal(entries['cnt-light'].textContent, '3', 'une vraie valeur déjà affichée ne doit pas être écrasée');
});

test('chaque image locale du README existe dans le dossier du plugin', () => {
  const readme = read('README.md');
  const images = [
    ...[...readme.matchAll(/<img\b[^>]*?\bsrc\s*=\s*(?:"([^"]+)"|'([^']+)'|([^\s>]+))/gi)].map((m) => m[1] ?? m[2] ?? m[3]),
    ...[...readme.matchAll(/!\[[^\]]*\]\(\s*(?:<([^>]+)>|([^\s)]+))/g)].map((m) => m[1] ?? m[2]),
  ].filter((src) => !/^(?:https?:\/\/|\/\/|data:)/i.test(src));
  assert.ok(images.length > 0, 'Le README doit contenir des captures locales');
  for (const src of images) {
    const path = decodeURIComponent(src.split(/[?#]/, 1)[0]);
    assert.ok(path && !isAbsolute(path) && !/^[a-z][\w+.-]*:/i.test(path), `Lien non relatif : ${src}`);
    const file = resolve(root, path);
    assert.ok(statSync(file).isFile(), `Image absente : ${src}`);
    const fromRoot = relative(realpathSync(root), realpathSync(file));
    assert.ok(!isAbsolute(fromRoot) && fromRoot !== '..' && !fromRoot.startsWith(`..${sep}`), `Image hors du plugin : ${src}`);
  }
});
