"""Fixed read-only upstream monitor feeds; no transparent proxy or redirects."""
from __future__ import annotations

import copy
import json
from pathlib import Path
import time
from urllib.parse import urlsplit
from urllib.request import Request, build_opener

from agent.printers import NoRedirect, finite, validate_printers

MAX_RESPONSE = 48 * 1024
SNAPSHOT_FIELDS = {"schema", "sequence", "generated_at", "host", "power", "guests", "storage", "disks",
                   "sensors", "gpus", "alerts", "sources", "faults", "limits"}


def validate_remote_collectors(values):
    if not isinstance(values, list) or len(values) > 4:
        raise ValueError("remote_collectors must be a list of at most four nodes")
    normalized = []
    allowed = {"id", "name", "url", "token_file", "poll_interval_s", "timeout_s", "ttl_s"}
    for value in values:
        if not isinstance(value, dict) or set(value) - allowed:
            raise ValueError("Remote collectors contain unsupported fields")
        item = value.copy()
        url = item.get("url")
        if isinstance(url, str) and urlsplit(url).path == "/api/v1/snapshot" and not urlsplit(url).query and not urlsplit(url).fragment:
            # Accept the display's full endpoint, store one fixed origin.
            item["url"] = url[:url.index("/api/v1/snapshot")]
        item["api_key_file"] = item.pop("token_file", "")
        normalized.append(item)
    result = validate_printers(normalized)
    for item in result:
        item["token_file"] = item.pop("api_key_file")
    return result


def empty_snapshot(name, address, sequence, now):
    """Safe unknown values, used for a missing or failed upstream sample."""
    from agent.printers import printer_snapshot
    dummy = {"id": "unused", "name": name, "url": "http://" + address, "ttl_s": 15}
    snapshot = printer_snapshot(dummy, {"ok": False, "enabled": True, "updated_at": None,
                                       "error": "initializing"}, None, sequence, now)
    snapshot.pop("printer")
    snapshot["sources"], snapshot["alerts"] = {}, []
    snapshot["host"].update(name=name, ip=address)
    return snapshot


class RemoteCollectorReader:
    def __init__(self, config, opener=None):
        self.config = validate_remote_collectors([config])[0]
        self.opener = opener or build_opener(NoRedirect()).open
        self.last_sequence, self.sequence_at = None, None

    def read(self):
        headers = {"Accept": "application/json"}
        if self.config["token_file"]:
            token = Path(self.config["token_file"]).read_text().strip()
            if len(token) < 32 or len(token) > 256 or not token.isascii() or any(c.isspace() for c in token):
                raise ValueError("Invalid remote display-token file")
            headers["Authorization"] = "Bearer " + token
        request = Request(self.config["url"] + "/api/v1/snapshot", headers=headers, method="GET")
        with self.opener(request, timeout=self.config["timeout_s"]) as response:
            raw = response.read(MAX_RESPONSE + 1)
        if len(raw) > MAX_RESPONSE:
            raise ValueError("Remote snapshot exceeds display capacity")
        def invalid(value):
            raise ValueError("Nonfinite remote snapshot")
        document = json.loads(raw, parse_constant=invalid)
        from agent.server import sample_timestamp
        if (not isinstance(document, dict) or type(document.get("schema")) is not int or document["schema"] != 1
                or document.get("demo") is True or "printer" in document
                or not isinstance(document.get("host"), dict)
                or type(document.get("sequence")) is not int or document["sequence"] < 0):
            raise ValueError("Expected a live schema1 Proxmox snapshot")
        now = time.time()
        stamp = sample_timestamp(document, now)
        if now - stamp > self.config["ttl_s"]:
            raise ValueError("Remote snapshot is stale")
        if document["sequence"] != self.last_sequence:
            self.last_sequence, self.sequence_at = document["sequence"], now
        elif now - self.sequence_at > self.config["ttl_s"]:
            raise ValueError("Remote snapshot sequence has stopped")
        for field in ("guests", "storage", "disks", "sensors", "gpus", "alerts"):
            if not isinstance(document.get(field, []), list) or not all(isinstance(item, dict) for item in document.get(field, [])):
                raise ValueError("Invalid remote inventory")
        if not isinstance(document.get("sources"), dict) or not all(isinstance(value, dict) for value in document["sources"].values()):
            raise ValueError("Invalid remote source status")
        for field in ("power", "limits", "faults"):
            if field in document and not isinstance(document[field], dict):
                raise ValueError("Invalid remote snapshot object")
        descriptor = document.get("node")
        if descriptor is not None and (not isinstance(descriptor, dict) or descriptor.get("type") != "proxmox"):
            raise ValueError("Remote endpoint is not a Proxmox feed")
        # Discard the upstream registry, URLs and unknown top-level extensions.
        own = {key: copy.deepcopy(value) for key, value in document.items() if key in SNAPSHOT_FIELDS}
        own.setdefault("power", {})
        own.setdefault("limits", {})
        for field in ("guests", "storage", "disks", "sensors", "gpus", "alerts"):
            own.setdefault(field, [])
        return {"snapshot": own, "generated_at": stamp, "error": None}


def remote_snapshot(config, state, data, sequence, now):
    """Expose upstream age; failed or stale feeds never retain live values."""
    stamp = data.get("generated_at") if data else state.get("updated_at")
    age = max(0, now - stamp) if stamp is not None else None
    own_state = state.copy()
    own_state.update(updated_at=stamp, age_s=round(age, 1) if age is not None else None)
    available = state.get("ok") and data and age is not None and age <= config["ttl_s"]
    if available:
        snapshot = copy.deepcopy(data["snapshot"])
        for source in snapshot.get("sources", {}).values():
            updated = finite(source.get("updated_at"))
            if updated is not None:
                source["age_s"] = round(max(0, now - updated), 1)
        for field, age_field, stamp_field in (("guests", "mem_age_s", "mem_updated_at"),
                                              ("gpus", "age_s", "updated_at"),
                                              ("sensors", "age_s", "updated_at")):
            for item in snapshot.get(field, []):
                updated = finite(item.get(stamp_field))
                if updated is not None:
                    item[age_field] = round(max(0, now - updated), 1)
    else:
        snapshot = empty_snapshot(config["name"], urlsplit(config["url"]).hostname, sequence, now)
        error = state.get("error") or "Remote snapshot expired/unavailable"
        own_state.update(ok=False, error=error)
        if state.get("error") != "initializing":
            snapshot["alerts"] = [{"id": f"remote/{config['id']}/unavailable", "severity": "warning",
                                   "message": "Remote Proxmox telemetry unavailable"}]
    snapshot.update(sequence=sequence, generated_at=round(now, 3))
    snapshot["sources"] = {**snapshot.get("sources", {}), "remote_feed": own_state}
    snapshot["feed"] = {"updated_at": stamp, "age_s": own_state["age_s"], "ttl_s": config["ttl_s"],
                        "error": own_state.get("error")}
    return snapshot
