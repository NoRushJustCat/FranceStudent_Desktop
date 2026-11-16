<script>
  // Pastille + menu du mode d'autorisation. L'état vit dans permstore.svelte.js.
  // up : le menu s'ouvre vers le haut (pastille placée en bas, dans la barre du composer).
  import { perm, MODES, setMode } from './permstore.svelte.js';

  let { up = false } = $props();

  let open = $state(false);
  let btn = $state(null);
  let pos = $state({ top: null, bottom: null, left: 0 });

  const cur = $derived(MODES.find((m) => m.id === perm.mode) ?? MODES[0]);

  function toggle() {
    if (!open && btn) {
      const r = btn.getBoundingClientRect();
      const w = 300;
      const left = Math.max(8, Math.min(r.left, window.innerWidth - w - 8));
      pos = up
        ? { top: null, bottom: window.innerHeight - r.top + 6, left }
        : { top: r.bottom + 6, bottom: null, left };
    }
    open = !open;
  }

  async function pick(id) {
    open = false;
    await setMode(id);
  }

  function onDoc(e) {
    if (!open) return;
    if (btn && btn.contains(e.target)) return;
    if (e.target.closest && e.target.closest('.pm-menu')) return;
    open = false;
  }

  function onKey(e) {
    if (e.key === 'Escape') open = false;
  }

  // Rattache le menu à <body> : échappe aux parents du composer (transform, overflow, z-index).
  function portal(node) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }
</script>

<svelte:window onclick={onDoc} onkeydown={onKey} />

<button
  class="pm-btn"
  class:danger={perm.mode === 'full'}
  class:plan={perm.mode === 'plan'}
  class:up
  bind:this={btn}
  onclick={toggle}
  title={'Autorisations : ' + cur.label}
  aria-label="Mode d'autorisation"
  aria-haspopup="menu"
  aria-expanded={open}
>
  {#if perm.mode === 'full'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 3 6v6c0 5 3.8 9.3 9 10 5.2-.7 9-5 9-10V6z"/><path d="M12 8v5"/><path d="M12 16.5v.01"/></svg>
  {:else if perm.mode === 'plan'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
  {:else}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 3 6v6c0 5 3.8 9.3 9 10 5.2-.7 9-5 9-10V6z"/></svg>
  {/if}
  <span>{cur.label}</span>
  <svg class="pm-chev" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>
</button>

{#if open}
  <div
    class="pm-menu"
    use:portal
    role="menu"
    style:top={pos.top != null ? pos.top + 'px' : null}
    style:bottom={pos.bottom != null ? pos.bottom + 'px' : null}
    style:left={pos.left + 'px'}
  >
    {#each MODES as m (m.id)}
      <button
        class="pm-item"
        class:sel={m.id === perm.mode}
        class:danger={m.id === 'full'}
        role="menuitemradio"
        aria-checked={m.id === perm.mode}
        onclick={() => pick(m.id)}
      >
        <span class="pm-name">{m.label}</span>
        <span class="pm-desc">{m.desc}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .pm-btn {
    display: inline-flex;
    align-items: center;
    flex: 0 0 auto;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm, 8px);
    background: var(--panel2);
    color: var(--dim);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    white-space: nowrap;
    transition: border-color 0.15s ease, color 0.15s ease;
  }
  .pm-chev {
    display: block;
    flex-shrink: 0;
    margin-left: 2px;
    transition: transform 0.15s;
  }
  .pm-btn.up .pm-chev {
    transform: rotate(180deg);
  }
  .pm-btn.up[aria-expanded='true'] .pm-chev {
    transform: none;
  }
  .pm-btn:not(.up)[aria-expanded='true'] .pm-chev {
    transform: rotate(180deg);
  }
  .pm-btn:hover {
    color: var(--text);
    border-color: var(--line2);
  }
  .pm-btn.plan {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent-text);
  }
  .pm-btn.danger {
    border-color: var(--danger);
    color: var(--danger);
  }
  .pm-menu {
    position: fixed;
    z-index: 1000;
    width: 300px;
    padding: 4px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--panel);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
  }
  .pm-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .pm-item:hover {
    background: var(--hover);
  }
  .pm-item.sel {
    background: var(--accent-soft);
  }
  .pm-item.danger .pm-name {
    color: var(--danger);
  }
  .pm-name {
    font-size: 13px;
    font-weight: 600;
  }
  .pm-desc {
    font-size: 11.5px;
    line-height: 1.35;
    color: var(--faint);
  }
</style>
