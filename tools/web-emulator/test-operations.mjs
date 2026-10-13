import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..'),assets=process.env.GLIMDOCK_EMULATOR_ASSETS||path.join(root,'collector-web/emulator');
const temporary=await mkdtemp(path.join(tmpdir(),'glimdock-agent-ui-'));
try{
 await writeFile(path.join(temporary,'firmware.mjs'),await readFile(path.join(assets,'firmware.js')));
 const {default:createFirmware}=await import(pathToFileURL(path.join(temporary,'firmware.mjs')));
 const f=await createFirmware({locateFile:name=>path.join(assets,name)});
 const call=(name,...args)=>f.ccall(name,'number',args.map(v=>typeof v==='string'?'string':'number'),args);
 const headerHeight=f._glimdock_width()===240?56:34,tabTop=f._glimdock_height()-40;
 const labels=()=>JSON.parse(f.UTF8ToString(f._glimdock_labels())),texts=()=>labels().map(l=>l.text);
 let clock=1000;const tick=(delta=40)=>{clock+=delta;f._glimdock_tick(clock);};
 const pointer=(x,y,down)=>{f._glimdock_pointer(x,y,down);tick();};
 const tap=l=>{assert(l,'control exists');pointer(l.x+Math.min(12,l.width/2),l.y+l.height/2,1);pointer(l.x+Math.min(12,l.width/2),l.y+l.height/2,0);};
 const switchMode=()=>{const wanted=f._glimdock_page()>=22?"Nodes":"Agents";tap(labels().find(l=>l.y>=headerHeight&&l.y<headerHeight+28&&l.text===wanted));tick(300);};
 const home=()=>{pointer(14,17,1);pointer(14,17,0);tick(300);};
 const now=Date.now()/1000;f._glimdock_time(now);
 const doc=JSON.parse(await readFile(path.join(root,'examples/agent-operations/demo.json'))),delta=now-doc.generated_at;
 doc.generated_at+=delta;for(const key of ['sources','systems','agents','tasks','events','resources','channels'])for(const item of doc[key])for(const field of ['sampled_at','heartbeat_at','updated_at','started_at','at','since'])if(typeof item[field]==='number')item[field]+=delta;
 assert.equal(call('glimdock_operations',JSON.stringify(doc)),1);switchMode();assert.equal(f._glimdock_page(),22);assert(!texts().includes('A')&&!texts().includes('N'),'mode toggle has no letter labels');
 assert(texts().includes('All systems'));
 const globalTabs=(agents=true)=>{for(const title of [agents?'All systems':'All systems','Clock',agents?'Pick system':'Pick system','Settings'])assert(labels().some(l=>l.text===title&&l.y>=tabTop),'shared global tab '+title);};globalTabs();
 tap(labels().find(l=>l.text==='Clock'&&l.y>=tabTop));assert.equal(f._glimdock_page(),20);globalTabs();assert(texts().includes('Recent')&&texts().includes('Running'),'Agents clock uses agent metrics');assert(!texts().includes('Avg CPU')&&!texts().includes('Avg RAM'),'Agents clock has no hardware summaries');assert(texts().some(t=>doc.systems.some(s=>s.name===t)),'Agents clock shortcuts are agent systems');tap(labels().find(l=>l.text==='Settings'&&l.y>=tabTop));assert.equal(f._glimdock_page(),7);globalTabs();tap(labels().find(l=>l.text==='Pick system'&&l.y>=tabTop));assert.equal(f._glimdock_page(),31);globalTabs();const agentSwitcher=labels().find(l=>l.text==='Switch to Nodes');assert(agentSwitcher);tap(labels().find(l=>l.text==='All systems'&&l.y>=tabTop));assert.equal(f._glimdock_page(),22);globalTabs();
 for(const l of labels().filter(l=>l.y>=headerHeight&&l.y<headerHeight+28&&['Nodes','Agents'].includes(l.text)))assert(l.x>=0&&l.x+l.width<=f._glimdock_width(),'both workspace labels fit within display');
 const many=structuredClone(doc);while(many.systems.length<6){const i=many.systems.length;many.systems.push({...structuredClone(doc.systems[0]),id:'additional-'+i,name:'System '+i});}many.limits.counts.systems=6;many.sequence++;call('glimdock_operations',JSON.stringify(many));f._glimdock_show(22);assert.equal(labels().filter(l=>many.systems.some(s=>s.name===l.text)&&l.y>=headerHeight).length,4,'six systems show four cards on first page');tap(labels().find(l=>l.text==='1/2 >'));assert.equal(labels().filter(l=>many.systems.some(s=>s.name===l.text)&&l.y>=headerHeight).length,2,'remaining systems are paged without adding rows');doc.sequence=many.sequence+1;call('glimdock_operations',JSON.stringify(doc));f._glimdock_show(22);
 const cards=labels().filter(l=>doc.systems.some(s=>s.name===l.text)&&l.y>=headerHeight);assert(cards.length<=4,'overview shows at most four system cards');assert(!texts().includes('Stale'),'fresh healthy/degraded sources are displayed as current');assert(texts().some(t=>/^[0-9]+ recent/.test(t)),'fresh source counts are visible');
 tap(cards[0]);
 for(const [field,stamp,title,target] of [['agents','heartbeat_at','name',23],['tasks','updated_at','title',24],['events','at','title',25]]){
   const group=doc[field].filter(r=>r.system_id===doc.systems[0].id);group.forEach((r,i)=>r[stamp]=now-200+i*5);if(field!=='events')group[1][stamp]=null;
   doc.sequence++;assert.equal(call('glimdock_operations',JSON.stringify(doc)),1);f._glimdock_show(target);
   const rendered=labels().filter(l=>group.some(r=>r[title]===l.text)&&l.y>=headerHeight).sort((a,b)=>a.y-b.y).map(l=>l.text);
   const expected=[...group].sort((a,b)=>a[stamp]==null?1:b[stamp]==null?-1:b[stamp]-a[stamp]).map(r=>r[title]);assert.deepEqual(rendered,expected,field+' are newest first with unknown timestamps last');
 }
 doc.sequence++;call('glimdock_operations',JSON.stringify(doc));f._glimdock_show(22);assert(texts().includes('Agents')&&texts().includes('Tasks')&&texts().includes('Events'),'selected system has four agent detail tabs');assert(texts().includes('ATTENTION'));assert(texts().includes('OPERATIONS'));
 for(const l of labels().filter(l=>l.y>=headerHeight&&l.y<tabTop))assert(l.y+l.height<=tabTop,'overview labels fit above navigation');
 tap(labels().find(l=>l.text==='Agents'&&l.y>=tabTop));assert.equal(f._glimdock_page(),23);
 const first=labels().find(l=>doc.agents.some(a=>a.name===l.text)&&l.y>=headerHeight);assert(first);tap(first);assert.equal(f._glimdock_page(),26);assert(texts().includes('Reported state'));
 tap(labels().find(l=>l.text==='\uf053'));assert.equal(f._glimdock_page(),23);
 // Returning drag must never activate a list row.
 pointer(60,headerHeight+46,1);pointer(60,headerHeight+21,1);pointer(60,headerHeight+46,1);pointer(60,headerHeight+46,0);tick(500);assert.equal(f._glimdock_page(),23,'returning swipe stays on the list');
 tap(labels().find(l=>l.text==='Tasks'&&l.y>=tabTop));assert.equal(f._glimdock_page(),24);
 const unknown=doc.tasks.find(t=>t.system_id===doc.systems[0].id);unknown.progress_pct=null;doc.sequence++;call('glimdock_operations',JSON.stringify(doc));f._glimdock_show(24);assert(texts().some(t=>t.endsWith(' • --')),'unknown progress stays unknown');
 const broken=structuredClone(doc);broken.agents[0].source_id='missing-source';assert.equal(call('glimdock_operations',JSON.stringify(broken)),0,'decoder rejects unknown source');
 const duplicate=structuredClone(doc);duplicate.agents.push(duplicate.agents[0]);duplicate.limits.counts.agents++;assert.equal(call('glimdock_operations',JSON.stringify(duplicate)),0,'decoder rejects duplicate scoped IDs');
 f._glimdock_show(22);f._glimdock_show(23);tick(400000);assert(texts().includes('Stale'),'expired heartbeat stops displaying reported activity');
 call('glimdock_operations_error','Upstream unavailable');f._glimdock_show(22);assert(texts().includes('OFFLINE'),'agent source failure is distinct from hardware feed');
 switchMode();assert(![22,23,24,25].includes(f._glimdock_page()));assert(!texts().includes('A')&&!texts().includes('N'),'Nodes mode has no letter labels');switchMode();assert.equal(f._glimdock_page(),22,'workspace button remembers agent overview');
 home();assert.equal(f._glimdock_page(),22,'logo returns to Agents home without changing modes');globalTabs();switchMode();home();assert.equal(f._glimdock_page(),19,'logo returns to Nodes home in Nodes mode');globalTabs(false);tap(labels().find(l=>l.text==='Pick system'&&l.y>=tabTop));assert.equal(f._glimdock_page(),13);const nodeSwitcher=labels().find(l=>l.text==='Switch to Agents');assert.deepEqual([nodeSwitcher.x,nodeSwitcher.y,nodeSwitcher.width,nodeSwitcher.height],[agentSwitcher.x,agentSwitcher.y,agentSwitcher.width,agentSwitcher.height],'picker workspace switches share the same position and size');tap(nodeSwitcher);assert.equal(f._glimdock_page(),22,'node dropdown can switch to Agents');globalTabs();home();assert.equal(f._glimdock_page(),22,'logo stays in Agents after dropdown switch');
 console.log(JSON.stringify({passed:true,checks:['shared global navigation','dropdown workspace switch','system pagination','newest activity first','explicit workspace switch','four tab navigation','four card overview','detail/back navigation','swipe guard','unknown progress','scoped identity validation','heartbeat expiry','source failure','mode-preserving logo home','return to Nodes']}));
}finally{await rm(temporary,{recursive:true,force:true});}
