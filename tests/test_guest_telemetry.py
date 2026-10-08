import base64
import concurrent.futures
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import unittest

from agent.collector import Collector, Config, Source, TrueNASReader, merge_guest_memory
from agent.guest_telemetry import (DRMRates, GuestReader, LINUX_SCRIPT, QGA_BRIDGE, QGA_OUTPUT_LIMIT,
                                 WINDOWS_SCRIPT, memory, parse_document, parse_meminfo, parse_nvidia_csv, qga_command)
from agent.gpus import GPUReader, merge_gpus
from agent.truenas_probe import os_memory


def document():
    return {"schema": 1, "generated_at": time.time(), "memory": {"total_bytes": 32 * 1024**3, "available_bytes": 13 * 1024**3},
            "cards": [{"name": "RTX 4080 SUPER", "pnp_id": "PCI\\VEN_10DE&DEV_2702&SUBSYS_0", "driver": "32.0.15", "AdapterRAM": 4 * 1024**3}],
            "nvidia_csv": "RTX 4080 SUPER, 00000000:01:00.0, 47, 2132, 16376, 43, 26.67, 315, 405, 0\n"}


class MemoryTests(unittest.TestCase):
    def test_cim_visible_memory_and_nvidia_vram_are_independent(self):
        parsed = parse_document(document())
        self.assertEqual(parsed["memory"]["mem_used_bytes"], 19 * 1024**3)
        self.assertEqual(parsed["gpus"][0]["mem_total_bytes"], 16376 * 1024**2)
        self.assertEqual(parsed["gpus"][0]["device_id"], "2702")
        self.assertEqual(parsed["gpus"][0]["fan_pct"], 0)
        self.assertEqual(parsed["gpus"][0]["driver"], "nvidia 32.0.15")

    def test_invalid_memory_and_arc_do_not_produce_fake_usage(self):
        for total, available, arc in ((100, 101, None), (0, 0, None), (100, -1, None), (100, 20, -1), (100, 20, 101)):
            with self.assertRaises(ValueError):
                memory(total, available, arc)
        result = parse_meminfo("MemTotal: 100 kB\nMemAvailable: 20 kB\n", "size 4 30720\n")
        self.assertEqual(result["mem_used_bytes"], 80 * 1024)
        self.assertEqual(result["mem_noncache_used_bytes"], 50 * 1024)
        self.assertEqual(result["memory_basis"], "guest-os-arc")

    def test_repeated_merge_preserves_pve_cache_and_failed_source_never_falls_back(self):
        pve = [{"id": 100, "status": "running", "mem_used_bytes": 32.1 * 1024**3,
                "mem_total_bytes": 32 * 1024**3, "mem_host_bytes": 32.1 * 1024**3}]
        saved = copy.deepcopy(pve)
        sampled = {100: parse_document(document())}
        statuses = {100: {"ok": True, "updated_at": time.time(), "error": None}}
        pve_status = {"ok": True, "updated_at": time.time(), "age_s": 0}
        for _ in range(3):
            merged = merge_guest_memory(pve, sampled, statuses, {100}, 101, pve_status, time.time())
            self.assertEqual(merged[0]["mem_used_bytes"], 19 * 1024**3)
            self.assertEqual(merged[0]["pve_mem_used_bytes"], saved[0]["mem_used_bytes"])
            self.assertEqual(pve, saved)
        statuses[100] = {"ok": False, "error": "QGA unavailable", "updated_at": time.time() - 30}
        merged = merge_guest_memory(pve, sampled, statuses, {100}, 101, pve_status, time.time())
        self.assertIsNone(merged[0]["mem_used_bytes"])
        self.assertIsNone(merged[0]["mem_total_bytes"])
        self.assertEqual(merged[0]["mem_error"], "QGA unavailable")
        pve[0]["status"] = "stopped"
        statuses[100]["ok"] = True
        self.assertIsNone(merge_guest_memory(pve, sampled, statuses, {100}, 101, pve_status, time.time())[0]["mem_used_bytes"])

    def test_nas_memory_error_does_not_remove_disk_and_pool_metrics(self):
        data = {"schema": 1, "generated_at": time.time(), "disks": [{"name": "sdc", "temp_c": 40}],
                "pools": [{"name": "nas", "used_bytes": 100, "total_bytes": 200, "status": "ONLINE"}],
                "alerts": [], "memory": {"mem_total_bytes": 100, "mem_available_bytes": 20, "mem_cache_bytes": 101}}
        def runner(argv, **kwargs):
            return subprocess.CompletedProcess(argv, 0, json.dumps(data), "")
        result = TrueNASReader(Config(truenas_ssh_host="192.0.2.13"), runner).read()
        self.assertIsNone(result["memory"])
        self.assertIsNotNone(result["memory_error"])
        self.assertEqual(result["disks"][0]["temp_c"], 40)
        self.assertEqual(result["storage"][0]["status"], "ONLINE")
        self.assertIsNone(result["error"])
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "meminfo").write_text("MemTotal: 100 kB\nMemAvailable: 20 kB\n")
            (root / "spl/kstat/zfs").mkdir(parents=True)
            (root / "spl/kstat/zfs/arcstats").write_text("size 4 30720\n")
            self.assertEqual(os_memory(root)["mem_noncache_used_bytes"], 50 * 1024)


