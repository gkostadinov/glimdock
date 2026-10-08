"""PCI GPU inventory, passthrough ownership, and honest optional DRM metrics."""
from __future__ import annotations

from pathlib import Path
import re
import shlex
import time

from agent.guest_telemetry import GPU_METRICS, finite, parse_nvidia_csv, validate_gpu_metrics

VENDORS = {"8086": "Intel", "10de": "NVIDIA", "1002": "AMD"}
VIRTUAL_VENDORS = {"1234", "1af4", "15ad", "1414", "1b36"}


def pci_id(value):
    match = re.search(r"(?:([0-9a-f]{4,8}):)?([0-9a-f]{2}):([0-9a-f]{2})(?:\.([0-7]))?", str(value).lower())
    if not match:
        return None
    domain, bus, slot, function = match.groups()
    return f"{(domain or '0000')[-4:]}:{bus}:{slot}.{function or '0'}"


def gpu_kind(vendor, name, device, slot):
    if re.search(r"Arc|DG[12]", name, re.I):
        return "discrete"
    if vendor == "8086" and (device == "a780" or re.search(r"UHD|HD Graphics|Iris|GT[123]|Raptor|Alder", name, re.I)):
        return "integrated"
    if vendor == "10de" or re.search(r"GeForce|Quadro|RTX|Radeon RX|Navi|Arc|DG[12]", name, re.I):
        return "discrete"
    if re.search(r"Raphael|Renoir|Cezanne|APU", name, re.I):
        return "integrated"
    return "unknown"


def file_number(path, scale=1):
    try:
        value = finite(path.read_text().strip())
        return value * scale if value is not None else None
    except OSError:
        return None


def ownership(config_root):
    result = {}
    for path in config_root.glob("*.conf"):
        try:
            vmid = int(path.stem)
            lines = path.read_text().splitlines()
        except (OSError, ValueError):
            continue
        for line in lines:
            if not re.match(r"hostpci\d+:\s*", line):
                continue
            for token in line.split(":", 1)[1].split(",", 1)[0].split(";"):
                slot = pci_id(token)
                if slot:
                    result[slot] = f"vm:{vmid}"
    return result


