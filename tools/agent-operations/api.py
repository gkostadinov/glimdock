#!/usr/bin/env python3
"""Read-only reference API and Glimdock design viewer. Does not execute agent work."""
from __future__ import annotations
import argparse,copy,datetime,hashlib,hmac,json,math,mimetypes,os,re,time
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.parse import urlparse,parse_qs
from jsonschema import Draft202012Validator
ROOT=Path(__file__).resolve().parents[2]
SCHEMA=json.loads((ROOT/'contracts/agent-operations/snapshot-v1.schema.json').read_text())
VALIDATOR=Draft202012Validator(SCHEMA)
MAX_BYTES=49152
ARRAYS=('systems','agents','tasks','events','resources','channels')
CAPS={'systems':8,'agents':32,'tasks':32,'events':32,'resources':16,'channels':8}

def validate(doc):
    json.dumps(doc,allow_nan=False)
    VALIDATOR.validate(doc)
    if len(json.dumps(doc,separators=(',',':'),ensure_ascii=False).encode())>MAX_BYTES:raise ValueError('Payload exceeds 48 KiB')
    sources={s['id'] for s in doc['sources']};systems={s['id'] for s in doc['systems']}
    if len(sources)!=len(doc['sources']) or len(systems)!=len(doc['systems']):raise ValueError('Duplicate source or system ID')
    for field in ARRAYS:
        seen=set()
        for row in doc[field]:
            identity=(row.get('system_id',row['id']),row['id'])
            if identity in seen:raise ValueError('Duplicate entity identity')
            seen.add(identity)
            if row['source_id'] not in sources:raise ValueError('Unknown source')
            if field!='systems' and row['system_id'] not in systems:raise ValueError('Unknown system')
        count=doc['limits']['counts'][field]
        if count<len(doc[field]) or (count>len(doc[field]) and not doc['limits']['truncated'].get(field)):raise ValueError('Count/truncation mismatch')
    for row in doc['systems']+doc['agents']:
        usage=row['usage']
        if usage['cost'] is not None and (usage['currency'] is None or usage['provenance'] is None):raise ValueError('Cost needs currency and provenance')
    return doc

def stamp(value):
    try:
        parsed=datetime.datetime.fromisoformat(str(value).replace('Z','+00:00'))
        if parsed.tzinfo is None:return None
        result=parsed.timestamp();return result if math.isfinite(result) and result>=0 else None
    except (ValueError,TypeError):return None

def opaque(prefix,value):return prefix+'-'+hashlib.sha256(str(value).encode()).hexdigest()[:16]
def usage():return {'input_tokens':None,'output_tokens':None,'cost':None,'currency':None,'provenance':None,'window':None}
def safe_title(value,default):
    text=re.sub(r'(?:https?://\S+|(?:[A-Za-z]:)?[/\\]\S+)','[private reference]',str(value or default))
    return ' '.join(text.split())[:96]
def read_json(path):
    if path.is_symlink() or path.stat().st_size>131072:raise ValueError('Metadata limit')
    return json.loads(path.read_text())

def bounded(doc):
    for field in ARRAYS:
        doc['limits']['counts'][field]=len(doc[field]);doc[field]=doc[field][:CAPS[field]]
        doc['limits']['truncated'][field]=doc['limits']['counts'][field]>len(doc[field])
    # Keep summary totals honest if a small consumer cannot receive all rows.
    while len(json.dumps(doc,separators=(',',':'),ensure_ascii=False).encode())>MAX_BYTES:
        field=max(('events','tasks','agents','resources','channels'),key=lambda k:len(doc[k]))
        if not doc[field]:raise ValueError('Cannot fit bounded telemetry')
        doc[field].pop();doc['limits']['truncated'][field]=True
    return validate(doc)

