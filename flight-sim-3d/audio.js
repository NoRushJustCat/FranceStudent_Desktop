/* AERO HORIZON 3D — synthèse procédurale Web Audio API.
 * Aucune ressource audio externe. Appeler unlock() depuis le clic sur « DÉCOLLER »,
 * ou laisser le premier pointerdown / keydown déverrouiller automatiquement l'audio.
 * throttle : 0..1 ; speed : vitesse indiquée en nœuds (kts).
 */

const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
const safe = (value, fallback = 0) => Number.isFinite(value) ? value : fallback;

export class FlightAudio {
  constructor() {
    this.ctx = null;
    this.master = null;
    this.volume = 0.7;
    this.muted = false;
    this.throttle = 0;
    this.speed = 0;
    this.windSpeed = 0;
    this.disposed = false;
    this.alarms = {
      stall: { active: false, timer: null, gate: null, step: 0 },
      pullUp: { active: false, timer: null, gate: null, step: 0 },
      overspeed: { active: false, timer: null, gate: null, step: 0 },
    };

    // La création d'AudioContext reste différée jusqu'à une interaction humaine.
    this._onInteraction = () => { this.unlock(); };
    if (typeof window !== 'undefined') {
      window.addEventListener('pointerdown', this._onInteraction, { once: true });
      window.addEventListener('keydown', this._onInteraction, { once: true });
    }
  }

  /** À appeler idéalement directement dans le gestionnaire du bouton DÉCOLLER. */
  unlock() {
    if (this.disposed || typeof window === 'undefined') return false;
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) return false;

    if (!this.ctx) {
      try {
        const ctx = new AudioContextClass();
        this.ctx = ctx;
        this.master = ctx.createGain();
        this.master.gain.value = this.muted ? 0 : this.volume;
        const limiter = ctx.createDynamicsCompressor();
        limiter.threshold.value = -18;
        limiter.knee.value = 12;
        limiter.ratio.value = 8;
        limiter.attack.value = 0.003;
        limiter.release.value = 0.2;
        this.master.connect(limiter).connect(ctx.destination);

        // Le même tampon de bruit blanc alimente les boucles et les bruitages.
        const length = Math.floor(ctx.sampleRate * 2);
        this.noiseBuffer = ctx.createBuffer(1, length, ctx.sampleRate);
        const samples = this.noiseBuffer.getChannelData(0);
        for (let i = 0; i < length; i++) samples[i] = Math.random() * 2 - 1;

        this._createContinuousSounds();
        this._applyEngine();
        this._applyWind();
        for (const name of Object.keys(this.alarms)) {
          if (this.alarms[name].active) this._startAlarm(name);
        }
      } catch (error) {
        console.warn('Audio Web indisponible :', error);
        this.ctx = null;
        return false;
      }
    }

