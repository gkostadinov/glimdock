#!/usr/bin/env python3
"""Design QA using real Playwright CLI snapshots and rendered-control refs."""
import json,re,subprocess,os,shlex
from pathlib import Path
from urllib.request import urlopen
ROOT=Path(__file__).resolve().parents[2]
WRAPPER=Path.home()/'.codex/skills/playwright/scripts/playwright_cli.sh'
CLI=shlex.split(os.environ['AGENT_OPS_PLAYWRIGHT_CLI']) if os.environ.get('AGENT_OPS_PLAYWRIGHT_CLI') else [str(WRAPPER)] if WRAPPER.exists() else ['npx','--yes','--package','@playwright/cli','playwright-cli']
SESSION='glimdock-agent-ops'
OUT=ROOT/'output/playwright';OUT.mkdir(parents=True,exist_ok=True)
checks=[]
def run(*args):
    p=subprocess.run([*CLI,'-s='+SESSION,*args],cwd=ROOT,capture_output=True,text=True,timeout=30)
    if p.returncode:raise RuntimeError(p.stdout+p.stderr)
    return p.stdout

def snapshot():return run('snapshot')
def click(text,exact=True):
    for attempt in range(3):
        s=snapshot()
        for line in s.splitlines():
            m=re.search(r'button (.*?) \[ref=([^]]+)\]',line)
            if m and (m[1].strip('"')==text if exact else text in m[1]):
                try:run('click',m[2]);return snapshot()
                except RuntimeError as error:
                    if 'not found in the current page snapshot' not in str(error):raise
                    break
        else:raise AssertionError('Button missing in current snapshot: '+text)
    raise AssertionError('Control kept changing before click: '+text)
def expect(value,message):
    if not value:raise AssertionError(message)
    checks.append(message)
def eval_json(expression):
    result=run('eval',expression)
    match=re.search(r'### Result\n(.*?)\n###',result,re.S)
    if not match:raise AssertionError(result)
    return json.loads(match[1])
def capture(name):run('screenshot','--filename',str(OUT/('agent-ops-'+name+'.png')))
run('open','http://127.0.0.1:8784/');run('resize','1280','900');snapshot()
click('Agent Hub',False)
expect('TRACKED WORK' in snapshot() and 'NEEDS INPUT' in snapshot(),'selected overview highlights tracked work and waiting input')
geometry=eval_json("() => { const c=document.querySelector('#content').getBoundingClientRect(), n=document.querySelector('#tabs').getBoundingClientRect(); return {tabs:document.querySelectorAll('#tabs button').length, cards:document.querySelectorAll('#content .card').length, fits:[...document.querySelectorAll('#content .card')].every(x=>x.getBoundingClientRect().bottom<=n.top+.5&&x.getBoundingClientRect().top>=c.top), width:document.querySelector('#lcd').offsetWidth,height:document.querySelector('#lcd').offsetHeight}; }")
expect(geometry['tabs']==4 and geometry['cards']==4 and geometry['fits'],'landscape overview has four cards/four tabs with no clipping')
expect((geometry['width'],geometry['height'])==(320,240),'landscape native geometry remains 320x240')
capture('overview')
click('Agents');expect('Coordinator' in snapshot(),'Agents tab exposes real agent rows')
expect(eval_json("() => [...document.querySelectorAll('.list-row')].every(r => { const top=r.querySelector('.row-top').getBoundingClientRect(), mid=r.querySelector('.row-mid').getBoundingClientRect(), sub=r.querySelector('.row-sub').getBoundingClientRect(), bar=r.querySelector('.bar').getBoundingClientRect();return top.bottom<=mid.top && mid.bottom<=sub.top && sub.bottom<=bar.top; })"),'agent row text and progress bar do not overlap');capture('agents')
click('Coordinator',False);expect('Heartbeat age' in snapshot() and 'Input tokens' in snapshot(),'agent detail includes source recency and reported usage');capture('agent-detail')
click('Open tracked task',False);expect('Open assigned agent' in snapshot(),'agent detail links to its scoped task');click('‹');expect('Heartbeat age' in snapshot(),'nested detail Back restores agent context')
click('Tasks');click('Approve dashboard layout',False);expect('Progress' in snapshot() and 'Unreported' in snapshot(),'task detail preserves unknown progress');capture('task-detail')
click('Events');click('Layout approval needed',False);expect('Metadata only.' in snapshot(),'event detail omits transcripts and raw messages');capture('event-detail')
click('Overview');click('OPERATIONS & USAGE',False);expect('Shared NAS channel' in snapshot() and 'Firmware checkout' in snapshot(),'operations view exposes channels, resources and usage');capture('operations')
click('Settings');click('Dark');click('‹');capture('dark-operations')
click('Settings');click('Portrait');click('‹');geometry=eval_json("() => ({w:document.querySelector('#lcd').offsetWidth,h:document.querySelector('#lcd').offsetHeight,tabs:document.querySelectorAll('#tabs button').length})");expect((geometry['w'],geometry['h'],geometry['tabs'])==(240,320,4),'portrait retains native size and four tabs');capture('portrait-operations')
click('Settings');click('White');click('Landscape');click('Demonstrate a stale source');click('‹');expect('Stale' in snapshot(),'expired source is visible');capture('stale')
click('Settings');click('Restore sample freshness')
with urlopen('http://127.0.0.1:8784/v1/capabilities',timeout=5) as response:capabilities=json.loads(response.read())
if 'hub' in capabilities['sources']:
    click('Shared hub');click('‹');s=snapshot();expect('METADATA' in s and 'SAMPLE' not in s,'live hub is clearly separate from sample data');capture('live-overview')
    click('OPERATIONS & USAGE',False);s=snapshot();expect('Usage is unavailable.' in s and 'No raw' not in s,'live hub leaves model usage unreported');capture('live-operations')
click('Settings');click('Sample data');click('‹');click('Agent Hub',False);click('Agents');snapshot()
# Real pointer excursion over a rendered agent row: it must not become a tap.
result=run('run-code',"async page => { const b=page.getByRole('button').filter({hasText:'Coordinator'}).first();const r=await b.boundingBox();await page.mouse.move(r.x+20,r.y+20);await page.mouse.down();await page.mouse.move(r.x+50,r.y+20);await page.mouse.move(r.x+20,r.y+20);await page.mouse.up();return {heading:await page.locator('#content h2').textContent()}; }")
expect('Agents' in result,'pointer excursion returning to a row does not activate it')
# Source expiry masks current numbers, not just the status badge.
click('Overview');click('Settings');click('Demonstrate a stale source');click('‹');s=snapshot();expect('button "TRACKED WORK --' in s,'stale source masks live-work count');click('Settings');click('Restore sample freshness');click('‹')
# Keep the user's displayed prototype at the primary overview.
(OUT/'agent-ops-ui-checks.json').write_text(json.dumps({'passed':True,'checks':checks,'count':len(checks)},indent=2)+'\n')
print(json.dumps({'passed':True,'count':len(checks),'checks':checks},indent=2))
