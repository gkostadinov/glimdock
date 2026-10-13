import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..'),assets=path.join(root,'collector-web/emulator');
const temporary=await mkdtemp(path.join(tmpdir(),'glimdock-agent-ui-'));
try{
 await writeFile(path.join(temporary,'firmware.mjs'),await readFile(path.join(assets,'firmware.js')));
 const {default:createFirmware}=await import(pathToFileURL(path.join(temporary,'firmware.mjs')));
 const f=await createFirmware({locateFile:name=>path.join(assets,name)});
 const call=(name,...args)=>f.ccall(name,'number',args.map(v=>typeof v==='string'?'string':'number'),args);
 const labels=()=>JSON.parse(f.UTF8ToString(f._glimdock_labels())),texts=()=>labels().map(l=>l.text);
 let clock=1000;const tick=(delta=40)=>{clock+=delta;f._glimdock_tick(clock);};
 const pointer=(x,y,down)=>{f._glimdock_pointer(x,y,down);tick();};
 const tap=l=>{assert(l,'control exists');pointer(l.x+Math.min(12,l.width/2),l.y+l.height/2,1);pointer(l.x+Math.min(12,l.width/2),l.y+l.height/2,0);};
 const logo=()=>{pointer(14,17,1);pointer(14,17,0);tick(300);};
 const now=Date.now()/1000;f._glimdock_time(now);
 const doc=JSON.parse(await readFile(path.join(root,'examples/agent-operations/demo.json'))),delta=now-doc.generated_at;
 doc.generated_at+=delta;for(const key of ['sources','systems','agents','tasks','events','resources','channels'])for(const item of doc[key])for(const field of ['sampled_at','heartbeat_at','updated_at','started_at','at','since'])if(typeof item[field]==='number')item[field]+=delta;
 assert.equal(call('glimdock_operations',JSON.stringify(doc)),1);logo();assert.equal(f._glimdock_page(),22);assert(!texts().includes('A')&&!texts().includes('N'),'mode toggle has no letter labels');
 assert(texts().includes('Agent systems'));assert(texts().includes('Agents')&&texts().includes('Tasks')&&texts().includes('Events'),'four agent tabs rendered');
 const cards=labels().filter(l=>doc.systems.some(s=>s.name===l.text)&&l.y>=34);assert(cards.length<=4,'overview shows at most four system cards');assert(!texts().includes('Stale'),'fresh healthy/degraded sources are displayed as current');assert(texts().some(t=>/^[0-9]+ recent/.test(t)),'fresh source counts are visible');
 tap(cards[0]);assert(texts().includes('NEEDS INPUT'));assert(texts().includes('OPERATIONS'));
 for(const l of labels().filter(l=>l.y>=34&&l.y<200))assert(l.y+l.height<=200,'overview labels fit above navigation');
 tap(labels().find(l=>l.text==='Agents'&&l.y>=200));assert.equal(f._glimdock_page(),23);
 const first=labels().find(l=>doc.agents.some(a=>a.name===l.text)&&l.y>=34);assert(first);tap(first);assert.equal(f._glimdock_page(),26);assert(texts().includes('Reported state'));
 tap(labels().find(l=>l.text==='\uf053'));assert.equal(f._glimdock_page(),23);
 // Returning drag must never activate a list row.
 pointer(60,80,1);pointer(60,55,1);pointer(60,80,1);pointer(60,80,0);tick(500);assert.equal(f._glimdock_page(),23,'returning swipe stays on the list');
 tap(labels().find(l=>l.text==='Tasks'&&l.y>=200));assert.equal(f._glimdock_page(),24);
 const unknown=doc.tasks.find(t=>t.system_id===doc.systems[0].id);unknown.progress_pct=null;doc.sequence++;call('glimdock_operations',JSON.stringify(doc));f._glimdock_show(24);assert(texts().some(t=>t.endsWith(' • --')),'unknown progress stays unknown');
 const broken=structuredClone(doc);broken.agents[0].source_id='missing-source';assert.equal(call('glimdock_operations',JSON.stringify(broken)),0,'decoder rejects unknown source');
 const duplicate=structuredClone(doc);duplicate.agents.push(duplicate.agents[0]);duplicate.limits.counts.agents++;assert.equal(call('glimdock_operations',JSON.stringify(duplicate)),0,'decoder rejects duplicate scoped IDs');
 f._glimdock_show(22);f._glimdock_show(23);tick(400000);assert(texts().includes('Stale'),'expired heartbeat stops displaying reported activity');
 call('glimdock_operations_error','Upstream unavailable');f._glimdock_show(22);assert(texts().includes('OFFLINE'),'agent source failure is distinct from hardware feed');
 logo();assert(![22,23,24,25].includes(f._glimdock_page()));assert(!texts().includes('A')&&!texts().includes('N'),'Nodes mode has no letter labels');logo();assert.equal(f._glimdock_page(),22,'logo remembers agent overview');
 console.log(JSON.stringify({passed:true,checks:['logo mode toggle','four tab navigation','four card overview','detail/back navigation','swipe guard','unknown progress','scoped identity validation','heartbeat expiry','source failure','return to Nodes']}));
}finally{await rm(temporary,{recursive:true,force:true});}
