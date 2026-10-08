"""Behavioral printer tests use authored synthetic status and a fixed HTTP boundary."""
import copy
import io
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch
from urllib.error import URLError
from urllib.parse import parse_qs, urlsplit

from agent.collector import Collector, Config, MAX_AGGREGATE, MAX_PAYLOAD, encode_snapshot, proxmox_node_id
from agent.printers import MoonrakerReader, normalize_printer, printer_snapshot, validate_printers

ROOT = Path(__file__).resolve().parents[1]
PRINTER = {"id": "workshop", "name": "Workshop", "url": "http://192.0.2.12:7125"}


def probe():
    return json.loads((ROOT / "tests/fixtures/moonraker-status.json").read_text())


class FakeMoonraker:
    def __init__(self):
        self.data = probe()
        self.calls = []
        self.failure, self.extra_bytes = False, False
        self.key = None

    def open(self, request, **kwargs):
        self.calls.append((request, kwargs))
        if self.failure:
            raise URLError("printer offline")
        if self.key is not None:
            if request.get_header("X-api-key") != self.key:
                raise AssertionError("API credential not attached to the fixed request")
        self.assert_read_only(request)
        path = urlsplit(request.full_url).path
        values = {"/printer/objects/list": {"objects": self.data["objects"]},
                  "/printer/info": {"hostname": "example-printer"},
                  "/printer/objects/query": {"eventtime": 100, "status": self.data["status"]},
                  "/server/files/metadata": {"estimated_time": 9660},
                  "/server/info": {"klippy_state": "ready"}}
        raw = json.dumps({"result": values[path]}).encode()
        if self.extra_bytes:
            raw += b" " * (64 * 1024)
        return io.BytesIO(raw)

    @staticmethod
    def assert_read_only(request):
        if request.get_method() != "GET" or request.data is not None:
            raise AssertionError("A read-only printer probe attempted a control request")


