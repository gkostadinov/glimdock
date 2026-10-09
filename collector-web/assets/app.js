import {finite,nodeSummary,prettyType,stripNode,metricText,sourceName,agentStatus,seenText,collectorAddress,agentCommand,escapeHtml as esc} from './data.js';

const $=id=>document.getElementById(id);
const state={display:'',management:'',bridge:false,bridgeManagement:false,connected:false,inventoryLoaded:false,collectorUrl:location.origin,pairing:null,nodes:[],config:null,view:'overview',busy:false,configAt:0,draft:null,draftVersion:null,emulator:null,emulatorLoading:null,previewNode:'',firmware:null};
const viewCopy={overview:['LIVE MONITORING','Your nodes, together.','A view of every device connected to your collector.'],nodes:['NODE MANAGEMENT','A home for every device.','Add, edit or pause the devices your collector monitors.'],device:['YOUR DESK DISPLAY','The same view, on any screen.','Explore the firmware and update your Glimdock over USB.']};
let refreshTimer=null;

function notice(message,error=false){$('notice').textContent=message;$('notice').hidden=!message;$('notice').classList.toggle('error',error);}
function managementReady(){return state.bridge?state.bridgeManagement:!!state.management;}
async function api(path,{management=false,method='GET',body}={}){
  const token=management?state.management:state.display;
  if(!state.bridge&&!token)throw new Error(management?'Enter a management key in Collector access to edit nodes.':'Connect with a display key first.');
  const headers={};if(!state.bridge)headers.Authorization=`Bearer ${token}`;
  if(body!==undefined)headers['Content-Type']='application/json';
  const response=await fetch(path,{method,headers,body:body===undefined?undefined:JSON.stringify(body),cache:'no-store',credentials:'same-origin',signal:AbortSignal.timeout(12000)});
  const payload=await response.json().catch(()=>({error:'Unexpected collector response'}));
  if(!response.ok){
    if(response.status===401){if(management){state.management='';state.config=null;renderNodes();}else{state.inventoryLoaded=false;state.nodes=[];connection(false,true);renderOverview();renderNodes();}}
    const error=new Error(response.status===401?(management?'The management key was not accepted. Enter the collector setup key to manage nodes.':'The display key was not accepted. Connect with the collector display key to view nodes.'):payload.error||`Collector returned ${response.status}.`);error.status=response.status;throw error;
  }
  return payload;
}
async function loadConfig(){if(!managementReady())return;state.config=await api('/api/v1/config',{management:true});state.configAt=Date.now();renderNodes();}
function connection(connected,error=false){state.connected=connected;$('connection-dot').className=`status-dot ${connected?'connected':error?'error':''}`;$('connection-label').textContent=connected?'Collector connected':error?'Connection interrupted':'Not connected';}
async function refresh(){
  if(state.busy || (!state.bridge&&!state.display))return;
  state.busy=true;
  try{
    const aggregate=await api('/api/v1/snapshots');
    state.nodes=(aggregate.nodes||[]).map(node=>nodeSummary(node));state.inventoryLoaded=true;connection(true);renderOverview();renderNodes();$('last-refresh').textContent=`Updated ${new Date().toLocaleTimeString([],{hour:'2-digit',minute:'2-digit',second:'2-digit'})}`;
    if($('notice').dataset.connectionError==='true'){notice('');delete $('notice').dataset.connectionError;}
    if(managementReady() && (!state.config || Date.now()-state.configAt>15000))try{await loadConfig();}catch(error){notice(`Telemetry connected. Node settings unavailable: ${error.message}`,true);}
  }catch(error){connection(false,true);state.nodes=state.nodes.map(n=>({...n,summary:{...n.summary,status:'offline',cpu_percent:null,memory_percent:null,temperature_c:null,progress_percent:null,sensors:null,guests:null,print_state:'unavailable'}}));renderOverview();renderNodes();notice(error.message,true);$('notice').dataset.connectionError='true';}
  finally{state.busy=false;}
}
function nodeIcon(node){return node.type==='klipper'?'⌁':['router','snmp'].includes(node.platform)?'⌘':'▤';}
function pill(status){return `<span class="status-pill ${['healthy','degraded','offline'].includes(status)?status:''}">${esc(status.charAt(0).toUpperCase()+status.slice(1))}</span>`;}
function metric(label,value,unit,bar=false,memory=false){return `<div><span class="metric-label">${label}</span><strong class="metric-value">${metricText(value,value!==null&&unit==='°C'?1:0)}<span class="metric-unit">${value===null?'':unit}</span></strong>${bar?`<div class="metric-bar ${memory?'memory':''}"><span style="width:${finite(value)??0}%"></span></div>`:''}</div>`;}
function renderOverview(){
  const nodes=state.nodes,known=state.inventoryLoaded;$('node-count').textContent=known?nodes.length:'—';$('stat-nodes').textContent=known?nodes.length:'—';
  $('overview-empty-title').textContent=known?'Bring your first device into view.':'Connect to view your devices.';
  $('overview-empty-description').textContent=known?'Pair an agent to send readings, or add a printer or polling feed.':'Enter your collector access keys to see its nodes and readings.';
  $('overview-empty-action').textContent=known?'Add a node':'Collector access';$('stat-healthy').textContent=state.connected?nodes.filter(n=>n.summary.status==='healthy').length:'—';$('stat-attention').textContent=state.connected?nodes.filter(n=>n.summary.status!=='healthy').length:'—';
  const sensorCounts=nodes.map(n=>n.summary.sensors).filter(n=>finite(n)!==null);$('stat-sensors').textContent=state.connected&&sensorCounts.length?sensorCounts.reduce((a,b)=>a+b,0):'—';$('overview-empty').hidden=nodes.length>0;
  $('node-cards').innerHTML=nodes.map(node=>{
    const s=node.summary,printer=node.type==='klipper',router=['router','snmp','api'].includes(node.platform),onlySensors=s.cpu_percent===null&&s.memory_percent===null;
    const metrics=printer?metric('PRINT PROGRESS',s.progress_percent,'%',true)+metric('TEMPERATURE',s.temperature_c,'°C'):onlySensors?metric('TEMPERATURE',s.temperature_c,'°C')+metric('SENSORS',s.sensors,''):metric('CPU',s.cpu_percent,'%',true)+metric('MEMORY',s.memory_percent,'%',true,true);
    const facts=printer?esc(s.print_state?.replaceAll('_',' ')||'Unknown'):`${s.sensors===null?'—':s.sensors} sensors${node.type==='proxmox'?` · ${s.guests===null?'—':s.guests} guests`:''}${s.temperature_c!==null&&!onlySensors?` · ${metricText(s.temperature_c,1)} °C`:''}`;
    return `<article class="node-card"><div class="node-card-top"><div class="node-title"><div class="node-icon ${printer?'printer':router?'router':''}" aria-hidden="true">${nodeIcon(node)}</div><div><h3 title="${esc(node.name)}">${esc(node.name||node.id)}</h3><p>${esc(prettyType(node))}</p></div></div>${pill(s.status)}</div><div class="card-metrics">${metrics}</div><div class="node-card-footer"><span class="card-facts">${facts}</span><button class="node-open" data-preview="${esc(node.id)}">Open display ↗</button></div></article>`;
  }).join('');
}
function renderNodes(){
  const config=state.config, nodes=config?.nodes || state.nodes;
  $('node-capacity').textContent=config?`${nodes.filter(n=>n.enabled!==false).length} active · ${config.max_nodes} supported per display`:'Up to 4 active nodes per display.';
  $('node-list').innerHTML=nodes.map(node=>{
    const live=state.nodes.find(n=>n.id===node.id),status=agentStatus(node,live);
    const source=node.origin==='agent'?`${sourceName(node)} · ${seenText(live?.last_seen ?? node.last_seen)}`:node.origin==='host'?sourceName(node):`${sourceName(node)} · ${node.url||node.address||node.id}`;
    return `<article class="node-row ${node.enabled===false?'disabled':''}"><div class="node-icon ${node.type==='klipper'?'printer':''}" aria-hidden="true">${nodeIcon(node)}</div><div class="row-identity"><strong>${esc(node.name||node.id)}</strong><p>${esc(prettyType(node))} · ${esc(source)}</p></div>${pill(status)}<div class="row-actions">${config?`${node.revoked?'':`<button class="button secondary" data-toggle="${esc(node.id)}">${node.enabled===false?'Resume':'Pause'}</button>`}<button class="button secondary" data-edit="${esc(node.id)}">Edit</button>`:`<button class="button secondary" data-preview="${esc(node.id)}">View</button>`}</div></article>`;
  }).join('') || (state.inventoryLoaded?'<div class="empty-state"><h2>No nodes configured yet.</h2><p>Pair a device to begin collecting readings.</p></div>':'<div class="empty-state"><h2>Connect to view your devices.</h2><p>The node inventory is available after connecting with your collector display key.</p><button class="button primary" data-action="access">Collector access</button></div>');
  $('host-restore').hidden=!config?.local_node || config.local_node.enabled!==false;
}
async function setView(view){state.view=view in viewCopy?view:'overview';document.querySelectorAll('[data-view]').forEach(button=>button.classList.toggle('active',button.dataset.view===state.view));for(const name of Object.keys(viewCopy))$(`view-${name}`).hidden=name!==state.view;[$('page-eyebrow').textContent,$('page-title').textContent,$('page-description').textContent]=viewCopy[state.view];$('add-node').hidden=state.view==='device';history.replaceState(null,'',`#${state.view}`);if(state.view==='device')await ensureEmulator();}
async function ensureEmulator(){
  if(!state.connected){$('emulator-state').textContent='Connect to the collector to load live firmware.';return;}
  if(state.emulator)return state.emulator;
  if(state.emulatorLoading)return state.emulatorLoading;
  $('emulator-state').textContent='Loading the firmware renderer…';
  state.emulatorLoading=(async()=>{
    try{
      const {mountFirmwareEmulator}=await import('/emulator/emulator.js');
      state.emulator=await mountFirmwareEmulator({canvas:$('firmware-canvas'),setupPaired:managementReady(),requestSnapshot:id=>api(`/api/v1/snapshot${id?'?node='+encodeURIComponent(id):''}`),requestConfig:()=>managementReady()?api('/api/v1/config',{management:true}):null,updateConfig:body=>api('/api/v1/config',{management:true,method:'POST',body}),onSelectNode:id=>{state.previewNode=id;},onStatus:status=>{$('emulator-state').textContent=typeof status==='string'?status:status.message;}});
      $('emulator-state').textContent='Live · same firmware renderer';
      if(state.previewNode && state.emulator.selectNode)await state.emulator.selectNode(state.previewNode);
      return state.emulator;
    }catch(error){$('emulator-state').textContent=`Firmware preview unavailable: ${error.message}`;throw error;}
    finally{state.emulatorLoading=null;}
  })();return state.emulatorLoading;
}
function requireManagement(){if(managementReady())return true;if(state.bridge){notice('This local bridge is read-only. Start it with a setup key file to manage nodes.');return false;}notice('Enter a management key to add or edit nodes.');$('access-dialog').showModal();$('management-key').focus();return false;}
async function openNode(node=null){
  if(!requireManagement())return;
  try{if(!state.config)await loadConfig();}catch(error){notice(error.message,true);return;}
  state.draft=node;state.draftVersion=state.config.version;$('node-form').reset();$('node-id').value=node?.id||'';$('node-origin').value=node?.origin||'feed';$('node-name').value=node?.name||'';$('node-type').value=node?.type||'server';$('node-platform').value=node?.platform??'';$('node-url').value=node?.url||'';$('node-enabled').checked=node?.enabled!==false;$('node-poll').value=node?.poll_interval_s??3;$('node-timeout').value=node?.timeout_s??2.5;$('node-ttl').value=node?.ttl_s??15;$('agent-interval').value=node?.poll_interval_s??3;$('agent-ttl').value=node?.ttl_s??15;$('node-secret').value='';$('clear-secret').checked=false;$('clear-secret-row').hidden=!node?.has_secret;$('node-secret').placeholder=node?.has_secret?'Leave blank to keep the saved key':'Key issued by the device agent';$('delete-node').hidden=!node?.id;$('node-form-title').textContent=node?.id?'Edit your node.':'Add a device.';$('node-form-eyebrow').textContent=node?.origin==='host'?'COLLECTOR HOST':node?.id?'NODE SETTINGS':'NEW NODE';$('node-error').textContent='';$('reload-node').hidden=true;$('pair-existing').hidden=!node?.id||node.origin==='host'||node.type!=='server';$('pair-existing').textContent=node?.origin==='agent'?'Create new pairing key':'Switch to push';$('revoke-agent').hidden=node?.origin!=='agent'||node.revoked;syncNodeFields();$('node-dialog').showModal();
}
function syncNodeFields(){
  const host=$('node-origin').value==='host',agent=$('node-origin').value==='agent',printer=$('node-type').value==='klipper',savedPrinter=!!state.draft?.id&&state.draft.type==='klipper',savedFeed=!!state.draft?.id&&state.draft.origin==='feed'&&!savedPrinter;
  $('feed-fields').hidden=host||agent;$('host-fields').hidden=!host;$('agent-fields').hidden=!agent;$('node-url').required=!host&&!agent;$('platform-field').hidden=$('node-type').value!=='server'||host||agent;
  $('node-type').disabled=agent;$('node-enabled').disabled=agent&&state.draft?.revoked===true;
  $('node-type').querySelector('[value="server"]').disabled=savedPrinter;$('node-type').querySelector('[value="klipper"]').disabled=host||agent||savedFeed;$('node-type').querySelector('[value="proxmox"]').disabled=agent||savedPrinter||(host&&state.config?.local_node?.platform!=='linux');
  $('node-secret-label').firstChild.textContent=printer?'API key ':'Feed key ';$('feed-help').textContent=printer?'Use the Moonraker base URL, for example http://printer:7125.':'Use a compatible polling feed. New native agents should be paired to send readings instead.';$('node-url').placeholder=printer?'http://printer:7125':'http://device:8765/api/v1/snapshot';
}
async function saveNode(event){
  event.preventDefault();if(!requireManagement())return;
  const node={type:$('node-type').value,origin:$('node-origin').value,name:$('node-name').value.trim(),enabled:$('node-enabled').checked};if($('node-id').value)node.id=$('node-id').value;
  if(node.origin==='feed'){Object.assign(node,{url:$('node-url').value.trim(),poll_interval_s:Number($('node-poll').value),timeout_s:Number($('node-timeout').value),ttl_s:Number($('node-ttl').value)});if(node.type!=='klipper')node.platform=node.type==='proxmox'?'linux':$('node-platform').value;if($('node-secret').value)node.secret=$('node-secret').value;else if($('clear-secret').checked)node.clear_secret=true;}
  if(node.origin==='agent'){node.poll_interval_s=Number($('agent-interval').value);node.ttl_s=Number($('agent-ttl').value);if(node.ttl_s<3*node.poll_interval_s){$('node-error').textContent='Expiry must allow at least three sending intervals.';return;}}
  $('save-node').disabled=true;$('node-error').textContent='';$('reload-node').hidden=true;
  try{await api('/api/v1/config',{management:true,method:'POST',body:{action:'upsert',version:state.draftVersion,node}});$('node-dialog').close();await loadConfig();await refresh();notice('Node saved. The collector will refresh its readings.');if(state.emulator)await state.emulator.refresh();}
  catch(error){$('node-error').textContent=error.message;if(error.status===409){$('reload-node').hidden=false;await loadConfig().catch(()=>{});}}
  finally{$('save-node').disabled=false;}
}
async function toggleNode(id){if(!requireManagement())return;const node=state.config?.nodes.find(n=>n.id===id);if(!node)return;try{await api('/api/v1/config',{management:true,method:'POST',body:{action:'upsert',version:state.config.version,node:{...stripNode(node),enabled:node.enabled===false}}});await loadConfig();await refresh();}catch(error){notice(error.message,true);}}
function openAccess(){$('access-error').textContent='';$('access-dialog').showModal();}
async function openAdd(){if(!requireManagement())return;try{if(!state.config)await loadConfig();$('add-dialog').showModal();}catch(error){notice(error.message,true);}}
async function openEnrollment(node=null){
  if(!requireManagement())return;try{if(!state.config)await loadConfig();}catch(error){notice(error.message,true);return;}
  state.draftVersion=state.config.version;state.enrollmentRenew=node?.origin==='agent';$('enrollment-form').reset();$('enrollment-id').value=node?.id||'';$('enrollment-name').value=node?.name||'';$('enrollment-platform').value=node?.platform||'';$('enrollment-url').value=state.collectorUrl;$('enrollment-interval').value=node?.poll_interval_s??3;$('enrollment-ttl').value=node?.ttl_s??15;$('enrollment-error').textContent='';
  $('enrollment-title').textContent=node?.id?'Pair this device to send readings.':'Let your device send readings.';
  $('enrollment-description').textContent=node?.origin==='agent'?'Creating a new pairing key revokes the current agent immediately. Stop the existing agent, then pair it again with the generated --re-enroll command and the same private state directory. Its node identity is retained.':node?.id?'This switches the existing node from polling to agent push and keeps its identity. Readings resume when the agent pairs.':'Create a pairing key, then run the Rust agent on your device. Its identity and readings are registered automatically.';
  $('enrollment-dialog').showModal();
}
function clearPairing(){state.pairing=null;$('pairing-token').value='';$('pairing-command').textContent='';$('pairing-error').textContent='';}
function renderPairing(){if(!state.pairing)return;$('pairing-command').textContent=agentCommand(state.pairing.url,$('pairing-os').value,state.pairing.reEnroll);}
async function issuePairing(event){
  event.preventDefault();if(!requireManagement())return;$('issue-pairing').disabled=true;$('enrollment-error').textContent='';
  try{
    const url=collectorAddress($('enrollment-url').value.trim()),node={name:$('enrollment-name').value.trim(),platform:$('enrollment-platform').value,kind:'server',interval_s:Number($('enrollment-interval').value),ttl_s:Number($('enrollment-ttl').value)};
    if(node.ttl_s<3*node.interval_s)throw new Error('Expiry must allow at least three sending intervals.');
    if($('enrollment-id').value)node.id=$('enrollment-id').value;
    const response=await api('/api/v1/config',{management:true,method:'POST',body:{action:'pair-agent',version:state.draftVersion,node}});
    clearPairing();state.pairing={...response.pairing,url,reEnroll:state.enrollmentRenew};if(!state.pairing.token)throw new Error('The collector did not return a pairing key. Create a new key to try again.');
    if(response.config){state.config=response.config;state.configAt=Date.now();}else await loadConfig();
    $('pairing-token').value=state.pairing.token;$('pairing-title').textContent=`Start the agent for ${node.name}.`;$('pairing-expiry').textContent=`Use this key once before ${new Date(state.pairing.expires_at*1000).toLocaleString()}.`;
    $('pairing-os').value=node.platform==='windows'?'windows':'unix';renderPairing();$('pairing-transport').textContent=url.startsWith('http:')?'LAN HTTP sends credentials and readings without encryption. Use it only on your trusted network; the command explicitly allows this connection. Prefer HTTPS for other networks.':'HTTPS certificates are verified. For a private CA, add --collector-ca-cert /ABSOLUTE/PRIVATE/ca.pem.';
    $('enrollment-dialog').close();if($('node-dialog').open)$('node-dialog').close();$('pairing-dialog').showModal();renderNodes();await refresh();notice('Pairing key created. This node is waiting for its agent.');
  }catch(error){$('enrollment-error').textContent=error.message;if(error.status===409){await loadConfig().catch(()=>{});state.draftVersion=state.config?.version;$('enrollment-error').textContent+=' Saved settings were refreshed; review this device before trying again.';}}
  finally{$('issue-pairing').disabled=false;}
}
function downloadPairing(){if(!state.pairing)return;const link=document.createElement('a'),url=URL.createObjectURL(new Blob([state.pairing.token+'\n'],{type:'text/plain'}));link.download='enrollment.key';link.href=url;link.click();URL.revokeObjectURL(url);}
async function revokeAgent(event){event.preventDefault();try{await api('/api/v1/config',{management:true,method:'POST',body:{action:'revoke-agent',version:state.draftVersion,id:$('node-id').value}});$('revoke-dialog').close();$('node-dialog').close();await loadConfig();await refresh();notice('Agent access revoked. Create a new pairing key to reconnect it.');}catch(error){$('revoke-error').textContent=error.message;}}
async function openPreview(id){state.previewNode=id;try{await setView('device');if(state.emulator?.selectNode)await state.emulator.selectNode(id);else if(state.emulator){await state.emulator.destroy();state.emulator=null;await ensureEmulator();}}catch(error){notice(error.message,true);}}
async function connect(event){event.preventDefault();state.config=null;state.inventoryLoaded=false;state.nodes=[];renderOverview();renderNodes();state.display=$('display-key').value.trim();state.management=$('management-key').value.trim();$('access-error').textContent='';try{await api('/api/v1/nodes');if(state.management)try{await loadConfig();}catch(error){notice(error.message,true);$('access-error').textContent=error.message;await refresh();return;}state.emulator?.setConnection({setupPaired:managementReady()});$('access-dialog').close();notice('');await refresh();if(state.view==='device')await ensureEmulator();}catch(error){$('access-error').textContent=error.message;}}
async function firmwareInfo(){try{const response=await fetch('/firmware/manifest.json',{cache:'no-store'});if(!response.ok)throw Error('No firmware bundle installed');state.firmware=await response.json();$('firmware-version').textContent=state.firmware.version||'Current collector build';$('firmware-source').hidden=!state.firmware.source_relink;if(state.firmware.source_relink)$('firmware-source').href=state.firmware.source_relink;}catch{$('firmware-version').textContent='Bundle not installed';}const secure=window.isSecureContext,serial='serial'in navigator;$('usb-context').textContent=!secure?'USB updates need HTTPS or localhost. Open this collector through a secure address or its local browser bridge.':!serial?'Open this interface in desktop Chrome or Edge to update over USB.':state.firmware?'Ready to connect your display.':'Build or install a firmware bundle before updating.';$('flash-firmware').disabled=!secure||!serial||!state.firmware;}
async function flashFirmware(){
  const button=$('flash-firmware');button.disabled=true;$('flash-progress').hidden=false;$('flash-progress').value=0;$('flash-log').textContent='';
  const log=line=>{$('flash-log').textContent=($('flash-log').textContent+line+'\n').slice(-16000);};
  try{const {flashGlimdock}=await import('./usb-flasher.js');await flashGlimdock({manifest:state.firmware,onLog:log,onStatus:message=>{$('flash-status').textContent=message;},onProgress:percent=>{$('flash-progress').value=percent;}});$('flash-status').textContent='Update complete. Your Glimdock is restarting.';}
  catch(error){$('flash-status').textContent=error.name==='NotFoundError'?'No device selected.':`Update stopped: ${error.message}`;log(error.message);}
  finally{button.disabled=false;}
}
document.querySelectorAll('[data-view]').forEach(button=>button.addEventListener('click',()=>setView(button.dataset.view).catch(error=>notice(error.message,true))));
document.querySelectorAll('[data-close]').forEach(button=>button.addEventListener('click',()=>$(button.dataset.close).close()));
$('access-button').addEventListener('click',()=>{$('access-error').textContent='';$('access-dialog').showModal();});$('access-form').addEventListener('submit',connect);
$('disconnect').addEventListener('click',async()=>{state.display=state.management='';$('display-key').value=$('management-key').value='';clearPairing();state.config=null;state.inventoryLoaded=false;state.nodes=[];connection(false);renderOverview();renderNodes();if(state.emulator)await state.emulator.destroy();state.emulator=null;$('access-dialog').close();notice('Disconnected.');});
$('add-node').addEventListener('click',openAdd);$('overview-empty-action').addEventListener('click',()=>state.inventoryLoaded?openAdd():openAccess());
document.body.addEventListener('click',event=>{const target=event.target.closest('button');if(!target)return;if(target.dataset.action==='access')openAccess();if(target.dataset.edit)openNode(state.config?.nodes.find(n=>n.id===target.dataset.edit));if(target.dataset.toggle)toggleNode(target.dataset.toggle);if(target.dataset.preview)openPreview(target.dataset.preview);});
$('add-agent').addEventListener('click',()=>{$('add-dialog').close();openEnrollment();});$('add-feed').addEventListener('click',()=>{$('add-dialog').close();openNode();});
$('enrollment-form').addEventListener('submit',issuePairing);$('pairing-os').addEventListener('change',renderPairing);$('download-pairing').addEventListener('click',downloadPairing);$('pairing-dialog').addEventListener('close',clearPairing);
$('copy-pairing').addEventListener('click',async()=>{if(!state.pairing)return;try{await navigator.clipboard.writeText(state.pairing.token);$('pairing-error').textContent='';notice('Pairing key copied.');}catch{$('pairing-error').textContent='Clipboard unavailable here. Save the key file instead.';}});
$('pair-existing').addEventListener('click',()=>openEnrollment(state.draft));$('revoke-agent').addEventListener('click',()=>{$('revoke-description').textContent=`${state.draft?.name||'This agent'} will lose access immediately and its readings will be cleared. The node remains configured; create a new pairing key to reconnect it.`;$('revoke-error').textContent='';$('revoke-dialog').showModal();});$('revoke-form').addEventListener('submit',revokeAgent);
$('restore-host').addEventListener('click',()=>openNode({...state.config.local_node,enabled:true}));$('node-type').addEventListener('change',syncNodeFields);$('node-form').addEventListener('submit',saveNode);
$('reload-node').addEventListener('click',async()=>{const id=$('node-id').value;try{await loadConfig();$('node-dialog').close();$('reload-node').hidden=true;await openNode(state.config.nodes.find(node=>node.id===id)||null);}catch(error){$('node-error').textContent=error.message;}});
$('delete-node').addEventListener('click',()=>{$('delete-description').textContent=`${state.draft?.name||'This node'} will be removed from monitoring. Its device and agent will keep running. A paired agent's access is revoked.`;$('delete-error').textContent='';$('delete-dialog').showModal();});
$('delete-form').addEventListener('submit',async event=>{event.preventDefault();try{await api('/api/v1/config',{management:true,method:'POST',body:{action:'delete',version:state.draftVersion,id:$('node-id').value}});$('delete-dialog').close();$('node-dialog').close();await loadConfig();await refresh();notice('Node removed from monitoring.');}catch(error){$('delete-error').textContent=error.message;}});
$('emulator-restart').addEventListener('click',async()=>{if(state.emulator)await state.emulator.destroy();state.emulator=null;await ensureEmulator().catch(error=>notice(error.message,true));});
$('emulator-screenshot').addEventListener('click',()=>{const link=document.createElement('a');link.download='glimdock-screen.png';link.href=$('firmware-canvas').toDataURL('image/png');link.click();});$('flash-firmware').addEventListener('click',flashFirmware);
async function start(){renderOverview();renderNodes();await firmwareInfo();try{const response=await fetch('/api/v1/web/info',{cache:'no-store'});if(response.ok){const info=await response.json();state.bridge=info.local_bridge===true;state.bridgeManagement=info.management_available===true;if(info.collector_url)state.collectorUrl=info.collector_url;}}catch{}$('access-button').hidden=state.bridge;if(state.bridge){await refresh();}else{$('access-dialog').showModal();}await setView(location.hash.slice(1)||'overview').catch(error=>notice(error.message,true));refreshTimer=setInterval(refresh,3000);}
window.addEventListener('pagehide',()=>{clearInterval(refreshTimer);state.display=state.management='';clearPairing();state.emulator?.destroy();});
start();
