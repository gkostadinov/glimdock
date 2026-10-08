"""Fixed guest OS memory/GPU reads through an existing QEMU guest agent.

No VM configuration, drivers, packages, ballooning, power, or guest files are
changed. Windows CIM supplies visible/available memory; NVIDIA's own tool
supplies VRAM and GPU metrics (Win32_VideoController.AdapterRAM is not used).
Linux reads procfs and DRM sysfs. Queries run at a slow independent cadence.
"""
from __future__ import annotations

import base64
import csv
import io
import json
import math
import re
import errno
import threading
import time

NVIDIA_FIELDS = "name,pci.bus_id,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,clocks.current.graphics,clocks.current.memory,fan.speed"
GPU_METRICS = ("utilization_pct", "mem_used_bytes", "mem_total_bytes", "temp_c", "power_w", "graphics_mhz", "memory_mhz", "fan_pct")


def finite(value):
    if value is None or isinstance(value, bool):
        return None
    try:
        result = float(value)
        return result if math.isfinite(result) else None
    except (ValueError, TypeError):
        return None


def memory(total, available, arc=None):
    total, available = finite(total), finite(available)
    if total is None or total <= 0 or available is None or not 0 <= available <= total:
        raise ValueError("Guest OS memory total/available invalid")
    arc = finite(arc)
    if arc is not None and not 0 <= arc <= total:
        raise ValueError("Guest ARC size invalid")
    used = total - available
    return {"mem_used_bytes": used, "mem_total_bytes": total, "mem_available_bytes": available,
            "mem_cache_bytes": arc, "mem_noncache_used_bytes": max(0, used - arc) if arc is not None else None,
            "memory_basis": "guest-os-arc" if arc is not None else "guest-os"}


def parse_meminfo(text, arc_text=None):
    values = {}
    for line in text.splitlines():
        key, _, rest = line.partition(":")
        fields = rest.split()
        if fields:
            values[key] = int(fields[0]) * (1024 if len(fields) > 1 and fields[1] == "kB" else 1)
    arc = None
    if arc_text:
        for line in arc_text.splitlines():
            fields = line.split()
            if len(fields) == 3 and fields[0] == "size":
                arc = int(fields[2])
    return memory(values.get("MemTotal"), values.get("MemAvailable"), arc)


def parse_nvidia_csv(text):
    result = []
    for row in csv.reader(io.StringIO(text)):
        if len(row) != 10:
            continue
        values = [value.strip() for value in row]
        gpu = {"name": values[0][:96], "pci_bus": values[1].lower(), "vendor": "NVIDIA",
               "vendor_id": "10de", "kind": "discrete", "driver": "nvidia", "status": "active", "error": None}
        for key, value in zip(GPU_METRICS, values[2:]):
            gpu[key] = finite(value)
        for key in ("mem_used_bytes", "mem_total_bytes"):
            if gpu[key] is not None:
                gpu[key] *= 1024 * 1024  # NVIDIA CSV reports MiB with nounits.
        validate_gpu_metrics(gpu)
        result.append(gpu)
    return result


def validate_gpu_metrics(gpu):
    for key in GPU_METRICS:
        value = finite(gpu.get(key))
        if value is not None:
            if key in ("utilization_pct", "fan_pct") and not 0 <= value <= 100:
                value = None
            elif key == "temp_c" and not -50 <= value <= 150:
                value = None
            elif key != "temp_c" and value < 0:
                value = None
            elif key == "mem_total_bytes" and value == 0:
                value = None
        gpu[key] = value
    if gpu.get("mem_total_bytes") is not None and gpu.get("mem_used_bytes") is not None and gpu["mem_used_bytes"] > gpu["mem_total_bytes"]:
        gpu["mem_used_bytes"] = None


WINDOWS_SCRIPT = r"""
$ErrorActionPreference='Stop'
$os=Get-CimInstance Win32_OperatingSystem
$cards=@(Get-CimInstance Win32_VideoController | ForEach-Object {
  @{name=$_.Name;pnp_id=$_.PNPDeviceID;driver=$_.DriverVersion;status=$_.Status}
})
$csv='';$gpuError=$null
$smi=Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
if($smi){
  $csv=(& $smi.Source '--query-gpu=QUERY_FIELDS' '--format=csv,noheader,nounits' 2>$null | Out-String)
  if($LASTEXITCODE -ne 0){$gpuError='NVIDIA metrics unavailable';$csv=''}
}
@{schema=1;generated_at=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds();
  memory=@{total_bytes=[double]$os.TotalVisibleMemorySize*1024;available_bytes=[double]$os.FreePhysicalMemory*1024};
  cards=$cards;nvidia_csv=$csv;gpu_error=$gpuError} | ConvertTo-Json -Compress -Depth 5
""".replace("QUERY_FIELDS", NVIDIA_FIELDS)