def hub_snapshot(path:Path,titles=False):
    now=time.time();errors=0;agents=[];tasks=[];events=[];resources=[];channels=[];registry={};states={};kind_counts={};source='shared-hub';system='agent-hub'
    def read(path):
        nonlocal errors
        try:
            if not path.resolve().is_relative_to(hub_root):raise ValueError('Metadata escapes source root')
            return read_json(path)
        except (OSError,ValueError,UnicodeError):errors+=1;return None
    hub_root=path.resolve()
    marker=read(path/'channel.json')
    if not marker:raise OSError('Source unavailable')
    for file in sorted((path/'registry').glob('*.json')):
        r=read(file)
        if not isinstance(r,dict):continue
        tool=str(r.get('tool','')).lower()
        if tool not in ('codex','claude','claude-code','claude code','gemini','gemini-cli','gemini cli'):continue
        beat=stamp(r.get('heartbeat_at'));declared=r.get('status');valid=beat is not None and beat<=now+30
        if declared not in ('active','idle','offline') or not valid:errors+=1;continue
        state='offline' if declared=='offline' else 'stale' if now-beat>=600 else 'idle' if declared=='idle' else 'unknown'
        ident=opaque('agent',r.get('id',file.stem));machine=str(r.get('machine',''))
        host={'macbook':'MacBook','macos-vm':'macOS VM','ubuntu-vm':'Ubuntu VM','windows':'Windows'}.get(machine,'Other host')
        registry[r.get('id')]=ident;states[ident]=state
        agents.append({'id':ident,'system_id':system,'source_id':source,'name':str(r.get('tool','Agent'))[:24]+' · '+host,'provider':str(r.get('tool','unknown'))[:48],'host_name':host,'state':state,'model':None,'heartbeat_at':beat,'ttl_s':600,'task_id':None,'usage':usage()})
    for folder in sorted((path/'tasks').iterdir()):
        if not folder.is_dir() or folder.is_symlink() or not (folder/'task.json').is_file():continue
        definition=read(folder/'task.json');s=read(folder/'state.json')
        if not isinstance(definition,dict) or not isinstance(s,dict):continue
        if definition.get('project')=='agent-channel-verification':continue
        state={'queued':'queued','active':'running','blocked':'blocked','done':'done'}.get(s.get('status'),'unknown')
        owner=s.get('owner');claim=read(folder/'claim'/'owner.json') if (folder/'claim'/'owner.json').is_file() else None
        if state in ('running','blocked') and (not claim or claim.get('agent')!=owner):state='unknown'
        ident=opaque('task',definition.get('id',folder.name));agent=registry.get(owner)
        title=safe_title(definition.get('title'),'Task') if titles else 'Tracked task '+ident[-6:]
        tasks.append({'id':ident,'system_id':system,'source_id':source,'title':title,'state':state,'agent_id':agent,'parent_task_id':None,'started_at':stamp(claim.get('claimed_at')) if claim else None,'updated_at':stamp(s.get('updated_at')),'progress_pct':None,'project':None})
        if state in ('running','blocked') and agent:
            for a in agents:
                if a['id']==agent and a['state'] not in ('offline','stale'):
                    a['state']=state;a['task_id']=ident
        for file in sorted((folder/'events').glob('*.json'))[-12:]:
            e=read(file)
            if not isinstance(e,dict):continue
            raw_kind=str(e.get('kind','unknown'));kind=raw_kind if raw_kind in ('created','claimed','completed','released','updated','progress','blocked') else 'other';at=stamp(e.get('at'))
            if at is None or at>now+30 or now-at>86400:continue
            # Metadata only: never export event text, notes or authorization.
            display={'created':'Task created','claimed':'Task claimed','completed':'Task completed','released':'Task released','updated':'Task status updated'}.get(kind,'Task event')
            events.append({'id':opaque('event',e.get('id',file.stem)),'system_id':system,'source_id':source,'severity':'info','kind':kind[:48],'title':display,'at':at,'agent_id':registry.get(e.get('agent')),'task_id':ident})
    for file in sorted((path/'resources').glob('*/owner.json')):
        r=read(file)
        if not isinstance(r,dict):continue
        name=str(r.get('resource','Shared resource'))
        resources.append({'id':opaque('lock',name),'system_id':system,'source_id':source,'name':safe_title(name,'Shared resource')[:64],'state':'held','owner_id':registry.get(r.get('agent')),'since':stamp(r.get('at'))})
    unacked=0;message_count=0
    hint=read(path/'messages'/'_published.index') if (path/'messages'/'_published.index').is_file() else None
    files=[path/'messages'/(x+'.json') for x in hint.get('ids',[]) if re.fullmatch(r'[A-Za-z0-9_.-]+',str(x))] if isinstance(hint,dict) else list((path/'messages').glob('*.json'))
    for file in files[-2048:]:
        r=read(file)
        if not isinstance(r,dict):continue
        at=stamp(r.get('at'))
        if at is None or now-at>86400:continue
        message_count+=1
        recipients=[key for key,val in registry.items() if states[val] not in ('offline','stale')] if r.get('recipient')=='all' else [r.get('recipient')] if r.get('recipient') in registry else []
        for recipient in recipients:
            if not re.fullmatch(r'[A-Za-z0-9_.-]+',str(recipient)):continue
            if not (path/'acks'/recipient/(file.stem+'.json')).is_file():unacked+=1
    tasks.sort(key=lambda t:(t['state'] not in ('blocked','running','queued'),-(t['updated_at'] or 0)))
    events.sort(key=lambda e:-e['at'])
    recent=sum(a['state'] not in ('offline','stale') for a in agents)
    summary={'agents_total':len(agents),'agents_recent':recent,'running':sum(t['state']=='running' for t in tasks),'queued':sum(t['state']=='queued' for t in tasks),'blocked':sum(t['state']=='blocked' for t in tasks),'needs_input':None,'failed_24h':None}
    health='degraded' if errors or summary['blocked'] else 'healthy'
    channels=[{'id':'shared-channel','system_id':system,'source_id':source,'name':'Shared channel • acknowledgments','status':health,'queued':summary['queued'],'unacked':unacked,'latency_ms':None,'sampled_at':now,'ttl_s':15},{'id':'windows-gateway','system_id':system,'source_id':source,'name':'Windows via Ubuntu gateway','status':'unknown','queued':None,'unacked':None,'latency_ms':None,'sampled_at':now,'ttl_s':15}]
    if errors:events.insert(0,{'id':'metadata-warning','system_id':system,'source_id':source,'severity':'warning','kind':'metadata_invalid','title':str(errors)+' metadata records unavailable','at':now,'agent_id':None,'task_id':None})
    doc={'schema_version':'agent-ops/v1','generated_at':now,'sequence':int(now),'demo':False,'sources':[{'id':source,'name':'Shared agent channel','status':health,'sampled_at':now,'ttl_s':15,'capabilities':['sessions','advisory-heartbeats','tracked-tasks','events','locks','acknowledgments'],'provenance':'Read-only shared-channel metadata; heartbeat is advisory, model/process liveness unknown'}],'systems':[{'id':system,'name':'Agent Hub','provider':'agent-hub','status':health,'source_id':source,'sampled_at':now,'ttl_s':15,'summary':summary,'usage':usage()}],'agents':agents,'tasks':tasks,'events':events,'resources':resources,'channels':channels,'limits':{'counts':{},'truncated':{}}}
    return bounded(doc)

