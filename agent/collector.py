#!/usr/bin/env python3
"""Linux/Proxmox telemetry collector. Uses only Python's standard library.

The root service reads privileged tools; a separate, unprivileged HTTP service
reads its atomic JSON snapshot. External commands are GET/read operations only.
CPU/IO/network rates are deltas, never boot totals disguised as rates. Unknown
values are JSON null. Slow probes run off the publication loop with at most one
in-flight probe per source; source freshness describes retained samples.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import dataclasses
import fcntl
import grp
import hashlib
import ipaddress
import json
import logging
import math
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import tempfile
import threading
import time
from typing import Any, Callable
from urllib.parse import urlsplit

LOG = logging.getLogger("homelab.collector")
MAX_PAYLOAD = 48 * 1024
MAX_AGGREGATE = 256 * 1024
MAX_NODES = 4
CAPS = {"guests": 48, "sensors": 64, "storage": 16, "disks": 16, "gpus": 8}
HOST_FIELDS = ("uptime_s cpu_pct load mem_used_bytes mem_total_bytes swap_used_bytes "
               "swap_total_bytes arc_bytes io_wait_pct net_rx_bps net_tx_bps "
               "disk_read_bps disk_write_bps").split()


@dataclasses.dataclass
class Config:
    host_ip: str = "192.0.2.10"
    node: str = ""
    display_name: str = ""
    enable_proxmox: bool = True
    printers: list[dict[str, Any]] = dataclasses.field(default_factory=list)
    remote_collectors: list[dict[str, Any]] = dataclasses.field(default_factory=list)
    expected_running_guests: list[int] = dataclasses.field(default_factory=list)
    truenas_ssh_host: str = ""
    truenas_ssh_user: str = "truenas_admin"
    truenas_ssh_key: str = "/etc/homelab-monitor/truenas_ed25519"
    truenas_known_hosts: str = "/etc/homelab-monitor/truenas_known_hosts"
    truenas_interval_s: float = 30
    truenas_guest_id: int = 101
    qga_guest_ids: list[int] = dataclasses.field(default_factory=list)
    guest_interval_s: float = 15
    enable_gpus: bool = True
    gpu_interval_s: float = 30
    enable_faults: bool = True
    faults_interval_s: float = 30
    interval_s: float = 3
    network_interfaces: list[str] = dataclasses.field(default_factory=list)
    disk_devices: list[str] = dataclasses.field(default_factory=list)
    smart_devices: list[str] = dataclasses.field(default_factory=list)
    enable_turbostat: bool = True
    enable_smart: bool = True
    enable_zfs: bool = True
    sensor_interval_s: float = 6
    proxmox_interval_s: float = 6
    turbostat_interval_s: float = 6
    smart_interval_s: float = 300
    zfs_interval_s: float = 30
    temperature_warning_c: float = 80
    temperature_critical_c: float = 90
    disk_temperature_warning_c: float = 50
    disk_temperature_critical_c: float = 60
    memory_warning_pct: float = 90
    memory_critical_pct: float = 97
    storage_warning_pct: float = 85
    storage_critical_pct: float = 95
    io_wait_warning_pct: float = 20
    io_wait_critical_pct: float = 40

    @classmethod
    def read(cls, path: str | None) -> "Config":
        if not path:
            return cls()
        data = json.loads(Path(path).read_text())
        return cls.from_dict(data)

    @classmethod
    def from_dict(cls, data: dict) -> "Config":
        """Validate the complete file before a configuration writer commits it."""
        if not isinstance(data, dict):
            raise ValueError("Configuration must be a JSON object")
        allowed = {f.name for f in dataclasses.fields(cls)}
        unknown = set(data) - allowed
        if unknown:
            raise ValueError(f"Unknown configuration keys: {', '.join(sorted(unknown))}")
        result = cls(**data)
        from agent.printers import validate_printers
        from agent.remote_feeds import validate_remote_collectors
        result.printers = validate_printers(result.printers)
        result.remote_collectors = validate_remote_collectors(result.remote_collectors)
        if len(result.printers) + len(result.remote_collectors) + int(result.enable_proxmox is True) > MAX_NODES:
            raise ValueError("Configuration supports at most four enabled nodes")
        for key in ("interval_s", "sensor_interval_s", "proxmox_interval_s",
                    "turbostat_interval_s", "smart_interval_s", "zfs_interval_s", "truenas_interval_s", "faults_interval_s", "guest_interval_s", "gpu_interval_s"):
            value = getattr(result, key)
            if not isinstance(value, (int, float)) or not math.isfinite(value) or value < 1:
                raise ValueError(f"{key} must be a finite number >= 1")
        for key in ("network_interfaces", "disk_devices", "smart_devices"):
            values = getattr(result, key)
            if not isinstance(values, list) or not all(isinstance(v, str) for v in values):
                raise ValueError(f"{key} must be a list of strings")
        if not isinstance(result.expected_running_guests, list) or not all(type(v) is int and v > 0 for v in result.expected_running_guests):
            raise ValueError("expected_running_guests must be a list of positive guest IDs")
        if not isinstance(result.qga_guest_ids, list) or not all(type(v) is int and 100 <= v <= 999999999 for v in result.qga_guest_ids) or len(result.qga_guest_ids) > 48:
            raise ValueError("qga_guest_ids must contain <=48 valid VM IDs")
        result.qga_guest_ids = list(dict.fromkeys(result.qga_guest_ids))
        if type(result.truenas_guest_id) is not int or not 100 <= result.truenas_guest_id <= 999999999:
            raise ValueError("truenas_guest_id must be a valid VM ID")
        if result.truenas_ssh_host and result.truenas_guest_id in result.qga_guest_ids:
            raise ValueError("TrueNAS VM should use its SSH memory probe rather than two guest sources")
        for key in ("enable_proxmox", "enable_turbostat", "enable_smart", "enable_zfs", "enable_faults", "enable_gpus"):
            if not isinstance(getattr(result, key), bool):
                raise ValueError(f"{key} must be true or false")
        for base in ("temperature", "disk_temperature", "memory", "storage", "io_wait"):
            suffix = "c" if "temperature" in base else "pct"
            low = getattr(result, f"{base}_warning_{suffix}")
            high = getattr(result, f"{base}_critical_{suffix}")
            if not all(isinstance(v, (int, float)) and math.isfinite(v) for v in (low, high)) or low >= high:
                raise ValueError(f"{base} warning must be lower than critical")
        if not isinstance(result.host_ip, str) or not isinstance(result.node, str):
            raise ValueError("host_ip and node must be strings")
        try:
            ipaddress.ip_address(result.host_ip)
        except ValueError as exc:
            raise ValueError("host_ip must be a valid IP address") from exc
        if result.node and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}", result.node):
            raise ValueError("node must be a valid stable Proxmox node name")
        if not isinstance(result.display_name, str) or len(result.display_name) > 64 or any(ord(c) < 32 for c in result.display_name):
            raise ValueError("display_name must contain at most 64 display characters")
        if not isinstance(result.truenas_ssh_host, str) or (result.truenas_ssh_host and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.:-]*", result.truenas_ssh_host)):
            raise ValueError("truenas_ssh_host must be a hostname/IP address")
        if not isinstance(result.truenas_ssh_user, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_-]*", result.truenas_ssh_user):
            raise ValueError("truenas_ssh_user must be an SSH username")
        for field in ("truenas_ssh_key", "truenas_known_hosts"):
            if not isinstance(getattr(result, field), str) or not Path(getattr(result, field)).is_absolute():
                raise ValueError(f"{field} must be an absolute path")
        return result


def proxmox_node_id(name: str) -> str:
    """Stable ASCII selector within the display's 63-character ID capacity."""
    slug = re.sub(r"[^A-Za-z0-9_.-]", "_", name)
    if not slug or not slug[0].isalnum():
        slug = "node_" + slug
    if len(slug) > 55:
        slug = slug[:46] + "-" + hashlib.sha256(name.encode("utf-8")).hexdigest()[:8]
    return f"proxmox:{slug}"


