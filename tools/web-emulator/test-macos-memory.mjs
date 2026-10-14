// Production parser + LVGL UI; only synthetic macOS host/guest readings.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdtemp, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const assets = path.resolve(process.env.GLIMDOCK_TEST_EMULATOR || path.join(root, 'collector-web/emulator'));
const temporary = await mkdtemp(path.join(tmpdir(), 'glimdock-memory-test-'));
try {
  await writeFile(path.join(temporary, 'firmware.mjs'), await readFile(path.join(assets, 'firmware.js')));
  const {default:createFirmware} = await import(pathToFileURL(path.join(temporary, 'firmware.mjs')));
  const f = await createFirmware({locateFile:name=>path.join(assets,name)});
  const call = (name,...args)=>f.ccall(name,'number',args.map(v=>typeof v==='string'?'string':'number'),args);
  const labels = ()=>JSON.parse(f.UTF8ToString(f._glimdock_labels()));
  const texts = ()=>labels().map(l=>l.text);
  const GiB=2**30,stamp=Date.UTC(2026,9,14,12)/1000;
  f._glimdock_time(stamp); let tick=1000;
  const advance=()=>f._glimdock_tick(tick+=600);
  const mac={id:'test:mac',type:'server',platform:'macos',name:'Example Mac',address:'192.0.2.10',status:'healthy'};
  const pve={id:'test:pve',type:'proxmox',platform:'linux',name:'Example PVE',address:'192.0.2.20',status:'healthy'};
  const memory={memory_basis:'macos-native',mem_used_bytes:9*GiB,mem_total_bytes:20*GiB,mem_available_bytes:11*GiB,mem_app_bytes:6*GiB,mem_wired_bytes:2*GiB,mem_compressed_bytes:GiB,mem_cache_bytes:5*GiB,memory_pressure:'normal',swap_used_bytes:0,swap_total_bytes:0};
  const doc={schema:1,demo:true,sequence:1,generated_at:stamp,node:mac,nodes:[mac,pve],host:{name:'Example Mac',ip:'192.0.2.10',...memory},sources:{memory:{enabled:true,ok:true,updated_at:stamp,age_s:0}}};
  assert.equal(call('glimdock_snapshot',JSON.stringify(doc)),1);advance();
  f._glimdock_show(10);advance();
  for(const t of ['Used','Total','Memory pressure','Normal','Compressed','App memory ~','Wired','Cached files','Swap used','0 B'])assert(texts().includes(t),t);
  assert(!texts().includes('ZFS ARC'),'Mac details omit unsupported ZFS ARC');
  doc.power={package_w:6.4,cpu_temp_c:54,cpu_power_basis:'macos-ioreport-estimate'};doc.sequence++;assert.equal(call('glimdock_snapshot',JSON.stringify(doc)),1);advance();f._glimdock_show(5);advance();assert(texts().includes('CPU power ~'));assert(texts().some(t=>t.includes('IOReport estimate')));

  const guest={id:103,name:'Example macOS VM',type:'vm',status:'running',...memory,memory_basis:'guest-os-macos',mem_assigned_bytes:20*GiB,pve_mem_used_bytes:20*GiB,pve_mem_total_bytes:20*GiB,mem_updated_at:stamp,mem_age_s:0};
  doc.node=pve;doc.host={name:'Example PVE',ip:'192.0.2.20'};doc.guests=[guest];doc.sequence++;
  assert.equal(call('glimdock_snapshot',JSON.stringify(doc)),1);advance();f._glimdock_show(1);advance();
  assert(texts().some(t=>t.includes('9.0 / 20.0 GiB')),'guest list shows OS memory rather than allocation');
  assert(texts().some(t=>t.includes('macOS RAM')&&t.includes('Normal')),'guest list shows native pressure');
  f._glimdock_show(4);advance();
  for(const t of ['Memory pressure','Normal','Compressed','App memory ~','Assigned RAM','Proxmox RAM'])assert(texts().includes(t),t);
  guest.mem_used_bytes=null;guest.mem_total_bytes=null;guest.memory_pressure=null;guest.mem_error='Native agent feed expired or unavailable';doc.sequence++;
  assert.equal(call('glimdock_snapshot',JSON.stringify(doc)),1);advance();f._glimdock_show(4);advance();
  assert(texts().includes('Unavailable'));assert(texts().includes(guest.mem_error),'explicit source error remains visible');
  doc.node=mac;doc.host={name:'Example Mac',ip:'192.0.2.10',...memory};doc.guests=[];doc.sources={};for(let i=0;i<18;i++)doc.sources[`source-${i}`]={enabled:true,ok:true,updated_at:stamp,age_s:0};doc.sequence++;assert.equal(call('glimdock_snapshot',JSON.stringify(doc)),1);advance();f._glimdock_show(6);advance();assert(texts().includes('source-17'),'native host keeps all eighteen source statuses');
  console.log(JSON.stringify({passed:true,viewport:[f._glimdock_width(),f._glimdock_height()],checks:['native host accounting','kernel pressure','compression and swap zero','guest OS versus Proxmox allocation','missing feed error']}));
} finally {await rm(temporary,{recursive:true,force:true});}
