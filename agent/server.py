#!/usr/bin/env python3
"""Unprivileged HTTP reader for atomic collector snapshots.

GET /api/v1/snapshot and /api/v1/nodes require the display's Bearer token.
GET /healthz returns only {"ok": bool}; it never exposes host metrics or keys.
The request path cannot invoke commands, choose a file, or control a guest.
Default plain HTTP is suitable for a trusted, isolated LAN; optional cert/key
enable HTTPS without changing this schema. Keep Proxmox credentials off ESP32.
"""
from __future__ import annotations

import argparse
import copy
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import logging
import math
import os
from pathlib import Path
import re
import ssl
import threading
import time
from urllib.parse import parse_qsl, urlsplit

from agent.collector import MAX_PAYLOAD, bounded_snapshot, encode_snapshot, proxmox_node_id
from agent.config_service import ConfigError, MAX_REQUEST, forward_config, strict_json

LOG = logging.getLogger("homelab.server")
MAX_REGISTRY_PAYLOAD = 256 * 1024
MAX_NODES = 4
FRESH_SECONDS = 15
NODE_ID = re.compile(r"(?=.{1,63}\Z)[a-z][a-z0-9_-]{0,31}:[A-Za-z0-9][A-Za-z0-9._-]{0,95}\Z")


class UnknownNodeError(LookupError):
    pass


def sample_timestamp(document: dict, now: float, demo: bool = False) -> float:
    generated = document.get("generated_at")
    if isinstance(generated, bool) or not isinstance(generated, (int, float)) or not math.isfinite(generated):
        raise ValueError("Missing/invalid sample timestamp")
    if generated > now + 60 and not demo:
        raise ValueError("Sample timestamp is in the future")
    return generated


def node_status(snapshot: dict, now: float) -> str:
    """Describe this node only; another node cannot change its health."""
    if not isinstance(snapshot.get("host"), dict):
        return "unknown"
    try:
        generated = sample_timestamp(snapshot, now)
    except (ValueError, TypeError):
        return "unknown"
    if now - generated > FRESH_SECONDS:
        return "offline"
    sources = snapshot.get("sources", {})
    enabled = [source for source in sources.values() if isinstance(source, dict) and source.get("enabled", True) is not False] if isinstance(sources, dict) else []
    if enabled and all(source.get("updated_at") is None and source.get("error") == "initializing" for source in enabled):
        return "unknown"
    if enabled and all(source.get("ok") is False for source in enabled):
        return "offline"
    if snapshot.get("alerts") or any(source.get("ok") is not True for source in enabled):
        return "degraded"
    return "healthy" if enabled else "unknown"


def node_descriptor(record: dict) -> dict:
    identity, kind = record.get("id"), record.get("type")
    if not isinstance(identity, str) or not NODE_ID.fullmatch(identity):
        raise ValueError("Invalid registry node ID")
    if not isinstance(kind, str) or not re.fullmatch(r"[a-z][a-z0-9_-]{0,31}", kind):
        raise ValueError("Invalid registry node type")
    name, address = record.get("name"), record.get("address")
    if not isinstance(name, str) or not name or len(name) > 96:
        raise ValueError("Invalid registry node name")
    if not isinstance(address, str) or len(address) > 255:
        raise ValueError("Invalid registry node address")
    return {"id": identity, "type": kind, "name": name, "address": address}


