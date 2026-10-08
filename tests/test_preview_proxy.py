import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch

from tools.preview import Handler


class PreviewProxyTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        display = Path(self.folder.name) / "display.token"
        setup = Path(self.folder.name) / "setup.token"
        display.write_text("read-only-test-credential-00000000")
        setup.write_text("configuration-test-credential-00000")
        self.calls = []
        calls = self.calls

        class Upstream(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def reply(self, code, body):
                data = json.dumps(body).encode()
                self.send_response(code)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self):
                calls.append((self.command, self.path, self.headers.get("Authorization")))
                self.reply(200, {"schema": 1, "version": "a" * 64, "nodes": []})

            def do_POST(self):
                calls.append((self.command, self.path, self.headers.get("Authorization")))
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                if body.get("version") == "outdated":
                    self.reply(409, {"error": "Configuration changed; reload before saving"})
                else:
                    self.reply(202, {"config": {"version": "b" * 64}, "applying": True})

        self.upstream = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
        self.proxy = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.environ = patch.dict(os.environ, {
            "HOMELAB_PREVIEW_TOKEN_FILE": str(display),
            "HOMELAB_PREVIEW_SETUP_TOKEN_FILE": str(setup),
            "HOMELAB_PREVIEW_UPSTREAM": f"http://127.0.0.1:{self.upstream.server_port}/api/v1/snapshot",
        })
        self.environ.start()
        for server in (self.upstream, self.proxy):
            threading.Thread(target=server.serve_forever, kwargs={"poll_interval": .01}, daemon=True).start()

    def tearDown(self):
        for server in (self.proxy, self.upstream):
            server.shutdown()
            server.server_close()
        self.environ.stop()
        self.folder.cleanup()

    def request(self, method, path, body=None, headers=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.proxy.server_port, timeout=2)
        connection.request(method, path, body=body, headers=headers or {})
        response = connection.getresponse()
        status, payload = response.status, json.loads(response.read())
        connection.close()
        return status, payload

    def headers(self):
        return {"Origin": f"http://127.0.0.1:{self.proxy.server_port}",
                "Content-Type": "application/json", "X-Homelab-Config": "1"}

    def test_config_uses_separate_credential_and_fixed_origin(self):
        self.assertEqual(self.request("GET", "/api/v1/config")[0], 200)
        self.assertEqual(self.calls[-1], ("GET", "/api/v1/config", "Bearer configuration-test-credential-00000"))
        self.assertEqual(self.request("GET", "/api/v1/snapshot?node=klipper%3Adesk")[0], 200)
        self.assertEqual(self.calls[-1], ("GET", "/api/v1/snapshot?node=klipper%3Adesk", "Bearer read-only-test-credential-00000000"))
        count = len(self.calls)
        self.assertEqual(self.request("GET", "/api/v1/config?url=http://elsewhere/")[0], 400)
        self.assertEqual(len(self.calls), count)

    def test_post_rejects_cross_origin_missing_guard_and_oversized_requests(self):
        body = json.dumps({"action": "delete", "id": "klipper:desk", "version": "a" * 64})
        for headers in ({}, {**self.headers(), "Origin": "https://elsewhere.invalid"},
                        {**self.headers(), "X-Homelab-Config": "0"}):
            self.assertEqual(self.request("POST", "/api/v1/config", body, headers)[0], 403)
        self.assertEqual(self.request("POST", "/api/v1/config", "x" * 4097, self.headers())[0], 400)
        self.assertEqual(self.request("POST", "/api/v1/snapshot", body, self.headers())[0], 404)
        self.assertEqual(self.calls, [])

    def test_post_preserves_acceptance_and_conflict_without_echoing_keys(self):
        status, payload = self.request("POST", "/api/v1/config", json.dumps({"action": "upsert"}), self.headers())
        self.assertEqual(status, 202)
        self.assertTrue(payload["applying"])
        self.assertNotIn("credential", json.dumps(payload))
        status, payload = self.request("POST", "/api/v1/config", json.dumps({"version": "outdated"}), self.headers())
        self.assertEqual(status, 409)
        self.assertIn("reload", payload["error"])
        self.assertEqual(self.calls[-1][2], "Bearer configuration-test-credential-00000")


if __name__ == "__main__":
    unittest.main()
