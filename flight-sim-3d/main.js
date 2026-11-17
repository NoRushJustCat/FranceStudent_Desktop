import * as THREE from 'three';
import { FlightAudio } from './audio.js';

// AERO HORIZON 3D. Unités internes : mètres, secondes, m/s ; cap 0 = nord (-Z).
const $ = id => document.getElementById(id);
const clamp = THREE.MathUtils.clamp, KTS = 1.943844, FT = 3.28084, NM = 1852;
const audio = new FlightAudio();
const renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: 'high-performance' });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
renderer.setSize(innerWidth, innerHeight);
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
$('canvas-container').append(renderer.domElement);
const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(72, innerWidth / innerHeight, 0.1, 23000);
scene.add(camera);
const mat = (color, extra = {}) => new THREE.MeshStandardMaterial({ color, metalness: .55, roughness: .48, ...extra });
function add(parent, geometry, material, x = 0, y = 0, z = 0) {
  const object = new THREE.Mesh(geometry, material);
  object.position.set(x, y, z); parent.add(object); return object;
}
function flat(parent, points, material, y = 0) {
  const shape = new THREE.Shape(); shape.moveTo(...points[0]);
  points.slice(1).forEach(p => shape.lineTo(...p)); shape.closePath();
  const object = add(parent, new THREE.ShapeGeometry(shape), material, 0, y);
  object.rotation.x = -Math.PI / 2; return object;
}
const metal = mat(0xaebfcd, { metalness: .8 });
const dark = mat(0x213247, { metalness: .7 });
const blue = mat(0x27cbe8, { emissive: 0x126790 });
const plane = new THREE.Group(); scene.add(plane);
const body = add(plane, new THREE.CylinderGeometry(.7, 1, 10, 12), metal);
body.rotation.x = Math.PI / 2;
const nose = add(plane, new THREE.ConeGeometry(.7, 5.2, 12), metal, 0, 0, -7.6);
nose.rotation.x = -Math.PI / 2;
const canopy = add(plane, new THREE.SphereGeometry(1, 16, 12), mat(0x176086, { transparent: true, opacity: .75, roughness: .1 }), 0, .7, -2.65);
canopy.scale.set(.82, .8, 2.5);
const ailerons = [], flames = [], engineLights = [];
for (const side of [-1, 1]) {
  flat(plane, [[side * .7,-2.6],[side * 8.2,2.2],[side * 8.3,3.3],[side * .8,2]], metal, -.1);
  flat(plane, [[side * 2.4,-1],[side * 7.5,2.35],[side * 7.5,2.5],[side * 2.4,-.8]], blue, -.08);
  flat(plane, [[side * .7,3.5],[side * 3.5,4.8],[side * 3.5,5.5],[side * .7,4.9]], dark, .03);
  flat(plane, [[side * .65,-4.4],[side * 2.2,-3.8],[side * 2.1,-3.4],[side * .7,-3.6]], metal, .12);
  const fin = add(plane, new THREE.BoxGeometry(.13, 2.65, 2.5), dark, side * 1.05, 1.32, 3.85);
  fin.rotation.z = -side * .19;
  const pivot = new THREE.Group(); pivot.position.set(side * 5.5, -.07, 1.5);
  add(pivot, new THREE.BoxGeometry(3.1,.08,.65), dark); plane.add(pivot); ailerons.push(pivot);
  const nozzle = add(plane, new THREE.CylinderGeometry(.48,.58,1.2,12), dark, side * .8,-.4,5.4);
  nozzle.rotation.x = Math.PI / 2;
  const fire = add(plane, new THREE.ConeGeometry(.45,3,12,1,true), new THREE.MeshBasicMaterial({ color: 0x54c8ff, transparent: true, opacity: .65, side: THREE.DoubleSide, depthWrite: false }), side * .8,-.4,6.9);
  fire.rotation.x = Math.PI / 2; flames.push(fire);
  const light = new THREE.PointLight(0x61afff,0,14); light.position.set(side * .8,-.4,6.2);
  plane.add(light); engineLights.push(light);
}
const gear = new THREE.Group(); plane.add(gear);
for (const [x,z] of [[-1.6,1.5],[1.6,1.5],[0,-4.7]]) {
  add(gear,new THREE.CylinderGeometry(.07,.07,1.25),dark,x,-1.1,z);
  add(gear,new THREE.TorusGeometry(.31,.13,7,12),dark,x,-1.7,z).rotation.y = Math.PI / 2;
}
// Cadre intérieur fixé à la caméra, seul en vue cockpit.
const cockpitFrame = new THREE.Group(); camera.add(cockpitFrame);
const frameMat = new THREE.MeshBasicMaterial({ color: 0x162232 });
for (const side of [-1,1]) add(cockpitFrame,new THREE.BoxGeometry(.11,1.7,.14),frameMat,side * 1.8,-.08,-1.4).rotation.z = side * .18;
add(cockpitFrame,new THREE.BoxGeometry(4,.7,.2),frameMat,0,-1.18,-1.5);

