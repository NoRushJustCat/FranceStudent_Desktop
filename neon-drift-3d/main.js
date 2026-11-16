/* =========================================================
   NEON DRIFT 3D — main.js
   Jeu d'arcade synthwave en Three.js (ES Modules, sans build)
   ========================================================= */
import * as THREE from 'https://unpkg.com/three@0.170.0/build/three.module.js';
import { audio } from './audio.js';

/* ---------- Utilitaires ---------- */
const $ = (id) => document.getElementById(id);
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const lerp = (a, b, t) => a + (b - a) * t;
const rand = (a, b) => a + Math.random() * (b - a);
const damp = (a, b, k, dt) => lerp(a, b, 1 - Math.exp(-k * dt));
const smooth = (t) => t * t * (3 - 2 * t);

const CFG = {
  limitX: 7.4,        // limite latérale du vaisseau
  roadHalf: 9, // demi-largeur de la piste
  spawnZ: -250,       // distance d'apparition
  killZ: 14,   // suppression derrière la caméra
  baseSpeed: 32,
  maxSpeed: 88,
  rampTime: 160,      // secondes pour atteindre la difficulté max
  gravity: 32,
  jumpV: 12.8,
  rest: 0.6,          // hauteur de vol au sol
  boostTime: 2.8,
  boostMult: 1.5,
  shipHW: 0.85, shipHH: 0.34, shipHD: 1.3, // hitbox du vaisseau (demi-dimensions)
  comboWindow: 3.2,
};

const BEST_KEY = 'neonDrift3dBest';
const loadBest = () => { try { return parseInt(localStorage.getItem(BEST_KEY) || '0', 10) || 0; } catch { return 0; } };
const saveBest = (v) => { try { localStorage.setItem(BEST_KEY, String(v)); } catch { /* ignore */ } };

/* =========================================================
   RENDU / SCÈNE
   ========================================================= */
const canvas = $('game');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: 'high-performance' });
let pixelRatio = Math.min(window.devicePixelRatio || 1, 1.75);
renderer.setPixelRatio(pixelRatio);
renderer.setSize(window.innerWidth, window.innerHeight);

const FOG_COLOR = new THREE.Color(0x2c0a4e);
const FOG_DENSITY = 0.0085;
const scene = new THREE.Scene();
scene.background = FOG_COLOR.clone();
scene.fog = new THREE.FogExp2(FOG_COLOR, FOG_DENSITY);

const camera = new THREE.PerspectiveCamera(68, window.innerWidth / window.innerHeight, 0.1, 2500);
camera.position.set(0, 3.6, 8.4);

// Lumières
scene.add(new THREE.HemisphereLight(0x8a5cff, 0x1a0030, 1.3));
const keyLight = new THREE.DirectionalLight(0xffa0f0, 2.4);
keyLight.position.set(3, 8, 10);
scene.add(keyLight);
const rimLight = new THREE.DirectionalLight(0x00e5ff, 1.6);
rimLight.position.set(-6, 3, -8);
scene.add(rimLight);

/* ---------- Post-processing (bloom) avec repli automatique ---------- */
let composer = null;
async function setupPostFX() {
  try {
    const [{ EffectComposer }, { RenderPass }, { UnrealBloomPass }, { OutputPass }] = await Promise.all([
      import('three/addons/postprocessing/EffectComposer.js'),
      import('three/addons/postprocessing/RenderPass.js'),
      import('three/addons/postprocessing/UnrealBloomPass.js'),
      import('three/addons/postprocessing/OutputPass.js'),
    ]);
    const s = renderer.getDrawingBufferSize(new THREE.Vector2());
    const rt = new THREE.WebGLRenderTarget(s.x, s.y, { type: THREE.HalfFloatType, samples: 4 });
    const c = new EffectComposer(renderer, rt);
    c.setPixelRatio(pixelRatio);
    c.setSize(window.innerWidth, window.innerHeight);
    c.addPass(new RenderPass(scene, camera));
    c.addPass(new UnrealBloomPass(new THREE.Vector2(window.innerWidth, window.innerHeight), 0.85, 0.5, 0.55));
    c.addPass(new OutputPass());
    composer = c;
  } catch (err) {
    console.warn('[NEON DRIFT] Bloom indisponible, rendu simple.', err);
    composer = null;
  }
}

