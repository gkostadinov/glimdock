#!/usr/bin/env python3
"""Verify more than four systems and the selected portrait overview."""
import copy,json,re,socket,subprocess,sys,tempfile,time,os,shlex
from pathlib import Path
from urllib.request import urlopen
ROOT=Path(__file__).resolve().parents[2]
WRAPPER=Path.home()/'.codex/skills/playwright/scripts/playwright_cli.sh'
CLI=shlex.split(os.environ['AGENT_OPS_PLAYWRIGHT_CLI']) if os.environ.get('AGENT_OPS_PLAYWRIGHT_CLI') else [str(WRAPPER)] if WRAPPER.exists() else ['npx','--yes','--package','@playwright/cli','playwright-cli']
SESSION='glimdock-agent-ops-limits'
def run(*args):
    result=subprocess.run([*CLI,'-s='+SESSION,*args],capture_output=True,text=True,cwd=ROOT,timeout=30)
    if result.returncode:raise RuntimeError(result.stdout+result.stderr)
    return result.stdout
def click(label):
    snapshot=run('snapshot')
    for line in snapshot.splitlines():
        match=re.search(r'button (.*?) \[ref=([^]]+)\]',line)
        if match and ' '.join(match[1].strip('"').split())==' '.join(label.split()):
            return run('click',match[2])
    raise AssertionError(label+' not found in latest snapshot')
def read(expression):
    result=run('eval',expression)
    return json.loads(re.search(r'### Result\n(.*?)\n###',result,re.S)[1])
checks=[]
(ROOT/'output/agent-operations').mkdir(parents=True,exist_ok=True)
(ROOT/'output/playwright').mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory() as folder:
    doc=json.loads((ROOT/'examples/agent-operations/minimal.json').read_text())
    now=time.time();doc['generated_at']=now;doc['sources'][0].update(sampled_at=now,ttl_s=300)
    base=doc['systems'][0];agent=doc['agents'][0];doc['systems']=[];doc['agents']=[]
    for index in range(6):
        system=copy.deepcopy(base);system.update(id='system-'+str(index),name='System '+str(index+1),sampled_at=now,ttl_s=300)
        doc['systems'].append(system)
        worker=copy.deepcopy(agent);worker.update(system_id=system['id'],heartbeat_at=now,ttl_s=300)
        doc['agents'].append(worker)
    doc['limits']['counts'].update(systems=6,agents=6)
    fixture=Path(folder)/'snapshot.json';fixture.write_text(json.dumps(doc))
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    process=subprocess.Popen([sys.executable,str(ROOT/'tools/agent-operations/api.py'),'--snapshot',str(fixture),'--port',str(port)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        for attempt in range(100):
            try:urlopen(f'http://127.0.0.1:{port}/v1/snapshot',timeout=.2);break
            except OSError:time.sleep(.05)
        else:raise AssertionError('Temporary file publisher did not start')
        run('open',f'http://127.0.0.1:{port}/?mode=file');run('resize','1280','900');run('snapshot')
        assert read("() => document.querySelector('#source-description').textContent").startswith('Publisher snapshot')
        checks.append('generic publisher is labeled independently of the shared hub')
        assert read("() => document.querySelectorAll('.system-grid .card').length")==4
        checks.append('six systems show only four cards on the first page')
        click('1 / 2 ›');assert read("() => document.querySelectorAll('.system-grid .card').length")==2
        checks.append('remaining systems are accessible on the second page')
        run('screenshot','--filename',str(ROOT/'output/playwright/agent-ops-system-pagination.png'))
        click('2 / 2 ›');click('System 1 custom · Healthy 1 recent · -- working -- waiting · -- queued')
        click('Settings');click('Portrait');click('‹')
        assert read("() => {const nav=document.querySelector('#tabs').getBoundingClientRect();return document.querySelectorAll('#content .card').length===4&&[...document.querySelectorAll('#content .card')].every(x=>x.getBoundingClientRect().bottom<=nav.top);}")
        checks.append('selected portrait overview fits four cards above four tabs')
        run('screenshot','--filename',str(ROOT/'output/playwright/agent-ops-portrait-overview.png'))
    finally:
        process.terminate();process.wait(timeout=5);run('close')
(ROOT/'output/agent-operations/edge-checks.json').write_text(json.dumps({'passed':True,'count':len(checks),'checks':checks},indent=2)+'\n')
print(json.dumps({'passed':True,'count':len(checks),'checks':checks}))