class SnapshotStore:
    def __init__(self, path: Path, demo: bool = False):
        self.path, self.demo = path, demo
        self.began = time.time()

    def _document(self, now: float) -> dict:
        with self.path.open("rb") as stream:
            raw = stream.read(MAX_REGISTRY_PAYLOAD + 1)
        if len(raw) > MAX_REGISTRY_PAYLOAD:
            raise ValueError("Registry exceeds capacity")
        def reject_constant(value):
            raise ValueError(f"Nonfinite JSON constant: {value}")
        document = json.loads(raw, parse_constant=reject_constant)
        if not isinstance(document, dict) or type(document.get("schema")) is not int or document["schema"] not in (1, 2):
            raise ValueError("Unknown snapshot schema")
        sample_timestamp(document, now, self.demo)
        if document["schema"] == 1:
            if len(raw) > MAX_PAYLOAD:
                raise ValueError("Snapshot exceeds display capacity")
            host = document.get("host", {})
            if not isinstance(host, dict):
                raise ValueError("Invalid snapshot host")
            name = str(host.get("name") or "local")[:96]
            descriptor = {"id": proxmox_node_id(name), "type": "proxmox", "name": name,
                          "address": str(host.get("ip") or host.get("address") or "")[:255]}
            document = {"schema": 2, "generated_at": document["generated_at"],
                        "sequence": document.get("sequence", 0), "nodes": [{**descriptor, "snapshot": document}]}
        nodes = document.get("nodes")
        if not isinstance(nodes, list) or not 0 <= len(nodes) <= MAX_NODES:
            raise ValueError("Invalid registry node count")
        identities = set()
        for record in nodes:
            if not isinstance(record, dict):
                raise ValueError("Invalid registry node")
            descriptor = node_descriptor(record)
            if descriptor["id"] in identities:
                raise ValueError("Duplicate registry node ID")
            identities.add(descriptor["id"])
            snapshot = record.get("snapshot")
            if not isinstance(snapshot, dict) or type(snapshot.get("schema")) is not int or snapshot["schema"] != 1:
                raise ValueError("Unknown node snapshot schema")
        if self.demo:
            document = copy.deepcopy(document)
            document["generated_at"] = round(now, 3)
            document["sequence"] = int((now - self.began) / 3) + 1
            for record in document["nodes"]:
                self._rebase_demo(record["snapshot"], now, document["sequence"])
        return document

    @staticmethod
    def _rebase_demo(snapshot: dict, now: float, sequence: int):
        snapshot["demo"] = True
        snapshot["generated_at"] = round(now, 3)
        snapshot["sequence"] = sequence
        for source in snapshot.get("sources", {}).values():
            source.update(updated_at=round(now, 3), age_s=0)
        for guest in snapshot.get("guests", []):
            guest.update(mem_updated_at=round(now, 3), mem_age_s=0)
        for gpu in snapshot.get("gpus", []):
            gpu.update(updated_at=round(now, 3), age_s=0)

    @staticmethod
    def _metadata(document: dict, now: float) -> list[dict]:
        return [{**node_descriptor(record), "status": node_status(record["snapshot"], now)} for record in document["nodes"]]

    def read(self, node: str | None = None) -> tuple[bytes, float]:
        if node is not None and (not isinstance(node, str) or not NODE_ID.fullmatch(node)):
            raise ValueError("Invalid node selector")
        now = time.time()
        document = self._document(now)
        records = document["nodes"]
        if not records:
            raise UnknownNodeError("no nodes")
        if node is None:
            selected = next((record for record in records if record["type"] == "proxmox"), records[0])
        else:
            selected = next((record for record in records if record["id"] == node), None)
            if selected is None:
                raise UnknownNodeError("Unknown registry node")
        snapshot = copy.deepcopy(selected["snapshot"])
        if not isinstance(snapshot.get("host"), dict):
            raise ValueError("Invalid snapshot host")
        generated = sample_timestamp(snapshot, now, self.demo)
        metadata = self._metadata(document, now)
        snapshot["node"] = next(item for item in metadata if item["id"] == selected["id"])
        snapshot["nodes"] = metadata
        raw = encode_snapshot(snapshot)
        if len(raw) > MAX_PAYLOAD:
            raw = encode_snapshot(bounded_snapshot(snapshot))
        return raw, max(0, now - generated)

    def read_nodes(self) -> bytes:
        now = time.time()
        document = self._document(now)
        return encode_snapshot({"schema": 2, "generated_at": document["generated_at"],
                                "sequence": document.get("sequence", 0), "nodes": self._metadata(document, now)})


class SnapshotHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    request_queue_size = 16

    def __init__(self, address, token: str, store: SnapshotStore, max_clients: int = 12,
                 setup_token: str | None = None, config_forwarder=forward_config):
        self.token, self.store = token, store
        if setup_token and setup_token == token:
            raise ValueError("Setup and display tokens must be different")
        self.setup_token, self.config_forwarder = setup_token, config_forwarder
        self.clients = threading.BoundedSemaphore(max_clients)
        super().__init__(address, SnapshotHandler)

    def process_request(self, request, client_address):
        if not self.clients.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except Exception:
            self.clients.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.clients.release()


class SnapshotHandler(BaseHTTPRequestHandler):
    server_version = "HomelabMonitor/1"
    sys_version = ""
    protocol_version = "HTTP/1.0"

    def setup(self):
        super().setup()
        self.connection.settimeout(5)

    def log_message(self, fmt, *args):
        # Do not log request targets/headers (tokens may be sent incorrectly by
        # a client). A numeric response code is sufficient for systemd logs.
        pass

    def reply(self, code: int, payload: bytes, content_type: str = "application/json; charset=utf-8", age: float | None = None):
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Connection", "close")
        if age is not None:
            self.send_header("X-Snapshot-Age", f"{age:.1f}")
        if code == 401:
            self.send_header("WWW-Authenticate", 'Bearer realm="homelab-display"')
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)

    def do_GET(self):
        target = urlsplit(self.path)
        if target.path == "/api/v1/config" and not target.query:
            self.config_operation("GET")
            return
        if target.path == "/healthz" and not target.query:
            try:
                _, age = self.server.store.read()
                healthy = age <= FRESH_SECONDS
            except (OSError, ValueError, TypeError, UnknownNodeError):
                healthy = False
            self.reply(200 if healthy else 503, b'{"ok":true}' if healthy else b'{"ok":false}')
            return
        if target.path not in ("/api/v1/snapshot", "/api/v1/nodes"):
            self.reply(404, b'{"error":"not found"}')
            return
        node, invalid_selector = None, False
        if target.query:
            if target.path == "/api/v1/nodes":
                self.reply(404, b'{"error":"not found"}')
                return
            try:
                pairs = parse_qsl(target.query, keep_blank_values=True, strict_parsing=True, max_num_fields=4)
            except ValueError:
                pairs, invalid_selector = [], True
            if any(key != "node" for key, _ in pairs):
                self.reply(404, b'{"error":"not found"}')
                return
            if len(pairs) != 1 or not NODE_ID.fullmatch(pairs[0][1]):
                invalid_selector = True
            else:
                node = pairs[0][1]
        supplied = self.headers.get("Authorization", "")
        expected = f"Bearer {self.server.token}"
        # compare_digest requires matching character encodings; never emit the
        # invalid header or token to output/logs on failure.
        if not hmac.compare_digest(supplied.encode("utf-8"), expected.encode("utf-8")):
            self.reply(401, b'{"error":"unauthorized"}')
            return
        if invalid_selector:
            self.reply(400, b'{"error":"invalid node selector"}')
            return
        try:
            if target.path == "/api/v1/nodes":
                self.reply(200, self.server.store.read_nodes())
            else:
                raw, age = self.server.store.read(node)
                self.reply(200, raw, age=age)
        except UnknownNodeError as exc:
            self.reply(404, b'{"error":"no nodes"}' if str(exc) == "no nodes" else b'{"error":"unknown node"}')
        except (OSError, ValueError, TypeError):
            self.reply(503, b'{"error":"snapshot unavailable"}')

    def do_HEAD(self):
        self.do_GET()

    def config_operation(self, method):
        supplied = self.headers.get("Authorization", "").encode("utf-8")
        token = self.server.setup_token
        if not token or not hmac.compare_digest(supplied, f"Bearer {token}".encode("ascii")):
            self.reply(401, b'{"error":"unauthorized"}')
            return
        body = None
        if method == "POST":
            try:
                if self.headers.get("Transfer-Encoding") or self.headers.get_content_type() != "application/json":
                    raise ValueError("Unsupported request encoding")
                lengths = self.headers.get_all("Content-Length", [])
                if len(lengths) != 1 or not lengths[0].isdigit():
                    raise ValueError("Content length required")
                length = int(lengths[0])
                if not 0 < length <= MAX_REQUEST - 128:
                    raise ValueError("Configuration body too large")
                raw = self.rfile.read(length)
                if len(raw) != length:
                    raise ValueError("Incomplete request")
                body = strict_json(raw)
                if not isinstance(body, dict):
                    raise ValueError("Object required")
            except (OSError, ValueError, TypeError):
                self.reply(400, b'{"error":"invalid configuration request"}')
                return
        try:
            status, response = self.server.config_forwarder(method, body)
            self.reply(status, encode_snapshot(response))
        except ConfigError as exc:
            self.reply(exc.status, encode_snapshot({"error": exc.message}))
        except (OSError, ValueError, TypeError):
            self.reply(503, b'{"error":"configuration service unavailable"}')

    def do_POST(self):
        target = urlsplit(self.path)
        if target.path != "/api/v1/config" or target.query:
            self.reply(404, b'{"error":"not found"}')
            return
        self.config_operation("POST")