/* ---------- Ciel dégradé + étoiles ---------- */
const sky = new THREE.Mesh(
  new THREE.SphereGeometry(1500, 32, 16),
  new THREE.ShaderMaterial({
    side: THREE.BackSide, depthWrite: false, fog: false,
    uniforms: { uHorizon: { value: FOG_COLOR }, uTop: { value: new THREE.Color(0x020010) }, uMid: { value: new THREE.Color(0x14043a) } },
    vertexShader: `varying vec3 vDir; void main(){ vDir = normalize(position); gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
    fragmentShader: `
      varying vec3 vDir; uniform vec3 uHorizon; uniform vec3 uTop; uniform vec3 uMid;
      void main(){
 float h = clamp(vDir.y, 0.0, 1.0);
        vec3 c = mix(uHorizon, uMid, smoothstep(0.0, 0.18, h));
        c = mix(c, uTop, smoothstep(0.1, 0.8, h));
 gl_FragColor = vec4(c, 1.0);
        #include <tonemapping_fragment>
        #include <colorspace_fragment>
      }`,
  })
);
sky.renderOrder = -3; sky.frustumCulled = false;
scene.add(sky);

const starCount = 900;
const starPos = new Float32Array(starCount * 3);
const starCol = new Float32Array(starCount * 3);
for (let i = 0; i < starCount; i++) {
  const u = Math.random() * Math.PI * 2;
  const y = Math.pow(Math.random(), 0.7) * 0.95 + 0.04;
  const r = Math.sqrt(1 - y * y);
  starPos.set([Math.cos(u) * r * 1200, y * 1200, Math.sin(u) * r * 1200], i * 3);
  const tint = Math.random();
  const b = rand(0.5, 1);
  if (tint < 0.2) starCol.set([b, 0.5 * b, b], i * 3);
  else if (tint < 0.4) starCol.set([0.5 * b, b, b], i * 3);
  else starCol.set([b, b, b], i * 3);
}
const starGeo = new THREE.BufferGeometry();
starGeo.setAttribute('position', new THREE.BufferAttribute(starPos, 3));
starGeo.setAttribute('color', new THREE.BufferAttribute(starCol, 3));
const stars = new THREE.Points(starGeo, new THREE.PointsMaterial({ size: 1.8, sizeAttenuation: false, vertexColors: true, transparent: true, opacity: 0.9, depthWrite: false, fog: false }));
stars.renderOrder = -2; stars.frustumCulled = false;
scene.add(stars);

/* ---------- Soleil rétro-futuriste ---------- */
const sunMat = new THREE.ShaderMaterial({
  transparent: true, depthWrite: false, fog: false,
  uniforms: { uTime: { value: 0 }, uBeat: { value: 0 } },
  vertexShader: `varying vec2 vUv; void main(){ vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
  fragmentShader: `
    varying vec2 vUv; uniform float uTime; uniform float uBeat;
    void main(){
      vec2 p = (vUv - 0.5) * 3.4;    // [-1.7, 1.7]
      float r = length(p);
      float y = p.y * 0.5 + 0.5;      // 0 bas, 1 haut du disque
      vec3 col; float a;
      if (r < 1.0) {
 col = mix(vec3(1.0, 0.1, 0.55), vec3(1.0, 0.88, 0.2), smoothstep(0.0, 1.0, y));
        float band = fract(y * 16.0 - uTime * 0.06);
        float cut = (1.0 - y) * 0.75 - 0.08;
 if (y < 0.62 && band < cut) discard;
        a = 1.0 - smoothstep(0.985, 1.0, r);
 col *= 1.15 + uBeat * 0.35;
      } else {
        float g = exp(-(r - 1.0) * 2.8);
        col = mix(vec3(1.0, 0.2, 0.7), vec3(0.6, 0.2, 1.0), clamp((r - 1.0) * 1.2, 0.0, 1.0));
        a = g * (0.4 + uBeat * 0.2);
      }
      gl_FragColor = vec4(col, a);
      #include <tonemapping_fragment>
      #include <colorspace_fragment>
    }`,
});
const SUN_R = 180;
const sun = new THREE.Mesh(new THREE.PlaneGeometry(SUN_R * 2 * 1.7, SUN_R * 2 * 1.7), sunMat);
sun.position.set(0, 100, -900);
sun.renderOrder = -1; sun.frustumCulled = false;
scene.add(sun);

/* ---------- Sol : grille néon infinie (shader) ---------- */
const groundMat = new THREE.ShaderMaterial({
  uniforms: {
    uScroll: { value: 0 }, uTime: { value: 0 }, uBeat: { value: 0 },
    uFogColor: { value: FOG_COLOR }, uFogDensity: { value: FOG_DENSITY }, uRoad: { value: CFG.roadHalf },
  },
  vertexShader: `
    varying vec3 vWorld; varying float vDist;
    void main(){
      vec4 wp = modelMatrix * vec4(position, 1.0);
      vWorld = wp.xyz;
      vec4 mv = viewMatrix * wp;
      vDist = length(mv.xyz);
      gl_Position = projectionMatrix * mv;
    }`,
  fragmentShader: `
    varying vec3 vWorld; varying float vDist;
    uniform float uScroll; uniform float uTime; uniform float uBeat; uniform float uRoad;
    uniform vec3 uFogColor; uniform float uFogDensity;
    float gridLine(vec2 coord, float w){
      vec2 g = abs(fract(coord - 0.5) - 0.5) / (fwidth(coord) * w);
      return 1.0 - min(min(g.x, g.y), 1.0);
    }
    void main(){
      vec2 p = vec2(vWorld.x, vWorld.z - uScroll);
      float ax = abs(vWorld.x);
      float onRoad = 1.0 - smoothstep(uRoad, uRoad + 0.35, ax);
      float g = gridLine(p / 4.0, 1.3);
      vec3 magenta = vec3(1.6, 0.12, 1.2);
      vec3 cyan = vec3(0.1, 1.3, 1.7);
      vec3 lineCol = mix(magenta, cyan, onRoad);
      vec3 base = mix(vec3(0.03, 0.0, 0.08), vec3(0.05, 0.0, 0.16), onRoad);
      float edge = 1.0 - smoothstep(0.0, 0.2, abs(ax - uRoad));
      float dash = (1.0 - smoothstep(0.05, 0.13, ax)) * step(0.5, fract(p.y / 8.0));
      vec3 col = base + lineCol * g * (1.0 + uBeat * 0.7);
      col += cyan * edge * (1.4 + uBeat * 0.8);
      col += vec3(0.6, 0.2, 1.4) * dash * 0.6;
      float f = 1.0 - exp(-pow(uFogDensity * vDist, 2.0));
      col = mix(col, uFogColor, clamp(f, 0.0, 1.0));
      gl_FragColor = vec4(col, 1.0);
      #include <tonemapping_fragment>
      #include <colorspace_fragment>
    }`,
});
const ground = new THREE.Mesh(new THREE.PlaneGeometry(1400, 1800), groundMat);
ground.rotation.x = -Math.PI / 2;
ground.position.set(0, 0, -700);
ground.frustumCulled = false;
scene.add(ground);

/* ---------- Géométries / matériaux partagés ---------- */
const GEO = {
  box: new THREE.BoxGeometry(1, 1, 1),
  plane: new THREE.PlaneGeometry(1, 1),
  ring: new THREE.RingGeometry(0.85, 1, 48),
  pillar: new THREE.CylinderGeometry(0.9, 0.9, 1, 14),
  band: new THREE.TorusGeometry(0.95, 0.07, 6, 24),
  orbCore: new THREE.IcosahedronGeometry(0.32, 1),
  orbShell: new THREE.OctahedronGeometry(0.55, 0),
  shieldIco: new THREE.IcosahedronGeometry(0.62, 1),
  shieldSph: new THREE.SphereGeometry(0.5, 16, 12),
  boostRing: new THREE.TorusGeometry(2.2, 0.14, 8, 44),
  boostRing2: new THREE.TorusGeometry(1.8, 0.05, 6, 44),
  mount: new THREE.ConeGeometry(1, 1, 4, 1),
};
GEO.boxEdges = new THREE.EdgesGeometry(GEO.box);
GEO.mountEdges = new THREE.EdgesGeometry(GEO.mount);

const hdr = (r, g, b) => new THREE.Color().setRGB(r, g, b, THREE.LinearSRGBColorSpace);
const MAT = {
  dark: new THREE.MeshStandardMaterial({ color: 0x140c30, metalness: 0.8, roughness: 0.35, emissive: 0x08041a }),
  laserRed: new THREE.MeshBasicMaterial({ color: hdr(3, 0.15, 0.3) }),
  laserOrange: new THREE.MeshBasicMaterial({ color: hdr(3, 1.3, 0.1) }),
  panelRed: new THREE.MeshBasicMaterial({ color: 0xff2040, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, depthWrite: false }),
  panelOrange: new THREE.MeshBasicMaterial({ color: 0xff9a10, transparent: true, opacity: 0.2, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, depthWrite: false }),
  edgeRed: new THREE.LineBasicMaterial({ color: 0xff3a5a }),
  edgeOrange: new THREE.LineBasicMaterial({ color: 0xffb020 }),
  edgeCyan: new THREE.LineBasicMaterial({ color: 0x00f0ff }),
  edgeMagenta: new THREE.LineBasicMaterial({ color: 0xff2bd6 }),
  glowCyan: new THREE.MeshBasicMaterial({ color: hdr(0.2, 2.2, 2.6) }),
  glowMagenta: new THREE.MeshBasicMaterial({ color: hdr(2.6, 0.3, 2.0) }),
  orbCore: new THREE.MeshBasicMaterial({ color: hdr(2.4, 1.9, 0.4) }),
  orbShell: new THREE.MeshBasicMaterial({ color: hdr(0.3, 1.6, 2.2), wireframe: true, transparent: true, opacity: 0.8 }),
  shieldWire: new THREE.MeshBasicMaterial({ color: hdr(0.2, 1.8, 3), wireframe: true }),
  shieldSph: new THREE.MeshBasicMaterial({ color: 0x20c0ff, transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false }),
  boostRing: new THREE.MeshBasicMaterial({ color: hdr(3, 0.4, 2.4) }),
  boostRing2: new THREE.MeshBasicMaterial({ color: hdr(0.4, 2.4, 3) }),
  mount: new THREE.MeshBasicMaterial({ color: 0x12002a }),
  mountLineA: new THREE.LineBasicMaterial({ color: 0xff2bd6 }),
  mountLineB: new THREE.LineBasicMaterial({ color: 0x00e5ff }),
};

function box(parent, w, h, d, x, y, z, mat, edge) {
  const m = new THREE.Mesh(GEO.box, mat);
  m.scale.set(w, h, d);
  m.position.set(x, y, z);
  if (edge) m.add(new THREE.LineSegments(GEO.boxEdges, edge));
  parent.add(m);
  return m;
}

/* ---------- Montagnes (scrolling) + chaîne lointaine ---------- */
const mountains = [];
const MOUNT_LEN = 440;
{
  const perSide = 26;
  for (let side = -1; side <= 1; side += 2) {
    for (let i = 0; i < perSide; i++) {
      const h = rand(14, 52), w = rand(16, 40);
      const m = new THREE.Mesh(GEO.mount, MAT.mount);
      m.scale.set(w, h, w);
      m.rotation.y = rand(0, Math.PI);
      m.add(new THREE.LineSegments(GEO.mountEdges, Math.random() < 0.6 ? MAT.mountLineA : MAT.mountLineB));
      const x = side * (26 + rand(0, 75));
      const z = -i * (MOUNT_LEN / perSide) + rand(-5, 5);
      m.position.set(x, h / 2, z);
      scene.add(m);
      mountains.push(m);
    }
  }
  const farMat = new THREE.MeshBasicMaterial({ color: 0x16003a, fog: false });
  const farLine = new THREE.LineBasicMaterial({ color: 0xff4fd8, fog: false, transparent: true, opacity: 0.75 });
  for (let i = 0; i < 40; i++) {
    const x = -1000 + i * 52 + rand(-18, 18);
    if (Math.abs(x) < 170) continue; // vallée centrale : on laisse voir le soleil
    const h = rand(60, 190), w = rand(110, 200);
    const m = new THREE.Mesh(GEO.mount, farMat);
    m.scale.set(w, h, w);
    m.rotation.y = rand(0, Math.PI);
    m.add(new THREE.LineSegments(GEO.mountEdges, farLine));
    m.position.set(x, h / 2, -820);
    scene.add(m);
  }
}

/* ---------- Pylônes néon ---------- */
const pylons = [];
const PYLON_STEP = 24, PYLON_N = 16, PYLON_LEN = PYLON_STEP * PYLON_N;
for (let i = 0; i < PYLON_N; i++) {
  for (let side = -1; side <= 1; side += 2) {
    const g = new THREE.Group();
    const lamp = (i + (side > 0 ? 1 : 0)) % 2 === 0 ? MAT.glowCyan : MAT.glowMagenta;
    const edge = lamp === MAT.glowCyan ? MAT.edgeCyan : MAT.edgeMagenta;
    box(g, 0.45, 10, 0.45, 0, 5, 0, MAT.dark, edge);
    box(g, 2.4, 0.3, 0.3, -side * 1.1, 9.7, 0, MAT.dark, edge);
    box(g, 0.7, 0.18, 0.7, -side * 2.2, 9.5, 0, lamp);
    box(g, 0.1, 8, 0.1, -side * 0.28, 4.8, 0, lamp);
    g.position.set(side * (CFG.roadHalf + 2.6), 0, -i * PYLON_STEP);
    scene.add(g);
    pylons.push(g);
  }
}

/* =========================================================
   PARTICULES
   ========================================================= */
function makeGlowTexture() {
  const c = document.createElement('canvas');
  c.width = c.height = 64;
  const x = c.getContext('2d');
  const g = x.createRadialGradient(32, 32, 0, 32, 32, 32);
  g.addColorStop(0, 'rgba(255,255,255,1)');
  g.addColorStop(0.35, 'rgba(255,255,255,0.55)');
  g.addColorStop(1, 'rgba(255,255,255,0)');
  x.fillStyle = g; x.fillRect(0, 0, 64, 64);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}
const glowTex = makeGlowTexture();

class Particles {
  constructor(max, size, gravity = 0) {
    this.max = max; this.i = 0; this.gravity = gravity;
    this.pos = new Float32Array(max * 3);
    this.col = new Float32Array(max * 3);
    this.base = new Float32Array(max * 3);
    this.vel = new Float32Array(max * 3);
    this.life = new Float32Array(max);
    this.maxLife = new Float32Array(max).fill(1);
    for (let k = 0; k < max; k++) this.pos[k * 3 + 1] = -999;
    const geo = new THREE.BufferGeometry();
    this.aPos = new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage);
    this.aCol = new THREE.BufferAttribute(this.col, 3).setUsage(THREE.DynamicDrawUsage);
    geo.setAttribute('position', this.aPos);
    geo.setAttribute('color', this.aCol);
    this.points = new THREE.Points(geo, new THREE.PointsMaterial({
      size, map: glowTex, vertexColors: true, transparent: true, depthWrite: false,
      blending: THREE.AdditiveBlending, sizeAttenuation: true,
    }));
    this.points.frustumCulled = false;
    scene.add(this.points);
  }
  emit(x, y, z, vx, vy, vz, r, g, b, life) {
    const k = this.i; this.i = (this.i + 1) % this.max;
    const o = k * 3;
    this.pos[o] = x; this.pos[o + 1] = y; this.pos[o + 2] = z;
    this.vel[o] = vx; this.vel[o + 1] = vy; this.vel[o + 2] = vz;
    this.base[o] = r; this.base[o + 1] = g; this.base[o + 2] = b;
    this.life[k] = life; this.maxLife[k] = life;
  }
  update(dt, worldSpeed) {
    for (let k = 0; k < this.max; k++) {
      if (this.life[k] <= 0) continue;
      const o = k * 3;
      this.life[k] -= dt;
      if (this.life[k] <= 0) {
        this.pos[o + 1] = -999;
        this.col[o] = this.col[o + 1] = this.col[o + 2] = 0;
        continue;
      }
      this.vel[o + 1] -= this.gravity * dt;
      this.pos[o] += this.vel[o] * dt;
      this.pos[o + 1] += this.vel[o + 1] * dt;
      this.pos[o + 2] += (this.vel[o + 2] + worldSpeed) * dt;
      if (this.pos[o + 1] < 0.05 && this.gravity > 0) { this.pos[o + 1] = 0.05; this.vel[o + 1] *= -0.4; }
      const t = this.life[k] / this.maxLife[k];
      const f = t * t;
      this.col[o] = this.base[o] * f; this.col[o + 1] = this.base[o + 1] * f; this.col[o + 2] = this.base[o + 2] * f;
    }
    this.aPos.needsUpdate = true;
    this.aCol.needsUpdate = true;
  }
  clear() {
    this.life.fill(0); this.col.fill(0);
    for (let k = 0; k < this.max; k++) this.pos[k * 3 + 1] = -999;
    this.aPos.needsUpdate = true; this.aCol.needsUpdate = true;
  }
}
const trail = new Particles(600, 0.6, 0);
const debris = new Particles(1800, 0.55, 9);

function burst(x, y, z, count, palette, speed = 9, life = 1.0) {
  for (let i = 0; i < count; i++) {
    const a = Math.random() * Math.PI * 2;
    const u = Math.random() * 2 - 1;
    const s = Math.sqrt(1 - u * u);
    const sp = speed * rand(0.3, 1);
    const c = palette[(Math.random() * palette.length) | 0];
    debris.emit(x + rand(-0.3, 0.3), y + rand(-0.3, 0.3), z, Math.cos(a) * s * sp, Math.abs(u) * sp * 0.9 + 1, Math.sin(a) * s * sp, c[0], c[1], c[2], life * rand(0.5, 1.2));
  }
}
const PAL = {
  fire: [[2.5, 0.5, 0.2], [2.5, 1.4, 0.2], [2, 0.2, 0.4]],
  ship: [[0.3, 2, 2.5], [2.5, 0.3, 2.2], [2.5, 2.5, 2.5]],
  gold: [[2.5, 2, 0.4], [2.5, 1.2, 0.2]],
  cyan: [[0.3, 2, 2.6], [0.6, 1, 2.6]],
  pink: [[2.6, 0.4, 2.2], [2, 1, 2.6], [0.5, 2, 2.6]],
};

/* ---------- Ondes de choc ---------- */
const fxList = [];
function shockwave(x, y, z, color, max = 12) {
  const m = new THREE.Mesh(GEO.ring, new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, depthWrite: false }));
  m.position.set(x, y, z);
  scene.add(m);
  fxList.push({ m, t: 0, dur: 0.6, max });
}
function updateFx(dt, ws) {
  for (let i = fxList.length - 1; i >= 0; i--) {
    const f = fxList[i];
    f.t += dt;
    const k = f.t / f.dur;
    if (k >= 1) { scene.remove(f.m); f.m.material.dispose(); fxList.splice(i, 1); continue; }
    f.m.scale.setScalar(1 + k * f.max);
    f.m.material.opacity = 0.9 * (1 - k);
    f.m.position.z += ws * dt;
  }
}

