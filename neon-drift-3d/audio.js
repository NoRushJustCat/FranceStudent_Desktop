/* =========================================================
   NEON DRIFT 3D — audio.js
   Synthèse 100 % Web Audio API : musique synthwave générative
   (kick, snare, hats, basse, arpège, pad, lead) + SFX + moteur.
   Aucun fichier audio externe.
   ========================================================= */

const STORE_KEY = 'neonDrift3dMuted';
const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);

// Progression Am - F - C - G (une mesure = 16 pas)
const CHORDS = [
  [57, 60, 64, 69], // Am
  [53, 57, 60, 65], // F
  [55, 60, 64, 67], // C
  [55, 59, 62, 67], // G
];
const ROOTS = [45, 41, 48, 43];
const ARP = [0, 1, 2, 3, 2, 1, 3, 2, 0, 1, 2, 3, 2, 1, 3, 2];
const LEAD = [
  [81, 0, 0, 81, 0, 0, 79, 0, 76, 0, 0, 0, 74, 0, 76, 0],
  [77, 0, 0, 77, 0, 0, 76, 0, 72, 0, 0, 0, 69, 0, 72, 0],
  [76, 0, 0, 76, 0, 0, 74, 0, 72, 0, 0, 0, 67, 0, 72, 0],
  [74, 0, 0, 74, 0, 0, 72, 0, 71, 0, 0, 0, 67, 0, 71, 0],
];

class AudioEngine {
  constructor() {
    this.ctx = null;
    this.muted = this._loadMuted();
    this.musicRunning = false;
    this.timer = null;
    this.step = 0;
    this.nextTime = 0;
    this.bpm = 108;
    this.intensity = 0;
    this.lastKick = -10;
    this.engine = null;
  }

  _loadMuted() {
    try { return localStorage.getItem(STORE_KEY) === '1'; } catch { return false; }
  }

  get ready() { return !!this.ctx; }