# This same fixed source can be copied into a guest-exec python3 -c argument.
# No third-party modules, temporary files, shell, or unsupported Intel PMU probe.
LINUX_SCRIPT = r'''
import json,time,re,subprocess,errno
from pathlib import Path
def read(path):
    try:return Path(path).read_text().strip()
    except OSError:return None
def num(path,scale=1):
    try:return float(read(path))*scale
    except (TypeError,ValueError):return None
mem=read('/proc/meminfo');arc=read('/proc/spl/kstat/zfs/arcstats')
cards=[]
for path in sorted(Path('/sys/bus/pci/devices').glob('*')):
    klass=read(path/'class')
    if not klass or not klass.startswith('0x03'):continue
    vendor=(read(path/'vendor') or '').replace('0x','');device=(read(path/'device') or '').replace('0x','')
    if vendor in ('1234','1af4','15ad','1414','1b36'):continue
    driver=(path/'driver').resolve().name if (path/'driver').exists() else ''
    item={'pci_bus':path.name,'vendor_id':vendor,'device_id':device,'driver':driver,
          'graphics_mhz':None,'utilization_pct':num(path/'gpu_busy_percent'),
          'mem_used_bytes':num(path/'mem_info_vram_used'),'mem_total_bytes':num(path/'mem_info_vram_total'),
          'temp_c':None,'power_w':None,'memory_mhz':None,'fan_pct':None,'error':None}
    for drm in (path/'drm').glob('card[0-9]*'):
        for clock in ('gt/gt0/rps_act_freq_mhz','gt_cur_freq_mhz'):
            value=num(drm/clock)
            if value is not None:item['graphics_mhz']=value;break
    for hwmon in (path/'hwmon').glob('hwmon*'):
        if item['temp_c'] is None:item['temp_c']=num(hwmon/'temp1_input',.001)
        if item['power_w'] is None:item['power_w']=num(hwmon/'power1_average',.000001)
        pwm=num(hwmon/'pwm1');maximum=num(hwmon/'pwm1_max')
        if pwm is not None and maximum and maximum>0:item['fan_pct']=pwm/maximum*100
    if driver in ('i915','xe') and item['utilization_pct'] is None:
        item['error']='GPU engine counters unavailable in passthrough guest'
        item['mem_used_bytes']=None;item['mem_total_bytes']=None
    cards.append(item)
clients={};scan_complete=True;scanned=0
for info in Path('/proc').glob('[0-9]*/fdinfo/*'):
    scanned+=1
    if scanned>32768:scan_complete=False;break
    try:content=info.read_text()
    except OSError as exc:
        if exc.errno not in (errno.ENOENT,errno.ESRCH):scan_complete=False
        continue
    values={}
    for line in content.splitlines():
        key,sep,value=line.partition(':')
        if sep:values[key]=value.strip()
    client=values.get('drm-client-id');device=values.get('drm-pdev')
    if not client or not device:continue
    identity=(device,client)
    counters={};capacities={}
    for key,value in values.items():
        match=re.fullmatch(r'drm-engine-(?!capacity-)([a-zA-Z0-9_-]+)',key)
        if match:
            pieces=value.split()
            if len(pieces)==2 and pieces[1]=='ns':
                try:counters[match[1]]=int(pieces[0])
                except ValueError:scan_complete=False
        match=re.fullmatch(r'drm-engine-capacity-([a-zA-Z0-9_-]+)',key)
        if match:
            try:capacities[match[1]]=int(value)
            except ValueError:scan_complete=False
    if counters:
        # Duplicate fds can be read at slightly different instants; keep the
        # largest counters once per stable DRM client+device identity.
        if identity not in clients:clients[identity]={'device':device,'client':client,'counters':counters,'capacities':capacities}
        else:
            for engine,value in counters.items():clients[identity]['counters'][engine]=max(value,clients[identity]['counters'].get(engine,0))
        if len(clients)>128:scan_complete=False;break
csv=''
try:
    p=subprocess.run(['nvidia-smi','--query-gpu=QUERY_FIELDS','--format=csv,noheader,nounits'],capture_output=True,text=True,timeout=3)
    if p.returncode==0:csv=p.stdout
except (OSError,subprocess.TimeoutExpired):pass
print(json.dumps({'schema':1,'generated_at':time.time(),'meminfo':mem,'arcstats':arc,'cards':cards,'nvidia_csv':csv,
                 'drm_stats':{'monotonic_s':time.monotonic(),'complete':scan_complete,'clients':list(clients.values())[:128]}},separators=(',',':')))
'''.replace("QUERY_FIELDS", NVIDIA_FIELDS)


