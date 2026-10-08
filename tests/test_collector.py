"""Behavioral tests: rates, hardware edge cases, staleness, wire bounds/auth."""
from __future__ import annotations

import concurrent.futures
import copy
import http.client
import grp
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from agent.collector import (CAPS, MAX_PAYLOAD, Config, ProcReader, ProxmoxReader, Rates, Source, TrueNASReader,
                             SmartReader, atomic_write, bounded_snapshot, cpu_delta,
                             encode_snapshot, generate_alerts, parse_cpu, parse_diskstats,
                             merge_power_sensors, parse_mem, parse_sensors, parse_smart, parse_turbostat, prepare_snapshot_directory)
from agent.server import SnapshotHTTPServer, SnapshotStore, load_token
from agent.truenas_probe import build_snapshot

ROOT = Path(__file__).resolve().parents[1]


def fixture():
    return json.loads((ROOT / "agent/demo.json").read_text())


class RateTests(unittest.TestCase):
    def test_cpu_guest_ticks_not_double_counted_and_iowait_excluded(self):
        counters = parse_cpu("cpu 100 0 20 400 10 0 0 0 40 0\ncpu0 100 0 20 400 10 0 0 0 40 0\n")
        self.assertEqual(len(counters["cpu"]), 8)
        busy, wait = cpu_delta(counters["cpu"], [90, 0, 10, 370, 0, 0, 0, 0])
        self.assertAlmostEqual(busy, 33.33, places=2)
        self.assertAlmostEqual(wait, 16.67, places=2)
        self.assertEqual(cpu_delta(counters["cpu"], None), (None, None))
        self.assertEqual(cpu_delta([0] * 8, counters["cpu"]), (None, None))

    def test_delta_rates_and_counter_reset(self):
        rates = Rates()
        self.assertIsNone(rates.rate("guest", 1000, 1))
        self.assertEqual(rates.rate("guest", 1200, 3), 100)
        self.assertIsNone(rates.rate("guest", 2, 5))
        self.assertEqual(rates.rate("guest", 202, 7), 100)
        self.assertIsNone(rates.rate("guest", None, 9))
        self.assertIsNone(rates.rate("guest", 302, 11))

    def test_disk_sectors_use_512_and_mem_available(self):
        disks = parse_diskstats("259 0 nvme0n1 8 0 2 3 10 0 4 6 0 0 0\n")
        self.assertEqual(disks["nvme0n1"], (1024, 2048))
        memory = parse_mem("MemTotal: 1000 kB\nMemAvailable: 600 kB\nSwapTotal: 100 kB\nSwapFree: 90 kB\n")
        self.assertEqual(memory["mem_used_bytes"], 400 * 1024)
        self.assertEqual(memory["swap_used_bytes"], 10 * 1024)

    def test_network_and_storage_selection_avoid_bridge_partition_double_counting(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "class/net/eno1/device").mkdir(parents=True)
            (root / "class/net/vmbr1").mkdir(parents=True)
            (root / "class/net/tap100i0").mkdir(parents=True)
            (root / "block/nvme0n1").mkdir(parents=True)
            reader = ProcReader(Config(), sys_root=root)
            self.assertEqual(reader.select_interfaces({"eno1": (), "vmbr1": (), "tap100i0": (), "lo": ()}), ["eno1"])
            self.assertEqual(reader.select_disks({"nvme0n1": (), "nvme0n1p1": (), "dm-0": (), "loop0": ()}), ["nvme0n1"])
            configured = ProcReader(Config(network_interfaces=["vmbr1"]), sys_root=root)
            self.assertEqual(configured.select_interfaces({"eno1": (), "vmbr1": ()}), ["vmbr1"])
            with self.assertRaises(ValueError):
                configured.select_interfaces({"eno1": ()})


