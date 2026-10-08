"""Configuration persistence, privilege separation and optimistic edits."""
from __future__ import annotations

import functools
import http.client
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from agent.collector import Config, encode_snapshot
from agent.config_service import (ConfigError, ConfigManager, ConfigSocketServer, RestartQueue,
                                  forward_config, strict_json)
from agent.server import SnapshotHTTPServer, SnapshotStore


class ConfigManagerTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.path = Path(self.folder.name) / "config.json"
        self.original = {"host_ip": "192.0.2.10", "node": "vm", "qga_guest_ids": [100, 102],
                         "truenas_ssh_key": "/private/nas-key", "expected_running_guests": [100, 101, 102]}
        self.path.write_text(json.dumps(self.original))
        self.applies = []
        self.manager = ConfigManager(self.path, lambda: self.applies.append(True))

    def tearDown(self):
        self.folder.cleanup()

    def upsert(self, **node):
        return self.manager.mutate({"action": "upsert", "version": self.manager.get()["version"], "node": node})

    def test_public_projection_excludes_private_paths_and_retains_original_options(self):
        public = self.manager.get()
        self.assertEqual(public["local_node"], {"id": "proxmox:vm", "name": "vm", "address": "192.0.2.10", "enabled": True})
        self.assertNotIn("/private/nas-key", json.dumps(public))
        saved = self.upsert(type="klipper", id="", name="Workshop Printer", url="http://192.0.2.12:7125", secret="printer-private-key")
        node = next(n for n in saved["config"]["nodes"] if n["type"] == "klipper")
        self.assertEqual(node["id"], "klipper:Workshop-Printer")
        self.assertTrue(node["has_secret"])
        self.assertTrue(saved["applying"])
        self.assertNotIn("printer-private-key", json.dumps(saved))
        config = json.loads(self.path.read_text())
        self.assertEqual(config["qga_guest_ids"], [100, 102])
        self.assertEqual(config["truenas_ssh_key"], "/private/nas-key")
        secret = Path(config["printers"][0]["api_key_file"])
        self.assertEqual(secret.read_text().strip(), "printer-private-key")
        self.assertEqual(secret.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.applies, [True])

    def test_omitted_or_blank_secret_retains_and_clear_never_deletes_external_file(self):
        external = Path(self.folder.name) / "external-user-key"
        external.write_text("user-owned-secret")
        original = {**self.original, "printers": [{"id": "desk", "name": "Desk", "url": "http://printer", "api_key_file": str(external)}]}
        self.path.write_text(json.dumps(original))
        for update in ({}, {"secret": ""}):
            self.upsert(type="klipper", id="klipper:desk", name="Renamed", **update)
            self.assertEqual(Config.read(str(self.path)).printers[0]["api_key_file"], str(external))
        result = self.upsert(type="klipper", id="klipper:desk", name="Renamed", clear_secret=True)
        self.assertFalse(next(n for n in result["config"]["nodes"] if n["id"] == "klipper:desk")["has_secret"])
        self.assertEqual(external.read_text(), "user-owned-secret")
        self.manager.mutate({"action": "delete", "version": self.manager.get()["version"], "id": "klipper:desk"})
        self.assertEqual(external.read_text(), "user-owned-secret")

    def test_saved_secret_is_not_forwarded_when_the_origin_changes(self):
        external = Path(self.folder.name) / "external-user-key"
        external.write_text("user-owned-secret")
        original = {**self.original, "printers": [{"id": "desk", "name": "Desk", "url": "http://printer:7125", "api_key_file": str(external)}]}
        self.path.write_text(json.dumps(original))
        self.upsert(type="klipper", id="klipper:desk", name="Desk", url="http://PRINTER:7125/")
        self.assertEqual(Config.read(str(self.path)).printers[0]["api_key_file"], str(external))
        saved = self.upsert(type="klipper", id="klipper:desk", name="Desk", url="http://other-printer:7125")
        self.assertFalse(next(node for node in saved["config"]["nodes"] if node["id"] == "klipper:desk")["has_secret"])
        self.assertEqual(Config.read(str(self.path)).printers[0]["api_key_file"], "")
        self.assertEqual(external.read_text(), "user-owned-secret")

    def test_old_version_cannot_overwrite_another_client_commit(self):
        other = ConfigManager(self.path)
        version = other.get()["version"]
        self.upsert(type="local-proxmox", id="proxmox:vm", name="Home Lab")
        with self.assertRaises(ConfigError) as caught:
            other.mutate({"action": "delete", "version": version, "id": "proxmox:vm"})
        self.assertEqual(caught.exception.status, 409)
        self.assertEqual(self.manager.get()["local_node"]["name"], "Home Lab")
        self.assertTrue(self.manager.get()["local_node"]["enabled"])

    def test_delete_restore_and_four_node_capacity_are_atomic(self):
        local = self.manager.get()["local_node"]["id"]
        for index in range(3):
            self.upsert(type="klipper", id=f"p{index}", name=f"Printer {index}", url=f"http://printer{index}")
        before = self.path.read_bytes()
        with self.assertRaises(ConfigError):
            self.upsert(type="proxmox-feed", id="remote", name="Remote", url="http://remote:8765", secret="r" * 43)
        self.assertEqual(self.path.read_bytes(), before)
        self.assertFalse((self.path.parent / "node-secrets").exists())
        self.manager.mutate({"action": "delete", "version": self.manager.get()["version"], "id": local})
        self.assertFalse(self.manager.get()["local_node"]["enabled"])
        self.upsert(type="klipper", id="p3", name="Printer 3", url="http://printer3")
        with self.assertRaises(ConfigError):
            self.upsert(type="local-proxmox", id=local, name="Home Lab")
        self.manager.mutate({"action": "delete", "version": self.manager.get()["version"], "id": "klipper:p3"})
        restored = self.upsert(type="local-proxmox", id=local, name="Home Lab")
        self.assertEqual(restored["config"]["local_node"]["id"], local)
        self.assertTrue(restored["config"]["local_node"]["enabled"])

    def test_invalid_paths_controls_and_urls_never_change_files(self):
        base = {"type": "klipper", "id": "printer", "name": "Printer", "url": "http://printer"}
        cases = [{**base, "api_key_file": "/etc/shadow"}, {**base, "id": "../../root"},
                 {**base, "url": "http://printer/printer/print/start"}, {**base, "url": "http://user:pass@printer"},
                 {**base, "command": "restart"}, {**base, "timeout_s": 50}, {**base, "secret": "x\nHeader: y"},
                 {**base, "secret": "key", "clear_secret": True}, {**base, "id": "remote:printer"},
                 {**base, "url": "file:///etc/shadow"}, {**base, "type": "ssh"}, {**base, "poll_interval_s": False}]
        before = self.path.read_bytes()
        for case in cases:
            with self.subTest(case=list(case)):
                with self.assertRaises(ConfigError) as caught:
                    self.upsert(**case)
                self.assertEqual(caught.exception.status, 400)
                self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(self.applies, [])

    def test_generated_ids_are_stable_and_collision_safe_and_remote_keys_private(self):
        first = self.upsert(type="klipper", name="Printer", url="http://first")
        second = self.upsert(type="klipper", name="Printer", url="http://second")
        self.assertEqual([n["id"] for n in second["config"]["nodes"] if n["type"] == "klipper"], ["klipper:Printer", "klipper:Printer-2"])
        remote = self.upsert(type="proxmox-feed", name="Remote", url="https://remote:8765/api/v1/snapshot", secret="r" * 43)
        node = next(n for n in remote["config"]["nodes"] if n["type"] == "proxmox-feed")
        self.assertEqual(node["id"], "remote:Remote")
        self.assertEqual(node["url"], "https://remote:8765")
        self.assertNotIn("r" * 43, json.dumps(remote))
        self.assertNotIn("token_file", json.dumps(remote))

    def test_strict_json_rejects_duplicate_fields_and_nonfinite_settings(self):
        for raw in ('{"version":"a","version":"b"}', '{"timeout_s":NaN}'):
            with self.assertRaises(ValueError):
                strict_json(raw)

    def test_managed_secret_is_removed_only_after_its_association_is_cleared(self):
        self.upsert(type="klipper", id="desk", name="Desk", url="http://printer", secret="managed-private-key")
        path = Path(Config.read(str(self.path)).printers[0]["api_key_file"])
        self.assertTrue(path.exists())
        self.upsert(type="klipper", id="klipper:desk", name="Desk", clear_secret=True)
        self.assertFalse(path.exists())

    def test_write_failure_does_not_apply_or_replace_the_existing_configuration(self):
        before = self.path.read_bytes()
        with patch("agent.config_service.atomic_private", side_effect=OSError("disk full")):
            with self.assertRaises(ConfigError) as caught:
                self.upsert(type="local-proxmox", id="proxmox:vm", name="Home Lab")
        self.assertEqual(caught.exception.status, 503)
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(self.applies, [])


class ConfigProtocolTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        root = Path(self.folder.name)
        self.path = root / "config.json"
        self.path.write_text('{"node":"vm","enable_proxmox":false}')
        self.manager = ConfigManager(self.path)
        self.socket_path = root / "config.sock"
        self.daemon = ConfigSocketServer(self.socket_path, self.manager)
        self.daemon_thread = threading.Thread(target=self.daemon.serve_forever, kwargs={"poll_interval": .01}, daemon=True)
        self.daemon_thread.start()
        snapshot = root / "snapshot.json"
        snapshot.write_bytes(encode_snapshot({"schema": 2, "generated_at": time.time(), "sequence": 1, "nodes": []}))
        self.display_token, self.setup_token = "d" * 43, "s" * 43
        self.http = SnapshotHTTPServer(("127.0.0.1", 0), self.display_token, SnapshotStore(snapshot), setup_token=self.setup_token,
                                      config_forwarder=functools.partial(forward_config, socket_path=self.socket_path))
        self.http_thread = threading.Thread(target=self.http.serve_forever, kwargs={"poll_interval": .01}, daemon=True)
        self.http_thread.start()

    def tearDown(self):
        self.http.shutdown()
        self.http.server_close()
        self.daemon.shutdown()
        self.daemon.server_close()
        self.http_thread.join(1)
        self.daemon_thread.join(1)
        self.folder.cleanup()

    def request(self, path, method="GET", body=None, token=None, raw=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.http.server_port, timeout=2)
        headers = {"Authorization": "Bearer " + (token or self.display_token)}
        if body is not None or raw is not None:
            headers["Content-Type"] = "application/json"
            raw = json.dumps(body) if raw is None else raw
        connection.request(method, path, body=raw, headers=headers)
        response = connection.getresponse()
        status, payload = response.status, json.loads(response.read())
        connection.close()
        return status, payload

    def test_display_token_cannot_manage_and_setup_token_cannot_control_or_read_feed(self):
        self.assertEqual(self.request("/api/v1/config")[0], 401)
        self.assertEqual(self.request("/api/v1/config", "POST", {"action": "delete"})[0], 401)
        self.assertEqual(self.request("/api/v1/snapshot", token=self.setup_token)[0], 401)
        self.assertEqual(self.request("/printer/print/cancel", "POST", {}, token=self.setup_token)[0], 404)
        self.assertEqual(self.request("/api/v1/config?path=/etc/shadow", token=self.setup_token)[0], 404)

    def test_empty_registry_keeps_setup_working_and_restoring_is_accepted(self):
        self.assertEqual(self.request("/api/v1/nodes")[1]["nodes"], [])
        self.assertEqual(self.request("/api/v1/snapshot"), (404, {"error": "no nodes"}))
        self.assertEqual(self.request("/healthz")[0], 503)
        status, public = self.request("/api/v1/config", token=self.setup_token)
        self.assertEqual(status, 200)
        self.assertEqual(public["nodes"], [])
        status, result = self.request("/api/v1/config", "POST", {"action": "upsert", "version": public["version"],
                                      "node": {"type": "local-proxmox", "id": public["local_node"]["id"], "name": "Home Lab"}}, self.setup_token)
        self.assertEqual(status, 202)
        self.assertTrue(result["applying"])
        self.assertTrue(result["config"]["local_node"]["enabled"])

    def test_http_reports_conflicts_and_invalid_json_without_leaking_input(self):
        _, public = self.request("/api/v1/config", token=self.setup_token)
        request = {"action": "upsert", "version": public["version"], "node": {"type": "klipper", "name": "Printer", "url": "http://printer"}}
        self.assertEqual(self.request("/api/v1/config", "POST", request, self.setup_token)[0], 202)
        self.assertEqual(self.request("/api/v1/config", "POST", request, self.setup_token)[0], 409)
        for raw in ('{"private-key":', '{"action":"upsert","action":"delete"}', '[]'):
            status, body = self.request("/api/v1/config", "POST", token=self.setup_token, raw=raw)
            self.assertEqual(status, 400)
            self.assertNotIn("private-key", json.dumps(body))


class ApplyQueueTests(unittest.TestCase):
    def test_saved_configuration_queues_only_the_fixed_collector_unit(self):
        calls, invoked = [], threading.Event()
        def runner(argv, **kwargs):
            calls.append((argv, kwargs))
            invoked.set()
            return type("Result", (), {"returncode": 0})()
        queue = RestartQueue(runner)
        try:
            queue.submit()
            self.assertTrue(invoked.wait(2))
            self.assertEqual(calls[0][0], ["/usr/bin/systemctl", "restart", "homelab-monitor-collector.service"])
            self.assertGreater(calls[0][1]["timeout"], 0)
        finally:
            queue.close()


if __name__ == "__main__":
    unittest.main()
