"""Bounded, read-only Moonraker monitoring for independent printer nodes.

Only fixed GET endpoints are queried. No G-Code, print controls, configuration,
images or camera URLs are accepted. Heater power is PWM duty, never watts.
"""
from __future__ import annotations

import copy
import json
import math
from pathlib import Path, PurePosixPath
import re
import time
from urllib.parse import urlencode, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

MAX_PRINTERS = 4
MAX_RESPONSE = 64 * 1024
BASE_OBJECTS = {
    "webhooks": "state,state_message",
    "print_stats": "state,message,filename,print_duration,total_duration,filament_used,info",
    "display_status": "progress,message",
    "virtual_sdcard": "progress,is_active",
    "pause_resume": "is_paused",
    "fan": "speed",
}
CORE_OBJECTS = {name: BASE_OBJECTS[name] for name in ("webhooks", "print_stats")}


def finite(value, minimum=None, maximum=None):
    if value is None or isinstance(value, bool):
        return None
    try:
        value = float(value)
    except (ValueError, TypeError):
        return None
    if not math.isfinite(value) or (minimum is not None and value < minimum) or (maximum is not None and value > maximum):
        return None
    return value


def text(value, size=96):
    return str(value or "").replace("\n", " ").replace("\r", " ")[:size]


def validate_printers(values):
    """Normalize reusable config while preserving an absent printer list."""
    if not isinstance(values, list) or len(values) > MAX_PRINTERS:
        raise ValueError("printers must be a list of at most four printer configurations")
    result, identities = [], set()
    allowed = {"id", "name", "url", "poll_interval_s", "timeout_s", "ttl_s", "api_key_file"}
    for item in values:
        if not isinstance(item, dict) or set(item) - allowed:
            raise ValueError("Each printer must contain only supported configuration fields")
        identity = item.get("id")
        if not isinstance(identity, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,31}", identity) or identity in identities:
            raise ValueError("Printer IDs must be unique stable names of at most 32 characters")
        name = item.get("name", identity)
        if not isinstance(name, str) or not name.strip() or len(name) > 64 or any(ord(c) < 32 for c in name):
            raise ValueError("Printer names must contain 1 to 64 display characters")
        url = item.get("url")
        if not isinstance(url, str) or len(url) > 240:
            raise ValueError("Printer URL must be an HTTP(S) URL")
        try:
            parsed = urlsplit(url)
            port = parsed.port
        except ValueError as exc:
            raise ValueError("Invalid printer URL") from exc
        if (parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username is not None
                or parsed.password is not None or parsed.query or parsed.fragment or parsed.path not in ("", "/")
                or any(c.isspace() for c in url) or (port is not None and port < 1)):
            raise ValueError("Printer URL must identify a fixed HTTP(S) host without credentials, query or path")
        normalized = dict(id=identity, name=name.strip(), url=url.rstrip("/"),
                          poll_interval_s=item.get("poll_interval_s", 5), timeout_s=item.get("timeout_s", 2.5),
                          ttl_s=item.get("ttl_s", 15), api_key_file=item.get("api_key_file", ""))
        for key, low, high in (("poll_interval_s", 2, 300), ("timeout_s", .5, 3), ("ttl_s", 5, 900)):
            value = normalized[key]
            if isinstance(value, bool) or not isinstance(value, (int, float)) or finite(value, low, high) is None:
                raise ValueError(f"Printer {key} is outside the supported bounds")
        if normalized["ttl_s"] < normalized["poll_interval_s"] * 2:
            raise ValueError("Printer ttl_s must allow at least two polling intervals")
        key_file = normalized["api_key_file"]
        if not isinstance(key_file, str) or (key_file and not Path(key_file).is_absolute()):
            raise ValueError("Printer api_key_file must be empty or an absolute local path")
        identities.add(identity)
        result.append(normalized)
    return result


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None  # Keep optional API credentials on the configured origin.


def empty_printer(identity):
    return {"id": identity, "host_name": "", "klippy_state": "unknown", "state": "unknown", "message": "",
            "filename": "", "progress_pct": None, "progress_basis": None, "print_duration_s": None,
            "total_duration_s": None, "current_layer": None, "total_layers": None, "filament_used_mm": None,
            "slicer_estimated_time_s": None, "remaining_s": None, "eta_at": None, "eta_basis": None,
            "heaters": [], "temperatures": [], "fan_pct": None, "filament_detected": None,
            "updated_at": None, "age_s": None, "error": None}


