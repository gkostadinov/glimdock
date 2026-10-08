"""Read journal and kernel faults without changing logging or host settings.

Journal history provides real timestamps across boots. dmesg covers current-boot
kernel messages and is merged with the journal by message and boot timestamp.
No generic 'error' matching: benign driver startup messages must not raise a
hardware failure. The monitor distinguishes historical events from 24h alerts.
"""
from __future__ import annotations
import hashlib
import json
import re
import subprocess
import time
from typing import Any, Callable

LOOKBACK_DAYS = 7
MAX_EVENTS = 16
PATTERN = (r"segfault|general protection fault|invalid opcode|dumped core|core dumped|"
           r"\[Hardware Error\]|Machine check events logged|Uncorrected.*(?:error|ECC)|"
           r"Out of memory: Killed process|oom-kill:|soft lockup|hard LOCKUP|"
           r"I/O error, dev|EXT4-fs error|XFS.*(?:corruption|Corruption)")

def classify(message: str) -> str | None:
    if re.search(r"segfault|SIGSEGV|signal 11", message, re.I):
        return "segfault"
    if re.search(r"general protection fault|invalid opcode", message, re.I):
        return "trap"
    if re.search(r"dumped core|core dumped", message, re.I):
        return "crash"
    if re.search(r"\[Hardware Error\]|Machine check events logged|Uncorrected.*(?:error|ECC)", message, re.I):
        return "hardware"
    if re.search(r"Out of memory: Killed process|oom-kill:", message, re.I):
        return "oom"
    if re.search(r"soft lockup|hard LOCKUP", message, re.I):
        return "lockup"
    if re.search(r"I/O error, dev|EXT4-fs error|XFS.*(?:corruption|Corruption)", message, re.I):
        return "io"
    return None

def event(message: Any, timestamp: float, boot: str = "", monotonic: float | None = None) -> dict | None:
    if not isinstance(message, str) or (kind := classify(message)) is None:
        return None
    # Addresses are not useful on the desk; retain process/core and fault type.
    boot = boot.replace('-', '')
    display = re.sub(r"\b(?:ip|sp) [0-9a-f]{8,}\b", "", message, flags=re.I)
    display = re.sub(r"[\x00-\x1f\x7f]", " ", display)
    display = re.sub(r"\s+", " ", display).strip()[:220]
    identity = hashlib.sha256(f"{boot}:{monotonic if monotonic is not None else timestamp}:{message}".encode()).hexdigest()[:16]
    return {"id": identity, "timestamp": timestamp, "kind": kind, "message": display,
            "_message": message, "_boot": boot, "_mono": monotonic}

def parse_journal(text: str) -> list[dict]:
    events = []
    for line in text.splitlines():
        try:
            row = json.loads(line)
            timestamp = int(row["__REALTIME_TIMESTAMP"]) / 1_000_000
            mono = int(row["__MONOTONIC_TIMESTAMP"]) / 1_000_000 if row.get("__MONOTONIC_TIMESTAMP") else None
            result = event(row.get("MESSAGE"), timestamp, row.get("_BOOT_ID", ""), mono)
            if result:
                events.append(result)
        except (ValueError, TypeError, KeyError):
            continue
    return events

def parse_dmesg(text: str, now: float, uptime_s: float, boot: str) -> list[dict]:
    """Uses monotonic kernel stamps, avoiding host timezone/locale formatting."""
    events = []
    for line in text.splitlines():
        match = re.match(r"\[\s*(\d+(?:\.\d+)?)\]\s*(.*)", line)
        if match:
            mono = float(match[1])
            result = event(match[2], now - uptime_s + mono, boot, mono)
            if result:
                events.append(result)
    return events

def merge_events(journal: list[dict], kernel: list[dict], now: float) -> dict:
    combined = journal[:]
    for candidate in kernel:
        # Journal monotonic time can include logging delay. Matching current
        # boot and message with a small tolerance avoids double-counting.
        duplicate = any(e["_message"] == candidate["_message"] and
                        e["_boot"] == candidate["_boot"] and
                        e["_mono"] is not None and abs(e["_mono"] - candidate["_mono"]) < 2
                        for e in journal)
        if not duplicate:
            combined.append(candidate)
    combined = [e for e in combined if now - LOOKBACK_DAYS * 86400 <= e["timestamp"] <= now + 60]
    combined.sort(key=lambda e: e["timestamp"], reverse=True)
    recent = [e for e in combined if e["timestamp"] >= now - 86400]
    # Kernel segfault and systemd core dump of the same process can be separate
    # records. Counts are matching log events, not a claimed unique crash count.
    return {"lookback_days": LOOKBACK_DAYS,
            "segfault_count_24h": sum(e["kind"] == "segfault" for e in recent),
            "event_count_24h": len(recent),
            "last_event_at": combined[0]["timestamp"] if combined else None,
            "history_event_count": len(combined),
            "history_truncated": max(0, len(combined) - MAX_EVENTS),
            "events": [{k: v for k, v in e.items() if not k.startswith("_")} for e in combined[:MAX_EVENTS]]}

def read_faults(runner: Callable) -> dict:
    now = time.time()
    errors = []
    journal, kernel = [], []
    try:
        result = runner(["journalctl", "--since", f"-{LOOKBACK_DAYS} days", "--no-pager",
                         "--output=json", "--grep", PATTERN, "--case-sensitive=no", "--lines=2000"],
                        timeout=8, allow_nonzero=True)
        if result.returncode:
            errors.append("journal unavailable")
        else:
            journal = parse_journal(result.stdout)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        errors.append("journal unavailable")
    try:
        result = runner(["dmesg", "--color=never", "--time-format=raw"], timeout=3, allow_nonzero=True)
        if result.returncode:
            errors.append("kernel log unavailable")
        else:
            from pathlib import Path
            boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
            uptime_s = float(Path("/proc/uptime").read_text().split()[0])
            kernel = parse_dmesg(result.stdout, now, uptime_s, boot)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        errors.append("kernel log unavailable")
    return {"faults": merge_events(journal, kernel, now), "error": "; ".join(errors) or None}
