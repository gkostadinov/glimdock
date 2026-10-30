import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync,statSync} from 'node:fs';
import test from 'node:test';
import worker from '../dist/server/index.js';

const directory='public/hardware/viewer/';
const read=name=>readFileSync(directory+name,'utf8');
const sha=value=>createHash('sha256').update(value).digest('hex');
const commit='115aa457c2c804d5e48c1b866748d57f49429dc8';
const release=`https://raw.githubusercontent.com/gkostadinov/glimdock/${commit}/enclosure/public/portable-r45/`;
const request=path=>worker.fetch(new Request('https://glimdock.com'+path),{},{});

test('the website viewer loads the immutable current release and keeps hardware references separate',()=>{
  const scene=JSON.parse(read('scene.json'));
  assert.equal(scene.source_commit,commit);
  assert.equal(scene.parts.length,34);
  const printable=new Set();
  for(const part of scene.parts){
    assert(part.file.startsWith(release),part.name);
    assert(!part.file.includes('..'),part.name);
    if(part.printable!==false){
      assert(part.print_file.startsWith(release+'stl/'),part.name);
      assert(part.step_file.startsWith(release+'cad/'),part.name);
      assert.equal(part.step_file.split('/').pop(),part.print_file.split('/').pop().replace(/\.stl$/,'.step'));
      printable.add(part.print_file);
    }else{
      assert(part.file.startsWith(release+'viewer/reference/'),part.name);
      assert.equal(part.print_file,undefined);
      assert.equal(part.step_file,undefined);
    }
  }
  assert.equal(printable.size,23);
  assert.equal(scene.parts.filter(part=>part.printable===false).length,5);
  assert(scene.parts.some(part=>part.name==='portrait-stand'&&part.coordinate_frame==='stand-world-Z-up'));
});

test('all local viewer runtime assets are hash-bound, with no geometry embedded in the Worker',()=>{
  const provenance=JSON.parse(read('provenance.json'));
  assert.equal(provenance.source_commit,commit);
  assert.equal(provenance.geometry_embedded_in_worker,false);
  for(const [name,hash] of Object.entries(provenance.runtime_assets))assert.equal(sha(readFileSync(directory+name)),hash,name);
  const walk=dir=>readdirSync(dir).flatMap(name=>statSync(dir+name).isDirectory()?walk(dir+name+'/'):[dir+name]);
  assert(!walk(directory).some(name=>/\.(stl|step|brep|3mf)$/i.test(name)));
  assert(read('app.js').length<550_000,'minified runtime bundle');
  assert(read('THREE-LICENSE.txt').includes('Permission is hereby granted'));
});

test('viewer and file list are served with metadata, valid download targets and separate printable labels',async()=>{
  for(const [route,file] of [['/hardware/viewer/','index.html'],['/hardware/viewer/files.html','files.html']]){
    const response=await request(route),html=await response.text();
    assert.equal(response.status,200);
    assert.equal(html,read(file));
    assert.equal((html.match(/<h1\b/g)||[]).length,1);
    assert(html.includes('name="description"'));
    assert(html.includes('rel="canonical"'));
    assert(!html.includes('href="../'),file);
    assert(!html.includes('href="data:,'),file);
  }
  const html=read('index.html');
  assert(html.includes('href="/hardware/"'));
  assert(html.includes('public r45 release on GitHub'));
  assert(html.includes('Portrait firmware and battery electrical setup require separate validation'));
  const files=read('files.html');
  assert.equal((files.match(/download>STL</g)||[]).length,23);
  assert.equal((files.match(/download>STEP</g)||[]).length,23);
  assert.equal((files.match(/download>BREP</g)||[]).length,23);
});

test('only the interactive enclosure page can connect to public GitHub geometry',async()=>{
  const viewer=await request('/hardware/viewer/');
  const policy=viewer.headers.get('content-security-policy');
  assert.match(policy,/connect-src 'self' https:\/\/raw\.githubusercontent\.com;/);
  assert.match(policy,/script-src 'self';/);
  assert.match(policy,/form-action 'none'/);
  assert.match(policy,/frame-ancestors 'none'/);
  assert(!policy.includes('unsafe-eval'));
  assert(!policy.includes('googletagmanager'));
  for(const route of ['/','/hardware/','/hardware/viewer/files.html','/demo/','/agents/demo/','/flash/','/not-a-page/']){
    const response=await request(route),csp=response.headers.get('content-security-policy');
    assert(!csp.includes('raw.githubusercontent.com'),route);
    if(route==='/demo/'||route==='/agents/demo/'){
      assert(csp.includes("connect-src 'self';"));
      assert(csp.includes('wasm-unsafe-eval'));
    }else assert(!csp.includes('wasm-unsafe-eval'),route);
    if(route==='/flash/')assert(response.headers.get('permissions-policy').includes('serial=(self)'));
  }
});