class PrinterTests(unittest.TestCase):
    def test_synthetic_print_temperatures_progress_and_labeled_estimate(self):
        raw = probe()
        raw["status"]["temperature_sensor mcu_temp"] = {"temperature": 45.5}
        raw["status"]["temperature_sensor raspberry_pi"] = {"temperature": 52.1}
        result = normalize_printer("workshop", raw["status"], {"estimated_time": 9660}, "example-printer", 1000)
        self.assertEqual(result["state"], "printing")
        self.assertEqual(result["host_name"], "example-printer")
        self.assertEqual(result["progress_pct"], 50)
        self.assertIsNone(result["current_layer"])
        self.assertEqual(result["slicer_estimated_time_s"], 9660)
        self.assertEqual(result["eta_basis"], "progress-average")
        self.assertGreater(result["eta_at"], 1000)
        nozzle = next(item for item in result["heaters"] if item["name"] == "Nozzle")
        self.assertEqual(nozzle, {"name": "Nozzle", "temp_c": 210, "target_c": 210, "duty_pct": 50})
        self.assertNotIn("power_w", nozzle)
        self.assertEqual(len(result["temperatures"]), 2)

    def test_paused_early_invalid_completed_and_shutdown_eta_states(self):
        raw = probe()["status"]
        for state in ("paused", "standby", "cancelled", "error"):
            item = copy.deepcopy(raw)
            item["print_stats"]["state"] = state
            result = normalize_printer("workshop", item, {}, "", 1000)
            self.assertIsNone(result["eta_at"])
            if state != "paused":
                self.assertIsNone(result["remaining_s"])
        for duration, progress in ((30, .3), (500, .01), (500, 1), (500, float("nan"))):
            item = copy.deepcopy(raw)
            item["print_stats"]["print_duration"] = duration
            item["display_status"]["progress"] = item["virtual_sdcard"]["progress"] = progress
            self.assertIsNone(normalize_printer("workshop", item, {}, "", 1000)["remaining_s"])
        item = copy.deepcopy(raw)
        item["print_stats"]["state"] = "complete"
        complete = normalize_printer("workshop", item, {}, "", 1000)
        self.assertEqual((complete["progress_pct"], complete["remaining_s"]), (100, 0))
        item["webhooks"]["state"] = "shutdown"
        shutdown = normalize_printer("workshop", item, {}, "", 1000)
        self.assertEqual(shutdown["state"], "unknown")
        self.assertIsNone(shutdown["remaining_s"])
        self.assertIsNone(shutdown["progress_pct"])

    def test_fixed_requests_metadata_cache_and_same_file_job_reset(self):
        remote = FakeMoonraker()
        reader = MoonrakerReader(PRINTER, remote.open)
        reader.read()
        reader.read()
        paths = [urlsplit(request.full_url).path for request, _ in remote.calls]
        self.assertEqual(paths.count("/server/files/metadata"), 1)
        self.assertEqual(paths.count("/printer/objects/list"), 1)
        query = next(request for request, _ in remote.calls if urlsplit(request.full_url).path == "/printer/objects/query")
        self.assertNotIn("configfile", parse_qs(urlsplit(query.full_url).query))
        self.assertIn("temperature_sensor mcu_temp", parse_qs(urlsplit(query.full_url).query))
        remote.data["status"]["print_stats"]["print_duration"] = 1
        fresh = reader.read()["printer"]
        self.assertIsNone(fresh["remaining_s"])
        self.assertEqual(sum(urlsplit(r.full_url).path == "/server/files/metadata" for r, _ in remote.calls), 2)

    def test_missing_optional_objects_are_not_queried_or_fabricated(self):
        remote = FakeMoonraker()
        remote.data["objects"] = ["webhooks", "print_stats", "extruder"]
        remote.data["status"] = {key: value for key, value in remote.data["status"].items() if key in remote.data["objects"]}
        result = MoonrakerReader(PRINTER, remote.open).read()["printer"]
        query = next(r for r, _ in remote.calls if urlsplit(r.full_url).path == "/printer/objects/query")
        self.assertEqual(set(parse_qs(urlsplit(query.full_url).query)), {"webhooks", "print_stats", "extruder"})
        self.assertIsNone(result["progress_pct"])
        self.assertIsNone(result["fan_pct"])
        self.assertIsNone(result["filament_detected"])
        self.assertEqual(result["temperatures"], [])

    def test_failed_or_expired_node_clears_metrics_without_mutating_retained_data(self):
        config = validate_printers([PRINTER])[0]
        printer = normalize_printer("workshop", probe()["status"], {}, "example-printer", 1000)
        data = {"printer": printer}
        before = copy.deepcopy(data)
        for state, now in (({"ok": False, "enabled": True, "updated_at": 1000, "age_s": 3, "error": "offline"}, 1003),
                           ({"ok": True, "enabled": True, "updated_at": 1020, "age_s": 0, "error": None}, 1020)):
            own = printer_snapshot(config, state, data, 4, now)
            self.assertIsNone(own["printer"]["progress_pct"])
            self.assertIsNone(own["printer"]["remaining_s"])
            self.assertEqual(own["printer"]["heaters"], [])
            self.assertFalse(own["sources"]["moonraker"]["ok"])
            self.assertIsNone(own["host"]["cpu_pct"])
            self.assertEqual(own["guests"], [])
            self.assertEqual(set(own["sources"]), {"moonraker"})
        self.assertEqual(data, before)

    def test_starting_and_disconnected_printers_have_degraded_readiness(self):
        from agent.server import node_status
        config = validate_printers([PRINTER])[0]
        for readiness in ("startup", "disconnected"):
            status = probe()["status"]
            status["webhooks"] = {"state": readiness, "state_message": ""}
            printer = normalize_printer("workshop", status, {}, "host", 1000)
            own = printer_snapshot(config, {"ok": True, "enabled": True, "updated_at": 1000, "error": None},
                                   {"printer": printer}, 4, 1000)
            self.assertEqual(node_status(own, 1000), "degraded")
            self.assertEqual(own["alerts"][0]["severity"], "warning")
            self.assertEqual(own["printer"]["klippy_state"], readiness)
            self.assertIsNone(own["printer"]["progress_pct"])

    def test_configured_printer_freshness_limit_is_exposed_to_clients(self):
        config = validate_printers([dict(PRINTER, poll_interval_s=30, ttl_s=90)])[0]
        printer = normalize_printer("workshop", probe()["status"], {}, "host", 1000)
        own = printer_snapshot(config, {"ok": True, "enabled": True, "updated_at": 1000, "error": None},
                               {"printer": printer}, 4, 1025)
        self.assertEqual(own["printer"]["ttl_s"], 90)
        self.assertEqual(own["printer"]["age_s"], 25)
        self.assertIsNotNone(own["printer"]["progress_pct"])

    def test_response_bound_and_private_api_key(self):
        remote = FakeMoonraker()
        with tempfile.TemporaryDirectory() as folder:
            key_file = Path(folder) / "printer.key"
            key_file.write_text("private-printer-api-key")
            remote.key = key_file.read_text()
            result = MoonrakerReader(dict(PRINTER, api_key_file=str(key_file)), remote.open).read()
            self.assertNotIn(remote.key, json.dumps(result))
            remote.key = None
            remote.extra_bytes = True
            with self.assertRaises(ValueError):
                MoonrakerReader(PRINTER, remote.open).get("/printer/objects/query")