class HardwareTests(unittest.TestCase):
    def power_snapshot(self):
        healthy = {"ok": True, "enabled": True, "updated_at": 1000, "age_s": 3, "error": None}
        return {"sensors": parse_sensors({"native": {"Rail": {"curr1_input": 1.5},
                                                    "PSU": {"power1_input": 25}}}),
                "power": {"package_w": 78.62, "cores_w": 76.62, "graphics_w": .16},
                "gpus": [{"id": "pci:0000:01:00.0", "name": "AD103 [GeForce RTX 4080 SUPER]",
                          "vendor": "NVIDIA", "owner": "vm:100", "status": "active", "error": None,
                          "power_w": 43.51, "updated_at": 998, "age_s": 5}],
                "sources": {name: healthy.copy() for name in ("turbostat", "gpus", "guest_100")}}

    def test_component_power_reuses_existing_data_preserves_native_and_cached_rows(self):
        snapshot = self.power_snapshot()
        before = copy.deepcopy(snapshot)
        rows = merge_power_sensors(snapshot, Config())
        self.assertEqual(snapshot, before)
        self.assertEqual(rows[:2], snapshot["sensors"])
        self.assertEqual(len(rows), 6)
        cpu = next(row for row in rows if row["id"] == "telemetry/cpu/package_w")
        gpu = next(row for row in rows if row.get("scope") == "gpu-board")
        self.assertEqual((cpu["value"], cpu["scope"], cpu["source"]), (78.62, "cpu-package", "turbostat"))
        self.assertEqual((gpu["name"], gpu["value"], gpu["source"], gpu["age_s"]), ("RTX 4080 SUPER board", 43.51, "guest_100", 5))
        self.assertEqual({row["name"]: row["value"] for row in rows if row.get("source") == "turbostat"},
                         {"CPU package": 78.62, "CPU cores": 76.62, "CPU graphics domain": .16})
        self.assertEqual(sum(row["kind"] == "current" for row in rows), 1)
        snapshot["sensors"] = rows
        self.assertEqual(merge_power_sensors(snapshot, Config()), rows)
        snapshot["power"]["package_w"] = 20
        refreshed = merge_power_sensors(snapshot, Config())
        self.assertEqual(next(row["value"] for row in refreshed if row.get("scope") == "cpu-package"), 20)
        self.assertEqual(cpu["value"], 78.62)
        snapshot["sources"]["turbostat"]["ok"] = False
        self.assertFalse(any(row.get("source") == "turbostat" for row in merge_power_sensors(snapshot, Config())))

    def test_component_power_omits_failed_disabled_stale_and_unknown_sources(self):
        for field, value in (("ok", False), ("enabled", False), ("age_s", 21), ("age_s", None)):
            with self.subTest(source="turbostat", field=field, value=value):
                snapshot = self.power_snapshot()
                snapshot["sources"]["turbostat"][field] = value
                self.assertFalse(any(row.get("source") == "turbostat" for row in merge_power_sensors(snapshot, Config())))
        for source, field, value in (("gpus", "ok", False), ("guest_100", "ok", False),
                                     ("gpus", "age_s", 91), ("guest_100", "age_s", 46)):
            with self.subTest(source=source, field=field, value=value):
                snapshot = self.power_snapshot()
                snapshot["sources"][source][field] = value
                self.assertFalse(any(row.get("scope") == "gpu-board" for row in merge_power_sensors(snapshot, Config())))
        for field, value in (("age_s", 46), ("age_s", None), ("status", "unavailable"), ("error", "GPU telemetry failed")):
            with self.subTest(gpu_field=field, value=value):
                snapshot = self.power_snapshot()
                snapshot["gpus"][0][field] = value
                self.assertFalse(any(row.get("scope") == "gpu-board" for row in merge_power_sensors(snapshot, Config())))

    def test_component_power_zero_is_valid_nonfinite_is_unknown_and_no_current_inferred(self):
        for value in (0, None, float("nan"), float("inf"), -1):
            with self.subTest(value=value):
                snapshot = self.power_snapshot()
                snapshot["sensors"] = []
                snapshot["power"] = {key: value for key in ("package_w", "cores_w", "graphics_w")}
                snapshot["gpus"][0]["power_w"] = value
                rows = merge_power_sensors(snapshot, Config())
                self.assertEqual(len(rows), 4 if value == 0 else 0)
                self.assertFalse(any(row["kind"] == "current" for row in rows))
                if rows:
                    self.assertEqual([row["value"] for row in rows], [0, 0, 0, 0])

    def test_component_power_remains_bounded_with_visible_omissions(self):
        snapshot = self.power_snapshot()
        snapshot["sensors"] = [{"id": f"native/{i}", "kind": "temperature", "value": 40} for i in range(64)]
        snapshot["sensors"] = merge_power_sensors(snapshot, Config())
        snapshot["alerts"] = []
        result = bounded_snapshot(snapshot)
        self.assertEqual(len(result["sensors"]), 64)
        self.assertEqual(result["limits"]["counts"]["sensors"], 68)
        self.assertEqual(result["limits"]["truncated"]["sensors"], 4)
        self.assertTrue(any(alert["id"] == "display/truncated" for alert in result["alerts"]))

    def test_nas_reduces_topology_without_double_counting_or_fabricating_smart(self):
        leaf = {"type": "DISK", "disk": "sdc", "status": "ONLINE", "children": [],
                "stats": {"read_errors": 1, "write_errors": 0, "checksum_errors": 2}}
        pool = {"name": "tank", "status": "ONLINE", "healthy": True, "allocated": 100, "size": 200,
                "topology": {"data": [{"type": "RAIDZ1", "children": [leaf], "stats": {"read_errors": 1}}]}}
        disks = [{"name": "sdc", "model": "WD HDD", "size": 1000}, {"name": "sda", "model": "QEMU_HARDDISK"}]
        alerts = [{"uuid": "bad", "level": "CRITICAL", "formatted": "Pool error", "dismissed": False},
                  {"uuid": "ignore", "level": "WARNING", "formatted": "Old alert", "dismissed": True}]
        result = build_snapshot([pool], disks, {"sdc": 41}, alerts, {"sdc": {"read_bytes": 100, "write_bytes": 200}})
        self.assertEqual(result["pools"][0]["read_errors"], 1)
        self.assertEqual(len(result["disks"]), 1)
        disk = result["disks"][0]
        self.assertIsNone(disk["health"])
        self.assertEqual(disk["zfs_status"], "ONLINE")
        self.assertEqual(disk["temp_c"], 41)
        self.assertEqual(disk["read_bytes"], 100)
        self.assertEqual(len(result["alerts"]), 1)
        self.assertEqual(result["temperature_cache_interval_s"], 300)

    def test_nas_ssh_host_verification_rates_and_boot_reset(self):
        document = {"schema": 1, "generated_at": time.time(), "boot_id": "one", "pools": [], "alerts": [],
                    "temperature_cache_interval_s": 300,
                    "disks": [{"name": "sdc", "model": "HDD", "temp_c": 41, "zfs_status": "ONLINE", "read_bytes": 1000, "write_bytes": 2000}]}
        calls = []
        def runner(argv, **kwargs):
            calls.append((argv, kwargs))
            return subprocess.CompletedProcess(argv, 0, json.dumps(document), "")
        reader = TrueNASReader(Config(truenas_ssh_host="192.0.2.13"), runner)
        with patch("agent.collector.time.monotonic", side_effect=[1, 3, 5]):
            first = reader.read()
            self.assertEqual(first["disks"][0]["name"], "NAS/sdc")
            self.assertIsNone(first["disks"][0]["read_bps"])
            document["disks"][0]["read_bytes"] = 1200
            self.assertEqual(reader.read()["disks"][0]["read_bps"], 100)
            document["boot_id"] = "two"
            self.assertIsNone(reader.read()["disks"][0]["read_bps"])
        argv, kwargs = calls[0]
        self.assertIn("StrictHostKeyChecking=yes", argv)
        self.assertIn("BatchMode=yes", argv)
        self.assertEqual(argv[-1], "snapshot")
        self.assertEqual(kwargs["timeout"], 15)
        document["generated_at"] = time.time() - 120
        with self.assertRaises(ValueError):
            reader.read()

    def test_sensors_flatten_and_historical_nct_max(self):
        sensors = parse_sensors({
            "nct6687-isa-0a20": {"CPU": {"temp1_input": 40, "temp1_max": 40},
                                   "fan": {"fan1_input": 800}, "Vcore": {"in0_input": 1.1}},
            "coretemp-isa-0000": {"Package": {"temp1_input": 55, "temp1_max": 80, "temp1_crit": 100, "temp1_crit_alarm": 1}},
            "power": {"Pkg": {"power1_input": 20, "power1_average": 21}},
        })
        self.assertEqual(len(sensors), 5)
        nct = next(s for s in sensors if s["name"] == "CPU")
        self.assertIsNone(nct["high"])
        core = next(s for s in sensors if s["name"] == "Package")
        self.assertEqual(core["high"], 80)
        self.assertTrue(core["alarm"])
        snapshot = fixture()
        snapshot["sensors"] = sensors
        snapshot["power"]["cpu_temp_c"] = 40
        alerts = generate_alerts(snapshot, Config())
        self.assertFalse(any(a["id"] == nct["id"] for a in alerts))
        self.assertTrue(any(a["id"] == core["id"] + "/alarm" for a in alerts))

    def test_turbostat_model_dependent_headers(self):
        text = "turbostat warning\nAvg_MHz Busy% Bzy_MHz CPU%c1 PkgTmp Pkg%pc10 PkgWatt CorWatt GFXWatt RAMWatt\n83 2.2 3800 8.1 47 51.3 12.7 10.5 0.16 0.00\n"
        power = parse_turbostat(text)
        self.assertEqual(power["cpu_mhz"], 83)
        self.assertEqual(power["busy_mhz"], 3800)
        self.assertEqual(power["package_w"], 12.7)
        self.assertEqual(power["cores_w"], 10.5)
        self.assertEqual(power["graphics_w"], .16)
        self.assertNotIn("ram_w", power)
        missing = parse_turbostat("Avg_MHz PkgWatt\n83 12.7\n")
        self.assertIsNone(missing["cores_w"])
        self.assertIsNone(missing["graphics_w"])
        self.assertEqual(power["cstate_pct"]["Pkg%pc10"], 51.3)
        self.assertIsNone(parse_turbostat("Avg_MHz Bzy_MHz\n- 3000\n")["package_w"])
        alternate = parse_turbostat("Avg_MHz Bzy_MHz Pk%pc10\n80 3000 58.6\n")
        self.assertEqual(alternate["cstate_pct"]["Pk%pc10"], 58.6)

    def test_invalid_nvme_high_does_not_suppress_real_critical_alarm(self):
        sensors = parse_sensors({"nvme-pci-0100": {"Composite": {"temp1_input": 86, "temp1_max": 65261, "temp1_crit": 84}}})
        self.assertIsNone(sensors[0]["high"])
        snapshot = fixture()
        snapshot["sensors"] = sensors
        alert = next(a for a in generate_alerts(snapshot, Config()) if a["id"] == sensors[0]["id"])
        self.assertEqual(alert["severity"], "critical")

    def test_standby_is_skipped_not_failure_and_nonzero_health_retained(self):
        sleeping = parse_smart({"smartctl": {"messages": [{"string": "Device is in STANDBY mode"}]}}, "sda", 3)
        self.assertEqual(sleeping["status"], "standby")
        self.assertIsNone(sleeping["health"])
        failed = parse_smart({"smart_status": {"passed": False}, "temperature": {"current": 55}}, "sdb", 8)
        self.assertEqual(failed["health"], "failed")
        self.assertEqual(failed["status"], "active")
        self.assertEqual(failed["temp_c"], 55)
        nvme = parse_smart({"smart_status": {"passed": True}, "nvme_smart_health_information_log":
                           {"critical_warning": 1, "percentage_used": 101, "media_errors": 4, "temperature": 64}}, "nvme0n1", 0)
        self.assertEqual(nvme["health"], "failed")
        self.assertEqual(len(nvme["warnings"]), 3)

    def test_smart_scan_never_opens_and_nvme_avoids_unsupported_standby(self):
        calls = []
        def runner(argv, **kwargs):
            calls.append(argv)
            document = {"devices": [{"name": "/dev/sda", "type": "sat"}, {"name": "/dev/nvme0", "type": "nvme"}]} if "--scan" in argv else {"smart_status": {"passed": True}}
            return subprocess.CompletedProcess(argv, 0, json.dumps(document), "")
        result = SmartReader(Config(), runner).read()
        self.assertEqual(len(result["disks"]), 2)
        self.assertNotIn("--scan-open", calls[0])
        self.assertIn("standby,3", calls[1])
        self.assertNotIn("-n", calls[2])
        self.assertEqual(calls[1][-2:], ["-d", "sat"])

    def test_nvme_controller_health_maps_to_namespace_io_name(self):
        def runner(argv, **kwargs):
            result = {"devices": [{"name": "/dev/nvme0", "type": "nvme"}]} if "--scan" in argv else {"smart_status": {"passed": True}}
            return subprocess.CompletedProcess(argv, 0, json.dumps(result), "")
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "block/nvme0n1").mkdir(parents=True)
            (root / "block/nvme0n1p1").mkdir(parents=True)
            result = SmartReader(Config(), runner, sys_root=root).read()
            self.assertEqual([d["name"] for d in result["disks"]], ["nvme0n1"])
            self.assertEqual(result["disks"][0]["health"], "passed")

    def test_proxmox_rates_filter_node_and_reset_on_reboot(self):
        guest = {"vmid": 100, "name": "Windows", "type": "qemu", "node": "vm", "status": "running",
                 "cpu": .2, "mem": 100, "maxmem": 200, "uptime": 100, "netin": 1000,
                 "netout": 2000, "diskread": 3000, "diskwrite": 4000}
        def runner(argv, **kwargs):
            path = argv[2]
            if path == "/cluster/resources":
                result = [guest.copy(), dict(guest, node="other", vmid=101)]
            elif path.endswith("/storage"):
                result = [{"storage": "local", "active": 0, "enabled": 1, "used": 100, "total": 200}]
            else:
                result = {"wait": .05}
            return subprocess.CompletedProcess(argv, 0, json.dumps(result), "")
        reader = ProxmoxReader(Config(node="vm"), runner)
        with patch("agent.collector.time.monotonic", side_effect=[1, 3, 5]):
            first = reader.read()
            self.assertEqual(len(first["guests"]), 1)
            self.assertIsNone(first["guests"][0]["net_rx_bps"])
            self.assertEqual(first["storage"][0]["status"], "offline")
            self.assertEqual(first["io_wait_pct"], 5)
            guest.update(uptime=102, netin=1200)
            self.assertEqual(reader.read()["guests"][0]["net_rx_bps"], 100)
            # A reboot can retain/increase a counter; uptime still invalidates it.
            guest.update(uptime=1, netin=1300)
            self.assertIsNone(reader.read()["guests"][0]["net_rx_bps"])


