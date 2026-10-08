"""Read-only remote feed scope, failure isolation, freshness and no local work."""
import copy
import io
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

from agent.collector import Collector, Config, MAX_AGGREGATE, MAX_PAYLOAD, encode_snapshot
from agent.remote_feeds import RemoteCollectorReader, remote_snapshot, validate_remote_collectors


def sample(now=None):
    return {"schema": 1, "sequence": 4, "generated_at": now if now is not None else time.time(),
            "host": {"name": "remote-native", "ip": "192.0.2.14", "cpu_pct": 23, "mem_used_bytes": 1024},
            "power": {"package_w": 7}, "guests": [{"id": 100, "name": "VM", "status": "running"}],
            "storage": [], "disks": [], "sensors": [], "gpus": [], "alerts": [],
            "sources": {"proc": {"enabled": True, "ok": True, "updated_at": now or time.time(), "error": None}},
            "node": {"id": "proxmox:remote-native", "type": "proxmox"},
            "nodes": [{"id": "klipper:private-other-printer", "address": "private-other-printer"}]}


REMOTE = {"id": "office", "name": "Office Lab", "url": "http://remote:8765"}


class RemoteReaderTests(unittest.TestCase):
    def test_one_fixed_get_and_private_bearer_without_upstream_registry(self):
        calls = []
        with tempfile.TemporaryDirectory() as folder:
            token = Path(folder) / "token"
            token.write_text("r" * 43)
            def opener(request, **kwargs):
                calls.append((request, kwargs))
                return io.BytesIO(encode_snapshot(sample()))
            result = RemoteCollectorReader(dict(REMOTE, token_file=str(token)), opener).read()
            request, options = calls[0]
            self.assertEqual(request.method, "GET")
            self.assertIsNone(request.data)
            self.assertEqual(request.full_url, "http://remote:8765/api/v1/snapshot")
            self.assertEqual(request.get_header("Authorization"), "Bearer " + "r" * 43)
            self.assertEqual(options["timeout"], 2.5)
            self.assertNotIn("r" * 43, json.dumps(result))
            self.assertNotIn("private-other-printer", json.dumps(result))
            self.assertEqual(result["snapshot"]["host"]["cpu_pct"], 23)

    def test_stale_nonfinite_oversized_demo_or_other_node_data_is_rejected(self):
        mutations = [lambda item: item.update(generated_at=time.time() - 60),
                     lambda item: item.update(generated_at=time.time() + 120),
                     lambda item: item.update(schema=2), lambda item: item.update(demo=True),
                     lambda item: item.update(printer={}), lambda item: item.update(host=None),
                     lambda item: item.update(sources=[]), lambda item: item.update(guests=["invalid"]),
                     lambda item: item.update(node={"type": "klipper"}),
                     lambda item: item.update(node=None, sequence=False)]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                document = sample()
                mutate(document)
                with self.assertRaises(ValueError):
                    RemoteCollectorReader(REMOTE, lambda *a, **kw: io.BytesIO(encode_snapshot(document))).read()
        for raw in (b"x" * (MAX_PAYLOAD + 1), b'{"schema":1,"generated_at":NaN}'):
            with self.assertRaises(ValueError):
                RemoteCollectorReader(REMOTE, lambda *a, **kw: io.BytesIO(raw)).read()

    def test_remote_url_cannot_be_a_control_or_credential_proxy(self):
        self.assertEqual(validate_remote_collectors([dict(REMOTE, url="https://remote:8765/api/v1/snapshot")])[0]["url"], "https://remote:8765")
        for url in ("http://remote/api/v1/snapshot?node=klipper:printer", "http://remote/printer/print/cancel",
                    "http://user:pass@remote", "file:///etc/shadow", "http://remote/#private"):
            with self.assertRaises(ValueError):
                validate_remote_collectors([dict(REMOTE, url=url)])

    def test_failed_or_expired_upstream_clears_metrics_and_preserves_cache(self):
        config = validate_remote_collectors([REMOTE])[0]
        data = {"snapshot": sample(1000), "generated_at": 1000}
        before = copy.deepcopy(data)
        healthy = {"enabled": True, "ok": True, "updated_at": 1000, "error": None}
        own = remote_snapshot(config, healthy, data, 20, 1005)
        self.assertEqual(own["host"]["cpu_pct"], 23)
        self.assertEqual(own["feed"]["age_s"], 5)
        self.assertEqual(own["generated_at"], 1005)
        self.assertEqual(own["sequence"], 20)
        self.assertEqual(own["sources"]["proc"]["age_s"], 5)
        for state, now in (({**healthy, "ok": False, "error": "connection failed"}, 1005), (healthy, 1020)):
            failed = remote_snapshot(config, state, data, 20, now)
            self.assertIsNone(failed["host"]["cpu_pct"])
            self.assertIsNone(failed["host"]["mem_used_bytes"])
            self.assertEqual(failed["guests"], [])
            self.assertFalse(failed["sources"]["remote_feed"]["ok"])
            self.assertEqual(set(failed["sources"]), {"remote_feed"})
        self.assertEqual(data, before)

    def test_slow_polling_uses_local_publication_and_preserves_upstream_age(self):
        config = validate_remote_collectors([dict(REMOTE, poll_interval_s=30, ttl_s=90)])[0]
        data = {"snapshot": sample(1000), "generated_at": 1000}
        state = {"enabled": True, "ok": True, "updated_at": 1000, "error": None}
        own = remote_snapshot(config, state, data, 24, 1025)
        self.assertEqual(own["generated_at"], 1025)
        self.assertEqual(own["sequence"], 24)
        self.assertEqual(own["feed"]["age_s"], 25)
        self.assertEqual(own["sources"]["proc"]["age_s"], 25)
        self.assertEqual(own["host"]["cpu_pct"], 23)
        self.assertIsNone(remote_snapshot(config, state, data, 25, 1091)["host"]["cpu_pct"])

    def test_fresh_http_timestamp_cannot_hide_a_frozen_upstream_sequence(self):
        now = [1000]
        reader = RemoteCollectorReader(REMOTE, lambda *a, **kw: io.BytesIO(encode_snapshot(sample(now[0]))))
        with patch("agent.remote_feeds.time.time", side_effect=lambda: now[0]):
            reader.read()
            now[0] = 1006
            reader.read()
            now[0] = 1016
            with self.assertRaisesRegex(ValueError, "sequence"):
                reader.read()


