<script>
  // Autorisation d'un appel d'outil MCP venu de France Student (pont MCP), affichée
  // par-dessus le chat. Une file d'attente gère plusieurs demandes (le Rust n'en envoie qu'une à la fois).
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';

  let queue = $state([]);
  let left = $state(0);
  let timer = null;
  const cur = $derived(queue[0] ?? null);

  function tick() {
    if (!cur) return;
    left = Math.max(0, Math.ceil((cur.deadline - Date.now()) / 1000));
    if (left <= 0) drop(cur.id);
  }

  function drop(id) {
    queue = queue.filter((q) => q.id !== id);
    if (!queue.length) {
      clearInterval(timer);
      timer = null;
    } else tick();
  }

  async function answer(allow) {
    const c = cur;
    if (!c) return;
    drop(c.id);
    try {
      await invoke('bridge_confirm_answer', { id: c.id, allow });
    } catch {
      /* la demande a expiré côté Rust : rien à faire */
    }
  }

  function onKey(e) {
    if (!cur) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      answer(false);
    }
  }

  onMount(() => {
    const offs = [];
    let dead = false;
    (async () => {
      const a = await listen('bridge:confirm', (e) => {
        const p = e.payload;
        if (queue.some((q) => q.id === p.id)) return;
        queue = [...queue, { ...p, deadline: Date.now() + (p.waitSecs ?? 120) * 1000 }];
        if (!timer) timer = setInterval(tick, 500);
        tick();
      });
      const b = await listen('bridge:confirm-close', (e) => drop(e.payload.id));
      if (dead) {
        a();
        b();
      } else offs.push(a, b);
    })();
    return () => {
      dead = true;
      offs.forEach((f) => f());
      clearInterval(timer);
    };
  });
</script>

<svelte:window onkeydown={onKey} />

{#if cur}
  <div class="overlay bc-ov">
    <div class="dialog bc" role="alertdialog" aria-modal="true" aria-labelledby="bc-title" tabindex="-1">
      <div class="bc-head">
        <span class="bc-badge" class:ro={cur.readOnly}>{cur.readOnly ? 'lecture seule' : 'peut modifier'}</span>
        <h3 id="bc-title">Autoriser cet outil MCP ?</h3>
 {#if queue.length > 1}<span class="bc-more">+{queue.length - 1} en attente</span>{/if}
      </div>
      <p class="bc-sub">France Student (via le pont MCP) demande de lancer :</p>
      <div class="bc-tool"><span>{cur.server}</span> › <b>{cur.tool}</b></div>
      {#if cur.args && cur.args !== '{}' && cur.args !== 'null'}
 <pre class="bc-args">{cur.args}</pre>
      {:else}
        <p class="bc-sub">Sans argument.</p>
      {/if}
      <div class="bc-foot">
        <span class="bc-timer">Refus automatique dans {left} s</span>
        <button type="button" class="bc-btn" onclick={() => answer(false)}>Refuser</button>
        <button type="button" class="bc-btn ok" onclick={() => answer(true)}>Autoriser</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .bc-ov {
    z-index: 400;
  }
  .bc {
    width: min(520px, 92vw);
  }
  .bc-head {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .bc-head h3 {
    margin: 0;
    font-size: 15px;
    flex: 1;
  }
  .bc-badge {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--warn);
    color: var(--warn);
  }
  .bc-badge.ro {
    border-color: var(--ok);
    color: var(--ok);
  }
  .bc-more {
    font-size: 11px;
    color: var(--dim);
  }
  .bc-sub {
    color: var(--dim);
    font-size: 13px;
    margin: 10px 0 6px;
  }
  .bc-tool {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 13px;
    color: var(--text);
  }
  .bc-tool span {
    color: var(--dim);
  }
  .bc-args {
    margin: 10px 0 0;
    max-height: 240px;
    overflow: auto;
    padding: 10px 12px;
    background: var(--panel2);
    border: 1px solid var(--line);
    border-radius: 8px;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .bc-foot {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 16px;
  }
  .bc-timer {
    flex: 1;
    font-size: 12px;
    color: var(--dim);
  }
  .bc-btn {
    padding: 7px 16px;
    border-radius: 8px;
    border: 1px solid var(--line2);
    background: var(--panel2);
    color: var(--text);
    cursor: pointer;
    font: inherit;
    font-size: 13px;
  }
  .bc-btn:hover {
    border-color: var(--danger);
  }
  .bc-btn.ok {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .bc-btn.ok:hover {
    filter: brightness(1.1);
  }
</style>
