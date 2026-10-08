#!/usr/bin/env python3
"""Loopback design preview. Keeps the display credential out of the browser.

HOMELAB_PREVIEW_TOKEN_FILE: root-readable local file containing only the token.
HOMELAB_PREVIEW_SETUP_TOKEN_FILE: optional separate configuration token file.
HOMELAB_PREVIEW_UPSTREAM: default http://192.0.2.10:8765/api/v1/snapshot.
"""
import argparse
import json
import os
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from urllib.parse import parse_qs, urlencode, urlsplit, urlunsplit

ROOT = Path(__file__).resolve().parents[1]

def native_gallery():
    for name in ("native-management-combined", "native-multi-node-combined", "native-multi-node-live", "native-sensor-power-live", "native-guest-gpu-live", "native-live"):
        current = ROOT / "output" / name
        if (current / "index.html").is_file():
            return current
    return ROOT / "output/native-live"

class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT / "preview"), **kwargs)

    def do_GET(self):
        requested = self.path.split("?", 1)[0]
        if requested.startswith("/native/"):
            name = requested.removeprefix("/native/")
            if not name or "/" in name or "\\" in name or name in (".", "..") or Path(name).suffix not in (".html", ".png", ".json"):
                self.send_error(404)
                return
            try:
                payload = (native_gallery() / name).read_bytes()
            except OSError:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", self.guess_type(name))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        if requested not in ("/api/v1/snapshot", "/api/v1/nodes", "/api/v1/config"):
            return super().do_GET()
        self.proxy_api(requested)

    def json_reply(self, code, payload):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def api_error(self, code, message):
        self.json_reply(code, json.dumps({"error": message}).encode())

    def end_headers(self):
        self.send_header("X-Frame-Options", "DENY")
        super().end_headers()

    def do_POST(self):
        if self.path != "/api/v1/config":
            self.api_error(404, "Unknown configuration endpoint")
            return
        allowed_origins = {f"http://127.0.0.1:{self.server.server_port}", f"http://localhost:{self.server.server_port}"}
        if self.headers.get("Origin") not in allowed_origins or self.headers.get("X-Homelab-Config") != "1":
            self.api_error(403, "Open configuration in this preview to make changes")
            return
        if self.headers.get("Content-Type", "").split(";", 1)[0].strip().lower() != "application/json":
            self.api_error(415, "Configuration requires JSON")
            return
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 < size <= 4096:
                raise ValueError
            self.connection.settimeout(5)
            payload = self.rfile.read(size)
            if len(payload) != size or not isinstance(json.loads(payload), dict):
                raise ValueError
        except (ValueError, OSError):
            self.api_error(400, "Invalid or oversized configuration request")
            return
        self.proxy_api("/api/v1/config", payload)

    def proxy_api(self, requested, request_body=None):
        is_config = requested == "/api/v1/config"
        token_path = os.environ.get("HOMELAB_PREVIEW_SETUP_TOKEN_FILE" if is_config else "HOMELAB_PREVIEW_TOKEN_FILE")
        if not token_path:
            self.api_error(503, "Preview setup token file is not configured" if is_config else "Live feed token file not configured")
            return
        try:
            token = Path(token_path).read_text().strip()
            upstream = urlsplit(os.environ.get("HOMELAB_PREVIEW_UPSTREAM", "http://192.0.2.10:8765/api/v1/snapshot"))
            query = parse_qs(urlsplit(self.path).query, keep_blank_values=True)
            if (requested != "/api/v1/snapshot" and query) or set(query) - {"node"} or any(len(v) != 1 or len(v[0]) > 63 for v in query.values()):
                self.api_error(400, "Invalid API selection")
                return
            path = upstream.path if requested == "/api/v1/snapshot" else requested
            url = urlunsplit(upstream._replace(path=path, query=urlencode({k:v[0] for k,v in query.items()}), fragment=""))
            headers = {"Authorization": "Bearer " + token, "Accept": "application/json"}
            if request_body is not None:
                headers["Content-Type"] = "application/json"
            request = Request(url, headers=headers, data=request_body, method="POST" if request_body is not None else "GET")
            with urlopen(request, timeout=4) as response:
                code = response.status
                payload = response.read(49153)
            if len(payload) > 49152:
                raise ValueError("Snapshot exceeds firmware limit")
        except HTTPError as error:
            allowed = (400, 401, 403, 404, 409, 413, 503) if is_config else (400, 404)
            code = error.code if error.code in allowed else 502
            if is_config:
                try:
                    message = json.loads(error.read(4097)).get("error")
                    if not isinstance(message, str) or len(message) > 256:
                        raise ValueError
                except (ValueError, AttributeError):
                    message = "Configuration service unavailable"
            else:
                message = "No nodes configured or node unavailable" if error.code == 404 else "Host feed unavailable"
            error.close()
            self.api_error(code, message)
            return
        except (OSError, URLError, ValueError):
            self.api_error(502, "Configuration service unavailable" if is_config else "Host feed unavailable")
            return
        self.json_reply(code, payload)

    def log_message(self, format, *args):
        pass

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=8766)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"Preview: http://127.0.0.1:{args.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