def normalize_printer(identity, status, metadata, host_name, now):
    """Keep job state authoritative; progress/ETA are explicitly estimates."""
    result = empty_printer(identity)
    hooks, stats = status.get("webhooks", {}), status.get("print_stats", {})
    result.update(host_name=text(host_name, 64), klippy_state=text(hooks.get("state") or "unknown", 24),
                  message=text(stats.get("message") or hooks.get("state_message") or status.get("display_status", {}).get("message"), 128),
                  updated_at=round(now, 3), age_s=0)
    state = stats.get("state")
    result["state"] = state if state in ("standby", "printing", "paused", "complete", "error", "cancelled") else "unknown"
    if status.get("pause_resume", {}).get("is_paused") is True and result["state"] == "printing":
        result["state"] = "paused"
    filename = stats.get("filename")
    result["filename"] = text(PurePosixPath(filename).name if isinstance(filename, str) else "", 96)
    result["print_duration_s"] = finite(stats.get("print_duration"), 0)
    result["total_duration_s"] = finite(stats.get("total_duration"), 0)
    result["filament_used_mm"] = finite(stats.get("filament_used"), 0)
    info = stats.get("info") if isinstance(stats.get("info"), dict) else {}
    for key, field in (("current_layer", "current_layer"), ("total_layers", "total_layer")):
        value = finite(info.get(field), 0, 1000000)
        result[key] = int(value) if value is not None and value.is_integer() else None
    progress = finite(status.get("display_status", {}).get("progress"), 0, 1)
    result["progress_basis"] = "display-status" if progress is not None else None
    if progress is None:
        progress = finite(status.get("virtual_sdcard", {}).get("progress"), 0, 1)
        result["progress_basis"] = "file-position" if progress is not None else None
    result["progress_pct"] = round(progress * 100, 2) if progress is not None else None
    result["slicer_estimated_time_s"] = finite(metadata.get("estimated_time"), .001)
    duration = result["print_duration_s"]
    if result["state"] == "complete":
        result.update(progress_pct=100, remaining_s=0, eta_basis="completed")
    elif (result["state"] in ("printing", "paused") and progress is not None and .05 <= progress < 1
          and duration is not None and duration >= 120):
        remaining = duration * (1 - progress) / progress
        result.update(remaining_s=round(remaining), eta_basis="progress-average")
        if result["state"] == "printing":
            result["eta_at"] = round(now + remaining)
    for name, values in status.items():
        if not isinstance(values, dict):
            continue
        if name == "heater_bed" or re.fullmatch(r"extruder\d*", name) or name.startswith("heater_generic "):
            if len(result["heaters"]) < 4:
                duty = finite(values.get("power"), 0, 1)
                label = "Bed" if name == "heater_bed" else "Nozzle" if name == "extruder" else name.split(" ", 1)[-1]
                result["heaters"].append({"name": text(label, 32), "temp_c": finite(values.get("temperature"), -50, 500),
                                          "target_c": finite(values.get("target"), 0, 500),
                                          "duty_pct": round(duty * 100, 2) if duty is not None else None})
        elif name.startswith("temperature_sensor ") and len(result["temperatures"]) < 8:
            result["temperatures"].append({"name": text(name.split(" ", 1)[1], 32),
                                           "temp_c": finite(values.get("temperature"), -50, 250)})
    fan = finite(status.get("fan", {}).get("speed"), 0, 1)
    result["fan_pct"] = round(fan * 100, 2) if fan is not None else None
    filament = [v["filament_detected"] for k, v in status.items()
                if k.startswith(("filament_switch_sensor ", "filament_motion_sensor ")) and isinstance(v, dict)
                and v.get("enabled") is True and type(v.get("filament_detected")) is bool]
    result["filament_detected"] = all(filament) if filament else None
    if result["klippy_state"] != "ready":
        # A previous job can persist in Klipper objects after a shutdown.
        result.update(state="unknown", progress_pct=None, remaining_s=None, eta_at=None, eta_basis=None)
    return result


