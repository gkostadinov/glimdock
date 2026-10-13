#!/usr/bin/env python3
"""Contract, privacy, missing-data and reference API checks, without NAS mutation."""
import copy,importlib.util,json,math,sys,tempfile,datetime,subprocess,secrets,socket,time
from pathlib import Path
from urllib.request import urlopen,Request
from urllib.error import HTTPError
sys.path.insert(0,str(Path(__file__).parent))
from api import ROOT,validate,hub_snapshot,demo_snapshot
from mappings import a2a_state,otel_usage
checks=[]
(ROOT/'output/agent-operations').mkdir(parents=True,exist_ok=True)
def check(condition,title):
    assert condition,title;checks.append(title)
def rejects(doc,title):
    try:validate(doc)
    except Exception:checks.append(title);return
    raise AssertionError(title)
doc=demo_snapshot();validate(doc);check(doc['demo'] is True,'sample fixture is explicitly synthetic')
minimal=json.loads((ROOT/'examples/agent-operations/minimal.json').read_text());validate(minimal);check(minimal['systems'][0]['usage']['cost'] is None,'minimal publisher retains unavailable usage')
for field,value,title in [('cost',-1,'negative cost rejected'),('input_tokens',-1,'negative token count rejected'),('input_tokens',False,'boolean token count rejected')]:
    bad=copy.deepcopy(doc);bad['systems'][0]['usage'][field]=value;rejects(bad,title)
bad=copy.deepcopy(doc);bad['systems'][0]['usage']['currency']=None;rejects(bad,'cost without currency rejected')
bad=copy.deepcopy(doc);bad['systems'][0]['usage']['provenance']=None;rejects(bad,'cost without provenance rejected')
bad=copy.deepcopy(doc);bad['agents'][0]['prompt']='sensitive prompt';rejects(bad,'raw prompt fields rejected')
bad=copy.deepcopy(doc);bad['agents'][1]=copy.deepcopy(bad['agents'][0]);rejects(bad,'duplicate scoped identity rejected')
bad=copy.deepcopy(doc);bad['agents'][0]['source_id']='missing';rejects(bad,'unknown source rejected')
bad=copy.deepcopy(doc);bad['limits']['counts']['agents']+=1;rejects(bad,'count mismatch requires truncation flag')
bad=copy.deepcopy(doc);bad['tasks'][0]['progress_pct']=101;rejects(bad,'out-of-range progress rejected')
bad=copy.deepcopy(doc);bad['systems'][0]['usage']['cost']=float('nan');rejects(bad,'non-finite usage rejected')
with tempfile.TemporaryDirectory() as temp:
    root=Path(temp);(root/'registry').mkdir();(root/'tasks/t1/claim').mkdir(parents=True);(root/'tasks/t1/events').mkdir();(root/'resources/r1').mkdir(parents=True);(root/'messages').mkdir();(root/'acks').mkdir()
    stamp=datetime.datetime.now(datetime.timezone.utc).isoformat()
    rows={'channel.json':{'version':1},'registry/a1.json':{'id':'a1','machine':'macbook','tool':'codex','status':'active','heartbeat_at':stamp,'project':'/private/secret/project','thread':'secret-thread'},'tasks/t1/task.json':{'id':'t1','title':'Do secret work','project':'/private/secret/project','authorization':'private permission'},'tasks/t1/state.json':{'status':'active','owner':'a1','updated_at':stamp,'note':'private note'},'tasks/t1/claim/owner.json':{'agent':'a1','claimed_at':stamp},'tasks/t1/events/e1.json':{'id':'e1','at':stamp,'agent':'a1','kind':'claimed','text':'private event text'},'resources/r1/owner.json':{'resource':'/private/secret/resource','agent':'a1','at':stamp},'messages/m1.json':{'id':'m1','at':stamp,'sender':'a1','recipient':'all','text':'private message text','topic':'private-topic'}}
    for path,obj in rows.items():(root/path).write_text(json.dumps(obj))
    before={p.relative_to(root):p.read_bytes() for p in root.rglob('*') if p.is_file()}
    live=hub_snapshot(root);text=json.dumps(live)
    for secret in ['secret-thread','private permission','private note','private event text','private message text','/private/secret','Do secret work','private-topic']:
        check(secret not in text,'private metadata omitted: '+secret)
    check(live['systems'][0]['summary']['running']==1,'tracked running count comes from valid claim ownership')
    check(live['agents'][0]['model'] is None and live['systems'][0]['usage']['cost'] is None and live['systems'][0]['summary']['needs_input'] is None,'hub does not invent model/usage/approval metrics')
    check(before=={p.relative_to(root):p.read_bytes() for p in root.rglob('*') if p.is_file()},'exporter never mutates hub files')
    (root/'registry/bad.json').write_text('')
    live=hub_snapshot(root);check(live['sources'][0]['status']=='degraded' and len(live['agents'])==1,'corrupt metadata is visible without hiding valid agents')
    (root/'tasks/t1/claim/owner.json').write_text(json.dumps({'agent':'other'}));live=hub_snapshot(root);check(live['tasks'][0]['state']=='unknown' and live['systems'][0]['summary']['running']==0,'inconsistent claim never appears as tracked running work')