const runwayZ = -1400;
function groundY(x,z) {
  const outside = Math.max(Math.abs(x)/200,Math.abs(z-runwayZ)/1850);
  const blend = THREE.MathUtils.smoothstep(outside,.8,1.7);
  return blend * Math.max(-4,45*Math.sin(x*.0015)*Math.cos(z*.0012)+18*Math.sin(x*.004+z*.002)+12*Math.cos(z*.005));
}
const terrain = new THREE.PlaneGeometry(23000,23000,130,130);
terrain.rotateX(-Math.PI/2);
const positions = terrain.attributes.position, colors = [];
const low = new THREE.Color(0x233b45), high = new THREE.Color(0x778779);
for (let i=0;i<positions.count;i++) {
  const y=groundY(positions.getX(i),positions.getZ(i)); positions.setY(i,y);
  const color=low.clone().lerp(high,clamp(.2+y/125,0,1));
  colors.push(color.r,color.g,color.b);
}
terrain.setAttribute('color',new THREE.Float32BufferAttribute(colors,3));
terrain.computeVertexNormals();
add(scene,terrain,new THREE.MeshStandardMaterial({vertexColors:true,roughness:1,side:THREE.DoubleSide}));
const asphalt=add(scene,new THREE.PlaneGeometry(76,3000),mat(0x25313b,{roughness:1}),0,.16,runwayZ);
asphalt.rotation.x=-Math.PI/2;
const markings=new THREE.MeshBasicMaterial({color:0xf3f3e6});
for (const x of [-35,35]) add(scene,new THREE.PlaneGeometry(1,2960),markings,x,.2,runwayZ).rotation.x=-Math.PI/2;
for (let z=runwayZ-1400;z<runwayZ+1400;z+=135) add(scene,new THREE.PlaneGeometry(1.4,60),markings,0,.21,z).rotation.x=-Math.PI/2;
const runwayLights=[];
for (let z=runwayZ-1480;z<=runwayZ+1480;z+=80) for (const x of [-40,40]) {
  const color=z<runwayZ-1300?0xff3750:z>runwayZ+1300?0x28ff9e:0xffedbc;
  runwayLights.push(add(scene,new THREE.SphereGeometry(1.5,6,5),new THREE.MeshBasicMaterial({color}),x,1.5,z));
}
const waypoints=[[0,150,-3800],[650,260,-5900],[-800,350,-8300],[250,200,-10700],[0,320,-13700]].map((p,index)=>{
  const group=new THREE.Group(); group.position.set(...p);
  add(group,new THREE.TorusGeometry(55,3,8,48),new THREE.MeshBasicMaterial({color:0xffd069}));
  add(group,new THREE.TorusGeometry(61,1,6,48),new THREE.MeshBasicMaterial({color:0xff822a}));
  scene.add(group); return {group,index,passed:false};
});
const cloudGeo=new THREE.SphereGeometry(1,9,6);
const cloudMat=new THREE.MeshLambertMaterial({color:0xe9eafb,transparent:true,opacity:.38,depthWrite:false});
for (let i=0;i<45;i++) {
  const cluster=new THREE.Group(); cluster.position.set(Math.sin(i*17.1)*(250+i%9*160),320+i%7*75,1400-i*365);
  for (let j=0;j<3;j++) add(cluster,cloudGeo,cloudMat,(j-1)*65,j%2*17,j*18).scale.set(95+j*17,25+j*8,48);
  scene.add(cluster);
}
const starCoordinates=[];
for(let i=0;i<400;i++) {
  const angle=i*2.39996, height=.15+i%87/115;
  const radius=Math.sqrt(1-height*height)*7600;
  starCoordinates.push(Math.sin(angle)*radius,height*7600,Math.cos(angle)*radius);
}
const starGeo=new THREE.BufferGeometry(); starGeo.setAttribute('position',new THREE.Float32BufferAttribute(starCoordinates,3));
const stars=new THREE.Points(starGeo,new THREE.PointsMaterial({color:0xffffff,size:16})); scene.add(stars);
const ambient=new THREE.HemisphereLight(0xd3e8ff,0x3c573b,2);
const sun=new THREE.DirectionalLight(0xfff5df,2.1); sun.position.set(-1100,2500,-1800);
scene.add(ambient,sun);
function weather(mode) {
  const palettes={
    noon:[0x83bee8,0xd2edff,0xfff5df,2.1,2,.000055],
    sunset:[0x794371,0xffaa9d,0xffa85b,1.9,1,.000095],
    night:[0x070e26,0x354773,0x6678ff,.65,.65,.00013],
  };
  const [sky,hemi,solar,light,skyLight,fog]=palettes[mode]||palettes.noon;
  scene.background=new THREE.Color(sky); scene.fog=new THREE.FogExp2(sky,fog);
  ambient.color.setHex(hemi); ambient.intensity=skyLight;
  sun.color.setHex(solar); sun.intensity=light; sun.position.y=mode==='sunset'?370:2500;
  stars.visible=mode==='night'; cloudMat.opacity=mode==='night'?.17:.38;
}
weather('noon');
const state={mode:'title',position:new THREE.Vector3(0,2.25,80),speed:0,vertical:0,throttle:0,
  fuel:100,yaw:0,pitch:0,roll:0,flaps:0,gear:true,brakes:false,stall:false,pullUp:false,
  overspeed:false,gforce:1,vsi:0,elapsed:0,distance:0,maxAlt:0,maxSpeed:0,rings:0,
  view:'chase',messageTime:0,landed:false};
