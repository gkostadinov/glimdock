#!/usr/bin/env python3
"""Privileged, fixed-scope node configuration over a local Unix socket.

The HTTP reader never receives private config or key paths. This service accepts
only a public projection request or one validated node upsert/delete, publishes
config atomically, and queues a restart of the collector unit only. It has no VM,
printer, shell, arbitrary file or arbitrary service operation.
"""
from __future__ import annotations

import argparse
import copy
import fcntl
import grp
import hashlib
import json
import logging
import os
from pathlib import Path
import re
import socket
import socketserver
import stat
import subprocess
import tempfile
import threading
from urllib.parse import urlsplit

from agent.collector import Config, encode_snapshot, proxmox_node_id

LOG = logging.getLogger("homelab.config")
MAX_REQUEST = 12 * 1024
MAX_RESPONSE = 16 * 1024
SOCKET_PATH = "/run/homelab-monitor/config.sock"
COLLECTOR_UNIT = "homelab-monitor-collector.service"
SLUG = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,31}\Z")
KINDS = {"klipper": ("printers", "klipper", "api_key_file"),
         "proxmox-feed": ("remote_collectors", "remote", "token_file")}


def same_origin(first, second):
    try:
        def origin(value):
            parsed = urlsplit(value)
            return parsed.scheme, parsed.hostname, parsed.port or (443 if parsed.scheme == "https" else 80)
        return origin(first) == origin(second)
    except (TypeError, ValueError):
        return False


class ConfigError(Exception):
    def __init__(self, status, message):
        self.status, self.message = status, message
        super().__init__(message)


def strict_json(raw):
    def invalid(value):
        raise ValueError("Nonfinite configuration JSON")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("Duplicate configuration field")
            result[key] = value
        return result
    return json.loads(raw, parse_constant=invalid, object_pairs_hook=unique)


def atomic_private(path, payload):
    """Private same-directory replacement; readers see complete old/new files."""
    temporary = None
    try:
        fd, temporary = tempfile.mkstemp(prefix=".config-", dir=path.parent)
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


