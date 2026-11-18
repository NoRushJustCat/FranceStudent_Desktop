<script>
  let { agents = [], value = null, disabled = false, onchange = () => {} } = $props();
  const id = $props.id();
  let menu;
  const selected = $derived(agents.find((a) => a.id === value) ?? agents[0]);

  function choose(agent) {
    menu.hidePopover();
    onchange(agent.id);
  }

  function navigate(event) {
    const buttons = [...menu.querySelectorAll('button')];
    const index = buttons.indexOf(document.activeElement);
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1
        : (index + (event.key === 'ArrowUp' ? -1 : 1) + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  }
</script>

<button class="trigger" popovertarget={id} {disabled} title="Choisir un modèle" style:anchor-name={'--' + id}>
  <span>{selected?.name ?? 'Choisir un modèle'}</span>
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
</button>
<div {id} bind:this={menu} popover="auto" role="dialog" tabindex="-1" aria-label="Choisir un modèle" class="model-menu" style:position-anchor={'--' + id} onkeydown={navigate}>
  <div class="heading">Choisir un modèle <span>{agents.length}</span></div>
  <div class="options">
    {#each agents as agent (agent.id)}
      <button class:active={agent.id === selected?.id} aria-pressed={agent.id === selected?.id} onclick={() => choose(agent)}>
        <span class="model-info"><span class="name">{agent.name}</span><span class="provider">{agent.modelProvider || 'Modèle IA'}</span></span>
        <span class="check" aria-hidden="true">{agent.id === selected?.id ? '✓' : ''}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .trigger { display: inline-flex; align-items: center; gap: 12px; height: 30px; max-width: 220px; padding: 0 10px; border: 1px solid var(--line); border-radius: 8px; background: var(--panel2); color: var(--text); font: inherit; font-size: 11px; cursor: pointer; }
  .trigger > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .trigger svg { flex-shrink: 0; color: var(--dim); }
  .trigger:hover, .trigger:focus-visible { border-color: var(--accent-line); }
  .trigger:disabled { opacity: .5; cursor: default; }
  .model-menu { position: fixed; inset: auto; bottom: anchor(top); left: anchor(left); margin: 0 0 8px; width: min(310px, calc(100vw - 24px)); max-height: min(460px, 70vh); padding: 6px; border: 1px solid var(--line2); border-radius: 14px; background: var(--panel); color: var(--text); box-shadow: 0 16px 48px #0005; position-try-fallbacks: flip-inline, flip-block; }
  .heading { display: flex; justify-content: space-between; padding: 9px 10px 11px; margin-bottom: 4px; border-bottom: 1px solid var(--line); font-size: 11px; font-weight: 600; color: var(--dim); }
  .heading span { color: var(--faint); }
  .options { max-height: min(390px, 55vh); overflow-y: auto; scrollbar-width: thin; }
  .options button { display: flex; align-items: center; justify-content: space-between; gap: 8px; width: 100%; min-height: 30px; padding: 5px 9px; border: 1px solid transparent; border-radius: 8px; background: transparent; color: var(--text); font: inherit; text-align: left; cursor: pointer; }
  .options button:hover { background: var(--hover); }
  .options button.active { background: var(--accent-soft); border-color: var(--accent-line); }
  .options button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .model-info { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .name { font-size: 12px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .provider { flex-shrink: 0; font-size: 10px; color: var(--dim); }
  .check { width: 14px; color: var(--accent-text); font-size: 13px; }
</style>