/* =========================================================
   VAISSEAU
   ========================================================= */
function buildShip() {
  const group = new THREE.Group();
  const hull = new THREE.MeshStandardMaterial({ color: 0x1c1a46, metalness: 0.85, roughness: 0.28, emissive: 0x0a0630 });
  const accent = new THREE.MeshStandardMaterial({ color: 0x3a0a58, metalness: 0.7, roughness: 0.35, emissive: 0x2a0630 });
  const glass = new THREE.MeshStandardMaterial({ color: 0x00e5ff, emissive: 0x0098c0, emissiveIntensity: 1.1, metalness: 0.1, roughness: 0.05, transparent: true, opacity: 0.88 });
  const cyanLine = new THREE.LineBasicMaterial({ color: 0x00f0ff });
  const pinkLine = new THREE.LineBasicMaterial({ color: 0xff2bd6 });

  // Fuselage (cône aplati, pointe vers -Z)
  const noseGeo = new THREE.ConeGeometry(0.58, 3.2, 5);
  const nose = new THREE.Mesh(noseGeo, hull);
  nose.rotation.x = -Math.PI / 2;
  nose.scale.set(1.1, 1, 0.55);
  nose.position.set(0, 0, -0.1);
  nose.add(new THREE.LineSegments(new THREE.EdgesGeometry(noseGeo), cyanLine));
  group.add(nose);

  // Bloc arrière
  box(group, 1.1, 0.42, 1.1, 0, 0, 1.2, hull, pinkLine);
  // Cockpit
  const cockpit = new THREE.Mesh(new THREE.SphereGeometry(0.4, 20, 14), glass);
  cockpit.scale.set(0.75, 0.5, 1.7);
  cockpit.position.set(0, 0.2, -0.05);
  group.add(cockpit);

  // Ailes (extrusion d'un profil en flèche)
  const ws = new THREE.Shape();
  ws.moveTo(0.35, 0.5); ws.lineTo(2.3, -0.95); ws.lineTo(2.3, -1.45); ws.lineTo(0.35, -1.15); ws.closePath();
  const wingGeo = new THREE.ExtrudeGeometry(ws, { depth: 0.08, bevelEnabled: false });
  wingGeo.rotateX(-Math.PI / 2);
  const wingEdges = new THREE.EdgesGeometry(wingGeo, 30);
  const wingMat = hull.clone(); wingMat.side = THREE.DoubleSide;
  for (const s of [-1, 1]) {
    const w = new THREE.Mesh(wingGeo, wingMat);
    w.scale.x = s;
    w.position.set(0, -0.06, 0.35);
    w.add(new THREE.LineSegments(wingEdges, pinkLine));
    group.add(w);
    // feu de bout d'aile
    box(group, 0.12, 0.12, 0.5, s * 2.25, 0, 1.25, MAT.glowCyan);
    // dérive
    const fin = box(group, 0.07, 0.7, 0.9, s * 0.7, 0.4, 1.15, accent, cyanLine);
    fin.rotation.z = -s * 0.3;
  }

  // Réacteurs
  const flames = [];
  const flameGeo = new THREE.ConeGeometry(0.24, 1.6, 12); flameGeo.translate(0, 0.8, 0);
  const flameInGeo = new THREE.ConeGeometry(0.12, 1.1, 10); flameInGeo.translate(0, 0.55, 0);
  const flameOuterMat = new THREE.MeshBasicMaterial({ color: 0xff4fe0, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false });
  const flameInnerMat = new THREE.MeshBasicMaterial({ color: 0xaef8ff, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending, depthWrite: false });
  for (const s of [-1, 1]) {
    const eng = new THREE.Mesh(new THREE.CylinderGeometry(0.28, 0.34, 0.9, 10), accent);
    eng.rotation.x = Math.PI / 2;
    eng.position.set(s * 0.78, 0, 1.55);
    group.add(eng);
    const disc = new THREE.Mesh(new THREE.CircleGeometry(0.27, 16), MAT.glowMagenta);
    disc.position.set(s * 0.78, 0, 2.02);
    group.add(disc);
    const fo = new THREE.Mesh(flameGeo, flameOuterMat);
    fo.rotation.x = Math.PI / 2; fo.position.set(s * 0.78, 0, 2.0);
    const fi = new THREE.Mesh(flameInGeo, flameInnerMat);
    fi.rotation.x = Math.PI / 2; fi.position.set(s * 0.78, 0, 2.0);
    group.add(fo, fi);
    flames.push(fo, fi);
  }

  const light = new THREE.PointLight(0xff3df0, 18, 14, 2);
  light.position.set(0, 0.2, 2.6);
  group.add(light);

  // Bulle de bouclier
  const shieldG = new THREE.Group();
  shieldG.add(new THREE.Mesh(new THREE.SphereGeometry(2.2, 24, 16), new THREE.MeshBasicMaterial({ color: 0x20c0ff, transparent: true, opacity: 0.1, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, depthWrite: false })));
  shieldG.add(new THREE.Mesh(new THREE.IcosahedronGeometry(2.25, 1), new THREE.MeshBasicMaterial({ color: hdr(0.2, 1.6, 2.4), wireframe: true, transparent: true, opacity: 0.4 })));
  shieldG.position.z = 0.4;
  shieldG.visible = false;
  group.add(shieldG);

  scene.add(group);

  // Halo au sol sous le vaisseau
  const glowMat = new THREE.MeshBasicMaterial({ map: glowTex, color: 0x00e5ff, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending, depthWrite: false });
  const groundGlow = new THREE.Mesh(new THREE.PlaneGeometry(4.5, 6), glowMat);
  groundGlow.rotation.x = -Math.PI / 2;
  groundGlow.position.y = 0.04;
  scene.add(groundGlow);

  return { group, flames, shieldG, light, groundGlow, glowMat };
}
const ship = buildShip();

