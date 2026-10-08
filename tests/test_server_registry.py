import copy
import http.client
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest

from agent.collector import MAX_PAYLOAD, encode_snapshot
from agent.server import MAX_REGISTRY_PAYLOAD, SnapshotHTTPServer, SnapshotStore, UnknownNodeError


def snapshot(name, age=0, sequence=1):
    now = time.time() - age
    return {"schema": 1, "generated_at": now, "sequence": sequence,
            "host": {"name": name, "ip": "192.0.2.10", "cpu_pct": 12},
            "guests": [], "storage": [], "disks": [], "sensors": [], "gpus": [], "alerts": [],
            "sources": {"proc": {"enabled": True, "ok": True, "updated_at": now, "error": None}}}


def node(identity, document=None):
    kind, name = identity.split(":", 1)
    return {"id": identity, "type": kind, "name": name, "address": "192.0.2.10",
            "snapshot": document if document is not None else snapshot(name)}


def registry(*nodes):
    return {"schema": 2, "generated_at": time.time(), "sequence": 90, "nodes": list(nodes)}


class RegistryStoreTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.path = Path(self.folder.name) / "snapshot.json"
        self.store = SnapshotStore(self.path)

    def tearDown(self):
        self.folder.cleanup()

    def write(self, document):
        self.path.write_bytes(encode_snapshot(document))

    def test_default_prefers_local_pve_and_selected_payload_has_no_other_snapshots(self):
        remote = snapshot("printer", sequence=12)
        remote["private_remote_marker"] = "never duplicate another node payload"
        self.write(registry(node("printer:desk", remote), node("proxmox:custom", snapshot("custom", sequence=34)), node("proxmox:vm")))
        body, age = self.store.read()
        selected = json.loads(body)
        self.assertEqual(selected["schema"], 1)
        self.assertEqual(selected["host"]["name"], "custom")
        self.assertEqual(selected["sequence"], 34)
        self.assertEqual(selected["node"]["id"], "proxmox:custom")
        self.assertEqual(len(selected["nodes"]), 3)
        self.assertNotIn(b"private_remote_marker", body)
        self.assertTrue(all("snapshot" not in descriptor for descriptor in selected["nodes"]))
        self.assertLess(age, 1)
        body, _ = self.store.read("printer:desk")
        self.assertEqual(json.loads(body)["host"]["name"], "printer")
        self.assertEqual(json.loads(body)["sequence"], 12)

    def test_default_uses_configured_pve_identity_without_lab_hardcode(self):
        self.write(registry(node("printer:desk"), node("proxmox:custom-host")))
        self.assertEqual(json.loads(self.store.read()[0])["node"]["id"], "proxmox:custom-host")

    def test_metadata_status_is_per_node_and_ignores_disabled_sources(self):
        healthy = snapshot("vm")
        healthy["sources"]["optional"] = {"enabled": False, "ok": False, "error": "disabled"}
        degraded = snapshot("printer")
        degraded["alerts"] = [{"id": "printer/shutdown", "severity": "critical"}]
        offline = snapshot("stopped")
        offline["sources"]["proc"]["ok"] = False
        unknown = snapshot("new")
        unknown["sources"]["proc"].update(ok=False, updated_at=None, error="initializing")
        self.write(registry(node("proxmox:vm", healthy), node("printer:desk", degraded), node("printer:stopped", offline), node("printer:new", unknown)))
        metadata = json.loads(self.store.read_nodes())
        self.assertEqual(metadata["schema"], 2)
        self.assertEqual([n["status"] for n in metadata["nodes"]], ["healthy", "degraded", "offline", "unknown"])
        self.assertTrue(all(set(n) == {"id", "type", "name", "address", "status"} for n in metadata["nodes"]))
        self.assertNotIn(b"cpu_pct", self.store.read_nodes())

    def test_stale_and_invalid_other_timestamps_do_not_fail_local_read(self):
        invalid = snapshot("invalid")
        invalid["generated_at"] = None
        self.write(registry(node("proxmox:vm"), node("printer:stale", snapshot("stale", age=90)), node("printer:invalid", invalid)))
        body, age = self.store.read()
        self.assertLess(age, 1)
        self.assertEqual([n["status"] for n in json.loads(body)["nodes"]], ["healthy", "offline", "unknown"])
        with self.assertRaises(ValueError):
            self.store.read("printer:invalid")
        _, age = self.store.read("printer:stale")
        self.assertGreater(age, 80)

    def test_unknown_or_malformed_selector_is_distinct(self):
        self.write(registry(node("proxmox:vm")))
        with self.assertRaises(UnknownNodeError):
            self.store.read("printer:absent")
        for selector in ("", "../shadow", "printer:x?token=x", "printer:x/y"):
            with self.assertRaises(ValueError):
                self.store.read(selector)

    def test_registry_ids_fit_the_display_selector_without_truncation(self):
        maximum = "proxmox:" + "x" * 55
        self.write(registry(node(maximum)))
        self.assertEqual(json.loads(self.store.read(maximum)[0])["node"]["id"], maximum)
        with self.assertRaises(ValueError):
            self.store.read(maximum + "x")
        self.write(registry(node(maximum + "x")))
        with self.assertRaises(ValueError):
            self.store.read()
        self.write(snapshot("a" * 90))
        body, _ = self.store.read()
        self.assertLessEqual(len(json.loads(body)["node"]["id"]), 63)

    def test_invalid_selected_host_is_rejected_without_breaking_another_node(self):
        invalid = snapshot("invalid")
        invalid["host"] = "invalid host object"
        self.write(registry(node("proxmox:vm"), node("printer:invalid", invalid)))
        self.assertEqual(json.loads(self.store.read()[0])["node"]["status"], "healthy")
        self.assertEqual(json.loads(self.store.read_nodes())["nodes"][1]["status"], "unknown")
        with self.assertRaises(ValueError):
            self.store.read("printer:invalid")
        self.write(invalid)
        with self.assertRaises(ValueError):
            self.store.read()

    def test_legacy_snapshot_gains_fallback_descriptor_and_demo_rebases(self):
        original = snapshot("custom-host", age=120)
        original["guests"] = [{"id": 100, "mem_updated_at": 1, "mem_age_s": 120}]
        original["gpus"] = [{"id": "pci:0", "updated_at": 1, "age_s": 120}]
        self.write(original)
        body, age = self.store.read()
        selected = json.loads(body)
        self.assertEqual(selected["node"]["id"], "proxmox:custom-host")
        self.assertEqual(selected["host"], original["host"])
        self.assertGreater(age, 100)
        demo, age = SnapshotStore(self.path, demo=True).read()
        rebased = json.loads(demo)
        self.assertTrue(rebased["demo"])
        self.assertLess(age, 1)
        self.assertEqual(rebased["sources"]["proc"]["age_s"], 0)
        self.assertEqual(rebased["guests"][0]["mem_age_s"], 0)
        self.assertEqual(rebased["gpus"][0]["age_s"], 0)
        self.assertEqual(json.loads(self.path.read_bytes()), original)

    def test_registry_bounds_and_duplicate_ids_fail_closed(self):
        duplicate = node("proxmox:vm")
        cases = [registry(*(node(f"printer:p{i}") for i in range(5))),
                 registry(duplicate, copy.deepcopy(duplicate)), registry(node("proxmox:vm"))]
        cases[-1]["padding"] = "x" * MAX_REGISTRY_PAYLOAD
        for document in cases:
            with self.subTest(count=len(document["nodes"])):
                self.write(document)
                with self.assertRaises(ValueError):
                    self.store.read()

    def test_aggregate_above_display_bound_keeps_each_selected_wire_bounded(self):
        large = snapshot("vm")
        large["sensors"] = [{"id": "temperature", "name": ""}]
        large["sensors"][0]["name"] = "x" * (MAX_PAYLOAD - 100 - len(encode_snapshot(large)))
        self.assertLessEqual(len(encode_snapshot(large)), MAX_PAYLOAD)
        nodes = [node("proxmox:vm", large)] + [node(f"printer:p{i}", copy.deepcopy(large)) for i in range(3)]
        for record in nodes:
            record["name"], record["address"] = "n" * 96, "a" * 255
        self.write(registry(*nodes))
        self.assertGreater(self.path.stat().st_size, MAX_PAYLOAD)
        self.assertLess(self.path.stat().st_size, MAX_REGISTRY_PAYLOAD)
        for record in nodes:
            body, _ = self.store.read(record["id"])
            selected = json.loads(body)
            self.assertLessEqual(len(body), MAX_PAYLOAD)
            self.assertEqual(selected["node"]["id"], record["id"])
            self.assertEqual(selected["limits"]["truncated"]["sensors"], 1)
            self.assertTrue(any(alert["id"] == "display/truncated" for alert in selected["alerts"]))


class RegistryHTTPTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.path = Path(self.folder.name) / "snapshot.json"
        self.document = registry(node("printer:desk", snapshot("desk", age=90)), node("proxmox:vm"))
        self.path.write_bytes(encode_snapshot(self.document))
        self.token = "a" * 43
        self.server = SnapshotHTTPServer(("127.0.0.1", 0), self.token, SnapshotStore(self.path))
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": .01}, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=1)
        self.folder.cleanup()

    def get(self, target, authorized=True, method="GET"):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=2)
        headers = {"Authorization": f"Bearer {self.token}"} if authorized else {}
        connection.request(method, target, headers=headers)
        response = connection.getresponse()
        code, body, headers = response.status, response.read(), dict(response.getheaders())
        connection.close()
        return code, body, headers

    def test_bearer_protects_metadata_and_every_selected_snapshot(self):
        for target in ("/api/v1/nodes", "/api/v1/snapshot", "/api/v1/snapshot?node=printer%3Adesk"):
            self.assertEqual(self.get(target, authorized=False)[0], 401)
            self.assertEqual(self.get(target)[0], 200)
        code, body, headers = self.get("/api/v1/snapshot?node=printer%3Adesk")
        self.assertEqual(json.loads(body)["host"]["name"], "desk")
        self.assertGreater(float(headers["X-Snapshot-Age"]), 80)

    def test_unknown_node_404_and_invalid_node_selectors_400(self):
        self.assertEqual(self.get("/api/v1/snapshot?node=printer:absent")[0], 404)
        for query in ("node=", "node=printer:x/y", "node=proxmox:vm&node=printer:desk", "node", "node=%FF"):
            self.assertEqual(self.get("/api/v1/snapshot?" + query)[0], 400)
        self.assertEqual(self.get("/api/v1/snapshot?token=anything")[0], 404)
        self.assertEqual(self.get("/api/v1/nodes?node=proxmox:vm")[0], 404)

    def test_health_uses_default_pve_and_not_remote_failures_or_registry_age(self):
        self.document["generated_at"] = time.time() - 120
        self.document["nodes"][0]["snapshot"]["sources"]["proc"]["ok"] = False
        self.path.write_bytes(encode_snapshot(self.document))
        self.assertEqual(self.get("/healthz", authorized=False)[0], 200)
        self.document["nodes"][1]["snapshot"]["generated_at"] = time.time() - 90
        self.path.write_bytes(encode_snapshot(self.document))
        self.assertEqual(self.get("/healthz", authorized=False)[0], 503)
        self.assertEqual(self.get("/api/v1/snapshot")[0], 200)

    def test_head_metadata_matches_content_length_and_incomplete_file_is_unavailable(self):
        code, body, headers = self.get("/api/v1/nodes")
        self.assertEqual(code, 200)
        self.assertEqual(int(headers["Content-Length"]), len(body))
        head_code, head_body, head_headers = self.get("/api/v1/nodes", method="HEAD")
        self.assertEqual(head_code, 200)
        self.assertEqual(head_body, b"")
        self.assertEqual(int(head_headers["Content-Length"]), len(body))
        self.path.write_bytes(b'{"schema":2')
        self.assertEqual(self.get("/api/v1/nodes")[0], 503)
        self.assertEqual(self.get("/healthz", authorized=False)[0], 503)


if __name__ == "__main__":
    unittest.main()
