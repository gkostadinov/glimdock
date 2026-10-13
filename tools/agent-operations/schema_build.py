"""Generate the portable Agent Operations v1 JSON Schema (no runtime secrets)."""
from pathlib import Path
import json
ROOT=Path(__file__).resolve().parents[2]
ID={'type':'string','pattern':'^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$'}
TEXT=lambda n=96:{'type':'string','maxLength':n}
NULL=lambda s:{'anyOf':[s,{'type':'null'}]}
NUM={'type':'number','minimum':0};COUNT={'type':'integer','minimum':0}
STAMP=NULL(NUM)
OBJ=lambda p,required=None:{'type':'object','additionalProperties':False,'properties':p,'required':list(p) if required is None else required}
ENUM=lambda *v:{'type':'string','enum':list(v)}
usage=OBJ({'input_tokens':NULL(COUNT),'output_tokens':NULL(COUNT),'cost':NULL(NUM),'currency':NULL({'type':'string','pattern':'^[A-Z]{3}$'}),'window':NULL(TEXT(32)),'provenance':NULL(TEXT(96))},['input_tokens','output_tokens','cost','currency','provenance'])
state=ENUM('queued','running','waiting','blocked','idle','done','failed','canceled','offline','stale','unknown')
common={'id':ID,'system_id':ID,'source_id':ID}
defs={
 'source':OBJ({'id':ID,'name':TEXT(64),'status':ENUM('healthy','degraded','offline','unknown'),'sampled_at':NUM,'ttl_s':{'type':'number','minimum':5,'maximum':3600},'capabilities':{'type':'array','maxItems':32,'uniqueItems':True,'items':TEXT(32)},'provenance':TEXT(160)}),
 'system':OBJ({'id':ID,'name':TEXT(64),'provider':TEXT(48),'status':ENUM('healthy','degraded','offline','unknown'),'source_id':ID,'sampled_at':NUM,'ttl_s':{'type':'number','minimum':5,'maximum':3600},'summary':OBJ({k:NULL(COUNT) for k in ['agents_total','agents_recent','running','queued','blocked','needs_input','failed_24h']}),'usage':usage}),
 'agent':OBJ({**common,'name':TEXT(64),'provider':TEXT(48),'host_name':NULL(TEXT(64)),'state':state,'model':NULL(TEXT(64)),'heartbeat_at':STAMP,'ttl_s':{'type':'number','minimum':5,'maximum':3600},'task_id':NULL(ID),'usage':usage}),
 'task':OBJ({**common,'title':TEXT(96),'state':state,'agent_id':NULL(ID),'parent_task_id':NULL(ID),'started_at':STAMP,'updated_at':STAMP,'progress_pct':NULL({'type':'number','minimum':0,'maximum':100}),'project':NULL(TEXT(64))}),
 'event':OBJ({**common,'severity':ENUM('info','warning','critical'),'kind':TEXT(48),'title':TEXT(96),'at':NUM,'agent_id':NULL(ID),'task_id':NULL(ID)}),
 'resource':OBJ({**common,'name':TEXT(64),'state':ENUM('held','available','unknown'),'owner_id':NULL(ID),'since':STAMP}),
 'channel':OBJ({**common,'name':TEXT(64),'status':ENUM('healthy','degraded','offline','unknown'),'queued':NULL(COUNT),'unacked':NULL(COUNT),'latency_ms':NULL(NUM),'sampled_at':NUM,'ttl_s':{'type':'number','minimum':5,'maximum':3600}}),
}
collections={'sources':('source',16),'systems':('system',8),'agents':('agent',64),'tasks':('task',64),'events':('event',64),'resources':('resource',32),'channels':('channel',16)}
props={'schema_version':{'const':'agent-ops/v1'},'generated_at':NUM,'sequence':COUNT,'demo':{'type':'boolean'}}
for field,(kind,limit) in collections.items():props[field]={'type':'array','maxItems':limit,'items':{'$ref':'#/$defs/'+kind}}
props['limits']=OBJ({'counts':OBJ({k:COUNT for k in collections if k!='sources'}),'truncated':OBJ({k:{'type':'boolean'} for k in collections if k!='sources'},[])})
schema={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:agent-ops:v1:snapshot','title':'Agent Operations Snapshot v1',**OBJ(props),'$defs':defs}
(ROOT/'contracts/agent-operations/snapshot-v1.schema.json').write_text(json.dumps(schema,indent=2)+'\n')