/* =========================================================
   ÉTAT DU JEU
   ========================================================= */
const G = {
  mode: 'title', // title | playing | paused | over
  t: 0, elapsed: 0, distance: 0, diff: 0,
  speedBase: CFG.baseSpeed, boostMul: 1, ws: 26,
  score: 0, best: loadBest(), combo: 0, comboT: 0, shield: 0, boostT: 0, invuln: 0,
  x: 0, vx: 0, y: CFG.rest, vy: 0, grounded: true, jumpBuf: 0,
  shake: 0, spawnAcc: 0, nextSpacing: 40, safeX: 0, sinceSpecial: 0,
  goTimer: 0, overShown: false, rotZ: 0, newRecord: false, frames: 0,
};
const obstacles = [];
const collectibles = [];
const mult = () => 1 + Math.min(7, Math.floor(G.combo / 4));

/* =========================================================
   OBSTACLES
   ========================================================= */
function laserSegment(o, x0, x1, jumpOnly) {
  const w = x1 - x0;
  if (w < 0.6) return;
  const cx = (x0 + x1) / 2;
  const beam = jumpOnly ? MAT.laserOrange : MAT.laserRed;
  const panel = jumpOnly ? MAT.panelOrange : MAT.panelRed;
  const edge = jumpOnly ? MAT.edgeOrange : MAT.edgeRed;
  for (const px of [x0, x1]) {
    box(o.group, 0.45, 1.9, 0.45, px, 0.95, 0, MAT.dark, edge);
    box(o.group, 0.6, 0.18, 0.6, px, 1.98, 0, beam);
  }
  for (const y of [0.35, 0.8, 1.25]) box(o.group, w, 0.07, 0.07, cx, y, 0, beam);
  const p = new THREE.Mesh(GEO.plane, panel);
  p.scale.set(w, 1.6, 1); p.position.set(cx, 0.8, 0);
  o.group.add(p);
  o.colliders.push({ t: 0, x: cx, y: 0.8, hw: w / 2 + 0.22, hh: 0.8, hd: 0.3 });
}

function gantrySegment(o, x0, x1) {
  const w = x1 - x0;
  if (w < 0.6) return;
  const cx = (x0 + x1) / 2;
  box(o.group, w, 4.5, 1.0, cx, 2.25, 0, MAT.dark, MAT.edgeMagenta);
  box(o.group, w, 0.1, 1.04, cx, 1.0, 0, MAT.glowMagenta);
  box(o.group, w, 0.1, 1.04, cx, 3.7, 0, MAT.glowCyan);
  o.colliders.push({ t: 0, x: cx, y: 2.25, hw: w / 2, hh: 2.25, hd: 0.5 });
}

function makeLaserWall(z, gapC, gapW, jumpOnly) {
  const o = { type: 'laser', group: new THREE.Group(), z, colliders: [] };
  const L = -CFG.roadHalf - 0.5, R = CFG.roadHalf + 0.5;
  if (jumpOnly) laserSegment(o, L, R, true);
  else { laserSegment(o, L, gapC - gapW / 2, false); laserSegment(o, gapC + gapW / 2, R, false); }
  return o;
}

function makeGantry(z, gapC, gapW) {
  const o = { type: 'gantry', group: new THREE.Group(), z, colliders: [] };
  const L = -CFG.roadHalf - 0.5, R = CFG.roadHalf + 0.5;
  gantrySegment(o, L, gapC - gapW / 2);
  gantrySegment(o, gapC + gapW / 2, R);
  box(o.group, gapW + 0.4, 0.6, 0.9, gapC, 3.2, 0, MAT.dark, MAT.edgeCyan);
  box(o.group, gapW, 0.08, 0.5, gapC, 2.88, 0, MAT.glowCyan);
  o.colliders.push({ t: 0, x: gapC, y: 3.2, hw: (gapW + 0.4) / 2, hh: 0.3, hd: 0.45 });
  return o;
}

