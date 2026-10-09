import test from 'node:test';
import assert from 'node:assert/strict';
import {nodeSummary,ratio,escapeHtml,agentStatus,seenText,agentCommand,collectorAddress} from '../assets/data.js';
test('summary preserves genuine zero and platform metrics',()=>{const n=nodeSummary({type:'server',status:'healthy',summary:{generated_at:100,ttl_s:15,cpu_percent:0,memory_percent:0,temperature_c:0,sensors:23}},101);assert.equal(n.summary.cpu_percent,0);assert.equal(n.summary.memory_percent,0);assert.equal(n.summary.temperature_c,0);assert.equal(n.summary.sensors,23);});
test('stale and offline summaries do not show old metrics',()=>{const n={status:'healthy',summary:{generated_at:100,ttl_s:15,cpu_percent:35,memory_percent:87}};assert.equal(nodeSummary(n,116).summary.cpu_percent,null);assert.equal(nodeSummary({...n,status:'offline'},101).summary.memory_percent,null);});
test('missing, invalid and overflowing metrics remain unknown',()=>{assert.equal(ratio(1,0),null);assert.equal(ratio(12,10),null);const n=nodeSummary({status:'healthy',summary:{generated_at:100,cpu_percent:101,memory_percent:NaN}},101);assert.equal(n.summary.cpu_percent,null);assert.equal(n.summary.memory_percent,null);assert.equal(escapeHtml('<img src=x onerror="alert(1)">'),'&lt;img src=x onerror=&quot;alert(1)&quot;&gt;');});

test('agent status and receipt age distinguish pairing, pause, revoke and stale measurements',()=>{
  const node={origin:'agent',registered:false,pairing_pending:true,enabled:true};
  assert.equal(agentStatus(node),'awaiting agent');assert.equal(agentStatus({...node,revoked:true}),'revoked');
  assert.equal(agentStatus({...node,registered:true,enabled:false}),'paused');assert.equal(agentStatus({...node,registered:true},{summary:{status:'offline'}}),'offline');
  assert.equal(seenText(null),'No readings received');assert.equal(seenText(100,125),'Last received 25s ago');
  const stale=nodeSummary({origin:'agent',last_seen:125,status:'healthy',summary:{generated_at:100,ttl_s:15,cpu_percent:50}},125);assert.equal(stale.summary.cpu_percent,null);assert.equal(stale.last_seen,125);
});
test('agent commands use key files and require explicit plain HTTP transport',()=>{
  assert.equal(collectorAddress('https://collector.example/'),'https://collector.example');
  for(const url of ['http://a:token@collector.example','https://collector.example/path','https://collector.example?key=secret','file:///tmp'])assert.throws(()=>collectorAddress(url));
  assert.match(agentCommand('http://192.0.2.10:8765'),/--allow-insecure-http/);assert.doesNotMatch(agentCommand('https://collector.example'),/--allow-insecure-http/);
  assert.doesNotMatch(agentCommand('https://collector.example'),/--re-enroll/);assert.match(agentCommand('https://collector.example','unix',true),/--re-enroll/);
  assert.match(agentCommand('https://collector.example','windows'),/--enrollment-key-file C:\\Private\\enrollment.key/);
});
