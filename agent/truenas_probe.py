#!/usr/bin/env python3
"""Fixed read-only TrueNAS SSH command; deploy under a restricted display key.

No arguments, incoming shell commands, credentials, or system configuration are
read. Four fixed middleware queries expose pools, disks, cached temperatures,
and active alerts. /proc/diskstats provides cumulative physical IO counters.
An unprivileged NAS admin runs these queries without sudo or SMART-device
access; pool ONLINE means ZFS health and never implies SMART passed.
"""
from __future__ import annotations

import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import time


def finite(value):
    if value is None or isinstance(value, bool):
        return None
    try:
        result = float(value)
        return result if math.isfinite(result) else None
    except (ValueError, TypeError):
        return None


def text(value, limit=64):
    return str(value or "").replace("\n", " ").replace("\r", " ")[:limit]


def query(method):
    result = subprocess.run(["midclt", "call", method], capture_output=True, text=True,
                            timeout=3, check=False, env={**os.environ, "LC_ALL": "C"}, stdin=subprocess.DEVNULL)
    if result.returncode:
        raise RuntimeError(f"{method}: query failed ({result.returncode})")
    return json.loads(result.stdout)


def disk_counters(path=Path("/proc/diskstats")):
    result = {}
    try:
        for line in path.read_text().splitlines():
            fields = line.split()
            if len(fields) >= 10:
                result[fields[2]] = {"read_bytes": int(fields[5]) * 512, "write_bytes": int(fields[9]) * 512}
    except (OSError, ValueError):
        pass
    return result


def os_memory(proc_root=Path("/proc")):
    values = {}
    for line in (proc_root / "meminfo").read_text().splitlines():
        key, _, rest = line.partition(":")
        fields = rest.split()
        if fields:
            values[key] = int(fields[0]) * (1024 if len(fields) > 1 and fields[1] == "kB" else 1)
    total, available = values.get("MemTotal"), values.get("MemAvailable")
    if total is None or total <= 0 or available is None or not 0 <= available <= total:
        raise ValueError("OS memory total/available invalid")
    arc = None
    try:
        for line in (proc_root / "spl/kstat/zfs/arcstats").read_text().splitlines():
            fields = line.split()
            if len(fields) == 3 and fields[0] == "size":
                arc = int(fields[2])
    except OSError:
        pass
    if arc is not None and not 0 <= arc <= total:
        raise ValueError("OS ARC size invalid")
    used = total - available
    return {"memory_basis": "guest-os-arc", "mem_total_bytes": total, "mem_available_bytes": available,
            "mem_used_bytes": used, "mem_cache_bytes": arc,
            "mem_noncache_used_bytes": max(0, used - arc) if arc is not None else None}


def build_snapshot(pools, disks, temperatures, alerts, counters=None):
    """Reduce middleware objects before they leave the NAS; no serials/paths."""
    disk_topology = {}
    pool_list = []
    for pool in pools:
        leaves = []
        def walk(vdev):
            children = vdev.get("children") or []
            if children:
                for child in children:
                    walk(child)
            elif vdev.get("type") == "DISK":
                leaves.append(vdev)
                name = vdev.get("disk") or vdev.get("device")
                if name:
                    disk_topology[name] = vdev
        for group in (pool.get("topology") or {}).values():
            for vdev in group or []:
                walk(vdev)
        errors = {key: sum(finite(v.get("stats", {}).get(key)) or 0 for v in leaves)
                  for key in ("read_errors", "write_errors", "checksum_errors")}
        scan = pool.get("scan") or {}
        pool_list.append({"name": text(pool.get("name")), "used_bytes": finite(pool.get("allocated")),
                          "total_bytes": finite(pool.get("size")), "status": text(pool.get("status") or "unknown", 24),
                          "healthy": pool.get("healthy") if isinstance(pool.get("healthy"), bool) else None,
                          "scrub_state": text(scan.get("state"), 24), "scrub_errors": finite(scan.get("errors")), **errors})
    disk_list = []
    counters = counters or {}
    for disk in disks:
        name = disk.get("name") or disk.get("devname")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
            continue
        # Virtual boot disks are redundant with Proxmox VM metrics. Keep all
        # actual passed-through drives, including non-pool/spare members.
        if "QEMU" in (disk.get("model") or "").upper():
            continue
        vdev = disk_topology.get(name, {})
        stats = vdev.get("stats", {})
        disk_list.append({"name": text(name), "model": text(disk.get("model")),
                          "temp_c": finite(temperatures.get(name)), "health": None, "status": "unknown",
                          "zfs_status": text(vdev.get("status") or "unknown", 24),
                          "read_errors": finite(stats.get("read_errors")),
                          "write_errors": finite(stats.get("write_errors")),
                          "checksum_errors": finite(stats.get("checksum_errors")),
                          "size_bytes": finite(disk.get("size")), "temperature_source": "truenas-cache",
                          **counters.get(name, {})})
    alert_list = []
    for alert in alerts:
        if alert.get("dismissed"):
            continue
        level = str(alert.get("level", "")).upper()
        if level not in ("WARN", "WARNING", "CRITICAL", "ALERT", "EMERGENCY", "ERROR"):
            continue
        alert_list.append({"id": text(alert.get("uuid") or alert.get("source") or alert.get("klass") or "alert", 80),
                           "severity": "warning" if level in ("WARN", "WARNING") else "critical",
                           "message": text(alert.get("formatted") or alert.get("text") or alert.get("source"), 120)})
    return {"schema": 1, "generated_at": round(time.time(), 3), "pools": pool_list[:16],
            "disks": disk_list[:16], "alerts": alert_list[:24],
            "counts": {"pools": len(pool_list), "disks": len(disk_list)},
            "temperature_cache_interval_s": 300, "smart_available": False}


def main():
    if len(sys.argv) != 1:
        raise SystemExit("This read-only probe takes no arguments")
    values, errors = {}, []
    for key, method, empty in (("pools", "pool.query", []), ("disks", "disk.query", []),
                               ("temperatures", "disk.temperatures", {}), ("alerts", "alert.list", [])):
        try:
            values[key] = query(method)
        except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as exc:
            values[key] = empty
            errors.append(text(exc, 120))
    document = build_snapshot(**values, counters=disk_counters())
    try:
        document["memory"], document["memory_error"] = os_memory(), None
    except (OSError, ValueError) as exc:
        document["memory"], document["memory_error"] = None, text(exc, 120)
    try:
        document["boot_id"] = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
    except OSError:
        document["boot_id"] = None
    document["error"] = "; ".join(errors) or None
    raw = json.dumps(document, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    if len(raw.encode()) > 60 * 1024:
        raise SystemExit("Probe payload too large")
    print(raw)


if __name__ == "__main__":
    main()