class PublicationTests(unittest.TestCase):
    def test_shared_snapshot_directory_group_is_applied_at_collector_start(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / "runtime/snapshot.json"
            current_group = grp.getgrgid(os.getgid()).gr_name
            prepare_snapshot_directory(output, current_group)
            self.assertEqual(output.parent.stat().st_gid, os.getgid())
            self.assertEqual(output.parent.stat().st_mode & 0o7777, 0o2750)
            atomic_write(output, fixture())
            self.assertEqual(output.stat().st_gid, os.getgid())
            with self.assertRaises(ValueError):
                prepare_snapshot_directory(output, "homelab-monitor-deliberately-absent-test-group")

    def test_fault_history_current_alerts_and_unknown_counts(self):
        snapshot = fixture()
        snapshot["faults"] = {"segfault_count_24h": None, "events": []}
        self.assertFalse(any(a["id"] == "faults/segfaults" for a in generate_alerts(snapshot, Config())))
        snapshot["faults"] = {"segfault_count_24h": 3, "events": [
            {"id": "old", "kind": "hardware", "timestamp": time.time() - 86401, "message": "Historic MCE"},
            {"id": "now", "kind": "hardware", "timestamp": time.time() - 5, "message": "Current MCE"}]}
        alerts = generate_alerts(snapshot, Config())
        self.assertTrue(any(a["id"] == "faults/segfaults" and a["severity"] == "critical" for a in alerts))
        self.assertTrue(any(a["id"] == "faults/now" and a["severity"] == "critical" for a in alerts))
        self.assertFalse(any(a["id"] == "faults/old" for a in alerts))

    def test_disabled_source_and_expected_guest_alerts(self):
        snapshot = fixture()
        snapshot["sources"]["zfs"] = {"enabled": False, "ok": False, "error": "disabled in configuration"}
        snapshot["sources"]["turbostat"] = {"enabled": True, "ok": False, "error": "missing"}
        alerts = generate_alerts(snapshot, Config(expected_running_guests=[100, 104, 999]))
        self.assertFalse(any(a["id"] == "source/zfs" for a in alerts))
        self.assertTrue(any(a["id"] == "source/turbostat" for a in alerts))
        self.assertTrue(any(a["id"] == "guest/104/stopped" and a["severity"] == "critical" for a in alerts))
        self.assertTrue(any(a["id"] == "guest/999/missing" for a in alerts))
        self.assertFalse(any(a["id"] == "guest/100/stopped" for a in alerts))

    def test_failed_probe_retains_last_good_only_until_ttl(self):
        source = Source(lambda: None, interval=6, ttl=20)
        success = concurrent.futures.Future()
        success.set_result({"value": 42})
        source.future = success
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
            source.advance(executor, 100, 1000)
            self.assertEqual(source.current(105), {"value": 42})
            fail = concurrent.futures.Future()
            fail.set_exception(RuntimeError("offline"))
            source.future = fail
            source.advance(executor, 110, 1010)
            self.assertFalse(source.status(110)["ok"])
            self.assertEqual(source.status(110)["updated_at"], 1000)
            self.assertEqual(source.current(110), {"value": 42})
            self.assertIsNone(source.current(121))
            self.assertEqual(source.status(121)["error"], "sample expired")

    def test_probe_never_launches_twice_when_inflight(self):
        source = Source(lambda: None, 3, 10)
        future = concurrent.futures.Future()
        source.future = future
        class NoSubmit:
            def submit(self, *_):
                raise AssertionError("Duplicate source probe")
        source.advance(NoSubmit(), 20, 20)
        self.assertIs(source.future, future)

    def test_bounds_and_payload_size_report_omissions(self):
        snapshot = fixture()
        for key, cap in CAPS.items():
            base = copy.deepcopy(snapshot[key][0])
            snapshot[key] = [copy.deepcopy(base) for _ in range(cap + 8)]
        result = bounded_snapshot(snapshot)
        for key, cap in CAPS.items():
            self.assertLessEqual(len(result[key]), cap)
            self.assertGreaterEqual(result["limits"]["truncated"][key], 8)
        self.assertLessEqual(len(encode_snapshot(result)), MAX_PAYLOAD)
        self.assertTrue(any(a["id"] == "display/truncated" for a in result["alerts"]))
        # Heavy Unicode is measured as UTF-8 bytes, not number of characters.
        for sensor in result["sensors"]:
            sensor["name"] = "温" * 200
        bounded_snapshot(result)
        self.assertLessEqual(len(encode_snapshot(result)), MAX_PAYLOAD)

    def test_atomic_writer_permissions_and_incomplete_file_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "snapshot.json"
            atomic_write(path, fixture())
            self.assertEqual(path.stat().st_mode & 0o777, 0o640)
            self.assertEqual(json.loads(path.read_text())["schema"], 1)
            self.assertEqual(len(list(Path(folder).glob(".snapshot-*"))), 0)
            path.write_text('{"schema":1')
            with self.assertRaises(ValueError):
                SnapshotStore(path).read()


class HTTPTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.path = Path(self.folder.name) / "snapshot.json"
        data = fixture()
        data["generated_at"] = time.time()
        atomic_write(self.path, data)
        self.token = "a" * 43
        self.server = SnapshotHTTPServer(("127.0.0.1", 0), self.token, SnapshotStore(self.path))
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": .01}, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=1)
        self.folder.cleanup()

    def get(self, target, token=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=2)
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        connection.request("GET", target, headers=headers)
        response = connection.getresponse()
        code, payload, headers = response.status, response.read(), dict(response.getheaders())
        connection.close()
        return code, payload, headers

    def test_auth_and_read_only_paths(self):
        code, body, headers = self.get("/api/v1/snapshot")
        self.assertEqual(code, 401)
        self.assertNotIn(b"guests", body)
        self.assertEqual(self.get("/api/v1/snapshot", "wrong")[0], 401)
        code, body, headers = self.get("/api/v1/snapshot", self.token)
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body)["schema"], 1)
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assertEqual(self.get("/api/v1/snapshot?token=" + self.token)[0], 404)
        self.assertEqual(self.get("/../../etc/shadow", self.token)[0], 404)
        code, body, _ = self.get("/healthz")
        self.assertEqual(code, 200)
        self.assertEqual(json.loads(body), {"ok": True})

    def test_stale_health_and_missing_snapshot(self):
        data = fixture()
        data["generated_at"] = time.time() - 90
        atomic_write(self.path, data)
        self.assertEqual(self.get("/healthz")[0], 503)
        # Clients get last snapshot + real timestamp to show explicit stale UI.
        code, body, headers = self.get("/api/v1/snapshot", self.token)
        self.assertEqual(code, 200)
        self.assertGreater(float(headers["X-Snapshot-Age"]), 80)
        self.path.unlink()
        self.assertEqual(self.get("/api/v1/snapshot", self.token)[0], 503)

    def test_token_validation_and_labeled_demo(self):
        with patch.dict(os.environ, {"HOMELAB_DISPLAY_TOKEN": "short"}):
            with self.assertRaises(ValueError):
                load_token()
        raw, age = SnapshotStore(ROOT / "agent/demo.json", demo=True).read()
        self.assertTrue(json.loads(raw)["demo"])
        self.assertLess(age, 1)
        self.assertGreater(json.loads(raw)["generated_at"], time.time() - 1)


if __name__ == "__main__":
    unittest.main()