class ConfigManager:
    def __init__(self, path=Path("/etc/homelab-monitor/config.json"), apply=None):
        self.path = Path(path)
        self.secret_dir = self.path.parent / "node-secrets"
        self.apply = apply or (lambda: None)
        self.lock = threading.Lock()

    def _load(self):
        try:
            raw = self.path.read_bytes()
            if len(raw) > 64 * 1024:
                raise ValueError("Configuration too large")
            document = strict_json(raw)
            config = Config.from_dict(document)
        except (OSError, ValueError, TypeError):
            raise ConfigError(503, "Configuration unavailable") from None
        return document, config, hashlib.sha256(raw).hexdigest()

    @staticmethod
    def projection(config, version):
        native = config.node or socket.gethostname()
        identity = proxmox_node_id(native)
        address = config.host_ip
        authority = "[" + address + "]" if ":" in address else address
        local = {"id": identity, "type": "local-proxmox", "origin": "local",
                 "name": config.display_name or native, "url": f"http://{authority}:8765/api/v1/snapshot",
                 "poll_interval_s": config.interval_s, "timeout_s": None, "ttl_s": 15, "has_secret": False}
        nodes = [local] if config.enable_proxmox else []
        for kind, (field, prefix, key) in KINDS.items():
            for item in getattr(config, field):
                nodes.append({"id": f"{prefix}:{item['id']}", "type": kind, "origin": "config",
                              "name": item["name"], "url": item["url"], "poll_interval_s": item["poll_interval_s"],
                              "timeout_s": item["timeout_s"], "ttl_s": item["ttl_s"], "has_secret": bool(item[key])})
        return {"schema": 1, "version": version, "max_nodes": 4, "nodes": nodes,
                "local_node": {"id": identity, "name": local["name"], "address": address, "enabled": config.enable_proxmox}}

    def get(self):
        with self.lock:
            _, config, version = self._load()
            return self.projection(config, version)

    @staticmethod
    def _identity(requested, prefix, name, existing):
        if not isinstance(requested, str):
            raise ConfigError(400, "Node id must be a string")
        if requested:
            if ":" in requested:
                supplied_prefix, requested = requested.split(":", 1)
                if supplied_prefix != prefix:
                    raise ConfigError(400, "Node id does not match its type")
            if not SLUG.fullmatch(requested):
                raise ConfigError(400, "Node id must be a stable ASCII name of at most 32 characters")
            return requested
        slug = re.sub(r"[^A-Za-z0-9_.-]+", "-", name).strip("_.-")[:32] or "node"
        if not slug[0].isalnum():
            slug = "node"
        candidate, suffix = slug, 2
        while candidate in existing:
            tail = f"-{suffix}"
            candidate = slug[:32 - len(tail)] + tail
            suffix += 1
        return candidate

    def mutate(self, request):
        if not isinstance(request, dict) or set(request) - {"action", "version", "node", "id"}:
            raise ConfigError(400, "Unsupported configuration request")
        action = request.get("action")
        if action not in ("upsert", "delete"):
            raise ConfigError(400, "Only node upsert/delete is supported")
        version = request.get("version")
        if not isinstance(version, str) or not re.fullmatch(r"[0-9a-f]{64}", version):
            raise ConfigError(400, "Configuration version is required")
        with self.lock:
            # Other config-service instances and optimistic clients cannot
            # overwrite a committed change after reading an earlier version.
            lock_path = self.path.parent / ".config.lock"
            fd = os.open(lock_path, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "w") as stream:
                fcntl.flock(stream, fcntl.LOCK_EX)
                document, config, actual = self._load()
                if actual != version:
                    raise ConfigError(409, "Configuration changed; reload before saving")
                changed = copy.deepcopy(document)
                old_secrets = {item[key] for _, (field, _, key) in KINDS.items()
                               for item in getattr(config, field) if item.get(key)}
                secret = None
                secret_field = secret_identity = None
                local_id = proxmox_node_id(config.node or socket.gethostname())
                if action == "delete":
                    if "node" in request or not isinstance(request.get("id"), str):
                        raise ConfigError(400, "Delete requires a node id")
                    identity = request["id"]
                    if identity == local_id:
                        changed["enable_proxmox"] = False
                    else:
                        found = False
                        for _, (field, prefix, _) in KINDS.items():
                            values = changed.get(field, [])
                            remaining = [item for item in values if f"{prefix}:{item['id']}" != identity]
                            if len(remaining) != len(values):
                                changed[field], found = remaining, True
                        if not found:
                            raise ConfigError(400, "Unknown node id")
                else:
                    if "id" in request:
                        raise ConfigError(400, "Upsert requires a node object")
                    node = request.get("node")
                    allowed = {"id", "type", "name", "url", "poll_interval_s", "timeout_s", "ttl_s", "secret", "clear_secret"}
                    if not isinstance(node, dict) or set(node) - allowed:
                        raise ConfigError(400, "Unsupported node fields")
                    kind, name = node.get("type"), node.get("name")
                    if not isinstance(name, str) or not name.strip() or len(name) > 64 or any(ord(c) < 32 for c in name):
                        raise ConfigError(400, "Node name must contain 1 to 64 display characters")
                    if kind == "local-proxmox":
                        identity = node.get("id", "")
                        if identity not in ("", local_id):
                            raise ConfigError(400, "Local node identity is fixed")
                        current = self.projection(config, actual)["local_node"]
                        authority = f"[{current['address']}]" if ":" in current["address"] else current["address"]
                        local_url = f"http://{authority}:8765/api/v1/snapshot"
                        if node.get("url") not in (None, "", local_url, current["address"]):
                            raise ConfigError(400, "Local address is fixed; configure a remote feed for another host")
                        if (not isinstance(node.get("secret", ""), str) or type(node.get("clear_secret", False)) is not bool
                                or node.get("secret") or node.get("clear_secret")):
                            raise ConfigError(400, "Local node does not accept a secret")
                        changed.update(display_name=name.strip(), enable_proxmox=True)
                    elif kind in KINDS:
                        field, prefix, secret_field = KINDS[kind]
                        values = copy.deepcopy(getattr(config, field))
                        identity = self._identity(node.get("id", ""), prefix, name, {value["id"] for value in values})
                        old = next((value for value in values if value["id"] == identity), {})
                        updated = {**old, "id": identity, "name": name.strip()}
                        for key in ("url", "poll_interval_s", "timeout_s", "ttl_s"):
                            if key in node:
                                updated[key] = node[key]
                        if type(node.get("clear_secret", False)) is not bool:
                            raise ConfigError(400, "clear_secret must be true or false")
                        if "secret" in node and not isinstance(node["secret"], str):
                            raise ConfigError(400, "Secret must be a string")
                        candidate = node.get("secret", "")
                        if candidate and (len(candidate) > 256 or not candidate.isascii() or any(c.isspace() for c in candidate)):
                            raise ConfigError(400, "Secret must be at most 256 ASCII characters without whitespace")
                        if candidate and node.get("clear_secret"):
                            raise ConfigError(400, "Supply a secret or clear it, not both")
                        if kind == "proxmox-feed" and candidate and len(candidate) < 32:
                            raise ConfigError(400, "Remote display token must contain at least 32 characters")
                        if candidate:
                            secret = candidate.encode("ascii") + b"\n"
                            secret_identity = f"{prefix}-{identity}-{hashlib.sha256(secret).hexdigest()[:12]}.token"
                            updated[secret_field] = str(self.secret_dir / secret_identity)
                        elif node.get("clear_secret"):
                            updated[secret_field] = ""
                        elif old and not same_origin(old.get("url", ""), updated.get("url", "")):
                            updated[secret_field] = ""
                        else:
                            updated.setdefault(secret_field, "")
                        changed[field] = [value for value in values if value["id"] != identity] + [updated]
                    else:
                        raise ConfigError(400, "Unsupported node type")
                try:
                    validated = Config.from_dict(changed)
                except (TypeError, ValueError):
                    raise ConfigError(400, "Invalid node configuration, URL, polling, or four-node capacity") from None
                created, existed = None, False
                if secret is not None:
                    self.secret_dir.mkdir(mode=0o700, exist_ok=True)
                    if self.secret_dir.is_symlink() or not self.secret_dir.is_dir():
                        raise ConfigError(503, "Managed secret storage unavailable")
                    os.chmod(self.secret_dir, 0o700)
                    created = self.secret_dir / secret_identity
                    existed = created.exists()
                    try:
                        atomic_private(created, secret)
                    except OSError:
                        raise ConfigError(503, "Managed secret could not be saved") from None
                try:
                    raw = json.dumps(changed, indent=2, ensure_ascii=False, allow_nan=False).encode("utf-8") + b"\n"
                    atomic_private(self.path, raw)
                except OSError:
                    if created is not None and not existed:
                        created.unlink(missing_ok=True)
                    raise ConfigError(503, "Configuration could not be saved") from None
                new_version = hashlib.sha256(raw).hexdigest()
                projection = self.projection(validated, new_version)
                new_secrets = {item[key] for _, (field, _, key) in KINDS.items()
                               for item in getattr(validated, field) if item.get(key)}
                for unused in old_secrets - new_secrets:
                    path = Path(unused)
                    if path.parent == self.secret_dir and re.fullmatch(r"(?:klipper|remote)-[A-Za-z0-9_.-]+-[0-9a-f]{12}\.token", path.name):
                        try:
                            path.unlink(missing_ok=True)
                        except OSError:
                            LOG.warning("An unused managed node secret could not be removed")
        self.apply()
        return {"config": projection, "applying": True}