function makePillar(z, x, move) {
  const o = { type: 'pillar', group: new THREE.Group(), z, colliders: [], move };
  const H = 6.5;
  const edge = move ? MAT.edgeCyan : MAT.edgeMagenta;
  const glow = move ? MAT.glowCyan : MAT.glowMagenta;
  const body = new THREE.Mesh(GEO.pillar, MAT.dark);
  body.scale.set(1, H, 1); body.position.y = H / 2;
  o.group.add(body);
  for (const y of [0.8, 2.2, 3.6, 5.0]) {
    const b = new THREE.Mesh(GEO.band, glow);
    b.rotation.x = Math.PI / 2; b.position.y = y;
    o.group.add(b);
  }
  box(o.group, 1.3, 0.14, 1.3, 0, H + 0.05, 0, glow);
  o.group.position.x = x;
  o.colliders.push({ t: 1, x, r: 0.92, h: H });
  return o;
}

function addObstacle(o) {
  o.group.position.z = o.z;
  scene.add(o.group);
  obstacles.push(o);
}

/* =========================================================
   COLLECTIBLES
   ========================================================= */
function addCollectible(kind, x, y, z) {
  const g = new THREE.Group();
  let radius = 1.5;
  if (kind === 'orb') {
    g.add(new THREE.Mesh(GEO.orbCore, MAT.orbCore));
    g.add(new THREE.Mesh(GEO.orbShell, MAT.orbShell));
  } else if (kind === 'shield') {
    g.add(new THREE.Mesh(GEO.shieldIco, MAT.shieldWire));
    g.add(new THREE.Mesh(GEO.shieldSph, MAT.shieldSph));
    radius = 1.9;
  } else {
    g.add(new THREE.Mesh(GEO.boostRing, MAT.boostRing));
    g.add(new THREE.Mesh(GEO.boostRing2, MAT.boostRing2));
    radius = 2.3;
  }
  g.position.set(x, y, z);
  scene.add(g);
  collectibles.push({ kind, group: g, x, y, z, radius, phase: Math.random() * 6 });
}

function clearEntities() {
  for (const o of obstacles) scene.remove(o.group);
  for (const c of collectibles) scene.remove(c.group);
  obstacles.length = 0; collectibles.length = 0;
  for (const f of fxList) { scene.remove(f.m); f.m.material.dispose(); }
  fxList.length = 0;
  trail.clear(); debris.clear();
}

/* =========================================================
   GÉNÉRATION (difficulté progressive, toujours franchissable)
   ========================================================= */
const spacingFor = (speed, diff) => Math.max(32, speed * lerp(1.25, 0.62, diff));

function pickType(diff) {
  const r = Math.random();
  if (diff < 0.1) return r < 0.55 ? 'laser' : 'pillar';
  return r < 0.38 ? 'pillar' : r < 0.72 ? 'laser' : 'gantry';
}

function spawnSegment(z, spacing, spacingTime) {
  const diff = G.diff;
  const spd = G.ws || CFG.baseSpeed;
  // chemin sûr atteignable depuis l'obstacle précédent
  const reach = clamp(18 * spacingTime * 0.55, 3, 11);
  const prev = G.safeX;
  let safe = clamp(prev + rand(-reach, reach), -6, 6);
  const type = pickType(diff);
  let jumpOnly = false;

  if (type === 'laser') {
    jumpOnly = Math.random() < 0.12 + 0.2 * diff;
    if (jumpOnly) safe = prev;
    addObstacle(makeLaserWall(z, safe, lerp(7.2, 4.6, diff), jumpOnly));
  } else if (type === 'gantry') {
    addObstacle(makeGantry(z, safe, lerp(7.5, 5.8, diff)));
  } else {
    const n = 2 + Math.floor(diff * 3) + (Math.random() < 0.4 ? 1 : 0);
    for (let i = 0; i < n; i++) {
      let x = rand(-8.4, 8.4), tries = 0;
      while (Math.abs(x - safe) < 2.7 && tries++ < 10) x = rand(-8.4, 8.4);
      if (Math.abs(x - safe) < 2.7) continue;
      let move = null;
      if (Math.random() < 0.25 + 0.5 * diff) {
        const side = x < safe ? -1 : 1;
        move = { base: x, amp: rand(1.2, 2.8), f: rand(1.2, 2.4), ph: rand(0, 6.28), lo: side < 0 ? -8.6 : safe + 2.7, hi: side < 0 ? safe - 2.7 : 8.6 };
      }
      addObstacle(makePillar(z + rand(-3, 3), x, move));
    }
  }

  // Collectibles entre l'obstacle précédent et celui-ci, sur le chemin sûr
  const pathPoint = (s) => ({ x: lerp(prev, safe, smooth(s)), z: z + spacing * lerp(0.78, 0.28, s) });
  G.sinceSpecial++;
  let special = null;
  if (G.sinceSpecial >= 4 && Math.random() < 0.28) {
    special = (G.shield < 2 && Math.random() < 0.55) ? 'shield' : 'ring';
    G.sinceSpecial = 0;
  }
  if (Math.random() < 0.8) {
    const n = 3 + ((Math.random() * 3) | 0);
    for (let k = 0; k < n; k++) {
      const s = k / (n - 1);
      if (special && Math.abs(s - 0.5) < 0.2) continue;
      const p = pathPoint(s);
      addCollectible('orb', p.x, 1.0, p.z);
    }
  }
  if (special) {
    const p = pathPoint(0.5);
    addCollectible(special, p.x, special === 'ring' ? 1.3 : 1.1, p.z);
  }
  // Arc d'orbes au-dessus des murs à sauter
  if (jumpOnly && Math.random() < 0.85) {
    const len = Math.min(spd * 0.6, spacing * 0.5);
    for (let k = 0; k < 5; k++) {
      const s = k / 4;
      addCollectible('orb', safe, CFG.rest + 0.3 + 2.0 * Math.sin(Math.PI * s), z + len * (s - 0.5));
    }
  }
  G.safeX = safe;
}

/* =========================================================
   ENTRÉES
   ========================================================= */
const input = {
  left: false, right: false, mode: 'keys',
  mouseNX: 0, lastMX: 0,
  touchActive: false, touchId: null, touchStartX: 0, touchX: 0, touchStartShip: 0, touchT: 0, touchMoved: false,
};

function queueJump() { if (G.mode === 'playing') G.jumpBuf = 0.14; }

function onFirstGesture() {
  if (audio.unlock() && !audio.musicRunning) audio.startMusic();
}

window.addEventListener('keydown', (e) => {
  onFirstGesture();
  const k = e.key.toLowerCase();
  if (['arrowleft', 'arrowright', 'arrowup', 'arrowdown', ' '].includes(k)) e.preventDefault();
  if (e.repeat && k !== 'arrowleft' && k !== 'arrowright') return;

  if (k === 'arrowleft' || k === 'a' || k === 'q') { input.left = true; input.mode = 'keys'; }
  else if (k === 'arrowright' || k === 'd') { input.right = true; input.mode = 'keys'; }
  else if (k === ' ' || k === 'spacebar' || k === 'arrowup' || k === 'w' || k === 'z') {
    if (G.mode === 'title') startGame();
    else if (G.mode === 'over' && G.overShown && G.goTimer < -0.5) startGame();
    else queueJump();
  } else if (k === 'enter') {
    if (G.mode === 'title') startGame();
    else if (G.mode === 'over' && G.overShown) startGame();
    else if (G.mode === 'paused') resumeGame();
  } else if (k === 'p' || k === 'escape') {
    if (G.mode === 'playing') pauseGame();
    else if (G.mode === 'paused') resumeGame();
  } else if (k === 'm') {
    toggleMute();
  }
});
window.addEventListener('keyup', (e) => {
  const k = e.key.toLowerCase();
  if (k === 'arrowleft' || k === 'a' || k === 'q') input.left = false;
  else if (k === 'arrowright' || k === 'd') input.right = false;
});
window.addEventListener('blur', () => { input.left = input.right = false; if (G.mode === 'playing') pauseGame(); });
document.addEventListener('visibilitychange', () => { if (document.hidden && G.mode === 'playing') pauseGame(); });