def load_token(path: str | None = None) -> str:
    token = Path(path).read_text().strip() if path else os.environ.get("HOMELAB_DISPLAY_TOKEN", "").strip()
    if len(token) < 32 or any(c.isspace() for c in token) or not token.isascii():
        raise ValueError("Display token must be >=32 ASCII characters with no whitespace")
    return token


def load_setup_token(path: str | None = None) -> str | None:
    token = Path(path).read_text().strip() if path else os.environ.get("HOMELAB_SETUP_TOKEN", "").strip()
    if not token:
        return None  # Legacy read-only installs continue to work.
    if len(token) < 32 or len(token) > 256 or not token.isascii() or any(c.isspace() for c in token):
        raise ValueError("Setup token must be >=32 ASCII characters without whitespace")
    return token


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", default="/run/homelab-monitor/snapshot.json")
    parser.add_argument("--bind", default="192.0.2.10", help="Trusted LAN address; use 127.0.0.1 for a local preview")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--token-file", help="Alternative to HOMELAB_DISPLAY_TOKEN environment variable")
    parser.add_argument("--setup-token-file", help="Optional separate credential for node configuration")
    parser.add_argument("--demo", action="store_true", help="Explicitly serve labeled synthetic fixture, loopback only")
    parser.add_argument("--cert", help="Optional PEM certificate for HTTPS")
    parser.add_argument("--key", help="Optional PEM key for HTTPS")
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    if args.demo and args.bind not in ("127.0.0.1", "::1", "localhost"):
        parser.error("--demo requires --bind 127.0.0.1 (or ::1/localhost)")
    if bool(args.cert) != bool(args.key):
        parser.error("--cert and --key must be supplied together")
    try:
        token = load_token(args.token_file)
        path = Path(__file__).with_name("demo.json") if args.demo else Path(args.snapshot)
        store = SnapshotStore(path, demo=args.demo)
        server = SnapshotHTTPServer((args.bind, args.port), token, store,
                                    setup_token=None if args.demo else load_setup_token(args.setup_token_file))
        if args.cert:
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.minimum_version = ssl.TLSVersion.TLSv1_2
            context.load_cert_chain(args.cert, args.key)
            server.socket = context.wrap_socket(server.socket, server_side=True)
        LOG.info("Serving %s telemetry on %s:%s", "demo" if args.demo else "live", args.bind, args.port)
        try:
            server.serve_forever(poll_interval=0.25)
        finally:
            server.server_close()
    except (OSError, ValueError) as exc:
        parser.exit(1, f"HTTP reader failed: {exc}\n")
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
