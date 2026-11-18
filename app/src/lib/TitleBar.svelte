<script>
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  let { canGoBack = false, canGoForward = false, onback, onforward } = $props();
  let win = null;
  let maxed = $state(false);
  let focused = $state(true);

  onMount(() => {
    try {
      win = getCurrentWindow();
    } catch {
      return; // navigateur (vite dev) : pas de fenêtre native
    }
    const offs = [];
    const sync = async () => {
      try {
        maxed = await win.isMaximized();
      } catch {}
    };
    sync();
    win.onResized(sync).then((f) => offs.push(f)).catch(() => {});
    win.onFocusChanged(({ payload }) => (focused = payload)).then((f) => offs.push(f)).catch(() => {});
    return () => offs.forEach((f) => f());
  });

  const act = (fn) => () => win?.[fn]().catch(() => {});
</script>

<header class="tb" class:blur={!focused} data-tauri-drag-region>
  <nav class="navigation" aria-label="Historique des conversations">
    <button type="button" class="b" disabled={!canGoBack} onclick={onback} aria-label="Retour" title="Retour">
      <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><path d="m12 5-7 7 7 7M5 12h14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
    <button type="button" class="b" disabled={!canGoForward} onclick={onforward} aria-label="Avancer" title="Avancer">
      <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><path d="m12 5 7 7-7 7M19 12H5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </button>
  </nav>
  <div class="ctl">
    <button type="button" class="b" onclick={act('minimize')} aria-label="Réduire" title="Réduire">
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M1 5h8" stroke="currentColor" stroke-width="1" fill="none" /></svg>
    </button>
    <button type="button" class="b" onclick={act('toggleMaximize')} aria-label={maxed ? 'Restaurer' : 'Agrandir'} title={maxed ? 'Restaurer' : 'Agrandir'}>
      {#if maxed}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2.5 2.5V1.5h6v6h-1M1.5 2.5h6v6h-6z" stroke="currentColor" stroke-width="1" fill="none" /></svg>
      {:else}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="1.5" y="1.5" width="7" height="7" stroke="currentColor" stroke-width="1" fill="none" /></svg>
      {/if}
    </button>
    <button type="button" class="b close" onclick={act('close')} aria-label="Fermer" title="Fermer">
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="currentColor" stroke-width="1" fill="none" /></svg>
    </button>
  </div>
</header>

<style>
  .tb {
    position: fixed;
    inset: 0 0 auto 0;
    height: var(--titlebar-h, 34px);
    display: flex;
    align-items: stretch;
    justify-content: flex-end;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    color: var(--dim);
    user-select: none;
    -webkit-user-select: none;
    z-index: 200;
    transition: color 0.16s ease;
  }

  .tb.blur {
    color: var(--faint);
  }

  .navigation {
    display: flex;
    margin-right: auto;
    padding-left: 8px;
  }

  .navigation .b {
    width: 32px;
  }

  .navigation .b:disabled {
    color: var(--faint);
    opacity: 0.4;
    pointer-events: none;
  }

  .ctl {
    display: flex;
  }

  .b {
    display: grid;
    place-items: center;
    width: 46px;
    height: 100%;
    padding: 0;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--dim);
    cursor: default;
    transition: background 0.12s ease, color 0.12s ease;
  }

  .b:hover {
    background: var(--hover);
    color: var(--text);
  }

  .b:active {
    background: var(--line2);
  }

  .b.close:hover {
    background: var(--danger);
    color: #fff;
  }
</style>