class MoonrakerReader:
    def __init__(self, config, opener=None):
        self.config = validate_printers([config])[0]
        self.opener = opener or build_opener(NoRedirect()).open
        self.objects = CORE_OBJECTS.copy()
        self.discovered, self.host_name = False, ""
        self.metadata, self.metadata_key, self.metadata_retry_at = {}, None, 0
        self.previous_eventtime, self.previous_duration, self.previous_filename = None, None, None

    def get(self, endpoint, params=None):
        if endpoint not in ("/server/info", "/printer/info", "/printer/objects/list", "/printer/objects/query", "/server/files/metadata"):
            raise ValueError("Unsupported read-only printer endpoint")
        url = self.config["url"] + endpoint + ("?" + urlencode(params) if params else "")
        headers = {"Accept": "application/json"}
        if self.config["api_key_file"]:
            key = Path(self.config["api_key_file"]).read_text().strip()
            if not key or len(key) > 256 or not key.isascii() or any(c.isspace() for c in key):
                raise ValueError("Invalid printer API key file")
            headers["X-Api-Key"] = key
        with self.opener(Request(url, headers=headers, method="GET"), timeout=self.config["timeout_s"]) as response:
            raw = response.read(MAX_RESPONSE + 1)
        if len(raw) > MAX_RESPONSE:
            raise ValueError("Moonraker response exceeds the monitoring bound")
        def invalid(value):
            raise ValueError("Nonfinite Moonraker JSON")
        document = json.loads(raw, parse_constant=invalid)
        if not isinstance(document, dict) or "error" in document or not isinstance(document.get("result"), dict):
            raise ValueError("Invalid Moonraker response")
        return document["result"]

    def discover(self):
        data = self.get("/printer/objects/list")
        objects = data.get("objects")
        if not isinstance(objects, list):
            raise ValueError("Moonraker object list unavailable")
        self.objects = {name: fields for name, fields in BASE_OBJECTS.items() if name in objects}
        if not all(name in objects for name in CORE_OBJECTS):
            raise ValueError("Klipper core webhooks/print_stats objects are unavailable")
        heaters = temps = filaments = 0
        for name in sorted(o for o in objects if isinstance(o, str)):
            if (name == "heater_bed" or re.fullmatch(r"extruder\d*", name) or name.startswith("heater_generic ")) and heaters < 4:
                self.objects[name] = "temperature,target,power"
                heaters += 1
            elif name.startswith("temperature_sensor ") and temps < 8:
                self.objects[name] = "temperature"
                temps += 1
            elif name.startswith(("filament_switch_sensor ", "filament_motion_sensor ")) and filaments < 4:
                self.objects[name] = "enabled,filament_detected"
                filaments += 1
        self.discovered = True
        try:
            self.host_name = text(self.get("/printer/info").get("hostname"), 64)
        except (OSError, ValueError):
            pass  # Host label is supplementary; do not discard valid telemetry.

    def read(self):
        if not self.discovered:
            try:
                self.discover()
            except (OSError, ValueError):
                pass  # Retry discovery later, while core status can still work.
        try:
            data = self.get("/printer/objects/query", self.objects)
        except (OSError, ValueError) as query_error:
            server = self.get("/server/info")
            state = server.get("klippy_state")
            if state in ("startup", "shutdown", "error", "disconnected"):
                self.discovered = False
                data = {"status": {"webhooks": {"state": state, "state_message": f"Klipper is {state}"}}}
                try:
                    info = self.get("/printer/info")
                    data["status"]["webhooks"]["state_message"] = info.get("state_message") or data["status"]["webhooks"]["state_message"]
                except (OSError, ValueError):
                    pass
            else:
                raise query_error
        status = data.get("status")
        if not isinstance(status, dict) or not isinstance(status.get("webhooks"), dict):
            raise ValueError("Klipper status response is incomplete")
        if status["webhooks"].get("state") == "ready" and not isinstance(status.get("print_stats"), dict):
            raise ValueError("Klipper print_stats status is unavailable")
        stats = status.get("print_stats") if isinstance(status.get("print_stats"), dict) else {}
        filename = stats.get("filename") if isinstance(stats.get("filename"), str) else ""
        duration, eventtime = finite(stats.get("print_duration"), 0), finite(data.get("eventtime"), 0)
        restarted = ((eventtime is not None and self.previous_eventtime is not None and eventtime < self.previous_eventtime)
                     or (duration is not None and self.previous_duration is not None and duration < self.previous_duration))
        if restarted or filename != self.previous_filename:
            self.metadata, self.metadata_key, self.metadata_retry_at = {}, None, 0
        if restarted:
            self.discovered = False
        now = time.time()
        if filename and (self.metadata_key != filename) and now >= self.metadata_retry_at:
            try:
                self.metadata = self.get("/server/files/metadata", {"filename": filename})
                self.metadata_key = filename
            except (OSError, ValueError):
                self.metadata_retry_at = now + 60
        self.previous_eventtime, self.previous_duration, self.previous_filename = eventtime, duration, filename
        printer = normalize_printer(self.config["id"], status, self.metadata, self.host_name, now)
        return {"printer": printer, "generated_at": round(now, 3), "error": None}