canvas.addEventListener('contextmenu', (e) => e.preventDefault());
canvas.addEventListener('pointerdown', (e) => {
  onFirstGesture();
  if (e.pointerType === 'mouse') {
    if (G.mode === 'playing') queueJump();
    return;
  }
  if (G.mode !== 'playing' || input.touchActive) return;
  input.touchActive = true; input.touchId = e.pointerId;
  input.touchStartX = input.touchX = e.clientX;
  input.touchStartShip = G.x; input.touchT = performance.now(); input.touchMoved = false;
  input.mode = 'touch';
  try { canvas.setPointerCapture(e.pointerId); } catch { /* ignore */ }
});
canvas.addEventListener('pointermove', (e) => {
  if (e.pointerType === 'mouse') {
    if (Math.abs(e.clientX - input.lastMX) > 3) { input.mode = 'mouse'; }
    input.lastMX = e.clientX;
    input.mouseNX = (e.clientX / window.innerWidth) * 2 - 1;
  } else if (input.touchActive && e.pointerId === input.touchId) {
    input.touchX = e.clientX;
    if (Math.abs(e.clientX - input.touchStartX) > 12) input.touchMoved = true;
  }
});
const endTouch = (e) => {
  if (!input.touchActive || e.pointerId !== input.touchId) return;
  input.touchActive = false;
  if (!input.touchMoved && performance.now() - input.touchT < 220) queueJump(); // tap = saut
  if (input.mode === 'touch') input.mode = 'keys';
};
canvas.addEventListener('pointerup', endTouch);
canvas.addEventListener('pointercancel', endTouch);

$('btn-jump').addEventListener('pointerdown', (e) => { e.preventDefault(); e.stopPropagation(); onFirstGesture(); queueJump(); });

/* ---------- Boutons UI ---------- */
function toggleMute() {
  audio.unlock();
  const m = audio.toggleMute();
  const b = $('btn-mute');
  b.textContent = m ? '🔇' : '🔊';
  b.setAttribute('aria-label', m ? 'Activer le son' : 'Couper le son');
  if (!m) { if (!audio.musicRunning) audio.startMusic(); audio.click(); }
}
$('btn-mute').textContent = audio.muted ? '🔇' : '🔊';
$('btn-mute').addEventListener('click', (e) => { e.currentTarget.blur(); toggleMute(); });
$('btn-pause').addEventListener('click', (e) => { e.currentTarget.blur(); if (G.mode === 'playing') pauseGame(); else if (G.mode === 'paused') resumeGame(); });
$('btn-play').addEventListener('click', () => { onFirstGesture(); startGame(); });
$('btn-resume').addEventListener('click', () => resumeGame());
$('btn-restart').addEventListener('click', () => startGame());
$('btn-menu-pause').addEventListener('click', () => toMenu());
$('btn-retry').addEventListener('click', () => startGame());
$('btn-menu-over').addEventListener('click', () => toMenu());
document.querySelectorAll('.btn').forEach((b) => b.addEventListener('pointerdown', () => audio.click()));

/* =========================================================
   FLUX DU JEU
   ========================================================= */
const screens = { title: $('screen-title'), pause: $('screen-pause'), over: $('screen-over') };
function showScreen(name) {
  for (const [k, el] of Object.entries(screens)) el.classList.toggle('show', k === name);
  document.body.classList.toggle('menu', G.mode === 'title');
}
function setHud(visible) {
  $('hud').classList.toggle('hidden', !visible);
  $('btn-pause').classList.toggle('hidden', !visible);
}

function resetRun() {
  clearEntities();
  Object.assign(G, {
    elapsed: 0, distance: 0, diff: 0, speedBase: CFG.baseSpeed, boostMul: 1, ws: CFG.baseSpeed,
    score: 0, combo: 0, comboT: 0, shield: 0, boostT: 0, invuln: 0,
    x: 0, vx: 0, y: CFG.rest, vy: 0, grounded: true, jumpBuf: 0, shake: 0,
    safeX: 0, sinceSpecial: 0, goTimer: 0, overShown: false, newRecord: false, rotZ: 0,
  });
  ship.group.visible = true;
  const sp = spacingFor(CFG.baseSpeed, 0);
  G.nextSpacing = sp;
  let last = -60;
  for (let z = -60; z > CFG.spawnZ; z -= sp) { spawnSegment(z, sp, sp / CFG.baseSpeed); last = z; }
  G.spawnAcc = last - CFG.spawnZ;
  $('boost-vignette').classList.remove('on');
}

function startGame() {
  audio.unlock();
  resetRun();
  G.mode = 'playing';
  showScreen(null);
  setHud(true);
  if (!audio.musicRunning) audio.startMusic();
  audio.resume();
  audio.stopEngine(); audio.startEngine();
  audio.start();
  $('hud-best').textContent = G.best.toLocaleString('fr-FR');
}

function pauseGame() {
  if (G.mode !== 'playing') return;
  G.mode = 'paused';
  showScreen('pause');
  audio.suspend();
}
function resumeGame() {
  if (G.mode !== 'paused') return;
  G.mode = 'playing';
  showScreen(null);
  audio.resume();
  last = performance.now();
}
function toMenu() {
  audio.stopEngine();
  clearEntities();
  Object.assign(G, { mode: 'title', ws: 26, x: 0, vx: 0, y: CFG.rest, vy: 0, shake: 0, boostMul: 1 });
  ship.group.visible = true;
  ship.shieldG.visible = false;
  setHud(false);
  showScreen('title');
  audio.resume();
  audio.setIntensity(0);
  $('title-best').textContent = G.best.toLocaleString('fr-FR');
  $('boost-vignette').classList.remove('on');
}

function gameOver() {
  G.mode = 'over';
  G.goTimer = 1.0;
  G.overShown = false;
  audio.stopEngine();
  audio.explosion();
  audio.gameOver();
  ship.group.visible = false;
  ship.shieldG.visible = false;
  burst(G.x, G.y, 0, 160, PAL.ship, 16, 1.6);
  burst(G.x, G.y, 0, 60, PAL.fire, 12, 1.2);
  shockwave(G.x, G.y, 0, 0x00f0ff, 16);
  shockwave(G.x, G.y, 0, 0xff2bd6, 10);
  G.shake = 1.6;
  flash('rgba(255,40,80,0.6)');
  $('boost-vignette').classList.remove('on');
  const final = Math.floor(G.score);
  if (final > G.best) { G.best = final; G.newRecord = true; saveBest(final); }
}
function showGameOver() {
  G.overShown = true;
  $('final-score').textContent = Math.floor(G.score).toLocaleString('fr-FR');
  $('final-best').textContent = G.best.toLocaleString('fr-FR');
  $('final-dist').textContent = Math.floor(G.distance).toLocaleString('fr-FR') + ' m';
  $('record-badge').classList.toggle('hidden', !G.newRecord);
  if (G.newRecord) audio.record();
  setHud(false);
  showScreen('over');
}

let flashTimer = 0;
function flash(color) {
  const el = $('flash');
  el.style.setProperty('--flash', color);
  el.classList.add('on');
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => el.classList.remove('on'), 60);
}

/* =========================================================
   MISE À JOUR
   ========================================================= */
function addScore(n) { G.score += n; }

function collect(c) {
  const wx = c.x, wy = c.y, wz = c.z;
  if (c.kind === 'orb') {
    G.combo++; G.comboT = CFG.comboWindow;
    addScore(100 * mult());
    audio.collect(Math.min(G.combo, 9));
    burst(wx, wy, wz, 14, PAL.gold, 5, 0.6);
    const p = $('combo-panel'); p.classList.add('bump'); setTimeout(() => p.classList.remove('bump'), 120);
  } else if (c.kind === 'shield') {
    G.shield = Math.min(2, G.shield + 1);
    addScore(150);
    audio.shield();
    burst(wx, wy, wz, 40, PAL.cyan, 8, 0.9);
    shockwave(wx, wy, wz, 0x20c0ff, 8);
    flash('rgba(0,200,255,0.35)');
  } else {
    G.boostT = CFG.boostTime;
    addScore(250 * mult());
    audio.boost();
    burst(wx, wy, wz, 60, PAL.pink, 12, 1.0);
    shockwave(wx, wy, wz, 0xff2bd6, 12);
    G.shake = Math.max(G.shake, 0.5);
    flash('rgba(255,43,214,0.4)');
  }
}

