import * as THREE from './vendor/three.module.js';

const stage = document.querySelector('#stage');
const $ = s => document.querySelector(s);
const state = {mode:'landscape', accessories:'capped', camera:'whole', explode:0, inspect:false, cutaway:false, theta:.43, phi:1.33, zoom:1};
const parts = [], hidden = new Set();
let manifest, renderer, camera, scene, floor, selected, fitDistance=190, target = new THREE.Vector3(), pending=false, ready=false;
const radians = v => Number(v || 0) * Math.PI / 180;
const toWorld = p => new THREE.Vector3(p[0]||0, p[1]||0, -(p[2]||0));
const clip = new THREE.Plane(new THREE.Vector3(-1,0,0), 0);

function stlGeometry(buffer, coordinateFrame='case-native') {
  const bytes = new Uint8Array(buffer), dv = new DataView(buffer);
  const count = bytes.length >= 84 ? dv.getUint32(80,true) : 0;
  let positions;
  if (count > 0 && 84 + count * 50 <= bytes.length && 84 + count * 50 >= bytes.length - 512) {
    positions = new Float32Array(count * 9);
    for (let t=0;t<count;t++) for(let v=0;v<3;v++) for(let k=0;k<3;k++) positions[t*9+v*3+k]=dv.getFloat32(84+t*50+12+v*12+k*4,true);
  } else {
    const text = new TextDecoder().decode(buffer), values=[];
    for(const m of text.matchAll(/vertex\s+([-+\d.eE]+)\s+([-+\d.eE]+)\s+([-+\d.eE]+)/g)) values.push(Number(m[1]),Number(m[2]),Number(m[3]));
    if(!values.length) throw new Error('STL has no vertices');
    positions=new Float32Array(values);
  }
  if(coordinateFrame==='stand-world-Z-up') {
    // Stand CAD is already in the desk frame. A proper -90° X rotation
    // converts Z-up to display Y-up without reflecting the physical part.
    for(let i=0;i<positions.length;i+=3) {
      const y=positions[i+1],z=positions[i+2];positions[i+1]=z;positions[i+2]=-y;
    }
  } else {
    // Native case CAD is X right / Y up / Z rear. Reflect display depth and
    // triangle winding together; the exported CAD meshes stay unchanged.
    for(let i=0;i<positions.length;i+=9) {
      for(let k=0;k<3;k++) positions[i+k*3+2]*=-1;
      for(let k=0;k<3;k++) {const t=positions[i+3+k];positions[i+3+k]=positions[i+6+k];positions[i+6+k]=t;}
    }
  }
  const g=new THREE.BufferGeometry();g.setAttribute('position',new THREE.BufferAttribute(positions,3));return g;
}
function inlineGeometry(p) {
  const g=new THREE.BufferGeometry(), v=Array.isArray(p.vertices[0])?p.vertices.flat():p.vertices;
  const positions=new Float32Array(v);for(let i=2;i<positions.length;i+=3)positions[i]*=-1;
  g.setAttribute('position',new THREE.BufferAttribute(positions,3));
  if(p.faces){const f=Array.isArray(p.faces[0])?p.faces.flat():p.faces;const indices=f.slice();for(let i=0;i<indices.length;i+=3)[indices[i+1],indices[i+2]]=[indices[i+2],indices[i+1]];g.setIndex(indices);}
  return g;
}
function smoothNormals(g) {
  g.computeVertexNormals();
  if(g.index) return;
  const pos=g.getAttribute('position'),norm=g.getAttribute('normal'),original=norm.array.slice(),groups=new Map(),cutoff=Math.cos(radians(42));
  for(let i=0;i<pos.count;i++) {const key=`${Math.round(pos.getX(i)*1e4)},${Math.round(pos.getY(i)*1e4)},${Math.round(pos.getZ(i)*1e4)}`;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(i);}
  for(const indices of groups.values()) if(indices.length>1) for(const i of indices) {if(Math.max(Math.abs(original[i*3]),Math.abs(original[i*3+1]),Math.abs(original[i*3+2]))>.99999)continue;let x=0,y=0,z=0;for(const j of indices) if(original[i*3]*original[j*3]+original[i*3+1]*original[j*3+1]+original[i*3+2]*original[j*3+2]>=cutoff){x+=original[j*3];y+=original[j*3+1];z+=original[j*3+2];}const l=Math.hypot(x,y,z)||1;norm.setXYZ(i,x/l,y/l,z/l);}
  norm.needsUpdate=true;
}
function dashboardTexture(portrait=false) {
  const c=document.createElement('canvas');c.width=portrait?720:960;c.height=portrait?960:720;
  const ctx=c.getContext('2d'),w=c.width,h=c.height;
  const round=(x,y,bw,bh,r,fill,stroke)=>{ctx.beginPath();ctx.roundRect(x,y,bw,bh,r);if(fill){ctx.fillStyle=fill;ctx.fill();}if(stroke){ctx.strokeStyle=stroke;ctx.lineWidth=1;ctx.stroke();}};
  const text=(s,x,y,size,color='#edf2f7',weight=400)=>{ctx.fillStyle=color;ctx.font=`${weight} ${size}px -apple-system,BlinkMacSystemFont,Arial`;ctx.fillText(s,x,y);};
  ctx.fillStyle='#080f15';ctx.fillRect(0,0,w,h);
  const grad=ctx.createLinearGradient(0,0,w,h);grad.addColorStop(0,'#0d1b24');grad.addColorStop(1,'#070d13');ctx.fillStyle=grad;ctx.fillRect(0,0,w,h);
  const pad=portrait?40:45; text('Homelab',pad,62,34,'#f3f4f6',550);text('10:24',w-pad-100,60,24,'#b7cbd8');ctx.fillStyle='#29d8ac';ctx.beginPath();ctx.arc(pad+5,94,5,0,Math.PI*2);ctx.fill();text('All systems nominal',pad+20,102,19,'#a9bcc9');
  const metrics=[['CPU','12%','#1de3d2'],['Memory','38%','#ae79ff'],['Storage','2.3 TB','#32d2ec'],['Network','312 Mbps','#c37dff']];
  const cols=portrait?1:2,rows=portrait?4:2,gap=18,top=135,bottom=h-113,cw=(w-2*pad-gap*(cols-1))/cols,ch=(bottom-top-gap*(rows-1))/rows;
  metrics.forEach(([title,value,color],i)=>{const x=pad+(i%cols)*(cw+gap),y=top+Math.floor(i/cols)*(ch+gap);round(x,y,cw,ch,17,'#101e2a','#263947');text(title,x+30,y+38,21,'#d0dbe2');text(value,x+30,y+ch*.67,portrait?35:34,'#f0f3f5',500);ctx.strokeStyle=color;ctx.lineWidth=3;ctx.beginPath();const sx=x+cw*.55,sy=y+ch*.66;for(let j=0;j<9;j++){const px=sx+j*cw*.043,py=sy-Math.sin(j*1.3+i)*ch*.06-(j>6?(j-6)*ch*.04:0);j?ctx.lineTo(px,py):ctx.moveTo(px,py);}ctx.stroke();ctx.fillStyle=color;ctx.fillRect(x+cw-18,y+19,4,16);});
  ctx.strokeStyle='#203441';ctx.beginPath();ctx.moveTo(0,h-87);ctx.lineTo(w,h-87);ctx.stroke();['Overview','Services','Logs','Settings'].forEach((s,i)=>{const x=(i+.5)*w/4,cy=h-58;ctx.fillStyle=i===0?'#25d6e0':'#849daa';if(i===0){ctx.beginPath();ctx.moveTo(x-12,cy);ctx.lineTo(x,cy-11);ctx.lineTo(x+12,cy);ctx.lineTo(x+8,cy);ctx.lineTo(x+8,cy+12);ctx.lineTo(x-8,cy+12);ctx.lineTo(x-8,cy);ctx.fill();}else{ctx.strokeStyle=ctx.fillStyle;ctx.lineWidth=3;ctx.strokeRect(x-9,cy-8,18,20);}ctx.textAlign='center';text(s,x,h-19,17,i===0?'#25d6e0':'#92a7b4');ctx.textAlign='left';});
  const t=new THREE.CanvasTexture(c);t.colorSpace=THREE.SRGBColorSpace;t.anisotropy=8;return t;
}
const screenTextures={landscape:dashboardTexture(false),portrait:dashboardTexture(true)};
function screenUV(g, portrait=false) {
  g.computeBoundingBox();const b=g.boundingBox,p=g.getAttribute('position'),a=new Float32Array(p.count*2),w=b.max.x-b.min.x||1,h=b.max.y-b.min.y||1;
  for(let i=0;i<p.count;i++){const u=(p.getX(i)-b.min.x)/w,v=(p.getY(i)-b.min.y)/h;a[i*2]=portrait?v:u;a[i*2+1]=portrait?1-u:v;}
  g.setAttribute('uv',new THREE.BufferAttribute(a,2));
}
function partMaterial(p) {
  const role=(p.role||'body').toLowerCase();
  if(['screen','display'].includes(role)) return new THREE.MeshBasicMaterial({color:'#ffffff',map:screenTextures.landscape,side:THREE.DoubleSide,toneMapped:false,polygonOffset:true,polygonOffsetFactor:-1,polygonOffsetUnits:-1});
  const glass=role==='glass',metal=['screw','fastener','metal'].includes(role),reference=p.reference||['battery','speaker','pcb','connector','hardware'].includes(role);
  return new THREE.MeshPhysicalMaterial({color:p.color||(glass?'#07090b':reference?'#405965':'#8251aa'),roughness:glass?.17:metal?.34:.62,metalness:metal?.65:0,clearcoat:glass?.6:.1,clearcoatRoughness:.35,side:THREE.DoubleSide,transparent:role==='reserve',opacity:role==='reserve'?.23:1,depthWrite:role!=='reserve',wireframe:role==='reserve'});
}
async function loadPart(p) {
  let g;
  if(p.vertices)g=inlineGeometry(p);else{const r=await fetch(p.file,{cache:'no-store'});if(!r.ok)throw Error(`${p.name}: ${r.status}`);g=stlGeometry(await r.arrayBuffer(),p.coordinate_frame);}
  smoothNormals(g);g.computeBoundingBox();if(['screen','display'].includes(p.role))screenUV(g);
  const mesh=new THREE.Mesh(g,partMaterial(p));mesh.name=p.name;mesh.castShadow=!['screen','display','reserve'].includes(p.role);mesh.receiveShadow=p.role!=='reserve';
  const group=new THREE.Group();group.add(mesh);scene.add(group);
  parts.push({record:p,mesh,group,baseTranslation:p.translation||[0,0,0],baseRotation:p.rotation||[0,0,0]});
}
function poseObject(obj,pose={}) {
  const r=pose.rotation||[0,0,0],t=pose.translation||pose.position||[0,0,0];
  const factor=pose.radians?1:Math.PI/180;obj.rotation.set(-r[0]*factor,-r[1]*factor,r[2]*factor);obj.position.copy(toWorld(t));
}
function viewConfig(){return manifest.views?.[state.mode]||{};}
function partVisible(p) {
  const r=p.record,v=viewConfig();if(hidden.has(r.name))return false;
  const orientation=state.mode==='portrait'?'portrait':'landscape';
  if(r.accessory_config&&r.accessory_config!=='cap'&&(state.accessories!==r.accessory_config||r.mount_orientation!==orientation))return false;
  if(r.accessory_config==='cap'&&state.accessories!=='capped'&&r.mount_orientation===orientation)return false;
  if(r.view&&r.view!==state.mode)return false;
  if(r.views&&!r.views.includes(state.mode))return false;
  if(Array.isArray(v.visible)&&!v.visible.includes(r.name)&&!v.visible.includes(r.group))return false;
  if(Array.isArray(v.hidden)&&(v.hidden.includes(r.name)||v.hidden.includes(r.group)))return false;
  if(state.inspect&&['body','shell','rear','cover','battery-cover','accessory','accessory-cap'].includes(r.role))return false;
  if(r.inspection_only&&!state.inspect&&state.explode===0)return false;
  return true;
}
function updateGeometry() {
  const v=viewConfig(),defaults=state.mode==='portrait'?{rotation:[18,0,-90]}:state.mode==='handheld'?{rotation:[0,0,0]}:{rotation:[18,0,0]};
  for(const p of parts) {
    const r=p.record;p.mesh.visible=partVisible(p);
    const pose=r.poses?.[state.mode]||v.parts?.[r.name]||(r.fixed?{}:v.pose||defaults);poseObject(p.group,pose);
    p.mesh.position.copy(toWorld(p.baseTranslation));const rot=p.baseRotation;p.mesh.rotation.set(-radians(rot[0]),-radians(rot[1]),radians(rot[2]));
    const e=r.explode||[0,0,0];p.mesh.position.addScaledVector(toWorld(e),state.explode);
    p.mesh.material.clippingPlanes=state.cutaway&&['body','shell','rear','cover','stand','base','battery-cover'].includes(r.role)?[clip]:[];
    if(['screen','display'].includes(r.role)){const portrait=state.mode==='portrait';p.mesh.material.map=portrait?screenTextures.portrait:screenTextures.landscape;screenUV(p.mesh.geometry,portrait);}
  }
  scene.updateMatrixWorld(true);
  const b=visibleBounds();floor.position.y=state.mode==='print'?0:(Number.isFinite(b.min.y)?b.min.y-.12:0);
}
function visibleBounds(){const b=new THREE.Box3();for(const p of parts)if(p.mesh.visible&&p.record.role!=='reserve')b.expandByObject(p.mesh);return b;}
function frame(resetZoom=true) {
  updateGeometry();const b=visibleBounds();if(b.isEmpty())return;target=b.getCenter(new THREE.Vector3());const size=b.getSize(new THREE.Vector3()),extent=Math.max(size.x,size.y,size.z);
  const radius=b.getBoundingSphere(new THREE.Sphere()).radius,vertical=radians(camera.fov),horizontal=2*Math.atan(Math.tan(vertical/2)*camera.aspect);
  fitDistance=radius/Math.sin(Math.min(vertical,horizontal)/2)*1.12;camera.near=.1;camera.far=Math.max(2000,extent*30);if(resetZoom)state.zoom=1;
  clip.constant=target.x;keyLight.target.position.copy(target);keyLight.position.set(target.x-extent*2,target.y+extent*3,target.z+extent*3);rimLight.position.set(target.x+extent*3,target.y+extent*2,target.z-extent*2);
  if(state.camera==='bezel'||state.camera==='back-detail'){const front=state.camera==='bezel',part=parts.find(p=>p.record.name===(front?'purple-body':'purple-rear-cover'));if(part){target=part.mesh.localToWorld(new THREE.Vector3(34,22,front?-.3:-32.15));fitDistance=65;}}
  Object.assign(keyLight.shadow.camera,{left:-extent*2,right:extent*2,top:extent*2,bottom:-extent*2,near:.1,far:extent*15});keyLight.shadow.camera.updateProjectionMatrix();
}
function requestRender(){if(pending||!ready)return;pending=true;requestAnimationFrame(()=>{pending=false;updateGeometry();const d=fitDistance*state.zoom;camera.position.set(target.x+d*Math.sin(state.phi)*Math.sin(state.theta),target.y+d*Math.cos(state.phi),target.z+d*Math.sin(state.phi)*Math.cos(state.theta));camera.lookAt(target);camera.updateProjectionMatrix();renderer.render(scene,camera);$('#status').textContent=state.explode>0?'Exploded CAD':(state.mode==='print'?'Rear print face at the bed plane · exported CAD':'Symmetric screw mounts · two shells');});}
function partLabel(p){return p.record.label||p.record.name;}
function selectPart(p) {
  if(selected?.mesh.material.emissive)selected.mesh.material.emissive.set('#000000');selected=p;
  if(p?.mesh.material.emissive){p.mesh.material.emissive.set('#7a47a0');p.mesh.material.emissiveIntensity=.14;}
  $('#part-name').textContent=p?partLabel(p):'Portable enclosure assembly';
  $('#part-description').textContent=p?(p.record.description||(p.record.printable===false?'Hardware reference · not a printed part':'Separate printable component')):'Exported enclosure geometry';
  const link=$('#selected-download');link.hidden=!(p&&(p.record.print_file||p.record.file)&&p.record.printable!==false);if(!link.hidden){const file=p.record.print_file||p.record.file;link.href=file;link.download=file.split('/').pop();}
  const step=$('#selected-step');step.hidden=link.hidden;if(!step.hidden){step.href='../cad/'+link.download.replace(/\.stl$/i,'.step');step.download=link.download.replace(/\.stl$/i,'.step');}
  document.querySelectorAll('.part-row button').forEach(b=>b.classList.toggle('selected',b.dataset.part===p?.record.name));requestRender();
}
function makeLegend() {
  const container=$('#part-list');container.replaceChildren();
  for(const p of parts){const r=p.record,row=document.createElement('div');row.className='part-row';const input=document.createElement('input');input.type='checkbox';input.checked=!hidden.has(r.name);input.setAttribute('aria-label',`Show ${r.label||r.name}`);input.addEventListener('change',()=>{input.checked?hidden.delete(r.name):hidden.add(r.name);state.inspect=false;$('#inspect').setAttribute('aria-pressed','false');if(selected===p&&!input.checked)selectPart(null);requestRender();});const b=document.createElement('button');b.dataset.part=r.name;b.title=r.description||r.name;const sw=document.createElement('span');sw.className='swatch';sw.style.background=r.color||'#8251aa';const label=document.createElement('span');label.className='part-title';label.textContent=r.label||r.name;b.append(sw,label);if(r.printable===false){const ref=document.createElement('span');ref.className='reference';ref.textContent='REF';b.append(ref);}b.addEventListener('click',()=>selectPart(p));row.append(input,b);container.append(row);}
}
function dimensions() {
  const d=manifest.dimensions||{},el=$('#dimensions');el.replaceChildren();
  for(const [k,v] of Object.entries(d)){const row=document.createElement('div'),dt=document.createElement('dt'),dd=document.createElement('dd');dt.textContent=k.replaceAll('_',' ');dd.textContent=Array.isArray(v)?v.map(x=>Number(x).toFixed(1).replace(/\.0$/,'')).join(' × ')+' mm':String(v);row.append(dt,dd);el.append(row);}
  if(manifest.fitNote)$('#fit-note').textContent=manifest.fitNote;
  if(manifest.notes){const el=$('#notes');el.replaceChildren();const ul=document.createElement('ul');for(const n of manifest.notes){const li=document.createElement('li');li.textContent=n;ul.append(li);}el.append(ul);}
  if(manifest.downloads){const map={body_stl:'#body-stl',body_step:'#body-step',project:'#print-project',rear_stl:'#rear-stl',rear_step:'#rear-step',drawer_stl:'#drawer-stl',drawer_step:'#drawer-step',landscape_stl:'#landscape-stl',landscape_step:'#landscape-step',portrait_stl:'#portrait-stl',portrait_step:'#portrait-step',source:'#body-source',stand_source:'#stand-source',complete_stls:'#complete-stls'};for(const [k,id]of Object.entries(map)){if(!$(id))continue;if(manifest.downloads[k])$(id).href=manifest.downloads[k];else $(id).setAttribute('aria-disabled','true');}}
}
let keyLight,rimLight;
async function init() {
  try {
    const res=await fetch('scene.json',{cache:'no-store'});if(!res.ok)throw Error('The CAD manifest is not available yet');manifest=await res.json();
    renderer=new THREE.WebGLRenderer({antialias:true,alpha:true,preserveDrawingBuffer:true});renderer.setPixelRatio(Math.min(devicePixelRatio,2));renderer.outputColorSpace=THREE.SRGBColorSpace;renderer.toneMapping=THREE.ACESFilmicToneMapping;renderer.toneMappingExposure=.8;renderer.shadowMap.enabled=true;renderer.shadowMap.type=THREE.PCFSoftShadowMap;renderer.localClippingEnabled=true;stage.prepend(renderer.domElement);
    camera=new THREE.PerspectiveCamera(34,1,.1,2000);scene=new THREE.Scene();scene.add(new THREE.HemisphereLight('#ffffff','#cac3bc',1.5));
    keyLight=new THREE.DirectionalLight('#fff7f1',2.1);keyLight.castShadow=true;keyLight.shadow.mapSize.set(2048,2048);keyLight.shadow.bias=-.0001;keyLight.shadow.normalBias=.05;keyLight.shadow.radius=4;scene.add(keyLight,keyLight.target);rimLight=new THREE.DirectionalLight('#ffffff',1.5);scene.add(rimLight);
    floor=new THREE.Mesh(new THREE.PlaneGeometry(3000,3000),new THREE.ShadowMaterial({opacity:.16}));floor.rotation.x=-Math.PI/2;floor.receiveShadow=true;scene.add(floor);
    // Sequential mesh reads keep local-server connections bounded and avoid
    // intermittent cached-response failures in the embedded WebKit browser.
    for(const part of manifest.parts)await loadPart(part);
    parts.sort((a,b)=>manifest.parts.findIndex(p=>p.name===a.record.name)-manifest.parts.findIndex(p=>p.name===b.record.name));makeLegend();dimensions();ready=true;$('#loading').hidden=true;stage.dataset.ready='true';
    let triangles=0;for(const p of parts)triangles+=p.mesh.geometry.index?p.mesh.geometry.index.count/3:p.mesh.geometry.getAttribute('position').count/3;const printableFiles=new Set(parts.filter(p=>p.record.printable!==false).map(p=>p.record.print_file||p.record.file));$('#mesh-stats').textContent=printableFiles.size+' printable parts · illustrative hardware envelopes';
    function resize(){renderer.setSize(stage.clientWidth,stage.clientHeight,false);camera.aspect=stage.clientWidth/stage.clientHeight;frame(false);requestRender();}new ResizeObserver(resize).observe(stage);resize();
    bindControls();
    window.portableReview={diagnostics:()=>({ready,mode:state.mode,accessories:state.accessories,camera:state.camera,bodyVertices:parts.find(p=>p.record.name==='purple-body').mesh.geometry.getAttribute('position').count,printableParts:printableFiles.size,explode:state.explode,inspect:state.inspect,cutaway:state.cutaway,revision:manifest.revision,bodyWorldBounds:(()=>{const b=new THREE.Box3().setFromObject(parts.find(p=>p.record.name==='purple-body').mesh);return{min:b.min.toArray(),max:b.max.toArray()};})(),visible:parts.filter(p=>p.mesh.visible).map(p=>p.record.name),parts:parts.length,triangles,bodyPose:viewConfig().pose||null}),manifest,setMode,setAccessories,setCamera,setLighting:values=>{if(values.exposure!==undefined)renderer.toneMappingExposure=values.exposure;if(values.key!==undefined)keyLight.intensity=values.key;if(values.rim!==undefined)rimLight.intensity=values.rim;if(values.fill!==undefined)scene.children.find(x=>x.isHemisphereLight).intensity=values.fill;requestRender();},select:name=>selectPart(parts.find(p=>p.record.name===name)),screenshot:()=>renderer.domElement.toDataURL('image/png')};
    const query=new URLSearchParams(location.search),initial=query.get('view');if(['landscape','portrait','handheld'].includes(initial))setMode(initial);setAccessories(['ears','antennas','eyes'].includes(query.get('accessories'))?query.get('accessories'):'capped');if(['bezel','back-detail','rear'].includes(query.get('camera')))setCamera(query.get('camera'));
  } catch(e){$('#loading').textContent=`Could not load the model: ${e.message}`;$('#loading').setAttribute('role','alert');stage.dataset.error=e.message;console.error(e);}
}
function setMode(mode){if(!['landscape','portrait','handheld'].includes(mode))return;state.mode=mode;state.camera='whole';document.querySelectorAll('[data-camera=bezel],[data-camera=back-detail]').forEach(b=>b.setAttribute('aria-pressed','false'));state.inspect=false;state.explode=0;state.cutaway=false;$('#explode').value=0;$('#explode-value').textContent='0%';$('#cutaway').checked=false;$('#inspect').setAttribute('aria-pressed','false');document.querySelectorAll('[data-mode]').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.mode===mode)));$('#view-label').textContent={landscape:'Landscape cradle',portrait:'Portrait cradle',handheld:'Handheld enclosure',print:'Rear-down · glass face up'}[mode];state.theta=.43;state.phi=mode==='print'?1.05:1.33;$('#inspect').disabled=mode==='print';$('#explode').disabled=mode==='print';$('#cutaway').disabled=mode==='print';selectPart(null);frame();requestRender();}
function setAccessories(value){
  if(!['ears','antennas','eyes','capped'].includes(value))return;
  state.accessories=value;
  document.querySelectorAll('[data-accessories]').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.accessories===value)));
  if(selected&&!partVisible(selected))selectPart(null);
  frame(false);requestRender();
}
function setCamera(value){
  state.camera=['bezel','back-detail'].includes(value)?value:'whole';
  document.querySelectorAll('[data-camera=bezel],[data-camera=back-detail]').forEach(b=>b.setAttribute('aria-pressed',String(state.camera===b.dataset.camera)));
  if(value==='bezel'||value==='back-detail'){state.theta=value==='bezel'?.65:Math.PI-.65;state.phi=1.18;frame();}
  else {frame();if(value==='reset'){state.theta=.43;state.phi=1.33;}if(value==='front'){state.theta=0;state.phi=Math.PI/2;}if(value==='rear'){state.theta=Math.PI-.35;state.phi=1.32;}if(value==='side'){state.theta=Math.PI/2;state.phi=Math.PI/2;}if(value==='contact'){state.theta=.4;state.phi=2.8;}}
  requestRender();
}
function bindControls(){
  document.querySelectorAll('[data-accessories]').forEach(b=>b.addEventListener('click',()=>setAccessories(b.dataset.accessories)));
  document.querySelectorAll('[data-mode]').forEach(b=>b.addEventListener('click',()=>setMode(b.dataset.mode)));
  document.querySelectorAll('[data-camera]').forEach(b=>b.addEventListener('click',()=>setCamera(b.dataset.camera)));
  $('#explode').addEventListener('input',e=>{state.explode=Number(e.target.value);$('#explode-value').textContent=Math.round(state.explode*100)+'%';state.inspect=false;$('#inspect').setAttribute('aria-pressed','false');frame(false);requestRender();});
  $('#inspect').addEventListener('click',()=>{state.inspect=!state.inspect;$('#inspect').setAttribute('aria-pressed',String(state.inspect));state.theta=Math.PI-.48;state.phi=1.22;frame(false);requestRender();});
  $('#cutaway').addEventListener('change',e=>{state.cutaway=e.target.checked;requestRender();});
  const pointers=new Map();let start,moved=false,pinch=0;
  const distance=()=>{const a=[...pointers.values()];return a.length>1?Math.hypot(a[0].x-a[1].x,a[0].y-a[1].y):0;};
  stage.addEventListener('pointerdown',e=>{if(e.button!==0)return;pointers.set(e.pointerId,{x:e.clientX,y:e.clientY});stage.setPointerCapture(e.pointerId);start={x:e.clientX,y:e.clientY};moved=false;pinch=distance();stage.classList.add('dragging');});
  stage.addEventListener('pointermove',e=>{const old=pointers.get(e.pointerId);if(!old)return;const dx=e.clientX-old.x,dy=e.clientY-old.y;pointers.set(e.pointerId,{x:e.clientX,y:e.clientY});if(pointers.size>1){const d=distance();if(pinch>0)state.zoom=Math.max(.35,Math.min(3,state.zoom*pinch/d));pinch=d;moved=true;}else{state.theta-=dx*.007;state.phi=Math.max(.12,Math.min(Math.PI-.12,state.phi+dy*.007));if(Math.hypot(e.clientX-start.x,e.clientY-start.y)>4)moved=true;}requestRender();});
  stage.addEventListener('pointerup',e=>{if(!moved&&pointers.size===1){const rect=renderer.domElement.getBoundingClientRect(),ray=new THREE.Raycaster(),uv=new THREE.Vector2((e.clientX-rect.left)/rect.width*2-1,-(e.clientY-rect.top)/rect.height*2+1);ray.setFromCamera(uv,camera);const hits=ray.intersectObjects(parts.filter(p=>p.mesh.visible).map(p=>p.mesh));selectPart(hits.length?parts.find(p=>p.mesh===hits[0].object):null);}pointers.delete(e.pointerId);if(!pointers.size)stage.classList.remove('dragging');pinch=distance();});
  stage.addEventListener('pointercancel',e=>{pointers.delete(e.pointerId);if(!pointers.size)stage.classList.remove('dragging');});stage.addEventListener('wheel',e=>{e.preventDefault();state.zoom=Math.max(.35,Math.min(3,state.zoom*Math.exp(e.deltaY*.001)));requestRender();},{passive:false});
  stage.addEventListener('keydown',e=>{const k={ArrowLeft:()=>state.theta-=.1,ArrowRight:()=>state.theta+=.1,ArrowUp:()=>state.phi=Math.max(.12,state.phi-.1),ArrowDown:()=>state.phi=Math.min(Math.PI-.12,state.phi+.1),'+':()=>state.zoom=Math.max(.35,state.zoom*.9),'=':()=>state.zoom=Math.max(.35,state.zoom*.9),'-':()=>state.zoom=Math.min(3,state.zoom*1.1),Home:()=>setCamera('reset')};if(k[e.key]){e.preventDefault();k[e.key]();requestRender();}});
}

init();