class RemoteCollectorTests(unittest.TestCase):
    def test_disabled_local_node_never_samples_proc_or_launches_local_probes(self):
        class NoProc:
            def read(self, now):
                raise AssertionError("Disabled local node must not read proc")
        collector = Collector(Config(enable_proxmox=False), NoProc())
        try:
            aggregate = collector.sample_aggregate()
            self.assertEqual(aggregate["nodes"], [])
            self.assertEqual(aggregate["sequence"], 1)
            self.assertTrue(all(source.future is None for source in collector.sources.values()))
            self.assertEqual(collector.sample_aggregate()["sequence"], 2)
        finally:
            collector.close()

    def test_four_printers_are_valid_only_when_local_is_disabled(self):
        printers = [{"id": f"p{i}", "name": f"Printer {i}", "url": f"http://printer{i}"} for i in range(4)]
        self.assertEqual(len(Config.from_dict({"enable_proxmox": False, "printers": printers}).printers), 4)
        with self.assertRaises(ValueError):
            Config.from_dict({"printers": printers})

    def test_remote_failure_does_not_affect_other_nodes_or_launch_local_probes(self):
        collector = Collector(Config(enable_proxmox=False, remote_collectors=[REMOTE,
                                        dict(REMOTE, id="backup", name="Backup", url="http://backup:8765")]))
        try:
            now, mono = time.time(), time.monotonic()
            for source in collector.remote_sources.values():
                source.next_run = float("inf")
            good = collector.remote_sources["office"]
            good.ok, good.error, good.last_probe_succeeded = True, None, True
            good.updated_at, good.updated_mono = now, mono
            good.data = {"snapshot": sample(now), "generated_at": now}
            bad = collector.remote_sources["backup"]
            bad.ok, bad.error = False, "connection failed"
            aggregate = collector.sample_aggregate()
            office, backup = aggregate["nodes"]
            self.assertEqual(office["id"], "remote:office")
            self.assertEqual(office["type"], "proxmox")
            self.assertEqual(office["status"], "healthy")
            self.assertEqual(backup["status"], "offline")
            self.assertEqual(office["snapshot"]["alerts"], [])
            self.assertFalse(any(source.future for source in collector.sources.values()))
            self.assertLess(len(encode_snapshot(aggregate)), MAX_AGGREGATE)
            self.assertTrue(all(len(encode_snapshot(item["snapshot"])) <= MAX_PAYLOAD for item in aggregate["nodes"]))
        finally:
            collector.close()


if __name__ == "__main__":
    unittest.main()
