import fs from 'node:fs';
import assert from 'node:assert/strict';

const src = fs.readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');
const block = src.slice(src.indexOf('  let navigationHistory ='), src.indexOf('  function go(uid)'));
assert.ok(block.includes('function navigateHistory'));
const harness = block.replaceAll('$state(', '(')
  .replace('const backIndex = $derived(historyTarget(-1));', '')
  .replace('const forwardIndex = $derived(historyTarget(1));', '');
new Function('assert', `
  let viewUid = null;
  let sessions = [{uid:1},{uid:2},{uid:3}];
  let effect;
  const $effect = fn => effect = fn;
  const untrack = fn => fn();
  function go(uid) { viewUid = uid; effect(); }
  ${harness}
  effect();
  assert.equal(historyTarget(-1), -1);
  assert.equal(historyTarget(1), -1);
  go(1); go(2);
  navigateHistory(historyTarget(-1));
  assert.equal(viewUid, 1);
  assert.equal(navigationHistory[historyTarget(1)], 2);
  navigateHistory(historyTarget(1));
  assert.equal(viewUid, 2);
  navigateHistory(historyTarget(-1));
  go(3);
  assert.equal(historyTarget(1), -1);
  sessions = sessions.filter(s => s.uid !== 1);
  navigateHistory(historyTarget(-1));
  assert.equal(viewUid, null);
  assert.equal(navigationHistory[historyTarget(1)], 3);
`)(assert);
console.log('Navigation history checks passed');