class RestartQueue:
    """Coalesce rapid saves into one bounded restart of a single fixed unit."""
    def __init__(self, runner=subprocess.run):
        self.runner, self.pending, self.stop = runner, threading.Event(), threading.Event()
        self.thread = threading.Thread(target=self._work, name="config-apply", daemon=True)
        self.thread.start()

    def submit(self):
        self.pending.set()

    def _work(self):
        while not self.stop.is_set():
            if not self.pending.wait(.5):
                continue
            self.pending.clear()
            if self.stop.wait(.25):
                break
            try:
                result = self.runner(["/usr/bin/systemctl", "restart", COLLECTOR_UNIT],
                                     timeout=100, capture_output=True, check=False)
                if result.returncode:
                    LOG.error("Collector apply failed; configuration is saved")
            except (OSError, subprocess.TimeoutExpired):
                LOG.error("Collector apply timed out/failed; configuration is saved")

    def close(self):
        self.stop.set()
        self.pending.set()
        self.thread.join(timeout=1)


class ConfigSocketServer(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True
    request_queue_size = 8

    def __init__(self, path, manager):
        self.manager, self.clients = manager, threading.BoundedSemaphore(8)
        super().__init__(str(path), ConfigSocketHandler)

    def process_request(self, request, address):
        if not self.clients.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, address)
        except Exception:
            self.clients.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.clients.release()