class GPUReader:
    def __init__(self, runner, sys_root=Path("/sys"), config_root=Path("/etc/pve/qemu-server")):
        self.runner, self.sys, self.config_root = runner, sys_root, config_root

    def read(self):
        names = {}
        try:
            for line in self.runner(["lspci", "-D", "-mm"], timeout=5).stdout.splitlines():
                fields = shlex.split(line)
                if len(fields) >= 4:
                    names[pci_id(fields[0])] = fields[3]
        except (OSError, RuntimeError, ValueError):
            pass
        owners = ownership(self.config_root)
        result = []
        timestamp = time.time()
        for path in sorted((self.sys / "bus/pci/devices").glob("*")):
            try:
                klass = path.joinpath("class").read_text().strip()
                vendor = path.joinpath("vendor").read_text().strip().removeprefix("0x")
                device = path.joinpath("device").read_text().strip().removeprefix("0x")
            except OSError:
                continue
            if not klass.startswith("0x03") or vendor in VIRTUAL_VENDORS:
                continue
            slot = pci_id(path.name)
            name = names.get(slot, f"{VENDORS.get(vendor, vendor)} GPU [{vendor}:{device}]")
            driver = path.joinpath("driver").resolve().name if path.joinpath("driver").exists() else ""
            gpu = {"id": f"pci:{slot}", "name": name[:96], "vendor": VENDORS.get(vendor, vendor),
                   "kind": gpu_kind(vendor, name, device, slot), "owner": owners.get(slot, "host"),
                   "driver": driver[:48], "status": "inventory", "updated_at": timestamp, "age_s": 0,
                   "error": None, "_vendor_id": vendor, "_device_id": device,
                   "utilization_kind": "gpu", **{key: None for key in GPU_METRICS}}
            if gpu["owner"] == "host" and driver not in ("vfio-pci", "vfio_pci"):
                gpu["utilization_pct"] = file_number(path / "gpu_busy_percent")
                gpu["mem_used_bytes"] = file_number(path / "mem_info_vram_used")
                gpu["mem_total_bytes"] = file_number(path / "mem_info_vram_total")
                for drm in path.joinpath("drm").glob("card[0-9]*"):
                    for clock in ("gt/gt0/rps_act_freq_mhz", "gt_cur_freq_mhz"):
                        value = file_number(drm / clock)
                        if value is not None:
                            gpu["graphics_mhz"] = value
                            break
                for hwmon in path.joinpath("hwmon").glob("hwmon*"):
                    gpu["temp_c"] = file_number(hwmon / "temp1_input", .001)
                    gpu["power_w"] = file_number(hwmon / "power1_average", .000001)
                    pwm, maximum = file_number(hwmon / "pwm1"), file_number(hwmon / "pwm1_max")
                    if pwm is not None and maximum and maximum > 0:
                        gpu["fan_pct"] = pwm / maximum * 100
                if any(gpu[key] is not None for key in GPU_METRICS):
                    gpu["status"] = "partial"
            validate_gpu_metrics(gpu)
            result.append(gpu)
        if any(g["owner"] == "host" and g["vendor"] == "NVIDIA" for g in result):
            try:
                from agent.guest_telemetry import NVIDIA_FIELDS
                rows = parse_nvidia_csv(self.runner(["nvidia-smi", f"--query-gpu={NVIDIA_FIELDS}", "--format=csv,noheader,nounits"], timeout=5).stdout)
                for row in rows:
                    for gpu in result:
                        if gpu["owner"] == "host" and gpu["id"] == f"pci:{pci_id(row['pci_bus'])}":
                            gpu.update({key: row.get(key) for key in GPU_METRICS})
                            gpu.update(status="active", error=None)
            except (OSError, RuntimeError, ValueError):
                for gpu in result:
                    if gpu["owner"] == "host" and gpu["vendor"] == "NVIDIA":
                        gpu["error"] = "NVIDIA metrics unavailable"
        return {"gpus": result, "error": None}


def merge_gpus(inventory, guest_data, guests, source_status, now):
    """Match device identity + owning VM; guest PCI buses can be renumbered."""
    result = []
    guest_map = {g["id"]: g for g in guests}
    for original in inventory:
        gpu = original.copy()
        gpu["age_s"] = round(max(0, now - gpu["updated_at"]), 1) if gpu.get("updated_at") is not None else None
        owner = gpu["owner"]
        if owner.startswith("vm:"):
            vmid = int(owner[3:])
            state = source_status.get(f"guest_{vmid}", {})
            data = guest_data.get(vmid)
            if guest_map.get(vmid, {}).get("status") != "running":
                data = None
                gpu["error"] = "Owning guest is stopped/unavailable"
            elif not state.get("ok"):
                data = None
                gpu["error"] = str(state.get("error") or "Guest GPU telemetry unavailable")[:120]
            if data is not None:
                matches = [card for card in data["gpus"] if card.get("vendor_id") == gpu.get("_vendor_id")
                           and card.get("device_id") == gpu.get("_device_id")]
                if len(matches) == 1:
                    card = matches[0]
                    gpu.update({key: card.get(key) for key in GPU_METRICS})
                    gpu.update(driver=str(card.get("driver") or gpu["driver"])[:48], updated_at=data["generated_at"],
                               age_s=round(max(0, now - data["generated_at"]), 1),
                               error=str(card["error"])[:120] if card.get("error") else None,
                               utilization_kind=card.get("utilization_kind", "gpu"))
                    gpu["status"] = "partial" if gpu["error"] else "active" if any(gpu[key] is not None for key in GPU_METRICS) else "inventory"
                else:
                    gpu["error"] = "Guest GPU identity missing/ambiguous"
                    data = None
            if data is None:
                gpu.update({key: None for key in GPU_METRICS})
                gpu["status"] = "unavailable"
                gpu["updated_at"], gpu["age_s"] = state.get("updated_at"), state.get("age_s")
        for key in list(gpu):
            if key.startswith("_"):
                gpu.pop(key)
        result.append(gpu)
    return result
