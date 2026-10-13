import {mountFirmwareEmulator} from '/demo/emulator/emulator.js';
import {createDemoTransport} from '/demo/data.js';
const query=new URLSearchParams(location.search);
if(query.get('embedded')==='1')document.body.dataset.embed='true';
if(['demo','hub','file'].includes(query.get('mode')))for(const link of document.querySelectorAll('.workspace-switch a')){const url=new URL(link.href);if(['/','/nodes/'].includes(url.pathname)){url.searchParams.set('mode',query.get('mode'));link.href=url;}}
const screen=document.querySelector('#screen'),description=document.querySelector('#source-description');
try {
  const capabilities=await fetch('/v1/capabilities',{cache:'no-store',redirect:'error'}).then(r=>{if(!r.ok)throw Error('Source unavailable');return r.json();});
  const available=capabilities.sources;
  const desired=query.get('mode'),mode=available.includes(desired)?desired:available.includes('hub')?'hub':available.includes('file')?'file':'demo';
  description.textContent=mode==='demo'?'Synthetic agent and hardware readings':mode==='hub'?'Live coordination metadata · Hardware readings simulated':'Publisher snapshot · Hardware readings simulated';
  window.glimdock=await mountFirmwareEmulator({canvas:document.querySelector('canvas'),requestSnapshot:createDemoTransport(),
    requestOperations:async()=>{const response=await fetch('/v1/snapshot?mode='+mode,{cache:'no-store',redirect:'error'});if(!response.ok)throw Error('Operations source unavailable');return response.json();},
    initialWorkspace:query.get('workspace')==='nodes'?'nodes':'agents',endpoint:'https://demo.invalid/api/v1/snapshot',setupPaired:false,
    onStatus:({state})=>{screen.dataset.ready=String(state==='connected');}
  });
} catch {description.textContent='Operations source unavailable';screen.dataset.ready='false';}
