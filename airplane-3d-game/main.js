import * as THREE from 'three';

const canvas = document.querySelector('#scene');
const renderer = new THREE.WebGLRenderer({ canvas, antialias:true, powerPreference:'high-performance' });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2)); renderer.setSize(innerWidth, innerHeight); renderer.shadowMap.enabled = true; renderer.outputColorSpace = THREE.SRGBColorSpace; renderer.toneMapping = THREE.ACESFilmicToneMapping; renderer.toneMappingExposure = 1.15;
const scene = new THREE.Scene(); scene.fog = new THREE.FogExp2(0x071528, 0.00085); scene.background = new THREE.Color(0x071528);
const camera = new THREE.PerspectiveCamera(67, innerWidth/innerHeight, .1, 3000); camera.position.set(0, 2.4, 10);
const clock = new THREE.Clock();

const hemi = new THREE.HemisphereLight(0x9bdcff, 0x071221, 2.2); scene.add(hemi);
const sun = new THREE.DirectionalLight(0xffd7a6, 3.5); sun.position.set(-90,120,80); sun.castShadow=true; scene.add(sun);
const cyanLight = new THREE.PointLight(0x4ddfff, 40, 80); scene.add(cyanLight);

const flight = new THREE.Group(); scene.add(flight);
const plane = new THREE.Group(); flight.add(plane);
const mat = (color, metal=0, rough=.4, emissive=0x000000) => new THREE.MeshStandardMaterial({color, metalness:metal, roughness:rough, emissive, emissiveIntensity:emissive ? 2 : 0});
const fuselageMat=mat(0xb8d3df,.75,.22), darkMat=mat(0x162b3d,.7,.28), orangeMat=mat(0xff8e42,.35,.3,0x7b1c05), glassMat=new THREE.MeshPhysicalMaterial({color:0x0e334c, metalness:.45, roughness:.08, transmission:.15, transparent:true, opacity:.9});
function mesh(g,m,pos=[0,0,0],rot=[0,0,0]) { const o=new THREE.Mesh(g,m); o.position.set(...pos); o.rotation.set(...rot); o.castShadow=true; o.receiveShadow=true; plane.add(o); return o; }
mesh(new THREE.CapsuleGeometry(.38,2.5,8,16),fuselageMat,[0,0,0],[Math.PI/2,0,0]);
mesh(new THREE.ConeGeometry(.38,.7,16),fuselageMat,[0,0,-1.62],[Math.PI/2,0,0]);
mesh(new THREE.ConeGeometry(.31,.7,16),darkMat,[0,0,1.62],[-Math.PI/2,0,0]);
mesh(new THREE.BoxGeometry(3.6,.08,.72),fuselageMat,[0,0,.15],[0,.02,-.04]);
mesh(new THREE.BoxGeometry(.09,1.3,.65),fuselageMat,[0,.08,1.06],[0,0,.18]);
mesh(new THREE.BoxGeometry(.08,.88,.42),orangeMat,[0,.62,1.05],[0,0,.18]);
mesh(new THREE.BoxGeometry(.13,.65,.44),darkMat,[0,.28,-.45],[0,0,0]);
mesh(new THREE.SphereGeometry(.27,12,8),glassMat,[0,.27,-.62]);
const engineGeo=new THREE.CylinderGeometry(.14,.19,.6,12); const e1=mesh(engineGeo,darkMat,[-1.18,-.08,.15],[Math.PI/2,0,0]); const e2=mesh(engineGeo,darkMat,[1.18,-.08,.15],[Math.PI/2,0,0]);
const exhaustMat=mat(0x47dcff,.1,.2,0x27cfff); mesh(new THREE.ConeGeometry(.12,.3,10),exhaustMat,[-1.18,-.08,.55],[-Math.PI/2,0,0]); mesh(new THREE.ConeGeometry(.12,.3,10),exhaustMat,[1.18,-.08,.55],[-Math.PI/2,0,0]);