function destroyObstacle(i, big) {
  const o = obstacles[i];
  const cx = o.group.position.x + (o.type === 'pillar' ? 0 : clamp(G.x, -8, 8));
  burst(cx, 1.2, o.z, big ? 70 : 40, PAL.fire, 11, 0.9);
  scene.remove(o.group);
  obstacles.splice(i, 1);
}

function hitObstacle(i) {
  if (G.boostT > 0) {
    addScore(50 * mult());
    audio.smash();
    G.shake = Math.max(G.shake, 0.45);
    destroyObstacle(i, false);
    return;
  }
  if (G.invuln > 0) return;
  if (G.shield > 0) {
    G.shield--;
    G.invuln = 1.2;
    audio.shieldBreak();
    G.shake = Math.max(G.shake, 0.8);
    shockwave(G.x, G.y, 0, 0x20c0ff, 10);
    flash('rgba(0,200,255,0.45)');
    destroyObstacle(i, true);
    return;
  }
  gameOver();
}

function collides(o, step) {
  const sx = G.x, sy = G.y;
  for (const c of o.colliders) {
    if (c.t === 0) {
      if (o.z + c.hd + CFG.shipHD < 0 || o.z - step - c.hd - CFG.shipHD > 0) continue;
      if (Math.abs(sx - c.x) < c.hw + CFG.shipHW && Math.abs(sy - c.y) < c.hh + CFG.shipHH) return true;
    } else {
      if (sy - CFG.shipHH > c.h || sy + CFG.shipHH < 0) continue;
      const dx = Math.max(Math.abs(sx - c.x) - CFG.shipHW, 0);
      // balayage en Z : distance minimale entre la trajectoire du pilier et le vaisseau
      const zmin = o.z - step, zmax = o.z;
      let dz = 0;
      if (zmin > CFG.shipHD) dz = zmin - CFG.shipHD;
      else if (zmax < -CFG.shipHD) dz = -CFG.shipHD - zmax;
      if (dx * dx + dz * dz < c.r * c.r) return true;
    }
  }
  return false;
}

function updatePlaying(dt) {
  G.elapsed += dt;
  G.diff = clamp(G.elapsed / CFG.rampTime, 0, 1);
  G.speedBase = CFG.baseSpeed + (CFG.maxSpeed - CFG.baseSpeed) * Math.pow(G.diff, 0.85);

  const wasBoost = G.boostT > 0;
  if (wasBoost) G.boostT -= dt;
  if (wasBoost && G.boostT <= 0) G.invuln = Math.max(G.invuln, 0.8);
  G.boostMul = damp(G.boostMul, G.boostT > 0 ? CFG.boostMult : 1, 4, dt);
  G.ws = G.speedBase * G.boostMul;
  G.invuln = Math.max(0, G.invuln - dt);
  if (G.combo > 0) { G.comboT -= dt; if (G.comboT <= 0) G.combo = 0; }

  // --- Direction ---
  const lim = CFG.limitX;
  const maxV = 19 + G.diff * 6;
  let desired = 0;
  if (input.mode === 'touch' && input.touchActive) {
    const target = clamp(input.touchStartShip + ((input.touchX - input.touchStartX) / window.innerWidth) * lim * 2.8, -lim, lim);
    desired = clamp((target - G.x) * 9, -maxV * 1.3, maxV * 1.3);
  } else if (input.mode === 'mouse') {
    const target = clamp(input.mouseNX * lim * 1.1, -lim, lim);
    desired = clamp((target - G.x) * 9, -maxV * 1.3, maxV * 1.3);
  } else {
    desired = ((input.right ? 1 : 0) - (input.left ? 1 : 0)) * maxV;
  }
  G.vx = damp(G.vx, desired, 11, dt);
  G.x += G.vx * dt;
  if (Math.abs(G.x) > lim) { G.x = clamp(G.x, -lim, lim); G.vx = 0; }

  // --- Saut ---
  G.jumpBuf = Math.max(0, G.jumpBuf - dt);
  if (G.jumpBuf > 0 && G.grounded) {
    G.vy = CFG.jumpV; G.grounded = false; G.jumpBuf = 0;
    audio.jump();
    burst(G.x, 0.2, 0.5, 10, PAL.cyan, 4, 0.4);
  }
  if (!G.grounded) {
    G.vy -= CFG.gravity * dt;
    G.y += G.vy * dt;
    if (G.y <= CFG.rest) {
      if (G.vy < -6) { audio.land(); burst(G.x, 0.2, 0.5, 12, PAL.pink, 5, 0.4); G.shake = Math.max(G.shake, 0.12); }
      G.y = CFG.rest; G.vy = 0; G.grounded = true;
    }
  }

  // --- Score ---
  G.distance += G.ws * dt;
  addScore(G.ws * dt * 0.22 * mult());

  // --- Génération ---
  G.spawnAcc += G.ws * dt;
  let guard = 0;
  while (G.spawnAcc >= G.nextSpacing && guard++ < 4) {
    const sp = G.nextSpacing;
    G.spawnAcc -= sp;
    spawnSegment(CFG.spawnZ + G.spawnAcc, sp, sp / G.ws);
    G.nextSpacing = spacingFor(G.speedBase, G.diff);
  }

  // --- Collectibles ---
  const step = G.ws * dt;
  for (let i = collectibles.length - 1; i >= 0; i--) {
    const c = collectibles[i];
    c.z += step;
    c.group.position.z = c.z;
    c.phase += dt;
    c.group.rotation.y += dt * (c.kind === 'ring' ? 0 : 2.5);
    if (c.kind === 'ring') c.group.scale.setScalar(1 + Math.sin(c.phase * 6) * 0.05);
    else c.group.position.y = c.y + Math.sin(c.phase * 3) * 0.12;
    if (c.z > CFG.killZ) { scene.remove(c.group); collectibles.splice(i, 1); continue; }
    // test avec balayage en Z
    const zmin = c.z - step, zmax = c.z;
    const dz = (zmin <= 0 && zmax >= 0) ? 0 : Math.min(Math.abs(zmin), Math.abs(zmax));
    const dx = c.x - G.x, dy = c.y - G.y;
    let hit;
    if (c.kind === 'ring') hit = dz < 1.6 && dx * dx + dy * dy < c.radius * c.radius;
    else hit = dx * dx + dy * dy + dz * dz < c.radius * c.radius;
    if (hit) { collect(c); scene.remove(c.group); collectibles.splice(i, 1); }
  }

  // --- Obstacles ---
  for (let i = obstacles.length - 1; i >= 0; i--) {
    const o = obstacles[i];
    o.z += step;
    o.group.position.z = o.z;
    if (o.move) {
      const m = o.move;
      const x = clamp(m.base + Math.sin(m.ph + G.t * m.f) * m.amp, m.lo, m.hi);
      o.group.position.x = x;
      o.colliders[0].x = x;
    }
    if (o.z > CFG.killZ) { scene.remove(o.group); obstacles.splice(i, 1); continue; }
    if (o.z > -12 && o.z < 12 && collides(o, step)) {
      hitObstacle(i);
      if (G.mode === 'over') return;
    }
  }

  // --- Traînée du vaisseau ---
  const boosting = G.boostT > 0;
  const n = boosting ? 3 : 2;
  for (const s of [-0.78, 0.78]) {
    for (let k = 0; k < n; k++) {
      const col = boosting ? [0.6, 1.8, 2.6] : [2.4, 0.4, 2.0];
      trail.emit(
        G.x + s + rand(-0.08, 0.08), G.y + rand(-0.08, 0.08), 2.1 + rand(0, 0.4),
 rand(-0.4, 0.4) - G.vx * 0.15, rand(-0.2, 0.3), rand(-1, 1),
 col[0], col[1], col[2], boosting ? 0.7 : 0.5
      );
    }
  }

  // --- Audio dynamique ---
  const norm = clamp((G.ws - CFG.baseSpeed) / (CFG.maxSpeed * 1.3 - CFG.baseSpeed), 0, 1);
  audio.updateEngine(norm, boosting);
  audio.setIntensity(clamp(G.diff * 0.85 + (boosting ? 0.15 : 0), 0, 1));
}

