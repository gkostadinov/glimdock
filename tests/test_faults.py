import json
import unittest
from agent.faults import classify, merge_events, parse_dmesg, parse_journal

class FaultTests(unittest.TestCase):
    def test_benign_startup_does_not_raise_fault(self):
        self.assertIsNone(classify("MCE: In-kernel MCE decoding enabled."))
        self.assertIsNone(classify("nvidia: loading out-of-tree module taints kernel."))
        self.assertIsNone(classify("ACPI: Error loading optional device, continuing"))
        self.assertEqual(classify("mce: [Hardware Error]: Machine check events logged"), "hardware")
        self.assertEqual(classify("worker: invalid opcode"), "trap")
        self.assertEqual(classify("worker dumped core, signal 6"), "crash")
        self.assertEqual(classify("worker dumped core, signal 11"), "segfault")

    def test_dmesg_journal_same_fault_is_counted_once(self):
        message = "pvesh[9729]: segfault at 4fffc ip 00007aa0ce6c1fe2 sp 00007ffc85a416d0 error 4 likely on CPU 4 (core 8, socket 0)"
        journal = parse_journal(json.dumps({"MESSAGE": message, "__REALTIME_TIMESTAMP": "950000000", "__MONOTONIC_TIMESTAMP": "50000000", "_BOOT_ID": "abcd1234"}))
        kernel = parse_dmesg("[   50.000000] " + message, 1000, 100, "abcd-1234")
        result = merge_events(journal, kernel, 1000)
        self.assertEqual(result["segfault_count_24h"], 1)
        self.assertEqual(len(result["events"]), 1)
        self.assertIn("core 8", result["events"][0]["message"])
        self.assertNotIn("00007aa0", result["events"][0]["message"])

    def test_history_is_visible_but_does_not_trigger_24h_count(self):
        now = 10 * 86400
        journal = parse_journal(json.dumps({"MESSAGE": "geekbench_avx2: segfault at 0", "__REALTIME_TIMESTAMP": str(int((now - 2 * 86400) * 1e6))}))
        result = merge_events(journal, [], now)
        self.assertEqual(result["segfault_count_24h"], 0)
        self.assertEqual(result["history_event_count"], 1)
        self.assertEqual(result["last_event_at"], now - 2 * 86400)

    def test_history_bounds_and_invalid_journal(self):
        now = 10 * 86400
        lines = [json.dumps({"MESSAGE": "worker: segfault at 0", "__REALTIME_TIMESTAMP": str(int((now-i) * 1e6))}) for i in range(30)]
        lines += ['not json', '{}', json.dumps({"MESSAGE": [1,2], "__REALTIME_TIMESTAMP": "4"})]
        result = merge_events(parse_journal('\n'.join(lines)), [], now)
        self.assertEqual(result["event_count_24h"], 30)
        self.assertEqual(len(result["events"]), 16)
        self.assertEqual(result["history_truncated"], 14)

if __name__ == '__main__':
    unittest.main()