const world = new THREE.Group(); scene.add(world);
const stars = new THREE.Group(); scene.add(stars); const starGeo=new THREE.BufferGeometry(); const starPos=[]; for(let i=0;i<900;i++) starPos.push((Math.random()-.5)*1700, Math.random()*520-40, -Math.random()*1800); starGeo.setAttribute('position',new THREE.Float32BufferAttribute(starPos,3)); stars.add(new THREE.Points(starGeo,new THREE.PointsMaterial({color:0x9ed7ee,size:1.5,sizeAttenuation:true,transparent:true,opacity:.7})));
const grid = new THREE.GridHelper(1800,45,0x16466b,0x0b2945); grid.position.y=-38; grid.position.z=-650; grid.rotation.x=0; scene.add(grid);

const gates=[]; const gateMat=new THREE.MeshBasicMaterial({color:0x70e7ff, transparent:true, opacity:.88, side:THREE.DoubleSide}); const gateGlow=new THREE.MeshBasicMaterial({color:0xffa455, transparent:true, opacity:.45, side:THREE.DoubleSide});
function makeGate(i) { const g=new THREE.Group(); const r=5.2 + (i%3)*.35; const ring=new THREE.Mesh(new THREE.TorusGeometry(r,.1,10,64),gateMat.clone()); const inner=new THREE.Mesh(new THREE.TorusGeometry(r-.32,.035,8,64),gateGlow); g.add(ring,inner); g.position.set((Math.random()-.5)*30, 4+Math.random()*18, -80-i*105); g.rotation.set((Math.random()-.5)*.32,(Math.random()-.5)*.36,(Math.random()-.5)*.4); world.add(g); gates.push({group:g, radius:r, passed:false}); }
for(let i=0;i<12;i++) makeGate(i);
for(let i=0;i<70;i++){ const b=new THREE.Mesh(new THREE.BoxGeometry(1+Math.random()*8,1+Math.random()*28,1+Math.random()*8),new THREE.MeshStandardMaterial({color:0x0b2740,roughness:1})); b.position.set((Math.random()-.5)*250, -25+Math.random()*16, -30-Math.random()*1250); b.rotation.y=Math.random(); world.add(b); }

const keys={}; addEventListener('keydown',e=>{ keys[e.code]=true; if(['ArrowUp','ArrowDown','Space'].includes(e.code)) e.preventDefault(); }); addEventListener('keyup',e=>keys[e.code]=false);
const state={running:false, score:0, passed:0, speed:1.1, pitch:0, roll:0, yaw:0, x:0, y:7, z:4};
let audioCtx, engineOsc, engineGain, muted=false;
function audioStart(){ if(audioCtx) return; audioCtx=new AudioContext(); engineOsc=audioCtx.createOscillator(); engineGain=audioCtx.createGain(); engineOsc.type='sawtooth'; engineOsc.frequency.value=65; engineGain.gain.value=.025; engineOsc.connect(engineGain).connect(audioCtx.destination); engineOsc.start(); }
function tone(freq,duration=.16){ if(!audioCtx||muted) return; const o=audioCtx.createOscillator(),g=audioCtx.createGain(); o.type='sine'; o.frequency.value=freq; g.gain.setValueAtTime(.08,audioCtx.currentTime); g.gain.exponentialRampToValueAtTime(.001,audioCtx.currentTime+duration); o.connect(g).connect(audioCtx.destination); o.start(); o.stop(audioCtx.currentTime+duration); }
function reset(){ state.running=true; state.score=0; state.passed=0; state.speed=1.1; state.pitch=state.roll=state.yaw=0; state.x=0; state.y=7; state.z=4; plane.position.set(0,0,0); plane.rotation.set(0,0,0); flight.position.set(0,7,4); gates.forEach((g,i)=>{g.passed=false;g.group.position.set((Math.random()-.5)*30,4+Math.random()*18,-80-i*105);}); document.querySelector('#start-screen').classList.remove('visible'); document.querySelector('#game-over').classList.remove('visible'); audioStart(); }
function end(win=true){state.running=false; document.querySelector('#final-score').textContent=String(state.score).padStart(6,'0'); document.querySelector('#result-title').textContent=win?'MISSION ACCOMPLIE':'VOL INTERROMPU'; document.querySelector('#result-message').textContent=win?'Tous les anneaux ont été traversés.':'Le cap a été perdu. Reprenez les commandes et retentez votre chance.'; document.querySelector('#game-over').classList.add('visible'); tone(win?880:130,.4); }
document.querySelector('#start-button').onclick=reset; document.querySelector('#restart-button').onclick=reset; document.querySelector('#sound-toggle').onclick=()=>{muted=!muted; document.querySelector('#sound-toggle').textContent=muted?'◌':'♫'; if(engineGain) engineGain.gain.value=muted?0:.025;};

