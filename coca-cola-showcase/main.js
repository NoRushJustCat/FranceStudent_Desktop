/* ==========================================================================
   main.js — Coca-Cola Red Experience
   --------------------------------------------------------------------------
   Orchestre : scène 3D du hero, studio de personnalisation, thèmes / gammes,
   i18n FR-EN, toasts, retour en haut, bulles du footer, tilt des cartes.

   Dépendances : Three.js r128 (global THREE, chargé avant en `defer`) et
   window.CocaAudio (audio.js, optionnel : le site reste utilisable sans son).

   Règles tenues ici :
   - tout est dans une IIFE, aucune variable globale (sauf window.CocaExperience) ;
   - audio.js branche lui-même les .sound-btn, #volume, #audioToggle, #fizzViz :
     main.js NE les rebranche PAS (sinon double déclenchement) ;
   - chaque module démarre dans un try/catch : une panne locale n'arrête pas le reste.
   ========================================================================== */
(() => {
  'use strict';

  /* ───────────────────────── 0. Configuration ───────────────────────── */
  const CFG = {
    // true : « Ouvrir une canette » lève le mute pour jouer le pschhh.
    // false : on respecte le choix « son coupé » (un toast explique comment l'activer).
    OPEN_FORCES_SOUND: false,
    HERO: { autoSpeed: 0.38, autoDelay: 2.2, bubbles: 150 },
    STUDIO: { autoSpeed: 0.22, autoDelay: 4.5, bubbles: 40 },
    KEYS: { lang: 'coca.lang', range: 'coca.range' }
  };

  /* ───────────────────────── 1. Utilitaires ─────────────────────────── */
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
  const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const rand = (a, b) => a + Math.random() * (b - a);
  const TAU = Math.PI * 2;
  const root = document.documentElement;
  const mq = (q) => (window.matchMedia ? window.matchMedia(q) : { matches: false });
  const mqReduce = mq('(prefers-reduced-motion: reduce)');
  const mqCoarse = mq('(pointer: coarse)');
  const LOW_MEM = mqCoarse.matches || (navigator.deviceMemory && navigator.deviceMemory <= 4);
  const TEX = LOW_MEM ? 1024 : 2048; // taille des textures de canette
  const store = {
    get(k) { try { return window.localStorage.getItem(k); } catch (e) { return null; } },
    set(k, v) { try { window.localStorage.setItem(k, v); } catch (e) { /* stockage indisponible */ } }
  };
  const safe = (label, fn) => {
    try { return fn(); } catch (e) { console.error('[Coca] ' + label, e); return undefined; }
  };
  const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r()));

  /* État partagé */
  const state = { lang: 'fr', range: 'classic', style: 'classic', finish: 'glossy', name: '' };
  let hero = null;
  let studio = null;
  let nameInput = null;

  /* ───────────────────────── 2. Données (gammes, canette) ───────────── */
  const RANGES = {
    classic: { label: 'Classic Red', c1: '#e41e2b', c2: '#7a0610', ink: '#ffffff', ribbon: '#ffffff', neon: '#ff3b4d', rim: '#ff4a58', rim2: '#ffc2c7', glow: '#ff2a3d' },
    zero: { label: 'Coca Zero', c1: '#1d1d22', c2: '#050506', ink: '#ffffff', ribbon: '#e41e2b', neon: '#ff2a3d', rim: '#e41e2b', rim2: '#ffffff', glow: '#e41e2b' },
    cherry: { label: 'Cherry Blast', c1: '#b3123f', c2: '#4a0620', ink: '#ffffff', ribbon: '#ffc2d4', neon: '#ff3d7f', rim: '#ff2d6f', rim2: '#ffb3c9', glow: '#ff2d6f' },
    vanilla: { label: 'Vanilla Chill', c1: '#f2d9a8', c2: '#c8873a', ink: '#7a1f12', ribbon: '#7a1f12', neon: '#ffc86b', rim: '#ffd38a', rim2: '#ffffff', glow: '#ffcf80' }
  };
  const FINISH_ALIAS = { gloss: 'glossy', glossy: 'glossy', matte: 'matte', metallic: 'chrome', chrome: 'chrome', frosted: 'frosted' };
  // Valeurs du matériau PBR par finition (le « glossy » reprend le cahier des charges : métal 0.85 / rugosité 0.22)
  const FINISHES = {
    glossy: { rough: 0.22, metal: 0.85, env: 1.15 },
    matte: { rough: 0.68, metal: 0.45, env: 0.8 },
    chrome: { rough: 0.07, metal: 1.0, env: 1.6 },
    frosted: { rough: 0.58, metal: 0.15, env: 0.9 }
  };
  const STYLE_IDS = ['classic', 'wave', 'neon', 'retro'];

  // Profil de la canette (unités arbitraires : rayon 0.8, hauteur ≈ 3.9)
  const CAN = { R: 0.8, yMin: -2.0, yMax: 1.89, bodyBot: -1.72, bodyTop: 1.3, neckTop: 1.7, lidY: 1.8 };
  CAN.af = (CAN.yMax - CAN.yMin) / (TAU * CAN.R); // rapport de densité x/y d'une texture carrée
  const FACE = Math.PI; // u = 0.5 de la texture est face caméra quand yaw = 0

  const FONTS = {
    logo: '"Yellowtail","Brush Script MT","Segoe Script",cursive',
    head: '"Outfit","Plus Jakarta Sans",system-ui,sans-serif',
    body: '"Plus Jakarta Sans","Outfit",system-ui,sans-serif'
  };

  /* ───────────────────────── 3. Toasts ──────────────────────────────── */
  const toast = (() => {
    const el = $('#toast');
    let timer = 0;
    return (msg, ms = 2600) => {
      if (!el || !msg) return;
      clearTimeout(timer);
      el.textContent = msg;
      el.classList.remove('is-show');
      void el.offsetWidth; // relance la transition
      el.classList.add('is-show');
      timer = setTimeout(() => el.classList.remove('is-show'), ms);
    };
  })();

  /* ───────────────────────── 4. i18n FR / EN ────────────────────────── */
  const MSG = {
    fr: {
      range: 'Gamme « {name} » sélectionnée', snap: 'Aperçu téléchargé', snapFail: 'Impossible de générer l’image',
      reset: 'Studio réinitialisé', noAudio: 'Audio indisponible sur ce navigateur',
      soundHint: 'Son coupé : active-le avec l’icône haut-parleur pour entendre le pschhh',
      fizz: 'Ça pétille !', no3d: 'La 3D n’est pas disponible sur cet appareil',
      soundOn: 'Son activé', soundOff: 'Son coupé', opened: 'Pschhh ! Bien frais.', lang: 'Français'
    },
    en: {
      range: '“{name}” range selected', snap: 'Preview downloaded', snapFail: 'Could not generate the image',
      reset: 'Studio reset', noAudio: 'Audio is not available in this browser',
      soundHint: 'Sound is off: turn it on with the speaker icon to hear the pschhh',
      fizz: 'It’s fizzing!', no3d: '3D is not available on this device',
      soundOn: 'Sound on', soundOff: 'Sound off', opened: 'Pschhh! Ice cold.', lang: 'English'
    }
  };
  const tr = (key, vars) => {
    let s = (MSG[state.lang] && MSG[state.lang][key]) || MSG.fr[key] || '';
    if (vars) Object.keys(vars).forEach((k) => { s = s.replace('{' + k + '}', vars[k]); });
    return s;
  };

  const I18N = (() => {
    // Le français vient du HTML (capturé au démarrage). Ici : l'anglais + quelques textes FR absents du HTML.
    const EN = {
      'meta.title': 'Coca-Cola Red Experience · Real Magic, Taste The Feeling',
      'meta.desc': 'Interactive 3D showcase: open the can, hear the pschhh, explore the flavors and customize your can with your name.',
      'nav.skip': 'Skip to content', 'nav.fizz': 'Fizz & Sounds', 'nav.flavors': 'Flavors', 'nav.studio': 'Studio',
      'nav.audio': 'Turn sound on', 'nav.lang': 'Language', 'nav.open': 'Open a can',
      'hero.badge': 'Limited edition · Red Experience',
      'hero.lead': 'Spin the can, open it and listen to the fizz. A 3D experience made to refresh your eyes and ears.',
      'hero.cta1': 'Taste the experience', 'hero.cta2': 'Customize my can', 'hero.range': 'Choose a range',
      'range.classic': 'Classic Red: the sparkling icon, deep red and lively bubbles.',
      'range.zero': 'Coca Zero: zero sugar, black and red, a crisp and bold taste.',
      'range.cherry': 'Cherry Blast: intense black cherry, deep red and explosive bubbles.',
      'range.vanilla': 'Vanilla Chill: golden cream, smooth vanilla and an ice-cold finish.',
      'hero.stageAria': 'Interactive 3D can. Drag to rotate it, click to open it.',
      'hero.hint': 'Drag to spin · Click to open',
      'fizz.eyebrow': 'Interactive Fizz & Sound Bar', 'fizz.title': 'Close your eyes, listen to the fizz',
      'fizz.sub': 'Every sound is synthesized live by your browser (Web Audio). They start after your click.',
      'fizz.pschhh': 'Pschhh', 'fizz.pschhhD': 'Opening the can', 'fizz.ice': 'Ice', 'fizz.iceD': 'Clinking in the glass',
      'fizz.fizz': 'Fizz', 'fizz.fizzD': 'Continuous fizz (on / off)', 'fizz.sip': 'Sip', 'fizz.sipD': 'Refreshing, ahhh',
      'fizz.volume': 'Volume',
      'flavors.eyebrow': 'Flavor Matrix 360°', 'flavors.title': 'Four flavors, one fizz',
      'flavors.sub': 'Hover (or touch) a card to tilt it in 3D. Sensory scores are indicative, out of 10.',
      'flavor.classic': 'The original taste: caramel, soft spices, lively bubbles.',
      'flavor.zero': 'Zero sugar, bold character: drier and sharper.',
      'flavor.cherry': 'An explosion of black cherry over cola.',
      'flavor.vanilla': 'Creamy and smooth, perfect ice-cold.',
      'meter.acid': 'Acidity', 'meter.fizz': 'Fizziness', 'meter.sweet': 'Sweetness', 'flavor.pick': 'Choose this flavor',
      'studio.eyebrow': 'Can Studio', 'studio.title': 'Your name, your can',
      'studio.sub': 'Type your name: it appears in real time on the 3D can.',
      'studio.stageAria': '3D preview of your custom can', 'studio.name': 'Your first name', 'studio.placeholder': 'Alex',
      'studio.style': 'Graphic style', 'studio.finish': 'Finish', 'studio.snap': 'Download preview', 'studio.reset': 'Reset',
      'style.classic': 'Classic', 'style.wave': 'Wave', 'style.neon': 'Neon', 'style.retro': 'Retro',
      'finish.glossy': 'Glossy', 'finish.matte': 'Matte', 'finish.chrome': 'Chrome', 'finish.frosted': 'Frosted',
      'footer.tagline': 'A showcase website for the pleasure of eyes, ears and bubbles.', 'footer.bubbles': 'Make it fizz',
      'footer.legal': 'Legal',
      'footer.disclaimer': 'Unofficial demo project, not affiliated with or endorsed by The Coca-Cola Company. Coca-Cola and related names are trademarks of their respective owners. The Cherry Blast and Vanilla Chill flavors are fictional concepts.',
      'footer.credits': 'Credits', 'footer.audioCredit': 'Sounds synthesized with the Web Audio API',
      'footer.top': 'Back to top', 'footer.fan': 'Fan project, non-commercial',
      'stage.noWebgl': 'The interactive 3D can needs WebGL, which is not available here.'
    };
    const FR_EXTRA = {
      'range.zero': 'Coca Zero : zéro sucre, noir et rouge, un goût net et franc.',
      'range.cherry': 'Cherry Blast : cerise noire intense, rouge profond et bulles explosives.',
      'range.vanilla': 'Vanilla Chill : crème dorée, douceur vanillée et fraîcheur glacée.',
      'finish.frosted': 'Givré',
      'stage.noWebgl': 'La canette 3D interactive nécessite WebGL, indisponible ici.'
    };
    const base = {}; // textes FR d'origine, lus dans le HTML
    const norm = (s) => s.replace(/\s+/g, ' ').trim();

    const setText = (el, v) => {
      if (!el.children.length) { if (el.textContent !== v) el.textContent = v; return; }
      const n = Array.from(el.childNodes).find((x) => x.nodeType === 3 && x.textContent.trim());
      if (n) n.textContent = v;
    };
    const capture = (scope = document) => {
      $$('[data-i18n]', scope).forEach((el) => { const k = el.dataset.i18n; if (!(k in base)) base[k] = norm(el.textContent); });
      $$('[data-i18n-placeholder]', scope).forEach((el) => { const k = el.dataset.i18nPlaceholder; if (!(('ph:' + k) in base)) base['ph:' + k] = el.getAttribute('placeholder') || ''; });
      $$('[data-i18n-aria]', scope).forEach((el) => { const k = el.dataset.i18nAria; if (!(('aria:' + k) in base)) base['aria:' + k] = el.getAttribute('aria-label') || ''; });
      if (!('meta.title' in base)) base['meta.title'] = document.title;
      const md = $('meta[name="description"]');
      if (md && !('meta.desc' in base)) base['meta.desc'] = md.getAttribute('content') || '';
    };
    const val = (key, prefix = '') => (state.lang === 'en'
      ? (EN[key] !== undefined ? EN[key] : base[prefix + key])
      : (FR_EXTRA[key] !== undefined ? FR_EXTRA[key] : base[prefix + key]));

    const applyTo = (scope = document) => {
      capture(scope);
      $$('[data-i18n]', scope).forEach((el) => { const v = val(el.dataset.i18n); if (v != null) setText(el, v); });
      $$('[data-i18n-placeholder]', scope).forEach((el) => { const v = val(el.dataset.i18nPlaceholder, 'ph:'); if (v != null) el.setAttribute('placeholder', v); });
      $$('[data-i18n-aria]', scope).forEach((el) => { const v = val(el.dataset.i18nAria, 'aria:'); if (v) el.setAttribute('aria-label', v); });
      // Libellés accessibles non couverts par data-i18n-aria : on les déduit du texte visible traduit
      $$('.meter', scope).forEach((m) => { const s = $('span', m); if (s) m.setAttribute('aria-label', s.textContent); });
      $$('fieldset.field', scope).forEach((f) => {
 const lg = $('legend', f), g = $('[role="radiogroup"]', f);
        if (lg && g) g.setAttribute('aria-label', lg.textContent);
      });
    };
    const setLang = (lang, { persist = true } = {}) => {
      state.lang = lang === 'en' ? 'en' : 'fr';
      root.lang = state.lang;
      applyTo();
      const t = val('meta.title'); if (t) document.title = t;
      const d = val('meta.desc'), md = $('meta[name="description"]');
      if (d && md) md.setAttribute('content', d);
      const sel = $('#langSelect'); if (sel && sel.value !== state.lang) sel.value = state.lang;
      if (persist) store.set(CFG.KEYS.lang, state.lang);
    };
    const init = () => {
      capture();
      const saved = store.get(CFG.KEYS.lang);
      setLang(saved === 'en' || saved === 'fr' ? saved : (root.lang === 'en' ? 'en' : 'fr'), { persist: false });
      const sel = $('#langSelect');
      if (sel) sel.addEventListener('change', () => { setLang(sel.value); });
    };
    return { init, setLang, applyTo };
  })();

  /* ───────────────────────── 5. Boucle d'animation partagée ─────────── */
  const ticker = (() => {
    const subs = new Set();
    let raf = 0, last = 0;
    const frame = (now) => {
      raf = 0;
      const dt = clamp((now - last) / 1000, 0.001, 0.05);
      last = now;
      let any = false;
      subs.forEach((s) => {
 if (!s.isActive()) return;
 any = true;
 try { s.update(dt, now / 1000); } catch (e) {
   console.error('[Coca] update', e);
          s.errors = (s.errors || 0) + 1;
        }
      });
      if (any) raf = requestAnimationFrame(frame);
    };
    return {
      add(s) { subs.add(s); },
      wake() { if (!raf) { last = performance.now(); raf = requestAnimationFrame(frame); } }
    };
  })();
  document.addEventListener('visibilitychange', () => ticker.wake());

  /* ───────────────────────── 6. Couleurs & texture : utilitaires 2D ──── */
  const hex2rgb = (h) => {
    h = h.replace('#', '');
    if (h.length === 3) h = h.split('').map((c) => c + c).join('');
    const n = parseInt(h, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  };
  const rgba = (h, a) => { const c = hex2rgb(h); return 'rgba(' + c[0] + ',' + c[1] + ',' + c[2] + ',' + a + ')'; };
  const mixHex = (a, b, t) => {
    const A = hex2rgb(a), B = hex2rgb(b);
    return 'rgb(' + Math.round(lerp(A[0], B[0], t)) + ',' + Math.round(lerp(A[1], B[1], t)) + ',' + Math.round(lerp(A[2], B[2], t)) + ')';
  };
  const mulberry32 = (a) => () => {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const makeCanvas = (w, h) => { const c = document.createElement('canvas'); c.width = w; c.height = h; return c; };

  const radialTexture = (stops, size = 128) => {
    const c = makeCanvas(size, size), x = c.getContext('2d');
    const g = x.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    stops.forEach(([o, col]) => g.addColorStop(o, col));
    x.fillStyle = g; x.fillRect(0, 0, size, size);
    const t = new THREE.CanvasTexture(c);
    return t;
  };
  const bubbleTexture = () => {
    const s = 96, c = makeCanvas(s, s), x = c.getContext('2d');
    const g = x.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
    g.addColorStop(0, 'rgba(255,255,255,0.05)'); g.addColorStop(0.62, 'rgba(255,255,255,0.10)');
    g.addColorStop(0.86, 'rgba(255,255,255,0.75)'); g.addColorStop(1, 'rgba(255,255,255,0)');
    x.fillStyle = g; x.fillRect(0, 0, s, s);
    x.fillStyle = 'rgba(255,255,255,0.95)'; x.beginPath(); x.ellipse(s * 0.36, s * 0.34, s * 0.09, s * 0.06, -0.6, 0, TAU); x.fill();
    return new THREE.CanvasTexture(c);
  };

  /* ───────────────────────── 7. Peintre de texture de canette ───────── */
  class Painter {
    constructor(W, H) {
      this.W = W; this.H = H;
      this.ppu = W / (TAU * CAN.R);          // pixels par unité, horizontalement
      this.sy = H / (CAN.yMax - CAN.yMin);   // pixels par unité, verticalement
      this.af = this.ppu / this.sy;   // < 1 : la texture est plus dense en vertical
    }
    Y(y) { return ((CAN.yMax - y) / (CAN.yMax - CAN.yMin)) * this.H; }
    // Repère « isotrope » : on y dessine comme sur une surface plane, la compensation est faite ici
    iso(ctx, cx, cy, rot, fn) {
      ctx.save(); ctx.translate(cx, cy); ctx.scale(1, 1 / this.af); if (rot) ctx.rotate(rot); fn(); ctx.restore();
    }
  }

  const spaced = (ctx, text, x, y, sp, mode) => {
    const chars = Array.from(text);
    const widths = chars.map((c) => ctx.measureText(c).width);
    const total = widths.reduce((a, b) => a + b, 0) + sp * (chars.length - 1);
    let cx = x - total / 2;
    const prev = ctx.textAlign; ctx.textAlign = 'left';
    chars.forEach((c, i) => {
      if (mode === 'stroke') ctx.strokeText(c, cx, y); else ctx.fillText(c, cx, y);
      cx += widths[i] + sp;
    });
    ctx.textAlign = prev;
    return total;
  };
  const fitFont = (ctx, text, fam, weight, maxW, maxH, spEm) => {
    let size = maxH;
    ctx.font = weight + ' ' + size + 'px ' + fam;
    const w = ctx.measureText(text).width + size * spEm * (Array.from(text).length - 1);
    if (w > maxW) { size *= maxW / w; ctx.font = weight + ' ' + size + 'px ' + fam; }
    return size;
  };
  const rrect = (ctx, x, y, w, h, r) => {
    r = Math.min(r, w / 2, h / 2);
    ctx.beginPath(); ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath();
  };

  // Logo « façon Coca-Cola » : approximation en police script + ruban dynamique (pas le logo officiel)
  const drawLogo = (ctx, p, pal, cx, cy, wUnits, o = {}) => {
    const w = wUnits * p.ppu;
    p.iso(ctx, cx, cy, 0, () => {
      ctx.font = '100px ' + FONTS.logo;
      const m = ctx.measureText('Coca-Cola').width || 400;
      const size = (100 * w) / m;
      ctx.font = size + 'px ' + FONTS.logo;
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      const neon = o.mode === 'neon';
      const color = o.color || pal.ink;
      if (neon) {
 ctx.shadowColor = pal.neon; ctx.shadowBlur = size * 0.35;
        ctx.lineWidth = size * 0.045; ctx.strokeStyle = pal.neon; ctx.strokeText('Coca-Cola', 0, 0);
 ctx.shadowBlur = size * 0.12; ctx.fillStyle = '#ffffff'; ctx.fillText('Coca-Cola', 0, 0);
      } else {
        ctx.shadowColor = 'rgba(0,0,0,0.35)'; ctx.shadowBlur = size * 0.05; ctx.shadowOffsetY = size * 0.03;
        ctx.fillStyle = color; ctx.fillText('Coca-Cola', 0, 0);
      }
      ctx.shadowColor = 'transparent'; ctx.shadowBlur = 0; ctx.shadowOffsetY = 0;
      // Ruban dynamique sous le logo
      const rw = w * 1.02, ry = size * 0.56;
      ctx.beginPath();
      ctx.moveTo(-rw / 2, ry);
      ctx.bezierCurveTo(-rw * 0.25, ry - size * 0.30, rw * 0.06, ry + size * 0.26, rw / 2, ry - size * 0.12);
      ctx.bezierCurveTo(rw * 0.08, ry + size * 0.46, -rw * 0.28, ry - size * 0.06, -rw / 2, ry);
      ctx.closePath();
      if (neon) {
        ctx.shadowColor = pal.neon; ctx.shadowBlur = size * 0.25; ctx.strokeStyle = pal.neon; ctx.lineWidth = size * 0.05; ctx.stroke();
        ctx.shadowBlur = 0;
      } else {
 ctx.fillStyle = o.ribbon || pal.ribbon; ctx.fill();
      }
    });
  };

  const front = (p, fn) => { fn(p.W * 0.5, 1); fn(0, 0.72); fn(p.W, 0.72); }; // face + dos (2 moitiés, raccord du joint)
  const hline = (ctx, p, y, thick, color) => { ctx.fillStyle = color; ctx.fillRect(0, p.Y(y) - (thick * p.sy) / 2, p.W, thick * p.sy); };

  const STYLES = {
    classic: {
      bg(ctx, p, pal) {
        const g = ctx.createLinearGradient(0, p.Y(CAN.neckTop), 0, p.Y(CAN.bodyBot));
        g.addColorStop(0, pal.c2); g.addColorStop(0.18, pal.c1); g.addColorStop(0.72, pal.c1); g.addColorStop(1, pal.c2);
        ctx.fillStyle = g; ctx.fillRect(0, 0, p.W, p.H);
      },
      deco(ctx, p, pal) {
        hline(ctx, p, 1.14, 0.012, rgba(pal.ink, 0.55));
        hline(ctx, p, -1.52, 0.012, rgba(pal.ink, 0.55));
 front(p, (cx, k) => drawLogo(ctx, p, pal, cx, p.Y(0.72), 1.2 * k));
      },
      name(ctx, p, pal, text, cx, cy, k) {
        p.iso(ctx, cx, cy, 0, () => {
          const t = text.toUpperCase();
          const s = fitFont(ctx, t, FONTS.head, 800, 1.28 * p.ppu * k, 0.34 * p.ppu * k, 0.08);
   ctx.textBaseline = 'middle'; ctx.fillStyle = pal.ink;
          ctx.shadowColor = 'rgba(0,0,0,0.35)'; ctx.shadowBlur = s * 0.12; ctx.shadowOffsetY = s * 0.06;
          spaced(ctx, t, 0, 0, s * 0.08, 'fill');
        });
      }
    },
    wave: {
      bg(ctx, p, pal) {
        const top = p.Y(CAN.neckTop), bot = p.Y(CAN.bodyBot);
        const g = ctx.createLinearGradient(0, top, 0, bot);
        g.addColorStop(0, pal.c1); g.addColorStop(1, pal.c2);
        ctx.fillStyle = g; ctx.fillRect(0, 0, p.W, p.H);
        const K = [2, 3, 4, 3, 2, 5, 4];
        for (let i = 0; i < K.length; i++) {
   const base = p.Y(1.45 - i * 0.55), amp = 0.13 * p.sy, ph = i * 1.7;
          ctx.beginPath(); ctx.moveTo(0, bot + 40);
   for (let x = 0; x <= p.W; x += 16) ctx.lineTo(x, base + amp * Math.sin((TAU * K[i] * x) / p.W + ph));
   ctx.lineTo(p.W, bot + 40); ctx.closePath();
   ctx.fillStyle = i % 2 ? rgba(pal.c2, 0.28) : rgba(pal.ink, 0.12);
   ctx.fill();
        }
      },
      deco(ctx, p, pal) {
 front(p, (cx, k) => drawLogo(ctx, p, pal, cx, p.Y(0.72), 1.2 * k));
      },
      name(ctx, p, pal, text, cx, cy, k) {
        p.iso(ctx, cx, cy, 0, () => {
          const t = text.toUpperCase();
          const s = fitFont(ctx, t, FONTS.head, 800, 1.28 * p.ppu * k, 0.34 * p.ppu * k, 0.06);
   ctx.textBaseline = 'middle'; ctx.lineJoin = 'round'; ctx.lineWidth = s * 0.16;
   ctx.strokeStyle = pal.c2 === '#050506' ? '#e41e2b' : pal.c2;
          spaced(ctx, t, 0, 0, s * 0.06, 'stroke');
   ctx.fillStyle = pal.ink; spaced(ctx, t, 0, 0, s * 0.06, 'fill');
        });
      }
    },
    neon: {
      bg(ctx, p, pal) {
        const g = ctx.createLinearGradient(0, p.Y(CAN.neckTop), 0, p.Y(CAN.bodyBot));
        g.addColorStop(0, '#050509'); g.addColorStop(0.5, mixHex('#050509', pal.c1, 0.22)); g.addColorStop(1, '#050509');
        ctx.fillStyle = g; ctx.fillRect(0, 0, p.W, p.H);
      },
      deco(ctx, p, pal) {
 ctx.save();
 ctx.shadowColor = pal.neon; ctx.shadowBlur = 22; ctx.fillStyle = pal.neon;
 [1.16, 1.06, -1.5, -1.6].forEach((y, i) => ctx.fillRect(0, p.Y(y) - 3, p.W, i % 2 ? 3 : 6));
        ctx.restore();
 ctx.strokeStyle = rgba(pal.neon, 0.16); ctx.lineWidth = 2;
        for (let i = 0; i < 16; i++) { const x = (p.W / 16) * i; ctx.beginPath(); ctx.moveTo(x, p.Y(1.0)); ctx.lineTo(x, p.Y(-1.45)); ctx.stroke(); }
        front(p, (cx, k) => drawLogo(ctx, p, pal, cx, p.Y(0.72), 1.2 * k, { mode: 'neon' }));
      },
      name(ctx, p, pal, text, cx, cy, k) {
        p.iso(ctx, cx, cy, 0, () => {
   const t = text.toUpperCase();
   const s = fitFont(ctx, t, FONTS.head, 800, 1.28 * p.ppu * k, 0.34 * p.ppu * k, 0.1);
   ctx.textBaseline = 'middle'; ctx.lineJoin = 'round';
   ctx.strokeStyle = pal.neon; ctx.lineWidth = s * 0.07;
   [s * 0.7, s * 0.35, s * 0.12].forEach((b) => { ctx.shadowColor = pal.neon; ctx.shadowBlur = b; spaced(ctx, t, 0, 0, s * 0.1, 'stroke'); });
   ctx.shadowBlur = s * 0.1; ctx.fillStyle = '#ffffff'; spaced(ctx, t, 0, 0, s * 0.1, 'fill');
        });
      }
    },
    retro: {
      bg(ctx, p, pal) {
        const g = ctx.createLinearGradient(0, p.Y(CAN.neckTop), 0, p.Y(CAN.bodyBot));
        g.addColorStop(0, pal.c2); g.addColorStop(0.15, pal.c1); g.addColorStop(1, pal.c1);
 ctx.fillStyle = g; ctx.fillRect(0, 0, p.W, p.H);
        const cream = '#fff1d0';
 [[1.12, 0.07, cream], [1.0, 0.035, pal.c2], [0.92, 0.035, cream], [-1.38, 0.035, cream], [-1.46, 0.035, pal.c2], [-1.54, 0.07, cream]]
   .forEach(([y, t, c]) => hline(ctx, p, y, t, c));
      },
      deco(ctx, p, pal) {
        front(p, (cx, k) => drawLogo(ctx, p, pal, cx, p.Y(0.55), 1.15 * k, { color: '#fff1d0', ribbon: '#fff1d0' }));
      },
      name(ctx, p, pal, text, cx, cy, k) {
        p.iso(ctx, cx, cy, 0, () => {
   const maxW = 1.2 * p.ppu * k, maxH = 0.4 * p.ppu * k;
   const s = fitFont(ctx, text, FONTS.logo, 400, maxW, maxH, 0.02);
   const w = Math.min(maxW, ctx.measureText(text).width) + s * 0.7, h = s * 1.25;
   rrect(ctx, -w / 2, -h / 2, w, h, h / 2);
          ctx.fillStyle = '#fff1d0'; ctx.fill();
          ctx.lineWidth = s * 0.07; ctx.strokeStyle = pal.c2 === '#050506' ? '#e41e2b' : pal.c2; ctx.stroke();
          ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
   ctx.fillStyle = pal.c2 === '#050506' ? '#050506' : pal.c2; ctx.fillText(text, 0, s * 0.04);
        });
      }
    }
  };

  const silverFill = (ctx, W, H, rng) => {
    const g = ctx.createLinearGradient(0, 0, 0, H);
    g.addColorStop(0, '#e3e7ec'); g.addColorStop(0.5, '#b9bfc7'); g.addColorStop(1, '#d5d9df');
    ctx.fillStyle = g; ctx.fillRect(0, 0, W, H);
    ctx.lineWidth = 1;
    for (let i = 0; i < 700; i++) { // aluminium brossé
      const y = rng() * H;
      ctx.strokeStyle = rng() > 0.5 ? 'rgba(255,255,255,0.07)' : 'rgba(0,0,0,0.06)';
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(W, y + (rng() - 0.5) * 2); ctx.stroke();
    }
  };
  const bubblesDecor = (ctx, p, rng, top, bot) => {
    ctx.strokeStyle = 'rgba(255,255,255,0.14)'; ctx.fillStyle = 'rgba(255,255,255,0.03)';
    for (let i = 0; i < 90; i++) {
      const x = rng() * p.W, y = top + rng() * (bot - top), r = (4 + rng() * rng() * 24) * (p.W / 2048);
      ctx.lineWidth = Math.max(1, r * 0.12);
      p.iso(ctx, x, y, 0, () => { ctx.beginPath(); ctx.arc(0, 0, r, 0, TAU); ctx.fill(); ctx.stroke(); });
    }
  };
  const sideLabels = (ctx, p, pal, rng) => {
    [0.25, 0.75].forEach((u) => {
      p.iso(ctx, p.W * u, p.Y(0.1), -Math.PI / 2, () => {
        ctx.font = '700 ' + 0.075 * p.ppu + 'px ' + FONTS.head;
        ctx.textBaseline = 'middle'; ctx.fillStyle = rgba(pal.ink, 0.5);
        spaced(ctx, u < 0.5 ? 'ICE COLD · REAL MAGIC' : 'TASTE THE FEELING', 0, 0, 0.012 * p.ppu, 'fill');
      });
    });
  };
  const finishBg = (ctx, p, pal, finish, top, bot) => {
    if (finish !== 'chrome') return;
    ctx.fillStyle = mixHex(pal.c1, '#c9d1da', 0.55); ctx.fillRect(0, top, p.W, bot - top);
    for (let i = 0; i < 12; i++) {
      const x = (p.W / 12) * i, w = p.W / 24;
      ctx.fillStyle = i % 2 ? 'rgba(255,255,255,0.20)' : 'rgba(0,0,0,0.16)';
      ctx.fillRect(x, top, w, bot - top);
    }
  };
  const finishTop = (ctx, p, finish, top, bot, rng) => {
    if (finish === 'matte') {
      for (let i = 0; i < 5000; i++) { ctx.fillStyle = rng() > 0.5 ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.06)'; ctx.fillRect(rng() * p.W, top + rng() * (bot - top), 2, 2); }
    } else if (finish === 'frosted') {
      ctx.fillStyle = 'rgba(255,255,255,0.24)'; ctx.fillRect(0, top, p.W, bot - top);
      for (let i = 0; i < 6000; i++) { ctx.fillStyle = rng() > 0.5 ? 'rgba(255,255,255,0.18)' : 'rgba(210,225,240,0.14)'; const s = 1 + rng() * 2.5; ctx.fillRect(rng() * p.W, top + rng() * (bot - top), s, s); }
    }
  };

  const paintBase = (ctx, p, o) => {
    const { W, H } = p, pal = RANGES[o.range] || RANGES.classic, st = STYLES[o.style] || STYLES.classic, rng = mulberry32(7);
    ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.globalCompositeOperation = 'source-over'; ctx.globalAlpha = 1;
    ctx.shadowColor = 'transparent'; ctx.shadowBlur = 0;
    silverFill(ctx, W, H, rng);
    const top = p.Y(CAN.neckTop), bot = p.Y(CAN.bodyBot);
    ctx.save(); ctx.beginPath(); ctx.rect(0, top, W, bot - top); ctx.clip();
    st.bg(ctx, p, pal);
    finishBg(ctx, p, pal, o.finish, top, bot);
    bubblesDecor(ctx, p, rng, top, bot);
    sideLabels(ctx, p, pal, rng);
    st.deco(ctx, p, pal);
    finishTop(ctx, p, o.finish, top, bot, rng);
    ctx.restore();
    ctx.fillStyle = 'rgba(0,0,0,0.25)'; ctx.fillRect(0, top - 2, W, 3); ctx.fillRect(0, bot - 1, W, 3); // sertissage
  };
  const paintNames = (ctx, p, o) => {
    const st = STYLES[o.style] || STYLES.classic, pal = RANGES[o.range] || RANGES.classic;
    const text = o.name || 'Taste the Feeling', k = o.name ? 1 : 0.62;
    [[0.5, 1], [0, 0.62], [1, 0.62]].forEach(([u, kk]) => {
      ctx.save(); st.name(ctx, p, pal, text, p.W * u, p.Y(-0.3), k * kk); ctx.restore();
    });
  };

  /* Gouttes de condensation : positions partagées entre texture couleur et carte bosse/rugosité */
  const dropData = (() => {
    const rng = mulberry32(21), list = [];
    const vMin = (CAN.yMax - (CAN.neckTop - 0.06)) / (CAN.yMax - CAN.yMin), vMax = (CAN.yMax - (CAN.bodyBot + 0.06)) / (CAN.yMax - CAN.yMin);
    const add = (n, rMin, rMax, big) => {
      for (let i = 0; i < n; i++) list.push({ u: rng(), v: vMin + rng() * (vMax - vMin), r: (rMin + rng() * rng() * (rMax - rMin)) / 2048, big });
    };
    add(520, 2.2, 9, false); add(34, 9, 17, true);
    return list;
  })();
  const dropLayers = {};
  const getDropLayer = (size) => {
    if (dropLayers[size]) return dropLayers[size];
    const c = makeCanvas(size, size), x = c.getContext('2d');
    dropData.forEach((d) => {
      const px = d.u * size, py = d.v * size, r = Math.max(1.2, d.r * size);
      const ry = r / CAN.af; // compense la densité verticale
      if (d.big && d.v < 0.9) { // traînée sous les grosses gouttes
        const len = ry * (4 + (d.u * 997 % 1) * 6);
 const g = x.createLinearGradient(0, py, 0, py + len);
 g.addColorStop(0, 'rgba(255,255,255,0.20)'); g.addColorStop(1, 'rgba(255,255,255,0)');
        x.fillStyle = g; x.fillRect(px - r * 0.28, py, r * 0.56, len);
      }
      x.save(); x.translate(px, py); x.scale(1, 1 / CAN.af * 1.08);
      const g = x.createRadialGradient(-r * 0.25, -r * 0.3, r * 0.05, 0, 0, r);
      g.addColorStop(0, 'rgba(255,255,255,0.55)'); g.addColorStop(0.45, 'rgba(255,255,255,0.10)');
      g.addColorStop(0.85, 'rgba(0,0,0,0.20)'); g.addColorStop(1, 'rgba(255,255,255,0.38)');
      x.fillStyle = g; x.beginPath(); x.arc(0, 0, r, 0, TAU); x.fill();
      x.fillStyle = 'rgba(255,255,255,0.9)'; x.beginPath(); x.arc(-r * 0.35, -r * 0.38, Math.max(0.7, r * 0.2), 0, TAU); x.fill();
      x.restore();
    });
    dropLayers[size] = c;
    return c;
  };
  const dropMasks = {};
  const getDropMask = (size) => {
    if (dropMasks[size]) return dropMasks[size];
    const c = makeCanvas(size, size), x = c.getContext('2d');
    x.fillStyle = '#000'; x.fillRect(0, 0, size, size);
    dropData.forEach((d) => {
      const px = d.u * size, py = d.v * size, r = Math.max(1.2, d.r * size);
      x.save(); x.translate(px, py); x.scale(1, 1 / CAN.af * 1.08);
      const g = x.createRadialGradient(0, 0, 0, 0, 0, r);
      g.addColorStop(0, 'rgba(255,255,255,1)'); g.addColorStop(0.7, 'rgba(255,255,255,0.7)'); g.addColorStop(1, 'rgba(255,255,255,0)');
      x.fillStyle = g; x.beginPath(); x.arc(0, 0, r, 0, TAU); x.fill();
      x.restore();
    });
    const data = x.getImageData(0, 0, size, size).data, out = new Uint8ClampedArray(size * size);
    for (let i = 0; i < out.length; i++) out[i] = data[i * 4];
    dropMasks[size] = out;
    return out;
  };
  // Carte « matériau » : R = bosse (gouttes), G = rugosité, B = métal (convention Three.js)
  const buildMatCanvas = (size, finish) => {
    const f = FINISHES[finish] || FINISHES.glossy, mask = getDropMask(size);
    const cv = makeCanvas(size, size), cx = cv.getContext('2d'), img = cx.createImageData(size, size), d = img.data;
    for (let y = 0; y < size; y++) {
      const wy = CAN.yMax - (y / size) * (CAN.yMax - CAN.yMin);
      const painted = wy <= CAN.neckTop && wy >= CAN.bodyBot;
      const r0 = painted ? f.rough : 0.16, m0 = painted ? f.metal : 1.0;
      for (let x = 0; x < size; x++) {
 const i = (y * size + x) * 4, mk = mask[y * size + x] / 255;
 d[i] = mask[y * size + x];
        d[i + 1] = (r0 * (1 - mk) + 0.05 * mk) * 255;
 d[i + 2] = m0 * (1 - mk) * 255;
 d[i + 3] = 255;
      }
    }
    cx.putImageData(img, 0, 0);
    return cv;
  };
  const buildLidCanvas = () => {
    const s = 512, c = makeCanvas(s, s), x = c.getContext('2d'), rng = mulberry32(5);
    const g = x.createRadialGradient(s / 2, s / 2, 10, s / 2, s / 2, s / 2);
    g.addColorStop(0, '#dfe3e8'); g.addColorStop(0.7, '#c4cad2'); g.addColorStop(1, '#a9b0b9');
    x.fillStyle = g; x.fillRect(0, 0, s, s);
    for (let i = 0; i < 500; i++) { // brossage circulaire
      x.strokeStyle = rng() > 0.5 ? 'rgba(255,255,255,0.08)' : 'rgba(0,0,0,0.07)';
      x.beginPath(); x.arc(s / 2, s / 2, rng() * s * 0.5, 0, TAU); x.stroke();
    }
    x.strokeStyle = 'rgba(0,0,0,0.28)'; x.lineWidth = 3; // embossage + ligne de découpe de l'ouverture
    x.beginPath(); x.arc(s / 2, s / 2, s * 0.43, 0, TAU); x.stroke();
    x.beginPath(); x.ellipse(s / 2, s * 0.26, s * 0.2, s * 0.115, 0, 0, TAU); x.stroke();
    x.fillStyle = 'rgba(255,255,255,0.5)'; x.beginPath(); x.arc(s / 2, s * 0.6, 5, 0, TAU); x.fill();
    return c;
  };

  /* ───────────────────────── 8. Ressorts & particules ───────────────── */
  class Spring {
    constructor(k, c, x = 0) { this.k = k; this.c = c; this.x = x; this.v = 0; this.t = x; }
    step(dt) {
      const n = Math.max(1, Math.ceil(dt / 0.008)), h = dt / n;
      for (let i = 0; i < n; i++) { this.v += (this.k * (this.t - this.x) - this.c * this.v) * h; this.x += this.v * h; }
      return this.x;
    }
  }

  const POINT_VERT = [
    'attribute float aSize; attribute float aAlpha; uniform float uPx; varying float vA;',
    'void main(){ vA = aAlpha; vec4 mv = modelViewMatrix * vec4(position, 1.0);',
    'gl_PointSize = max(1.0, aSize * uPx / -mv.z); gl_Position = projectionMatrix * mv; }'
  ].join('\n');
  const POINT_FRAG = [
    'uniform sampler2D uMap; uniform vec3 uColor; varying float vA;',
    'void main(){ vec4 c = texture2D(uMap, gl_PointCoord); gl_FragColor = vec4(uColor * c.rgb, c.a * vA); }'
  ].join('\n');

  class Particles {
    // kind : 'ambient' (bulles en boucle) ou 'burst' (gerbes ponctuelles)
    constructor(n, kind, tex) {
      this.n = n; this.kind = kind; this.active = n;
      this.pos = new Float32Array(n * 3); this.vel = new Float32Array(n * 3);
      this.size = new Float32Array(n); this.sizeBase = new Float32Array(n);
      this.alpha = new Float32Array(n); this.baseA = new Float32Array(n);
      this.life = new Float32Array(n); this.maxLife = new Float32Array(n); this.ph = new Float32Array(n);
      this.geo = new THREE.BufferGeometry();
      this.aPos = new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage);
      this.aSize = new THREE.BufferAttribute(this.size, 1).setUsage(THREE.DynamicDrawUsage);
      this.aAlpha = new THREE.BufferAttribute(this.alpha, 1).setUsage(THREE.DynamicDrawUsage);
      this.geo.setAttribute('position', this.aPos);
      this.geo.setAttribute('aSize', this.aSize);
      this.geo.setAttribute('aAlpha', this.aAlpha);
      this.uniforms = { uMap: { value: tex }, uColor: { value: new THREE.Color(0xffffff) }, uPx: { value: 400 } };
      this.mat = new THREE.ShaderMaterial({ uniforms: this.uniforms, vertexShader: POINT_VERT, fragmentShader: POINT_FRAG, transparent: true, depthWrite: false });
      this.points = new THREE.Points(this.geo, this.mat);
      this.points.frustumCulled = false;
      if (kind === 'ambient') for (let i = 0; i < n; i++) this.respawn(i, true);
    }
    respawn(i, init) {
      const j = i * 3, a = Math.random() * TAU, r = rand(0.95, 2.3);
      this.pos[j] = Math.cos(a) * r; this.pos[j + 2] = Math.sin(a) * r * 0.8 - 0.2;
      this.pos[j + 1] = init ? rand(-2.4, 3.0) : rand(-2.5, -2.2);
      this.vel[j + 1] = rand(0.22, 0.85);
      this.sizeBase[i] = rand(0.035, 0.12) * (Math.random() < 0.12 ? 1.8 : 1);
      this.baseA[i] = rand(0.35, 0.9); this.ph[i] = rand(0, TAU);
    }
    setDensity(d) { this.active = Math.round(this.n * clamp(d, 0, 1)); }
    emit(origin, count, power = 1) {
      let c = 0;
      for (let i = 0; i < this.n && c < count; i++) {
        if (this.life[i] > 0) continue;
        const j = i * 3, a = Math.random() * TAU, s = rand(1.4, 4.6) * power, spread = rand(0.12, 0.5);
        this.pos[j] = origin.x + Math.cos(a) * 0.12; this.pos[j + 1] = origin.y; this.pos[j + 2] = origin.z + Math.sin(a) * 0.12;
 this.vel[j] = Math.cos(a) * s * spread; this.vel[j + 1] = s; this.vel[j + 2] = Math.sin(a) * s * spread;
        this.life[i] = this.maxLife[i] = rand(0.7, 1.8);
        this.sizeBase[i] = rand(0.04, 0.14); c++;
      }
    }
    update(dt, t, speed = 1) {
      const P = this.pos, V = this.vel;
      for (let i = 0; i < this.n; i++) {
        const j = i * 3;
        if (this.kind === 'ambient') {
   if (i >= this.active) { this.alpha[i] = 0; continue; }
          P[j + 1] += V[j + 1] * dt * speed;
   P[j] += (Math.sin(t * (0.8 + this.ph[i] * 0.1) + this.ph[i]) * 0.22 + Math.sin(t * 2.1 + this.ph[i] * 2) * 0.06) * dt;
          P[j + 2] += Math.cos(t * 0.7 + this.ph[i]) * 0.18 * dt;
   if (P[j + 1] > 3.1) this.respawn(i, false);
   const y = P[j + 1];
          this.alpha[i] = this.baseA[i] * clamp(Math.min((y + 2.6) / 0.6, (3.1 - y) / 0.8), 0, 1);
   this.size[i] = this.sizeBase[i];
 } else if (this.life[i] > 0) {
   this.life[i] -= dt;
   V[j + 1] -= 4.2 * dt;
   const drag = Math.max(0, 1 - 1.2 * dt);
   V[j] *= drag; V[j + 1] *= drag; V[j + 2] *= drag;
   P[j] += V[j] * dt; P[j + 1] += V[j + 1] * dt; P[j + 2] += V[j + 2] * dt;
   const lr = clamp(this.life[i] / this.maxLife[i], 0, 1);
          this.alpha[i] = Math.pow(lr, 0.7) * 0.9;
   this.size[i] = this.sizeBase[i] * (1 + (1 - lr) * 0.5);
        } else {
   this.alpha[i] = 0;
        }
      }
      this.aPos.needsUpdate = true; this.aSize.needsUpdate = true; this.aAlpha.needsUpdate = true;
    }
  }

  /* ───────────────────────── 9. Environnement (reflets studio) ──────── */
  const buildEnvTexture = (renderer) => {
    const envScene = new THREE.Scene();
    const cv = makeCanvas(8, 256), c = cv.getContext('2d');
    const g = c.createLinearGradient(0, 0, 0, 256);
    g.addColorStop(0, '#59616e'); g.addColorStop(0.45, '#2b2f38'); g.addColorStop(0.55, '#1a1c22'); g.addColorStop(1, '#0c0d10');
    c.fillStyle = g; c.fillRect(0, 0, 8, 256);
    const sky = new THREE.CanvasTexture(cv); sky.encoding = THREE.sRGBEncoding;
    envScene.add(new THREE.Mesh(new THREE.SphereGeometry(20, 32, 16), new THREE.MeshBasicMaterial({ map: sky, side: THREE.BackSide })));
    // Softboxes : leur intensité > 1 donne les reflets verticaux brillants sur le métal
    [[7, 4, [6, 5, 8], 8], [1.2, 9, [-9, 0, 3], 6], [1.2, 9, [9, 0, 2], 5], [8, 2, [0, 10, 0], 3.5], [1, 8, [-3, 0, -9], 4], [1, 8, [3, 0, -9], 4]]
      .forEach(([w, h, pos, k]) => {
        const m = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ color: new THREE.Color(k, k, k), side: THREE.DoubleSide }));
        m.position.set(pos[0], pos[1], pos[2]); m.lookAt(0, 0, 0); envScene.add(m);
      });
    const pm = new THREE.PMREMGenerator(renderer);
    const rt = pm.fromScene(envScene, 0.03);
    pm.dispose();
    envScene.traverse((o) => { if (o.geometry) o.geometry.dispose(); if (o.material) o.material.dispose(); });
    sky.dispose();
    return rt.texture;
  };

  /* ───────────────────────── 10. Scène 3D de canette ────────────────── */
  const canProfile = () => {
    const R = CAN.R;
    const pts = [
      [0, -1.80], [0.18, -1.805], [0.34, -1.83], [0.47, -1.89], [0.55, -1.96], [0.60, -1.995], [0.65, -2.0],
      [0.70, -1.985], [0.745, -1.945], [0.78, -1.88], [0.795, -1.80], [R, -1.72],      // base cintrée
      [R, -1.0], [R, -0.2], [R, 0.6], [R, 1.3],      // fût
      [0.797, 1.36], [0.787, 1.43], [0.768, 1.50], [0.74, 1.57], [0.705, 1.635], [0.675, 1.69], [0.664, 1.73], // épaule
      [0.662, 1.78], [0.676, 1.808], [0.692, 1.835], [0.69, 1.865], [0.672, 1.885], [0.645, 1.89],     // col + sertissage
      [0.62, 1.878], [0.60, 1.855], [0.59, 1.83], [0.585, 1.80]    // paroi intérieure de l'opercule
    ];
    return pts.map((q) => new THREE.Vector2(q[0], q[1]));
  };

  const tabShape = () => {
    const s = new THREE.Shape(), x0 = -0.15, x1 = 0.15, y0 = -0.26, y1 = 0.28, r = 0.1;
    s.moveTo(x0 + r, y0); s.lineTo(x1 - r, y0); s.quadraticCurveTo(x1, y0, x1, y0 + r);
    s.lineTo(x1, y1 - r); s.quadraticCurveTo(x1, y1, x1 - r, y1); s.lineTo(x0 + r, y1);
    s.quadraticCurveTo(x0, y1, x0, y1 - r); s.lineTo(x0, y0 + r); s.quadraticCurveTo(x0, y0, x0 + r, y0);
    const hole = new THREE.Path(); hole.absellipse(0, -0.14, 0.085, 0.07, 0, TAU, true);
    s.holes.push(hole);
    return s;
  };

  class CanScene {
    static create(container, opts) {
      try { return new CanScene(container, opts); } catch (e) {
 console.warn('[Coca] 3D indisponible :', e);
        $$('canvas', container).forEach((c) => c.remove());
        return null;
      }
    }

    constructor(container, opts) {
      this.container = container; this.opts = opts;
      this.range = state.range;
      this.style = opts.names ? state.style : 'classic';
      this.finish = opts.names ? state.finish : 'glossy';
      this.name = opts.names ? state.name : '';
      this.visible = true; this.dead = false; this.dirty = true; this.shown = false;
      this.w = 0; this.h = 0; this.age = 0; this.frames = 0; this.acc = 0;
      this.yaw = 0; this.yawVel = 0; this.pitch = 0; this.pitchT = 0; this.idle = 0; this.snap = null; this.drag = null;
      this.baseRoll = opts.kind === 'hero' ? -0.12 : 0;
      this.squash = new Spring(110, 9, 1); this.hop = new Spring(110, 9, 0);
      this.tab = new Spring(70, 9, 0); this.openAmt = new Spring(90, 11, 0);
      this.wobble = 0; this.flash = 0; this.excite = 0; this.seq = null;

      const r = this.renderer = new THREE.WebGLRenderer({ antialias: !mqCoarse.matches, alpha: true, powerPreference: 'high-performance' });
      this.pr = Math.min(window.devicePixelRatio || 1, mqCoarse.matches ? 1.75 : 2);
      r.setPixelRatio(this.pr);
      r.outputEncoding = THREE.sRGBEncoding;
      r.toneMapping = THREE.ACESFilmicToneMapping; r.toneMappingExposure = 1.05;
      r.setClearColor(0x000000, 0);
      this.canvas = r.domElement;
      container.insertBefore(this.canvas, container.firstChild);
      this.canvas.addEventListener('webglcontextlost', (e) => e.preventDefault());

      this.scene = new THREE.Scene();
      this.camera = new THREE.PerspectiveCamera(28, 1, 0.1, 60);
      this.scene.environment = buildEnvTexture(r);
      this.aniso = Math.min(8, r.capabilities.getMaxAnisotropy());

      this.buildLights();
      this.buildCan();
      this.buildBackdrop();
      this.buildParticles();
      this.bindPointer();
      this.observe();
      this.resize(true);
      ticker.add(this);
      ticker.wake();
    }

    isActive() { return this.visible && !document.hidden && !this.dead && !!this.renderer; }

    /* --- Construction --- */
    buildLights() {
      const pal = RANGES[this.range];
      this.key = new THREE.SpotLight(0xffffff, 1.4, 0, 0.6, 0.7, 1); this.key.position.set(3.5, 4.5, 6);
      this.fill = new THREE.HemisphereLight(0xffffff, 0x200608, 0.35);
      this.rimA = new THREE.DirectionalLight(pal.rim, 1.3); this.rimA.position.set(-4, 1.5, -3);
      this.rimB = new THREE.DirectionalLight(pal.rim2, 0.9); this.rimB.position.set(4, 0.5, -3);
      this.tRimA = new THREE.Color(pal.rim); this.tRimB = new THREE.Color(pal.rim2); this.tGlow = new THREE.Color(pal.glow);
      this.scene.add(this.key, this.fill, this.rimA, this.rimB);
    }

    buildCan() {
      const maxA = this.aniso;
      this.root = new THREE.Group(); this.can = new THREE.Group();
      this.root.add(this.can); this.scene.add(this.root);

      // Corps : révolution du profil, UV recalculés en fonction de la hauteur
      const geo = new THREE.LatheGeometry(canProfile(), mqCoarse.matches ? 72 : 128);
      const pos = geo.attributes.position, uv = geo.attributes.uv;
      for (let i = 0; i < pos.count; i++) uv.setY(i, (pos.getY(i) - CAN.yMin) / (CAN.yMax - CAN.yMin));
      uv.needsUpdate = true;

      this.painter = new Painter(TEX, TEX);
      this.texCanvas = makeCanvas(TEX, TEX); this.texCtx = this.texCanvas.getContext('2d');
      if (this.opts.names) { this.baseCanvas = makeCanvas(TEX, TEX); this.baseCtx = this.baseCanvas.getContext('2d'); }
      this.tex = new THREE.CanvasTexture(this.texCanvas);
      this.tex.encoding = THREE.sRGBEncoding; this.tex.anisotropy = maxA; this.tex.wrapS = THREE.RepeatWrapping;
      this.matCanvas = makeCanvas(1, 1);
      this.matTex = new THREE.CanvasTexture(this.matCanvas);
      this.matTex.wrapS = THREE.RepeatWrapping; this.matTex.anisotropy = maxA;
      this.matSize = 1024; this.baseKey = ''; this.matFinish = '';

      this.bodyMat = new THREE.MeshStandardMaterial({
        map: this.tex, roughnessMap: this.matTex, metalnessMap: this.matTex, bumpMap: this.matTex,
        bumpScale: 2, roughness: 1, metalness: 1, envMapIntensity: 1.15
      });
      this.body = new THREE.Mesh(geo, this.bodyMat);
      this.can.add(this.body);

      // Opercule en aluminium brossé
      const lidTex = new THREE.CanvasTexture(buildLidCanvas()); lidTex.encoding = THREE.sRGBEncoding; lidTex.anisotropy = maxA;
      const lidGeo = new THREE.CircleGeometry(0.585, 64); lidGeo.rotateX(-Math.PI / 2);
      this.lid = new THREE.Mesh(lidGeo, new THREE.MeshStandardMaterial({ map: lidTex, metalness: 1, roughness: 0.35, envMapIntensity: 1.1 }));
      this.lid.position.y = CAN.lidY; this.can.add(this.lid);

      // Anneau de sertissage chromé
      const ringGeo = new THREE.TorusGeometry(0.655, 0.032, 14, 96); ringGeo.rotateX(Math.PI / 2);
      this.ring = new THREE.Mesh(ringGeo, new THREE.MeshStandardMaterial({ color: 0xdfe4ea, metalness: 1, roughness: 0.08, envMapIntensity: 1.5 }));
      this.ring.position.y = 1.862; this.can.add(this.ring);

      // Ouverture (visible une fois la canette ouverte)
      const opGeo = new THREE.CircleGeometry(1, 40); opGeo.rotateX(-Math.PI / 2);
      this.opening = new THREE.Mesh(opGeo, new THREE.MeshStandardMaterial({ color: 0x0b0504, metalness: 0.2, roughness: 0.45 }));
      this.opening.position.set(0, CAN.lidY + 0.003, -0.28); this.opening.scale.set(0.0001, 1, 0.0001); this.opening.visible = false;
      this.can.add(this.opening);

      // Languette (pivot au niveau du rivet)
      const tabGeo = new THREE.ExtrudeGeometry(tabShape(), { depth: 0.014, bevelEnabled: false, curveSegments: 14 });
      tabGeo.rotateX(-Math.PI / 2);
      const alu = new THREE.MeshStandardMaterial({ color: 0xcfd5dc, metalness: 1, roughness: 0.22, envMapIntensity: 1.3 });
      this.tabPivot = new THREE.Group(); this.tabPivot.position.set(0, CAN.lidY + 0.012, 0.05);
      this.tabPivot.add(new THREE.Mesh(tabGeo, alu));
      const rivet = new THREE.Mesh(new THREE.CylinderGeometry(0.045, 0.05, 0.04, 20), alu); rivet.position.y = 0.014;
      this.tabPivot.add(rivet); this.can.add(this.tabPivot);
    }

    buildBackdrop() {
      this.haloTex = radialTexture([[0, 'rgba(255,255,255,0.95)'], [0.35, 'rgba(255,255,255,0.35)'], [1, 'rgba(255,255,255,0)']]);
      this.halo = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.haloTex, color: new THREE.Color(RANGES[this.range].glow), blending: THREE.AdditiveBlending, transparent: true, depthWrite: false, opacity: 0.5 }));
      this.halo.scale.set(7.5, 7.5, 1); this.halo.position.set(0, 0, -1.6); this.scene.add(this.halo);
      this.shadowTex = radialTexture([[0, 'rgba(0,0,0,0.65)'], [0.6, 'rgba(0,0,0,0.25)'], [1, 'rgba(0,0,0,0)']]);
      const sg = new THREE.PlaneGeometry(1, 1); sg.rotateX(-Math.PI / 2);
      this.shadow = new THREE.Mesh(sg, new THREE.MeshBasicMaterial({ map: this.shadowTex, transparent: true, depthWrite: false }));
      this.shadow.position.y = -2.06; this.shadow.scale.set(3.2, 1, 3.2); this.scene.add(this.shadow);
    }

    buildParticles() {
      this.bubbleTex = bubbleTexture();
      const nAmb = mqCoarse.matches ? Math.round(this.opts.bubbles * 0.55) : this.opts.bubbles;
      this.ambient = new Particles(nAmb, 'ambient', this.bubbleTex);
      this.burst = new Particles(240, 'burst', this.bubbleTex);
      this.scene.add(this.ambient.points, this.burst.points);
    }

    /* --- Interactions --- */
    bindPointer() {
      const el = this.canvas;
      el.addEventListener('pointerdown', (e) => {
 if (e.button > 0) return;
 try { el.setPointerCapture(e.pointerId); } catch (err) { /* ignore */ }
        this.drag = { id: e.pointerId, x: e.clientX, y: e.clientY, t0: performance.now(), moved: 0, last: e.timeStamp, type: e.pointerType };
 this.snap = null;
      });
      el.addEventListener('pointermove', (e) => {
        const d = this.drag;
 if (!d || d.id !== e.pointerId) return;
 const dx = e.clientX - d.x, dy = e.clientY - d.y;
        d.x = e.clientX; d.y = e.clientY; d.moved += Math.abs(dx) + Math.abs(dy);
        const dYaw = dx * (TAU / Math.max(320, this.w * 0.9));
        this.yaw += dYaw;
        if (d.type !== 'touch') this.pitchT = clamp(this.pitchT + dy * 0.004, -0.35, 0.35);
        const dtm = Math.max(e.timeStamp - d.last, 1) / 1000; d.last = e.timeStamp;
        this.yawVel = lerp(this.yawVel, clamp(dYaw / dtm, -14, 14), 0.4);
      });
      const end = (e) => {
 const d = this.drag;
        if (!d || d.id !== e.pointerId) return;
        this.drag = null; this.idle = 0;
        try { el.releasePointerCapture(e.pointerId); } catch (err) { /* ignore */ }
        if (e.type === 'pointerup' && d.moved < 6 && performance.now() - d.t0 < 450 && this.opts.onTap) this.opts.onTap();
      };
      el.addEventListener('pointerup', end); el.addEventListener('pointercancel', end);
    }

    observe() {
      if (!('IntersectionObserver' in window)) return;
      this.io = new IntersectionObserver((entries) => {
        this.visible = entries[entries.length - 1].isIntersecting;
 if (this.visible) { this.resize(); ticker.wake(); }
      }, { rootMargin: '80px' });
      this.io.observe(this.container);
    }

    /* --- Changements d'état --- */
    markDirty() { this.dirty = true; if (this.isActive()) ticker.wake(); }
    setRange(id) {
      if (!RANGES[id] || id === this.range) return;
      this.range = id;
      const pal = RANGES[id];
      this.tRimA.set(pal.rim); this.tRimB.set(pal.rim2); this.tGlow.set(pal.glow);
      this.squash.v -= 1.6; this.flash = Math.max(this.flash, 0.45);
      this.markDirty();
    }
    setStyle(s) { if (STYLES[s] && s !== this.style) { this.style = s; this.markDirty(); } }
    setFinish(f) { f = FINISH_ALIAS[f] || 'glossy'; if (f !== this.finish) { this.finish = f; this.markDirty(); } }
    setName(n) { if (n !== this.name) { this.name = n; this.markDirty(); } }
    faceFront() { this.snap = Math.round(this.yaw / TAU) * TAU; this.yawVel = 0; ticker.wake(); }
    resetView() { this.snap = Math.round(this.yaw / TAU) * TAU; this.pitchT = 0; this.yawVel = 0; this.idle = 0; ticker.wake(); }

    paint() {
      const o = { range: this.range, style: this.style, finish: this.finish, name: this.name };
      const p = this.painter, key = o.range + '|' + o.style + '|' + o.finish;
      const finalCtx = this.texCtx;
      if (this.opts.names) {
 if (this.baseKey !== key) { paintBase(this.baseCtx, p, o); this.baseKey = key; }
 finalCtx.clearRect(0, 0, p.W, p.H); finalCtx.drawImage(this.baseCanvas, 0, 0);
      } else {
        paintBase(finalCtx, p, o);
      }
      paintNames(finalCtx, p, o);
      finalCtx.drawImage(getDropLayer(p.W), 0, 0);
      this.tex.needsUpdate = true;

      if (this.matFinish !== o.finish) {
 this.matFinish = o.finish;
        this.matTex.image = buildMatCanvas(this.matSize, o.finish);
 this.matTex.needsUpdate = true;
 this.bodyMat.envMapIntensity = (FINISHES[o.finish] || FINISHES.glossy).env;
 this.bodyMat.needsUpdate = true;
      }
    }
    flush() { if (this.dirty) { this.paint(); this.dirty = false; } }

    /* Ouverture de la canette : sursaut, languette, gerbe de bulles. Renvoie false si déjà en cours. */
    openCan() {
      if (this.seq) return false;
      const m = mqReduce.matches ? 0.35 : 1;
      this.seq = { t: 0, tab: false, fired: false };
      this.tab.x = 0; this.tab.v = 0; this.tab.t = 0;
      this.openAmt.x = 0; this.openAmt.v = 0; this.openAmt.t = 0; this.opening.visible = false;
      this.squash.v -= 3.4 * m; this.hop.v += 3.0 * m; this.wobble = m;
      this.yawVel += (Math.random() < 0.5 ? -1 : 1) * 1.2 * m;
      this.snap = null;
      ticker.wake();
      return true;
    }
    fire() {
      this.openAmt.t = 1; this.flash = 1; this.excite = 1;
      const v = new THREE.Vector3(0, 1.9, -0.28);
      this.root.updateMatrixWorld(true); this.can.localToWorld(v);
      this.burst.emit(v, mqReduce.matches ? 60 : 160, 1);
    }

    /* --- Boucle --- */
    update(dt, t) {
      if (this.dirty) { this.paint(); this.dirty = false; }
      this.age += dt;
      const motion = !mqReduce.matches;

      // Rotation : inertie (damping) + rotation lente au repos
      if (this.drag) this.idle = 0; else this.idle += dt;
      const typing = this.opts.kind === 'studio' && nameInput && document.activeElement === nameInput;
      const auto = motion && this.idle > this.opts.autoDelay && !typing ? this.opts.autoSpeed : 0;
      if (!this.drag) {
 if (this.snap !== null) {
   const d = this.snap - this.yaw;
          this.yaw += d * (1 - Math.exp(-dt * 7)); this.yawVel = 0;
   if (Math.abs(d) < 0.002) { this.yaw = this.snap; this.snap = null; }
        } else {
   this.yawVel += (auto - this.yawVel) * (1 - Math.exp(-dt * 1.4));
   this.yaw += this.yawVel * dt;
        }
        this.pitchT *= Math.exp(-dt * 1.6);
      }
      this.pitch += (this.pitchT - this.pitch) * (1 - Math.exp(-dt * 10));

      // Séquence d'ouverture
      if (this.seq) {
 const s = this.seq; s.t += dt;
        if (!s.tab && s.t >= 0.03) { s.tab = true; this.tab.t = 1; }
        if (!s.fired && s.t >= 0.11) { s.fired = true; this.fire(); this.opening.visible = true; }
 if (s.t >= 1.8) this.seq = null;
      }
      const sq = this.squash.step(dt), hop = this.hop.step(dt), tab = this.tab.step(dt), op = this.openAmt.step(dt);
      this.squash.t = 1; this.hop.t = 0;
      this.wobble *= Math.exp(-dt * 4.5); this.flash *= Math.exp(-dt * 3.2); this.excite *= Math.exp(-dt * 0.7);

      // Flottaison organique + transformations
      const fl = motion ? Math.sin(t * 0.9) * 0.06 + Math.sin(t * 0.37 + 1.3) * 0.03 : 0;
      this.root.position.y = fl + hop;
      this.can.rotation.set(
        this.pitch + (motion ? Math.sin(t * 0.5 + 0.7) * 0.02 : 0),
        FACE + this.yaw,
        this.baseRoll + (motion ? Math.sin(t * 0.6) * 0.025 : 0) + Math.sin(t * 34) * 0.05 * this.wobble,
        'YXZ'
      );
      const sx = 1 + (1 - sq) * 0.45;
      this.can.scale.set(sx, sq, sx);
      this.tabPivot.rotation.x = -tab * 1.05;
      const os = clamp(op, 0, 1.15);
      this.opening.scale.set(Math.max(0.0001, os * 0.17), 1, Math.max(0.0001, os * 0.1));
      const sh = 3.2 * (1 - clamp(hop, 0, 0.5) * 0.7 - fl * 0.4);
      this.shadow.scale.set(sh, 1, sh); this.shadow.material.opacity = 0.9 - clamp(hop, 0, 0.5) * 0.6;

      // Lumières : transition douce vers la gamme + flash à l'ouverture
      const k = 1 - Math.exp(-dt * 5);
      this.rimA.color.lerp(this.tRimA, k); this.rimB.color.lerp(this.tRimB, k); this.halo.material.color.lerp(this.tGlow, k);
      this.key.intensity = 1.4 + this.flash * 2.2;
      this.halo.material.opacity = 0.5 + this.flash * 0.5 + (motion ? Math.sin(t * 1.3) * 0.04 : 0);
      this.camera.position.x = Math.sin(t * 60) * 0.015 * this.flash;

      // Bulles (plus vives quand le fizz audio joue)
      const A = window.CocaAudio;
      const lvl = this.opts.audioReactive && A && A.isFizzing && A.isFizzing() ? A.getLevel() : 0;
      this.ambient.setDensity(motion ? 1 : 0.25);
      this.ambient.update(dt, t, (motion ? 1 : 0.3) * (1 + this.excite * 2.5 + (this.fizzing ? 0.8 : 0) + lvl * 3));
      this.burst.update(dt, t);

      this.adaptQuality(dt);
      this.renderer.render(this.scene, this.camera);
      if (!this.shown) { this.shown = true; if (this.opts.onFirstFrame) this.opts.onFirstFrame(); }
    }

    // Résolution dynamique : si la machine peine, on baisse le pixel ratio (jamais sous 1)
    adaptQuality(dt) {
      if (this.age < 3) return;
      this.frames++; this.acc += dt;
      if (this.acc < 2) return;
      const fps = this.frames / this.acc; this.frames = 0; this.acc = 0;
      if (fps < 40 && this.pr > 1) { this.pr = Math.max(1, this.pr - 0.25); this.resize(true); }
    }

    /* --- Redimensionnement --- */
    resize(force) {
      if (!this.renderer) return;
      const w = this.container.clientWidth, h = this.container.clientHeight;
      if (w < 2 || h < 2) return;
      if (!force && w === this.w && h === this.h) return;
      this.w = w; this.h = h;
      this.renderer.setPixelRatio(this.pr);
      this.renderer.setSize(w, h, false);
      this.camera.aspect = w / h;
      this.fit();
    }
    fit() {
      const t = Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2)), a = this.camera.aspect;
      const dist = Math.max(this.opts.fitH / 2 / t, this.opts.fitW / 2 / (t * a));
      this.camera.position.set(0, this.opts.camY, dist);
      this.camera.lookAt(0, this.opts.lookY, 0);
      this.camera.updateProjectionMatrix();
      const size = this.renderer.getDrawingBufferSize(new THREE.Vector2());
      const px = size.y / (2 * t);
      this.ambient.uniforms.uPx.value = px; this.burst.uniforms.uPx.value = px;
    }

    /* --- Export PNG (studio) : rendu haute résolution, fond aux couleurs de la gamme --- */
    exportPNG(w = 1200, h = 1500) {
      this.flush();
      const r = this.renderer, size = r.getSize(new THREE.Vector2()), pal = RANGES[this.range];
      let url = '';
      try {
        r.setPixelRatio(1); r.setSize(w, h, false);
        this.camera.aspect = w / h; this.fit();
        r.render(this.scene, this.camera);
 const out = makeCanvas(w, h), c = out.getContext('2d');
        const bg = c.createRadialGradient(w / 2, h * 0.45, 40, w / 2, h * 0.45, h * 0.75);
        bg.addColorStop(0, pal.c1 === '#1d1d22' ? '#3a3a42' : pal.c1); bg.addColorStop(1, '#0b0b0e');
        c.fillStyle = bg; c.fillRect(0, 0, w, h);
        c.drawImage(r.domElement, 0, 0, w, h);
        c.fillStyle = 'rgba(255,255,255,0.7)'; c.textAlign = 'center';
        c.font = '600 26px ' + FONTS.body;
 c.fillText((this.name ? this.name + ' · ' : '') + 'Coca-Cola Red Experience', w / 2, h - 44);
        url = out.toDataURL('image/png');
      } finally {
        r.setPixelRatio(this.pr); r.setSize(size.x, size.y, false);
        this.camera.aspect = size.x / size.y; this.fit();
        r.render(this.scene, this.camera);
      }
      return url;
    }
  }

  /* ───────────────────────── 11. Bulles du footer (canvas 2D) ───────── */
  class FooterBubbles {
    constructor(canvas) {
      this.c = canvas; this.x = canvas.getContext('2d'); this.list = []; this.visible = false; this.spawn = 0; this.dpr = 1;
      this.resize();
      for (let i = 0; i < 22; i++) this.add(true);
      const host = canvas.parentElement || canvas;
      if ('IntersectionObserver' in window) {
        new IntersectionObserver((e) => { this.visible = e[e.length - 1].isIntersecting; if (this.visible) ticker.wake(); }).observe(host);
      } else { this.visible = true; }
      ticker.add(this);
    }
    isActive() { return this.visible && !document.hidden && this.x; }
    resize() {
      const r = this.c.getBoundingClientRect(); this.dpr = Math.min(window.devicePixelRatio || 1, 2);
      const w = Math.round(r.width * this.dpr), h = Math.round(r.height * this.dpr);
      if (w > 0 && h > 0 && (this.c.width !== w || this.c.height !== h)) { this.c.width = w; this.c.height = h; }
    }
    add(anywhere, fast) {
      const H = this.c.height || 300, W = this.c.width || 600, r = rand(3, 14) * this.dpr;
      this.list.push({
 x: rand(0, W), y: anywhere ? rand(0, H) : H + r + rand(0, 30), r, ph: rand(0, TAU),
        vy: rand(26, 80) * this.dpr * (fast ? rand(1.5, 3) : 1), a: rand(0.25, 0.7)
      });
    }
    burst(n) { for (let i = 0; i < n; i++) this.add(false, true); ticker.wake(); }
    update(dt, t) {
      const x = this.x, W = this.c.width, H = this.c.height;
      if (!W || !H) { this.resize(); return; }
      if (!mqReduce.matches) { this.spawn += dt * 5; while (this.spawn > 1) { this.spawn -= 1; if (this.list.length < 90) this.add(false); } }
      x.clearRect(0, 0, W, H);
      x.lineWidth = Math.max(1, this.dpr);
      for (let i = this.list.length - 1; i >= 0; i--) {
        const b = this.list[i];
 b.y -= b.vy * dt; b.x += Math.sin(t * 1.3 + b.ph) * 14 * this.dpr * dt;
        if (b.y < -b.r * 2) { this.list.splice(i, 1); continue; }
 const fade = clamp(b.y / (H * 0.25), 0, 1) * b.a;
        x.strokeStyle = 'rgba(255,255,255,' + fade + ')'; x.fillStyle = 'rgba(255,255,255,' + fade * 0.12 + ')';
        x.beginPath(); x.arc(b.x, b.y, b.r, 0, TAU); x.fill(); x.stroke();
        x.fillStyle = 'rgba(255,255,255,' + fade * 0.9 + ')';
        x.beginPath(); x.arc(b.x - b.r * 0.35, b.y - b.r * 0.35, Math.max(1, b.r * 0.18), 0, TAU); x.fill();
      }
    }
  }

  /* ───────────────────────── 12. Gammes, thèmes ─────────────────────── */
  const setChecked = (items, active) => {
    items.forEach((b) => {
      const on = b === active;
      b.classList.toggle('is-active', on); b.setAttribute('aria-checked', on ? 'true' : 'false'); b.tabIndex = on ? 0 : -1;
    });
  };

  const setRange = (id, { notify = false, persist = true } = {}) => {
    if (!RANGES[id]) id = 'classic';
    state.range = id;
    root.dataset.theme = id;
    const btns = $$('.range__btn');
    setChecked(btns, btns.find((b) => b.dataset.range === id));
    $$('.flavor-card').forEach((c) => {
      const on = c.dataset.range === id;
      c.classList.toggle('is-active', on); if (on) c.setAttribute('aria-current', 'true'); else c.removeAttribute('aria-current');
    });
    const desc = $('#rangeDesc');
    if (desc) { desc.dataset.i18n = 'range.' + id; I18N.applyTo(desc.parentElement || desc); }
    const meta = $('meta[name="theme-color"]'); if (meta) meta.setAttribute('content', RANGES[id].c1);
    if (hero) hero.setRange(id);
    if (studio) studio.setRange(id);
    if (persist) store.set(CFG.KEYS.range, id);
    if (notify) toast(tr('range', { name: RANGES[id].label }));
  };

  /* Groupe radio accessible : clic + flèches clavier */
  const radioGroup = (container, selector, onSelect) => {
    container.addEventListener('click', (e) => {
      const b = e.target.closest(selector);
      if (b && container.contains(b)) onSelect(b);
    });
    container.addEventListener('keydown', (e) => {
      const dir = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
      const list = $$(selector, container);
      let next = null;
      if (dir) { const i = list.indexOf(document.activeElement); if (i >= 0) next = list[(i + dir + list.length) % list.length]; }
      else if (e.key === 'Home') next = list[0];
      else if (e.key === 'End') next = list[list.length - 1];
      if (next) { e.preventDefault(); next.focus(); onSelect(next); }
    });
  };

  /* ───────────────────────── 13. Studio (contrôles) ─────────────────── */
  const sanitizeName = (v) => v.replace(/[\u0000-\u001f\u007f<>]/g, '').replace(/\s+/g, ' ').trimStart().slice(0, 12);
  const slug = (s) => s.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

  const bindStudio = () => {
    nameInput = $('#canName');
    const count = $('#nameCount');
    const updateCount = () => { if (count) count.textContent = Array.from(nameInput.value).length + '/12'; };
    const styleBtns = $$('[data-style]'), finishBtns = $$('[data-finish]');

    if (nameInput) {
      nameInput.addEventListener('input', () => {
        const clean = sanitizeName(nameInput.value);
 if (clean !== nameInput.value) nameInput.value = clean;
        state.name = clean.trim(); updateCount();
        if (studio) { studio.setName(state.name); studio.faceFront(); }
      });
      updateCount();
    }
    const styleGroup = $('.chips[role="radiogroup"]:has([data-style])') || (styleBtns[0] && styleBtns[0].parentElement);
    const finishGroup = (finishBtns[0] && finishBtns[0].parentElement);
    const pickStyle = (b) => {
      state.style = b.dataset.style; setChecked(styleBtns, b);
      if (studio) studio.setStyle(state.style);
    };
    const pickFinish = (b) => {
      state.finish = FINISH_ALIAS[b.dataset.finish] || 'glossy'; setChecked(finishBtns, b);
      if (studio) studio.setFinish(state.finish);
    };
    if (styleGroup) radioGroup(styleGroup, '[data-style]', pickStyle);
    if (finishGroup) radioGroup(finishGroup, '[data-finish]', pickFinish);

    const reset = $('#resetStudio');
    if (reset) reset.addEventListener('click', () => {
      state.name = ''; state.style = 'classic'; state.finish = 'glossy';
      if (nameInput) { nameInput.value = ''; updateCount(); }
      setChecked(styleBtns, styleBtns.find((b) => b.dataset.style === 'classic'));
      setChecked(finishBtns, finishBtns.find((b) => FINISH_ALIAS[b.dataset.finish] === 'glossy'));
      if (studio) { studio.setName(''); studio.setStyle('classic'); studio.setFinish('glossy'); studio.resetView(); }
      toast(tr('reset'));
    });

    const snap = $('#snapBtn');
    if (snap) snap.addEventListener('click', () => {
      if (!studio) { toast(tr('no3d')); return; }
      try {
        const url = studio.exportPNG();
        const a = document.createElement('a');
        a.href = url; a.download = 'coca-cola-' + (slug(state.name) || 'canette') + '-' + state.range + '.png';
        document.body.appendChild(a); a.click(); a.remove();
        toast(tr('snap'));
      } catch (e) { console.error('[Coca] snapshot', e); toast(tr('snapFail')); }
    });
  };

  /* ───────────────────────── 14. Interface générale ─────────────────── */
  const bindScroll = () => {
    const nav = $('.nav'), up = $('#toTop');
    let ticking = false;
    const apply = () => {
      ticking = false;
      const y = window.scrollY || root.scrollTop;
      if (nav) nav.classList.toggle('is-scrolled', y > 24);
      if (up) up.classList.toggle('is-visible', y > 600);
    };
    window.addEventListener('scroll', () => { if (!ticking) { ticking = true; requestAnimationFrame(apply); } }, { passive: true });
    apply();
    if (up) up.addEventListener('click', (e) => {
      e.preventDefault();
      window.scrollTo({ top: 0, behavior: mqReduce.matches ? 'auto' : 'smooth' });
    });
  };

  const bindTilt = () => {
    const MAX = 9;
    $$('[data-tilt]').forEach((card) => {
      let raf = 0, nx = null, resetTimer = 0;
      const apply = () => {
 raf = 0; if (!nx) return;
 card.style.setProperty('--rx', nx.rx + 'deg'); card.style.setProperty('--ry', nx.ry + 'deg');
        card.style.setProperty('--mx', nx.mx + '%'); card.style.setProperty('--my', nx.my + '%');
      };
      const move = (e) => {
        if (mqReduce.matches) return;
 const r = card.getBoundingClientRect();
 const px = clamp((e.clientX - r.left) / r.width, 0, 1), py = clamp((e.clientY - r.top) / r.height, 0, 1);
        nx = { rx: ((0.5 - py) * 2 * MAX).toFixed(2), ry: ((px - 0.5) * 2 * MAX).toFixed(2), mx: (px * 100).toFixed(1), my: (py * 100).toFixed(1) };
        if (!raf) raf = requestAnimationFrame(apply);
      };
      const reset = () => { nx = { rx: 0, ry: 0, mx: 50, my: 0 }; if (!raf) raf = requestAnimationFrame(apply); };
      card.addEventListener('pointermove', move);
      card.addEventListener('pointerleave', reset);
      card.addEventListener('pointercancel', reset);
      card.addEventListener('pointerdown', (e) => { if (e.pointerType === 'touch') { clearTimeout(resetTimer); move(e); } });
      card.addEventListener('pointerup', (e) => { if (e.pointerType === 'touch') { clearTimeout(resetTimer); resetTimer = setTimeout(reset, 500); } });
    });
  };

  const bindRanges = () => {
    const group = $('.range');
    if (group) radioGroup(group, '.range__btn', (b) => setRange(b.dataset.range));
    $$('[data-pick]').forEach((b) => b.addEventListener('click', () => setRange(b.dataset.pick, { notify: true })));
    // Roving tabindex initial
    const btns = $$('.range__btn');
    setChecked(btns, btns.find((b) => b.dataset.range === state.range) || btns[0]);
  };

  /* ───────────────────────── 15. Audio : liaison douce ──────────────── */
  const audioOK = () => !!(window.CocaAudio && window.CocaAudio.__isCocaAudio && window.CocaAudio.isSupported());
  let soundHintShown = false;

  const playPschhh = () => {
    if (!audioOK()) return;
    const A = window.CocaAudio;
    const ok = A.play('pschhh', CFG.OPEN_FORCES_SOUND);
    if (!ok && A.isMuted() && !soundHintShown) { soundHintShown = true; toast(tr('soundHint'), 4200); }
  };

  const openCan = () => {
    if (hero) { if (!hero.openCan()) return; } else { toast(tr('no3d')); }
    playPschhh();
  };

  const bindAudioGlue = () => {
    if (!audioOK()) {
      // audio.js absent ou Web Audio indisponible : on le dit au clic, sans rien casser
      $$('.sound-btn, #audioToggle').forEach((b) => {
        b.setAttribute('aria-disabled', 'true');
        b.addEventListener('click', () => toast(tr('noAudio')));
      });
      return;
    }
    const A = window.CocaAudio;
    // Un pschhh déclenché depuis la sound bar fait aussi sursauter la canette du hero
    A.on('play', (e) => { if (e && e.type === 'pschhh' && hero && !hero.seq) hero.openCan(); });
    A.on('fizz', (on) => { if (hero) hero.fizzing = !!on; ticker.wake(); });
    A.on('mute', (m) => toast(m ? tr('soundOff') : tr('soundOn'), 1600));
  };

  const bindHero = () => {
    const btn = $('#openCanBtn');
    if (btn) btn.addEventListener('click', () => {
      if (hero && !hero.visible) {
        hero.container.scrollIntoView({ behavior: mqReduce.matches ? 'auto' : 'smooth', block: 'center' });
        setTimeout(openCan, mqReduce.matches ? 0 : 650);
      } else openCan();
    });
  };

  const bindFooter = () => {
    const canvas = $('#footerBubbles');
    let fb = null;
    if (canvas && canvas.getContext) fb = safe('footer', () => new FooterBubbles(canvas));
    const btn = $('#bubbleBtn');
    if (btn) btn.addEventListener('click', () => {
      if (fb) fb.burst(70);
      if (audioOK()) window.CocaAudio.startFizz(true);
      toast(tr('fizz'), 1800);
    });
    return fb;
  };

  /* ───────────────────────── 16. Démarrage des scènes 3D ────────────── */
  const waitFonts = (ms = 2500) => {
    if (!document.fonts || !document.fonts.load) return Promise.resolve();
    const loads = ['120px "Yellowtail"', '800 60px "Outfit"', '700 40px "Plus Jakarta Sans"']
      .map((f) => document.fonts.load(f, 'Coca-Cola Aa').catch(() => null));
    return Promise.race([Promise.all(loads), new Promise((r) => setTimeout(r, ms))]);
  };

  const showNo3D = (container) => {
    if (!container) return;
    const loader = $('.stage__loader', container); if (loader) loader.classList.add('is-hidden');
    if ($('.stage__fallback', container)) return;
    const p = document.createElement('p');
    p.className = 'stage__fallback'; p.dataset.i18n = 'stage.noWebgl';
    container.appendChild(p); I18N.applyTo(container);
  };

  const initScenes = async () => {
    const heroEl = $('#canStage'), studioEl = $('#studioStage');
    if (typeof THREE === 'undefined') {
      console.warn('[Coca] Three.js non chargé (réseau ?)');
      showNo3D(heroEl); showNo3D(studioEl); toast(tr('no3d'));
      return;
    }
    await waitFonts();

    if (heroEl) {
      hero = CanScene.create(heroEl, {
        kind: 'hero', names: false, fitW: 3.9, fitH: 5.0, camY: 0.25, lookY: -0.05,
        autoSpeed: CFG.HERO.autoSpeed, autoDelay: CFG.HERO.autoDelay, bubbles: CFG.HERO.bubbles,
        audioReactive: true, onTap: () => openCan(),
 onFirstFrame: () => {
          heroEl.classList.add('is-ready');
   const l = $('#stageLoader'); if (l) l.classList.add('is-hidden');
 }
      });
      if (!hero) { showNo3D(heroEl); toast(tr('no3d')); }
    }

    if (studioEl) {
      // Création différée : le studio est loin sous le pli, inutile de payer sa GPU au chargement
      const make = () => {
 studio = CanScene.create(studioEl, {
   kind: 'studio', names: true, fitW: 3.5, fitH: 4.7, camY: 0.2, lookY: -0.05,
   autoSpeed: CFG.STUDIO.autoSpeed, autoDelay: CFG.STUDIO.autoDelay, bubbles: CFG.STUDIO.bubbles, audioReactive: false
        });
 if (!studio) showNo3D(studioEl);
      };
      if ('IntersectionObserver' in window) {
        const io = new IntersectionObserver((entries) => {
   if (entries.some((e) => e.isIntersecting)) { io.disconnect(); safe('studio', make); }
        }, { rootMargin: '700px' });
 io.observe(studioEl);
      } else safe('studio', make);
    }

    // Les polices peuvent arriver après le délai d'attente : on repeint pour avoir le vrai logo
    if (document.fonts && document.fonts.ready) {
      document.fonts.ready.then(() => { if (hero) hero.markDirty(); if (studio) studio.markDirty(); });
    }
  };

  /* ───────────────────────── 17. Redimensionnement global ───────────── */
  const bindResize = (getFooter) => {
    let raf = 0;
    const run = () => {
      raf = 0;
      if (hero) hero.resize(); if (studio) studio.resize();
      const f = getFooter(); if (f) f.resize();
    };
    const queue = () => { if (!raf) raf = requestAnimationFrame(run); };
    window.addEventListener('resize', queue);
    window.addEventListener('orientationchange', queue);
    if ('ResizeObserver' in window) {
      const ro = new ResizeObserver(queue);
      [$('#canStage'), $('#studioStage'), $('#footerBubbles')].forEach((el) => { if (el) ro.observe(el); });
    }
  };

  /* ───────────────────────── 18. Boot ───────────────────────────────── */
  const boot = () => {
    safe('i18n', () => I18N.init());
    const saved = store.get(CFG.KEYS.range);
    state.range = RANGES[saved] ? saved : (RANGES[root.dataset.theme] ? root.dataset.theme : 'classic');
    safe('ranges', () => { bindRanges(); setRange(state.range, { persist: false }); });
    safe('scroll', bindScroll);
    safe('tilt', bindTilt);
    safe('studio-ui', bindStudio);
    safe('audio', bindAudioGlue);
    safe('hero-ui', bindHero);
    const footer = safe('footer-ui', bindFooter) || null;
    safe('resize', () => bindResize(() => footer));
    safe('3d', () => initScenes().catch((e) => { console.error('[Coca] 3D', e); showNo3D($('#canStage')); }));
  };

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
  else boot();

  // Petit espace de noms public (débogage, intégration) : le seul global de ce fichier
  window.CocaExperience = Object.freeze({
    version: '1.0.0',
    setRange: (id) => setRange(id),
    setLang: (l) => I18N.setLang(l),
    openCan,
    get state() { return Object.assign({}, state); }
  });
})();
