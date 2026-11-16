<script>
  import { tick } from 'svelte';
  import { dev, log, clearLog, exportText } from './devlog.svelte.js';

  // Commandes Tauri en lecture seule autorisées depuis la console.
  const READONLY = ['get_settings', 'get_usage', 'list_agents'];

  let level = $state('all');
  let filter = $state('');
  let input = $state('');
  let follow = $state(true);
  let box;
  const history = [];
  let hIdx = -1;

  const shown = $derived(
    dev.entries.filter(
      (e) =>
        (level === 'all' || e.level === level) &&
        (!filter || `${e.source} ${e.message} ${e.data || ''}`.toLowerCase().includes(filter.toLowerCase()))
    )
  );

  $effect(() => {
    shown.length;
    if (follow) tick().then(() => box && (box.scrollTop = box.scrollHeight));
  });

  const time = (t) => new Date(t).toLocaleTimeString('fr-FR');

  async function run() {
    const line = input.trim();
    if (!line) return;
    history.unshift(line);
    hIdx = -1;
    input = '';
    log('cmd', 'console', `> ${line}`);
    const [cmd, ...rest] = line.split(/\s+/);
    if (cmd === 'help') {
      log(
        'info',
        'console',
        'Commandes : help · clear · copy · invoke <commande> [json] (lecture seule : ' + READONLY.join(', ') + ')'
      );
    } else if (cmd === 'clear') {
      clearLog();
    } else if (cmd === 'copy') {
      await copy();
    } else if (cmd === 'invoke') {
      const name = rest[0];
      if (!READONLY.includes(name)) return log('error', 'console', `Commande non autorisée : ${name || '(vide)'}`);
      try {
        const args = rest.length > 1 ? JSON.parse(rest.slice(1).join(' ')) : {};
        const r = await window.__TAURI_INTERNALS__.invoke(name, args);
        log('info', 'console', `${name} → résultat`, r);
      } catch (e) {
        log('error', 'console', `${name} → erreur`, e);
      }
    } else {
      log('warn', 'console', `Commande inconnue : ${cmd} (tape « help »)`);
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(exportText());
      log('info', 'console', `Journal copié (${dev.entries.length} entrées)`);
    } catch (e) {
      log('error', 'console', 'Copie impossible', e);
    }
  }

  function onkey(e) {
    if (e.key === 'Enter') run();
    else if (e.key === 'ArrowUp' && history.length) {
      hIdx = Math.min(hIdx + 1, history.length - 1);
      input = history[hIdx];
      e.preventDefault();
    } else if (e.key === 'ArrowDown') {
      hIdx = Math.max(hIdx - 1, -1);
      input = hIdx < 0 ? '' : history[hIdx];
      e.preventDefault();
    }
  }
</script>

<div class="dc">
  <div class="bar">
    <select bind:value={level} aria-label="Niveau">
      <option value="all">Tout</option>
      <option value="error">Erreurs</option>
      <option value="warn">Avertissements</option>
      <option value="info">Infos</option>
    </select>
    <input class="flt" placeholder="Filtrer…" bind:value={filter} />
    <label class="fol"><input type="checkbox" bind:checked={follow} /> Suivre</label>
    <button type="button" onclick={copy}>Copier</button>
    <button type="button" onclick={clearLog}>Vider</button>
  </div>

  <div class="log" bind:this={box}>
    {#each shown as e (e.id)}
      <div class="row {e.level}">
        <span class="ts">{time(e.t)}</span>
        <span class="src">{e.source}</span>
        <span class="msg">{e.message}</span>
        {#if e.data}
          <details>
            <summary>détails</summary>
            <pre>{e.data}</pre>
          </details>
        {/if}
      </div>
    {:else}
      <div class="empty">Aucune entrée. Utilise l’application, puis reviens ici. Tape « help » pour les commandes.</div>
    {/each}
  </div>

  <div class="prompt">
    <span>&gt;</span>
    <input bind:value={input} onkeydown={onkey} placeholder="help" spellcheck="false" autocomplete="off" />
  </div>
</div>

<style>
  .dc { display: flex; flex-direction: column; gap: 8px; height: 100%; min-height: 360px; font-size: 12px; }
  .bar { display: flex; gap: 8px; align-items: center; }
  .flt { flex: 1; }
  .bar input, .bar select, .bar button, .prompt input {
    background: var(--bg-2, #1b1b1f); color: inherit; border: 1px solid var(--border, #333);
    border-radius: 6px; padding: 4px 8px; font: inherit;
  }
  .bar button { cursor: pointer; }
  .fol { display: flex; gap: 4px; align-items: center; white-space: nowrap; }
  .log {
    flex: 1; overflow: auto; background: var(--bg-0, #0d0d10); border: 1px solid var(--border, #333);
    border-radius: 8px; padding: 6px; font-family: ui-monospace, Consolas, monospace;
  }
  .row { padding: 2px 4px; border-bottom: 1px solid rgba(255, 255, 255, 0.04); word-break: break-word; }
  .row.error { color: #ff7b72; }
  .row.warn { color: #e3b341; }
  .row.cmd { color: #79c0ff; }
  .ts { opacity: 0.5; margin-right: 6px; }
  .src { opacity: 0.7; margin-right: 6px; }
  .src::after { content: ' ·'; }
  pre { margin: 4px 0 2px; white-space: pre-wrap; max-height: 240px; overflow: auto; opacity: 0.9; }
  summary { cursor: pointer; opacity: 0.6; }
  .empty { opacity: 0.5; padding: 12px; }
  .prompt { display: flex; gap: 6px; align-items: center; font-family: ui-monospace, Consolas, monospace; }
  .prompt input { flex: 1; }
</style>