class ConfigSocketHandler(socketserver.StreamRequestHandler):
    def handle(self):
        self.request.settimeout(5)
        try:
            raw = self.rfile.readline(MAX_REQUEST + 1)
            if len(raw) > MAX_REQUEST or not raw.endswith(b"\n"):
                raise ConfigError(400, "Configuration request too large")
            request = strict_json(raw)
            if not isinstance(request, dict) or set(request) - {"method", "body"}:
                raise ConfigError(400, "Unsupported configuration operation")
            if request.get("method") == "GET" and "body" not in request:
                response = {"status": 200, "body": self.server.manager.get()}
            elif request.get("method") == "POST" and "body" in request:
                response = {"status": 202, "body": self.server.manager.mutate(request["body"])}
            else:
                raise ConfigError(400, "Only configuration GET/POST is supported")
        except ConfigError as exc:
            response = {"status": exc.status, "body": {"error": exc.message}}
        except (OSError, ValueError, TypeError):
            response = {"status": 503, "body": {"error": "Configuration service unavailable"}}
        self.wfile.write(encode_snapshot(response) + b"\n")


def forward_config(method, body=None, socket_path=SOCKET_PATH):
    request = {"method": method}
    if body is not None:
        request["body"] = body
    raw = encode_snapshot(request) + b"\n"
    if len(raw) > MAX_REQUEST:
        raise ConfigError(400, "Configuration request too large")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(str(socket_path))
        connection.sendall(raw)
        with connection.makefile("rb") as stream:
            response = stream.readline(MAX_RESPONSE + 1)
    if len(response) > MAX_RESPONSE or not response.endswith(b"\n"):
        raise ValueError("Invalid configuration service response")
    document = strict_json(response)
    if (not isinstance(document, dict) or document.get("status") not in (200, 202, 400, 409, 503)
            or not isinstance(document.get("body"), dict)):
        raise ValueError("Invalid configuration service response")
    return document["status"], document["body"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", default="/etc/homelab-monitor/config.json")
    parser.add_argument("--socket", default=SOCKET_PATH)
    parser.add_argument("--socket-group", default="homelab-monitor")
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    path = Path(args.socket)
    path.parent.mkdir(parents=True, exist_ok=True)
    gid = grp.getgrnam(args.socket_group).gr_gid
    os.chown(path.parent, -1, gid)
    os.chmod(path.parent, 0o2750)
    if path.exists() or path.is_symlink():
        if not stat.S_ISSOCK(path.lstat().st_mode):
            raise SystemExit("Configuration socket path is not a socket")
        path.unlink()
    queue = RestartQueue()
    try:
        with ConfigSocketServer(path, ConfigManager(Path(args.config), queue.submit)) as server:
            os.chown(path, 0, gid)
            os.chmod(path, 0o660)
            server.serve_forever(poll_interval=.25)
    finally:
        queue.close()
        path.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
