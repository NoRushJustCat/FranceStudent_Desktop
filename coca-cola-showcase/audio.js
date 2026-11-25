/* ==========================================================================
   audio.js — Sons procéduraux de l'expérience Coca-Cola (Web Audio API)
   --------------------------------------------------------------------------
   - Autonome : aucune dépendance, aucun fichier audio. Tout est synthétisé.
   - API globale : window.CocaAudio
       init()      crée / réveille l'AudioContext (idempotent)
       play(type, opts)   'pschhh' | 'ice' | 'sip' | 'fizz' (démarre le fizz)
startFizz() stopFizz() toggleFizz() isFizzing()
setFizzDensity(0..1) getFizzDensity()
       setVolume(0..1) getVolume()
       setMuted(bool) toggleMute() isMuted()
       getAnalyser() getFrequencyData() getLevel()
       connectVisualizer(canvas) disconnectVisualizer()
       on(evt, fn) off(evt, fn)  évènements : 'play' 'fizz' 'mute' 'volume' 'state'
isSupported() getState()
   - Règle « force » : play(type, true) / toggleFizz(true) lèvent le mute (geste
     explicite de l'utilisateur, ex. clic sur un pad). Sans « force », un son
     demandé alors que le mute est actif est simplement ignoré (retour false).
   - Le son est COUPÉ par défaut (cohérent avec le bouton « Activer le son »).
   - Aucune méthode ne lève d'exception : en cas de problème elle renvoie false.
   - Liaison automatique du DOM (désactivable) : .sound-btn[data-sound], #volume,
     #audioToggle, #soundStatus, #fizzViz. Pour la couper, mettre
     <html data-audio-bind="off"> ou window.COCA_AUDIO_AUTOBIND = false avant ce script.
   ========================================================================== */
