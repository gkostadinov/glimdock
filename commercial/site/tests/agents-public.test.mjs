import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import worker from '../dist/server/index.js';
const read=path=>readFileSync('public/'+path,'utf8');
test('public agent demo contains only synthetic source data and no telemetry connection',()=>{
  const sample=JSON.parse(read('agents/demo/snapshot.json'));
  assert.equal(sample.demo,true);assert.equal(sample.schema_version,'agent-ops/v1');
  assert(Buffer.byteLength(JSON.stringify(sample))<=49152);
  assert.equal(sample.systems.length,2);assert.equal(sample.agents.length,15);
  const index=read('agents/demo/index.html'),script=read('demo/agent-data.js');
  assert(index.includes('data-public-demo="true"'));
  assert(index.includes('/demo/app.js'));
  assert(script.includes('createOperationsDemo'));
  assert(script.includes('structuredClone(fixture)'));
  assert(!script.includes('fetch('));
  assert(!/10\.0\.0\.|192\.168\.|172\.16\.|\/Users\//.test(index+script+JSON.stringify(sample)));
});
test('public artifacts retain source fingerprints and exact copyable prompts',()=>{
  const provenance=JSON.parse(read('agents/demo/provenance.json'));
  for(const [path,digest] of Object.entries(provenance.files)){
    assert.equal(createHash('sha256').update(readFileSync('public/'+path)).digest('hex'),digest,path);
  }
  const prompts=read('docs/source/AGENT-PROMPTS.md').matchAll(/```text\n([\s\S]*?)\n```/g);
  const names=['adapter','collector','firmware','review'];
  [...prompts].forEach((match,index)=>assert.equal(read('docs/agents-api/prompts/'+names[index]+'.txt'),match[1]+'\n'));
});
test('agent frame permits the actual WASM UI and same-origin assets only',async()=>{
  const response=await worker.fetch(new Request('https://glimdock.com/agents/demo/'),{},{});
  assert.equal(response.status,200);
  const csp=response.headers.get('content-security-policy');
  assert(csp.includes("frame-ancestors 'self'"));assert(csp.includes("connect-src 'self';"));
  assert(csp.includes('wasm-unsafe-eval'));assert(!csp.includes('googletagmanager'));
  assert(csp.includes("form-action 'none'"));
});
test('developer pages expose accurate status, schemas and prompts',async()=>{
  for(const route of ['/agents/','/docs/','/docs/agents-api/','/updates/','/hardware/']){
    const response=await worker.fetch(new Request('https://glimdock.com'+route),{},{});
    assert.equal(response.status,200,route);
    const html=await response.text();assert.equal((html.match(/<h1\b/g)||[]).length,1);
    assert(html.includes('https://glimdock.com'+route));assert(!html.includes('10.0.0.'));
  }
  const docs=read('docs/agents-api/index.html');assert(docs.includes('The Rust collector reads a normalized operations source'));
  assert.equal((docs.match(/data-copy=/g)||[]).length,4);
});

test('regular consented Google tag images are allowed while demo images remain local',async()=>{
 const regular=await worker.fetch(new Request('https://glimdock.com/'),{},{});
 const imagePolicy=regular.headers.get('content-security-policy').split(';').find(p=>p.trim().startsWith('img-src'));
 assert(imagePolicy.includes('https://www.googletagmanager.com'));
 const frame=await worker.fetch(new Request('https://glimdock.com/agents/demo/'),{},{});
 const frameImages=frame.headers.get('content-security-policy').split(';').find(p=>p.trim().startsWith('img-src'));
 assert.equal(frameImages.trim(),"img-src 'self' data:");
});
