// Documentation addresses and invented readings only. No local devices are queried.
const GiB = 1024 ** 3;
const definitions = [
  {id:'demo:mac', type:'server', platform:'macos', name:'Studio Mac', address:'192.0.2.10:8765', cpu:18, memory:36, temperature:54, sensors:5},
  {id:'demo:lab', type:'proxmox', platform:'linux', name:'Mini lab', address:'192.0.2.11:8765', cpu:29, memory:52, temperature:51, sensors:7},
  {id:'demo:router', type:'server', platform:'router', name:'Home router', address:'192.0.2.12:8765', cpu:14, memory:22, temperature:44, sensors:2},
  {id:'demo:printer', type:'klipper', platform:'other', name:'Workshop printer', address:'192.0.2.13:7125', progress:68, temperature:215},
  {id:'demo:switch',parent_id:'demo:router',type:'server',platform:'router',name:'Desk switch',address:'192.0.2.14:8765',cpu:8,memory:18,temperature:37,sensors:2},
];

export const demoNodes = definitions.filter(node=>!node.parent_id).map(({id, name, platform, type}) => ({id, name, platform, type}));

export function createDemoTransport(now = () => Date.now()) {
  let sequence = 0;
  return id => {
    const stamp = Math.floor(now() / 1000);
    const nodes = definitions.map(value => ({
      id:value.id, parent_id:value.parent_id||'',type:value.type, platform:value.platform, name:value.name,
      address:value.address, status:'healthy',
      summary:{
        cpu_percent:value.cpu ?? null, memory_percent:value.memory ?? null,
        temperature_c:value.temperature, sensors:value.sensors ?? 2, guests:value.type==='proxmox'?4:0,
        progress_percent:value.progress ?? null, print_state:value.progress == null ? '' : 'printing',
        generated_at:stamp, age_s:0, ttl_s:15, status:'healthy',
      },
    }));
    const index = id ? nodes.findIndex(node => node.id === id) : 0;
    if (index < 0) return {status:404, body:{error:'Unknown example node.'}};
    const node = nodes[index], details = definitions[index];
    const printer = node.type === 'klipper';
    const router = node.platform === 'router';
    const lab=node.type==='proxmox';
    const total = router ? 512 * 1024 ** 2 : lab?64*GiB:32 * GiB;
    return {
      schema:1, demo:true, sequence:++sequence, generated_at:stamp, node, nodes,
      host:{
        name:node.name, ip:node.address.split(':')[0], uptime_s:86400 * 12,
        cpu_pct:printer ? null : details.cpu, cpu_cores:printer ? [] : router ? [14] : [15,21,10,26,12,19,20,22],
        load:router ? [0.3,0.2,0.2] : [1.2,1.1,0.9],
        mem_used_bytes:printer ? null : total * details.memory / 100,
        mem_total_bytes:printer ? null : total, swap_used_bytes:0, swap_total_bytes:2 * GiB,
        ...(node.platform === 'macos' ? {memory_basis:'macos-native',memory_pressure:'normal',mem_available_bytes:total*(1-details.memory/100),mem_app_bytes:total*details.memory/100-3*GiB,mem_wired_bytes:2*GiB,mem_compressed_bytes:GiB,mem_cache_bytes:8*GiB} : {}),
        net_rx_bps:router ? 4_800_000 : 360_000, net_tx_bps:router ? 820_000 : 120_000,
        disk_read_bps:router ? null : 1_200_000, disk_write_bps:router ? null : 640_000,
      },
      power:{cpu_temp_c:printer ? null : details.temperature, package_w:lab?38:node.platform==='macos'?6.4:null,...(node.platform==='macos'?{cpu_power_basis:'macos-ioreport-estimate'}:{})},
      platform:{
        os:node.platform, release:node.platform === 'macos' ? 'macOS' : node.platform === 'windows' ? 'Windows' : router ? 'Router firmware' : 'Linux',
        architecture:node.platform === 'macos' ? 'aarch64' : 'x86_64',
        battery:node.platform === 'macos' ? {percent:82,power_plugged:true} : null,
        interfaces:printer ? [] : [
          {name:router ? 'wan' : 'Ethernet',status:'up',speed_mbps:1000,rx_bps:router ? 4_800_000 : 360_000,tx_bps:router ? 820_000 : 120_000,errors_in:0,errors_out:0,drops_in:0,drops_out:0},
          ...(router ? [{name:'lan',status:'up',speed_mbps:1000,rx_bps:820_000,tx_bps:4_800_000,errors_in:0,errors_out:0,drops_in:0,drops_out:0}] : []),
        ],
      },
      storage:router || printer ? [] : [{id:'system',name:'System drive',status:'available',used_bytes:380 * GiB,total_bytes:1000 * GiB},...(lab?[{id:'pool',name:'Data pool',type:'zfspool',status:'available',used_bytes:2300*GiB,total_bytes:8000*GiB}]:[])],
      sensors:printer ? [] : node.platform === 'macos' ? [
        {id:'cpu-die',name:'CPU temperature',chip:'Apple silicon (sample)',kind:'temperature',value:54,unit:'°C'},
        {id:'cpu-watts',name:'CPU package (estimate)',chip:'Apple silicon (sample)',kind:'power',value:6.4,unit:'W'},
        {id:'battery',name:'Battery charge',chip:'Native host',kind:'other',value:82,unit:'%'},
        {id:'net-rx',name:'Network receive',chip:'Native host',kind:'other',value:360,unit:'KB/s'},
        {id:'net-tx',name:'Network transmit',chip:'Native host',kind:'other',value:120,unit:'KB/s'},
      ] : [
        {id:'cpu-temp',name:router ? 'Board temperature' : 'CPU package',chip:router ? 'SNMP' : 'Example hardware telemetry',kind:'temperature',value:details.temperature,unit:'°C',high:80,crit:95},
        {id:'cpu-load',name:'Processor load',chip:router ? 'SNMP' : 'Native host',kind:'other',value:details.cpu,unit:'%'},
        ...(router ? [] : [
          {id:'cpu-fan',name:'CPU fan',chip:'Example hardware telemetry',kind:'fan',value:1020,unit:'RPM'},
          {id:'cpu-power',name:'CPU package',chip:'Example hardware telemetry',kind:'power',value:38,unit:'W'},
          ...(lab?[{id:'board-amps',name:'CPU rail',chip:'Example hardware telemetry',kind:'current',value:4.8,unit:'A'},{id:'cpu-volt',name:'CPU supply',chip:'Example hardware telemetry',kind:'voltage',value:1.12,unit:'V'},{id:'nvme-temp',name:'NVMe temperature',chip:'Example storage',kind:'temperature',value:43,unit:'°C'}]:[]),
        ]),
      ],
      guests:lab?[
        {id:101,name:'Windows workstation',type:'vm',status:'running',cpu_pct:14,mem_used_bytes:4.6*GiB,mem_total_bytes:32*GiB,mem_assigned_bytes:32*GiB,mem_available_bytes:27.4*GiB,memory_basis:'guest-os',mem_updated_at:stamp,mem_age_s:0,uptime_s:172800,net_rx_bps:44000,net_tx_bps:13000},
        {id:102,name:'NAS services',type:'vm',status:'running',cpu_pct:6,mem_used_bytes:6.2*GiB,mem_total_bytes:24*GiB,mem_cache_bytes:8*GiB,mem_noncache_used_bytes:6.2*GiB,mem_assigned_bytes:24*GiB,memory_basis:'guest-os',mem_updated_at:stamp,mem_age_s:0,uptime_s:604800,net_rx_bps:1200000,net_tx_bps:340000},
        {id:104,name:'macOS studio',type:'vm',status:'running',cpu_pct:8,mem_used_bytes:9*GiB,mem_total_bytes:20*GiB,mem_assigned_bytes:20*GiB,pve_mem_used_bytes:20*GiB,pve_mem_total_bytes:20*GiB,mem_host_bytes:20*GiB,mem_available_bytes:11*GiB,mem_app_bytes:6*GiB,mem_wired_bytes:2*GiB,mem_compressed_bytes:GiB,mem_cache_bytes:5*GiB,memory_pressure:'normal',swap_used_bytes:0,swap_total_bytes:0,memory_basis:'guest-os-macos',mem_updated_at:stamp,mem_age_s:0,uptime_s:172800},
        {id:103,name:'Home automation',type:'lxc',status:'running',cpu_pct:3,mem_used_bytes:1.2*GiB,mem_total_bytes:2*GiB,memory_basis:'proxmox',uptime_s:604800},
      ]:[],
      gpus:lab?[
        {id:'sample:integrated',name:'Integrated GPU (sample)',vendor:'Intel',kind:'integrated',owner:'host',driver:'i915',status:'inventory',utilization_pct:null,temp_c:45,power_w:null,updated_at:stamp,age_s:0},
        {id:'sample:discrete',name:'Discrete GPU (sample)',vendor:'NVIDIA',kind:'discrete',owner:'vm:101',driver:'nvidia',status:'active',utilization_pct:18,temp_c:48,power_w:57,mem_used_bytes:2.1*GiB,mem_total_bytes:8*GiB,updated_at:stamp,age_s:0},
      ]:[],
      disks:lab?[
        {name:'nvme0n1',model:'System NVMe (sample)',health:'passed',status:'active',temp_c:43,wear_pct:4,media_errors:0,power_on_hours:2400,spare_pct:100,read_bps:1200000,write_bps:640000},
        {name:'sda',model:'Data SSD (sample)',health:'passed',status:'active',temp_c:36,reallocated_sectors:0,pending_sectors:0,power_on_hours:3400,zfs_status:'ONLINE',read_errors:0,write_errors:0,checksum_errors:0},
      ]:[],alerts:[],
      sources:{[printer ? 'Moonraker' : router ? 'SNMP' : 'Native host']:{enabled:true,ok:true,updated_at:stamp,age_s:0}},
      ...(printer ? {printer:{
        id:node.id,name:node.name,host:node.address,host_name:'Workshop',klippy_state:'ready',state:'printing',
        message:'Synthetic example print',filename:'glimdock-fit-coupon.gcode',progress_pct:68,
        print_duration_s:3240,total_duration_s:4765,remaining_s:1525,eta_at:stamp+1525,
        progress_basis:'example',eta_basis:'example',current_layer:94,total_layers:138,
        heaters:[{name:'extruder',temp_c:215,target_c:215,duty_pct:45},{name:'heater_bed',temp_c:60,target_c:60,duty_pct:30}],
        temperatures:[],fan_pct:70,updated_at:stamp,age_s:0,ttl_s:15,
      }} : {}),
    };
  };
}