(function (global) {
  'use strict';

  if (global.CocaAudio && global.CocaAudio.__isCocaAudio) return;

  var AC = global.AudioContext || global.webkitAudioContext;
  var SUPPORTED = typeof AC === 'function';

  /* Niveaux de mixage (0..1, avant le master). Non calibrés à l'oreille :
     c'est ici qu'il faut ajuster si un son est trop fort ou trop discret. */
  var LEVELS = {
    click: 0.55,     // déclic de languette
    ring: 0.10,      // résonance métallique de la languette
    hiss: 0.95,      // souffle principal du « pschhh »
    whoosh: 0.45,    // grave du souffle
    thump: 0.50,     // « pop » de pression
    ice: 0.30,       // tintement des glaçons
    sip: 0.70,       // gorgée
    glug: 0.22,      // déglutition
    popTick: 0.38,   // micro-pops de bulles (claquement)
    popBloop: 0.12,  // micro-pops de bulles (bloop)
    fizzHiss: 0.07,  // pétillement continu, couche 1
    fizzHiss2: 0.05  // pétillement continu, couche 2
  };

  var DURATIONS = { pschhh: 1.7, ice: 1.1, sip: 1.5 };
  var ALIASES = {
    pschhh: 'pschhh', psshh: 'pschhh', open: 'pschhh', can: 'pschhh',
    ice: 'ice', cubes: 'ice', glacons: 'ice', 'glaçons': 'ice',
    fizz: 'fizz', bubbles: 'fizz',
    sip: 'sip', gulp: 'sip', drink: 'sip'
  };
  var MAX_VOICES = 40;

  /* ------------------------------ État ---------------------------------- */
  var ctx = null, master = null, comp = null, analyser = null, bus = null;
  var noiseBuf = null, brownBuf = null;
  var volume = 0.6;
  var muted = true;
  var gestureSeen = false;
  var autoSuspended = false;
  var voices = 0;
  var fizz = null;
  var fizzTarget = 0.5;
  var listeners = {};
  var freqData = null;
  var emptyData = new Uint8Array(256);
  var viz = null;
  var uiBound = false;

  /* ---------------------------- Utilitaires ----------------------------- */
  function noop() {}
  function clamp(v, a, b) { return v < a ? a : (v > b ? b : v); }
  function rand(a, b) { return a + Math.random() * (b - a); }
  function isForce(o) { return o === true || !!(o && o.force); }

  function on(name, fn) {
    if (typeof fn !== 'function') return noop;
    (listeners[name] = listeners[name] || []).push(fn);
    return function () { off(name, fn); };
  }
  function off(name, fn) {
    var l = listeners[name];
    if (!l) return;
    var i = l.indexOf(fn);
    if (i >= 0) l.splice(i, 1);
  }
  function emit(name, data) {
    var l = listeners[name];
    if (!l) return;
    l.slice().forEach(function (fn) { try { fn(data); } catch (e) { /* un écouteur fautif ne doit rien casser */ } });
  }

  /* --------------------------- Graphe audio ----------------------------- */
  function targetGain() { return muted ? 0 : Math.pow(volume, 1.6); }

  function applyGain() {
    if (!ctx || !master) return;
    try { master.gain.setTargetAtTime(targetGain(), ctx.currentTime, 0.03); } catch (e) { /* ignore */ }
  }

  function makeNoise() {
    var sr = ctx.sampleRate, len = Math.floor(sr * 3), i;
    noiseBuf = ctx.createBuffer(1, len, sr);
    var d = noiseBuf.getChannelData(0);
    for (i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;

    brownBuf = ctx.createBuffer(1, len, sr);
    var b = brownBuf.getChannelData(0), last = 0, w;
    for (i = 0; i < len; i++) {
      w = Math.random() * 2 - 1;
      last = (last + 0.02 * w) / 1.02;
      b[i] = last * 3.5;
    }
  }

  function buildGraph() {
    master = ctx.createGain();
    master.gain.value = targetGain();

    comp = ctx.createDynamicsCompressor();
    try {
      comp.threshold.value = -16;
      comp.knee.value = 18;
      comp.ratio.value = 5;
      comp.attack.value = 0.004;
      comp.release.value = 0.22;
    } catch (e) { /* valeurs par défaut */ }

    analyser = ctx.createAnalyser();
    analyser.fftSize = 512;
    analyser.smoothingTimeConstant = 0.8;
    analyser.minDecibels = -95;
    analyser.maxDecibels = -25;

    bus = ctx.createGain();
    bus.connect(master);
    master.connect(comp);
    comp.connect(analyser);
    analyser.connect(ctx.destination);

    makeNoise();

    try {
      ctx.addEventListener('statechange', function () {
 emit('state', ctx.state);
 if (ctx.state === 'running') removeUnlock();
      });
    } catch (e) { /* anciens Safari */ }
  }

  function resumeCtx() {
    if (!ctx || ctx.state === 'running' || ctx.state === 'closed') return;
    try {
      var p = ctx.resume();
      if (p && typeof p.catch === 'function') p.catch(noop);
    } catch (e) { /* ignore */ }
  }

  function init() {
    try {
      if (!SUPPORTED) return false;
      if (ctx) { resumeCtx(); return true; }
      try {
        ctx = new AC({ latencyHint: 'interactive' });
      } catch (e1) {
        ctx = new AC();
      }
      buildGraph();
      resumeCtx();
      return true;
    } catch (e) {
      ctx = null;
      return false;
    }
  }

  /* Prêt à jouer ? (contexte créé, et un geste utilisateur a déjà eu lieu) */
  function ready() {
    if (!SUPPORTED) return false;
    if (!ctx) {
      if (!gestureSeen) return false;
      if (!init()) return false;
    }
    if (ctx.state !== 'running') {
      if (!gestureSeen) return false;
      resumeCtx();
    }
    return true;
  }

  /* ------------------- Déverrouillage au premier geste ------------------ */
  var UNLOCK_EVENTS = ['pointerdown', 'mousedown', 'touchstart', 'touchend', 'click', 'keydown'];

  function onGesture(e) {
    if (e && e.isTrusted === false) return;
    gestureSeen = true;
    if (!init()) { removeUnlock(); return; }
    try { // astuce iOS : jouer un buffer vide pour ouvrir la sortie audio
      var s = ctx.createBufferSource();
      s.buffer = ctx.createBuffer(1, 1, 22050);
      s.connect(ctx.destination);
      s.start(0);
    } catch (err) { /* ignore */ }
    resumeCtx();
    if (ctx.state === 'running') removeUnlock();
  }

  function addUnlock() {
    UNLOCK_EVENTS.forEach(function (n) {
      try { global.addEventListener(n, onGesture, { capture: true, passive: true }); } catch (e) { /* ignore */ }
    });
  }
  function removeUnlock() {
    UNLOCK_EVENTS.forEach(function (n) {
      try { global.removeEventListener(n, onGesture, { capture: true }); } catch (e) { /* ignore */ }
    });
  }

  /* --------------------------- Briques de synthèse ---------------------- */
  function osc(type, f) {
    var o = ctx.createOscillator();
    o.type = type;
    o.frequency.value = f;
    return o;
  }
  function filt(type, f, q) {
    var b = ctx.createBiquadFilter();
    b.type = type;
    b.frequency.value = f;
    if (q != null) b.Q.value = q;
    return b;
  }
  function gn(v) {
    var g = ctx.createGain();
    g.gain.value = v;
    return g;
  }
  function noiseSrc(buf, loop) {
    var s = ctx.createBufferSource();
    s.buffer = buf;
    s.loop = !!loop;
    return s;
  }

  /* Enveloppe percussive : attaque linéaire courte puis décroissance exponentielle. */
  function pluck(param, t, peak, atk, dec) {
    param.setValueAtTime(0.0001, t);
    param.linearRampToValueAtTime(peak, t + atk);
    param.exponentialRampToValueAtTime(0.0001, t + atk + dec);
  }

  /* Enveloppe ADSR (+ palier « hold »). Retourne l'instant de fin. */
  function adsr(param, t, peak, a, d, s, hold, r) {
    var sus = Math.max(peak * s, 0.0001);
    param.setValueAtTime(0.0001, t);
    param.linearRampToValueAtTime(peak, t + a);
    param.exponentialRampToValueAtTime(sus, t + a + d);
    param.setValueAtTime(sus, t + a + d + hold);
    param.exponentialRampToValueAtTime(0.0001, t + a + d + hold + r);
    return t + a + d + hold + r;
  }

  /* Voix : un gain de sortie + un « gardien » qui libère tout à la fin. */
  function createVoice(maxDur) {
    if (!ctx || voices >= MAX_VOICES) return null;
    var out = ctx.createGain();
    out.connect(bus);
    var t0 = ctx.currentTime + 0.02;
    var done = false;
    var keeper = ctx.createOscillator();
    var kg = ctx.createGain();
    kg.gain.value = 0;
    keeper.connect(kg);
    kg.connect(out);

    function dispose() {
      if (done) return;
      done = true;
      voices--;
      try { keeper.disconnect(); kg.disconnect(); out.disconnect(); } catch (e) { /* ignore */ }
    }
    keeper.onended = dispose;
    keeper.start(t0);
    keeper.stop(t0 + maxDur);
    setTimeout(dispose, (maxDur + 2) * 1000); // filet de sécurité
    voices++;
    return { out: out, t0: t0 };
  }

  /* Rafale de bruit très brève (claquement, tick). */
  function noiseBurst(dest, t, dur, ftype, freq, q, peak) {
    var s = noiseSrc(noiseBuf, false);
    var f = filt(ftype, freq, q);
    var g = gn(0.0001);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(peak, t + 0.0008);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    s.connect(f); f.connect(g); g.connect(dest);
    s.start(t, rand(0, Math.max(0.01, noiseBuf.duration - dur - 0.05)), dur + 0.01);
  }

  /* Une bulle qui éclate : claquement filtré (68 %) ou petit « bloop » (32 %). */
  function pop(dest, t, vel) {
    if (Math.random() < 0.68) {
      noiseBurst(dest, t, rand(0.004, 0.010), 'bandpass', rand(3200, 9500), rand(2, 6), LEVELS.popTick * vel * rand(0.6, 1));
    } else {
      var f0 = rand(1600, 4200);
      var o = osc('sine', f0);
      var g = gn(0.0001);
      var d = rand(0.012, 0.03);
      o.frequency.setValueAtTime(f0, t);
      o.frequency.exponentialRampToValueAtTime(f0 * rand(1.3, 2), t + d);
      pluck(g.gain, t, LEVELS.popBloop * vel, 0.002, d);
      o.connect(g); g.connect(dest);
      o.start(t);
      o.stop(t + d + 0.03);
    }
  }

  /* Programme une pluie de pops (processus de Poisson). decay = cadence qui retombe. */
  function schedulePops(dest, t, dur, rate, decay) {
    var at = t, end = t + dur, n = 0;
    while (at < end && n < 90) {
      pop(dest, at, rand(0.4, 1));
      var r = decay ? rate * (1 - 0.8 * (at - t) / dur) : rate;
      at += -Math.log(1 - Math.random()) / Math.max(r, 1);
      n++;
    }
  }

  /* ------------------------------ pschhh -------------------------------- */
  function tabClick(v, t, vel) {
    // Déclic : triangle bref dont la hauteur chute, filtré passe-bande
    var o = osc('triangle', 2600);
    o.frequency.setValueAtTime(2600, t);
    o.frequency.exponentialRampToValueAtTime(900, t + 0.03);
    var bp = filt('bandpass', 2400, 2.5);
    var g = gn(0.0001);
    pluck(g.gain, t, LEVELS.click * vel, 0.001, 0.035);
    o.connect(bp); bp.connect(g); g.connect(v.out);
    o.start(t);
    o.stop(t + 0.06);

    // Résonance d'aluminium : deux sinus inharmoniques très courts
    [3170, 4910].forEach(function (f, i) {
      var r = osc('sine', f * rand(0.99, 1.01));
      var rg = gn(0.0001);
      pluck(rg.gain, t, LEVELS.ring * vel / (i + 1), 0.001, 0.09 - i * 0.03);
      r.connect(rg); rg.connect(v.out);
      r.start(t);
      r.stop(t + 0.12);
    });

    noiseBurst(v.out, t, 0.006, 'highpass', 4000, 0.7, 0.3 * vel);
  }

  function playPschhh() {
    var v = createVoice(2.4);
    if (!v) return false;
    var t = v.t0;

    tabClick(v, t, 1);   // « tic »
    tabClick(v, t + 0.055, 0.7); // « tac » (la languette revient)

    var h = t + 0.075;    // début de la dépressurisation

    // Souffle principal : bruit blanc -> passe-haut -> passe-bande qui descend, ADSR percutant
    var n1 = noiseSrc(noiseBuf, false);
    var hp = filt('highpass', 1400, 0.7);
    var bp = filt('bandpass', 7500, 0.8);
    bp.frequency.setValueAtTime(7500, h);
    bp.frequency.exponentialRampToValueAtTime(3000, h + 1.4);
    var g1 = gn(0.0001);
    var end1 = adsr(g1.gain, h, LEVELS.hiss, 0.006, 0.12, 0.42, 0.4, 0.95);
    n1.connect(hp); hp.connect(bp); bp.connect(g1); g1.connect(v.out);
    n1.start(h, rand(0, 1.2), end1 - h + 0.05);

    // Corps grave du souffle
    var n2 = noiseSrc(noiseBuf, false);
    var bp2 = filt('bandpass', 1500, 0.6);
    var g2 = gn(0.0001);
    var end2 = adsr(g2.gain, h, LEVELS.whoosh, 0.01, 0.22, 0.12, 0.08, 0.45);
    n2.connect(bp2); bp2.connect(g2); g2.connect(v.out);
    n2.start(h, rand(0, 1.2), end2 - h + 0.05);

    // « Pop » de pression
    var th = osc('sine', 170);
    th.frequency.setValueAtTime(170, h);
    th.frequency.exponentialRampToValueAtTime(50, h + 0.16);
    var tg = gn(0.0001);
    pluck(tg.gain, h, LEVELS.thump, 0.003, 0.18);
    th.connect(tg); tg.connect(v.out);
    th.start(h);
    th.stop(h + 0.25);

    // Les bulles qui s'excitent juste après l'ouverture
    schedulePops(v.out, h + 0.25, 1.3, 55, true);
    return true;
  }

  /* ------------------------------- ice ---------------------------------- */
  var ICE_RATIOS = [1, 2.756, 5.404];   // rapports inharmoniques (barre vibrante)
  var ICE_DECAYS = [0.34, 0.17, 0.08];
  var ICE_AMPS = [1, 0.6, 0.3];

  function clink(v, t, vel) {
    var base = rand(2100, 3400);
    var dest = v.out;
    if (ctx.createStereoPanner) {
      try {
        var pan = ctx.createStereoPanner();
        pan.pan.value = rand(-0.6, 0.6);
        pan.connect(v.out);
 dest = pan;
      } catch (e) { dest = v.out; }
    }
    var amFreq = rand(14, 36);
    for (var i = 0; i < 3; i++) {
      var f = base * ICE_RATIOS[i] * rand(0.985, 1.015);
      var dec = ICE_DECAYS[i] * rand(0.8, 1.2);
      var o = osc('sine', f);
      var env = gn(0.0001);
      pluck(env.gain, t, LEVELS.ice * vel * ICE_AMPS[i], 0.0015, dec);
      // Modulation d'amplitude (battements du verre)
      var am = gn(0.7);
      var lfo = osc('sine', amFreq * (1 + i * 0.37));
      var lg = gn(0.3);
      lfo.connect(lg); lg.connect(am.gain);
      o.connect(env); env.connect(am); am.connect(dest);
      var stop = t + 0.0015 + dec + 0.05;
      o.start(t); o.stop(stop);
      lfo.start(t); lfo.stop(stop);
    }
    noiseBurst(dest, t, 0.007, 'highpass', 6000, 0.7, 0.16 * vel);
  }

  function playIce() {
    var v = createVoice(2.0);
    if (!v) return false;
    var n = 3 + Math.floor(Math.random() * 3); // 3 à 5 tintements
    var at = v.t0;
    for (var i = 0; i < n; i++) {
      clink(v, at, i === 0 ? 1 : rand(0.5, 0.9));
      at += rand(0.055, 0.16);
    }
    return true;
  }

  /* ------------------------------- sip ---------------------------------- */
  function glug(v, t, f, amp) {
    var o = osc('sine', f);
    o.frequency.setValueAtTime(f * 1.25, t);
    o.frequency.exponentialRampToValueAtTime(f * 0.7, t + 0.11);
    var lp = filt('lowpass', 900, 0.7);
    var g = gn(0.0001);
    pluck(g.gain, t, amp, 0.012, 0.12);
    o.connect(lp); lp.connect(g); g.connect(v.out);
    o.start(t);
    o.stop(t + 0.2);
  }

  function playSip() {
    var v = createVoice(2.2);
    if (!v) return false;
    var t = v.t0;

    // Aspiration : bruit brun -> passe-bas résonant qui monte puis redescend
    var n = noiseSrc(brownBuf, false);
    var lp = filt('lowpass', 300, 5);
    lp.frequency.setValueAtTime(300, t);
    lp.frequency.exponentialRampToValueAtTime(1500, t + 0.45);
    lp.frequency.exponentialRampToValueAtTime(450, t + 1.0);
    var g = gn(0.0001);
    var end = adsr(g.gain, t, LEVELS.sip, 0.18, 0.1, 0.8, 0.55, 0.3);
    n.connect(lp); lp.connect(g); g.connect(v.out);
    n.start(t, rand(0, 1.5), end - t + 0.05);

    // Léger voile liquide
    var n2 = noiseSrc(noiseBuf, false);
    var bp = filt('bandpass', 2600, 0.6);
    var g2 = gn(0.0001);
    var end2 = adsr(g2.gain, t + 0.05, 0.10, 0.2, 0.1, 0.6, 0.45, 0.3);
    n2.connect(bp); bp.connect(g2); g2.connect(v.out);
    n2.start(t + 0.05, rand(0, 1.5), end2 - t);

    // Déglutition + petites bulles
    glug(v, t + 1.0, 190, LEVELS.glug);
    glug(v, t + 1.2, 150, LEVELS.glug * 0.7);
    schedulePops(v.out, t + 0.05, 0.9, 16, false);
    return true;
  }

  /* ------------------------------- fizz --------------------------------- */
  function fizzTick() {
    if (!fizz || !ctx || ctx.state !== 'running') return;
    var f = fizz;
    var now = ctx.currentTime;
    var dt = Math.max(0, now - f.lastTick);
    f.lastTick = now;
    f.ticks++;

    f.bloom *= Math.exp(-dt / 1.6);
    f.density += (fizzTarget - f.density) * Math.min(1, dt * 3);

    var el = now - f.t0;
    var wob = 0.85 + 0.15 * Math.sin(el * 0.9) + 0.08 * Math.sin(el * 2.7 + 1.3);
    var d = clamp(f.density * wob + f.bloom, 0.02, 1);
    var rate = 5 + 80 * d * d;   // pops / seconde (≈ 25 à densité 0.5)

    if (f.ticks % 4 === 0) { // on ne retouche les filtres que de temps en temps
      try {
        f.hp.frequency.setTargetAtTime(4200 + 1800 * d + rand(-300, 300), now, 0.12);
 f.bp.frequency.setTargetAtTime(7000 + 2500 * Math.sin(el * 0.5), now, 0.2);
 f.g1.gain.setTargetAtTime(LEVELS.fizzHiss * (0.6 + 0.9 * d), now, 0.15);
        f.g2.gain.setTargetAtTime(LEVELS.fizzHiss2 * (0.6 + 0.9 * d), now, 0.15);
      } catch (e) { /* ignore */ }
    }

    if (f.nextPop < now) f.nextPop = now + 0.005; // pas de rafale après une pause
    var horizon = now + 0.14, guard = 0;
    while (f.nextPop < horizon && guard++ < 14) {
      pop(f.out, f.nextPop, rand(0.5, 1));
      f.nextPop += -Math.log(1 - Math.random()) / rate;
    }
  }

  function startFizz(opts) {
    try {
      if (fizz) return true;
      if (isForce(opts) && muted) setMuted(false);
      if (muted) return false;
      if (!ready()) return false;

      var t = ctx.currentTime + 0.02;
      var out = gn(0.0001);
      out.connect(bus);
      out.gain.setValueAtTime(0.0001, t);
      out.gain.linearRampToValueAtTime(1, t + 0.35);

      // Couche 1 : souffle aigu (passe-haut)
      var s1 = noiseSrc(noiseBuf, true);
      var hp = filt('highpass', 4800, 0.6);
      var g1 = gn(LEVELS.fizzHiss);
      s1.connect(hp); hp.connect(g1); g1.connect(out);

      // Couche 2 : voile « sifflant » (passe-bande)
      var s2 = noiseSrc(noiseBuf, true);
      var bp = filt('bandpass', 8000, 0.9);
      var g2 = gn(LEVELS.fizzHiss2);
      s2.connect(bp); bp.connect(g2); g2.connect(out);

      // Deux LFO lents : le pétillement « respire »
      var l1 = osc('sine', 0.37);
      var l1g = gn(LEVELS.fizzHiss * 0.35);
      l1.connect(l1g); l1g.connect(g1.gain);
      var l2 = osc('sine', 0.23);
      var l2g = gn(LEVELS.fizzHiss2 * 0.45);
      l2.connect(l2g); l2g.connect(g2.gain);

      s1.start(t, rand(0, 2));
      s2.start(t, rand(0, 2));
      l1.start(t);
      l2.start(t);

      fizz = {
        out: out, hp: hp, bp: bp, g1: g1, g2: g2,
 sources: [s1, s2, l1, l2],
        t0: t, lastTick: t, nextPop: t, ticks: 0,
        density: fizzTarget,
        bloom: 0.45,    // « versé à l'instant » : ça pétille fort puis ça se calme
 timer: setInterval(fizzTick, 35)
      };
      emit('fizz', true);
      return true;
    } catch (e) {
      fizz = null;
      return false;
    }
  }

  function stopFizz() {
    if (!fizz) return false;
    var f = fizz;
    fizz = null;
    clearInterval(f.timer);
    try {
      var now = ctx.currentTime;
      var cur = f.out.gain.value;
      f.out.gain.cancelScheduledValues(now);
      f.out.gain.setValueAtTime(cur, now);
      f.out.gain.setTargetAtTime(0, now, 0.1);
      f.sources.forEach(function (s) { try { s.stop(now + 0.7); } catch (e) { /* ignore */ } });
    } catch (e) { /* ignore */ }
    setTimeout(function () { try { f.out.disconnect(); } catch (e) { /* ignore */ } }, 900);
    emit('fizz', false);
    return true;
  }

  function toggleFizz(force) {
    if (fizz) { stopFizz(); return false; }
    return startFizz(force);
  }

  function isFizzing() { return !!fizz; }

  function setFizzDensity(d) {
    d = Number(d);
    if (!isFinite(d)) return fizzTarget;
    fizzTarget = clamp(d, 0, 1);
    return fizzTarget;
  }
  function getFizzDensity() { return fizzTarget; }

  function bumpFizz(amount) {
    if (fizz) fizz.bloom = Math.min(0.6, fizz.bloom + amount);
  }

  /* ----------------------------- API publique --------------------------- */
  var PLAYERS = { pschhh: playPschhh, ice: playIce, sip: playSip };

  function play(type, opts) {
    try {
      type = ALIASES[String(type || '').toLowerCase()];
      if (!type) return false;
      if (type === 'fizz') return startFizz(opts);
      if (isForce(opts) && muted) setMuted(false);
      if (muted) return false;
      if (!ready()) return false;
      if (!PLAYERS[type]()) return false;
      if (type === 'ice') bumpFizz(0.25);
      else if (type === 'sip') bumpFizz(0.12);
      else bumpFizz(0.3);
      emit('play', { type: type, duration: DURATIONS[type] });
      return true;
    } catch (e) {
      return false;
    }
  }

  function setVolume(v) {
    v = Number(v);
    if (!isFinite(v)) return volume;
    volume = clamp(v, 0, 1);
    applyGain();
    emit('volume', volume);
    return volume;
  }
  function getVolume() { return volume; }

  function setMuted(m) {
    m = !!m;
    if (m === muted) return muted;
    muted = m;
    applyGain();
    emit('mute', muted);
    return muted;
  }
  function toggleMute() { return setMuted(!muted); }
  function isMuted() { return muted; }

  function getAnalyser() { return analyser; }

  function getFrequencyData() {
    if (!analyser) return emptyData;
    try {
      if (!freqData || freqData.length !== analyser.frequencyBinCount) {
        freqData = new Uint8Array(analyser.frequencyBinCount);
      }
      analyser.getByteFrequencyData(freqData);
      return freqData;
    } catch (e) {
      return emptyData;
    }
  }

  /* Niveau global 0..1 (pratique pour faire pulser une animation). */
  function getLevel() {
    var d = getFrequencyData(), n = Math.floor(d.length * 0.66), s = 0;
    if (!n) return 0;
    for (var i = 0; i < n; i++) s += d[i];
    return s / n / 255;
  }

  /* --------------------------- Visualiseur canvas ----------------------- */
  var VIZ_BARS = 56;

  function readColors() {
    var cs = global.getComputedStyle(document.documentElement);
    function v(n, fb) {
      var x = cs.getPropertyValue(n);
      x = x && x.trim();
      return x || fb;
    }
    return { a: v('--brand', '#f40009'), b: v('--neon', '#ff6b6b') };
  }

  function resizeViz() {
    if (!viz) return;
    var c = viz.canvas;
    var r = c.getBoundingClientRect();
    var dpr = Math.min(global.devicePixelRatio || 1, 2);
    var w = Math.round(r.width * dpr), h = Math.round(r.height * dpr);
    viz.dpr = dpr;
    if (w > 0 && h > 0 && (Math.abs(c.width - w) > 1 || Math.abs(c.height - h) > 1)) {
      c.width = w;
      c.height = h;
      viz.grad = null;
    }
  }

  function startViz() {
    if (viz && !viz.raf && viz.visible && !document.hidden) {
      viz.raf = global.requestAnimationFrame(drawViz);
    }
  }

  function drawViz(now) {
    var s = viz;
    if (!s) return;
    s.raf = 0;
    if (!s.canvas.isConnected) { disconnectVisualizer(); return; }
    if (!s.visible || document.hidden) return; // relancé par IntersectionObserver / visibilitychange
    try {
      var c = s.c2, W = s.canvas.width, H = s.canvas.height;
      if (s.frame++ % 45 === 0) { s.colors = readColors(); s.grad = null; }
      if (!s.grad) {
 try {
          s.grad = c.createLinearGradient(0, 0, 0, H);
          s.grad.addColorStop(0, s.colors.b);
   s.grad.addColorStop(0.5, s.colors.a);
   s.grad.addColorStop(1, s.colors.b);
        } catch (e) {
   s.grad = '#ff2a3d';
        }
      }
      c.clearRect(0, 0, W, H);
      var data = analyser ? getFrequencyData() : null;
      var bars = VIZ_BARS;
      var gap = Math.max(2, (W / bars) * 0.28);
      var bw = Math.max(1, (W - gap * (bars - 1)) / bars);
      var t = now / 1000;
      var minH = 2 * s.dpr;
      var span = data ? Math.min(190, data.length - 2) : 0;

      c.fillStyle = s.grad;
      c.beginPath();
      for (var i = 0; i < bars; i++) {
        var m = 0;
        if (span) {
          var lo = Math.floor(Math.pow(i / bars, 1.7) * span) + 1;
   var hi = Math.max(lo + 1, Math.floor(Math.pow((i + 1) / bars, 1.7) * span) + 2);
   for (var k = lo; k < hi && k < data.length; k++) if (data[k] > m) m = data[k];
   m /= 255;
 }
        var idle = 0.04 + 0.025 * Math.sin(t * 2 + i * 0.45); // légère ondulation au repos
        var h = Math.max(minH, Math.max(idle, m) * H * 0.92);
        var x = i * (bw + gap), y = (H - h) / 2;
 if (c.roundRect) c.roundRect(x, y, bw, h, bw / 2); else c.rect(x, y, bw, h);
      }
      c.fill();
    } catch (e) { /* un dessin raté ne doit pas casser la page */ }
    if (viz === s) s.raf = global.requestAnimationFrame(drawViz);
  }

  function connectVisualizer(canvas) {
    try {
      canvas = canvas || document.getElementById('fizzViz');
      if (!canvas || !canvas.getContext) return false;
      var c2 = canvas.getContext('2d');
      if (!c2) return false;
      disconnectVisualizer();
      viz = {
        canvas: canvas, c2: c2, raf: 0, frame: 0, dpr: 1,
        colors: { a: '#f40009', b: '#ff6b6b' }, grad: null,
        visible: true, io: null, ro: null
      };
      resizeViz();
      if ('IntersectionObserver' in global) {
        viz.io = new IntersectionObserver(function (entries) {
   if (!viz) return;
   viz.visible = entries[entries.length - 1].isIntersecting;
   if (viz.visible) startViz();
        });
        viz.io.observe(canvas);
      }
      if ('ResizeObserver' in global) {
 viz.ro = new ResizeObserver(resizeViz);
 viz.ro.observe(canvas);
      } else {
 global.addEventListener('resize', resizeViz);
      }
      startViz();
      return true;
    } catch (e) {
      viz = null;
      return false;
    }
  }

  function disconnectVisualizer() {
    if (!viz) return;
    var s = viz;
    viz = null;
    try {
      if (s.raf) global.cancelAnimationFrame(s.raf);
      if (s.io) s.io.disconnect();
      if (s.ro) s.ro.disconnect();
      global.removeEventListener('resize', resizeViz);
    } catch (e) { /* ignore */ }
  }

  /* ------------------------------- Cycle de vie ------------------------- */
  document.addEventListener('visibilitychange', function () {
    try {
      if (document.hidden) {
        if (ctx && ctx.state === 'running') {
          autoSuspended = true;
   var p = ctx.suspend();
   if (p && typeof p.catch === 'function') p.catch(noop);
        }
      } else {
 if (autoSuspended) { autoSuspended = false; resumeCtx(); }
        startViz();
      }
    } catch (e) { /* ignore */ }
  });

  /* ------------------------ Liaison automatique du DOM ------------------ */
  var MSG = {
    fr: {
      pschhh: 'Ouverture de la canette', ice: 'Glaçons', sip: 'Gorgée',
      fizz_on: 'Pétillement activé', fizz_off: 'Pétillement coupé',
      sound_on: 'Son activé', sound_off: 'Son coupé',
      unsupported: "Audio non pris en charge par ce navigateur"
    },
    en: {
      pschhh: 'Opening the can', ice: 'Ice cubes', sip: 'Sip',
      fizz_on: 'Fizz on', fizz_off: 'Fizz off',
      sound_on: 'Sound on', sound_off: 'Sound off',
      unsupported: 'Audio is not supported by this browser'
    }
  };

  function bindUI() {
    try {
      if (uiBound) return;
      var root = document.documentElement;
      if (global.COCA_AUDIO_AUTOBIND === false || root.getAttribute('data-audio-bind') === 'off') return;
      uiBound = true;

      var statusEl = document.getElementById('soundStatus');
      var toggle = document.getElementById('audioToggle');
      var slider = document.getElementById('volume');
      var pads = [].slice.call(document.querySelectorAll('.sound-btn[data-sound]'));
      var announceTimer = 0;

      function announce(key) {
 if (!statusEl) return;
        var lang = ((root.lang || 'fr') + '').slice(0, 2);
        var dict = MSG[lang] || MSG.fr;
 var text = dict[key] || '';
        clearTimeout(announceTimer);
        statusEl.textContent = '';
 announceTimer = setTimeout(function () { statusEl.textContent = text; }, 40);
      }

      function padFor(type) {
        for (var i = 0; i < pads.length; i++) {
          if (pads[i].getAttribute('data-sound') === type) return pads[i];
        }
        return null;
      }

      if (!SUPPORTED) {
 announce('unsupported');
 pads.forEach(function (b) { b.setAttribute('aria-disabled', 'true'); });
        return;
      }

      // Pads
      pads.forEach(function (btn) {
 btn.addEventListener('click', function () {
   var type = btn.getAttribute('data-sound');
          if (type === 'fizz') toggleFizz(true); else play(type, true);
        });
      });

      on('play', function (e) {
        var btn = padFor(e.type);
        if (!btn) return;
 btn.classList.remove('is-hit');
 void btn.offsetWidth; // relance l'animation
        btn.classList.add('is-hit', 'is-playing');
 clearTimeout(btn.__cocaTimer);
        btn.__cocaTimer = setTimeout(function () { btn.classList.remove('is-playing', 'is-hit'); }, e.duration * 1000);
 announce(e.type);
      });

      on('fizz', function (state) {
 var btn = padFor('fizz');
 if (btn) {
   btn.setAttribute('aria-pressed', state ? 'true' : 'false');
   btn.classList.toggle('is-playing', !!state);
 }
        announce(state ? 'fizz_on' : 'fizz_off');
      });

      // Bouton son de la navbar : aria-pressed="true" = son activé
      function syncMute(m) { if (toggle) toggle.setAttribute('aria-pressed', m ? 'false' : 'true'); }
      syncMute(muted);
      on('mute', function (m) { syncMute(m); announce(m ? 'sound_off' : 'sound_on'); });
      if (toggle) {
 toggle.addEventListener('click', function () {
   toggleMute();
          if (!muted) init();
 });
      }

      // Volume (0-100 dans le HTML -> 0..1) + variable --fill pour la piste WebKit
      if (slider) {
        var applyVol = function (fromUser) {
   var min = parseFloat(slider.min) || 0, max = parseFloat(slider.max) || 100;
          var v = clamp(((parseFloat(slider.value) || 0) - min) / (max - min), 0, 1);
   setVolume(v);
          slider.style.setProperty('--fill', (v * 100) + '%');
   if (fromUser && v > 0 && muted) setMuted(false); // bouger le volume = vouloir du son
        };
        slider.addEventListener('input', function () { applyVol(true); });
 applyVol(false);
      }

      // Visualiseur
      var vizCanvas = document.getElementById('fizzViz');
      if (vizCanvas) connectVisualizer(vizCanvas);
    } catch (e) { /* la liaison DOM est un bonus, jamais bloquante */ }
  }

  /* --------------------------------- Export ----------------------------- */
  global.CocaAudio = {
    __isCocaAudio: true,
    version: '1.0.0',
    init: init,
    play: play,
    startFizz: startFizz,
    stopFizz: stopFizz,
    toggleFizz: toggleFizz,
    isFizzing: isFizzing,
    setFizzDensity: setFizzDensity,
    getFizzDensity: getFizzDensity,
    setVolume: setVolume,
    getVolume: getVolume,
    setMuted: setMuted,
    toggleMute: toggleMute,
    isMuted: isMuted,
    getAnalyser: getAnalyser,
    getFrequencyData: getFrequencyData,
    getLevel: getLevel,
    connectVisualizer: connectVisualizer,
    disconnectVisualizer: disconnectVisualizer,
    on: on,
    off: off,
    isSupported: function () { return SUPPORTED; },
    getState: function () { return ctx ? ctx.state : 'idle'; }
  };

  if (SUPPORTED) addUnlock();

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', bindUI);
  } else {
    bindUI();
  }
})(typeof window !== 'undefined' ? window : this);