function updateShip(dt) {
  const g = ship.group;
  const bob = G.mode === 'over' ? 0 : Math.sin(G.t * 4) * 0.04;
  g.position.set(G.x, G.y + bob, 0);
  G.rotZ = damp(G.rotZ, -G.vx * 0.045, 10, dt);
  g.rotation.z = G.rotZ;
  g.rotation.y = -G.vx * 0.012;
  g.rotation.x = clamp(G.vy * 0.025, -0.4, 0.4);

  const boosting = G.boostT > 0 && G.mode === 'playing';
  const flick = (boosting ? 1.9 : 1) * (0.85 + Math.random() * 0.35);
  for (const f of ship.flames) f.scale.set(1, flick, 1);
  ship.light.intensity = (boosting ? 40 : 18) * (0.85 + Math.random() * 0.3);

  ship.shieldG.visible = G.shield > 0 && G.mode !== 'over';
  if (ship.shieldG.visible) {
    const s = 1 + Math.sin(G.t * 5) * 0.03;
    ship.shieldG.scale.setScalar(s);
    ship.shieldG.rotation.y += dt * 0.8;
  }
  // clignotement d'invulnérabilité
  if (G.mode === 'playing') g.visible = !(G.invuln > 0 && !boosting && Math.floor(G.t * 18) % 2 === 0);

  const h = G.y - CFG.rest;
  ship.groundGlow.position.x = G.x;
  ship.groundGlow.scale.setScalar(1 / (1 + h * 0.35));
  ship.glowMat.opacity = 0.7 / (1 + h * 0.5);
  ship.glowMat.color.set(boosting ? 0xff2bd6 : 0x00e5ff);
  ship.groundGlow.visible = G.mode !== 'over';
}

let scrollAcc = 0; // défilement de la grille (cellule = 4 unités)

function updateWorld(dt, ws) {
  groundMat.uniforms.uScroll.value = scrollAcc;
  for (const m of mountains) {
    m.position.z += ws * dt;
    if (m.position.z > 50) m.position.z -= MOUNT_LEN;
  }
  for (const p of pylons) {
    p.position.z += ws * dt;
    if (p.position.z > 24) p.position.z -= PYLON_LEN;
  }
}

function updateCamera(dt) {
  const speedN = clamp((G.ws - CFG.baseSpeed) / (CFG.maxSpeed * 1.3 - CFG.baseSpeed), 0, 1.3);
  const fov = 66 + speedN * 16;
  camera.fov = damp(camera.fov, fov, 4, dt);
  camera.updateProjectionMatrix();

  let tx, ty, tz, lx, ly, lz;
  if (G.mode === 'title') {
    tx = Math.sin(G.t * 0.25) * 3.5; ty = 2.7 + Math.sin(G.t * 0.4) * 0.3; tz = 7.6;
    lx = 0; ly = 1.2; lz = -6;
  } else {
    tx = G.x * 0.55; ty = 3.5 + (G.y - CFG.rest) * 0.35; tz = 8.4 + (G.mode === 'over' ? 2 : 0);
    lx = G.x * 0.35; ly = 1.3 + (G.y - CFG.rest) * 0.3; lz = -14;
  }
  camera.position.x = damp(camera.position.x, tx, 6, dt);
  camera.position.y = damp(camera.position.y, ty, 6, dt);
  camera.position.z = damp(camera.position.z, tz, 3, dt);

  if (G.shake > 0.001) {
    const s = G.shake * 0.35;
    camera.position.x += rand(-s, s); camera.position.y += rand(-s, s) * 0.7;
    G.shake = Math.max(0, G.shake - dt * 2.6);
  }
  camera.lookAt(lx, ly, lz);
  if (G.mode !== 'title') camera.rotateZ(-G.vx * 0.0035);
  if (G.shake > 0.001) camera.rotateZ(rand(-1, 1) * G.shake * 0.02);
}

/* ---------- HUD ---------- */
const hudCache = {};
function setText(id, v) {
  if (hudCache[id] === v) return;
  hudCache[id] = v;
  $(id).textContent = v;
}
function updateHud() {
  const sc = Math.floor(G.score);
  setText('hud-score', sc.toLocaleString('fr-FR'));
  setText('hud-best', Math.max(G.best, sc).toLocaleString('fr-FR'));
  setText('hud-speed', String(Math.round(G.ws * 4.5)));
  setText('hud-combo', 'x' + mult() + (G.combo > 0 ? ' · ' + G.combo : ''));
  $('combo-bar').style.transform = `scaleX(${G.combo > 0 ? clamp(G.comboT / CFG.comboWindow, 0, 1) : 0})`;
  $('combo-panel').classList.toggle('idle', G.combo === 0);
  $('pip-1').classList.toggle('on', G.shield >= 1);
  $('pip-2').classList.toggle('on', G.shield >= 2);
  const b = G.boostT > 0;
  $('boost-wrap').classList.toggle('on', b);
  $('boost-vignette').classList.toggle('on', b);
  if (b) $('boost-bar').style.transform = `scaleX(${clamp(G.boostT / CFG.boostTime, 0, 1)})`;
}

/* =========================================================
   BOUCLE PRINCIPALE
   ========================================================= */
let last = performance.now();
let slowFrames = 0;

function applySize() {
  const w = window.innerWidth, h = window.innerHeight;
  renderer.setPixelRatio(pixelRatio);
  renderer.setSize(w, h);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  if (composer) { composer.setPixelRatio(pixelRatio); composer.setSize(w, h); }
}
window.addEventListener('resize', applySize);

function perfGuard(dt) {
  if (G.frames < 180) return;
  if (dt > 1 / 36) slowFrames++; else slowFrames = Math.max(0, slowFrames - 1);
  if (slowFrames > 90) {
    slowFrames = 0;
    if (pixelRatio > 1.01) { pixelRatio = 1; applySize(); }
    else if (composer) { composer = null; applySize(); }
  }
}

function frame(now) {
  requestAnimationFrame(frame);
  const dt = Math.min(0.05, (now - last) / 1000);
  last = now;
  G.frames++;

  if (G.mode !== 'paused') {
    G.t += dt;
    let ws;
    if (G.mode === 'playing') { updatePlaying(dt); ws = G.ws; }
    else if (G.mode === 'over') {
      G.ws = damp(G.ws, 5, 2.5, dt); ws = G.ws;
      for (const o of obstacles) { o.z += ws * dt; o.group.position.z = o.z; }
      for (const c of collectibles) { c.z += ws * dt; c.group.position.z = c.z; }
      G.goTimer -= dt;
      if (G.goTimer <= 0 && !G.overShown) showGameOver();
    } else { // title : défilement d'ambiance
      G.ws = damp(G.ws, 26, 2, dt); ws = G.ws;
      G.x = Math.sin(G.t * 0.7) * 3.2; G.vx = Math.cos(G.t * 0.7) * 2.2;
    }

    if (G.mode !== 'playing') G.invuln = 0;
    scrollAcc = (scrollAcc + ws * dt) % 4;
    updateWorld(dt, ws);
    updateShip(dt);
    trail.update(dt, ws);
    debris.update(dt, ws);
    updateFx(dt, ws);
    updateCamera(dt);
    if (G.mode === 'playing') updateHud();

    const beat = audio.beat();
    groundMat.uniforms.uTime.value = G.t;
    groundMat.uniforms.uBeat.value = beat;
    sunMat.uniforms.uTime.value = G.t;
    sunMat.uniforms.uBeat.value = beat;
    stars.material.opacity = 0.75 + Math.sin(G.t * 1.7) * 0.15;
  }
  sky.position.copy(camera.position);
  perfGuard(dt);

  if (composer) composer.render();
  else renderer.render(scene, camera);

  if (G.frames === 2) $('loading').classList.add('done');
}

/* ---------- Démarrage ---------- */
$('hud-best').textContent = G.best.toLocaleString('fr-FR');
$('title-best').textContent = G.best.toLocaleString('fr-FR');
document.body.classList.add('menu');
setupPostFX().finally(() => requestAnimationFrame(frame));