def number(value: Any) -> float | None:
    """Convert valid finite telemetry to a number; reject boolean/NaN/infinity."""
    if value is None or isinstance(value, bool):
        return None
    try:
        result = float(value)
        return result if math.isfinite(result) else None
    except (ValueError, TypeError):
        return None


def compact_text(value: Any, length: int = 64) -> str:
    return str(value or "").replace("\n", " ").replace("\r", " ")[:length]


def command(argv: list[str], timeout: float = 5, allow_nonzero: bool = False) -> subprocess.CompletedProcess:
    """No shell, bounded execution, inherited locale fixed for numeric parsing."""
    env = {**os.environ, "LC_ALL": "C", "LANG": "C"}
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout, env=env, check=False)
    if result.returncode and not allow_nonzero:
        raise RuntimeError(f"{argv[0]} exit {result.returncode}: {compact_text(result.stderr, 160)}")
    return result


class Rates:
    """Tracks counters independently. A reset or a first sample yields null."""
    def __init__(self):
        self.previous: dict[str, tuple[float, float]] = {}

    def rate(self, key: str, counter: Any, now: float) -> float | None:
        value = number(counter)
        if value is None or value < 0:
            self.previous.pop(key, None)
            return None
        previous = self.previous.get(key)
        self.previous[key] = (value, now)
        if previous is None or now <= previous[1] or value < previous[0]:
            return None
        return round((value - previous[0]) / (now - previous[1]), 2)


def parse_cpu(text: str) -> dict[str, list[int]]:
    result = {}
    for line in text.splitlines():
        fields = line.split()
        if fields and re.fullmatch(r"cpu\d*", fields[0]):
            # guest and guest_nice are already part of user and nice.
            result[fields[0]] = [int(v) for v in fields[1:9]]
    if "cpu" not in result:
        raise ValueError("/proc/stat has no CPU counters")
    return result


def cpu_delta(current: list[int], previous: list[int] | None) -> tuple[float | None, float | None]:
    if previous is None or len(current) < 5 or len(previous) != len(current):
        return None, None
    delta = [a - b for a, b in zip(current, previous)]
    if any(v < 0 for v in delta) or sum(delta) <= 0:
        return None, None
    total = sum(delta)
    return round((total - delta[3] - delta[4]) * 100 / total, 2), round(delta[4] * 100 / total, 2)


def parse_mem(text: str) -> dict[str, int]:
    values = {}
    for line in text.splitlines():
        key, _, rest = line.partition(":")
        fields = rest.split()
        if fields:
            values[key] = int(fields[0]) * (1024 if len(fields) > 1 and fields[1] == "kB" else 1)
    if "MemTotal" not in values or "MemAvailable" not in values:
        raise ValueError("/proc/meminfo missing MemTotal/MemAvailable")
    return {"mem_total_bytes": values["MemTotal"],
            "mem_used_bytes": max(0, values["MemTotal"] - values["MemAvailable"]),
            "swap_total_bytes": values.get("SwapTotal"),
            "swap_used_bytes": max(0, values.get("SwapTotal", 0) - values.get("SwapFree", 0))}


def parse_net(text: str) -> dict[str, tuple[int, int]]:
    result = {}
    for line in text.splitlines():
        name, sep, counters = line.partition(":")
        if sep:
            fields = counters.split()
            if len(fields) >= 16:
                result[name.strip()] = (int(fields[0]), int(fields[8]))
    return result


def parse_diskstats(text: str) -> dict[str, tuple[int, int]]:
    result = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 10:
            # Linux diskstats sectors are always 512 bytes, even on 4K media.
            result[fields[2]] = (int(fields[5]) * 512, int(fields[9]) * 512)
    return result


class ProcReader:
    def __init__(self, config: Config, proc_root: Path = Path("/proc"), sys_root: Path = Path("/sys")):
        self.config, self.proc, self.sys = config, proc_root, sys_root
        self.cpu_previous: dict[str, list[int]] = {}
        self.rates = Rates()
        self.disk_rates: dict[str, tuple[float | None, float | None]] = {}
        self.interfaces: list[str] = []
        self.devices: list[str] = []

    def select_interfaces(self, available: dict[str, Any]) -> list[str]:
        if self.config.network_interfaces:
            missing = set(self.config.network_interfaces) - set(available)
            if missing:
                raise ValueError(f"Configured network interfaces absent: {', '.join(sorted(missing))}")
            return list(dict.fromkeys(self.config.network_interfaces))
        # Select physical interfaces/bonds, not vmbr + ports + veth together.
        candidates = []
        net = self.sys / "class/net"
        for name in sorted(available):
            path = net / name
            if name != "lo" and ((path / "device").exists() or (path / "bonding").exists()):
                # A bond and its slaves would otherwise be counted twice.
                if (path / "master/bonding").exists():
                    continue
                candidates.append(name)
        if candidates:
            return candidates
        # Virtual-only hosts: default route interface alone is the best fallback.
        try:
            for line in (self.proc / "net/route").read_text().splitlines()[1:]:
                fields = line.split()
                if len(fields) > 3 and fields[1] == "00000000" and fields[0] in available:
                    return [fields[0]]
        except OSError:
            pass
        return []

    def select_disks(self, available: dict[str, Any]) -> list[str]:
        if self.config.disk_devices:
            result = [Path(n).name for n in self.config.disk_devices]
            missing = set(result) - set(available)
            if missing:
                raise ValueError(f"Configured disk devices absent: {', '.join(sorted(missing))}")
            return list(dict.fromkeys(result))
        # /sys/block contains whole devices. Skip stacked dm/md and loop devices;
        # their IO is represented by physical members and would be double counted.
        return [name for name in sorted(available)
                if re.fullmatch(r"(?:sd[a-z]+|hd[a-z]+|vd[a-z]+|xvd[a-z]+|nvme\d+n\d+|mmcblk\d+)", name)
                and (self.sys / "block" / name).exists()]

    def read(self, now: float) -> dict[str, Any]:
        host = {key: None for key in HOST_FIELDS}
        host.update(name=socket.gethostname().split(".")[0], ip=self.config.host_ip, cpu_cores=[])
        errors = []
        try:
            current = parse_cpu((self.proc / "stat").read_text())
            host["cpu_pct"], host["io_wait_pct"] = cpu_delta(current["cpu"], self.cpu_previous.get("cpu"))
            host["cpu_cores"] = [cpu_delta(current[key], self.cpu_previous.get(key))[0]
                                 for key in sorted(current, key=lambda k: int(k[3:]) if k != "cpu" else -1)
                                 if key != "cpu"][:128]
            self.cpu_previous = current
        except (OSError, ValueError) as exc:
            self.cpu_previous = {}
            errors.append(f"cpu: {exc}")
        for filename, parser in (("meminfo", parse_mem),):
            try:
                host.update(parser((self.proc / filename).read_text()))
            except (OSError, ValueError) as exc:
                errors.append(f"memory: {exc}")
        for filename, key, count in (("uptime", "uptime_s", 1), ("loadavg", "load", 3)):
            try:
                values = [float(v) for v in (self.proc / filename).read_text().split()[:count]]
                host[key] = int(values[0]) if count == 1 else values
            except (OSError, ValueError, IndexError) as exc:
                errors.append(f"{filename}: {exc}")
        try:
            net = parse_net((self.proc / "net/dev").read_text())
            self.interfaces = self.select_interfaces(net)
            pairs = [tuple(self.rates.rate(f"net/{name}/{i}", value, now)
                           for i, value in enumerate(net[name])) for name in self.interfaces]
            for i, key in enumerate(("net_rx_bps", "net_tx_bps")):
                if pairs and all(p[i] is not None for p in pairs):
                    host[key] = round(sum(p[i] for p in pairs), 2)
            if not self.interfaces:
                errors.append("network: no physical/default interface; configure network_interfaces")
        except (OSError, ValueError) as exc:
            errors.append(f"network: {exc}")
        try:
            disks = parse_diskstats((self.proc / "diskstats").read_text())
            self.devices = self.select_disks(disks)
            self.disk_rates = {name: tuple(self.rates.rate(f"disk/{name}/{i}", value, now)
                                           for i, value in enumerate(disks[name])) for name in self.devices}
            for i, key in enumerate(("disk_read_bps", "disk_write_bps")):
                if self.disk_rates and all(p[i] is not None for p in self.disk_rates.values()):
                    host[key] = round(sum(p[i] for p in self.disk_rates.values()), 2)
        except (OSError, ValueError) as exc:
            errors.append(f"disk IO: {exc}")
        try:
            for line in (self.proc / "spl/kstat/zfs/arcstats").read_text().splitlines():
                fields = line.split()
                if fields and fields[0] == "size" and len(fields) == 3:
                    host["arc_bytes"] = int(fields[2])
        except (OSError, ValueError):
            pass  # No ZFS is a legitimate absence, not a fabricated zero.
        return {"host": host, "error": "; ".join(errors) or None}


