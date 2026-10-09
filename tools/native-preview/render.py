#!/usr/bin/env python3
"""Build and capture the actual LVGL firmware UI without an ESP32 or network.

Usage: python3 tools/native-preview/render.py --snapshot output/live/snapshot.json
"""
from __future__ import annotations
import argparse
import datetime
import hashlib
import html
import json
import math
import os
import re
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[2]


def configured_rotation():
    """Use the firmware default unless the caller explicitly selects a rotation."""
    config = (ROOT / "firmware/include/config.h").read_text()
    match = re.search(r"^#define\s+HOMELAB_ROTATION\s+([0-3])\s*$", config, re.MULTILINE)
    if not match:
        raise ValueError("Cannot determine firmware HOMELAB_ROTATION; set --rotation explicitly")
    return int(match.group(1))


def viewport(rotation):
    return (240, 320) if rotation % 2 == 0 else (320, 240)


def scalar(value):
    if value is None or isinstance(value, bool):
        return "NAN"
    return repr(value) if isinstance(value, (int, float)) and math.isfinite(value) else "NAN"


def fixture(document, path):
    fingerprint = hashlib.sha256(json.dumps(document, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
    lines = ["#pragma once", f'inline const char *fixtureFingerprint() {{ return "{fingerprint}"; }}', "inline void loadFixture(Snapshot &s, NetworkState &n) {",
             "new (&s) Snapshot{}; n = NetworkState{}; s.valid = true;",
             "n.configured = true; n.wifi = true; s.sequence = 1;",
             'strlcpy(n.message, "Saved fixture rendered locally", sizeof(n.message));',
             'strlcpy(n.ip, "192.0.2.240", sizeof(n.ip));']
    def text(target, value):
        lines.append(f"strlcpy({target}, {json.dumps(str(value or ''), ensure_ascii=False)}, sizeof({target}));")
    def num(target, value):
        lines.append(f"{target} = {scalar(value)};")
    def descriptor(target, node):
        for field in ("id", "type", "platform", "name", "address", "status"):
            text(target + "." + field, node.get(field))
        summary = node.get("summary")
        if isinstance(summary, dict):
            lines.append(target + ".summary.available = true;")
            for key, field in {"cpu_percent":"cpu", "memory_percent":"memory", "temperature_c":"temp", "progress_percent":"progress", "generated_at":"generated", "age_s":"age"}.items():
                num(target + ".summary." + field, summary.get(key))
            text(target + ".summary.printState", summary.get("print_state"))
            text(target + ".summary.status", summary.get("status", node.get("status")))
            ttl = summary.get("ttl_s", 15)
            num(target + ".summary.ttl", ttl if isinstance(ttl, (float, int)) and not isinstance(ttl, bool) and math.isfinite(ttl) and 5 <= ttl <= 900 else 15)
            for key in ("sensors", "guests"):
                value = summary.get(key)
                if isinstance(value, (float, int)) and not isinstance(value, bool) and math.isfinite(value) and 0 <= value <= 65535 and int(value) == value:
                    lines.append(f"{target}.summary.{key} = {int(value)};")
    lines.append(f"s.demo = {'true' if document.get('demo') else 'false'};")
    node=document.get("node", {})
    descriptor("s.node", node)
    nodes=document.get("nodes",[])[:4];lines.append(f"s.nNodes = {len(nodes)};")
    for i,node in enumerate(nodes):
        descriptor(f"s.nodes[{i}]", node)
    printer=document.get("printer")
    if isinstance(printer,dict):
        lines.append("s.printer.present = true;")
        for key,field in {"id":"id","name":"name","host":"host","host_name":"hostName","klippy_state":"klippyState","state":"state","message":"message","filename":"filename","progress_basis":"progressBasis","eta_basis":"etaBasis","error":"error"}.items():text("s.printer."+field,printer.get(key))
        for key,field in {"progress_pct":"progress","fan_pct":"fan","print_duration_s":"printDuration","total_duration_s":"totalDuration","current_layer":"currentLayer","total_layers":"totalLayers","filament_used_mm":"filament","slicer_estimated_time_s":"slicerTime","remaining_s":"remaining","eta_at":"eta","updated_at":"updated","age_s":"age"}.items():num("s.printer."+field,printer.get(key))
        ttl=printer.get("ttl_s",15);num("s.printer.ttl",ttl if isinstance(ttl,(float,int)) and not isinstance(ttl,bool) and math.isfinite(ttl) and 5<=ttl<=900 else 15)
        detected=printer.get("filament_detected");lines.append(f"s.printer.filamentDetected = {1 if detected is True else 0 if detected is False else -1};")
        heaters=printer.get("heaters",[])[:8];lines.append(f"s.printer.nHeaters = {len(heaters)};")
        for i,h in enumerate(heaters):
            text(f"s.printer.heaters[{i}].name",h.get("name"))
            for key,field in {"temp_c":"temp","target_c":"target","duty_pct":"duty"}.items():num(f"s.printer.heaters[{i}]."+field,h.get(key))
        temperatures=printer.get("temperatures",[])[:16];lines.append(f"s.printer.nTemps = {len(temperatures)};")
        for i,t in enumerate(temperatures):text(f"s.printer.temperatures[{i}].name",t.get("name"));num(f"s.printer.temperatures[{i}].temp",t.get("temp_c"))
    host = document.get("host", {})
    text("s.host", host.get("name", "Device")); text("s.ip", host.get("ip", ""))
    platform = document.get("platform") or {}
    for field, value in {"osRelease":host.get("os_release", platform.get("release")), "osVersion":host.get("os_version", platform.get("version")), "architecture":host.get("architecture", platform.get("architecture")), "uptimeScope":platform.get("uptime_scope")}.items(): text("s." + field, value)
    count = host.get("cpu_count", platform.get("cpu_count"))
    if isinstance(count, int) and not isinstance(count, bool) and 0 < count <= 65535: lines.append(f"s.cpuCount = {count};")
    battery = platform.get("battery") or {}
    num("s.batteryPercent", battery.get("percent")); num("s.batteryRemaining", battery.get("seconds_left"))
    if isinstance(battery.get("power_plugged"), bool): lines.append(f"s.batteryPlugged = {1 if battery['power_plugged'] else 0};")
    interfaces = platform.get("interfaces", [])[:16]; lines.append(f"s.nInterfaces = {len(interfaces)};")
    for i, interface in enumerate(interfaces):
        text(f"s.interfaces[{i}].name", interface.get("name")); text(f"s.interfaces[{i}].status", interface.get("status", "unknown"))
        for key, field in {"speed_mbps":"speed", "rx_bps":"rx", "tx_bps":"tx", "errors_in":"errorsIn", "errors_out":"errorsOut", "drops_in":"dropsIn", "drops_out":"dropsOut"}.items(): num(f"s.interfaces[{i}]." + field, interface.get(key))
    for key, field in {"uptime_s":"uptime", "cpu_pct":"cpu", "mem_used_bytes":"memUsed", "mem_total_bytes":"memTotal", "swap_used_bytes":"swapUsed", "swap_total_bytes":"swapTotal", "arc_bytes":"arc", "io_wait_pct":"iowait", "net_rx_bps":"rx", "net_tx_bps":"tx", "disk_read_bps":"read", "disk_write_bps":"write"}.items():
        num("s." + field, host.get(key))
    for i, value in enumerate(host.get("load", [])[:3]): num(f"s.load[{i}]", value)
    cores = host.get("cpu_cores", [])[:64]
    lines.append(f"s.nCores = {len(cores)};")
    for i, value in enumerate(cores): num(f"s.cores[{i}]", value)
    power = document.get("power", {})
    for key, field in {"package_w":"watts", "cpu_temp_c":"temp", "cpu_mhz":"mhz", "busy_mhz":"busyMhz"}.items(): num("s." + field, power.get(key))
    cstates = list(power.get("cstate_pct", {}).items())[:10]
    lines.append(f"s.nCstates = {len(cstates)};")
    for i, (key, value) in enumerate(cstates): text(f"s.cstateNames[{i}]", key); num(f"s.cstates[{i}]", value)
    definitions = [
        ("guests", "guests", "nGuests", 48, {"name":"name", "type":"type", "status":"status", "memory_basis":"memoryBasis", "mem_error":"memError"}, {"id":"id", "cpu_pct":"cpu", "mem_used_bytes":"used", "mem_total_bytes":"total", "uptime_s":"uptime", "net_rx_bps":"rx", "net_tx_bps":"tx", "disk_read_bps":"read", "disk_write_bps":"write", "mem_available_bytes":"available", "mem_cache_bytes":"cache", "mem_noncache_used_bytes":"noncache", "mem_assigned_bytes":"assigned", "mem_host_bytes":"hostMem", "pve_mem_used_bytes":"pveUsed", "pve_mem_total_bytes":"pveTotal", "mem_updated_at":"memUpdated", "mem_age_s":"memAge"}),
        ("gpus", "gpus", "nGpus", 8, {"id":"id", "name":"name", "vendor":"vendor", "kind":"kind", "owner":"owner", "driver":"driver", "status":"status", "utilization_kind":"utilizationKind", "error":"error"}, {"utilization_pct":"utilization", "mem_used_bytes":"memUsed", "mem_total_bytes":"memTotal", "temp_c":"temp", "power_w":"power", "graphics_mhz":"graphicsMhz", "memory_mhz":"memoryMhz", "fan_pct":"fan", "updated_at":"updated", "age_s":"age"}),
        ("storage", "pools", "nPools", 16, {"id":"id", "name":"name", "status":"status"}, {"used_bytes":"used", "total_bytes":"total"}),
        ("disks", "disks", "nDisks", 16, {"name":"name", "model":"model", "health":"health", "status":"status", "zfs_status":"zfsStatus"}, {"temp_c":"temp", "read_bps":"read", "write_bps":"write", "wear_pct":"wear", "media_errors":"mediaErrors", "power_on_hours":"powerHours", "spare_pct":"spare", "reallocated_sectors":"reallocated", "pending_sectors":"pending", "read_errors":"zfsReadErrors", "write_errors":"zfsWriteErrors", "checksum_errors":"zfsChecksumErrors"}),
        ("sensors", "sensors", "nSensors", 64, {"id":"id", "name":"name", "chip":"chip", "kind":"kind", "unit":"unit"}, {"value":"value", "crit":"crit", "high":"high"}),
        ("alerts", "alerts", "nAlerts", 24, {"id":"id", "severity":"severity", "message":"message"}, {}),
    ]
    for key, array, count, limit, texts, numbers in definitions:
        values = document.get(key, [])[:limit]
        lines.append(f"s.{count} = {len(values)};")
        for i, value in enumerate(values):
            for source, field in texts.items():
                item = value.get(source)
                if key == "guests" and source == "memory_basis" and not isinstance(item,str): item = "proxmox"
                if key == "gpus" and source in ("kind","owner","status","utilization_kind") and not isinstance(item,str): item = {"kind":"unknown","owner":"unknown","status":"inventory","utilization_kind":"gpu"}[source]
                if key == "disks" and source in ("health", "status") and not isinstance(item, str): item = "unknown"
                text(f"s.{array}[{i}].{field}", item)
            for source, field in numbers.items():
                if source == "id": lines.append(f"s.{array}[{i}].{field} = {int(value.get(source, 0))};")
                else: num(f"s.{array}[{i}].{field}", value.get(source))
    sources = list(document.get("sources", {}).items())[:16]
    lines.append(f"s.nSources = {len(sources)};")
    for i, (key, value) in enumerate(sources):
        text(f"s.sources[{i}].name", key); text(f"s.sources[{i}].error", value.get("error"))
        lines.append(f"s.sources[{i}].ok = {'true' if value.get('ok') else 'false'};")
        lines.append(f"s.sources[{i}].enabled = {'false' if value.get('enabled') is False else 'true'};")
        num(f"s.sources[{i}].age", value.get("age_s")); num(f"s.sources[{i}].updated", value.get("updated_at"))
    faults = document.get("faults") or {}
    lines.append(f"s.faultLookbackDays = {int(faults.get('lookback_days', 7))};")
    for key, field in {"segfault_count_24h":"segfaults24h", "event_count_24h":"faultEvents24h", "last_event_at":"lastFaultAt"}.items(): num("s." + field, faults.get(key))
    events = faults.get("events", [])[:16]
    lines.append(f"s.nFaults = {len(events)};")
    for i, value in enumerate(events):
        for key in ("id", "kind", "message"): text(f"s.faults[{i}].{key}", value.get(key))
        num(f"s.faults[{i}].timestamp", value.get("timestamp"))
    lines.append("s.truncated = " + ("true" if (any(document.get("limits", {}).get("truncated", {}).values()) or len(platform.get("interfaces", [])) > 16) else "false") + ";")
    # Match network.cpp's thermal-first stable ordering used on the display.
    lines.append('auto rank=[](const char *kind){return !strcmp(kind,"temperature")?0:!strcmp(kind,"fan")?1:!strcmp(kind,"voltage")?2:!strcmp(kind,"power")?3:!strcmp(kind,"current")?4:5;};')
    lines.append('std::stable_sort(s.sensors,s.sensors+s.nSensors,[&](const Sensor&a,const Sensor&b){int delta=rank(a.kind)-rank(b.kind);return delta?delta<0:strcmp(a.chip,b.chip)<0;});')
    lines.append("}")
    lines.append("inline void loadConfigurationFixture(NodeConfiguration &c) {")
    lines.append("c = NodeConfiguration{};")
    config=document.get('_configuration')
    if isinstance(config,dict):
        lines.append("c.available = true;")
        text('c.version',config.get('version'))
        local=config.get('local_node',{})
        for key,field in {'id':'localId','name':'localName','address':'localAddress','type':'localType','platform':'localPlatform'}.items():text('c.'+field,local.get(key))
        lines.append(f"c.localEnabled = {'true' if local.get('enabled') else 'false'};")
        nodes=config.get('nodes',[])[:4];lines.append(f"c.count = {len(nodes)};")
        for i,node in enumerate(nodes):
            for key in ('id','type','platform','origin','name','url'):
                value = node.get(key)
                if key == 'type' and value in {'server', 'proxmox'}:
                    value = ('local-' + value) if node.get('origin') in {'host', 'local'} else value + '-feed'
                text(f'c.nodes[{i}].'+key,value)
            for key,field in {'poll_interval_s':'poll','timeout_s':'timeout','ttl_s':'ttl'}.items():num(f'c.nodes[{i}].'+field,node.get(key))
            lines.append(f"c.nodes[{i}].hasSecret = {'true' if node.get('has_secret') else 'false'};")
    lines.append("}")
    path.write_text("\n".join(lines) + "\n")
    return fingerprint


def png(ppm):
    header, width_height, max_color, raw = ppm.read_bytes().split(b"\n", 3)
    if header != b"P6" or max_color != b"255": raise ValueError("Expected RGB PPM")
    width, height = map(int, width_height.split())
    def chunk(kind, data): return struct.pack(">I",len(data)) + kind + data + struct.pack(">I",zlib.crc32(kind+data)&0xffffffff)
    scanlines = b"".join(b"\0" + raw[y*width*3:(y+1)*width*3] for y in range(height))
    result = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR",struct.pack(">IIBBBBB",width,height,8,2,0,0,0)) + chunk(b"IDAT",zlib.compress(scanlines,9)) + chunk(b"IEND",b"")
    ppm.with_suffix(".png").write_bytes(result); ppm.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", type=Path, default=ROOT/"examples/snapshots/host.json")
    parser.add_argument("--output", type=Path, default=ROOT/"output/native")
    parser.add_argument("--build-dir", type=Path)
    parser.add_argument("--lvgl-source", type=Path, default=Path(os.environ["LVGL_SOURCE_DIR"]) if "LVGL_SOURCE_DIR" in os.environ else None)
    parser.add_argument("--rotation",type=int,choices=range(4),default=configured_rotation(),help="Display rotation; defaults to firmware config (0/2 portrait, 1/3 landscape)")
    parser.add_argument("--pages", help="Capture selected page numbers only, e.g. 0,3,6")
    parser.add_argument("--config",type=Path,help="Saved public GET /api/v1/config projection; secrets must be absent")
    args = parser.parse_args()
    width, height = viewport(args.rotation)
    if args.lvgl_source is None:
        candidates = [ROOT/"firmware/.pio/libdeps/homelab_s3/lvgl"]
        args.lvgl_source = next((candidate for candidate in candidates if (candidate/"CMakeLists.txt").is_file()), None)
    if args.lvgl_source is None: parser.error("Set --lvgl-source to LVGL 9.3.0 sources (or run a firmware build first)")
    if args.build_dir is None:
        output_key = hashlib.sha256(str(args.output.resolve()).encode()).hexdigest()[:10]
        args.build_dir = Path(tempfile.gettempdir()) / ("homelab-native-preview-" + output_key + "-r" + str(args.rotation))
    args.build_dir.mkdir(parents=True, exist_ok=True)
    document = json.loads(args.snapshot.read_text())
    if args.config:
        config=json.loads(args.config.read_text())
        def has_secret_fields(v):
            if isinstance(v,dict):return any(k in {'secret','password','token','setup_token','display_token'} or has_secret_fields(x) for k,x in v.items())
            return isinstance(v,list) and any(has_secret_fields(x) for x in v)
        if has_secret_fields(config):parser.error('Config input must contain only the public projection, without secrets')
        document['_configuration']=config
    if document.get("schema") != 1: parser.error("Input must be a schema-1 collector snapshot")
    fingerprint = fixture(document, args.build_dir/"fixture.h")
    # Apple make can miss header changes within the same timestamp second.
    # Force the one fixture-dependent translation unit to rebuild every run.
    for suffix in ("o", "obj"):
        object_file = args.build_dir / "CMakeFiles/native-preview.dir" / ("renderer.cpp." + suffix)
        object_file.unlink(missing_ok=True)
    commands = [["cmake","-S",str(Path(__file__).parent),"-B",str(args.build_dir),f"-DLVGL_SOURCE_DIR={args.lvgl_source.resolve()}","-DCMAKE_BUILD_TYPE=Release",f"-DHOMELAB_ROTATION={args.rotation}"], ["cmake","--build",str(args.build_dir),"--target","native-preview","-j","8"]]
    log = args.build_dir/"build.log"
    with log.open("w") as stream:
        for command in commands:
            result = subprocess.run(command,stdout=stream,stderr=subprocess.STDOUT)
            if result.returncode: print(log.read_text()[-12000:],file=sys.stderr); return result.returncode
    command = [str(args.build_dir/"native-preview"),str(args.output.resolve())]
    if args.pages: command.append(args.pages)
    result = subprocess.run(command, stdout=subprocess.PIPE, text=True)
    print(result.stdout, end="")
    if result.returncode: return result.returncode
    if ("fixture_sha256=" + fingerprint) not in result.stdout:
        print("Rendered fixture fingerprint does not match input; rebuild the native cache", file=sys.stderr)
        return 1
    if f"viewport={width}x{height} rotation={args.rotation}" not in result.stdout:
        print("Native viewport does not match selected rotation",file=sys.stderr)
        return 1
    for ppm in args.output.glob("*.ppm"): png(ppm)
    captures = [line.split(".ppm:", 1)[0] for line in result.stdout.splitlines() if ".ppm:" in line]
    (args.output/"snapshot.json").write_text(json.dumps(document, ensure_ascii=False, indent=2)+"\n")
    (args.output/"memory.txt").write_text(result.stdout)
    (args.output/"provenance.json").write_text(json.dumps({"renderer":"Actual firmware/src/main.cpp + LVGL 9.3 software renderer", "snapshot":str(args.snapshot.resolve()),"demo":bool(document.get("demo")),"native_resolution":[width,height],"rotation":args.rotation,"board_sha256":hashlib.sha256((ROOT/"firmware/src/board.h").read_bytes()).hexdigest(),"hardware_tested":False,"captured_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"fixture_sha256":fingerprint,"firmware_sha256":hashlib.sha256((ROOT/"firmware/src/main.cpp").read_bytes()).hexdigest(),"model_sha256":hashlib.sha256((ROOT/"firmware/src/model.h").read_bytes()).hexdigest(),"lv_conf_sha256":hashlib.sha256((ROOT/"firmware/include/lv_conf.h").read_bytes()).hexdigest(),"captures":captures},indent=2)+"\n")
    title = "Actual firmware UI / " + ("demo" if document.get("demo") else str(document.get("host",{}).get("name","saved snapshot")))
    cards = "".join(f'<figure><a href="{name}.png"><img src="{name}.png" width="{width}" height="{height}" alt="{name.replace("-"," ")}"></a><figcaption>{name.replace("-"," ")}</figcaption></figure>' for name in captures)
    gallery = '<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>'+html.escape(title)+'</title><style>body{margin:0;background:#f2f5f8;color:#101e30;font:16px system-ui;padding:28px}h1{font-size:24px;margin:0 0 8px}p{color:#526275;margin:0 0 28px}main{display:grid;grid-template-columns:repeat(auto-fit,PREVIEW_WIDTHpx);gap:26px}figure{margin:0}img{display:block;border-radius:12px;outline:1px solid #dce5ed}figcaption{color:#526275;font-size:13px;margin-top:10px;text-transform:capitalize}a{color:inherit}</style><h1>'+html.escape(title)+'</h1><p>Production LVGL renderer · PREVIEW_WIDTH × PREVIEW_HEIGHT RGB565 · Rotation PREVIEW_ROTATION · Device hardware verification pending</p><main>'+cards+'</main></html>\n'
    (args.output/"index.html").write_text(gallery.replace("PREVIEW_WIDTH",str(width)).replace("PREVIEW_HEIGHT",str(height)).replace("PREVIEW_ROTATION",str(args.rotation)))
    print(f"Native UI PNGs: {args.output}")
    return 0

if __name__ == "__main__": raise SystemExit(main())