    // resume() doit être invoqué depuis le geste utilisateur sur les navigateurs mobiles.
    if (this.ctx.state === 'suspended') this.ctx.resume().catch(() => {});
    this._removeInteractionListeners();
    return true;
  }

  _removeInteractionListeners() {
    if (typeof window === 'undefined') return;
    window.removeEventListener('pointerdown', this._onInteraction);
    window.removeEventListener('keydown', this._onInteraction);
  }

  _createContinuousSounds() {
    const ctx = this.ctx;
    const loop = () => {
      const source = ctx.createBufferSource();
      source.buffer = this.noiseBuffer;
      source.loop = true;
      source.start();
      return source;
    };

    this.engineNoise = loop();
    this.engineFilter = ctx.createBiquadFilter();
    this.engineFilter.type = 'bandpass';
    this.engineFilter.Q.value = 0.65;
    this.engineNoiseGain = ctx.createGain();
    this.engineNoise.connect(this.engineFilter).connect(this.engineNoiseGain).connect(this.master);

    this.rumble = ctx.createOscillator();
    this.rumble.type = 'sawtooth';
    this.rumbleFilter = ctx.createBiquadFilter();
    this.rumbleFilter.type = 'lowpass';
    this.rumbleFilter.frequency.value = 170;
    this.rumbleGain = ctx.createGain();
    this.rumble.connect(this.rumbleFilter).connect(this.rumbleGain).connect(this.master);
    this.rumble.start();

    this.whine = ctx.createOscillator();
    this.whine.type = 'sine';
    this.whineGain = ctx.createGain();
    this.whine.connect(this.whineGain).connect(this.master);
    this.whine.start();
    this.whineLFO = ctx.createOscillator();
    this.whineLFO.type = 'sine';
    this.whineMod = ctx.createGain();
    this.whineMod.gain.value = 12;
    this.whineLFO.connect(this.whineMod).connect(this.whine.frequency);
    this.whineLFO.start();

    this.windNoise = loop();
    this.windFilter = ctx.createBiquadFilter();
    this.windFilter.type = 'bandpass';
    this.windFilter.Q.value = 0.45;
    this.windGain = ctx.createGain();
    this.windNoise.connect(this.windFilter).connect(this.windGain).connect(this.master);
  }

  _smooth(param, value, timeConstant = 0.09) {
    param.setTargetAtTime(value, this.ctx.currentTime, timeConstant);
  }

  _applyEngine() {
    if (!this.ctx) return;
    const t = this.throttle;
    const air = clamp(this.speed / 450, 0, 1);
    this._smooth(this.engineFilter.frequency, 220 + t * 2400 + air * 290);
    this._smooth(this.engineNoiseGain.gain, 0.025 + t * 0.28);
    this._smooth(this.rumble.frequency, 38 + t * 52);
    this._smooth(this.rumbleGain.gain, 0.026 + t * 0.085);
    this._smooth(this.whine.frequency, 370 + t * 1120 + air * 100);
    this._smooth(this.whineLFO.frequency, 5 + t * 9);
    this._smooth(this.whineMod.gain, 9 + t * 23);
    this._smooth(this.whineGain.gain, 0.004 + t * t * 0.065);
  }

  /** throttle entre 0 et 1 ; speed en kts. */
  updateEngine(throttle, speed) {
    this.throttle = clamp(safe(throttle), 0, 1);
    this.speed = Math.max(0, safe(speed));
    this._applyEngine();
  }

  _applyWind() {
    if (!this.ctx) return;
    const v = clamp(this.windSpeed / 450, 0, 1);
    this._smooth(this.windFilter.frequency, 550 + v * 3800);
    this._smooth(this.windGain.gain, v * v * 0.24);
  }

  /** speed en kts ; indépendant de updateEngine pour faciliter la boucle de jeu. */
  updateWind(speed) {
    this.windSpeed = Math.max(0, safe(speed));
    this._applyWind();
  }

  /** Oscillateur ponctuel avec enveloppe ; déconnexion des nœuds après lecture. */
  _tone(frequency, duration, gain, options = {}) {
    if (!this.ctx || this.ctx.state === 'closed') return;
    const ctx = this.ctx;
    const at = ctx.currentTime + (options.delay || 0);
    const oscillator = ctx.createOscillator();
    const envelope = ctx.createGain();
    oscillator.type = options.type || 'sine';
    oscillator.frequency.setValueAtTime(Math.max(20, frequency), at);
    if (options.endFrequency) {
      oscillator.frequency.exponentialRampToValueAtTime(Math.max(20, options.endFrequency), at + duration);
    }
    envelope.gain.setValueAtTime(0, at);
    envelope.gain.linearRampToValueAtTime(gain, at + Math.min(0.015, duration * 0.2));
    envelope.gain.exponentialRampToValueAtTime(0.0001, at + duration);
    oscillator.connect(envelope).connect(options.bus || this.master);
    oscillator.onended = () => { oscillator.disconnect(); envelope.disconnect(); };
    oscillator.start(at);
    oscillator.stop(at + duration + 0.01);
  }

  _burst(duration, gain, startFrequency, endFrequency, options = {}) {
    if (!this.ctx || this.ctx.state === 'closed') return;
    const ctx = this.ctx;
    const at = ctx.currentTime + (options.delay || 0);
    const source = ctx.createBufferSource();
    source.buffer = this.noiseBuffer;
    const filter = ctx.createBiquadFilter();
    filter.type = options.filter || 'bandpass';
    filter.Q.value = options.q || 0.7;
    filter.frequency.setValueAtTime(Math.max(30, startFrequency), at);
    filter.frequency.exponentialRampToValueAtTime(Math.max(30, endFrequency), at + duration);
    const envelope = ctx.createGain();
    envelope.gain.setValueAtTime(0.0001, at);
    envelope.gain.linearRampToValueAtTime(gain, at + Math.min(0.02, duration * 0.1));
    envelope.gain.exponentialRampToValueAtTime(0.0001, at + duration);
    source.connect(filter).connect(envelope).connect(options.bus || this.master);
    source.onended = () => { source.disconnect(); filter.disconnect(); envelope.disconnect(); };
    source.start(at);
    source.stop(at + duration + 0.01);
  }

  _alarmTick(name) {
    const alarm = this.alarms[name];
    if (!alarm.active || !this.ctx || this.ctx.state !== 'running') return;
    const bus = alarm.gate;
    if (name === 'stall') {
      // Avertisseur urgent, haché, en rafales.
      this._tone(840, 0.13, 0.15, { type: 'square', bus });
    } else if (name === 'pullUp') {
      // Alternance descendante « whoop-whoop » : pas de voix préenregistrée.
      this._tone(alarm.step % 2 ? 740 : 1040, 0.24, 0.20,
        { type: 'sawtooth', endFrequency: alarm.step % 2 ? 400 : 600, bus });
    } else {
      this._tone(alarm.step % 2 ? 610 : 690, 0.12, 0.12, { type: 'triangle', bus });
    }
    alarm.step++;
  }

  _startAlarm(name) {
    const alarm = this.alarms[name];
    if (!this.ctx || alarm.timer !== null) return;
    alarm.gate = ctxGain(this.ctx, this.master);
    alarm.step = 0;
    const interval = name === 'stall' ? 205 : name === 'pullUp' ? 320 : 480;
    this._alarmTick(name);
    alarm.timer = setInterval(() => this._alarmTick(name), interval);
  }

  _setAlarm(name, active) {
    const alarm = this.alarms[name];
    if (this.disposed || alarm.active === Boolean(active)) return;
    alarm.active = Boolean(active);
    if (alarm.active) {
      this._startAlarm(name);
    } else {
      if (alarm.timer !== null) clearInterval(alarm.timer);
      alarm.timer = null;
      if (alarm.gate && this.ctx && this.ctx.state !== 'closed') {
        alarm.gate.gain.cancelScheduledValues(this.ctx.currentTime);
        alarm.gate.gain.setTargetAtTime(0, this.ctx.currentTime, 0.01);
        // Les dernières impulsions programmées s'achèvent avant déconnexion.
        const oldGate = alarm.gate;
        setTimeout(() => oldGate.disconnect(), 500);
      }
      alarm.gate = null;
    }
  }

  playStallAlarm(active) { this._setAlarm('stall', active); }
  playPullUpAlarm(active) { this._setAlarm('pullUp', active); }
  playOverspeedAlarm(active) { this._setAlarm('overspeed', active); }

  playGearSound(isDeploying) {
    if (!this.ctx) return;
    const down = Boolean(isDeploying);
    this._burst(1.25, 0.16, down ? 1500 : 800, down ? 350 : 1800, { q: 1.1 });
    this._tone(down ? 160 : 110, 1.1, 0.09,
      { type: 'sawtooth', endFrequency: down ? 95 : 200 });
    this._tone(430, 0.10, 0.12, { delay: 1.2, type: 'triangle' });
  }

  playFlapsSound() {
    if (!this.ctx) return;
    for (let i = 0; i < 3; i++) {
      this._burst(0.065, 0.11, 1200, 320, { delay: i * 0.11 });
      this._tone(180 - i * 15, 0.08, 0.075, { delay: i * 0.11, type: 'square' });
    }
  }

  playWaypointRing() {
    if (!this.ctx) return;
    [523.25, 659.25, 783.99, 1046.5].forEach((note, index) => {
      this._tone(note, 0.5, 0.09, { delay: index * 0.105 });
      this._tone(note * 2, 0.32, 0.025, { delay: index * 0.105 });
    });
  }

  playCrashSound() {
    if (!this.ctx) return;
    // Enveloppes exponentielles dans _burst / _tone : impact puis grondement décroissant.
    this._burst(2.8, 0.75, 3600, 90, { filter: 'lowpass', q: 0.6 });
    this._tone(115, 2.1, 0.3, { type: 'sawtooth', endFrequency: 28 });
    this._burst(0.13, 0.3, 5200, 400, { filter: 'highpass' });
  }

  setVolume(volume) {
    this.volume = clamp(safe(volume, this.volume), 0, 1);
    if (this.ctx) this._smooth(this.master.gain, this.muted ? 0 : this.volume, 0.025);
    return this.volume;
  }

  setMasterVolume(volume) { return this.setVolume(volume); }
  toggleMute() {
    this.muted = !this.muted;
    if (this.ctx) this._smooth(this.master.gain, this.muted ? 0 : this.volume, 0.025);
    return this.muted;
  }
  isMuted() { return this.muted; }

  /** Libère le contexte et les minuteries lors d'un changement de scène définitif. */
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this._removeInteractionListeners();
    for (const alarm of Object.values(this.alarms)) {
      if (alarm.timer !== null) clearInterval(alarm.timer);
      alarm.timer = null;
      alarm.active = false;
    }
    if (this.ctx) {
      for (const source of [this.engineNoise, this.windNoise, this.rumble, this.whine, this.whineLFO]) {
        try { source.stop(); } catch { /* source déjà arrêtée */ }
      }
      this.ctx.close().catch(() => {});
      this.ctx = null;
    }
  }
}

function ctxGain(ctx, destination) {
  const gain = ctx.createGain();
  gain.gain.value = 1;
  gain.connect(destination);
  return gain;
}