def parse_sensors(data: dict[str, Any]) -> list[dict[str, Any]]:
    """Flatten sensors -j feature inputs, preserving labels and thresholds."""
    result = []
    kinds = {"temp": ("temperature", "°C"), "fan": ("fan", "RPM"),
             "in": ("voltage", "V"), "power": ("power", "W"), "curr": ("current", "A")}
    for chip, features in data.items():
        if not isinstance(features, dict):
            continue
        for label, readings in features.items():
            if not isinstance(readings, dict):
                continue
            for field, raw in readings.items():
                match = re.fullmatch(r"(temp|fan|in|power|curr)(\d+)_(input|average)", field)
                if not match:
                    continue
                prefix, index, metric = match.groups()
                if metric == "average" and f"{prefix}{index}_input" in readings:
                    continue
                value = number(raw)
                if value is None:
                    continue
                kind, unit = kinds[prefix]
                base = f"{prefix}{index}"
                alarm = any(number(readings.get(f"{base}_{suffix}")) == 1 for suffix in ("alarm", "crit_alarm", "max_alarm"))
                fault = number(readings.get(f"{base}_fault")) == 1
                critical = number(readings.get(f"{base}_crit"))
                high = None if chip.startswith("nct6687") else number(readings.get(f"{base}_max"))
                if kind == "temperature":
                    # Unsupported NVMe thresholds can be exposed as e.g.65261.
                    # Keep usable critical limits even when high is malformed.
                    critical = critical if critical is None or -40 <= critical <= 200 else None
                    high = high if high is None or -40 <= high <= 200 else None
                    if critical is not None and high is not None and high >= critical:
                        high = None
                result.append({"id": compact_text(f"{chip}/{base}", 96), "name": compact_text(label),
                               "chip": compact_text(chip), "kind": kind, "value": round(value, 3),
                               "unit": unit, "crit": critical,
                               # nct6687d publishes historical maxima here, not
                               # limits. Never convert its observed max to alarm.
                               "high": high,
                               "alarm": alarm, "fault": fault})
    return sorted(result, key=lambda s: (s["kind"] != "temperature", s["chip"], s["name"]))


def parse_turbostat(text: str) -> dict[str, Any]:
    header = None
    row = None
    for line in text.splitlines():
        fields = line.split()
        if "Avg_MHz" in fields or "Bzy_MHz" in fields or "PkgWatt" in fields:
            header = fields
        elif header and len(fields) == len(header) and any(number(v) is not None for v in fields):
            row = dict(zip(header, fields))
            break  # --Summary's first data row is the system summary.
    if not row:
        raise ValueError("No turbostat summary row")
    return {"package_w": number(row.get("PkgWatt")), "cores_w": number(row.get("CorWatt")),
            "graphics_w": number(row.get("GFXWatt")), "cpu_mhz": number(row.get("Avg_MHz")),
            "busy_mhz": number(row.get("Bzy_MHz")), "cpu_temp_c": number(row.get("PkgTmp")),
            "cstate_pct": {k: number(v) for k, v in row.items()
                           if re.fullmatch(r"(?:CPU%c\d+|Pkg%pc\d+|Pk%pc\d+|C\d+[A-Za-z]*%)", k) and number(v) is not None}}


def merge_power_sensors(snapshot: dict[str, Any], config: Config) -> list[dict[str, Any]]:
    """Expose existing component watts without mutating cached hwmon rows.

    RAPL package power and GPU board power are different component scopes,
    never whole-system power or inferred rail current. A failed/stale source
    must not turn a retained reading into a newly healthy sensor.
    """
    # Own rows are regenerated from source state if a caller reapplies this
    # merge; their previous value must not survive a subsequent source failure.
    result = [sensor.copy() for sensor in snapshot.get("sensors", [])
              if not str(sensor.get("id", "")).startswith(("telemetry/cpu/", "telemetry/gpu/"))]
    identities = {sensor.get("id") for sensor in result}
    sources = snapshot.get("sources", {})

    def fresh(state, limit):
        age = number(state.get("age_s"))
        return bool(state.get("ok") and state.get("enabled", True)
                    and age is not None and 0 <= age <= limit)

    def append(identity, name, chip, value, source, updated_at, age, scope):
        watts = number(value)
        if identity in identities or watts is None or watts < 0:
            return
        result.append({"id": compact_text(identity, 96), "name": compact_text(name),
                       "chip": compact_text(chip), "kind": "power", "value": round(watts, 3),
                       "unit": "W", "crit": None, "high": None, "alarm": False, "fault": False,
                       "source": source, "updated_at": updated_at, "age_s": age,
                       "error": None, "scope": scope})
        identities.add(identity)

    turbo = sources.get("turbostat", {})
    if fresh(turbo, max(20, config.turbostat_interval_s * 3)):
        for key, name, scope in (("package_w", "CPU package", "cpu-package"),
                                 ("cores_w", "CPU cores", "cpu-cores"),
                                 ("graphics_w", "CPU graphics domain", "cpu-graphics")):
            append(f"telemetry/cpu/{key}", name, "turbostat / RAPL",
                   snapshot.get("power", {}).get(key), "turbostat",
                   turbo.get("updated_at"), turbo.get("age_s"), scope)
    inventory = sources.get("gpus", {})
    if fresh(inventory, max(90, config.gpu_interval_s * 3)):
        for gpu in snapshot.get("gpus", []):
            if gpu.get("status") != "active" or gpu.get("error"):
                continue
            owner = str(gpu.get("owner", "host"))
            source = f"guest_{owner[3:]}" if owner.startswith("vm:") else "gpus"
            limit = max(45, config.guest_interval_s * 3) if owner.startswith("vm:") else max(90, config.gpu_interval_s * 3)
            state = sources.get(source, {})
            age = number(gpu.get("age_s"))
            if not fresh(state, limit) or age is None or not 0 <= age <= limit:
                continue
            name = str(gpu["name"])
            model = re.search(r"\[([^\]]+)\]", name)
            name = re.sub(r"^(?:NVIDIA\s+)?GeForce\s+", "", model[1] if model else name)
            append(f"telemetry/gpu/{gpu['id']}/power", f"{name} board",
                   f"{gpu.get('vendor', 'GPU')} / {owner}", gpu.get("power_w"), source,
                   gpu.get("updated_at"), age, "gpu-board")
    return result


