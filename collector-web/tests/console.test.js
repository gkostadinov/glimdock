import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';

const html=await readFile(new URL('../index.html',import.meta.url),'utf8');
class Element {
  constructor(id=''){this.id=id;this.value='';this.textContent='';this.innerHTML='';this.hidden=false;this.disabled=false;this.checked=false;this.open=false;this.dataset={};this.listeners={};this.firstChild={textContent:''};this.options={};this.classList={toggle(){}};}
  addEventListener(type,handler){(this.listeners[type]??=[]).push(handler);}
  async emit(type,event={}){for(const handler of this.listeners[type]||[])await handler({preventDefault(){},...event});}
  showModal(){this.open=true;}
  close(){this.open=false;return this.emit('close');}
  reset(){}
  focus(){}
  querySelector(key){return this.options[key]??=new Element();}
  click(){this.clicked=true;}
}
const settle=async()=>{for(let i=0;i<3;i++)await new Promise(resolve=>setImmediate(resolve));};
async function consoleFixture({bridge=false,displayValid=true,managementValid=true,nodes=[],config={version:'v1',max_nodes:4,nodes:[]}}={}){
  const elements=Object.fromEntries([...html.matchAll(/id="([^"]+)"/g)].map(match=>[match[1],new Element(match[1])]));
  const body=new Element('body'),requests=[],created=[];
  globalThis.document={getElementById:id=>{assert.ok(elements[id],`DOM id ${id} exists`);return elements[id];},querySelectorAll:()=>[],body,createElement:()=>{const el=new Element();created.push(el);return el;}};
  globalThis.location={origin:'http://127.0.0.1:8766',hash:'#nodes'};
  globalThis.history={replaceState(){}};
  globalThis.window={isSecureContext:true,addEventListener(){}};
  Object.defineProperty(globalThis,'navigator',{configurable:true,value:{clipboard:{writeText:async()=>{}}}});
  globalThis.setInterval=()=>1;globalThis.clearInterval=()=>{};
  let saved=config;
  globalThis.fetch=async(path,opts={})=>{
    const request={path,...opts,body:opts.body?JSON.parse(opts.body):undefined};requests.push(request);
    const result=(payload,status=200)=>({ok:status===200,status,json:async()=>payload});
    if(path==='/firmware/manifest.json')return result({},404);
    if(path==='/api/v1/web/info')return result({local_bridge:bridge,management_available:bridge,collector_url:bridge?'http://192.0.2.10:8765':null});
    if(path==='/api/v1/nodes')return displayValid?result({nodes}):result({error:'Unauthorized'},401);
    if(path==='/api/v1/snapshots')return displayValid?result({nodes}):result({error:'Unauthorized'},401);
    if(path==='/api/v1/config'){
      if(!managementValid)return result({error:'Unauthorized'},401);
      if(opts.method==='POST'){
        const body=request.body;
        if(body.action==='pair-agent'){
          saved={...saved,version:'v2',nodes:[{id:body.node.id||'agent:office-mac',name:body.node.name,origin:'agent',type:'server',platform:body.node.platform,enabled:true,registered:false,pairing_pending:true,poll_interval_s:3,ttl_s:15}]};
          return result({config:saved,pairing:{token:'issued-only-once',node_id:saved.nodes[0].id,expires_at:Date.now()/1000+300,interval_s:3,ttl_s:15}});
        }
        if(body.action==='upsert')saved={...saved,version:'v3',nodes:saved.nodes.map(node=>node.id===body.node.id?{...node,...body.node}:node)};
        if(body.action==='revoke-agent')saved={...saved,version:'v4',nodes:saved.nodes.map(node=>node.id===body.id?{...node,revoked:true,enabled:false,registered:false,pairing_pending:false}:node)};
        if(body.action==='delete')saved={...saved,version:'v5',nodes:saved.nodes.filter(node=>node.id!==body.id)};
      }
      return result(saved);
    }
    throw new Error(`Unexpected request ${path}`);
  };
  await import(`../assets/app.js?console-test=${Math.random()}`);await settle();
  const clickNode=async(dataset)=>{await body.emit('click',{target:{closest:()=>({dataset})}});await settle();};
  return {elements,requests,created,clickNode,submit:async id=>{await elements[id].emit('submit');await settle();}};
}

test('console authentication and agent management transitions',async t=>{
  await t.test('unknown inventory is not represented as zero before authentication',async()=>{
    const {elements:e,requests}=await consoleFixture();
    assert.equal(e['stat-nodes'].textContent,'—');assert.equal(e['node-count'].textContent,'—');
    assert.match(e['node-list'].innerHTML,/Connect to view/);assert.equal(e['overview-empty-title'].textContent,'Connect to view your devices.');
    assert.equal(e['access-dialog'].open,true);assert.ok(!requests.some(r=>r.path==='/api/v1/snapshots'));
  });
  await t.test('bad display key retains the access dialog and unknown inventory',async()=>{
    const {elements:e,submit}=await consoleFixture({displayValid:false});e['display-key'].value='wrong';await submit('access-form');
    assert.match(e['access-error'].textContent,/display key was not accepted/);assert.equal(e['access-dialog'].open,true);assert.equal(e['stat-nodes'].textContent,'—');
  });
  await t.test('management rejection preserves authenticated telemetry without edit capability',async()=>{
    const {elements:e,submit}=await consoleFixture({managementValid:false,nodes:[{id:'host',name:'Host',type:'server',summary:{generated_at:Date.now()/1000,ttl_s:15,status:'healthy'}}]});
    e['display-key'].value='display';e['management-key'].value='wrong';await submit('access-form');
    assert.match(e['access-error'].textContent,/management key was not accepted/);assert.equal(e['stat-nodes'].textContent,1);assert.doesNotMatch(e['node-list'].innerHTML,/data-edit/);
    assert.equal(e['access-dialog'].open,true);
  });
  await t.test('pairing uses management role, bridged collector address and key-file commands, then forgets the issued key',async()=>{
    const {elements:e,submit,requests}=await consoleFixture({bridge:true});
    await e['add-node'].emit('click');assert.equal(e['add-dialog'].open,true);await e['add-agent'].emit('click');await settle();
    assert.equal(e['enrollment-url'].value,'http://192.0.2.10:8765');e['enrollment-name'].value='Office Mac';e['enrollment-platform'].value='macos';await submit('enrollment-form');
    const issued=requests.find(r=>r.body?.action==='pair-agent');assert.equal(issued.body.version,'v1');assert.deepEqual(issued.body.node,{name:'Office Mac',platform:'macos',kind:'server',interval_s:3,ttl_s:15});
    assert.equal(e['pairing-dialog'].open,true);assert.equal(e['pairing-token'].value,'issued-only-once');
    assert.match(e['pairing-command'].textContent,/--collector-url 'http:\/\/192\.0\.2\.10:8765'/);assert.match(e['pairing-command'].textContent,/--enrollment-key-file/);assert.doesNotMatch(e['pairing-command'].textContent,/issued-only-once/);assert.match(e['node-list'].innerHTML,/awaiting agent/i);
    await e['pairing-dialog'].close();assert.equal(e['pairing-token'].value,'');assert.equal(e['pairing-command'].textContent,'');
  });
  await t.test('polling migration keeps the old node ID and prevents invalid pairing requests',async()=>{
    const node={id:'remote:office',name:'Office',origin:'feed',type:'server',platform:'linux',enabled:true,url:'http://192.0.2.20:8765',poll_interval_s:3,ttl_s:15};
    const {elements:e,requests,submit,clickNode}=await consoleFixture({bridge:true,config:{version:'v1',max_nodes:4,nodes:[node]}});
    await clickNode({edit:node.id});await e['pair-existing'].emit('click');await settle();assert.match(e['enrollment-description'].textContent,/switches the existing node/);
    e['enrollment-url'].value='https://user:secret@collector.example/';await submit('enrollment-form');assert.match(e['enrollment-error'].textContent,/without a path, credentials or query/);assert.ok(!requests.some(r=>r.body?.action==='pair-agent'));
    e['enrollment-url'].value='https://collector.example';e['enrollment-ttl'].value=5;await submit('enrollment-form');assert.match(e['enrollment-error'].textContent,/three sending intervals/);assert.ok(!requests.some(r=>r.body?.action==='pair-agent'));
    e['enrollment-ttl'].value=15;await submit('enrollment-form');assert.equal(requests.find(r=>r.body?.action==='pair-agent').body.node.id,node.id);assert.match(e['pairing-command'].textContent,/https:\/\/collector.example/);assert.doesNotMatch(e['pairing-command'].textContent,/--allow-insecure-http/);await e['pairing-dialog'].close();
  });
  await t.test('existing push node retains identity across pause, rename, re-pair, revoke and delete',async()=>{
    const node={id:'remote:This-Mac',name:'This Mac',origin:'agent',type:'server',platform:'macos',enabled:true,registered:true,agent_id:'private-device-identity',has_secret:true,poll_interval_s:3,ttl_s:15};
    const {elements:e,requests,submit,clickNode}=await consoleFixture({bridge:true,config:{version:'v1',max_nodes:4,nodes:[node]}});
    await clickNode({toggle:node.id});let edit=requests.findLast(r=>r.body?.action==='upsert');assert.equal(edit.body.node.id,node.id);assert.equal(edit.body.node.enabled,false);assert.equal(edit.body.node.agent_id,undefined);assert.equal(edit.body.node.has_secret,undefined);
    await clickNode({edit:node.id});assert.equal(e['feed-fields'].hidden,true);assert.equal(e['node-url'].required,false);e['node-name'].value='Renamed Mac';await submit('node-form');edit=requests.findLast(r=>r.body?.action==='upsert');assert.equal(edit.body.node.id,node.id);assert.equal(edit.body.node.name,'Renamed Mac');
    await clickNode({edit:node.id});await e['pair-existing'].emit('click');await settle();assert.equal(e['enrollment-id'].value,node.id);assert.match(e['enrollment-description'].textContent,/revokes the current agent/);await submit('enrollment-form');assert.match(e['pairing-command'].textContent,/--re-enroll/);assert.equal(requests.findLast(r=>r.body?.action==='pair-agent').body.node.id,node.id);await e['pairing-dialog'].close();await clickNode({edit:node.id});
    await e['revoke-agent'].emit('click');await submit('revoke-form');const revoked=requests.find(r=>r.body?.action==='revoke-agent');assert.equal(revoked.body.id,node.id);assert.match(e['node-list'].innerHTML,/Revoked/);assert.doesNotMatch(e['node-list'].innerHTML,/data-toggle/);
    await clickNode({edit:node.id});await e['delete-node'].emit('click');await submit('delete-form');assert.equal(requests.find(r=>r.body?.action==='delete').body.id,node.id);assert.match(e['node-list'].innerHTML,/No nodes configured/);
  });
});
