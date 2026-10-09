export const platformNames = {linux:'Linux',macos:'macOS',windows:'Windows',router:'Router',snmp:'SNMP device',api:'Platform API',other:'Device'};
export function finite(value) { return typeof value === 'number' && Number.isFinite(value) ? value : null; }
export function percent(value) { const n=finite(value); return n === null || n < 0 || n > 100 ? null : n; }
export function ratio(used,total) { const a=finite(used),b=finite(total); return a!==null && b!==null && b>0 && a>=0 && a<=b ? 100*a/b : null; }
export function prettyType(node) { return node.type==='proxmox' ? 'Proxmox' : node.type==='klipper' ? 'Klipper printer' : platformNames[node.platform] || 'Device'; }
export function nodeSummary(entry,now=Date.now()/1000) {
  const snapshot=entry.snapshot || {}, host=snapshot.host || {}, printer=snapshot.printer || {};
  const supplied=entry.summary || snapshot.node?.summary || {};
  const generated=finite(supplied.generated_at ?? snapshot.generated_at), ttl=finite(supplied.ttl_s) ?? 15;
  const suppliedAge=finite(supplied.age_s), age=generated===null ? suppliedAge : Math.max(suppliedAge??0,now-generated,0);
  const fresh=age!==null && age<=ttl;
  const status=entry.status || supplied.status || snapshot.node?.status || 'unknown';
  const usable=fresh && !['offline','unknown','disabled'].includes(status);
  const sensors=Array.isArray(snapshot.sensors)?snapshot.sensors:[];
  const temp=finite(supplied.temperature_c ?? snapshot.power?.cpu_temp_c ?? sensors.find(s=>s.kind==='temperature' && finite(s.value)!==null)?.value);
  return {...entry, snapshot:undefined, summary:{
    status:fresh?status:'offline', age_s:age,
    cpu_percent:usable?percent(supplied.cpu_percent ?? host.cpu_pct):null,
    memory_percent:usable?percent(supplied.memory_percent ?? ratio(host.mem_used_bytes,host.mem_total_bytes)):null,
    temperature_c:usable?temp:null,
    progress_percent:usable?percent(supplied.progress_percent ?? printer.progress_pct):null,
    print_state:usable?supplied.print_state ?? printer.state ?? 'unknown':'unavailable',
    sensors:usable?finite(supplied.sensors) ?? sensors.filter(s=>finite(s.value)!==null).length:null,
    guests:usable?finite(supplied.guests) ?? (Array.isArray(snapshot.guests)?snapshot.guests.length:0):null,
  }};
}
export function stripNode(node) {
  const fields=['id','type','platform','origin','enabled','name','url','poll_interval_s','timeout_s','ttl_s'];
  return Object.fromEntries(fields.filter(key=>node[key]!==undefined).map(key=>[key,node[key]]));
}
export function metricText(value,decimals=0) { return finite(value)===null ? '—' : value.toFixed(decimals); }
export function escapeHtml(value) { return String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