QGA_OUTPUT_LIMIT = 64 * 1024
# Use the installed Proxmox transport, including guest-sync-delimited, without
# Agent::agent_cmd's separate 3-second guest-ping preflight. No user-supplied
# command, path, script, QMP action or socket is accepted by this bridge.
QGA_BRIDGE = r'''
use strict;
use warnings;
use JSON;
use MIME::Base64 qw(decode_base64);
use PVE::QemuConfig;
use PVE::QemuServer::Agent;
use PVE::QemuServer::Helpers;
use PVE::QemuServer::Monitor;

my ($vmid, $action, $pid) = @ARGV;
die "Invalid telemetry VM ID\n" if !defined($vmid) || $vmid !~ /^[1-9][0-9]{2,8}$/;
die "Invalid telemetry action\n" if !defined($action)
    || $action !~ /^(get-osinfo|exec-windows|exec-linux|exec-status)$/;
die "Invalid telemetry arguments\n" if scalar(@ARGV) != ($action eq 'exec-status' ? 3 : 2);
if ($action eq 'exec-status') {
    die "Invalid telemetry process ID\n" if !defined($pid)
        || $pid !~ /^[1-9][0-9]{0,9}$/ || $pid > 4294967295;
}
my $conf = PVE::QemuConfig->load_config($vmid);
die "QEMU guest agent is not configured/enabled\n"
    if !PVE::QemuServer::Agent::get_qga_key($conf, 'enabled');
die "VM $vmid is not running\n" if !PVE::QemuServer::Helpers::vm_running_locally($vmid);

my ($command, %params);
if ($action eq 'get-osinfo') {
    $command = 'guest-get-osinfo';
} elsif ($action eq 'exec-status') {
    $command = 'guest-exec-status';
    $params{pid} = int($pid);
} else {
    $command = 'guest-exec';
    $params{'capture-output'} = JSON::true;
    if ($action eq 'exec-windows') {
        $params{path} = 'powershell.exe';
        $params{arg} = ['-NoProfile', '-NonInteractive', '-EncodedCommand', '__WINDOWS_BASE64__'];
    } else {
        $params{path} = 'python3';
        $params{arg} = ['-c', decode_base64('__LINUX_BASE64__')];
    }
}
my $res = PVE::QemuServer::Monitor::mon_cmd($vmid, $command, %params, timeout => 12);
PVE::QemuServer::Agent::check_agent_error($res, 'Guest telemetry action failed');
if ($action eq 'exec-status') {
    # Match Agent::qemu_exec_status's wire decoding and boolean conversion.
    for my $key ('out-data', 'err-data') {
        if ($res->{$key}) {
            my $decoded = eval { decode_base64($res->{$key}) };
            warn $@ if $@;
            $res->{$key} = $decoded if defined($decoded);
        }
        if (defined($res->{$key}) && length($res->{$key}) > 65536) {
            $res->{$key} = '';
            $res->{$key eq 'out-data' ? 'out-truncated' : 'err-truncated'} = 1;
        }
    }
    for my $key (keys %$res) {
        $res->{$key} = $res->{$key} ? 1 : 0 if JSON::is_bool($res->{$key});
    }
}
print JSON::encode_json($res);
'''.replace('__WINDOWS_BASE64__', base64.b64encode(WINDOWS_SCRIPT.encode('utf-16le')).decode('ascii')).replace(
    '__LINUX_BASE64__', base64.b64encode(LINUX_SCRIPT.encode('utf-8')).decode('ascii'))