def demo_snapshot():
    doc=json.loads((ROOT/'examples/agent-operations/demo.json').read_text());delta=time.time()-doc['generated_at']
    def rebase(value):
        if isinstance(value,dict):
            for key,item in value.items():
                if key in ('generated_at','sampled_at','heartbeat_at','started_at','updated_at','since','at') and isinstance(item,(int,float)):value[key]+=delta
                else:rebase(item)
        elif isinstance(value,list):
            for item in value:rebase(item)
    rebase(doc);doc['sequence']=int(time.time());return validate(doc)

def serve(args):
    token=Path(args.read_token_file).read_text().strip() if args.read_token_file else None
    if args.read_token_file and (not token or len(token)<24):raise ValueError('Use a read token at least 24 characters long')
    if args.bind not in ('127.0.0.1','::1','localhost') and not token:raise ValueError('LAN serving requires --read-token-file')
    static={'/':ROOT/'design/agent-operations/index.html','/app.js':ROOT/'design/agent-operations/app.js','/style.css':ROOT/'design/agent-operations/style.css','/brand/mark.svg':ROOT/'collector-web/assets/mark.svg','/brand/font.woff2':ROOT/'collector-web/assets/space-grotesk.woff2','/schema.json':ROOT/'contracts/agent-operations/snapshot-v1.schema.json'}
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass # Never log headers, keys or snapshot bodies.
        def send(self,status,data,mime='application/json'):
            if not isinstance(data,bytes):data=json.dumps(data,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()
            self.send_response(status);self.send_header('Content-Type',mime);self.send_header('Cache-Control','no-store');self.send_header('X-Content-Type-Options','nosniff');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
        def do_GET(self):
            url=urlparse(self.path)
            if url.path.startswith('/v1/'):
                if token and not hmac.compare_digest(self.headers.get('Authorization',''),'Bearer '+token):return self.send(401,{'error':'read_token_required'})
                query=parse_qs(url.query)
                if set(query)-{'mode'} or query.get('mode',['demo'])[0] not in ('demo','hub','file'):return self.send(400,{'error':'unsupported_query'})
                if url.path=='/v1/capabilities':return self.send(200,{'schema_version':'agent-ops/v1','read_only':True,'max_snapshot_bytes':MAX_BYTES,'auth':'bearer' if token else 'loopback-only','sources':['demo']+(['hub'] if args.hub else [])+(['file'] if args.snapshot else [])})
                if url.path!='/v1/snapshot':return self.send(404,{'error':'not_found'})
                mode=query.get('mode',['hub' if args.hub else 'file' if args.snapshot else 'demo'])[0]
                try:
                    doc=hub_snapshot(Path(args.hub),args.include_task_titles) if mode=='hub' and args.hub else validate(read_json(Path(args.snapshot))) if mode=='file' and args.snapshot else demo_snapshot() if mode=='demo' else None
                    if doc is None:return self.send(404,{'error':'source_not_configured'})
                    return self.send(200,doc)
                except Exception:return self.send(503,{'error':'source_unavailable_or_invalid'})
            if url.path in static:
                file=static[url.path];return self.send(200,file.read_bytes(),mimetypes.guess_type(str(file))[0] or 'application/octet-stream')
            return self.send(404,{'error':'not_found'})
        def do_POST(self):self.send(405,{'error':'read_only'})
    server=ThreadingHTTPServer((args.bind,args.port),Handler);print(f'Agent operations design/API: http://{args.bind}:{args.port}/',flush=True);server.serve_forever()

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--bind',default='127.0.0.1');parser.add_argument('--port',type=int,default=8784);parser.add_argument('--hub');parser.add_argument('--snapshot');parser.add_argument('--read-token-file');parser.add_argument('--include-task-titles',action='store_true');serve(parser.parse_args())