check(a2a_state('TASK_STATE_AUTH_REQUIRED')=='waiting' and a2a_state('working')=='running' and a2a_state('future-state')=='unknown','A2A mappings preserve waiting and unknown states')
check(otel_usage({'gen_ai.usage.input_tokens':0})['input_tokens']==0 and otel_usage({})['input_tokens'] is None,'OpenTelemetry mapping distinguishes measured zero from unreported tokens')
spec=importlib.util.spec_from_file_location('public_publisher',ROOT/'examples/agent-operations/publisher.py');publisher=importlib.util.module_from_spec(spec);spec.loader.exec_module(publisher)
with tempfile.TemporaryDirectory() as temp:
    target=Path(temp)/'snapshot.json';publisher.publish_snapshot(minimal,target)
    check(json.loads(target.read_text())==minimal,'atomic publisher preserves source timestamps and unavailable metrics')
    original=target.read_bytes();bad=copy.deepcopy(minimal);bad['prompt']='private content'
    try:publisher.publish_snapshot(bad,target)
    except Exception:pass
    else:raise AssertionError('Invalid publication accepted')
    check(target.read_bytes()==original,'invalid publisher update preserves last accepted snapshot')
# The localhost viewer serves a real bounded read-only contract.
with urlopen('http://127.0.0.1:8784/v1/snapshot?mode=demo',timeout=5) as r:body=r.read();validate(json.loads(body));check(len(body)<=49152,'HTTP sample response fits the ESP32 JSON budget')
try:urlopen(Request('http://127.0.0.1:8784/v1/snapshot',data=b'{}',method='POST'),timeout=5)
except HTTPError as e:check(e.code==405,'API rejects writes/control commands')
with urlopen('http://127.0.0.1:8784/v1/capabilities',timeout=5) as r:check(json.loads(r.read())['read_only'],'API advertises read-only capability')
with tempfile.TemporaryDirectory() as temp:
    token=secrets.token_urlsafe(32);keyfile=Path(temp)/'read-token';keyfile.write_text(token);keyfile.chmod(0o600)
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    process=subprocess.Popen([sys.executable,str(Path(__file__).with_name('api.py')),'--port',str(port),'--read-token-file',str(keyfile)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    endpoint=f'http://127.0.0.1:{port}/v1/snapshot'
    try:
        for attempt in range(100):
            try:urlopen(endpoint,timeout=.2)
            except HTTPError as error:
                check(error.code==401,'reader authentication rejects missing bearer key');break
            except OSError:time.sleep(.05)
        else:raise AssertionError('Temporary authenticated API did not start')
        try:urlopen(Request(endpoint,headers={'Authorization':'Bearer incorrect'}),timeout=2)
        except HTTPError as error:check(error.code==401,'reader authentication rejects incorrect bearer key')
        else:raise AssertionError('Incorrect key accepted')
        with urlopen(Request(endpoint,headers={'Authorization':'Bearer '+token}),timeout=2) as response:
            validate(json.loads(response.read()));check(response.status==200,'reader authentication accepts correct bearer key')
    finally:process.terminate();process.wait(timeout=5)
(ROOT/'output/agent-operations/api-checks.json').write_text(json.dumps({'passed':True,'count':len(checks),'checks':checks},indent=2)+'\n');print(json.dumps({'passed':True,'count':len(checks)},indent=2))