def qga_command(vmid, action, runner, pid=None):
    """Only fixed telemetry actions through the host's installed PVE library."""
    if type(vmid) is not int or not 100 <= vmid <= 999999999:
        raise ValueError("Invalid telemetry VM ID")
    if action not in ("get-osinfo", "exec-windows", "exec-linux", "exec-status"):
        raise ValueError("Invalid telemetry action")
    argv = ["perl", "-e", QGA_BRIDGE, str(vmid), action]
    if action == "exec-status":
        if type(pid) is not int or not 1 <= pid <= 4294967295:
            raise ValueError("Invalid telemetry process ID")
        argv.append(str(pid))
    elif pid is not None:
        raise ValueError("Unexpected telemetry process ID")
    raw = runner(argv, timeout=20)
    if len(raw.stdout.encode("utf-8")) > QGA_OUTPUT_LIMIT * 3:
        raise ValueError("Guest telemetry response exceeds output bound")
    result = json.loads(raw.stdout)
    if not isinstance(result, dict):
        raise ValueError("Guest telemetry response is not an object")
    return result


def parse_document(document):
    if document.get("schema") != 1:
        raise ValueError("Guest telemetry schema invalid")
    timestamp = finite(document.get("generated_at"))
    if timestamp is None or not -60 <= time.time() - timestamp <= 90:
        raise ValueError("Guest telemetry sample expired/invalid")
    if document.get("meminfo") is not None:
        mem = parse_meminfo(document["meminfo"], document.get("arcstats"))
    else:
        mem = memory(document.get("memory", {}).get("total_bytes"), document.get("memory", {}).get("available_bytes"))
    cards = []
    for card in document.get("cards") or []:
        card = card.copy()
        if card.get("pnp_id"):
            match = re.search(r"VEN_([0-9A-F]{4}).*DEV_([0-9A-F]{4})", card["pnp_id"], re.I)
            if not match:
                continue
            card["vendor_id"], card["device_id"] = [v.lower() for v in match.groups()]
        if card.get("vendor_id") in ("1234", "1af4", "15ad", "1414", "1b36"):
            continue
        for key in GPU_METRICS:
            card[key] = finite(card.get(key))
        validate_gpu_metrics(card)
        cards.append(card)
    nvidia = parse_nvidia_csv(document.get("nvidia_csv") or "")
    for index, gpu in enumerate(nvidia):
        matching = [c for c in cards if c.get("vendor_id") == "10de" and (c.get("pci_bus") == gpu["pci_bus"] or c.get("name") == gpu["name"])]
        if not matching:
            matching = [c for c in cards if c.get("vendor_id") == "10de"]
            matching = matching[index:index+1]
        if matching:
            driver = matching[0].get("driver")
            matching[0].update(gpu)
            if driver and driver != "nvidia":
                matching[0]["driver"] = f"nvidia {driver}"[:48]
        else:
            cards.append(gpu)
    return {"memory": mem, "gpus": cards[:8], "generated_at": timestamp, "gpu_error": document.get("gpu_error"),
            "drm_stats": document.get("drm_stats"), "error": None}


class DRMRates:
    """Deduplicated client engine nanoseconds, normalized by engine capacity."""
    def __init__(self):
        self.previous, self.mono = {}, None

    def apply(self, cards, stats):
        if not isinstance(stats, dict):
            return
        now = finite(stats.get("monotonic_s"))
        complete = stats.get("complete") is True
        clients = {}
        for client in stats.get("clients", [])[:128]:
            key = (client.get("device"), str(client.get("client")))
            counters = {k: finite(v) for k, v in client.get("counters", {}).items()}
            if key not in clients:
                clients[key] = dict(client, counters=counters)
            else:
                for engine, value in counters.items():
                    if value is not None:
                        clients[key]["counters"][engine] = max(value, clients[key]["counters"].get(engine) or 0)
        elapsed = now - self.mono if now is not None and self.mono is not None else None
        sums, capacities, known, changed, decreased = {}, {}, set(), set(), set()
        for key, client in clients.items():
            device = key[0]
            previous = self.previous.get(key)
            if previous is None:
                changed.add(device)
                continue
            for engine, value in client["counters"].items():
                before = previous["counters"].get(engine)
                if value is None or before is None or value < before:
                    changed.add(device)
                    if value is not None and before is not None and value < before:
                        # DRM counters can temporarily decrease. Retain the
                        # previous maximum until they catch up, rather than
                        # turning recovery into a false busy spike.
                        client["counters"][engine] = before
                        decreased.add(device)
                    continue
                identity = (device, engine)
                capacity = finite(client.get("capacities", {}).get(engine, 1))
                if capacity is not None and capacity > 0:
                    sums[identity] = sums.get(identity, 0) + value - before
                    capacities[identity] = max(capacities.get(identity, 1), capacity)
                    known.add(device)
        for key in set(self.previous) - set(clients):
            changed.add(key[0])
        for card in cards:
            if card.get("driver") not in ("i915", "xe"):
                continue
            device = card.get("pci_bus")
            card["utilization_kind"] = "busiest-engine"
            card["utilization_pct"] = None
            if not complete:
                card["error"] = "DRM engine counter scan incomplete"
            elif device in decreased:
                card["error"] = "DRM counters decreased; waiting for catchup"
            elif elapsed is None or elapsed <= 0 or device not in known:
                card["error"] = "Collecting DRM engine baseline"
            else:
                percentages = [delta / (elapsed * 1e9 * capacities.get((dev, engine), 1)) * 100
                               for (dev, engine), delta in sums.items() if dev == device]
                card["utilization_pct"] = min(100, max(0, max(percentages))) if percentages else None
                card["error"] = "DRM client coverage changed; partial engine load" if device in changed else None
        if complete and now is not None:
            self.previous, self.mono = clients, now