def parse_smart(data: dict[str, Any], name: str, exit_code: int) -> dict[str, Any]:
    messages = " ".join(str(m.get("string", "")) for m in data.get("smartctl", {}).get("messages", []))
    standby = exit_code == 3 or bool(re.search(r"STANDBY|SLEEP|low.power", messages, re.I))
    passed = data.get("smart_status", {}).get("passed")
    status = "standby" if standby else "error" if exit_code & 7 else "active"
    health = None if standby else "failed" if passed is False or exit_code & 8 else "passed" if passed is True else "unknown"
    temperature = number(data.get("temperature", {}).get("current"))
    if temperature is None:
        temperature = number(data.get("nvme_smart_health_information_log", {}).get("temperature"))
    if temperature is None:
        for attribute in data.get("ata_smart_attributes", {}).get("table", []):
            if attribute.get("id") in (190, 194):
                temperature = number(attribute.get("raw", {}).get("value"))
                if temperature is not None:
                    break
    if temperature is not None and not -50 <= temperature <= 150:
        temperature = None
    warnings = []
    if not standby:
        if exit_code & 16:
            warnings.append("SMART prefail attribute below threshold")
        elif exit_code & (32 | 64 | 128):
            warnings.append("SMART historical errors; inspect smartctl")
        nvme = data.get("nvme_smart_health_information_log", {})
        if number(nvme.get("critical_warning")):
            warnings.insert(0, "NVMe critical warning")
            health = "failed"
        if number(nvme.get("percentage_used")) is not None and number(nvme["percentage_used"]) >= 100:
            warnings.append("NVMe endurance estimate exhausted")
        if number(nvme.get("media_errors")):
            warnings.append("NVMe media errors recorded")
    nvme = data.get("nvme_smart_health_information_log", {})
    attributes = {a.get("id"): number(a.get("raw", {}).get("value"))
                  for a in data.get("ata_smart_attributes", {}).get("table", [])}
    return {"name": compact_text(name), "model": compact_text(data.get("model_name") or data.get("product")),
            "temp_c": None if standby else temperature, "health": health, "status": status,
            "read_bps": None, "write_bps": None, "warnings": warnings[:3],
            "wear_pct": None if standby else number(nvme.get("percentage_used")),
            "media_errors": None if standby else number(nvme.get("media_errors")),
            "power_on_hours": None if standby else number(data.get("power_on_time", {}).get("hours", nvme.get("power_on_hours"))),
            "spare_pct": None if standby else number(nvme.get("available_spare")),
            "reallocated_sectors": None if standby else attributes.get(5),
            "pending_sectors": None if standby else attributes.get(197)}


class ProxmoxReader:
    def __init__(self, config: Config, runner: Callable = command):
        self.config, self.runner, self.rates = config, runner, Rates()
        self.guest_uptime: dict[str, float] = {}

    def read(self) -> dict[str, Any]:
        resources = json.loads(self.runner(["pvesh", "get", "/cluster/resources", "--output-format", "json"], timeout=5).stdout)
        if not isinstance(resources, list):
            raise ValueError("Proxmox resources must be an array")
        node = self.config.node or socket.gethostname().split(".")[0]
        guests, storage = [], []
        now = time.monotonic()
        for item in resources:
            if item.get("node") != node:
                continue
            kind = item.get("type")
            if kind in ("qemu", "lxc"):
                guest_id = int(item["vmid"])
                identity = f"{kind}/{guest_id}"
                uptime = number(item.get("uptime"))
                previous_uptime = self.guest_uptime.get(identity)
                if uptime is not None and previous_uptime is not None and uptime < previous_uptime:
                    for key in list(self.rates.previous):
                        if key.startswith(identity + "/"):
                            self.rates.previous.pop(key)
                if uptime is not None:
                    self.guest_uptime[identity] = uptime
                guest = {"id": guest_id, "name": compact_text(item.get("name") or f"{kind}-{guest_id}"),
                         "type": "vm" if kind == "qemu" else "lxc", "status": compact_text(item.get("status") or "unknown", 16),
                         "cpu_pct": round(number(item["cpu"]) * 100, 2) if number(item.get("cpu")) is not None else None,
                         "mem_used_bytes": number(item.get("mem")), "mem_total_bytes": number(item.get("maxmem")),
                         "mem_host_bytes": number(item.get("memhost")), "mem_assigned_bytes": number(item.get("maxmem")),
                         "uptime_s": number(item.get("uptime"))}
                for src, dst in (("netin", "net_rx_bps"), ("netout", "net_tx_bps"),
                                 ("diskread", "disk_read_bps"), ("diskwrite", "disk_write_bps")):
                    # Stopped guests can carry an old cumulative counter, not live IO.
                    rate = self.rates.rate(f"{kind}/{guest_id}/{src}", item.get(src), now)
                    guest[dst] = rate if guest["status"] == "running" else None
                guests.append(guest)
            elif kind == "storage":
                storage.append({"id": compact_text(item.get("id"), 96),
                                "name": compact_text(item.get("storage") or item.get("id")),
                                "used_bytes": number(item.get("disk")), "total_bytes": number(item.get("maxdisk")),
                                "status": compact_text(item.get("status") or "unknown", 16)})
        # Cluster resources do not consistently expose storage active/enabled.
        # This read obtains authoritative status/capacity for the local node.
        store_data = json.loads(self.runner(["pvesh", "get", f"/nodes/{node}/storage", "--output-format", "json"], timeout=5).stdout)
        storage = [{"id": compact_text(f"storage/{node}/{item.get('storage')}", 96),
                    "name": compact_text(item.get("storage")), "used_bytes": number(item.get("used")),
                    "total_bytes": number(item.get("total")),
                    "status": "disabled" if item.get("enabled") == 0 else "online" if item.get("active") == 1 else
                              "offline" if item.get("active") == 0 else "unknown"}
                   for item in store_data]
        node_error = None
        wait_pct = None
        try:
            status = json.loads(self.runner(["pvesh", "get", f"/nodes/{node}/status", "--output-format", "json"], timeout=5).stdout)
            if number(status.get("wait")) is not None:
                wait_pct = round(number(status["wait"]) * 100, 2)
        except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as exc:
            node_error = compact_text(exc, 160)
        return {"guests": sorted(guests, key=lambda g: g["id"]),
                "storage": sorted(storage, key=lambda s: s["name"]), "io_wait_pct": wait_pct, "node_error": node_error}


class SmartReader:
    def __init__(self, config: Config, runner: Callable = command, sys_root: Path = Path("/sys")):
        self.config, self.runner, self.sys = config, runner, sys_root

    def read(self) -> dict[str, Any]:
        if self.config.smart_devices:
            devices = [{"name": v if v.startswith("/dev/") else f"/dev/{v}"} for v in self.config.smart_devices]
        else:
            scan = self.runner(["smartctl", "--scan", "-j"], timeout=5)
            devices = json.loads(scan.stdout).get("devices", [])
        disks, errors = [], []
        for device in devices[:CAPS["disks"]]:
            name = device["name"]
            args = ["smartctl", "-j", "-a", name]
            # NVMe does not implement ATA/SCSI standby power-mode checks.
            if device.get("type") != "nvme" and not Path(name).name.startswith("nvme"):
                args += ["-n", "standby,3"]
            if device.get("type"):
                args += ["-d", device["type"]]
            try:
                raw = self.runner(args, timeout=5, allow_nonzero=True)
                data = json.loads(raw.stdout)
                disk = parse_smart(data, Path(name).name, raw.returncode)
                names = [Path(name).name]
                if re.fullmatch(r"nvme\d+", names[0]):
                    # smartctl scans controllers; IO counters name namespaces.
                    # Associate the controller's health with its real block
                    # namespaces instead of displaying duplicate unknown disks.
                    namespaces = sorted(path.name for path in (self.sys / "block").glob(names[0] + "n*")
                                        if re.fullmatch(re.escape(names[0]) + r"n\d+", path.name))
                    if namespaces:
                        names = namespaces
                for namespace in names:
                    disks.append(dict(disk, name=namespace))
                if disk["status"] == "error":
                    errors.append(f"{Path(name).name}: SMART read exit {raw.returncode}")
            except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as exc:
                disks.append({"name": compact_text(Path(name).name), "model": "", "temp_c": None,
                              "health": None, "status": "error", "read_bps": None, "write_bps": None})
                errors.append(f"{Path(name).name}: {compact_text(exc, 80)}")
        return {"disks": disks, "error": "; ".join(errors) or None, "disks_total": max(len(devices), len(disks))}


