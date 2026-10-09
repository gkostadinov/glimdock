// Run the actual WASM/LVGL firmware in Node, including continuous touch events.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdtemp, mkdir, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const assets = path.join(root, 'collector-web/emulator');
const temporary = await mkdtemp(path.join(tmpdir(), 'glimdock-wasm-test-'));
try {
  // .mjs lets Node consume the browser ES module without changing repo tooling.
  await writeFile(path.join(temporary, 'firmware.mjs'), await readFile(path.join(assets, 'firmware.js')));
  const {default: createFirmware} = await import(pathToFileURL(path.join(temporary, 'firmware.mjs')));
  const firmware = await createFirmware({locateFile: name => path.join(assets,name)});
  const call = (name, ...args) => firmware.ccall(name, 'number', args.map(v => typeof v === 'string' ? 'string' : 'number'), args);
  const labels = () => JSON.parse(firmware.UTF8ToString(firmware._glimdock_labels()));
  const texts = () => labels().map(label => label.text);
  let clock = 1000;
  const tick = (delta = 40) => { clock += delta; firmware._glimdock_tick(clock); };
  const requests = [];
  firmware.onRequest = request => requests.push(request);
  const stamp = Math.floor(Date.now() / 1000);
  const mac = {id:'test:mac', type:'server', platform:'macos', name:'Test Mac', address:'127.0.0.1:8765', status:'healthy', summary:{cpu_percent:0, memory_percent:0, temperature_c:null, sensors:0, guests:0, generated_at:stamp, age_s:0, ttl_s:15, status:'healthy'}};
  const pve = {id:'test:pve', type:'proxmox', platform:'linux', name:'Test PVE', address:'127.0.0.2:8765', status:'healthy', summary:{cpu_percent:19, memory_percent:38, temperature_c:42, sensors:6, guests:3, generated_at:stamp, age_s:0, ttl_s:15, status:'healthy'}};
  const printer = {id:'test:printer', type:'klipper', platform:'other', name:'Test printer', address:'127.0.0.3:7125', status:'healthy', summary:{progress_percent:65, print_state:'printing', temperature_c:220, generated_at:stamp, age_s:0, ttl_s:15, status:'healthy'}};
  const document = {schema:1, sequence:1, generated_at:stamp, demo:false, node:mac, nodes:[mac,pve,printer], host:{name:'Test Mac',ip:'127.0.0.1',cpu_pct:0,mem_used_bytes:0,mem_total_bytes:1000,uptime_s:800}, power:{cpu_temp_c:null}, platform:{os:'macos'}, sources:{native:{enabled:true,ok:true,age_s:0,updated_at:stamp}}};
  assert.equal(firmware._glimdock_width(),320); assert.equal(firmware._glimdock_height(),240);
  assert.equal(firmware._glimdock_page(),19, 'actual firmware starts on all nodes');
  assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);
  tick(600);
  assert(texts().includes('All nodes'));
  assert(texts().some(text => text.includes('CPU 0%') && text.includes('RAM 0%')), 'zero is displayed as a valid value');
  assert(texts().some(text => text.includes('0 sensors') && text.includes('--')), 'unknown temperature stays unknown');
  assert(texts().some(text => text.includes('3 guests')), 'Proxmox keeps its additional inventory');
  assert(texts().some(text => text.includes('printing 65%')), 'printer summary is platform specific');
  const pixelPointer = firmware._glimdock_pixels() >>> 1;
  assert(new Set(firmware.HEAPU16.subarray(pixelPointer,pixelPointer+320*240)).size>100, 'LVGL rasterizer produced a real frame');
  if(process.argv[2]){
    const output=path.resolve(process.argv[2]);await mkdir(output,{recursive:true});
    await writeFile(path.join(output,'snapshot.json'),JSON.stringify(document,null,2)+'\n');
    const pixels=firmware.HEAPU16.subarray(pixelPointer,pixelPointer+320*240),rgb=new Uint8Array(320*240*3);
    for(let i=0;i<pixels.length;i++){const value=pixels[i];rgb[i*3]=Math.floor(((value>>>11)&31)*255/31);rgb[i*3+1]=Math.floor(((value>>>5)&63)*255/63);rgb[i*3+2]=Math.floor((value&31)*255/31);}
    await writeFile(path.join(output,'wasm-all-nodes.ppm'),Buffer.concat([Buffer.from('P6\n320 240\n255\n'),rgb]));
  }
  const pveLabel = labels().find(label => label.text==='Test PVE');
  assert(pveLabel);
  tick();firmware._glimdock_pointer(pveLabel.x+12,pveLabel.y+4,1);
  tick();firmware._glimdock_pointer(pveLabel.x+12,pveLabel.y+4,0);tick();
  await Promise.resolve();
  assert(requests.some(request => request.kind==='select' && request.payload==='test:pve'), 'real bento tap selects stable node ID');
  assert.equal(firmware._glimdock_page(),0,'bento selection opens selected overview');
  document.node=pve; document.host.name='Test PVE'; document.sequence++;
  assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);tick(600);
  assert.equal(call('glimdock_select','test:mac'),1,'external web overview links use the same selector');
  assert.equal(call('glimdock_select','missing:node'),0,'unregistered IDs are rejected');
  await Promise.resolve();
  document.node=mac; document.host.name='Test Mac'; document.sequence++;
  assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);tick(600);
  firmware._glimdock_show(19);tick();
  mac.summary.status='offline';mac.status='offline';mac.summary.cpu_percent=82;
  document.sequence++;assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);tick(600);
  assert(texts().some(text=>text.includes('macOS')&&text.includes('Offline')));
  assert(!texts().some(text=>text.includes('CPU 82%')),'offline summary does not show stale measurements');
  mac.status=mac.summary.status='healthy';mac.summary.generated_at=stamp-90;mac.summary.age_s=90;
  document.sequence++;assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);tick(600);
  assert(texts().some(text=>text.includes('macOS')&&text.includes('Stale')),'per-node TTL is applied');
  assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);tick(16000);
  assert.equal(call('glimdock_snapshot',JSON.stringify(document)),1);tick(600);
  assert(texts().includes('STALE'),'replayed sequence retains the original receipt time and fleet header becomes stale');
  assert.equal(call('glimdock_snapshot',JSON.stringify({...document,schema:9})),0);
  assert.equal(call('glimdock_snapshot',' '.repeat(48*1024+1)),0);
  const version='a'.repeat(64);
  const config={schema:1,version,local_node:{enabled:false,type:'server',platform:'linux'},nodes:[{id:'test:mac',type:'server',platform:'macos',origin:'feed',name:'Test Mac',url:'http://127.0.0.1:8765',poll_interval_s:3,timeout_s:2,ttl_s:15,has_secret:true}]};
  assert.equal(call('glimdock_config',JSON.stringify(config),200),1,'new host-agnostic public configuration is parsed');
  firmware._glimdock_show(15);tick();await Promise.resolve();
  assert(requests.some(request=>request.kind==='config'));
  assert.equal(call('glimdock_config',JSON.stringify({error:'Pair configuration token.'}),401),0);
  tick(600);
  assert(texts().some(text=>text.includes('Setup token rejected')),'management role error appears in production form');
  const hostConfig={schema:1,version,local_node:{enabled:true,id:'test:mac',name:'Test Mac',address:'127.0.0.1',type:'server',platform:'macos'},nodes:[{id:'test:mac',type:'server',platform:'macos',origin:'host',name:'Test Mac',poll_interval_s:3,timeout_s:2,ttl_s:15,has_secret:false}]};
  assert.equal(call('glimdock_config',JSON.stringify(hostConfig),200),1);
  firmware._glimdock_show(16);tick();
  assert(texts().includes('Server'),'host mode uses a generic server label');
  assert(texts().some(text=>text.includes('Platform: macOS')),'host editor displays the actual host platform');
  const pveMode=labels().find(label=>label.text==='Proxmox');assert(pveMode);
  tick();firmware._glimdock_pointer(pveMode.x+12,pveMode.y+4,1);tick();firmware._glimdock_pointer(pveMode.x+12,pveMode.y+4,0);tick();
  assert(texts().some(text=>text.includes('Proxmox collection requires a Linux host.')),'non-Linux host cannot enable Proxmox');
  let save;
  for(let scroll=0;scroll<=1000;scroll+=40){firmware._glimdock_scroll(scroll);save=labels().find(label=>label.text==='Save node'&&label.y>=34&&label.y+label.height<200);if(save)break;}
  assert(save,'host Save control can be reached');
  tick();firmware._glimdock_pointer(save.x+12,save.y+4,1);tick();firmware._glimdock_pointer(save.x+12,save.y+4,0);tick();await Promise.resolve();
  const hostUpdate=requests.filter(request=>request.kind==='update').at(-1);assert(hostUpdate);
  assert.equal(JSON.parse(hostUpdate.payload).node.platform,'macos','browser transport preserves the real host platform when saving');
  const manifest=JSON.parse(await readFile(path.join(assets,'manifest.json'),'utf8'));
  assert.equal(manifest.public_build,true);
  console.log(JSON.stringify({passed:true,viewport:[320,240],production_ui:true,production_parser:true,checks:['all-node startup','RGB565 rendering','zero/unknown values','platform summaries','continuous LVGL touch','stable-ID selection','external selector','offline masking','per-node freshness TTL','replayed sequence stale header','schema/payload limits','host-agnostic configuration','management role error','native host platform editor','non-Linux Proxmox rejection','host platform save']}));
} finally { await rm(temporary,{recursive:true,force:true}); }
