import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import worker from '../dist/server/index.js';
import {mountInstaller} from '../public/flash/installer-controller.js';
import {validateFlashManifest} from '../public/flash/flash-plan.js';

const manifest=JSON.parse(readFileSync('public/firmware/manifest.json','utf8'));
class Element{
  constructor(){this.listeners={};this.dataset={};this.disabled=false;this.checked=false;this.hidden=true;this.textContent='';this.value=0;}
  addEventListener(name,handler){this.listeners[name]=handler;}
  fire(name,event={}){return this.listeners[name]?.(event);}
}
function fixture(options={}){
  const elements=new Map();const document={getElementById:id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);}};
  const window={isSecureContext:true,listeners:{},addEventListener(name,fn){this.listeners[name]=fn;},...options.window};
  const navigator={serial:{requestPort(){}}};let calls=0;
  const view=mountInstaller({document,window,navigator,location:{origin:'https://glimdock.com'},fetcher:async()=>({ok:true,json:async()=>structuredClone(manifest)}),validateManifest:validateFlashManifest,flash:async()=>{calls++;},...options,window});
  const get=id=>document.getElementById(id);
  return {...view,get,window,calls:()=>calls,confirm:()=>{get('board-confirmed').checked=true;get('board-confirmed').fire('change');},click:()=>get('install-firmware').fire('click')};
}

test('public installer serves four manifest-verified images without collector authentication',async()=>{
  const page=await worker.fetch(new Request('https://glimdock.com/flash/'),{},{});
  assert.equal(page.status,200);const html=await page.text();assert(html.includes('Flash firmware'));assert(html.includes('No local server to install'));assert.equal((html.match(/<h1\b/g)||[]).length,1);
  const response=await worker.fetch(new Request('https://glimdock.com/firmware/manifest.json'),{},{});
  assert.equal(response.status,200);assert(response.headers.get('cache-control').includes('no-cache'));assert.deepEqual(await response.json(),manifest);
  assert.deepEqual(validateFlashManifest(manifest,'https://glimdock.com').map(p=>p.offset),[0,32768,57344,65536]);
  for(const part of manifest.parts){
    const image=await worker.fetch(new Request('https://glimdock.com'+part.path),{},{});
    assert.equal(image.status,200);assert.equal(image.headers.get('content-type'),'application/octet-stream');
    const bytes=Buffer.from(await image.arrayBuffer());assert.equal(bytes.length,part.size);assert.equal(createHash('sha256').update(bytes).digest('hex'),part.sha256);
  }
  assert.equal(manifest.source_relink,`https://github.com/gkostadinov/glimdock/releases/download/${manifest.release}/source-relink.tar.gz`);
});
test('Serial permission is limited to the standalone installer',async()=>{
  for(const [path,expected] of [['/flash/','serial=(self)'],['/','serial=()'],['/demo/','serial=()']]){
    const response=await worker.fetch(new Request('https://glimdock.com'+path),{},{});assert(response.headers.get('permissions-policy').includes(expected));
  }
});
test('unsupported browsers and insecure contexts never enable USB installation',async()=>{
  for(const options of [{navigator:{}},{window:{isSecureContext:false}}]){
    const f=fixture(options);await f.ready;f.confirm();await f.click();assert.equal(f.calls(),0);assert(f.get('install-firmware').disabled);assert.equal(f.get('install-status').dataset.state,'unsupported');
  }
});
test('installation needs both verified manifest and deliberate board confirmation',async()=>{
  const f=fixture();await f.ready;assert(f.get('install-firmware').disabled);await f.click();assert.equal(f.calls(),0);f.confirm();assert(!f.get('install-firmware').disabled);
});
test('failed or incompatible manifests cannot reach the chooser',async()=>{
  for(const fetcher of [async()=>({ok:false}),async()=>({ok:true,json:async()=>({...manifest,board:'waveshare-v2'})}),async()=>({ok:true,json:async()=>({...manifest,source_relink:'https://other.example/source.tar.gz'})})]){
    const f=fixture({fetcher});await f.ready;f.confirm();await f.click();assert(f.get('install-firmware').disabled);assert.equal(f.calls(),0);assert.equal(f.get('install-status').dataset.state,'error');
  }
});
test('a click enters the shared updater immediately and prevents duplicate programming',async()=>{
  let begin=0,finish;
  const f=fixture({flash:()=>{begin++;return new Promise(resolve=>{finish=resolve;});}});await f.ready;f.confirm();const pending=f.click();
  assert.equal(begin,1);assert(f.get('install-firmware').disabled);assert(f.get('board-confirmed').disabled);await f.click();assert.equal(begin,1);
  let prevented=false;f.window.listeners.beforeunload({preventDefault(){prevented=true;}});assert(prevented);
  finish();await pending;assert(!f.get('install-next').hidden);assert.equal(f.get('install-progress').value,100);assert.equal(f.get('install-status').dataset.state,'success');
});
test('canceling the browser port chooser is retryable without a success message',async()=>{
  const f=fixture({flash:async()=>{throw new DOMException('No port selected','NotFoundError');}});await f.ready;f.confirm();await f.click();
  assert(!f.get('install-firmware').disabled);assert(f.get('install-next').hidden);assert(f.get('install-progress').hidden);assert(f.get('install-status').textContent.includes('No port selected'));
});
test('USB and hash errors remain visible, permit retry and never report completion',async()=>{
  const f=fixture({flash:async({onStatus,onProgress,onLog})=>{onStatus('Checking');onProgress(20);onLog('<device text>');throw Error('SHA-256 verification failed.');}});await f.ready;f.confirm();await f.click();
  assert.equal(f.get('install-status').dataset.state,'error');assert(f.get('install-status').textContent.includes('SHA-256'));assert.equal(f.get('install-log').textContent,'<device text>\n');assert(f.get('install-next').hidden);assert(!f.get('install-firmware').disabled);
});
test('header, landing page and setup guide link directly to the public installer',()=>{
  for(const path of ['index.html','open/index.html','setup/index.html','docs/index.html','updates/index.html'])assert(readFileSync('public/'+path,'utf8').includes('href="/flash/"'),path);
});