def read_zfs(runner: Callable = command) -> dict[str, Any]:
    raw = runner(["zpool", "list", "-H", "-p", "-o", "name,size,alloc,health"], timeout=5)
    pools = []
    for line in raw.stdout.splitlines():
        fields = line.split()
        if len(fields) >= 4:
            pools.append({"name": compact_text(fields[0]), "total_bytes": number(fields[1]),
                          "used_bytes": number(fields[2]), "status": compact_text(fields[3], 24)})
    return {"pools": pools}


class TrueNASReader:
    """Read a fixed NAS helper via a restricted key; verify its host identity."""
    def __init__(self, config: Config, runner: Callable = command):
        self.config, self.runner, self.rates = config, runner, Rates()
        self.boot_id = None

    def read(self):
        config = self.config
        argv = ["ssh", "-T", "-i", config.truenas_ssh_key,
                "-o", "BatchMode=yes", "-o", "PasswordAuthentication=no",
                "-o", "IdentitiesOnly=yes", "-o", "StrictHostKeyChecking=yes",
                "-o", f"UserKnownHostsFile={config.truenas_known_hosts}",
                "-o", "ConnectTimeout=5", "-o", "LogLevel=ERROR",
                "-l", config.truenas_ssh_user, config.truenas_ssh_host, "snapshot"]
        result = self.runner(argv, timeout=15)
        if len(result.stdout.encode()) > 60 * 1024:
            raise ValueError("NAS helper payload exceeds limit")
        document = json.loads(result.stdout)
        if document.get("schema") != 1:
            raise ValueError("Unsupported NAS helper schema")
        timestamp = number(document.get("generated_at"))
        if timestamp is None or not -60 <= time.time() - timestamp <= 90:
            raise ValueError("NAS helper sample timestamp invalid/stale")
        if self.boot_id and document.get("boot_id") != self.boot_id:
            self.rates = Rates()
        self.boot_id = document.get("boot_id")
        mono = time.monotonic()
        disks = []
        for item in document.get("disks", [])[:16]:
            disk = {"name": compact_text(f"NAS/{item['name']}"), "model": compact_text(item.get("model")),
                    "temp_c": number(item.get("temp_c")), "health": None, "status": "unknown",
                    "zfs_status": compact_text(item.get("zfs_status") or "unknown", 24),
                    "temperature_source": "truenas-cache",
                    "read_bps": self.rates.rate(f"{item['name']}/read", item.get("read_bytes"), mono),
                    "write_bps": self.rates.rate(f"{item['name']}/write", item.get("write_bytes"), mono)}
            for field in ("read_errors", "write_errors", "checksum_errors", "size_bytes"):
                disk[field] = number(item.get(field))
            disks.append(disk)
        storage = [{"id": compact_text(f"nas/pool/{p['name']}", 96), "name": compact_text(f"NAS · {p['name']}"),
                    "used_bytes": number(p.get("used_bytes")), "total_bytes": number(p.get("total_bytes")),
                    "status": compact_text(p.get("status") or "unknown", 24),
                    "healthy": p.get("healthy") if isinstance(p.get("healthy"), bool) else None,
                    "scrub_state": compact_text(p.get("scrub_state"), 24),
                    **{f: number(p.get(f)) for f in ("scrub_errors", "read_errors", "write_errors", "checksum_errors")}}
                   for p in document.get("pools", [])[:16]]
        alerts = [{"id": compact_text(f"nas/{a.get('id', 'alert')}", 96), "severity": a["severity"],
                   "message": compact_text(f"NAS: {a.get('message', '')}", 120)}
                  for a in document.get("alerts", [])[:24] if a.get("severity") in ("warning", "critical")]
        memory_data, memory_error = None, document.get("memory_error")
        if document.get("memory") is not None:
            try:
                from agent.guest_telemetry import memory
                raw_memory = document["memory"]
                memory_data = memory(raw_memory.get("mem_total_bytes"), raw_memory.get("mem_available_bytes"), raw_memory.get("mem_cache_bytes"))
                memory_data["memory_basis"] = "guest-os-arc"
            except (ValueError, TypeError, AttributeError):
                memory_error = "NAS OS memory sample invalid"
        return {"disks": disks, "storage": storage, "alerts": alerts,
                "memory": memory_data, "memory_error": compact_text(memory_error, 120) if memory_error else None, "generated_at": timestamp,
                "temperature_cache_interval_s": number(document.get("temperature_cache_interval_s")),
                "error": compact_text(document["error"], 180) if document.get("error") else None,
                "counts": document.get("counts", {})}


def read_kernel_faults():
    from agent.faults import read_faults
    return read_faults(command)


@dataclasses.dataclass
class Source:
    probe: Callable
    interval: float
    ttl: float
    enabled: bool = True
    future: concurrent.futures.Future | None = None
    next_run: float = 0
    data: Any = None
    updated_at: float | None = None
    updated_mono: float | None = None
    ok: bool = False
    error: str | None = "initializing"
    last_probe_succeeded: bool = False
    discard_pending: bool = False

    def reset(self, error="Guest changed; waiting for OS sample"):
        self.data = self.updated_at = self.updated_mono = None
        self.ok = self.last_probe_succeeded = False
        self.error, self.next_run = error, 0
        if self.future is not None:
            if self.future.cancel():
                self.future = None
            else:
                self.discard_pending = True

    def advance(self, executor: concurrent.futures.Executor, mono: float, wall: float):
        if not self.enabled:
            self.error = "disabled in configuration"
            return
        if self.future is not None and self.future.done():
            try:
                result = self.future.result()
                if not self.discard_pending:
                    self.data, self.updated_at, self.updated_mono = result, wall, mono
                    self.error = result.get("error") if isinstance(result, dict) else None
                    self.ok = not bool(self.error)
                    self.last_probe_succeeded = True
            except Exception as exc:
                self.ok, self.error = False, compact_text(f"{type(exc).__name__}: {exc}", 180)
                self.last_probe_succeeded = False
                LOG.warning("Source probe failed: %s", self.error)
            self.future = None
            self.discard_pending = False
        if self.future is None and mono >= self.next_run:
            self.future = executor.submit(self.probe)
            self.next_run = mono + self.interval


    def current(self, mono: float) -> Any:
        if self.updated_mono is None or mono - self.updated_mono > self.ttl:
            return None
        return self.data

    def status(self, mono: float) -> dict[str, Any]:
        expired = self.updated_mono is not None and mono - self.updated_mono > self.ttl
        return {"ok": self.ok and not expired, "enabled": self.enabled, "updated_at": self.updated_at,
                "age_s": round(mono - self.updated_mono, 1) if self.updated_mono is not None else None,
                "error": "sample expired" if expired else self.error}