class GuestReader:
    """No concurrent repeated guest exec: retain pending PID across timeouts."""
    def __init__(self, vmid, runner):
        self.vmid, self.runner = vmid, runner
        self.os_kind, self.pending_pid = None, None
        self.drm = DRMRates()
        self.epoch = 0
        self.state_lock = threading.Lock()

    def reset(self, restarted=False):
        with self.state_lock:
            self.epoch += 1
            self.os_kind = None
            self.drm = DRMRates()
            if restarted:
                self.pending_pid = None  # Previous-boot process cannot still exist.

    def check_epoch(self, epoch):
        # Call while holding state_lock before any probe state is committed.
        if epoch != self.epoch:
            raise RuntimeError("Guest changed while telemetry probe was running")

    def read(self):
        with self.state_lock:
            epoch, pending_pid, os_kind = self.epoch, self.pending_pid, self.os_kind
        if pending_pid is None:
            if os_kind is None:
                info = qga_command(self.vmid, "get-osinfo", self.runner)
                os_kind = "windows" if info.get("id") in ("mswindows", "windows") or "windows" in str(info.get("name", "")).lower() else "linux"
                with self.state_lock:
                    self.check_epoch(epoch)
                    self.os_kind = os_kind
            with self.state_lock:
                self.check_epoch(epoch)
            launched = qga_command(self.vmid, f"exec-{os_kind}", self.runner)
            if launched.get("pid") is None:
                raise RuntimeError("Guest telemetry launch returned no process ID")
            pending_pid = int(launched["pid"])
            with self.state_lock:
                self.check_epoch(epoch)
                self.pending_pid = pending_pid
        deadline = time.monotonic() + 6
        while True:
            try:
                with self.state_lock:
                    self.check_epoch(epoch)
                result = qga_command(self.vmid, "exec-status", self.runner, pending_pid)
            except RuntimeError as exc:
                if re.search(r"(?:pid|process).*(?:not found|not exist)|invalid.*pid", str(exc), re.I):
                    with self.state_lock:
                        self.check_epoch(epoch)
                        self.pending_pid = None
                raise
            with self.state_lock:
                self.check_epoch(epoch)
            if result.get("exited") or time.monotonic() >= deadline:
                break
            time.sleep(.2)
        if not result.get("exited"):
            if result.get("pid") is not None:
                with self.state_lock:
                    self.check_epoch(epoch)
                    self.pending_pid = int(result["pid"])
            raise RuntimeError("Guest telemetry query still running")
        with self.state_lock:
            self.check_epoch(epoch)
            self.pending_pid = None
        if result.get("exitcode") != 0 or result.get("out-truncated") or result.get("err-truncated"):
            with self.state_lock:
                self.check_epoch(epoch)
                self.os_kind = None
            raise RuntimeError("Guest telemetry query failed/truncated")
        try:
            if len(result.get("out-data", "").encode("utf-8")) > QGA_OUTPUT_LIMIT:
                raise ValueError("Guest telemetry output exceeds output bound")
            parsed = parse_document(json.loads(result.get("out-data", "")))
            with self.state_lock:
                self.check_epoch(epoch)
                self.drm.apply(parsed["gpus"], parsed.get("drm_stats"))
            return parsed
        except (ValueError, TypeError):
            with self.state_lock:
                self.check_epoch(epoch)
                self.os_kind = None
            raise
