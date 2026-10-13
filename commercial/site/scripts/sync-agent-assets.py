#!/usr/bin/env python3
"""Publish only synthetic agent-demo inputs and public integration materials."""
from pathlib import Path
import hashlib,json,re,shutil
ROOT=Path(__file__).resolve().parents[3]
PUBLIC=ROOT/'commercial/site/public'
DEMO=PUBLIC/'agents/demo';DEMO.mkdir(parents=True,exist_ok=True)
fixture=ROOT/'examples/agent-operations/demo.json'
assert json.loads(fixture.read_text())['demo'] is True
html=(PUBLIC/'demo/index.html').read_text().replace('<title>Glimdock firmware demo</title>','<title>Glimdock agent operations firmware demo</title>').replace('https://glimdock.com/demo/','https://glimdock.com/agents/demo/').replace('<body>','<body data-public-demo="true">').replace('with four example nodes','with synthetic agent systems')
(DEMO/'index.html').write_text(html)
for obsolete in ['app.js','style.css']:(DEMO/obsolete).unlink(missing_ok=True)
shutil.copyfile(fixture,DEMO/'snapshot.json')
script='// Synthetic values only; no network or personal telemetry.\nconst fixture = '+json.dumps(json.loads(fixture.read_text()),separators=(',',':'))+';\n'
script+='export function createOperationsDemo(){let sequence=0;return ()=>{const copy=structuredClone(fixture), offset=Date.now()/1000-fixture.generated_at;copy.generated_at+=offset;copy.sequence=++sequence;for(const key of ["sources","systems","agents","tasks","events","resources","channels"]){for(const item of copy[key])for(const field of ["sampled_at","heartbeat_at","updated_at","started_at","at","since"])if(typeof item[field]==="number")item[field]+=offset;}return copy;};}\n'
(PUBLIC/'demo/agent-data.js').write_text(script)
docs=PUBLIC/'docs/agents-api';docs.mkdir(parents=True,exist_ok=True)
for name in ['openapi.json','snapshot-v1.schema.json']:shutil.copyfile(ROOT/'contracts/agent-operations'/name,docs/name)
shutil.copyfile(ROOT/'examples/agent-operations/minimal.json',docs/'minimal.json')
shutil.copyfile(ROOT/'examples/agent-operations/publisher.py',docs/'publisher.py')
sources=PUBLIC/'docs/source';sources.mkdir(parents=True,exist_ok=True)
for name in ['AGENT-OPERATIONS.md','AGENT-INTEGRATION.md','AGENT-PROMPTS.md']:
    shutil.copyfile(ROOT/'public-docs'/name,sources/name)
shutil.copyfile(ROOT/'docs/private-conversations.md',sources/'PRIVATE-CONVERSATIONS.md')
prompts=re.findall(r'```text\n(.*?)\n```',(ROOT/'public-docs/AGENT-PROMPTS.md').read_text(),re.S)
assert len(prompts)==4
folder=docs/'prompts';folder.mkdir(parents=True,exist_ok=True)
for name,text in zip(['adapter','collector','firmware','review'],prompts):(folder/(name+'.txt')).write_text(text+'\n')
paths=[p for p in [*DEMO.iterdir(),PUBLIC/'demo/agent-data.js',PUBLIC/'demo/app.js',PUBLIC/'demo/emulator/manifest.json',*docs.glob('*.*'),*folder.iterdir(),*sources.iterdir()] if p.name!='provenance.json']
manifest={'schema':1,'status':'Production firmware UI with synthetic values only','schema_version':'agent-ops/v1','demo':True,'files':{p.relative_to(PUBLIC).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}}
(DEMO/'provenance.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Published synthetic agent demo and four public integration prompts.')