  /** À appeler depuis un geste utilisateur (clic / touche). */
  unlock() {
    if (!this.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) return false;
      const ctx = (this.ctx = new AC({ latencyHint: 'interactive' }));

      this.master = ctx.createGain();
      this.master.gain.value = this.muted ? 0 : 0.85;
      const comp = ctx.createDynamicsCompressor();
      comp.threshold.value = -16; comp.ratio.value = 4; comp.attack.value = 0.003; comp.release.value = 0.2;
      this.master.connect(comp).connect(ctx.destination);

      this.musicBus = ctx.createGain(); this.musicBus.gain.value = 0.55; this.musicBus.connect(this.master);
      this.sfxBus = ctx.createGain(); this.sfxBus.gain.value = 0.9; this.sfxBus.connect(this.master);

      // Delay (écho de l'arpège)
      this.delayIn = ctx.createGain(); this.delayIn.gain.value = 0.35;
      this.delay = ctx.createDelay(1.0); this.delay.delayTime.value = 0.34;
      const fb = ctx.createGain(); fb.gain.value = 0.38;
      const dlp = ctx.createBiquadFilter(); dlp.type = 'lowpass'; dlp.frequency.value = 2600;
      this.delayIn.connect(this.delay); this.delay.connect(dlp); dlp.connect(fb); fb.connect(this.delay);
      dlp.connect(this.musicBus);

      // Bruit blanc réutilisable
      const len = ctx.sampleRate * 2;
      this.noiseBuf = ctx.createBuffer(1, len, ctx.sampleRate);
      const d = this.noiseBuf.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
    }
    if (this.ctx.state === 'suspended') this.ctx.resume().catch(() => {});
    return true;
  }

  suspend() { if (this.ctx && this.ctx.state === 'running') this.ctx.suspend().catch(() => {}); }
  resume() { if (this.ctx && this.ctx.state === 'suspended') this.ctx.resume().catch(() => {}); }

  /* ---------- Mute ---------- */
  setMuted(m) {
    this.muted = !!m;
    try { localStorage.setItem(STORE_KEY, this.muted ? '1' : '0'); } catch { /* ignore */ }
    if (this.ctx) this.master.gain.setTargetAtTime(this.muted ? 0 : 0.85, this.ctx.currentTime, 0.03);
    return this.muted;
  }
  toggleMute() { return this.setMuted(!this.muted); }

  /* ---------- Briques de base ---------- */
  _tone({ freq, type = 'sine', t, dur = 0.2, vol = 0.2, attack = 0.005, bus, slide = null, cutoff = null, cutoffEnd = null, detune = 0, send = false }) {
    const ctx = this.ctx;
    t = t ?? ctx.currentTime;
    const o = ctx.createOscillator();
    o.type = type;
    o.frequency.setValueAtTime(freq, t);
    if (slide) o.frequency.exponentialRampToValueAtTime(Math.max(1, slide), t + dur);
    o.detune.value = detune;

    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(vol, t + attack);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);

    let node = o;
    if (cutoff) {
      const f = ctx.createBiquadFilter();
      f.type = 'lowpass'; f.Q.value = 4;
      f.frequency.setValueAtTime(cutoff, t);
      if (cutoffEnd) f.frequency.exponentialRampToValueAtTime(cutoffEnd, t + dur);
      o.connect(f); node = f;
    }
    node.connect(g);
    g.connect(bus || this.sfxBus);
    if (send) g.connect(this.delayIn);
    o.start(t);
    o.stop(t + dur + 0.05);
  }

  _noise({ t, dur = 0.2, vol = 0.2, type = 'bandpass', freq = 1000, freqEnd = null, q = 1, bus, attack = 0.002 }) {
    const ctx = this.ctx;
    t = t ?? ctx.currentTime;
    const src = ctx.createBufferSource();
    src.buffer = this.noiseBuf;
    const f = ctx.createBiquadFilter();
    f.type = type; f.Q.value = q;
    f.frequency.setValueAtTime(freq, t);
    if (freqEnd) f.frequency.exponentialRampToValueAtTime(Math.max(20, freqEnd), t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(vol, t + attack);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    src.connect(f); f.connect(g); g.connect(bus || this.sfxBus);
    src.start(t, Math.random() * 1.2);
    src.stop(t + dur + 0.05);
  }

  /* ---------- Musique ---------- */
  startMusic() {
    if (!this.ctx || this.musicRunning) return;
    this.musicRunning = true;
    this.step = 0;
    this.nextTime = this.ctx.currentTime + 0.08;
    this.timer = setInterval(() => this._tick(), 25);
  }
  stopMusic() {
    this.musicRunning = false;
    if (this.timer) { clearInterval(this.timer); this.timer = null; }
  }
  /** 0 → calme (menu), 1 → intense (vitesse max). */
  setIntensity(x) {
    this.intensity = Math.min(1, Math.max(0, x));
    this.bpm = 106 + this.intensity * 30;
  }
  /** Impulsion visuelle (0..1) synchronisée sur la grosse caisse. */
  beat() {
    if (!this.ctx || !this.musicRunning) return 0;
    const dt = this.ctx.currentTime - this.lastKick;
    if (dt < 0) return 0;
    return Math.exp(-dt * 7);
  }

  _tick() {
    if (!this.ctx || !this.musicRunning) return;
    const now = this.ctx.currentTime;
    if (this.nextTime < now - 0.4) this.nextTime = now + 0.05; // onglet en arrière-plan : on resynchronise
    while (this.nextTime < now + 0.12) {
      this._scheduleStep(this.step, this.nextTime);
      this.nextTime += 60 / this.bpm / 4;
      this.step++;
    }
  }

  _scheduleStep(step, t) {
    const s = step % 16;
    const bar = Math.floor(step / 16) % 4;
    const k = this.intensity;
    const sixteenth = 60 / this.bpm / 4;

    if (s % 4 === 0) this.lastKick = t; // la pulsation visuelle fonctionne même en mode muet
    if (this.muted) return;

    const bus = this.musicBus;

    // Grosse caisse (4 temps)
    if (s % 4 === 0) {
      this._tone({ freq: 150, slide: 42, type: 'sine', t, dur: 0.28, vol: 0.9, attack: 0.002, bus });
      this._noise({ t, dur: 0.03, vol: 0.15, type: 'highpass', freq: 2500, bus });
    }
    // Clap / caisse claire (temps 2 et 4)
    if (s === 4 || s === 12) {
      this._noise({ t, dur: 0.18, vol: 0.32, type: 'bandpass', freq: 1900, q: 0.8, bus });
      this._tone({ freq: 210, slide: 120, type: 'triangle', t, dur: 0.1, vol: 0.25, bus });
    }
    // Charleston
    if (s % 4 === 2) this._noise({ t, dur: 0.09, vol: 0.12, type: 'highpass', freq: 7500, bus });
    else if (k > 0.3 && s % 2 === 1) this._noise({ t, dur: 0.03, vol: 0.05, type: 'highpass', freq: 8500, bus });

    // Basse (croches)
    if (s % 2 === 0) {
      const note = ROOTS[bar] + (s % 4 === 2 ? 12 : 0);
      this._tone({ freq: mtof(note), type: 'sawtooth', t, dur: sixteenth * 1.8, vol: 0.2, attack: 0.004, cutoff: 450 + k * 1100, cutoffEnd: 180, bus });
    }
    // Arpège (16es)
    {
      const n = CHORDS[bar][ARP[s]] + (s >= 8 ? 12 : 0);
      this._tone({ freq: mtof(n), type: 'sawtooth', t, dur: sixteenth * 1.4, vol: 0.04 + k * 0.035, attack: 0.003, cutoff: 1800 + k * 1800, cutoffEnd: 600, bus, send: true });
    }
    // Pad
    if (s === 0) {
      const dur = sixteenth * 16;
      for (const n of CHORDS[bar].slice(0, 3)) {
 this._tone({ freq: mtof(n - 12), type: 'sawtooth', t, dur, vol: 0.035, attack: 0.5, cutoff: 900, bus, detune: -7 });
 this._tone({ freq: mtof(n - 12), type: 'sawtooth', t, dur, vol: 0.035, attack: 0.5, cutoff: 900, bus, detune: 7 });
      }
    }
    // Lead (quand l'intensité monte)
    if (k > 0.4) {
      const n = LEAD[bar][s];
      if (n) this._tone({ freq: mtof(n), type: 'square', t, dur: sixteenth * 3, vol: 0.05 + (k - 0.4) * 0.05, attack: 0.01, cutoff: 3200, cutoffEnd: 1400, bus, send: true });
    }
  }

  /* ---------- Moteur ---------- */
  startEngine() {
    if (!this.ctx || this.engine) return;
    const ctx = this.ctx;
    const o1 = ctx.createOscillator(); o1.type = 'sawtooth'; o1.frequency.value = 55;
    const o2 = ctx.createOscillator(); o2.type = 'square'; o2.frequency.value = 27.5;
    const f = ctx.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = 300; f.Q.value = 3;
    const g = ctx.createGain(); g.gain.value = 0;
    o1.connect(f); o2.connect(f); f.connect(g); g.connect(this.sfxBus);
    o1.start(); o2.start();
    g.gain.setTargetAtTime(this.muted ? 0 : 0.05, ctx.currentTime, 0.2);
    this.engine = { o1, o2, f, g };
  }
  updateEngine(norm, boosting) {
    if (!this.engine) return;
    const t = this.ctx.currentTime;
    const base = 50 + norm * 70 + (boosting ? 30 : 0);
    this.engine.o1.frequency.setTargetAtTime(base, t, 0.1);
    this.engine.o2.frequency.setTargetAtTime(base / 2, t, 0.1);
    this.engine.f.frequency.setTargetAtTime(260 + norm * 900 + (boosting ? 600 : 0), t, 0.1);
  }
  stopEngine() {
    if (!this.engine) return;
    const { o1, o2, g } = this.engine;
    const t = this.ctx.currentTime;
    g.gain.setTargetAtTime(0, t, 0.08);
    o1.stop(t + 0.5); o2.stop(t + 0.5);
    this.engine = null;
  }

  /* ---------- Effets sonores ---------- */
  _ok() { return this.ctx && !this.muted; }

  collect(combo = 1) {
    if (!this._ok()) return;
    const scale = [0, 2, 4, 7, 9, 12, 14, 16, 19];
    const n = 72 + scale[Math.min(scale.length - 1, Math.max(0, combo - 1))];
    const t = this.ctx.currentTime;
    this._tone({ freq: mtof(n), type: 'triangle', t, dur: 0.14, vol: 0.28 });
    this._tone({ freq: mtof(n + 7), type: 'sine', t: t + 0.055, dur: 0.2, vol: 0.22 });
  }
  shield() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    [64, 68, 71, 76].forEach((n, i) => this._tone({ freq: mtof(n), type: 'triangle', t: t + i * 0.06, dur: 0.3, vol: 0.22 }));
    this._noise({ t, dur: 0.4, vol: 0.06, type: 'highpass', freq: 3000, freqEnd: 9000 });
  }
  shieldBreak() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._noise({ t, dur: 0.45, vol: 0.4, type: 'bandpass', freq: 3500, freqEnd: 300, q: 0.7 });
    this._tone({ freq: 700, slide: 90, type: 'sawtooth', t, dur: 0.35, vol: 0.25, cutoff: 2500 });
  }
  boost() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._tone({ freq: 180, slide: 1400, type: 'sawtooth', t, dur: 0.7, vol: 0.22, cutoff: 3500, cutoffEnd: 5000 });
    this._noise({ t, dur: 0.8, vol: 0.2, type: 'bandpass', freq: 400, freqEnd: 6000, q: 0.6 });
    this._tone({ freq: mtof(79), type: 'square', t: t + 0.25, dur: 0.25, vol: 0.1, cutoff: 3000 });
  }
  jump() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._tone({ freq: 260, slide: 720, type: 'square', t, dur: 0.18, vol: 0.12, cutoff: 2600 });
    this._noise({ t, dur: 0.12, vol: 0.05, type: 'highpass', freq: 4000 });
  }
  land() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._noise({ t, dur: 0.14, vol: 0.2, type: 'lowpass', freq: 700, freqEnd: 120 });
    this._tone({ freq: 90, slide: 45, type: 'sine', t, dur: 0.14, vol: 0.3 });
  }
  smash() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._noise({ t, dur: 0.35, vol: 0.35, type: 'lowpass', freq: 4000, freqEnd: 200, q: 0.8 });
    this._tone({ freq: 220, slide: 50, type: 'sawtooth', t, dur: 0.25, vol: 0.2, cutoff: 1500 });
  }
  explosion() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._noise({ t, dur: 1.2, vol: 0.7, type: 'lowpass', freq: 3500, freqEnd: 70, q: 0.9 });
    this._tone({ freq: 130, slide: 28, type: 'sine', t, dur: 0.9, vol: 0.8 });
    this._tone({ freq: 90, slide: 30, type: 'sawtooth', t, dur: 0.8, vol: 0.25, cutoff: 800, cutoffEnd: 100 });
  }
  gameOver() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime + 0.35;
    [69, 65, 62, 57].forEach((n, i) =>
      this._tone({ freq: mtof(n), type: 'sawtooth', t: t + i * 0.22, dur: 0.5, vol: 0.14, cutoff: 1800, cutoffEnd: 300 }));
  }
  record() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime + 0.5;
    [72, 76, 79, 84, 88].forEach((n, i) =>
      this._tone({ freq: mtof(n), type: 'triangle', t: t + i * 0.09, dur: 0.3, vol: 0.2 }));
  }
  click() {
    if (!this._ok()) return;
    this._tone({ freq: 900, slide: 1500, type: 'square', dur: 0.05, vol: 0.08, cutoff: 3000 });
  }
  start() {
    if (!this._ok()) return;
    const t = this.ctx.currentTime;
    this._tone({ freq: 120, slide: 900, type: 'sawtooth', t, dur: 0.5, vol: 0.18, cutoff: 2500, cutoffEnd: 4000 });
    this._noise({ t, dur: 0.5, vol: 0.12, type: 'bandpass', freq: 300, freqEnd: 5000 });
  }
}

export const audio = new AudioEngine();
