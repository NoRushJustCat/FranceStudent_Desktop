<script>
  // Terminal intégré : shell PowerShell persistant + sorties des commandes lancées par l'IA.
  // L'état vit dans termstore.svelte.js : il survit à la fermeture du panneau et au changement d'onglet.
  import { onMount, tick } from 'svelte';
  import { term, startTerm, runTerm, killTerm, clearTerm } from './termstore.svelte.js';

  let { cwd = null } = $props();

  let box;
  let inp;
  let val = $state('');
  let stick = true; // on suit le bas tant que l'utilisateur n'a pas remonté
  let hi = -1; // position dans l'historique (-1 = saisie en cours)
  let draft = '';

  // Dernier dossier du chemin, pour l'invite.
  const short = (p) => {
    const s = String(p || '')
      .replace(/\\/g, '/')
      .replace(/\/+$/, '');
    if (!s) return 'PS';
    const parts = s.split('/').filter(Boolean);
    return parts.length ? parts[parts.length - 1] : s;
  };

  const busy = $derived(term.alive && term.pending > 0);
  const status = $derived(
    term.starting ? 'démarrage…' : !term.alive ? 'arrêté' : term.pending > 0 ? 'en cours…' : 'prêt'
  );

  onMount(() => {
    startTerm(cwd);
    inp?.focus();
  });

  function onscroll() {
    if (box) stick = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  }

  // Autoscroll : nouvelle ligne ou ligne en cours d'écriture.
  $effect(() => {
    void term.lines.length;
    void term.lines[term.lines.length - 1]?.t;
    if (!stick) return;
    tick().then(() => {
      if (box) box.scrollTop = box.scrollHeight;
    });
  });

  // Le terminal est visible : plus rien n'est « non lu ».
  $effect(() => {
    void term.lines.length;
    term.unread = 0;
  });

  async function submit() {
    const text = val;
    val = '';
    hi = -1;
    draft = '';
    stick = true;
    await runTerm(text, cwd);
  }

  const hasSel = () =>
    (inp && inp.selectionStart !== inp.selectionEnd) || String(window.getSelection?.() ?? '') !== '';

  function key(e) {
    if (e.key === 'Enter' && !e.isComposing) {
      e.preventDefault();
      submit();
    } else if (e.key === 'ArrowUp') {
      const h = term.history;
      if (!h.length) return;
      e.preventDefault();
      if (hi === -1) {
        draft = val;
        hi = h.length - 1;
      } else if (hi > 0) {
        hi--;
      }
      val = h[hi];
    } else if (e.key === 'ArrowDown') {
      if (hi === -1) return;
      e.preventDefault();
      const h = term.history;
      if (hi < h.length - 1) {
        hi++;
        val = h[hi];
      } else {
        hi = -1;
        val = draft;
      }
    } else if (e.ctrlKey && !e.shiftKey && !e.altKey && (e.key === 'c' || e.key === 'C')) {
      // Ctrl+C : copie s'il y a une sélection, sinon interrompt la commande en cours.
      if (!hasSel() && term.pending > 0) {
        e.preventDefault();
        killTerm();
      }
    } else if (e.ctrlKey && (e.key === 'l' || e.key === 'L')) {
      e.preventDefault();
      clearTerm();
    }
  }

  // Clic dans la zone de sortie : focus sur la saisie, sauf si on est en train de sélectionner du texte.
  function focusIn() {
    if (String(window.getSelection?.() ?? '')) return;
    inp?.focus();
  }
</script>

<div class="tm">
  <div class="bar">
    <span class="dot" class:busy class:off={!term.alive}></span>
    <span class="stt">{status}</span>
    <span class="cwd" title={term.cwd}>{term.cwd}</span>
    <span class="sp"></span>
    <button type="button" class="b" onclick={() => killTerm()} disabled={!term.alive} title="Interrompre : arrête le shell (Ctrl+C)">
      Interrompre
    </button>
    <button type="button" class="b" onclick={() => clearTerm()} title="Effacer (Ctrl+L)">Effacer</button>
  </div>

  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="log" bind:this={box} {onscroll} onclick={focusIn}>
    {#if term.lines.length === 0}
      <p class="hint">Terminal PowerShell. Les commandes lancées par l'IA apparaissent aussi ici, avec leur sortie.</p>
    {/if}
    {#each term.lines as l}
      {#if l.k === 'in'}
        <div class="ln"><span class="pr">{short(l.cwd)}&gt;</span> {l.t}</div>
      {:else if l.k === 'ai'}
        <div class="ai" class:bad={l.err}>
          <div class="aih">
            <span class="tag">IA</span>
            <code class="cmd">$ {l.cmd}</code>
          </div>
          {#if l.t}
            <pre class="ao">{l.t}</pre>
          {:else}
            <div class="ae">(aucune sortie)</div>
          {/if}
        </div>
      {:else}
        <div class="ln {l.k}">{l.t}</div>
      {/if}
    {/each}
  </div>

  <div class="inrow">
    <span class="pr">{short(term.cwd)}&gt;</span>
    <input
      bind:this={inp}
      bind:value={val}
      onkeydown={key}
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      aria-label="Commande"
      placeholder={busy ? 'commande en cours…' : 'Entrer une commande'}
    />
  </div>
</div>

<style>
  .tm {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: rgba(0, 0, 0, 0.25);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    height: 34px;
    padding: 0 10px 0 14px;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
    color: var(--dim);
    font-size: 11.5px;
  }
  .dot {
    flex-shrink: 0;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
  }
  .dot.busy {
    background: var(--warn);
    animation: tmpulse 1s ease-in-out infinite;
  }
  .dot.off {
    background: var(--faint);
  }
  @keyframes tmpulse {
    50% {
      opacity: 0.35;
    }
  }
  .stt {
    flex-shrink: 0;
    white-space: nowrap;
  }
  .cwd {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    color: var(--faint);
  }
  .sp {
    flex: 1;
  }
  .b {
    flex-shrink: 0;
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: transparent;
    color: var(--dim);
    font-size: 11.5px;
    cursor: pointer;
  }
  .b:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .b:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .log {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 8px 12px;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.5;
    color: var(--text);
    cursor: text;
    user-select: text;
  }
  .hint {
    margin: 14px 2px;
    color: var(--faint);
    font-family: inherit;
    font-size: 12px;
  }
  .ln {
    min-height: 1.5em;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .ln.err {
    color: var(--danger);
  }
  .ln.sys {
    color: var(--faint);
    font-style: italic;
  }
  .pr {
    color: var(--accent-text);
    user-select: none;
  }

  /* Commande lancée par l'IA + sa sortie. */
  .ai {
    margin: 8px 0;
    border: 1px solid var(--line);
    border-left: 3px solid var(--accent-line, var(--line));
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.03);
  }
  .ai.bad {
    border-left-color: var(--danger);
  }
  .aih {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--line);
  }
  .tag {
    flex-shrink: 0;
    padding: 0 6px;
    border-radius: 4px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
  }
  .cmd {
    min-width: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
  }
  .ao {
    margin: 0;
    padding: 6px 10px;
    max-height: 260px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--mono);
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--dim);
  }
  .ai.bad .ao {
    color: var(--danger);
  }
  .ae {
    padding: 6px 10px;
    color: var(--faint);
    font-size: 11.5px;
    font-style: italic;
  }

  .inrow {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    padding: 8px 12px;
    border-top: 1px solid var(--line);
    background: var(--panel);
    font-family: var(--mono);
    font-size: 12px;
  }
  .inrow input {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    caret-color: var(--accent-text);
  }
  .inrow input::placeholder {
    color: var(--faint);
  }
</style>