const keys=new Set(),mouse=new THREE.Vector2();
let mouseControl=true,invert=false,accumulator=0,previous=performance.now();
const show=(id,yes)=>$(id).classList.toggle('hidden',!yes);
const label=(id,value)=>{$(id).textContent=value;};
function quiet(){audio.playStallAlarm(false);audio.playPullUpAlarm(false);audio.playOverspeedAlarm(false);}
function message(value,seconds=3){label('hud-message',value);state.messageTime=seconds;}
function reset(){
  state.position.set(0,2.25,80);
  Object.assign(state,{speed:0,vertical:0,throttle:0,fuel:100,yaw:0,pitch:0,roll:0,
    flaps:0,gear:true,brakes:false,stall:false,pullUp:false,overspeed:false,
    gforce:1,vsi:0,elapsed:0,distance:0,maxAlt:0,maxSpeed:0,rings:0,
    view:'chase',messageTime:0,landed:false});
  keys.clear(); mouse.set(0,0); quiet();
  for(const wp of waypoints){wp.passed=false;wp.group.visible=true;}
  audio.updateEngine(0,0);audio.updateWind(0);
  camera.position.set(0,7,110);camera.lookAt(0,2,-200);
  updatePlane(0);hud();
}
function start(){
  audio.unlock();audio.setVolume(.7);
  weather(document.querySelector('input[name="weather"]:checked')?.value||'noon');
  mouseControl=$('opt-mouse').checked;invert=$('opt-invert').checked;
  reset();state.mode='flying';accumulator=0;previous=performance.now();
  show('title-screen',false);show('pause-screen',false);show('gameover-screen',false);
  show('hud',true);$('hud').setAttribute('aria-hidden','false');
  message('MAJ : GAZ / TIRER À 140 KTS',5);
}
function pause(){
  if(state.mode!=='flying')return;
  state.mode='pause';keys.clear();quiet();audio.setVolume(0);show('pause-screen',true);
}
function resume(){
  if(state.mode!=='pause')return;
  audio.unlock();audio.setVolume(.7);state.mode='flying';previous=performance.now();show('pause-screen',false);
}
function menu(){
  state.mode='title';keys.clear();quiet();audio.setVolume(0);
  show('hud',false);$('hud').setAttribute('aria-hidden','true');
  show('pause-screen',false);show('gameover-screen',false);show('title-screen',true);
}
function crash(reason){
  if(state.mode!=='flying')return;
  state.mode='crash';keys.clear();quiet();audio.playCrashSound();
  label('crash-reason',reason);
  label('stat-time',`${String(Math.floor(state.elapsed/60)).padStart(2,'0')}:${String(Math.floor(state.elapsed%60)).padStart(2,'0')}`);
  label('stat-distance',`${(state.distance/NM).toFixed(1)} NM`);
  label('stat-maxalt',`${Math.round(state.maxAlt*FT)} FT`);
  label('stat-maxspeed',`${Math.round(state.maxSpeed*KTS)} KTS`);
  label('stat-waypoints',String(state.rings));show('gameover-screen',true);
  setTimeout(()=>{if(state.mode==='crash')audio.setVolume(0);},3000);
}
function keyOf(event){
  if(event.code.startsWith('Shift'))return 'shift';
  if(event.code.startsWith('Control'))return 'control';
  return event.key.toLowerCase();
}
const controls=new Set(['z','s','q','d','a','e','w','x','g','f','b','c','p','m','escape','shift','control','arrowup','arrowdown','arrowleft','arrowright']);
window.addEventListener('keydown',event=>{
  const key=keyOf(event);
  if(controls.has(key))event.preventDefault();
  if(event.repeat){if(state.mode==='flying')keys.add(key);return;}
  if(key==='p'||key==='escape'){
    if(state.mode==='flying')pause();else if(state.mode==='pause')resume();return;
  }
  if(state.mode!=='flying')return;
  keys.add(key);
  if(key==='g'){state.gear=!state.gear;audio.playGearSound(state.gear);}
  if(key==='f'){state.flaps=(state.flaps+1)%3;audio.playFlapsSound();}
  if(key==='b')state.brakes=!state.brakes;
  if(key==='c')state.view=state.view==='chase'?'cockpit':'chase';
  if(key==='m')audio.toggleMute();
});
window.addEventListener('keyup',event=>keys.delete(keyOf(event)));
window.addEventListener('blur',()=>{keys.clear();pause();});
renderer.domElement.addEventListener('pointermove',event=>{
  const rect=renderer.domElement.getBoundingClientRect();
  mouse.set(clamp((event.clientX-rect.left-rect.width/2)/(rect.width*.4),-1,1),
    clamp((event.clientY-rect.top-rect.height/2)/(rect.height*.4),-1,1));
});
for(const [id,handler] of [['btn-start',start],['btn-resume',resume],
  ['btn-restart-pause',start],['btn-menu-pause',menu],['btn-restart',start],['btn-menu',menu]]){
  $(id).addEventListener('click',handler);
}
function step(dt){
  const s=state,v=s.speed;s.elapsed+=dt;
  s.throttle=clamp(s.throttle+dt*.36*(Number(keys.has('shift')||keys.has('w'))-Number(keys.has('control')||keys.has('x'))),0,1);
  if(s.fuel<=0)s.throttle=0;
  s.fuel=Math.max(0,s.fuel-dt*(.0015+.009*s.throttle));
  const pitchKey=Number(keys.has('z')||keys.has('arrowup'))-Number(keys.has('s')||keys.has('arrowdown'));
  const rollKey=Number(keys.has('q')||keys.has('arrowleft'))-Number(keys.has('d')||keys.has('arrowright'));
  const yawKey=Number(keys.has('a'))-Number(keys.has('e'));
  const pitchTarget=clamp(pitchKey*.48+(mouseControl?mouse.y*(invert?1:-1)*.4:0),-.62,.62);
  const rollTarget=clamp(rollKey*.95+(mouseControl?-mouse.x*.88:0),-1.2,1.2);
  s.pitch+=(pitchTarget-s.pitch)*(1-Math.exp(-dt*1.7));
  s.roll+=(rollTarget-s.roll)*(1-Math.exp(-dt*2.1));
  s.yaw+=(yawKey*.42+Math.sin(s.roll)*clamp(v/90,0,1.4)*.4)*dt;
  const floor=groundY(s.position.x,s.position.z);
  const grounded=s.position.y<=floor+(s.gear?2.25:1.45)+.04&&s.vertical<=0;
  const flightPath=Math.atan2(s.vertical,Math.max(v,14)),aoa=s.pitch-flightPath;
  const stallSpeed=(s.flaps?59:65)/KTS;
  const lift=9.81*(v/76)**2*(.86+clamp(aoa,-.24,.35)*2.7+s.flaps*.17)*
    clamp((v-stallSpeed*.6)/(stallSpeed*.4),.04,1)*clamp(1-Math.max(0,Math.abs(aoa)-.38)*2.8,.16,1);
  s.gforce=clamp(lift/9.81,-2,9);
  s.stall=!grounded&&v*KTS<65&&s.pitch>-.1;
  const drag=.55+v*v*(.00025+(s.gear?.00019:0)+s.flaps*.00013+(s.brakes?.00085:0))+(grounded?.55:0);
  const thrust=s.fuel>0?22.5*s.throttle:0;
  s.speed=clamp(v+(thrust-drag-9.81*Math.sin(flightPath))*dt,0,410);
  const verticalAccel=lift*Math.cos(s.roll)-9.81+thrust*Math.sin(s.pitch)*.24;
  if(grounded)s.vertical=verticalAccel>1?Math.max(0,s.vertical+verticalAccel*dt):0;
  else {
    s.vertical+=(verticalAccel-s.vertical*.08)*dt;
    if(s.stall){s.vertical-=3.8*dt;s.pitch-=.08*dt;}
  }
  s.position.y+=s.vertical*dt;
  s.position.x+=-Math.sin(s.yaw)*s.speed*dt;
  s.position.z+=-Math.cos(s.yaw)*s.speed*dt;
  s.distance+=s.speed*dt;
  const surface=groundY(s.position.x,s.position.z),touch=s.position.y<=surface+(s.gear?2.25:1.45);
  if(touch){
    const runway=Math.abs(s.position.x)<34&&Math.abs(s.position.z-runwayZ)<1470;
    if(!s.gear&&s.speed>12){crash('Atterrissage sans train');return;}
    if(s.vertical< -4.5||(s.speed>55&&(Math.abs(s.roll)>.48||Math.abs(s.pitch)>.32))){crash('Impact violent avec le sol');return;}
    if(!grounded&&runway&&s.speed>24&&!s.landed){message('ATTERRISSAGE RÉUSSI');s.landed=true;}
    s.position.y=surface+(s.gear?2.25:1.45);s.vertical=0;
  } else if(s.position.y-surface>9)s.landed=false;
  s.maxAlt=Math.max(s.maxAlt,s.position.y);s.maxSpeed=Math.max(s.maxSpeed,s.speed);
  s.vsi+=(s.vertical*FT*60-s.vsi)*(1-Math.exp(-dt*1.8));
  s.pullUp=!touch&&s.position.y-surface<Math.max(25,-s.vertical*8)&&s.vertical< -3;
  s.overspeed=s.speed*KTS>700;
  for(const wp of waypoints){
    if(wp.passed||s.position.distanceTo(wp.group.position)>58)continue;
    wp.passed=true;wp.group.visible=false;s.rings++;
    audio.playWaypointRing();message(`ANNEAU ${wp.index+1} FRANCHI`);
  }
  s.messageTime=Math.max(0,s.messageTime-dt);
  audio.updateEngine(s.throttle,s.speed*KTS);audio.updateWind(s.speed*KTS);
  audio.playStallAlarm(s.stall);audio.playPullUpAlarm(s.pullUp);audio.playOverspeedAlarm(s.overspeed);
}
function updatePlane(time){
  plane.position.copy(state.position);plane.rotation.order='YXZ';
  plane.rotation.set(state.pitch,state.yaw,state.roll);
  gear.visible=state.gear;
  ailerons.forEach((a,i)=>{a.rotation.x=state.roll*(i?-.22:.22)+state.pitch*.08;});
  flames.forEach((f,i)=>{
    f.visible=state.mode==='flying'&&state.throttle>.42;
    f.scale.setScalar(.65+state.throttle*.8+Math.sin(time*28+i)*.07);
    engineLights[i].intensity=f.visible?state.throttle*4:0;
  });
}
const lookMatrix=new THREE.Matrix4(),targetRotation=new THREE.Quaternion();
function updateCamera(dt,time){
  const inside=state.view==='cockpit'&&state.mode==='flying';
  plane.visible=!inside;cockpitFrame.visible=inside;
  if(inside){
    camera.position.copy(plane.localToWorld(new THREE.Vector3(0,1,-2.7)));
    camera.quaternion.slerp(plane.quaternion,1-Math.exp(-dt*10));
  }else{
    camera.position.lerp(plane.localToWorld(new THREE.Vector3(0,5.5,23)),1-Math.exp(-dt*3.5));
    const focus=plane.localToWorld(new THREE.Vector3(0,.7,-42));
    const up=new THREE.Vector3(0,1,0).applyQuaternion(plane.quaternion);
    lookMatrix.lookAt(camera.position,focus,up);targetRotation.setFromRotationMatrix(lookMatrix);
    camera.quaternion.slerp(targetRotation,1-Math.exp(-dt*5));
  }
  if(state.stall&&state.mode==='flying'){
    camera.position.x+=Math.sin(time*47)*.1;camera.position.y+=Math.cos(time*41)*.1;
  }
  camera.fov=72+clamp(state.speed/90,0,1)*(inside?4:10);camera.updateProjectionMatrix();
}
function buildHUD(){
  for(let degree=-720;degree<=720;degree+=10){
    const tick=document.createElement('span');
    tick.className='tape-tick'+(degree%30===0?' major':'')+(degree%90===0?' cardinal':'');
    const bearing=(degree%360+360)%360;
    tick.textContent=degree%30===0?({0:'N',90:'E',180:'S',270:'W'}[bearing]||String(bearing).padStart(3,'0')):'';
    $('heading-tape').append(tick);
  }
  for(let degree=-80;degree<=80;degree+=10){
    if(!degree)continue;
    const rung=document.createElement('div');rung.className='pitch-rung'+(degree<0?' neg':'');
    rung.style.top=`${-degree*4}px`;
    for(let k=0;k<2;k++){
      const number=document.createElement('span');number.textContent=Math.abs(degree);rung.append(number);
    }
    $('pitch-ladder').append(rung);
  }
  for(const degree of [-60,-45,-30,-20,-10,0,10,20,30,45,60]){
    const rad=THREE.MathUtils.degToRad(degree),r=degree%30===0?112:119;
    const mark=document.createElementNS('http://www.w3.org/2000/svg','line');
    mark.setAttribute('x1',Math.sin(rad)*129);mark.setAttribute('y1',-Math.cos(rad)*129);
    mark.setAttribute('x2',Math.sin(rad)*r);mark.setAttribute('y2',-Math.cos(rad)*r);
    $('roll-scale').append(mark);
  }
}
buildHUD();
function hud(){
  const s=state,heading=(THREE.MathUtils.radToDeg(s.yaw)%360+360)%360;
  label('hud-heading',`${String(Math.round(heading)%360).padStart(3,'0')}°`);
  label('hud-speed',Math.round(s.speed*KTS));
  label('hud-altitude',Math.round(Math.max(0,s.position.y*FT)).toLocaleString('fr-FR'));
  label('hud-vsi',`${s.vsi>=0?'+':''}${Math.round(s.vsi/10)*10}`);
  label('hud-throttle',`${Math.round(s.throttle*100)}%`);
  $('throttle-fill').style.height=`${s.throttle*100}%`;
  label('hud-flaps',`${s.flaps*15}°`);label('hud-brakes',s.brakes?'ON':'OFF');
  label('hud-gear',s.gear?'DOWN':'UP');label('hud-gforce',s.gforce.toFixed(1));
  label('hud-fuel-text',`${Math.ceil(s.fuel)}%`);$('hud-fuel-bar').style.width=`${s.fuel}%`;
  label('hud-camera',s.view==='chase'?'EXT':'INT');
  $('heading-tape').style.transform=`translateX(${-72*40-20-heading*4}px)`;
  $('pitch-ladder').style.transform=`rotate(${-THREE.MathUtils.radToDeg(s.roll)}deg) translateY(${clamp(THREE.MathUtils.radToDeg(s.pitch),-80,80)*4}px)`;
  $('roll-scale').setAttribute('transform',`rotate(${-THREE.MathUtils.radToDeg(s.roll)})`);
  $('fpv').style.transform=`translate(${clamp(-s.roll*24,-55,55)}px,${clamp((s.pitch-Math.atan2(s.vertical,Math.max(12,s.speed)))*180,-85,85)}px)`;
  for(const [id,on] of [['alert-stall',s.stall],['alert-pullup',s.pullUp],['alert-overspeed',s.overspeed],['alert-fuel',s.fuel<20]]) show(id,on&&s.mode==='flying');
  show('hud-message',s.messageTime>0&&s.mode==='flying');
  radar();
}
function radar(){
  const canvas=$('radar'),ctx=canvas.getContext('2d'),w=canvas.width,h=canvas.height;
  ctx.clearRect(0,0,w,h);ctx.save();ctx.translate(w/2,h/2);
  const scale=w*.45/7500,yaw=state.yaw;
  const point=(x,z)=>{
    const dx=x-state.position.x,dz=z-state.position.z;
    return [(dx*Math.cos(yaw)-dz*Math.sin(yaw))*scale,(dx*Math.sin(yaw)+dz*Math.cos(yaw))*scale];
  };
  ctx.strokeStyle='#00ffaa';ctx.lineWidth=2;
  ctx.beginPath();ctx.moveTo(...point(0,runwayZ+1500));ctx.lineTo(...point(0,runwayZ-1500));ctx.stroke();
  for(const wp of waypoints){
    if(wp.passed)continue;
    const [x,y]=point(wp.group.position.x,wp.group.position.z);
    if(Math.hypot(x,y)>w*.48)continue;
    ctx.strokeStyle='#ffcc66';ctx.beginPath();ctx.arc(x,y,4,0,Math.PI*2);ctx.stroke();
  }
  ctx.fillStyle='#00ffaa';ctx.beginPath();ctx.moveTo(0,-9);ctx.lineTo(-6,7);
  ctx.lineTo(0,4);ctx.lineTo(6,7);ctx.closePath();ctx.fill();ctx.restore();
  const next=waypoints.find(wp=>!wp.passed);
  label('radar-waypoint',next?`WPT ${next.index+1}`:'PARCOURS TERMINÉ');
  label('radar-distance',next?`${(state.position.distanceTo(next.group.position)/NM).toFixed(1)} NM`:'-- NM');
}
function render(now){
  requestAnimationFrame(render);
  let dt=Math.min((now-previous)/1000,.08);previous=now;if(document.hidden)dt=0;
  if(state.mode==='flying'){
    accumulator=Math.min(accumulator+dt,.15);
    while(accumulator>=1/60){
      step(1/60);accumulator-=1/60;
      if(state.mode!=='flying'){accumulator=0;break;}
    }
  }else accumulator=0;
  updatePlane(now/1000);updateCamera(dt,now/1000);
  if(state.mode==='flying'||state.mode==='crash')hud();
  const blink=Math.sin(now*.006)>-.2;
  runwayLights.forEach((light,i)=>{light.visible=i%9!==0||blink;});
  renderer.render(scene,camera);
}
window.addEventListener('resize',()=>{
  camera.aspect=innerWidth/innerHeight;camera.updateProjectionMatrix();
  renderer.setSize(innerWidth,innerHeight);renderer.setPixelRatio(Math.min(devicePixelRatio,2));
});
document.addEventListener('visibilitychange',()=>{if(document.hidden)pause();});
reset();requestAnimationFrame(render);