def printer_snapshot(config, state, data, sequence, now):
    """Build an isolated schema1 printer node; all absent host metrics are null."""
    printer = copy.deepcopy(data["printer"]) if data else empty_printer(config["id"])
    own_state = state.copy()
    stamp = printer.get("updated_at") or state.get("updated_at")
    age = max(0, now - stamp) if stamp is not None else None
    if not state.get("ok") or age is None or age > config["ttl_s"]:
        previous = printer
        printer = empty_printer(config["id"])
        printer.update(host_name=previous.get("host_name", ""), filename=previous.get("filename", ""),
                       updated_at=stamp, age_s=round(age, 1) if age is not None else None,
                       error=text(state.get("error") or "Printer sample expired/unavailable", 128))
        own_state.update(ok=False, error=printer["error"], updated_at=stamp, age_s=printer["age_s"])
    else:
        printer["age_s"] = round(age, 1)
        own_state.update(updated_at=stamp, age_s=printer["age_s"])
    printer["ttl_s"] = config["ttl_s"]
    host = {key: None for key in ("uptime_s", "cpu_pct", "mem_used_bytes", "mem_total_bytes", "swap_used_bytes", "swap_total_bytes",
                                  "arc_bytes", "io_wait_pct", "net_rx_bps", "net_tx_bps", "disk_read_bps", "disk_write_bps")}
    host.update(name=config["name"], ip=urlsplit(config["url"]).hostname, cpu_cores=[], load=[])
    alerts = []
    def alert(identity, severity, message):
        alerts.append({"id": f"printer/{config['id']}/{identity}", "severity": severity, "message": text(message, 120)})
    if printer["error"] and state.get("error") != "initializing":
        alert("unavailable", "warning", "Printer telemetry unavailable")
    elif printer["klippy_state"] in ("shutdown", "error"):
        alert("klipper", "critical", printer["message"] or f"Klipper {printer['klippy_state']}")
    elif printer["klippy_state"] in ("startup", "disconnected"):
        alert("klipper", "warning", printer["message"] or f"Klipper {printer['klippy_state']}")
    elif printer["state"] == "error":
        alert("print", "critical", printer["message"] or "Print job failed")
    elif printer["state"] == "paused":
        alert("paused", "warning", "Print paused")
    if printer["filament_detected"] is False and printer["state"] in ("printing", "paused"):
        alert("filament", "warning", "Printer filament not detected")
    if printer["klippy_state"] == "ready" and printer["state"] == "unknown":
        alert("status", "warning", "Printer job state unavailable")
    return {"schema": 1, "sequence": sequence, "generated_at": round(now, 3), "host": host,
            "power": {"package_w": None, "cores_w": None, "graphics_w": None, "cpu_mhz": None,
                      "busy_mhz": None, "cpu_temp_c": None, "cstate_pct": {}},
            "guests": [], "storage": [], "disks": [], "sensors": [], "gpus": [], "alerts": alerts,
            "sources": {"moonraker": own_state}, "printer": printer,
            "faults": {"lookback_days": 7, "segfault_count_24h": None, "event_count_24h": None,
                       "last_event_at": None, "events": []}, "limits": {"counts": {}}}