def generate_alerts(snapshot: dict[str, Any], config: Config) -> list[dict[str, str]]:
    result = list(snapshot.get("alerts", []))
    def alert(identity, severity, message):
        result.append({"id": compact_text(identity, 96), "severity": severity, "message": compact_text(message, 120)})
    def threshold(identity, title, value, warning, critical, unit):
        if value is not None and (value >= warning or value >= critical):
            alert(identity, "critical" if value >= critical else "warning", f"{title} {value:.0f}{unit}")
    host = snapshot["host"]
    if host["mem_total_bytes"] and host["mem_used_bytes"] is not None:
        threshold("host/memory", "RAM used", host["mem_used_bytes"] * 100 / host["mem_total_bytes"],
                  config.memory_warning_pct, config.memory_critical_pct, "%")
    threshold("host/iowait", "IO wait", host["io_wait_pct"], config.io_wait_warning_pct, config.io_wait_critical_pct, "%")
    threshold("host/temperature", "CPU temperature", snapshot["power"]["cpu_temp_c"],
              config.temperature_warning_c, config.temperature_critical_c, "°C")
    for sensor in snapshot["sensors"]:
        if sensor.get("fault"):
            alert(f"{sensor['id']}/fault", "warning", f"{sensor['name']} sensor fault")
        if sensor.get("alarm"):
            alert(f"{sensor['id']}/alarm", "critical", f"{sensor['name']} hardware sensor alarm")
        if sensor["kind"] == "temperature":
            warning = sensor["high"] if sensor["high"] is not None else config.temperature_warning_c
            critical = sensor["crit"] if sensor["crit"] is not None else config.temperature_critical_c
            threshold(sensor["id"], sensor["name"], sensor["value"], warning, critical, "°C")
    for disk in snapshot["disks"]:
        if disk["health"] == "failed":
            alert(f"disk/{disk['name']}/smart", "critical", f"{disk['name']} SMART health failed")
        elif disk["status"] == "error":
            alert(f"disk/{disk['name']}/read", "warning", f"{disk['name']} SMART unavailable")
        threshold(f"disk/{disk['name']}/temperature", disk["name"], disk["temp_c"],
                  config.disk_temperature_warning_c, config.disk_temperature_critical_c, "°C")
        for index, warning in enumerate(disk.get("warnings", [])):
            alert(f"disk/{disk['name']}/warning{index}", "warning", f"{disk['name']}: {warning}")
        for field, title in (("reallocated_sectors", "reallocated sectors"), ("pending_sectors", "pending sectors")):
            if disk.get(field):
                alert(f"disk/{disk['name']}/{field}", "critical" if field == "pending_sectors" else "warning",
                      f"{disk['name']}: {disk[field]:.0f} {title}")
        if disk.get("zfs_status") in ("DEGRADED", "FAULTED", "UNAVAIL", "OFFLINE", "REMOVED"):
            alert(f"disk/{disk['name']}/zfs", "critical", f"{disk['name']} ZFS {disk['zfs_status']}")
        for field in ("read_errors", "write_errors", "checksum_errors"):
            if disk.get(field):
                alert(f"disk/{disk['name']}/{field}", "critical", f"{disk['name']}: {disk[field]:.0f} ZFS {field.replace('_', ' ')}")
    for storage in snapshot["storage"]:
        if storage["total_bytes"] and storage["used_bytes"] is not None:
            threshold(storage["id"], f"{storage['name']} used", storage["used_bytes"] * 100 / storage["total_bytes"],
                      config.storage_warning_pct, config.storage_critical_pct, "%")
        if storage["status"] in ("offline", "DEGRADED", "FAULTED", "UNAVAIL", "SUSPENDED"):
            alert(f"{storage['id']}/health", "critical", f"{storage['name']} {storage['status']}")
        elif storage.get("healthy") is False:
            alert(f"{storage['id']}/health", "critical", f"{storage['name']} reports unhealthy")
        if storage.get("scrub_errors"):
            alert(f"{storage['id']}/scrub", "critical", f"{storage['name']}: scrub errors detected")
    for name in ("proc", "proxmox", "sensors", "smart", "turbostat", "zfs", "nas", "faults", "gpus"):
        source = snapshot["sources"].get(name, {})
        if source and source.get("enabled", True) and not source.get("ok"):
            alert(f"source/{name}", "warning", f"{name} telemetry unavailable")
    if snapshot["sources"].get("proxmox", {}).get("ok"):
        guests = {g["id"]: g for g in snapshot["guests"]}
        for vmid in config.expected_running_guests:
            guest = guests.get(vmid)
            if guest is None:
                alert(f"guest/{vmid}/missing", "warning", f"Expected guest {vmid} absent from node")
            elif guest["status"] != "running":
                alert(f"guest/{vmid}/stopped", "critical", f"{guest['name']} is {guest['status']} (expected running)")
    for guest in snapshot["guests"]:
        if guest.get("memory_basis", "proxmox") != "proxmox" and guest["status"] == "running" and guest.get("mem_error"):
            alert(f"guest/{guest['id']}/memory", "warning", f"{guest['name']} OS memory unavailable")
    faults = snapshot.get("faults", {})
    count = faults.get("segfault_count_24h")
    if count:
        alert("faults/segfaults", "critical" if count >= 3 else "warning", f"{count} segfaults in the last 24h")
    cutoff = time.time() - 86400
    for event in faults.get("events", []):
        timestamp = number(event.get("timestamp"))
        if timestamp is not None and timestamp >= cutoff and event.get("kind") != "segfault":
            alert(f"faults/{event.get('id', 'event')}", "critical" if event.get("kind") in ("hardware", "oom", "lockup") else "warning",
                  compact_text(event.get("message"), 120))
    return sorted(result, key=lambda a: a["severity"] != "critical")[:24]


def bounded_snapshot(snapshot: dict[str, Any]) -> dict[str, Any]:
    """Fixed firmware bounds and a hard payload ceiling; omissions are visible."""
    limits = {"max_payload_bytes": MAX_PAYLOAD, "truncated": {}, "counts": {}, "network_interfaces": [], "disk_devices": []}
    limits.update(snapshot.get("limits", {}))
    snapshot["limits"] = limits
    for key, cap in CAPS.items():
        snapshot.setdefault(key, [])
        count = max(limits["counts"].get(key, 0), len(snapshot[key]))
        limits["counts"][key] = count
        snapshot[key] = snapshot[key][:cap]
        limits["truncated"][key] = max(0, count - len(snapshot[key]))
        limits[f"max_{key}"] = cap
    while len(encode_snapshot(snapshot)) > MAX_PAYLOAD:
        candidates = [key for key in CAPS if snapshot[key]]
        if not candidates:
            raise ValueError("Base snapshot exceeds payload limit")
        largest = max(candidates, key=lambda k: len(json.dumps(snapshot[k])))
        snapshot[largest].pop()
        limits["truncated"][largest] += 1
    if any(limits["truncated"].values()):
        alert = {"id": "display/truncated", "severity": "warning", "message": "Display capacity reached; some items omitted"}
        if not any(a["id"] == alert["id"] for a in snapshot["alerts"]):
            snapshot["alerts"] = [alert] + snapshot["alerts"][:23]
        # Reserve room for this notification without ever crossing the wire cap.
        while len(encode_snapshot(snapshot)) > MAX_PAYLOAD:
            largest = max((k for k in CAPS if snapshot[k]), key=lambda k: len(json.dumps(snapshot[k])))
            snapshot[largest].pop()
            limits["truncated"][largest] += 1
    return snapshot


