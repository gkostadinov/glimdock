import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import test from 'node:test';
import path from 'node:path';
import createFirmware from '../public/demo/emulator/firmware.js';
import {createDemoTransport,demoNodes} from '../public/demo/data.js';
import worker from '../dist/server/index.js';
const assets=path.resolve('public/demo/emulator');
const sha=value=>createHash('sha256').update(value).digest('hex');
test('website renderer is byte-identical to the collector public renderer',()=>{
  const manifest=JSON.parse(readFileSync(path.join(assets,'manifest.json'),'utf8'));
  assert.equal(manifest.public_build,true);assert.equal(manifest.width,320);assert.equal(manifest.height,240);
  for(const name of ['firmware.wasm','firmware.js','emulator.js','manifest.json','LICENSES.txt']){
    const value=readFileSync(path.join(assets,name));
    assert.equal(sha(value),sha(readFileSync(path.resolve('../../collector-web/emulator',name))),name);
    if(manifest.assets[name])assert.equal(sha(value),manifest.assets[name].sha256,name);
  }
});
test('all synthetic nodes have stable IDs, fresh sequences and documentation addresses',()=>{
  const transport=createDemoTransport(()=>1_791_551_000_000),first=transport('');
  assert.equal(first.demo,true);assert.equal(first.nodes.filter(node=>!node.parent_id).length,4);assert.equal(first.nodes.length,5);
  assert.deepEqual(first.nodes.map(node=>node.platform),['macos','linux','router','other','router']);
  assert.deepEqual(first.nodes.filter(node=>!node.parent_id).map(node=>node.id),demoNodes.map(node=>node.id));
  for(const node of first.nodes)assert.match(node.address,/^192\.0\.2\./);
  assert.equal(transport('').sequence,first.sequence+1);assert.equal(transport('unknown:node').status,404);
  assert.equal(first.power.cpu_temp_c,54);assert.equal(first.power.cpu_power_basis,'macos-ioreport-estimate');
  assert.equal(transport('demo:router').platform.interfaces.length,2);
  assert.equal(transport('demo:switch').node.parent_id,'demo:router');
  const lab=transport('demo:lab');assert.equal(lab.guests.length,4);assert.equal(lab.gpus.length,2);assert(lab.guests[0].mem_used_bytes<lab.guests[0].mem_total_bytes);assert.equal(lab.guests[0].memory_basis,'guest-os');assert.equal(lab.guests.find(g=>g.memory_basis==='guest-os-macos').memory_pressure,'normal');assert.equal(first.host.memory_basis,'macos-native');
  const app=readFileSync('public/demo/app.js','utf8'),data=readFileSync('public/demo/data.js','utf8');
  assert(!/\bfetch\s*\(|XMLHttpRequest|WebSocket/.test(app+data),'demo must not request telemetry from a device');
  assert(!/requestConfig:|updateConfig:/.test(app),'public website cannot write collector settings');
});
test('production parser and LVGL draw all four platform cards and printer readings',async()=>{
  const firmware=await createFirmware({locateFile:name=>path.join(assets,name)});
  const call=(name,...args)=>firmware.ccall(name,'number',args.map(value=>typeof value==='string'?'string':'number'),args);
  const texts=()=>JSON.parse(firmware.UTF8ToString(firmware._glimdock_labels())).map(label=>label.text);
  const transport=createDemoTransport();
  assert.equal(call('glimdock_snapshot',JSON.stringify(transport(''))),1);firmware._glimdock_tick(1600);
  assert.equal(firmware._glimdock_page(),19);
  for(const name of demoNodes.map(node=>node.name))assert(texts().includes(name),name);
  for(const platform of ['macOS','Proxmox','Router'])assert(texts().some(text=>text.includes(platform)),platform);
  assert(texts().some(text=>text.includes('printing 68%')),'Klipper summary uses the real parser field');
  const pixels=firmware.HEAPU16.subarray(firmware._glimdock_pixels()>>>1,(firmware._glimdock_pixels()>>>1)+320*240);
  assert(new Set(pixels).size>100,'real LVGL frame');
  assert.equal(call('glimdock_select','demo:printer'),1,'use the same stable-ID selector as the live collector');
  firmware._glimdock_tick(1700);
  assert.equal(call('glimdock_snapshot',JSON.stringify(transport('demo:printer'))),1);
  firmware._glimdock_show(0);firmware._glimdock_tick(2300);
  assert(texts().some(text=>text.includes('68')),'printer page shows parsed progress');
  assert(texts().some(text=>text.includes('215')),'printer page shows parsed heater temperature');
});
test('the demo alone permits WebAssembly and same-origin embedding',async()=>{
  const root=await worker.fetch(new Request('https://glimdock.com/'),{},{});
  const demo=await worker.fetch(new Request('https://glimdock.com/demo/'),{},{});
  const wasm=await worker.fetch(new Request('https://glimdock.com/demo/emulator/firmware.wasm'),{},{});
  const mainCsp=root.headers.get('content-security-policy'),demoCsp=demo.headers.get('content-security-policy');
  assert.equal(root.status,200);assert.equal(demo.status,200);assert.equal(wasm.status,200);
  assert(!mainCsp.includes('wasm-unsafe-eval'));assert.match(demoCsp,/script-src 'self' 'wasm-unsafe-eval'/);
  assert.match(demoCsp,/frame-ancestors 'self'/);assert.match(demoCsp,/connect-src 'self';/);assert.match(demoCsp,/form-action 'none'/);
  assert.equal(wasm.headers.get('content-type'),'application/wasm');
});