class RegistryConfigTests(unittest.TestCase):
    def config(self, value):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "config.json"
            path.write_text(json.dumps(value))
            return Config.read(str(path))

    def test_old_config_and_missing_optional_fields_remain_usable(self):
        old = self.config({"host_ip": "192.0.2.10"})
        self.assertEqual(old.printers, [])
        self.assertEqual(old.display_name, "")
        current = self.config({"printers": [PRINTER]})
        self.assertEqual(current.printers[0]["poll_interval_s"], 5)
        self.assertEqual(current.printers[0]["api_key_file"], "")

    def test_invalid_registry_settings_fail_before_network_requests(self):
        for changed in ({"id": "unsafe:id"}, {"id": "x" * 33}, {"url": "http://user:secret@host:7125"},
                        {"url": "http://host:7125/printer/gcode"}, {"poll_interval_s": "5"}, {"ttl_s": 5},
                        {"timeout_s": 20}, {"api_key_file": "relative.key"}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                self.config({"printers": [dict(PRINTER, **changed)]})
        with self.assertRaises(ValueError):
            self.config({"printers": [PRINTER, PRINTER]})
        with self.assertRaises(ValueError):
            self.config({"host_ip": "not-an-ip"})
        a = proxmox_node_id("a" * 64)
        self.assertLessEqual(len(a), 63)
        self.assertNotEqual(a, proxmox_node_id("a" * 63 + "b"))
        self.assertEqual(proxmox_node_id("vm"), "proxmox:vm")

    def test_printer_failure_is_isolated_from_local_pve_and_aggregate_is_bounded(self):
        class Proc:
            interfaces, devices, disk_rates = [], [], {}
            def read(self, now):
                return {"host": {"name": "native-host", "ip": "192.0.2.10", "mem_total_bytes": 100,
                                 "mem_used_bytes": 10, "io_wait_pct": 0}, "error": None}
        config = Config(display_name="Home Lab", printers=[PRINTER], enable_gpus=False, enable_smart=False,
                        enable_turbostat=False, enable_zfs=False, enable_faults=False)
        collector = Collector(config, Proc())
        try:
            for source in list(collector.sources.values()) + list(collector.printer_sources.values()):
                source.next_run = float("inf")
            own = collector.printer_sources["workshop"]
            own.error, own.ok = "offline", False
            aggregate = collector.sample_aggregate()
            pve, printer = aggregate["nodes"]
            self.assertEqual((pve["id"], pve["name"]), ("proxmox:native-host", "Home Lab"))
            self.assertEqual(printer["id"], "klipper:workshop")
            self.assertNotIn("moonraker", pve["snapshot"]["sources"])
            self.assertFalse(any("printer" in alert["id"] for alert in pve["snapshot"]["alerts"]))
            self.assertEqual(set(printer["snapshot"]["sources"]), {"moonraker"})
            self.assertLess(len(encode_snapshot(aggregate)), MAX_AGGREGATE)
            self.assertTrue(all(len(encode_snapshot(n["snapshot"])) < MAX_PAYLOAD for n in aggregate["nodes"]))
        finally:
            collector.close()


if __name__ == "__main__":
    unittest.main()