def encode_snapshot(snapshot: dict[str, Any]) -> bytes:
    return json.dumps(snapshot, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")


def atomic_write(path: Path, snapshot: dict[str, Any]):
    """Same-directory replace: HTTP readers see only complete old/new JSON."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        fd, temporary = tempfile.mkstemp(prefix=".snapshot-", dir=path.parent)
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), 0o640)
            stream.write(encode_snapshot(snapshot))
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def merge_guest_memory(guests, data, states, enabled_ids, nas_id, pve_status, now):
    """Keep guest OS memory separate from PVE estimates and QEMU footprint.

    An enabled but failed/stale OS probe yields unknown primary memory, never a
    fallback to host footprint. Copy records so cached PVE samples stay intact.
    """
    result = []
    for original in guests:
        guest = original.copy()
        vmid = guest["id"]
        guest.update(pve_mem_used_bytes=original["mem_used_bytes"], pve_mem_total_bytes=original["mem_total_bytes"],
                     mem_assigned_bytes=original.get("mem_assigned_bytes", original["mem_total_bytes"]),
                     mem_host_bytes=original.get("mem_host_bytes"), memory_basis="proxmox",
                     mem_available_bytes=None, mem_cache_bytes=None, mem_noncache_used_bytes=None,
                     mem_updated_at=pve_status.get("updated_at"), mem_age_s=pve_status.get("age_s"),
                     mem_error=compact_text(pve_status["error"], 120) if pve_status.get("error") else None)
        if vmid in enabled_ids:
            guest.update(memory_basis="guest-os-arc" if vmid == nas_id else "guest-os",
                         mem_used_bytes=None, mem_total_bytes=None)
            state = states.get(vmid, {})
            sample = data.get(vmid)
            error = state.get("error") or "Guest OS memory unavailable"
            if guest["status"] != "running":
                error, sample = f"Guest is {guest['status']}", None
            elif not pve_status.get("ok"):
                error, sample = "Proxmox guest state unavailable", None
            elif not state.get("ok"):
                sample = None
            timestamp = sample.get("generated_at") if sample else state.get("updated_at")
            guest["mem_updated_at"] = timestamp
            guest["mem_age_s"] = round(max(0, now - timestamp), 1) if timestamp is not None else None
            if sample:
                guest.update(sample["memory"])
                guest["mem_error"] = None
            else:
                guest["mem_error"] = compact_text(error, 120)
        result.append(guest)
    return result


class Collector:
    def __init__(self, config: Config, proc_reader: ProcReader | None = None):
        self.config, self.proc = config, proc_reader or ProcReader(config)
        self.sequence = 0
        self.executor = concurrent.futures.ThreadPoolExecutor(max_workers=10, thread_name_prefix="telemetry")
        from agent.guest_telemetry import GuestReader
        from agent.gpus import GPUReader
        from agent.printers import MoonrakerReader, validate_printers
        from agent.remote_feeds import RemoteCollectorReader, validate_remote_collectors
        self.printer_configs = validate_printers(config.printers)
        self.printer_sources = {item["id"]: Source(MoonrakerReader(item).read, item["poll_interval_s"], item["ttl_s"])
                                for item in self.printer_configs}
        self.remote_configs = validate_remote_collectors(config.remote_collectors)
        self.remote_sources = {item["id"]: Source(RemoteCollectorReader(item).read, item["poll_interval_s"], item["ttl_s"])
                               for item in self.remote_configs}
        self.guest_readers = {vmid: GuestReader(vmid, command) for vmid in config.qga_guest_ids}
        self.guest_epochs = {}
        self.sources = {
            "proxmox": Source(ProxmoxReader(config).read, config.proxmox_interval_s, max(30, config.proxmox_interval_s * 3)),
            "sensors": Source(lambda: {"sensors": parse_sensors(json.loads(command(["sensors", "-j"]).stdout))},
                              config.sensor_interval_s, max(20, config.sensor_interval_s * 3)),
            "turbostat": Source(lambda: {"power": parse_turbostat(command(
                ["turbostat", "--quiet", "--Summary", "--interval", "1", "--num_iterations", "1"], timeout=5).stdout)},
                config.turbostat_interval_s, max(20, config.turbostat_interval_s * 3), config.enable_turbostat),
            "smart": Source(SmartReader(config).read, config.smart_interval_s, max(900, config.smart_interval_s * 3), config.enable_smart),
            "zfs": Source(read_zfs, config.zfs_interval_s, max(90, config.zfs_interval_s * 3), config.enable_zfs),
            "nas": Source(TrueNASReader(config).read, config.truenas_interval_s, max(90, config.truenas_interval_s * 3), bool(config.truenas_ssh_host)),
            "faults": Source(read_kernel_faults, config.faults_interval_s, max(90, config.faults_interval_s * 3), config.enable_faults),
            "gpus": Source(GPUReader(command).read, config.gpu_interval_s, max(90, config.gpu_interval_s * 3), config.enable_gpus),
        }
        for vmid, reader in self.guest_readers.items():
            self.sources[f"guest_{vmid}"] = Source(reader.read, config.guest_interval_s, max(45, config.guest_interval_s * 3))

    def close(self):
        self.executor.shutdown(wait=True, cancel_futures=True)

    def sample(self) -> dict[str, Any]:
        mono, wall = time.monotonic(), time.time()
        self.sequence += 1
        snapshot = {"schema": 1, "sequence": self.sequence, "generated_at": round(wall, 3),
                    "host": {}, "power": {"package_w": None, "cores_w": None, "graphics_w": None,
                                           "cpu_mhz": None, "busy_mhz": None,
                                           "cpu_temp_c": None, "cstate_pct": {}},
                    "guests": [], "storage": [], "disks": [], "sensors": [], "gpus": [], "alerts": [], "sources": {},
                    "faults": {"lookback_days": 7, "segfault_count_24h": None, "event_count_24h": None,
                               "last_event_at": None, "events": []},
                    "limits": {"counts": {}}}
        proc = self.proc.read(mono)
        snapshot["host"] = proc["host"]
        snapshot["sources"]["proc"] = {"ok": not bool(proc["error"]), "enabled": True, "updated_at": wall, "age_s": 0, "error": proc["error"]}
        for name, source in self.sources.items():
            source.advance(self.executor, mono, wall)
            snapshot["sources"][name] = source.status(mono)
            data = source.current(mono)
            if data is None:
                continue
            if name == "proxmox":
                snapshot["guests"], snapshot["storage"] = [g.copy() for g in data["guests"]], data["storage"]
                for guest in snapshot["guests"]:
                    vmid = guest["id"]
                    epoch = (guest["status"], guest["uptime_s"], guest["mem_total_bytes"])
                    previous = self.guest_epochs.get(vmid)
                    rebooted = bool(previous and epoch[1] is not None and previous[1] is not None and epoch[1] < previous[1])
                    if previous and (epoch[0] != previous[0] or epoch[2] != previous[2] or
                                     rebooted):
                        if vmid in self.guest_readers:
                            self.guest_readers[vmid].reset(restarted=epoch[0] != previous[0] or rebooted)
                            self.sources[f"guest_{vmid}"].reset()
                        if vmid == self.config.truenas_guest_id:
                            # Preserve NAS disk/pool data; guest RAM is cleared
                            # below until a NAS sample newer than this epoch.
                            self.guest_epochs[f"nas_reset_{vmid}"] = wall
                    self.guest_epochs[vmid] = epoch
                snapshot["sources"]["proxmox_node"] = {"ok": not bool(data.get("node_error")),
                    "updated_at": source.updated_at, "age_s": snapshot["sources"][name]["age_s"], "error": data.get("node_error")}
                if data.get("io_wait_pct") is not None:
                    snapshot["host"]["io_wait_pct"] = data["io_wait_pct"]
            elif name == "sensors":
                snapshot["sensors"] = data["sensors"]
            elif name == "turbostat":
                snapshot["power"] = data["power"].copy()
            elif name == "smart":
                snapshot["disks"] = [disk.copy() for disk in data["disks"]]
                snapshot["limits"]["counts"]["disks"] = data.get("disks_total", len(data["disks"]))
            elif name == "zfs":
                snapshot["storage"] = snapshot["storage"] + [dict(id=f"zpool/{p['name']}", **p) for p in data["pools"]]
            elif name == "nas":
                snapshot["limits"]["counts"]["storage"] = len(snapshot["storage"]) + max(len(data["storage"]), data["counts"].get("pools", 0))
                snapshot["limits"]["counts"]["disks"] = max(len(snapshot["disks"]), snapshot["limits"]["counts"].get("disks", 0)) + max(len(data["disks"]), data["counts"].get("disks", 0))
                snapshot["storage"] = snapshot["storage"] + data["storage"]
                snapshot["disks"] = snapshot["disks"] + [disk.copy() for disk in data["disks"]]
                snapshot["alerts"] += data["alerts"]
                snapshot["sources"]["nas"]["temperature_cache_interval_s"] = data["temperature_cache_interval_s"]
            elif name == "faults":
                snapshot["faults"] = data["faults"]
        guest_data = {}
        memory_states = {}
        for vmid in self.guest_readers:
            source = self.sources[f"guest_{vmid}"]
            state = source.status(mono)
            current = source.current(mono)
            if current:
                state["updated_at"] = current["generated_at"]
                state["age_s"] = round(max(0, wall - current["generated_at"]), 1)
                if state["age_s"] > source.ttl:
                    state.update(ok=False, error="Guest OS sample expired")
            memory_states[vmid] = state
            if state["ok"] and current is not None:
                guest_data[vmid] = current
        enabled_memory = set(self.config.qga_guest_ids)
        if self.config.truenas_ssh_host:
            vmid = self.config.truenas_guest_id
            enabled_memory.add(vmid)
            source = self.sources["nas"]
            data = source.current(mono)
            error = (data.get("memory_error") or ("NAS OS memory unavailable" if not data.get("memory") else None)) if data else source.status(mono)["error"]
            if data and data.get("generated_at", 0) < self.guest_epochs.get(f"nas_reset_{vmid}", 0):
                error = "Guest changed; waiting for NAS OS sample"
            if data and wall - data["generated_at"] > source.ttl:
                error = "NAS OS memory sample expired"
            memory_states[vmid] = dict(source.status(mono), ok=bool(data and source.last_probe_succeeded and not error), error=error)
            if memory_states[vmid]["ok"]:
                guest_data[vmid] = dict(memory=data["memory"], generated_at=data["generated_at"], gpus=[])
        snapshot["guests"] = merge_guest_memory(snapshot["guests"], guest_data, memory_states, enabled_memory,
                                               self.config.truenas_guest_id, snapshot["sources"]["proxmox"], wall)
        from agent.gpus import merge_gpus
        inventory_source = self.sources["gpus"]
        inventory = inventory_source.current(mono)
        if inventory:
            snapshot["gpus"] = merge_gpus(inventory["gpus"], guest_data, snapshot["guests"], snapshot["sources"], wall)
            if not inventory_source.status(mono)["ok"]:
                for gpu in snapshot["gpus"]:
                    gpu.update({key: None for key in ("utilization_pct", "mem_used_bytes", "mem_total_bytes", "temp_c", "power_w", "graphics_mhz", "memory_mhz", "fan_pct")})
                    gpu.update(status="unavailable", error=compact_text(inventory_source.error, 120))
        if snapshot["power"]["cpu_temp_c"] is None:
            cpu_temps = [s["value"] for s in snapshot["sensors"] if s["kind"] == "temperature"
                         and re.search(r"coretemp|k10temp|zenpower|cpu_thermal", s["chip"], re.I)]
            if cpu_temps:
                snapshot["power"]["cpu_temp_c"] = max(cpu_temps)
        snapshot["sensors"] = merge_power_sensors(snapshot, self.config)
        for disk in snapshot["disks"]:
            if disk["name"] in self.proc.disk_rates:
                disk["read_bps"], disk["write_bps"] = self.proc.disk_rates[disk["name"]]
        # Even without smartctl, physical disk throughput remains available.
        known = {d["name"] for d in snapshot["disks"]}
        for name in self.proc.devices:
            if name not in known:
                read, write = self.proc.disk_rates.get(name, (None, None))
                snapshot["disks"].append({"name": name, "model": "", "temp_c": None, "health": None,
                                          "status": "unknown", "read_bps": read, "write_bps": write})
        snapshot["limits"].update(network_interfaces=self.proc.interfaces, disk_devices=self.proc.devices)
        snapshot["alerts"] = generate_alerts(snapshot, self.config)
        return bounded_snapshot(snapshot)

    def sample_aggregate(self) -> dict[str, Any]:
        """Publish independent node snapshots without changing legacy sample()."""
        from agent.printers import printer_snapshot
        from agent.remote_feeds import remote_snapshot
        snapshot = self.sample() if self.config.enable_proxmox else None
        if snapshot is None:
            # Disabled means no local proc, PVE, guest or hardware probes run.
            self.sequence += 1
        now, mono = time.time(), time.monotonic()
        nodes = []
        if snapshot is not None:
            native_name = self.config.node or snapshot["host"]["name"]
            nodes.append({"id": proxmox_node_id(native_name), "type": "proxmox",
                          "name": self.config.display_name or native_name, "address": self.config.host_ip,
                          "snapshot": snapshot})
        for item in self.printer_configs:
            source = self.printer_sources[item["id"]]
            source.advance(self.executor, mono, now)
            state = source.status(mono)
            data = source.current(mono)
            own = bounded_snapshot(printer_snapshot(item, state, data, self.sequence, now))
            nodes.append({"id": f"klipper:{item['id']}", "type": "klipper", "name": item["name"],
                          "address": urlsplit(item["url"]).netloc,
                          "snapshot": own})
        for item in self.remote_configs:
            source = self.remote_sources[item["id"]]
            source.advance(self.executor, mono, now)
            own = bounded_snapshot(remote_snapshot(item, source.status(mono), source.current(mono), self.sequence, now))
            nodes.append({"id": f"remote:{item['id']}", "type": "proxmox", "name": item["name"],
                          "address": urlsplit(item["url"]).netloc, "snapshot": own})
        for node in nodes:
            own = node["snapshot"]
            enabled = [s for s in own["sources"].values() if s.get("enabled", True)]
            if enabled and all(s.get("updated_at") is None and s.get("error") == "initializing" for s in enabled):
                node["status"] = "unknown"
            elif enabled and all(s.get("ok") is False for s in enabled):
                node["status"] = "offline"
            elif any(not s.get("ok") for s in enabled) or own["alerts"]:
                node["status"] = "degraded"
            else:
                node["status"] = "healthy" if enabled else "unknown"
        aggregate = {"schema": 2, "sequence": self.sequence, "generated_at": round(now, 3), "nodes": nodes}
        if len(nodes) > MAX_NODES or len(encode_snapshot(aggregate)) > MAX_AGGREGATE:
            raise ValueError("Node aggregate exceeds its storage bound")
        return aggregate


def prepare_snapshot_directory(output: Path, snapshot_group: str | None = None):
    """Apply reader group after systemd's per-command runtime-dir setup.

    Proxmox IPC requires primary gid0. systemd assigns RuntimeDirectory to that
    primary group on each command launch, so ExecStartPre chown is insufficient.
    The collector itself sets group/setgid once before writing atomic files.
    Standalone collectors leave ownership alone unless explicitly requested.
    """
    output.parent.mkdir(parents=True, exist_ok=True)
    if snapshot_group is not None:
        try:
            gid = grp.getgrnam(snapshot_group).gr_gid
        except KeyError as exc:
            raise ValueError(f"Snapshot reader group not found: {snapshot_group}") from exc
        os.chown(output.parent, -1, gid)
        os.chmod(output.parent, 0o2750)


def run(config: Config, output: Path, once: bool = False, snapshot_group: str | None = None):
    prepare_snapshot_directory(output, snapshot_group)
    collector = Collector(config)
    stop = threading.Event()
    if threading.current_thread() is threading.main_thread():
        for sig in (signal.SIGTERM, signal.SIGINT):
            signal.signal(sig, lambda *_: stop.set())
    try:
        # Prevent a second collector from launching duplicate turbostat probes.
        output.parent.mkdir(parents=True, exist_ok=True)
        with (output.parent / ".collector.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            while not stop.is_set():
                began = time.monotonic()
                snapshot = collector.sample_aggregate()
                atomic_write(output, snapshot)
                if once:
                    # --once waits for initial source probes, then gathers again
                    # after one second to establish real CPU/IO delta rates.
                    local_sources = list(collector.sources.values()) if config.enable_proxmox else []
                    for source in local_sources + list(collector.printer_sources.values()) + list(collector.remote_sources.values()):
                        if source.future:
                            try:
                                source.future.result(timeout=90)
                            except Exception:
                                pass
                    stop.wait(1)
                    atomic_write(output, collector.sample_aggregate())
                    break
                stop.wait(max(0, config.interval_s - (time.monotonic() - began)))
    finally:
        collector.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", help="JSON configuration; defaults are safe for local Proxmox")
    parser.add_argument("--output", default="/run/homelab-monitor/snapshot.json")
    parser.add_argument("--snapshot-group", help="Reader group for shared setgid snapshot directory (systemd installation)")
    parser.add_argument("--once", action="store_true", help="Collect one initialized snapshot and exit")
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    try:
        run(Config.read(args.config), Path(args.output), args.once, args.snapshot_group)
    except (OSError, ValueError) as exc:
        parser.exit(1, f"Collector failed: {exc}\n")


if __name__ == "__main__":
    main()