function update(dt){ if(!state.running) return; const pitchInput=(keys.KeyW?1:0)-(keys.KeyS?1:0); const rollInput=(keys.KeyD?1:0)-(keys.KeyA?1:0); const yawInput=(keys.KeyE?1:0)-(keys.KeyQ?1:0); const speedInput=(keys.ShiftLeft?1:0)-(keys.ArrowDown?1:0)+(keys.ArrowUp?1:0)*.25;
 state.pitch=THREE.MathUtils.lerp(state.pitch,pitchInput*.36,dt*3.5); state.roll=THREE.MathUtils.lerp(state.roll,rollInput*.62,dt*4); state.yaw=THREE.MathUtils.lerp(state.yaw,yawInput*.28,dt*2.5); state.speed=THREE.MathUtils.clamp(state.speed+speedInput*dt*.32, .65,1.8);
 state.x+=(-rollInput*state.speed*9 + yawInput*state.speed*3)*dt; state.y+=(-pitchInput*state.speed*7)*dt; state.x=THREE.MathUtils.clamp(state.x,-36,36); state.y=THREE.MathUtils.clamp(state.y,-5,37);
 flight.position.lerp(new THREE.Vector3(state.x,state.y,state.z),dt*2.7); plane.rotation.z=THREE.MathUtils.lerp(plane.rotation.z,-state.roll,dt*5); plane.rotation.x=THREE.MathUtils.lerp(plane.rotation.x,state.pitch,dt*5); plane.rotation.y=THREE.MathUtils.lerp(plane.rotation.y,state.yaw,dt*5);
 world.position.z += state.speed*42*dt; stars.position.z += state.speed*10*dt; if(world.position.z>100) world.position.z=0;
 for(const gate of gates){ const p=gate.group.position.clone(); p.z+=world.position.z; const dx=state.x-p.x, dy=state.y-p.y, dz=state.z-p.z; const d=Math.hypot(dx,dy); if(!gate.passed && p.z>state.z-1){ gate.passed=true; if(d<gate.radius*.72){ state.passed++; state.score+=Math.round(1000/state.speed); tone(620+state.passed*35,.22); gate.group.children.forEach(m=>m.material.color.set(0xffb45c)); } else { state.score=Math.max(0,state.score-250); tone(170,.28); } } if(gate.passed && p.z>35) { gate.group.position.z-=1260; gate.passed=false; gate.group.position.x=(Math.random()-.5)*30; gate.group.position.y=4+Math.random()*18; } }
 if(state.passed>=12) end(true); const altitude=Math.round(1200+state.y*35); document.querySelector('#score').textContent=String(state.score).padStart(6,'0'); document.querySelector('#gates').textContent=`${state.passed} / 12`; document.querySelector('#speed').textContent=Math.round(270+state.speed*75); document.querySelector('#altitude').textContent=String(altitude).padStart(4,'0'); document.querySelector('#heading').textContent=String(Math.round((state.yaw*130+360)%360)).padStart(3,'0'); document.querySelector('#throttle-fill').style.width=`${((state.speed-.65)/1.15)*100}%`; if(engineOsc) engineOsc.frequency.value=50+state.speed*45; }

function animate(){ requestAnimationFrame(animate); const dt=Math.min(clock.getDelta(),.05); update(dt); const target=new THREE.Vector3().copy(flight.position); camera.position.lerp(new THREE.Vector3(target.x*.22,target.y+2.4,target.z+10),dt*2); camera.lookAt(target.x*.55,target.y*.78,target.z-32); cyanLight.position.copy(flight.position); renderer.render(scene,camera); } animate();
addEventListener('resize',()=>{camera.aspect=innerWidth/innerHeight;camera.updateProjectionMatrix();renderer.setSize(innerWidth,innerHeight);renderer.setPixelRatio(Math.min(devicePixelRatio,2));});