@unittest.skipUnless(shutil.which("perl"), "PVE bridge requires the host Perl runtime")
class BridgeContractTests(unittest.TestCase):
    """Execute the actual bridge against small installed-library stand-ins."""
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)
        modules = {
            "JSON.pm": """package JSON; use JSON::PP (); sub true {JSON::PP::true()} sub encode_json {JSON::PP::encode_json($_[0])} sub is_bool {JSON::PP::is_bool($_[0])} 1;""",
            "PVE/QemuConfig.pm": """package PVE::QemuConfig; sub load_config { return {agent => $ENV{HOMELAB_TEST_AGENT_ENABLED}}; } 1;""",
            "PVE/QemuServer/Helpers.pm": """package PVE::QemuServer::Helpers; sub vm_running_locally { return $ENV{HOMELAB_TEST_VM_RUNNING}; } 1;""",
            "PVE/QemuServer/Agent.pm": """package PVE::QemuServer::Agent;
                sub get_qga_key { return $_[0]->{agent}; }
                sub check_agent_error { die 'Agent error: '.$_[0]->{error}->{desc} if $_[0]->{error}; return 1; } 1;""",
            "PVE/QemuServer/Monitor.pm": """package PVE::QemuServer::Monitor;
                use JSON; use JSON::PP (); use MIME::Base64 qw(encode_base64);
                sub mon_cmd {
                    my ($vmid,$command,%params)=@_;
                    open(my $fh,'>>',$ENV{HOMELAB_TEST_CALL_LOG}) or die $!;
                    print $fh JSON::encode_json({vmid=>$vmid,command=>$command,params=>\\%params}),"\\n";
                    close($fh);
                    return {id=>'mswindows'} if $command eq 'guest-get-osinfo';
                    return {pid=>42} if $command eq 'guest-exec';
                    return {exited=>JSON::true(),exitcode=>0,'out-truncated'=>JSON::PP::false(),
                            'out-data'=>encode_base64($ENV{HOMELAB_TEST_OUTPUT},''),
                            'err-data'=>encode_base64('guest diagnostic','')} if $command eq 'guest-exec-status';
                    die "unexpected command $command";
                } 1;""",
        }
        for name, content in modules.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        self.log = self.root / "calls.jsonl"
        self.env = {**os.environ, "PERL5LIB": str(self.root), "HOMELAB_TEST_AGENT_ENABLED": "1",
                    "HOMELAB_TEST_VM_RUNNING": "1", "HOMELAB_TEST_CALL_LOG": str(self.log),
                    "HOMELAB_TEST_OUTPUT": json.dumps(document())}

    def runner(self, argv, **kwargs):
        result = subprocess.run(argv, env=self.env, capture_output=True, text=True, timeout=kwargs["timeout"])
        if result.returncode:
            raise RuntimeError(result.stderr)
        return result

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_fixed_actions_keep_pve_sync_transport_and_decode_status_wire_data(self):
        reader = GuestReader(100, self.runner)
        self.assertEqual(reader.read()["memory"]["mem_used_bytes"], 19 * 1024**3)
        calls = self.calls()
        self.assertEqual([row["command"] for row in calls], ["guest-get-osinfo", "guest-exec", "guest-exec-status"])
        self.assertTrue(all(row["params"]["timeout"] == 12 for row in calls))
        args = calls[1]["params"]
        self.assertEqual(args["path"], "powershell.exe")
        self.assertTrue(args["capture-output"])
        self.assertEqual(base64.b64decode(args["arg"][-1]).decode("utf-16le"), WINDOWS_SCRIPT)
        status = qga_command(100, "exec-status", self.runner, 42)
        self.assertEqual(json.loads(status["out-data"]), json.loads(self.env["HOMELAB_TEST_OUTPUT"]))
        self.assertEqual(status["err-data"], "guest diagnostic")
        self.assertEqual(type(status["exited"]), int)
        self.assertEqual(status["out-truncated"], 0)
        qga_command(102, "exec-linux", self.runner)
        self.assertEqual(self.calls()[-1]["params"]["arg"], ["-c", LINUX_SCRIPT])

    def test_disabled_or_stopped_guest_and_invalid_actions_never_reach_monitor(self):
        for flag in ("HOMELAB_TEST_AGENT_ENABLED", "HOMELAB_TEST_VM_RUNNING"):
            self.env[flag] = "0"
            with self.assertRaises(RuntimeError):
                qga_command(100, "exec-windows", self.runner)
            self.env[flag] = "1"
        for vmid, action, pid in ((99, "get-osinfo", None), (100, "guest-shutdown", None),
                                  (100, "exec-status", -1), (100, "exec-windows", 42)):
            with self.assertRaises(ValueError):
                qga_command(vmid, action, self.runner, pid)
        # The Perl boundary rejects bypassing Python validation as well.
        for args in (("100", "guest-shutdown"), ("100", "exec-windows", "arbitrary-script"),
                     ("100", "exec-status", "-1")):
            result = subprocess.run(["perl", "-e", QGA_BRIDGE, *args], env=self.env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_oversized_decoded_guest_output_is_truncated_and_reader_clears_consumed_pid(self):
        self.env["HOMELAB_TEST_OUTPUT"] = "x" * (QGA_OUTPUT_LIMIT + 1)
        status = qga_command(100, "exec-status", self.runner, 42)
        self.assertEqual(status["out-truncated"], 1)
        self.assertEqual(status["out-data"], "")
        reader = GuestReader(100, self.runner)
        reader.pending_pid = 42
        with self.assertRaisesRegex(RuntimeError, "truncated"):
            reader.read()
        self.assertIsNone(reader.pending_pid)


class GuestLifecycleTests(unittest.TestCase):
    def test_library_transport_timeout_reuses_pending_pid_without_relaunch(self):
        calls, status_count = [], [0]
        def runner(argv, **kwargs):
            calls.append((argv, kwargs))
            self.assertEqual(kwargs["timeout"], 20)
            if argv[4] == "get-osinfo":
                value = {"id": "mswindows"}
            elif argv[4] == "exec-windows":
                value = {"pid": 42}
            else:
                status_count[0] += 1
                if status_count[0] == 1:
                    raise subprocess.TimeoutExpired(argv, 20)
                self.assertEqual(argv[-1], "42")
                value = {"exited": 1, "exitcode": 0, "out-data": json.dumps(document())}
            return subprocess.CompletedProcess(argv, 0, json.dumps(value), "")
        reader = GuestReader(100, runner)
        with self.assertRaises(subprocess.TimeoutExpired):
            reader.read()
        self.assertEqual(reader.pending_pid, 42)
        self.assertEqual(reader.read()["memory"]["mem_used_bytes"], 19 * 1024**3)
        self.assertIsNone(reader.pending_pid)
        self.assertEqual([a[4] for a, _ in calls], ["get-osinfo", "exec-windows", "exec-status", "exec-status"])

    def test_inflight_reset_cannot_repopulate_old_os_pid_or_drm_state(self):
        for stage in ("get-osinfo", "exec-windows", "exec-status"):
            with self.subTest(stage=stage):
                entered, release = threading.Event(), threading.Event()
                def runner(argv, **kwargs):
                    action = argv[4]
                    if action == stage:
                        entered.set()
                        self.assertTrue(release.wait(2))
                    value = ({"id": "mswindows"} if action == "get-osinfo" else
                             {"pid": 42} if action == "exec-windows" else
                             {"exited": 1, "exitcode": 0, "out-data": json.dumps(document())})
                    return subprocess.CompletedProcess(argv, 0, json.dumps(value), "")
                reader = GuestReader(100, runner)
                with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                    future = executor.submit(reader.read)
                    self.assertTrue(entered.wait(2))
                    reader.reset(restarted=True)
                    drm_after_reset = reader.drm
                    release.set()
                    with self.assertRaisesRegex(RuntimeError, "Guest changed"):
                        future.result(timeout=2)
                self.assertIsNone(reader.os_kind)
                self.assertIsNone(reader.pending_pid)
                self.assertIs(reader.drm, drm_after_reset)
                self.assertIsNone(reader.drm.mono)

    def test_publisher_restart_invalidates_cached_os_metrics(self):
        class Proc:
            interfaces, devices, disk_rates = [], [], {}
            def read(self, now):
                return {"host": {"name": "test", "mem_total_bytes": 100, "mem_used_bytes": 10, "io_wait_pct": 0}, "error": None}
        config = Config(qga_guest_ids=[100], enable_gpus=False, enable_smart=False,
                        enable_turbostat=False, enable_zfs=False, enable_faults=False)
        collector = Collector(config, Proc())
        try:
            guest = {"id": 100, "status": "running", "name": "Windows", "uptime_s": 100,
                     "mem_used_bytes": 32.1 * 1024**3, "mem_total_bytes": 32 * 1024**3}
            for name, data in (("proxmox", {"guests": [guest], "storage": []}),
                               ("sensors", {"sensors": []}), ("guest_100", parse_document(document()))):
                source = collector.sources[name]
                source.data, source.ok, source.last_probe_succeeded = data, True, True
                source.updated_mono, source.updated_at, source.next_run = time.monotonic(), time.time(), float("inf")
                source.error = None
            self.assertEqual(collector.sample()["guests"][0]["mem_used_bytes"], 19 * 1024**3)
            self.assertEqual(guest["mem_used_bytes"], 32.1 * 1024**3)
            collector.sources["guest_100"].probe = lambda: None
            collector.guest_readers[100].pending_pid = 42
            guest["uptime_s"] = 1
            changed = collector.sample()["guests"][0]
            self.assertIsNone(changed["mem_used_bytes"])
            self.assertIsNone(collector.guest_readers[100].pending_pid)
            self.assertIn("waiting", changed["mem_error"])
        finally:
            collector.close()

    def test_async_exec_invalid_pending_pid_recovers_and_reboot_clears_pid(self):
        calls, attempt = [], [0]
        def runner(argv, **kwargs):
            calls.append(argv)
            if argv[4] == "get-osinfo":
                value = {"id": "mswindows"}
            elif argv[4] == "exec-windows":
                attempt[0] += 1
                value = {"pid": 40 + attempt[0]}
            else:
                if attempt[0] == 1:
                    raise RuntimeError("PID 41 not found")
                value = {"exited": 1, "exitcode": 0, "out-data": json.dumps(document())}
            return subprocess.CompletedProcess(argv, 0, json.dumps(value), "")
        reader = GuestReader(100, runner)
        with self.assertRaises(RuntimeError):
            reader.read()
        self.assertIsNone(reader.pending_pid)
        self.assertEqual(reader.read()["memory"]["mem_used_bytes"], 19 * 1024**3)
        self.assertEqual(sum(a[4] == "exec-windows" for a in calls), 2)
        reader.pending_pid = 42
        reader.reset(restarted=True)
        self.assertIsNone(reader.pending_pid)
        self.assertIsNone(reader.os_kind)

    def test_restart_discards_old_inflight_source_result(self):
        source = Source(lambda: None, 15, 45)
        old = concurrent.futures.Future()
        old.set_running_or_notify_cancel()
        source.future = old
        source.reset()
        old.set_result({"memory": {"mem_used_bytes": 19}})
        class Executor:
            def submit(self, probe):
                return concurrent.futures.Future()
        source.advance(Executor(), 10, 100)
        self.assertIsNone(source.data)
        self.assertFalse(source.ok)
        self.assertIsNot(source.future, old)


class GPUAndDRMTests(unittest.TestCase):
    def test_nvidia_unknowns_and_invalid_bounds_are_null(self):
        gpu = parse_nvidia_csv("NVIDIA, 0000:01:00.0, N/A, -1, -2, 43, N/A, 0, 405, 101\n")[0]
        for field in ("utilization_pct", "mem_used_bytes", "mem_total_bytes", "power_w", "fan_pct"):
            self.assertIsNone(gpu[field])
        self.assertEqual(gpu["graphics_mhz"], 0)

    def test_inventory_detects_display_class_0380_and_owner(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for slot, vendor, device, klass in (("0000:00:02.0", "8086", "a780", "038000"),
                                                  ("0000:01:00.0", "10de", "2702", "030000"),
                                                  ("0000:02:00.0", "1234", "1111", "030000")):
                path = root / "sys/bus/pci/devices" / slot
                path.mkdir(parents=True)
                for filename, value in (("vendor", vendor), ("device", device), ("class", klass)):
                    (path / filename).write_text("0x" + value)
            configs = root / "configs"
            configs.mkdir()
            (configs / "102.conf").write_text("hostpci0: 0000:00:02,pcie=1\n")
            (configs / "100.conf").write_text("hostpci0: 0000:01:00.0,pcie=1\n")
            def runner(argv, **kwargs):
                return subprocess.CompletedProcess(argv, 0, '0000:00:02.0 "Display" "Intel" "UHD Graphics 770"\n0000:01:00.0 "VGA" "NVIDIA" "RTX 4080 SUPER"\n', "")
            gpus = GPUReader(runner, root / "sys", configs).read()["gpus"]
            self.assertEqual(len(gpus), 2)
            self.assertEqual(gpus[0]["owner"], "vm:102")
            self.assertEqual(gpus[0]["kind"], "integrated")
            self.assertEqual(gpus[1]["owner"], "vm:100")

    def test_guest_gpu_bus_renumbering_match_and_stopped_values_clear(self):
        inventory = [{"id": "pci:0000:00:02.0", "name": "Intel", "vendor": "Intel", "kind": "integrated",
                      "owner": "vm:102", "driver": "vfio-pci", "status": "inventory", "updated_at": time.time(),
                      "_vendor_id": "8086", "_device_id": "a780"}]
        sample = {102: {"generated_at": time.time(), "gpus": [{"vendor_id": "8086", "device_id": "a780", "pci_bus": "0000:01:00.0", "driver": "i915", "graphics_mhz": 0, "utilization_pct": 12, "utilization_kind": "busiest-engine"}]}}
        states = {"guest_102": {"ok": True}}
        guests = [{"id": 102, "status": "running"}]
        gpu = merge_gpus(inventory, sample, guests, states, time.time())[0]
        self.assertEqual(gpu["graphics_mhz"], 0)
        self.assertEqual(gpu["utilization_pct"], 12)
        self.assertEqual(gpu["driver"], "i915")
        self.assertNotIn("_vendor_id", gpu)
        guests[0]["status"] = "stopped"
        gpu = merge_gpus(inventory, sample, guests, states, time.time())[0]
        self.assertIsNone(gpu["graphics_mhz"])
        self.assertEqual(gpu["status"], "unavailable")

    def test_drm_client_dedup_capacity_reset_catchup_and_incomplete_scan(self):
        rates = DRMRates()
        cards = [{"driver": "i915", "pci_bus": "0000:01:00.0"}]
        def stats(now, value, complete=True):
            client = {"device": "0000:01:00.0", "client": "18", "counters": {"render": value}, "capacities": {"render": 2}}
            return {"monotonic_s": now, "complete": complete, "clients": [client, copy.deepcopy(client)]}
        rates.apply(cards, stats(10, 10_000_000_000))
        self.assertIsNone(cards[0]["utilization_pct"])
        rates.apply(cards, stats(20, 20_000_000_000))
        self.assertEqual(cards[0]["utilization_pct"], 50)  # Not100 from duplicatefds.
        rates.apply(cards, stats(30, 19_000_000_000))
        self.assertIsNone(cards[0]["utilization_pct"])
        rates.apply(cards, stats(40, 21_000_000_000))
        self.assertEqual(cards[0]["utilization_pct"], 5)  # Lowercounter wasn't committed.
        rates.apply(cards, stats(50, 22_000_000_000, complete=False))
        self.assertIsNone(cards[0]["utilization_pct"])
        self.assertIn("incomplete", cards[0]["error"])
        rates.apply(cards, stats(60, 23_000_000_000))
        self.assertEqual(cards[0]["utilization_pct"], 5)  # Keepslastcompletebaseline.


if __name__ == "__main__":
    unittest.main()
